use std::io::Stdin;
use std::time::Duration;

use tokio::sync::watch;
use tokio::time::{sleep, Interval, MissedTickBehavior, interval};
use tokio_tungstenite::tungstenite::handshake::client::{generate_key, Request};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::http::Uri;
use tokio_util::sync::CancellationToken;
use futures_util::{SinkExt, StreamExt};
use machineid_rs::{IdBuilder, Encryption, HWIDComponent};
use serde::Deserialize;
use tokio::io::{self, AsyncReadExt};

const UPDATE_HERTZ: u64 = 20;
const RECONNECT_HERTZ: u64 = 2;
const PING_HERTZ: u64 = 1; // this also defines the max latency

type Websocket = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

struct Handset {
    /// only sends clicks, not when the switch is released
    switch_tx: watch::Sender<()>,
    /// sends true when a handset is connected, and false on disconnect
    presence_tx: watch::Sender<bool>,
    /// sends the wanted state of the led, not necessarrily bound to any events
    led_tx: watch::Sender<bool>,
    led_rx: watch::Receiver<bool>,
}

impl Handset {
    fn new() -> Self {
        let (led_tx, led_rx) = watch::channel(false);
        Self {
            switch_tx: watch::Sender::new(()),
            presence_tx: watch::Sender::new(true),
            led_tx,
            led_rx,
        }
    }
    async fn update(&mut self) {
        tokio::join!(
            async {
                let mut old_led = false;
                loop {
                    self.led_rx.changed().await.expect("kabum");
                    let led = *self.led_rx.borrow();
                    if led != old_led {
                        println!("Active: {led}");
                        old_led = led;
                    }
                }
            },
            async {
                let mut stdin = io::stdin();
                let mut buf = [0u8; 128];
                loop {
                    stdin.read(&mut buf).await.expect("kablooey");
                    self.switch_tx.send_replace(());
                }
            },
        );
    }
}

struct HandsetCommunicator {
    id: String,
    switch_rx: watch::Receiver<()>,
    presence_rx: watch::Receiver<bool>,
    led_tx: watch::Sender<bool>,
    connection: Connection,
    ping_interval: Interval,
}

#[derive(Debug, Deserialize)]
struct ServerState {
    game: GameState,
    connection: ConnectionState,
}

#[derive(Debug, Deserialize)]
struct GameState {
    contestants: Vec<Contestant>,
}

#[derive(Debug, Deserialize, Clone)]
struct Contestant {
    indicate: bool,
    name_hint: String,
}

#[derive(Debug, Deserialize)]
struct ConnectionState {
    controlling: Option<usize>,
}

impl HandsetCommunicator {
    fn from_handset_with_request(machine_id: String, handset: &Handset, socket_address: Uri) -> Self {
        let mut reconnect_interval = interval(Duration::from_millis(1000/RECONNECT_HERTZ));
        reconnect_interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut ping_interval = interval(Duration::from_millis(1000/PING_HERTZ));
        ping_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let id = format!("{}", machine_id);

        Self {
            id: id.to_owned(),
            switch_rx: handset.switch_tx.subscribe(),
            presence_rx: handset.presence_tx.subscribe(),
            led_tx: handset.led_tx.clone(),
            connection: Connection{
                id,
                uri: socket_address,
                reconnect_interval,
                inner: SocketState::Unconnected,
                ping_in_transit: false,
            },
            ping_interval,
        }
    }
    async fn communicate(&mut self, cancellation_token: CancellationToken) {
        let mut switch_rx = self.switch_rx.to_owned();
        let mut presence_rx = self.presence_rx.to_owned();

        loop {
            let auto_reconnect = *presence_rx.borrow();
            tokio::select! {
                _ = cancellation_token.cancelled() => { return; },
                Ok(_) = switch_rx.changed() => {
                    if self.connection.is_disconnected() {
                        let new = !*self.led_tx.borrow();
                        self.led_tx.send_replace(new);
                    } else {
                        if let Some(me) = self.connection.me_index() {
                            let response = serde_json::json!({
                                "type": "buzz",
                                "contestant": me,
                            }).to_string();
                            self.connection.send(&response).await;
                        }
                    }
                },
                Ok(_) = presence_rx.changed() => {
                    let presence = *presence_rx.borrow();
                    println!("{}: presence: {presence}", self.id);
                    if !presence { self.connection.disconnect() }
                },
                msg = self.connection.receive(auto_reconnect) => {
                    let Some(msg) = msg else { continue };
                    let Some(me) = self.connection.me() else {
                        // register a new contestant by default
                        let mut response = serde_json::json!({
                            "type": "connect_contestant",
                            "name_hint": self.id,
                        }).to_string();
                        if let Some(index) = msg.game.contestants.iter()
                            .position(|c| c.name_hint == self.id)
                        {
                            // contestant with our name found, reconnect instead
                            response = serde_json::json!({
                                "type": "reconnect_contestant",
                                "contestant": index,
                            }).to_string();
                        }
                        self.connection.send(&response).await;
                        sleep(Duration::from_secs(1)).await;
                        continue;
                    };
                    let my_led = *self.led_tx.borrow();
                    if my_led != me.indicate {
                        self.led_tx.send_replace(!my_led);
                    }
                },
                _ = self.ping_interval.tick() => { self.connection.ping().await; },
            }
        }
    }
}

enum SocketState {
    Unconnected,
    Connected{socket: Websocket},
    Registered{socket: Websocket, me: (usize, Contestant)},
}
impl SocketState {
    fn register(&mut self, me_index: usize, me: Contestant) {
        let me = (me_index, me);
        // https://stackoverflow.com/a/45119209/371128
        let old = std::mem::replace(self, Self::Unconnected);
        *self = match old {
            Self::Unconnected => Self::Unconnected,
            Self::Connected { socket } => Self::Registered { socket , me },
            Self::Registered { socket, .. } => Self::Registered { socket, me },
        };
    }
}

struct Connection {
    id: String, // for convenience, matches id from HandsetCommunicator
    inner: SocketState,
    uri: Uri,
    reconnect_interval: Interval,
    ping_in_transit: bool,
}

impl Connection {
    fn me(&self) -> Option<&Contestant> {
        match &self.inner {
            SocketState::Unconnected => None,
            SocketState::Connected { .. } => None,
            SocketState::Registered { me: (_, me), .. } => Some(me),
        }
    }
    fn me_index(&self) -> Option<usize> {
        match &self.inner {
            SocketState::Unconnected => None,
            SocketState::Connected { .. } => None,
            SocketState::Registered { me: (me_index, _), .. } => Some(*me_index),
        }
    }
    fn is_disconnected(&self) -> bool {
        matches!(self.inner, SocketState::Unconnected)
    }
    async fn send(&mut self, msg: &str) {
        let s = match self.inner {
            SocketState::Unconnected => { return; },
            SocketState::Connected { ref mut socket } => socket,
            SocketState::Registered { ref mut socket, .. } => socket,
        };
        if let Err(e) = s.send(msg.into()).await {
            println!("{}: failed to send message, reconnecting: {:?}", self.id, e);
            self.disconnect();
        }
    }
    async fn receive(&mut self, reconnect: bool) -> Option<ServerState> {
        // try to reconnect
        if self.is_disconnected() && reconnect {
            self.connect().await;
        }
        let socket = match self.inner {
            SocketState::Unconnected => { 
                self.reconnect_interval.tick().await;
                return None;
            },
            SocketState::Connected { socket: ref mut s } => s,
            SocketState::Registered { socket: ref mut s, .. } => s,
        };
        let result = socket.next().await;
        let msg = match result {
            Some(Ok(msg)) => msg,
            // something got borked, try to reconnect later...
            None => {
                println!("{}: no messages to receive, socket likely closed", self.id);
                self.disconnect();
                return None;
            },
            Some(Err(e)) => {
                println!("{}: error receiving messages: {}", self.id, e);
                self.disconnect();
                return None;
            },
        };
        let msg = match msg {
            Message::Ping(payload) => {
                let _ = socket.send(Message::Pong(payload)).await;
                return None;
            },
            Message::Pong(_) => {
                self.ping_in_transit = false;
                return None;
            },
            Message::Close(..) => {
                self.disconnect();
                return None;
            },
            Message::Text(msg) => msg,
            _ => { return None; },
        };
        let msg = match serde_json::from_str::<ServerState>(&msg) {
            Err(e) => {
                println!("{}: error parsing server message: {:?}: {}", self.id, e, msg);
                return None;
            },
            Ok(msg) => msg,
        };
        println!("{}: received {:?}", self.id, &msg);

        match msg.connection.controlling {
            None => {},
            Some(i) => {
                let Some(me) = msg.game.contestants.get(i) else {
                    println!("{}: server thought this was controlling contestant {}, but there are only {} contestants connected", self.id, i, msg.game.contestants.len());
                    self.disconnect();
                    return None;
                };
                self.inner.register(i, me.clone());
            },
        }

        Some(msg)
    }
    async fn connect(&mut self) {
        if matches!(self.inner, SocketState::Connected { .. } | SocketState::Registered { .. }) {
            return;
        }
        match tokio_tungstenite::connect_async(build_request(self.uri.to_owned())).await {
            Err(e) => { println!("{}: failure to connect: {}", self.id, e); },
            Ok((s,_)) => self.inner = SocketState::Connected { socket: s },
        }
    }
    fn disconnect(&mut self) {
        self.inner = SocketState::Unconnected;
    }
    async fn ping(&mut self) {
        // we are supposed to ping again, but the previos one hasn't come back yet...
        if self.ping_in_transit {
            self.ping_in_transit = false;
            self.disconnect();
            println!("{}: missing Pong! reconnecting", self.id);
            return;
        }
        let socket = match self.inner {
            SocketState::Unconnected => { return; },
            SocketState::Connected { socket: ref mut s } => s,
            SocketState::Registered { socket: ref mut s, .. } => s,
        };
        if let Err(e) = socket.send(Message::Ping("ping".into())).await {
            self.disconnect();
            println!("{}: error while sending ping: {:?}", self.id, e);
        };
        self.ping_in_transit = true;
    }
}

fn build_request(uri: tokio_tungstenite::tungstenite::http::Uri) -> Request {
    Request::get(uri.to_owned())
        .header("Host", uri.host().expect("no host in websocket URL"))
        .header("User-Agent", format!("{} ({})", env!("CARGO_BIN_NAME"), env!("CARGO_PKG_VERSION")))
        .header("Accept", "application/json")
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", generate_key())
        .body(())
        .expect("failed to build connection request")
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::result::Result<(), std::boxed::Box<dyn std::error::Error>> {
    let mut machine_id = IdBuilder::new(Encryption::SHA256)
        .add_component(HWIDComponent::SystemID)
        .add_component(HWIDComponent::CPUID)
        .add_component(HWIDComponent::MachineName)
        .build("nonceorsomminidunno")
        .expect("Can't build a machine id");
    machine_id.truncate(16);

    let uri: Uri = std::env::args()
        .nth(1)
        .expect(&format!("usage: {} <ws-address>", env!("CARGO_BIN_NAME")))
        .parse()
        .expect("Could not parse ws-address");
    println!("connecting to {:?}", uri);

    let mut handset = Handset::new();

    let communicators: Vec<_> = [&handset].iter()
        .map(|h| HandsetCommunicator::from_handset_with_request(machine_id.to_owned(), h, uri.to_owned()))
        .collect();

    let cancellation_token = CancellationToken::new();

    tokio::join!(
        async {
            // handle websocket communication
            let mut tasks = tokio::task::JoinSet::new();
            for mut communicator in communicators {
                let cancellation_token = cancellation_token.clone();
                tasks.spawn(async move { communicator.communicate(cancellation_token).await });
            }
            while (tasks.join_next().await).is_some() { }
        },
        async {
            handset.update().await;
        },
        async {
            // handle termination
            if let Err(sigint_error) = tokio::signal::ctrl_c().await {
                dbg!(sigint_error);
            }
            println!("Stopping");
            cancellation_token.cancel();
        },
    );
    Ok(())
}

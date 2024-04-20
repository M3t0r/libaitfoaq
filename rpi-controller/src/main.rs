use std::time::Duration;

use rppal::gpio::{Gpio, InputPin, OutputPin, Error as GPIOError};
use tokio::sync::watch;
use tokio::time::{Interval, MissedTickBehavior, interval};
use tokio_tungstenite::tungstenite::handshake::client::{generate_key, Request};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::http::Uri;
use tokio_util::sync::CancellationToken;
use futures_util::{SinkExt, StreamExt};
use machineid_rs::{IdBuilder, Encryption, HWIDComponent};
use serde::Deserialize;

const UPDATE_HERTZ: u64 = 20;
const RECONNECT_HERTZ: u64 = 2;
const PING_HERTZ: u64 = 1; // this also defines the max latency

type Websocket = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

struct PinTiples {
    switch: u8,
    presence: u8,
    led: u8,
}

impl PinTiples {
    fn new(switch: u8, presence: u8, led: u8) -> Self {
        Self {
            switch,
            presence,
            led,
        }
    }
}

struct Handset {
    switch: InputPin,
    presence: InputPin,
    led: OutputPin,
    switch_flank: bool,

    /// only sends clicks, not when the switch is released
    switch_tx: watch::Sender<()>,
    /// sends true when a handset is connected, and false on disconnect
    presence_tx: watch::Sender<bool>,
    /// sends the wanted state of the led, not necessarrily bound to any events
    led_tx: watch::Sender<bool>,
    led_rx: watch::Receiver<bool>,
}

impl TryFrom<&PinTiples> for Handset {
    type Error = GPIOError;
    fn try_from(pins: &PinTiples) -> Result<Self, Self::Error> {
        let gpio = Gpio::new()?;
        let (led_tx, led_rx) = watch::channel(false);
        Ok(Self {
            switch: gpio.get(pins.switch)?.into_input_pullup(),
            presence: gpio.get(pins.presence)?.into_input_pullup(),
            led: gpio.get(pins.led)?.into_output_high(),
            switch_flank: true,

            switch_tx: watch::Sender::new(()),
            presence_tx: watch::Sender::new(false),
            led_tx,
            led_rx,
        })
    }
}

impl Handset {
    fn update(&mut self) {
        let switch = self.switch.is_low();
        if self.switch_flank ^ switch {
            self.switch_flank = switch;
            if switch {
                self.switch_tx.send_replace(());
            }
        }

        let presence = self.presence.is_low();
        if *self.presence_tx.borrow() != presence {
            self.presence_tx.send_replace(presence);
        }

        self.led.write((*self.led_rx.borrow() as u8).into());
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
        let id = format!("{}-{}", machine_id, handset.switch.pin());

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
                    println!("{}: switch activated", self.id);
                    if self.connection.is_disconnected() {
                        let new = !*self.led_tx.borrow();
                        self.led_tx.send_replace(new);
                    }
                },
                Ok(_) = presence_rx.changed() => {
                    let presence = *presence_rx.borrow();
                    println!("{}: presence changed: {}", self.id, presence);
                    if !presence { self.connection.disconnect() }
                },
                msg = self.connection.receive(auto_reconnect) => {
                    let Some(msg) = msg else { continue };
                    let Some(me) = self.connection.me() else {
                        let response = if msg.game.contestants.iter()
                            .any(|c| c.name_hint == self.id)
                        {
                            // contestant with our name found, reconnect
                            "blubber".to_owned()
                        } else {
                            // no contestant with our name, register
                            "blabber".to_owned()
                        };
                        self.connection.send(&response).await;
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
    Registered{socket: Websocket, me: Contestant},
}
impl SocketState {
    fn register(&mut self, me: Contestant) {
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
            SocketState::Registered { me, .. } => Some(me),
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
                println!("{}: error parsing server message: {:?}", self.id, e);
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
                self.inner.register(me.clone());
            },
        }

        Some(msg)
    }
    async fn connect(&mut self) {
        if matches!(self.inner, SocketState::Connected { .. } | SocketState::Registered { .. }) {
            return;
        }
        match tokio_tungstenite::connect_async(build_request(self.uri.to_owned())).await {
            Err(e) => {dbg!(e);},
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

    let uri: Uri = "ws://localhost:3000/websocket".parse().unwrap();

    let mut handsets: Vec<Handset> = [
        PinTiples::new(21, 20, 26),
        PinTiples::new(13, 19, 16),
        PinTiples::new(5, 6, 12),
        PinTiples::new(0, 1, 7),
    ]
        .iter()
        .map(|p| p.try_into())
        .collect::<Result<Vec<_>,_>>()?;

    let communicators: Vec<_> = handsets.iter()
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
            // handle hardware pins
            let mut interval = interval(Duration::from_millis(1000/UPDATE_HERTZ));
            while !cancellation_token.is_cancelled() {
                for handset in &mut handsets {
                    handset.update();
                }
                interval.tick().await;
            }
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

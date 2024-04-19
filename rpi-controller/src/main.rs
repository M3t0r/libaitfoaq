use std::{borrow::Borrow, thread::sleep};
use std::time::Duration;

use rppal::gpio::{Gpio, InputPin, OutputPin, Error as GPIOError};
use tokio::sync::watch;
use tokio::time::{Interval, MissedTickBehavior, interval};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use futures_util::StreamExt;
use url::Url;

const UPDATE_HERTZ: u64 = 20;
const RECONNECT_HERTZ: u64 = 2;

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
    id: u8,
    switch_rx: watch::Receiver<()>,
    presence_rx: watch::Receiver<bool>,
    led_tx: watch::Sender<bool>,

    socket_address: Url,
    socket: Option<Websocket>,
    reconnect_interval: Interval,
}

impl HandsetCommunicator {
    fn from_handset_with_url(handset: &Handset, url: Url) -> Self {
        let mut reconnect_interval = interval(Duration::from_millis(1000/RECONNECT_HERTZ));
        reconnect_interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

        Self {
            id: handset.switch.pin(),
            switch_rx: handset.switch_tx.subscribe(),
            presence_rx: handset.presence_tx.subscribe(),
            led_tx: handset.led_tx.clone(),
            socket_address: url,
            socket: None,
            reconnect_interval,
        }
    }
    async fn communicate(&mut self, cancellation_token: CancellationToken) {
        self.connect().await;
        let mut switch_rx = self.switch_rx.to_owned();
        let mut presence_rx = self.presence_rx.to_owned();
        loop {
            tokio::select! {
                _ = cancellation_token.cancelled() => { return; },
                Ok(_) = switch_rx.changed() => {
                    println!("{}: switch activated", self.id);
                    if self.socket.is_none() {
                        let new = !*self.led_tx.borrow();
                        self.led_tx.send_replace(new);
                    }
                },
                Ok(_) = presence_rx.changed() => {
                    println!("{}: presence changed: {}", self.id, *presence_rx.borrow());
                },
                msg = self.receive() => { dbg!(msg); },
            }
        }
    }
    async fn connect(&mut self) {
        if self.socket.is_some() { return; }
        match tokio_tungstenite::connect_async(self.socket_address.to_owned()).await {
            Err(e) => {dbg!(e);},
            Ok((s,_)) => self.socket = Some(s),
        }
    }
    async fn receive(&mut self) -> Option<Message> {
        // try to reconnect
        if self.socket.is_none() {
            self.reconnect_interval.tick().await;
            self.connect().await;
        }
        match self.socket {
            None => None,
            Some(ref mut s) => {
                let result = s.next().await;
                match result {
                    Some(Ok(msg)) => return Some(msg),
                    // something got borked, try to reconnect later...
                    None => {
                        println!("{}: no messages to receive, socket likely closed", self.id);
                        self.socket.take();
                        return None;
                    },
                    Some(Err(e)) => {
                        println!("{}: error receiving messages: {}", self.id, e);
                        self.socket.take();
                        return None;
                    },
                }
            },
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::result::Result<(), std::boxed::Box<dyn std::error::Error>> {
    let url = Url::parse("ws://hopper.m3:3000/websocket")?;
    let mut handsets: Vec<Handset> = vec![
        PinTiples::new(21, 20, 26),
        PinTiples::new(13, 19, 16),
        PinTiples::new(5, 6, 12),
        PinTiples::new(0, 1, 7),
    ]
        .iter()
        .map(|p| p.try_into())
        .collect::<Result<Vec<_>,_>>()?;

    let communicators: Vec<_> = handsets.iter()
        .map(|h| HandsetCommunicator::from_handset_with_url(h, url.to_owned()))
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
            while let Some(_) = tasks.join_next().await { }
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

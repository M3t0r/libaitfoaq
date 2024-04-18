use std::{borrow::Borrow, thread::sleep};
use std::time::Duration;

use rppal::gpio::{Gpio, InputPin, OutputPin, Error as GPIOError};
use tokio::sync::watch;
use tokio::time::interval;
use tokio_util::sync::CancellationToken;
use futures_util::StreamExt;

const UPDATE_HERTZ: u64 = 20;

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
    switch_tx: watch::Sender<bool>,
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

            switch_tx: watch::Sender::new(false),
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
                self.switch_tx.send_replace(true);
            }
        }

        let presence = self.presence.is_low();
        if *self.presence_tx.borrow() != presence {
            self.presence_tx.send_replace(presence);
        }

        self.led.write((*self.led_rx.borrow() as u8).into());
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::result::Result<(), std::boxed::Box<dyn std::error::Error>> {
    let url = url::Url::parse("ws://localhost:3000/websocket")?;
    let mut handsets: Vec<Handset> = vec![
        PinTiples::new(21, 20, 26),
        PinTiples::new(13, 19, 16),
        PinTiples::new(5, 6, 12),
        PinTiples::new(0, 1, 7),
    ]
        .iter()
        .map(|p| p.try_into())
        .collect::<Result<Vec<_>,_>>()?;

    let cancellation_token = CancellationToken::new();

    tokio::join!(
        async {
            let (mut socket, _) = tokio_tungstenite::connect_async(url).await.expect("socket connection error");
            while let Some(msg) = socket.next().await {
                let msg = msg.expect("socket read error");
                dbg!(msg);
            }
        },
        async {
            let mut interval = interval(Duration::from_millis(1000/UPDATE_HERTZ));
            while !cancellation_token.is_cancelled() {
                for handset in &mut handsets {
                    handset.update();
                }
                interval.tick().await;
            }
        },
        async {
            if let Err(sigint_error) = tokio::signal::ctrl_c().await {
                dbg!(sigint_error);
            }
            println!("Stopping");
            cancellation_token.cancel();
        },
    );
    Ok(())
}

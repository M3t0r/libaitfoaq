use rppal::gpio::{Gpio, Pin, InputPin, OutputPin};

use std::time::Duration;
use std::thread::sleep;

struct Handset {
    switch: InputPin,
    presence: InputPin,
    led: OutputPin,
    switch_flank: bool,
}

impl Handset {
    fn new(switch: Pin, presence: Pin, led: Pin) -> Self {
        Self {
            switch: switch.into_input_pullup(),
            presence: presence.into_input_pullup(),
            led: led.into_output(),
            switch_flank: true,
        }
    }
}

fn main() -> std::result::Result<(), std::boxed::Box<dyn std::error::Error>> {
    let pins = Gpio::new()?;
    let mut handset = Handset::new(pins.get(3)?, pins.get(4)?, pins.get(14)?);

    loop {
        println!(
            "Presence: {:?}, Switch: {:?}, LED: {:?}",
            handset.presence.read(),
            handset.switch.read(),
            handset.led.is_set_high()
        );

        let switch = handset.switch.is_low();
        if handset.switch_flank ^ switch {
            handset.switch_flank = switch;
            if switch {
                handset.led.toggle();
            }
        }

        sleep(Duration::from_millis(50))
    }
}

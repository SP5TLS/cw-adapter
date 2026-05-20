#![no_std]
#![no_main]

// Minimal RP2350 sanity-check binary: brings up the HAL, prints a defmt
// line every second, and toggles the on-board LED on GP25. No USB, no
// InterruptExecutor, no mode pins. If this doesn't boot, the issue is
// deeper than the CW adapter's USB stack.

use defmt::*;
use embassy_executor::Spawner;
use embassy_rp::gpio::{Level, Output};
use embassy_time::{Duration, Timer};

use {defmt_rtt as _, panic_probe as _};

defmt::timestamp!("{=u64:us}", embassy_time::Instant::now().as_micros());

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    info!("rp2350_blinky: hello");

    let mut led = Output::new(p.PIN_25, Level::Low);
    let mut tick: u32 = 0;
    loop {
        led.toggle();
        info!("tick {}", tick);
        tick = tick.wrapping_add(1);
        Timer::after(Duration::from_millis(500)).await;
    }
}

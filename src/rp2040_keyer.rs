#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::{InterruptExecutor, Spawner};
use embassy_rp::bind_interrupts;
use embassy_rp::gpio::{Input, Pull};
use embassy_rp::peripherals::USB;
use embassy_rp::pwm::{Config as PwmConfig, Pwm};
use embassy_rp::usb::{Driver, InterruptHandler};
use embassy_usb::Builder;
use fixed::traits::ToFixed;
use static_cell::StaticCell;
use cw_adapter::keyer_app::KeyerApp;
use cw_adapter::midi_interrupt::MidiInterruptClass;
use podsdr_keyer::{KeyerConfig, KeyerEngine};

use {defmt_rtt as _, panic_probe as _};

defmt::timestamp!("{=u64:us}", embassy_time::Instant::now().as_micros());

#[defmt::panic_handler]
fn panic() -> ! {
    cortex_m::asm::udf()
}

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
});

/// Sidetone frequency in Hz.
const SIDETONE_FREQ: u32 = 600;
/// RP2040 system clock in Hz.
const SYS_CLK: u32 = 125_000_000;
/// PWM divider (integer part) — chosen so that top fits in u16.
const PWM_DIV: u32 = 4;
/// PWM top value for the sidetone frequency.
const PWM_TOP: u16 = (SYS_CLK / PWM_DIV / SIDETONE_FREQ - 1) as u16;

/// USB device task — runs at thread-mode (lowest) priority.
#[embassy_executor::task]
async fn usb_task(mut usb: embassy_usb::UsbDevice<'static, Driver<'static, USB>>) {
    usb.run().await;
}

/// Keying task — runs at elevated priority via InterruptExecutor.
#[embassy_executor::task]
async fn keying_task(
    mut app: KeyerApp<'static, Driver<'static, USB>>,
    dit_pin: Input<'static>,
    dah_pin: Input<'static>,
    btn_a_pin: Input<'static>,
    btn_b_pin: Input<'static>,
    mut buzzer: Pwm<'static>,
) {
    // Pre-compute configs for on/off to avoid allocation in the hot loop
    let mut cfg_on = PwmConfig::default();
    cfg_on.top = PWM_TOP;
    cfg_on.divider = (PWM_DIV as u16).to_fixed();
    cfg_on.compare_a = PWM_TOP / 2; // 50% duty = square wave

    let mut cfg_off = cfg_on.clone();
    cfg_off.compare_a = 0; // silent

    // Start silent
    buzzer.set_config(&cfg_off);

    app.run(
        || dit_pin.is_low(),
        || dah_pin.is_low(),
        || btn_a_pin.is_low(),
        || btn_b_pin.is_low(),
        |on| {
            if on {
                buzzer.set_config(&cfg_on);
            } else {
                buzzer.set_config(&cfg_off);
            }
        },
    ).await;
}

/// MIDI settings reader task — reads incoming MIDI CC on channel 16.
#[embassy_executor::task]
async fn midi_settings_task(
    mut read_ep: <Driver<'static, USB> as embassy_usb::driver::Driver<'static>>::EndpointOut,
) {
    use cw_adapter::keyer_app::{parse_midi_cc, CONFIG_CHANNEL};
    use embassy_usb::driver::{Endpoint, EndpointOut};

    let mut buf = [0u8; 64];
    loop {
        read_ep.wait_enabled().await;
        match read_ep.read(&mut buf).await {
            Ok(len) => {
                let mut offset = 0;
                while offset + 4 <= len {
                    if let Some(update) = parse_midi_cc(&buf[offset..offset + 4]) {
                        CONFIG_CHANNEL.send(update).await;
                    }
                    offset += 4;
                }
            }
            Err(_) => {}
        }
    }
}

/// InterruptExecutor running on SWI_IRQ_1 (above thread-mode priority).
static EXECUTOR_HIGH: InterruptExecutor = InterruptExecutor::new();

#[unsafe(no_mangle)]
#[allow(non_snake_case)]
unsafe extern "C" fn SWI_IRQ_1() {
    unsafe { EXECUTOR_HIGH.on_interrupt() }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    info!("CW Keyer (MIDI) starting");

    // USB Driver & Config
    let driver = Driver::new(p.USB, Irqs);
    let mut config = embassy_usb::Config::new(0x16c0, 0x27db);
    config.manufacturer = Some("Custom CW");
    config.product = Some("CW Keyer (MIDI)");
    config.serial_number = Some("12345678");
    config.max_power = 100;
    config.max_packet_size_0 = 64;
    config.device_class = 0x00;
    config.device_sub_class = 0x00;
    config.device_protocol = 0x00;
    config.composite_with_iads = false;

    static CONFIG_DESCRIPTOR: StaticCell<[u8; 512]> = StaticCell::new();
    static BOS_DESCRIPTOR: StaticCell<[u8; 256]> = StaticCell::new();
    static CONTROL_BUF: StaticCell<[u8; 64]> = StaticCell::new();

    let mut builder = Builder::new(
        driver,
        config,
        CONFIG_DESCRIPTOR.init([0; 512]),
        BOS_DESCRIPTOR.init([0; 256]),
        &mut [],
        CONTROL_BUF.init([0; 64]),
    );

    // MIDI interface: 1 IN jack (keyer output), 1 OUT jack (settings input)
    let mut midi_class = MidiInterruptClass::new(&mut builder, 1, 1, 64, 1);
    let midi_read_ep = midi_class.take_read_ep();

    let usb = builder.build();
    spawner.spawn(usb_task(usb)).unwrap();

    // PWM buzzer on GP16 (PWM slice 0, channel A)
    let mut pwm_config = PwmConfig::default();
    pwm_config.top = PWM_TOP;
    pwm_config.divider = (PWM_DIV as u16).to_fixed();
    pwm_config.compare_a = 0; // start silent
    let buzzer = Pwm::new_output_a(p.PWM_SLICE0, p.PIN_16, pwm_config);

    // Keying task at elevated priority
    let hi_spawner = EXECUTOR_HIGH.start(embassy_rp::pac::Interrupt::SWI_IRQ_1);

    let dit_pin = Input::new(p.PIN_14, Pull::Up);
    let dah_pin = Input::new(p.PIN_15, Pull::Up);
    let btn_a_pin = Input::new(p.PIN_17, Pull::Up);
    let btn_b_pin = Input::new(p.PIN_18, Pull::Up);

    let engine = KeyerEngine::new(KeyerConfig::default());
    let app = KeyerApp {
        midi: midi_class,
        engine,
        sidetone_enabled: true,
    };

    hi_spawner.spawn(keying_task(app, dit_pin, dah_pin, btn_a_pin, btn_b_pin, buzzer)).unwrap();

    if let Some(read_ep) = midi_read_ep {
        spawner.spawn(midi_settings_task(read_ep)).unwrap();
    }

    loop {
        embassy_time::Timer::after(embassy_time::Duration::from_secs(3600)).await;
    }
}

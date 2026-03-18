#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_rp::bind_interrupts;
use embassy_rp::gpio::{Input, Pull};
use embassy_rp::peripherals::USB;
use embassy_rp::usb::{Driver, InterruptHandler};
use embassy_usb::Builder;
use static_cell::StaticCell;
use cw_adapter::common::{CwApp, LaunchMode};
#[cfg(feature = "gamepad")]
use cw_adapter::common::GamepadReport;
#[cfg(feature = "serial")]
use cw_adapter::cdc_serial_state::{CdcWithSerialState, State as CdcState};
#[cfg(any(feature = "keyboard", feature = "gamepad"))]
use usbd_hid::descriptor::SerializedDescriptor;
#[cfg(feature = "keyboard")]
use usbd_hid::descriptor::KeyboardReport;

use {defmt_rtt as _, panic_probe as _};

defmt::timestamp!("{=u64:us}", embassy_time::Instant::now().as_micros());

#[defmt::panic_handler]
fn panic() -> ! {
    cortex_m::asm::udf()
}

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
});

#[embassy_executor::task]
async fn usb_task(mut usb: embassy_usb::UsbDevice<'static, Driver<'static, USB>>) {
    usb.run().await;
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());

    // 1. Initial Launch Mode Detection
    // Three mode-select pins, all pull-up (connect to GND to activate).
    // S0=GP16, S1=GP17, S2=GP18.
    // Pin combination → mode:
    //   open/open/open → Composite   (all interfaces active)
    //   S0/open/open   → KeyboardOnly
    //   open/S1/open   → GamepadOnly
    //   open/open/S2   → SerialOnly
    //   S0/S1/open     → MidiOnly
    //   any other      → Composite   (fallback)
    let s0 = Input::new(p.PIN_16, Pull::Up);
    let s1 = Input::new(p.PIN_17, Pull::Up);
    let s2 = Input::new(p.PIN_18, Pull::Up);

    #[allow(unreachable_patterns)]
    let launch_mode = match (s0.is_low(), s1.is_low(), s2.is_low()) {
        (false, false, false) => LaunchMode::Composite,
        #[cfg(feature = "keyboard")]
        (true, false, false) => LaunchMode::KeyboardOnly,
        #[cfg(feature = "gamepad")]
        (false, true, false) => LaunchMode::GamepadOnly,
        #[cfg(feature = "serial")]
        (false, false, true) => LaunchMode::SerialOnly,
        #[cfg(feature = "midi")]
        (true, true, false) => LaunchMode::MidiOnly,
        _ => LaunchMode::Composite,
    };

    info!("Launch Mode: {:?}", launch_mode);

    // 2. USB Driver & Config
    let driver = Driver::new(p.USB, Irqs);
    let mut config = embassy_usb::Config::new(0x16c0, 0x27db);
    config.manufacturer = Some("Custom CW");
    config.product = Some(launch_mode.product_name());
    config.serial_number = Some("12345678");
    config.max_power = 100;
    config.max_packet_size_0 = 64;

    // Config::new() defaults to composite (0xEF/0x02/0x01 + composite_with_iads=true).
    // For single-interface modes, clear these so the device presents as a single-function class.
    if !matches!(launch_mode, LaunchMode::Composite) {
        config.device_class = 0x00;
        config.device_sub_class = 0x00;
        config.device_protocol = 0x00;
        config.composite_with_iads = false;
    }

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

    // 3. Conditional Interface Initialization

    #[cfg(feature = "serial")]
    let serial = if matches!(launch_mode, LaunchMode::Composite | LaunchMode::SerialOnly) {
        static CDC_STATE: StaticCell<CdcState> = StaticCell::new();
        Some(CdcWithSerialState::new(
            &mut builder,
            CDC_STATE.init(CdcState::new()),
            64,
        ))
    } else {
        None
    };

    #[cfg(feature = "keyboard")]
    let keyboard = if matches!(
        launch_mode,
        LaunchMode::Composite | LaunchMode::KeyboardOnly
    ) {
        use embassy_usb::class::hid::{Config as HidConfig, HidWriter, State as HidState};
        static KBD_STATE: StaticCell<HidState> = StaticCell::new();
        let kbd_config = HidConfig {
            report_descriptor: KeyboardReport::desc(),
            poll_ms: 1,
            request_handler: None,
            max_packet_size: 8,
        };
        Some(HidWriter::<'_, _, 8>::new(
            &mut builder,
            KBD_STATE.init(HidState::new()),
            kbd_config,
        ))
    } else {
        None
    };

    #[cfg(feature = "gamepad")]
    let gamepad = if matches!(launch_mode, LaunchMode::Composite | LaunchMode::GamepadOnly) {
        use embassy_usb::class::hid::{Config as HidConfig, HidWriter, State as HidState};
        static PAD_STATE: StaticCell<HidState> = StaticCell::new();
        let pad_config = HidConfig {
            report_descriptor: GamepadReport::desc(),
            poll_ms: 1,
            request_handler: None,
            max_packet_size: 8,
        };
        Some(HidWriter::<'_, _, 8>::new(
            &mut builder,
            PAD_STATE.init(HidState::new()),
            pad_config,
        ))
    } else {
        None
    };

    #[cfg(feature = "midi")]
    let midi = if matches!(launch_mode, LaunchMode::Composite | LaunchMode::MidiOnly) {
        use embassy_usb::class::midi::MidiClass;
        // n_in_jacks=1 (device→host, keyer events), n_out_jacks=0 (host→device, unused).
        // NOTE: embassy-usb 0.4.0 always allocates both bulk endpoints regardless of jack counts,
        // so the OUT endpoint exists but is never read.
        Some(MidiClass::new(&mut builder, 1, 0, 64))
    } else {
        None
    };

    // 4. Build & Spawn
    let usb = builder.build();
    spawner.spawn(usb_task(usb)).unwrap();

    // 5. Run App
    let dit_pin = Input::new(p.PIN_14, Pull::Up);
    let dah_pin = Input::new(p.PIN_15, Pull::Up);

    let mut app = CwApp {
        #[cfg(feature = "keyboard")]
        keyboard,
        #[cfg(feature = "gamepad")]
        gamepad,
        #[cfg(feature = "serial")]
        serial,
        #[cfg(feature = "midi")]
        midi,
    };

    app.run(|| dit_pin.is_low(), || dah_pin.is_low()).await;
}

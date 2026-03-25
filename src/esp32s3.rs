#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_usb::Builder;
use esp_hal::gpio::{Input, Pull};
use esp_hal::interrupt::software::SoftwareInterruptControl;
use esp_hal::otg_fs::asynch::{Config as DriverConfig, Driver};
use esp_hal::otg_fs::Usb;
use esp_hal::timer::timg::TimerGroup;
use esp_hal_embassy::InterruptExecutor;
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

use {esp_backtrace as _, esp_println as _};

// IDF bootloader (bundled with espflash) reads esp_app_desc_t from the very
// start of the DROM segment (flash offset 0x10020) to validate the image.
// Without it the bootloader interprets code bytes as fields like
// min_efuse_blk_rev_full and may refuse to boot on older silicon.
// Layout mirrors esp_app_format.h in IDF 5.x (256 bytes total).
#[repr(C)]
struct EspAppDesc {
    magic: u32,                  // 0x00: must be 0xABCD5432
    secure_version: u32,         // 0x04
    _reserv1: [u32; 2],          // 0x08
    version: [u8; 32],           // 0x10
    project_name: [u8; 32],      // 0x30
    time: [u8; 16],              // 0x50
    date: [u8; 16],              // 0x60
    idf_ver: [u8; 32],           // 0x70
    app_elf_sha256: [u8; 32],    // 0x90  (zeros = not computed)
    min_efuse_blk_rev_full: u16, // 0xB0  0 = accept any chip
    max_efuse_blk_rev_full: u16, // 0xB2  0xFFFF = no upper limit
    _reserv2: [u32; 19],         // 0xB4..0xFF (pad to 256 bytes)
}

const _CHECK_SIZE: [u8; 256] = [0u8; core::mem::size_of::<EspAppDesc>()];

#[unsafe(link_section = ".esp_app_desc")]
#[used]
static ESP_APP_DESC: EspAppDesc = EspAppDesc {
    magic: 0xABCD5432,
    secure_version: 0,
    _reserv1: [0; 2],
    version: *b"0.1.0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
    project_name: *b"cw-adapter\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
    time: *b"\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
    date: *b"\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
    idf_ver: *b"esp-hal-0.23\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
    app_elf_sha256: [0u8; 32],
    min_efuse_blk_rev_full: 0,
    max_efuse_blk_rev_full: 0xFFFF,
    _reserv2: [0u32; 19],
};

// Defmt requires a timestamp! macro. A proper time source isn't available
// before the embassy time driver is initialised, so we emit a static 0.
// For real timing, replace with a call to esp_hal::time::now().ticks() or similar.
defmt::timestamp!("{=u64}", 0u64);

#[defmt::panic_handler]
fn defmt_panic() -> ! {
    esp_println::println!("defmt panic");
    loop {}
}

/// USB device task — runs at thread-mode (lowest) priority.
#[embassy_executor::task]
async fn usb_task(mut usb: embassy_usb::UsbDevice<'static, Driver<'static>>) {
    usb.run().await;
}

/// Keying task — runs at elevated priority via InterruptExecutor so that USB
/// enumeration/control work in `usb_task` cannot delay key reads or MIDI writes.
#[embassy_executor::task]
async fn keying_task(
    mut app: CwApp<'static, Driver<'static>>,
    dit_pin: Input<'static>,
    dah_pin: Input<'static>,
) {
    app.run(|| dit_pin.is_low(), || dah_pin.is_low()).await;
}

/// InterruptExecutor running on SWI0 at Priority2 (above thread-mode Priority1).
static INT_EXECUTOR: StaticCell<InterruptExecutor<0>> = StaticCell::new();

#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {
    let peripherals = esp_hal::init(esp_hal::Config::default());

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_hal_embassy::init(timg0.timer0);

    // Obtain software interrupt 0 for the high-priority executor.
    let sw_ints = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);

    // 1. Initial Launch Mode Detection
    // Three mode-select pins, all pull-up (connect to GND to activate).
    // S0=GPIO6, S1=GPIO7, S2=GPIO8.
    // Pin combination → mode:
    //   open/open/open → Composite   (all interfaces active)
    //   S0/open/open   → KeyboardOnly
    //   open/S1/open   → GamepadOnly
    //   open/open/S2   → SerialOnly
    //   S0/S1/open     → MidiOnly
    //   any other      → Composite   (fallback)
    let s0 = Input::new(peripherals.GPIO6, Pull::Up);
    let s1 = Input::new(peripherals.GPIO7, Pull::Up);
    let s2 = Input::new(peripherals.GPIO8, Pull::Up);

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
    // ESP32-S3: GPIO 20 is USB_D+, GPIO 19 is USB_D-
    let usb = Usb::new(peripherals.USB0, peripherals.GPIO20, peripherals.GPIO19);
    static EP_OUT_BUFFER: StaticCell<[u8; 1024]> = StaticCell::new();
    let driver = Driver::new(usb, EP_OUT_BUFFER.init([0; 1024]), DriverConfig::default());

    let mut config = embassy_usb::Config::new(0x16c0, 0x27db);
    config.manufacturer = Some("Custom CW");
    config.product = Some(launch_mode.product_name());
    config.serial_number = Some("87654321");
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
            max_packet_size: 8,
            request_handler: None,
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
            max_packet_size: 8,
            request_handler: None,
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
        use cw_adapter::midi_interrupt::MidiInterruptClass;
        // n_in_jacks=1 (device→host, keyer events), n_out_jacks=0 (host→device, unused).
        // Interrupt endpoint with poll_ms=1 guarantees 1 ms host polling (unlike bulk).
        Some(MidiInterruptClass::new(&mut builder, 1, 0, 64, 1))
    } else {
        None
    };

    // 4. Build & Spawn
    let usb = builder.build();

    // USB device task runs at default thread-mode priority (Priority1).
    spawner.spawn(usb_task(usb)).unwrap();

    // 5. Run keying task at elevated priority via InterruptExecutor.
    // SoftwareInterrupt<0> drives the executor at Priority2, ensuring key reads
    // and MIDI/HID writes preempt USB enumeration/control work.
    let int_executor = INT_EXECUTOR.init(InterruptExecutor::new(sw_ints.software_interrupt0));
    let hi_spawner = int_executor.start(esp_hal::interrupt::Priority::Priority2);

    let dit_pin = Input::new(peripherals.GPIO4, Pull::Up);
    let dah_pin = Input::new(peripherals.GPIO5, Pull::Up);

    let app = CwApp {
        #[cfg(feature = "keyboard")]
        keyboard,
        #[cfg(feature = "gamepad")]
        gamepad,
        #[cfg(feature = "serial")]
        serial,
        #[cfg(feature = "midi")]
        midi,
    };

    hi_spawner.spawn(keying_task(app, dit_pin, dah_pin)).unwrap();

    // Main thread has nothing left to do — yield forever.
    loop {
        embassy_time::Timer::after(embassy_time::Duration::from_secs(3600)).await;
    }
}

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
use cw_adapter::keyer_app::KeyerApp;
use cw_adapter::midi_interrupt::MidiInterruptClass;
use podsdr_keyer::{KeyerConfig, KeyerEngine};

use {esp_backtrace as _, esp_println as _};

// IDF bootloader app descriptor (see esp32s3.rs for full documentation).
#[repr(C)]
struct EspAppDesc {
    magic: u32,
    secure_version: u32,
    _reserv1: [u32; 2],
    version: [u8; 32],
    project_name: [u8; 32],
    time: [u8; 16],
    date: [u8; 16],
    idf_ver: [u8; 32],
    app_elf_sha256: [u8; 32],
    min_efuse_blk_rev_full: u16,
    max_efuse_blk_rev_full: u16,
    _reserv2: [u32; 19],
}

const _CHECK_SIZE: [u8; 256] = [0u8; core::mem::size_of::<EspAppDesc>()];

#[unsafe(link_section = ".esp_app_desc")]
#[used]
static ESP_APP_DESC: EspAppDesc = EspAppDesc {
    magic: 0xABCD5432,
    secure_version: 0,
    _reserv1: [0; 2],
    version: *b"0.1.0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
    project_name: *b"cw-keyer\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
    time: *b"\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
    date: *b"\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
    idf_ver: *b"esp-hal-0.23\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
    app_elf_sha256: [0u8; 32],
    min_efuse_blk_rev_full: 0,
    max_efuse_blk_rev_full: 0xFFFF,
    _reserv2: [0u32; 19],
};

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

/// Keying task — runs at elevated priority via InterruptExecutor.
#[embassy_executor::task]
async fn keying_task(
    mut app: KeyerApp<'static, Driver<'static>>,
    dit_pin: Input<'static>,
    dah_pin: Input<'static>,
) {
    app.run(|| dit_pin.is_low(), || dah_pin.is_low()).await;
}

/// MIDI settings reader task — reads incoming MIDI CC on channel 16.
#[embassy_executor::task]
async fn midi_settings_task(
    mut read_ep: <Driver<'static> as embassy_usb::driver::Driver<'static>>::EndpointOut,
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

/// InterruptExecutor running on SWI0 at Priority2 (above thread-mode Priority1).
static INT_EXECUTOR: StaticCell<InterruptExecutor<0>> = StaticCell::new();

#[esp_hal_embassy::main]
async fn main(spawner: Spawner) {
    let peripherals = esp_hal::init(esp_hal::Config::default());

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_hal_embassy::init(timg0.timer0);

    let sw_ints = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);

    info!("CW Keyer (MIDI) starting");

    // USB Driver & Config
    let usb = Usb::new(peripherals.USB0, peripherals.GPIO20, peripherals.GPIO19);
    static EP_OUT_BUFFER: StaticCell<[u8; 1024]> = StaticCell::new();
    let driver = Driver::new(usb, EP_OUT_BUFFER.init([0; 1024]), DriverConfig::default());

    let mut config = embassy_usb::Config::new(0x16c0, 0x27db);
    config.manufacturer = Some("Custom CW");
    config.product = Some("CW Keyer (MIDI)");
    config.serial_number = Some("87654321");
    config.max_power = 100;
    config.max_packet_size_0 = 64;
    // Single-function MIDI device
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

    // Build & Spawn
    let usb = builder.build();
    spawner.spawn(usb_task(usb)).unwrap();

    // Keying task at elevated priority
    let int_executor = INT_EXECUTOR.init(InterruptExecutor::new(sw_ints.software_interrupt0));
    let hi_spawner = int_executor.start(esp_hal::interrupt::Priority::Priority2);

    let dit_pin = Input::new(peripherals.GPIO4, Pull::Up);
    let dah_pin = Input::new(peripherals.GPIO5, Pull::Up);

    let engine = KeyerEngine::new(KeyerConfig::default());
    let app = KeyerApp {
        midi: midi_class,
        engine,
    };

    hi_spawner.spawn(keying_task(app, dit_pin, dah_pin)).unwrap();

    // MIDI settings reader on the main executor
    if let Some(read_ep) = midi_read_ep {
        spawner.spawn(midi_settings_task(read_ep)).unwrap();
    }

    loop {
        embassy_time::Timer::after(embassy_time::Duration::from_secs(3600)).await;
    }
}

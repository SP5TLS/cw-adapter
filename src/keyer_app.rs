use embassy_time::{Duration, Ticker};
use embassy_usb::driver::Driver;
use embassy_sync::channel::Channel;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;

use crate::midi_interrupt::MidiInterruptClass;
use podsdr_keyer::{KeyerEngine, KeyerMode, KeyerOutput};

// MIDI note for the formed key-line signal (C4)
const MIDI_NOTE_KEY: u8 = 60;

// --- MIDI CC settings protocol ---
// Settings are received as MIDI CC on channel 16 (status 0xBF).
// USB-MIDI packet: [CIN_status, 0xBF, cc_number, value]

pub const CC_MODE: u8 = 1;
pub const CC_SPEED_WPM: u8 = 2;
pub const CC_WEIGHT: u8 = 3;
pub const CC_KEYS_REVERSED: u8 = 4;
pub const CC_IAMBIC_B_TIMING: u8 = 5;
pub const CC_KEYING_COMP: u8 = 6;
pub const CC_FARNSWORTH: u8 = 7;
pub const CC_HANG_TIME: u8 = 8;
pub const CC_AUTO_SPACING: u8 = 9;
pub const CC_DYNAMIC_RATIO: u8 = 10;

/// A single keyer config update received via MIDI CC.
#[derive(Debug, Clone, Copy)]
pub enum ConfigUpdate {
    Mode(KeyerMode),
    SpeedWpm(u8),
    Weight(u8),
    KeysReversed(bool),
    IambicBTiming(u8),
    KeyingComp(u8),
    Farnsworth(u8),
    HangTime(u32),
    AutoSpacing(bool),
    DynamicRatio(bool),
}

/// Shared channel for config updates from MIDI reader to keying task.
pub static CONFIG_CHANNEL: Channel<CriticalSectionRawMutex, ConfigUpdate, 8> = Channel::new();

/// Parse a USB-MIDI CC packet on channel 16 into a ConfigUpdate.
pub fn parse_midi_cc(packet: &[u8]) -> Option<ConfigUpdate> {
    if packet.len() < 4 {
        return None;
    }
    let status = packet[1];
    if status != 0xBF {
        return None;
    }
    let cc = packet[2];
    let val = packet[3];
    match cc {
        CC_MODE => {
            let mode = match val {
                0 => KeyerMode::Straight,
                1 => KeyerMode::IambicA,
                2 => KeyerMode::IambicB,
                3 => KeyerMode::Bug,
                4 => KeyerMode::Ultimatic,
                5 => KeyerMode::SinglePaddle,
                _ => return None,
            };
            Some(ConfigUpdate::Mode(mode))
        }
        CC_SPEED_WPM => Some(ConfigUpdate::SpeedWpm(val.clamp(5, 60))),
        CC_WEIGHT => Some(ConfigUpdate::Weight(val.clamp(25, 75))),
        CC_KEYS_REVERSED => Some(ConfigUpdate::KeysReversed(val != 0)),
        CC_IAMBIC_B_TIMING => Some(ConfigUpdate::IambicBTiming(val.min(100))),
        CC_KEYING_COMP => Some(ConfigUpdate::KeyingComp(val.min(50))),
        CC_FARNSWORTH => Some(ConfigUpdate::Farnsworth(val.min(60))),
        CC_HANG_TIME => Some(ConfigUpdate::HangTime(val as u32 * 10)),
        CC_AUTO_SPACING => Some(ConfigUpdate::AutoSpacing(val != 0)),
        CC_DYNAMIC_RATIO => Some(ConfigUpdate::DynamicRatio(val != 0)),
        _ => None,
    }
}

fn apply_config_update(engine: &mut KeyerEngine, update: ConfigUpdate) {
    match update {
        ConfigUpdate::Mode(m) => engine.config.mode = m,
        ConfigUpdate::SpeedWpm(v) => engine.config.speed_wpm = v,
        ConfigUpdate::Weight(v) => engine.config.weight = v,
        ConfigUpdate::KeysReversed(v) => engine.config.keys_reversed = v,
        ConfigUpdate::IambicBTiming(v) => engine.config.iambic_b_timing_percent = v,
        ConfigUpdate::KeyingComp(v) => engine.config.keying_compensation_ms = v,
        ConfigUpdate::Farnsworth(v) => engine.config.farnsworth_wpm = v,
        ConfigUpdate::HangTime(v) => engine.config.hang_time_ms = v,
        ConfigUpdate::AutoSpacing(v) => engine.config.auto_spacing = v,
        ConfigUpdate::DynamicRatio(v) => engine.config.dynamic_ratio = v,
    }
}

// --- Debouncer ---

pub struct Debouncer {
    state: bool,
    integration_counter: u8,
    threshold: u8,
}

impl Debouncer {
    pub fn new(initial_state: bool, threshold: u8) -> Self {
        debug_assert!(threshold > 0, "Debouncer threshold must be > 0");
        Self {
            state: initial_state,
            integration_counter: 0,
            threshold,
        }
    }

    pub fn update(&mut self, raw_state: bool) -> bool {
        if raw_state != self.state {
            self.integration_counter += 1;
            if self.integration_counter >= self.threshold {
                self.state = raw_state;
                self.integration_counter = 0;
            }
        } else {
            self.integration_counter = 0;
        }
        self.state
    }
}

// --- Keyer App ---

pub struct KeyerApp<'a, D: Driver<'a>> {
    pub midi: MidiInterruptClass<'a, D>,
    pub engine: KeyerEngine,
}

impl<'a, D: Driver<'a>> KeyerApp<'a, D> {
    pub async fn run(
        &mut self,
        mut dit_paddle: impl FnMut() -> bool,
        mut dah_paddle: impl FnMut() -> bool,
    ) -> ! {
        let mut dit_debounce = Debouncer::new(false, 8);
        let mut dah_debounce = Debouncer::new(false, 8);
        let mut prev_key_down = false;
        let mut ticker = Ticker::every(Duration::from_millis(1));

        loop {
            ticker.next().await;

            // Read and debounce paddles
            let dit_pressed = dit_debounce.update(dit_paddle());
            let dah_pressed = dah_debounce.update(dah_paddle());

            // Feed paddles to keyer engine
            self.engine.set_paddle(dit_pressed, dah_pressed);

            // Check for config updates from MIDI settings task
            while let Ok(update) = CONFIG_CHANNEL.try_receive() {
                apply_config_update(&mut self.engine, update);
            }

            // Tick the keyer engine (1ms per tick)
            let output = self.engine.tick();

            // Map keyer output to MIDI
            let key_down = match output {
                Some(KeyerOutput::KeyDown) => true,
                Some(KeyerOutput::KeyUp) => false,
                Some(KeyerOutput::PttRequest(_)) | None => continue,
            };

            if key_down == prev_key_down {
                continue;
            }
            prev_key_down = key_down;

            let packet = if key_down {
                [0x09, 0x90, MIDI_NOTE_KEY, 0x7F] // Note On
            } else {
                [0x08, 0x80, MIDI_NOTE_KEY, 0x00] // Note Off
            };
            self.midi.write_packet(&packet).await.ok();
        }
    }
}

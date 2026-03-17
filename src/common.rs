use embassy_time::{Duration, Timer};
use embassy_usb::driver::Driver;
#[cfg(any(feature = "keyboard", feature = "gamepad"))]
use embassy_usb::class::hid::HidWriter;
// generator_prelude::* is always needed for the #[gen_hid_descriptor] macro on GamepadReport.
use usbd_hid::descriptor::generator_prelude::*;
#[cfg(feature = "keyboard")]
use usbd_hid::descriptor::KeyboardReport;
#[cfg(feature = "serial")]
use crate::cdc_serial_state::CdcWithSerialState;

#[cfg(feature = "midi")]
use embassy_usb::class::midi::MidiClass;

// MIDI note numbers (vail-adapter convention: C4=dit, D4=dah)
#[cfg(feature = "midi")]
const MIDI_NOTE_DIT: u8 = 60;
#[cfg(feature = "midi")]
const MIDI_NOTE_DAH: u8 = 62;

// --- HID Descriptors ---

#[gen_hid_descriptor(
    (collection = APPLICATION, usage_page = GENERIC_DESKTOP, usage = GAMEPAD) = {
        (usage_page = BUTTON, usage_min = 1, usage_max = 8) = {
            #[packed_bits 8] #[item_settings data, variable, absolute] buttons=input;
        };
    }
)]
pub struct GamepadReport {
    pub buttons: u8,
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

// --- App Logic ---

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum LaunchMode {
    Composite,
    #[cfg(feature = "keyboard")]
    KeyboardOnly,
    #[cfg(feature = "gamepad")]
    GamepadOnly,
    #[cfg(feature = "serial")]
    SerialOnly,
    #[cfg(feature = "midi")]
    MidiOnly,
}

impl LaunchMode {
    pub fn product_name(self) -> &'static str {
        match self {
            LaunchMode::Composite => "CW Interface",
            #[cfg(feature = "keyboard")]
            LaunchMode::KeyboardOnly => "CW Interface (Keys)",
            #[cfg(feature = "gamepad")]
            LaunchMode::GamepadOnly => "CW Interface (Pad)",
            #[cfg(feature = "serial")]
            LaunchMode::SerialOnly => "CW Interface (Serial)",
            #[cfg(feature = "midi")]
            LaunchMode::MidiOnly => "CW Interface (MIDI)",
        }
    }
}

pub struct CwApp<'a, D: Driver<'a>> {
    #[cfg(feature = "keyboard")]
    pub keyboard: Option<HidWriter<'a, D, 8>>,
    #[cfg(feature = "gamepad")]
    pub gamepad: Option<HidWriter<'a, D, 8>>,
    #[cfg(feature = "serial")]
    pub serial: Option<CdcWithSerialState<'a, D>>,
    #[cfg(feature = "midi")]
    pub midi: Option<MidiClass<'a, D>>,
}

impl<'a, D: Driver<'a>> CwApp<'a, D> {
    pub async fn run(
        &mut self,
        mut dit_paddle: impl FnMut() -> bool,
        mut dah_paddle: impl FnMut() -> bool,
    ) -> ! {
        // Paddles are active-low (Pull::Up). At rest is_low() == false == not pressed.
        let mut dit_debounce = Debouncer::new(false, 5);
        let mut dah_debounce = Debouncer::new(false, 5);

        #[cfg(feature = "serial")]
        let mut prev_dit_ser = false;
        #[cfg(feature = "serial")]
        let mut prev_dah_ser = false;

        #[cfg(feature = "midi")]
        let mut prev_dit = false;
        #[cfg(feature = "midi")]
        let mut prev_dah = false;

        loop {
            let raw_dit = dit_paddle();
            let raw_dah = dah_paddle();

            let dit_pressed = dit_debounce.update(raw_dit);
            let dah_pressed = dah_debounce.update(raw_dah);

            // 1. Keyboard Output
            #[cfg(feature = "keyboard")]
            if let Some(ref mut kbd) = self.keyboard {
                let mut key_report = KeyboardReport::default();
                // Pack pressed keys into the keycodes array in order.
                // index advances after each entry so simultaneous presses land in separate slots.
                let mut index = 0usize;
                if dit_pressed {
                    key_report.keycodes[index] = 0x1D; // 'z'
                    index += 1;
                }
                if dah_pressed {
                    key_report.keycodes[index] = 0x1B; // 'x'
                    index += 1;
                }
                let _ = index;
                kbd.write_serialize(&key_report).await.ok();
            }

            // 2. Gamepad Output
            #[cfg(feature = "gamepad")]
            if let Some(ref mut pad) = self.gamepad {
                let mut buttons = 0u8;
                if dit_pressed {
                    buttons |= 0b0000_0001;
                }
                if dah_pressed {
                    buttons |= 0b0000_0010;
                }
                pad.write_serialize(&GamepadReport { buttons }).await.ok();
            }

            // 3. Serial State Output (DCD = dit, DSR = dah) — event-driven, send on state transitions only
            #[cfg(feature = "serial")]
            if let Some(ref mut ser) = self.serial {
                if dit_pressed != prev_dit_ser || dah_pressed != prev_dah_ser {
                    ser.send_serial_state(dit_pressed, dah_pressed).await.ok();
                    prev_dit_ser = dit_pressed;
                    prev_dah_ser = dah_pressed;
                }
            }

            // 4. MIDI Output — event-driven, send on state transitions only
            #[cfg(feature = "midi")]
            if let Some(ref mut midi) = self.midi {
                let dit_changed = dit_pressed != prev_dit;
                let dah_changed = dah_pressed != prev_dah;
                if dit_changed || dah_changed {
                    let mut buf = [0u8; 8];
                    let mut len = 0;
                    if dit_changed {
                        buf[len..len + 4].copy_from_slice(if dit_pressed {
                            &[0x09, 0x90, MIDI_NOTE_DIT, 0x7F]
                        } else {
                            &[0x08, 0x80, MIDI_NOTE_DIT, 0x00]
                        });
                        len += 4;
                        prev_dit = dit_pressed;
                    }
                    if dah_changed {
                        buf[len..len + 4].copy_from_slice(if dah_pressed {
                            &[0x09, 0x90, MIDI_NOTE_DAH, 0x7F]
                        } else {
                            &[0x08, 0x80, MIDI_NOTE_DAH, 0x00]
                        });
                        len += 4;
                        prev_dah = dah_pressed;
                    }
                    midi.write_packet(&buf[..len]).await.ok();
                }
            }

            Timer::after(Duration::from_millis(1)).await;
        }
    }
}

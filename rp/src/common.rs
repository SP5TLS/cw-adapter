#[cfg(feature = "serial")]
use crate::cdc_serial_state::CdcWithSerialState;
#[cfg(any(feature = "serial", feature = "midi"))]
use embassy_time::{Duration, Timer};
use embassy_usb::driver::Driver;

#[cfg(feature = "midi")]
use crate::midi_interrupt::MidiInterruptClass;

// MIDI note numbers (vail-adapter convention: C4=dit, D4=dah)
#[cfg(feature = "midi")]
const MIDI_NOTE_DIT: u8 = 60;
#[cfg(feature = "midi")]
const MIDI_NOTE_DAH: u8 = 62;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, defmt::Format)]
pub enum LaunchMode {
    Composite,
    #[cfg(feature = "serial")]
    SerialOnly,
    #[cfg(feature = "midi")]
    MidiOnly,
}

impl LaunchMode {
    pub fn product_name(self) -> &'static str {
        match self {
            LaunchMode::Composite => "CW Interface",
            #[cfg(feature = "serial")]
            LaunchMode::SerialOnly => "CW Interface (Serial)",
            #[cfg(feature = "midi")]
            LaunchMode::MidiOnly => "CW Interface (MIDI)",
        }
    }
}

pub struct CwApp<'a, D: Driver<'a>> {
    #[cfg(feature = "serial")]
    pub serial: Option<CdcWithSerialState<'a, D>>,
    #[cfg(feature = "midi")]
    pub midi: Option<MidiInterruptClass<'a, D>>,
    // Keep the generic `D` referenced when no transport feature is on, so the
    // crate (and rp2350_blinky, which depends on it implicitly) still compiles.
    #[cfg(not(any(feature = "serial", feature = "midi")))]
    pub _marker: core::marker::PhantomData<&'a D>,
}

#[cfg(any(feature = "serial", feature = "midi"))]
impl<'a, D: Driver<'a>> CwApp<'a, D> {
    pub async fn run(
        &mut self,
        mut dit_paddle: impl FnMut() -> bool,
        mut dah_paddle: impl FnMut() -> bool,
    ) -> ! {
        // Paddles are active-low (Pull::Up). At rest is_low() == false == not pressed.
        let mut dit_debounce = Debouncer::new(false, 8);
        let mut dah_debounce = Debouncer::new(false, 8);

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

            // Serial State Output (DCD = dit, DSR = dah) — event-driven, send on state transitions only
            #[cfg(feature = "serial")]
            if let Some(ref mut ser) = self.serial
                && (dit_pressed != prev_dit_ser || dah_pressed != prev_dah_ser)
            {
                ser.send_serial_state(dit_pressed, dah_pressed).await.ok();
                prev_dit_ser = dit_pressed;
                prev_dah_ser = dah_pressed;
            }

            // MIDI Output — event-driven, send on state transitions only
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
                    }
                    if dah_changed {
                        buf[len..len + 4].copy_from_slice(if dah_pressed {
                            &[0x09, 0x90, MIDI_NOTE_DAH, 0x7F]
                        } else {
                            &[0x08, 0x80, MIDI_NOTE_DAH, 0x00]
                        });
                        len += 4;
                    }
                    match midi.write_packet(&buf[..len]).await {
                        Ok(()) => {
                            if dit_changed {
                                prev_dit = dit_pressed;
                            }
                            if dah_changed {
                                prev_dah = dah_pressed;
                            }
                        }
                        Err(_) => {
                            // Reset so we re-send state after reconnect.
                            prev_dit = false;
                            prev_dah = false;
                        }
                    }
                }
            }

            Timer::after(Duration::from_micros(250)).await;
        }
    }
}

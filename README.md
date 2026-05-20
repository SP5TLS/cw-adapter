# CW Interface (RP2040 / RP2350 / ESP32-S3)

A low-latency CW (Morse code) interface that appears as a composite USB device exposing two outputs:
1. **Serial Port (CDC-ACM)**: Reports paddle state via modem status lines — DCD (Dit) and DSR (Dah).
2. **USB MIDI**: Sends MIDI Note On/Off events — Note C4 (60) for Dit, Note D4 (62) for Dah (vail-adapter convention).

## Repository layout

Two independent Cargo crates, each pinned to its own ecosystem:

* `rp/`      — RP2040 + RP2350 firmware on the embassy-rp 0.10 / embassy-usb 0.6 stack.
* `esp32s3/` — ESP32-S3 firmware on esp-hal 0.23 / embassy-usb 0.4.

The split is forced by Cargo: both stacks declare `links = "embassy-time-queue"` via different
`embassy-time-queue-utils` versions, and a single resolved graph can only claim that links field
once. A virtual workspace is therefore deliberately *not* used. `scripts/check_shared_sources.py`
runs in CI to catch unintended drift between the two copies of `cdc_serial_state.rs`,
`common.rs`, and `midi_interrupt.rs`.

## Pinout

### RP2040 / RP2350 (e.g., Raspberry Pi Pico, Pico 2)
Same wiring on both chips.
- **GP14**: Dit Paddle (active low)
- **GP15**: Dah Paddle (active low)
- **GP16**: Mode Switch S0 — pull LOW to force Serial-only at boot
- **GP17**: Mode Switch S1 — pull LOW to force MIDI-only at boot

The RP2350 build targets RP2350A (QFN-60, Pico 2 silicon). For RP2350B (QFN-80) boards, change `embassy-rp/rp235xa` to `embassy-rp/rp235xb` in `rp/Cargo.toml`.

### ESP32-S3
- **GPIO4**: Dit Paddle (active low)
- **GPIO5**: Dah Paddle (active low)
- **GPIO6**: Mode Switch S0 — pull LOW to force Serial-only at boot
- **GPIO7**: Mode Switch S1 — pull LOW to force MIDI-only at boot
- **GPIO19**: USB D-
- **GPIO20**: USB D+

## Launch Modes

The two mode-select pins are read at startup (internal pull-ups enabled; connect pin to GND to activate). Both open = Composite (Serial + MIDI both visible to host).

| S0 (GP16/GPIO6) | S1 (GP17/GPIO7) | Mode        | USB interfaces active             |
|-----------------|-----------------|-------------|-----------------------------------|
| open            | open            | Composite   | All compiled-in interfaces        |
| **GND**         | open            | Serial Only | CDC-ACM serial port               |
| open            | **GND**         | MIDI Only   | USB MIDI                          |
| **GND**         | **GND**         | Composite   | All compiled-in interfaces (fallback) |

## Building

By default both interfaces are compiled in and selected at runtime via the mode switch pins. You can instead compile a single-interface build, which eliminates the unused code. Mode-switch pins are still read in single-interface builds but their values are ignored — the firmware always activates the one compiled-in interface.

### For RP2040:
```bash
cd rp

# Both interfaces (runtime mode switch selects which one is active)
cargo +nightly build --release --target thumbv6m-none-eabi --bin rp2040 --no-default-features --features rp2040,serial,midi

# Serial only
cargo +nightly build --release --target thumbv6m-none-eabi --bin rp2040 --no-default-features --features rp2040,serial

# MIDI only
cargo +nightly build --release --target thumbv6m-none-eabi --bin rp2040 --no-default-features --features rp2040,midi
```

### For RP2350 (Pico 2):
```bash
cd rp

# Both interfaces
cargo +nightly build --release --target thumbv8m.main-none-eabihf --bin rp2350 --no-default-features --features rp2350,serial,midi

# Serial only
cargo +nightly build --release --target thumbv8m.main-none-eabihf --bin rp2350 --no-default-features --features rp2350,serial

# MIDI only
cargo +nightly build --release --target thumbv8m.main-none-eabihf --bin rp2350 --no-default-features --features rp2350,midi
```

`elf2uf2-rs` hardcodes the RP2040 family ID, so RP2350 UF2 outputs are patched by `scripts/patch_uf2_family.py` to the RP2350-ARM-S family ID. The Pico 2 BOOTSEL drive otherwise rejects the file.

### For ESP32-S3:
Requires the Xtensa toolchain (`espup install`).
```bash
cd esp32s3

# Both interfaces
cargo +esp build --release --target xtensa-esp32s3-none-elf -Zbuild-std=core --bin esp32s3 --no-default-features --features serial,midi

# Serial only
cargo +esp build --release --target xtensa-esp32s3-none-elf -Zbuild-std=core --bin esp32s3 --no-default-features --features serial

# MIDI only
cargo +esp build --release --target xtensa-esp32s3-none-elf -Zbuild-std=core --bin esp32s3 --no-default-features --features midi
```

`cargo run` in `esp32s3/` is wired to `scripts/run.sh`, which performs the `save-image → patch_esp32s3.py → write-bin → monitor` recipe in one shot. Set `ESPFLASH_PORT` to skip the interactive port picker.

## Flashing

### RP2040 / RP2350 with probe-rs
```bash
cd rp
cargo +nightly run --bin rp2040 --no-default-features --features rp2040,serial,midi --target thumbv6m-none-eabi
cargo +nightly run --bin rp2350 --no-default-features --features rp2350,serial,midi --target thumbv8m.main-none-eabihf
```

### ESP32-S3 with espflash
```bash
cd esp32s3
cargo +esp run --release --no-default-features --features serial,midi
# Or specify port:
ESPFLASH_PORT=/dev/ttyUSB0 cargo +esp run --release --no-default-features --features serial,midi
```

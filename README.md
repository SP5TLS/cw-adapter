# CW Interface (RP2040 / RP2350 / ESP32-S3)

This project implements a low-latency CW (Morse code) interface that appears as a composite USB device:
1. **HID Keyboard**: Sends 'z' for Dit and 'x' for Dah (HID keycodes 0x1D / 0x1B; actual character depends on host Caps Lock / Shift state, typically lowercase).
2. **HID Gamepad**: Sends Button 1 for Dit and Button 2 for Dah.
3. **Serial Port (CDC-ACM)**: Reports paddle state via modem status lines — DCD (Dit) and DSR (Dah).
4. **USB MIDI**: Sends MIDI Note On/Off events — Note C4 (60) for Dit, Note D4 (62) for Dah (vail-adapter convention).

## Pinout

### RP2040 / RP2350 (e.g., Raspberry Pi Pico, Pico 2)
Same wiring on both chips.
- **GP14**: Dit Paddle (active low)
- **GP15**: Dah Paddle (active low)
- **GP16**: Mode Switch S0 (see table below)
- **GP17**: Mode Switch S1 (see table below)
- **GP18**: Mode Switch S2 (see table below)

The RP2350 build targets RP2350A (QFN-60, Pico 2 silicon). For RP2350B (QFN-80) boards, change `embassy-rp/rp235xa` to `embassy-rp/rp235xb` in `Cargo.toml`.

### ESP32-S3
- **GPIO4**: Dit Paddle (active low)
- **GPIO5**: Dah Paddle (active low)
- **GPIO6**: Mode Switch S0 (see table below)
- **GPIO7**: Mode Switch S1 (see table below)
- **GPIO8**: Mode Switch S2 (see table below)
- **GPIO19**: USB D-
- **GPIO20**: USB D+

## Launch Modes

Three mode-select pins are read at startup (internal pull-ups enabled; connect pin to GND to activate). No jumpers = Composite (default).

| S0 (GP16/GPIO6) | S1 (GP17/GPIO7) | S2 (GP18/GPIO8) | Mode          | USB interfaces active                 |
|-----------------|-----------------|-----------------|---------------|---------------------------------------|
| open            | open            | open            | Composite     | All compiled-in interfaces            |
| **GND**         | open            | open            | Keyboard Only | HID keyboard                          |
| open            | **GND**         | open            | Gamepad Only  | HID gamepad                           |
| open            | open            | **GND**         | Serial Only   | CDC-ACM serial port                   |
| **GND**         | **GND**         | open            | MIDI Only     | USB MIDI                              |
| any other combo |                 |                 | Composite     | All compiled-in interfaces (fallback) |

## Building

By default all three interfaces (keyboard, gamepad, serial) are compiled in and selected at runtime via the mode switch pins. You can instead compile firmware with only a single interface, which eliminates the unused code entirely. Note: the mode switch pins are still read at startup in single-interface builds, but their values are ignored — the firmware always activates the one compiled-in interface regardless of pin state.

### For RP2040:
```bash
# All interfaces (runtime mode switch selects which one is active)
cargo build --bin rp2040 --features rp2040 --target thumbv6m-none-eabi

# Keyboard only
cargo build --bin rp2040 --no-default-features --features rp2040,keyboard,defmt --target thumbv6m-none-eabi

# Gamepad only
cargo build --bin rp2040 --no-default-features --features rp2040,gamepad,defmt --target thumbv6m-none-eabi

# Serial only
cargo build --bin rp2040 --no-default-features --features rp2040,serial,defmt --target thumbv6m-none-eabi

# MIDI only
cargo build --bin rp2040 --no-default-features --features rp2040,midi,defmt --target thumbv6m-none-eabi
```

### For RP2350 (Pico 2):
```bash
# All interfaces (runtime mode switch selects which one is active)
cargo build --bin rp2350 --no-default-features --features rp2350,defmt,keyboard,gamepad,serial,midi --target thumbv8m.main-none-eabihf

# Keyboard only
cargo build --bin rp2350 --no-default-features --features rp2350,keyboard,defmt --target thumbv8m.main-none-eabihf

# Gamepad only
cargo build --bin rp2350 --no-default-features --features rp2350,gamepad,defmt --target thumbv8m.main-none-eabihf

# Serial only
cargo build --bin rp2350 --no-default-features --features rp2350,serial,defmt --target thumbv8m.main-none-eabihf

# MIDI only
cargo build --bin rp2350 --no-default-features --features rp2350,midi,defmt --target thumbv8m.main-none-eabihf
```

The UF2 conversion uses `elf2uf2-rs` followed by `python3 scripts/patch_uf2_family.py <file.uf2> 0xe48bff59` to rewrite the family ID from RP2040 to RP2350-ARM-S. The Pico 2 BOOTSEL drive rejects RP2040 family IDs.

### For ESP32-S3:
Note: Requires Xtensa toolchain.
```bash
# All interfaces (runtime mode switch selects which one is active)
cargo build --bin esp32s3 --features esp32s3 --target xtensa-esp32s3-none-elf

# Keyboard only
cargo build --bin esp32s3 --no-default-features --features esp32s3,keyboard,defmt --target xtensa-esp32s3-none-elf

# Gamepad only
cargo build --bin esp32s3 --no-default-features --features esp32s3,gamepad,defmt --target xtensa-esp32s3-none-elf

# Serial only
cargo build --bin esp32s3 --no-default-features --features esp32s3,serial,defmt --target xtensa-esp32s3-none-elf

# MIDI only
cargo build --bin esp32s3 --no-default-features --features esp32s3,midi,defmt --target xtensa-esp32s3-none-elf
```

## Flashing

### For RP2040:
```bash
probe-rs run --chip RP2040 --bin rp2040 --features rp2040
```

### For RP2350 (Pico 2):
```bash
probe-rs run --chip RP235x --bin rp2350 --features rp2350
```

### For ESP32-S3:
```bash
espflash flash --monitor --bin esp32s3 --features esp32s3
```

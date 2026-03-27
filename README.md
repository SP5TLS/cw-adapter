# CW Interface (RP2040 & ESP32-S3)

This project implements a low-latency CW (Morse code) interface that appears as a composite USB device:
1. **HID Keyboard**: Sends 'z' for Dit and 'x' for Dah (HID keycodes 0x1D / 0x1B; actual character depends on host Caps Lock / Shift state, typically lowercase).
2. **HID Gamepad**: Sends Button 1 for Dit and Button 2 for Dah.
3. **Serial Port (CDC-ACM)**: Reports paddle state via modem status lines — DCD (Dit) and DSR (Dah).
4. **USB MIDI**: Sends MIDI Note On/Off events — Note C4 (60) for Dit, Note D4 (62) for Dah (vail-adapter convention).

## Pinout

### RP2040 (e.g., Raspberry Pi Pico)
- **GP14**: Dit Paddle (active low)
- **GP15**: Dah Paddle (active low)
- **GP16**: Mode Switch S0 (see table below)
- **GP17**: Mode Switch S1 (see table below)
- **GP18**: Mode Switch S2 (see table below)

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

## CW Keyer Mode

A separate firmware variant adds a built-in iambic keyer engine (from `podsdr-keyer`). Instead of passing raw paddle state to the host, the keyer forms complete CW elements locally and outputs a single key-line signal via USB MIDI (Note On/Off, note 60).

### Keyer Pinout

The keyer builds re-use the mode-select pins (unused in keyer firmware) for a sidetone buzzer and two command buttons.

| Function | RP2040 | ESP32-S3 | Notes |
|----------|--------|----------|-------|
| Dit paddle | GP14 | GPIO4 | Active low (internal pull-up) |
| Dah paddle | GP15 | GPIO5 | Active low (internal pull-up) |
| Buzzer | GP16 | GPIO6 | PWM output, 600 Hz square wave |
| Button A (Speed −) | GP17 | GPIO7 | Active low (internal pull-up) |
| Button B (Speed +) | GP18 | GPIO8 | Active low (internal pull-up) |

### Buzzer Wiring

Connect a passive piezo buzzer between the buzzer pin and GND. No driver transistor is needed for small piezo elements (< 20 mA) — the GPIO can drive them directly.

```
Buzzer Pin ──┬── Passive Piezo (+)
             │
            GND ── Passive Piezo (−)
```

For louder output or a speaker, add an NPN transistor (e.g. 2N2222) or MOSFET:

```
Buzzer Pin ──[ 1kΩ ]──┬── Base (B)
                       │
              Speaker ─┤── Collector (C)
                +V ────┘
                       │
              GND ─────┤── Emitter (E)
```

### Button Functions

Inspired by the [k3ng CW keyer](https://github.com/k3ng/k3ng_cw_keyer) command button design:

| Action | Effect |
|--------|--------|
| Press Button A | Decrease speed by 1 WPM |
| Press Button B | Increase speed by 1 WPM |
| Hold Button A/B | Auto-repeat speed change (500 ms initial delay, then every 100 ms) |
| Press both A+B simultaneously | Cycle keyer mode: Iambic B → A → Straight → Bug → Ultimatic → Single Paddle → Iambic B |

Wire each button between the pin and GND (normally open, momentary). Internal pull-ups are enabled.

### MIDI CC Settings

The keyer accepts runtime configuration via MIDI Control Change messages on **channel 16** (status byte `0xBF`):

| CC # | Parameter | Value range |
|------|-----------|-------------|
| 1 | Mode | 0=Straight, 1=Iambic A, 2=Iambic B, 3=Bug, 4=Ultimatic, 5=Single Paddle |
| 2 | Speed (WPM) | 5–60 |
| 3 | Weight | 25–75 (50 = standard) |
| 4 | Keys reversed | 0/1 |
| 5 | Iambic B timing % | 0–100 (33 = CMOS Super Keyer default) |
| 6 | Keying compensation (ms) | 0–50 |
| 7 | Farnsworth WPM | 0–60 (0 = disabled) |
| 8 | Hang time | 0–127 (value × 10 ms) |
| 9 | Auto spacing | 0/1 |
| 10 | Dynamic ratio | 0/1 |

Default: Iambic B, 23 WPM, weight 50, 600 Hz sidetone.

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

# CW Keyer (MIDI, built-in iambic keyer)
cargo build --bin rp2040-keyer --no-default-features --features rp2040,keyer,defmt --target thumbv6m-none-eabi
```

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

# CW Keyer (MIDI, built-in iambic keyer)
cargo build --bin esp32s3-keyer --no-default-features --features esp32s3,keyer,defmt --target xtensa-esp32s3-none-elf
```

## Flashing

### For RP2040:
```bash
probe-rs run --chip RP2040 --bin rp2040 --features rp2040
```

### For ESP32-S3:
```bash
espflash flash --monitor --bin esp32s3 --features esp32s3
```

#!/usr/bin/env bash
# Run inside cw-adapter-builder container.
# Builds all ESP32-S3 firmware variants and writes flash-ready binaries to /app/_site/.
set -euo pipefail

[ -f /home/esp/export-esp.sh ] && source /home/esp/export-esp.sh

mkdir -p /app/_site

for feat in all keyboard gamepad serial midi; do
    BUILD_FEAT="esp32s3,defmt"
    [ "$feat" = "all" ] \
        && BUILD_FEAT="$BUILD_FEAT,keyboard,gamepad,serial,midi" \
        || BUILD_FEAT="$BUILD_FEAT,$feat"

    echo "=== Building ESP32-S3 / $feat ==="
    cargo +esp build --release \
        --target xtensa-esp32s3-none-elf \
        --no-default-features \
        --features "$BUILD_FEAT" \
        --bin esp32s3 \
        -Zbuild-std=core

    echo "=== Generating merged flash image for $feat ==="
    espflash save-image \
        --chip esp32s3 \
        --ignore-app-descriptor \
        --merge \
        target/xtensa-esp32s3-none-elf/release/esp32s3 \
        /app/_site/firmware-${feat}.bin

    python3 /app/scripts/patch_esp32s3.py /app/_site/firmware-${feat}.bin /app/_site/firmware-${feat}.bin --offset 0x10000
    python3 /app/scripts/trim-binary.py /app/_site/firmware-${feat}.bin
done

echo "=== Building RP2040 variants ==="
for feat in all keyboard gamepad serial midi; do
    BUILD_FEAT="rp2040,defmt"
    [ "$feat" = "all" ] \
        && BUILD_FEAT="$BUILD_FEAT,keyboard,gamepad,serial,midi" \
        || BUILD_FEAT="$BUILD_FEAT,$feat"

    echo "=== Building RP2040 / $feat ==="
    cargo +nightly build --release \
        --target thumbv6m-none-eabi \
        --no-default-features \
        --features "$BUILD_FEAT" \
        --bin rp2040

    echo "=== Converting to UF2 for $feat ==="
    elf2uf2-rs \
        target/thumbv6m-none-eabi/release/rp2040 \
        /app/_site/firmware-rp2040-${feat}.uf2
done

# RP2350-ARM-Secure family ID (Pico 2). The BOOTSEL drive rejects UF2 files
# that carry the RP2040 family ID, so we patch the field after elf2uf2-rs.
RP2350_FAMILY_ID=0xe48bff59

echo "=== Building RP2350 variants ==="
for feat in all keyboard gamepad serial midi; do
    BUILD_FEAT="rp2350,defmt"
    [ "$feat" = "all" ] \
        && BUILD_FEAT="$BUILD_FEAT,keyboard,gamepad,serial,midi" \
        || BUILD_FEAT="$BUILD_FEAT,$feat"

    echo "=== Building RP2350 / $feat ==="
    cargo +nightly build --release \
        --target thumbv8m.main-none-eabihf \
        --no-default-features \
        --features "$BUILD_FEAT" \
        --bin rp2350

    echo "=== Converting to UF2 for $feat ==="
    elf2uf2-rs \
        target/thumbv8m.main-none-eabihf/release/rp2350 \
        /app/_site/firmware-rp2350-${feat}.uf2
    python3 /app/scripts/patch_uf2_family.py \
        /app/_site/firmware-rp2350-${feat}.uf2 "$RP2350_FAMILY_ID"
done

echo "=== Done ==="
ls -lh /app/_site/firmware-*.bin /app/_site/firmware-*.uf2

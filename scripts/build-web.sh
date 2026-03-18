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

echo "=== Done ==="
ls -lh /app/_site/firmware-*.bin /app/_site/firmware-*.uf2

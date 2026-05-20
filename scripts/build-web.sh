#!/usr/bin/env bash
# Run inside cw-adapter-builder container.
# Builds all firmware variants and writes flash-ready binaries to /app/_site/.
set -euo pipefail

[ -f /home/esp/export-esp.sh ] && source /home/esp/export-esp.sh

mkdir -p /app/_site

# ── ESP32-S3 ─────────────────────────────────────────────────────────────────
for feat in all serial midi; do
    [ "$feat" = "all" ] && BUILD_FEAT="serial,midi" || BUILD_FEAT="$feat"

    echo "=== Building ESP32-S3 / $feat ==="
    (cd /app/esp32s3 && cargo +esp build --release \
        --target xtensa-esp32s3-none-elf \
        --no-default-features \
        --features "$BUILD_FEAT" \
        --bin esp32s3 \
        -Zbuild-std=core)

    echo "=== Generating merged flash image for $feat ==="
    espflash save-image \
        --chip esp32s3 \
        --ignore-app-descriptor \
        --merge \
        /app/esp32s3/target/xtensa-esp32s3-none-elf/release/esp32s3 \
        /app/_site/firmware-${feat}.bin

    python3 /app/scripts/patch_esp32s3.py /app/_site/firmware-${feat}.bin /app/_site/firmware-${feat}.bin --offset 0x10000
    python3 /app/scripts/trim-binary.py /app/_site/firmware-${feat}.bin
done

# ── RP2040 ───────────────────────────────────────────────────────────────────
echo "=== Building RP2040 variants ==="
for feat in all serial midi; do
    BUILD_FEAT="rp2040"
    [ "$feat" = "all" ] \
        && BUILD_FEAT="$BUILD_FEAT,serial,midi" \
        || BUILD_FEAT="$BUILD_FEAT,$feat"

    echo "=== Building RP2040 / $feat ==="
    (cd /app/rp && cargo +nightly build --release \
        --target thumbv6m-none-eabi \
        --no-default-features \
        --features "$BUILD_FEAT" \
        --bin rp2040)

    echo "=== Converting to UF2 for $feat ==="
    elf2uf2-rs \
        /app/rp/target/thumbv6m-none-eabi/release/rp2040 \
        /app/_site/firmware-rp2040-${feat}.uf2
done

# ── RP2350 ───────────────────────────────────────────────────────────────────
# Pico 2 BOOTSEL rejects UF2s with the RP2040 family ID; patch_uf2_family.py
# rewrites every block to RP2350-ARM-S (default) after elf2uf2-rs.

echo "=== Building RP2350 variants ==="
for feat in all serial midi; do
    BUILD_FEAT="rp2350"
    [ "$feat" = "all" ] \
        && BUILD_FEAT="$BUILD_FEAT,serial,midi" \
        || BUILD_FEAT="$BUILD_FEAT,$feat"

    echo "=== Building RP2350 / $feat ==="
    (cd /app/rp && cargo +nightly build --release \
        --target thumbv8m.main-none-eabihf \
        --no-default-features \
        --features "$BUILD_FEAT" \
        --bin rp2350)

    echo "=== Converting to UF2 for $feat ==="
    elf2uf2-rs \
        /app/rp/target/thumbv8m.main-none-eabihf/release/rp2350 \
        /app/_site/firmware-rp2350-${feat}.uf2
    python3 /app/scripts/patch_uf2_family.py \
        /app/_site/firmware-rp2350-${feat}.uf2
done

echo "=== Done ==="
ls -lh /app/_site/firmware-*.bin /app/_site/firmware-*.uf2

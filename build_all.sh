#!/bin/bash
set -euo pipefail

# Load ESP environment variables
[ -f /home/esp/export-esp.sh ] && source /home/esp/export-esp.sh

TARGETS=("thumbv6m-none-eabi" "xtensa-esp32s3-none-elf")
FEATURES=("all" "keyboard" "gamepad" "serial" "midi")

for target in "${TARGETS[@]}"; do
  for feat in "${FEATURES[@]}"; do
    echo "----------------------------------------------------------------"
    echo "Building for $target with feature set $feat..."
    echo "----------------------------------------------------------------"
    
    if [ "$target" == "thumbv6m-none-eabi" ]; then
      ARCH_FEAT="rp2040"
      BIN="rp2040"
      CARGO_CMD="cargo +nightly"
      STD_ARG=""
    else
      ARCH_FEAT="esp32s3"
      BIN="esp32s3"
      CARGO_CMD="cargo +esp"
      STD_ARG="-Zbuild-std=core"
    fi
    
    BUILD_FEAT="$ARCH_FEAT,defmt"
    if [ "$feat" == "all" ]; then
      BUILD_FEAT="$BUILD_FEAT,keyboard,gamepad,serial,midi"
    else
      BUILD_FEAT="$BUILD_FEAT,$feat"
    fi
    
    $CARGO_CMD build --release --target "$target" --no-default-features --features "$BUILD_FEAT" --bin "$BIN" $STD_ARG
  done
done
echo "All builds completed successfully!"

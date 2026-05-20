#!/bin/bash
set -euo pipefail

# Build every feature combination for both crates.
# Each crate has its own deps, lockfile, and .cargo/config.toml — they
# are independent and must be built from inside their own directory.

[ -f /home/esp/export-esp.sh ] && source /home/esp/export-esp.sh

REPO_ROOT="$(cd "$(dirname "$0")" && pwd)"
FEATURES=("all" "serial" "midi")

build_rp() {
  local arch="$1" target="$2" bin="$3"
  for feat in "${FEATURES[@]}"; do
    local build_feat="$arch"
    [ "$feat" = "all" ] \
      && build_feat="$build_feat,serial,midi" \
      || build_feat="$build_feat,$feat"

    echo "----------------------------------------------------------------"
    echo "Building rp/ ($bin) features=$build_feat"
    echo "----------------------------------------------------------------"
    (cd "$REPO_ROOT/rp" && \
      cargo +nightly build --release \
        --target "$target" \
        --no-default-features \
        --features "$build_feat" \
        --bin "$bin")
  done
}

build_esp32s3() {
  for feat in "${FEATURES[@]}"; do
    local build_feat
    [ "$feat" = "all" ] \
      && build_feat="serial,midi" \
      || build_feat="$feat"

    echo "----------------------------------------------------------------"
    echo "Building esp32s3/ features=$build_feat"
    echo "----------------------------------------------------------------"
    (cd "$REPO_ROOT/esp32s3" && \
      cargo +esp build --release \
        --target xtensa-esp32s3-none-elf \
        --no-default-features \
        --features "$build_feat" \
        --bin esp32s3 \
        -Zbuild-std=core)
  done
}

build_rp rp2040 thumbv6m-none-eabi rp2040
build_rp rp2350 thumbv8m.main-none-eabihf rp2350
build_esp32s3

echo "All builds completed successfully!"

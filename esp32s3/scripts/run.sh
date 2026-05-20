#!/usr/bin/env bash
# Cargo runner for the esp32s3 crate.
#
# `espflash flash` alone does not produce a bootable image on chips that
# have a stock ESP-IDF v5 bootloader on flash, because esp-hal 0.23 does
# not place `esp_app_desc_t` at the DROM offset (app+0x20) the bootloader
# reads — the bytes it sees as `min/max_efuse_blk_rev_full` are random
# code, the chip rev check fails, and the chip boot-loops back into
# download mode.
#
# This script wraps the canonical recipe:
#   1. save-image --merge       (build a flash-ready image at offset 0)
#   2. patch_esp32s3.py         (rewrite efuse_blk_rev fields + checksum)
#   3. espflash write-bin 0x0   (write the patched image)
#   4. espflash monitor         (stream defmt logs; only if stdin is a TTY)
#
# Invoked by `cargo run` via runner = "scripts/run.sh" in .cargo/config.toml.
# `$1` is the path to the freshly built ELF (always relative to crate root).
set -euo pipefail

ELF="$1"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CRATE_DIR="$(dirname "$SCRIPT_DIR")"
PATCH_PY="$CRATE_DIR/../scripts/patch_esp32s3.py"
IMG="$(dirname "$ELF")/$(basename "$ELF").flash.bin"

[ -f "$PATCH_PY" ] || { echo "missing: $PATCH_PY" >&2; exit 1; }

# Set ESPFLASH_PORT to skip espflash's interactive port picker.
PORT_ARG=()
[ -n "${ESPFLASH_PORT:-}" ] && PORT_ARG=(--port "$ESPFLASH_PORT")

echo "==> save-image --merge"
espflash save-image \
    --chip esp32s3 \
    --ignore-app-descriptor \
    --merge \
    "$ELF" \
    "$IMG"

echo "==> patch_esp32s3.py"
python3 "$PATCH_PY" "$IMG" "$IMG" --offset 0x10000

echo "==> write-bin 0x0"
espflash write-bin "${PORT_ARG[@]}" 0x0 "$IMG"

if [ -t 0 ]; then
    echo "==> reset + monitor"
    exec espflash monitor "${PORT_ARG[@]}" --elf "$ELF"
else
    echo "==> reset (no TTY — skipping monitor)"
    exec espflash reset "${PORT_ARG[@]}"
fi

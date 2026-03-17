#!/usr/bin/env python3
"""Patch an ESP32-S3 app binary so the IDF 5.x bootloader accepts it.

The IDF 5.x bootloader reads esp_app_desc_t from the start of the DROM
segment (flash offset 0x10020 = binary file offset 0x0020).  When building
with esp-hal the struct is not placed there by default, so the bootloader
reads stale code bytes and may see a bogus min_efuse_blk_rev_full value
(e.g. 0x29D0 = v107.4) which causes it to reject the image with:

    Image requires efuse blk rev >= v107.4, but chip is v1.3

This script:
  1. Sets min_efuse_blk_rev_full = 0  (accept any chip revision)
  2. Sets max_efuse_blk_rev_full = 0xFFFF  (no upper limit)
  3. Recalculates the image checksum byte
  4. Recalculates the SHA-256 hash (when hash_appended = 1)

Usage:
    python3 scripts/patch_esp32s3.py INPUT.bin OUTPUT.bin

The output file is a drop-in replacement ready for espflash write-flash.
"""

import hashlib
import sys


def patch(input_path: str, output_path: str) -> None:
    with open(input_path, "rb") as f:
        data = bytearray(f.read())

    # ── Locate esp_app_desc_t ──────────────────────────────────────────────
    # App image layout (ESP image format v1):
    #   0x00  8 bytes  basic header  (magic, seg_count, flash_mode, ...)
    #   0x08  16 bytes extended header (wp_pin, clk_drv, chip_id, ...,
    #                                   hash_appended)
    #   0x18  first segment header (load_addr, size)
    #   0x20  first segment data  ← esp_app_desc_t starts here
    #
    # esp_app_desc_t field offsets (IDF 5.x):
    #   0x00  magic             u32  (must be 0xABCD5432)
    #   0x04  secure_version    u32
    #   0x08  reserv1           u32[2]
    #   0x10  version           u8[32]
    #   0x30  project_name      u8[32]
    #   0x50  time              u8[16]
    #   0x60  date              u8[16]
    #   0x70  idf_ver           u8[32]
    #   0x90  app_elf_sha256    u8[32]
    #   0xB0  min_efuse_blk_rev_full  u16  ← patch target
    #   0xB2  max_efuse_blk_rev_full  u16  ← patch target
    #   0xB4  reserv2           u32[19]
    #   (total 256 bytes)
    #
    # So the patch targets are at binary offsets 0x20 + 0xB0 = 0xD0 and 0xD2.

    APP_DESC_OFFSET = 0x20          # start of first segment data
    MIN_EFUSE_OFFSET = APP_DESC_OFFSET + 0xB0   # 0xD0
    MAX_EFUSE_OFFSET = APP_DESC_OFFSET + 0xB2   # 0xD2

    magic = int.from_bytes(data[APP_DESC_OFFSET:APP_DESC_OFFSET + 4], "little")
    if magic == 0xABCD5432:
        print(f"  esp_app_desc_t magic found at 0x{APP_DESC_OFFSET:04x} ✓")
    else:
        print(
            f"  Warning: expected magic 0xABCD5432 at 0x{APP_DESC_OFFSET:04x}, "
            f"got 0x{magic:08x}.  Patching anyway …"
        )

    old_min = int.from_bytes(data[MIN_EFUSE_OFFSET:MIN_EFUSE_OFFSET + 2], "little")
    old_max = int.from_bytes(data[MAX_EFUSE_OFFSET:MAX_EFUSE_OFFSET + 2], "little")
    print(f"  min_efuse_blk_rev_full: 0x{old_min:04x} → 0x0000")
    print(f"  max_efuse_blk_rev_full: 0x{old_max:04x} → 0xFFFF")

    data[MIN_EFUSE_OFFSET]     = 0x00
    data[MIN_EFUSE_OFFSET + 1] = 0x00
    data[MAX_EFUSE_OFFSET]     = 0xFF
    data[MAX_EFUSE_OFFSET + 1] = 0xFF

    # ── Locate checksum and (optional) SHA-256 ────────────────────────────
    # After all segment data:
    #   N padding bytes (all 0x00) to make (offset + N + 1) a multiple of 16
    #   1 byte checksum
    #   (if hash_appended) 32 bytes SHA-256

    segment_count = data[1]
    hash_appended = data[8 + 15]   # extended header byte 15

    offset = 8 + 16                 # skip basic + extended header
    for _ in range(segment_count):
        seg_size = int.from_bytes(data[offset + 4: offset + 8], "little")
        offset = offset + 8 + seg_size

    # Align so that (offset + padding + 1) % 16 == 0
    padding = (15 - (offset % 16)) % 16
    checksum_offset = offset + padding

    # ── Recalculate checksum ──────────────────────────────────────────────
    # XOR of every segment data byte, initial value 0xEF.
    checksum = 0xEF
    seg_offset = 8 + 16
    for _ in range(segment_count):
        seg_size = int.from_bytes(data[seg_offset + 4: seg_offset + 8], "little")
        seg_data_start = seg_offset + 8
        for b in data[seg_data_start: seg_data_start + seg_size]:
            checksum ^= b
        seg_offset = seg_data_start + seg_size

    old_checksum = data[checksum_offset]
    print(f"  checksum at 0x{checksum_offset:05x}: 0x{old_checksum:02x} → 0x{checksum:02x}")
    data[checksum_offset] = checksum

    # ── Recalculate SHA-256 (if present) ──────────────────────────────────
    if hash_appended:
        sha256_offset = checksum_offset + 1
        sha256 = hashlib.sha256(data[:sha256_offset]).digest()
        old_sha = data[sha256_offset: sha256_offset + 32].hex()
        print(f"  SHA-256 at 0x{sha256_offset:05x}: {old_sha[:16]}… → {sha256.hex()[:16]}…")
        data[sha256_offset: sha256_offset + 32] = sha256
    else:
        print("  hash_appended=0, skipping SHA-256 update")

    with open(output_path, "wb") as f:
        f.write(data)

    print(f"  Written {len(data)} bytes to {output_path}")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        print(f"Usage: {sys.argv[0]} INPUT.bin OUTPUT.bin", file=sys.stderr)
        sys.exit(1)
    print(f"Patching {sys.argv[1]} …")
    patch(sys.argv[1], sys.argv[2])
    print("Done.")

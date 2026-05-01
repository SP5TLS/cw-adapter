#!/usr/bin/env python3
"""Rewrite the UF2 family ID in every block of a UF2 file.

elf2uf2-rs 2.2.0 hardcodes the RP2040 family ID (0xe48bff56), so its output
is rejected by the RP2350 BOOTSEL drive. This script patches the family ID
to whatever value the caller specifies — for our ARM-Secure rp235x build
that's 0xe48bff59 (RP2350-ARM-S). Family IDs are documented at
https://github.com/raspberrypi/pico-feedback/blob/main/UF2_FAMILY_IDS.md.

UF2 layout (one 512-byte block per page):
    0x00  magic1     0x0A324655  "UF2\\n"
    0x04  magic2     0x9E5D5157
    0x08  flags
    0x0C  targetAddr
    0x10  payloadSize
    0x14  blockNo
    0x18  numBlocks
    0x1C  familyID   <-- rewritten in place
    0x20..0x1FB  payload (≤476 bytes)
    0x1FC  magicEnd  0x0AB16F30
"""
import struct
import sys

BLOCK = 512
FAMILY_OFFSET = 28
MAGIC1 = 0x0A324655
MAGIC2 = 0x9E5D5157
MAGIC_END = 0x0AB16F30


def main() -> int:
    if len(sys.argv) != 3:
        print(f"usage: {sys.argv[0]} <uf2-file> <family-id-hex>", file=sys.stderr)
        return 2
    path = sys.argv[1]
    family = int(sys.argv[2], 0)

    with open(path, "rb") as f:
        data = bytearray(f.read())

    if len(data) % BLOCK != 0 or len(data) == 0:
        print(f"error: {path} is not a multiple of {BLOCK} bytes", file=sys.stderr)
        return 1

    for off in range(0, len(data), BLOCK):
        m1, m2 = struct.unpack_from("<II", data, off)
        end = struct.unpack_from("<I", data, off + BLOCK - 4)[0]
        if m1 != MAGIC1 or m2 != MAGIC2 or end != MAGIC_END:
            print(f"error: bad UF2 magic at offset {off:#x}", file=sys.stderr)
            return 1
        struct.pack_into("<I", data, off + FAMILY_OFFSET, family)

    with open(path, "wb") as f:
        f.write(data)

    print(f"patched {len(data) // BLOCK} blocks in {path} -> family {family:#010x}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

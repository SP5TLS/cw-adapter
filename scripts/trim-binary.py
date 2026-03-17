#!/usr/bin/env python3
"""Trim trailing 0xFF padding from a flash image and align to 4 KB."""
import os
import sys


def trim(path):
    data = open(path, "rb").read()

    i = len(data)
    while i > 0 and data[i - 1] == 0xFF:
        i -= 1
    i = (i + 0xFFF) & ~0xFFF  # align up to 4 KB

    tmp = path + ".tmp"
    try:
        open(tmp, "wb").write(data[:i])
        os.replace(tmp, path)
    except Exception:
        if os.path.exists(tmp):
            os.unlink(tmp)
        raise

    print(f"  {path}: {len(data) // 1024} KB → {i // 1024} KB")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        print(f"Usage: {sys.argv[0]} <firmware.bin>", file=sys.stderr)
        sys.exit(1)
    trim(sys.argv[1])

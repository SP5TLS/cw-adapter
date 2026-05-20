#!/usr/bin/env python3
"""Detect unintended drift between the rp/ and esp32s3/ copies of shared sources.

cdc_serial_state.rs, common.rs, and midi_interrupt.rs exist in both crates
because the two pin different embassy-usb / usbd-hid versions and a unified
workspace would re-trigger the `links = "embassy-time-queue"` collision the
crate split was meant to resolve.

That's a maintenance hazard: every USB fix has to be made twice. This script
runs in CI, normalises the *known* API divergences between embassy-usb 0.6
(rp) and 0.4 (esp32s3), and fails if anything else has drifted.

Allowed normalisations:
  • `endpoint_interrupt_in(None, …)` ↔ `endpoint_interrupt_in(…)`
  • `endpoint_interrupt_out(None, …)` ↔ `endpoint_interrupt_out(…)`
  • `endpoint_bulk_in(None, …)`       ↔ `endpoint_bulk_in(…)`
  • `endpoint_bulk_out(None, …)`      ↔ `endpoint_bulk_out(…)`
  • Trailing whitespace, blank-line collapse, any-line cosmetic re-flow are
    handled by collapsing all runs of whitespace.

If you need a new exception, add it to NORMALISERS below with a comment that
points at the embassy-usb / usbd-hid change that motivated it.
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
PAIRS = [
    ("rp/src/cdc_serial_state.rs", "esp32s3/src/cdc_serial_state.rs"),
    ("rp/src/common.rs",           "esp32s3/src/common.rs"),
    ("rp/src/midi_interrupt.rs",   "esp32s3/src/midi_interrupt.rs"),
]

# Each tuple is (pattern, replacement). Applied to BOTH files before
# comparing — patterns strip the embassy-usb 0.6 "scheduling slot" arg that
# isn't present in 0.4.
NORMALISERS: list[tuple[re.Pattern[str], str]] = [
    (re.compile(r"endpoint_interrupt_in\(None,\s*"),  "endpoint_interrupt_in("),
    (re.compile(r"endpoint_interrupt_out\(None,\s*"), "endpoint_interrupt_out("),
    (re.compile(r"endpoint_bulk_in\(None,\s*"),       "endpoint_bulk_in("),
    (re.compile(r"endpoint_bulk_out\(None,\s*"),      "endpoint_bulk_out("),
]


def normalise(text: str) -> str:
    for pat, rep in NORMALISERS:
        text = pat.sub(rep, text)
    # Collapse any run of whitespace (incl. newlines) to a single space so
    # rustfmt re-flow doesn't trigger false positives.
    return re.sub(r"\s+", " ", text).strip()


def main() -> int:
    bad = 0
    for a, b in PAIRS:
        ap, bp = REPO / a, REPO / b
        if not ap.exists() or not bp.exists():
            print(f"missing: {ap if not ap.exists() else bp}", file=sys.stderr)
            bad += 1
            continue
        na = normalise(ap.read_text())
        nb = normalise(bp.read_text())
        if na != nb:
            bad += 1
            print(f"DRIFT: {a} vs {b}", file=sys.stderr)
            # Cheap diff hint: dump the first differing chunk.
            for i, (ca, cb) in enumerate(zip(na, nb)):
                if ca != cb:
                    lo = max(0, i - 60)
                    hi = i + 60
                    print(f"  near char {i}:", file=sys.stderr)
                    print(f"    rp:       …{na[lo:hi]!r}…", file=sys.stderr)
                    print(f"    esp32s3:  …{nb[lo:hi]!r}…", file=sys.stderr)
                    break
            else:
                # One is a strict prefix of the other.
                print(
                    f"  length mismatch: rp={len(na)} esp32s3={len(nb)}",
                    file=sys.stderr,
                )
    if bad:
        print(
            f"\n{bad} shared source pair(s) drifted. Make the change in both "
            f"crates, or add a NORMALISER in scripts/check_shared_sources.py.",
            file=sys.stderr,
        )
        return 1
    print("shared sources OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())

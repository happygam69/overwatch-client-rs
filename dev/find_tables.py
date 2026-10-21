"""Find large arrays of code pointers (jump/dispatch tables) in the module image.

The client switches on an asset's *type* when it loads it, and the type is a
12-bit field, so the switch is a table of a few thousand entries. A run of that
many consecutive values that all point into the image cannot occur by chance, so
the run's *index* is the type number: entry 0x006 of such a table is the
animation chunk's loader, which is the function this hunt is after.

Two shapes are reported: absolute virtual addresses, and 32-bit relative offsets
(the compiler can emit either).

Usage:
    python find_tables.py [--min-run N] [--image PATH] [--base 0x...]

Read-only.
"""

from __future__ import annotations

import sys

import numpy as np

IMAGE = "D:/ow-dumps/module_v2full.bin"
BASE = 0x7FF7C4D20000


def runs(mask: np.ndarray, min_run: int) -> list[tuple[int, int]]:
    """(start, length) of every True run of at least `min_run` in `mask`."""
    idx = np.flatnonzero(mask)
    if idx.size == 0:
        return []
    breaks = np.flatnonzero(np.diff(idx) > 1)
    starts = np.concatenate(([0], breaks + 1))
    ends = np.concatenate((breaks, [idx.size - 1]))
    out = []
    for s, e in zip(starts, ends):
        length = e - s + 1
        if length >= min_run:
            out.append((int(idx[s]), int(length)))
    return out


def main() -> int:
    args = sys.argv[1:]
    image = args[args.index("--image") + 1] if "--image" in args else IMAGE
    base = int(args[args.index("--base") + 1], 16) if "--base" in args else BASE
    min_run = int(args[args.index("--min-run") + 1]) if "--min-run" in args else 128

    raw = np.fromfile(image, dtype=np.uint8)
    size = len(raw)
    print(f"image {size} B, base 0x{base:x}, span 0x{base:x}..0x{base + size:x}")

    # Absolute addresses: value in [base, base+size).
    vals = np.frombuffer(raw, dtype="<u4")
    n = len(vals)
    lo, hi = base, base + size
    abs_mask = (vals >= lo) & (vals < hi)
    for start, length in runs(abs_mask, min_run):
        print(f"\nTEXT-pointer run: {length} entries at file 0x{start * 4:x} (v2b 0x{base + start * 4:x})")
        table = vals[start : start + length]
        for i in (0, 1, 6, 0x0C, 0x1B, length - 1):
            if i < length:
                print(f"    entry[{i:#06x}] = 0x{table[i]:x}")

    # Relative offsets: value < size, and the run must be long, so noise dies.
    rel_mask = vals < size
    for start, length in runs(rel_mask, max(min_run * 4, 1024)):
        print(f"\nRVA-like run: {length} entries at file 0x{start * 4:x} (v2b 0x{base + start * 4:x})")
        table = vals[start : start + length]
        for i in (0, 1, 6, 0x0C):
            if i < length:
                print(f"    entry[{i:#06x}] = 0x{table[i]:x}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

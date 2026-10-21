"""Find RIP-relative references to addresses inside the flattened module image.

`find_refs.py` scans captured *regions* (many small blobs at their own bases).
The flattened image (`module_v2full.bin`) is one file mapped at one base, so the
same arithmetic applies to file offsets: for a 4-byte little-endian window `v`
at file offset `o`, an instruction ending at `o + 4` references
`base + o + 4 + v`.

Usage:
    python refs_image.py <target_hex> [target_hex ...] [--image PATH] [--base 0x...]

Prints every hit as `target -> file offset (= v2b address)` with a little context
so the instruction can be looked up in Ghidra.  Read-only.
"""

from __future__ import annotations

import sys

import numpy as np

IMAGE = "D:/ow-dumps/module_v2full.bin"
BASE = 0x7FF7C4D20000


def scan(image: str, base: int, targets: set[int]) -> dict[int, list[int]]:
    data = np.fromfile(image, dtype=np.uint8)
    n = len(data)
    hits: dict[int, list[int]] = {t: [] for t in targets}
    for shift in range(4):
        v = np.frombuffer(data[shift:], dtype="<u4", count=(n - shift) // 4)
        offs = base + shift + 4 + np.arange(len(v), dtype=np.uint64) * 4
        cand = offs + v.astype(np.uint64)
        for t in targets:
            for i in np.nonzero(cand == t)[0]:
                hits[t].append(int(offs[i] - 4 - base))
    return hits


def main() -> int:
    args = sys.argv[1:]
    targets = set()
    for a in args:
        if a.startswith("0x") or a.startswith("0X"):
            try:
                targets.add(int(a, 16))
            except ValueError:
                pass
    if not targets:
        print(__doc__)
        return 2
    image = args[args.index("--image") + 1] if "--image" in args else IMAGE
    base = int(args[args.index("--base") + 1], 16) if "--base" in args else BASE

    hits = scan(image, base, targets)
    for t in sorted(targets):
        print(f"=== 0x{t:016x}  {len(hits[t])} reference(s)")
        for off in sorted(hits[t]):
            print(f"    insn end 0x{base + off:016x}   (file 0x{off:x})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

"""Flatten captured code regions into one image Ghidra can import.

Ghidra wants a single binary with one base address. The capture is thousands of
separate regions, so this lays them into one buffer at their real offsets and
zero-fills the gaps. Importing at the recorded base keeps every RIP-relative
reference resolving, which is the whole point: a disassembly at the wrong
address has plausible instructions and meaningless operands.

    python flatten.py <dumpdir> <lo> <hi> <out.bin>
"""

from __future__ import annotations

import sys

import regions as R


def main() -> int:
    dump = sys.argv[1]
    lo = int(sys.argv[2], 16)
    hi = int(sys.argv[3], 16)
    out_path = sys.argv[4]

    space = R.Space(R.load(dump))
    span = hi - lo
    image = bytearray(span)
    covered = 0
    for region in space.regions:
        if region.base < lo or region.end > hi:
            continue
        start = region.base - lo
        data = region.data()
        image[start : start + len(data)] = data
        covered += len(data)

    with open(out_path, "wb") as fh:
        fh.write(image)

    print(
        f"{out_path}: {span / 1048576:.1f} MB span, {covered / 1048576:.1f} MB of code, "
        f"base {lo:#x}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

"""Find code that reads the animation clip header, by the loads it must make.

The clip chunk (asset type 0x006) stores a u16 frame count at `+0x0c` and a u16
fps at `+0x0e` (see `docs/formats/animation.md`), so whatever decodes it must load
those two words from the same object within a few instructions. That pair is a
much tighter fingerprint than the asset type number, which never appears as an
immediate near the reader.

Scans the flattened image for `movzx r32, word ptr [reg + 0x0c]` and
`mov r32, dword ptr [reg + 0x0c]`, then looks for a sibling load with displacement
0x0e (and optionally a float/dword load at 0x14) nearby.

Usage:
    python find_anim_decoder.py [--image PATH] [--base 0x...] [--window N]

Read-only.
"""

from __future__ import annotations

import struct
import sys

import numpy as np

IMAGE = "D:/ow-dumps/module_v2full.bin"
BASE = 0x7FF7C4D20000


def hits(raw: bytes) -> list[tuple[int, str]]:
    """Offsets of plausible 0x0c-displacement loads, with the form seen."""
    out = []
    # movzx r32, word ptr [r64 + disp8]: 0F B7 4r dd   (dd = 0x0c)
    for i in range(len(raw) - 4):
        b0, b1 = raw[i], raw[i + 1]
        if b0 == 0x0F and b1 == 0xB7:
            modrm = raw[i + 2]
            if (modrm & 0xC0) == 0x40 and (modrm & 0x07) != 4 and raw[i + 3] == 0x0C:
                out.append((i, "movzx word"))
            continue
        # REX-prefixed movzx (r8..r15): 44/45 OF B7 4r dd
        if b0 in (0x44, 0x45) and b1 == 0x0F and raw[i + 2] == 0xB7:
            modrm = raw[i + 3]
            if (modrm & 0xC0) == 0x40 and (modrm & 0x07) != 4 and raw[i + 4] == 0x0C:
                out.append((i, "movzx word (rex)"))
            continue
        # mov r32, dword ptr [r64 + disp8]: 8B 4r dd
        if b0 == 0x8B and (b1 & 0xC0) == 0x40 and (b1 & 0x07) != 4 and raw[i + 2] == 0x0C:
            out.append((i, "mov dword"))
    return out


def has_sibling(raw: bytes, at: int, disp: int, window: int) -> bool:
    """A load with `disp` in [at - window, at + window]."""
    lo, hi = max(0, at - window), min(len(raw) - 4, at + window)
    for i in range(lo, hi):
        for pat, need in ((b"\x0f\xb7", 4), (b"\x8b", 3)):
            if raw[i : i + len(pat)] != pat:
                continue
            base = i + len(pat)
            if base + 2 > len(raw):
                continue
            modrm = raw[base]
            if (modrm & 0xC0) == 0x40 and (modrm & 0x07) != 4 and raw[base + 1] == disp:
                return True
    return False


def main() -> int:
    args = sys.argv[1:]
    image = args[args.index("--image") + 1] if "--image" in args else IMAGE
    base = int(args[args.index("--base") + 1], 16) if "--base" in args else BASE
    window = int(args[args.index("--window") + 1]) if "--window" in args else 0x40

    raw = open(image, "rb").read()
    found = hits(raw)
    print(f"{len(found)} candidate 0x0c-loads in {len(raw)} bytes")

    # Cluster: a decoder reads 0x0c and 0x0e close together, and maybe 0x14.
    clusters = []
    for at, kind in found:
        if not has_sibling(raw, at, 0x0E, window):
            continue
        third = has_sibling(raw, at, 0x14, window)
        clusters.append((at, kind, third))

    # Merge clusters within 0x200 of each other: one function, one site.
    merged: list[tuple[int, int, int]] = []
    for at, _kind, third in clusters:
        if merged and at - merged[-1][0] < 0x200:
            prev = merged[-1]
            merged[-1] = (prev[0], prev[1] + 1, prev[2] + (1 if third else 0))
        else:
            merged.append((at, 1, 1 if third else 0))
    merged.sort(key=lambda t: -t[1])
    print(f"{len(clusters)} sites also load 0x0e within {window} bytes; {len(merged)} clusters")
    for at, count, thirds in merged[:25]:
        print(
            f"   v2b 0x{base + at:x}  file 0x{at:x}  {count} 0x0c-load(s), "
            f"{thirds} with a 0x14 load nearby"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

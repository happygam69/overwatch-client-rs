"""Find decoded map chunk data in a memory capture.

A map chunk on disk is BLTE-compressed, so its bytes are not searchable. Once
the game loads one, the decoded bytes sit in memory, and they start with a
header that is recognisable: a placeable count, an instance-data offset, a
placeable offset that is always 16, and a zero.

The pattern alone is weak, so every candidate is *parsed*: the placeables must
walk cleanly by their declared sizes, and the model references in them must
carry the model type. That is what makes a hit a hit rather than a coincidence.

    python find_mapchunks.py [dumpdir]
"""

from __future__ import annotations

import sys

import regions as R

MARKER = bytes([0x10, 0, 0, 0, 0, 0, 0, 0])
COMMON = 24
MODEL_TYPE = 0x00C


def reverse12(value: int) -> int:
    out = 0
    for i in range(12):
        if value & (1 << i):
            out |= 1 << (11 - i)
    return out


def type_of(guid: int) -> int:
    return reverse12((guid >> 48) & 0xFFF) + 1


def u32(d: bytes, at: int) -> int:
    return int.from_bytes(d[at : at + 4], "little")


def looks_like_map_chunk(d: bytes, at: int) -> tuple[int, int] | None:
    """Return (placeable count, span) when `at` starts a readable map chunk."""
    if at + 16 > len(d):
        return None
    count = u32(d, at)
    instance_offset = u32(d, at + 4)
    if not (1 <= count <= 200_000):
        return None
    if instance_offset < count * COMMON:
        return None

    cursor = at + 16
    models = 0
    for _ in range(count):
        if cursor + COMMON > len(d):
            return None
        size = u32(d, cursor + 20)
        if size < COMMON or size > 0x400_0000 or cursor + size > len(d):
            return None
        model = u32(d, cursor + COMMON) | (u32(d, cursor + COMMON + 4) << 32)
        if type_of(model) == MODEL_TYPE:
            models += 1
        cursor += size
    # A map chunk's placeables are mostly models; a coincidence will not be.
    if models * 2 < count:
        return None
    return count, cursor - at


def main() -> int:
    dump = sys.argv[1] if len(sys.argv) > 1 else "dumps_mapped"
    space = R.Space(R.load(dump))
    print(f"{dump}: {len(space.regions)} regions")

    found = []
    scanned = 0
    for region in space.regions:
        data = region.data()
        scanned += len(data)
        at = 0
        while True:
            at = data.find(MARKER, at)
            if at < 0:
                break
            start = at - 8
            if start >= 0:
                hit = looks_like_map_chunk(data, start)
                if hit:
                    count, span = hit
                    found.append((region.base + start, count, span, region.path))
            at += 1

    print(f"scanned {scanned / 1048576:.0f} MB; {len(found)} map chunk(s)")
    for address, count, span, path in found[:40]:
        print(f"  {address:#014x}  {count:>6} placeables  {span:>9} bytes")
    if found:
        total = sum(f[1] for f in found)
        print(f"{total} placeables across the chunks found")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

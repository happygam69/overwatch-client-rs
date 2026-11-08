"""Where do `UNW_FLAG_CHAININFO` fragments live, and how far from their parent?

    python dev/chain_placement.py

Answers the terminology question the docs have been sloppy about: a chained
`.pdata` entry is *not* a separate cold section or page - it is another
`RUNTIME_FUNCTION` in the same `.text`, linked to its parent through a chained
UNWIND_INFO record. This prints, from the install's own bytes:

  * which sections contain chained entries (expect `.text` only);
  * how many parents have all their fragments immediately after the hot part;
  * the distance distribution parent.begin -> fragment.begin.

Reads the install in place; writes nothing.
"""

from __future__ import annotations

import struct
import sys

sys.path.insert(0, str(__import__("pathlib").Path(__file__).parent))
from pe_layout import INSTALL, BASE, sections  # noqa: E402


def u32(b: bytes, i: int) -> int:
    return struct.unpack_from("<I", b, i)[0]


def main() -> int:
    image = INSTALL.read_bytes()
    pe = u32(image, 0x3C)
    opt = struct.unpack_from("<H", image, pe + 20)[0]
    s0 = pe + 24 + opt
    secs = sections(image)
    pdata = next(s for s in secs if s["name"] == ".pdata")
    pd = image[pdata["raw"]: pdata["raw"] + pdata["rsz"]]

    def off(rva: int) -> int:
        """RVA -> file offset. Reading the image at an RVA is wrong: the sections
        are not identity-mapped, so every UNWIND_INFO read needs this."""
        for s in secs:
            if s["va"] <= rva < s["raw_end"]:
                return s["raw"] + (rva - s["va"])
        raise KeyError(f"rva 0x{rva:x} is not raw-backed")

    def section_of(rva: int) -> str:
        for s in secs:
            if s["va"] <= rva < s["virt_end"]:
                return s["name"]
        return "<none>"

    # Every RUNTIME_FUNCTION: begin, end, unwind rva, chained flag.
    entries: dict[int, tuple[int, int, int, bool]] = {}
    for i in range(0, len(pd) - 11, 12):
        beg, end, unw = u32(pd, i), u32(pd, i + 4), u32(pd, i + 8)
        if beg == 0 or end <= beg:
            continue
        flags = (image[off(unw)] >> 3) & 0x1F
        entries[beg] = (beg, end, unw, bool(flags & 4))

    # Resolve each chained entry to its transitive root.
    def root_of(rva: int) -> tuple[int, int]:
        cur, depth = rva, 0
        while True:
            e = entries.get(cur)
            if e is None or not e[3]:
                return cur, depth
            count = image[off(e[2]) + 2]
            skip = 4 + 2 * count + (2 if count & 1 else 0)
            cur, depth = u32(image, off(e[2]) + skip), depth + 1

    pieces: dict[int, list[int]] = {}
    for rva, (_, _, _, chained) in entries.items():
        if not chained:
            continue
        root, _ = root_of(rva)
        pieces.setdefault(root, []).append(rva)

    chained_secs: dict[str, int] = {}
    for rva in entries:
        if entries[rva][3]:
            chained_secs[section_of(rva)] = chained_secs.get(section_of(rva), 0) + 1
    root_secs: dict[str, int] = {}
    for rva in pieces:
        root_secs[section_of(rva)] = root_secs.get(section_of(rva), 0) + 1

    print(f"chained entries by section : {chained_secs}")
    print(f"parents      by section    : {root_secs}")

    adjacent = 0
    gap = 0
    dist: dict[int, int] = {}
    depths: dict[int, int] = {}
    for root, frags in pieces.items():
        end = entries[root][1]
        # Sort the chain by address and walk it: the next piece should start
        # where the previous one ends.
        ordered = sorted(frags + [root])
        ok = all(entries[ordered[i]][0] == entries[ordered[i - 1]][1]
                 for i in range(1, len(ordered)))
        if ok:
            adjacent += 1
        else:
            gap += 1
        for f in frags:
            delta = entries[f][0] - root
            bucket = 0 if delta == 0 else 1 << (delta.bit_length() - 1)
            dist[bucket] = dist.get(bucket, 0) + 1
            _, depth = root_of(f)
            depths[depth] = depths.get(depth, 0) + 1

    print(f"parents with every piece adjacent to the previous : {adjacent}")
    print(f"parents with a gap between pieces                : {gap}")
    print("fragment distance from parent (power-of-two buckets):")
    for k in sorted(dist):
        print(f"  >= 0x{k:x} bytes : {dist[k]}")
    print("chain depth (parent hops):", dict(sorted(depths.items())))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

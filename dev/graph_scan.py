"""Find graph-container objects in a heap capture by the deserializer's shape.

The materializer (FUN_7ff7c6f7e310 / FUN_7ff7c6ff6880) leaves three structures
on the graph object:
    G+0x1c8  key -> node hash map   (buckets/entries 0xC stride/values/mask)
    G+0x1d0  key-order u32 vector   (array, count, capacity)
    G+0x290  dispatch table holder  S, with [S+0x60] a pair of pointers and
             [S+0x68] the mode (1 selects pair[0], else pair[1]); the selected
             pointer is an array of qwords indexed by node+0x1a8.

Scan heap regions for an object that has all three, then print the keys, the
decoded (type, section, index) fields, and the first table entries with their
address class (module code / heap / other).

Usage:
    python graph_scan.py <heap.bin> <base-hex> [--limit N]
    python graph_scan.py --dir dumps_v2b [--limit N]
"""

from __future__ import annotations

import os
import struct
import sys

MOD_LO, MOD_HI = 0x7FF7C4D20000, 0x7FF7C9000000


class Image:
    def __init__(self, path: str, base: int):
        self.base = base
        with open(path, "rb") as fh:
            self.data = fh.read()
        self.end = base + len(self.data)

    def u64(self, addr: int) -> int | None:
        if not (self.base <= addr and addr + 8 <= self.end):
            return None
        return struct.unpack_from("<Q", self.data, addr - self.base)[0]

    def u32(self, addr: int) -> int | None:
        if not (self.base <= addr and addr + 4 <= self.end):
            return None
        return struct.unpack_from("<I", self.data, addr - self.base)[0]

    def i32(self, addr: int) -> int | None:
        if not (self.base <= addr and addr + 4 <= self.end):
            return None
        return struct.unpack_from("<i", self.data, addr - self.base)[0]

    def heap(self, addr: int) -> bool:
        return addr is not None and self.base <= addr < self.end


def klass(addr: int | None) -> str:
    if addr is None:
        return "none"
    if MOD_LO <= addr < MOD_HI:
        return "code"
    if addr > 0x100000000:
        return "heap"
    return "other"


def try_graph(img: Image, g: int) -> None:
    m = img.u64(g + 0x1C8)
    v = img.u64(g + 0x1D0)
    s = img.u64(g + 0x290)
    if not (img.heap(m) and img.heap(v) and img.heap(s)):
        return
    # vector shape
    arr = img.u64(v)
    cnt = img.i32(v + 8)
    cap = img.i32(v + 0xC)
    if not (img.heap(arr) and cnt is not None and cap is not None and 0 < cnt < 200 and cap >= cnt):
        return
    keys = [img.u32(arr + k * 4) for k in range(cnt)]
    if any(k is None for k in keys):
        return
    # key plausibility: byte1 small, index part small
    if not all((k >> 8 & 0xFF) < 0x40 and (k >> 16) < 0x10000 for k in keys):
        return
    # S shape
    pair = img.u64(s + 0x60)
    mode = img.u64(s + 0x68)
    if not img.heap(pair) or mode is None or mode > 1:
        return
    sel = pair if mode == 1 else pair + 8
    table = img.u64(sel)
    if not img.heap(table):
        return
    ents = [img.u64(table + k * 8) for k in range(8)]
    # at least one entry should be a module code pointer
    if not any(e is not None and MOD_LO <= e < MOD_HI for e in ents):
        return

    print(f"\nGRAPH CANDIDATE G={g:#x}")
    print(f"  map {m:#x}  keyvec {v:#x} (array {arr:#x} count {cnt} cap {cap})"
          f"  S {s:#x} pair {pair:#x} mode {mode}")
    print("  keys (hex):     " + " ".join(f"{k:08x}" for k in keys[:32]))
    print("  keys (x,b,c):   " + " ".join(
        f"({k & 0xff},{k >> 8 & 0xFF},{k >> 16})" for k in keys[:24]))
    print("  distinct low byte (type?): " + ",".join(
        str(t) for t in sorted({k & 0xFF for k in keys})))
    print("  table entries:  " + " ".join(f"{e and hex(e)}[{klass(e)}]" for e in ents))
    # map sanity: bucket count from mask
    mask = img.u64(m + 0x18)
    bkt = img.u64(m)
    print(f"  map buckets {bkt and hex(bkt)} mask {mask and hex(mask)}")


def scan_image(img: Image, limit: int) -> int:
    found = 0
    step = 8
    for off in range(0, len(img.data) - 8, step):
        q = struct.unpack_from("<Q", img.data, off)[0]
        # candidate graph pointers live in the heap image itself
        if not (img.base <= q < img.end):
            continue
        # cheap pre-filter: [q+0x290] must be a heap pointer
        if not img.heap(img.u64(q + 0x290)):
            continue
        before = found
        try_graph(img, q)
        found += 1 if found == before else 0
        if found >= limit:
            break
    return found


def main() -> int:
    limit = 4
    if "--limit" in sys.argv:
        limit = int(sys.argv[sys.argv.index("--limit") + 1])
    if "--dir" in sys.argv:
        d = sys.argv[sys.argv.index("--dir") + 1]
        root = d if os.path.isabs(d) else os.path.join("D:/ow-dumps", d)
        hits = 0
        names = sorted(os.listdir(root))
        for name in names:
            parts = name.split("_")
            if len(parts) < 2:
                continue
            try:
                base = int(parts[0], 16)
                size = int(parts[1], 16)
            except ValueError:
                continue
            if base < 0x100000000 or size < (1 << 20):
                continue
            img = Image(os.path.join(root, name), base)
            n = scan_image(img, limit - hits)
            if n:
                print(f"--- {name}: {n} candidate(s)")
                hits += n
            if hits >= limit:
                break
        print(f"\ntotal candidates: {hits}")
        return 0

    path, base = sys.argv[1], int(sys.argv[2], 16)
    img = Image(path, base)
    print(f"image {path} base {base:#x} size {len(img.data)/1048576:.0f} MB")
    n = scan_image(img, limit)
    print(f"\ncandidates: {n}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

"""Raw qword scan of an ETL for loader code addresses.

The ETL is not decoded; sampled-profile payloads and stack frames store the
RIP as a little-endian qword. This finds the loader base from the UTF-16
module-name string and then counts qwords whose RVA falls in the pad family.
"""
from __future__ import annotations

import argparse
import mmap
import struct
import sys

import numpy as np

NAME = "Overwatch_loader.dll".encode("utf-16-le")
REGIONS = [
    ("cand", 0x14380e0, 0x144ee19),
    ("padA", 0x10b72f0, 0x10bb994),
    ("padB", 0x10c1e20, 0x10d06c4),
    ("pad", 0x1d77f80, 0x1d7bf80),
]

def region(rva: int) -> str:
    for n, lo, hi in REGIONS:
        if lo <= rva < hi:
            return n
    return ""

def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("etl")
    ap.add_argument("--base", type=lambda s: int(s, 16), default=None)
    args = ap.parse_args()

    f = open(args.etl, "rb")
    m = mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ)
    print("size", len(m), flush=True)

    base = args.base
    if base is None:
        p = m.find(NAME)
        print("name hits", p >= 0)
        while p >= 0:
            lo = max(0, p - 96)
            hi = min(len(m), p + 96)
            chunk = m[lo:hi]
            vals = [struct.unpack_from("<Q", chunk, i)[0] for i in range(0, len(chunk) - 8, 8)]
            cand = [v for v in vals if 0x7FF000000000 <= v < 0x800000000000]
            print(f"  at {p:#x}: {[hex(v) for v in cand[:8]]}")
            p = m.find(NAME, p + 1)
        if base is None:
            print("pass --base if the base is not obvious")
            return 1
    print("base", hex(base), flush=True)

    CH = 64 * 1024 * 1024
    hits = {}
    total = 0
    for off in range(0, len(m), CH):
        b = m[off:off + CH]
        n = len(b) // 8
        v = np.frombuffer(b[:n * 8], dtype=np.uint64)
        sel = (v >= np.uint64(base)) & (v < np.uint64(base + 0x2410000))
        if not sel.any():
            continue
        for x in v[sel]:
            rva = int(x) - base
            r = region(rva)
            if r:
                hits[rva] = hits.get(rva, 0) + 1
                total += 1
        if (off // CH) % 8 == 0:
            print("  off", hex(off), "hits", total, flush=True)
    print("pad-family qword hits", total, "distinct", len(hits))
    for rva, n in sorted(hits.items(), key=lambda kv: -kv[1])[:80]:
        print(f"  {rva:#09x} {region(rva):<6} x{n}")
    return 0

if __name__ == "__main__":
    raise SystemExit(main())

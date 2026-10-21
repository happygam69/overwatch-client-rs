"""Chase statescript node chains in the warm live capture.

The warm session's module base is 0x7ff70f... and every module address in it
translates from the v2b session by -0xB6800000 (verified when the band base was
solved).  This scans the live heaps for element/host vtables, chases the
documented layout (E+0x38 executor, E+0x300/E+0x318 chain members, node base =
ptr-0x158, node+0x1a0 graph, node+0x1a8 type index) and dumps both the
per-graph dispatch table ([G+0x290] -> S -> pair[S+0x60] selected by S+0x68)
and the materializer's structures (G+0x1c8 key->node map, G+0x1d0 key-order
vector, G+8 section array).

Usage:
    python live_chain.py <capture-dir> [--limit N]
"""

from __future__ import annotations

import os
import struct
import sys

DELTA = 0xB6800000
V2B_ELEM_VT = 0x7FF7C7E40360
V2B_HOST_VT = 0x7FF7C7E37000
MOD_LO, MOD_HI = 0x7FF7C4D20000, 0x7FF7C9000000


class Cap:
    def __init__(self, root: str):
        self.regions = []
        for sub in ("module", "heap_0x24080000000", "heap_0xfda0000000", "lit"):
            d = os.path.join(root, sub)
            if not os.path.isdir(d):
                continue
            for name in os.listdir(d):
                parts = name[:-4].split("_")
                if len(parts) < 2:
                    continue
                try:
                    base = int(parts[0], 16)
                    size = int(parts[1], 16)
                except ValueError:
                    continue
                self.regions.append((base, size, os.path.join(d, name)))
        self.regions.sort()
        self._cache_addr = None
        self._cache_data = None

    def read(self, addr: int, length: int) -> bytes | None:
        if self._cache_data is not None and self._cache_addr <= addr and \
                addr + length <= self._cache_addr + len(self._cache_data):
            off = addr - self._cache_addr
            return self._cache_data[off:off + length]
        for base, size, path in self.regions:
            if base <= addr < base + size:
                with open(path, "rb") as fh:
                    fh.seek(addr - base)
                    data = fh.read(min(length, base + size - addr))
                self._cache_addr = addr
                self._cache_data = data
                return data if len(data) == length else data
        return None

    def u64(self, addr: int) -> int | None:
        b = self.read(addr, 8)
        return struct.unpack("<Q", b)[0] if b and len(b) == 8 else None

    def u32(self, addr: int) -> int | None:
        b = self.read(addr, 4)
        return struct.unpack("<I", b)[0] if b and len(b) == 4 else None

    def i32(self, addr: int) -> int | None:
        b = self.read(addr, 4)
        return struct.unpack("<i", b)[0] if b and len(b) == 4 else None

    def name_of(self, addr: int) -> str:
        for base, size, path in self.regions:
            if base <= addr < base + size:
                return os.path.basename(path)
        return "-"


def scan(cap: Cap, needle: int, limit: int) -> list[int]:
    hits = []
    pat = struct.pack("<Q", needle)
    for base, size, path in cap.regions:
        if MOD_LO <= base < MOD_HI:
            continue
        with open(path, "rb") as fh:
            data = fh.read()
        off = 0
        while True:
            i = data.find(pat, off)
            if i < 0:
                break
            if i % 8 == 0:
                hits.append(base + i)
                if len(hits) >= limit:
                    return hits
            off = i + 1
    return hits


def dump_graph(cap: Cap, g: int, idx: int) -> None:
    print(f"      graph G={g:#x} in {cap.name_of(g)}")
    s = cap.u64(g + 0x290)
    print(f"      [G+0x290] S = {s and hex(s)}")
    if s:
        pair = cap.u64(s + 0x60)
        mode = cap.u64(s + 0x68)
        print(f"        [S+0x60] pair = {pair and hex(pair)}  [S+0x68] mode = {mode}")
        if pair:
            sel = pair if mode == 1 else pair + 8
            arr = cap.u64(sel)
            print(f"        selected slot = {sel:#x} -> array {arr and hex(arr)}")
            if arr:
                for k in range(0, min(8, max(0, idx + 2))):
                    e = cap.u64(arr + k * 8)
                    print(f"          table[{k}] = {e and hex(e)}  ({cap.name_of(e) if e else '-'})")
    # materializer structures
    m = cap.u64(g + 0x1C8)
    v = cap.u64(g + 0x1D0)
    print(f"      [G+0x1c8] map = {m and hex(m)}   [G+0x1d0] keyvec = {v and hex(v)}")
    if v:
        arr = cap.u64(v)
        cnt = cap.i32(v + 8)
        cap_ = cap.i32(v + 0xC)
        print(f"        keyvec: array {arr and hex(arr)} count {cnt} cap {cap_}")
        if arr and cnt and 0 < cnt < 200:
            ks = []
            for k in range(cnt):
                ks.append(cap.u32(arr + k * 4))
            print("        keys: " + " ".join(f"{k:08x}" for k in ks[:48]))
            print("        key fields: " + " ".join(
                f"({k & 0xff},{k >> 8 & 0xff},{k >> 16})" for k in ks[:24]))
    if m:
        bucket = cap.u64(m)
        ent = cap.u64(m + 8)
        val = cap.u64(m + 16)
        mask = cap.u64(m + 24)
        print(f"        map: bucket {bucket and hex(bucket)} entries {ent and hex(ent)}"
              f" values {val and hex(val)} mask {mask and hex(mask)}")
        if ent:
            for k in range(6):
                e = cap.u32(ent + k * 0xC)
                key = cap.u32(ent + k * 0xC + 8)
                nxt = cap.u32(ent + k * 0xC + 4)
                if e != 0xFFFFFFFF or key:
                    print(f"          entry[{k}] = {e:08x} next {nxt:08x} key {key:08x}")


def main() -> int:
    root = sys.argv[1] if len(sys.argv) > 1 else "D:/ow-dumps/live_warm"
    limit = 8
    if "--limit" in sys.argv:
        limit = int(sys.argv[sys.argv.index("--limit") + 1])
    cap = Cap(root)

    # sanity: vtables exist at the translated addresses
    for v2b, nm in ((V2B_ELEM_VT, "element"), (V2B_HOST_VT, "host")):
        live = v2b - DELTA
        q = cap.u64(live)
        print(f"{nm} vtable {live:#x}: first slot {q and hex(q)} ({cap.name_of(q) if q else '-'})")

    for v2b, nm in ((V2B_ELEM_VT, "element"), (V2B_HOST_VT, "host")):
        hits = scan(cap, v2b - DELTA, limit)
        print(f"\n== {nm} vtable hits: {len(hits)}")
        for e in hits:
            print(f"  E={e:#x} in {cap.name_of(e)}")
            x = cap.u64(e + 0x38)
            arr = cnt = None
            if x:
                arr = cap.u64(x + 0x30)
                cnt = cap.i32(x + 0x38)
                cur = cap.u64(x + 0x430)
                print(f"    E+0x38 executor X={x:#x}  X+0x30 array={arr and hex(arr)}"
                      f" count={cnt} X+0x430 cur={cur and hex(cur)}")
            for off in (0x300, 0x318):
                p = cap.u64(e + off)
                if not p:
                    print(f"    E+{off:#x} = 0")
                    continue
                n = p - 0x158
                g = cap.u64(n + 0x1A0)
                idx = cap.i32(n + 0x1A8)
                flags = cap.u32(n + 0x1B4)
                print(f"    E+{off:#x} = {p:#x}  node {n:#x} (in {cap.name_of(n)})"
                      f"  N+0x1a0 graph {g and hex(g)}  N+0x1a8 idx {idx}"
                      f"  N+0x1b4 flags {flags and hex(flags)}")
                if g:
                    dump_graph(cap, g, idx or 0)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

"""Find live statescript runtime objects by their vtables and chase the dispatch.

Search: element vtable `0x7ff7c7e40360` and host vtable `0x7ff7c7e37000`
(v2b session) at object head, over the heap regions of a capture.

For each element found, chase the layout read from the code:
    E+0x38   -> executor X
    X+0x30   -> element array, X+0x38 = count, X+0x430 = current element
    X+0x490  -> context compared by the node gate, X+0x914, X+0x15a8 holder
    E+0x300 / E+0x318 -> node-chain member pointers; node base = ptr - 0x158
    N+0x1a0 -> graph G, N+0x1a8 = type index, N+0x1b4 flags, N+0x190 context
    S = [G+0x290]; P = [S+0x60]; mode = [S+0x68];
    impl = [[P + (0 if mode==1 else 8)]][N+0x1a8]

prints the impl address, which is the per-node-type implementation entry.

Usage:
    OW_DUMPS=dumps_v2b python find_script_instances.py [--limit N] [--host]
"""

from __future__ import annotations

import os
import struct
import sys

import regions as R

ELEM_VT = 0x7FF7C7E40360
HOST_VT = 0x7FF7C7E37000
MOD_LO, MOD_HI = 0x7FF7C4D20000, 0x7FF7C9000000


class Cap:
    def __init__(self, dirname: str):
        self.dir = R.capture_dir(dirname)
        self.regions = []
        for name in os.listdir(self.dir):
            parts = name.split("_")
            if len(parts) < 2:
                continue
            try:
                base = int(parts[0], 16)
                size = int(parts[1], 16)
            except ValueError:
                continue
            self.regions.append((base, size, os.path.join(self.dir, name)))
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
                    data = fh.read()
                self._cache_addr, self._cache_data = base, data
                off = addr - base
                return data[off:off + length]
        return None

    def q(self, addr: int) -> int | None:
        b = self.read(addr, 8)
        return struct.unpack("<Q", b)[0] if b and len(b) == 8 else None

    def i32(self, addr: int) -> int | None:
        b = self.read(addr, 4)
        return struct.unpack("<i", b)[0] if b and len(b) == 4 else None


def scan(cap: Cap, needle: int, limit: int, heap_only: bool):
    pat = struct.pack("<Q", needle)
    found = []
    total = 0
    for k, (base, size, path) in enumerate(cap.regions):
        if heap_only and MOD_LO <= base < MOD_HI:
            continue
        total += size
        with open(path, "rb") as fh:
            data = fh.read()
        i = 0
        while True:
            j = data.find(pat, i)
            if j < 0:
                break
            found.append(base + j)
            i = j + 8
            if limit and len(found) >= limit:
                return found, total
        if k % 2000 == 0:
            print(f"  ... {k}/{len(cap.regions)} regions, {total/1048576:.0f} MB, "
                  f"{len(found)} hit(s)", flush=True)
    return found, total


def chase(cap: Cap, e: int):
    print(f"\n=== element @ {e:#x} ===")
    for off in (0x30, 0x38, 0x300, 0x318, 0x3b0, 0x418, 0x420):
        v = cap.q(e + off)
        print(f"  E+{off:#05x} = {v:#x}" if v is not None else f"  E+{off:#05x} = ?")
    x = cap.q(e + 0x38)
    if x:
        print(f"  executor X = {x:#x}")
        for off in (0x30, 0x38, 0x430, 0x490, 0x914, 0xd20, 0x1394, 0x15a8):
            v = cap.q(x + off) if off != 0x914 and off != 0x38 and off != 0xd20 \
                else cap.i32(x + off)
            print(f"    X+{off:#05x} = {v:#x}" if v is not None else f"    X+{off:#05x} = ?")
        for s in (0x300, 0x318):
            p = cap.q(e + s)
            if not p:
                continue
            n = p - 0x158
            print(f"  chain via E+{s:#x}: member {p:#x} -> node {n:#x}")
            for step in range(6):
                if not n:
                    break
                g = cap.q(n + 0x1a0)
                idx = cap.i32(n + 0x1a8)
                fl = cap.read(n + 0x1b4, 1)
                ctx = cap.q(n + 0x190)
                nxt = cap.q(n + 0x160)
                print(f"    node[{step}] {n:#x}: graph={g if not g else hex(g)} "
                      f"idx={idx} flags={fl.hex() if fl else '?'} "
                      f"ctx={ctx if not ctx else hex(ctx)} next_member={nxt if not nxt else hex(nxt)}")
                if g:
                    s_ptr = cap.q(g + 0x290)
                    if s_ptr:
                        p60 = cap.q(s_ptr + 0x60)
                        mode = cap.q(s_ptr + 0x68)
                        print(f"      S={s_ptr:#x} P60={p60 if not p60 else hex(p60)} mode={mode}")
                        if p60 and idx is not None:
                            slotp = p60 if mode == 1 else p60 + 8
                            arr = cap.q(slotp)
                            if arr:
                                impl = cap.q(arr + 8 * idx)
                                print(f"      impl_table={arr:#x}  impl[{idx}] = "
                                      f"{impl if not impl else hex(impl)}")
                n = (nxt - 0x158) if nxt else 0


def main() -> int:
    limit = 4096
    if "--limit" in sys.argv:
        limit = int(sys.argv[sys.argv.index("--limit") + 1])
    mode = "host" if "--host" in sys.argv else "elem"
    cap = Cap(R.capture_dir("dumps_v2b"))
    print(f"{len(cap.regions)} regions")
    needle = HOST_VT if mode == "host" else ELEM_VT
    found, total = scan(cap, needle, limit, heap_only=False)
    print(f"scan done: {total/1048576:.0f} MB, {len(found)} instance(s) of "
          f"{'host' if mode == 'host' else 'element'} vtable")
    for a in found[:25]:
        print(f"  {a:#x}")
    if mode == "elem":
        for a in found[:6]:
            chase(cap, a)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

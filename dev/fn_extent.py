"""Recover a function's TRUE extent when the image stores it as `.pdata` fragments.

    python dev/fn_extent.py 0x14201ae60 [0x1420c685d ...]

The current image is known to fragment some functions: one real body becomes a
root `.pdata` entry plus any number of `UNW_FLAG_CHAININFO` fragments whose
bodies live elsewhere. A fragment start is not a function start, so every
address read out of a table or a listing must be walked back to the chain root
before it is called a function.

Reads the flat image at `dev/data/offline_recover/module_offline.bin`
(file offset == RVA, image base 0x140000000) and the plaintext `.pdata` run at
RVA 0x450b000. Read-only.

For each address it prints:
  - the `.pdata` entry that covers it (begin/end, unwind flags);
  - the chain root entry and the chain of fragments above it;
  - the union extent of every entry in the chain, and whether the addresses are
    contiguous (one body) or scattered (fragments).

`--group` prints every entry that shares the root of the given address.
"""

from __future__ import annotations

import struct
import sys

IMG = "dev/data/offline_recover/module_offline.bin"
BASE = 0x140000000
PDATA_RVA = 0x450B000
IMG_SIZE = 0x4838000

UNW_FLAG_EHANDLER = 1
UNW_FLAG_UHANDLER = 2
UNW_FLAG_CHAININFO = 4


def load() -> bytes:
    with open(IMG, "rb") as f:
        return f.read()


def entries(data: bytes) -> list[tuple[int, int, int]]:
    out: list[tuple[int, int, int]] = []
    prev = 0
    n = (len(data) - PDATA_RVA) // 12
    for i in range(n):
        begin, end, unwind = struct.unpack_from("<III", data, PDATA_RVA + 12 * i)
        if begin >= end or end >= IMG_SIZE or unwind >= IMG_SIZE:
            break
        if i and begin < prev:
            break
        prev = begin
        out.append((begin, end, unwind))
    return out


def unwind(data: bytes, rva: int):
    """Return (version, flags, size_of_prolog, count_codes, chained) for an UNWIND_INFO."""
    off = rva
    if off + 4 > len(data):
        return None
    b0 = data[off]
    version = b0 & 7
    flags = (b0 >> 3) & 7
    size_prolog = data[off + 1]
    count = data[off + 2]
    frame = data[off + 3]
    chained = None
    if flags & UNW_FLAG_CHAININFO:
        # slots are 2 bytes each; chained RUNTIME_FUNCTION follows, aligned up
        tail = off + 4 + 2 * count
        if (off + 4) & 1:
            tail += 0
        tail = off + 4 + 2 * count
        if tail & 1:
            tail += 1
        if tail + 12 <= len(data):
            cb, ce, cu = struct.unpack_from("<III", data, tail)
            chained = (cb, ce, cu)
    return version, flags, size_prolog, count, frame, chained


def covering(ents, rva: int):
    for k, (b, e, u) in enumerate(ents):
        if b <= rva < e:
            return k
    return None


def root_of(data: bytes, ents, rva: int):
    k = covering(ents, rva)
    if k is None:
        return None, []
    chain = [k]
    seen = {k}
    b, e, u = ents[k]
    while True:
        info = unwind(data, u)
        if info is None or info[5] is None:
            break
        cb, ce, cu = info[5]
        pk = covering(ents, cb)
        if pk is None or pk in seen:
            break
        chain.append(pk)
        seen.add(pk)
        b, e, u = ents[pk]
    return chain[-1], chain


def main() -> int:
    data = load()
    ents = entries(data)
    print(f"pdata entries: {len(ents):,}")
    for arg in sys.argv[1:]:
        if arg == "--group":
            continue
        va = int(arg, 16)
        rva = va - BASE
        k = covering(ents, rva)
        if k is None:
            print(f"{va:#x}: not inside a .pdata entry")
            continue
        b, e, u = ents[k]
        info = unwind(data, u)
        flags = info[1] if info else -1
        tag = "CHAINED fragment" if flags & UNW_FLAG_CHAININFO else "root entry"
        print(f"\n{va:#x}: .pdata {b + BASE:#x}-{e + BASE:#x} ({e - b:#x} B) "
              f"unwind {u:#x} flags {flags} -> {tag}")
        rk, chain = root_of(data, ents, rva)
        if rk is None:
            continue
        rb, re_, ru = ents[rk]
        print(f"  root: {rb + BASE:#x}-{re_ + BASE:#x} ({re_ - rb:#x} B); chain {len(chain)}")
        for idx in chain:
            cb, ce, _cu = ents[idx]
            print(f"    frag {cb + BASE:#x}-{ce + BASE:#x} ({ce - cb:#x} B)")
        if "--group" in sys.argv:
            members = [i for i, (_b, _e, uu) in enumerate(ents)
                       if uu == ru or (unwind(data, uu) and unwind(data, uu)[5]
                                       and unwind(data, uu)[5][2] == ru)]
            lo = min(ents[i][0] for i in members)
            hi = max(ents[i][1] for i in members)
            print(f"  group: {len(members)} entries, span {lo + BASE:#x}-{hi + BASE:#x}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

"""Find every static reference to a set of addresses, and name the function it sits in.

    python dev/callers.py 0x14201ae60 0x142014b50 ...        # calls, jumps, pointers
    python dev/callers.py --no-fn 0x14201ae60                # bare hit list

The image is known to store some functions as `.pdata` chained fragments, so a
hit address alone does not identify a caller. Every hit is therefore resolved to
the **chain root** of its containing function through `dev/fn_extent.py`'s
`.pdata` walk, and printed as `root+delta` so two hits in one function group
together.

Reference forms tested (each explicitly counted, so "0" is a measurement):

  E8  rel32      call
  E9  rel32      jmp
  0F 8x rel32    conditional jump
  8-byte little-endian absolute pointer (vtable slots, function tables)
  4-byte RVA (self-relative dispatch tables) -- reported as a separate count
       because a code-range copy of an address is *not* a dispatch entry;
       the caller must inspect the indexing code.

Read-only: reads `dev/data/offline_recover/module_offline.bin`
(offset == RVA, base 0x140000000).
"""

from __future__ import annotations

import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fn_extent as fe

BASE = 0x140000000
TEXT_START = 0x1000
TEXT_END = 0x3E00000  # generous .text bound; the image's code is inside it


def scan_rel(d: bytes, opcode: bytes, targets: dict[int, str]):
    out = []
    pos = TEXT_START
    n = len(d)
    while True:
        i = d.find(opcode, pos, min(n, TEXT_END))
        if i < 0:
            break
        pos = i + 1
        if i + len(opcode) + 4 > n:
            break
        t = BASE + i + len(opcode) + 4 + struct.unpack_from("<i", d, i + len(opcode))[0]
        if t in targets:
            out.append((BASE + i, opcode.hex(), t))
    return out


def scan_jcc(d: bytes, targets: dict[int, str]):
    out = []
    pos = TEXT_START
    while True:
        i = d.find(b"\x0f", pos, min(len(d), TEXT_END))
        if i < 0:
            break
        pos = i + 1
        if i + 6 > len(d):
            break
        if 0x80 <= d[i + 1] <= 0x8F:
            t = BASE + i + 6 + struct.unpack_from("<i", d, i + 2)[0]
            if t in targets:
                out.append((BASE + i, f"jcc {d[i + 1]:#x}", t))
    return out


def scan_ptr(d: bytes, targets: dict[int, str]):
    out = []
    for t in targets:
        pat = struct.pack("<Q", t)
        pos = 0
        while True:
            i = d.find(pat, pos)
            if i < 0:
                break
            pos = i + 1
            out.append((BASE + i, "qword", t))
    return out


def scan_rva4(d: bytes, targets: dict[int, str]):
    out = []
    for t in targets:
        pat = struct.pack("<I", t - BASE)
        pos = 0
        while True:
            i = d.find(pat, pos)
            if i < 0:
                break
            pos = i + 1
            out.append((BASE + i, "dword-rva", t))
    return out


def main() -> int:
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    show_fn = "--no-fn" not in sys.argv
    targets = {int(a, 16): a for a in args}
    d = fe.load()
    ents = fe.entries(d)

    def where(va: int) -> str:
        if not show_fn:
            return ""
        rk, chain = fe.root_of(d, ents, va - BASE)
        if rk is None:
            return "  (no .pdata)"
        rb = ents[rk][0] + BASE
        return f"  in {rb:#x}+{va - rb:#x}" + (f" (chain {len(chain)})" if len(chain) > 1 else "")

    groups = {
        "E8 call": scan_rel(d, b"\xe8", targets),
        "E9 jmp": scan_rel(d, b"\xe9", targets),
        "jcc": scan_jcc(d, targets),
        "8-byte pointer": scan_ptr(d, targets),
        "4-byte RVA": scan_rva4(d, targets),
    }
    for name, hits in groups.items():
        print(f"== {name}: {len(hits)} hit(s)")
        for va, kind, t in sorted(hits):
            print(f"  {va:#x} -> {t:#x}  [{kind}]{where(va)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

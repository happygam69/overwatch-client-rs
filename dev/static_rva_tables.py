"""The image-base-relative 4-byte RVA dispatch tables, and what they select.

The client's computed jumps over small integer switches look like this
(`0x14208e747..0x14208e75f`, the case maps.md cites):

    lea  rcx, [rip - <image base>]        ; rcx = image base
    mov  ecx, [rcx + rax*4 + <table rva>] ; a 4-byte RVA, indexed by the switch
    lea  rax, [rip - <image base>]        ; image base again
    add  rcx, rax                         ; absolute target
    jmp  rcx

So a table entry is an *image-base-relative* RVA, and "is function F reachable"
becomes: does any such table hold an RVA inside F, and does code index that
table? This probe enumerates the tables by their indexing instruction (the
`mov r?d, [base + rax*4 + disp32]` form), then answers for a given address.

The `.pdata` copies at `0x14469xxxx` and the code-range copy in `.rdata` are
*not* dispatch tables: their triples are `{begin rva, end rva, unwind rva}`, so
a function's RVA appears there as a range boundary. This probe separates them.

Usage:
    python dev/static_rva_tables.py                     # enumerate
    python dev/static_rva_tables.py 0x20a02f6           # ask about one RVA
"""

from __future__ import annotations

import struct
import sys

IMAGE = "dev/data/offline_recover/module_offline.bin"
BASE = 0x140000000
TEXT_LO, TEXT_HI = 0x140001000, 0x1433C4000
RDATA_LO, RDATA_HI = 0x143300000, 0x144600000


def load() -> bytes:
    with open(IMAGE, "rb") as f:
        return f.read()


def dispatch_sites(img: bytes) -> dict[int, list[int]]:
    """disp32 -> the addresses of instructions that index a table with it.

    The form is `mov r32, [rcx + rax*4 + disp32]`: opcode `8B`, ModRM
    mod=10/rm=100, SIB scale=10/index=rax/base=rcx, then the disp32. Requiring
    the SIB and the rax index keeps this to the four-byte table index; the base
    is checked separately by the caller.
    """
    sites: dict[int, list[int]] = {}
    i = TEXT_LO - BASE
    while True:
        i = img.find(b"\x8b", i)
        if i < 0 or i + BASE >= TEXT_HI:
            break
        b = img[i:i + 8]
        if len(b) >= 7 and (b[1] & 0xC7) == 0x84 and b[2] in (0x81, 0x89, 0x91, 0x99,
                                                             0xA1, 0xA9, 0xB1, 0xB9):
            disp = struct.unpack_from("<i", b, 3)[0] & 0xFFFFFFFF
            sites.setdefault(disp, []).append(i + BASE)
        i += 1
    return sites


def entries(img: bytes, rva: int, cap: int = 256) -> list[int]:
    out = []
    for k in range(cap):
        at = rva + 4 * k
        if at < 0 or at + 4 > len(img):
            break
        v = struct.unpack_from("<I", img, at)[0]
        out.append(v)
        if k > 4 and not (0x1000 <= v < TEXT_HI - BASE):
            break
    return out


def main() -> int:
    img = load()
    sites = dispatch_sites(img)
    # The tables live in `.text`, in the tail of the function that indexes
    # them, so "is this a table" is decided by its contents: its first entries
    # are code RVAs.
    def looks_like_table(rva: int) -> bool:
        first = entries(img, rva, 4)
        return len(first) >= 3 and all(0x1000 <= v < TEXT_HI - BASE for v in first[:3])

    tables = {rva: s for rva, s in sites.items() if looks_like_table(rva)}
    print(f"{sum(len(s) for s in sites.values())} indexing site(s), "
          f"{len(sites)} distinct table RVA(s), {len(tables)} holding code RVAs")

    if len(sys.argv) < 2:
        for rva, s in sorted(tables.items()):
            ents = entries(img, rva)
            body = " ".join(f"{e:08x}" for e in ents[:8])
            print(f"  table {BASE + rva:#x} ({len(s)} site(s)): {body}")
        return 0

    target = int(sys.argv[1], 16) & 0xFFFFFFFF
    # A function's span, so "inside" covers a switch case body too.
    span = None
    if len(sys.argv) > 2:
        span = (target, int(sys.argv[2], 16) & 0xFFFFFFFF)
    print(f"target rva {target:#x}" + (f" .. {span[1]:#x}" if span else ""))

    hits = 0
    for rva, s in sorted(tables.items()):
        for e in entries(img, rva):
            inside = (e == target) if span is None else (span[0] <= e < span[1])
            if inside:
                hits += 1
                print(f"  HIT table {BASE + rva:#x} entry {e:#x} at site(s) "
                      f"{[hex(a) for a in s]}")
    print(f"{hits} dispatch-table entry/entries select the target")

    # Every 4-byte occurrence of the RVA, with the region it sits in.
    pat = struct.pack("<I", target)
    i = 0
    seen = []
    while True:
        i = img.find(pat, i)
        if i < 0:
            break
        va = i + BASE
        region = ("pdata-copy" if 0x144600000 <= va < 0x144900000
                  else "rdata" if RDATA_LO <= va < RDATA_HI
                  else "text" if TEXT_LO <= va < TEXT_HI
                  else "other")
        seen.append((va, region))
        i += 1
    print(f"{len(seen)} 4-byte occurrence(s) of the RVA:",
          [(hex(v), r) for v, r in seen])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

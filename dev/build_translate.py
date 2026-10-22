"""Translate a function's address from one client build to the next.

    python dev/build_translate.py --old <image> --new <image> [--out FILE]

The client updates itself, and an update moves code: between the 2.25.0.0 and
2.25.0.1 builds almost every page sits somewhere else. A page-content match
therefore finds almost nothing - a page holds a different mix of functions
after the patch.

A *function* is the unit that survives: the patch moves code, and a function
keeps its bytes. So this matches on the function's own body, taken from the
runtime `.pdata` (the unwind table, which the capture holds in full for both
builds).

The result is a table: the old function's address -> the new one's. Every
address in the docs belongs to a build, and this is what carries them across.
"""

from __future__ import annotations

import hashlib
import json
import pathlib
import struct
import sys

PAGE = 4096
TEXT_RVA = 0x1000
TEXT_SIZE = 0x2F2B3D3
# The unwind table's place in the image moves a little between builds, so it
# is found by its own shape rather than by a hard-coded offset: a run of
# 12-byte entries whose first function starts at the image base.
PDATA_SEARCH = range(0x4500000, 0x4530000, 4)


def find_pdata(blob: bytes) -> int:
    """The file offset of the runtime unwind table.

    Scored by how many of the first entries are valid, and by whether the
    first of them starts at the image base - the table's first function is the
    first function in `.text`.
    """
    best = (0, -1, False)
    for off in PDATA_SEARCH:
        good = 0
        first_is_base = False
        for k in range(200):
            base = off + k * 12
            if base + 12 > len(blob):
                break
            begin, end, _ = struct.unpack_from("<III", blob, base)
            if TEXT_RVA <= begin < end < TEXT_RVA + TEXT_SIZE:
                good += 1
                if k == 0 and begin == TEXT_RVA:
                    first_is_base = True
        score = good + (1000 if first_is_base else 0)
        if score > best[1]:
            best = (off, score, first_is_base)
    return best[0]


def functions(blob: bytes) -> list[tuple[int, int]]:
    """(begin rva, end rva) from the runtime unwind table."""
    out = []
    pdata_off = find_pdata(blob)
    pdata = blob[pdata_off : pdata_off + 0x24D968]
    for off in range(0, len(pdata) - 12, 12):
        begin, end, _unwind = struct.unpack_from("<III", pdata, off)
        if begin == 0 and end == 0:
            continue
        if not (TEXT_RVA <= begin < end < TEXT_RVA + TEXT_SIZE):
            continue
        out.append((begin, end))
    return out


def main() -> int:
    args = sys.argv[1:]
    old_path = new_path = None
    out_path = None
    for i, a in enumerate(args):
        if a == "--old" and i + 1 < len(args):
            old_path = pathlib.Path(args[i + 1])
        elif a == "--new" and i + 1 < len(args):
            new_path = pathlib.Path(args[i + 1])
        elif a == "--out" and i + 1 < len(args):
            out_path = pathlib.Path(args[i + 1])
    if not old_path or not new_path:
        print(__doc__)
        return 2

    old = old_path.read_bytes()
    new = new_path.read_bytes()
    old_fns = functions(old)
    new_fns = functions(new)
    print(f"{old_path.name}: {len(old_fns)} functions from .pdata")
    print(f"{new_path.name}: {len(new_fns)} functions from .pdata")

    # hash the new build's function bodies
    index: dict[str, list[int]] = {}
    for begin, end in new_fns:
        h = hashlib.sha1(new[begin:end]).hexdigest()
        index.setdefault(h, []).append(begin)

    table: dict[int, int | None] = {}
    same = moved = void = 0
    for begin, end in old_fns:
        h = hashlib.sha1(old[begin:end]).hexdigest()
        hits = index.get(h, [])
        if len(hits) == 1:
            target = hits[0]
            table[begin] = target
            if target == begin:
                same += 1
            else:
                moved += 1
        else:
            table[begin] = None
            void += 1
    total = same + moved + void
    print(f"\n{old_path.name} -> {new_path.name}: {total} functions")
    print(f"  same address {same:>7} ({100 * same / total:.1f}%)")
    print(f"  moved         {moved:>7} ({100 * moved / total:.1f}%)")
    print(f"  gone          {void:>7} ({100 * void / total:.1f}%) - the function does not exist in the new build")

    if out_path:
        out_path.write_text(json.dumps({str(k): v for k, v in table.items()}), encoding="utf-8")
        print(f"wrote {out_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

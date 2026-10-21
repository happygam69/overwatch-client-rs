"""Trace pointer chains inside a session-coherent image.

Every pointer chain we chase must start here, not in a merged image: a
merged image mixes session bases, so pointers resolve to garbage rvas
and the chain 'goes cold' for reasons that are our fault, not the game's.

Usage:
    python dev/trace_chain.py <img.bin> <rva> [rva ...] [--depth 3]
        [--runs E:\\ow-dumps\\live_page_diff]

The image must be a file written by session_merge.py (a .bin plus a
.json sidecar holding `base`).  A qword that lands inside the module
window resolves to `value - base`, we test whether that page is warm in
the image, and recurse.  A qword outside the window is marked `ext`; if
--runs is given, we test the value against every run's meta.json module
bases: a value that matches exactly one run's main-module window is
printed as an external-session pointer with the rva it names there
(those are salvageable: rva targets are session-stable, only pointer
VALUES move).
"""
import argparse
import json
import os
import struct

import numpy as np


def load_runs(runs_dir):
    bases = []
    if not runs_dir:
        return bases
    for name in sorted(os.listdir(runs_dir)):
        meta_path = os.path.join(runs_dir, name, "meta.json")
        if not os.path.isfile(meta_path):
            continue
        try:
            meta = json.load(open(meta_path, encoding="utf-8"))
        except Exception:
            continue
        for m in meta.get("modules", []):
            bases.append((name, m[0], int(m[1]), int(m[2])))
    return bases


def ext_resolution(value, bases):
    hits = []
    for name, module, base, _size in bases:
        Δ = value - base
        if 0 <= Δ < 0x6000000:
            hits.append((name, module, Δ))
    return hits


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("img")
    ap.add_argument("starts", nargs="+")
    ap.add_argument("--depth", type=int, default=3)
    ap.add_argument("--runs", default=r"E:\ow-dumps\live_page_diff")
    ap.add_argument("--base", default="0",
                    help="module base for images without a json sidecar")
    args = ap.parse_args()

    if os.path.isfile(args.img + ".json"):
        side = json.load(open(args.img + ".json", encoding="utf-8"))
        base = int(side["base"])
    else:
        base = int(args.base, 16)
    a = np.fromfile(args.img, np.uint8)
    size = a.size
    qword = lambda rva: struct.unpack_from("<Q", a, rva)[0]
    warm = lambda rva: a[(rva >> 12) << 12:((rva >> 12) << 12) + 4096].any()
    n_pages = size >> 12

    bases = load_runs(args.runs)
    print(f"image {args.img}: base {base:#x}, {n_pages} pages, "
          f"{int(np.count_nonzero(a))} nonzero bytes; "
          f"known runs {len(bases)}")

    for start in args.starts:
        rva = int(start, 16)
        print(f"\n== {rva:#x} ==")
        stack = [(rva, 0)]
        seen = set()
        while stack:
            r, d = stack.pop(0)
            if r in seen or d > args.depth:
                continue
            seen.add(r)
            ind = "  " * d
            if not (0 <= r < size - 8):
                print(f"{ind}{r:#x}: out of image")
                continue
            v = qword(r)
            tag = "warm" if warm(r) else "COLD"
            if base <= v < base + size:
                t = v - base
                ttag = "warm" if warm(t & ~0xfff) else "COLD"
                print(f"{ind}{r:#x}[{tag}] -> {t:#x}([{ttag}] page "
                      f"{t >> 12:#x})")
                stack.append((t, d + 1))
            else:
                print(f"{ind}{r:#x}[{tag}] -> ext {v:#x}")
                hits = ext_resolution(v, bases)
                if hits:
                    for name, Δ in hits[:2]:
                        print(f"{ind}    ext matches {name}/{module}: "
                              f"rva {Δ:#x}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())


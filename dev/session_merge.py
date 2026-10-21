"""Build a SESSION-COHERENT main-module image from one capture run.

Problem this fixes: warm_merge.py unions pages across sessions without
rebasing, so pointer values inside pages reference each page's own
session base. Tracing a pointer chain across a merged image hops between
menus - today's blend-tree hunt died exactly there. A single run's pages
are coherent: everything a pointer references was in the same address
space at the same time.

Usage:
    python dev/session_merge.py <run_dir> [--module Overwatch.exe]
        [--out PATH]

Reads the run's warm.csv, flattens all captured pages of the module into
one numpy array sized by the session's module size (from meta.json),
keeps the session base in a .json sidecar, and reports np/warm/zero
counts.  Read-only against the repo; output lands on D:/ow-dumps/.
"""
import argparse
import csv
import os
import json

import numpy as np


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("run_dir")
    ap.add_argument("--module", default="Overwatch.exe")
    ap.add_argument("--out", default="")
    args = ap.parse_args()
    out = args.out or (r"D:\ow-dumps\session_" +
                       os.path.basename(args.run_dir) + "_" +
                       args.module.replace(".", "_") + ".bin")
    meta = json.load(open(os.path.join(args.run_dir, "meta.json")))
    mods = {m[0]: (m[1], m[2]) for m in meta["modules"]}
    if args.module not in mods:
        print(f"{args.module} not in run meta")
        return 2
    base, size = mods[args.module]
    n = size >> 12
    img = np.zeros((n, 4096), np.uint8)
    pages = 0
    src = {}
    with open(os.path.join(args.run_dir, "warm.csv"), newline="",
              encoding="utf-8") as fh:
        for r in csv.DictReader(fh):
            if r["module"] != args.module:
                continue
            v = int(r["session"], 16)
            if v < base:
                continue
            off = (v - base) >> 12
            if off >= n:
                continue
            if args.module == "Overwatch.exe":
                path = os.path.join(args.run_dir, "pages",
                                    f"v2b_{int(r['v2b'], 16):012x}.bin")
            else:
                path = os.path.join(args.run_dir, "pages",
                                    f"{args.module}_{v - base:012x}.bin")
            if not os.path.isfile(path):
                continue
            blob = open(path, "rb").read(4096)
            img[off] = np.frombuffer(blob, np.uint8)[:4096]
            pages += 1
            src[r["src"]] = src.get(r["src"], 0) + 1
    warm = sum(1 for row in img if row.any())
    img.tofile(out)
    side = out + ".json"
    with open(side, "w", encoding="utf-8") as fh:
        json.dump({"base": base, "module": args.module,
                   "size": n * 4096, "captured_pages": pages,
                   "warm_pages": warm, "src": src},
                  fh, indent=1)
    print(f"wrote {out}: {pages} pages captured, {warm} nonzero survivors; "
          f"src {src}; base {base:#x}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

"""Classify every 4 KB page of the flattened captures against the on-disk PEs.

Why this exists: the page-crypt recon needs one reliable answer - is the
at-rest ciphertext of a mapped page identical to the bytes the file on disk
holds at that RVA?  Earlier ad-hoc shell probes disagreed with each other
(stride-sampling artifacts, wrong candidate subsets), so this survey does
the whole matrix once, with a full 4096-byte histogram per page:

* classifies every page as cipher / plain-code / plain-data / zero by entropy
  and non-zero count,
* maps each page's RVA onto the on-disk PE section table (Overwatch.exe and
  Overwatch_loader.dll), and counts differing bytes for every page,
* compares every page across the two standing captures
  (module_after_diff.bin, module_v2full.bin) to find golden pairs
  (cipher in one, plaintext in the other),
* writes a per-page CSV summary for the next step and prints a compact
  table.

Usage:
    python page_crypt_survey.py [--out PATH]

Read-only; writes only its CSV report under D:/ow-dumps unless --out given.
"""
from __future__ import annotations

import argparse
import csv
import struct
import sys

import numpy as np

V2B_BASE = 0x7FF7C4D20000
PAGE = 4096
EXE = r"E:\Overwatch\_retail_\Overwatch.exe"
LDR = r"E:\Overwatch\_retail_\Overwatch_loader.dll"
CUR_NAME = r"D:\ow-dumps\module_after_diff.bin"
OLD_NAME = r"D:\ow-dumps\module_v2full.bin"
DEFAULT_OUT = r"D:\ow-dumps\page_crypt_survey.csv"


def sections(path: str):
    d = open(path, "rb").read()
    pe = d.find(b"PE\0\0")
    nsec = struct.unpack_from("<H", d, pe + 6)[0]
    optsz = struct.unpack_from("<H", d, pe + 4 + 16)[0]
    secoff = pe + 4 + 16 + 4 + optsz
    secs = []
    for i in range(nsec):
        o = secoff + i * 40
        name = d[o:o + 8].rstrip(b"\0").decode("latin1")
        vsz, va, rsz, roff = struct.unpack_from("<IIII", d, o + 8)
        secs.append((name, va, vsz, roff, rsz))
    return d, secs


def r2f(secs, rva: int):
    """RVA -> (file offset or None, section name)."""
    for name, va, vsz, roff, rsz in secs:
        if va <= rva < va + rsz:
            return roff + (rva - va), name
    return None, None


def ent_full(page: np.ndarray) -> float:
    c = np.bincount(page, minlength=256).astype(np.float64)
    p = c / len(page)
    return float(-(p[p > 0] * np.log2(p[p > 0])).sum())


def classify(ent: float, nz: int) -> str:
    if nz == 0:
        return "zero"
    if ent >= 7.85:
        return "cipher"
    if ent <= 7.2 and nz >= 64:
        return "plain"
    return "other"


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=DEFAULT_OUT)
    args = ap.parse_args()

    exe_bytes, exe_secs = sections(EXE)
    ldr_bytes, ldr_secs = sections(LDR)
    cur = open(CUR_NAME, "rb").read()
    old = open(OLD_NAME, "rb").read()
    npages = min(len(cur), len(old)) // PAGE
    curp = np.frombuffer(cur[:npages * PAGE], np.uint8).reshape(npages, PAGE)
    oldp = np.frombuffer(old[:npages * PAGE], np.uint8).reshape(npages, PAGE)

    ents_c = np.array([ent_full(curp[i]) for i in range(npages)])
    ents_o = np.array([ent_full(oldp[i]) for i in range(npages)])
    nz_c = np.count_nonzero(curp, axis=1)
    nz_o = np.count_nonzero(oldp, axis=1)
    cls_c = [classify(ents_c[i], int(nz_c[i])) for i in range(npages)]
    cls_o = [classify(ents_o[i], int(nz_o[i])) for i in range(npages)]

    # per-page: diff byte count against the on-disk exe (cur capture)
    diff_cur = np.zeros(npages, dtype=np.int64)
    sec_name = []
    nofile = 0
    for i in range(npages):
        fo, name = r2f(exe_secs, i * PAGE)
        if fo is None or fo + PAGE > len(exe_bytes):
            nofile += 1
            sec_name.append("")
            continue
        sec_name.append(name)
        diff_cur[i] = int(np.count_nonzero(curp[i] != np.frombuffer(
            exe_bytes[fo:fo + PAGE], np.uint8)))

    # golden pairs: cipher in one capture, plain in the other
    gold = [(i, cls_c[i], cls_o[i]) for i in range(npages)
            if (cls_c[i] == "cipher" and cls_o[i] == "plain")
            or (cls_o[i] == "cipher" and cls_c[i] == "plain")]

    counts = {}
    for i in range(npages):
        key = (cls_c[i], cls_o[i], diff_cur[i] == 0, nofile if False else
               (0 if sec_name[i] else 1))
        counts[key] = counts.get(key, 0) + 1

    print(f"pages {npages}  (image lens: cur {len(cur)}, old {len(old)})")
    for cls in ("cipher", "plain", "zero", "other"):
        c = cls_c.count(cls)
        o = cls_o.count(cls)
        print(f"  {cls:6} cur {c:6d}  old {o:6d}   (nofile rows: {nofile})")
    print("  class x file-diff (cur capture):")
    for cls in ("cipher", "plain", "zero", "other"):
        ids = same = few = many = nf = 0
        for i in range(npages):
            if cls_c[i] != cls:
                continue
            if not sec_name[i]:
                nf += 1
            elif diff_cur[i] == 0:
                same += 1
            elif diff_cur[i] <= 64:
                few += 1
            else:
                many += 1
        print(f"    {cls:6} identical {same:6d}  few<=64 {few:5d}  "
              f"many {many:5d}  no-file-span {nf:5d}")
    print(f"  golden pairs (cipher in one, plain in other): {len(gold)}")

    with open(args.out, "w", newline="", encoding="utf-8") as fh:
        w = csv.writer(fh)
        w.writerow(["rva", "sec", "ent_cur", "nz_cur", "cls_cur",
                    "ent_old", "nz_old", "cls_old", "file_diff"])
        for i in range(npages):
            w.writerow(["0x%06x" % (i * PAGE), sec_name[i],
                        "%.3f" % ents_c[i], int(nz_c[i]), cls_c[i],
                        "%.3f" % ents_o[i], int(nz_o[i]), cls_o[i],
                        int(diff_cur[i])])
    print(f"wrote {args.out}")

    # cipher pages identical to the file: pin the at-rest == disk question
    xs = [(i * PAGE, sec_name[i]) for i in range(npages)
          if cls_c[i] == "cipher" and sec_name[i] and diff_cur[i] == 0]
    print(f"  cipher & byte-identical to disk: {len(xs)}")
    for rva, nm in xs[:12]:
        print(f"    rva 0x{rva:06x} {nm}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

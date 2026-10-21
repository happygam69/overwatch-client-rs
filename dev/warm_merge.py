"""Merge warm_poll captures over the flattened v2b image, page by page.

Each warm_poll run stores the most-populated snapshot of every 4 KB page it
ever caught warm, keyed by the flattened-image address (v2b).  This tool walks
those pages, compares each one against the same page in the base image
(module_v2full.bin, base 0x7FF7C4D20000), keeps whichever has more non-zero
bytes, and writes the union.  Any session's warm set fills another capture's
cold gaps, so repeated runs only add.

Memory is bounded on purpose: the output image is allocated once pre-sized,
the base file is copied in 8 MB chunks, and every comparison is a 4 KB
bytes slice.  No whole-image duplicates and no per-byte Python objects.

Usage:
    python warm_merge.py [--base PATH] [--out PATH] [--module NAME]
                         [--report PATH] run_dir [run_dir ...]

`--module` selects one module from each run's warm.csv (default
`Overwatch.exe`; use `Overwatch_loader.dll` with the loader base/image).
Both v3 `v2b_<addr>.bin` pages and the older `<addr>.bin` pages are accepted.
"""

from __future__ import annotations

import csv
import os
import sys

import numpy as np

V2B_BASE = 0x7FF7C4D20000
DEFAULT_BASE = "D:/ow-dumps/module_v2full.bin"
DEFAULT_OUT = "D:/ow-dumps/warm_best.bin"
PAGE = 0x1000
CHUNK = 1 << 23  # 8 MB
WARM_MIN = 64
ENT_MAX = 7.2
# Uniform filler (`0xCC`, `0xC3`, `0x90`) is readable but not code; v3 can
# capture it as a 4096-nonzero, zero-entropy page. It must never displace a
# real code/data page.
ENT_MIN = 0.5


def classify(blob: bytes) -> tuple[int, float]:
    """(non-zero bytes, byte entropy); real code is ~6.0-6.5, ciphertext ~8.0."""
    arr = np.frombuffer(blob, dtype=np.uint8)
    counts = np.bincount(arr, minlength=256).astype(np.float64)
    nz = int(len(blob) - counts[0])
    p = counts / len(blob)
    ent = float(-(p[p > 0] * np.log2(p[p > 0])).sum())
    return nz, ent


def main() -> int:
    args = sys.argv[1:]
    def opt(name, default=None):
        return args[args.index(name) + 1] if name in args else default

    base_path = opt("--base", DEFAULT_BASE)
    out_path = opt("--out", DEFAULT_OUT)
    module = opt("--module", "Overwatch.exe")
    report_path = opt("--report", os.path.join(os.path.dirname(out_path), "warm_best.csv"))
    runs = [a for a in args if os.path.isdir(a) and a not in (base_path, out_path)]
    if not runs:
        print(__doc__)
        return 2

    # Pass 1: catalogue the pages, no page bytes held.
    entries: list[tuple[str, int]] = []  # (page file, file offset)
    max_off = os.path.getsize(base_path)
    skipped = 0
    for run in runs:
        csv_path = os.path.join(run, "warm.csv")
        pages_dir = os.path.join(run, "pages")
        if not os.path.isfile(csv_path) or not os.path.isdir(pages_dir):
            print(f"{run}: no warm.csv/pages, skipped")
            continue
        with open(csv_path, newline="", encoding="utf-8") as fh:
            for row in csv.DictReader(fh):
                if row.get("module") != module:
                    continue
                v2b = int(row["v2b"], 16)
                # v3 names main pages `v2b_<addr>.bin`; warm_poll and the
                # first live_page_diff runs used `<addr>.bin`; non-main
                # modules are `<module>_<rva>.bin`.
                candidates = [
                    os.path.join(pages_dir, f"v2b_{v2b:012x}.bin"),
                    os.path.join(pages_dir, f"{v2b:012x}.bin"),
                    os.path.join(pages_dir, f"{module}_{v2b:012x}.bin"),
                ]
                path = next((p for p in candidates if os.path.isfile(p)), None)
                if path is None:
                    skipped += 1
                    continue
                off = v2b - (V2B_BASE if module == "Overwatch.exe" else 0)
                if off < 0:
                    skipped += 1
                    continue
                entries.append((path, off))
                if off + PAGE > max_off:
                    max_off = off + PAGE
    print(f"{len(entries)} {module} page(s) found across {len(runs)} run(s); "
          f"image target {max_off / 1048576:.1f} MB ({skipped} skipped)")

    # Allocate once; copy the base file in bounded chunks.
    img = bytearray(max_off)
    base_len = os.path.getsize(base_path)
    with open(base_path, "rb") as fh:
        off = 0
        while off < base_len:
            chunk = fh.read(min(CHUNK, base_len - off))
            if not chunk:
                break
            img[off:off + len(chunk)] = chunk
            off += len(chunk)
    print(f"base loaded: {base_len / 1048576:.1f} MB")

    # Pass 2: best accepted snapshot per page.
    rows = []
    skipped_enc = 0
    for path, off in entries:
        with open(path, "rb") as fh:
            blob = fh.read(PAGE)
        if len(blob) < 64:
            continue
        new_nz, new_ent = classify(blob)
        if new_nz < WARM_MIN or new_ent > ENT_MAX or new_ent < ENT_MIN:
            skipped_enc += 1
            continue
        old = bytes(img[off:off + len(blob)])
        base_nz, base_ent = classify(old)
        base_ok = (
            base_nz >= WARM_MIN
            and base_ent <= ENT_MAX
            and base_ent >= ENT_MIN
        )
        if not base_ok or new_nz > base_nz:
            img[off:off + len(blob)] = blob
            rows.append((off + V2B_BASE, base_nz, new_nz))

    with open(out_path, "wb") as fh:
        fh.write(img)
    print(f"wrote {out_path}: {len(img) / 1048576:.1f} MB, "
          f"{len(rows)} page(s) replaced, {skipped_enc} encrypted skipped")

    with open(report_path, "w", newline="", encoding="utf-8") as fh:
        w = csv.writer(fh)
        w.writerow(["v2b", "base_nonzero", "new_nonzero"])
        for v2b, base_nz, new_nz in sorted(rows):
            w.writerow([f"{v2b:012x}", base_nz, new_nz])

    recovered = [r for r in rows if r[1] == 0 and r[2] >= 64]
    print(f"fully recovered pages (cold in the old image): {len(recovered)}")
    bands: dict[int, int] = {}
    for v2b, _b, _n in recovered:
        bands[v2b >> 20] = bands.get(v2b >> 20, 0) + 1
    for band in sorted(bands):
        print(f"  {band << 20:012x}..{(band << 20) + 0xFFFFF:012x}  {bands[band]} page(s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

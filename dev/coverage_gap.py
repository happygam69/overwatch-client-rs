"""Coverage gap auditor for a session-coherent image.

Answers the operator's standing requirement: after every capture, list
precisely which pages stayed cold, grouped by section, and - for .text -
name the function ranges (from the install's .pdata) those cold pages
belong to.  "We didn't get animation" must become a listed, attributable
gap we can chase, not a vague need for another capture.

Usage:
    python dev/coverage_gap.py <img.bin> [--exe PATH] [--csv PATH]
        [--min 8]

<img.bin> is a dev/session_merge.py output (sidecar .json carries the
base).  --exe is the install's Overwatch.exe on disk, source of the
section table and .pdata.  Cold = all-zero 4 KiB page in the image.
Report: per section, contiguous cold ranges (start/end rva, npages,
n functions for .text), and the top rows by function count.  --min is
the smallest gap (in pages) to report.
"""
import argparse
import json
import struct

import numpy as np


def sections(exe_bytes):
    pe = struct.unpack_from("<I", exe_bytes, 0x3C)[0]
    nsec = struct.unpack_from("<H", exe_bytes, pe + 6)[0]
    opt = struct.unpack_from("<H", exe_bytes, pe + 20)[0]
    s0 = pe + 24 + opt
    out = []
    for i in range(nsec):
        o = s0 + 40 * i
        name = exe_bytes[o:o + 8].split(b"\x00")[0].decode("latin-1")
        vsz, va, rsz, ro = struct.unpack_from("<IIII", exe_bytes, o + 8)
        if vsz:
            out.append((name, va, vsz))
    return out


def pdata_ranges(exe_bytes, exe_secs):
    pdata = [(va, vsz) for (name, va, vsz) in exe_secs
             if name == ".pdata"]
    if not pdata:
        return []
    va, vsz = pdata[0]
    # raw offset for .pdata (virtual==raw for all our PE sections here,
    # but resolve properly anyway)
    rsz, ro = None, None
    pe = struct.unpack_from("<I", exe_bytes, 0x3C)[0]
    nsec = struct.unpack_from("<H", exe_bytes, pe + 6)[0]
    opt = struct.unpack_from("<H", exe_bytes, pe + 20)[0]
    s0 = pe + 24 + opt
    for i in range(nsec):
        o = s0 + 40 * i
        name = exe_bytes[o:o + 8].split(b"\x00")[0].decode("latin-1")
        vsz2, va2, rsz2, ro2 = struct.unpack_from("<IIII", exe_bytes, o + 8)
        if name == ".pdata" and rsz2:
            ro = ro2
            break
    if ro is None:
        return []
    rs = []
    raw = exe_bytes[ro:ro + vsz]
    for i in range(0, len(raw) - 12, 12):
        beg, end = struct.unpack_from("<II", raw, i)
        if beg and end > beg:
            rs.append((beg, end - beg))
    return rs


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("img")
    ap.add_argument("--exe", default=r"E:\Overwatch\_retail_\Overwatch.exe")
    ap.add_argument("--csv", default="")
    ap.add_argument("--min", type=int, default=8)
    args = ap.parse_args()
    side = json.load(open(args.img + ".json", encoding="utf-8"))
    a = np.fromfile(args.img, np.uint8)
    exe = open(args.exe, "rb").read()
    secs = sections(exe)
    pdrs = pdata_ranges(exe, secs)

    cold_rows = []
    for name, va, vsz in secs:
        p0, p1 = va >> 12, (va + vsz) >> 12
        if p1 > a.size >> 12:
            p1 = a.size >> 12
        sec = a[(p0 << 12):(p1 << 12)]
        dead = np.count_nonzero(sec.reshape(p1 - p0, 4096).any(axis=1) == 0)
        total = p1 - p0
        cold_rows.append((name, va, vsz, total, int(dead)))
    print(f"image {args.img}: {a.size >> 12} pages")
    for name, va, vsz, total, dead in cold_rows:
        print(f"  {name:8s} va {va:#010x} size {vsz:#x}: {total} pages, "
              f"{dead} cold, {dead * 100 // max(total, 1)}%")

    # contiguous cold ranges per section + .text attribution
    rows = []
    for name, va, vsz in secs:
        p0, p1 = va >> 12, (va + vsz) >> 12
        if p1 > a.size >> 12:
            p1 = a.size >> 12
        for p in range(p0, p1):
            pass
        sec = a[(p0 << 12):(p1 << 12)].reshape(-1, 4096)
        alive = sec.any(axis=1)
        run = None
        for i in range(alive.size):
            if not alive[i] and run is None:
                run = i
            elif alive[i] and run is not None:
                if i - run >= args.min:
                    beg = (p0 + run) << 12
                    end = (p0 + i) << 12
                    nf = sum(1 for b, ln in pdrs
                             if b < end and b + ln > beg)
                    rows.append((name, beg, end, i - run, nf))
                run = None
        if run is not None and alive.size - run >= args.min:
            beg = (p0 + run) << 12
            end = (p0 + alive.size) << 12
            nf = sum(1 for b, ln in pdrs if b < end and b + ln > beg)
            rows.append((name, beg, end, alive.size - run, nf))

    rows.sort(key=lambda r: -r[4])
    print(f"\ncold ranges (>= {args.min} pages), top by .text function "
          f"count:")
    for name, beg, end, np_, nf in rows[:24]:
        print(f"  {name:8s} {beg:#x}-{end:#x} ({np_} pages, "
              f"{nf} pdata ranges)")
    if args.csv:
        with open(args.csv, "w", encoding="utf-8") as fh:
            fh.write("section,start_rva,end_rva,pages,pdata_funcs\n")
            for name, beg, end, np_, nf in rows:
                fh.write(f"{name},{beg:#x},{end:#x},{np_},{nf}\n")
        print(f"\nwrote {args.csv} ({len(rows)} ranges)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

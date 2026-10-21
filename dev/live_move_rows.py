"""What actually moves while the client animates: a row-level differential.

`live_pose_rank.py` hunts *quaternion* arrays and reports `moved = 0.00` for every
candidate even with the hero gallery animating, so the live pose is probably not
four-float rotations (an earlier round already suspected matrices).  This probe
assumes nothing about the representation: for every writable region it reads a
chunk, waits, reads the same chunk again, and reports the runs of 16-byte cells
that changed *and* still look like floats.  Counters, handles and hashes are
filtered out by the float test rather than by a name.

Usage:
    python live_move_rows.py --pid N [--gap-ms 120] [--min-run 4] [--top 40]
                             [--max-mb 4096]

Read-only: OpenProcess(VM_READ | VM_QUERY), ReadProcessMemory only.
"""

from __future__ import annotations

import sys
import time

import numpy as np

import dump_process as DP

SKIP = DP.PAGE_NOACCESS | DP.PAGE_GUARD
WRITABLE = 0x04 | 0x08 | 0x40 | 0x80


def main() -> int:
    args = sys.argv[1:]

    def flag(name, default):
        return type(default)(args[args.index(name) + 1]) if name in args else default

    pid = flag("--pid", 0)
    gap_ms = flag("--gap-ms", 120)
    min_run = flag("--min-run", 4)
    top = flag("--top", 40)
    max_mb = flag("--max-mb", 4096)
    only = args[args.index("--only-range") + 1] if "--only-range" in args else ""
    lo = hi = 0
    if only:
        lo, hi = (int(x, 0) for x in only.split(","))
    if not pid:
        print(__doc__)
        return 2

    handle = DP.open_target(pid)
    chunk = 4 << 20
    budget = max_mb << 20
    scanned = 0
    chunks = 0
    total_changed = 0
    best = (0, 0)
    hits: list[tuple[int, int, bytes, bytes]] = []

    for base, size, protect, kind, _alloc in DP.regions(handle):
        if protect & SKIP or not (protect & WRITABLE):
            continue
        if size > (2 << 30):
            continue
        if only and not (lo <= base < hi):
            continue
        off = 0
        while off < size and scanned < budget:
            n = min(chunk, size - off)
            a = DP.read_region(handle, base + off, n)
            if not a:
                off += n
                continue
            time.sleep(gap_ms / 1000.0)
            b = DP.read_region(handle, base + off, n)
            if not b or len(b) != len(a):
                off += n
                continue
            scanned += len(a)
            chunks += 1
            if chunks % 200 == 0:
                print("  ... %d MB scanned, %d changed cells, %d float runs so far"
                      % (scanned >> 20, total_changed, len(hits)), flush=True)
            A = np.frombuffer(a, dtype=np.uint8).reshape(-1, 16)
            B = np.frombuffer(b, dtype=np.uint8).reshape(-1, 16)
            idx = np.flatnonzero(np.any(A != B, axis=1))
            total_changed += len(idx)
            if len(idx) >= min_run:
                runs = []
                start = prev = idx[0]
                for i in idx[1:]:
                    if i - prev <= 2:
                        prev = i
                        continue
                    runs.append((start, prev))
                    start = prev = i
                runs.append((start, prev))
                fa = np.frombuffer(a, dtype="<f4")
                for (s, e) in runs:
                    if e - s + 1 < min_run:
                        continue
                    seg = fa[s * 4:(e + 1) * 4]
                    fin = np.isfinite(seg)
                    if fin.mean() < 0.95:
                        continue
                    if np.abs(seg[fin]).max() > 1e6:
                        continue
                    hits.append((base + off + s * 16, e - s + 1,
                                 a[s * 16:(e + 1) * 16], b[s * 16:(e + 1) * 16]))
                    if e - s + 1 > best[0]:
                        best = (e - s + 1, base + off + s * 16)
                        fs = np.frombuffer(a, dtype="<f4")[s * 4:s * 4 + 12]
                        gs = np.frombuffer(b, dtype="<f4")[s * 4:s * 4 + 12]
                        print("  NEW LONGEST @0x%x  %d cells"
                              % (base + off + s * 16, e - s + 1), flush=True)
                        print("     before " + " ".join("%+.4f" % v for v in fs),
                              flush=True)
                        print("     after  " + " ".join("%+.4f" % v for v in gs),
                              flush=True)
            off += n

    hits.sort(key=lambda t: -t[1])
    print("%d MB scanned with a %d ms gap; %d cells changed, %d moving float runs"
          % (scanned >> 20, gap_ms, total_changed, len(hits)))
    for (addr, cells, sa, sb) in hits[:top]:
        fa = np.frombuffer(sa, dtype="<f4")
        fb = np.frombuffer(sb, dtype="<f4")
        print("\n@0x%x  %d cells (%d bytes)  first f32 before/after:"
              % (addr, cells, cells * 16))
        print("   before " + " ".join("%+.4f" % v for v in fa[:10]))
        print("   after  " + " ".join("%+.4f" % v for v in fb[:10]))
        # a 3x4 or 4x4 matrix read, if the run is long enough
        if cells * 16 >= 64:
            for off in (0, 16, 32, 48):
                row = fa[off // 4:off // 4 + 4]
                if len(row) == 4 and abs(float(np.linalg.norm(row[:3])) - 1.0) < 0.05:
                    print("   row at +0x%02x looks orthonormal: %s"
                          % (off, " ".join("%+.4f" % v for v in row)))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

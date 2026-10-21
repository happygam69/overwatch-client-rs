"""Find transform-shaped records with any orientation (not just yaw).

The yaw-only pattern (pattern_scan) matched standing bones and renderer
palette slots. A camera or a pitched bone has a full orthonormal basis, so
this scan accepts three mutually-orthogonal unit rows, then a position triple
repeated bit-identically 16 bytes later:

    +0x00  row0  (unit, 4th column small)
    +0x10  row1  (unit)
    +0x20  row2  (unit)
    +0x30  pos = (x, y, z, 0)
    +0x40  pos_copy == pos

Usage:
    python relaxed_scan.py [--pid N] [--out D:/ow-dumps/trace/transforms.txt]
"""

from __future__ import annotations

import sys
import time

import numpy as np

import dump_process as DP
from hunt_actor import find_pid
from vel_scan import domain


def main() -> int:
    args = sys.argv[1:]
    pid = int(args[args.index("--pid") + 1]) if "--pid" in args else find_pid()
    out = args[args.index("--out") + 1] if "--out" in args else r"D:\ow-dumps\trace\transforms.txt"
    if not pid:
        print("no Overwatch process found")
        return 1
    handle = DP.open_target(pid)
    regs = domain(handle)
    total = sum(s for _, s in regs)
    print(f"pid {pid}: {len(regs)} region(s), {total/1e9:.2f} GB")

    hits = []
    t0 = time.time()
    for base, size in regs:
        blob = DP.read_region(handle, base, size)
        if not blob or len(blob) < 128:
            continue
        n = len(blob) // 4
        f = np.frombuffer(blob[: n * 4], dtype="<f4")
        u = np.frombuffer(blob[: n * 4], dtype="<u4")
        m = n - 20
        if m <= 0:
            continue

        def F(k):
            return f[k : k + m]

        def U(k):
            return u[k : k + m]

        a0, a1, a2 = F(0), F(1), F(2)
        b0, b1, b2 = F(4), F(5), F(6)
        c0, c1, c2 = F(8), F(9), F(10)
        la = a0 * a0 + a1 * a1 + a2 * a2
        lb = b0 * b0 + b1 * b1 + b2 * b2
        lc = c0 * c0 + c1 * c1 + c2 * c2
        dab = a0 * b0 + a1 * b1 + a2 * b2
        dac = a0 * c0 + a1 * c1 + a2 * c2
        dbc = b0 * c0 + b1 * c1 + b2 * c2
        ok = (
            np.isfinite(la) & np.isfinite(lb) & np.isfinite(lc)
            & (la > 0.98) & (la < 1.02) & (lb > 0.98) & (lb < 1.02) & (lc > 0.98) & (lc < 1.02)
            & (np.abs(dab) < 0.05) & (np.abs(dac) < 0.05) & (np.abs(dbc) < 0.05)
            & (np.abs(F(3)) < 1.0) & (np.abs(F(7)) < 1.0) & (np.abs(F(11)) < 1.0)
            & (U(16) == U(12)) & (U(17) == U(13)) & (U(18) == U(14)) & (U(15) == 0)
            & np.isfinite(F(12)) & np.isfinite(F(13)) & np.isfinite(F(14))
            & (np.abs(F(12)) < 4000) & (np.abs(F(13)) < 500) & (np.abs(F(14)) < 4000)
            & ((np.abs(F(12)) > 0.0001) | (np.abs(F(14)) > 0.0001))
        )
        for i in np.nonzero(ok)[0]:
            addr = base + int(i) * 4
            hits.append((addr, float(f[i + 12]), float(f[i + 13]), float(f[i + 14])))
    print(f"{len(hits)} hit(s) in {time.time()-t0:.1f}s")
    with open(out, "w") as fh:
        for addr, x, y, z in sorted(hits):
            fh.write(f"{addr:#014x}  ({x:.4f},{y:.4f},{z:.4f})\n")
    print(f"written to {out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

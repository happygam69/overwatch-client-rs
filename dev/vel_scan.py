"""Velocity-signature scan: find the (vx, 0, vz) walk-velocity triple.

Moving at walk speed puts a 3-float velocity vector with a near-zero vertical
component and a horizontal magnitude of ~5.5 m/s somewhere in the heap; idling
zeroes it. Two full passes -- one with S+D held (slides along walls), one
idle -- and the intersection of "looks like walking speed now" with "is zero
when idle" is a very small candidate set, no shape assumption involved.

Usage:
    python vel_scan.py [--pid N] [--speed-min 4.5] [--speed-max 6.5]

READ-ONLY; sends keys like inject_movement.ps1.
"""

from __future__ import annotations

import sys
import time

import numpy as np

import dump_process as DP
from hunt_actor import find_pid, focus_window, send_key

MODULE_BASE = 0x7FF70E520000
MODULE_END = MODULE_BASE + 0x6000000


def domain(handle):
    regs = []
    for base, size, protect, kind, _alloc in DP.regions(handle):
        if protect & (DP.PAGE_NOACCESS | DP.PAGE_GUARD):
            continue
        if kind == DP.MEM_IMAGE:
            continue
        if base < MODULE_END and base + size > MODULE_BASE:
            continue
        if size < 0x1000:
            continue
        regs.append((base, size))
    return regs


def walk_hits(handle, regs, vmin, vmax):
    hits = []
    lo2, hi2 = vmin * vmin, vmax * vmax
    for base, size in regs:
        blob = DP.read_region(handle, base, size)
        if not blob or len(blob) < 64:
            continue
        nf = len(blob) // 4
        f = np.frombuffer(blob[: nf * 4], dtype="<f4")
        m = nf - 2
        if m <= 0:
            continue
        x = f[:m]
        y = f[1 : m + 1]
        z = f[2 : m + 2]
        finite = np.isfinite(x) & np.isfinite(y) & np.isfinite(z)
        h2 = x * x + z * z
        ok = finite & (np.abs(y) <= 0.05) & (h2 >= lo2) & (h2 <= hi2) \
            & (np.abs(x) <= 20) & (np.abs(z) <= 20)
        for i in np.nonzero(ok)[0]:
            hits.append((base + int(i) * 4, float(x[i]), float(y[i]), float(z[i])))
    return hits


def probe(handle, addr, width=24):
    blob = DP.read_region(handle, addr - 16, 16 + width)
    if not blob:
        return None
    f = np.frombuffer(blob[: len(blob) // 4 * 4], dtype="<f4")
    return f


def main() -> int:
    args = sys.argv[1:]
    pid = int(args[args.index("--pid") + 1]) if "--pid" in args else find_pid()
    vmin = float(args[args.index("--speed-min") + 1]) if "--speed-min" in args else 4.5
    vmax = float(args[args.index("--speed-max") + 1]) if "--speed-max" in args else 6.5
    if not pid:
        print("no Overwatch process found")
        return 1
    handle = DP.open_target(pid)
    regs = domain(handle)
    total = sum(s for _, s in regs)
    print(f"pid {pid}: {len(regs)} region(s), {total/1e9:.2f} GB")

    if not focus_window(pid):
        print("WARNING: focus not confirmed; keys may not land")

    send_key("s", True)
    send_key("d", True)
    t0 = time.time()
    moving = walk_hits(handle, regs, vmin, vmax)
    t1 = time.time()
    print(f"moving pass: {len(moving)} candidate(s) in {t1-t0:.1f}s (S+D held)")

    send_key("s", False)
    send_key("d", False)
    time.sleep(0.6)
    idle = walk_hits(handle, regs, vmin, vmax)
    t2 = time.time()
    print(f"idle pass: {len(idle)} candidate(s) in {t2-t1:.1f}s")

    # idle-zero test for the moving candidates
    print("\nmoving candidates whose value is ~0 (or absent) when idle:")
    confirmed = []
    for addr, x, y, z in moving:
        f = probe(handle, addr)
        if f is None:
            continue
        v = f[4:7]
        sp = float(np.hypot(v[0], v[2]))
        if np.all(np.abs(v) < 0.6):
            confirmed.append((addr, (x, y, z), tuple(float(t) for t in v)))
    for addr, mv, iv in confirmed[:60]:
        print(f"  {addr:#014x}  move ({mv[0]:8.3f},{mv[1]:8.3f},{mv[2]:8.3f})  ->  idle ({iv[0]:8.3f},{iv[1]:8.3f},{iv[2]:8.3f})")
    print(f"\n{len(confirmed)} confirmed of {len(moving)}")

    # also: values that are near-zero in the idle pass but walking-shaped
    # during movement are the same thing seen from the other side; report the
    # raw neighborhood of the first few confirmed hits
    for addr, mv, iv in confirmed[:5]:
        f = probe(handle, addr, 48)
        if f is None:
            continue
        ctx = " ".join(f"{v:8.3f}" for v in f[:16])
        print(f"\ncontext {addr:#014x}: {ctx}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

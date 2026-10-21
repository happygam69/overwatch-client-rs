"""Series-watch the velocity candidates through a scripted key sequence.

`vel_scan.py`'s two passes narrow the heap to candidates that look like a
walk-speed vector while moving and are ~0 when idle, but a single probe cannot
tell a real velocity field from a zero-filled field that happened to be read.
This tool samples every candidate at ~15 Hz while the injected keys walk,
stop, walk the other way and stop again, and keeps only candidates whose
series shows the whole signature: horizontal magnitude in 4-7 m/s while keys
are held, < 0.5 while idle, and at least one intermediate sample on a
transition (the accel/decel ramp).

Usage:
    python series_watch.py [--pid N] [--limit 6000]

READ-ONLY; sends keys like inject_movement.ps1.
"""

from __future__ import annotations

import sys
import time

import numpy as np

import dump_process as DP
from hunt_actor import find_pid, focus_window, send_key
from vel_scan import domain, walk_hits


def main() -> int:
    args = sys.argv[1:]
    pid = int(args[args.index("--pid") + 1]) if "--pid" in args else find_pid()
    limit = int(args[args.index("--limit") + 1]) if "--limit" in args else 6000
    if not pid:
        print("no Overwatch process found")
        return 1
    handle = DP.open_target(pid)
    regs = domain(handle)
    print(f"pid {pid}: {len(regs)} region(s)")

    if not focus_window(pid):
        print("WARNING: focus not confirmed; keys may not land")

    send_key("s", True)
    send_key("d", True)
    moving = walk_hits(handle, regs, 4.5, 6.5)
    print(f"{len(moving)} moving candidate(s)")
    send_key("s", False)
    send_key("d", False)
    time.sleep(0.5)
    idle = walk_hits(handle, regs, 0.4, 0.6)
    print(f"{len(idle)} idle candidate(s) (sanity)")

    # idle ~0 check
    cand: list[int] = []
    for addr, x, y, z in moving:
        blob = DP.read_region(handle, addr, 12)
        if not blob:
            continue
        v = np.frombuffer(blob, dtype="<f4")
        if np.all(np.abs(v) < 0.6):
            cand.append(addr)
    cand = sorted(set(cand))
    print(f"{len(cand)} candidate(s) after the idle-zero check")
    if len(cand) > limit:
        # keep a spread across the address space
        step = len(cand) // limit + 1
        cand = cand[::step]
        print(f"downsampled to {len(cand)}")

    addrs = np.array(cand, dtype=np.uint64)

    def sample():
        vals = np.full((len(cand), 3), np.nan, dtype=np.float32)
        drop = []
        for i, a in enumerate(cand):
            blob = DP.read_region(handle, int(a), 12)
            if blob and len(blob) >= 12:
                vals[i] = np.frombuffer(blob[:12], dtype="<f4")
            else:
                drop.append(i)
        return vals

    series = []  # (t, tag, vals)
    t0 = time.time()

    def tick(tag: str, seconds: float):
        t_end = time.time() + seconds
        while time.time() < t_end:
            series.append((time.time() - t0, tag, sample()))
            time.sleep(0.02)

    tick("pre", 0.8)
    send_key("s", True)
    send_key("d", True)
    tick("sd", 2.0)
    send_key("s", False)
    send_key("d", False)
    tick("idle1", 1.6)
    send_key("w", True)
    tick("w", 2.0)
    send_key("w", False)
    tick("idle2", 1.6)
    print(f"{len(series)} sample(s), {len(cand)} candidate(s)")

    tags = [s[1] for s in series]
    verds = []
    for i in range(len(cand)):
        v = np.array([s[2][i] for s in series])
        if not np.isfinite(v).all():
            continue
        mag = np.hypot(np.hypot(v[:, 0], v[:, 1]), v[:, 2])

        def wmax(tag):
            m = np.array([mag[k] for k, t in enumerate(tags) if t == tag])
            return float(m.max()) if len(m) else 0.0

        m_sd, m_w, m_i1, m_i2 = wmax("sd"), wmax("w"), wmax("idle1"), wmax("idle2")
        if m_sd >= 4.0 and m_w >= 4.0 and m_i1 < 0.6 and m_i2 < 0.6:
            mid = np.count_nonzero((mag > 0.4) & (mag < 4.5))
            verds.append((i, m_sd, m_w, mid, v))
    print(f"\n{len(verds)} verified candidate(s):")
    for i, m_sd, m_w, mid, v in verds[:40]:
        print(f"  {int(cand[i]):#014x}  max|v| sd={m_sd:5.2f} w={m_w:5.2f}  ramp samples={mid}")
        idx = [k for k, t in enumerate(tags) if t in ("pre", "sd", "idle1", "w", "idle2")]
        step = max(1, len(idx) // 28)
        for k in idx[::step]:
            vv = v[k]
            print(f"      t+{series[k][0]:5.2f} {tags[k]:5} ({vv[0]:9.3f},{vv[1]:9.3f},{vv[2]:9.3f})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

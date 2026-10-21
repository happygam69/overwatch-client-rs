"""Ramp detector v2: long idle-constancy, rail exclusion, two walk directions.

v1 caught the moving range targets because a 0.5 s idle check is shorter than
their period. v2 requires every candidate offset to be bit-identical across
five idle samples over 2.5 s, skips the target/rail signature
(first two floats exactly (0, +-1)), and walks twice with different keys so a
true player-position hit ramps in both directions while a periodic mover of
the map cannot pass the idle gate at all.

Usage:
    python family_ramp2.py [--pid N]
"""

from __future__ import annotations

import sys
import time

import numpy as np

import dump_process as DP
from hunt_actor import find_pid, focus_window, send_key

FAMILIES = [
    (0x024080000000, 0x10000000),
    (0x00fda0000000, 0x0010000000),
]


def main() -> int:
    args = sys.argv[1:]
    pid = int(args[args.index("--pid") + 1]) if "--pid" in args else find_pid()
    if not pid:
        print("no Overwatch process found")
        return 1
    handle = DP.open_target(pid)

    regs = []
    for base, size, protect, kind, _alloc in DP.regions(handle):
        if protect & (DP.PAGE_NOACCESS | DP.PAGE_GUARD):
            continue
        for fb, fs in FAMILIES:
            lo = max(base, fb)
            hi = min(base + size, fb + fs)
            if lo < hi:
                regs.append((lo, hi - lo))
    total = sum(s for _, s in regs)
    print(f"pid {pid}: {len(regs)} region(s), {total / 1e6:.0f} MB")

    if not focus_window(pid):
        print("WARNING: focus not confirmed; keys may not land")

    def readall():
        return [DP.read_region(handle, b, s) for b, s in regs]

    t0 = time.time()
    idle = []
    for k in range(5):
        idle.append(readall())
        time.sleep(0.6)
    print(f"idle reads done at t+{time.time()-t0:.1f}s")

    def walk(keys, seconds, n):
        for k in keys:
            send_key(k, True)
        rs = []
        for _ in range(n):
            rs.append((time.time() - t0, readall()))
            time.sleep(seconds)
        for k in keys:
            send_key(k, False)
        return rs

    print("walk 1: S+D")
    w1 = walk(["s", "d"], 0.5, 4)
    print("walk 2: W")
    w2 = walk(["w"], 0.5, 4)
    print(f"walks done at t+{time.time()-t0:.1f}s")

    def const_mask(samples, k):
        # not bit-exact: breathing animations keep a live record oscillating
        # within ~0.1 m; the walk phase then has to show direction-consistent
        # 5.5 m/s ramps, which breathing cannot fake
        na = min(len(s) for s in samples if s[k]) // 4
        if na <= 0:
            return None, None
        ref = samples[0][k]
        f0 = np.frombuffer(ref[: na * 4], dtype="<f4")
        dev = np.zeros(na, dtype=np.float32)
        for s in samples[1:]:
            if not s[k]:
                return None, None
            f = np.frombuffer(s[k][: na * 4], dtype="<f4")
            dev = np.maximum(dev, np.abs(f - f0))
        m = dev <= 0.12
        return m, None

    RAIL_ADDRS = [
        0x0240813cd5e8, 0x0240813cd688, 0x0240813cd908,
        0x0240823c1998, 0x0240871490d0, 0x024087149520, 0x024087149690,
        0x02408714c3d0, 0x02408714c820, 0x02408714c990,
    ]

    HITS = {}
    for k, (b, s) in enumerate(regs):
        m, _ = const_mask(idle, k)
        if m is None:
            continue
        nf = len(m)
        fref = np.frombuffer(idle[0][k][: nf * 4], dtype="<f4")
        for walk_idx, (name, rs) in enumerate((("sd", w1), ("w", w2))):
            rows = []
            for t, parts in rs:
                p = parts[k]
                if not p or len(p) < nf * 4:
                    rows = []
                    break
                rows.append((t, np.frombuffer(p[: nf * 4], dtype="<f4")))
            if not rows:
                continue
            for i in np.nonzero(m[: nf - 2])[0]:
                # rail/target signature: first two floats exactly 0, +-1
                if fref[i] == 0.0 and abs(fref[i + 1]) == 1.0:
                    continue
                addr = b + i * 4
                if any(abs(addr - ra) < 0x200 for ra in RAIL_ADDRS):
                    continue
                ok = True
                dirs = []
                for (ta, fa), (tb, fb) in zip(rows, rows[1:]):
                    xa, ya, za = float(fa[i]), float(fa[i + 1]), float(fa[i + 2])
                    xb, yb, zb = float(fb[i]), float(fb[i + 1]), float(fb[i + 2])
                    if not (np.isfinite(xa) and np.isfinite(xb) and np.isfinite(ya)
                            and np.isfinite(yb) and np.isfinite(za) and np.isfinite(zb)):
                        ok = False
                        break
                    dt = tb - ta
                    dxx, dzz = xb - xa, zb - za
                    dh = (dxx * dxx + dzz * dzz) ** 0.5
                    exp = 5.5 * dt
                    if not (0.5 * exp <= dh <= 1.5 * exp):
                        ok = False
                        break
                    if abs(yb - ya) > 0.6:
                        ok = False
                        break
                    if abs(xa) > 4000 or abs(za) > 4000:
                        ok = False
                        break
                    dirs.append((dxx / dh, dzz / dh))
                if ok and len(dirs) >= 2:
                    d0 = dirs[0]
                    for d in dirs[1:]:
                        if d0[0] * d[0] + d0[1] * d[1] < 0.9:
                            ok = False
                            break
                if ok:
                    HITS[addr] = "both" if addr in HITS and HITS[addr] != name else name
                    break
    print(f"\n{len(HITS)} ramp hit(s) after idle gate and rail exclusion:")
    for addr, name in sorted(HITS.items())[:80]:
        blob = DP.read_region(handle, addr, 12)
        cur = tuple(float(v) for v in np.frombuffer(blob, dtype="<f4")) if blob else None
        print(f"  {addr:#014x}  [{name:4}]  now {cur}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

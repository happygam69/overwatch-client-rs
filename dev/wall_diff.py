"""Wall-pinned input diff: heap offsets that only change under a held key.

With the player pinned against a wall facing it, holding W attempts movement
while the rendered view stays frozen, so renderer/streaming churn stops. The
tool scans the whole heap twice (idle, then W held), recording per region the
offsets whose bytes changed between the region's back-to-back reads, and
reports the offsets that changed only in the W scan. Those are the
input-to-velocity path's live fields.

Usage:
    python wall_diff.py [--pid N] [--pin-secs 8]
"""

from __future__ import annotations

import ctypes
import sys
import time

import numpy as np

import dump_process as DP
from hunt_actor import find_pid, focus_window, send_key
from vel_scan import domain

MODULE_BASE = 0x7FF70E520000
MODULE_END = MODULE_BASE + 0x6000000


def scan(handle, regs) -> dict[int, np.ndarray]:
    out: dict[int, np.ndarray] = {}
    for base, size in regs:
        a = DP.read_region(handle, base, size)
        b = DP.read_region(handle, base, size)
        if not a or not b or len(a) != len(b) or len(a) < 64:
            continue
        n = len(a) // 4
        ua = np.frombuffer(a[: n * 4], dtype="<u4")
        ub = np.frombuffer(b[: n * 4], dtype="<u4")
        diff = ua != ub
        cnt = int(np.count_nonzero(diff))
        if cnt == 0 or cnt > 200_000:
            continue
        out[base] = np.nonzero(diff)[0].astype(np.int64)
    return out


def main() -> int:
    args = sys.argv[1:]
    pid = int(args[args.index("--pid") + 1]) if "--pid" in args else find_pid()
    pin = float(args[args.index("--pin-secs") + 1]) if "--pin-secs" in args else 8.0
    if not pid:
        print("no Overwatch process found")
        return 1
    handle = DP.open_target(pid)
    regs = domain(handle)
    print(f"pid {pid}: {len(regs)} region(s)")

    if not focus_window(pid):
        print("WARNING: focus not confirmed; keys may not land")

    print(f"pinning against the wall ({pin:.0f}s of W)...")
    send_key("w", True)
    time.sleep(pin)
    # keep W held for the whole session; the player stays pinned

    t0 = time.time()
    b_scan = scan(handle, regs)
    print(f"W-held scan done t+{time.time()-t0:.0f}s: {len(b_scan)} region(s) with changes")

    # release W briefly for the idle baseline
    send_key("w", False)
    time.sleep(1.0)
    t1 = time.time()
    a_scan = scan(handle, regs)
    print(f"idle scan done t+{time.time()-t1:.0f}s: {len(a_scan)} region(s) with changes")

    only_b = []
    for base, offs in b_scan.items():
        a_offs = a_scan.get(base)
        if a_offs is None:
            only_b.append((base, offs))
            continue
        common = np.intersect1d(offs, a_offs, assume_unique=True)
        rest = np.setdiff1d(offs, common, assume_unique=True)
        if len(rest):
            only_b.append((base, rest))
    total = sum(len(o) for _, o in only_b)
    print(f"\n{total} offset(s) changed only while W was held:")
    shown = 0
    for base, offs in sorted(only_b, key=lambda p: -len(p[1])):
        if shown >= 400:
            break
        blob = DP.read_region(handle, base + int(offs[0]) - 16, 48) if len(offs) else None
        f = np.frombuffer(blob, dtype="<f4") if blob else None
        vals = ""
        if f is not None and len(f) >= 12:
            vals = " ctx " + " ".join(f"{v:9.3f}" for v in f[4:10])
        addrs = ", ".join(f"{base + int(o)*4:#x}" for o in offs[:6])
        print(f"  {base:#014x}: {len(offs):6} offset(s)  first: {addrs}{vals}")
        shown += len(offs)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

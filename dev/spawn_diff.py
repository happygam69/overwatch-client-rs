"""Spawn-determinism diff: locate player fields by re-entry equality.

The allocator is deterministic across practice-range sessions in one process:
after re-entry the same 224 movers exist at identical addresses. So with the
player at a fresh spawn (S1), walked away (S2), and at a fresh spawn again
(S3), the player's own state fields are exactly those that differ between S1
and S2 and are bit-identical between S1 and S3. No motion model, no value
shapes, no ambient-motion confusion -- bots and rails fail the S3==S1 test.

Phases (run in the background; CUA does the leave/re-enter in the wait):
  1. snapshot S1 (in memory), hold W 6 s, snapshot S2 -> mask2 = S2 != S1
  2. print "WAITING"; sleep --wait seconds for the manual re-entry
  3. snapshot S3 -> report mask2 & (S3 == S1) with float context

Usage:
    python spawn_diff.py [--pid N] [--wait 150]
"""

from __future__ import annotations

import sys
import time

import numpy as np

import dump_process as DP
from hunt_actor import find_pid, focus_window, send_key

FAMILIES = [
    (0x024080000000, 0x20000000),
    (0x024120000000, 0x40000000),
    (0x024140000000, 0x20000000),
    (0x00fda0000000, 0x0010000000),
]


def regions_in(handle):
    regs = []
    for base, size, protect, kind, _alloc in DP.regions(handle):
        if protect & (DP.PAGE_NOACCESS | DP.PAGE_GUARD):
            continue
        if kind == DP.MEM_IMAGE:
            continue
        for fb, fs in FAMILIES:
            lo = max(base, fb)
            hi = min(base + size, fb + fs)
            if lo < hi:
                regs.append((lo, hi - lo))
    return regs


def main() -> int:
    args = sys.argv[1:]
    pid = int(args[args.index("--pid") + 1]) if "--pid" in args else find_pid()
    wait = float(args[args.index("--wait") + 1]) if "--wait" in args else 150.0
    if not pid:
        print("no Overwatch process found")
        return 1
    handle = DP.open_target(pid)
    regs = regions_in(handle)
    total = sum(s for _, s in regs)
    print(f"pid {pid}: {len(regs)} region(s), {total/1e6:.0f} MB", flush=True)

    if not focus_window(pid):
        print("WARNING: focus not confirmed; keys may not land", flush=True)

    s1 = {}
    for base, size in regs:
        b = DP.read_region(handle, base, size)
        if b:
            s1[base] = b
    print("S1 taken (spawn)", flush=True)

    print("walking W 6.5 s...", flush=True)
    send_key("w", True)
    time.sleep(6.5)
    send_key("w", False)
    time.sleep(0.6)

    mask2 = {}
    for base, size in regs:
        b = DP.read_region(handle, base, size)
        a = s1.get(base)
        if not b or not a or len(a) != len(b):
            continue
        n = len(a) // 4
        ua = np.frombuffer(a[: n * 4], dtype="<u4")
        ub = np.frombuffer(b[: n * 4], dtype="<u4")
        d = np.nonzero(ua != ub)[0]
        if len(d):
            mask2[base] = d.astype(np.int64)
    tot2 = sum(len(v) for v in mask2.values())
    print(f"S2 taken (walked): {tot2} changed offset(s)", flush=True)
    print("WAITING for leave/re-enter...", flush=True)

    time.sleep(wait)
    print("resuming; taking S3", flush=True)
    time.sleep(3)

    hits = []
    for base, offs in mask2.items():
        b = DP.read_region(handle, base, int(offs[-1]) * 4 + 4)
        a = s1.get(base)
        if not b or not a:
            continue
        for o in offs:
            p = int(o) * 4
            if p + 4 > len(b):
                continue
            if b[p : p + 4] == a[p : p + 4]:
                hits.append(base + p)
    print(f"\n{len(hits)} offset(s) walked-away AND restored at spawn:", flush=True)
    for a in hits[:300]:
        base = None
        for b_, s_ in regs:
            if b_ <= a < b_ + s_:
                base = b_
                break
        blob = DP.read_region(handle, a - 16, 44)
        ctx = ""
        if blob and len(blob) >= 40:
            f = np.frombuffer(blob, dtype="<f4")
            ctx = " ".join(f"{v:9.3f}" for v in f[4:10])
        u = DP.read_region(handle, a, 4)
        print(f"  {a:#014x}  {u.hex() if u else '??'}   {ctx}", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

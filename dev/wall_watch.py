"""Scan for W-only changing offsets, then behaviorally verify each candidate.

Stage 1 (as `wall_diff.py`): two full-heap passes, one idle and one with W
held while the player is pinned against a wall; offsets that change only in
the W pass are candidates.
Stage 2: sample every candidate at ~10 Hz through pre-idle / W-held / post-
idle and keep only those whose bytes change during the W window and not in
either idle window -- the input-path signature, immune to ambient churn.

Usage:
    python wall_watch.py [--pid N] [--pin-secs 6]
"""

from __future__ import annotations

import json
import sys
import time

import numpy as np

import dump_process as DP
from hunt_actor import find_pid, focus_window, send_key
from vel_scan import domain
from wall_diff import scan


def main() -> int:
    args = sys.argv[1:]
    pid = int(args[args.index("--pid") + 1]) if "--pid" in args else find_pid()
    pin = float(args[args.index("--pin-secs") + 1]) if "--pin-secs" in args else 6.0
    if not pid:
        print("no Overwatch process found")
        return 1
    handle = DP.open_target(pid)
    regs = domain(handle)
    print(f"pid {pid}: {len(regs)} region(s)")

    if not focus_window(pid):
        print("WARNING: focus not confirmed; keys may not land")

    print(f"pinning ({pin:.0f}s W)...")
    send_key("w", True)
    time.sleep(pin)

    b_scan = scan(handle, regs)
    print(f"W scan: {len(b_scan)} region(s) with changes")
    send_key("w", False)
    time.sleep(0.8)
    a_scan = scan(handle, regs)
    print(f"idle scan: {len(a_scan)} region(s) with changes")

    cand: list[int] = []
    for base, offs in b_scan.items():
        a_offs = a_scan.get(base)
        if a_offs is None:
            rest = offs
        else:
            rest = np.setdiff1d(offs, a_offs, assume_unique=True)
        for o in rest:
            cand.append(base + int(o) * 4)
    cand = sorted(set(cand))
    print(f"{len(cand)} candidate offset(s)")

    with open(r"D:\ow-dumps\trace\wall_cands.json", "w") as fh:
        json.dump(cand, fh)

    # stage 2: behavioral watch
    series = []
    rows = np.full((len(cand), 4), 0, dtype=np.uint32) if cand else None
    t0 = time.time()

    def sample():
        for k, a in enumerate(cand):
            b = DP.read_region(handle, a, 4)
            rows[k, 0] = int.from_bytes(b, "little") if b and len(b) == 4 else 0xFEFEFEFE

    def tick(tag, secs):
        te = time.time() + secs
        while time.time() < te:
            sample()
            series.append((tag, rows.copy()))
            time.sleep(0.08)

    tick("pre", 1.2)
    send_key("w", True)
    tick("w", 2.0)
    send_key("w", False)
    tick("post", 1.2)

    tags = [s[0] for s in series]
    verdict = []
    for k in range(len(cand)):
        vals = np.array([s[1][k, 0] for s in series])
        pre = vals[: sum(1 for t in tags if t == "pre")]
        w = vals[len(pre) : len(pre) + sum(1 for t in tags if t == "w")]
        post = vals[len(pre) + len(w) :]
        pre_ch = len(set(pre.tolist()))
        w_ch = len(set(w.tolist()))
        post_ch = len(set(post.tolist()))
        if w_ch >= 2 and pre_ch == 1 and post_ch == 1:
            verdict.append((cand[k], pre_ch, w_ch, post_ch, int(pre[0]), int(w[0]), int(w[-1])))
    print(f"\n{len(verdict)} behaviorally verified (change only under W):")
    for a, pc, wc, poc, v0, vw0, vw1 in verdict[:80]:
        blob = DP.read_region(handle, a - 16, 40)
        f = np.frombuffer(blob, dtype="<f4") if blob else None
        ctx = " ".join(f"{v:9.3f}" for v in f[4:10]) if f is not None and len(f) >= 10 else ""
        print(f"  {a:#014x}  pre={pc} w={wc} post={poc}  {v0:#010x} -> {vw0:#010x} .. {vw1:#010x}   {ctx}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

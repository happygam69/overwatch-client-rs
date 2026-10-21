"""Series-watch all actor transforms through a scripted key sequence.

`pattern_scan.py` lists every yaw-basis + duplicated-position structure in the
heap (114 hits, including per-frame copies). This tool samples all of them at
~40 Hz while the injected keys script: idle, S+D, idle, W, idle. The player's
transform is the one that is still in every idle window and advances at walk
speed in both key windows.

Usage:
    python actor_watch.py [--pid N] [--seconds 3]
"""

from __future__ import annotations

import sys
import time

import numpy as np

import dump_process as DP
from hunt_actor import find_pid, focus_window, send_key


def load_actors(path: str) -> list[int]:
    out = []
    with open(path) as f:
        for line in f:
            parts = line.split()
            if parts and parts[0].startswith("0x"):
                out.append(int(parts[0], 16))
    return out


def main() -> int:
    args = sys.argv[1:]
    pid = int(args[args.index("--pid") + 1]) if "--pid" in args else find_pid()
    secs = float(args[args.index("--seconds") + 1]) if "--seconds" in args else 3.0
    path = args[args.index("--actors") + 1] if "--actors" in args else r"D:\ow-dumps\trace\actors.txt"
    if not pid:
        print("no Overwatch process found")
        return 1
    handle = DP.open_target(pid)
    addrs = load_actors(path)
    print(f"pid {pid}: {len(addrs)} actor transform(s)")

    if not focus_window(pid):
        print("WARNING: focus not confirmed; keys may not land")

    series: list[tuple[float, str, dict[int, tuple]]] = []
    t0 = time.time()

    def sample():
        vals = {}
        for a in addrs:
            blob = DP.read_region(handle, a + 0x30, 12)
            if blob and len(blob) == 12:
                v = np.frombuffer(blob, dtype="<f4")
                vals[a] = (float(v[0]), float(v[1]), float(v[2]))
        return vals

    def tick(tag: str, seconds: float):
        t_end = time.time() + seconds
        while time.time() < t_end:
            series.append((time.time() - t0, tag, sample()))
            time.sleep(0.025)

    tick("pre", 1.2)
    send_key("s", True)
    send_key("d", True)
    tick("sd", secs)
    send_key("s", False)
    send_key("d", False)
    tick("idle1", 1.2)
    send_key("w", True)
    tick("w", secs)
    send_key("w", False)
    tick("idle2", 1.2)
    print(f"{len(series)} sample(s)")

    def phase(tag):
        return [(t, v) for t, tg, v in series if tg == tag]

    results = []
    for a in addrs:
        row = {}
        for tag in ("pre", "sd", "idle1", "w", "idle2"):
            ph = phase(tag)
            pts = [v[a] for _, v in ph if a in v]
            if len(pts) < 2:
                row[tag] = None
                continue
            p0, p1 = np.array(pts[0]), np.array(pts[-1])
            d = float(np.hypot(p1[0] - p0[0], p1[2] - p0[2]))
            row[tag] = d
        results.append((a, row))

    print("\ncandidates moving only when keys are held:")
    n = 0
    for a, row in results:
        if None in row.values():
            continue
        d_pre, d_sd, d_i1, d_w, d_i2 = row["pre"], row["sd"], row["idle1"], row["w"], row["idle2"]
        if d_sd > 3.0 and d_w > 3.0 and d_i1 < 0.3 and d_i2 < 0.3 and d_pre < 0.3:
            n += 1
            pts = [(t, v[a]) for t, tg, v in series if a in v]
            p_first = pts[0][1]
            p_last = pts[-1][1]
            print(f"  {a:#014x}  pre={d_pre:5.2f} sd={d_sd:6.2f} idle1={d_i1:5.2f} w={d_w:6.2f} idle2={d_i2:5.2f}")
            for t, v in pts[:: max(1, len(pts) // 20)]:
                print(f"      t+{t:5.2f}  ({v[0]:9.3f},{v[1]:8.3f},{v[2]:9.3f})")
    if not n:
        print("  (none) — dumping per-phase displacements of the top movers instead:")
        ranked = sorted([r for _, r in results if None not in r.values()],
                        key=lambda r: -(r["sd"] + r["w"]))
        for row in ranked[:8]:
            print(f"  pre={row['pre']:6.2f} sd={row['sd']:7.2f} idle1={row['idle1']:6.2f} "
                  f"w={row['w']:7.2f} idle2={row['idle2']:6.2f}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

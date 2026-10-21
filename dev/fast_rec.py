"""High-rate ramp capture: 200 Hz samples of the player node on flat ground.

The node's position updates are the client's own mover ticks; sampling at
200 Hz resolves them individually (update instants + per-update step), which
is what the acceleration fit needs. The key timeline written to the file is
the script's own (authoritative), with GetAsyncKeyState recorded alongside as
a cross-check.

Usage:
    python fast_rec.py [--pid N] [--out D:/ow-dumps/trace/player_fast.jsonl]
"""

from __future__ import annotations

import json
import struct
import sys
import time

import numpy as np

import dump_process as DP
from hunt_actor import find_pid, focus_window, send_key
from live_trace import read_keys

NODE = 0x0241DDB61880
POS_AT = 0xD0
READ_BASE = NODE + 0x40
BLOCK = 0x100  # pos at NODE+0x50 (=0xd0 of the 0x1800-based record)


def main() -> int:
    args = sys.argv[1:]
    pid = int(args[args.index("--pid") + 1]) if "--pid" in args else find_pid()
    out_path = args[args.index("--out") + 1] if "--out" in args else r"D:\ow-dumps\trace\player_fast.jsonl"
    if not pid:
        print("no Overwatch process found")
        return 1
    handle = DP.open_target(pid)
    if not focus_window(pid):
        print("WARNING: focus not confirmed; keys may not land")

    script = [
        ([], 2.0),
        (["w"], 1.0), ([], 1.0),
        (["w"], 1.0), ([], 0.8),
        (["s"], 1.0), ([], 0.8),
        (["d"], 1.0), ([], 0.8),
        (["a"], 1.0), ([], 0.8),
        (["w"], 0.5), (["w", "d"], 0.5), ([], 0.8),
    ]

    out = open(out_path, "w")
    out.write(json.dumps({"trace": 1, "kind": "playerFast", "hz": 200.0,
                          "node": hex(NODE),
                          "started": time.strftime("%Y-%m-%d %H:%M:%S")}) + "\n")

    n = 0
    t0 = time.perf_counter()
    for keys, secs in script:
        for k in keys:
            send_key(k, True)
        t_end = time.perf_counter() + secs
        while time.perf_counter() < t_end:
            blob = DP.read_region(handle, READ_BASE, BLOCK)
            rec = {"t": round(time.perf_counter() - t0, 5),
                   "held": keys,
                   "phys": read_keys()}
            if blob and len(blob) == BLOCK:
                arr = np.frombuffer(blob, dtype="<u4")
                rec["u"] = [int(v) for v in arr]
            out.write(json.dumps(rec) + "\n")
            n += 1
            time.sleep(0.004)
        for k in keys:
            send_key(k, False)
    out.close()
    dur = time.perf_counter() - t0
    print(f"{n} sample(s) in {dur:.1f}s ({n/dur:.0f} Hz) -> {out_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Generate a synthetic crouch/air trace to exercise the probe and the differ.

NOT a client capture.  This builds a node-format trace (the shape
`live_trace.py node` writes) from the *model's* constants, so that
`ctrl_probe.py`'s crouch and air-control branches — which have never run,
because no client trace holds `ctrl` (verified: all six existing captures
lack it) — are exercised offline before the operator's capture depends on
them.  It also lets the differ's per-segment coverage be seen on crouch and
air segments.

Timeline, shaped like `inject_movement.ps1`'s schedule: idle, walk, crouch walk
with both crouch transitions (ctrl toggled while W stays held), crouch from
standstill then crouch-walk from rest, a moving jump, and a standing jump with
W pressed mid-air.  Speeds, ramps, gravity and impulse are
`ow-sim/src/movement.rs`'s measured values, so the differ should find no
divergence — agreement with the *model* is the point, and none of it is
evidence about the client.

Usage: python dev/synth_ctrl_trace.py [out.jsonl]
"""

from __future__ import annotations

import json
import math
import struct
import sys

DT = 0.032309          # the measured client frame dt
ACCEL = 121.4
FRICTION = 72.6
GRAVITY = 17.37
JUMP = 5.33
WALK = 5.5
CROUCH = 3.0
YAW = 189.5668         # degrees, the flat trace's
START = (53.897, 0.711, 20.094)

KT = ("w", "a", "s", "d", "space", "ctrl", "shift", "lmb", "rmb")


def bits(v: float) -> int:
    return struct.unpack("<I", struct.pack("<f", v))[0]


def move_toward_vec(v, target, limit):
    dx, dz = target[0] - v[0], target[1] - v[1]
    n = math.hypot(dx, dz)
    if n <= limit or n == 0.0:
        return target
    s = limit / n
    return (v[0] + dx * s, v[1] + dz * s)


def main() -> int:
    out_path = sys.argv[1] if len(sys.argv) > 1 else "D:/ow-dumps/trace/synth_ctrl.jsonl"
    yaw = math.radians(YAW)
    fwd = (math.sin(yaw), math.cos(yaw))

    # (keys held, duration s, space pulsed at the block's first frame)
    blocks = [
        ({}, 1.0, False),
        ({"w": 1}, 2.0, False),                       # walk
        ({"w": 1, "ctrl": 1}, 2.0, False),            # crouch walk (5.5 -> 3.0)
        ({"ctrl": 1}, 1.0, False),                    # crouch, stopped
        ({}, 1.0, False),
        ({"ctrl": 1}, 1.0, False),                    # crouch from standstill
        ({"w": 1, "ctrl": 1}, 2.0, False),            # crouch walk from rest (0 -> 3.0)
        ({"w": 1}, 1.5, False),                       # ctrl released, W kept: crouch -> walk
        ({}, 1.0, False),
        ({"w": 1}, 0.5, False),                       # run-up...
        ({"w": 1, "space": 1}, 0.1, True),            # ...then a moving jump
        ({"w": 1}, 2.4, False),
        ({}, 1.0, False),
        ({"space": 1}, 0.15, True),                   # standing jump...
        ({"w": 1}, 1.85, False),                      # ...W pressed mid-air
        ({}, 1.0, False),
    ]

    x, z = START[0], START[2]
    y, vy, grounded = START[1], 0.0, True
    v = (0.0, 0.0)
    samples = []
    t = 0.0
    for keys, dur, jump in blocks:
        n = max(1, round(dur / DT))
        for k in range(n):
            if jump and k == 0 and grounded:
                vy = JUMP
                grounded = False
            if grounded:  # airborne the horizontal is preserved (the sim's rule)
                speed = CROUCH if keys.get("ctrl") else WALK
                target = (fwd[0] * speed, fwd[1] * speed) if keys.get("w") else (0.0, 0.0)
                limit = (ACCEL if keys.get("w") else FRICTION) * DT
                v = move_toward_vec(v, target, limit)
            x += v[0] * DT
            z += v[1] * DT
            if not grounded:
                vy -= GRAVITY * DT
                y += vy * DT
                if y <= START[1]:
                    y, vy, grounded = START[1], 0.0, True
            rec = {
                "i": len(samples),
                "t": round(t, 6),
                "pos": [bits(x), bits(y), bits(z), 0],
                "vel": [0, 0, 0, 0],
                "yaw": round(YAW, 4),
                "keys": {f: keys.get(f, 0) for f in KT},
            }
            samples.append(rec)
            t += DT

    with open(out_path, "w") as out:
        out.write(json.dumps({
            "trace": 1, "kind": "dmBody", "addr": "node_0xsynth_pos_feet",
            "hz": round(1.0 / DT, 4), "keys": 1,
        }, separators=(",", ":")) + "\n")
        for rec in samples:
            out.write(json.dumps(rec, separators=(",", ":")) + "\n")
    print(f"{len(samples)} synthetic samples -> {out_path}  "
          f"(a fixture, not a client capture)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

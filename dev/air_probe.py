#!/usr/bin/env python3
"""Measure the airborne horizontal acceleration from a keyed trace.

`ctrl_probe.py` prints the airborne *steps*; this fits the *rate*.  A grounded
walk on a slope also moves vertically, so runs are selected by the ballistic
law: real airborne frames change their per-frame dy by g*dt^2 (~0.0048 m at
60 fps and 17.37 m/s^2), while a slope walk's dy is constant.

The measured rule (2026-10-15, `player_ctrl.jsonl`): with a direction held in
the air the per-frame horizontal step grows by AIR_ACCEL * dt^2 until it holds
at the slot speed; with no direction held the step is bit-identical.  Runs
whose increments are much smaller than the full ramp come from a copy that
lags the body, or from a run the world blocked part-way, so the fit is taken
from the full ramps and the weaker ones are listed, not averaged in.

    python dev/air_probe.py [D:/ow-dumps/trace/player_ctrl.jsonl]
"""
import json
import struct
import sys

f32 = lambda b: struct.unpack('<f', struct.pack('<I', b))[0]
GRAVITY = 17.37            # m/s^2, measured (movement.rs GRAVITY)


def main(path: str) -> int:
    lines = [json.loads(l) for l in open(path)]
    hdr, samples = lines[0], lines[1:]
    w = [s for i, s in enumerate(samples) if i == 0 or samples[i - 1]['pos'] != s['pos']]

    frames = []
    for j in range(len(w) - 1):
        a, b = w[j], w[j + 1]
        dx = f32(b['pos'][0]) - f32(a['pos'][0])
        dy = f32(b['pos'][1]) - f32(a['pos'][1])
        dz = f32(b['pos'][2]) - f32(a['pos'][2])
        frames.append((a['i'], dx, dy, dz, b['t'] - a['t'], a['keys'], a['t']))

    # The ballistic decrement per write is g*dt^2 at the client's own write
    # cadence (the trace may be sampled at 60 or 200 Hz while the client writes
    # at 30-60 Hz), so the band comes from the trace's median write interval.
    dts = sorted(f[4] for f in frames if f[4] > 0)
    dt_write = dts[len(dts) // 2] if dts else 1.0 / hdr.get('hz', 60.0)
    gstep = GRAVITY * dt_write * dt_write
    lo, hi = 0.7 * gstep, 1.4 * gstep
    print(f"write cadence {dt_write * 1000:.2f} ms -> ballistic decrement {gstep * 1000:.2f} mm/write")
    bal = [lo <= abs(frames[j + 1][2] - frames[j][2]) <= hi
           and abs(frames[j + 1][2]) < 0.25 for j in range(len(frames) - 1)]
    runs, cur = [], []
    for j, ok in enumerate(bal):
        if ok:
            cur.append(j)
        elif cur:
            if len(cur) >= 5:
                runs.append(cur)
            cur = []
    if len(cur) >= 5:
        runs.append(cur)

    keymap = lambda k: ''.join(c for c, f in (('W', k['w']), ('A', k['a']), ('S', k['s']),
                                             ('D', k['d']), ('J', k['space']),
                                             ('C', k['ctrl'])) if f) or '-'
    dt = dt_write
    print(f"{path}: {len(runs)} ballistic run(s) of >=5 frames at {1 / dt:.0f} Hz writes")

    # Fit the *motion window* inside each ballistic run, with medians: a run
    # often begins with stationary airborne frames (the key lands mid-air), and
    # a time window that includes them understates the rate — that is how a
    # first version of this probe read 5 m/s^2 where the per-write increments
    # say 18.  `median(step increase) / median(dt)^2` is the estimator: the
    # steps are exact and the median dt is stable even when the client's frame
    # time varies (measured 18.1 m/s^2 at a 33.1 ms cadence in the custom map
    # and 19.2 at 16.7 ms in the practice range).
    fits = []
    for r in runs:
        step = [(frames[j][1] ** 2 + frames[j][3] ** 2) ** 0.5 for j in r]
        moving = [k for k, s in enumerate(step) if s > 0.2 * max(step)]
        if len(moving) < 4 or moving[0] == 0 and step[0] > 0.2 * max(step):
            pass
        if len(moving) < 4:
            continue
        lo, hi = moving[0], moving[-1]
        win = r[lo : hi + 1]
        incs = sorted(step[k + 1] - step[k] for k in range(lo, hi))
        dts = sorted(frames[j][4] for j in win if frames[j][4] > 0)
        if len(incs) < 3 or not dts:
            continue
        dt_med = dts[len(dts) // 2]
        inc_med = incs[len(incs) // 2]
        accel = inc_med / (dt_med * dt_med)
        keys = sorted(set(keymap(frames[j][5]) for j in win))
        plateau = step[hi]
        print(f"  i={frames[win[0]][0]:6d} {len(win):3d} writes moving  keys {keys}  "
              f"dt {dt_med * 1000:5.2f} ms  increase {inc_med * 1000:+6.2f} mm/write "
              f"-> {accel:+7.2f} m/s^2   (last step {plateau:.5f} m)")
        fits.append(accel)

    if fits:
        fits.sort()
        full = [f for f in fits if f >= 10.0]
        weak = [f for f in fits if f < 10.0]
        if full:
            print()
            print(f"full ramps (the recorded copy carries the body's motion): "
                  f"{len(full)} run(s), mean {sum(full) / len(full):.2f} m/s^2 "
                  f"<- the AIR_ACCEL fit")
        if weak:
            print(f"weaker ramps (a lagging copy, or a partly blocked run): "
                  f"{len(weak)} run(s), {min(weak):.2f}..{max(weak):.2f} m/s^2")
    return 0


if __name__ == '__main__':
    raise SystemExit(main(sys.argv[1] if len(sys.argv) > 1
                          else "D:/ow-dumps/trace/player_ctrl.jsonl"))

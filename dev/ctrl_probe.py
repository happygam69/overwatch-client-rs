#!/usr/bin/env python3
"""Measure crouch, jump and air-control behaviour from a captured trace.

The companion to `live_trace.py node`: once a trace taken with `ctrl` and
`space` held exists (the injected schedule in `inject_movement.ps1`, or the
five hand steps in STATUS-handoff.md), this prints the numbers the movement
model needs, in the same style as the earlier trace measurements:

  * every steady segment: the key set, the plateau step (m, and m/s at the
    client's own write cadence), and the ramp-in/out step sequences;
  * crouch: the crouch plateau speed (expect the 3.0 slot) and the transition
    steps between the walk and crouch plateaus (the speed ramp both ways);
  * jumps: the dy sequence — both the step into the jump segment and its first
    internal step are printed, because the key/position sampling alignment
    decides which one carries the impulse frame — the per-frame dy decrements
    (the gravity), and the discrete ascent;
  * air control: after a jump, the horizontal steps while a direction is held
    (whether a key pressed in the air accelerates, and by how much).

Usage: python dev/ctrl_probe.py D:/ow-dumps/trace/player_ctrl.jsonl
"""
import collections
import json
import math
import struct
import sys

f32 = lambda b: struct.unpack('<f', struct.pack('<I', b))[0]
KT = ('w', 'a', 's', 'd', 'space', 'ctrl', 'shift', 'lmb', 'rmb')


def load(path):
    """Read a trace; a typo'd or unreadable path gets a message, not a traceback."""
    try:
        lines = [json.loads(l) for l in open(path)]
    except OSError as e:
        print(f"{path}: {e}")
        sys.exit(2)
    except json.JSONDecodeError as e:
        print(f"{path}: not a trace (JSON lines): {e}")
        sys.exit(2)
    if len(lines) < 2:
        print(f"{path}: no samples")
        sys.exit(2)
    hdr = lines[0]
    return hdr, lines[1:]


def keys_of(s):
    k = s.get('keys') or s.get('phys') or {}
    return tuple(int(k.get(f, 0)) for f in KT)


def writes(samples):
    w = []
    for s in samples:
        if w and w[-1]['pos'] == s['pos']:
            continue
        w.append(s)
    return w


def step(g, j):
    dx = f32(g[j + 1]['pos'][0]) - f32(g[j]['pos'][0])
    dy = f32(g[j + 1]['pos'][1]) - f32(g[j]['pos'][1])
    dz = f32(g[j + 1]['pos'][2]) - f32(g[j]['pos'][2])
    return dx, dy, dz, math.hypot(dx, dz)


def name_of(k):
    parts = []
    for f, label in (('w', 'W'), ('a', 'A'), ('s', 'S'), ('d', 'D'),
                     ('space', 'jump'), ('ctrl', 'crouch'), ('shift', 'shift')):
        if k[KT.index(f)]:
            parts.append(label)
    return '+'.join(parts) if parts else 'idle'


def segments(w):
    segs, cur = [], [w[0]]
    for s in w[1:]:
        if keys_of(s) == keys_of(cur[-1]['keys']) if False else keys_of(s) == keys_of(cur[-1]):
            cur.append(s)
        else:
            segs.append(cur)
            cur = [s]
    segs.append(cur)
    return segs


def main(path):
    hdr, samples = load(path)
    w = writes(samples)
    print('%s: %d samples @ %g Hz, %d writes (kind %s)'
          % (path.split('/')[-1], len(samples), hdr.get('hz', 200.0), len(w),
             hdr.get('kind', '?')))
    iv = collections.Counter(w[i + 1]['i'] - w[i]['i'] for i in range(len(w) - 1))
    mean_iv = sum(k * v for k, v in iv.items()) / sum(iv.values())
    print('  write cadence: mean %.3f samples = %.5f s at %g Hz'
          % (mean_iv, mean_iv / hdr.get('hz', 200.0), hdr.get('hz', 200.0)))

    w_idx = 0
    for g in segments(w):
        seg_start = w_idx
        w_idx += len(g)
        if len(g) < 4:
            continue
        steps = [step(g, j) for j in range(len(g) - 1)]
        hs = [s[3] for s in steps]
        dys = [s[1] for s in steps]
        k = keys_of(g[0])
        # plateau = the most common horizontal step in the interior
        interior = [round(x, 6) for x in hs[1:-1]] or [round(x, 6) for x in hs]
        plateau = collections.Counter(interior).most_common(1)[0][0]
        span = g[-1]['t'] - g[0]['t'] if 't' in g[-1] and 't' in g[0] else 0.0
        n = len(g) - 1
        print('\n  i=%4d %-12s n=%3d  plateau step %.6f m (%.3f m/s at %.5f s/frame)'
              % (g[0]['i'], name_of(k), len(g), plateau,
                 plateau * 30.0 if span == 0 else plateau / (span / n), span / n if n else 0))
        if any(abs(v) > 0.002 for v in dys):
            print('     dy seq: %s' % ' '.join('%+.5f' % v for v in dys[:14]))
            dec = [dys[i + 1] - dys[i] for i in range(min(len(dys) - 1, 12))]
            if dec:
                print('     dy deltas: %s' % ' '.join('%+.5f' % v for v in dec[:12]))
        into = (f32(g[0]['pos'][1]) - f32(w[seg_start - 1]['pos'][1])) if seg_start else 0.0
        if k[KT.index('ctrl')]:
            print('     CROUCH segment: plateau %.4f m/s (the 3.0 slot is expected; '
                  'step into it %+.5f m)'
                  % (plateau * 30.0 if span == 0 else plateau / (span / n), into))
        if k[KT.index('space')]:
            print('     JUMP segment: first dy %+.5f m (step into it %+.5f m; the key '
                  'sample decides which carries the impulse frame)'
                  % (dys[0] if dys else 0.0, into))
        print('     first steps: %s' % ' '.join('%.5f' % x for x in hs[:8]))

    # air-control summary: horizontal step while airborne with a direction held
    print('\n  air-control check (horizontal steps while dy != 0 and a key held):')
    for g in segments(w):
        k = keys_of(g[0])
        if not (k[KT.index('w')] or k[KT.index('a')] or k[KT.index('s')] or k[KT.index('d')]):
            continue
        airborne = [step(g, j)[3] for j in range(len(g) - 1)
                    if abs(step(g, j)[1]) > 0.005]
        if len(airborne) >= 3:
            print('    i=%4d %-12s airborne horizontal steps: %s'
                  % (g[0]['i'], name_of(k),
                     ' '.join('%.5f' % x for x in airborne[:12])))


if __name__ == '__main__':
    for p in sys.argv[1:]:
        main(p)
        print()

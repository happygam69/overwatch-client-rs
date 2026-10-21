#!/usr/bin/env python3
"""Measure the client's movement per write (frame) from a captured trace.

Prints, per steady segment of identical keys+yaw:
  - write cadence in samples (200 Hz samples per frame)
  - the distinct per-write step magnitudes (m), and their value x 30
  - the implied speed for a candidate frame dt

Usage: python dev/trace_probe.py D:/ow-dumps/trace/player_fast3_keys.jsonl
"""
import collections
import json
import math
import struct
import sys

f32 = lambda b: struct.unpack('<f', struct.pack('<I', b))[0]
kt = lambda k: (k['w'], k['a'], k['s'], k['d'], k['space'], k['ctrl'], k['shift'],
                k['lmb'], k['rmb'])
NAMES = {(1, 0, 0, 0): 'W', (0, 1, 0, 0): 'A', (0, 0, 1, 0): 'S',
         (0, 0, 0, 1): 'D', (1, 0, 0, 1): 'W+D', (1, 0, 1, 0): 'W+S',
         (0, 0, 0, 0): 'idle'}


def load(path):
    lines = [json.loads(l) for l in open(path)]
    return lines[0], lines[1:]


def writes_of(samples):
    w = []
    for s in samples:
        if w and w[-1]['pos'] == s['pos']:
            continue
        w.append(s)
    return w


def segments_of(w):
    segs, cur = [], [w[0]]
    for s in w[1:]:
        if kt(s['keys']) == kt(cur[-1]['keys']) and s['yaw'] == cur[-1]['yaw']:
            cur.append(s)
        else:
            segs.append(cur)
            cur = [s]
    segs.append(cur)
    return segs


def step(g, j):
    dx = f32(g[j + 1]['pos'][0]) - f32(g[j]['pos'][0])
    dz = f32(g[j + 1]['pos'][2]) - f32(g[j]['pos'][2])
    return dx, dz, math.hypot(dx, dz)


def main(path):
    hdr, samples = load(path)
    w = writes_of(samples)
    iv = collections.Counter(w[i + 1]['i'] - w[i]['i'] for i in range(len(w) - 1))
    common = iv.most_common(4)
    mean_iv = sum(k * v for k, v in iv.items()) / sum(iv.values())
    print('%s: %d samples @ %g Hz, %d writes' % (path.split('/')[-1], len(samples),
                                                 hdr.get('hz', 200.0), len(w)))
    print('  write interval (samples): %s  mean %.3f = %.5f s at 200 Hz'
          % (common, mean_iv, mean_iv / 200.0))
    for g in segments_of(w):
        if len(g) < 8:
            continue
        name = NAMES.get(kt(g[0]['keys'])[:4], str(kt(g[0]['keys'])[:4]))
        if kt(g[0]['keys'])[4:7] != (0, 0, 0):
            name += '+mod'
        steps = [step(g, j)[2] for j in range(len(g) - 1)]
        c = collections.Counter(round(x, 6) for x in steps[1:-1] or steps)
        top = c.most_common(2)
        span = g[-1]['t'] - g[0]['t']
        n = len(g) - 1
        print('  i=%4d %-5s n=%3d span=%.3fs dt/frame=%.5fs  top steps: %s'
              % (g[0]['i'], name, len(g), span, span / n,
                 ['%.6f m (x30=%.5f)' % (k, k * 30) for k, _ in top]))
        if name == 'idle' or len(g) < 20:
            print('     seq:', ' '.join('%.6f' % x for x in steps[:14]))


if __name__ == '__main__':
    for p in sys.argv[1:]:
        main(p)
        print()

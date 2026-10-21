"""Extract the ghost loader copy and stride-search it for w.

    python dev/work/ghost_scan.py
"""

import sys
import numpy as np

sys.path.insert(0, r'dev')
from scan_dump import Dump

DUMP = r'D:\ow-dumps\ow_heap_full.dmp'
GHOST = 0x1BA39390000
SIZE = 0x2410000
REGIONS = {
    'data': (0x1df0000, 0x1ef7b01),
    'rdata': (0x1d70000, 0x1dec8c4),
    'eid': (0x1f80000, 0x2410000),
    'pdata': (0x1f00000, 0x1f10b3c),
}

d = Dump(DUMP)
img = np.zeros(SIZE, dtype=np.uint8)
for start, length, rva in d.ranges:
    lo = max(start, GHOST)
    hi = min(start + length, GHOST + SIZE)
    if lo >= hi:
        continue
    src = rva + (lo - start)
    img[lo - GHOST:hi - GHOST] = np.frombuffer(d.m[src:src + (hi - lo)], dtype=np.uint8)
open(r'dev/work/ghost_loader.bin', 'wb').write(img.tobytes())

arr = np.load(r'dev/work/w_per_page.npy')
dd = {int(p): int(w) for p, w in arr}
pages = sorted(dd)
runs = []
cur = [pages[0]]
for p in pages[1:]:
    if p == cur[-1] + 1:
        cur.append(p)
    else:
        runs.append(cur); cur = [p]
runs.append(cur)
runs.sort(key=len, reverse=True)
W = np.array([dd[p] for p in runs[0]], dtype=np.uint32)
W4 = W >> 2
print('run', runs[0][0], runs[0][-1], len(W))

def check(E, a, S, T, op):
    n = len(T)
    if a + S * (n - 1) >= len(E):
        return False
    e0 = int(E[a])
    C = (e0 ^ int(T[0])) if op == 'xor' else ((e0 - int(T[0])) & 0xffff)
    for k in range(1, n):
        e = int(E[a + S * k])
        if op == 'xor':
            if (e ^ int(T[k])) != C:
                return False
        else:
            if (e - int(T[k])) & 0xffff != C:
                return False
    return True

for rname, (lo, hi) in REGIONS.items():
    sub = img[lo:hi]
    print('region', rname, hex(lo), hex(hi))
    for width, dt in ((2, np.uint16), (4, np.uint32)):
        E = np.frombuffer(sub[:len(sub) // width * width].tobytes(), dtype=dt)
        for S in list(range(2, 33)) + [40, 48, 64, 96, 128, 192, 256, 384]:
            for o in range(min(width, 4)):
                if len(E) <= o + S * len(W):
                    continue
                Ev = E[o:]
                Dx = Ev[:-S] ^ Ev[S:]
                Ds = (Ev[S:] - Ev[:-S]).astype(np.uint16)
                for Tname, T in (('w', W), ('w4', W4)):
                    for op, D, tgt in (('xor', Dx, int(T[0] ^ T[1])),
                                       ('sub', Ds, int((T[1] - T[0]) & 0xffff))):
                        pos = np.flatnonzero(D == tgt)
                        for a in pos[:20]:
                            if check(Ev, int(a), S, T, op):
                                print(f'  HIT {rname} {Tname} {op} w{width} S{S} o{o} at {lo + int(a)*width:#x}')
print('done')

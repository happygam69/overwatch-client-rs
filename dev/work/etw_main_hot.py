"""Rank the hot main-module addresses found in an ETL.

    python dev/work/etw_main_hot.py
"""

import mmap
import sys

import numpy as np

ETL = r'D:\ow-dumps\etw\cpu_20261021_124450.etl'
MAIN = 0x7FF6A2D80000
SIZE = 0x4838000
CH = 64 * 1024 * 1024

f = open(ETL, 'rb')
m = mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ)
offsets = []
for off in range(0, len(m), CH):
    b = m[off:off + CH]
    n = len(b) // 8
    v = np.frombuffer(b[:n * 8], dtype=np.uint64)
    sel = (v >= np.uint64(MAIN)) & (v < np.uint64(MAIN + SIZE))
    if sel.any():
        for i in np.flatnonzero(sel):
            offsets.append(off + int(i) * 8)
    if (off // CH) % 16 == 0:
        print('off', hex(off), 'hits', len(offsets), flush=True)

offsets.sort()
print('total main qwords', len(offsets))
allv = np.array([int.from_bytes(m[o:o + 8], 'little') - MAIN for o in offsets], dtype=np.uint64)
u, c = np.unique(allv, return_counts=True)
order = np.argsort(-c)
print('ALL top:')
for i in order[:80]:
    print('  %08x %6d' % (int(u[i]), int(c[i])))

# first qword of each consecutive run of main qwords
firsts = []
prev = None
for o in offsets:
    if prev is None or o != prev + 8:
        firsts.append(o)
    prev = o
fv = np.array([int.from_bytes(m[o:o + 8], 'little') - MAIN for o in firsts], dtype=np.uint64)
u2, c2 = np.unique(fv, return_counts=True)
order2 = np.argsort(-c2)
print('FIRST top:')
for i in order2[:80]:
    print('  %08x %6d' % (int(u2[i]), int(c2[i])))

"""Find the qword tables pointing into the page metadata range.

    python dev/work/find_meta_table.py
"""

import mmap
import struct
import sys

import numpy as np

sys.path.insert(0, r'dev')
from scan_dump import Dump

DUMP = r'D:\ow-dumps\ow_heap_full.dmp'
META = 0x1BA36B90000
META_END = META + 0x41000

d = Dump(DUMP)
# map file offset -> VA
def va_of(off):
    for s, l, r in d.ranges:
        if r <= off < r + l:
            return s + (off - r)
    return None

f = open(DUMP, 'rb')
m = mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ)
CH = 64 * 1024 * 1024
hits = []
for off in range(0, len(m), CH):
    b = m[off:off + CH]
    n = len(b) // 8
    v = np.frombuffer(b[:n * 8], dtype=np.uint64)
    sel = (v >= np.uint64(META)) & (v < np.uint64(META_END))
    if sel.any():
        for i in np.flatnonzero(sel):
            hits.append(off + int(i) * 8)
    if (off // CH) % 16 == 0:
        print('off', hex(off), 'hits', len(hits), flush=True)
print('total hits', len(hits))
print('first', [hex(x) for x in hits[:20]])

runs = []
cur = [hits[0]] if hits else []
for x in hits[1:]:
    if x == cur[-1] + 8:
        cur.append(x)
    else:
        runs.append(cur); cur = [x]
if cur:
    runs.append(cur)
runs.sort(key=len, reverse=True)
for r in runs[:10]:
    va = va_of(r[0])
    print('run', len(r), 'file', hex(r[0]), 'va', hex(va) if va else None,
          'firsts', [hex(struct.unpack_from('<Q', m, x)[0]) for x in r[:4]])

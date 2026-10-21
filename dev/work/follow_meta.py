"""Follow the type-A record pointers hunting for a stored window start.

    python dev/work/follow_meta.py
"""

import struct
import sys

import numpy as np

sys.path.insert(0, r'dev')
from scan_dump import Dump

DUMP = r'D:\ow-dumps\ow_heap_full.dmp'
TBL = 0x1BA37437D58
N = 1439
MAIN = 0x7FF6A2D80000
MAIN_SIZE = 0x4838000

d = Dump(DUMP)
def read_va(va, n):
    for s, l, r in d.ranges:
        if s <= va < s + l:
            o = r + (va - s)
            if o + n > r + l:
                n = r + l - o
            return d.m[o:o + n]
    return None

wmap = {int(p): int(w) for p, w in np.load(r'dev/work/w_per_page.npy')}
tbl = read_va(TBL, N * 8)
print('table bytes', len(tbl) if tbl else None)
found = 0
for i in range(N):
    ptr = struct.unpack_from('<Q', tbl, i * 8)[0]
    rec = read_va(ptr, 0x90)
    if not rec or len(rec) < 0x90:
        continue
    page = struct.unpack_from('<Q', rec, 0x88)[0]
    if not (MAIN <= page < MAIN + MAIN_SIZE):
        continue
    pg = (page - MAIN) // 0x1000
    if pg not in wmap:
        continue
    w = wmap[pg]
    a = struct.unpack_from('<Q', rec, 0x68)[0]
    b = struct.unpack_from('<Q', rec, 0x70)[0]
    c = struct.unpack_from('<Q', rec, 0x78)[0]
    print(f'idx {i} rec {ptr:#x} page {pg} w {w} ptrs {a:#x} {b:#x} {c:#x}')
    for name, t in (('a', a), ('b', b), ('c', c)):
        if t < 0x10000:
            continue
        data = read_va(t, 0x100)
        if not data:
            continue
        hits = []
        for off in range(0, len(data) - 1):
            if struct.unpack_from('<H', data, off)[0] == w:
                hits.append(off)
        if hits:
            print(f'   {name} {t:#x} w-u16 at {[hex(x) for x in hits[:8]]} data {data[:0x40].hex()}')
    found += 1
    if found >= 20:
        break
print('found', found)

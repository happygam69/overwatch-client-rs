"""Test the page metadata record fields for the window start.

    python dev/work/meta_record_w.py
"""

import struct
import sys

import numpy as np

sys.path.insert(0, r'dev')
from scan_dump import Dump

DUMP = r'D:\ow-dumps\ow_heap_full.dmp'
TABLE_VA = 0x1BA36B90000
TABLE_LEN = 0x41000
MAIN = 0x7FF6A2D80000
MAIN_SIZE = 0x4838000

d = Dump(DUMP)
src = None
for s, l, r in d.ranges:
    if s <= TABLE_VA < s + l:
        src = r + (TABLE_VA - s)
        break
raw = d.m[src:src + TABLE_LEN]
wmap = {int(p): int(w) for p, w in np.load(r'dev/work/w_per_page.npy')}

rows = []
for p in range(0, len(raw) - 0x90, 8):
    for typ, poff in (('B', 0), ('A', 0x88)):
        if p + poff + 8 > len(raw):
            continue
        page = struct.unpack_from('<Q', raw, p + poff)[0]
        if not (MAIN <= page < MAIN + MAIN_SIZE) or page % 0x1000:
            continue
        pg = (page - MAIN) // 0x1000
        if pg not in wmap:
            continue
        rec = raw[p:p + 0x90]
        rows.append((typ, pg, wmap[pg], page, rec))
        break

print('rows', len(rows), 'A', sum(1 for r in rows if r[0] == 'A'), 'B', sum(1 for r in rows if r[0] == 'B'))
if not rows:
    raise SystemExit(1)

for typ in ('A', 'B'):
    sub = [r for r in rows if r[0] == typ]
    print('--- type', typ, len(sub))
    for off in range(0, 0x90 - 1):
        ok = sum(1 for r in sub if struct.unpack_from('<H', r[4], off)[0] == r[2])
        if ok > 2:
            print('  u16 off', hex(off), 'hits', ok, '/', len(sub))
    for off in range(0, 0x90 - 3):
        ok = sum(1 for r in sub if struct.unpack_from('<I', r[4], off)[0] == r[2])
        if ok > 2:
            print('  u32 off', hex(off), 'hits', ok, '/', len(sub))
    # qword windows
    for off in range(0, 0x90 - 7):
        q = [struct.unpack_from('<Q', r[4], off)[0] for r in sub]
        for s in range(0, 53):
            ok = sum(1 for r, v in zip(sub, q) if ((v >> s) & 0xfff) == r[2])
            if ok > max(3, len(sub) // 100):
                print('  qword off', hex(off), 'shift', s, 'hits', ok, '/', len(sub))

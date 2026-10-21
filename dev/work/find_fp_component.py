"""Locate first-person component scalars in the heap dump.

    python dev/work/find_fp_component.py
"""

import mmap
import struct
import sys

sys.path.insert(0, r'dev')
from scan_dump import Dump

DUMP = r'D:\ow-dumps\ow_heap_full.dmp'
PATTERNS = {
    'four': struct.pack('<4f', 50.0, 1.25, 0.875, 0.8),
    'pos': struct.pack('<3f', 0.1375, 0.23375, 0.20625),
    'three': struct.pack('<3f', 1.25, 0.875, 0.8),
    'two': struct.pack('<2f', 1.25, 0.875),
}

d = Dump(DUMP)
def va_of(off):
    for s, l, r in d.ranges:
        if r <= off < r + l:
            return s + (off - r)
    return None

for name, pat in PATTERNS.items():
    print('===', name, pat.hex())
    p = d.m.find(pat)
    n = 0
    while p >= 0:
        va = va_of(p)
        print(' hit', hex(p), 'va', hex(va) if va else None)
        ctx = d.m[p - 0x20:p + 0x60]
        print('  ctx', ctx.hex())
        n += 1
        if n >= 20:
            break
        p = d.m.find(pat, p + 1)
    print('count shown', n)

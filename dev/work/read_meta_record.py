"""Dump two page metadata records field by field.

    python dev/work/read_meta_record.py
"""

import struct
import sys

sys.path.insert(0, r'dev')
from scan_dump import Dump

d = Dump(r'D:\ow-dumps\ow_heap_full.dmp')
MAIN = 0x7FF6A2D80000

def read(va, n):
    for s, l, r in d.ranges:
        if s <= va < s + l:
            o = r + (va - s)
            return d.m[o:o + n]
    return None

for va in (0x1BA36B905C8, 0x1BA36BCC5F0):
    b = read(va, 0x90)
    print('===', hex(va))
    for o in range(0, 0x90, 8):
        q = struct.unpack_from('<Q', b, o)[0]
        d0 = struct.unpack_from('<I', b, o)[0]
        tag = ''
        if MAIN <= q < MAIN + 0x4838000:
            tag = ' page %d' % ((q - MAIN) // 0x1000)
        print('+%02x %016x %08x%s' % (o, q, d0, tag))

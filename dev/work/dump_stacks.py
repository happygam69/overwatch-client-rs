"""Find loader code addresses on the dump's thread stacks.

    python dev/work/dump_stacks.py
"""

import struct
import sys

sys.path.insert(0, r'dev')
from scan_dump import Dump

DUMP = r'D:\ow-dumps\ow_heap_full.dmp'
LOADER = 0x7ffc52d50000
LSIZE = 0x2410000
GHOST = 0x1BA39390000
PAD = 0x1ba3b107f80

REGIONS = [
    ('cand', 0x14380e0, 0x144ee19),
    ('padA', 0x10b72f0, 0x10bb994),
    ('padB', 0x10c1e20, 0x10d06c4),
    ('pad', 0x1d77f80, 0x1d7bf80),
]

def region(rva):
    for n, lo, hi in REGIONS:
        if lo <= rva < hi:
            return n
    return ''

d = Dump(DUMP)
print('threads', len(d.threads))
for tid, teb, ss, sz, srva, rip in d.threads:
    r = ''
    if LOADER <= rip < LOADER + LSIZE:
        r = region(rip - LOADER)
    if r or rip == 0:
        print(f'tid {tid} rip {rip:#x} {r} stack {ss:#x}+{sz:#x}')
    src = None
    for rs, rl, rr in d.ranges:
        if rs <= ss < rs + rl:
            src = rr + (ss - rs)
            break
    if src is None:
        continue
    b = d.m[src:src + sz]
    hits = []
    for off in range(0, len(b) - 8, 8):
        v = struct.unpack_from('<Q', b, off)[0]
        if LOADER <= v < LOADER + LSIZE:
            rv = v - LOADER
            hits.append((off, rv, region(rv)))
    if hits:
        print(f'  tid {tid} stack hits {len(hits)}')
        for off, rv, rr in hits[:20]:
            print(f'    stack+{off:#x} -> {rv:#x} {rr}')

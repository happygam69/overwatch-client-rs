"""Find candidate page-path PCs on the dump's thread stacks.

    python dev/work/dump_cand_stack.py
"""

import struct
import sys

sys.path.insert(0, r'dev')
from scan_dump import Dump

DUMP = r'D:\ow-dumps\ow_heap_full.dmp'
BASE = 0x7ffc52d50000
TARGETS = {
    'page_caller': 0x1015a99,
    'loop_leaf': 0x144c839,
    'vp_ret': 0x14462cb,
    'vp_caller': 0x1a0d53b,
    'padB_caller': 0x8a4ad5,
    'padB_leaf': 0x10c7692,
}

d = Dump(DUMP)
for tid, teb, ss, sz, srva, rip in d.threads:
    src = None
    for rs, rl, rr in d.ranges:
        if rs <= ss < rs + rl:
            src = rr + (ss - rs)
            break
    if src is None:
        continue
    b = d.m[src:src + sz]
    for name, rva in TARGETS.items():
        needle = struct.pack('<Q', BASE + rva)
        p = b.find(needle)
        while p >= 0:
            if p % 8 == 0:
                print(f'tid {tid} {name} at stack+{p:#x} (va {ss + p:#x})')
                lo = max(0, p - 0x600)
                for q in range(lo, min(len(b) - 8, p + 0x40), 8):
                    v = struct.unpack_from('<Q', b, q)[0]
                    tag = ''
                    if BASE <= v < BASE + 0x2410000:
                        tag = ' <L%x>' % (v - BASE)
                    if q in (p, p + 8, p - 8, p - 0x4dc):
                        print('   %+05x %016x%s' % (q - p, v, tag))
                print('   w-candidate at -0x4dc:', b[p - 0x4dc:p - 0x4dc + 16].hex() if p >= 0x4dc else 'n/a')
            p = b.find(needle, p + 1)

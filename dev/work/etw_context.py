"""Print the stack frames around sampled loader PCs in an ETL.

    python dev/work/etw_context.py
"""

import mmap
import struct

ETL = r'D:\ow-dumps\etw\cpu_20261021_124450.etl'
BASE = 0x7ffc52d50000
SIZE = 0x2410000
TARGETS = [
    ('cand_loop', 0x144c839),
    ('padB_m1', 0x10c7692),
    ('padB_m2', 0x10c9835),
    ('padB_hot', 0x10c35a0),
    ('cand_vp_ret', 0x14462cb),
]

f = open(ETL, 'rb')
m = mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ)
for name, rva in TARGETS:
    needle = struct.pack('<Q', BASE + rva)
    p = m.find(needle)
    n = 0
    print('===', name, hex(rva))
    while p >= 0 and n < 4:
        lo = max(0, p - 64)
        chunk = m[lo:p + 72]
        qs = []
        for i in range(0, len(chunk) - 7, 8):
            v = struct.unpack_from('<Q', chunk, i)[0]
            tag = ''
            if BASE <= v < BASE + SIZE:
                tag = ' <L%x>' % (v - BASE)
            qs.append('%016x%s' % (v, tag))
        print(' off', hex(p), ' '.join(qs))
        p = m.find(needle, p + 1)
        n += 1

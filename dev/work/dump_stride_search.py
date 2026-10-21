"""Search the full heap dump for window-start runs at strides.

    python dev/work/dump_stride_search.py
"""

import struct
import sys
import numpy as np

sys.path.insert(0, r'dev')
from scan_dump import Dump

DUMP = r'D:\ow-dumps\ow_heap_full.dmp'
arr = np.load(r'dev/work/w_per_page.npy')
d = {int(p): int(w) for p, w in arr}
pages = sorted(d)
runs = []
cur = [pages[0]]
for p in pages[1:]:
    if p == cur[-1] + 1:
        cur.append(p)
    else:
        runs.append(cur); cur = [p]
runs.append(cur)
runs.sort(key=len, reverse=True)
W = [int(d[p]) for p in runs[0]]
W4 = [w >> 2 for w in W]
print('run', runs[0][0], runs[0][-1], len(W), W[:4])

dump = Dump(DUMP)
hits = 0
for label, T in (('w', W), ('w4', W4)):
    needle = struct.pack('<H', T[0])
    print('needle', label, needle.hex(), 'ranges', len(dump.ranges), flush=True)
    for ri, (start, length, rva) in enumerate(dump.ranges):
        end = rva + length
        p = dump.m.find(needle, rva, end)
        while p >= 0:
            pos = p
            # test possible record strides
            for S in range(2, 129, 2):
                if pos + S + 2 > end:
                    break
                if struct.unpack_from('<H', dump.m, pos + S)[0] != T[1]:
                    continue
                ok = True
                for k in range(2, len(T)):
                    if pos + k * S + 2 > end or struct.unpack_from('<H', dump.m, pos + k * S)[0] != T[k]:
                        ok = False
                        break
                if ok:
                    print(f'  HIT {label} stride {S} va {start + (pos - rva):#x} range {start:#x}+{length:#x}')
                    hits += 1
            p = dump.m.find(needle, p + 1, end)
        if ri % 5000 == 0:
            print('  range', ri, 'hits', hits, flush=True)
print('hits', hits)

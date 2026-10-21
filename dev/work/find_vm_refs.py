"""Find the functions holding both viewmodel constants 0x758/0x778.

    python dev/work/find_vm_refs.py
"""

import bisect
import struct
from collections import defaultdict

d = open(r'dev/work/live_main_flat.bin', 'rb').read()
e = struct.unpack_from('<I', d, 0x3C)[0]
opt = e + 24
magic = struct.unpack_from('<H', d, opt)[0]
ddir = opt + (112 if magic == 0x20B else 96)
exc, exsize = struct.unpack_from('<II', d, ddir + 24)
ents = sorted(struct.unpack_from('<III', d, exc + i * 12) for i in range(exsize // 12))
starts = [a for a, b, c in ents]

def fn(x):
    i = bisect.bisect_right(starts, x) - 1
    if i < 0:
        return None
    a, b, c = ents[i]
    return a if a <= x < b else None

hits = defaultdict(dict)
for pat, tag in ((b'\x58\x07\x00\x00', '0x758'), (b'\x78\x07\x00\x00', '0x778')):
    pos = d.find(pat)
    while pos >= 0:
        f = fn(pos)
        if f is not None:
            hits[f][tag] = hits[f].get(tag, 0) + 1
        pos = d.find(pat, pos + 1)

both = [(f, v) for f, v in hits.items() if len(v) == 2]
print('functions with both', len(both), 'total funcs', len(hits))
for f, v in sorted(both)[:80]:
    print('  %08x %s' % (f, v))
print('only 758', sum(1 for v in hits.values() if list(v) == ['0x758']))
print('only 778', sum(1 for v in hits.values() if list(v) == ['0x778']))

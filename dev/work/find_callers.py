"""Find the callers of the first-person consumer and its bind.

    python dev/work/find_callers.py
"""

import bisect
import struct

PATH = r'dev/work/live_main_flat.bin'
d = open(PATH, 'rb').read()
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
    return (a, b) if a <= x < b else None

TARGETS = {0xD2F060: 'consumer', 0xD2FAA0: 'bind', 0xD2FDCA: 'end'}
for t, name in TARGETS.items():
    print('===', name, hex(t))
    n = 0
    for i in range(len(d) - 5):
        op = d[i]
        if op == 0xE8:
            disp = struct.unpack_from('<i', d, i + 1)[0]
            if i + 5 + disp == t:
                f = fn(i)
                print(' call at', hex(i), 'fn', hex(f[0]) if f else None)
                n += 1
        elif op == 0xE9:
            disp = struct.unpack_from('<i', d, i + 1)[0]
            if i + 5 + disp == t:
                f = fn(i)
                print(' jmp at', hex(i), 'fn', hex(f[0]) if f else None)
                n += 1
    print(' total', n)

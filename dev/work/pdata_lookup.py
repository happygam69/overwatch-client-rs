"""Look up the loader .pdata function bounds that contain RVAs.

    python dev/work/pdata_lookup.py <rva> [rva ...]
"""

import struct, sys, bisect

d = open(r'D:/ow-dumps/loader_disk_rva.bin', 'rb').read()
e_lfanew = struct.unpack_from('<I', d, 0x3c)[0]
assert d[e_lfanew:e_lfanew + 4] == b'PE\0\0'
nsec = struct.unpack_from('<H', d, e_lfanew + 6)[0]
opt = e_lfanew + 24
magic = struct.unpack_from('<H', d, opt)[0]
ddir = opt + (112 if magic == 0x20b else 96)
exc_rva, exc_size = struct.unpack_from('<II', d, ddir + 3 * 8)
sec = opt + struct.unpack_from('<H', d, e_lfanew + 20)[0]
secs = []
for i in range(nsec):
    o = sec + i * 40
    name = d[o:o + 8].rstrip(b'\0').decode('latin1')
    vsize, va, rsize, raw = struct.unpack_from('<IIII', d, o + 8)
    secs.append((name, va, vsize, raw, rsize))
print('exception dir', hex(exc_rva), hex(exc_size), 'dirs', secs)
print('sections', secs)

# map RVA to file offset
def off(rva):
    for name, va, vsize, raw, rsize in secs:
        if va <= rva < va + max(vsize, rsize):
            return raw + (rva - va)
    return None

fo = off(exc_rva)
entries = []
n = exc_size // 12
for i in range(n):
    b, e, u = struct.unpack_from('<III', d, fo + i * 12)
    if b:
        entries.append((b, e, u))
entries.sort()
starts = [b for b, e, u in entries]
print('entries', len(entries))

def find(a):
    i = bisect.bisect_right(starts, a) - 1
    if i < 0:
        return None
    b, e, u = entries[i]
    if b <= a < e:
        return (b, e, u)
    # sometimes zero-sized
    if b <= a < e + 16:
        return (b, e, u)
    return None

for a in sys.argv[1:]:
    x = int(a, 16)
    print(hex(x), find(x), 'off', hex(off(x)) if off(x) else None)

"""Find the code sites that read first-person component type 0x35.

    python dev/work/find_getcomp35.py
"""

import bisect
import struct

from capstone import Cs, CS_ARCH_X86, CS_MODE_64

PATH = r'dev/work/live_main_flat.bin'
STATIC = 0x7FF7C4D20000

d = open(PATH, 'rb').read()
assert d[:2] == b'MZ'
e = struct.unpack_from('<I', d, 0x3C)[0]
assert d[e:e + 4] == b'PE\0\0'
nsec = struct.unpack_from('<H', d, e + 6)[0]
opt = e + 24
magic = struct.unpack_from('<H', d, opt)[0]
ddir = opt + (112 if magic == 0x20B else 96)
exc, exsize = struct.unpack_from('<II', d, ddir + 3 * 8)
sec = opt + struct.unpack_from('<H', d, e + 20)[0]
print('exc', hex(exc), hex(exsize), 'nsec', nsec)
entries = []
for i in range(exsize // 12):
    b, en, u = struct.unpack_from('<III', d, exc + i * 12)
    if b:
        entries.append((b, en, u))
entries.sort()
starts = [b for b, en, u in entries]
print('pdata entries', len(entries))

def contain(rva):
    i = bisect.bisect_right(starts, rva) - 1
    if i < 0:
        return None
    b, en, u = entries[i]
    return (b, en) if b <= rva < en else None

md = Cs(CS_ARCH_X86, CS_MODE_64)
needle = b'\xb2\x35'
hits = []
pos = d.find(needle)
while pos >= 0:
    ins = list(md.disasm(d[pos:pos + 48], pos))
    if ins and ins[0].mnemonic == 'mov' and ins[0].op_str.replace(' ', '') == 'dl,0x35':
        for j, x in enumerate(ins[1:9]):
            if x.mnemonic == 'call' and ' + 0x10]' in x.op_str:
                fn = contain(pos)
                hits.append((pos, x.address, fn, ins[:12]))
                break
    pos = d.find(needle, pos + 1)

print('hits', len(hits))
for pos, call, fn, ins in hits:
    fns = ('%x..%x' % (fn[0], fn[1])) if fn else '?'
    st = hex(STATIC + fn[0]) if fn else '?'
    print(f'\nsite {pos:#x} call {call:#x} fn {fns} static {st}')
    for x in ins:
        print('  %08x %-8s %s' % (x.address, x.mnemonic, x.op_str))

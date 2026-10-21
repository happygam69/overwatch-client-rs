"""List the candidate frame's slot references by stack offset.

    python dev/work/stackrefs.py
"""

import re
from capstone import Cs, CS_ARCH_X86, CS_MODE_64

d = open(r'D:/ow-dumps/loader_disk_rva.bin', 'rb').read()
lo, hi = 0x14380e0, 0x144ee19
md = Cs(CS_ARCH_X86, CS_MODE_64)

def refs(disp, label):
    pat = disp.to_bytes(4, 'little')
    print(f'--- {label} [rsp+{disp:#x}]')
    n = 0
    start = lo
    while True:
        i = d.find(pat, start, hi)
        if i < 0:
            break
        start = i + 1
        # candidate instruction start within 8 bytes before the disp
        for back in range(1, 9):
            s = i - back
            if s < lo:
                continue
            ins = next(md.disasm(d[s:s + 16], s), None)
            if ins is None:
                continue
            if ins.address <= i < ins.address + ins.size and f'rsp + {disp:#x}]' in ins.op_str:
                print(f'  {ins.address:#010x} {ins.bytes.hex():<24} {ins.mnemonic} {ins.op_str}')
                n += 1
                break
    print('  count', n)

for disp in (0x120, 0x150, 0x1b8, 0x1c0, 0x1bc, 0x324, 0x328, 0x340, 0x360, 0x390, 0x4e8, 0x4e4, 0x598, 0x740, 0x5a8):
    refs(disp, '')

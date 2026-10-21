"""Disassemble a loader code range at the given RVAs.

    python dev/work/dis_range.py <image> <lo-rva> <hi-rva>
"""

import sys
from capstone import Cs, CS_ARCH_X86, CS_MODE_64

path = sys.argv[1] if len(sys.argv) > 1 else r'D:/ow-dumps/loader_disk_rva.bin'
lo = int(sys.argv[2], 16)
hi = int(sys.argv[3], 16)
d = open(path, 'rb').read()
md = Cs(CS_ARCH_X86, CS_MODE_64)
pc = lo
while pc < hi:
    ins = next(md.disasm(d[pc:pc + 15], pc), None)
    if ins is None:
        print(f'{pc:#x}  ??')
        pc += 1
        continue
    print(f'{pc:#x}  {ins.bytes.hex():<20} {ins.mnemonic:<8} {ins.op_str}')
    pc += ins.size

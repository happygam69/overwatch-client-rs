"""Find the writes to the candidate descriptor's stack fields.

    python dev/work/stack_field_writers.py
"""

import sys
from collections import deque
from capstone import Cs, CS_ARCH_X86, CS_MODE_64

PATH = r'D:/ow-dumps/loader_disk_rva.bin'
d = open(PATH, 'rb').read()
md = Cs(CS_ARCH_X86, CS_MODE_64)
md.detail = True

LO, HI = 0x14380e0, 0x144ee19

def walk(lo, hi, entry):
    seen = {}
    work = deque([entry])
    while work:
        pc = work.popleft()
        while lo <= pc < hi:
            if pc in seen:
                break
            ins = next(md.disasm(d[pc:pc + 15], pc), None)
            if ins is None:
                break
            seen[pc] = ins
            mn = ins.mnemonic
            fall = pc + ins.size
            if mn.startswith('j'):
                ops = ins.operands
                if ops and ops[0].type == 2:
                    t = ops[0].imm
                    if lo <= t < hi:
                        work.append(t)
                if mn == 'jmp':
                    break
                pc = fall
                continue
            if mn == 'call':
                ops = ins.operands
                if ops and ops[0].type == 2:
                    t = ops[0].imm
                    if lo <= t < hi:
                        work.append(t)
                pc = fall
                continue
            if mn in ('ret', 'retf', 'int3', 'ud2', 'hlt'):
                break
            pc = fall
    return seen

seen = walk(LO, HI, LO)
addrs = sorted(seen)
print('decoded', len(seen))
for field in ('0x424', '0x428', '0x42c', '0x778', '0x780', '0x788', '0x790'):
    print('== field', field)
    for a in addrs:
        i = seen[a]
        s = i.op_str
        if f'[rsp + {field}]' not in s:
            continue
        dst, _, src = s.partition(',')
        if f'[rsp + {field}]' in dst:
            print(f'  {a:#x} {i.mnemonic:<8} {s}')
            idx = addrs.index(a)
            for b in addrs[max(0, idx - 6):idx]:
                j = seen[b]
                print(f'      {b:#x} {j.mnemonic:<8} {j.op_str}')

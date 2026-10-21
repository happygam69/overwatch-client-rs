"""Print the instruction context around candidate loader addresses.

    python dev/work/trace_back.py <addr> [addr ...]
"""

import sys
from collections import deque
from capstone import Cs, CS_ARCH_X86, CS_MODE_64, CS_OP_IMM

PATH = r'D:/ow-dumps/loader_disk_rva.bin'
d = open(PATH, 'rb').read()
md = Cs(CS_ARCH_X86, CS_MODE_64)
md.detail = True
LO, HI = 0x14380e0, 0x144ee19

seen = {}
targets = set([LO])
work = deque([LO])
while work:
    pc = work.popleft()
    while LO <= pc < HI:
        if pc in seen:
            break
        ins = next(md.disasm(d[pc:pc + 15], pc), None)
        if ins is None:
            break
        seen[pc] = ins
        mn = ins.mnemonic
        fall = pc + ins.size
        if mn.startswith('j'):
            if ins.operands and ins.operands[0].type == CS_OP_IMM:
                t = ins.operands[0].imm
                if LO <= t < HI:
                    targets.add(t)
                    work.append(t)
            if mn == 'jmp':
                break
            pc = fall
            continue
        if mn == 'call':
            if ins.operands and ins.operands[0].type == CS_OP_IMM:
                t = ins.operands[0].imm
                if LO <= t < HI:
                    targets.add(t)
                    work.append(t)
            pc = fall
            continue
        if mn in ('ret', 'retf', 'int3', 'ud2', 'hlt'):
            break
        pc = fall

insns = sorted(seen.items())
idx = {a: i for i, (a, _) in enumerate(insns)}

def show(a, back=60, fwd=25):
    if a not in idx:
        print('not decoded', hex(a)); return
    i = idx[a]
    s = max(0, i - back); e = min(len(insns), i + fwd)
    print(f'===== {a:#x}')
    for j in range(s, e):
        ad, ins = insns[j]
        mark = ' <==' if ad == a else ''
        print(f'  {ad:#x} {ins.bytes.hex():<24} {ins.mnemonic} {ins.op_str}{mark}')

for a in sys.argv[1:]:
    show(int(a, 16))

"""Measure the decoded coverage of the four loader VM regions.

    python dev/work/recursive_disasm.py
"""

import sys
from collections import deque
from capstone import Cs, CS_ARCH_X86, CS_MODE_64, CS_OP_IMM

PATH = r'D:/ow-dumps/loader_disk_rva.bin'
d = open(PATH, 'rb').read()
md = Cs(CS_ARCH_X86, CS_MODE_64)
md.detail = True

FUNCS = {
    'cand': (0x14380e0, 0x144ee19),
    'padA': (0x10b72f0, 0x10bb994),
    'padB': (0x10c1e20, 0x10d06c4),
    'rival': (0x442110, 0x4484a6),
}

def walk(lo, hi, entry=None):
    entry = entry or lo
    seen = {}
    blocks = {}
    work = deque([entry])
    bad = []
    while work:
        pc = work.popleft()
        while lo <= pc < hi:
            if pc in seen:
                break
            if pc + 16 > len(d):
                break
            ins = next(md.disasm(d[pc:pc + 15], pc), None)
            if ins is None:
                bad.append(pc)
                break
            seen[pc] = ins
            mn = ins.mnemonic
            fall = pc + ins.size
            if mn.startswith('j'):
                op = ins.operands
                if op and op[0].type == CS_OP_IMM:
                    t = op[0].imm
                    if lo <= t < hi:
                        work.append(t)
                if mn == 'jmp':
                    pc = None
                    break
                pc = fall
                continue
            if mn == 'call':
                op = ins.operands
                if op and op[0].type == CS_OP_IMM:
                    t = op[0].imm
                    if lo <= t < hi:
                        work.append(t)
                pc = fall
                continue
            if mn in ('ret', 'retf', 'int3', 'ud2', 'hlt'):
                pc = None
                break
            pc = fall
        if pc is None:
            pass
    return seen, bad

for name, (lo, hi) in FUNCS.items():
    seen, bad = walk(lo, hi)
    addrs = sorted(seen)
    print(name, hex(lo), 'decoded', len(seen), 'insns, bad', len(bad))
    # instruction density within span
    print('  first', hex(addrs[0]) if addrs else '-', 'last', hex(addrs[-1]) if addrs else '-')
    # count pad-[r12] style uses
    pad_uses = [a for a, i in seen.items() if 'r12' in i.op_str and '[' in i.op_str]
    print('  r12-mem ops', len(pad_uses))
    for a in pad_uses[:12]:
        i = seen[a]
        print(f'    {a:#x} {i.mnemonic} {i.op_str}')
    # contiguous coverage ratio
    span = hi - lo
    covered = sum(i.size for i in seen.values())
    print(f'  covered bytes {covered} / {span} = {covered/span:.2%}')

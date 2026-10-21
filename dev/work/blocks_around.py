"""Show the candidate VM basic blocks around the tracked sites.

    python dev/work/blocks_around.py
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

def block_start(a):
    p = a
    while True:
        prev = None
        for x in range(p - 15, p):
            i = seen.get(x)
            if i is not None and i.address + i.size == p:
                prev = x
                break
        if prev is None or p in targets or seen[prev].mnemonic in ('jmp', 'ret', 'retf', 'int3', 'ud2', 'hlt') or seen[prev].mnemonic.startswith('j'):
            return p
        p = prev

def print_block(a, limit=80):
    s = block_start(a)
    print(f'--- block at {s:#x} (for {a:#x})')
    p = s
    n = 0
    while p in seen and n < limit:
        i = seen[p]
        mark = ' <==' if p == a else ''
        print(f'  {i.address:#x} {i.bytes.hex():<24} {i.mnemonic} {i.op_str}{mark}')
        if i.mnemonic.startswith('j') or i.mnemonic in ('ret', 'retf', 'int3', 'ud2', 'hlt'):
            break
        p += i.size
        n += 1

for a in (0x143a249, 0x143b304, 0x143f5f7, 0x143f61d, 0x143fc0f, 0x144000e,
          0x1440f0d, 0x144476d, 0x1443075, 0x144bcaf, 0x144c839, 0x1439638,
          0x14479e1):
    if a in seen:
        print_block(a)
    else:
        print('not decoded', hex(a))

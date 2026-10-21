"""Show the pad-family B basic blocks around its mask sites.

    python dev/work/padb_blocks.py
"""

from collections import deque
from capstone import Cs, CS_ARCH_X86, CS_MODE_64, CS_OP_IMM

PATH = r'D:/ow-dumps/loader_disk_rva.bin'
d = open(PATH, 'rb').read()
md = Cs(CS_ARCH_X86, CS_MODE_64)
md.detail = True
LO, HI = 0x10c1e20, 0x10d06c4
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
    for _ in range(200):
        prev = None
        for x in range(p - 15, p):
            i = seen.get(x)
            if i is not None and i.address + i.size == p:
                prev = x
                break
        if prev is None or p in targets:
            return p
        pi = seen[prev]
        if pi.mnemonic.startswith('j') or pi.mnemonic in ('ret', 'retf', 'int3', 'ud2', 'hlt'):
            return p
        p = prev
    return p

def print_block(a, before=0, after=40):
    s = block_start(a)
    print(f'--- block {s:#x} (site {a:#x})')
    p = s
    n = 0
    while p in seen and n < after:
        i = seen[p]
        mark = ' <==' if p == a else ''
        print(f'  {i.address:#x} {i.bytes.hex():<24} {i.mnemonic} {i.op_str}{mark}')
        if i.mnemonic.startswith('j') or i.mnemonic in ('ret', 'retf', 'int3', 'ud2', 'hlt'):
            break
        p += i.size
        n += 1

for a in (0x10c4a94, 0x10c6b51, 0x10c702a, 0x10c7052, 0x10c7692,
          0x10c881e, 0x10c8828, 0x10c982f, 0x10c98b6, 0x10ccc1a):
    print_block(a)

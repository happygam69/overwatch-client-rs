"""Crypto-density scan of the CURRENT loader: per-.pdata-function density of
ARX ops (rol/ror small-imms), SIMD (pxor/pshufb/movdqu/paddd), bswap, plus
xrefs to the MD5-init constant block at 0x1d77830.

Usage: python dev/ldr_cryptoscan.py
"""
import struct
from collections import defaultdict

import capstone as cs

DISK = r"D:\ow-dumps\loader_disk_rva.bin"
b = open(DISK, "rb").read()
TEXT = (0x10000, 0x1D639E6)
md = cs.Cs(cs.CS_ARCH_X86, cs.CS_MODE_64)
md.detail = True

# 1) MD5-init xrefs: rip-refs to 0x1d77830..0x1d77840 region
mdref = []
for p in range(TEXT[0] >> 12, TEXT[1] >> 12):
    rva = p << 12
    pg = b[rva:rva + 0x1000]
    if not any(pg):
        continue
    for ins in md.disasm(pg, rva):
        if ins.mnemonic in ("nop", "ret"):
            break
        for op in ins.operands:
            if op.type == cs.x86.X86_OP_MEM and op.mem.base == cs.x86.X86_REG_RIP:
                t = ins.address + ins.size + op.mem.disp
                if 0x1D77800 <= t < 0x1D77890:
                    mdref.append((ins.address, t, ins.mnemonic, ins.op_str))
            elif op.type == cs.x86.X86_OP_IMM and 0x1D77800 <= op.imm < 0x1D77890:
                mdref.append((ins.address, op.imm, ins.mnemonic, ins.op_str))
print(f"refs to md5-init block: {len(mdref)}")
for a, t, m, o in mdref[:20]:
    print(f"  {a:#x} -> {t:#x}: {m} {o}")

print("\ncontext around 0x1d77820:")
for i in range(0, 0x70, 8):
    v = struct.unpack_from("<Q", b, 0x1D77820 + i)[0]
    print(f"  {0x1D77820 + i:#x}: {v:#018x}")

# 2) per-function crypto density
pe = struct.unpack_from("<I", b, 0x3C)[0]
nsec = struct.unpack_from("<H", b, pe + 6)[0]
opt = struct.unpack_from("<H", b, pe + 20)[0]
s0 = pe + 24 + opt
pd = None
for i in range(nsec):
    o = s0 + 40 * i
    name = b[o:o + 8].split(b"\x00")[0]
    vsz, va, rsz, ro = struct.unpack_from("<IIII", b, o + 8)
    if name.startswith(b".pdata"):
        pd = va
fns = []
p = pd
while True:
    beg, end, un = struct.unpack_from("<III", b, p)
    if beg < TEXT[0] or beg >= TEXT[1]:
        break
    fns.append((beg, end))
    p += 12

scores = []
for beg, end in fns:
    n = 0
    arx = simd = bswap = 0
    immsmall = 0
    for ins in md.disasm(b[beg:end], beg):
        if ins.mnemonic in ("nop", "ret", "jmp", "call"):
            if ins.mnemonic == "ret":
                break
            continue
        n += 1
        m = ins.mnemonic
        if m in ("ror", "rol", "rcl", "rcr"):
            for op in ins.operands:
                if op.type == cs.x86.X86_OP_IMM and 1 <= op.imm <= 31:
                    arx += 3
        elif m in ("shl", "shr"):
            arx += 1
        elif m in ("xor",):
            arx += 2
        elif m in ("pxor", "pshufb", "paddd", "psubd", "movdqu", "movdqa",
                   "aesenc", "aesenclast"):
            simd += 4
        elif m == "bswap":
            bswap += 3
    if n < 40:
        continue
    score = (arx + simd + bswap) / n
    scores.append((score, arx, simd, bswap, n, beg, end))

scores.sort(key=lambda x: -x[0])
print(f"\ntop crypto-density functions (of {len(scores)} scanned):")
print("score arx simd bswap n fn")
for s, arx, simd, bsw, n, beg, end in scores[:40]:
    print(f"{s:6.3f} {arx:4} {simd:4} {bsw:4} {n:6}  {beg:#x}..{end:#x}")

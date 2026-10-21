"""Work-digest of an engine satellite body: writes, call-indirect sites,
large constants, SIMD ops, rip refs - non-dispatch blocks only.

Usage: python dev/ldr_work_digest.py <begin> <end>
"""
import sys

import capstone as cs

DISK = r"D:\ow-dumps\loader_disk_rva.bin"
b = open(DISK, "rb").read()
md = cs.Cs(cs.CS_ARCH_X86, cs.CS_MODE_64)
md.detail = True

begin, end = (int(sys.argv[1], 16), int(sys.argv[2], 16)) if len(sys.argv) > 2 \
    else (0x1068260, 0x106957B)
code = b[begin:end]
n = 0
words_total = 0
disp = []
for ins in md.disasm(code, begin):
    if ins.mnemonic == "ret":
        break
    n += 1
    m, ops = ins.mnemonic, ins.op_str
    words = ops.split(", ")
    ops_real = 0
    rng = any(w.startswith("[rip") or "0x" in w for w in words)
    cls = ""
    if m.startswith(("mov", "movabs")) and "rip" in ops:
        cls = "RIP"
    elif m.startswith("mov") and "[" in ops and "rip" not in ops:
        cls = "MEM"
    elif m.startswith(("xor", "add", "sub", "shl", "shr", "ror", "rol",
                       "imul", "and", "or", "not", "neg")):
        cls = "ARX"
    elif m.startswith(("movdqa", "movdqu", "pxor", "pand", "por", "pshufb",
                       "padd", "movaps", "movups", "paddd", "psrldq",
                       "movq", "test", "cmp")):
        cls = "SIMD"
    elif m in ("call", "jmp"):
        if "q" in ops or "ptr" in ops:
            cls = "CALLI"
        else:
            cls = "CALL"
    if cls in ("RIP", "MEM", "CALLI", "SIMD", "CALL") or \
            (m == "movabs" and abs(ops.count(",")) < 3):
        disp.append((ins.address, m, ops, cls))

print(f"instructions: {n}; selected lines: {len(disp)}")
for a, m, ops, cls in disp[:250]:
    print(f"  {a:#x}  {cls:<5} {m} {ops}")

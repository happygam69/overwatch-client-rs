"""Pdata-guided anchor sweep over the DISK loader (plaintext .text).

Collects, per .pdata function (and per page for the .pdata-less band):
  - rip-relative targets inside .eid / .gxfg / .eidsig
  - immediates inside .eid range or equal to main-exe .text extent markers
  - gs:/fs: segment reads
  - rip references into the IAT (0x1dcdd58 .. +0x960)
  - 4 KiB arithmetic (add reg, 0x1000 style)

Writes D:/ow-dumps/ldr_sweep_hits.csv and prints a compact summary.

Usage: python dev/ldr_sweep.py
"""
import csv
import struct

import capstone as cs

DISK = r"D:\ow-dumps\loader_disk_rva.bin"
PDATA_RVA, PDATA_SIZE = 0x1F00000, 0x10B3C
TEXT = (0x10000, 0x1D63A00)
EID = (0x1F80000, 0x2410000)
GXFG = (0x1F30000, 0x1F32610)
EIDSIG = (0x1F20000, 0x1F20020)
IAT = (0x1DCDD58, 0x1DCDD58 + 0x960)
SIZE_MARKS = {0x2F94A00: "main .text rawsz", 0x2F94913: "main .text va-sz?",
              0x2F95A00: "main .text end-page", 0x2F94: "page count 12180",
              0x12180: "page count dec"}

b = open(DISK, "rb").read()
out = open(r"D:\ow-dumps\ldr_sweep_hits.csv", "w", newline="", encoding="utf-8")
w = csv.writer(out)
w.writerow(["kind", "fn_begin", "fn_end", "ins_rva", "detail"])

md = cs.Cs(cs.CS_ARCH_X86, cs.CS_MODE_64)
md.detail = True


def span_hits(begin, end, fn_begin, fn_end, tag):
    n = 0
    for ins in md.disasm(b[begin:end], begin):
        n += 1
        for op in ins.operands:
            if op.type == cs.x86.X86_OP_MEM:
                m = op.mem
                if m.segment in (cs.x86.X86_REG_FS, cs.x86.X86_REG_GS):
                    w.writerow(["seg", fn_begin, fn_end, ins.address, f"{m.segment}"])
                if m.base == cs.x86.X86_REG_RIP:
                    t = ins.address + ins.size + m.disp
                    for name, (a, z) in (("eid", EID), ("gxfg", GXFG),
                                         ("eidsig", EIDSIG), ("iat", IAT)):
                        if a <= t < z:
                            w.writerow([name, fn_begin, fn_end, ins.address,
                                        f"rip {t:#x} {ins.mnemonic} {ins.op_str}"])
            elif op.type == cs.x86.X86_OP_IMM:
                v = op.imm & 0xFFFFFFFFFFFFFFFF
                if EID[0] <= v < EID[1]:
                    w.writerow(["eid_imm", fn_begin, fn_end, ins.address,
                                f"{ins.mnemonic} {ins.op_str}"])
                elif v in SIZE_MARKS:
                    w.writerow(["sizemark", fn_begin, fn_end, ins.address,
                                f"{SIZE_MARKS[v]} {ins.mnemonic} {ins.op_str}"])
        if ins.mnemonic == "add" and "0x1000" in ins.op_str:
            w.writerow(["step4k", fn_begin, fn_end, ins.address,
                        f"{ins.mnemonic} {ins.op_str}"])
    return n


# 1) .pdata functions
# the disk image is FLAT: byte offset == RVA (verified: MZ header at 0,
# sane .pdata triples at 0x1f00000)
def rva2off(rva):
    return rva


pd = rva2off(PDATA_RVA)
count = (PDATA_SIZE) // 12
tot_ins = 0
fn_list = []
for i in range(count):
    beg, end, un = struct.unpack_from("<III", b, pd + 12 * i)
    if beg < TEXT[0] or beg >= TEXT[1]:
        continue
    fn_list.append((beg, end))
    tot_ins += span_hits(beg, end, beg, end, "pdata")

print(f"pdata functions in .text: {len(fn_list)}, instructions {tot_ins}")

# 2) pages of .text without pdata coverage (the band) - disasm from page start
covered = set()
for beg, end in fn_list:
    for p in range(beg >> 12, (end - 1 >> 12) + 1):
        covered.add(p)
band_pages = 0
band_ins = 0
for p in range(TEXT[0] >> 12, TEXT[1] >> 12):
    if p in covered:
        continue
    pg = b[p << 12:(p + 1) << 12]
    if not any(pg):
        continue
    band_pages += 1
    band_ins += span_hits(p << 12, (p << 12) + 0x1000, p << 12, (p + 1) << 12, "page")
print(f"band pages scanned: {band_pages}, instructions {band_ins}")

# summary from csv
out.close()
rows = list(csv.reader(open(r"D:\ow-dumps\ldr_sweep_hits.csv", encoding="utf-8")))[1:]
from collections import Counter
c = Counter(r[0] for r in rows)
print("hit kinds:", dict(c))
for kind in c:
    if kind in ("eid", "eid_imm", "sizemark", "seg"):
        print(f"--- {kind} ---")
        for r in [r for r in rows if r[0] == kind][:25]:
            print("  fn", r[1], r[2], "ins", r[3], r[4])

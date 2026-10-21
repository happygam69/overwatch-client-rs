"""Re-digest the sweep with decimal CSV fields + missed-page diff + disasm.

Usage: python dev/ldr_dig2.py
"""
import csv
from collections import defaultdict

rows = list(csv.reader(open(r"D:\ow-dumps\ldr_sweep_hits.csv", encoding="utf-8")))[1:]

print("== step4k (true rvas) ==")
for kind, fb, fe, ins, det in rows:
    if kind == "step4k":
        print(f"  fn {int(fb):#x}..{int(fe):#x} ins {int(ins):#x} {det}")

segs = defaultdict(int)
for kind, fb, fe, ins, det in rows:
    if kind == "seg":
        segs[(int(fb), int(fe))] += 1
print("\n== top seg-read functions ==")
for (fb, fe), n in sorted(segs.items(), key=lambda kv: -kv[1])[:10]:
    print(f"  fn {fb:#x}..{fe:#x}: {n}")

iat = defaultdict(list)
for kind, fb, fe, ins, det in rows:
    if kind == "iat":
        slot = int(det.split()[1], 16)
        iat[slot].append((int(ins), int(fb)))
top = sorted(iat.items(), key=lambda kv: -len(kv[1]))[:12]
print("\n== top IAT slots by refs ==")
for slot, lst in top:
    fns = sorted({fb for _, fb in lst})
    print(f"  slot {slot:#x}: {len(lst)} refs, fns {[hex(f) for f in fns[:6]]}")

print("\n== disk vs warm diff (missed pages) ==")
DISK = r"D:\ow-dumps\loader_disk_rva.bin"
WARM = r"D:\ow-dumps\loader_after_diff.bin"
disk = open(DISK, "rb").read()
warm = open(WARM, "rb").read()
# header/data sections rvas
SECS = {"hdr": (0, 0x1000), ".text": (0x10000, 0x1D639E6), ".rdata": (0x1D70000, 0x1DEC8C4),
        ".data": (0x1DF0000, 0x1EF7B01), ".pdata": (0x1F00000, 0x1F10B3C),
        ".gxfg": (0x1F30000, 0x1F32610), ".tls": (0x1F40000, 0x1F40049),
        "gap": (0x1D639E6 + 1, 0x1D70000 - 1)}


def which(rva):
    for name, (a, z) in SECS.items():
        if a <= rva <= z:
            return name
    return "other"


missed = []
for p in range(0x2410000 >> 12):
    dp = disk[p << 12:(p + 1) << 12]
    wp = warm[p << 12:(p + 1) << 12]
    if dp != wp and any(dp):
        missed.append((p << 12, which(p << 12)))
print(f"pages where disk != warm: {len(missed)}")
cnt = defaultdict(int)
runs = []
for rva, sec in missed:
    cnt[sec] += 1
    if runs and runs[-1][1] + 0x1000 == rva and runs[-1][2] == sec:
        runs[-1][1] = rva
    else:
        runs.append([rva, rva, sec])
print("per section:", dict(cnt))
print("runs (start..end section):")
for a, z, sec in runs[:40]:
    print(f"  {a:#x}..{z + 0xFFF:#x} {sec}")

# disasm the page-stepper function tail
import capstone as cs
md = cs.Cs(cs.CS_ARCH_X86, cs.CS_MODE_64)
print("\n== disasm 0x1d53700..0x1d53a00 (page-stepper tail) ==")
for ins in md.disasm(disk[0x1D53700:0x1D53A00], 0x1D53700):
    mark = ""
    if ins.mnemonic.startswith(("xor", "rol", "ror", "shl", "shr", "not", "add")) and \
       ("0x1000" in ins.op_str or "0x40" == ins.op_str):
        mark = "  <<<<"
    print(f"  {ins.address:#09x}  {ins.bytes.hex():<20} {ins.mnemonic} {ins.op_str}{mark}")

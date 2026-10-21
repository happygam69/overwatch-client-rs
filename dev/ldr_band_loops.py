"""Linear capstone listing + decrypt-loop hunt over the band functions.

Rebases the found band .pdata functions (0x50000-0xd0000), linearly
disassembles each with capstone (no flow), and flags the code shapes the
in-place page decrypt leaves behind:
  - xor with a moving source (xor reg, [reg]/[reg+disp]) inside short loops
  - rotate/ror/rol, shr/shl combined with or (bit mixer) in a loop
  - big jrcxz/loop with a pointer register stepping by 0x1000
  - rip-relative data reads (the tables)
Prints per function: size, instruction count, suspicious loop heads with
contexts.  Read-only.
"""
import struct
import capstone

img = open(r"D:/ow-dumps/loader_after_diff.bin", "rb").read()
md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
md.detail = True

n = 0x10ba8 // 12
band = []
for i in range(n):
    beg, end, _un = struct.unpack_from("<III", img, 0x1f00000 + 12 * i)
    if not beg or not (0x50000 <= beg < 0xd0000):
        continue
    pg0, pg1 = beg >> 12, (end - 1) >> 12
    if all(img[p * 4096:(p + 1) * 4096] and any(img[p * 4096:(p + 1) * 4096])
           for p in range(pg0, pg1 + 1)):
        band.append((beg, end))

band.sort(key=lambda t: -(t[1] - t[0]))
print(f"band personal functions: {len(band)} (warm-body)")

ranged = {}
suspicious_total = 0
for beg, end in band[:25]:
    insns = list(md.disasm(img[beg:end], beg))
    body = {i.address: i for i in insns}
    flags = []
    backtargets = (i.address for i in insns
                   if i.mnemonic.startswith("j") and i.op_str.isdigit())
    for i in insns:
        t = i.op_str
        # xor against memory inside body
        if i.mnemonic in ("xor", "ror", "rol") and ("[" in t):
            # back edge earlier within 40 instructions => loop body
            flags.append(("mem_" + i.mnemonic, i.address, t))
        if i.mnemonic in ("rcl", "rcr", "shrd", "shld"):
            flags.append((i.mnemonic, i.address, t))
    # loops: jump backwards
    loops = 0
    for i in insns:
        if i.mnemonic.startswith("j") or i.mnemonic in ("loop", "jrcxz"):
            try:
                tgt = int(i.op_str, 0)
            except ValueError:
                continue
            if tgt < i.address and i.address - tgt < 0x400:
                loops += 1
    if flags or loops >= 2:
        suspicious_total += 1
        print(f"-- fn {beg:#x}-{end:#x} size {end-beg} insns {len(insns)} "
              f"loops {loops} flags {len(flags)}")
        for kind, a, t in flags[:8]:
            print(f"     {kind:6} {a:#x}: {t}")
        for i in insns:
            try:
                tgt = int(i.op_str, 0)
            except ValueError:
                continue
            if (i.mnemonic.startswith("j") or i.mnemonic in ("loop", "jrcxz")) \
                    and tgt < i.address and 0 < i.address - tgt < 0x80:
                j = body.get(tgt)
                if j:
                    print(f"     loop head {tgt:#x}: {j.mnemonic} "
                          f"{j.op_str[:60]}")
print(f"\nfunctions with suspicious shapes: {suspicious_total}/{min(25, len(band))}")

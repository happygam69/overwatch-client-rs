"""Find VEH/exception-handler anchors in the DISK loader: immediates
STATUS_ACCESS_VIOLATION (0xC0000005) and friends, with context disasm.

Usage: python dev/ldr_vehunt.py
"""
import capstone as cs

DISK = r"D:\ow-dumps\loader_disk_rva.bin"
b = open(DISK, "rb").read()
TEXT = (0x10000, 0x1D639E6)
md = cs.Cs(cs.CS_ARCH_X86, cs.CS_MODE_64)
md.detail = True

PATS = {0xC0000005: "AV", 0x80000001: "GUARD_PAGE", 0xC0000094: "DIV0",
        0xC0000096: "PRIV_INSTR", 0x0000000E: "E?"}
# also: protection constants used with NtProtect: 0x01 NOACCESS, 0x40 ERW
hits = []
for p in range(TEXT[0] >> 12, TEXT[1] >> 12):
    rva = p << 12
    pg = b[rva:rva + 0x1000]
    if not any(pg):
        continue
    for ins in md.disasm(pg, rva):
        if ins.mnemonic in ("nop", "jmp", "call", "ret"):
            if ins.mnemonic == "ret":
                break
            continue
        for op in ins.operands:
            if op.type == cs.x86.X86_OP_IMM and op.imm in PATS:
                hits.append((ins.address, ins.mnemonic, ins.op_str, PATS[op.imm]))
            elif op.type == cs.x86.X86_OP_IMM and op.imm == 0x40 and \
                    ins.mnemonic in ("mov", "push") and "d" in ins.mnemonic:
                pass  # too noisy
print(f"exception-constant hits: {len(hits)}")
for addr, mn, ops, tag in hits[:30]:
    print(f"  {addr:#x} [{tag}] {mn} {ops}")
    # context: 6 instructions before via re-disasm
    ctx = b[max(0, addr - 0x30):addr + 0x10]
    pre = list(md.disasm(ctx, max(0, addr - 0x30)))
    for ci in pre[-6:]:
        print(f"      {ci.address:#x}  {ci.mnemonic} {ci.op_str}")
    print()

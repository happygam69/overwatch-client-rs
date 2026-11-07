"""Byte-level scan of ConstructEngineSubsystems for 64-bit stores with a
disp32 in the engine-slot range, independent of instruction alignment.

Encodings matched:
  48 89 /r  mov r/m64, r64   with mod=10 (disp32)
  48 89 /r  with SIB (rm=100) and no index (SIB index=100) and base=rbx/rbp/rax
Reports every hit with its disp, modrm and the register being stored.
"""
from __future__ import annotations

IMAGE = "dev/data/offline_recover/module_offline.bin"
BASE = 0x140000000
START, END = 0x140554220, 0x1405559b8
REGS = ["rax", "rcx", "rdx", "rbx", "rsp", "rbp", "rsi", "rdi"]
BASE_REGS = ["rax", "rcx", "rdx", "rbx", "rsp", "rbp", "rsi", "rdi"]


def main() -> int:
    with open(IMAGE, "rb") as f:
        f.seek(START - BASE)
        code = f.read(END - START)
    hits = []
    for i in range(len(code) - 7):
        if code[i] != 0x48 or code[i + 1] != 0x89:
            continue
        modrm = code[i + 2]
        mod = modrm >> 6
        reg = (modrm >> 3) & 7
        rm = modrm & 7
        if mod != 2:
            continue
        if rm == 4:
            # SIB form: need base + no index
            sib = code[i + 3]
            if (sib & 7) not in (3, 4, 5):
                continue
            if ((sib >> 3) & 7) != 4:
                continue
            disp = int.from_bytes(code[i + 4:i + 8], "little", signed=False)
            hits.append((START + i, BASE_REGS[sib & 7], disp, REGS[reg], "sib"))
            continue
        disp = int.from_bytes(code[i + 3:i + 7], "little", signed=False)
        hits.append((START + i, BASE_REGS[rm], disp, REGS[reg], "modrm"))
    print(f"total 64-bit disp32 stores in range: {len(hits)}")
    slot_hits = [h for h in hits if 0x200 <= h[2] <= 0x4ff]
    for a, base, disp, src, form in sorted(slot_hits, key=lambda h: h[2]):
        print(f"  {a:012x}  [ {base} + {disp:#06x} ] <- {src}  ({form})")
    print(f"slot-range hits: {len(slot_hits)}")
    missing = [0x228, 0x230, 0x350, 0x418, 0x440]
    print("missing-slot byte hits:", [(hex(a), b, hex(d), s) for a, b, d, s, f in slot_hits if d in missing])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

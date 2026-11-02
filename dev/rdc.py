"""Disassemble a VA range, resolving RIP-relative operands to their float quads.

Usage: python dev/rdc.py 0x142cce430 0x142cce6e8
Reads the flat map at `dev/data/offline_recover/module_offline.bin`
(`offset = VA - 0x140000000`) and prints capstone's x86-64 Intel text with each
`[rip + X]` target annotated as `_0xADDR_(v0,v1,v2,v3)`.
"""
import re
import struct
import sys

import capstone

BASE = 0x140000000
MAP = "dev/data/offline_recover/module_offline.bin"


def main():
    start, end = (int(sys.argv[1], 16), int(sys.argv[2], 16)) if len(sys.argv) == 3 else (0x142cce430, 0x142cce6e8)
    data = open(MAP, "rb").read()
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    for insn in md.disasm(data[start - BASE : end - BASE], start):
        text = f"{insn.mnemonic:9} {insn.op_str}"
        target = re.search(r"rip \+ (0x[0-9a-f]+)", insn.op_str)
        if target:
            va = insn.address + insn.size + int(target.group(1), 16)
            raw = data[va - BASE : va - BASE + 16]
            if len(raw) == 16:
                vals = [struct.unpack("<f", raw[i : i + 4])[0] for i in range(0, 16, 4)]
                text += f"   ; 0x{va:x} = ({vals[0]:.9g}, {vals[1]:.9g}, {vals[2]:.9g}, {vals[3]:.9g})"
        print(f"{insn.address:#012x}  {text}")


if __name__ == "__main__":
    main()

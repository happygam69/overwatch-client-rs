"""Disassemble or hex-dump a virtual-address range of the local image.

The Ghidra bridge serves decompiled code well but its disassembly endpoints
return nothing, so when the register-level question is "which registers carry
the arguments" this reads the bytes of the offline-recovered module image
(base 0x140000000) and runs capstone over them.

Usage:
    python dev/rd.py 0x142cb8d20 0x142cb8dc0     # disassemble a range
    python dev/rd.py 0x142cb8d20 0x142cb8dc0 -x  # hex dump only

The image is read-only input: nothing here writes to it. Only conclusions
(behaviour + addresses) belong in the docs, never the listing.

`module_offline.bin` is a flat copy of the module: its header's section table
is a patched stub, so the mapping is `offset = VA - 0x140000000`.
"""

from __future__ import annotations

import sys

IMAGE = "dev/data/offline_recover/module_offline.bin"
BASE = 0x140000000


def read(va: int, length: int) -> bytes:
    with open(IMAGE, "rb") as f:
        f.seek(va - BASE)
        return f.read(length)


def main() -> int:
    if len(sys.argv) < 3:
        print(__doc__)
        return 1
    start = int(sys.argv[1], 16)
    end = int(sys.argv[2], 16)
    code = read(start, end - start)
    if "-x" in sys.argv:
        for i in range(0, len(code), 16):
            row = code[i:i + 16]
            print(f"{start + i:012x}  {row.hex(' ')}")
        return 0
    from capstone import CS_ARCH_X86, CS_MODE_64, Cs

    md = Cs(CS_ARCH_X86, CS_MODE_64)
    for ins in md.disasm(code, start):
        print(f"{ins.address:012x}  {ins.mnemonic:<8} {ins.op_str}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

"""Every instruction in `.text` whose memory operand has a given displacement.

    python dev/disp_scan.py 0x1ad8 [0xa8c ...] [--writes]

Why: a struct field read out of a decompiler is an offset, and the question
"who writes this field" is answered by the instruction that stores through it.
Linear disassembly of this image is unreliable in the obfuscated regions, so the
scan is anchored on the displacement bytes themselves: every occurrence of the
4-byte little-endian value is re-decoded from a few positions back with capstone
and kept only when a real instruction carries a memory operand with exactly that
displacement. Each hit is resolved to its `.pdata` chain root through
`dev/fn_extent.py`, because a fragment start is not a function start.

`--writes` keeps only stores (destination operand is memory); the default keeps
every memory operand. Read-only against
`dev/data/offline_recover/module_offline.bin`.
"""

from __future__ import annotations

import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import capstone

import fn_extent as fe

IMG = "dev/data/offline_recover/module_offline.bin"
BASE = 0x140000000
TEXT_LO = 0x1000
TEXT_HI = 0x2F2C3D3


def root_of(blob: bytes, ents, addr: int) -> int:
    """Return the chain root of the .pdata entry covering `addr` (or `addr`)."""
    k, _chain = fe.root_of(blob, ents, addr - BASE)
    if k is None:
        return addr
    return BASE + ents[k][0]


def main() -> int:
    args = sys.argv[1:]
    writes = "--writes" in args
    if writes:
        args.remove("--writes")
    disps = {int(a, 16) if a.lower().startswith("0x") else int(a) for a in args}
    blob = open(IMG, "rb").read()
    ents = fe.entries(blob)
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    md.detail = True
    for disp in sorted(disps):
        pat = struct.pack("<i", disp)
        pos = TEXT_LO
        hits = []
        while True:
            i = blob.find(pat, pos, TEXT_HI)
            if i < 0:
                break
            pos = i + 1
            best = None
            for back in range(2, 10):
                start = i - back
                if start < TEXT_LO:
                    continue
                ins = next(md.disasm(blob[start : start + 15], BASE + start, 1), None)
                if ins is None:
                    continue
                if ins.address + ins.size != BASE + i + 4:
                    continue
                mem = [
                    (k, o)
                    for k, o in enumerate(ins.operands)
                    if o.type == capstone.x86.X86_OP_MEM
                    and o.mem.disp == disp
                    and o.mem.base != capstone.x86.X86_REG_RIP
                ]
                if not mem:
                    continue
                # A shorter start can decode a *different* instruction that happens
                # to consume the same displacement (8b 89 .. vs 48 8b 89 ..). Keep
                # every candidate and prefer one that begins with a REX prefix,
                # then the longest; that resolves the pair the way Ghidra does.
                rex = 0x40 <= blob[start] <= 0x4F
                rank = (1 if rex else 0, back)
                if best is None or rank > best[0]:
                    best = (rank, ins, mem)
            if best is None:
                continue
            _rank, ins, mem = best
            k, o = mem[0]
            if writes and k != 0:
                continue
            base_name = ins.reg_name(o.mem.base) if o.mem.base else "-"
            hits.append(
                (ins.address, f"{ins.mnemonic} {ins.op_str}", base_name, k == 0)
            )
        print(f"== disp {disp:#x}: {len(hits)} memory operand(s)")
        groups: dict[int, list[str]] = {}
        for addr, text, base_name, is_store in hits:
            root = root_of(blob, ents, addr)
            groups.setdefault(root, []).append(
                f"    {addr:#x} {'W' if is_store else 'r'} [{base_name}]  {text}"
            )
        for root in sorted(groups):
            print(f"  function {root:#x}")
            for line in groups[root]:
                print(line)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

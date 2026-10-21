"""Scan a session image for code references to given rvas.

Writes to the descriptor cells come from constructors in .text; once we
hold them we hold the class registration sites, and those in turn hold
the six node-class code pointers.

Usage:
    python dev/xref_rva.py <img.bin> <rva> [rva ...]

Reads the image (session-coherent or with --base for a merged one),
disassembles every nonzero 4 KiB page with capstone, resolves
[rip+disp] and immediate targets, and prints each page that references
one of the requested rvas, with the instruction rva and encoding type.
Disassembly starts at the page base; alignment-imprecise starts mean
this can miss instructions whose first byte shares a boundary - we also
rescan from each offset stride of 1 up to 8 to catch shifted starts.
"""
import argparse
import json
import struct

import capstone as cs


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("img")
    ap.add_argument("rvas", nargs="+")
    ap.add_argument("--base", default="0")
    args = ap.parse_args()

    a = open(args.img, "rb").read()
    try:
        side = json.load(open(args.img + ".json", encoding="utf-8"))
        base = int(side["base"])
    except FileNotFoundError:
        base = int(args.base, 16)
    targets = set(int(r, 16) for r in args.rvas)

    md = cs.Cs(cs.CS_ARCH_X86, cs.CS_MODE_64)
    md.detail = True
    n = len(a) >> 12
    found = 0
    for p in range(n):
        pg = a[p << 12:(p + 1) << 12]
        if not any(pg):
            continue
        hits = []
        # disassemble from each of the first 8 offsets to be robust to
        # shifted instruction starts
        for stride in range(min(8, len(pg))):
            code = pg[stride:]
            for ins in md.disasm(code, (p << 12) + stride):
                m = ins.mnemonic
                if m in ("nop", "jmp", "call", "ret"):
                    if m == "ret":
                        break
                rip_t = None
                for op in ins.operands:
                    if op.type == cs.x86.X86_OP_MEM and op.mem.base == cs.x86.X86_REG_RIP:
                        rip_t = ins.address + ins.size + op.mem.disp
                    elif op.type == cs.x86.X86_OP_IMM and 0 < op.imm < 0x6000000:
                        rip_t = op.imm  # absolute immediate
                    if rip_t is not None and rip_t in targets:
                        hits.append((ins.address, rip_t, m, ins.op_str,
                                     "rip" if op.type == cs.x86.X86_OP_MEM
                                     else "imm"))
                        rip_t = None
        if hits:
            found += 1
            for ins_rva, tgt, m, opstr, kind in hits[:6]:
                print(f"{args.img.rsplit('\\\\', 1)[-1][:14]}: "
                      f"{ins_rva:#x} -> {tgt:#x} [{kind}] {m} {opstr}")
    print(f"scanned {n} pages of {args.img}; "
          f"{found} pages with hits")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

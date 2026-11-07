"""Find every RIP-relative reference to one or more data addresses in .text.

Usage:
    python dev/engine_ref_scan.py [--writes|--reads] 0x143b7f410 [more targets...]

The scan does not linearly disassemble 49 MB. A RIP-relative memory operand is
`... disp32` ending the instruction, so for a hit at byte position `p` the
target is `p + 4 + disp32`. The script walks the section with a sliding 4-byte
window, solves `disp32 = target - (p + 4)`, and confirms the match by
disassembling backwards from `p` for every plausible instruction length until
capstone produces exactly one instruction that ends at `p + 4`, carries a
`rip`-relative operand and whose computed target equals the requested address.

`--reads` / `--writes` filter on the memory operand's access flags (a `lea`
has no memory access, so it is dropped by both filters but still printed by
the unfiltered form).

Output: `insn_va,fn_va,text` where `fn_va` is the .pdata function start
containing the reference, or `?` when no entry covers it.
"""
from __future__ import annotations

import struct
import sys

import capstone

IMAGE = "dev/data/offline_recover/module_offline.bin"
BASE = 0x140000000
TEXT_START = 0x140001000
TEXT_END = 0x142F2C400


def load_pdata() -> list[tuple[int, int]]:
    """[(begin, end)] from the project image's own .pdata, as (VA, VA)."""
    pdata = open(IMAGE, "rb").read()[0x14450B000 - BASE : 0x144758A00 - BASE]
    out = []
    for i in range(0, len(pdata) - 12, 12):
        # RUNTIME_FUNCTION: BeginAddress, EndAddress, UnwindInfoAddress.
        begin, end = struct.unpack_from("<II", pdata, i)
        if begin == 0 and end == 0:
            continue
        out.append((BASE + begin, BASE + end))
    out.sort()
    return out


def find_fn(pdata: list[tuple[int, int]], va: int) -> int | None:
    lo, hi = 0, len(pdata)
    while lo < hi:
        mid = (lo + hi) // 2
        if pdata[mid][0] <= va:
            lo = mid + 1
        else:
            hi = mid
    for i in range(max(0, lo - 3), lo):
        b, e = pdata[i]
        if b <= va < e:
            return b
    return None


def main() -> int:
    args = sys.argv[1:]
    want_write = "--writes" in args
    want_read = "--reads" in args
    targets = [int(a, 16) for a in args if not a.startswith("--")]
    if not targets:
        print(__doc__)
        return 1
    img = open(IMAGE, "rb").read()
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    md.detail = True
    pdata = load_pdata()

    for target in targets:
        print(f"# target {target:#x}")
        n = 0
        for p in range(TEXT_START, TEXT_END):
            off = p - BASE
            d = target - (p + 4)
            if not (-0x80000000 <= d <= 0x7FFFFFFF):
                continue
            if img[off : off + 4] != struct.pack("<i", d):
                continue
            insn = None
            for length in range(7, 15):
                start = p + 4 - length
                if start < TEXT_START:
                    continue
                last = None
                for ins in md.disasm(img[start - BASE : p + 4 - BASE], start):
                    last = ins
                if last is None or last.address + last.size != p + 4:
                    continue
                for op in last.operands:
                    if (
                        op.type == capstone.x86.X86_OP_MEM
                        and op.mem.base == capstone.x86.X86_REG_RIP
                        and last.address + last.size + op.mem.disp == target
                    ):
                        if want_write and not (op.access & capstone.CS_AC_WRITE):
                            break
                        if want_read and not (op.access & capstone.CS_AC_READ):
                            break
                        insn = last
                        break
                if insn is not None:
                    break
            if insn is None:
                continue
            fn = find_fn(pdata, insn.address)
            fn_s = f"{fn:#x}" if fn is not None else "?"
            print(f"0x{insn.address:x},{fn_s},{insn.mnemonic} {insn.op_str}")
            n += 1
        print(f"# {n} references")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

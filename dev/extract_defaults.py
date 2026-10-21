"""Extract a class's field-default table from its descriptor's comparator/provider.

The compare-side function (descriptor slot static_68-ish) maps field hash to a
default value and answers "is the field at its default"; the provide-side
function writes the default into memory.  Both are compare chains:

    cmp edx, <HASH> ; je LEAF

with leaf kinds:
    comparator:  movss xmm0,[rip+C]  -> float default at C
                 xorps xmm0,xmm0     -> 0.0
                 cmp byte ptr [rcx], IMM / cmp dword ptr [rcx], IMM
    provider:    mov dword ptr [rcx], IMM  -> the default bits

This walks both and prints hash / kind / default, cross-checkable against the
generated class file in the reference checkout.

Usage: python extract_defaults.py <comparator> <provider> [label]
"""

from __future__ import annotations

import re
import struct
import sys

import regions as R
import disasm_field_sites as D


def float_at(space: R.Space, addr: int) -> float | None:
    b = space.read(addr, 4)
    if not b:
        return None
    return struct.unpack("<f", b)[0]


def walk(space: R.Space, start: int, size: int):
    """Yield (hash, kind, operand) for each compare leaf."""
    b = space.read(start, size)
    if not b:
        return
    insns = list(D.md.disasm(b, start))
    by_addr = {i.address: i for i in insns}

    # index of every `cmp edx, imm32`
    rows = []
    for i, ins in enumerate(insns):
        if ins.mnemonic != "cmp":
            continue
        m = re.match(r"(?:edx|ecx), (0x[0-9a-f]+)$", ins.op_str)
        if not m:
            continue
        h = int(m.group(1), 16)
        if h <= 0xFFFFFF:
            continue
        # find the je that follows and its target
        je = None
        for j in range(i + 1, min(i + 4, len(insns))):
            if insns[j].mnemonic == "je":
                je = insns[j]
                break
        if not je:
            continue
        target = int(je.op_str, 16)
        # forward-slice the leaf for its first value-defining instruction
        leaf = []
        addr = target
        for _ in range(8):
            ins = by_addr.get(addr)
            if ins is None:
                # disassemble onward from here
                chunk = space.read(addr, 0x20)
                ins = next(D.md.disasm(chunk, addr), None)
                if ins is None:
                    break
            leaf.append(ins)
            if ins.mnemonic in ("ret", "jmp", "jne", "je", "sete"):
                break
            addr += ins.size
        rows.append((h, leaf))
    return rows


def main() -> int:
    if len(sys.argv) < 3:
        print(__doc__)
        return 1
    cmp_addr = int(sys.argv[1], 16)
    prov_addr = int(sys.argv[2], 16)
    label = sys.argv[3] if len(sys.argv) > 3 else "class"
    space = R.Space(R.load("dumps_v2"))

    defaults: dict[int, tuple[str, str]] = {}
    for h, leaf in walk(space, cmp_addr, 0x200):
        desc = " ".join(f"{i.mnemonic} {i.op_str}" for i in leaf)[:100]
        defaults[h] = ("cmp", desc)
    for h, leaf in walk(space, prov_addr, 0x200):
        desc = " ".join(f"{i.mnemonic} {i.op_str}" for i in leaf)[:100]
        old = defaults.get(h)
        defaults[h] = (old[0] if old else "prov", (old[1] + " || " if old else "") + desc)

    out = f"{label}_defaults.txt"
    with open(out, "w", encoding="utf-8") as fh:
        fh.write(f"# {label} field defaults, from comparator {cmp_addr:#x} / provider {prov_addr:#x}\n")
        for h in sorted(defaults):
            fh.write(f"{h:08x}  {defaults[h][1]}\n")
    print(f"{len(defaults)} fields -> {out}")
    for h in sorted(defaults)[:40]:
        print(f"  {h:08x}  {defaults[h][1]}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

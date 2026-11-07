"""Enumerate the client's out-of-line 64-bit obfuscation primitives.

The engine and its subsystems store every pointer/handle in memory as
`transform(value, site constants)` rather than as a plain value. The transform
is usually inline, but a family of tiny out-of-line helpers of the shape

    mov rax, [rcx]        (or [rcx+disp])
    <ror/xor/add/sub/neg/not with imm64 or imm8>
    ...
    mov [rcx], rax
    ret

exists for fields that are encoded/decoded in more than one place. This script
finds them mechanically so the family can be modelled once:

    python dev/codec_scan.py            # summary: distinct op-sequences
    python dev/codec_scan.py --list     # every site
    python dev/codec_scan.py --const 0x4daa5a973c0808b6   # `movabs`/disp sites

Each printed site is `fn_va,size,n_ops,ops` with ops written as
`op:imm` (e.g. `sub:0x574da7a34c106b16`), in execution order.
"""
from __future__ import annotations

import collections
import struct
import sys

import capstone

IMAGE = "dev/data/offline_recover/module_offline.bin"
BASE = 0x140000000
TEXT_START = 0x140001000
TEXT_END = 0x142F2C400
MAX_LEN = 0x60

# mov rax,[rcx] / mov rax,[rcx+disp8] / mov rax,[rcx+disp32]
HEADS = (b"\x48\x8b\x01", b"\x48\x8b\x41", b"\x48\x8b\x81")
# mov [rcx],rax / mov [rcx+disp8],rax / mov [rcx+disp32],rax
TAILS = (b"\x48\x89\x01", b"\x48\x89\x41", b"\x48\x89\x81")

ALLOWED_MNEMONICS = {"mov", "ror", "rol", "xor", "sub", "add", "neg", "not", "imul", "shl", "shr", "sar", "movabs"}


def const_sites(img: bytes, targets: list[int]) -> None:
    """Sites in `.text` whose bytes carry one of the imm64s.

    `movabs r64, imm64` is `48 B8+r` plus the literal, so a plain byte match
    finds both the movabs form and a rip-relative displacement of the same
    value; to a reader they mean the same thing: the constant is used here.
    """
    for target in targets:
        needle = struct.pack("<Q", target)
        at = []
        start = 0
        while True:
            i = img.find(needle, start)
            if i < 0:
                break
            va = BASE + i
            if TEXT_START <= va < TEXT_END:
                at.append(va)
            start = i + 1
        print(f"# constant {target:#018x}: {len(at)} sites")
        for va in at[:40]:
            print(f"{va:#x}")
        if len(at) > 40:
            print(f"# ... {len(at) - 40} more")


def main() -> int:
    show_all = "--list" in sys.argv
    img = open(IMAGE, "rb").read()
    if "--const" in sys.argv:
        args = sys.argv[sys.argv.index("--const") + 1 :]
        const_sites(img, [int(a, 16) for a in args if not a.startswith("--")])
        return 0
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    md.detail = False

    sites = []
    for p in range(TEXT_START, TEXT_END):
        off = p - BASE
        if img[off : off + 3] not in HEADS:
            continue
        insns = []
        ok = True
        end = None
        for ins in md.disasm(img[off : off + MAX_LEN], p):
            if ins.mnemonic == "ret":
                end = ins.address + ins.size
                break
            if ins.mnemonic not in ALLOWED_MNEMONICS:
                ok = False
                break
            insns.append(ins)
        if not ok or end is None or not insns:
            continue
        last = insns[-1]
        if last.mnemonic != "mov" or "[rcx" not in last.op_str:
            continue
        ops = []
        for ins in insns[1:-1]:
            op = ins.mnemonic
            if op == "movabs":
                op = "mov"
            ops.append(f"{op}:{ins.op_str.split(', ')[-1]}")
        sites.append((p, end - p, ops))

    families = collections.Counter(tuple(o for o in ops) for _, _, ops in sites)
    print(f"sites={len(sites)} distinct_op_sequences={len(families)}")
    for seq, n in families.most_common():
        print(f"{n:5d}  {' '.join(seq) if seq else '(identity)'}")
    if show_all:
        print("--- sites ---")
        for va, size, ops in sorted(sites):
            print(f"{va:#x},{size:#x},{' '.join(ops) if ops else '(identity)'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

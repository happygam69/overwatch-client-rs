#!/usr/bin/env python3
"""Find the code that reads or writes one struct-field offset.

    python dev/find_field_writes.py 0x108              # stores only
    python dev/find_field_writes.py 0x108 --loads       # loads too
    python dev/find_field_writes.py 0x108 --near 0x1421

Recovering a format from the *consumer* side is easy; finding the *producer*
means asking "who writes this field?", and Ghidra's xrefs do not answer that
for a register-relative offset. This scans the recovered module image for the
load and store opcodes that carry the requested displacement, then disassembles
each hit on its own.

The scan is a byte-pattern search, not a linear disassembly: walking 8 MB with
capstone's detail mode at ~950k instructions takes minutes, while the forms are
a handful of `REX 89 /r` (store) and `REX 8B /r` (load) shapes. A hit inside a
longer instruction's operand bytes is possible, so each hit is re-disassembled
at its own address and kept only if the disassembly really has that
displacement.

Read-only: it reads the operator's recovered image and prints.
"""

from __future__ import annotations

import argparse
import pathlib
import sys

from capstone import CS_ARCH_X86, CS_MODE_64, Cs
from capstone.x86 import X86_OP_MEM

ROOT = pathlib.Path(__file__).resolve().parent.parent
IMAGE = ROOT / "dev" / "data" / "offline_recover" / "module_offline.bin"
BASE = 0x140000000
TEXT = (0x140001000, 0x1433C4000)
REX = (0x48, 0x49, 0x4C, 0x4D)
STORE = 0x89
LOAD = 0x8B


def load_image() -> bytes:
    if not IMAGE.is_file():
        sys.exit(f"missing image: {IMAGE}")
    return IMAGE.read_bytes()


def hits(code: bytes, opcode: int, want: int) -> list[int]:
    """Offsets of `mov r/m, r` / `mov r, r/m` with a displacement equal to `want`.

    A displacement below `0x80` assembles as a sign-extended **disp8** (mod = 1),
    not disp32, so both encodings must be scanned: reading only mod = 2 reports
    zero stores for every small offset such as `+0x48`. RIP-relative (mod = 0,
    rm = 5) and SIB-only forms are not register+disp and are not matched.
    Immediate stores (`mov [mem], imm`) use a different opcode and are not
    scanned, so this is a targeted lookup, not a complete census.
    """
    out = []
    n = len(code)
    fits_disp8 = -0x80 <= want <= 0x7F
    # 64-bit forms carry REX.W; 32-bit forms (`mov [rdi+0x114], eax`) do not, and
    # the counters in these structures are 32-bit, so both must be scanned.
    for prefix in tuple(bytes((r,)) for r in REX) + (b"",):
        needle = prefix + bytes((opcode,))
        at = 0
        while True:
            at = code.find(needle, at)
            if at < 0:
                break
            modrm = code[at + len(prefix) + 1]
            mod = modrm >> 6
            # The displacement follows modrm (+ SIB when rm = 4) in both forms.
            disp_at = at + len(prefix) + 3 if modrm & 7 == 4 else at + len(prefix) + 2
            if mod == 1 and fits_disp8 and disp_at + 1 <= n:
                if int.from_bytes(code[disp_at : disp_at + 1], "little", signed=True) == want:
                    out.append(at)
            elif mod == 2 and disp_at + 4 <= n:
                if int.from_bytes(code[disp_at : disp_at + 4], "little", signed=True) == want:
                    out.append(at)
            at += 1
    return out


def show(code: bytes, found: list[int], lo: int, want: int) -> int:
    md = Cs(CS_ARCH_X86, CS_MODE_64)
    md.detail = True
    shown = 0
    for at in found:
        insn = next(md.disasm(code[at : at + 12], lo + at), None)
        if insn is None or not any(op.type == X86_OP_MEM for op in insn.operands):
            continue
        print(f"  {insn.address:#011x}  {insn.mnemonic} {insn.op_str}")
        shown += 1
    return shown


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("offset", help="displacement to look for, e.g. 0x108")
    ap.add_argument("--loads", action="store_true", help="include loads (mov reg, [mem])")
    ap.add_argument("--near", default="", help="only addresses starting with this prefix")
    ap.add_argument("--limit", type=int, default=400, help="cap per opcode")
    args = ap.parse_args()

    want = int(args.offset, 0)
    blob = load_image()
    lo, hi = TEXT
    code = blob[lo - BASE : hi - BASE]

    for label, opcode in (("stores", STORE), ("loads", LOAD)):
        if label == "loads" and not args.loads:
            continue
        found = hits(code, opcode, want)
        if args.near:
            found = [a for a in found if f"{lo + a:#x}".startswith(args.near)]
        found = found[: args.limit]
        print(f"{label} touching {want:#x}: {len(found)} shown")
        show(code, found, lo, want)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

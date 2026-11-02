"""Recover a function's block graph from the image bytes, and tell a computed
jump from junk-filler obfuscation.

    python dev/probe_policy_blocks.py                      # default: 0x14202bc90
    python dev/probe_policy_blocks.py --at 0x14202bc90 --end 0x14202c394
    python dev/probe_policy_blocks.py --scan               # only list jump idioms

`0x14202bc90` (the movement hit policy) is a block dispatcher: a block ends by
decoding a constant into the next block's address and jumping through a
register. Ghidra follows those targets literally and lands mid-instruction in
unrelated functions, so its decompiler output for such an address is invalid.

This probe does what the decompiler cannot: it walks the function from its
entry, decodes each instruction, evaluates the register that carries the jump
target through a small constant interpreter (`mov`/`movabs`/`add`/`sub`/`xor`/
`or`/`and`/`shl`/`shr`/`ror`/`rol`/`lea`/`imul`/`neg`/`not`, a constant stack
for `push`/`pop`, and a volatile clobber after `call`), and records the
successor. A resolved target outside the function is reported as a wrong
decode - that is the signature to watch for, and the reason this tool exists.

Read-only: it reads the operator's image and prints. It does not write to the
install or the Ghidra project.
"""

from __future__ import annotations

import argparse
import pathlib
import struct
import sys

from capstone import CS_ARCH_X86, CS_MODE_64, Cs
from capstone.x86 import (
    X86_OP_IMM,
    X86_OP_MEM,
    X86_OP_REG,
    X86_REG_RIP,
)

DEFAULT_EXE = pathlib.Path(r"E:\Overwatch\_retail_\Overwatch.exe")
OFFLINE_IMAGE = pathlib.Path(__file__).resolve().parent / "data" / "offline_recover" / "module_offline.bin"
BASE = 0x140000000

R64 = {
    "rax": 0, "rcx": 1, "rdx": 2, "rbx": 3, "rsp": 4, "rbp": 5, "rsi": 6, "rdi": 7,
    "r8": 8, "r9": 9, "r10": 10, "r11": 11, "r12": 12, "r13": 13, "r14": 14, "r15": 15,
}
SUB = {
    "eax": "rax", "ax": "rax", "al": "rax", "ah": "rax",
    "ecx": "rcx", "cx": "rcx", "cl": "rcx", "ch": "rcx",
    "edx": "rdx", "dx": "rdx", "dl": "rdx", "dh": "rdx",
    "ebx": "rbx", "bx": "rbx", "bl": "rbx", "bh": "rbx",
    "esp": "rsp", "sp": "rsp", "ebp": "rbp", "bp": "rbp",
    "esi": "rsi", "si": "rsi", "edi": "rdi", "di": "rdi",
    "r8d": "r8", "r8w": "r8", "r8b": "r8",
    "r9d": "r9", "r9w": "r9", "r9b": "r9",
    "r10d": "r10", "r10w": "r10", "r10b": "r10",
    "r11d": "r11", "r11w": "r11", "r11b": "r11",
    "r12d": "r12", "r12w": "r12", "r12b": "r12",
    "r13d": "r13", "r13w": "r13", "r13b": "r13",
    "r14d": "r14", "r14w": "r14", "r14b": "r14",
    "r15d": "r15", "r15w": "r15", "r15b": "r15",
}
MASK = (1 << 64) - 1
VOLATILE = ("rax", "rcx", "rdx", "r8", "r9", "r10", "r11")


def find_image() -> bytes:
    """The offline-recovered flat module image: offset == VA - base.

    The install's `.text` is encrypted; only `.pdata` and the headers are
    plaintext there, so code bytes must come from this copy.
    """
    if OFFLINE_IMAGE.is_file():
        return OFFLINE_IMAGE.read_bytes()
    sys.exit(f"missing {OFFLINE_IMAGE}")


def sections(image: bytes) -> list[tuple[int, int, int, int]]:
    """(va, vsz, raw_off, raw_size) per section."""
    pe = struct.unpack_from("<I", image, 0x3C)[0]
    nsec = struct.unpack_from("<H", image, pe + 6)[0]
    opt = struct.unpack_from("<H", image, pe + 20)[0]
    s0 = pe + 24 + opt
    out = []
    for i in range(nsec):
        o = s0 + 40 * i
        vsz, va, rsz, roff = struct.unpack_from("<IIII", image, o + 8)
        out.append((va + BASE, vsz, roff, rsz))
    return out


class Image:
    def __init__(self, data: bytes) -> None:
        self.data = data
        # The recovered module is flat: its section table is a patched stub, so
        # map VA - BASE directly (see dev/rd.py).
        self.flat = data if data[:2] != b"MZ" or True else None
        self.sec = sections(data) if data[:2] == b"MZ" else []

    def read(self, va: int, n: int) -> bytes:
        if self.flat is not None:
            o = va - BASE
            return self.flat[o : o + n]
        for sva, _vsz, roff, rsz in self.sec:
            if sva <= va and va + n <= sva + rsz:
                o = roff + (va - sva)
                return self.data[o : o + n]
        raise ValueError(f"0x{va:x} not in a raw-backed section")

    def u64(self, va: int) -> int:
        return struct.unpack("<Q", self.read(va, 8))[0]


def canon(insn, op) -> str | None:
    name = insn.reg_name(op.reg)
    if name in R64:
        return name
    return SUB.get(name)


def const_of(insn, op, regs: dict[str, int]) -> int | None:
    if op.type == X86_OP_IMM:
        return op.imm & MASK
    if op.type == X86_OP_REG:
        r = canon(insn, op)
        return regs.get(r) if r else None
    return None


def eval_target(insn, regs: dict[str, int], img: Image) -> int | None:
    """The concrete address an indirect jmp/call leaves for."""
    if not insn.operands:
        return None
    op = insn.operands[0]
    if op.type == X86_OP_REG:
        r = canon(insn, op)
        return regs.get(r) if r else None
    if op.type == X86_OP_MEM:
        base = op.mem.base
        if base == X86_REG_RIP:
            slot = insn.address + insn.size + op.mem.disp
            try:
                return img.u64(slot)
            except ValueError:
                return None
    return None


def step(insn, regs: dict[str, int], stack: list[int], img: Image) -> None:
    """Apply the instruction's effect on the constant register file."""
    m = insn.mnemonic
    ops = insn.operands
    dst = ops[0] if ops else None

    def setr(r: str, v: int | None) -> None:
        if v is None:
            regs.pop(r, None)
        else:
            regs[r] = v & MASK

    def dstreg() -> str | None:
        return canon(insn, dst) if dst is not None and dst.type == X86_OP_REG else None

    if m in ("mov", "movabs"):
        r = dstreg()
        if r:
            setr(r, const_of(insn, ops[1], regs))
        return
    if m == "movzx" or m == "movsx" or m == "movsxd":
        r = dstreg()
        if r:
            v = const_of(insn, ops[1], regs)
            if v is not None and m == "movzx":
                setr(r, v & 0xFFFFFFFF)
            else:
                setr(r, None)
        return
    if m == "lea":
        r = dstreg()
        if r and ops[1].type == X86_OP_MEM and ops[1].mem.base != X86_REG_RIP:
            b = canon(insn, type("O", (), {"reg": ops[1].mem.base})()) if ops[1].mem.base else None
            bv = 0 if b is None else regs.get(b)
            iv = None if ops[1].mem.index == 0 else regs.get(insn.reg_name(ops[1].mem.index))
            if bv is not None and (iv is not None or ops[1].mem.index == 0):
                setr(r, bv + (iv or 0) * ops[1].mem.scale + ops[1].mem.disp)
            else:
                setr(r, None)
        elif r:
            setr(r, None)
        return
    if m in ("add", "sub", "xor", "or", "and", "shl", "shr", "sar", "ror", "rol", "imul"):
        r = dstreg()
        if not r:
            return
        a = regs.get(r)
        b = const_of(insn, ops[1], regs) if len(ops) > 1 else None
        if a is None or b is None:
            setr(r, None)
            return
        if m == "add":
            setr(r, a + b)
        elif m == "sub":
            setr(r, a - b)
        elif m == "xor":
            setr(r, a ^ b)
        elif m == "or":
            setr(r, a | b)
        elif m == "and":
            setr(r, a & b)
        elif m == "imul":
            setr(r, a * b)
        elif b == 0 or b > 63:
            setr(r, None)
        elif m == "shl":
            setr(r, a << b)
        elif m == "shr":
            setr(r, a >> b)
        elif m == "sar":
            setr(r, (a - (1 << 64) if a >> 63 else a) >> b)
        elif m == "ror":
            setr(r, ((a >> b) | (a << (64 - b))) & MASK)
        elif m == "rol":
            setr(r, ((a << b) | (a >> (64 - b))) & MASK)
        return
    if m == "neg":
        r = dstreg()
        if r:
            a = regs.get(r)
            setr(r, None if a is None else (-a) & MASK)
        return
    if m == "not":
        r = dstreg()
        if r:
            a = regs.get(r)
            setr(r, None if a is None else (~a) & MASK)
        return
    if m == "xchg":
        r1, r2 = dstreg(), canon(insn, ops[1]) if ops[1].type == X86_OP_REG else None
        if r1 and r2:
            a, b = regs.get(r1), regs.get(r2)
            setr(r1, b)
            setr(r2, a)
        return
    if m == "push":
        v = const_of(insn, ops[0], regs) if ops else None
        stack.append(v)  # type: ignore[arg-type]
        return
    if m == "pop":
        r = dstreg()
        if r:
            v = stack.pop() if stack else None
            setr(r, v if isinstance(v, int) else None)
        return
    if m == "call":
        for r in VOLATILE:
            regs.pop(r, None)
        return


def walk(img: Image, start: int, end: int, max_steps: int = 20000) -> dict:
    md = Cs(CS_ARCH_X86, CS_MODE_64)
    md.detail = True
    md.skipdata = True

    seen: set[int] = set()
    jumps: list[dict] = []
    unresolved: list[dict] = []
    outside: list[dict] = []
    bad: list[int] = []
    work = [start]
    steps = 0

    while work:
        addr = work.pop()
        if addr in seen or not (start <= addr < end):
            if not (start <= addr < end):
                outside.append({"from": addr})
            continue
        regs: dict[str, int] = {}
        stack: list[int] = []
        while addr < end:
            steps += 1
            if steps > max_steps:
                return {"error": "step budget"}
            if addr in seen:
                break
            seen.add(addr)
            chunk = img.read(addr, 16)
            insn = next(md.disasm(chunk, addr, count=1), None)
            if insn is None or insn.id == 0:  # skipdata byte
                bad.append(addr)
                addr += 1
                continue
            m = insn.mnemonic
            if m == "ret" or m == "jmp" and not insn.operands:
                break
            if m == "jmp":
                tgt = eval_target(insn, regs, img)
                if tgt is None:
                    unresolved.append({"at": insn.address, "text": f"{m} {insn.op_str}",
                                       "known": {k: hex(v) for k, v in regs.items()}})
                    break
                jumps.append({"at": insn.address, "text": f"{m} {insn.op_str}",
                              "target": tgt, "computed": insn.operands[0].type != X86_OP_IMM})
                work.append(tgt)
                break
            if m.startswith("j") and insn.operands and insn.operands[0].type == X86_OP_IMM:
                work.append(insn.operands[0].imm & MASK)
                step(insn, regs, stack, img)
                addr += insn.size
                continue
            step(insn, regs, stack, img)
            addr += insn.size
    return {"seen": seen, "jumps": jumps, "unresolved": unresolved,
            "outside": [o for o in outside if o["from"] != start], "bad": bad}


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--at", default="0x14202bc90")
    ap.add_argument("--end", default="0x14202c394")
    ap.add_argument("--scan", action="store_true")
    args = ap.parse_args()
    start, end = int(args.at, 16), int(args.end, 16)
    img = Image(find_image())

    if args.scan:
        md = Cs(CS_ARCH_X86, CS_MODE_64)
        md.detail = True
        n = 0
        for insn in md.disasm(img.read(start, end - start), start):
            if insn.mnemonic in ("jmp", "call") and insn.operands:
                if insn.operands[0].type in (X86_OP_REG, X86_OP_MEM):
                    n += 1
                    print(f"  0x{insn.address:x}  {insn.mnemonic} {insn.op_str}")
        print(f"-- {n} indirect jmp/call in {end - start} bytes (linear sweep)")
        return 0

    r = walk(img, start, end)
    if "error" in r:
        print(f"error: {r['error']}")
        return 1
    print(f"function 0x{start:x}..0x{end:x}  ({end - start} bytes)")
    print(f"reached   {len(r['seen'])} instructions")
    print(f"edges     {len(r['jumps'])} jumps")
    unresolved = r["unresolved"]
    outside = r["outside"]
    print(f"unresolved {len(unresolved)}   outside {len(outside)}   undecodable {len(r['bad'])}")
    for j in r["jumps"]:
        if j["computed"]:
            flag = "" if start <= j["target"] < end else "   <-- OUTSIDE"
            print(f"  0x{j['at']:x}  {j['text']:<24} -> 0x{j['target']:x}{flag}")
    for u in unresolved:
        print(f"  UNRESOLVED 0x{u['at']:x}  {u['text']}  known={u['known']}")
    for o in outside:
        print(f"  OUTSIDE jump landed at 0x{o['from']:x}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

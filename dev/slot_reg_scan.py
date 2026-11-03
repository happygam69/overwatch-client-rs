"""Find vtable-slot dispatches that go through a **register**, not a memory operand.

    python dev/slot_reg_scan.py 0x40 [0x100 ...] [--window N] [--reach BYTES]

Why this exists: a virtual method can be reached by three encodings and the
project's earlier scans only covered one of them plus `E8`:

  1. `call qword ptr [reg + disp]`      -- `FF /2` (covered by call_slot_scan.py);
  2. `mov reg, [vt + disp]` then `call reg` -- the slot value is loaded first
     and the call's operand is a register.  MSVC does this whenever the slot
     value is needed twice or the call is scheduled late;
  3. `mov reg, [vt + disp]` then `jmp reg` (tail call), or `jmp [reg+disp]`
     -- `FF /4`.

A "no caller" claim for a virtual method is only a measurement once all three
are counted.  This tool counts forms 2 and 3.

Method: scan `.text` for a 64-bit load (`REX.W 8B /r`) whose displacement is the
requested slot offset -- both the `disp8` and the `disp32` encoding decode to the
same `mem.disp`, so one test covers both.  From each such load the tool walks the
**control-flow graph** forward (worklist over successors, merging the taint set at
each join, bounded by `--window` decoded instructions and the containing `.pdata`
root's extent) carrying a taint set of registers that hold the loaded value.  The
taint survives `mov` copies, `push`/`pop` and stack spills; it is killed when the
register is overwritten and by a `call` to a caller-saved register's value.
Every `call reg` / `jmp reg` that consumes the taint is a hit.

Hits are **candidates**, because the taint is a conservative over-approximation
on merged paths: each is printed with the load address and its `.pdata` chain
root so the site can be read by hand.

Read-only: reads `dev/data/offline_recover/module_offline.bin`
(file offset == RVA, image base 0x140000000).
"""

from __future__ import annotations

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import capstone
from capstone import x86

import fn_extent as fe

IMG = "dev/data/offline_recover/module_offline.bin"
BASE = 0x140000000
TEXT_LO = 0x1000
TEXT_HI = 0x2F2C3D3

# REX with W set: a 64-bit load is what carries a vtable slot.
REX_W = {0x48, 0x49, 0x4C, 0x4D, 0x4A, 0x4B, 0x4E, 0x4F}

CALLER_SAVED = {n for n in ("rax", "rcx", "rdx", "rsi", "rdi", "r8", "r9", "r10", "r11")}


def _decode(md, blob, addr):
    off = addr - BASE
    return next(md.disasm(blob[off: off + 15], addr, 1), None)


def is_slot_load(md, ins, disp: int) -> bool:
    """`mov reg, qword ptr [base + disp]` -- the slot value is the *source*."""
    if ins.mnemonic != "mov" or len(ins.operands) != 2:
        return False
    dst, src = ins.operands
    if dst.type != x86.X86_OP_REG or src.type != x86.X86_OP_MEM:
        return False
    return src.mem.disp == disp and src.size == 8


def _memkey(op) -> tuple:
    return (op.mem.segment, op.mem.base, op.mem.index, op.mem.scale, op.mem.disp)


def extents(blob, ents, addr: int):
    """Union extent of the `.pdata` chain containing `addr`."""
    k, chain = fe.root_of(blob, ents, addr - BASE)
    if k is None:
        return None
    lo = hi = None
    for idx in chain:
        b, e, _u = ents[idx]
        lo = b if lo is None else min(lo, b)
        hi = e if hi is None else max(hi, e)
    return BASE + lo, BASE + hi


def walk_forward(md, blob, load_ins, disp: int, window: int, reach: int, bounds):
    """Carry the taint from a slot load to every `call reg` / `jmp reg` reachable."""
    hits = []
    start = load_ins.address + load_ins.size
    tainted0 = frozenset({load_ins.operands[0].reg})
    lo, hi = bounds if bounds else (start - reach, start + reach)

    # address -> frozenset of tainted regs on entry; memory taint is global
    seen: dict[int, frozenset] = {}
    mem_tainted: set = set()
    work = [(start, tainted0)]
    steps = 0
    while work:
        addr, taint = work.pop()
        if addr < lo or addr >= hi:
            continue
        if not (taint or mem_tainted):
            continue
        prev = seen.get(addr)
        if prev is not None and taint <= prev:
            continue
        seen[addr] = taint if prev is None else (prev | taint)
        ins = _decode(md, blob, addr)
        if ins is None:
            continue
        steps += 1
        if steps > window:
            break
        mn = ins.mnemonic
        ops = ins.operands
        nxt = ins.address + ins.size
        t = set(taint)

        if mn in ("call", "jmp") and ops:
            op0 = ops[0]
            if op0.type == x86.X86_OP_REG and op0.reg in t:
                hits.append((ins.address, mn, md.reg_name(op0.reg), load_ins.address))
                continue
            if mn == "jmp":
                if op0.type == x86.X86_OP_MEM and op0.mem.disp == disp:
                    hits.append((ins.address, "jmp-mem", "", load_ins.address))
                continue
            # an ordinary call: caller-saved registers are clobbered
            t = {r for r in t if md.reg_name(r) not in CALLER_SAVED}
            if t or mem_tainted:
                work.append((nxt, frozenset(t)))
            continue
        if mn == "ret":
            continue
        if mn == "jmp" and ops and ops[0].type == x86.X86_OP_IMM:
            work.append((ops[0].imm, frozenset(t)))
            continue
        if mn.startswith("j") and ops and ops[0].type == x86.X86_OP_IMM:
            work.append((ops[0].imm, frozenset(t)))
            work.append((nxt, frozenset(t)))
            continue
        if mn in ("xor", "sub") and len(ops) == 2 and ops[0].type == x86.X86_OP_REG \
                and ops[1].type == x86.X86_OP_REG and ops[0].reg == ops[1].reg:
            t.discard(ops[0].reg)
            work.append((nxt, frozenset(t)))
            continue
        if mn == "mov" and len(ops) == 2:
            dst, src = ops
            if dst.type == x86.X86_OP_REG:
                if src.type == x86.X86_OP_REG:
                    if src.reg in t:
                        t.add(dst.reg)
                    else:
                        t.discard(dst.reg)
                elif src.type == x86.X86_OP_MEM:
                    if _memkey(src) in mem_tainted:
                        t.add(dst.reg)
                    else:
                        t.discard(dst.reg)
                work.append((nxt, frozenset(t)))
                continue
            if dst.type == x86.X86_OP_MEM and src.type == x86.X86_OP_REG:
                if src.reg in t:
                    mem_tainted.add(_memkey(dst))
                else:
                    mem_tainted.discard(_memkey(dst))
                work.append((nxt, frozenset(t)))
                continue
            work.append((nxt, frozenset(t)))
            continue
        if mn == "push":
            if ops and ops[0].type == x86.X86_OP_REG and ops[0].reg in t:
                mem_tainted.add(("<push>",))
            work.append((nxt, frozenset(t)))
            continue
        if mn == "pop":
            if ops and ops[0].type == x86.X86_OP_REG:
                if ("<push>",) in mem_tainted:
                    t.add(ops[0].reg)
                else:
                    t.discard(ops[0].reg)
            work.append((nxt, frozenset(t)))
            continue
        # any other instruction: a write to a tainted register kills the taint
        _reads, writes = ins.regs_access()
        for r in writes:
            t.discard(r)
        work.append((nxt, frozenset(t)))
    return hits


def main() -> int:
    args = sys.argv[1:]
    window, reach = 400, 1200
    if "--window" in args:
        k = args.index("--window")
        window = int(args[k + 1])
        del args[k: k + 2]
    if "--reach" in args:
        k = args.index("--reach")
        reach = int(args[k + 1])
        del args[k: k + 2]
    disps = sorted({int(a, 16) for a in args})
    blob = open(IMG, "rb").read()
    ents = fe.entries(blob)
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    md.detail = True
    print(f"pdata entries: {len(ents):,}")

    for disp in disps:
        print(f"\n== slot +{disp:#x} -- register-indirect and tail forms")
        loads = 0
        seen = set()
        for_reach = 0
        pos = TEXT_LO
        while True:
            i = blob.find(b"\x8b", pos, TEXT_HI)
            if i < 0:
                break
            pos = i + 1
            if i == TEXT_LO or blob[i - 1] not in REX_W:
                continue
            start = i - 1
            ins = _decode(md, blob, BASE + start)
            if ins is None or not is_slot_load(md, ins, disp):
                continue
            loads += 1
            b = extents(blob, ents, BASE + start)
            for site, mn, reg, load in walk_forward(md, blob, ins, disp, window, reach, b):
                key = (site, mn)
                if key in seen:
                    continue
                seen.add(key)
                k, chain = fe.root_of(blob, ents, site - BASE)
                root = BASE + ents[k][0] if k is not None else site
                frag = "" if (k is None or len(chain) == 1) else f" (chain {len(chain)})"
                print(f"   {site:#x}  {mn} {reg:<5} load {load:#x}  in {root:#x}{frag}")
                for_reach += 1
        print(f"   slot loads with disp {disp:#x}: {loads}")
        print(f"   candidate register/tail dispatches: {for_reach}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

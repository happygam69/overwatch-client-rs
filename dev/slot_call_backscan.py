"""Count every register-indirect dispatch that consumes a vtable slot value.

    python dev/slot_call_backscan.py 0x40 [0x100 ...] [--module LO HI] [--ctx N]

The question this answers: when a virtual method is only ever reached through a
vtable, was *any* of the three encodings used?  `dev/callers.py` covers `E8`/`E9`/
`jcc`/8-byte pointers/4-byte RVAs; `dev/call_slot_scan.py` covers
`call qword ptr [reg+disp]` (`FF /2`, both disp8 and disp32).  What is left is:

  * `mov reg, [base+disp]` followed by `call reg`  -- the slot value is loaded
    into a register and the call operand is a register;
  * the same, followed by `jmp reg` (tail call);
  * `mov reg, [base+disp]` followed by `jmp qword ptr [reg+disp2]` (a slot
    value used as a second-level table base).

Method (this is the cheap direction): sweep each `.pdata` root function body
**once**, linearly, keeping a per-register and per-stack-slot "last load of a
requested slot" marker.  The marker survives `mov` copies, `push`/`pop` and
spills; it is cleared when the register is overwritten, by an arithmetic
instruction, and (for caller-saved registers) by any ordinary call.  When a
`call reg` / `jmp reg` consumes a marked register, that is a hit and the load
address is printed with it.

A linear sweep inside a `.pdata` root decodes some bytes that are padding or
data; a hit is therefore a **candidate** and is printed with its function so the
site can be read by hand.  Fragment starts are not function starts: every
function is the chain root from `dev/fn_extent.py` and the sweep covers the
union extent of the whole chain.

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

CALLER_SAVED = {"rax", "rcx", "rdx", "rsi", "rdi", "r8", "r9", "r10", "r11"}


def _memkey(op) -> tuple:
    return (op.mem.segment, op.mem.base, op.mem.index, op.mem.scale, op.mem.disp)


def roots(blob: bytes, ents):
    """(root_start, lo, hi) for every `.pdata` chain root, one entry per body.

    Built with a begin-address index instead of `fn_extent.covering`, so the walk
    is linear in the number of entries (a covering scan per entry is quadratic on
    a 200k-entry table).
    """
    by_begin = {b: k for k, (b, _e, _u) in enumerate(ents)}
    memo: dict[int, int] = {}

    def root_index(k: int) -> int:
        chain = []
        seen = set()
        cur = k
        while True:
            if cur in memo:
                r = memo[cur]
                break
            if cur in seen:
                r = cur
                break
            chain.append(cur)
            seen.add(cur)
            info = fe.unwind(blob, ents[cur][2])
            if info is None or info[5] is None:
                r = cur
                break
            nk = by_begin.get(info[5][0])
            if nk is None:
                r = cur
                break
            cur = nk
        for c in chain:
            memo[c] = r
        return r

    groups: dict[int, list[int]] = {}
    for k in range(len(ents)):
        groups.setdefault(root_index(k), []).append(k)
    out = []
    for rk, members in groups.items():
        lo = min(ents[m][0] for m in members)
        hi = max(ents[m][1] for m in members)
        out.append((BASE + ents[rk][0], BASE + lo, BASE + hi))
    return out


def sweep(md, blob, lo: int, hi: int, disps, hits, loads):
    """One linear pass over a function body, markers keyed by register/slot."""
    reg = {}   # reg name -> load address of a requested slot
    mem = {}   # mem key -> load address of a requested slot
    for ins in md.disasm(blob[lo - BASE: hi - BASE], lo):
        mn = ins.mnemonic
        ops = ins.operands
        if mn in ("call", "jmp") and ops:
            op0 = ops[0]
            if op0.type == x86.X86_OP_REG:
                name = md.reg_name(op0.reg)
                if name in reg:
                    hits.append((ins.address, mn, name) + reg[name])
            elif op0.type == x86.X86_OP_MEM:
                # a slot value used as a table base: jmp/call [reg+small]
                key = _memkey(op0)
                base = md.reg_name(key[1]) if key[1] else ""
                if base in reg and key[2] == 0:
                    hits.append((ins.address, mn + "-mem", base) + reg[base])
            if mn == "call":
                for r in list(reg):
                    if r in CALLER_SAVED:
                        del reg[r]
                mem.clear()
            continue
        if mn == "ret":
            reg.clear()
            mem.clear()
            continue
        if len(ops) == 2 and mn == "mov":
            dst, src = ops
            if dst.type == x86.X86_OP_REG:
                dname = md.reg_name(dst.reg)
                if src.type == x86.X86_OP_REG:
                    sname = md.reg_name(src.reg)
                    if sname in reg:
                        reg[dname] = reg[sname]
                    else:
                        reg.pop(dname, None)
                elif src.type == x86.X86_OP_MEM:
                    key = _memkey(src)
                    if key in mem:
                        reg[dname] = mem[key]
                    else:
                        reg.pop(dname, None)
                        if src.mem.disp in disps and src.size == 8:
                            reg[dname] = (ins.address, src.mem.disp)
                            loads[ins.address] = (md.reg_name(src.reg), src.mem.disp)
                else:
                    reg.pop(dname, None)
                continue
            if dst.type == x86.X86_OP_MEM and src.type == x86.X86_OP_REG:
                sname = md.reg_name(src.reg)
                key = _memkey(dst)
                if sname in reg:
                    mem[key] = reg[sname]
                else:
                    mem.pop(key, None)
                continue
        if mn == "push" and ops:
            if ops[0].type == x86.X86_OP_REG:
                name = md.reg_name(ops[0].reg)
                if name in reg:
                    mem[("<push>",)] = reg[name]
            continue
        if mn == "pop" and ops:
            if ops[0].type == x86.X86_OP_REG:
                name = md.reg_name(ops[0].reg)
                if ("<push>",) in mem:
                    reg[name] = mem[("<push>",)]
                else:
                    reg.pop(name, None)
            continue
        # any other instruction: writes kill the marker
        _reads, writes = ins.regs_access()
        for r in writes:
            reg.pop(md.reg_name(r), None)


def main() -> int:
    args = sys.argv[1:]
    ctx = 3
    mod = None
    if "--ctx" in args:
        k = args.index("--ctx")
        ctx = int(args[k + 1])
        del args[k: k + 2]
    if "--module" in args:
        k = args.index("--module")
        mod = (int(args[k + 1], 16), int(args[k + 2], 16))
        del args[k: k + 3]
    disps = sorted({int(a, 16) for a in args})
    blob = open(IMG, "rb").read()
    ents = fe.entries(blob)
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    md.detail = True
    rts = roots(blob, ents)
    print(f"pdata entries {len(ents):,}; root functions {len(rts):,}")
    if mod:
        rts = [r for r in rts if mod[0] <= r[1] < mod[1]]
        print(f"restricted to {mod[0]:#x}..{mod[1]:#x}: {len(rts):,} root functions")

    total_hits = []
    total_loads: dict[int, int] = {}
    for _r0, lo, hi in rts:
        hits: list = []
        loads: dict = {}
        sweep(md, blob, lo, hi, set(disps), hits, loads)
        for _a, (_base_reg, d) in loads.items():
            total_loads[d] = total_loads.get(d, 0) + 1
        for site, mn, name, load, d in hits:
            total_hits.append((site, mn, name, load, d, lo, hi))

    for d in disps:
        print(f"\n== slot +{d:#x}")
        print(f"   64-bit loads with disp {d:#x}: {total_loads.get(d, 0)}")
        print("   register-indirect / tail dispatches consuming one: "
              f"{len([h for h in total_hits if h[4] == d])}")
    print("\nsites (site, form, reg, load, slot, containing root function):")
    for site, mn, name, load, d, lo, hi in sorted(total_hits):
        print(f"   {site:#x}  {mn} {name:<5} load {load:#x} +{d:#x}  in {lo:#x}-{hi:#x}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

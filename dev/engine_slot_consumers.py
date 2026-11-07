"""Which functions consume which `[engine + slot]`, and what they do with it.

Phase A finds every RIP-relative reference to the engine global `0x143b7f410`
(the same sliding-`disp32` technique as `dev/engine_ref_scan.py`, vectorised
with numpy), and the `.pdata` function that contains it.

Phase B disassembles each such function's whole body and follows three
origins:

* `engine`   — a register loaded from `[rip + 0x143b7f410]`;
* `slot=S`   — a register loaded from `[engine + S]` (a subsystem object);
* `&slot=S`  — a register computed as `engine + S` (a slot address, `lea`).

Every memory operand whose base register carries one of those origins is
recorded: the slot, the displacement inside the object and whether it is read,
written or called through (a vtable slot). A call drops the volatile
registers; any other write to a register clears its origin; `xor rX,rX` zeroes
it. So each record is what the instructions say, not a guess.

Output: `dev/engine_slot_consumers.csv`
(`slot,fn,callsite,kind,disp,detail`) and a per-slot summary on stdout, with
each function's name from `docs/function-names.csv` when it has one.

Usage: python dev/engine_slot_consumers.py [--csv PATH] [--slot 0x2e8]
"""

from __future__ import annotations

import csv
import struct
import sys
from collections import defaultdict

import numpy as np

import capstone

IMAGE = "dev/data/offline_recover/module_offline.bin"
BASE = 0x140000000
TEXT_START = 0x140001000
TEXT_END = 0x142F2C400
ENGINE = 0x143B7F410
SLOT_LO = 0x220
SLOT_HI = 0x448

CS = capstone.CS_ARCH_X86
MD = capstone.CS_MODE_64
OP_MEM = capstone.x86.X86_OP_MEM
OP_REG = capstone.x86.X86_OP_REG
RIP = capstone.x86.X86_REG_RIP
AC_READ = capstone.CS_AC_READ
AC_WRITE = capstone.CS_AC_WRITE

# Windows x64: these are the registers a `call` may destroy.
VOLATILE = {"rax", "rcx", "rdx", "r8", "r9", "r10", "r11"}

WRITES_FIRST = {
    "mov", "movabs", "movaps", "movups", "movdqa", "movdqu", "movsd", "movss",
    "movsx", "movsxd", "movzx", "lea", "xor", "add", "sub", "and", "or", "adc",
    "sbb", "imul", "shl", "shr", "sar", "not", "neg", "inc", "dec", "bswap",
    "pop", "sete", "setne", "seta", "setb", "setg", "setl", "cmove", "cmovne",
    "cmova", "cmovb", "cmovg", "cmovl", "cmovae", "cmovbe", "cmovge", "cmovle",
    "xchg", "movq", "movd", "cvtsi2sd", "cvtsi2ss", "cvtss2sd", "cvtsd2ss",
}

# Sub-register -> 64-bit parent.
PARENT = {}
for _p, _subs in {
    "rax": "eax ax al ah",
    "rbx": "ebx bx bl bh",
    "rcx": "ecx cx cl ch",
    "rdx": "edx dx dl dh",
    "rsi": "esi si sil",
    "rdi": "edi di dil",
    "rbp": "ebp bp bpl",
    "rsp": "esp sp spl",
}.items():
    PARENT[_p] = _p
    for _s in _subs.split():
        PARENT[_s] = _p
for _n in range(8, 16):
    PARENT[f"r{_n}"] = f"r{_n}"
    PARENT[f"r{_n}d"] = f"r{_n}"
    PARENT[f"r{_n}w"] = f"r{_n}"
    PARENT[f"r{_n}b"] = f"r{_n}"


def reg_name(md, rid: int) -> str | None:
    return PARENT.get(md.reg_name(rid))


def load_pdata(img: bytes) -> list[tuple[int, int]]:
    pdata = img[0x14450B000 - BASE : 0x144758A00 - BASE]
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
    """The outermost range that contains `va`.

    `.pdata` lists mid-function fragments (chained entries) as their own
    ranges, so the innermost match is often a fragment start. The outermost
    containing range is the function a reader would call by name.
    """
    lo, hi = 0, len(pdata)
    while lo < hi:
        mid = (lo + hi) // 2
        if pdata[mid][0] <= va:
            lo = mid + 1
        else:
            hi = mid
    best = None
    for i in range(max(0, lo - 8), lo):
        b, e = pdata[i]
        if b <= va < e and (best is None or b < best):
            best = b
    return best


def ref_sites(img: bytes, target: int) -> list[int]:
    """Instruction addresses with a rip-relative operand equal to `target`.

    A rip-relative operand's `disp32` ends the instruction, so for a hit at
    byte `p` the instruction ends at `p + 4` and `target == (p + 4) + disp`.
    The `disp32` scan is vectorised; each hit is then confirmed by
    disassembling backwards for every plausible length and keeping the one
    instruction that ends exactly at `p + 4` with that target.
    """
    cache = f"dev/data/engine_refs_{target:x}.txt"
    try:
        return [int(x) for x in open(cache).read().split()]
    except FileNotFoundError:
        pass
    md = capstone.Cs(CS, MD)
    md.detail = True
    hits: list[int] = []
    chunk = 1 << 22
    start = TEXT_START
    while start < TEXT_END - 8:
        end = min(start + chunk, TEXT_END - 4)
        b = np.frombuffer(img, dtype=np.uint8, count=end - start + 4, offset=start - BASE)
        v = (
            b[:-3].astype(np.int64)
            | (b[1:-2].astype(np.int64) << 8)
            | (b[2:-1].astype(np.int64) << 16)
            | (b[3:].astype(np.int64) << 24)
        )
        # sign-extend from bit 31
        v = np.where(v >= (1 << 31), v - (1 << 32), v)
        pos = np.arange(start, start + len(v), dtype=np.int64)
        want = target - (pos + 4)
        for p in pos[v == want]:
            insn = confirm(md, img, int(p) + 4, target)
            if insn is not None:
                hits.append(insn)
        start = end
    open(cache, "w").write(chr(10).join(str(h) for h in hits))
    return hits


def confirm(md, img: bytes, end: int, target: int):
    for length in range(6, 16):
        start = end - length
        if start < TEXT_START:
            continue
        last = None
        for ins in md.disasm(img[start - BASE : end - BASE], start):
            last = ins
        if last is None or last.address + last.size != end:
            continue
        for op in last.operands:
            if op.type == OP_MEM and op.mem.base == RIP and last.address + last.size + op.mem.disp == target:
                return last.address
    return None


def kind_of(ins, op) -> str:
    if ins.mnemonic in ("call", "jmp"):
        return "call"
    if op.access == AC_WRITE:
        return "write"
    if op.access == AC_READ:
        return "read"
    # Capstone does not always flag access; fall back to position.
    return "write" if (ins.operands and ins.operands[0] is op and ins.mnemonic in WRITES_FIRST) else "read"


def scan_body(img: bytes, md, fn: int, end: int, slot: int | None):
    """Yield (callsite, kind, disp, detail, related_slot) for one function."""
    code = img[fn - BASE : end - BASE]
    eng: set[str] = set()
    slotobj: dict[str, int] = {}
    engaddr: dict[str, int] = {}
    vtbl: dict[str, int] = {}
    out = []
    for ins in md.disasm(code, fn):
        ops = ins.operands
        # 1. reads through known origins (before the destination is updated)
        for op in ops:
            if op.type != OP_MEM:
                continue
            base = reg_name(md, op.mem.base) if op.mem.base else None
            idx = reg_name(md, op.mem.index) if op.mem.index else None
            disp = op.mem.disp
            kind = kind_of(ins, op)
            if base in eng:
                if ins.mnemonic == "lea":
                    if SLOT_LO <= disp <= SLOT_HI and disp % 8 == 0 and slot in (None, disp):
                        out.append((ins.address, "addr", disp, "", disp))
                    continue
                if not (SLOT_LO <= disp <= SLOT_HI and disp % 8 == 0):
                    continue
                if slot is not None and disp != slot:
                    continue
                out.append((ins.address, "slot_" + kind, disp, "", disp))
            elif base in engaddr:
                s = engaddr[base]
                if slot is not None and s != slot:
                    continue
                if disp == 0 and kind != "write":
                    out.append((ins.address, "load", s, "", s))
                else:
                    out.append((ins.address, "field_" + kind, disp, f"slot+0x{disp:x}", s))
            elif base in slotobj:
                s = slotobj[base]
                if slot is not None and s != slot:
                    continue
                out.append((ins.address, "field_" + kind, disp, f"slot+0x{disp:x}", s))
                if kind != "write" and disp == 0:
                    vtbl[base] = s
            elif base in vtbl:
                s = vtbl[base]
                if slot is not None and s != slot:
                    continue
                out.append((ins.address, "vcall", disp, f"slot vtable+0x{disp:x}", s))
            elif idx in eng and disp % 8 == 0 and SLOT_LO <= disp <= SLOT_HI:
                out.append((ins.address, "indexed", disp, f"index={idx}", disp))

        # 2. destination: a fresh origin, or a clear
        dest = None
        if ops and ops[0].type == OP_REG and ins.mnemonic in WRITES_FIRST:
            dest = reg_name(md, ops[0].reg)
        if ins.mnemonic == "pop" and ops and ops[0].type == OP_REG:
            dest = reg_name(md, ops[0].reg)
        if dest:
            eng.discard(dest)
            slotobj.pop(dest, None)
            engaddr.pop(dest, None)
            vtbl.pop(dest, None)
            # `mov rX, [rip+ENGINE]`
            if ops and len(ops) >= 2 and ops[1].type == OP_MEM:
                src = ops[1]
                if src.mem.base == RIP and ins.address + ins.size + src.mem.disp == ENGINE:
                    eng.add(dest)
                else:
                    sbase = reg_name(md, src.mem.base) if src.mem.base else None
                    sdisp = src.mem.disp
                    if sbase in eng and SLOT_LO <= sdisp <= SLOT_HI and sdisp % 8 == 0:
                        slotobj[dest] = sdisp
                    elif sbase in engaddr and sdisp == 0:
                        slotobj[dest] = engaddr[sbase]
                    elif sbase in slotobj and sdisp == 0 and src.mem.index == 0:
                        slotobj[dest] = slotobj[sbase]
            elif ops and len(ops) >= 2 and ops[1].type == OP_REG:
                src = reg_name(md, ops[1].reg)
                if src in eng:
                    eng.add(dest)
                if src in slotobj:
                    slotobj[dest] = slotobj[src]
                if src in engaddr:
                    engaddr[dest] = engaddr[src]
            elif ins.mnemonic == "lea" and ops and len(ops) >= 2 and ops[1].type == OP_MEM:
                src = ops[1]
                sbase = reg_name(md, src.mem.base) if src.mem.base else None
                if sbase in eng and SLOT_LO <= src.mem.disp <= SLOT_HI and src.mem.disp % 8 == 0:
                    engaddr[dest] = src.mem.disp
        elif ins.mnemonic == "xor" and len(ops) == 2 and ops[0].type == OP_REG and ops[1].type == OP_REG and ops[0].reg == ops[1].reg:
            pass  # handled by dest clear above

        if ins.mnemonic in ("call", "ret", "jmp") or ins.mnemonic.startswith("j"):
            for r in VOLATILE:
                eng.discard(r)
                slotobj.pop(r, None)
                engaddr.pop(r, None)
                vtbl.pop(r, None)
    return out


def load_names() -> dict[int, str]:
    names = {}
    try:
        for r in csv.DictReader(open("docs/function-names.csv", encoding="utf-8", newline="")):
            try:
                a = int(r["address"], 16)
            except (ValueError, KeyError):
                continue
            names.setdefault(a, r["name"])
    except FileNotFoundError:
        pass
    return names


def main() -> int:
    args = sys.argv[1:]
    want_slot = int(args[args.index("--slot") + 1], 16) if "--slot" in args else None
    csv_path = args[args.index("--csv") + 1] if "--csv" in args else "dev/engine_slot_consumers.csv"
    img = open(IMAGE, "rb").read()
    md = capstone.Cs(CS, MD)
    md.detail = True
    pdata = load_pdata(img)
    names = load_names()

    sites = ref_sites(img, ENGINE)
    fns = {}
    for a in sites:
        f = find_fn(pdata, a)
        fns.setdefault(f, []).append(a)
    print(f"engine global: {len(sites)} rip references in {len(fns)} .pdata functions")

    rows = []
    per_slot = defaultdict(lambda: defaultdict(list))
    starts = {b: e for b, e in pdata}
    for fn in sorted(f for f in fns if f is not None):
        end = starts.get(fn)
        if end is None:
            continue
        for callsite, kind, disp, detail, rel in scan_body(img, md, fn, end, want_slot):
            rows.append(
                {
                    "slot": f"0x{rel:x}",
                    "fn": f"0x{fn:x}",
                    "callsite": f"0x{callsite:x}",
                    "kind": kind,
                    "disp": f"0x{disp:x}",
                    "detail": detail,
                }
            )
            per_slot[rel][kind].append((fn, callsite, disp, detail))

    rows.sort(key=lambda r: (r["slot"], r["fn"], r["callsite"]))
    with open(csv_path, "w", newline="", encoding="utf-8") as f:
        wr = csv.DictWriter(f, fieldnames=["slot", "fn", "callsite", "kind", "disp", "detail"])
        wr.writeheader()
        wr.writerows(rows)
    print(f"wrote {len(rows)} records to {csv_path}")

    for slot in sorted(per_slot):
        kinds = per_slot[slot]
        consumers = sorted({fn for v in kinds.values() for fn, *_ in v})
        label = ", ".join(
            f"{names.get(fn, f'{fn:#x}')}" for fn in consumers[:6]
        )
        total = sum(len(v) for v in kinds.values())
        print(
            f"+0x{slot:x}: {total:3d} accesses in {len(consumers):3d} fn  "
            f"{'+'.join(sorted(k for k in kinds)):<40} {label}"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

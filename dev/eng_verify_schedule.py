"""Re-derive ConstructEngineSubsystems' schedule from the instruction stream.

The census in `dev/engine_subsystems.csv` was a scripted measurement that was
re-checked once. This script derives every row mechanically from
`dev/engine_dump.asm` (capstone linear sweep of the function, written by
`dev/eng_dump.py`) and diffs the result against the CSV, so each row has a
decode rule instead of a note.

Rules, all from the instructions themselves:

* `Alloc(size, 0)` is `xor edx,edx; mov ecx,SIZE; call 0x141baff30`.
* Object identity is a token: the address of the `Alloc` call for an allocated
  object, or the address of a factory call for a returned one. `mov`/`lea`
  copy a token, a memory operand inherits its base register's token, and a
  call clobbers RAX/RCX/RDX, so a value that does not derive from an object
  has no token.
* A slot store is `mov qword ptr [ENGINE + DISP], REG` with DISP in
  0x220..0x448. Its producer is the constructor or factory that made the
  register's token. The alloc size is attributed only while the register
  still holds the object itself (pristine); an adjustor computed from it
  (slot 0x448) stores a derived value and has no size.
* A registration is the `RegisterWorldInterface` (0x141c52d10) call whose RDX
  argument carries the same token; the value can come from the register or
  from a slot the token was stored into.
* A vtable store is `lea REG,[rip + X]` followed by
  `mov qword ptr [OBJ (+off)], REG`.

Usage: python dev/eng_verify_schedule.py [--write]
Prints a per-row diff against dev/engine_subsystems.csv (exit 1 on mismatch)
and, with --write, writes dev/engine_subsystems.verified.csv.
"""

from __future__ import annotations

import csv
import re
import sys
from dataclasses import dataclass

DUMP = "dev/engine_dump.asm"
CSV = "dev/engine_subsystems.csv"
OUT = "dev/engine_subsystems.verified.csv"

ALLOC = 0x141BAFF30
REGISTER = 0x141C52D10
STUB_CTOR = 0x141C578C0
TAIL_ATTACH = 0x140BC7590
ENGINE_GLOBAL = 0x143B7F410
START = 0x140554220
END = 0x1405559B9
SLOT_LO = 0x220
SLOT_HI = 0x448
SLOT_STRIDE = 8

LINE = re.compile(r"^([0-9a-f]{12})\s+([a-z]+)\s*(.*)$")
REG = r"(?:r[a-z0-9]{1,2}|e[a-z]{2})"
MEM = re.compile(r"qword ptr \[([a-z0-9]+)(?: \+ (0x[0-9a-f]+))?\]")
DERIVED_MEM = re.compile(r"(?:[a-z]+ ptr )?\[([a-z0-9]+)(?: [+-] (?:0x[0-9a-f]+|[0-9]+)(?: [+-] [a-z0-9]+)?)?\]")


@dataclass
class Insn:
    addr: int
    mnem: str
    ops: str

    def __str__(self) -> str:
        return f"0x{self.addr:x} {self.mnem} {self.ops}".rstrip()


@dataclass
class Store:
    idx: int
    addr: int
    disp: int
    src: str
    producer: int | None
    alloc_size: int | None
    vtables: list[tuple[int, int]]
    token: int | None
    register_insn: int | None = None


def load(path: str = DUMP) -> list[Insn]:
    out = []
    for raw in open(path, encoding="utf-8"):
        m = LINE.match(raw.strip())
        if m:
            addr = int(m.group(1), 16)
            # The function is START..END-1 (`ret` at END-1, then int3). The
            # dump may be wider; anything past the padding is a neighbour's
            # call and must not be counted.
            if START <= addr < END:
                out.append(Insn(addr, m.group(2), m.group(3).strip()))
    return out


def imm(text: str) -> int | None:
    m = re.fullmatch(r"(-?)0x([0-9a-f]+)", text.strip())
    if not m:
        return None
    v = int(m.group(2), 16)
    return -v if m.group(1) else v


def call_target(ins: Insn) -> int | None:
    return imm(ins.ops) if ins.mnem == "call" else None


def dest_reg(ins: Insn) -> str | None:
    if ins.mnem not in ("mov", "movabs", "lea", "movsxd", "xor", "add", "sub", "xorps", "movups"):
        return None
    head = ins.ops.split(",", 1)[0].strip()
    return head if re.fullmatch(REG, head) else None


def mem(ops: str) -> tuple[str, int] | None:
    m = MEM.fullmatch(ops.strip())
    if not m:
        return None
    return m.group(1), int(m.group(2), 16) if m.group(2) else 0


def store_parts(ins: Insn) -> tuple[str, int, str] | None:
    if ins.mnem != "mov":
        return None
    m = re.fullmatch(r"(qword ptr \[[^\]]+\]), (\S+)", ins.ops.strip())
    if not m:
        return None
    base = mem(m.group(1))
    if base is None:
        return None
    return base[0], base[1], m.group(2)


def lea_target(ins: Insn, next_addr: int) -> tuple[str, int] | None:
    if ins.mnem != "lea":
        return None
    m = re.fullmatch(r"([a-z0-9]+), \[rip \+ (0x[0-9a-f]+)\]", ins.ops.strip())
    if not m:
        return None
    return m.group(1), next_addr + int(m.group(2), 16)


def rip_load(ins: Insn, next_addr: int) -> tuple[str, int] | None:
    if ins.mnem != "mov":
        return None
    m = re.fullmatch(r"([a-z0-9]+), qword ptr \[rip \+ (0x[0-9a-f]+)\]", ins.ops.strip())
    if not m:
        return None
    return m.group(1), next_addr + int(m.group(2), 16)


def derive(insns: list[Insn]) -> tuple[list[dict], dict]:
    CALL_CLOBBER = ("rax", "rcx", "rdx", "r8", "r9", "r10", "r11")
    engine_regs = {"rbx"}
    oid: dict[str, int] = {}
    pristine: dict[str, bool] = {}
    alloc_of: dict[int, int] = {}
    ctor_of: dict[int, int] = {}
    registered: dict[int, int] = {}
    slot_tok: dict[int, int | None] = {}
    stores: list[Store] = []
    vtables_by_tok: dict[int, list[tuple[int, int]]] = {}
    pending_lea: tuple[str, int] | None = None
    obj_tok: int | None = None
    rdx_oid: int | None = None
    alloc_size_seen: int | None = None
    inplace: dict[int, int] = {}  # stack offset -> token of the call that rewrote it
    inplace_arg: int | None = None

    for i, ins in enumerate(insns):
        nxt = insns[i + 1] if i + 1 < len(insns) else None
        next_addr = nxt.addr if nxt else ins.addr + 8
        target = call_target(ins)

        rl = rip_load(ins, next_addr)
        if rl and rl[1] == ENGINE_GLOBAL:
            engine_regs.add(rl[0])

        # Alloc(size, 0): the size is the immediate moved into ECX just before.
        if ins.mnem == "mov" and re.fullmatch(r"ecx, 0x[0-9a-f]+", ins.ops.strip()):
            if nxt and call_target(nxt) == ALLOC:
                alloc_size_seen = imm(ins.ops.split(",")[1])

        if target is not None:
            if target == ALLOC:
                tok = ins.addr
                oid["rax"] = tok
                pristine["rax"] = True
                obj_tok = tok
                alloc_of[tok] = alloc_size_seen if alloc_size_seen is not None else 0
                pending_lea = None
                rdx_oid = None
            else:
                # The constructor's RCX setup can be separated from the call
                # by argument LEAs, so scan back over mov/lea/xor to the last
                # `mov rcx, REG` before the call.
                is_ctor = False
                for k in range(i - 1, max(i - 5, -1), -1):
                    prev = insns[k]
                    if prev.mnem not in ("mov", "lea", "xor", "movabs"):
                        break
                    if re.fullmatch(r"rcx, [a-z0-9]+", prev.ops.strip()):
                        is_ctor = oid.get(prev.ops.split(",")[1].strip()) is not None
                        break
                # The null side of a guard passes RBP as the object: it builds
                # nothing, so the call only clobbers the caller-saved set.
                null_call = any(
                    re.fullmatch(r"(rdi|rdx), rbp", insns[k].ops.strip())
                    for k in range(i - 1, max(i - 4, -1), -1)
                )
                tok = obj_tok if is_ctor and obj_tok is not None else ins.addr
                if null_call:
                    rdx_oid = None
                    continue
                for r in CALL_CLOBBER:
                    oid.pop(r, None)
                    pristine.pop(r, None)
                if is_ctor and obj_tok is not None:
                    ctor_of[obj_tok] = target
                else:
                    ctor_of[tok] = target
                oid["rax"] = tok
                pristine["rax"] = True
                if target == REGISTER:
                    if rdx_oid is not None:
                        registered[rdx_oid] = ins.addr
                    rdx_oid = None
                if inplace_arg is not None:
                    inplace[inplace_arg] = ins.addr
                    inplace_arg = None

        lt = lea_target(ins, next_addr)
        if lt is not None:
            pending_lea = lt

        # `lea rcx,[rsp+X]` just before a call means the callee rewrites that
        # slot in place; the value read back belongs to the call.
        if ins.mnem == "lea" and re.fullmatch(r"rcx, \[rsp \+ 0x[0-9a-f]+\]", ins.ops.strip()):
            inplace_arg = int(ins.ops.split("+")[1].strip(" ]"), 16)
        elif ins.mnem not in ("lea", "mov", "push", "call", "test", "je", "jmp"):
            inplace_arg = None

        sp = store_parts(ins)
        if sp is not None:
            base, disp, src = sp
            if pending_lea and src == pending_lea[0]:
                vtok = oid.get(base)
                if vtok is not None:
                    vtables_by_tok.setdefault(vtok, []).append((disp, pending_lea[1]))
                pending_lea = None
            elif base in engine_regs and SLOT_LO <= disp <= SLOT_HI and disp % SLOT_STRIDE == 0:
                tok = oid.get(src)
                slot_tok[disp] = tok
                stores.append(
                    Store(
                        idx=i,
                        addr=ins.addr,
                        disp=disp,
                        src=src,
                        producer=ctor_of.get(tok) if tok is not None else None,
                        alloc_size=alloc_of.get(tok)
                        if tok is not None and pristine.get(src)
                        else None,
                        vtables=list(vtables_by_tok.get(tok, [])),
                        token=tok,
                    )
                )

        # Data flow for the register forms the body uses. The null side of a
        # `test rax,rax; je; ...; mov rax,rbp` guard is skipped: the linear
        # sweep lists both paths, and RBP is the zero register there.
        if re.fullmatch(r"[a-z0-9]+, rbp", ins.ops.strip()) or ins.ops.strip() == "rbp":
            pass
        elif ins.mnem in ("mov", "movsxd") and DERIVED_MEM.fullmatch(
            ins.ops.split(",", 1)[1].strip()
        ):
            dst, rest = ins.ops.split(",", 1)
            base = DERIVED_MEM.fullmatch(rest.strip()).group(1)
            oid[dst.strip()] = oid.get(base)
            pristine[dst.strip()] = False
            m = re.fullmatch(r"(?:[a-z]+ ptr )?\[rsp \+ (0x[0-9a-f]+)\]", rest.strip())
            if m and int(m.group(1), 16) in inplace:
                oid[dst.strip()] = inplace[int(m.group(1), 16)]
                pristine[dst.strip()] = True
        elif ins.mnem in ("mov", "lea") and re.fullmatch(r"[a-z0-9]+, [a-z0-9]+", ins.ops.strip()):
            dst, sr = (x.strip() for x in ins.ops.split(",", 1))
            oid[dst] = oid.get(sr)
            pristine[dst] = pristine.get(sr, False)
        elif ins.mnem == "lea":
            dst, rest = (x.strip() for x in ins.ops.split(",", 1))
            m = DERIVED_MEM.fullmatch(rest)
            if m:
                oid[dst] = oid.get(m.group(1))
                pristine[dst] = pristine.get(m.group(1), False)
            else:
                oid.pop(dst, None)
                pristine[dst] = False
        elif ins.mnem in ("add", "sub", "xor") and re.fullmatch(REG + r", " + REG, ins.ops.strip()):
            dst, sr = (x.strip() for x in ins.ops.split(",", 1))
            if oid.get(dst) is None and oid.get(sr) is not None:
                oid[dst] = oid[sr]
            pristine[dst] = False
        else:
            dr = dest_reg(ins)
            if dr:
                # Arithmetic keeps a value's origin (the 0x220 decode chain is
                # five adds/xors/rors over the decoded pointer); only a load
                # or a fresh constant replaces it.
                if ins.mnem in ("mov", "movabs", "movsxd", "movups", "movzx", "movsx", "lea"):
                    oid.pop(dr, None)
                pristine[dr] = False

        # `mov rdx, X` is the interface argument of the next RWI call.
        if ins.mnem == "mov" and ins.ops.strip().startswith("rdx, "):
            arg = ins.ops.strip()[len("rdx, "):]
            parsed = mem(arg)
            if parsed and parsed[0] in engine_regs:
                rdx_oid = slot_tok.get(parsed[1])
            elif re.fullmatch(REG, arg):
                rdx_oid = oid.get(arg)
            else:
                rdx_oid = None

    for st in stores:
        if st.token is not None and st.token in registered:
            st.register_insn = registered[st.token]

    rows = []
    for st in stores:
        vtab = ""
        if st.vtables and min(st.vtables)[0] == 0:
            vtab = f"0x{min(st.vtables)[1]:x}"
        rows.append(
            {
                "slot_offset": f"0x{st.disp:x}",
                "producer": f"0x{st.producer:x}" if st.producer else "",
                "alloc_size": f"0x{st.alloc_size:x}" if st.alloc_size is not None else "",
                "vtable": vtab,
                "store_insn": f"0x{st.addr:x}",
                "register_insn": f"0x{st.register_insn:x}" if st.register_insn else "",
                "src": st.src,
                "extra_vtables": ";".join(
                    f"+0x{off:x}=0x{t:x}" for off, t in sorted(st.vtables) if off != 0
                ),
            }
        )

    counts = {
        "instructions": len(insns),
        "alloc": sum(1 for ins in insns if call_target(ins) == ALLOC),
        "register": sum(1 for ins in insns if call_target(ins) == REGISTER),
        "tail_attach": sum(1 for ins in insns if call_target(ins) == TAIL_ATTACH),
        "stores": len(stores),
        "stores_without_registration": sum(1 for s in stores if s.register_insn is None),
    }
    return rows, counts


def main() -> int:
    insns = load()
    got, counts = derive(insns)
    print("counts:", counts)
    print(f"derived rows: {len(got)}")

    want = list(csv.DictReader(open(CSV, encoding="utf-8")))
    by_slot = {r["slot_offset"]: r for r in want}
    pairs = [
        ("producer", "ctor_or_source"),
        ("alloc_size", "alloc_size"),
        ("vtable", "vtable"),
        ("store_insn", "store_insn"),
        ("register_insn", "call_insn"),
    ]
    bad = 0
    for g in got:
        w = by_slot.get(g["slot_offset"])
        if not w:
            print(f"NEW ROW {g}")
            bad += 1
            continue
        for gk, wk in pairs:
            if g[gk] != w[wk]:
                print(f"MISMATCH {g['slot_offset']} {wk}: csv={w[wk]!r} derived={g[gk]!r}")
                bad += 1
    seen = {g["slot_offset"] for g in got}
    for slot in by_slot:
        if slot not in seen:
            print(f"MISSING ROW {slot}: csv={by_slot[slot]}")
            bad += 1

    gaps = [o for o in range(SLOT_LO, SLOT_HI + 1, SLOT_STRIDE) if f"0x{o:x}" not in seen]
    print("gaps:", [f"0x{o:x}" for o in gaps])

    if "--write" in sys.argv:
        with open(OUT, "w", newline="", encoding="utf-8") as f:
            wr = csv.DictWriter(f, fieldnames=list(got[0].keys()))
            wr.writeheader()
            wr.writerows(got)
        print(f"wrote {OUT}")

    print("OK" if bad == 0 else f"{bad} mismatches")
    return 0 if bad == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())

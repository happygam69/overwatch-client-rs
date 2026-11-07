"""Profile every subsystem class: its constructor and its vtables.

For each row of `dev/engine_subsystems.verified.csv` this reads the producer's
own body and the vtables the class installs (the schedule block's inline
stores plus the stores the constructor makes itself):

* does the constructor call the shared stub `0x141c578c0` (the abstract base);
* which vtables land at which object offset (`lea reg,[rip+X]` then
  `mov [this+off], reg`);
* which fields the constructor initialises directly (`mov [this+off], imm` and
  `mov [this+off], reg` for non-vtable registers), with their values;
* which functions it calls (`call rel32`);
* every vtable's entries, and for each entry its first calls and static
  references — with names from `docs/function-names.csv` where a target has
  one.

This is a **one level** read: a helper the constructor calls is listed by
address, not expanded. It answers "what shape is this class", and it is the
input to naming; a name is only written down where the evidence is read.

Output: `dev/engine_classes.json` (all of it) and a per-slot summary on stdout.

Usage: python dev/eng_class_profile.py [--slot 0x288] [--vtable 0x142f84920]
"""

from __future__ import annotations

import csv
import io
import json
import re
import sys
from collections import OrderedDict

import capstone

IMAGE = "dev/data/offline_recover/module_offline.bin"
BASE = 0x140000000
TEXT_LO = 0x140001000
TEXT_HI = 0x142F2C400
STUB = 0x141C578C0
OP_MEM = capstone.x86.X86_OP_MEM
OP_REG = capstone.x86.X86_OP_REG
OP_IMM = capstone.x86.X86_OP_IMM
RIP = capstone.x86.X86_REG_RIP
REG = r"(?:r[a-z0-9]{1,2}|e[a-z]{2})"

WRITES_FIRST = {
    "mov", "movabs", "movups", "movaps", "movdqu", "movdqa", "movss", "movsd",
    "lea", "xor", "add", "sub", "and", "or", "adc", "sbb", "imul", "shl",
    "shr", "sar", "not", "neg", "inc", "dec", "bswap", "pop", "movsxd",
    "movzx", "movsx", "cmove", "cmovne", "cvtsi2ss", "cvtsi2sd",
}


def load_image():
    return open(IMAGE, "rb").read()


def load_pdata(img: bytes):
    pdata = img[0x14450B000 - BASE : 0x144758A00 - BASE]
    out = []
    for i in range(0, len(pdata) - 12, 12):
        # RUNTIME_FUNCTION: BeginAddress, EndAddress, UnwindInfoAddress.
        begin = int.from_bytes(pdata[i : i + 4], "little")
        end = int.from_bytes(pdata[i + 4 : i + 8], "little")
        if begin == 0 and end == 0:
            continue
        out.append((BASE + begin, BASE + end))
    out.sort()
    return out


def fn_end(pdata, va):
    """End of the `.pdata` range that starts at `va`.

    A small leaf function (no stack adjustment) can be missing from `.pdata`
    entirely. The stub `0x141c578c0` and the `+0x220` codec `0x1405437f0` are
    both such functions: each is a `call` target, so it is a real function, but
    the next `RUNTIME_FUNCTION` belongs to the function after it. For those the
    bound used here is the next range's start.
    """
    lo, hi = 0, len(pdata)
    while lo < hi:
        mid = (lo + hi) // 2
        if pdata[mid][0] <= va:
            lo = mid + 1
        else:
            hi = mid
    for i in range(max(0, lo - 4), lo):
        if pdata[i][0] == va:
            return pdata[i][1], True
    if lo < len(pdata) and pdata[lo][0] > va and pdata[lo][0] - va <= 0x2000:
        return pdata[lo][0], False
    return None, False


def load_names():
    names = {}
    for r in csv.DictReader(open("docs/function-names.csv", encoding="utf-8", newline="")):
        try:
            a = int(r["address"], 16)
        except (ValueError, KeyError):
            continue
        names.setdefault(a, r["name"])
    return names


def is_code(va):
    return TEXT_LO <= va < TEXT_HI


def rip_target(ins):
    for op in ins.operands:
        if op.type == OP_MEM and op.mem.base == RIP:
            return ins.address + ins.size + op.mem.disp
    return None


def call_target(ins):
    if ins.mnemonic != "call":
        return None
    for op in ins.operands:
        if op.type == OP_IMM:
            return op.imm
    return None


def reg_name(md, rid):
    return md.reg_name(rid) if rid else None


def first_written_reg(md, ins):
    if ins.mnemonic not in WRITES_FIRST:
        return None
    if not ins.operands or ins.operands[0].type != OP_REG:
        return None
    return reg_name(md, ins.operands[0].reg)


def profile_ctor(img, md, pdata, producer):
    """Read one constructor body: this-loads, vtable stores, field inits, calls."""
    end, exact = fn_end(pdata, producer)
    if end is None:
        return {"error": "no .pdata range", "producer": producer}
    code = img[producer - BASE : end - BASE]
    this_regs = {"rcx"}  # the object arrives in RCX on Windows x64
    vtable_src = {}  # reg -> vtable address (from a preceding lea)
    vtables = []
    fields = []
    calls = []
    stub = False
    stack_self = False
    for ins in md.disasm(code, producer):
        if not exact and ins.mnemonic in ("ret", "jmp", "ud2", "int3"):
            end = ins.address + ins.size  # a leaf function: stop at its terminator
            break
        t = call_target(ins)
        if t is not None:
            calls.append(t)
            if t == STUB:
                stub = True
            # volatile registers are gone; RCX no longer means `this`
            this_regs -= {"rax", "rcx", "rdx", "r8", "r9", "r10", "r11"}
            vtable_src.clear()
            continue
        dest = first_written_reg(md, ins)
        rt = rip_target(ins)
        if dest and ins.mnemonic == "lea" and rt is not None:
            vtable_src[dest] = rt
            continue
        if dest and ins.mnemonic == "lea" and ins.operands[1].type == OP_MEM:
            base = reg_name(md, ins.operands[1].mem.base)
            if base in this_regs and ins.operands[1].mem.disp == 0:
                stack_self = True  # `lea rX,[this]`; rX is the same object
                this_regs.add(dest)
            continue
        if dest and ins.mnemonic == "mov" and len(ins.operands) >= 2:
            src = ins.operands[1]
            if src.type == OP_REG:
                sr = reg_name(md, src.reg)
                if sr in this_regs:
                    this_regs.add(dest)
                else:
                    this_regs.discard(dest)
                continue
            if src.type == OP_MEM and src.mem.base != RIP:
                base = reg_name(md, src.mem.base)
                if base in this_regs and src.mem.disp == 0:
                    this_regs.add(dest)  # loaded the object pointer from [this]
                else:
                    this_regs.discard(dest)
                continue
            if src.type == OP_MEM and src.mem.base == RIP:
                this_regs.discard(dest)
                continue
            if src.type == OP_IMM:
                this_regs.discard(dest)
                continue
        # a store into [this+off]
        if ins.mnemonic in ("mov", "movups", "movaps", "movss", "movsd") and ins.operands:
            dst = ins.operands[0]
            if dst.type == OP_MEM and dst.mem.base != RIP:
                base = reg_name(md, dst.mem.base)
                if base in this_regs:
                    off = dst.mem.disp
                    src = ins.operands[1] if len(ins.operands) > 1 else None
                    if src is not None and src.type == OP_REG:
                        sr = reg_name(md, src.reg)
                        if sr in vtable_src:
                            vtables.append({"offset": off, "vtable": vtable_src[sr], "site": ins.address})
                        else:
                            fields.append({"offset": off, "kind": "reg", "value": sr, "site": ins.address})
                    elif src is not None and src.type == OP_IMM:
                        fields.append({"offset": off, "kind": "imm", "value": src.imm, "site": ins.address})
        if dest:
            if dest not in this_regs:
                vtable_src.pop(dest, None)
    return OrderedDict(
        producer=f"0x{producer:x}",
        body_end=f"0x{end:x}",
        size=f"0x{end - producer:x}",
        bound="pdata" if exact else "leaf-terminator",
        stub_base=stub or producer == STUB,
        vtables=vtables,
        fields=fields,
        calls=[f"0x{c:x}" for c in calls],
    )


def profile_vtable(img, md, names, vt, end=None, limit=128):
    """Read a vtable between `vt` and `end` (the next known vtable, at most).

    There is no length field: a vtable ends where the next class's begins. The
    caller passes the next address in the union of every vtable this pass
    knows, which is what keeps a read from walking into its neighbour. Entries
    that are not code stop the read early.

    The word that stopped the read is recorded as a final entry with
    `code: False` and a `reason`: `flush` (the bank's non-code separator),
    `next-table` (the next known vtable starts here) or `limit` (the cap).
    """
    entries = []
    reason, raw = "limit", None
    for i in range(limit):
        off = vt + i * 8
        if end is not None and off >= end:
            reason = "next-table"
            break
        if not (BASE <= off < BASE + len(img) - 8):
            reason = "image-end"
            break
        val = int.from_bytes(img[off - BASE : off - BASE + 8], "little")
        if not is_code(val):
            reason, raw = "flush", val
            break
        entries.append({"index": i, "target": f"0x{val:x}", "code": True, "name": names.get(val)})
    entries.append({"index": len(entries), "code": False, "reason": reason, "value": raw})
    return entries


def profile_method(img, md, names, target, max_insn=60):
    """First-level look at a vtable method: calls, static refs, immediates."""
    calls, refs = [], []
    for i, ins in enumerate(md.disasm(img[target - BASE : target - BASE + 320], target)):
        if i >= max_insn or ins.mnemonic == "ret":
            break
        t = call_target(ins)
        if t is not None:
            calls.append({"target": f"0x{t:x}", "name": names.get(t)})
        rt = rip_target(ins)
        if rt is not None and is_code(rt):
            refs.append(f"0x{rt:x}")
    static = None
    two = list(md.disasm(img[target - BASE : target - BASE + 16], target))[:2]
    if (
        len(two) == 2
        and two[0].mnemonic == "lea"
        and two[0].op_str.startswith("rax, [rip")
        and two[1].mnemonic == "ret"
    ):
        static = next(iter([f"0x{rip_target(two[0]):x}" if rip_target(two[0]) is not None else None]))
    return {"calls": calls, "code_refs": refs, "returns_static": static}


def main() -> int:
    args = sys.argv[1:]
    only_slot = int(args[args.index("--slot") + 1], 16) if "--slot" in args else None
    only_vt = int(args[args.index("--vtable") + 1], 16) if "--vtable" in args else None
    out_path = args[args.index("--json") + 1] if "--json" in args else "dev/engine_classes.json"

    img = load_image()
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
    md.detail = True
    pdata = load_pdata(img)
    names = load_names()

    rows = list(csv.DictReader(open("dev/engine_subsystems.verified.csv", encoding="utf-8", newline="")))
    ctors = OrderedDict()
    for r in rows:
        slot = int(r["slot_offset"], 16)
        if only_slot is not None and slot != only_slot:
            continue
        p = int(r["producer"], 16)
        if p not in ctors:
            ctors[p] = profile_ctor(img, md, pdata, p)

    # the vtables: those the schedule block stores and those the ctor installs
    vt_set = OrderedDict()
    for r in rows:
        slot = int(r["slot_offset"], 16)
        if only_slot is not None and slot != only_slot:
            continue
        if r["vtable"]:
            vt_set.setdefault(int(r["vtable"], 16), None)
        for part in r["extra_vtables"].split(";"):
            if "=" in part:
                vt_set.setdefault(int(part.split("=")[1], 16), None)
    for c in ctors.values():
        for v in c.get("vtables", []):
            vt_set.setdefault(v["vtable"], None)

    known = sorted(vt_set)
    bounds = {v: n for v, n in zip(known, known[1:])}
    for vt in list(vt_set):
        if only_vt is not None and vt != only_vt:
            continue
        entries = profile_vtable(img, md, names, vt, end=bounds.get(vt))
        for e in entries:
            if e.get("code"):
                e.update(profile_method(img, md, names, int(e["target"], 16)))
        vt_set[vt] = entries

    data = {"ctors": {f"0x{int(k):x}": v for k, v in ctors.items()}, "vtables": {f"0x{int(k):x}": v for k, v in vt_set.items()}}
    with open(out_path, "w", encoding="utf-8") as f:
        json.dump(data, f, indent=1)

    print(f"{len(ctors)} constructors, {len(vt_set)} vtables -> {out_path}")
    for r in rows:
        slot = int(r["slot_offset"], 16)
        if only_slot is not None and slot != only_slot:
            continue
        c = ctors.get(int(r["producer"], 16))
        if not c or "error" in c:
            print(f"+0x{slot:x}  {r['producer']}  ERROR {c.get('error') if c else '?'}")
            continue
        vts = ", ".join(f"+0x{v['offset']:x}=0x{v['vtable']:x}" for v in c["vtables"])
        imm = sum(1 for f in c["fields"] if f["kind"] == "imm")
        print(
            f"+0x{slot:x}  ctor {r['producer']} size={c['size']:>5} "
            f"stub={'y' if c['stub_base'] else 'n'}  vt=[{vts}]  fields={len(c['fields'])}({imm} imm)  calls={len(c['calls'])}"
        )
    if only_vt is not None and only_vt in vt_set:
        print(f"\nvtable 0x{only_vt:x}:")
        for e in vt_set[only_vt]:
            if not e.get("code"):
                print(f"  [{e['index']}] non-code {e['target']}")
                continue
            cs = ", ".join(
                f"{c['name'] or c['target']}" for c in e.get("calls", [])
            )
            print(f"  [{e['index']}] {e['target']} {e['name'] or ''}  calls: {cs}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

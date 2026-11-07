"""Build the engine-slot table by walking the disassembly block by block.

A block is the instruction range between two consecutive `call 0x141c52d10`
(the registration call that ends every subsystem construction).  For each
block this extracts: Alloc size, constructor call, vtable stores into the new
object, the engine-slot store (if any) and the registration call address.
"""
from __future__ import annotations
import json
import re

DUMP = "dev/engine_dump.asm"
ALLOC = "0x141baff30"
RWI = "0x141c52d10"
BASE = 0x140000000
STORE = re.compile(r"^mov\s+(qword|dword) ptr \[(\w+)(?: \+ (0x[0-9a-f]+))?\], (\w+)$")
LEA = re.compile(r"^lea\s+(\w+), \[rip \+ (0x[0-9a-f]+)\]$")
MOVI = re.compile(r"^mov\s+(\w+), qword ptr \[rip \+ (0x[0-9a-f]+)\]$")
ECX = re.compile(r"^mov\s+ecx, (0x[0-9a-f]+)$")


def load():
    rows = []
    for line in open(DUMP, encoding="utf-8"):
        line = line.rstrip("\n")
        if not line.strip():
            continue
        addr, rest = line.split("  ", 1)
        rows.append((int(addr, 16), rest.strip()))
    return rows


def main() -> int:
    rows = load()
    lens = {}
    for i, (a, _) in enumerate(rows):
        lens[a] = (rows[i + 1][0] if i + 1 < len(rows) else a + 16) - a

    def resolve(a, disp):
        return a + lens[a] + disp

    rwi = [a for a, t in rows if t.startswith("call ") and t.split()[-1] == RWI]
    print(f"# rwi calls: {len(rwi)}")
    out = []
    prev = None
    for r in rwi:
        lo = 0 if prev is None else prev
        block = [(a, t) for a, t in rows if lo < a <= r]
        prev = r
        size = None
        ctor = None
        vtables = []
        stores = []
        alloc_at = None
        for idx, (a, t) in enumerate(block):
            m = ECX.match(t)
            if m:
                size = int(m.group(1), 16)
            if t.startswith("call "):
                tgt = t.split()[-1]
                if tgt == ALLOC:
                    alloc_at = a
                    ctor = None
                    continue
                if ctor is None and alloc_at is not None:
                    ctor = tgt
            sm = STORE.match(t)
            if sm:
                disp = int(sm.group(3), 16) if sm.group(3) else 0
                stores.append((a, sm.group(2), disp, sm.group(4), sm.group(1)))
            lm = LEA.match(t)
            if lm:
                vtables.append((a, lm.group(1), resolve(a, int(lm.group(2), 16))))
            mm = MOVI.match(t)
            if mm:
                stores.append((a, mm.group(1), -1, f"global:{resolve(a, int(mm.group(2), 16)):#x}", "qword"))
        slots = [s for s in stores if 0x220 <= s[2] <= 0x4ff and s[1] in ("rbx", "rax", "rcx", "rdx", "rdi", "rsi")]
        out.append({
            "rwi": r, "alloc": alloc_at, "size": size, "ctor": ctor,
            "vtables": vtables, "stores": stores, "slots": slots,
            "n_ins": len(block),
        })
    with open("dev/eng_blocks.json", "w", encoding="utf-8") as f:
        json.dump(out, f, indent=1)
    for i, b in enumerate(out):
        slots = ", ".join(f"[{s[1]}+{s[2]:#x}]@{s[0]:x}<-{s[3]}" for s in b["slots"]) or "-"
        vt = ", ".join(f"{v[2]:#x}->[{v[1]}]@{v[0]:x}" for v in b["vtables"]) or "-"
        print(f"{i:02d} alloc={b['alloc'] and hex(b['alloc'])} size={b['size'] and hex(b['size'])} ctor={b['ctor']} rwi={b['rwi']:#x}")
        print(f"     slots: {slots}")
        print(f"     vtabs: {vt}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

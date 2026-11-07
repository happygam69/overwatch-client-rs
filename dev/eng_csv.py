"""Produce the engine-slot table from the linear-sweep disassembly.

Blocks are delimited by `call 0x141c52d10` (RegisterWorldInterface), which
terminates every subsystem construction.  Within a block:
  producer = last `call Alloc` before the slot store + the first non-Alloc
             call after it (the constructor);
  vtable   = `lea REG,[rip+X]` followed by `mov [OBJREG(+0x10)], REG` where
             OBJREG is the register that is later stored into the engine slot.
"""
from __future__ import annotations
import re

DUMP = "dev/engine_dump.asm"
ALLOC = "0x141baff30"
RWI = "0x141c52d10"
ENGINE_GLOBAL = 0x143B7F410
STORE = re.compile(r"^mov\s+(qword|dword) ptr \[(\w+)(?: \+ (0x[0-9a-f]+))?\], (\w+)$")
LEA = re.compile(r"^lea\s+(\w+), \[rip \+ (0x[0-9a-f]+)\]$")
MOVI = re.compile(r"^mov\s+(\w+), qword ptr \[rip \+ (0x[0-9a-f]+)\]$")
ECX = re.compile(r"^mov\s+ecx, (0x[0-9a-f]+)$")
SLOT_LO, SLOT_HI = 0x220, 0x448


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
    n = len(rows)
    lens = {a: (rows[i + 1][0] if i + 1 < n else a + 16) - a for i, (a, _) in enumerate(rows)}

    def tgt(a, disp):
        return a + lens[a] + disp

    eng_load = {}
    for i, (a, t) in enumerate(rows):
        m = MOVI.match(t)
        if m and tgt(a, int(m.group(2), 16)) == ENGINE_GLOBAL:
            eng_load.setdefault(i, set()).add(m.group(1))

    def engine_base(i, reg):
        if reg == "rbx":
            return True
        for j in range(i - 1, max(-1, i - 5), -1):
            if reg in eng_load.get(j, ()):
                return True
        return False

    def ctarget(t):
        p = t.split()
        return p[-1] if len(p) == 2 and p[0] == "call" else None

    rwi_idx = [i for i, (a, t) in enumerate(rows) if ctarget(t) == RWI]
    blocks = []
    prev = -1
    for r in rwi_idx:
        blocks.append((prev + 1, r))
        prev = r

    recs = []
    for lo, hi in blocks:
        for i in range(lo, hi + 1):
            a, t = rows[i]
            m = STORE.match(t)
            if not m:
                continue
            disp = int(m.group(3), 16) if m.group(3) else 0
            if not (SLOT_LO <= disp <= SLOT_HI):
                continue
            if not engine_base(i, m.group(2)):
                continue
            objreg = m.group(4)
            # producer: last Alloc strictly before the store, inside the block
            alloc = None
            for j in range(i - 1, lo - 1, -1):
                if ctarget(rows[j][1]) == ALLOC:
                    alloc = j
                    break
            size, ctor = "", ""
            if alloc is not None:
                for k in range(alloc - 1, max(-1, alloc - 4), -1):
                    mm = ECX.match(rows[k][1])
                    if mm:
                        size = mm.group(1)
                        break
                for k in range(alloc + 1, i):
                    tt = rows[k][1]
                    if tt.startswith("call ") and tt.split()[-1] != ALLOC:
                        ctor = tt.split()[-1]
                        break
            # vtable stores into objreg
            vts = []
            for k in range(lo, i):
                mm = LEA.match(rows[k][1])
                if not mm or mm.group(1) != objreg:
                    continue
                vaddr = tgt(rows[k][0], int(mm.group(2), 16))
                if k + 1 <= i:
                    sm = STORE.match(rows[k + 1][1])
                    if sm and sm.group(4) == objreg and sm.group(2) == objreg:
                        d = int(sm.group(3), 16) if sm.group(3) else 0
                        vts.append((vaddr, d))
            recs.append({
                "slot": disp, "store": a, "objreg": objreg, "size": size,
                "ctor": ctor, "vts": vts, "rwi": rows[hi][0], "lo": rows[lo][0],
                "hi": rows[hi][0],
            })

    recs.sort(key=lambda r: r["slot"])
    for r in recs:
        print(f"slot={r['slot']:#05x} store={r['store']:#x} obj={r['objreg']} "
              f"size={r['size']} ctor={r['ctor']} rwi={r['rwi']:#x} "
              f"vt={[hex(v) + f'@{d:#x}' for v, d in r['vts']]}")
    print(f"# slot stores: {len(recs)}")
    import json
    json.dump(recs, open("dev/eng_recs.json", "w"), indent=1)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

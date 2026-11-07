"""Per-block reconstruction of ConstructEngineSubsystems (0x140554220)."""
import json, re

ALLOC = "0x141baff30"
RWI = "0x141c52d10"
BASE_CTOR = "0x141c578c0"
ENGINE_GLOBAL = 0x143B7F410

rows = []
for line in open("dev/engine_dump.asm", encoding="utf-8"):
    line = line.rstrip("\n")
    if not line.strip():
        continue
    a, rest = line.split("  ", 1)
    rows.append((int(a, 16), rest.strip()))
n = len(rows)
lens = {a: (rows[i + 1][0] if i + 1 < n else a + 16) - a for i, (a, _) in enumerate(rows)}

def tgt(a, disp):
    return a + lens[a] + disp

STORE = re.compile(r"^mov\s+(qword|dword) ptr \[(\w+)(?: \+ (0x[0-9a-f]+))?\], (\w+)$")
LEA = re.compile(r"^lea\s+(\w+), \[rip \+ (0x[0-9a-f]+)\]$")
MOVR = re.compile(r"^mov\s+(\w+), (\w+)$")
MOVIMM = re.compile(r"^mov\s+(\w+), (0x[0-9a-f]+)$")
MOVG = re.compile(r"^mov\s+(\w+), qword ptr \[rip \+ (0x[0-9a-f]+)\]$")

rwi = [i for i, (a, t) in enumerate(rows) if t.split() == ["call", RWI]]
blocks, prev = [], -1
for r in rwi:
    blocks.append((prev + 1, r))
    prev = r

out = []
for bi, (lo, hi) in enumerate(blocks):
    eng_regs = {"rbx"}
    for i in range(lo, hi + 1):
        m = MOVG.match(rows[i][1])
        if m and tgt(rows[i][0], int(m.group(2), 16)) == ENGINE_GLOBAL:
            eng_regs.add(m.group(1))
    for i in range(lo, hi + 1):
        a, t = rows[i]
        m = STORE.match(t)
        if not m:
            continue
        base = m.group(2)
        disp = int(m.group(3), 16) if m.group(3) else 0
        if base not in eng_regs or not (0x220 <= disp <= 0x448):
            continue
        obj = m.group(4)
        rec = {"block": bi, "slot": disp, "store": a, "objreg": obj, "engreg": base,
               "size": "", "ctor": "", "vt": "", "vt_insn": "", "src_kind": "",
               "factory_call": "", "call_insn": rows[hi][0]}
        # allocation + constructor
        alloc_i = None
        for j in range(lo, i):
            if rows[j][1].split() == ["call", ALLOC]:
                alloc_i = j
        if alloc_i is not None:
            for k in range(alloc_i - 1, max(-1, alloc_i - 4), -1):
                mm = MOVIMM.match(rows[k][1])
                if mm and mm.group(1) in ("ecx", "rcx"):
                    rec["size"] = mm.group(2)
                    break
            for k in range(alloc_i + 1, i):
                tt = rows[k][1]
                if tt.startswith("call ") and tt.split()[-1] != ALLOC:
                    rec["ctor"] = tt.split()[-1]
                    break
        # where does the object register come from?
        for k in range(i - 1, lo - 1, -1):
            mm = MOVR.match(rows[k][1])
            if mm and mm.group(1) == obj:
                src = mm.group(2)
                if src in ("rax", "rdi", "rsi"):
                    rec["src_kind"] = f"mov {obj},{src}"
                else:
                    rec["src_kind"] = f"mov {obj},{src} (?)"
                break
        if obj == "rax" and rec["ctor"]:
            rec["src_kind"] = "ctor result in rax"
        # factory: a call whose return lands in obj without a preceding Alloc
        if not rec["ctor"]:
            calls = [(rows[k][0], rows[k][1].split()[-1]) for k in range(lo, i) if rows[k][1].startswith("call ")]
            rec["factory_call"] = ";".join(f"{c:#x}->{f}" for c, f in calls)
        # vtable: lea REG,[rip+X] then mov [obj],REG
        rec["vt_all"] = []
        for k in range(lo, i):
            mm = LEA.match(rows[k][1])
            if not mm:
                continue
            vreg = mm.group(1)
            for j in range(k + 1, min(i, k + 4) + 1):
                sm = STORE.match(rows[j][1])
                if sm and sm.group(2) == obj and sm.group(4) == vreg:
                    d = int(sm.group(3), 16) if sm.group(3) else 0
                    rec["vt_all"].append([d, hex(tgt(rows[k][0], int(mm.group(2), 16))),
                                          hex(rows[j][0])])
                    if d == 0:
                        rec["vt"] = hex(tgt(rows[k][0], int(mm.group(2), 16)))
                        rec["vt_insn"] = hex(rows[j][0])
                    break
        out.append(rec)

out.sort(key=lambda r: r["slot"])
json.dump(out, open("dev/eng_tab.json", "w"), indent=1)
print(f"{'slot':>6} {'store':>12} {'blk':>3} {'obj':>4} {'size':>6} {'ctor':>12} {'vtable':>12} {'src':>22}")
for r in out:
    print(f"{r['slot']:#06x} {r['store']:#012x} {r['block']:>3} {r['objreg']:>4} "
          f"{r['size']:>6} {r['ctor']:>12} {r['vt']:>12} {r['src_kind']:>22}")
print("rows", len(out))
print("--- blocks with no slot store ---")
have = {r["block"] for r in out}
for bi, (lo, hi) in enumerate(blocks):
    if bi not in have:
        calls = [rows[k][1].split()[-1] for k in range(lo, hi + 1) if rows[k][1].startswith("call ")]
        print(f"b{bi} {rows[lo][0]:#x}..{rows[hi][0]:#x} calls={calls}")

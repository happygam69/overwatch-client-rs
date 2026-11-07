"""Emit dev/engine_subsystems.csv from dev/eng_tab.json plus tail/attach facts."""
import json

ENGINE_GLOBAL = 0x143B7F410
BASE_CTOR = "0x141c578c0"

rows = []
for line in open("dev/engine_dump.asm", encoding="utf-8"):
    line = line.rstrip("\n")
    if not line.strip():
        continue
    a, rest = line.split("  ", 1)
    rows.append((int(a, 16), rest.strip()))

# tail attach pass: mov rcx/rdx,[rax+SLOT] ... call 0x140bc7590
attach = {}
cur = None
for a, t in rows:
    if a < 0x1405556E3:
        continue
    if t.startswith("mov      rcx, qword ptr [rax + ") or t.startswith("mov      rdx, qword ptr [rax + "):
        cur = int(t.split("[rax + ")[1].rstrip("]"), 16)
    elif t.split() == ["call", "0x140bc7590"] and cur is not None:
        attach[cur] = a
        cur = None

recs = json.load(open("dev/eng_tab.json"))
SPECIAL = {
    0x220: ("0x1405437f0", "", "",
            "no Alloc for this slot: ([engine+0x330]+0x60) is passed to 0x1405437f0 at "
            "0x140555325 (rcx=&temp), the result is then decoded by the xor/add/ror chain "
            "0x14055532a..0x14055536a before the store; the block's own Alloc 0x980 "
            "(ctor 0x140750b70, 0x140555379/0x140555386) is registered at 0x140555396 but "
            "never stored, so there is no 0x141c52d10 call for this slot's value"),
    0x340: ("0x140594090", "", "0x140554ae3",
            "factory call 0x140594090 returns the object (no inline Alloc); registered at "
            "0x140554ae3 with rdx=[engine+0x340]"),
    0x358: ("0x1406cc990", "", "0x140554af2",
            "factory call 0x1406cc990 returns the object (no inline Alloc); its store and "
            "its registration straddle a block boundary - the 0x141c52d10 call at "
            "0x140554af2 is the first instruction of the next block"),
    0x448: ("0x140723890", "", "0x140554b8f",
            "adjusted subobject pointer, not a fresh allocation: rdi = object from "
            "0x140554b6f (Alloc 0x68, ctor 0x140723890, registered at 0x140554b8f); "
            "rax = rdi + 0x10 + (int32)[[rdi+0x10]+4], computed at 0x140554b99..0x140554ba8"),
}

out = []
for r in recs:
    slot = r["slot"]
    ctor, size, note, vt = r["ctor"], r["size"], "", r["vt"]
    call = hex(r["call_insn"])
    if slot in SPECIAL:
        ctor, size, call, note = SPECIAL[slot]
    if ctor == BASE_CTOR:
        note = f"base ctor 0x141c578c0 then vtable stored inline at {r['vt_insn']}"
    extras = [(d, v) for d, v, _ in r.get("vt_all", []) if d != 0]
    if extras:
        note = (note + "; " if note else "") + "extra vtable slot(s) set inline: " + \
            ", ".join(f"+{d:#x}={v}" for d, v in extras)
    if r["engreg"] != "rbx":
        note = (note + "; " if note else "") + \
            f"engine pointer reloaded into {r['engreg']} from global {ENGINE_GLOBAL:#x}, " \
            f"so the store base is {r['engreg']} and not rbx"
    if slot in attach:
        note = (note + "; " if note else "") + \
            f"re-attached: obj+0x10 passed to 0x140bc7590 at {attach[slot]:#x}"
    if slot == 0x268:
        note = (note + "; " if note else "") + \
            "this tail attach is guarded by `cmp qword ptr [rsi+0xe0], 0` at 0x14055597c"
    if slot == 0x330:
        note = (note + "; " if note else "") + \
            "object is read back at 0x140555309 and its +0x60 subobject feeds slot 0x220"
    if slot == 0x308:
        note = (note + "; " if note else "") + \
            "dedicated ctor 0x140bdf740 is followed by an inline vtable override at " \
            "0x1405546b7, so the class's own vtable is replaced here"
    if slot == 0x3F8:
        note = (note + "; " if note else "") + \
            "same class as 0x420: identical secondary vtable +0x10=0x142f7e168"
    if slot == 0x420:
        note = (note + "; " if note else "") + \
            "same class as 0x3f8: identical secondary vtable +0x10=0x142f7e168"
    out.append({
        "slot_offset": f"{slot:#05x}",
        "ctor_or_source": ctor,
        "alloc_size": size,
        "vtable": vt,
        "store_insn": hex(r["store"]),
        "call_insn": call,
        "note": note,
    })

out.sort(key=lambda r: int(r["slot_offset"], 16))
hdr = ["slot_offset", "ctor_or_source", "alloc_size", "vtable", "store_insn", "call_insn", "note"]
with open("dev/engine_subsystems.csv", "w", encoding="utf-8", newline="") as f:
    f.write(",".join(hdr) + "\n")
    for r in out:
        vals = []
        for k in hdr:
            v = r[k]
            if "," in v or '"' in v:
                v = '"' + v.replace('"', '""') + '"'
            vals.append(v)
        f.write(",".join(vals) + "\n")
print("rows:", len(out), "attach slots:", len(attach))

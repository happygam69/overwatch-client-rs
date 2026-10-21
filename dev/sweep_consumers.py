"""Rank *executable* functions by how many mover/volley/health runtime slots
they touch.

`sweep_offset_uses.py` scans every captured region, so image data disassembled
as code outnumbers real code and buries the answer; and it keys targets by
offset number alone, so one offset shared by several classes is labelled once.
This variant is the behaviour hunt's version:

  * only regions with protect == 0x20 (PAGE_EXECUTE_READ) are disassembled;
  * each class's slots are scored separately;
  * mover slots are split into the movement subset and the rest, so a
    serializer (which walks every slot in offset order) can be told from a
    mover consumer (which reads a handful of movement slots).

Runtime slots are the class-file/descriptor offsets + 4, per
docs/formats/character-controller.md.

Usage:
    python sweep_consumers.py             # writes dev/consumers_mover.txt
    OW_DUMPS=dumps_v2 python sweep_consumers.py
"""

from __future__ import annotations

import capstone

import regions as R
from disasm_field_sites import md

# Runtime slot -> label. Labels are the established role where one exists
# (character-controller.md), else the field hash.
MOVER: dict[int, str] = {
    0x0C: "1302b05c", 0x4C: "jump.struct", 0x6C: "761cc74e", 0x7C: "1972c687",
    0x80: "7a70e3e3", 0x84: "4a576bc2", 0x88: "03e04168", 0x8C: "915ff368",
    0x90: "d97b3735", 0x94: "d4ae9d05", 0x98: "5c6b4602", 0x9C: "6b7fdb90",
    0xA0: "e54438f7", 0xA4: "aebad1b0", 0xA8: "08ffaa0a", 0xAC: "side",
    0xB0: "back", 0xB4: "forward", 0xB8: "b470b8da", 0xBC: "aa6f4ce1",
    0xC0: "crouch", 0xC4: "daa0618b", 0xC8: "e631ba84", 0xCC: "d24bf4e6",
    0xD0: "e0a0fe25", 0xD4: "6f25aa23", 0xD8: "4e03e892", 0xDC: "142da8d4",
    0xE0: "speed", 0xE4: "8895a9c8", 0xE8: "ec289987", 0xEC: "c2c40969",
    0xF0: "fall_terminal", 0xF4: "975f21a5", 0xF8: "b99ec983", 0xFC: "cef4898c",
    0x100: "e91d1664", 0x104: "f227cbb6", 0x108: "808374ca", 0x10C: "06b1ef76",
    0x110: "3d76c05d", 0x114: "b31ad888", 0x118: "4a5244c1", 0x11C: "5d90dbdb",
    0x120: "d9ecb731", 0x124: "5632f333", 0x128: "d51c467d", 0x12C: "6db41d17",
    0x12E: "d4a3c694", 0x12F: "fd92c072", 0x130: "1ff06061", 0x131: "5aa2e07c",
}
# The jump struct's six inline floats (docs/formats/character-controller.md).
MOVER.update({0x50: "jump.16882d22", 0x54: "jump.746b3248", 0x58: "jump.0.98",
              0x5C: "jump.d68b7d62", 0x60: "jump.9fcdbe78", 0x64: "jump.766afa5a"})

MOVEMENT = {0x4C, 0x50, 0x54, 0x58, 0x5C, 0x60, 0x64, 0x7C,
            0xAC, 0xB0, 0xB4, 0xC0, 0xE0, 0xF0}

VOLLEY: dict[int, str] = {
    0xD0: "projectileMotions", 0xF8: "projectileEntity", 0x100: "projectileVisual",
    0x108: "aimID", 0x110: "projectileLifetime", 0x118: "castLength",
    0x128: "projectileSpeed", 0x1C0: "numProjectilesPerShot",
    0x1C8: "numShotsPerSecond",
}

HEALTH: dict[int, str] = {
    0x08: "5af3c1f6=7.5", 0x0C: "e64867ce=-1", 0x10: "e0ee1282=0",
    0x14: "cdb100d1=1", 0x18: "a80bab3f=1",
}

SETS = {"mover": MOVER, "volley": VOLLEY, "health": HEALTH}

# Float ops touch a field as a scalar the code computes with; a copier or
# serializer walking the whole struct uses mov/movups. The ranking that
# matters here counts float-op hits on the movement slots.
FLOAT_OPS = {
    "movss", "movaps", "movups", "movsd", "addss", "subss", "mulss", "divss",
    "comiss", "ucomiss", "minss", "maxss", "sqrtss", "cvtss2sd", "cvtsd2ss",
    "addps", "subps", "mulps", "divps", "andps", "orps", "xorps", "movd",
    "movq", "unpcklps", "shufps", "cmpltss", "cmpless", "cmpnless",
}


def is_new_function(prev: capstone.CsInsn | None, ins: capstone.CsInsn) -> bool:
    """A crude function boundary: a prologue right after a terminator."""
    if prev is None:
        return True
    if prev.mnemonic in ("ret", "jmp", "int3"):
        return ins.mnemonic in ("push", "sub", "mov", "endbr64", "lea")
    return False


def main() -> int:
    space = R.Space(R.load(R.capture_dir("dumps_range")))
    code = [r for r in space.regions if r.protect == 0x20]
    print(f"{len(code)} executable regions of {len(space.regions)}")

    # func -> class -> {slot: (addr, text)}
    hits: dict[int, dict[str, dict[int, tuple[int, str]]]] = {}
    for reg in code:
        data = reg.data()
        func = reg.base
        prev: capstone.CsInsn | None = None
        for ins in md.disasm(data, reg.base):
            if is_new_function(prev, ins):
                func = ins.address
            for op in ins.operands:
                if op.type != capstone.x86.X86_OP_MEM:
                    continue
                m = op.mem
                if m.base in (0, capstone.x86.X86_REG_RIP):
                    continue
                for cls, slots in SETS.items():
                    if m.disp in slots:
                        hits.setdefault(func, {}).setdefault(cls, {})[m.disp] = (
                            ins.address, f"{ins.mnemonic} {ins.op_str}")
            prev = ins
        del data

    rows = []
    for func, per in hits.items():
        mv = per.get("mover", {})
        mv_distinct = {d for d in mv if d in MOVEMENT}
        mv_float = {d for d, (_, text) in mv.items()
                    if d in MOVEMENT and text.split(" ", 1)[0].lower() in FLOAT_OPS}
        rows.append((func, len(mv_float), len(mv_distinct), len(mv), per))
    rows.sort(key=lambda r: (-r[1], -r[2], -r[3]))

    out = R.out_name("consumers_mover.txt")
    with open(out, "w", encoding="utf-8") as fh:
        fh.write("# float_moveslots  move_slots  mover_slots  function  (exec = +0x1096A0000)  hits\n")
        for func, nfloat, nmove, nmv, per in rows:
            fh.write(f"{nfloat:2d} {nmove:2d} {nmv:4d} {func:#x}\n")
            for cls, slots in sorted(per.items()):
                for d, (a, text) in sorted(slots.items()):
                    fh.write(f"      {cls:6} {d:#05x} {MOVER.get(d) or SETS[cls].get(d) or '?':18} {a:#x}  {text}\n")
    print(f"{len(rows)} functions touch a mover slot -> {out}\n")

    MOVER_HITS = 0x1096A0000
    for func, nfloat, nmove, nmv, per in rows[:30]:
        mv = per.get("mover", {})
        fnames = sorted(MOVER.get(d, "?") for d, (_, t) in mv.items()
                        if d in MOVEMENT and t.split(" ", 1)[0].lower() in FLOAT_OPS)
        print(f"  float={nfloat:2d} move={nmove:2d} slots={nmv:2d} exec={func + MOVER_HITS:#x}  {','.join(fnames)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

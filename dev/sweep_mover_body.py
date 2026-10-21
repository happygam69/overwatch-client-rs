"""Find functions that touch BOTH mover movement slots and dmBody/dmWorld
fields -- the mover->body bridge the byte-exact rewrite needs.

The default providers write the whole mover table; copiers move it wholesale.
The consumer we want reads a handful of movement slots (speed, side/back/forward,
crouch, jump struct, fall terminal, 1972c687) and drives a physics body:
linearVelocity (+0x30), gravityScale (+0x16c), linearDamping (+0x170),
characterMover (+0x1b6), and the world's m_inv_dt0 (+0x198) / m_gravity (+0x1a0).

Usage:
    python sweep_mover_body.py            # dumps_exec
"""

from __future__ import annotations

import capstone

import regions as R
from disasm_field_sites import md

MOVEMENT = {
    0x4C: "jump.struct", 0x50: "jump.16882d22", 0x54: "jump.746b3248",
    0x58: "jump.0.98", 0x5C: "jump.d68b7d62", 0x60: "jump.9fcdbe78",
    0x64: "jump.766afa5a", 0x7C: "1972c687", 0xAC: "side", 0xB0: "back",
    0xB4: "forward", 0xC0: "crouch", 0xE0: "speed", 0xF0: "fall_terminal",
}
BODY = {
    0x30: "linVel.x", 0x34: "linVel.y", 0x38: "linVel.z",
    0x40: "angVel.x", 0x130: "force.x",
    0x164: "mass", 0x168: "invMass", 0x16C: "gravityScale",
    0x170: "linearDamping", 0x174: "angularDamping",
    0x1AE: "isAwake", 0x1B1: "useContinuous", 0x1B6: "characterMover",
}
WORLD = {
    0x198: "m_inv_dt0", 0x1A0: "m_gravity.x", 0x1A4: "m_gravity.y",
    0x1A8: "m_gravity.z",
}


def is_new_function(prev, ins):
    if prev is None:
        return True
    if prev.mnemonic in ("ret", "jmp", "int3"):
        return ins.mnemonic in ("push", "sub", "mov", "endbr64", "lea")
    if prev.mnemonic == "int3":
        return True
    return False


def main() -> int:
    space = R.Space(R.load(R.capture_dir("dumps_exec")))
    code = [r for r in space.regions if r.protect == 0x20]
    print(f"{len(code)} executable regions")

    hits: dict[int, dict[str, dict[int, tuple[int, str]]]] = {}
    for reg in code:
        data = reg.data()
        func = reg.base
        prev = None
        for ins in md.disasm(data, reg.base):
            if is_new_function(prev, ins):
                func = ins.address
            for op in ins.operands:
                if op.type != capstone.x86.X86_OP_MEM:
                    continue
                m = op.mem
                if m.base in (0, capstone.x86.X86_REG_RIP):
                    continue
                if m.disp in MOVEMENT:
                    hits.setdefault(func, {}).setdefault("mover", {})[m.disp] = (
                        ins.address, f"{ins.mnemonic} {ins.op_str}")
                elif m.disp in BODY:
                    hits.setdefault(func, {}).setdefault("body", {})[m.disp] = (
                        ins.address, f"{ins.mnemonic} {ins.op_str}")
                elif m.disp in WORLD:
                    hits.setdefault(func, {}).setdefault("world", {})[m.disp] = (
                        ins.address, f"{ins.mnemonic} {ins.op_str}")
            prev = ins
        del data

    rows = []
    for func, per in hits.items():
        nm = len(per.get("mover", {}))
        nb = len(per.get("body", {}))
        nw = len(per.get("world", {}))
        if nm >= 3 and (nb + nw) >= 2:
            rows.append((nm, nb + nw, func, per))
    rows.sort(key=lambda r: (-min(r[0], r[1]), -r[0] - r[1]))

    out = R.out_name("mover_body_funcs.txt")
    with open(out, "w", encoding="utf-8") as fh:
        fh.write("# mover_slots body+world_slots function  (exec)  hits\n")
        for nm, nbs, func, per in rows:
            fh.write(f"{nm:2d} {nbs:2d} {func:#x}\n")
            for cls in ("mover", "body", "world"):
                for d, (a, text) in sorted(per.get(cls, {}).items()):
                    label = MOVEMENT.get(d) or BODY.get(d) or WORLD.get(d)
                    fh.write(f"      {cls:5} {d:#05x} {label:18} {a:#x}  {text}\n")
    print(f"{len(rows)} mover+body functions -> {out}\n")
    for nm, nbs, func, per in rows[:40]:
        mv = ",".join(MOVEMENT[d] for d in sorted(per.get("mover", {})))
        bd = ",".join((BODY.get(d) or WORLD.get(d)) for d in sorted(per.get("body", {})) | set(per.get("world", {})))
        print(f"  mover={nm:2d} body={nbs:2d} {func:#x}\n      M: {mv}\n      B: {bd}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

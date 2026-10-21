"""Rank executable functions by co-occurrence of *real* dmBody/dmWorld offsets.

The Domino schema is now recovered (docs/formats/domino.md): dmBody is 0x1c0
with linearVelocity @0x30, gravityScale @0x16c, linearDamping @0x170,
characterMover @0x1b6; dmWorld has m_inv_dt0 @0x198 and m_gravity @0x1a0.

Unlike the component-slot sweeps, these are displacements into one known class,
so co-occurrence of several of them in one function is a strong signature:
the world stepper reads m_inv_dt0 + m_gravity; the body integration reads/writes
linearVelocity and the damping/gravity scales; the character mover path tests
the characterMover flag at +0x1b6.

Usage:
    python sweep_physics.py            # default: dumps_exec (executable regions)
    OW_DUMPS=dumps_v2b python sweep_physics.py
"""

from __future__ import annotations

import capstone

import regions as R
from disasm_field_sites import md

# dmBody / dmWorld field displacements (the struct's own offsets).
BODY = {
    0x30: "linVel.x", 0x34: "linVel.y", 0x38: "linVel.z",
    0x40: "angVel.x",
    0x130: "force.x",
    0x158: "fixtureList", 0x15C: "fixtureCount",
    0x164: "mass", 0x168: "invMass", 0x16C: "gravityScale",
    0x170: "linearDamping", 0x174: "angularDamping",
    0x178: "inertiaScale", 0x17C: "sleepTime",
    0x1AE: "isAwake", 0x1B1: "useContinuous", 0x1B4: "isFast",
    0x1B6: "characterMover",
}
WORLD = {
    0x198: "m_inv_dt0",
    0x1A0: "m_gravity.x", 0x1A4: "m_gravity.y", 0x1A8: "m_gravity.z",
    0x4C4: "m_useSleep", 0x4C5: "m_useWarmStarting", 0x4C6: "m_useContinuous",
}
ALL = dict(BODY)
ALL.update(WORLD)

# Offsets that are weak alone (0x30 is a common displacement); score strong
# ones separately.
STRONG = {0x16C, 0x170, 0x1B6, 0x198, 0x1A0, 0x1A4, 0x1A8, 0x4C4, 0x4C5, 0x4C6}


def is_new_function(prev: capstone.CsInsn | None, ins: capstone.CsInsn) -> bool:
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
    print(f"{len(code)} executable regions of {len(space.regions)}")

    hits: dict[int, dict[int, tuple[int, str]]] = {}
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
                if m.disp in ALL:
                    hits.setdefault(func, {})[m.disp] = (
                        ins.address, f"{ins.mnemonic} {ins.op_str}")
            prev = ins
        del data

    rows = []
    for func, hs in hits.items():
        strong = {d for d in hs if d in STRONG}
        rows.append((len(strong), len(hs), func, hs))
    rows.sort(key=lambda r: (-r[0], -r[1]))

    out = R.out_name("physics_funcs.txt")
    with open(out, "w", encoding="utf-8") as fh:
        fh.write("# strong  total  function  (exec; exec->v2b = -0x71820000)  hits\n")
        for ns, nt, func, hs in rows:
            fh.write(f"{ns:2d} {nt:4d} {func:#x}\n")
            for d, (a, text) in sorted(hs.items()):
                fh.write(f"      {d:#05x} {ALL[d]:18} {a:#x}  {text}\n")
    print(f"{len(rows)} functions touch a physics offset -> {out}\n")
    for ns, nt, func, hs in rows[:40]:
        names = ",".join(ALL[d] for d in sorted(hs))
        print(f"  strong={ns:2d} total={nt:3d} {func:#x}  {names}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

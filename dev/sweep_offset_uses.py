"""Rank functions by which known struct offsets they touch.

The runtime field tables give the exact offset of every serialized field on a
component's runtime object (the descriptor's field array, offset column + 4).
Gameplay code that consumes a component reads several of those offsets; code
that merely passes the pointer on reads none.  So: disassemble the captured
code, collect every instruction whose memory operand has a displacement in the
target set, group by containing function, and rank by how many *distinct*
offsets the function touches.

The knock against displacement matching is false positives -- a single 0xe0
means nothing in a 50 MB binary.  Co-occurrence of four or more distinct
offsets from one class's field table does, because those offsets are not a
natural cluster: they are spaced by the class's own serialized layout.

Usage:
    python sweep_offset_uses.py                 # sweep the default dumps
    OW_DUMPS=dumps_live python sweep_offset_uses.py
"""

from __future__ import annotations

import sys

import capstone

import regions as R
from disasm_field_sites import md

# Runtime offsets (field-table offset + 4) for the classes combat cares about.
# Labels are the hash's field name where one is known, else the hash.
TARGETS: dict[int, str] = {}


def add(cls: str, pairs: list[tuple[int, str]]) -> None:
    for off, name in pairs:
        TARGETS[off] = f"{cls}.{name}"


add("mover", [
    (0x0C, "inst1"), (0x4C, "jump"), (0x7C, "1972c687"), (0x94, "d4ae9d05=0.25"),
    (0xA0, "aebad1b0"), (0xAC, "side"), (0xB0, "back"), (0xB4, "forward"),
    (0xC0, "crouch"), (0xCC, "d24bf4e6=0.5"), (0xE0, "speed"),
    (0xE4, "8895a9c8"), (0xF0, "fall_terminal"), (0x100, "e91d1664=4.5"),
    (0x104, "f227cbb6=1.0"), (0x108, "808374ca=0.5"), (0x114, "b31ad888=0"),
    (0x118, "4a5244c1=15"), (0x11C, "5d90dbdb=8"), (0x120, "d9ecb731=15"),
    (0x124, "5632f333=0.5"), (0x128, "d51c467d=50"),
])
add("volley", [
    (0xD0, "projectileMotions"), (0xF8, "projectileEntity"), (0x100, "projectileVisual"),
    (0x108, "aimID"), (0x110, "projectileLifetime"), (0x118, "castLength"),
    (0x128, "projectileSpeed"), (0x1C0, "numProjectilesPerShot"),
    (0x1C8, "numShotsPerSecond"),
])
add("health", [
    (0x08, "5af3c1f6=7.5"), (0x0C, "e64867ce=-1"), (0x10, "e0ee1282=0"),
    (0x14, "cdb100d1=1"), (0x18, "a80bab3f=1"),
])


def is_new_function(prev: capstone.CsInsn | None, ins: capstone.CsInsn) -> bool:
    """A crude function boundary: a prologue right after a terminator."""
    if prev is None:
        return True
    if prev.mnemonic in ("ret", "jmp", "int3"):
        return ins.mnemonic in ("push", "sub", "mov", "endbr64", "lea")
    if prev.mnemonic == "int3":
        return True
    return False


def main() -> int:
    space = R.Space(R.load(R.capture_dir("dumps_range")))
    print(f"regions: {len(space.regions)}, target offsets: {len(TARGETS)}")

    # Every capture holds one 2 GB blob (mapped game data).  Reading a region
    # that big, on top of whatever else runs, is what took the machine down
    # once; code lives in far smaller regions, so cap and say what was skipped.
    MAX_REGION = 64 << 20
    skipped = 0
    done = 0

    hits: dict[int, list[tuple[int, int, str]]] = {}
    for reg in space.regions:
        if reg.size > MAX_REGION:
            skipped += 1
            continue
        data = reg.data()
        if len(data) < 0x100:
            continue
        done += 1
        if done % 200 == 0:
            print(f"  {done} regions, {len(hits)} candidate functions", flush=True)
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
                if m.disp in TARGETS:
                    hits.setdefault(func, []).append((m.disp, ins.address, f"{ins.mnemonic} {ins.op_str}"))
            prev = ins
        del data
    print(f"scanned {done} regions, skipped {skipped} over {MAX_REGION >> 20} MB")

    rows = []
    for func, hs in hits.items():
        offs = {d for d, _, _ in hs}
        rows.append((len(offs), len(hs), func, offs, hs))
    rows.sort(key=lambda r: (-r[0], -r[1]))

    out = R.out_name("mover_offset_funcs.txt")
    with open(out, "w", encoding="utf-8") as fh:
        fh.write("# distinct  hits  function  offsets\n")
        for n, h, func, offs, hs in rows:
            names = ",".join(sorted(TARGETS[o] for o in offs))
            fh.write(f"{n:2d} {h:4d} {func:#x}  {names}\n")
    print(f"{len(rows)} functions touch at least one target -> {out}\n")
    for n, h, func, offs, hs in rows[:25]:
        names = sorted(TARGETS[o] for o in offs)
        print(f"  {n:2d} distinct {h:4d} hits  {func:#014x}  {', '.join(names)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

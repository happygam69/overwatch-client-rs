"""Trace the type-3 triangle test's closest-point GJK over the diff corpus.

Runs the real `0x142caed30` (the type-3 mesh narrowphase) once per case in
`dev/mesh_support_diff.py`'s corpus with a code hook on the three GJK bodies,
and reports for every case:

* how many times the triangle test's slow path (`0x142cc9460`) ran;
* how many times the 3-simplex (`0x142cc8d80`) and 4-simplex (`0x142cc7740`)
  bodies ran, and the simplex they were handed;
* the GJK's `param_1` output words (point, normal, third vector, iteration slot)
  and its return flag, captured at the moment it returns.

`--dump` also writes the 3/4-simplex call inputs to `dev/data/gjk_simplex.json`
(gitignored) so the Rust sub-simplex units can be pinned against the client's
own bytes.

Read-only: nothing here writes the image, the install or the Ghidra project.
"""

from __future__ import annotations

import json
import os
import struct
import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "verify_harness"))

import mesh_fixture as mf  # noqa: E402
from emu_oracle import EmuOracle, V2B_BASE  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn import x86_const as x  # noqa: E402

import mesh_support_diff as msd  # noqa: E402

IMAGE = msd.IMAGE
MESH_BODY = msd.MESH_BODY
GJK = 0x142CC9460
SIMPLEX3 = 0x142CC8D80
SIMPLEX4 = 0x142CC7740
TRIANGLE_TEST = 0x142CCE430

SIM_STRIDE = 0x50
SIM_RECORDS = 4

# The triangle test's descriptor, as the slow path builds it at `0x142cce6e8`.
DESC = [
    ("point_array", 0x00, "u64"),
    ("point_count", 0x08, "i32"),
    ("radius_a", 0x10, "vec4"),
    ("other_array", 0x20, "u64"),
    ("other_count", 0x28, "i32"),
    ("radius_b", 0x30, "vec4"),
    ("translation_a", 0x40, "vec4"),
    ("rotation_a", 0x50, "vec4"),
    ("translation_b", 0x60, "vec4"),
    ("rotation_b", 0x70, "vec4"),
    ("sweep_direction", 0x80, "vec4"),
    ("motion", 0x90, "vec4"),
    ("flag0", 0xA0, "u8"),
    ("flag1", 0xA1, "u8"),
]


def _vec(mem: bytes, at: int) -> list[float]:
    return list(struct.unpack_from("<4f", mem, at))


def simplex_snapshot(mem: bytes, base: int) -> dict:
    vertices = []
    for i in range(SIM_RECORDS):
        at = base + i * SIM_STRIDE
        if at + SIM_STRIDE > len(mem):
            break
        vertices.append(
            {
                "dw": _vec(mem, at + 0x00),
                "p": _vec(mem, at + 0x10),
                "w": _vec(mem, at + 0x20),
                "bary": _vec(mem, at + 0x30),
                "idx_a": struct.unpack_from("<i", mem, at + 0x40)[0],
                "idx_b": struct.unpack_from("<i", mem, at + 0x44)[0],
            }
        )
    return {"count": struct.unpack_from("<i", mem, base + 0x140)[0], "vertices": vertices}


def main(argv) -> int:
    dump = "--dump" in argv
    oracle = EmuOracle(IMAGE)
    oracle.load_function(V2B_BASE + (MESH_BODY - 0x140000000), max_size=0x2000)

    state = {"calls": [], "active": None, "s3": [], "s4": []}
    u = oracle.uc

    def hook(uu, address, size, ud):
        if address == V2B_BASE + (GJK - 0x140000000):
            rcx = uu.reg_read(x.UC_X86_REG_RCX)
            rdx = uu.reg_read(x.UC_X86_REG_RDX)
            mem = bytes(uu.mem_read(rdx, 0xA4))
            desc = {}
            for name, off, kind in DESC:
                if kind == "vec4":
                    desc[name] = _vec(mem, off)
                elif kind == "u64":
                    desc[name] = struct.unpack_from("<Q", mem, off)[0]
                elif kind == "i32":
                    desc[name] = struct.unpack_from("<i", mem, off)[0]
                else:
                    desc[name] = mem[off]
            state["active"] = {"out": rcx, "desc": desc, "steps": 0}
            state["calls"].append(state["active"])
        elif address in (
            V2B_BASE + (SIMPLEX3 - 0x140000000),
            V2B_BASE + (SIMPLEX4 - 0x140000000),
        ):
            rdx = uu.reg_read(x.UC_X86_REG_RDX)
            rcx = uu.reg_read(x.UC_X86_REG_RCX)
            snap = simplex_snapshot(bytes(uu.mem_read(rdx, 0x144)), 0)
            rec = {"out": rcx - 0, "simplex": snap}
            if address == V2B_BASE + (SIMPLEX3 - 0x140000000):
                state["s3"].append(rec)
            else:
                state["s4"].append(rec)
            if state["active"] is not None:
                state["active"]["steps"] += 1

    u.hook_add(UC_HOOK_CODE, hook, begin=0, end=0xFFFFFFFFFFFFFFFF)

    rows = []
    for name, world, block, shape in msd.corpus():
        state["calls"].clear()
        state["s3"].clear()
        state["s4"].clear()
        out = msd.oracle_out(oracle, 1.0, world, block, shape)
        rows.append(
            {
                "case": name,
                "gjk_calls": len(state["calls"]),
                "simplex3": len(state["s3"]),
                "simplex4": len(state["s4"]),
                "out": out.hex(),
                "descriptors": [c["desc"] for c in state["calls"]],
            }
        )
        print(
            f"{name:44s} gjk={len(state['calls'])} s3={len(state['s3'])} "
            f"s4={len(state['s4'])}"
        )
    reached = [r for r in rows if r["gjk_calls"]]
    print(
        f"cases: {len(rows)}  cases reaching the GJK: {len(reached)}  "
        f"3-simplex calls: {sum(r['simplex3'] for r in rows)}  "
        f"4-simplex calls: {sum(r['simplex4'] for r in rows)}"
    )
    if dump:
        os.makedirs("dev/data", exist_ok=True)
        with open("dev/data/gjk_trace.json", "w", encoding="utf-8") as f:
            json.dump(rows, f, indent=1)
        print("wrote dev/data/gjk_trace.json")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))

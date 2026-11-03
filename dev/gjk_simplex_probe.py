"""Oracle probe for the closest-point sub-simplex routines.

Runs the real bytes of `0x142cc8d80` (3-simplex) and `0x142cc7740`
(4-simplex) on a caller-supplied simplex and prints what they leave behind:
the reduced vertex count, every vertex's barycentric, and the direction vector
they write through their first argument.

    python dev/gjk_simplex_probe.py            # the built-in shapes
    python dev/gjk_simplex_probe.py --json out.json

Simplex record (0x50 bytes per vertex, count at +0x140):

    +0x00 dw   `w - p`, the Minkowski-difference point
    +0x10 p    the point on the first set
    +0x20 w    the point on the second set
    +0x30 bary the barycentric the routine rewrites
    +0x40 i32 index into the first set, +0x44 i32 index into the second

Read-only: nothing here writes the image, the install or the Ghidra project.
"""

from __future__ import annotations

import json
import os
import struct
import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "verify_harness"))

from emu_oracle import ARENA_BASE, ArgSpec, EmuOracle, V2B_BASE  # noqa: E402

IMAGE = "dev/data/offline_recover/module_offline.bin"
SIMPLEX3 = 0x142CC8D80
SIMPLEX4 = 0x142CC7740
STRIDE = 0x50
OUT_OFF = 0x5000
SIM_OFF = 0x5100


def quad(v) -> bytes:
    lanes = list(v) + [0.0] * (4 - len(v))
    return b"".join(struct.pack("<f", float(x)) for x in lanes[:4])


def simplex(dws, count: int) -> bytes:
    rec = bytearray(0x150)
    for i, dw in enumerate(dws):
        at = i * STRIDE
        rec[at : at + 16] = quad(dw)
        rec[at + 0x10 : at + 0x20] = quad([dw[0] + 0.25, dw[1] - 0.5, dw[2] + 1.0])
        rec[at + 0x20 : at + 0x30] = quad(dw)
        rec[at + 0x30 : at + 0x40] = quad([1.0, 1.0, 1.0, 1.0])
        struct.pack_into("<ii", rec, at + 0x40, i, i + 1)
    struct.pack_into("<i", rec, 0x140, count)
    return bytes(rec)


def describe(sim: bytes) -> dict:
    out = {
        "count": struct.unpack_from("<i", sim, 0x140)[0],
        "dir": [0.0, 0.0, 0.0, 0.0],
        "vertices": [],
    }
    for i in range(4):
        at = i * STRIDE
        out["vertices"].append(
            {
                "dw": list(struct.unpack_from("<4f", sim, at)),
                "p": list(struct.unpack_from("<4f", sim, at + 0x10)),
                "w": list(struct.unpack_from("<4f", sim, at + 0x20)),
                "bary": list(struct.unpack_from("<4f", sim, at + 0x30)),
                "idx": list(struct.unpack_from("<2i", sim, at + 0x40)),
            }
        )
    return out


def run(oracle: EmuOracle, entry: int, dws, count: int) -> dict:
    args = [
        ArgSpec(kind="ptr", mem=bytes(0x40), arena_offset=OUT_OFF),
        ArgSpec(kind="ptr", mem=simplex(dws, count), arena_offset=SIM_OFF),
    ]
    result = oracle.call(V2B_BASE + (entry - 0x140000000), args, ret_kind="u64")
    if result.fault:
        raise RuntimeError(f"oracle fault: {result.fault}")
    out = bytes(oracle.uc.mem_read(ARENA_BASE + OUT_OFF, 0x40))
    sim = bytes(oracle.uc.mem_read(ARENA_BASE + SIM_OFF, 0x150))
    described = describe(sim)
    described["dir"] = list(struct.unpack_from("<4f", out, 0))
    described["ret"] = result.ret_bits & 0xFF
    return described


def main(argv) -> int:
    oracle = EmuOracle(IMAGE)
    oracle.load_function(V2B_BASE + (SIMPLEX3 - 0x140000000), max_size=0x800)
    oracle.load_function(V2B_BASE + (SIMPLEX4 - 0x140000000), max_size=0x1700)

    cases = [
        ("3-simplex, all three far on +x", SIMPLEX3, [(2.0, 0.0, 0.0), (2.0, 1.0, 0.0), (2.0, 0.0, 1.0)], 3),
        ("3-simplex, straddling the origin", SIMPLEX3, [(-1.0, -1.0, -1.0), (1.0, -1.0, -1.0), (-1.0, 1.0, -1.0)], 3),
        ("3-simplex, edge region", SIMPLEX3, [(2.0, 2.0, 0.0), (1.0, 1.0, 0.0), (5.0, 5.0, 3.0)], 3),
        ("3-simplex, vertex region", SIMPLEX3, [(2.0, 2.0, 2.0), (4.0, 3.0, 2.0), (4.0, 4.0, 3.0)], 3),
        ("4-simplex, all on +x", SIMPLEX4, [(2.0, 0.0, 0.0), (2.0, 1.0, 0.0), (2.0, 0.0, 1.0), (2.0, 1.0, 1.0)], 4),
        ("4-simplex, enclosing the origin", SIMPLEX4, [(-1.0, -1.0, -1.0), (1.0, -1.0, -1.0), (-1.0, 1.0, -1.0), (-1.0, -1.0, 1.0)], 4),
    ]
    rows = []
    for name, entry, dws, count in cases:
        described = run(oracle, entry, dws, count)
        rows.append({"case": name, "result": described})
        print(f"{name}")
        print(f"  ret={described['ret']} count={described['count']} dir={['%.6g' % v for v in described['dir']]}")
        for i, vtx in enumerate(described["vertices"][: described["count"]]):
            print(
                f"   v{i} dw={['%.6g' % x for x in vtx['dw'][:3]]} "
                f"bary={['%.6g' % x for x in vtx['bary']]} idx={vtx['idx']}"
            )
        weight = [0.0, 0.0, 0.0]
        for i, vtx in enumerate(described["vertices"][: described["count"]]):
            for lane in range(3):
                weight[lane] += vtx["bary"][lane] * vtx["dw"][lane]
        print(f"   sum bary*dw = {['%.6g' % x for x in weight]}")
    if "--json" in argv:
        path = argv[argv.index("--json") + 1]
        with open(path, "w", encoding="utf-8") as f:
            json.dump(rows, f, indent=1)
        print(f"wrote {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))

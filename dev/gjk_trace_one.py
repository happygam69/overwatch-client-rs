"""Trace the client's closest-point GJK one descriptor at a time.

Prints every loop-top state (captured with a code hook on `0x142cc97ce`, where
the convergence test runs): the iteration and simplex count, the closest point,
the running fraction, the radius, the direction the last reduction left, and
each cached vertex's `dw` and support indices. That is the state a future
transcription of `0x142cc7740` has to reproduce; `ow_sim`'s own side is
compared by `dev/gjk_closest_diff.py`.

    python dev/gjk_trace_one.py --case seeded:0
    python dev/gjk_trace_one.py --case live:11

Read-only: nothing here writes the image, the install or the Ghidra project.
"""

from __future__ import annotations

import os
import struct
import subprocess
import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "verify_harness"))

from emu_oracle import ARENA_BASE, ArgSpec, EmuOracle, V2B_BASE  # noqa: E402
from unicorn import UC_HOOK_CODE, x86_const as x  # noqa: E402

import gjk_closest_diff as gcd  # noqa: E402

LOOP_TOP = 0x142CC97CE
REJECT = 0x142CC9D51
OUTPUT = 0x142CC9D47


def xmm(uu, index: int):
    raw = uu.reg_read(getattr(x, f"UC_X86_REG_XMM{index}")) & ((1 << 128) - 1)
    return struct.unpack("<4f", raw.to_bytes(16, "little"))

def gpr(uu, name: str) -> int:
    return uu.reg_read(getattr(x, f"UC_X86_REG_{name.upper()}"))


def trace_client(oracle: EmuOracle, desc, first, second):
    blob = bytearray(desc)
    at_first = ARENA_BASE + gcd.ARRAY_OFF
    at_second = at_first + len(first)
    struct.pack_into("<Q", blob, 0x00, at_first)
    struct.pack_into("<Q", blob, 0x20, at_second)
    args = [
        ArgSpec(kind="ptr", mem=bytes(0x40), arena_offset=gcd.OUT_OFF),
        ArgSpec(kind="ptr", mem=bytes(blob), arena_offset=gcd.DESC_OFF),
        ArgSpec(kind="ptr", mem=first, arena_offset=gcd.ARRAY_OFF),
        ArgSpec(kind="ptr", mem=second, arena_offset=gcd.ARRAY_OFF + len(first)),
    ]
    seen = []

    def hook(uu, address, size, ud):
        if address == V2B_BASE + (LOOP_TOP - 0x140000000):
            base = gpr(uu, "rbp") - 0x50
            count = gpr(uu, "rdx")
            dws = []
            idx = []
            for slot in range(max(0, min(count, 4))):
                at = base + slot * 0x50
                raw = bytes(uu.mem_read(at, 0x50))
                dws.append(list(struct.unpack_from("<4f", raw, 0))[:3])
                idx.append(list(struct.unpack_from("<2i", raw, 0x40)))
            seen.append(
                (
                    "top",
                    gpr(uu, "r15"),
                    gpr(uu, "rdx"),
                    xmm(uu, 2),
                    xmm(uu, 15),
                    xmm(uu, 14),
                    xmm(uu, 3),
                    dws,
                    idx,
                )
            )
        elif address == V2B_BASE + (REJECT - 0x140000000):
            seen.append(("reject", gpr(uu, "r15")))
        elif address == V2B_BASE + (OUTPUT - 0x140000000):
            seen.append(("output", gpr(uu, "r15"), gpr(uu, "rdx")))
    handle = oracle.uc.hook_add(UC_HOOK_CODE, hook, begin=0, end=0xFFFFFFFFFFFFFFFF)
    result = oracle.call(V2B_BASE + (gcd.GJK - 0x140000000), args, ret_kind="u64")
    oracle.uc.hook_del(handle)
    print(f"client ret={result.ret_bits & 0xFF}")
    for row in seen:
        if row[0] == "top":
            _, r15, rdx, x2, x15, x14, x3, dws, idx = row
            print(
                f"  top  iter={r15} count={rdx} XMM2={['%.6g' % v for v in x2]} "
                f"XMM15={['%.6g' % v for v in x15]} XMM14={['%.6g' % v for v in x14]} "
                f"XMM3={['%.6g' % v for v in x3]} dw={dws} idx={idx}"
            )
        else:
            print(f"  {row}")


def trace_rust(desc, first, second):
    line = ["call", "gjk_closest", "4"]
    for field in (bytes(0x40), desc, first, second):
        line += ["ptr", field.hex(), "-", "-"]
    env = dict(os.environ)
    env["OW_GJK_TRACE"] = "1"
    proc = subprocess.run(
        [gcd.PROBE], input=(" ".join(line) + "\n").encode(), capture_output=True, env=env
    )
    print("ow-sim trace:")
    for row in proc.stderr.decode().strip().splitlines():
        print("  " + row)
    print("ow-sim out:", proc.stdout.decode().strip()[:80])


def main(argv) -> int:
    which = "seeded:0"
    if "--case" in argv:
        which = argv[argv.index("--case") + 1]
    kind, index = which.split(":")
    index = int(index)
    oracle = EmuOracle(gcd.IMAGE)
    oracle.load_function(V2B_BASE + (gcd.GJK - 0x140000000), max_size=0x1000)
    if kind == "live":
        live = list(gcd.live_descriptors(oracle))
        desc, first, second = live[index]
    else:
        desc, first, second = list(gcd.seeded_descriptors(11, index + 1))[index]
    print(f"descriptor {which}: count_a={struct.unpack_from('<i', desc, 0x8)[0]} "
          f"count_b={struct.unpack_from('<i', desc, 0x28)[0]}")
    for at, name in ((0x10, "rad_a"), (0x30, "rad_b"), (0x40, "t_a"), (0x50, "q_a"),
                     (0x60, "t_b"), (0x70, "q_b"), (0x80, "dir"), (0x90, "motion")):
        print(f"  {name:6s} {['%.6g' % v for v in gcd.unpack_quad(desc, at)]}")
    print(f"  flags a0={desc[0xA0]} a1={desc[0xA1]}")
    print(f"  A: {first.hex()}")
    print(f"  B: {second.hex()}")
    trace_client(oracle, desc, first, second)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))

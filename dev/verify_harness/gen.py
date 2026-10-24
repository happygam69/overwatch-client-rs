"""Deterministic input generation for one unit.

Same contract as the Sandustry harness's gen.js: a seeded PRNG, a declared
signature, and an adversarial edge pool that is always added on top of the
random draws. The same (seed, iteration) pair always produces the same input,
so both sides of the differential always see identical bytes.
"""

from __future__ import annotations

import random
import struct
from dataclasses import dataclass
from typing import Iterator, List, Sequence

from emu_oracle import ArgSpec

# Values every f32 edge pool contains: sign zeros, the wrap-relevant
# magnitudes seen in the decompiles (0.004 settle threshold, 600 decay,
# 3.0/0.05 fov clamps), denormals, and the IEEE specials.
F32_EDGE_POOL = [
    0.0,
    -0.0,
    1.0,
    -1.0,
    0.5,
    -0.5,
    2.0,
    -2.0,
    3.0,
    0.05,
    0.004,
    -0.004,
    600.0,
    1e-6,
    -1e-6,
    1e-30,
    3.4028235e38,  # f32 max
    -3.4028235e38,
    1.4e-45,  # smallest positive normal
    float("nan"),
    float("inf"),
    float("-inf"),
]

# Raw bit patterns worth probing: all-ones (negative NaN / -1), exp-only.
F32_EDGE_BITS = [0xFFFFFFFF, 0x7FFFFFFF, 0x7F800000, 0xFF800000, 0x00000001]


@dataclass
class UnitArg:
    name: str
    kind: str  # f32 | f64 | u64 | ptr | ptr:f32x4 | ptr:f32xN
    ptr_len: int = 0  # for ptr: pointee length in bytes


def _draw_f32(rng: random.Random) -> float:
    pick = rng.randrange(6)
    if pick == 0:
        return rng.choice(F32_EDGE_POOL)
    if pick == 1:
        return struct.unpack("<f", struct.pack("<I", rng.choice(F32_EDGE_BITS)))[0]
    if pick == 2:
        return rng.uniform(-10.0, 10.0)
    if pick == 3:
        return rng.uniform(-1e3, 1e3)
    if pick == 4:
        return float(rng.randint(-8, 8))
    return struct.unpack("<f", struct.pack("<I", rng.getrandbits(32)))[0]


def _draw_ptr_mem(rng: random.Random, length: int) -> bytes:
    """Pointee bytes. Half structured (flag-bit sweeps, monotone ramps),
    half raw noise - random coordinates over structured state is the single
    largest source of blind spots, per the Sandustry fixture rules."""
    mode = rng.randrange(4)
    n4 = length // 4
    if mode == 0:
        return bytes(rng.getrandbits(8) for _ in range(length))
    if mode == 1:
        # Sweep the dword flag field (last dword) through single bits.
        words = [rng.getrandbits(32) for _ in range(n4)]
        if n4 >= 1:
            words[-1] = rng.choice([0, 1, 2, 4, 8, 0x80000000, rng.getrandbits(32)])
        return b"".join(struct.pack("<I", w & 0xFFFF_FFFF) for w in words)
    if mode == 2:
        # Monotone ramp of small floats - the shape wrap/clamp code expects.
        base = rng.uniform(-4.0, 4.0)
        step = rng.uniform(-1.0, 1.0)
        words = [struct.unpack("<I", struct.pack("<f", base + step * i))[0] for i in range(n4)]
        return b"".join(struct.pack("<I", w) for w in words)
    # Structured edge pool per lane.
    out = b""
    for _ in range(n4):
        v = rng.choice(F32_EDGE_POOL)
        out += struct.pack("<f", v)
    return out


def generate_args(specs: Sequence[UnitArg], seed: int, i: int) -> List[ArgSpec]:
    """Produce the i-th input for a unit. Deterministic in (seed, i)."""
    rng = random.Random((seed << 20) ^ i)
    out: List[ArgSpec] = []
    for s in specs:
        if s.kind == "f32":
            out.append(ArgSpec(kind="f32", value=_draw_f32(rng)))
        elif s.kind == "f64":
            out.append(ArgSpec(kind="f64", value=_draw_f32(rng)))
        elif s.kind == "u64":
            out.append(ArgSpec(kind="u64", bits=rng.getrandbits(32)))
        elif s.kind.startswith("ptr"):
            out.append(ArgSpec(kind="ptr", mem=_draw_ptr_mem(rng, s.ptr_len)))
        else:
            raise ValueError(f"unknown kind {s.kind!r}")
    return out


def corpus(specs: Sequence[UnitArg], seed: int, n: int) -> Iterator[List[ArgSpec]]:
    for i in range(n):
        yield generate_args(specs, seed, i)


def focused_f32_edge_cases(specs: Sequence[UnitArg]) -> List[List[ArgSpec]]:
    """A small deterministic set that pins every f32 arg to each edge value
    in isolation (other args at 0.0). Always runs before the random corpus."""
    cases: List[List[ArgSpec]] = []
    for idx, s in enumerate(specs):
        if s.kind != "f32":
            continue
        for v in F32_EDGE_POOL:
            row: List[ArgSpec] = []
            for j, t in enumerate(specs):
                if t.kind == "f32":
                    row.append(ArgSpec(kind="f32", value=v if j == idx else 0.0))
                elif t.kind == "f64":
                    row.append(ArgSpec(kind="f64", value=0.0))
                elif t.kind == "u64":
                    row.append(ArgSpec(kind="u64", bits=0))
                else:
                    row.append(ArgSpec(kind="ptr", mem=b"\x00" * t.ptr_len))
            cases.append(row)
    # And one all-edge-pairs sweep for two-f32 signatures (the wrap shape).
    f32_idx = [j for j, t in enumerate(specs) if t.kind == "f32"]
    if len(f32_idx) == 2:
        for a in F32_EDGE_POOL[:8]:
            for b in F32_EDGE_POOL[:8]:
                row = []
                for j, t in enumerate(specs):
                    if t.kind == "f32":
                        val = a if j == f32_idx[0] else b
                        row.append(ArgSpec(kind="f32", value=val))
                    elif t.kind == "f64":
                        row.append(ArgSpec(kind="f64", value=0.0))
                    elif t.kind == "u64":
                        row.append(ArgSpec(kind="u64", bits=0))
                    else:
                        row.append(ArgSpec(kind="ptr", mem=b"\x00" * t.ptr_len))
                cases.append(row)
    return cases

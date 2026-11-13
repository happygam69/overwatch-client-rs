"""Inputs for the `fn/SegmentAabbTransform` unit (`0x142697ac0`).

The function transforms a segment's two 4-wide endpoints into the two corners
of their AABB in a matrix's frame. It branches on nothing, so the declared
cases aim at the *arithmetic*: the identity, a mirrored basis (the extent takes
absolute values, so the corners must not swap), a rotation, a degenerate
segment (half-extent zero), a translation-only matrix, values where `a + b` and
`b + a` differ (`FNV`-style odd mantissas so a reordered add shows), and the
special patterns (-0.0, inf, NaN) whose propagation is part of the contract.

The random tail uses the same generator for every case, so a mutation that
changes the order of one `addps` lands on a value that distinguishes it.
"""

from __future__ import annotations

import struct
from math import nan
from typing import List

from emu_oracle import ArgSpec

ARENA_0 = 0x1000
ARENA_1 = 0x2000
ARENA_2 = 0x3000

IDENTITY = [
    1.0, 0.0, 0.0, 0.0,
    0.0, 1.0, 0.0, 0.0,
    0.0, 0.0, 1.0, 0.0,
    0.0, 0.0, 0.0, 1.0,
]
MIRRORED = [
    -1.0, 0.0, 0.0, 0.0,
    0.0, 1.0, 0.0, 0.0,
    0.0, 0.0, 1.0, 0.0,
    0.0, 0.0, 0.0, 1.0,
]
TRANSLATED = [
    1.0, 0.0, 0.0, 0.0,
    0.0, 1.0, 0.0, 0.0,
    0.0, 0.0, 1.0, 0.0,
    10.0, -20.0, 0.5, 1.0,
]
# A column basis with mixed signs and unequal lengths, so each term of the
# extent and the centre contributes a distinguishable value.
ROTATED = [
    0.0, 1.0, 0.0, 0.0,
    -2.0, 0.0, 0.0, 0.0,
    0.0, 0.0, 0.5, 0.0,
    1.5, 2.5, -3.5, 1.0,
]
# Odd mantissas: `1.0 + 1.0e-7` and `1.0e-7 + 1.0` agree, but
# `(a + b) + c` and `a + (b + c)` do not, so an addps reorder shows.
ODD = [
    1.0000001, 1.0, 0.0, 0.0,
    0.0, 1.0000001, 0.0, 0.0,
    0.0, 0.0, 1.0000001, 0.0,
    1.0e-7, 1.0e-7, 1.0e-7, 1.0,
]

SEGMENTS = [
    [0.0, 0.0, 0.0, 0.0, 2.0, 4.0, 6.0, 0.0],
    [1.0, 2.0, 3.0, 0.0, 1.0, 2.0, 3.0, 0.0],   # degenerate: e == 0
    [-1.0, 0.5, -2.0, 0.0, 3.5, -4.5, 0.25, 1.0],  # lane 3 is used, not masked
    [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    [-0.0, -0.0, -0.0, -0.0, 0.0, 0.0, 0.0, 0.0],  # -0.0 vs +0.0 is observable
    [1.0e-30, 1.0e30, -1.0e-30, 0.0, -1.0e30, 1.0e-30, 1.0e30, 0.0],
    [float("inf"), 0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 0.0],  # inf propagates, agreed
]
# A NaN *input* is excluded on purpose: where the body meets two NaNs, the
# emulator clears the sign of the one it returns (`0x7fc00001` whichever operand
# carries it) while the instruction keeps the destination's, so the two sides
# are not comparable. Measured 2026-11-20; see this unit's acknowledgedGaps.

CASES = [
    (IDENTITY, SEGMENTS[0]),
    (MIRRORED, SEGMENTS[0]),
    (TRANSLATED, SEGMENTS[1]),
    (ROTATED, SEGMENTS[2]),
    (IDENTITY, SEGMENTS[3]),
    (ROTATED, SEGMENTS[4]),
    (ODD, SEGMENTS[5]),
    (IDENTITY, SEGMENTS[6]),
    (TRANSLATED, SEGMENTS[4]),
    (ODD, SEGMENTS[6]),
]


def _f32(value: float) -> bytes:
    return struct.pack("<f", value)


def _rng(seed: int) -> List[float]:
    """xorshift32, deterministic in the seed: floats across the whole range,
    including the bit patterns that are not numbers."""
    out: List[float] = []
    x = seed | 1
    for _ in range(24):
        x ^= (x << 13) & 0xFFFF_FFFF
        x ^= x >> 17
        x ^= (x << 5) & 0xFFFF_FFFF
        # Finite only: a random exponent of 0xff would be inf/NaN, and a NaN
        # operand pair is not comparable across the two engines (see above).
        # The sign, the mantissa and the exponent (0 = denormal) stay random.
        exponent = (x >> 23) % 0xFF
        bits = (x & 0x807F_FFFF) | (exponent << 23)
        out.append(struct.unpack("<f", struct.pack("<I", bits))[0])
    return out


def make_args(i: int) -> List[ArgSpec]:
    if i < len(CASES):
        matrix, segment = CASES[i]
    else:
        values = _rng(i * 2654435761)
        matrix = values[:16]
        segment = values[16:24]
    out = b"\x00" * 32
    m = b"".join(_f32(v) for v in matrix)
    s = b"".join(_f32(v) for v in segment)
    return [
        ArgSpec(kind="ptr", mem=out, arena_offset=ARENA_0),
        ArgSpec(kind="ptr", mem=m, arena_offset=ARENA_1),
        ArgSpec(kind="ptr", mem=s, arena_offset=ARENA_2),
    ]

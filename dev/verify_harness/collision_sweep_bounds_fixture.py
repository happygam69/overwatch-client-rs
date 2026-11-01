"""Inputs for the `collision_sweep_bounds` unit (`0x142cd5540`).

One case is a query block plus its local support points. The block's `+0x30`
pointer is the arena address the harness will map the point array to, so the
oracle dereferences the real array; the point array is appended to the same
pointee so the probe sees the same bytes behind the baked pointer.
"""

from __future__ import annotations

import random
import struct
from typing import List

from emu_oracle import ARENA_BASE, ArgSpec

BLOCK_OFF = 0x4000
OUT_EXPANSION_OFF = 0x4100
OUT_START_OFF = 0x4200
OUT_END_OFF = 0x4300
BLOCK_LEN = 0x80

# Quaternions worth pinning: the identity, quarter turns, a non-unit scale, a
# zero quaternion (the leaf never normalises), and the IEEE specials.
QUAT_EDGE = [
    (0.0, 0.0, 0.0, 1.0),
    (0.0, 0.0, 0.70710678, 0.70710678),
    (0.70710678, 0.0, 0.0, 0.70710678),
    (0.0, 0.70710678, 0.0, 0.70710678),
    (0.5, 0.5, 0.5, 0.5),
    (2.0, -2.0, 0.5, 0.25),
    (0.0, 0.0, 0.0, 0.0),
    (-0.0, 0.0, -0.0, 1.0),
    (float("nan"), 0.0, 0.0, 1.0),
    (float("inf"), 0.0, 0.0, 1.0),
    (1.0, 1.0, 1.0, -1.0),
]

_SCALARS = [
    0.0,
    -0.0,
    1.0,
    -1.0,
    0.5,
    2.0,
    -3.5,
    0.001,
    -1e-7,
    1e6,
    -1e6,
    float("nan"),
    float("inf"),
    float("-inf"),
]


def _cases():
    rng = random.Random(142_5540)
    out = []
    # Deterministic structured sweep: point counts 0..8, every quaternion edge
    # reached, ordinary and special scalars mixed.
    for i in range(64):
        count = i % 9
        quat = QUAT_EDGE[i % len(QUAT_EDGE)]
        points = [
            (
                rng.uniform(-10.0, 10.0),
                rng.uniform(-10.0, 10.0),
                rng.uniform(-10.0, 10.0),
            )
            for _ in range(count)
        ]
        if i % 7 == 0:
            points = [
                (_SCALARS[(i + k) % len(_SCALARS)], p[1], p[2])
                for k, p in enumerate(points)
            ]
        p0 = (rng.uniform(-5.0, 5.0), rng.uniform(-5.0, 5.0), rng.uniform(-5.0, 5.0))
        p1 = (p0[0] + rng.uniform(-2.0, 2.0), p0[1], p0[2] + rng.uniform(-2.0, 2.0))
        off = (rng.uniform(0.0, 1.0), rng.uniform(0.0, 1.0), rng.uniform(0.0, 1.0))
        out.append((points, quat, p0, p1, off))
    # A degenerate block: count 0 but a non-zero extent, so the early exit is
    # exercised with outputs that would differ if the fold ran.
    out.append(([], (0.0, 0.0, 0.0, 1.0), (1.0, 2.0, 3.0), (-4.0, -5.0, -6.0), (7.0, 8.0, 9.0)))
    out.append(([], (0.0, 0.0, 0.0, 1.0), (0.0, 0.0, 0.0), (0.0, 0.0, 0.0), (0.0, 0.0, 0.0)))
    return out


CASES = _cases()


def _fbits(value: float) -> int:
    return struct.unpack("<I", struct.pack("<f", value))[0]


def _quad_bytes(values) -> bytes:
    return b"".join(struct.pack("<I", _fbits(v)) for v in values)


def _points_bytes(points) -> bytes:
    out = bytearray()
    for p in points:
        out += _quad_bytes([p[0], p[1], p[2], 0.0])
    return bytes(out)


def _block_bytes(case) -> bytes:
    points, quat, p0, p1, off = case
    block = bytearray(BLOCK_LEN)
    block[0x00:0x10] = _quad_bytes([p0[0], p0[1], p0[2], 0.0])
    block[0x10:0x20] = _quad_bytes([p1[0], p1[1], p1[2], 0.0])
    # The point array starts right after the block inside the same pointee.
    struct.pack_into("<Q", block, 0x30, ARENA_BASE + BLOCK_OFF + BLOCK_LEN)
    struct.pack_into("<i", block, 0x38, len(points))
    block[0x40:0x50] = _quad_bytes(quat)
    block[0x50:0x60] = _quad_bytes([off[0], off[1], off[2], 0.0])
    return bytes(block) + _points_bytes(points)


def make_args(i: int) -> List[ArgSpec]:
    case = CASES[i % len(CASES)]
    return [
        ArgSpec(kind="ptr", mem=_block_bytes(case), arena_offset=BLOCK_OFF),
        ArgSpec(kind="ptr", mem=bytes(32), arena_offset=OUT_EXPANSION_OFF),
        ArgSpec(kind="ptr", mem=bytes(32), arena_offset=OUT_START_OFF),
        ArgSpec(kind="ptr", mem=bytes(32), arena_offset=OUT_END_OFF),
    ]

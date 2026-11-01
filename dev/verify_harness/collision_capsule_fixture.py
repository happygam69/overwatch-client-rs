"""Inputs for the `collision_capsule` unit (`0x142ccd7b0`, type 1).

One case is a capsule record `{P0, P1, radius}`, a shape transform
`{position, world->local quaternion}` and a query block whose support-point
count is `<= 0`. The declared specials (first, so the mutation corpus covers
them) pin the degenerate capsule, both endpoint sphere recursions, the
inside-cylinder fallback and the sweep-parameter exits.
"""

from __future__ import annotations

import math
import random
import struct
from typing import List

from emu_oracle import ArgSpec

RECORD_OFF = 0x4000
OUT_OFF = 0x4100
BLOCK_OFF = 0x4200
XFORM_OFF = 0x4300

QUATS = [
    (0.0, 0.0, 0.0, 1.0),
    (0.0, 0.0, 0.70710678, 0.70710678),
    (0.70710678, 0.0, 0.0, 0.70710678),
    (0.0, 0.70710678, 0.0, 0.70710678),
    (0.5, -0.5, 0.5, 0.5),
    (2.0, 0.0, 0.0, 2.0),
    (0.0, 0.0, 0.0, 0.0),
]


# Two floats whose squares sum, in the client's (x^2 + y^2) + z^2 fold, to
# exactly the leaf's epsilon (0x057a0000). They pin the strict comparators.
EPS_X = struct.unpack("<f", struct.pack("<I", 0x227CFB72))[0]
EPS_Y = struct.unpack("<f", struct.pack("<I", 0x1C7FFFC4))[0]


def _normalize(v):
    length = math.sqrt(v[0] * v[0] + v[1] * v[1] + v[2] * v[2])
    if length == 0.0:
        return (0.0, 0.0, 1.0)
    return (v[0] / length, v[1] / length, v[2] / length)


def _cases():
    # (p0, p1, radius, start, end, motion, expansion, flag, quat)
    specials = [
        # Degenerate capsule: the sphere fallback at the midpoint, aimed
        # through the centre.
        ((0.0, 0.0, -1.0), (0.0, 0.0, -1.0), 1.0, (0.0, 0.0, 10.0),
         (0.0, 0.0, -10.0), (1.0, 1.0, 1.0), 0.0, 1, QUATS[0]),
        # A side hit: the sweep passes beside the capsule's barrel.
        ((0.0, 0.0, -2.0), (0.0, 0.0, 2.0), 1.0, (0.0, 0.0, 5.0),
         (0.0, 0.0, -5.0), (1.0, 1.0, 1.0), 0.0, 0, QUATS[0]),
        # The entry lands before P0: the sphere recursion at P0.
        ((0.0, 0.0, 0.0), (0.0, 0.0, 4.0), 1.0, (0.0, 0.0, -6.0),
         (0.0, 0.0, 2.0), (1.0, 1.0, 1.0), 0.0, 0, QUATS[0]),
        # The entry lands after P1: the sphere recursion at P1.
        ((0.0, 0.0, 0.0), (0.0, 0.0, 4.0), 1.0, (0.0, 0.0, 10.0),
         (0.0, 0.0, 3.0), (1.0, 1.0, 1.0), 0.0, 0, QUATS[0]),
        # Start inside the infinite cylinder but past the cap: the sphere
        # recursion at the clamped axis point.
        ((0.0, 0.0, 0.0), (0.0, 0.0, 4.0), 1.0, (0.2, 0.0, 5.5),
         (2.0, 0.0, 5.5), (1.0, 1.0, 1.0), 0.0, 0, QUATS[0]),
        # Start inside the capped capsule with the flag clear, then set.
        ((0.0, 0.0, 0.0), (0.0, 0.0, 4.0), 1.0, (0.2, 0.0, 2.0),
         (2.0, 0.0, 2.0), (1.0, 1.0, 1.0), 0.0, 0, QUATS[0]),
        ((0.0, 0.0, 0.0), (0.0, 0.0, 4.0), 1.0, (0.2, 0.0, 2.0),
         (2.0, 0.0, 2.0), (1.0, 1.0, 1.0), 0.0, 1, QUATS[0]),
        # The sweep does not reach the capsule inside the motion budget.
        ((0.0, 0.0, 0.0), (0.0, 0.0, 4.0), 1.0, (0.0, 0.0, 9.0),
         (0.0, 0.0, 0.0), (0.05, 0.05, 0.05), 0.0, 0, QUATS[0]),
        # A parallel sweep along the axis, offset sideways: the parallel
        # guard must pass and the closest approach decides.
        ((0.0, 0.0, 0.0), (0.0, 0.0, 4.0), 1.0, (0.5, 0.0, -3.0),
         (0.5, 0.0, 7.0), (1.0, 1.0, 1.0), 0.0, 0, QUATS[0]),
        # A miss far beside the barrel.
        ((0.0, 0.0, 0.0), (0.0, 0.0, 4.0), 1.0, (5.0, 0.0, -3.0),
         (5.0, 0.0, 7.0), (1.0, 1.0, 1.0), 0.0, 0, QUATS[0]),
        # Expansion makes the same side sweep hit.
        ((0.0, 0.0, -2.0), (0.0, 0.0, 2.0), 0.5, (0.0, 0.0, 5.0),
         (0.0, 0.0, -5.0), (1.0, 1.0, 1.0), 1.0, 0, QUATS[0]),
    ]

    # The remaining exits: the inside report's degenerate segment, the swept
    # path's degenerate segment, a sweep whose closest approach is behind its
    # start, and a hit exactly on the axis (a zero-radius capsule).
    specials.extend([
        ((0.0, 0.0, 0.0), (0.0, 0.0, 4.0), 1.0, (0.2, 0.0, 2.0),
         (0.2, 0.0, 2.0), (1.0, 1.0, 1.0), 0.0, 1, QUATS[0]),
        ((0.0, 0.0, 0.0), (0.0, 0.0, 4.0), 1.0, (5.0, 0.0, 2.0),
         (5.0, 0.0, 2.0), (1.0, 1.0, 1.0), 0.0, 1, QUATS[0]),
        ((0.0, 0.0, 0.0), (0.0, 0.0, 4.0), 1.0, (2.0, 0.0, 2.0),
         (4.0, 0.0, 2.0), (1.0, 1.0, 1.0), 0.0, 0, QUATS[0]),
        ((0.0, 0.0, 0.0), (0.0, 0.0, 4.0), 0.0, (5.0, 0.0, 2.0),
         (-5.0, 0.0, 2.0), (1.0, 1.0, 1.0), 0.0, 0, QUATS[0]),
    ])

    # Exact-boundary cases: `|axis|^2 == eps`, the start exactly on the
    # cylinder surface, exactly on the cap surface, and a segment whose
    # squared length is exactly eps inside the capsule.
    specials.extend([
        ((0.0, 0.0, 0.0), (EPS_X, EPS_Y, 0.0), 1.0, (0.0, 0.0, 10.0),
         (0.0, 0.0, -10.0), (1.0, 1.0, 1.0), 0.0, 1, QUATS[0]),
        ((0.0, 0.0, 0.0), (0.0, 0.0, 4.0), 1.0, (1.0, 0.0, 2.0),
         (1.0, 0.0, 6.0), (1.0, 1.0, 1.0), 0.0, 0, QUATS[0]),
        ((0.0, 0.0, 0.0), (0.0, 0.0, 4.0), 1.0, (0.5, 0.0, 4.8660253882408142),
         (-1.0, 0.0, 4.8660253882408142), (1.0, 1.0, 1.0), 0.0, 0, QUATS[0]),
        ((0.0, 0.0, 0.0), (0.0, 0.0, 4.0), 1.0, (0.0, 0.0, 0.0),
         (EPS_X, EPS_Y, 0.0), (1.0, 1.0, 1.0), 0.0, 1, QUATS[0]),
    ])

    rng = random.Random(1_420_5541)
    out = list(specials)
    for i in range(96):
        radius = [0.5, 1.0, 0.05, 2.0, 0.0][i % 5]
        p0 = (
            rng.uniform(-2.0, 2.0),
            rng.uniform(-2.0, 2.0),
            rng.uniform(-2.0, 2.0),
        )
        axis = _normalize(
            (rng.uniform(-1, 1), rng.uniform(-1, 1), rng.uniform(-1, 1))
        )
        length = rng.uniform(0.0, 5.0)
        p1 = (
            p0[0] + axis[0] * length,
            p0[1] + axis[1] * length,
            p0[2] + axis[2] * length,
        )
        position = (
            rng.uniform(-3.0, 3.0),
            rng.uniform(-3.0, 3.0),
            rng.uniform(-3.0, 3.0),
        )
        quat = QUATS[i % len(QUATS)]
        expansion = [0.0, 0.0, 0.25][i % 3]
        motion = [(1.0, 1.0, 1.0), (0.5, 0.5, 0.5)][i % 2]
        flag = 1 if i % 3 == 0 else 0
        # Half the cases aim at the capsule's midpoint, half are random.
        midpoint = (
            position[0] + 0.5 * (p0[0] + p1[0]),
            position[1] + 0.5 * (p0[1] + p1[1]),
            position[2] + 0.5 * (p0[2] + p1[2]),
        )
        if i % 2 == 0:
            direction = _normalize(
                (rng.uniform(-1, 1), rng.uniform(-1, 1), rng.uniform(-1, 1))
            )
            distance = radius + rng.uniform(0.5, 6.0)
            start = (
                midpoint[0] + direction[0] * distance,
                midpoint[1] + direction[1] * distance,
                midpoint[2] + direction[2] * distance,
            )
            toward = _normalize(
                (midpoint[0] - start[0], midpoint[1] - start[1], midpoint[2] - start[2])
            )
            travel = distance * rng.uniform(0.5, 2.0)
            end = (
                start[0] + toward[0] * travel,
                start[1] + toward[1] * travel,
                start[2] + toward[2] * travel,
            )
        else:
            start = (
                rng.uniform(-8.0, 8.0),
                rng.uniform(-8.0, 8.0),
                rng.uniform(-8.0, 8.0),
            )
            direction = _normalize(
                (rng.uniform(-1, 1), rng.uniform(-1, 1), rng.uniform(-1, 1))
            )
            travel = rng.uniform(0.1, 6.0)
            end = (
                start[0] + direction[0] * travel,
                start[1] + direction[1] * travel,
                start[2] + direction[2] * travel,
            )
        out.append((p0, p1, radius, start, end, motion, expansion, flag, quat))
    return out


CASES = _cases()


def _fbits(value: float) -> int:
    return struct.unpack("<I", struct.pack("<f", value))[0]


def _quad(values) -> bytes:
    return b"".join(struct.pack("<I", _fbits(v)) for v in values)


def _record(case) -> bytes:
    p0, p1, radius = case[0], case[1], case[2]
    rec = bytearray(0x30)
    rec[0x00:0x10] = _quad([p0[0], p0[1], p0[2], 0.0])
    rec[0x10:0x20] = _quad([p1[0], p1[1], p1[2], 0.0])
    rec[0x20:0x24] = struct.pack("<I", _fbits(radius))
    return bytes(rec)


def _block(case) -> bytes:
    start, end, motion, expansion = case[3], case[4], case[5], case[6]
    block = bytearray(0x78)
    block[0x00:0x10] = _quad([start[0], start[1], start[2], 0.0])
    block[0x10:0x20] = _quad([end[0], end[1], end[2], 0.0])
    block[0x20:0x30] = _quad([motion[0], motion[0], motion[0], motion[0]])
    struct.pack_into("<i", block, 0x38, 0)
    block[0x50:0x60] = _quad([expansion, expansion, expansion, expansion])
    block[0x72] = case[7]
    return bytes(block)


def _xform(case) -> bytes:
    # The transform's position is the origin, so the declared specials (which
    # assume an identity frame) aim in world space; the quaternion varies so
    # the local-frame fold is exercised.
    quat = case[8]
    out = bytearray(0x20)
    out[0x10:0x20] = _quad(quat)
    return bytes(out)


def make_args(i: int) -> List[ArgSpec]:
    case = CASES[i % len(CASES)]
    return [
        ArgSpec(kind="ptr", mem=_record(case), arena_offset=RECORD_OFF),
        ArgSpec(kind="ptr", mem=bytes(0x48), arena_offset=OUT_OFF),
        ArgSpec(kind="ptr", mem=_block(case), arena_offset=BLOCK_OFF),
        ArgSpec(kind="ptr", mem=_xform(case), arena_offset=XFORM_OFF),
    ]

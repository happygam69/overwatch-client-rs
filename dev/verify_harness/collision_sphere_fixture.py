"""Inputs for the `collision_sphere` unit (`0x142ccdf40`, type 0).

One case is a type-0 sphere record `{centre, radius}`, a shape transform
`{position, world->local quaternion}` and a query block whose support-point
count is `<= 0` (the count `> 0` path hands off to the unread `0x142cc9460`).
The cases mix segments aimed at the sphere (hits), segments that start inside
(the `+0x72` flag decides), and random segments that miss.
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
    (-0.0, 0.0, -0.0, 1.0),
    (float("nan"), 0.0, 0.0, 1.0),
    (float("inf"), 0.0, 0.0, 1.0),
]


# Two floats whose squares sum, inside the client's (x^2 + y^2) + z^2 fold,
# to exactly the leaf's epsilon (0x057a0000). They pin the strictness of the
# three `> eps` tests.
EPS_X = struct.unpack("<f", struct.pack("<I", 0x227CFB72))[0]
EPS_Y = struct.unpack("<f", struct.pack("<I", 0x1C7FFFC4))[0]

def _normalize(v):
    length = math.sqrt(v[0] * v[0] + v[1] * v[1] + v[2] * v[2])
    if length == 0.0:
        return (0.0, 0.0, 1.0)
    return (v[0] / length, v[1] / length, v[2] / length)


def _cases():
    # The declared specials come first: the mutation stage's pinned corpus is
    # the first 48 cases, so every boundary the comparators depend on must be
    # in that prefix.
    specials = []
    # p_sq == reach_sq exactly (the start sits on the expanded surface), and
    # a grazing segment where reach_sq == offset_sq exactly. Both pin a strict
    # comparison: the ray path is taken and hits at t = 0.
    specials.append(((0.0, 0.0, 0.0), 1.0, (0.0, 0.0, 0.0), QUATS[0],
                     (1.0, 0.0, 0.0), (1.0, 0.0, 2.0), (1.0, 1.0, 1.0), 0.0, 1))
    # delta_sq == eps exactly, start inside: the normalisation guard's
    # no-direction edge.
    specials.append(((0.0, 0.0, 0.0), 1.0, (0.0, 0.0, 0.0), QUATS[0],
                     (0.0, 0.0, 0.0), (EPS_X, EPS_Y, 0.0), (1.0, 1.0, 1.0), 0.0, 1))
    # d_sq == eps exactly, start outside (a zero-radius sphere): the ray
    # path's degenerate-direction exit, which must be strict.
    specials.append(((0.0, 0.0, 0.0), 0.0, (0.0, 0.0, 0.0), QUATS[0],
                     (0.0, 0.0, 0.0), (EPS_X, EPS_Y, 0.0), (1.0, 1.0, 1.0), 0.0, 1))
    # A zero-radius sphere with the segment aimed exactly through its centre:
    # the hit point lands on the local origin, so the "no direction" normal
    # edge runs.
    specials.append(((0.0, 0.0, 0.0), 0.0, (0.0, 0.0, 0.0), QUATS[0],
                     (0.0, 0.0, 20.0), (0.0, 0.0, 0.0), (1.0, 1.0, 1.0), 0.0, 0))
    # A long segment that reaches the sphere only after the motion budget, and
    # the same geometry with a budget that allows the hit.
    specials.append(((0.0, 0.0, 0.0), 1.0, (0.0, 0.0, 0.0), QUATS[0],
                     (0.0, 0.0, 20.0), (0.0, 0.0, 0.0), (0.05, 0.05, 0.05), 0.0, 0))
    specials.append(((0.0, 0.0, 0.0), 1.0, (0.0, 0.0, 0.0), QUATS[0],
                     (0.0, 0.0, 20.0), (0.0, 0.0, 0.0), (1.0, 1.0, 1.0), 0.0, 0))
    # Degenerate segment outside the sphere.
    specials.append(((0.0, 0.0, 0.0), 1.0, (0.0, 0.0, 0.0), QUATS[0],
                     (0.0, 0.0, 5.0), (0.0, 0.0, 5.0), (1.0, 1.0, 1.0), 0.0, 1))
    # Inside the sphere with a degenerate segment.
    specials.append(((0.0, 0.0, 0.0), 1.0, (0.0, 0.0, 0.0), QUATS[0],
                     (0.0, 0.0, 0.0), (0.0, 0.0, 0.0), (1.0, 1.0, 1.0), 0.0, 1))
    # A start exactly at the centre with a real segment.
    specials.append(((0.0, 0.0, 0.0), 1.0, (0.0, 0.0, 0.0), QUATS[0],
                     (0.0, 0.0, 0.0), (0.0, 0.0, 3.0), (1.0, 1.0, 1.0), 0.0, 1))
    # Inside the sphere with the "hit while already inside" flag clear.
    specials.append(((0.0, 0.0, 0.0), 1.0, (0.0, 0.0, 0.0), QUATS[0],
                     (0.0, 0.0, 0.0), (0.0, 0.0, 3.0), (1.0, 1.0, 1.0), 0.0, 0))

    rng = random.Random(1_420_5540)
    out = list(specials)
    for i in range(96):
        radius = [0.5, 1.0, 0.05, 4.0, 0.0, 1e-3][i % 6]
        center = (
            rng.uniform(-2.0, 2.0),
            rng.uniform(-2.0, 2.0),
            rng.uniform(-2.0, 2.0),
        )
        position = (
            rng.uniform(-3.0, 3.0),
            rng.uniform(-3.0, 3.0),
            rng.uniform(-3.0, 3.0),
        )
        quat = QUATS[i % len(QUATS)]
        expansion = [0.0, 0.0, 0.25, 1.5][i % 4]
        motion = [(1.0, 1.0, 1.0), (0.5, 0.5, 0.5)][i % 2]
        flag = 1 if i % 3 == 0 else 0

        # A point on the sphere's world surface: the inverse rotation is the
        # shape's own quaternion, so with q = identity the world centre is
        # position + centre.
        world_center = (
            position[0] + center[0],
            position[1] + center[1],
            position[2] + center[2],
        )
        mode = i % 5
        if mode in (0, 1):
            distance = radius + rng.uniform(1.0, 6.0)
            direction = _normalize(
                (rng.uniform(-1, 1), rng.uniform(-1, 1), rng.uniform(-1, 1))
            )
            start = (
                world_center[0] + direction[0] * distance,
                world_center[1] + direction[1] * distance,
                world_center[2] + direction[2] * distance,
            )
            jitter = (rng.uniform(-0.3, 0.3), rng.uniform(-0.3, 0.3), rng.uniform(-0.3, 0.3))
            toward = _normalize(
                (
                    world_center[0] - start[0] + jitter[0],
                    world_center[1] - start[1] + jitter[1],
                    world_center[2] - start[2] + jitter[2],
                )
            )
            travel = distance * rng.uniform(0.5, 2.0)
            end = (
                start[0] + toward[0] * travel,
                start[1] + toward[1] * travel,
                start[2] + toward[2] * travel,
            )
        elif mode == 2:
            offset = [
                rng.uniform(-0.4, 0.4) * max(radius, 1e-2),
                rng.uniform(-0.4, 0.4) * max(radius, 1e-2),
                rng.uniform(-0.4, 0.4) * max(radius, 1e-2),
            ]
            start = (
                world_center[0] + offset[0],
                world_center[1] + offset[1],
                world_center[2] + offset[2],
            )
            direction = _normalize(
                (rng.uniform(-1, 1), rng.uniform(-1, 1), rng.uniform(-1, 1))
            )
            travel = rng.uniform(0.5, 3.0)
            end = (
                start[0] + direction[0] * travel,
                start[1] + direction[1] * travel,
                start[2] + direction[2] * travel,
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
            travel = rng.uniform(0.1, 4.0)
            end = (
                start[0] + direction[0] * travel,
                start[1] + direction[1] * travel,
                start[2] + direction[2] * travel,
            )
        out.append(
            (center, radius, position, quat, start, end, motion, expansion, flag)
        )
    return out


CASES = _cases()


def _fbits(value: float) -> int:
    return struct.unpack("<I", struct.pack("<f", value))[0]


def _quad(values) -> bytes:
    return b"".join(struct.pack("<I", _fbits(v)) for v in values)


def _record(case) -> bytes:
    center, radius = case[0], case[1]
    rec = bytearray(0x20)
    rec[0x00:0x10] = _quad([center[0], center[1], center[2], 0.0])
    rec[0x10:0x14] = struct.pack("<I", _fbits(radius))
    return bytes(rec)


def _block(case) -> bytes:
    start, end, motion, expansion = case[4], case[5], case[6], case[7]
    block = bytearray(0x78)
    block[0x00:0x10] = _quad([start[0], start[1], start[2], 0.0])
    block[0x10:0x20] = _quad([end[0], end[1], end[2], 0.0])
    # The caller splats its scalar motion and expansion across all four lanes.
    block[0x20:0x30] = _quad([motion[0], motion[0], motion[0], motion[0]])
    # +0x30 support-point pointer is never dereferenced on the count <= 0 path.
    struct.pack_into("<i", block, 0x38, 0)
    block[0x50:0x60] = _quad([expansion, expansion, expansion, expansion])
    block[0x72] = case[8]
    return bytes(block)


def _xform(case) -> bytes:
    position, quat = case[2], case[3]
    out = bytearray(0x20)
    out[0x00:0x10] = _quad([position[0], position[1], position[2], 0.0])
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

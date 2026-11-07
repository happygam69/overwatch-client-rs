"""Inputs for the `closest_point` unit (`0x142cc9460`).

The client's signature is `(out, descriptor)`. The descriptor carries the two
point-array pointers, so the fixture patches them with absolute arena
addresses; the probe takes the two arrays as its third and fourth arguments
(fixture plumbing - the descriptor's pointers are ignored on the Rust side,
the bytes the client reads are the same arrays).

Descriptor, `0xA4` bytes:

    +0x00 ptr  first array             +0x20 ptr  second array
    +0x08 i32  first count             +0x28 i32  second count
    +0x10 quad first radius            +0x30 quad second radius
    +0x40 quad first translation       +0x60 quad second translation
    +0x50 quad first rotation          +0x70 quad second rotation
    +0x80 quad direction               +0x90 quad motion budget
    +0xA0 u8   flag0                   +0xA1 u8   flag1

The first array is the triangle (`3` points in the real caller); the second is
the swept shape's support cloud (`1..8` points). The corpus is ordered so the
mutation prefix covers the early exits first: a contact with a non-zero
extension, the flag-1 shrink, the flag-0 rejection, a penetration simplex, a
motion-budget exhaustion and the iteration cap.

The values are deterministic: `make_args(i)` is a pure function of `i`.
"""

from __future__ import annotations

import random
import struct
from typing import List, Tuple

from emu_oracle import ARENA_BASE, ArgSpec

OUT_OFF = 0x4000
DESC_OFF = 0x4100
ARRAY_A_OFF = 0x4200
ARRAY_B_OFF = 0x4400
DESC_LEN = 0xA4
MAX_POINTS = 8


def _quad(values) -> bytes:
    lanes = list(values) + [0.0] * (4 - len(values))
    return struct.pack("<4f", *[float(v) for v in lanes[:4]])


def _f32(value: float) -> float:
    return struct.unpack("<f", struct.pack("<f", value))[0]


# The radius the client's own arithmetic produces for the exact-equality case:
# radius_a 1.0, radius_b 0.01 -> offset 1.0 -> radius 1.00125f.
_EQUALITY_RADIUS = _f32(_f32(0.00125) + _f32(_f32(_f32(0.01) + 1.0) - _f32(0.01)))


def _random_quat(rng: random.Random) -> bytes:
    if rng.random() < 0.5:
        return _quad([0.0, 0.0, 0.0, 1.0])
    return _quad([rng.uniform(-1.0, 1.0) for _ in range(4)])


def _case(
    first: List[Tuple[float, float, float, float]],
    second: List[Tuple[float, float, float, float]],
    radius_a: float = 0.01,
    radius_b: float = 0.0,
    translation_a=(0.0, 0.0, 0.0),
    rotation_a=(0.0, 0.0, 0.0, 1.0),
    translation_b=(0.0, 0.0, 0.0),
    rotation_b=(0.0, 0.0, 0.0, 1.0),
    direction=(0.0, 0.0, 1.0),
    motion: float = 1.0,
    flag0: int = 0,
    flag1: int = 0,
) -> Tuple[bytes, bytes, bytes]:
    """One (descriptor, first-array, second-array) triple with the pointers
    left at zero; `make_args` patches them."""
    desc = bytearray(DESC_LEN)
    struct.pack_into("<i", desc, 0x08, len(first))
    desc[0x10:0x20] = _quad([radius_a] * 4)
    struct.pack_into("<i", desc, 0x28, len(second))
    desc[0x30:0x40] = _quad([radius_b] * 4)
    desc[0x40:0x50] = _quad(translation_a)
    desc[0x50:0x60] = _quad(rotation_a)
    desc[0x60:0x70] = _quad(translation_b)
    desc[0x70:0x80] = _quad(rotation_b)
    desc[0x80:0x90] = _quad(direction)
    desc[0x90:0xA0] = _quad([motion] * 4)
    desc[0xA0] = flag0
    desc[0xA1] = flag1
    first_bytes = b"".join(_quad(p) for p in first)
    second_bytes = b"".join(_quad(p) for p in second)
    return bytes(desc), first_bytes, second_bytes


def _seeded(rng: random.Random, count: int) -> List[Tuple[bytes, bytes, bytes]]:
    """The corpus shape the dev probe uses: random point clouds, radii,
    rotations, directions, budgets and both flags. Point counts run `1..8`."""
    cases: List[Tuple[bytes, bytes, bytes]] = []
    for _ in range(count):
        count_a = rng.choice([1, 2, 3, 3, 3, 4, 6])
        count_b = rng.choice([1, 1, 2, 3, 5, 8])
        scale = rng.choice([1e-3, 0.2, 1.0, 4.0])
        first = [
            tuple([rng.uniform(-1.0, 1.0) * scale for _ in range(3)] + [0.0])
            for _ in range(count_a)
        ]
        second = [
            tuple([rng.uniform(-1.0, 1.0) * scale for _ in range(3)] + [0.0])
            for _ in range(count_b)
        ]
        desc = bytearray(DESC_LEN)
        struct.pack_into("<i", desc, 0x08, count_a)
        desc[0x10:0x20] = _quad([rng.choice([0.0, 0.01, 0.05, 0.3])] * 4)
        struct.pack_into("<i", desc, 0x28, count_b)
        desc[0x30:0x40] = _quad([rng.choice([0.0, 0.01, 0.05, 0.3])] * 4)
        desc[0x40:0x50] = _quad([rng.uniform(-1.0, 1.0) * scale for _ in range(3)])
        desc[0x50:0x60] = _random_quat(rng)
        desc[0x60:0x70] = _quad([rng.uniform(-1.0, 1.0) * scale for _ in range(3)])
        desc[0x70:0x80] = _random_quat(rng)
        desc[0x80:0x90] = _quad([rng.uniform(-1.0, 1.0) for _ in range(3)])
        desc[0x90:0xA0] = _quad([rng.choice([0.0, 0.25, 0.5, 1.0])] * 4)
        desc[0xA0] = rng.choice([0, 1, 1])
        desc[0xA1] = rng.choice([0, 0, 1])
        cases.append(
            (
                bytes(desc),
                b"".join(_quad(p) for p in first),
                b"".join(_quad(p) for p in second),
            )
        )
    return cases


def _cases() -> List[Tuple[bytes, bytes, bytes]]:
    tri = [
        (1.0, 0.0, 0.0, 0.0),
        (0.0, 1.0, 0.0, 0.0),
        (0.0, 0.0, 0.0, 0.0),
    ]
    unit_tri = [
        (0.0, 0.0, 0.0, 0.0),
        (1.0, 0.0, 0.0, 0.0),
        (0.0, 1.0, 0.0, 0.0),
    ]
    cases: List[Tuple[bytes, bytes, bytes]] = [
        # The walk's own descriptor: a triangle against one support point, the
        # 0.35 expansion and the read motion budget. This is the case the live
        # character sweep builds.
        _case(
            unit_tri,
            [(0.25, 0.25, 0.5, 0.0)],
            radius_b=0.35,
            direction=(0.0, 0.0, -1.0),
        ),
        # A radius below the client's 0.01 floor on shape A, with a real
        # contact so the published point carries the floored radius.
        _case(
            unit_tri,
            [(0.25, 0.25, 0.5, 0.0)],
            radius_a=0.0,
            radius_b=0.35,
            direction=(0.0, 0.0, -1.0),
        ),
        # The same for shape B: the floored radius changes the convergence
        # radius and the residual.
        _case(
            unit_tri,
            [(0.25, 0.25, 0.5, 0.0)],
            radius_a=0.01,
            radius_b=0.0,
            direction=(0.0, 0.0, -1.0),
        ),
        # A distance between the real convergence radius and the 0.00125
        # addend's 1.5x mutation: the residual separates the two.
        _case(
            [(0.0, 0.0, 0.0, 0.0)],
            [(-1.0015, 0.0, 0.0, 0.0)],
            radius_a=1.0,
            radius_b=0.01,
            direction=(1.0, 0.0, 0.0),
            flag0=1,
        ),
        # Exact equality: distance_sq == radius_sq bit for bit. The client's
        # strict `<` walks to the iteration cap and accepts; a `<=` mutation
        # takes the inside branch and rejects (flag 0).
        _case(
            [(0.0, 0.0, 0.0, 0.0)],
            [(-_EQUALITY_RADIUS, 0.0, 0.0, 0.0)],
            radius_a=1.0,
            radius_b=0.01,
            direction=(1.0, 0.0, 0.0),
            flag0=0,
            flag1=0,
        ),
        # A contact at zero distance that the first convergence test accepts
        # through the non-zero extension path.
        _case(
            tri,
            [(0.2, 0.2, 0.0, 0.0)],
            radius_a=0.05,
            radius_b=0.1,
            translation_a=(0.5, 0.5, 0.0),
            direction=(0.0, 0.0, 1.0),
            motion=1.0,
            flag0=1,
        ),
        # Starting inside the radius with flag0 set: the output is published
        # without any support search.
        _case(
            unit_tri,
            [(0.1, 0.1, 0.0, 0.0)],
            radius_a=0.5,
            radius_b=0.5,
            direction=(0.0, 0.0, 1.0),
            flag0=1,
        ),
        # Flag0 clear and flag1 clear: the inside case is a miss.
        _case(
            unit_tri,
            [(0.1, 0.1, 0.0, 0.0)],
            radius_a=0.5,
            radius_b=0.5,
            direction=(0.0, 0.0, 1.0),
            flag0=0,
            flag1=0,
        ),
        # Flag1 set: the inside case records the distance in `best` and shrinks
        # the radius instead of accepting.
        _case(
            unit_tri,
            [(0.0, 0.0, 0.05, 0.0)],
            radius_a=0.2,
            radius_b=0.2,
            direction=(0.0, 0.0, 1.0),
            flag0=0,
            flag1=1,
        ),
        # A thin sweep with a small motion budget: the support step overshoots
        # the budget and the walk rejects.
        _case(
            unit_tri,
            [(0.0, 0.0, 4.0, 0.0)],
            radius_a=0.01,
            radius_b=0.01,
            direction=(0.0, 1.0, 0.0),
            motion=0.01,
        ),
        # A direction parallel to the support normal: the slope test rejects.
        _case(
            unit_tri,
            [(0.0, 0.0, 1.0, 0.0)],
            radius_a=0.01,
            radius_b=0.0,
            direction=(0.0, 0.0, 1.0),
            motion=1.0,
        ),
        # A tetrahedron around the origin with flag0 set: the four-vertex body
        # runs and the penetration branch publishes.
        _case(
            [
                (1.0, 1.0, 1.0, 0.0),
                (-1.0, -1.0, 1.0, 0.0),
                (-1.0, 1.0, -1.0, 0.0),
                (1.0, -1.0, -1.0, 0.0),
            ],
            [(0.0, 0.0, 0.0, 0.0)],
            radius_a=0.01,
            radius_b=0.01,
            direction=(0.0, 0.0, 1.0),
            flag0=1,
            flag1=0,
        ),
        # The same tetrahedron with flag0 clear: the penetration is a miss.
        _case(
            [
                (1.0, 1.0, 1.0, 0.0),
                (-1.0, -1.0, 1.0, 0.0),
                (-1.0, 1.0, -1.0, 0.0),
                (1.0, -1.0, -1.0, 0.0),
            ],
            [(0.0, 0.0, 0.0, 0.0)],
            radius_a=0.01,
            radius_b=0.01,
            direction=(0.0, 0.0, 1.0),
            flag0=0,
            flag1=0,
        ),
        # Duplicate support pairs: the first and second arrays both repeat a
        # point, so the walk re-projects the cached simplex instead of growing.
        _case(
            [
                (0.0, 0.0, 0.0, 0.0),
                (0.0, 0.0, 0.0, 0.0),
                (1.0, 0.0, 0.0, 0.0),
            ],
            [(0.5, 0.5, 0.5, 0.0), (0.5, 0.5, 0.5, 0.0)],
            radius_a=0.05,
            radius_b=0.05,
            direction=(1.0, 0.0, 0.0),
        ),
        # A rotated frame on both sides: the composed rows and both
        # translations are exercised.
        _case(
            tri,
            [(0.2, -0.1, 0.3, 0.0), (0.0, 0.4, 0.1, 0.0)],
            radius_a=0.03,
            radius_b=0.07,
            translation_a=(1.0, 2.0, 3.0),
            rotation_a=(0.1, 0.2, 0.3, 0.92736185),
            translation_b=(-0.5, 0.25, 1.5),
            rotation_b=(-0.2, 0.1, 0.05, 0.97339794),
            direction=(0.3, -0.4, 0.8660254),
            motion=0.75,
            flag0=1,
        ),
    ]
    rng = random.Random(0x142CC9460)
    cases.extend(_seeded(rng, 200))
    return cases


CASES = _cases()


def make_args(i: int) -> List[ArgSpec]:
    desc, first, second = CASES[i % len(CASES)]
    desc = bytearray(desc)
    struct.pack_into("<Q", desc, 0x00, ARENA_BASE + ARRAY_A_OFF)
    struct.pack_into("<Q", desc, 0x20, ARENA_BASE + ARRAY_B_OFF)
    return [
        ArgSpec(kind="ptr", mem=bytes(0x40), arena_offset=OUT_OFF),
        ArgSpec(kind="ptr", mem=bytes(desc), arena_offset=DESC_OFF),
        ArgSpec(kind="ptr", mem=first, arena_offset=ARRAY_A_OFF),
        ArgSpec(kind="ptr", mem=second, arena_offset=ARRAY_B_OFF),
    ]

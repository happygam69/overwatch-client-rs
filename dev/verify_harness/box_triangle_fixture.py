"""Inputs for the `box_triangle_overlap` unit (`0x142cd58a0`).

Each case is the gather's box - eight floats, the low corner then the high
corner - and the three vertices of one candidate triangle as float4 quads.
The `w` lane is carried through every test untouched, so it is part of the
compared channel and gets its own values.

The corpus is ordered so the mutation run, which only takes a prefix, always
sees every early exit: the slab reject, the normal-axis reject, each of the
three edge rejects, and the accept.
"""

from __future__ import annotations

import random
import struct
from typing import List

from emu_oracle import ArgSpec

BOX_OFF = 0x4000
V0_OFF = 0x4100
V1_OFF = 0x4200
V2_OFF = 0x4300

SPECIALS = [
    0.0,
    -0.0,
    1.0,
    -1.0,
    0.5,
    -0.5,
    2.0,
    -2.0,
    1e-30,
    -1e-30,
    float("inf"),
    float("-inf"),
    float("nan"),
    1e30,
    -1e30,
    3.4028235e38,
]


def _quad(v) -> bytes:
    return struct.pack("<4f", *v)


def _cases():
    # A unit box centred on the origin, reused by most cases.
    unit = (-1.0, -1.0, -1.0, 0.0, 1.0, 1.0, 1.0, 0.0)
    # A box whose low and high corners are not sorted, so the half extent is
    # negative: the client takes it as given.
    flipped = (1.0, 1.0, 1.0, 0.0, -1.0, -1.0, -1.0, 0.0)

    cases = [
        # Accept: an interior triangle.
        (unit, (0.0, 0.0, 0.0, 0.0), (1.0, 0.0, 0.0, 0.0), (0.0, 1.0, 0.0, 0.0)),
        # Accept: a triangle lying in the box's +x face.
        (unit, (1.0, -1.0, -1.0, 0.0), (1.0, 1.0, -1.0, 0.0), (1.0, 0.0, 1.0, 0.0)),
        # Accept: an axis-aligned triangle far inside a large box.
        (
            (-10.0, -10.0, -10.0, 0.0, 10.0, 10.0, 10.0, 0.0),
            (0.0, 0.0, 0.0, 0.0),
            (1.0, 0.0, 0.0, 0.0),
            (0.0, 1.0, 0.0, 0.0),
        ),
        # Slab reject on x, then on y, then on z: a triangle wholly to one side.
        (unit, (2.0, 0.0, 0.0, 0.0), (3.0, 0.0, 0.0, 0.0), (2.0, 1.0, 0.0, 0.0)),
        (unit, (0.0, 2.0, 0.0, 0.0), (0.0, 3.0, 0.0, 0.0), (1.0, 2.0, 0.0, 0.0)),
        (unit, (0.0, 0.0, 2.0, 0.0), (0.0, 0.0, 3.0, 0.0), (1.0, 0.0, 2.0, 0.0)),
        # Slab reject in the negative direction.
        (unit, (-2.0, 0.0, 0.0, 0.0), (-3.0, 0.0, 0.0, 0.0), (-2.0, 1.0, 0.0, 0.0)),
        # The triangle's AABB overlaps but it lies just past a corner: only the
        # normal axis or an edge axis can reject it.
        (
            unit,
            (1.01, 1.01, 1.01, 0.0),
            (2.01, 1.01, 1.01, 0.0),
            (1.01, 2.01, 1.01, 0.0),
        ),
        # A thin sliver whose normal axis alone separates it from the box.
        (
            (-1.0, -1.0, -1.0, 0.0, 1.0, 1.0, 1.0, 0.0),
            (2.0, -2.0, 2.0, 0.0),
            (2.0, 2.0, 2.0, 0.0),
            (-2.0, 0.0, 2.0, 0.0),
        ),
        # Degenerate: all three vertices equal, so every edge has zero length
        # and the normalisation divides by zero.
        (unit, (0.5, 0.5, 0.5, 0.0), (0.5, 0.5, 0.5, 0.0), (0.5, 0.5, 0.5, 0.0)),
        (unit, (0.5, 0.5, 0.5, 0.0), (0.5, 0.5, 0.5, 0.0), (0.5, 0.5, 0.5, 0.0)),
        # Degenerate: two vertices equal (one zero-length edge).
        (unit, (0.0, 0.0, 0.0, 0.0), (0.0, 0.0, 0.0, 0.0), (1.0, 1.0, 1.0, 0.0)),
        # Degenerate: collinear vertices, so the normal is zero.
        (unit, (-1.0, -1.0, -1.0, 0.0), (0.0, 0.0, 0.0, 0.0), (1.0, 1.0, 1.0, 0.0)),
        # A zero-size box at a triangle vertex.
        (
            (0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
            (-1.0, -1.0, 0.0, 0.0),
            (1.0, -1.0, 0.0, 0.0),
            (0.0, 1.0, 0.0, 0.0),
        ),
        # Unsorted corners: the half extent goes negative.
        (flipped, (0.0, 0.0, 0.0, 0.0), (1.0, 0.0, 0.0, 0.0), (0.0, 1.0, 0.0, 0.0)),
        # The `w` lanes matter only through the carried quads, so give them
        # values that are not zero and a NaN.
        (unit, (0.0, 0.0, 0.0, 3.0), (1.0, 0.0, 0.0, -2.0), (0.0, 1.0, 0.0, 0.5)),
        (
            (-1.0, -1.0, -1.0, 1.0, 1.0, 1.0, 1.0, 1.0),
            (0.0, 0.0, 0.0, 0.0),
            (1.0, 0.0, 0.0, 0.0),
            (0.0, 1.0, 0.0, 0.0),
        ),
        (
            (-1.0, -1.0, -1.0, 0.0, 1.0, 1.0, 1.0, 0.0),
            (0.0, 0.0, 0.0, float("nan")),
            (1.0, 0.0, 0.0, 0.0),
            (0.0, 1.0, 0.0, 0.0),
        ),
        # The edge axes' tolerance band. The mutation stage only takes a prefix
        # of the corpus, and a scale change to an edge axis (`1/sqrt(e.e)`
        # replaced by `sqrt(e.e)`, or the `1.0` replaced) is invisible unless a
        # case sits inside `0 < edge lane <= 0.005`: on such a case the client
        # accepts while the scaled lane is over the tolerance and rejects.
        # Each case below was translated until the deciding lane of that edge
        # measured 0.0045 against `0x142cd58a0`, then the client's own bytes
        # fixed the verdict (accept).
        #
        # Edge 1 decides alone (lane 0.029, x/y/z slabs and edges 2/3 clear):
        # with the axis zeroed the client accepts, so this case is what makes a
        # zeroed `1.0` observable.
        (unit, (-1.25, -0.5, 1.0, 0.0), (0.75, 0.25, 1.25, 0.0), (-1.5, -1.75, 1.75, 0.0)),
        # Edge 2 decides alone (lane 0.194): the same, for `e12`.
        (unit, (-1.75, 1.25, 1.5, 0.0), (-1.0, 0.25, 1.75, 0.0), (-1.75, -1.0, -0.75, 0.0)),
        # Edge 1's lane 0.0045, `e01 . e01` 4.625: a 1.5 or 4.625 scale of the
        # axis puts the lane over 0.005 and the client would reject, so the
        # normalize mutants diverge here. The client accepts.
        (
            unit,
            (-1.0387098789215088, -0.5, 1.0, 0.0),
            (0.961290180683136, 0.25, 1.25, 0.0),
            (-1.2887098789215088, -1.75, 1.75, 0.0),
        ),
        # Edge 2's lane 0.0045, `e12 . e12` 8.375: the same for the second edge.
        (
            unit,
            (-1.75, 1.25, 0.767363965511322, 0.0),
            (-1.0, 0.25, 1.0173640251159668, 0.0),
            (-1.75, -1.0, -1.4826359748840332, 0.0),
        ),
        # An unsorted w corner (`high.w` below `low.w`) with a `w` edge: the
        # edge lanes' own `w` lane is then `-2*|axis.w|*half.w` = 4 on edge 1
        # and 2.83 on edge 2, so a widened xyz mask (`& 7` -> `& 0xf`) rejects
        # while the client, which masks with 7, accepts. The corner is unsorted
        # in `w` only, the way the `flipped` case is unsorted in xyz.
        (
            (-1.0, -1.0, -1.0, 0.0, 1.0, 1.0, 1.0, -2.0),
            (0.0, 0.0, 0.0, 0.0),
            (1.0, 0.0, 0.0, 2.0),
            (0.0, 1.0, 0.0, 0.0),
        ),
    ]

    # Every special value in one coordinate of one vertex, against the unit
    # box: this is where the ordered compares and the `0.005` tolerance part
    # ways with a naive port.
    for s in SPECIALS:
        cases.append((unit, (s, 0.0, 0.0, 0.0), (1.0, 0.0, 0.0, 0.0), (0.0, 1.0, 0.0, 0.0)))
        cases.append((unit, (0.0, 0.0, 0.0, 0.0), (s, s, s, 0.0), (0.0, 1.0, 0.0, 0.0)))
        cases.append(
            (
                (-1.0, -1.0, -1.0, 0.0, s, s, s, 0.0),
                (0.0, 0.0, 0.0, 0.0),
                (1.0, 0.0, 0.0, 0.0),
                (0.0, 1.0, 0.0, 0.0),
            )
        )

    # A deterministic sweep. Half the triangles sit near the box, half are
    # spread over a range wide enough to make the slab and normal tests both
    # decide cases.
    rng = random.Random(0x142C_D58A0)
    for i in range(220):
        scale = 1.5 if i % 2 == 0 else 6.0
        lo = [rng.uniform(-1.0, 1.0) for _ in range(3)]
        hi = [lo[k] + rng.uniform(0.05, 2.0) for k in range(3)]
        box8 = tuple(lo + [0.0] + hi + [0.0])
        verts = [
            tuple([rng.uniform(-scale, scale) for _ in range(3)] + [0.0])
            for _ in range(3)
        ]
        cases.append((box8, verts[0], verts[1], verts[2]))

    # Small triangles near the box faces: these are the accept/reject boundary
    # and they make the edge tolerances load-bearing.
    for i in range(60):
        base = [rng.uniform(-1.2, 1.2) for _ in range(3)]
        e1 = [rng.uniform(-0.2, 0.2) for _ in range(3)]
        e2 = [rng.uniform(-0.2, 0.2) for _ in range(3)]
        v0 = tuple(base + [0.0])
        v1 = tuple([base[k] + e1[k] for k in range(3)] + [0.0])
        v2 = tuple([base[k] + e2[k] for k in range(3)] + [0.0])
        cases.append((unit, v0, v1, v2))

    return cases


CASES = _cases()


def make_args(i: int) -> List[ArgSpec]:
    box8, v0, v1, v2 = CASES[i % len(CASES)]
    return [
        ArgSpec(kind="ptr", mem=_quad(box8[0:4]) + _quad(box8[4:8]), arena_offset=BOX_OFF),
        ArgSpec(kind="ptr", mem=_quad(v0), arena_offset=V0_OFF),
        ArgSpec(kind="ptr", mem=_quad(v1), arena_offset=V1_OFF),
        ArgSpec(kind="ptr", mem=_quad(v2), arena_offset=V2_OFF),
    ]

"""Inputs for the `controller_step_stage` unit (`0x142cf5020`).

The client's step stage takes the variant block (the plane array's pointer at
`+0x440` and the plane count at `+0x44c`), an output position, an output
round count, the requested position and the epsilon quad. A plane record is
`0x40` bytes: the normal and the offset at `+0x00`, the push limit at `+0x10`
and the accumulated push at `+0x20`.

The array lives right after the block inside the block's own pointee and the
block's `+0x440` holds the arena address of the array, so the oracle
dereferences the real bytes and the probe sees the same ones in the same
pointee.
"""

from __future__ import annotations

import random
import struct
from typing import List

from emu_oracle import ARENA_BASE, ArgSpec

BLOCK_OFF = 0x5000
BLOCK_LEN = 0x450
PLANE_LEN = 0x40
OUT_POS_OFF = 0x6000
OUT_COUNT_OFF = 0x6100
REQUESTED_OFF = 0x6200
EPS_OFF = 0x6300

# The client's epsilon: `MoverAdvance` writes `0x3ba3d70a` into the sweep
# block every frame, so the stage always converges to five millimetres.
EPS = 0.005

# The normals worth pinning: the three axes, a tilted one, a degenerate one
# and the unit vector with a zero length.
NORMALS = [
    (0.0, 1.0, 0.0),
    (1.0, 0.0, 0.0),
    (0.0, 0.0, 1.0),
    (0.0, -1.0, 0.0),
    (-1.0, 0.0, 0.0),
    (0.0, 0.0, -1.0),
    (0.57735026, 0.57735026, 0.57735026),
    (-0.26726124, 0.53452248, 0.80178373),
    (0.0, 0.0, 0.0),
    (1.0, 1.0, 1.0),
]

# The limits worth pinning: nothing, the skin, a small cap, the client's
# usual order of magnitude and a cap that never binds.
LIMITS = [0.0, 0.005, 0.01, 0.1, 1.0, 100.0, 1.0e6]

# The offsets: the plane's own distance, so a point at `offset` along the
# normal sits exactly on the plane.
OFFSETS = [0.0, 0.005, -0.005, 0.2, -0.2, 1.0, -1.0, 12.5]


def _fbits(value: float) -> int:
    return struct.unpack("<I", struct.pack("<f", value))[0]


def _quad_bytes(values) -> bytes:
    return b"".join(struct.pack("<I", _fbits(v)) for v in values)


def _cases():
    rng = random.Random(5020)
    out = []
    # No plane at all: the stage answers the requested position unchanged.
    out.append(([], (0.0, 0.0, 0.0, 0.0)))
    # One plane per count from 1 to 12, so the unrolled group of four and the
    # tail that handles the remainder are both reached.
    for count in range(1, 13):
        planes = []
        for index in range(count):
            normal = NORMALS[index % len(NORMALS)]
            limit = LIMITS[(index * 3 + count) % len(LIMITS)]
            offset = OFFSETS[(index * 5 + count) % len(OFFSETS)]
            planes.append((normal, offset, limit))
        requested = (
            rng.uniform(-3.0, 3.0),
            rng.uniform(-3.0, 3.0),
            rng.uniform(-3.0, 3.0),
            0.0,
        )
        out.append((planes, requested))
    # The point exactly on a plane, half a skin above it and a skin below it:
    # the candidate crosses zero around there.
    for offset in OFFSETS:
        out.append(([((0.0, 1.0, 0.0), offset, 1.0)], (0.0, offset, 0.0, 0.0)))
        out.append(([((0.0, 1.0, 0.0), offset, 1.0)], (0.0, offset - 0.005, 0.0, 0.0)))
        out.append(([((0.0, 1.0, 0.0), offset, 1.0)], (0.0, offset + 0.005, 0.0, 0.0)))
    # Two planes that push against each other, and four that do.
    out.append(
        (
            [
                ((1.0, 0.0, 0.0), 0.0, 1.0),
                ((-1.0, 0.0, 0.0), 0.0, 1.0),
            ],
            (-1.0, 0.0, 0.0, 0.0),
        )
    )
    out.append(
        (
            [
                ((1.0, 0.0, 0.0), 0.0, 1.0),
                ((-1.0, 0.0, 0.0), 0.0, 1.0),
                ((0.0, 1.0, 0.0), 0.0, 1.0),
                ((0.0, -1.0, 0.0), 0.0, 1.0),
            ],
            (-1.0, -1.0, 0.0, 0.0),
        )
    )
    # A limit that caps the push at the skin, so the stage creeps.
    for limit in (0.0, 0.001, 0.005, 0.01):
        out.append(([((0.0, 1.0, 0.0), 0.0, limit)], (0.0, -2.0, 0.0, 0.0)))
    # The fourth lane: the client writes all four, so the request's own lane
    # has to travel with it.
    out.append(([((0.0, 1.0, 0.0), 0.0, 1.0)], (0.0, -1.0, 0.0, 1.0)))
    out.append(([], (1.0, 2.0, 3.0, 4.0)))
    # A plain walk with a floor and a wall.
    out.append(
        (
            [
                ((0.0, 1.0, 0.0), 0.0, 100.0),
                ((1.0, 0.0, 0.0), 5.0, 100.0),
            ],
            (0.0, 0.5, 0.0, 0.0),
        )
    )
    return out


CASES = _cases()


def _plane_bytes(plane) -> bytes:
    normal, offset, limit = plane
    out = bytearray(PLANE_LEN)
    out[0x00:0x10] = _quad_bytes([normal[0], normal[1], normal[2], offset])
    out[0x10:0x20] = _quad_bytes([limit] * 4)
    # The accumulated push starts at zero, the way the solver's pair walk
    # writes it.
    out[0x20:0x30] = _quad_bytes([0.0] * 4)
    return bytes(out)


def make_args(i: int) -> List[ArgSpec]:
    planes, requested = CASES[i % len(CASES)]
    block = bytearray(BLOCK_LEN)
    array = b"".join(_plane_bytes(plane) for plane in planes)
    struct.pack_into("<Q", block, 0x440, ARENA_BASE + BLOCK_OFF + BLOCK_LEN)
    struct.pack_into("<i", block, 0x44C, len(planes))
    return [
        ArgSpec(
            kind="ptr",
            mem=bytes(block) + array,
            arena_offset=BLOCK_OFF,
        ),
        ArgSpec(kind="ptr", mem=bytes(16), arena_offset=OUT_POS_OFF),
        ArgSpec(kind="ptr", mem=bytes(4), arena_offset=OUT_COUNT_OFF),
        ArgSpec(kind="ptr", mem=_quad_bytes(requested), arena_offset=REQUESTED_OFF),
        ArgSpec(kind="ptr", mem=_quad_bytes([EPS] * 4), arena_offset=EPS_OFF),
    ]

"""Inputs for the `collision_shape_box` unit (`0x142cd5c80`).

One case is a transform block and a shape box. The transform block's `+0x00`
is the rotation's first row and its `+0x10` the quaternion, already
conjugated by the caller (`0x143110d50` flips the sign of the vector part);
the shape box's `+0x00` is the local minimum and `+0x10` the local maximum.
The function writes the transformed box's minimum at the output's `+0x00` and
its maximum at `+0x10`.

The quaternions worth pinning are the ones the client itself can produce: the
identity, the quarter turns about each axis, a non-unit quaternion (the
client never normalises), the zero quaternion, and the IEEE specials. The
boxes worth pinning are the ordinary ones, a degenerate box, an inverted box
(the minimum above the maximum, so the half extent is negative) and one that
carries a fourth lane.
"""

from __future__ import annotations

import random
import struct
from typing import List

from emu_oracle import ArgSpec

OUT_OFF = 0x4000
TRANSFORM_OFF = 0x4100
BOX_OFF = 0x4200

TRANSFORM_LEN = 0x20
BOX_LEN = 0x20

# The quaternion the caller hands over: already conjugated, so the vector
# part is the negated one. The zero quaternion is in the list because the
# client's own arithmetic turns it into the identity rotation.
#
# Every value is finite: an infinity in the quaternion reaches `0 * inf`,
# and the emulator's softfloat answers that invalid operation with
# 0x7fc00000 where the x86 hardware QNaN indefinite is 0xffc00000 - the
# same limitation fn/ShapeWorldTransform and fn/ClosestPointGjk declare.
QUAT_EDGE = [
    (0.0, 0.0, 0.0, 1.0),
    (0.0, 0.0, -0.70710678, 0.70710678),
    (0.0, -0.70710678, 0.0, 0.70710678),
    (-0.70710678, 0.0, 0.0, 0.70710678),
    (-0.5, -0.5, -0.5, 0.5),
    (2.0, -2.0, 0.5, 0.25),
    (0.0, 0.0, 0.0, 0.0),
    (-0.0, 0.0, -0.0, 1.0),
    (1.0, 1.0, 1.0, -1.0),
    (1e-30, -1e-30, 1e-30, 1e-30),
    (1e30, 1e30, -1e30, 1e30),
    (-3.0, 0.25, 7.5, -0.125),
]

# The row the client builds for the transform: the translation of the shape's
# own frame, and the fourth lane the row's `w`.
ROW_EDGE = [
    (0.0, 0.0, 0.0, 0.0),
    (1.0, 2.0, 3.0, 4.0),
    (-1.5, 0.25, -8.0, 0.5),
    (1e6, -1e6, 1e-6, -1e-6),
]

BOX_EDGE = [
    # (minimum, maximum)
    ((0.0, 0.0, 0.0, 0.0), (0.0, 0.0, 0.0, 0.0)),
    ((-1.0, -1.0, -1.0, 0.0), (1.0, 1.0, 1.0, 0.0)),
    ((1.0, 1.0, 1.0, 0.0), (-1.0, -1.0, -1.0, 0.0)),
    ((0.5, -2.0, 3.0, 4.0), (0.5, -2.0, 3.0, 4.0)),
    ((-1e6, -1e6, -1e6, 0.0), (1e6, 1e6, 1e6, 0.0)),
    ((-1e-6, 1e-6, -1e-6, 0.0), (1e-6, -1e-6, 1e-6, 0.0)),
]


def _cases():
    rng = random.Random(23680)
    out = []
    # A deterministic sweep: every quaternion edge against every row edge,
    # then the ordinary boxes.
    for quat in QUAT_EDGE:
        for row in ROW_EDGE:
            out.append((quat, row, BOX_EDGE[1]))
    # Every box edge against the identity and against a quarter turn.
    for index, box in enumerate(BOX_EDGE):
        out.append((QUAT_EDGE[index % len(QUAT_EDGE)], ROW_EDGE[index % len(ROW_EDGE)], box))
    # Random ordinary cases: the bulk of the corpus.
    for _ in range(72):
        quat = tuple(rng.uniform(-1.0, 1.0) for _ in range(4))
        row = tuple(rng.uniform(-4.0, 4.0) for _ in range(4))
        low = tuple(rng.uniform(-10.0, 10.0) for _ in range(3))
        high = tuple(low[axis] + rng.uniform(0.0, 4.0) for axis in range(3))
        out.append((quat, row, (low + (0.0,), high + (0.0,))))
    return out


CASES = _cases()


def _fbits(value: float) -> int:
    return struct.unpack("<I", struct.pack("<f", value))[0]


def _quad_bytes(values) -> bytes:
    return b"".join(struct.pack("<I", _fbits(v)) for v in values)


def _transform_bytes(quat, row) -> bytes:
    return _quad_bytes(row) + _quad_bytes(quat)


def _box_bytes(box) -> bytes:
    low, high = box
    return _quad_bytes(low) + _quad_bytes(high)


def make_args(i: int) -> List[ArgSpec]:
    quat, row, box = CASES[i % len(CASES)]
    return [
        ArgSpec(kind="ptr", mem=bytes(32), arena_offset=OUT_OFF),
        ArgSpec(kind="ptr", mem=_transform_bytes(quat, row), arena_offset=TRANSFORM_OFF),
        ArgSpec(kind="ptr", mem=_box_bytes(box), arena_offset=BOX_OFF),
    ]

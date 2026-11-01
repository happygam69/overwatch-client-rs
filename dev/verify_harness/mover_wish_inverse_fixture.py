"""Inputs for the `fn/MoverWishInverse` unit.

The client block starts at `0x140cb537b`, after the `FUN_140cb83b0` state
lookup. It reads the 4x4 column-major matrix from `[RSP+0x30]` and the vector
from `RBX`, computes the three cross products and `det = c0 . (c1 x c2)`,
inverts `R` with `rcpps` + Newton, and writes `R^-1 * (v - c3)` back through
`RBX`.

The matrix is fixed because the unit's state block is fixed; it is a generic
invertible affine (`det = 39/256`, exact in f32) so every shuffle lane is
distinct and a wrong lane order is observable. The vector varies per case.
"""

from __future__ import annotations

import random
import struct
from typing import List

from emu_oracle import ArgSpec

VEC_LEN = 16
MATRIX_LEN = 64

MATRIX = struct.pack(
    "<16f",
    # c0, c1, c2, c3
    0.5, 0.25, 0.125, 0.0,
    -0.25, 0.5, 0.0, 0.0,
    0.125, -0.125, 0.5, 0.0,
    1.5, -2.25, 0.75, 1.0,
)
MATRIX_HEX = MATRIX.hex()

_FINITE = [
    0.0, -0.0, 1.0, -1.0, 2.0, -2.0, 0.5, -0.5,
    3.0, -3.0, 0.25, -0.25, 7.0, -7.0, 1e-3, -1e-3,
]

_PINNED = [
    (0.0, 0.0, 0.0, 0.0),
    (1.0, 2.0, 3.0, 1.0),
    (-1.0, -2.0, -3.0, 1.0),
    (0.5, -0.5, 0.25, 1.0),
    (1.5, -2.25, 0.75, 1.0),
    (2.0, 2.0, 2.0, 2.0),
    (-0.25, 0.125, -0.5, 0.0),
]


def _case(i: int):
    if i < len(_PINNED):
        return _PINNED[i]
    rng = random.Random(0x494E_5652 ^ i)
    return tuple(rng.choice(_FINITE) for _ in range(4))


def make_args(i: int) -> List[ArgSpec]:
    vector = struct.pack("<4f", *_case(i))
    return [
        ArgSpec(kind="ptr", mem=vector),
        ArgSpec(kind="ptr", mem=MATRIX),
    ]

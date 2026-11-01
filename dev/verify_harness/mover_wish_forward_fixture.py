"""Inputs for the `fn/MoverWishForward` unit.

The client leaf block starts at `0x140cb5534` (after the `FUN_140cb83b0`
state-lookup branch). It takes the vector pointer in `RBX` and a 4x4
column-major matrix at `[RSP+0x30]`; the unit's `startRegs` seeds RBX and a
smaller RSP so the block's `ADD RSP,0x70; POP RBX; RET` lands on the harness
sentinel. The matrix is fixed (1..16) and is also present in the unit's
`state` snapshot; the probe receives the same bytes as its second argument.
"""

from __future__ import annotations

import random
import struct
from typing import List

from emu_oracle import ArgSpec

VEC_LEN = 16
MATRIX_LEN = 64
MATRIX = struct.pack("<16f", *[float(i) for i in range(1, 17)])
MATRIX_HEX = MATRIX.hex()

_FINITE = [
    0.0, -0.0, 1.0, -1.0, 2.0, -2.0, 0.5, -0.5,
    3.0, -3.0, 0.25, -0.25, 7.0, -7.0, 1e-3, -1e-3,
]

_PINNED = [
    (1.0, 2.0, 3.0, 4.0),
    (0.0, 0.0, 0.0, 0.0),
    (-1.0, -2.0, -3.0, -4.0),
    (1.0, 0.0, 0.0, 0.0),
    (0.0, 1.0, 0.0, 0.0),
    (0.0, 0.0, 1.0, 0.0),
    (0.0, 0.0, 0.0, 1.0),
]


def _case(i: int):
    if i < len(_PINNED):
        return _PINNED[i]
    rng = random.Random(0x5749_5348 ^ i)
    return tuple(rng.choice(_FINITE) for _ in range(4))


def make_args(i: int) -> List[ArgSpec]:
    vector = struct.pack("<4f", *_case(i))
    return [
        ArgSpec(kind="ptr", mem=vector),
        ArgSpec(kind="ptr", mem=MATRIX),
    ]

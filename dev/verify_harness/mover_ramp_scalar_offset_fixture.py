"""Inputs for the `fn/MoverRampScalarOffset` unit.

`MoverRampScalarOffset` (`0x140ca7570`) reads the flags byte at
`body + 0x10d0` and adds the scalar at `body + 0x10fc`. The base ramp fixture
supplies the timer and the two scale slots; this module copies the flag bit
into the body and sets a deterministic finite offset.
"""

from __future__ import annotations

import random
import struct
from typing import List

import mover_ramp_fixture as base
from emu_oracle import ArgSpec


def make_args(i: int) -> List[ArgSpec]:
    args = base.make_args(i)
    body = bytearray(args[0].mem)
    body[0x10D0] = args[2].mem[0]
    rng = random.Random(0x5241_4D4F ^ i)
    struct.pack_into("<I", body, 0x10FC, rng.choice(base.FINITE_POOL))
    return [ArgSpec(kind="ptr", mem=bytes(body))]

"""Inputs for the `fn/MoverRampScalar` unit.

The scalar leaf `MoverRampScalar` (`0x140ca75f0`) reads the flags byte from
`body + 0x10d0` itself (the vector unit passes that block as a separate
pointee). The base ramp fixture keeps the flags byte in its third pointee, so
this module copies it into the body before returning the one body argument.
"""

from __future__ import annotations

from typing import List

import mover_ramp_fixture as base
from emu_oracle import ArgSpec


def make_args(i: int) -> List[ArgSpec]:
    args = base.make_args(i)
    body = bytearray(args[0].mem)
    body[0x10D0] = args[2].mem[0]
    return [ArgSpec(kind="ptr", mem=bytes(body))]

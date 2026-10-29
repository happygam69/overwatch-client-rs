"""Structured inputs for fn/TextureRequestCount.

The function is `FUN_142e37600` (`int (int payload_count, int mip_count, int
base_mips, int streamed)`). It has two branches: `mip - base <= streamed`
returns 0, otherwise it computes `max(1, payload - streamed - 1)`. The specials
pin both branches, the `max(1, …)` clamp, and the signed `int` edges (the
function's arguments are `int`, so 0xFFFFFFFF is -1).
"""

from __future__ import annotations

from typing import List

from emu_oracle import ArgSpec
from gen import UnitArg, generate_args

SPECS = [
    UnitArg("payload_count", "u64"),
    UnitArg("mip_count", "u64"),
    UnitArg("base_mips", "u64"),
    UnitArg("streamed", "u64"),
]


def _case(payload: int, mip: int, base: int, streamed: int) -> List[ArgSpec]:
    return [
        ArgSpec(kind="u64", bits=payload & 0xFFFF_FFFF_FFFF_FFFF),
        ArgSpec(kind="u64", bits=mip & 0xFFFF_FFFF_FFFF_FFFF),
        ArgSpec(kind="u64", bits=base & 0xFFFF_FFFF_FFFF_FFFF),
        ArgSpec(kind="u64", bits=streamed & 0xFFFF_FFFF_FFFF_FFFF),
    ]


_SPECIALS: List[List[ArgSpec]] = [
    # payload - streamed - 1, and the max(1, ...) clamp.
    _case(4, 8, 3, 0),
    _case(4, 8, 3, 1),
    _case(4, 8, 3, 2),
    _case(4, 8, 3, 3),
    _case(1, 8, 3, 0),
    _case(2, 8, 3, 0),
    _case(0, 8, 3, 0),
    # mip - base <= streamed: the zero branch, including equality.
    _case(4, 4, 3, 1),
    _case(4, 3, 3, 0),
    _case(4, 8, 8, 0),
    _case(4, 0, 0, 0),
    _case(4, 16, 16, 0),
    _case(4, 16, 15, 0),
    # int edges: the callee reads 32-bit signed values.
    _case(4, 0xFFFF_FFFF, 0, 0),          # mip = -1
    _case(4, 0x8000_0000, 0, 0),          # mip = INT_MIN
    _case(0x7FFF_FFFF, 0x7FFF_FFFF, 0, 0),
    _case(4, 0, 0xFFFF_FFFF, 0),          # base = -1
    _case(0xFFFF_FFFF, 8, 3, 0),          # payload = -1
]


def make_args(i: int) -> List[ArgSpec]:
    if i < len(_SPECIALS):
        return _SPECIALS[i]
    return generate_args(SPECS, 4241, i - len(_SPECIALS))

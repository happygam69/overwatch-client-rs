"""Inputs for the `fn/VariantPosition` unit.

Reuses the shared world-position state block: the handle table at the global
`0x1442a0bc0`, two records and both arrays. Each case passes a variant
object pointer (or zero); the dispatcher selects `[variant+0x10]` for tag 1
(rigid) and `[variant+8]` otherwise (character).
"""

from __future__ import annotations

from typing import List

from emu_oracle import ArgSpec
from world_position_fixture import STATE_BASE, STATE_HEX

CASES = [0, 140702205611072, 140702205611104, 140702205611200, 140702205611232, 140702205611136, 140702205611168, 140702205611264, 140702205611296, 140702205611328, 140702205611360]


def make_args(i: int) -> List[ArgSpec]:
    return [
        ArgSpec(kind="u64", bits=CASES[i % len(CASES)]),
        ArgSpec(kind="ptr", mem=bytes.fromhex(STATE_HEX), arena_offset=0x6000),
        ArgSpec(kind="u64", bits=STATE_BASE),
    ]

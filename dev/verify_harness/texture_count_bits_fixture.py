"""Structured inputs for fn/TextureCountBitsShifted.

The function is `FUN_142e374d0` (`uint (uint value, char bias)`): it popcounts
`value >> ((bias + 2) & 31)`. The specials pin every reachable shift for a
dense value, the all-ones value at each of the five shifts the client's bias
selects, and the `char` bias wrap at 0, 31, 32, 255 and 256.
"""

from __future__ import annotations

from typing import List

from emu_oracle import ArgSpec
from gen import UnitArg, generate_args

SPECS = [UnitArg("value", "u64"), UnitArg("bias", "u64")]


def _case(value: int, bias: int) -> List[ArgSpec]:
    return [
        ArgSpec(kind="u64", bits=value & 0xFFFF_FFFF_FFFF_FFFF),
        ArgSpec(kind="u64", bits=bias & 0xFFFF_FFFF_FFFF_FFFF),
    ]


_SPECIALS: List[List[ArgSpec]] = [
    _case(0, 0),
    _case(0xFFFF_FFFF, 0),
    _case(0xFFFF_FFFF, 1),
    _case(0xFFFF_FFFF, 2),
    _case(0xF, 1),
    _case(0x1F, 1),
    _case(0xFF, 0),
    _case(0xFF, 1),
    _case(0xFF, 2),
    _case(0x8000_0000, 0),
    _case(0x8000_0000, 1),
    _case(0x8000_0000, 29),
    _case(0x8000_0000, 30),
    _case(0x8000_0000, 31),
    _case(0x8000_0000, 32),
    _case(0x8000_0000, 33),
    _case(0x8000_0000, 63),
    _case(0x8000_0000, 255),
    _case(0x8000_0000, 256),
]


def make_args(i: int) -> List[ArgSpec]:
    if i < len(_SPECIALS):
        return _SPECIALS[i]
    return generate_args(SPECS, 4241, i - len(_SPECIALS))

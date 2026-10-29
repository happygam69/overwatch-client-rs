"""Structured inputs for fn/TexturePayloadGuid.

The function is `FUN_142e37520` (`u64 (u64 parent, uint index)`). The generic
random corpus draws a 64-bit parent, but the type-dependent base branch reads
the reversed 12-bit type field, so a random parent lands on "other type" almost
always. These specials pin the three types the client distinguishes (texture
0x004, 0x0F1, other), the kept bits 36..39, the cleared bits 40..47, and the
index field's four bits before the generic corpus.
"""

from __future__ import annotations

from typing import List

from emu_oracle import ArgSpec
from gen import UnitArg, generate_args

SPECS = [UnitArg("parent", "u64"), UnitArg("index", "u64")]


def _case(parent: int, index: int) -> List[ArgSpec]:
    return [
        ArgSpec(kind="u64", bits=parent & 0xFFFF_FFFF_FFFF_FFFF),
        ArgSpec(kind="u64", bits=index & 0xFFFF_FFFF_FFFF_FFFF),
    ]


_TEXTURE = 0x0C00_0000_0000_09C1
_F1 = 0x00F0_0000_0000_0001  # type_of == 0x0F1
_OTHER = 0x0110_0000_0000_0001

_SPECIALS: List[List[ArgSpec]] = [
    # The real payloads of 0x0C000000000009C1, and index-field edges.
    _case(_TEXTURE, 0),
    _case(_TEXTURE, 1),
    _case(_TEXTURE, 2),
    _case(_TEXTURE, 3),
    _case(_TEXTURE, 0xF),
    _case(_TEXTURE, 0x10),
    _case(_TEXTURE, 0xFFFF_FFFF),
    # Bits 36..39 survive; 40..47 are cleared by the 0xF0FFFFFFFF mask.
    _case(_TEXTURE | (0xF << 36), 2),
    _case(_TEXTURE | (0xFF << 40), 1),
    _case(_TEXTURE | (0xFF << 40) | (0xF << 36), 3),
    # The three base branches: texture -> 0, 0x0F1 -> 1, anything else -> 3.
    _case(_F1, 1),
    _case(_F1, 0xF),
    _case(_OTHER, 1),
    _case(0x02E0_0000_0000_0002, 1),  # a hero GUID (other type)
    _case(0, 0),
    _case(0xFFFF_FFFF_FFFF_FFFF, 0xFFFF_FFFF),
]


def make_args(i: int) -> List[ArgSpec]:
    if i < len(_SPECIALS):
        return _SPECIALS[i]
    return generate_args(SPECS, 4241, i - len(_SPECIALS))

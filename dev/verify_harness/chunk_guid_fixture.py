"""Deterministic table-slot inputs for the `fn/ChunkGuid` unit.

The client's `MapChunkGuid` (`0x141b32e20`) takes a map id and a table slot,
reads the kind qword at `+8` of the stride-`0x20` entry in the table at
`0x1439aa320`, and combines it with the map id. The shipped
`ow_tank::mapguid::chunk_guid` takes the kind directly, so the probe reads the
same table slot from the pointee below and calls the shipped function.

The table keeps the install's kinds for the map loader's 31 slots. The four
slots outside that set (`0x1D`, `0x1E`, `0x21`, `0x22`) are repurposed as the
formula's edge kinds: `0`, `0xFF`, `0xFFFF`, `0x1_0000` and `0xFFFF_FFFF`
(slot `0` already carries kind `0`). Every case is a pure function of its
index.
"""

from __future__ import annotations

import random
import struct
from typing import List

from emu_oracle import ArgSpec

TABLE_ENTRIES = 0x24
TABLE_STRIDE = 0x20
TABLE_LEN = TABLE_ENTRIES * TABLE_STRIDE

# The current image's 0x24-entry table at `0x1439aa320`: slot index at `+0`,
# kind at `+8` (see docs/formats/maps.md).
_REAL_KINDS = [
    0x00, 0x01, 0x03, 0x04, 0x06, 0x07, 0x09, 0x0A, 0x0B,
    0x0C, 0x0D, 0x0E, 0x0F, 0x15, 0x16, 0x17, 0x18, 0x19,
    0x1A, 0x1C, 0x1D, 0x1E, 0x20, 0x21, 0x22, 0x24, 0x25,
    0x26, 0x28, 0x11, 0x12, 0x13, 0x1B, 0x10, 0x23, 0x27,
]

_KINDS = list(_REAL_KINDS)
# These four slots are outside the map loader's fixed 31 (0x1F, 0x20, 0x23
# are the loader slots here; 0x1D, 0x1E, 0x21 and 0x22 are not requested).
_KINDS[0x1D] = 0xFF
_KINDS[0x1E] = 0xFFFF
_KINDS[0x21] = 0x1_0000
_KINDS[0x22] = 0xFFFF_FFFF


def kind_table() -> bytes:
    """The client's table bytes with the formula edges pinned."""
    out = bytearray(TABLE_LEN)
    for slot, kind in enumerate(_KINDS):
        struct.pack_into("<Q", out, slot * TABLE_STRIDE, slot)
        struct.pack_into("<Q", out, slot * TABLE_STRIDE + 8, kind)
    return bytes(out)


TABLE = kind_table()

# (map, slot) cases that pin the required map and kind edges. Slots 0x1D,
# 0x1E and 0x21 carry kinds 0xFF, 0xFFFF and 0x1_0000; slot 0x22 carries
# 0xFFFF_FFFF, which also proves the kind's high half is discarded.
_PINNED = [
    (0x0000_0000_0000_0000, 0x00),  # kind 0
    (0x0000_0000_FFFF_FFFF, 0x1D),  # kind 0xFF
    (0x0000_0001_0000_0000, 0x1E),  # kind 0xFFFF
    (0xFFFF_FFFF_FFFF_FFFF, 0x21),  # kind 0x1_0000
    (0x1234_5678_9ABC_DEF0, 0x22),  # kind 0xFFFF_FFFF
    (0x8000_0000_0000_0000, 0x1F),  # real loader slot, kind 0x13
    (0xDEAD_BEEF_CAFE_F00D, 0x20),  # real loader slot, kind 0x1B
    (0x0000_0000_FFFF_FFFF, 0x23),  # real loader slot, kind 0x27
]

_MAP_EDGES = [
    0x0000_0000_0000_0000,
    0x0000_0000_FFFF_FFFF,
    0x0000_0001_0000_0000,
    0xFFFF_FFFF_FFFF_FFFF,
    0x7FFF_FFFF_FFFF_FFFF,
    0x8000_0000_0000_0000,
    0xDEAD_BEEF_CAFE_F00D,
]


def make_args(i: int) -> List[ArgSpec]:
    """The i-th input: one map id, one table slot, and the fixture table."""
    if i < len(_PINNED):
        map_id, slot = _PINNED[i]
    else:
        # Walk every slot in order so the first 48 cases (the mutation
        # corpus) cover all 0x24 entries, then keep cycling.
        slot = (i - len(_PINNED)) % TABLE_ENTRIES
        if i % 4 == 0:
            map_id = _MAP_EDGES[(i // 4) % len(_MAP_EDGES)]
        else:
            map_id = random.Random(0x4348_4B47 ^ i).getrandbits(64)
    return [
        ArgSpec(kind="u64", bits=map_id),
        ArgSpec(kind="u64", bits=slot),
        ArgSpec(kind="ptr", mem=TABLE),
    ]

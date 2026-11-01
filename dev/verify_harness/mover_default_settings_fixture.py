"""Inputs for the `fn/DefaultMoverSettings` unit.

The client constructor `0x1405a87d0` writes a fixed set of fields and four
self-referential pointers. The fixture pins the object at a known arena
address so the probe can be given the same `base` and compute identical
pointer values. Every case starts from a different fill, so a byte the
constructor does not write must survive on both sides.
"""

from __future__ import annotations

import random
from typing import List

from emu_oracle import ARENA_BASE, ArgSpec

OUT_OFF = 0x4000
OUT_LEN = 0x700
BASE = ARENA_BASE + OUT_OFF

_PINNED = [
    0x00,
    0xFF,
    0xAA,
    0x55,
]


def _fill(i: int) -> bytes:
    if i < len(_PINNED):
        return bytes([_PINNED[i]]) * OUT_LEN
    rng = random.Random(0x5345_5454 ^ i)
    return bytes(rng.getrandbits(8) for _ in range(OUT_LEN))


def make_args(i: int) -> List[ArgSpec]:
    return [
        ArgSpec(kind="ptr", mem=_fill(i), arena_offset=OUT_OFF),
        ArgSpec(kind="u64", bits=BASE),
        ArgSpec(kind="u64", bits=0),
    ]

"""Inputs for the `collision_filter` unit (`0x142cbb350`).

Each case is two eight-byte filter word blocks: the query descriptor's field
(first argument) and the shape entry's `+0x98` field (second argument). The
layout is `+0`/`+2` two 16-bit masks, `+4` a signed id, `+7` the mode byte.
"""

from __future__ import annotations

import random
import struct
from typing import List

from emu_oracle import ArgSpec

A_OFF = 0x4000
B_OFF = 0x4100

# Mask pairs worth pinning: empty, full, single bits that overlap, single bits
# that do not, and one that only meets in the second pair.
MASKS = [0x0000, 0xFFFF, 0x0001, 0x0002, 0x00FF, 0xFF00, 0x8000, 0x0101]

# Ids worth pinning: zero, the sign pair, unequal magnitudes, the extremes.
IDS = [0, 1, -1, 2, -2, 3, -3, 7, -7, 32767, -32768, 100, -100]


def _cases():
    # The mode path must be told apart from the full path: with the mode byte
    # set, an id pair the full rule rejects still passes on the masks alone.
    # These sit first so the mutation corpus always covers them.
    cases = [
        (1, 1, 1, 1, -1, -1, 1),
        (1, 1, 1, 1, 3, -3, 1),
        (1, 1, 1, 1, 0, 0, 1),
    ]
    # The mode byte alternates so the mutation corpus (the first 48 cases)
    # always sees both modes, and mask-zero pairs land in that prefix too.
    for i in range(len(MASKS) * len(MASKS) * 2):
        a_mask = MASKS[i % len(MASKS)]
        b_mask = MASKS[(i // len(MASKS)) % len(MASKS)]
        cases.append((a_mask, b_mask, a_mask, b_mask, 1, 1, i % 2))
    # The signed-id rule, with both masks always intersecting.
    for x in IDS:
        for y in IDS:
            cases.append((1, 1, 1, 1, x, y, 0))
    # The first mask intersects but the second does not, and the reverse.
    cases.append((1, 1, 1, 0, 5, 5, 0))
    cases.append((1, 1, 0, 1, 5, 5, 0))
    cases.append((1, 0, 1, 1, 5, 5, 0))
    cases.append((0, 1, 1, 0, 5, 5, 0))
    return cases


CASES = _cases()


def _words(case) -> bytes:
    a_mask, b_mask, a2, b2, x, y, mode = case
    a = bytearray(8)
    struct.pack_into("<HHh", a, 0, a2, a_mask, x)
    a[7] = mode
    b = bytearray(8)
    struct.pack_into("<HHh", b, 0, b_mask, b2, y)
    return bytes(a) + bytes(b)


def make_args(i: int) -> List[ArgSpec]:
    case = CASES[i % len(CASES)]
    raw = _words(case)
    return [
        ArgSpec(kind="ptr", mem=raw[:8], arena_offset=A_OFF),
        ArgSpec(kind="ptr", mem=raw[8:], arena_offset=B_OFF),
    ]

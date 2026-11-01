"""Inputs for the `fn/MoverGate` unit.

The body is pinned in the arena. Its `+0x690` array points at the 0x30-byte
records at arena `+0x1000`; each record's `+0x28` object holds a fake
vtable at arena `+0x1400`, whose `+0x78` slot is one of two image stubs:
`0x14053a070` (`mov al,1; ret`) or `0x140548d00` (`xor eax,eax; ret`).
The candidate receives the same per-record results as a byte slice.
"""

from __future__ import annotations

import random
import struct
from typing import List

from emu_oracle import ARENA_BASE, ArgSpec

BODY_LEN = 0x1c00
REC_OFF = 0x1000
OBJ_OFF = 0x1200
VT_OFF = 0x1400
TRUE_STUB = 0x00007ff7c525a070
FALSE_STUB = 0x00007ff7c5268d00

CASES = [
    (0, [], [], 0, 1),
    (0, [], [], 1, 0),
    (0, [], [], 1, 1),
    (0, [(0, 1)], [1], 1, 1),
    (0, [(0, 1)], [0], 1, 1),
    (0, [(0, 1), (0, 1)], [1, 0], 1, 1),
    (0, [(0, 1), (0, 1)], [1, 1], 1, 1),
    (5, [(5, 6)], [1], 1, 1),
    (6, [(5, 6)], [0], 1, 1),
    (3, [(0, 4), (2, 5), (3, 4)], [1, 0, 1], 1, 1),
    (3, [(0, 4), (2, 5), (3, 4)], [1, 1, 1], 1, 1),
    (2, [(10, 20), (0, 1)], [0, 0], 1, 1),
    (7, [(0, 8), (1, 2)], [0, 1], 1, 1),
    (1, [(0, 2)], [1], 1, 1),
    (0, [(0, 1), (5, 6), (0, 5)], [1, 1, 1], 1, 1),
    (3, [(0, 4), (5, 6)], [1, 1], 1, 1),
    (0, [(0, 1), (5, 6)], [1, 0], 1, 1),
]


def build_body(case, i: int) -> bytes:
    rid, windows, results, flag_a, flag_b = case
    rng = random.Random(0x4741_5445 ^ i)
    body = bytearray(rng.getrandbits(8) for _ in range(BODY_LEN))
    body[0x1A78] = flag_a
    body[0x658] = flag_b
    body[0xE4:0xE8] = rid.to_bytes(4, "little")
    body[0x698:0x69C] = len(windows).to_bytes(4, "little")
    body[0x690:0x698] = (ARENA_BASE + REC_OFF).to_bytes(8, "little")
    for k, (lo, hi) in enumerate(windows):
        rec = REC_OFF + k * 0x30
        struct.pack_into("<i", body, rec + 0x1C, lo)
        struct.pack_into("<i", body, rec + 0x20, hi)
        struct.pack_into("<Q", body, rec + 0x28, ARENA_BASE + OBJ_OFF + k * 0x40)
        obj = OBJ_OFF + k * 0x40
        struct.pack_into("<Q", body, obj, ARENA_BASE + VT_OFF + k * 0x80)
        stub = TRUE_STUB if results[k] else FALSE_STUB
        struct.pack_into("<Q", body, VT_OFF + k * 0x80 + 0x78, stub)
    return bytes(body)


def make_args(i: int) -> List[ArgSpec]:
    case = CASES[i % len(CASES)]
    return [
        ArgSpec(kind="ptr", mem=build_body(case, i), arena_offset=0),
        ArgSpec(kind="u64", bits=ARENA_BASE),
        ArgSpec(kind="ptr", mem=bytes(case[2]), arena_offset=0x7000),
    ]

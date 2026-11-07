"""Inputs for the `mover_on_ground` unit (`0x140ca91e0`).

The body is pinned in the arena; the state channel is its whole 0x1c00 bytes, so
every field the client may stamp (`+0x10d0` flags, `+0x10e0` frame stamp,
`+0x1a76` latch) is compared. The latch starts at both values so a spurious or
missing write is a divergence.

The frame id at `+0xe4` is distinctive per case so a stamp is recognisable.
"""

from __future__ import annotations

import random
from typing import List

from emu_oracle import ArgSpec

BODY_LEN = 0x1C00

# (flags, aux, latch_before). `aux` is the take-off's `dl` only.
CASES = [
        (0x00000000, 0, 0),
    (0x00000000, 0, 1),
    (0x00000001, 0, 0),
    (0x00000001, 0, 1),
    (0x00000004, 0, 0),
    (0x00000004, 0, 1),
    (0x00000008, 0, 0),
    (0x00000008, 0, 1),
    (0x0000000c, 0, 0),
    (0x0000000c, 0, 1),
    (0x00080000, 0, 0),
    (0x00080000, 0, 1),
    (0x00800000, 0, 0),
    (0x00800000, 0, 1),
    (0x00800004, 0, 0),
    (0x00800004, 0, 1),
    (0x0080000c, 0, 0),
    (0x0080000c, 0, 1),
    (0x00800000, 0, 0),
    (0x00800000, 0, 1),
    (0x01000000, 0, 0),
    (0x01000000, 0, 1),
    (0xdeadbeef, 0, 0),
    (0xdeadbeef, 0, 1),
    (0xffffffff, 0, 0),
    (0xffffffff, 0, 1),
    (0x00000400, 0, 0),
    (0x00000400, 0, 1),
    (0x80000000, 0, 0),
    (0x80000000, 0, 1),
]


def build_body(case, i: int) -> bytes:
    flags, _aux, latch = case
    rng = random.Random(1196576597 ^ i)
    body = bytearray(rng.getrandbits(8) for _ in range(BODY_LEN))
    body[0xE4:0xE8] = (0x1234_5600 + i).to_bytes(4, "little")
    body[0x10D0:0x10D4] = (flags & 0xFFFF_FFFF).to_bytes(4, "little")
    body[0x10E0:0x10E4] = (0xAAAA_AAAA).to_bytes(4, "little")
    body[0x1A76] = latch
    return bytes(body)


def make_args(i: int) -> List[ArgSpec]:
    case = CASES[i % len(CASES)]
    return [
        ArgSpec(kind="ptr", mem=build_body(case, i), arena_offset=0),
    ]

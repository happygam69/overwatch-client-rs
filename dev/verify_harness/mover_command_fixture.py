"""Deterministic body inputs for the `fn/BuildMoverPhysicsCommand` unit.

The client builder `0x140cb74a0` reads:

    gate          body + 0x1a78, +0x658, +0x698 (record count)
    first word    body + 0x1a7b, +0x1a75, +0x1a60
    action block  body + 0x1a79, +0x652, +0x1a7a, +0x1a7f, +0x1a60
    tail          body + 0x23c

The fixture pins the gate's record count to zero, so the gate cannot enter
its indirect record loop and is exactly `+0x1a78 != 0 && +0x658 != 0`.

The two obfuscated byte tests are pinned at their exact boundaries:

    (state_1a79 + 0x59) ^ 0x71  == 0xec  -> state_1a79 == 0x44
    rotr(byte_1a7a, 1) + 0x4d rotated left == 0xc5 -> byte_1a7a == 0x2b
"""

from __future__ import annotations

import random
import struct
from typing import List

from emu_oracle import ArgSpec

BODY_LEN = 0x1A80
OUT_LEN = 8

_BODY_ACTION = 0x23C
_BODY_GATE_RECORDS = 0x698
_BODY_STATE = 0x1A60
_BODY_1A75 = 0x1A75
_BODY_1A78 = 0x1A78
_BODY_1A79 = 0x1A79
_BODY_1A7A = 0x1A7A
_BODY_1A7B = 0x1A7B
_BODY_1A7F = 0x1A7F
_BODY_652 = 0x652
_BODY_658 = 0x658


def _case(i: int):
    pinned = [
        # gate_1a78, gate_658, 1a7b, 1a75, state, 1a79, 652, 1a7a, 1a7f, action
        (1, 1, 0, 0, 0, 0x44, 1, 0x00, 0, 0x1234),
        (0, 1, 0, 0, 0, 0x44, 1, 0x00, 0, 0x1234),
        (1, 0, 0, 0, 0, 0x44, 1, 0x00, 0, 0x1234),
        (0, 0, 0, 0, 0, 0x44, 1, 0x00, 0, 0x1234),
        (1, 1, 1, 0, 0, 0x44, 1, 0x00, 0, 0x1234),
        (1, 1, 0, 1, 0, 0x44, 1, 0x00, 0, 0x1234),
        (1, 1, 0, 0, 1, 0x44, 1, 0x00, 0, 0x1234),
        (1, 1, 0, 0, 2, 0x44, 1, 0x00, 0, 0x1234),
        (1, 1, 0, 0, 3, 0x44, 1, 0x00, 0, 0x1234),
        (1, 1, 0, 0, 4, 0x44, 1, 0x00, 0, 0x1234),
        (1, 1, 0, 0, -1, 0x44, 1, 0x00, 0, 0x1234),
        # The 0x44 boundary and its neighbours for the first byte test.
        (1, 1, 0, 0, 0, 0x43, 1, 0x00, 0, 0x1234),
        (1, 1, 0, 0, 0, 0x44, 1, 0x00, 0, 0x1234),
        (1, 1, 0, 0, 0, 0x45, 1, 0x00, 0, 0x1234),
        (1, 1, 0, 0, 0, 0x46, 1, 0x00, 0, 0x1234),
        (1, 1, 0, 0, 0, 0x00, 1, 0x00, 0, 0x1234),
        (1, 1, 0, 0, 0, 0xFF, 1, 0x00, 0, 0x1234),
        # The 0x2b boundary for the rotated-byte test.
        (1, 1, 0, 0, 0, 0x44, 1, 0x2A, 0, 0x1234),
        (1, 1, 0, 0, 0, 0x44, 1, 0x2B, 0, 0x1234),
        (1, 1, 0, 0, 0, 0x44, 1, 0x2C, 0, 0x1234),
        (1, 1, 0, 0, 0, 0x44, 1, 0x00, 0, 0x1234),
        (1, 1, 0, 0, 0, 0x44, 1, 0xFF, 0, 0x1234),
        # The 0x652 gate and the 0x1a7f clear path. The 0x45 state byte
        # fails the first test, so these reach the rotate/mask block.
        (1, 1, 0, 0, 0, 0x45, 1, 0x2B, 0, 0x1234),
        (1, 1, 0, 0, 0, 0x45, 1, 0x2A, 0, 0x1234),
        (1, 1, 0, 0, 0, 0x44, 0, 0x2B, 0, 0x1234),
        (1, 1, 0, 0, 0, 0x44, 1, 0x2B, 1, 0x1234),
        (1, 1, 0, 0, 1, 0x44, 0, 0x2B, 0, 0x1234),
        (1, 1, 0, 0, 1, 0x45, 1, 0x2B, 0, 0x1234),
        (1, 1, 0, 0, 4, 0x44, 0, 0x2B, 0, 0x1234),
        (1, 1, 1, 1, 4, 0x44, 1, 0x2B, 0, 0x1234),
        # The action word edges.
        (1, 1, 0, 0, 0, 0x44, 1, 0x2B, 0, 0x0000),
        (1, 1, 0, 0, 0, 0x44, 1, 0x2B, 0, 0xFFFF),
        (0, 0, 0, 0, 0, 0x44, 1, 0x2B, 0, 0x0000),
        (0, 0, 0, 0, 0, 0x44, 1, 0x2B, 0, 0xFFFF),
    ]
    if i < len(pinned):
        return pinned[i]
    rng = random.Random(0x434D_4453 ^ i)
    return (
        1 if rng.getrandbits(1) else 0,
        1 if rng.getrandbits(1) else 0,
        1 if rng.getrandbits(1) else 0,
        1 if rng.getrandbits(1) else 0,
        rng.choice([0, 1, 2, 3, 4, -1, 100]),
        rng.choice([0x00, 0x43, 0x44, 0x45, 0x46, 0xFF, rng.getrandbits(8)]),
        1 if rng.getrandbits(1) else 0,
        rng.choice([0x00, 0x2A, 0x2B, 0x2C, 0xFF, rng.getrandbits(8)]),
        1 if rng.getrandbits(1) else 0,
        rng.getrandbits(16),
    )


def make_args(i: int) -> List[ArgSpec]:
    gate_1a78, gate_658, b1a7b, b1a75, state, b1a79, b652, b1a7a, b1a7f, action = _case(i)
    body = bytearray(BODY_LEN)
    body[_BODY_1A78] = gate_1a78
    body[_BODY_658] = gate_658
    struct.pack_into("<i", body, _BODY_GATE_RECORDS, 0)
    body[_BODY_1A7B] = b1a7b
    body[_BODY_1A75] = b1a75
    struct.pack_into("<i", body, _BODY_STATE, state)
    body[_BODY_1A79] = b1a79
    body[_BODY_652] = b652
    body[_BODY_1A7A] = b1a7a
    body[_BODY_1A7F] = b1a7f
    struct.pack_into("<H", body, _BODY_ACTION, action)
    return [
        ArgSpec(kind="ptr", mem=bytes(body)),
        ArgSpec(kind="ptr", mem=b"\xcc" * OUT_LEN),
    ]

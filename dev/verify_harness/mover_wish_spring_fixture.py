"""Inputs for the `fn/MoverWishSpring` unit.

The client `MoverWishSpring` (`0x140cc0dc0`) copies the input object's
`+0x60..0x6c` into body `+0x1140` when `input+0x98 != 0` and
`(body+0x10d0 & 0x80000c) == 0`, then runs the `+0x10fc` / `+0x1100` spring.

The unit pins the **matching state branch**: body `+0x1520/+0x1524` equal
body `+0x78/+0x7c`, and the state id is `0xffffffff`. With that sentinel the
client's `FUN_140ca5d00` state lookup returns false and leaves the two local
vectors unchanged, so the probe can supply the identity transform.

`body+8` encodes the service object `T = 0x0000000100000000`; the unit's
`state` block at that base holds the encoded descriptor at `T+0x28`. The
oracle needs the descriptor only because `FUN_140ca5d00` reads it before the
sentinel check; its value does not affect the path.
"""

from __future__ import annotations

import random
import struct
from typing import List

from emu_oracle import ArgSpec

BODY_LEN = 0x1C00
INPUT_LEN = 0xA0
BODY_SERVICE = 0xCA3E3A1D3D45D5CA
STATE_BASE = 0x0000000100000000
STATE_HEX = "00" * 0x28 + "8f73c8d50779fc3a"

_BODY_WISH = 0x1140
_BODY_DIRECTION = 0xC0
_BODY_TURN = 0x90
_BODY_80 = 0x80
_BODY_5C0 = 0x5C0
_BODY_CLAMP = 0x688
_BODY_SPRING = 0x10FC
_BODY_SPRING2 = 0x1100
_BODY_VEL = 0x110C
_BODY_PAIR_A = 0x1104
_BODY_PAIR_B = 0x1108
_INPUT_WISH = 0x60
_INPUT_GATE = 0x98

# Movement fields are bounded in the client; keep the random corpus in that
# range so FP overflow/NaN payload behaviour does not masquerade as a
# divergence. The pinned cases still cover zero, signed zero and tiny values.
_FINITE = [
    0x0000_0000, 0x8000_0000, 0x3F80_0000, 0xBF80_0000,
    0x4000_0000, 0xC000_0000, 0x3F00_0000, 0xBF00_0000,
    0x4040_0000, 0xC040_0000, 0x3E80_0000, 0xBE80_0000,
    0x3DCC_CCCD, 0xBDCC_CCCD, 0x3A83_126F, 0xBA83_126F,
]

_CLAMP_PAIRS = [
    (0xBF80_0000, 0x3F80_0000),
    (0x0000_0000, 0x3F80_0000),
    (0xC000_0000, 0x4000_0000),
    (0xBF00_0000, 0x3F00_0000),
    (0x0000_0000, 0x0000_0000),
    (0xBF80_0000, 0xBF00_0000),
]


def _f32(bits: int) -> bytes:
    return struct.pack("<I", bits)


_PINNED = [
    # (gate, dt bits, body wish, input wish, direction, turn, f80, f5c0,
    #  clamp, spring, spring2, vel)
    (1, 0x3C83_126F, (0x3F80_0000, 0x3F80_0000, 0x4040_0000, 0),
     (0x3F80_0000, 0x3F80_0000, 0x4040_0000, 0),
     (0x3F00_0000, 0xBF80_0000, 0x4000_0000, 0),
     (0x3DCC_CCCD, 0x3E4C_CCCD, 0x3E99_999A, 0),
     (0x3E80_0000, 0x3F00_0000, 0x3F40_0000, 0),
     (0x3E4C_CCCD, 0x3DCC_CCCD),
     (0xBF80_0000, 0x3F80_0000),
     (0x3F00_0000, 0x3E80_0000, 0x3F00_0000)),
    (0, 0x3C83_126F, (0x3F80_0000, 0x3F80_0000, 0x4040_0000, 0),
     (0x4000_0000, 0x4040_0000, 0x4080_0000, 0),
     (0x3F00_0000, 0xBF80_0000, 0x4000_0000, 0),
     (0x3DCC_CCCD, 0x3E4C_CCCD, 0x3E99_999A, 0),
     (0x3E80_0000, 0x3F00_0000, 0x3F40_0000, 0),
     (0x3E4C_CCCD, 0x3DCC_CCCD),
     (0xBF80_0000, 0x3F80_0000),
     (0x3F00_0000, 0x3E80_0000, 0x3F00_0000)),
    (1, 0x0000_0000, (0x3F80_0000, 0x3F80_0000, 0x4040_0000, 0),
     (0x3F80_0000, 0x3F80_0000, 0x4040_0000, 0),
     (0x3F00_0000, 0xBF80_0000, 0x4000_0000, 0),
     (0x3DCC_CCCD, 0x3E4C_CCCD, 0x3E99_999A, 0),
     (0x3E80_0000, 0x3F00_0000, 0x3F40_0000, 0),
     (0x3E4C_CCCD, 0x3DCC_CCCD),
     (0xBF80_0000, 0x3F80_0000),
     (0x3F00_0000, 0x3E80_0000, 0x3F00_0000)),
    (1, 0x3586_37BD, (0x3F80_0000, 0x3F80_0000, 0x4040_0000, 0),
     (0x3F80_0000, 0x3F80_0000, 0x4040_0000, 0),
     (0x3F00_0000, 0xBF80_0000, 0x4000_0000, 0),
     (0x3DCC_CCCD, 0x3E4C_CCCD, 0x3E99_999A, 0),
     (0x3E80_0000, 0x3F00_0000, 0x3F40_0000, 0),
     (0x3E4C_CCCD, 0x3DCC_CCCD),
     (0xBF80_0000, 0x3F80_0000),
     (0x3F00_0000, 0x3E80_0000, 0x3F00_0000)),
    # Residual zero: direction is zero.
    (1, 0x3C83_126F, (0x3F80_0000, 0x3F80_0000, 0x4040_0000, 0),
     (0x3F80_0000, 0x3F80_0000, 0x4040_0000, 0),
     (0, 0, 0, 0),
     (0, 0, 0, 0),
     (0, 0, 0, 0),
     (0, 0),
     (0xBF80_0000, 0x3F80_0000),
     (0x3F00_0000, 0x3E80_0000, 0x3F00_0000)),
    # Tiny span: pair_update's early zero path.
    (1, 0x3C83_126F, (0x3F80_0000, 0x3F80_0000, 0x4040_0000, 0),
     (0x3F80_0000, 0x3F80_0000, 0x4040_0000, 0),
     (0x3F00_0000, 0xBF80_0000, 0x4000_0000, 0),
     (0x3DCC_CCCD, 0x3E4C_CCCD, 0x3E99_999A, 0),
     (0x3E80_0000, 0x3F00_0000, 0x3F40_0000, 0),
     (0x3E4C_CCCD, 0x3DCC_CCCD),
     (0, 0),
     (0x3F00_0000, 0x3E80_0000, 0x3F00_0000)),
]


def _bits(value: float) -> int:
    return struct.unpack("<I", struct.pack("<f", value))[0]


def _case(i: int):
    if i < len(_PINNED):
        return _PINNED[i]
    rng = random.Random(0x5753_5052 ^ i)
    gate = 1 if rng.getrandbits(1) else 0
    dt = rng.choice(_FINITE)
    body_wish = tuple(rng.choice(_FINITE) for _ in range(3)) + (0,)
    input_wish = tuple(rng.choice(_FINITE) for _ in range(3)) + (0,)
    direction = tuple(rng.choice(_FINITE) for _ in range(3)) + (0,)
    turn = tuple(rng.choice(_FINITE) for _ in range(3)) + (0,)
    f80 = tuple(rng.choice(_FINITE) for _ in range(3)) + (0,)
    f5c0 = (rng.choice(_FINITE), rng.choice(_FINITE))
    clamp = rng.choice(_CLAMP_PAIRS)
    spring = (rng.choice(_FINITE), rng.choice(_FINITE), rng.choice(_FINITE))
    return (gate, dt, body_wish, input_wish, direction, turn, f80, f5c0, clamp, spring)


def make_args(i: int) -> List[ArgSpec]:
    gate, dt, body_wish, input_wish, direction, turn, f80, f5c0, clamp, spring = _case(i)

    body = bytearray(BODY_LEN)
    struct.pack_into("<Q", body, 8, BODY_SERVICE)
    struct.pack_into("<I", body, 0x10D0, 0)
    struct.pack_into("<I", body, 0x10EC, 0x3F80_0000)  # timer 1.0
    struct.pack_into("<I", body, 0x5A8, 0x4040_0000)   # scale clear 3.0
    struct.pack_into("<I", body, 0x5B4, 0x40E0_0000)   # scale set 7.0
    struct.pack_into("<I", body, 0x1520, 0xFFFFFFFF)
    struct.pack_into("<I", body, 0x1524, 0xFFFFFFFF)
    struct.pack_into("<I", body, 0x78, 0xFFFFFFFF)
    struct.pack_into("<I", body, 0x7C, 0xFFFFFFFF)
    body[0x1A77] = 0
    struct.pack_into("<4I", body, _BODY_WISH, *body_wish)
    struct.pack_into("<4I", body, _BODY_DIRECTION, *direction)
    struct.pack_into("<4I", body, _BODY_TURN, *turn)
    struct.pack_into("<4I", body, _BODY_80, *f80)
    struct.pack_into("<2I", body, _BODY_5C0, *f5c0)
    struct.pack_into("<2I", body, _BODY_CLAMP, *clamp)
    struct.pack_into("<3I", body, _BODY_SPRING, *spring)
    struct.pack_into("<I", body, _BODY_PAIR_A, 0)
    struct.pack_into("<I", body, _BODY_PAIR_B, 0)

    # The state compare has two branches: a mismatch on the first id and a
    # mismatch on the second. Every eighth case takes one of them. The
    # sentinel state id keeps FUN_140ca5d00/FUN_140ca7700 on the identity
    # path, and body+0x1ad8 = 0 makes FUN_142cbcec0 return the image's zero
    # vector.
    if i % 8 == 3:
        struct.pack_into("<2I", body, 0x78, 0xFFFFFFFF, 0)
        struct.pack_into("<Q", body, 0x1AD8, 0)
    elif i % 8 == 7:
        struct.pack_into("<2I", body, 0x78, 0, 0)
        struct.pack_into("<Q", body, 0x1AD8, 0)

    # One case flips the flag mask that gates the wish copy.
    if i == 7:
        struct.pack_into("<I", body, 0x10D0, 0x80000C)

    # One pinned zero-residual case: zero ramp and zero direction/turn/scalars
    # so the spring's |residual| <= 1/512 branch runs.
    if i == 6:
        struct.pack_into("<I", body, 0x5A8, 0)
        struct.pack_into("<I", body, 0x5B4, 0)
        struct.pack_into("<4I", body, _BODY_DIRECTION, 0, 0, 0, 0)
        struct.pack_into("<4I", body, _BODY_TURN, 0, 0, 0, 0)
        struct.pack_into("<4I", body, _BODY_80, 0, 0, 0, 0)

    inp = bytearray(INPUT_LEN)
    inp[_INPUT_GATE] = gate
    struct.pack_into("<4I", inp, _INPUT_WISH, *input_wish)

    return [
        ArgSpec(kind="ptr", mem=bytes(body)),
        ArgSpec(kind="ptr", mem=bytes(inp)),
        ArgSpec(kind="f32", value=struct.unpack("<f", struct.pack("<I", dt))[0], bits=dt),
    ]

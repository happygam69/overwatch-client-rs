"""Deterministic inputs for the `fn/MoverRampVector` unit.

The client leaf `MoverRampVector` (`0x140ca75b0`) reads:

    timer        body + 0x10ec
    scale_clear  body + 0x5a8   (flags bit 1 clear)
    scale_set    body + 0x5b4   (flags bit 1 set)
    direction    body + 0xc0 .. 0xd0
    flag byte    flags + 0x00
    offset       flags + 0x2c

and writes four f32 lanes. `flags` is the caller's `body + 0x10d0` block; the
unit passes it as its own 0x30-byte pointee so both channels are controlled.

The cases pin the timer clamp (including NaN, +inf and -inf), the CMOVZ scale
selection, signed zero and subnormals. The movement inputs themselves are
finite: a NaN payload in the scale, offset or direction is not produced by the
client's input path, and which NaN the CPU propagates is not part of the
client's bytes. The timer's NaN is reachable in the unit because MINSS turns it
into 1.0 before the multiply, so it is pinned.
"""

from __future__ import annotations

import random
import struct
from typing import List

from emu_oracle import ArgSpec

BODY_LEN = 0x1160
FLAGS_LEN = 0x30
OUT_LEN = 16

_BODY_TIMER = 0x10EC
_BODY_SCALE_CLEAR = 0x5A8
_BODY_SCALE_SET = 0x5B4
_BODY_DIRECTION = 0xC0
_FLAGS_OFFSET = 0x2C

_SMALLEST_SUBNORMAL = 0x0000_0001
_LARGEST_FINITE = 0x7F7F_FFFF


def _f32(bits: int) -> bytes:
    return struct.pack("<I", bits)


_PINNED = [
    # (timer bits, flags bit set, scale_clear bits, scale_set bits,
    #  offset bits, direction bits)
    (0x4020_0000, False, 0x4040_0000, 0x40E0_0000, 0x3E80_0000,
     (0x3F00_0000, 0xBF80_0000, 0x4000_0000, 0x8000_0000)),
    (0x4020_0000, True, 0x4040_0000, 0x40E0_0000, 0x3E80_0000,
     (0x3F00_0000, 0xBF80_0000, 0x4000_0000, 0x8000_0000)),
    (0x0000_0000, False, 0x4040_0000, 0x40E0_0000, 0x3E80_0000,
     (0x3F80_0000, 0x3F80_0000, 0x3F80_0000, 0x3F80_0000)),
    (0x3F80_0000, False, 0x4040_0000, 0x40E0_0000, 0x0000_0000,
     (0x3F80_0000, 0x3F80_0000, 0x3F80_0000, 0x3F80_0000)),
    (0xBF80_0000, False, 0x4040_0000, 0x40E0_0000, 0x0000_0000,
     (0x3F80_0000, 0x3F80_0000, 0x3F80_0000, 0x3F80_0000)),
    (0x7FC0_0000, False, 0x4040_0000, 0x40E0_0000, 0x0000_0000,
     (0x3F80_0000, 0x3F80_0000, 0x3F80_0000, 0x3F80_0000)),
    (0x7F80_0000, False, 0x4040_0000, 0x40E0_0000, 0x0000_0000,
     (0x3F80_0000, 0x3F80_0000, 0x3F80_0000, 0x3F80_0000)),
    (0xFF80_0000, False, 0x4040_0000, 0x40E0_0000, 0x0000_0000,
     (0x3F00_0000, 0x4000_0000, 0xC000_0000, 0x3E80_0000)),
    (_SMALLEST_SUBNORMAL, False, 0x4040_0000, 0x40E0_0000, 0x0000_0000,
     (0x3F80_0000, 0x3F80_0000, 0x3F80_0000, 0x3F80_0000)),
    (0x3F80_0000, False, 0x0000_0000, 0x40E0_0000, 0x0000_0000,
     (0x3F80_0000, 0x3F80_0000, 0x3F80_0000, 0x3F80_0000)),
    (0x3F80_0000, True, 0x4040_0000, 0x0000_0000, 0x0000_0000,
     (0x3F80_0000, 0x3F80_0000, 0x3F80_0000, 0x3F80_0000)),
    (0x3F80_0000, False, _SMALLEST_SUBNORMAL, 0x40E0_0000, 0x0000_0000,
     (0x3F80_0000, 0x3F80_0000, 0x3F80_0000, 0x3F80_0000)),
    (0x3F80_0000, True, 0x4040_0000, _LARGEST_FINITE, 0x0000_0000,
     (0x3F80_0000, 0x3F80_0000, 0x3F80_0000, 0x3F80_0000)),
    (0x3F80_0000, False, 0x4040_0000, 0x40E0_0000, 0x0000_0000,
     (0x3F80_0000, 0x3F80_0000, 0x3F80_0000, 0x3F80_0000)),
    (0x3F80_0000, False, 0x4040_0000, 0x40E0_0000, 0xBE80_0000,
     (0x3F80_0000, 0x3F80_0000, 0x3F80_0000, 0x3F80_0000)),
    (0x3F80_0000, False, 0x4040_0000, 0x40E0_0000, _SMALLEST_SUBNORMAL,
     (0x3F80_0000, 0x3F80_0000, 0x3F80_0000, 0x3F80_0000)),
    (0x3F80_0000, False, 0x4040_0000, 0x40E0_0000, 0x0000_0000,
     (0x0000_0000, 0x8000_0000, 0x3F80_0000, 0xBF80_0000)),
    (0x3F80_0000, False, 0x4040_0000, 0x40E0_0000, 0x0000_0000,
     (_LARGEST_FINITE, 0xFF7F_FFFF, _SMALLEST_SUBNORMAL, 0x8000_0001)),
    (0x3F80_0000, False, 0x4040_0000, 0x40E0_0000, 0x0000_0000,
     (0x3F00_0000, 0xBF00_0000, 0x4000_0000, 0xC000_0000)),
    (0x3F80_0000, False, 0xBF80_0000, 0x3F80_0000, 0x0000_0000,
     (0x3F80_0000, 0x3F80_0000, 0x3F80_0000, 0x3F80_0000)),
    (0x0DA2_4260, False, 0x4040_0000, 0x40E0_0000, 0x0000_0000,
     (0x3F80_0000, 0x3F80_0000, 0x3F80_0000, 0x3F80_0000)),
    (0x3F80_0000, False, 0x4040_0000, 0x40E0_0000, _LARGEST_FINITE,
     (0x3F80_0000, 0x3F80_0000, 0x3F80_0000, 0x3F80_0000)),
    (0x3F80_0000, True, 0x3F80_0000, 0x3F80_0000, 0x0000_0000,
     (0x3F80_0000, 0x3F80_0000, 0x3F80_0000, 0x3F80_0000)),
    (0x3F80_0000, False, 0x4040_0000, 0x40E0_0000, 0x0000_0000,
     (0x3E80_0000, 0xBE80_0000, 0x4000_0000, 0xC000_0000)),
]

# Finite bit patterns only: zeros, subnormals, normals and the largest finite.
_POOL = FINITE_POOL = [
    0x0000_0000, 0x8000_0000, 0x3F80_0000, 0xBF80_0000,
    0x4000_0000, 0xC000_0000, 0x3F00_0000, 0xBF00_0000,
    0x4040_0000, 0xC040_0000, 0x40E0_0000, 0xC0E0_0000,
    0x0000_0001, 0x8000_0001, 0x0080_0000, 0x8080_0000,
    0x00FF_FFFF, 0x80FF_FFFF, 0x7F7F_FFFF, 0xFF7F_FFFF,
    0x3E80_0000, 0xBE80_0000, 0x4020_0000, 0xC020_0000,
]


def make_args(i: int) -> List[ArgSpec]:
    """The i-th controlled body/out/flags triple."""
    if i < len(_PINNED):
        timer, bit, clear, set_, offset, direction = _PINNED[i]
    else:
        rng = random.Random(0x5241_4D50 ^ i)
        timer = rng.choice(_POOL)
        bit = bool(rng.getrandbits(1))
        clear = rng.choice(_POOL)
        set_ = rng.choice(_POOL)
        offset = rng.choice(_POOL)
        direction = tuple(rng.choice(_POOL) for _ in range(4))

    body = bytearray(BODY_LEN)
    body[_BODY_TIMER:_BODY_TIMER + 4] = _f32(timer)
    body[_BODY_SCALE_CLEAR:_BODY_SCALE_CLEAR + 4] = _f32(clear)
    body[_BODY_SCALE_SET:_BODY_SCALE_SET + 4] = _f32(set_)
    for lane, bits in enumerate(direction):
        at = _BODY_DIRECTION + 4 * lane
        body[at:at + 4] = _f32(bits)

    flags = bytearray(FLAGS_LEN)
    flags[0] = 0x02 if bit else 0x00
    flags[_FLAGS_OFFSET:_FLAGS_OFFSET + 4] = _f32(offset)

    return [
        ArgSpec(kind="ptr", mem=bytes(body)),
        ArgSpec(kind="ptr", mem=b"\0" * OUT_LEN),
        ArgSpec(kind="ptr", mem=bytes(flags)),
    ]

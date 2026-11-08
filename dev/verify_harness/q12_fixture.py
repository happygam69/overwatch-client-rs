"""Inputs for the Q12 scaler unit (`0x141c6c830`).

The client's function takes `(dst, src, count)`: it reads `count` dwords from
`src` as `f32` and writes `count` dwords to `dst` as Q12 integers
(`value * 4096`, truncated, saturating). Every branch of the integer path is
reachable from a small declared set:

- `exponent >= 138` with shift `0..7` (large magnitudes, left shift);
- `exponent >= 146` (shift >= 8, saturate at `i32::MAX`);
- `exponent < 138` with shift `1..23` (the normal range);
- `exponent < 115` (shift > 23, underflow to 0: subnormals, zero, 1e-5);
- the sign path (`-1.0`, `-0.0`, `-inf`, negative `NaN` -> `i32::MIN`).

`dst` is filled with a sentinel so an unwritten tail cannot pass, and `count`
is always tied to the buffer length: the client has no bounds check, so a
fixture that draws them independently would fault or write out of the arena.
"""

from __future__ import annotations

import random
import struct
from typing import List

from emu_oracle import ArgSpec

DST_AT = 0x1000
SRC_AT = 0x2000
SENTINEL = 0xCD
MAX_ELEMS = 64

# (bit patterns, count). `count` is normally the buffer length; the first entry
# of the third block overrides it to 0 so the client's `test r8d,r8d; je` early
# exit is exercised - without it coverage stops at 4 of 5 branches.
DECLARED: List[tuple] = [
    # 1.0, -1.0, +0.0, -0.0, 1/60 (the client's own call-site constant),
    # 2048.0 (exponent 138, shift 0), 1e-5 (underflow), 1e30 (saturate)
    ([0x3F800000, 0xBF800000, 0x00000000, 0x80000000, 0x3C888889, 0x45000000,
      0x3727C5AC, 0x7149F2CA], None),
    # +inf, -inf, quiet NaN, sign-set NaN, min subnormal, -min subnormal,
    # 4096.0 (shift 1), 0.5 (shift 12)
    ([0x7F800000, 0xFF800000, 0x7FC00000, 0xFFC00000, 0x00000001, 0x80000001,
      0x45800000, 0x3F000000], None),
    # the two saturation boundaries: 2^19 exactly (shift 8) and just below it
    ([0x49800000, 0x497FFFFF, 0x4A000000, 0xCA000000], None),
    # count = 0: the loop is skipped entirely and dst keeps its sentinel
    ([0x3F800000], 0),
]


def _random_bits(i: int) -> List[int]:
    rng = random.Random(0x1200 + i)
    count = 1 + (i * 7) % MAX_ELEMS
    out = []
    for _ in range(count):
        # Half the draws stay in the ranges the path treats differently, so the
        # corpus is not dominated by exponent-255 garbage.
        pick = rng.randrange(4)
        if pick == 0:
            out.append(0x3F800000 | rng.getrandbits(23) & 0x007FFFFF)
        elif pick == 1:
            out.append(rng.getrandbits(32))
        elif pick == 2:
            exponent = rng.randrange(115, 160)
            out.append((rng.getrandbits(1) << 31) | (exponent << 23)
                       | (rng.getrandbits(23)))
        else:
            exponent = rng.randrange(0, 115)
            out.append((rng.getrandbits(1) << 31) | (exponent << 23)
                       | (rng.getrandbits(23)))
    return out


def make_args(i: int) -> List[ArgSpec]:
    if i < len(DECLARED):
        bits, override = DECLARED[i]
        count = len(bits) if override is None else override
    else:
        bits = _random_bits(i)
        count = len(bits)
    src = b"".join(struct.pack("<I", b) for b in bits)
    # Both buffers are the size of the source, even when `count` is smaller: the
    # candidate compares the whole buffer, and a zero-length pointee is not a
    # memory surface.
    dst = bytes([SENTINEL]) * len(src)
    return [
        ArgSpec(kind="ptr", mem=dst, arena_offset=DST_AT),
        ArgSpec(kind="ptr", mem=src, arena_offset=SRC_AT),
        ArgSpec(kind="u64", bits=count),
    ]

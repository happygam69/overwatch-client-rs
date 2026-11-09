"""Deterministic context inputs for the `fn/Md5Final` unit.

The client's `Md5Final` (`0x1426a5010`) takes the 0x58-byte context and a
16-byte digest destination. The function's shape depends only on
`fill = (count as u32) & 0x3f`, and it has three conditional edges:

* `0x1426a5044 jbe` — `fill + 1 <= 0x38`: one transform, padding to `0x38`.
* `0x1426a504a jae` — in that path, `fill + 1 >= 0x38`, i.e. exactly `0x38`:
  there is nothing to zero, so the `memset` is skipped.
* `0x1426a5068` is the two-block path's join; its `0x1426a508e jae` is the
  skip above and its `0x1426a5068` edge is reached only when
  `fill + 1 < 0x40` (a `0x40` fill needs no zeroing either).

The corpus walks `fill` across every region boundary: `0x36` (one byte to
zero), `0x37` (none, the skip edge), `0x38`/`0x3E` (two-block path with and
without an interior `memset`) and `0x3F` (two blocks, no zeroing). Counts are
also driven across the 32-bit boundary and into the `<< 3` overflow, because
the message length is written as `count << 3` over the buffer's last eight
bytes.

The digest destination starts at `0xA5`: the client writes all sixteen bytes
from the four chained words, so a partial write is a compared divergence. The
64-byte buffer starts as a non-zero pattern so the padding zeroing is visible
on the context channel.
"""

from __future__ import annotations

from typing import List, Tuple

from emu_oracle import ArgSpec

CTX_LEN = 0x58
DIGEST_SENTINEL = 0xA5
RFC_INIT = (0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476)


def _lcg_bytes(seed: int, n: int) -> bytes:
    """Version-independent deterministic bytes (a plain 32-bit LCG)."""
    x = seed & 0xFFFFFFFF
    out = bytearray(n)
    for i in range(n):
        x = (x * 1664525 + 1013904223) & 0xFFFFFFFF
        out[i] = x >> 24
    return bytes(out)


def _ctx(count: int, seed: int) -> bytes:
    state = b"".join(word.to_bytes(4, "little") for word in RFC_INIT)
    return count.to_bytes(8, "little") + state + _lcg_bytes(seed, 0x40)


# (count, buffer seed).
Case = Tuple[int, int]

SPECIALS: List[Case] = [
    # Determinism case: fill = 0, one transform, padding to 0x38.
    (0, 0x10),
    (1, 0x11),
    # fill = 0x36: the short path zeroes exactly one byte.
    (0x36, 0x12),
    # fill = 0x37: n = 0x38, the `jae` skip edge (nothing to zero).
    (0x37, 0x13),
    # fill = 0x38 and 0x3E: the two-block path with an interior memset.
    (0x38, 0x14),
    (0x3E, 0x15),
    # fill = 0x38 with a different buffer pattern, to separate the first
    # transform from the second.
    (0x38, 0x16),
    # fill = 0x3F: n = 0x40, the two-block path with no interior memset.
    (0x3F, 0x17),
    # count = 64 has fill 0 but a non-zero bit length.
    (64, 0x18),
    # 32-bit boundary: fill 63, count << 3 wraps.
    (0xFFFF_FFFF, 0x19),
    # A set high word: fill still comes from the low dword.
    (0x1_0000_0010, 0x1A),
    (0x1_0000_0037, 0x1B),
    # count << 3 drops everything: the length word must be zero here.
    (1 << 61, 0x1C),
    (0x2_0000_0000, 0x1D),
]


def make_case(i: int) -> Case:
    """The i-th (count, seed) case. Pure function of i."""
    if i < len(SPECIALS):
        return SPECIALS[i]
    r = i - len(SPECIALS)
    count = (r * 0x0100_0021) % (1 << 64)
    return count, 0x30000 + r


def make_args(i: int) -> List[ArgSpec]:
    count, seed = make_case(i)
    return [
        ArgSpec(kind="ptr", mem=_ctx(count, seed), arena_offset=0),
        ArgSpec(
            kind="ptr",
            mem=bytes([DIGEST_SENTINEL]) * 16,
            arena_offset=0x80,
        ),
    ]

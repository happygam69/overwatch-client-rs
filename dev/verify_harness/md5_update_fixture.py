"""Deterministic context/input pairs for the `fn/Md5Update` unit.

The client's `Md5Update` (`0x1426a4f40`) takes the 0x58-byte context, the input
pointer and the input length, and has four conditional edges:

* `0x1426a4f81 je` — `fill != 0 && len >= 0x40 - fill`: complete the buffered
  block and transform it.
* `0x1426a4fb2 jb` — `len >= 0x40` after that step: transform whole blocks
  straight from the input.
* `0x1426a4fd6 jne` — the block loop's back edge (taken when `blocks > 1`).
* `0x1426a4fdb je` — a remainder exists: copy it to `buf + fill`.

The corpus pins both edges of each: fills 0/5/32/63 against lengths below,
exactly at and above `0x40 - fill`, one block against many blocks, and an empty
remainder against non-empty ones. The `count` field is also driven across the
32-bit boundary, because the client reads `fill` from the **low dword** of the
64-bit count before adding the length.

Every case supplies a 0x58-byte context (so all three client fields are
compared) and a data buffer at least `len` bytes long; one case supplies a data
buffer longer than `len`, so bytes past the length must come back untouched.
"""

from __future__ import annotations

from typing import List, Tuple

from emu_oracle import ArgSpec

CTX_LEN = 0x58
RFC_INIT = (0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476)


def _lcg_bytes(seed: int, n: int) -> bytes:
    """Version-independent deterministic bytes (a plain 32-bit LCG)."""
    x = seed & 0xFFFFFFFF
    out = bytearray(n)
    for i in range(n):
        x = (x * 1664525 + 1013904223) & 0xFFFFFFFF
        out[i] = x >> 24
    return bytes(out)


def _seq(seed: int, n: int) -> bytes:
    return bytes((seed + i * 7) & 0xFF for i in range(n))


def _ctx(count: int, seed: int) -> bytes:
    """A client context whose count field is `count`."""
    state = b"".join(word.to_bytes(4, "little") for word in RFC_INIT)
    return count.to_bytes(8, "little") + state + _lcg_bytes(seed, 0x40)


# (count, data length, len, data seed). `len` may be shorter than the data
# length, which is then compared past the input length.
Case = Tuple[int, int, int, int]

SPECIALS: List[Case] = [
    # Determinism case: a fresh context, no input at all (count and context
    # must come back unchanged).
    (0, 0, 0, 0x10),
    # fill = 0, remainder-only inputs.
    (0, 1, 1, 0x11),
    (0, 2, 2, 0x12),
    (0, 63, 63, 0x13),
    # fill = 0, exact block, one block and many blocks (the loop back edge).
    (0, 64, 64, 0x14),
    (0, 65, 65, 0x15),
    (0, 128, 128, 0x16),
    (0, 192, 192, 0x17),
    # fill = 5: below space (0x3b = 59), exactly space, one past it.
    (5, 1, 1, 0x18),
    (5, 58, 58, 0x19),
    (5, 59, 59, 0x1A),
    (5, 60, 60, 0x1B),
    (5, 59 + 64, 59 + 64, 0x1C),
    (5, 59 + 65, 59 + 65, 0x1D),
    (5, 59 + 128, 59 + 128, 0x1E),
    # fill = 63: space = 1, so the first branch is the whole fill.
    (63, 1, 1, 0x1F),
    (63, 2, 2, 0x20),
    (63, 64, 64, 0x21),
    # fill = 32, exactly space.
    (32, 32, 32, 0x22),
    # The 32-bit count boundary: fill is the low dword's remainder, so
    # 0xFFFFFFFF has fill 63 and the count wraps to 0 after one byte.
    (0xFFFF_FFFF, 1, 1, 0x23),
    (0xFFFF_FFFF, 2, 2, 0x24),
    # A set high word: fill still comes from the low dword.
    (0x1_0000_0000 + 7, 100, 100, 0x25),
    (0x1_0000_0007, 52, 52, 0x26),
    # Many whole blocks, and many blocks with a remainder.
    (0, 4096, 4096, 0x27),
    (33, 1000, 1000, 0x28),
    # Data buffer longer than `len`: the client must not read or touch the
    # bytes past the length.
    (7, 100, 40, 0x29),
    (0, 200, 64, 0x2A),
]


def make_case(i: int) -> Case:
    """The i-th (count, data_len, len, seed) case. Pure function of i."""
    if i < len(SPECIALS):
        return SPECIALS[i]
    r = i - len(SPECIALS)
    count = (r * 0x0100_0021) % (1 << 64)
    length = [0, 1, 31, 32, 63, 64, 65, 96, 127, 128, 129, 500][r % 12]
    extra = r % 5 == 0
    return count, length + (16 if extra else 0), length, 0x30000 + r


def make_args(i: int) -> List[ArgSpec]:
    count, data_len, length, seed = make_case(i)
    data = _seq(seed, data_len)
    return [
        ArgSpec(kind="ptr", mem=_ctx(count, seed ^ 0x5555), arena_offset=0),
        ArgSpec(kind="ptr", mem=data, arena_offset=0x80),
        ArgSpec(kind="u64", bits=length),
    ]

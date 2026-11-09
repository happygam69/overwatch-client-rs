"""Deterministic context inputs for the `fn/Md5Init` unit.

The client's `Md5Init` (`0x1426a5100`) takes the 0x58-byte MD5 context and
writes only `+0x00..+0x18`: one 8-byte zero store at `+0x00` (the message byte
count) and the four RFC 1321 words at `+0x08`. The 64-byte input buffer at
`+0x18` is deliberately left alone, so every case fills it with a non-zero
pattern: a class that zeroed or rewrote the buffer would be a compared
divergence.

The context comes in with several fill patterns because the function has no
branch at all — the mutation stage is what carries this unit, and a state
initialiser whose stores move by a field still passes a zeroed buffer.
"""

from __future__ import annotations

from typing import List

from emu_oracle import ArgSpec

CTX_LEN = 0x58

# The client's words, read from the four `mov dword` stores at
# `0x1426a5100..0x1426a511c`.
INIT_WORDS = (0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476)


def _lcg_bytes(seed: int, n: int) -> bytes:
    """Version-independent deterministic bytes (a plain 32-bit LCG)."""
    x = seed & 0xFFFFFFFF
    out = bytearray(n)
    for i in range(n):
        x = (x * 1664525 + 1013904223) & 0xFFFFFFFF
        out[i] = x >> 24
    return bytes(out)


SPECIALS: List[bytes] = [
    # Determinism case: every byte outside the three written fields is 0x5A.
    bytes([0x5A]) * CTX_LEN,
    bytes(CTX_LEN),
    bytes([0xFF]) * CTX_LEN,
    bytes([0x01]) * CTX_LEN,
    bytes([0x80]) * CTX_LEN,
    # A context that already holds a previous digest: the init must replace
    # the count and the four state words.
    bytes.fromhex(
        "1020304050607080"
        "01234567" "89abcdef" "fedcba98" "76543210"
    )
    + bytes([0xAA]) * (CTX_LEN - 0x18),
    # Alternating bits in every field.
    bytes([0xAA, 0x55]) * (CTX_LEN // 2),
]


def make_case(i: int) -> bytes:
    """The i-th context. Pure function of i."""
    if i < len(SPECIALS):
        return SPECIALS[i]
    return _lcg_bytes(0x10000 + i, CTX_LEN)


def make_args(i: int) -> List[ArgSpec]:
    return [ArgSpec(kind="ptr", mem=make_case(i), arena_offset=0)]

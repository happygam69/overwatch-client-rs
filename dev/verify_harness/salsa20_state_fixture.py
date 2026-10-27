"""Deterministic key/nonce/counter inputs for the `fn/Salsa20StateInit` unit.

The client's `Salsa20StateInit` (`0x1424e3230`) takes an output buffer, a
16-byte key, an 8-byte nonce and a 64-bit block counter. The oracle is the
client's own bytes; this module only builds the inputs.

Case 0 is the determinism input and the pinned known-answer case: key
`01..10`, nonce `10..17`, counter 0. The 64 bytes it produces are pinned by
`state_init_matches_the_client_state_vector` in `ow-casc`'s `blte.rs`, which
also fixes the diagonal word order. The cases that follow pin the counter's
word split (low word all ones, high word one, both words all ones) and then
sweep keys, nonces and counters deterministically.

The output buffer starts as a nonzero pattern in every case, so a build that
skips one of the sixteen stores leaves a visible difference instead of a zero
that may already be correct.
"""

from __future__ import annotations

from typing import List, Tuple

from emu_oracle import ArgSpec

# Every byte nonzero: an unwritten word is never accidentally equal to the
# initial value.
_INITIAL_STATE = bytes((i * 37 + 11) & 0xFF for i in range(64))

_FF_KEY = bytes([0xFF]) * 16
_FF_NONCE = bytes([0xFF]) * 8
_ZERO_KEY = bytes(16)
_ZERO_NONCE = bytes(8)
_SEQ_KEY = bytes(range(16))
_SEQ_NONCE = bytes(range(8))

# (key, nonce, counter). The first case is the determinism case and the
# known-answer vector; the corpus is front-loaded because the mutation stage
# scores `cases[:48]`.
SPECIALS: List[Tuple[bytes, bytes, int]] = [
    (bytes(range(1, 17)), bytes(range(0x10, 0x18)), 0),
    (_ZERO_KEY, _ZERO_NONCE, 0),
    (_FF_KEY, _FF_NONCE, 0),
    (_ZERO_KEY, _ZERO_NONCE, 1),
    (_ZERO_KEY, _ZERO_NONCE, 2),
    (_ZERO_KEY, _ZERO_NONCE, 0xFFFF_FFFF),
    (_ZERO_KEY, _ZERO_NONCE, 0x1_0000_0000),
    (_ZERO_KEY, _ZERO_NONCE, 0xFFFF_FFFF_FFFF_FFFF),
    (_ZERO_KEY, _ZERO_NONCE, 0x0123_4567_89AB_CDEF),
    (_SEQ_KEY, _SEQ_NONCE, 0xDEAD_BEEF_CAFE_F00D),
    (bytes([0x80]) + bytes(15), _ZERO_NONCE, 0),
    (bytes([0xAA, 0x55]) * 8, bytes.fromhex("0123456789abcdef"), 0x1122_3344),
    (_FF_KEY, _ZERO_NONCE, 0xFFFF_FFFF_0000_0000),
    (_ZERO_KEY, _FF_NONCE, 0x0000_0000_FFFF_FFFF),
    (bytes(range(16)), bytes([0x5A]) * 8, 0x8000_0000),
    (bytes([0x42]) * 16, bytes([0x24]) * 8, 0x7FFF_FFFF_FFFF_FFFF),
]


def _lcg_bytes(seed: int, n: int) -> bytes:
    """Version-independent deterministic bytes (a plain 32-bit LCG)."""
    x = seed & 0xFFFFFFFF
    out = bytearray(n)
    for i in range(n):
        x = (x * 1664525 + 1013904223) & 0xFFFFFFFF
        out[i] = x >> 24
    return bytes(out)


def _lcg_u64(seed: int, i: int) -> int:
    x = (seed + i * 0x9E37_79B9) & 0xFFFF_FFFF_FFFF_FFFF
    for _ in range(3):
        x = (x * 6364136223846793005 + 1442695040888963407) & 0xFFFF_FFFF_FFFF_FFFF
    return x


def make_case(i: int) -> Tuple[bytes, bytes, int]:
    """The i-th (key, nonce, counter). Pure function of i."""
    if i < len(SPECIALS):
        return SPECIALS[i]
    r = i - len(SPECIALS)
    return _lcg_bytes(0x40000 + r, 16), _lcg_bytes(0x50000 + r, 8), _lcg_u64(0x60000, r)


def make_args(i: int) -> List[ArgSpec]:
    key, nonce, counter = make_case(i)
    return [
        ArgSpec(kind="ptr", mem=_INITIAL_STATE),
        ArgSpec(kind="ptr", mem=key),
        ArgSpec(kind="ptr", mem=nonce),
        ArgSpec(kind="u64", bits=counter),
    ]

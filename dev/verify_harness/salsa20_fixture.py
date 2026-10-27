"""Deterministic Salsa20 inputs for the `fn/Salsa20Xor` unit.

The client's `Salsa20Xor` (`0x1424e32c0`) takes a 64-byte Salsa20 state, a
buffer and a length. The state is what `Salsa20StateInit` (`0x1424e3230`)
writes from a 16-byte key, an 8-byte nonce and a 64-bit block counter. That
initializer stores the state in the diagonal order: words 0..3 are the four
`expand 16-byte k` constants, the key words sit at 5, 4, 9 and 12, the nonce
at 6 and 11, and the counter at 8 (low) and 13 (high).

The fixture builds exactly that state, so the oracle sees the same state the
client's BLTE path would build. The probe parses the same state back into
key/nonce/counter and runs the shipped `ow_casc::blte::salsa20`, which always
starts at block 0; the probe discards the first `counter` blocks to reach the
requested starting block.

Case 0 is the determinism input. The first `len(SPECIALS)` cases are pinned
vectors: zero and all-ones keys, block-boundary lengths, partial tails and
nonzero counters. Later cases are a deterministic length/byte sweep. Every
case is a pure function of its index.
"""

from __future__ import annotations

import struct
from typing import List, Tuple

from emu_oracle import ArgSpec

TAU = b"expand 16-byte k"

# The client's state for key 01..10, nonce 10..17, counter 0, read from
# `Salsa20StateInit` at 0x1424e3230 through the emulator. Pinning it here makes
# the layout an explicit part of the input digest: wrong key/nonce/counter
# placement cannot silently agree with the probe.
_STATE_KAT = bytes.fromhex(
    "657870616e642031362d62797465206b"
    "05060708010203041011121301020304"
    "00000000090a0b0c0506070814151617"
    "0d0e0f10000000000d0e0f10090a0b0c"
)


def state_from_key_nonce_counter(key: bytes, nonce: bytes, counter: int) -> bytes:
    """The 64-byte state the client's `Salsa20StateInit` builds."""
    if len(key) != 16 or len(nonce) != 8:
        raise ValueError("key must be 16 bytes and nonce 8 bytes")
    if counter < 0 or counter >= 1 << 64:
        raise ValueError("counter out of range")
    const = [struct.unpack_from("<I", TAU, 4 * i)[0] for i in range(4)]
    k = [struct.unpack_from("<I", key, 4 * i)[0] for i in range(4)]
    n = [struct.unpack_from("<I", nonce, 4 * i)[0] for i in range(2)]
    # The standard Salsa20 matrix, then stored diagonally.
    x = [
        const[0], k[0], k[1], k[2], k[3], const[1], n[0], n[1],
        counter & 0xFFFFFFFF, (counter >> 32) & 0xFFFFFFFF,
        const[2], k[0], k[1], k[2], k[3], const[3],
    ]
    y = [
        x[0], x[5], x[10], x[15],
        x[12], x[1], x[6], x[11],
        x[8], x[13], x[2], x[7],
        x[4], x[9], x[14], x[3],
    ]
    return struct.pack("<16I", *y)


assert (
    state_from_key_nonce_counter(bytes(range(1, 17)), bytes(range(0x10, 0x18)), 0)
    == _STATE_KAT
), "Salsa20 state layout no longer matches the pinned client vector"


def _lcg_bytes(seed: int, n: int) -> bytes:
    """Version-independent deterministic bytes (a plain 32-bit LCG)."""
    x = seed & 0xFFFFFFFF
    out = bytearray(n)
    for i in range(n):
        x = (x * 1664525 + 1013904223) & 0xFFFFFFFF
        out[i] = x >> 24
    return bytes(out)


def _seq(start: int, n: int) -> bytes:
    return bytes((start + i) & 0xFF for i in range(n))


_ZERO_KEY = bytes(16)
_ZERO_NONCE = bytes(8)
_FF_KEY = bytes([0xFF]) * 16
_FF_NONCE = bytes([0xFF]) * 8
_SEQ_KEY = bytes(range(16))
_SEQ_NONCE = bytes(range(8))

# (key, nonce, counter, data). The corpus is intentionally front-loaded: the
# mutation stage scores `cases[:48]`, so every boundary shape must be there.
SPECIALS: List[Tuple[bytes, bytes, int, bytes]] = [
    # Determinism case: every key, nonce and constant word is nonzero.
    (bytes(range(1, 17)), bytes(range(0x10, 0x18)), 0, _seq(0, 64)),
    # Empty and single-byte inputs: the two early-outs in Salsa20Xor.
    (_ZERO_KEY, _ZERO_NONCE, 0, b""),
    (_ZERO_KEY, _ZERO_NONCE, 0, b"\x00"),
    (_ZERO_KEY, _ZERO_NONCE, 0, b"\xFF\x00\xFF"),
    # Incomplete, exact and one-past one block.
    (_ZERO_KEY, _ZERO_NONCE, 0, _seq(0, 63)),
    (_ZERO_KEY, _ZERO_NONCE, 0, _seq(0, 64)),
    (_ZERO_KEY, _ZERO_NONCE, 0, _seq(0, 65)),
    (_FF_KEY, _FF_NONCE, 0, bytes([0xFF]) * 255),
    (_FF_KEY, _FF_NONCE, 0, bytes([0xFF]) * 256),
    (_FF_KEY, _FF_NONCE, 0, bytes([0xFF]) * 257),
    # The client's counter starts at zero for a fresh chunk; these pin the
    # nonzero starting block the state can also express.
    (_SEQ_KEY, _SEQ_NONCE, 1, _seq(0, 64)),
    (_SEQ_KEY, _SEQ_NONCE, 2, _seq(0, 192)),
    (_SEQ_KEY, _SEQ_NONCE, 3, _seq(0, 65)),
    # The ECRYPT 128-bit-key reference key shape.
    (bytes([0x80]) + bytes(15), _ZERO_NONCE, 0, bytes(64)),
    # Both 256-byte dispatch and the 64-byte dispatch in one call.
    (bytes(range(1, 17)), bytes(range(0x10, 0x18)), 0, _seq(0, 300)),
    (bytes(range(1, 17)), bytes(range(0x10, 0x18)), 0, _seq(0, 511)),
    (bytes(range(1, 17)), bytes(range(0x10, 0x18)), 0, _seq(0, 512)),
    (bytes(range(1, 17)), bytes(range(0x10, 0x18)), 0, _seq(0, 513)),
    (bytes([0x42]) * 16, bytes([0x24]) * 8, 3, _lcg_bytes(0x51, 1000)),
    (bytes(range(1, 17)), bytes(range(0x10, 0x18)), 1, _lcg_bytes(0x52, 100)),
    (bytes([0xAA, 0x55]) * 8, bytes.fromhex("0123456789abcdef"), 0, _seq(0, 128)),
    (bytes(range(1, 17)), bytes(range(0x10, 0x18)), 0, _lcg_bytes(0x53, 4096)),
    (bytes([0x00, 0x11]) * 8, bytes([0x5A]) * 8, 2, _seq(0, 1)),
    (bytes([0xFE]) * 16, bytes([0x01]) * 8, 0, bytes(64)),
]

_LENGTHS = [
    0, 1, 2, 3, 7, 15, 16, 31, 32, 63, 64, 65, 100, 127, 128, 129,
    191, 192, 255, 256, 257, 300, 511, 512, 513, 1000, 1024, 1025, 4096,
]


def make_case(i: int) -> Tuple[bytes, bytes, int, bytes]:
    """The i-th (key, nonce, counter, data) case. Pure function of i."""
    if i < len(SPECIALS):
        return SPECIALS[i]
    r = i - len(SPECIALS)
    key = _lcg_bytes(0x10000 + r, 16)
    nonce = _lcg_bytes(0x20000 + r, 8)
    counter = r % 4
    length = _LENGTHS[r % len(_LENGTHS)]
    return key, nonce, counter, _lcg_bytes(0x30000 + r, length)


def make_args(i: int) -> List[ArgSpec]:
    key, nonce, counter, data = make_case(i)
    state = state_from_key_nonce_counter(key, nonce, counter)
    return [
        ArgSpec(kind="ptr", mem=state),
        ArgSpec(kind="ptr", mem=data),
        ArgSpec(kind="u64", bits=len(data)),
    ]

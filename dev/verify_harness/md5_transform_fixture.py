"""Deterministic block/state inputs for the `fn/Md5Transform` unit.

The client's 64-round block core (`Md5Transform`, exe `0x1426ab530`) takes the
64-byte message block and the four-word chaining state, and leaves the state
words at their post-block values. Both buffers are compared, so the block must
come back unchanged and every one of the sixteen state bytes is pinned.

Case 0 is the determinism case: the message block is the byte sequence
`0x00..0x3F` and the state is the RFC 1321 initial value. The remaining cases
cover the block patterns that flip every message-word bit (all zeros, all ones,
a byte ramp, seeded random) against the state patterns that make the carry and
rotation chains distinguishable (RFC init, zeros, all ones, seeded random).
"""

from __future__ import annotations

from typing import List, Tuple

from emu_oracle import ArgSpec

RFC_INIT = (0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476)

Case = Tuple[bytes, bytes]  # (block, state)


def _block(fill: int) -> bytes:
    return bytes([fill]) * 64


def _lcg_bytes(seed: int, n: int) -> bytes:
    """Version-independent deterministic bytes (a plain 32-bit LCG)."""
    x = seed & 0xFFFFFFFF
    out = bytearray(n)
    for i in range(n):
        x = (x * 1664525 + 1013904223) & 0xFFFFFFFF
        out[i] = x >> 24
    return bytes(out)


def _state(words: Tuple[int, int, int, int]) -> bytes:
    return b"".join(w.to_bytes(4, "little") for w in words)


SPECIALS: List[Case] = [
    (bytes(range(64)), _state(RFC_INIT)),
    (_block(0x00), _state(RFC_INIT)),
    (_block(0xFF), _state(RFC_INIT)),
    (bytes(range(64)), _state((0, 0, 0, 0))),
    (bytes(range(64)), _state((0xFFFF_FFFF,) * 4)),
    (_block(0x00), _state((0, 0, 0, 0))),
    (_block(0x00), _state((0xFFFF_FFFF,) * 4)),
    (_block(0xFF), _state((0, 0, 0, 0))),
    (_block(0xFF), _state((0xFFFF_FFFF,) * 4)),
    (bytes(range(64)), _state((0x8000_0000, 0x7FFF_FFFF, 1, 0xFFFF_FFFE))),
    (_lcg_bytes(0x10000, 64), _state(RFC_INIT)),
    (bytes(range(63, -1, -1)), _state(RFC_INIT)),
    (bytes([0x80]) + bytes(63), _state(RFC_INIT)),
    (bytes(63) + b"\x80", _state(RFC_INIT)),
    (bytes(range(64)), _state((0x0123_4567, 0x89AB_CDEF, 0xFEDC_BA98, 0x7654_3210))),
    (_lcg_bytes(0x20000, 64), _state((0xDEAD_BEEF,) * 4)),
]


def make_case(i: int) -> Case:
    if i < len(SPECIALS):
        return SPECIALS[i]
    r = i - len(SPECIALS)
    block_id = r % 4
    state_id = (r // 4) % 4
    if block_id == 0:
        block = _block(0x00)
    elif block_id == 1:
        block = _block(0xFF)
    elif block_id == 2:
        block = bytes(range(64))
    else:
        block = _lcg_bytes(0x30000 + r, 64)
    if state_id == 0:
        state = _state(RFC_INIT)
    elif state_id == 1:
        state = _state((0, 0, 0, 0))
    elif state_id == 2:
        state = _state((0xFFFF_FFFF,) * 4)
    else:
        state = _lcg_bytes(0x40000 + r, 16)
    return block, state


def make_args(i: int) -> List[ArgSpec]:
    block, state = make_case(i)
    return [
        ArgSpec(kind="ptr", mem=block, arena_offset=0),
        ArgSpec(kind="ptr", mem=state, arena_offset=0x80),
    ]

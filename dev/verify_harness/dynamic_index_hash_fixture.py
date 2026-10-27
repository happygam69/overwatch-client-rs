"""Deterministic data/state inputs for the `fn/DynamicIndexHash` unit.

The client's `DynamicIndexHash` (`0x142512c60`) takes a byte buffer, its
length, and two in/out 32-bit state words. The oracle is the client's own
bytes; this module only builds the inputs.

The client's loop has three paths, selected by the data pointer's low two
bits (16-bit aligned, odd, otherwise 16-bit). `arena_offset` pins that edge so
every path is exercised. The first cases are the pinned vectors from the real
`0300000159.idx` file header and its first two entries (the values `index.rs`
checks against that file), then every tail length 0..12 at each alignment,
then lengths that run the 12-byte block loop, then a deterministic sweep.

Every buffer is padded to the unit's declared 96-byte pointee, so bytes past
`len` are compared too: the callee must leave them alone.
"""

from __future__ import annotations

import struct
from typing import List, Tuple

from emu_oracle import ArgSpec

DATA_LEN = 96

# Real `0300000159.idx` values: the 16-byte file header and two 18-byte
# entries. The client stores the header hash `0xc52f303d`; the rolling entry
# states are the values `index.rs`'s test pins.
_FILE_HEADER = bytes.fromhex("070003000405091e000000c0ff000000")
_ENTRY_1 = bytes.fromhex("00001d9bbc07919efd0fd56f31a2b4220000")
_ENTRY_2 = bytes.fromhex("00003400342943950309a1666c77513a0000")


def _seq(seed: int, n: int) -> bytes:
    return bytes((seed + i * 7) & 0xFF for i in range(n))


def _words(seed: int) -> Tuple[int, int]:
    raw = _seq(seed, 8)
    return struct.unpack("<II", raw)


def _cases() -> List[Tuple[int, bytes, int, int]]:
    out: List[Tuple[int, bytes, int, int]] = [
        (0, _FILE_HEADER, 0, 0),
        (0, _ENTRY_1, 0, 0),
        (0, _ENTRY_2, 0xEF6C_3F29, 0xD114_251E),
        (0, b"", 0, 0),
        (0, b"", 0xDEAD_BEEF, 0x1234_5678),
    ]
    # Every tail length at each of the three paths. The first 48 cases are the
    # mutation corpus, so they must contain all three alignments and the short
    # tails a `match` arm mutant can break.
    for align in (0, 1, 2):
        for n in range(0, 13):
            s0, s1 = _words(0x900 + align * 0x40 + n)
            out.append((align, _seq(0x10 + align * 0x100 + n, n), s0, s1))
    for n in range(0, 4):
        s0, s1 = _words(0x980 + n)
        out.append((3, _seq(0x310 + n, n), s0, s1))
    # Remaining odd-path tails, then lengths that enter the 12-byte block loop.
    for n in range(4, 13):
        s0, s1 = _words(0x990 + n)
        out.append((3, _seq(0x320 + n, n), s0, s1))
    for n in (13, 18, 24, 25, 36, 37, 48, 96):
        for align in (0, 1, 2, 3):
            s0, s1 = _words(0xA00 + n + align * 0x40)
            out.append((align, _seq(0x20 + n + align * 0x100, n), s0, s1))
    return out


SPECIALS = _cases()

_LENGTHS = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 18, 23, 24, 25, 30, 36,
    37, 48, 60, 72, 84, 95, 96,
]


def _lcg_bytes(seed: int, n: int) -> bytes:
    """Version-independent deterministic bytes (a plain 32-bit LCG)."""
    x = seed & 0xFFFFFFFF
    out = bytearray(n)
    for i in range(n):
        x = (x * 1664525 + 1013904223) & 0xFFFFFFFF
        out[i] = x >> 24
    return bytes(out)


def make_case(i: int) -> Tuple[int, bytes, int, int]:
    """The i-th (alignment, data, state0, state1). Pure function of i."""
    if i < len(SPECIALS):
        return SPECIALS[i]
    r = i - len(SPECIALS)
    align = r % 4
    length = _LENGTHS[r % len(_LENGTHS)]
    s0, s1 = _words(0xB0000 + r)
    return align, _lcg_bytes(0xC0000 + r, length), s0, s1


def make_args(i: int) -> List[ArgSpec]:
    align, data, s0, s1 = make_case(i)
    buf = data + bytes(DATA_LEN - len(data))
    return [
        ArgSpec(kind="ptr", mem=buf, arena_offset=align),
        ArgSpec(kind="u64", bits=len(data)),
        ArgSpec(kind="ptr", mem=struct.pack("<I", s0), arena_offset=0x1000),
        ArgSpec(kind="ptr", mem=struct.pack("<I", s1), arena_offset=0x1004),
    ]

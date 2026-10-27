"""Deterministic inputs for the `fn/DynamicIndexKeyHash` unit.

The client's `DynamicIndexKeyHash` (`0x142513230`) takes a 19-byte key, its
length and a 32-bit init word. `DynamicIndexSegmentParse` (`0x14250fcf0`) calls
it once per 24-byte page record as `(record + 4, 0x13, 0)` and compares
`hash | 0x80000000` with the record's first dword.

The client's load loop has three paths, selected by the key pointer's low two
bits (16-bit aligned, even-not-4, odd). `arena_offset` pins that edge so every
path is exercised. All three assemble the same twelve bytes, so the return
value must not depend on the alignment.

The install's page tables are zero-filled, so there is no real page record
with a stored hash. The first cases are the client's own outputs for the page
key shape (19 zero bytes), a real 19-byte span of the first entry block of
`0300000159.idx`, empty keys, every tail length 1..12 and the block loop.
Later cases sweep every length 0..64 at every alignment and a range of init
words. Every buffer is padded to the unit's declared 96-byte pointee, so bytes
past `len` are compared too: the callee must leave them alone.
"""

from __future__ import annotations

from typing import List, Tuple

from emu_oracle import ArgSpec

DATA_LEN = 96

# Real `0300000159.idx` bytes: a 19-byte span of each of the first two entry
# block records. The page tables themselves are all zero, so the genuine
# 19-byte page-key shape is 19 zero bytes, pinned above.
_ENTRY_1_19 = bytes.fromhex("00001d9bbc07919efd0fd56f31a2b422000000")
_ENTRY_2_19 = bytes.fromhex("00003400342943950309a1666c77513a000000")


def _seq(seed: int, n: int) -> bytes:
    return bytes((seed + i * 7) & 0xFF for i in range(n))


def _cases() -> List[Tuple[int, bytes, int]]:
    """The pinned (alignment, key, init) cases, then the full cross product."""
    pinned: List[Tuple[int, bytes, int]] = [
        # Empty input is the client's case 0: return c before the final mix.
        (0, b"", 0),
        (0, b"", 1),
        (0, b"", 0xFFFF_FFFF),
        # Only R8D reaches c, so the upper init bits must not matter.
        (0, b"", 0x1_0000_0000),
        (0, b"", 0x1_0000_0001),
        (0, b"", 0xFFFF_FFFF_FFFF_FFFF),
        # The real page-key shape: the install's page tables are zero-filled.
        (0, bytes(19), 0),
        (0, bytes(19), 0xFFFF_FFFF),
        # The real 19-byte entry-block spans, next to the client's page key.
        (0, _ENTRY_1_19, 0),
        (0, _ENTRY_2_19, 0),
        # The page key is 19 bytes; its 12-byte block plus a 7-byte tail.
        (0, _seq(0x00, 19), 0),
        (0, _seq(0x00, 19), 1),
        (0, _seq(0x00, 19), 0xFFFF_FFFF),
        (0, _seq(0x00, 19), 0x1_0000_0001),
        # Every remaining tail length 1..12, nonzero bytes so a shortened tail
        # add cannot agree by accident.
        (0, _seq(0x20, 13), 0),
        (0, _seq(0x30, 25), 0),
        (0, _seq(0x40, 24), 0),
        (0, _seq(0x50, 8), 0),
        (0, _seq(0x60, 9), 0),
        (0, _seq(0x70, 10), 0),
        (0, _seq(0x80, 14), 0),
        (0, _seq(0x90, 15), 0),
        (0, _seq(0xA0, 16), 0),
        (0, _seq(0xB0, 17), 0),
        (0, _seq(0xC0, 18), 0),
        (0, _seq(0xD0, 23), 0),
    ]
    # Every length 0..64 at every pointer alignment, with the init words the
    # unit requires. `n >= 13` enters the twelve-byte loop in each path.
    for n in range(0, 65):
        for align in range(4):
            init = (0, 1, 0xFFFF_FFFF, 0xDEAD_BEEF)[(n + align) % 4]
            pinned.append((align, _seq(0x1000 + n * 7 + align * 0x40, n), init))
    return pinned


SPECIALS = _cases()

# Lengths used by the deterministic sweep after the pinned cross product.
_LENGTHS = list(range(0, 65)) + [18, 19, 23, 24, 25, 36, 48, 60, 64]


def _lcg_bytes(seed: int, n: int) -> bytes:
    """Version-independent deterministic bytes (a plain 32-bit LCG)."""
    x = seed & 0xFFFFFFFF
    out = bytearray(n)
    for i in range(n):
        x = (x * 1664525 + 1013904223) & 0xFFFFFFFF
        out[i] = x >> 24
    return bytes(out)


def _after_specials(r: int) -> Tuple[int, bytes, int]:
    """The deterministic sweep that follows the pinned cross product."""
    align = r % 4
    n = _LENGTHS[(r // 4) % len(_LENGTHS)]
    init = (0, 1, 0xFFFF_FFFF, 0xDEAD_BEEF, 0x1_0000_0001)[r % 5]
    return align, _lcg_bytes(0x10000 + r, n), init


def make_case(i: int) -> Tuple[int, bytes, int]:
    """The i-th (alignment, key, init). Pure function of i."""
    if i < len(SPECIALS):
        return SPECIALS[i]
    return _after_specials(i - len(SPECIALS))


def make_args(i: int) -> List[ArgSpec]:
    align, key, init = make_case(i)
    buf = key + bytes(DATA_LEN - len(key))
    return [
        ArgSpec(kind="ptr", mem=buf, arena_offset=align),
        ArgSpec(kind="u64", bits=len(key)),
        ArgSpec(kind="u64", bits=init),
    ]

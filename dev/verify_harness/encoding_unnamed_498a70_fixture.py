"""Deterministic inputs for the `fn/EncodingEspecStringIndex` unit.

The client (`EncodingEspecStringIndex`, exe `0x142498a70`) takes one
pointer, in RCX, to a struct: the espec block size (u32 at `+0x24`), the
block pointer (`+0x28`), and a pointer vector at `+0x50` whose buffer is
at `+0x58`, size at `+0x60` and capacity at `+0x64`. It rejects a block
whose last byte is not zero (status 8), counts the NUL bytes, reserves
the vector to that count, and appends every string start
(`base + offset`) to the vector, returning 0. For an empty block the
last-byte offset is the zero-extended 32-bit `count - 1`, so the read
lands at `base + 0xFFFFFFFF` - in this harness the `0xC3`-filled sentinel
page - and returns 8; the byte at `base - 1` is not read. At every NUL
the string-length helper returns the string's length, so an empty
string (a NUL at the block start or after the previous NUL) returns 8
before its append, keeping any appends already made.

One 0x3000-byte blob carries everything: the 0x68-byte object header at
0, `0xA5` sentinel bytes up to the espec block at 0x1000 (whose
preceding byte at 0x0FFF is the empty-block `before` byte), and the
pointer buffer at 0x2000, pre-filled with `0xCCCCCCCCCCCCCCCC` so an
unwritten slot is observable. The header embeds absolute arena
addresses (`ARENA_BASE + offset`) for the block and the buffer; the
allocator slot at `+0x50` is zero because the grow path never reads it
on the modelled path.

Lengths and counts stay tied: the stored size is exactly the block
length, and every case pins `nul_count <= capacity` and
`size + nul_count <= capacity`, so the reserve at `0x142388210` is a
no-op and no append reaches `size == capacity` - the allocator-vtable
grow path is the unit's declared gap, and the probe rejects a case that
would grow. The tightest no-grow cases have
`capacity == nul_count` (size 0) or `capacity == size + nul_count`.
"""

from __future__ import annotations

from typing import List, Tuple

from emu_oracle import ARENA_BASE, ArgSpec

BLOCK_OFF = 0x1000
INDEX_OFF = 0x2000
BLOB_LEN = 0x3000
HEADER_LEN = 0x68
COUNT_FIELD = 0x24
BLOCK_FIELD = 0x28
ALLOC_FIELD = 0x50
INDEX_FIELD = 0x58
SIZE_FIELD = 0x60
CAPACITY_FIELD = 0x64
INDEX_SENTINEL = 0xCCCCCCCCCCCCCCCC

# (block bytes, byte at base-1, incoming size, capacity)
Case = Tuple[bytes, int, int, int]


def _nul_count(block: bytes) -> int:
    return block.count(0)


def _cases() -> List[Case]:
    out: List[Case] = []

    def add(block: bytes, before: int = 0, size: int = 0, capacity=None):
        nul = _nul_count(block)
        if capacity is None:
            capacity = size + nul + 8
        assert nul <= capacity, (block, nul, capacity)
        assert size + nul <= capacity, (block, size, nul, capacity)
        out.append((bytes(block), before, size, capacity))

    # Case 0: the determinism case. Two short strings.
    add(b"ab\x00cd\x00")

    # Empty blocks: the last-byte read lands at `base + 0xFFFFFFFF`
    # (zero-extended count-1), the sentinel page, and returns 8,
    # whatever byte sits at `base - 1`.
    add(b"", before=0x00)
    add(b"", before=0x41)
    add(b"", before=0x00, size=3, capacity=8)
    add(b"", before=0xFF, size=3, capacity=8)

    # One element, and a single empty string.
    add(b"a\x00", capacity=1)
    add(b"\x00", capacity=1)
    add(b"\x00", capacity=8)

    # Match at the first byte: the first string is empty.
    add(b"\x00a\x00")
    add(b"\x00a\x00", capacity=2)  # tightest: capacity == NUL count
    # Match at the last byte only, after a long prefix.
    add(bytes(range(1, 256)) + b"\x00")

    # No match at all: unterminated blocks return 8 and write nothing,
    # even when they contain a NUL.
    add(b"abc")
    add(b"ab\x00cd")
    add(b"a")
    add(b"\xff\xfe")

    # All-zero blocks: every string is empty.
    add(b"\x00\x00\x00", capacity=3)
    add(b"\x00" * 9, capacity=9)

    # Consecutive terminators and high bytes.
    add(b"a\x00\x00")
    add(b"\xff\xfe\x00\x80\x00")
    add(b"a\x00\x00b\x00\x00")

    # Appends land at the incoming size, not at zero.
    add(b"a\x00b\x00", size=2, capacity=8)
    add(b"a\x00b\x00c\x00", size=1, capacity=4)  # tightest with size 1
    add(b"\x00", size=5, capacity=6)

    # A long string and many strings (the indices run far from zero).
    add(b"x" * 255 + b"\x00")
    add(b"".join(b"e%d\x00" % i for i in range(32)), capacity=32)
    add(b"".join(b"e%d\x00" % i for i in range(32)))
    return out


SPECIALS = _cases()


def _lcg(seed: int, n: int) -> int:
    x = seed & 0xFFFFFFFF
    for _ in range(n):
        x = (x * 1664525 + 1013904223) & 0xFFFFFFFF
    return x >> 24


def _random(r: int) -> Case:
    length = r % 65
    block = bytearray()
    for k in range(length):
        block.append(_lcg(0x498A70 + r, k) % 4 if (r + k) % 3 else _lcg(0x1234 + r, k))
    if r % 4 == 0 and block:
        # Force an unterminated tail.
        block[-1] = 0x7F
    elif block:
        block[-1] = 0
    nul = _nul_count(bytes(block))
    size = r % 4
    capacity = size + nul + (r % 9)
    return bytes(block), 0xA5 if r % 5 == 0 else 0, size, capacity


def make_case(i: int) -> Case:
    if i < len(SPECIALS):
        return SPECIALS[i]
    return _random(i - len(SPECIALS))


def make_args(i: int) -> List[ArgSpec]:
    block, before, size, capacity = make_case(i)
    assert len(block) <= 0xF00, len(block)
    assert INDEX_OFF + capacity * 8 <= BLOB_LEN, capacity
    blob = bytearray(b"\xA5" * BLOB_LEN)
    blob[0:HEADER_LEN] = bytes(HEADER_LEN)
    blob[COUNT_FIELD : COUNT_FIELD + 4] = len(block).to_bytes(4, "little")
    blob[BLOCK_FIELD : BLOCK_FIELD + 8] = (ARENA_BASE + BLOCK_OFF).to_bytes(8, "little")
    blob[ALLOC_FIELD : ALLOC_FIELD + 8] = (0).to_bytes(8, "little")
    blob[INDEX_FIELD : INDEX_FIELD + 8] = (ARENA_BASE + INDEX_OFF).to_bytes(8, "little")
    blob[SIZE_FIELD : SIZE_FIELD + 4] = size.to_bytes(4, "little")
    blob[CAPACITY_FIELD : CAPACITY_FIELD + 4] = capacity.to_bytes(4, "little")
    blob[BLOCK_OFF - 1] = before
    blob[BLOCK_OFF : BLOCK_OFF + len(block)] = block
    for slot in range(capacity):
        at = INDEX_OFF + slot * 8
        blob[at : at + 8] = INDEX_SENTINEL.to_bytes(8, "little")
    return [ArgSpec(kind="ptr", mem=bytes(blob), arena_offset=0)]

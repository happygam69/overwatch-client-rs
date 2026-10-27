"""Deterministic inputs for the `fn/CmfKey154504` unit.

The client's `CmfKey154504` (`0x142e48a20`) reads the 512-byte table at
`0x1436100c0` (the unit's `state` block) and writes 32 key bytes to its output
pointer. It reads no header field and no file name: the table is the only
input, so this unit varies the untouched header/output bytes instead of
pretending there is a second input.

The table is synthetic and not game data. It is the `(i * 37 + 11) & 0xFF`
pattern (a permutation of 0..255) with three pinned entries:

* `table[0x120] = 162` is the client's seed `table[0x100 + 32]`;
* `table[162] = 0x00` is the first byte read, so the walk exercises 0x00;
* `table[255] = 0xFF` is the last byte read (162 + 3 * 31), so it exercises
  0xFF.

The walk is therefore 162, 165, ..., 255: 32 distinct indices, all 32 output
bytes covered. The seed-offset mutant (`0x100 + 32` -> `0x100 - 32`) reads
`table[0xE0]`, which is 0x6B, so it changes the first output byte.
"""

from __future__ import annotations

from typing import List

from emu_oracle import ArgSpec

TABLE_SIZE = 512
KEY_LEN = 32
HEADER_LEN = 48


def key_table() -> bytes:
    """The synthetic 512-byte table, with the seed and byte edges pinned."""
    out = bytearray((i * 37 + 11) & 0xFF for i in range(TABLE_SIZE))
    out[0x120] = 162
    out[162] = 0x00
    out[255] = 0xFF
    return bytes(out)


TABLE = key_table()


def make_args(i: int) -> List[ArgSpec]:
    """The i-th input: an untouched header, an output buffer, and the length.

    The header bytes are passed in RCX (the client does not read them). The
    output buffer starts as a different pattern for each case, so the
    differential proves the routine writes all 32 bytes rather than leaving
    any of the input in place.
    """
    header = bytes((i * 7 + j) & 0xFF for j in range(HEADER_LEN))
    out = bytes((i * 29 + j) & 0xFF for j in range(KEY_LEN))
    return [
        ArgSpec(kind="ptr", mem=header),
        ArgSpec(kind="ptr", mem=out),
        ArgSpec(kind="u64", bits=KEY_LEN),
    ]

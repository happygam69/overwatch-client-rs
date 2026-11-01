"""Inputs for the `cell_bytes_fixup` unit (`0x141ff51a0`).

One case is the runtime cell block: the outer `{inner header, mode}` pair, the
inner header, the 0x10-byte entries, the record-pointer slots and the 17-qword
records. Every internal pointer is a *relative* offset in `mem` and listed in
`relocs`, so the oracle rebases it to the arena and the probe rebases it to the
block base the unit passes as its second (candidate-only) argument. The record
qwords are data, not relocations: they are the function's output and are
compared exactly.

The two allocation modes (outer mode 0, entry mode 0) are not generated: the
client allocates a 0x10 object there and then dereferences its zeroed first
qword, and the shipping loader converts the file's entries before this pass.
"""

from __future__ import annotations

import random
import struct

from emu_oracle import ARENA_BASE, ArgSpec

BLOCK_OFF = 0x1000
BLOCK_LEN = 0xC00

HEADER = 0x00
INNER = 0x10
ENTRIES = 0x30
SLOTS = 0x100
RECORDS = 0x400
RECORD_STRIDE = 0x100
MAX_ENTRIES = 8


def _q(block: bytearray, at: int, value: int) -> None:
    struct.pack_into("<Q", block, at, value)


def _u32(block: bytearray, at: int, value: int) -> None:
    struct.pack_into("<I", block, at, value)


def _case(i: int):
    rng = random.Random(0xCE11 + i)
    header_mode = 1 if i % 3 != 0 else 2 + (i % 2)
    entry_mode = 1 if i % 5 != 0 else 2 + (i % 2)
    count = [0, 1, 2, 3, 5, 8][i % 6]
    return header_mode, entry_mode, count, rng


def _block(i: int) -> bytes:
    header_mode, entry_mode, count, rng = _case(i)
    block = bytearray(BLOCK_LEN)
    relocs = []

    # Outer pair: {inner header, mode}. The mode lives at +8 and selects the
    # inner header's layout; the inner pointer is the same in both layouts.
    _q(block, HEADER, INNER)
    _q(block, HEADER + 8, header_mode)
    relocs.append(HEADER)

    if header_mode == 1:
        _q(block, INNER, ENTRIES)
        _u32(block, INNER + 8, count)
        relocs.append(INNER)
    else:
        _u32(block, INNER, count)
        _q(block, INNER + 8, ENTRIES)
        relocs.append(INNER + 8)

    for e in range(count):
        entry = ENTRIES + e * 0x10
        slot = SLOTS + e * 0x20
        record = RECORDS + e * RECORD_STRIDE
        _q(block, entry, slot)
        _q(block, entry + 8, entry_mode)
        relocs.append(entry)
        if entry_mode == 1:
            _q(block, slot, record)
            relocs.append(slot)
        else:
            _q(block, slot, 0xDEADBEEF)
            _q(block, slot + 8, record)
            relocs.append(slot + 8)

        for k in range(17):
            at = record + k * 8
            if (e + k) % 5 == 0:
                value = 0
            elif k == 16 and i % 7 == 0:
                # A wrapping boundary: the client adds without a bounds check.
                value = 0xFFFFFFFFFFFFFFF0
            elif i % 11 == 3:
                value = rng.getrandbits(32)
            else:
                value = 0x20 + k * 8 + e * 0x10
            _q(block, at, value)

    return bytes(block), tuple(relocs)


def make_args(i: int):
    block, relocs = _block(i)
    return [
        ArgSpec(
            kind="ptr",
            mem=block,
            arena_offset=BLOCK_OFF,
            relocs=relocs,
        ),
        # Candidate-only: the block's arena address, so the converted record
        # qwords match the oracle's absolute values byte for byte.
        ArgSpec(kind="u64", bits=ARENA_BASE + BLOCK_OFF),
    ]

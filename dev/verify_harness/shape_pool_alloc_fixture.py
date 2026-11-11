"""`ShapePoolAlloc` (`0x142cb00c0`) inputs: a shape pool's record and its
entries array.

The client pops one index from the pool's free list: the head is at
`pool+0x14`, the next free index is the popped entry's own link dword at
`entry+0xA0`, the used count at `pool+0x08` goes up by one and the high-water
mark at `pool+0x10` becomes the larger of itself and `index + 1`.

The record's base slot holds the entries array's *relative* offset and is
declared as a relocation, so the oracle rebases it to the arena and reaches
the same entries the probe is handed. The grow path (the free head is `-1`)
is not generated: the client allocates there through its runtime allocator,
which cannot run in the image (the heap vtable is null offline).
"""

from __future__ import annotations

import random
import struct

from emu_oracle import ARENA_BASE, ArgSpec

POOL_OFF = 0x1000
ENTRIES_OFF = 0x2000

RECORD_LEN = 0x18
ENTRY_LEN = 0xC0
ENTRY_LINK = 0xA0
MAX_ENTRIES = 6


def _special(i: int):
    """(capacity, used, high, free head, the free list)."""
    table = [
        # one entry, one free index: the smallest possible pop
        (1, 0, 0, 0, [0xFFFFFFFF]),
        # the free head is the last index: the high-water mark must rise
        (4, 2, 2, 3, [0xFFFFFFFF, 0xFFFFFFFF, 0xFFFFFFFF, 0xFFFFFFFF]),
        # a chain: 2 -> 0 -> -1
        (4, 1, 1, 2, [1, 0xFFFFFFFF, 0xFFFFFFFF, 0xFFFFFFFF]),
        # the high-water mark is already above index+1: it must not fall
        (4, 3, 9, 1, [0xFFFFFFFF, 0xFFFFFFFF, 0xFFFFFFFF, 0xFFFFFFFF]),
        # the used count is at its maximum: the increment wraps
        (4, 0xFFFFFFFF, 0, 0, [0xFFFFFFFF, 0xFFFFFFFF, 0xFFFFFFFF, 0xFFFFFFFF]),
    ]
    return table[i % len(table)]


def _case(i: int):
    if i < 5:
        return _special(i)
    rng = random.Random(0x504F_4F4C ^ i)
    capacity = rng.randrange(1, MAX_ENTRIES + 1)
    used = rng.randrange(0, capacity + 1)
    high = rng.randrange(0, capacity + 2)
    free = rng.randrange(0, capacity)
    # A free list from `free` down to -1, the other entries unused.
    chain = []
    nxt = free
    for index in range(capacity):
        if index == free:
            chain.append(nxt)
            nxt = index
        else:
            chain.append(0xFFFFFFFF)
    # `free` links to the next index in the chain, and the last one to -1.
    order = list(range(free, capacity))
    links = [0xFFFFFFFF] * capacity
    for pos, index in enumerate(order):
        links[index] = order[pos + 1] if pos + 1 < len(order) else 0xFFFFFFFF
    return capacity, used, high, free, links


def build(i: int):
    """(the pool record, the entries array, the record's relocations)."""
    capacity, used, high, free, links = _case(i)
    pool = bytearray(RECORD_LEN)
    # The base slot: the entries array's offset *relative to the record*.
    # The harness rebases a relocation as `pointee_address + relative`, so the
    # relative offset is the difference between the two arena offsets.
    struct.pack_into("<Q", pool, 0x00, ENTRIES_OFF - POOL_OFF)
    struct.pack_into("<I", pool, 0x08, used)
    struct.pack_into("<I", pool, 0x0C, capacity)
    struct.pack_into("<I", pool, 0x10, high)
    struct.pack_into("<I", pool, 0x14, free)

    entries = bytearray(capacity * ENTRY_LEN)
    for index, link in enumerate(links):
        struct.pack_into("<I", entries, index * ENTRY_LEN + ENTRY_LINK, link)
    # Data outside the link field, so a spurious write is caught.
    for index in range(capacity):
        struct.pack_into("<Q", entries, index * ENTRY_LEN + 0x00, 0xA5A5_0000 + index)
        struct.pack_into("<I", entries, index * ENTRY_LEN + 0x08, index)
    return bytes(pool), bytes(entries), [0x00]


def make_args(i: int):
    pool, entries, relocs = build(i)
    return [
        ArgSpec(kind="ptr", mem=pool, arena_offset=POOL_OFF, relocs=tuple(relocs)),
        ArgSpec(kind="ptr", mem=entries, arena_offset=ENTRIES_OFF),
        # Candidate-only: the pool record's own address. The harness rebases a
        # relocation as `pointee_address + relative`, so the probe rebases the
        # same slot the same way and both sides reach the same entries.
        ArgSpec(kind="u64", bits=ARENA_BASE + POOL_OFF),
    ]

"""`ShapePoolAlloc` (`0x142cb00c0`) as a **steps** trace: the free list walked
several times on one pool.

The single-call unit (`fn/ShapePoolAlloc`) proves one pop against a pool the
fixture hands over. It cannot see the part that matters for a link list: the
client writes the *next* free index into the popped entry's own link at
`entry+0xA0`, so the second pop must read what the first pop wrote. A pool that
is right on call one and wrong on call two is exactly the bug a per-function
unit misses.

Everything lives in the unit's declared state snapshot at guest addresses, so
the state carries across the steps and the state channel compares it after
every call:

    S + 0x0100 + i*0x20   pool record  i (records are 0x18 bytes, 0x20 slot)
    S + 0x1000 + i*0x480  entries      i (6 entries of 0xC0 = 0x480)

The record is the client's own layout (`domino_world::pool`): the entries base
offset at `+0x00` (relative to the record, which is what the client adds), the
used count at `+0x08`, the capacity at `+0x0C`, the high-water mark at `+0x10`
and the free head at `+0x14`. Each entry carries a link at `+0xA0` and noise
elsewhere, so a spurious write to an entry is caught by the state channel.

Eight pools cover: the chain lengths 1..4, a non-contiguous chain (`2 -> 0`),
a saturated used count and a high-water mark that must not fall. The number of
steps is the chain length, so a case pops until the list is empty - the last
pop leaves the free head at `-1`, which the next call would answer with the
allocator, and that path cannot run offline (the heap vtable is null).

Run `python dev/verify_harness/shape_pool_alloc_steps_fixture.py` to print the
snapshot hex the unit JSON carries, so the two never drift.
"""

from __future__ import annotations

import struct

from emu_oracle import ArgSpec

STATE_BASE = 0x200000000000
SIZE = 0x3400
POOL_BASE = 0x0100
POOL_STRIDE = 0x20
RECORD_LEN = 0x18
ENTRIES_BASE = 0x1000
ENTRIES_STRIDE = 0x480
ENTRY_LEN = 0xC0
ENTRY_LINK = 0xA0
FREE_EMPTY = 0xFFFFFFFF

# (capacity, used, high, the free-list chain, head first). A chain of `n`
# indices means `n` pops, the last of which leaves the head at -1.
CASES = [
    (1, 0, 0, [0]),
    (4, 0, 0, [3, 2, 1, 0]),
    (4, 1, 1, [1, 0]),
    (4, 0xFFFFFFFF, 0, [0]),
    (3, 0, 9, [2, 1, 0]),
    (2, 0, 0, [1, 0]),
    (4, 2, 2, [0]),
    (4, 0, 0, [2, 0]),
]

POOLS = len(CASES)
CAPACITY = 6


def build_snapshot() -> bytes:
    snap = bytearray(SIZE)
    for i, (capacity, used, high, chain) in enumerate(CASES):
        pool_off = POOL_BASE + i * POOL_STRIDE
        ent_off = ENTRIES_BASE + i * ENTRIES_STRIDE
        # The slot is the entries array's absolute guest address: the client
        # uses it as a pointer. The harness rebases a declared relocation into
        # exactly that form (`pointee_address + relative`), which is why the
        # arena fixture can store a relative offset and this one cannot.
        struct.pack_into("<Q", snap, pool_off + 0x00, STATE_BASE + ent_off)
        struct.pack_into("<I", snap, pool_off + 0x08, used)
        struct.pack_into("<I", snap, pool_off + 0x0C, capacity)
        struct.pack_into("<I", snap, pool_off + 0x10, high)
        struct.pack_into("<I", snap, pool_off + 0x14, chain[0])
        for entry in range(CAPACITY):
            base = ent_off + entry * ENTRY_LEN
            struct.pack_into("<Q", snap, base + 0x00, 0xA5A5_0000 + i * 0x100 + entry)
            struct.pack_into("<I", snap, base + 0x08, entry)
        for pos, index in enumerate(chain):
            base = ent_off + index * ENTRY_LEN + ENTRY_LINK
            nxt = chain[pos + 1] if pos + 1 < len(chain) else FREE_EMPTY
            struct.pack_into("<I", snap, base, nxt)
    return bytes(snap)


SNAPSHOT = build_snapshot()


def make_steps(i: int):
    """The steps of corpus case `i`: one call per index in the chain.

    Every step carries the same three arguments - the pool and its entries are
    fixed in the snapshot. What changes between the steps is the state the
    previous call left behind, which is the whole point of the unit.
    """
    index = i % POOLS
    _capacity, _used, _high, chain = CASES[index]
    pool = STATE_BASE + POOL_BASE + index * POOL_STRIDE
    entries = STATE_BASE + ENTRIES_BASE + index * ENTRIES_STRIDE
    step = [
        ArgSpec(kind="u64", bits=pool),
        ArgSpec(kind="u64", bits=entries),
        ArgSpec(kind="u64", bits=CAPACITY * ENTRY_LEN),
    ]
    return [[ArgSpec(kind=a.kind, bits=a.bits) for a in step] for _ in chain]


if __name__ == "__main__":
    print(SNAPSHOT.hex())

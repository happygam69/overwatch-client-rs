"""Inputs for the `fn/KeyTableFind` unit (`0x141b331f0`).

Four arguments: the record block (four `0x30`-byte records), the count, the
`0x28`-byte key, and the flags word whose low byte is the client's `r9b`.

The cases are curated first, because the function's three subtle rules each need
a witness, and then extended with seeded random ones:

1. an exact match returns its index (`rec[0x10]`, `rec[0x18]` and `rec[0]` all
   equal, `rec[8] == key[8]`);
2. a **partial** match - `key[8] != 0` and `key[0] == 0` - makes any record with
   the same `rec[8]` a candidate, and the **last** candidate wins;
3. the `+0x20` comparison only happens when the flags byte is non-zero.

The curated list walks `count` through `-1, 0, 1..4` so the early exit, the
single-record paths and the "last candidate" rule are all reached.
"""

from __future__ import annotations

import struct

from emu_oracle import ArgSpec

STRIDE = 0x30
MAX_RECORDS = 4
ARENA = 0x7000
GAP = 0x400


def record(zero=0, eight=0, ten=0, eighteen=0, twenty=0) -> bytes:
    out = bytearray(STRIDE)
    for slot, value in ((0x00, zero), (0x08, eight), (0x10, ten), (0x18, eighteen), (0x20, twenty)):
        struct.pack_into("<Q", out, slot, value & 0xFFFF_FFFF_FFFF_FFFF)
    return bytes(out)


def key(zero=0, eight=0, ten=0, eighteen=0, twenty=0) -> bytes:
    out = bytearray(0x28)
    for slot, value in ((0x00, zero), (0x08, eight), (0x10, ten), (0x18, eighteen), (0x20, twenty)):
        struct.pack_into("<Q", out, slot, value & 0xFFFF_FFFF_FFFF_FFFF)
    return bytes(out)


def block(records: list[bytes]) -> bytes:
    """Pad to MAX_RECORDS so the pointer length is the same in every case."""
    out = b"".join(records)
    return out + b"\x00" * (STRIDE * MAX_RECORDS - len(out))


CURATED = [
    # (records, count bits, key, flags)
    # empty and negative counts: the early exit
    ([], 0, key(ten=1, eighteen=2), 0),
    ([record(1, 2, 3, 4, 5)], 0xFFFF_FFFF, key(ten=1, eighteen=2), 0),
    # exact match at each position
    ([record(9, 1, 2, 3, 4)], 1, key(9, 1, 2, 3, 4), 0),
    ([record(1, 1, 2, 3, 4), record(9, 1, 2, 3, 4)], 2, key(9, 1, 2, 3, 4), 0),
    ([record(1, 1, 2, 3, 4), record(2, 1, 2, 3, 4), record(9, 1, 2, 3, 4)], 3, key(9, 1, 2, 3, 4), 0),
    # no match: the prefilter fields differ, one at a time
    ([record(9, 1, 2, 3, 4)], 1, key(9, 1, 7, 3, 4), 0),
    ([record(9, 1, 2, 3, 4)], 1, key(9, 1, 2, 7, 4), 0),
    # no match: the key fields agree but rec[0] differs and the key is not partial
    ([record(9, 1, 2, 3, 4)], 1, key(7, 1, 2, 3, 4), 0),
    # partial key: two candidates, the LAST one wins
    ([record(0, 5, 2, 3, 4), record(0, 5, 2, 3, 4)], 2, key(0, 5, 2, 3, 4), 0),
    # partial key whose candidate carries a different rec[0]
    ([record(1, 5, 2, 3, 4)], 1, key(0, 5, 2, 3, 4), 0),
    # the sticky partial: an earlier record sets it, a later one uses it
    ([record(0, 11, 2, 3, 4), record(5, 12, 2, 3, 4), record(9, 12, 2, 3, 4)], 3, key(0, 12, 2, 3, 4), 0),
    # key[8] == 0 disables the partial rule entirely
    ([record(0, 0, 2, 3, 4)], 1, key(5, 0, 2, 3, 4), 0),
    # the +0x20 comparison, flag off then on
    ([record(9, 1, 2, 3, 4)], 1, key(9, 1, 2, 3, 99), 0),
    ([record(9, 1, 2, 3, 4)], 1, key(9, 1, 2, 3, 99), 1),
    ([record(9, 1, 2, 3, 99)], 1, key(9, 1, 2, 3, 99), 1),
    # the flag is the low byte only: 0x100 must not count as set
    ([record(9, 1, 2, 3, 4)], 1, key(9, 1, 2, 3, 99), 0x100),
]


def _rng(seed: int):
    state = [seed]

    def next_u64() -> int:
        state[0] = (state[0] * 6364136223846793005 + 1442695040888963407) & 0xFFFF_FFFF_FFFF_FFFF
        return state[0]

    return next_u64


def make_args(i: int):
    """Arguments for case `i`: the curated list first, then seeded random ones."""
    if i < len(CURATED):
        records, count, k, flags = CURATED[i]
        return [
            ArgSpec(kind="ptr", mem=block(records), arena_offset=ARENA),
            ArgSpec(kind="u64", bits=count),
            ArgSpec(kind="ptr", mem=k, arena_offset=ARENA + GAP),
            ArgSpec(kind="u64", bits=flags),
        ]
    next_u64 = _rng(0x5EED_0000 + (i - len(CURATED)))
    count = next_u64() % 5  # 0..4
    # A small field alphabet so matches actually happen.
    alphabet = [0, 1, 2, 3]
    pick = lambda: alphabet[next_u64() % len(alphabet)]
    recs = [record(pick(), pick(), pick(), pick(), pick()) for _ in range(count)]
    # Half the cases copy a record's fields into the key, which forces a match
    # or a candidate on that line.
    if recs and next_u64() % 2 == 0:
        src = recs[next_u64() % len(recs)]
        fields = [struct.unpack_from("<Q", src, off)[0] for off in (0, 8, 0x10, 0x18, 0x20)]
        if next_u64() % 2 == 0:
            fields[0] = 0  # make the key partial
        k = key(*fields)
    else:
        k = key(pick(), pick(), pick(), pick(), pick())
    flags = 1 if next_u64() % 2 else 0
    return [
        ArgSpec(kind="ptr", mem=block(recs), arena_offset=ARENA),
        ArgSpec(kind="u64", bits=count),
        ArgSpec(kind="ptr", mem=k, arena_offset=ARENA + GAP),
        ArgSpec(kind="u64", bits=flags),
    ]

"""Inputs for `fn/KeyTableFindMask` (`0x141b330c0`).

The client forwards its own four arguments to the search, so every case the
search unit uses is a case here too - plus one the *mask* needs and the search
does not: a table with more than 64 records, where the index reaches bit 6 and
`shl rax, cl` uses `cl` (the low six bits). Without that case a mutant that
drops the `cl` masking is unobservable.
"""

from __future__ import annotations

from emu_oracle import ArgSpec

import keytable_find_fixture as base

WIDE_COUNT = 70
WIDE_AT = 64  # index 64 -> cl = 0 -> 1 << 0 = 1
# The key must not sit inside the record block: 70 records are 0x840
# bytes from ARENA, so the key goes at ARENA + 0x1000.
ARENA = base.ARENA
GAP = 0x1000


def _wide() -> list:
    """70 records, the match at index 64; every other record differs at +0x10."""
    records = [base.record(1, 9, 0x100 + i, 2, 3) for i in range(WIDE_COUNT)]
    records[WIDE_AT] = base.record(0x77, 9, 0x999, 2, 3)
    return [
        ArgSpec(kind="ptr", mem=base.block(records), arena_offset=ARENA),
        ArgSpec(kind="u64", bits=WIDE_COUNT),
        ArgSpec(kind="ptr", mem=base.key(0x77, 9, 0x999, 2, 3), arena_offset=ARENA + GAP),
        ArgSpec(kind="u64", bits=0),
    ]


def make_args(i: int):
    if i == 0:
        return _wide()
    return base.make_args(i - 1)

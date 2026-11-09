"""Deterministic (object, events) cases for `fn/DynamicIndexNormalizeIntervals`.

The client's `DynamicIndexNormalizeIntervals` (`0x1425208d0`) takes the
interval buffer at `[obj+0x128]` with its count at `[obj+0x130]`, sorts the
16-byte events (`{u64 start, i32 delta}`) by start with `0x14251ff30`, then
walks equal-start groups summing their deltas with 32-bit wrap: a group whose
coverage crosses from `<= 0` to `> 0` emits one `+1` event at the group's
start, one that crosses from `> 0` to `<= 0` emits one `-1` event, and every
other group is dropped. It compacts the emitted events forward from the
buffer's base and stores the new count.

The input arrays here are **already sorted by start**, which makes the client's
sort a no-op and therefore the whole buffer deterministic: the bytes past the
new count are the sorted array's own tail, which both sides leave alone. Two
events that share a start carry identical bytes, so an unstable sort cannot
reorder them observably either. (An unsorted input cannot be compared byte for
byte: the client's sort leaves the dropped events in its own order, and
`0x14251ff30` is not ported. That restriction is the unit's declared gap.)

The corpus covers the empty buffer, a single event of each sign, a resident
pair, a non-resident pair (`-0x40000000`), identical and cancelling duplicates,
groups that do not cross, coverage that goes negative and recovers, and 32-bit
wrap-through of the running sum.
"""

from __future__ import annotations

from typing import List, Tuple

from emu_oracle import ARENA_BASE, ArgSpec

OBJ_OFFSET = 0
ARRAY_OFFSET = 0x1000
OBJ_LEN = 0x138
BASE_FIELD = 0x128
COUNT_FIELD = 0x130
NON_RESIDENT = -0x40000000

# Events the client's normalize would emit for each case, computed here so the
# corpus is readable (the harness still compares against the oracle, not this).
Case = Tuple[List[Tuple[int, int]], List[Tuple[int, int]]]


def _cases() -> List[Case]:
    return [
        # Empty buffer: base == end, the sort never runs.
        ([], []),
        # One event each way.
        ([(0, 1)], [(0, 1)]),
        ([(0, -1)], []),
        ([(5, 1)], [(5, 1)]),
        # A resident pair opens and closes the coverage.
        ([(10, 1), (20, -1)], [(10, 1), (20, -1)]),
        # A non-resident pair: the magnitudes do not cross zero.
        ([(10, NON_RESIDENT), (20, -NON_RESIDENT)], []),
        # Identical duplicates: the group sums to +2 and the tail is the
        # identical second event.
        ([(7, 1), (7, 1)], [(7, 1)]),
        # Cancelling duplicates: the group sums to zero and emits nothing.
        ([(7, 1), (7, -1)], []),
        # A group that stays below zero the whole way.
        ([(3, -1), (4, -1), (9, 1)], [(9, 1)]),
        # Coverage goes negative and comes back through zero at one point.
        ([(1, 1), (2, -3), (3, 2), (4, -1)], [(1, 1), (3, 1), (4, -1)]),
        # Two separate open/close pairs.
        ([(1, 1), (2, -1), (3, 1), (4, -1)], [(1, 1), (2, -1), (3, 1), (4, -1)]),
        # A group that closes only part of the coverage: 2 open, then -1 leaves
        # coverage at 1, so nothing is emitted (the boundary "sum <= 0" is not
        # reached while coverage is above zero).
        ([(1, 1), (2, 1), (3, -1)], [(1, 1), (2, 1)]),
        # A group that closes exactly: 2 open, then -2 leaves zero.
        ([(1, 1), (2, 1), (3, -2)], [(1, 1), (2, 1), (3, -1)]),
        # 32-bit wrap: 0x7fffffff twice wraps the sum negative, so the second
        # group closes a coverage that never really closed.
        ([(1, 0x7FFFFFFF), (2, 0x7FFFFFFF)], [(1, 1), (2, -1)]),
        # Wrap the other way: -0x80000000 twice sums back to zero.
        ([(1, -0x80000000), (1, -0x80000000), (2, 1)], [(2, 1)]),
        # A duplicate group that closes coverage: +1 +1 then -1 -1.
        (
            [(1, 1), (1, 1), (5, -1), (5, -1)],
            [(1, 1), (5, -1)],
        ),
        # A long mixed buffer (the loop runs across many groups).
        (
            [(0, 1), (0, 1), (0, 1), (0, -3),
             (10, NON_RESIDENT), (10, -1), (20, -NON_RESIDENT), (20, 1),
             (30, 1), (40, -2), (50, 1)],
            [(0, 1), (10, NON_RESIDENT), (20, -NON_RESIDENT), (40, -1)],
        ),
        # Maximum 32-bit deltas against zero.
        ([(9, 0x7FFFFFFF), (9, 1)], [(9, 1)]),
        # A single group with four identical events.
        ([(2, 1), (2, 1), (2, 1), (2, 1)], [(2, 1)]),
        # Interleaved duplicates at two starts.
        ([(1, 1), (1, 1), (1, -2), (3, 1), (3, 1)], [(3, 1)]),
        # Large starts (the packed `archive << 30 | offset` space).
        (
            [(0x4000000010, 1), (0x4000000020, -1)],
            [(0x4000000010, 1), (0x4000000020, -1)],
        ),
        # Top of the u64 range: no arithmetic on the start, only ordering.
        ([(0xFFFFFFFFFFFFFFF0, 1), (0xFFFFFFFFFFFFFFF0, -1)], []),
    ]


SPECIALS = _cases()


def _events_bytes(events: List[Tuple[int, int]]) -> bytes:
    out = bytearray()
    for start, delta in events:
        out += (start & 0xFFFFFFFFFFFFFFFF).to_bytes(8, "little")
        out += (delta & 0xFFFFFFFF).to_bytes(4, "little")
        out += b"\x00" * 4  # the record's padding, untouched by the client
    return bytes(out)


def _random(r: int) -> List[Tuple[int, int]]:
    """A generated case: n sorted starts with 1..3 deltas drawn from a small
    pool (including the non-resident magnitudes) so groups and crossings vary."""
    pool = [1, -1, 1, -1, 2, -2, NON_RESIDENT, -NON_RESIDENT, 0x7FFFFFFF, 1]
    n = 1 + r % 6
    starts = []
    at = (r * 7) % 0x1000
    for _ in range(n):
        starts.append(at)
        at += 1 + ((r * 13) % 40)
    events: List[Tuple[int, int]] = []
    for index, start in enumerate(starts):
        group = 1 + ((r >> (index + 1)) % 3)
        if group > 1:
            # Identical duplicates only: an unstable sort may reorder equal
            # keys, and identical records make that invisible.
            delta = pool[(r + index) % len(pool)]
            for _ in range(group):
                events.append((start, delta))
        else:
            events.append((start, pool[(r + index * 3) % len(pool)]))
    return events


def make_case(i: int) -> List[Tuple[int, int]]:
    if i < len(SPECIALS):
        return SPECIALS[i][0]
    return _random(i - len(SPECIALS))


def make_obj(events: List[Tuple[int, int]]) -> bytes:
    obj = bytearray(OBJ_LEN)
    obj[BASE_FIELD : BASE_FIELD + 8] = (ARENA_BASE + ARRAY_OFFSET).to_bytes(8, "little")
    obj[COUNT_FIELD : COUNT_FIELD + 4] = len(events).to_bytes(4, "little")
    obj[COUNT_FIELD + 4 : COUNT_FIELD + 8] = (len(events) + 0x10).to_bytes(4, "little")
    return bytes(obj)


def make_args(i: int) -> List[ArgSpec]:
    events = make_case(i)
    return [
        ArgSpec(kind="ptr", mem=make_obj(events), arena_offset=OBJ_OFFSET),
        ArgSpec(kind="ptr", mem=_events_bytes(events), arena_offset=ARRAY_OFFSET),
    ]

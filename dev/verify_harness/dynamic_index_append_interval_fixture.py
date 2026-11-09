"""Deterministic (object, events, entry, resident) cases for
`fn/DynamicIndexAppendInterval`.

The client's `DynamicIndexAppendInterval` (`0x142520700`) takes the key-state
object in RCX, a pointer to a 16-byte chosen entry `{u64 start, u32 len}` in
RDX, and the resident flag in R8B. It writes two 16-byte events
`{u64 start, i32 delta}` into the buffer at `[obj+0x128]` at the old count
and old count + 1, incrementing the count at `[obj+0x130]` twice:
`(start, +1)` and `(start + (u32)len, -1)` when resident, or
`(start, -0x40000000)` and `(start + (u32)len, +0x40000000)` when not, in
which case it tail-calls `DynamicIndexNormalizeIntervals` (`0x1425208d0`).
The end is a 64-bit wrapping add of the 32-bit length; the delta negation is
a 32-bit `neg`. The padding byte of each record is never written.

The capacity-grow path (`capacity <= count + 1` at `0x14252073a`) calls a
log function and the allocator vtable at `[obj]`, so it is the unit's
declared gap: every case here pins `capacity > count + 1` (the tightest is
`count + 2`, one past the `jb` boundary), and the probe rejects a case that
would grow.

For non-resident cases the appended pair sits strictly after every existing
start and `end > start`, so the appended events keep the buffer sorted and
the normalize's sort is a no-op - the same restriction
`fn/DynamicIndexNormalizeIntervals` declares. Resident appends never sort,
so resident cases may carry unsorted existing events, duplicate starts,
zero lengths and wrapping ends.

Surface: 3 register args (RCX obj, RDX entry, R8B resident) - not 4.
The event buffer is not a register argument; the client reaches it via
`[obj+0x128]`. It is therefore embedded inside the obj pointee: one
0x2000-byte blob holds the 0x138-byte object header at 0, 0xA5 sentinel
bytes 0x138..0x1000, and the 4096-byte event buffer at 0x1000. The header's
base field at +0x128 holds the relative offset 0x1000 (u64); the ArgSpec
`relocs=(0x128,)` rebases it to blob_addr+0x1000 on the oracle side, and
the compared-memory channel masks that slot on both sides (so the Rust
side can keep the relative value). Count is at +0x130, capacity at
+0x134. The 16-byte entry is a separate pointee at arena offset 0x2000,
and resident is a u64 register arg (1/0) in R8.

Lengths and counts stay tied: the count never exceeds the buffer, and
`capacity` is always `count + 2` or more.
"""

from __future__ import annotations

from typing import List, Tuple

from emu_oracle import ArgSpec

OBJ_OFFSET = 0
ARRAY_OFFSET = 0x1000
ENTRY_OFFSET = 0x2000
ARRAY_LEN = 0x1000
OBJ_LEN = 0x138
BASE_FIELD = 0x128
COUNT_FIELD = 0x130
CAPACITY_FIELD = 0x134
NON_RESIDENT = -0x40000000
U64 = 0xFFFFFFFFFFFFFFFF

# (existing events, start, len, resident, capacity)
Case = Tuple[List[Tuple[int, int]], int, int, bool, int]


def _cap(count: int, slack: int = 16) -> int:
    return count + slack


def _cases() -> List[Case]:
    cases: List[Case] = []

    def add(events, start, length, resident, capacity=None):
        cases.append(
            (list(events), start, length, resident,
             _cap(len(events)) if capacity is None else capacity)
        )

    # Empty buffer, resident: a plain open/close pair.
    add([], 0x10, 0x10, True)
    # Empty, resident, zero length: both events at the same start, +1/-1.
    add([], 0, 0, True)
    add([], 0x4000000010, 0x20, True)  # archive 1, packed space
    # Empty, non-resident: the magnitudes sum to zero, normalize drops both.
    add([], 0x10, 0x10, False)
    # (No non-resident zero-length case: its two events share a start with
    # different deltas, and the client's unstable sort could swap them in
    # the tail - the same restriction fn/DynamicIndexNormalizeIntervals
    # declares for non-identical duplicates.)
    add([], 0, 1, False)
    # Resident zero length over an existing event at the same start.
    add([(0x10, 1)], 0x10, 0, True)
    # Resident length wrapping the u64 end: start near the top, end wraps low.
    add([], 0xFFFFFFFFFFFFFFF0, 0x20, True)
    add([], U64, 1, True)  # end is exactly 0
    add([], U64, 0xFFFFFFFF, True)  # longest u32 length, wraps to U64-1+... low
    # Resident start at the archive boundary (archive 3 top -> archive 4 low).
    add([], (3 << 30) | 0x3FFFFFF0, 0x20, True)
    add([], (1 << 30), 0x3FFFFFFF, True)  # one below the next archive
    # Duplicate starts: an existing event at the appended start (resident
    # appends never sort, so this is exact).
    add([(0x10, 1), (0x10, 1)], 0x10, 8, True)
    add([(0x10, 1), (0x20, -1)], 0x10, 8, True)
    # Unsorted existing events, resident: proves the resident path does not
    # sort (a sort would reorder the first two records observably).
    add([(0x30, 1), (0x10, 1), (0x20, -1)], 0x40, 4, True)
    # Non-resident append after a sorted resident pair: normalize keeps the
    # pair and drops the non-resident pair (coverage never crosses zero).
    add([(0x10, 1), (0x20, -1)], 0x30, 5, False)
    # Non-resident append over open coverage: closes it, then reopens it.
    add([(0x10, 1)], 0x20, 5, False)
    # Non-resident append with coverage already negative after the pair.
    add([(0x10, NON_RESIDENT), (0x20, -NON_RESIDENT)], 0x30, 5, False)
    # Capacity boundary, tightest no-grow slack: capacity == count + 2, so
    # count + 1 < capacity by exactly one (the `jb` at 0x14252073a).
    add([], 0x10, 4, True, capacity=2)
    add([(0x10, 1)], 0x20, 4, True, capacity=3)
    # Count at the client's initial capacity 16, one below the grow edge:
    # count + 1 == 15 < 16. (counts 15/16 at capacity 16 would grow.)
    add([(i * 0x10, 1 if i % 2 == 0 else -1) for i in range(14)],
        0x1000, 0x20, True, capacity=16)
    add([(i * 0x10, 1) for i in range(17)], 0x1000, 0x20, True, capacity=64)
    # A long existing run (the loop/write indices run far from zero).
    add([(i * 8, 1 if i % 2 == 0 else -1) for i in range(64)],
        0x1000, 0x40, True)
    add([(i * 8, 1 if i % 2 == 0 else -1) for i in range(32)],
        0x1000, 0x40, False)
    # Non-resident append whose end lands exactly on a following... no
    # following event exists; end == start + len is the last record.
    add([(0x10, 1), (0x18, -1)], 0x20, 0x10, False)
    # Max-length resident range: end = start + 0xffffffff (no wrap here).
    add([], 0x1000, 0xFFFFFFFF, True)
    # Start below, end above a duplicate group (resident, unsorted tail).
    add([(0x50, -1), (0x50, 1)], 0x40, 0x30, True)
    return cases


SPECIALS = _cases()


def _random(r: int) -> Case:
    """A generated case. Resident: arbitrary starts/lengths including wrap.
    Non-resident: sorted existing events with the appended pair strictly
    after them, so normalize's sort stays a no-op (the declared gap)."""
    resident = (r % 2) == 0
    count = r % 20
    if resident:
        events = []
        for i in range(count):
            start = (r * 17 + i * 29) % 0x1000
            events.append((start, 1 if (i + r) % 2 == 0 else -1))
        start = (r * 131) % 0x2000
        if r % 7 == 0:
            start = U64 - (r % 0x40)  # near the top: len wraps the end
        length = (r * 41) % 0x100
        if r % 11 == 0:
            length = 0
        if r % 13 == 0:
            length = 0xFFFFFFFF
    else:
        events = []
        at = (r * 7) % 0x100
        for i in range(count):
            events.append((at, 1 if i % 2 == 0 else -1))
            at += 8 + ((r * 3 + i) % 16)
        start = at + 1 + (r % 17)
        length = 1 + ((r * 5) % 0x40)
    capacity = count + 2 + (r % 24)
    return (events, start & U64, length, resident, capacity)


def make_case(i: int) -> Case:
    if i < len(SPECIALS):
        return SPECIALS[i]
    return _random(i - len(SPECIALS))


def make_obj(count: int, capacity: int) -> bytes:
    obj = bytearray(OBJ_LEN)
    # Relative offset only; relocs=(BASE_FIELD,) rebases it on the
    # oracle side. Rust ignores this slot.
    obj[BASE_FIELD : BASE_FIELD + 8] = ARRAY_OFFSET.to_bytes(8, "little")
    obj[COUNT_FIELD : COUNT_FIELD + 4] = count.to_bytes(4, "little")
    obj[CAPACITY_FIELD : CAPACITY_FIELD + 4] = capacity.to_bytes(4, "little")
    return bytes(obj)


def _events_bytes(events: List[Tuple[int, int]]) -> bytes:
    out = bytearray(b"\xA5" * ARRAY_LEN)
    for index, (start, delta) in enumerate(events):
        at = index * 16
        out[at : at + 8] = (start & U64).to_bytes(8, "little")
        out[at + 8 : at + 12] = (delta & 0xFFFFFFFF).to_bytes(4, "little")
        out[at + 12 : at + 16] = b"\xCC" * 4  # padding the client never writes
    return bytes(out)


def _entry_bytes(start: int, length: int) -> bytes:
    out = bytearray(b"\xCC" * 16)  # bytes 12..16: never read, must stay put
    out[0:8] = (start & U64).to_bytes(8, "little")
    out[8:12] = (length & 0xFFFFFFFF).to_bytes(4, "little")
    return bytes(out)


def make_args(i: int) -> List[ArgSpec]:
    events, start, length, resident, capacity = make_case(i)
    blob = bytearray(b"\xA5" * 0x2000)
    blob[0:OBJ_LEN] = make_obj(len(events), capacity)
    blob[ARRAY_OFFSET : ARRAY_OFFSET + ARRAY_LEN] = _events_bytes(events)
    return [
        ArgSpec(kind="ptr", mem=bytes(blob), arena_offset=OBJ_OFFSET, relocs=(BASE_FIELD,)),
        ArgSpec(kind="ptr", mem=_entry_bytes(start, length), arena_offset=ENTRY_OFFSET),
        ArgSpec(kind="u64", bits=1 if resident else 0),
    ]

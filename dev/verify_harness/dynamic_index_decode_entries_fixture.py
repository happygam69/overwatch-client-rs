"""Deterministic (entries, indirect, contiguous, initial state) cases for
`fn/DynamicIndexDecodeEntries`.

The client's `DynamicIndexDecodeEntries` (`0x1425209a0`) takes the key-state
object in RCX, an 18-byte entry range `[RDX, R8)`, a pointer array
`[R9, stack1)` whose entries point at 19-byte payload records, and a
contiguous 19-byte record range `[stack2, stack3)`. It clears the interval
count at `[obj+0x130]`, decodes the entries (one record is sink type 1,
more are type 2 then type 6), then the pointed-at records in pointer order
with their own type byte, then the contiguous records, and tail-jumps to
`DynamicIndexNormalizeIntervals` (`0x1425208d0`). A null or empty range of
any source is skipped; the found flag at `+0x10` and the chosen entry at
`+0x18` are preserved until a sink changes them.

Surface: one 0x4000-byte blob plus six u64 register/stack values holding
absolute arena addresses (`ARENA_BASE + offset`, or 0 for null). The blob
holds the key-state header at 0, a disabled logger (level 0 at its +8, so
the sink's type-4/5 log call returns at once) at 0x400, the 256-record
event buffer at 0x1000, the entries at 0x2000, the pointer array at 0x2200,
the records it points at (32-byte stride) at 0x2400 and the contiguous
records at 0x2800. The header's logger pointer (+0x08) and event base
(+0x128) are relative offsets rebased by `relocs=(0x08, 0x128)`; the
pointer array holds absolute arena addresses, which are fixed.

Every case keeps all appended interval starts and ends distinct (the
initial chosen entry's too, for a type-4 append): the client's unstable
sort then has a unique order, so the normalized tail is deterministic -
the same restriction `fn/DynamicIndexNormalizeIntervals` declares. The
capacity is pinned at 256, past every append a case can do (the grow path
is the unit's declared gap). Lengths and counts stay tied: each range's
end is its begin plus the exact stride times its record count.
"""

from __future__ import annotations

from typing import List, Optional, Tuple

from emu_oracle import ARENA_BASE, ArgSpec

BLOB_LEN = 0x4000
LOGGER_OFFSET = 0x400
EVENTS_OFFSET = 0x1000
EVENTS_LEN = 0x1000
ENTRIES_OFFSET = 0x2000
PTR_ARRAY_OFFSET = 0x2200
PAYLOADS_OFFSET = 0x2400
CONTIGUOUS_OFFSET = 0x2800
CAPACITY = 256
U64 = 0xFFFFFFFFFFFFFFFF

# (entries, indirect, contiguous, found, chosen loc, chosen size,
#  entries_end_delta)
# A source of None is null, [] is empty, otherwise its records.
# entries: (loc, size); indirect/contiguous: (loc, size, type).
# entries_end_delta is None for a normal range (end = begin + 18*len);
# an int overrides the end as begin + delta (a negative delta makes an
# inverted range, end < begin, for the 0x142520ad3 taken edge).
Case = Tuple[
    Optional[List[Tuple[int, int]]],
    Optional[List[Tuple[int, int, int]]],
    Optional[List[Tuple[int, int, int]]],
    bool,
    int,
    int,
    Optional[int],
]

SENTINEL_CHOSEN = (0x1111111111111111, 0x22222222)


def _cases() -> List[Case]:
    cases: List[Case] = []

    def add(entries, indirect, contiguous, found=True, chosen=SENTINEL_CHOSEN,
            entries_end_delta=None):
        cases.append((entries, indirect, contiguous, found, chosen[0],
                      chosen[1], entries_end_delta))

    # Everything null/empty: only the count clear and the final normalize.
    # The found flag and chosen entry are preserved untouched.
    add(None, None, None)
    # All three ranges empty (begin == end, non-null).
    add([], [], [])
    # One 18-byte entry: sink type 1 (found, chosen, clear, append).
    add([(0x1000, 0x40)], None, None)
    # Two entries: type 2 for the first, type 6 for the second.
    add([(0x1000, 0x40), (0x1100, 0x40)], None, None, found=False)
    # Three entries: the type-6 loop runs twice.
    add([(0x1000, 0x40), (0x1100, 0x40), (0x1200, 0x40)], None, None)
    # Out-of-order entry starts: the final normalize sorts, the appends
    # themselves are in file order.
    add([(0x3000, 0x40), (0x1000, 0x40), (0x2000, 0x40)], None, None)
    # Overlapping resident intervals: coverage never drops, so the merge
    # emits only the outer pair and the tail carries the inner records.
    add([(0x1000, 0x200), (0x1100, 0x200)], None, None)
    # A wrapping interval end: start near the u64 top, end wraps low.
    add([(0xFFFFFFFFFFFFFF00, 0x100)], None, None)
    # One indirect record of each sink type 1..7, in decode order, over
    # disjoint intervals. Type 3 zeroes the chosen entry, so the type-4
    # record appends the zero entry; type 7 normalizes immediately.
    add(None, [
        (0x1000, 0x40, 1), (0x1100, 0x40, 2), (0x1200, 0x40, 3),
        (0x1300, 0x40, 4), (0x1400, 0x40, 5), (0x1500, 0x40, 6),
        (0x1600, 0x40, 7),
    ], None)
    # Ignored types: 0, 8 and 255 change nothing (found/chosen preserved).
    add(None, [(0x1000, 0x40, 0), (0x1100, 0x40, 8), (0x1200, 0x40, 255)], None,
        found=False)
    # Type 4 alone appends the preserved chosen entry, log call included.
    add(None, [(0x9999, 0x10, 4)], None, found=False)
    # Type 5 alone clears (already empty) and logs; nothing is appended.
    add(None, [(0x9999, 0x10, 5)], None)
    # Type 7 alone: a non-resident pair that the immediate normalize drops.
    add(None, [(0x1000, 0x40, 7)], None, found=False)
    # Type 3 alone clears found and the chosen entry.
    add(None, [(0x1000, 0x40, 3)], None)
    # A type-2 indirect record replaces the chosen entry after the entries.
    add([(0x1000, 0x40)], [(0x1100, 0x80, 2)], None)
    # Contiguous records: type 6 then type 7 (immediate normalize drops the
    # non-resident pair, the resident pair survives the final normalize).
    add(None, None, [(0x1000, 0x40, 6), (0x1100, 0x40, 7)])
    # Contiguous types 1..5 in order, disjoint intervals.
    add(None, None, [
        (0x1000, 0x40, 1), (0x1100, 0x40, 2), (0x1200, 0x40, 3),
        (0x1300, 0x40, 4), (0x1400, 0x40, 5),
    ])
    # All three sources together, in the client's decode order.
    add([(0x1000, 0x40), (0x1100, 0x40)],
        [(0x1200, 0x40, 6), (0x1300, 0x40, 1)],
        [(0x1400, 0x40, 6), (0x1500, 0x40, 7)])
    # Null entries with the other sources present.
    add(None, [(0x1000, 0x40, 6)], [(0x1100, 0x40, 6)])
    # An empty (non-null) entry range with the other sources present.
    add([], [(0x1000, 0x40, 6)], [(0x1100, 0x40, 6)])
    # Null pointer array with entries and contiguous records present.
    add([(0x1000, 0x40)], None, [(0x1100, 0x40, 6)])
    # An empty pointer array with entries and contiguous records present.
    add([(0x1000, 0x40)], [], [(0x1100, 0x40, 6)])
    # Null contiguous range with entries and indirect records present.
    add([(0x1000, 0x40)], [(0x1100, 0x40, 6)], None)
    # An empty contiguous range with entries and indirect records present.
    add([(0x1000, 0x40)], [(0x1100, 0x40, 6)], [])
    # Two indirect records: the pointer loop runs twice.
    add(None, [(0x1000, 0x40, 6), (0x1100, 0x40, 6)], None)
    # Three contiguous records: the loop runs, then exits on equality.
    add(None, None, [(0x1000, 0x40, 6), (0x1100, 0x40, 6), (0x1200, 0x40, 6)])
    # A non-resident interval overlapping resident coverage.
    add([(0x1000, 0x200)], [(0x1100, 0x40, 7)], None)
    # Zero-size chosen entry preserved through a type-6 append.
    add(None, [(0x1000, 0x40, 6)], None, found=True, chosen=(0x5000, 0))
    # Zero chosen entry with a type-4 record: appends (0, 0) - that pair
    # shares a start with different deltas, so use type 5 instead, which
    # appends nothing, and a type-1 record carrying the zero-size entry.
    add(None, [(0x1000, 0x40, 5)], None, found=False, chosen=(0, 0))
    # Many entries (the count division and the loops run far from zero).
    add([(0x1000 + i * 0x100, 0x40) for i in range(8)], None, None)
    add(None,
        [(0x1000 + i * 0x100, 0x40, 6) for i in range(5)],
        [(0x2000 + i * 0x100, 0x40, 6) for i in range(4)])
    # Adjacent disjoint intervals: the first interval's end sits strictly
    # between the two starts.
    add([(0x1000, 0x100), (0x1200, 0x100)], None, None)
    # Inverted entries range (end = begin - 36): the signed count is -2,
    # whose low dword is > 1 unsigned, so the client falls through to
    # the count>1 path, decodes the first entry as type 2, then the
    # pre-loop check at 0x142520ad3 (begin+18 >= end, unsigned) is
    # taken and the type-6 loop body never runs. This is the only way
    # to take that edge: a monotonic range with count >= 2 has
    # end-begin >= 36, so begin+18 < end and the edge is not taken.
    add([(0x1000, 0x40)], [(0x1100, 0x40, 6)], None, entries_end_delta=-36)
    return cases


SPECIALS = _cases()


def _random(r: int) -> Case:
    used = set()

    def fresh(size: int) -> Tuple[int, int]:
        for _ in range(64):
            loc = ((r * 0x37 + len(used) * 0x101) % 0x4000) + 0x1000
            if loc not in used and (loc + size) not in used:
                used.add(loc)
                used.add(loc + size)
                return loc, size
        raise AssertionError("no fresh interval")

    # The initial chosen entry may be appended by a type-4 record; keep
    # its interval out of the way from the start.
    chosen_loc = 0x9000 + (r % 0x100) * 0x100
    used.add(chosen_loc)
    used.add(chosen_loc + 0x30)

    entries = None
    if r % 7:
        entries = [fresh(1 + (r * 11 + i * 7) % 0x80) for i in range(r % 5)]
    indirect = None
    if r % 5:
        indirect = []
        for i in range(r % 4):
            loc, size = fresh(1 + (r * 13 + i * 5) % 0x80)
            indirect.append((loc, size, (r + i * 3) % 10))
    contiguous = None
    if r % 3:
        contiguous = []
        for i in range(r % 4):
            loc, size = fresh(1 + (r * 17 + i * 3) % 0x80)
            contiguous.append((loc, size, (r * 2 + i * 7) % 10))
    return (entries, indirect, contiguous, (r % 2) == 0, chosen_loc, 0x30, None)


def make_case(i: int) -> Case:
    if i < len(SPECIALS):
        return SPECIALS[i]
    return _random(i - len(SPECIALS))


def _entry_bytes(loc: int, size: int, fill: int) -> bytes:
    out = bytearray(bytes([fill]) * 18)
    out[9:14] = (loc & ((1 << 40) - 1)).to_bytes(5, "big")
    out[14:18] = size.to_bytes(4, "little")
    return bytes(out)


def _record_bytes(loc: int, size: int, kind: int, fill: int) -> bytes:
    out = bytearray(bytes([fill]) * 19)
    out[9:14] = (loc & ((1 << 40) - 1)).to_bytes(5, "big")
    out[14:18] = size.to_bytes(4, "little")
    out[18] = kind & 0xFF
    return bytes(out)


def make_args(i: int) -> List[ArgSpec]:
    entries, indirect, contiguous, found, chosen_loc, chosen_size, entries_end_delta = (
        make_case(i)
    )
    blob = bytearray(b"\xA5" * BLOB_LEN)
    # Key-state header: logger (reloc 0x08) and events base (reloc 0x128)
    # are relative offsets; the disabled logger's level at 0x408 is 0.
    blob[LOGGER_OFFSET : LOGGER_OFFSET + 32] = bytes(32)
    blob[0x08:0x10] = LOGGER_OFFSET.to_bytes(8, "little")
    blob[0x10] = 1 if found else 0
    blob[0x18:0x20] = chosen_loc.to_bytes(8, "little")
    blob[0x20:0x24] = chosen_size.to_bytes(4, "little")
    blob[0x24:0x28] = bytes(4)  # the trailing dword is stack-zero in the oracle
    blob[0x128:0x130] = EVENTS_OFFSET.to_bytes(8, "little")
    blob[0x130:0x134] = (77).to_bytes(4, "little")  # cleared before any read
    blob[0x134:0x138] = CAPACITY.to_bytes(4, "little")
    # Events: a per-slot sentinel record (distinct start, uniform padding)
    # so an untouched slot is recognisable and a moved pad is not.
    for slot in range(EVENTS_LEN // 16):
        at = EVENTS_OFFSET + slot * 16
        blob[at : at + 8] = (0xA5A5A5A5A5A5A5A5 ^ slot).to_bytes(8, "little")
        blob[at + 8 : at + 12] = (0xA5A5A5A5).to_bytes(4, "little")
        blob[at + 12 : at + 16] = b"\xCC" * 4
    for index, (loc, size) in enumerate(entries or []):
        at = ENTRIES_OFFSET + index * 18
        blob[at : at + 18] = _entry_bytes(loc, size, index + 1)
    for index, (loc, size, kind) in enumerate(indirect or []):
        blob[PTR_ARRAY_OFFSET + index * 8 : PTR_ARRAY_OFFSET + index * 8 + 8] = (
            ARENA_BASE + PAYLOADS_OFFSET + index * 32
        ).to_bytes(8, "little")
        at = PAYLOADS_OFFSET + index * 32
        blob[at : at + 19] = _record_bytes(loc, size, kind, index + 1)
        blob[at + 19 : at + 32] = b"\xCC" * 13
    for index, (loc, size, kind) in enumerate(contiguous or []):
        at = CONTIGUOUS_OFFSET + index * 19
        blob[at : at + 19] = _record_bytes(loc, size, kind, index + 1)

    def rng(source, offset, stride):
        if source is None:
            return 0, ARENA_BASE + offset
        return ARENA_BASE + offset, ARENA_BASE + offset + stride * len(source)

    e_lo, e_hi = rng(entries, ENTRIES_OFFSET, 18)
    if entries_end_delta is not None:
        # Inverted range: end is begin + delta (< begin), not begin + 18*len.
        e_hi = ARENA_BASE + ENTRIES_OFFSET + entries_end_delta
    p_lo, p_hi = rng(indirect, PTR_ARRAY_OFFSET, 8)
    c_lo, c_hi = rng(contiguous, CONTIGUOUS_OFFSET, 19)
    return [
        ArgSpec(kind="ptr", mem=bytes(blob), arena_offset=0, relocs=(0x08, 0x128)),
        ArgSpec(kind="u64", bits=e_lo),
        ArgSpec(kind="u64", bits=e_hi),
        ArgSpec(kind="u64", bits=p_lo),
        ArgSpec(kind="u64", bits=p_hi),
        ArgSpec(kind="u64", bits=c_lo),
        ArgSpec(kind="u64", bits=c_hi),
    ]

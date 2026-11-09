"""Deterministic (entries, pages, query) cases for `fn/DynamicIndexFindEntry`.

The client's `DynamicIndexFindEntry` (`0x1425111a0`) takes the segment in
RCX, the 9-byte key in RDX, the key state in R8, an out page word in R9 and
an out record word on the stack. It lower-bound walks the segment's
18-byte entries (count at segment +0x60, base at +0x58), expands the equal
range, scans the page table (base at +0x68, page count at +0x70) in
page/record order stopping at the first zero stored hash, collects the
matching 19-byte payloads, and calls `DynamicIndexDecodeEntries`
(`0x1425209a0`) with the equal range and the collected payloads. The out
words are the first zero's (page, record), (page_count, 21) for a full
table, and (0, 0) for a zero page count; each is written only when its
pointer is non-null.

Surface: one 0x6000-byte blob plus four u64 arena addresses (key, key
state, out page, out record; 0 for a null out word). The blob holds the
segment at 0 (entries base, entry count, page-table base, page count),
the allocator slot at 0x1000, the key state at 0x2000 (fresh, as
`DynamicIndexFindContextInit` builds it: found 0, zero chosen, inline
interval buffer at +0x28, capacity 16 at +0x134), the entries at 0x3000,
the pages at 0x4000, the disabled logger at 0x5200, the key at 0x5600 and
the out words at 0x5700. The segment's and state's internal pointers
travel as relocs inside the blob, as in the decode unit.

Every case keeps decode appends within the fresh capacity of 16 events
(at most eight appends) and page matches within the stack pointer
array's 32: the grow paths through the allocator vtable are the unit's
declared gaps. All appended interval starts and ends are distinct, so
the client's unstable normalize sort has a unique order.
"""

from __future__ import annotations

from typing import List, Optional, Tuple

from emu_oracle import ARENA_BASE, ArgSpec

BLOB_LEN = 0x6000
CTX = 0x2000
ENTRIES = 0x3000
PAGES = 0x4000
LOGGER = 0x5200
KEY_AT = 0x5600
OUT_PAGE = 0x5700
OUT_RECORD = 0x5708


def K(n: int) -> bytes:
    return bytes([n & 0xFF]) * 9


# (entries, page_count, pages, key, out_page, out_record, found, chosen)
# entries: [(key9, loc, size)]; pages: [(stored, key9, loc, size, type)]
# in slot order (a zero stored hash is a slot with no record, so a short
# list's next slot is the first zero); out_*: whether the pointer is
# non-null; found/chosen: the caller-built key state (fresh zero unless
# stated, for the type-4 case whose interval must not share a start).
Case = Tuple[List[Tuple[bytes, int, int]], int, List[Tuple[int, bytes, int, int, int]],
             bytes, bool, bool, bool, Tuple[int, int]]


def _cases() -> List[Case]:
    cases: List[Case] = []

    def add(entries, page_count, pages, key, out_page=True, out_record=True,
            found=False, chosen=(0, 0)):
        cases.append((entries, page_count, pages, key, out_page, out_record,
                      found, chosen))

    def e(n, loc, size=0x40):
        return (K(n), loc, size)

    # Empty entries, zero pages: the walk is skipped, the page scan is
    # skipped, the decode is the empty range; out words (0, 0).
    add([], 0, [], K(1))
    # Same, with both out pointers null.
    add([], 0, [], K(1), out_page=False, out_record=False)
    # One record, equal: mid is 0 (the backward je is taken).
    add([e(10, 0x1000)], 0, [], K(10))
    # One record, key before and after.
    add([e(10, 0x1000)], 0, [], K(5))
    add([e(10, 0x1000)], 0, [], K(15))
    # Five records: first, middle and last equal; before, between, after.
    five = [e(10, 0x1000), e(20, 0x1100), e(30, 0x1200), e(40, 0x1300),
            e(50, 0x1400)]
    add(five, 0, [], K(10))
    add(five, 0, [], K(30))
    add(five, 0, [], K(50))
    add(five, 0, [], K(5))
    add(five, 0, [], K(25))
    add(five, 0, [], K(60))
    # Duplicates: three equal entries in the middle (both expansion
    # loops run more than one step) and a duplicate pair at the edges.
    add([e(10, 0x1000), e(20, 0x1100), e(20, 0x1200), e(20, 0x1300),
         e(30, 0x1400)], 0, [], K(20))
    add([e(20, 0x1000), e(20, 0x1100)], 0, [], K(20))
    add([e(20, 0x1000), e(20, 0x1100), e(30, 0x1200)], 0, [], K(20))
    # Unsorted entries: the walk is the client's memcmp walk, not a sort.
    add([e(30, 0x1000), e(10, 0x1100), e(20, 0x1200)], 0, [], K(10))
    add([e(30, 0x1000), e(10, 0x1100), e(20, 0x1200)], 0, [], K(25))
    # Keys sharing the first eight bytes: the backward and forward walks
    # stop on the ninth byte, in both directions.
    ka = b"\x05" * 8 + b"\x00"
    kb = b"\x05" * 8 + b"\x01"
    kc = b"\x05" * 8 + b"\x02"
    add([(ka, 0x1000, 0x40), (kb, 0x1100, 0x40), (kc, 0x1200, 0x40)], 0, [], kb)
    add([(ka, 0x1000, 0x40), (kb, 0x1100, 0x40)], 0, [], kb)
    add([(ka, 0x1000, 0x40), (kb, 0x1100, 0x40)], 0, [], ka)
    # Out-pointer null combinations, with a hit and a miss.
    add(five, 0, [], K(30), out_page=False, out_record=True)
    add(five, 0, [], K(30), out_page=True, out_record=False)
    add(five, 0, [], K(25), out_page=False, out_record=False)
    # Pages: one page whose first slot is zero, and a page count with no
    # records at all in two pages.
    add([], 1, [], K(1))
    add([], 2, [], K(1))
    add(five, 1, [], K(30))
    # Page scan compares: first-byte mismatch, then qword mismatch after
    # a matching first byte, then ninth-byte mismatch after a matching
    # qword, then a type-6 match, then the zero that stops the scan.
    q = K(30)
    pages = [
        (0x80000001, K(31), 0x2000, 0x40, 6),
        (0x80000002, b"\x1e" + b"\x00" * 8, 0x2100, 0x40, 6),
        (0x80000003, b"\x1e" * 8 + b"\x1f", 0x2200, 0x40, 6),
        (0x80000004, q, 0x2300, 0x40, 6),
    ]
    add([], 1, pages, q)
    add(five, 1, pages, q)
    # Page types 1, 2, 3 and 7 over disjoint intervals, then a zero.
    add([], 1, [
        (0x80000001, q, 0x2000, 0x40, 1),
        (0x80000002, q, 0x2100, 0x40, 2),
        (0x80000003, q, 0x2200, 0x40, 3),
        (0x80000004, q, 0x2300, 0x40, 7),
    ], q)
    # Type 5 alone clears; type 4 alone appends the caller-built chosen
    # entry (kept clear of every other interval so starts stay distinct).
    add([], 1, [(0x80000001, q, 0x2000, 0x40, 5)], q)
    add([], 1, [(0x80000001, q, 0x2000, 0x40, 4)], q,
        found=False, chosen=(0x9000, 0x30))
    # A full page of non-matching records: out (1, 21).
    add([], 1, [(0x80000001 + i, K(40 + i), 0x2000 + i * 0x100, 0x40, 6)
                for i in range(21)], K(30))
    # Zero at page 1, record 2: a full page 0 then two records.
    add([], 2, [(0x80000001 + i, K(40 + i), 0x2000 + i * 0x100, 0x40, 6)
                for i in range(21)] + [
        (0x80000101, K(61), 0x4000, 0x40, 6),
        (0x80000102, q, 0x4100, 0x40, 6)], K(30))
    # Match in the last slot of the last page, table full: out (2, 21).
    full = [(0x80000001 + i, K(40 + (i % 20)), 0x2000 + i * 0x100, 0x40, 6)
            for i in range(41)] + [(0x80000101, q, 0x6000, 0x40, 6)]
    add([], 2, full, q)
    # Entries and pages together, in the client's decode order.
    add(five, 1, [(0x80000001, q, 0x2400, 0x40, 6),
                  (0x80000002, q, 0x2500, 0x40, 1)], K(30))
    # Forward expansion whose equal run extends to the last entry: the
    # equal pair at indexes 1-2, queried at 20, records mid 1, steps
    # forward once (entry 2 matches), and the `cmp r15d, esi; jb` at
    # 0x1425112ce falls through because hi == count after the step. The
    # taken edge (hi < count after a step) is already covered by the
    # three equal entries in the middle above.
    add([e(10, 0x1000), e(20, 0x1100), e(20, 0x1200)], 0, [], K(20))
    # Zero stored hash at page 0, record 1: the scan collects record 0,
    # then the `cmp dword ptr [rbx-4], 0; je` at 0x142511344 takes its
    # edge to 0x14251145b (out words (0, 1)). The payload bytes of the
    # zero record are never read beyond the hash word.
    add([], 1, [(0x80000001, q, 0x2000, 0x40, 6),
                (0, K(31), 0x2100, 0x40, 6)], q)
    return cases


SPECIALS = _cases()


def _random(r: int) -> Case:
    locs = iter(range(0x1000, 0x10000, 0x100))

    def fresh_loc() -> int:
        return next(locs)

    n = r % 6
    base = (r * 7) % 200
    entries = []
    for i in range(n):
        # Sorted, with an occasional duplicate of the previous key.
        key_n = base + i * 3 + (1 if i and r % 4 == 0 else 0)
        entries.append((K(key_n), fresh_loc(), 0x40))
    if entries and r % 3 == 0:
        query = entries[r % len(entries)][0]
    else:
        query = K(base + (r % 9) - 2)
    page_count = r % 3
    pages = []
    if page_count:
        slots = r % 6
        for i in range(slots):
            if r % 5 == 0 and i == 0:
                key = query
            elif r % 5 == 1 and i == 1:
                key = bytes([query[0]]) + b"\x00" * 8
            elif r % 5 == 2 and i == 2:
                key = query[:8] + bytes([(query[8] + 1) & 0xFF])
            else:
                key = K(200 + i)
            kind = (r + i) % 8
            if kind == 4:  # keep the zero-chosen append out of random cases
                kind = 5
            pages.append((0x80000001 + i, key, fresh_loc(), 0x40, kind))
    return (entries, page_count, pages, query, r % 2 == 0, r % 3 == 0,
            False, (0, 0))


def make_case(i: int) -> Case:
    if i < len(SPECIALS):
        return SPECIALS[i]
    return _random(i - len(SPECIALS))


def _entry_bytes(key: bytes, loc: int, size: int) -> bytes:
    out = bytearray(18)
    out[0:9] = key
    out[9:14] = (loc & ((1 << 40) - 1)).to_bytes(5, "big")
    out[14:18] = size.to_bytes(4, "little")
    return bytes(out)


def make_args(i: int) -> List[ArgSpec]:
    entries, page_count, pages, key, out_page, out_record, found, chosen = (
        make_case(i)
    )
    blob = bytearray(b"\xA5" * BLOB_LEN)
    # Allocator slot and disabled logger.
    blob[0x1000 : 0x1000 + 32] = bytes(32)
    blob[LOGGER : LOGGER + 32] = bytes(32)
    # Segment at 0.
    blob[0x00:0x08] = (0x1000).to_bytes(8, "little")
    blob[0x08:0x10] = LOGGER.to_bytes(8, "little")
    blob[0x58:0x60] = ENTRIES.to_bytes(8, "little")
    blob[0x60:0x64] = len(entries).to_bytes(4, "little")
    blob[0x68:0x70] = PAGES.to_bytes(8, "little")
    blob[0x70:0x74] = page_count.to_bytes(4, "little")
    # Fresh key state at CTX (FindContextInit shape).
    blob[CTX + 0x00 : CTX + 0x08] = (0x1000).to_bytes(8, "little")
    blob[CTX + 0x08 : CTX + 0x10] = LOGGER.to_bytes(8, "little")
    blob[CTX + 0x10] = 1 if found else 0
    blob[CTX + 0x18 : CTX + 0x20] = chosen[0].to_bytes(8, "little")
    blob[CTX + 0x20 : CTX + 0x24] = chosen[1].to_bytes(4, "little")
    blob[CTX + 0x24 : CTX + 0x28] = bytes(4)
    blob[CTX + 0x128 : CTX + 0x130] = (CTX + 0x28).to_bytes(8, "little")
    blob[CTX + 0x130 : CTX + 0x134] = (0).to_bytes(4, "little")
    blob[CTX + 0x134 : CTX + 0x138] = (16).to_bytes(4, "little")
    for slot in range(16):
        at = CTX + 0x28 + slot * 16
        blob[at : at + 8] = (0xA5A5A5A5A5A5A5A5 ^ slot).to_bytes(8, "little")
        blob[at + 8 : at + 12] = (0xA5A5A5A5).to_bytes(4, "little")
        blob[at + 12 : at + 16] = b"\xCC" * 4
    for index, (ekey, loc, size) in enumerate(entries):
        at = ENTRIES + index * 18
        blob[at : at + 18] = _entry_bytes(ekey, loc, size)
    for index, (stored, pkey, loc, size, kind) in enumerate(pages):
        at = PAGES + index * 24
        blob[at : at + 4] = stored.to_bytes(4, "little")
        blob[at + 4 : at + 13] = pkey
        blob[at + 13 : at + 18] = (loc & ((1 << 40) - 1)).to_bytes(5, "big")
        blob[at + 18 : at + 22] = size.to_bytes(4, "little")
        blob[at + 22] = kind & 0xFF
        blob[at + 23] = 0xCC
    blob[KEY_AT : KEY_AT + 9] = key
    blob[OUT_PAGE : OUT_PAGE + 4] = (0xA5A5A5A5).to_bytes(4, "little")
    blob[OUT_RECORD : OUT_RECORD + 4] = (0xA5A5A5A5).to_bytes(4, "little")
    relocs = (0x00, 0x08, 0x58, 0x68, CTX + 0x00, CTX + 0x08, CTX + 0x128)
    return [
        ArgSpec(kind="ptr", mem=bytes(blob), arena_offset=0, relocs=relocs),
        ArgSpec(kind="u64", bits=ARENA_BASE + KEY_AT),
        ArgSpec(kind="u64", bits=ARENA_BASE + CTX),
        ArgSpec(kind="u64", bits=ARENA_BASE + OUT_PAGE if out_page else 0),
        ArgSpec(kind="u64", bits=ARENA_BASE + OUT_RECORD if out_record else 0),
    ]

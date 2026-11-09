"""Deterministic (entries, pages, key, request) cases for
`fn/DynamicIndexResolveRange`.

The client's `DynamicIndexResolveRange` (`0x1425115d0`) takes the segment in
RCX, the 9-byte key in RDX, a 16-byte request `{u64 start, u32 len}` in R8, the
out-range pointer in R9 and the out-entry pointer on the stack. It builds a
fresh key state, calls `DynamicIndexFindEntry` (`0x1425111a0`) with both out
words null, and then bounds the request against the chosen entry's stored
size (`2` = no entry, `4` = past the end), writes the absolute range, checks
residency (`3` = not resident) and finally writes the chosen entry when the
out pointer is non-null.

Surface: one 0x6000-byte blob plus four u64 arena addresses (key, request,
out range, out entry; 0 for a null out entry). The blob holds the segment at 0
(entries base, entry count, page-table base, page count, logger), the
allocator slot at 0x1000, the 18-byte entries at 0x3000, the 24-byte page
records at 0x4000, the disabled logger at 0x5200, the key at 0x5600, the
request at 0x5700, the out range at 0x5800 and the out entry at 0x5900. The
segment's internal pointers travel as relocs inside the blob; the request and
the two out words are sentinel-filled so every write is visible.

Every case keeps decode appends within the fresh state's 16-event capacity
(at most eight appends) and page matches within the pointer array's 32: the
grow paths through the allocator vtable are the unit's declared gaps, exactly
as in `fn/DynamicIndexFindEntry`. All appended interval starts are distinct,
so the client's unstable normalize sort has a unique order.

Status 3 needs resident coverage that does not cover the range: a page record
of type 5 clears the interval buffer, which leaves the buffer empty and the
resident answer false, while a zero length skips the check entirely (status
0). A page record of type 3 clears the found flag, which is status 2.
"""

from __future__ import annotations

from typing import List, Tuple

from emu_oracle import ARENA_BASE, ArgSpec

BLOB_LEN = 0x6000
ENTRIES = 0x3000
PAGES = 0x4000
LOGGER = 0x5200
KEY_AT = 0x5600
REQUEST = 0x5700
OUT_RANGE = 0x5800
OUT_ENTRY = 0x5900
PAGE_SIZE = 0x200
RECORD_SIZE = 24
RECORDS_PER_PAGE = 21


def K(n: int) -> bytes:
    return bytes([n & 0xFF]) * 9


# (entries, page_count, pages, key, start, len, out_entry)
# entries: [(key9, loc, size)]; pages: [(stored, key9, loc, size, type)] in
# slot order (a zero stored hash ends the scan, so a short list's next slot is
# the first zero); out_entry: whether the pointer is non-null.
Case = Tuple[
    List[Tuple[bytes, int, int]],
    int,
    List[Tuple[int, bytes, int, int, int]],
    bytes,
    int,
    int,
    bool,
]


def _cases() -> List[Case]:
    cases: List[Case] = []

    def add(entries, page_count, pages, key, start, length, out_entry=True):
        cases.append((entries, page_count, pages, key, start, length, out_entry))

    def e(n, loc, size=0x40):
        return (K(n), loc, size)

    def page(kind, n=10, loc=0x2000, size=0x40, stored=0x80000001):
        return (stored, K(n), loc, size, kind)

    # No entries at all: status 2, nothing written.
    add([], 0, [], K(1), 0, 0x10)
    add([], 0, [], K(1), 0, 0x10, out_entry=False)
    add([], 1, [], K(1), 0, 0x10)
    # A key the walk cannot find: status 2.
    add([e(10, 0x1000)], 0, [], K(11), 0, 0x10)
    add([e(10, 0x1000), e(30, 0x1100)], 0, [], K(20), 0, 0x10)
    # Found, resident: start 0 with the whole entry, a prefix, the empty
    # range, the last byte, and a start at the far end.
    add([e(10, 0x1000)], 0, [], K(10), 0, 0x40)
    add([e(10, 0x1000)], 0, [], K(10), 0, 0x10)
    add([e(10, 0x1000)], 0, [], K(10), 0, 0)
    add([e(10, 0x1000)], 0, [], K(10), 0x3F, 1)
    add([e(10, 0x1000)], 0, [], K(10), 0x40, 0)
    # The length 0xffffffff means "to the end of the entry".
    add([e(10, 0x1000)], 0, [], K(10), 0x10, 0xFFFFFFFF)
    add([e(10, 0x1000)], 0, [], K(10), 0, 0xFFFFFFFF)
    add([e(10, 0x1000)], 0, [], K(10), 0x40, 0xFFFFFFFF)
    # Past the end: start beyond the size, and start + len past it.
    add([e(10, 0x1000)], 0, [], K(10), 0x41, 0)
    add([e(10, 0x1000)], 0, [], K(10), 0x41, 0xFFFFFFFF)
    add([e(10, 0x1000)], 0, [], K(10), 0xFFFFFFFF, 1)
    add([e(10, 0x1000)], 0, [], K(10), 0x30, 0x11)
    add([e(10, 0x1000)], 0, [], K(10), 0x10, 0x31)
    add([e(10, 0x1000, 0)], 0, [], K(10), 0, 1)
    # A zero stored size: only the empty request fits.
    add([e(10, 0x1000, 0)], 0, [], K(10), 0, 0)
    add([e(10, 0x1000, 0)], 0, [], K(10), 0, 0xFFFFFFFF)
    # The out-entry pointer null: the status still follows the same path.
    add([e(10, 0x1000)], 0, [], K(10), 0, 0x10, out_entry=False)
    add([e(10, 0x1000)], 0, [], K(10), 0x30, 0x11, out_entry=False)
    add([], 0, [], K(1), 0, 0x10, out_entry=False)
    # Not resident: a type-5 page record clears the interval buffer, so the
    # resident walk finds no events at all. A zero length skips the check.
    add([e(10, 0x1000)], 1, [page(5)], K(10), 0, 0x10)
    add([e(10, 0x1000)], 1, [page(5)], K(10), 0, 0)
    add([e(10, 0x1000)], 1, [page(5)], K(10), 0x10, 0xFFFFFFFF)
    add([e(10, 0x1000)], 1, [page(5)], K(10), 0x40, 0)
    add([e(10, 0x1000)], 1, [page(5)], K(10), 0, 0x10, out_entry=False)
    # A type-2 record also clears the intervals (but keeps the entry).
    add([e(10, 0x1000)], 1, [page(2, loc=0x1000)], K(10), 0, 0x10)
    # A type-3 record clears the found flag: status 2.
    add([e(10, 0x1000)], 1, [page(3)], K(10), 0, 0x10)
    # Type 4 re-appends the current chosen entry, which keeps coverage.
    add([e(10, 0x1000)], 1, [page(4)], K(10), 0, 0x10)
    # Type 6 appends the page record's own interval after the entry's.
    add([e(10, 0x1000)], 1, [page(6, loc=0x1100, size=0x40)], K(10), 0, 0x10)
    add([e(10, 0x1000)], 1, [page(6, loc=0x1100, size=0x40)], K(10), 0x30, 0x10)
    # Type 7 appends a non-resident interval with its own (negative) delta:
    # the coverage the walk accumulates before the entry's events is negative
    # only when the request starts at or after the non-resident interval.
    add([e(10, 0x1000)], 1, [page(7, loc=0x0F00, size=0x80)], K(10), 0, 0x10)
    add([e(10, 0x1000)], 1, [page(7, loc=0x0F00, size=0x80)], K(10), 0x40, 0)
    # Several page records with the same key and distinct starts: every type
    # runs, in page/record order, before the request is looked at.
    add([e(10, 0x1000)], 1,
        [page(1, loc=0x1200), page(6, loc=0x1300), page(7, loc=0x1400),
         page(5, loc=0x1500)], K(10), 0, 0x10)
    add([e(10, 0x1000)], 1,
        [page(1, loc=0x1200), page(6, loc=0x1300), page(6, loc=0x1400)],
        K(10), 0, 0x40)
    # Duplicate entries for one key: the first is the chosen entry.
    add([e(10, 0x1000), e(10, 0x1100)], 0, [], K(10), 0, 0x40)
    add([e(10, 0x1000), e(10, 0x1100), e(10, 0x1200)], 0, [], K(10), 0x10, 0x20)
    # A page record whose key differs: skipped by the scan, so the entry's
    # own interval is all the residency sees.
    add([e(10, 0x1000)], 1, [page(5, n=11)], K(10), 0, 0x10)
    # A request that starts inside the entry but on an interval boundary.
    add([e(10, 0x1000, 0x100)], 0, [], K(10), 0xFF, 1)
    add([e(10, 0x1000, 0x100)], 0, [], K(10), 0x100, 0)
    add([e(10, 0x1000, 0x100)], 0, [], K(10), 0x100, 1)
    return cases


SPECIALS = _cases()


def _random(r: int) -> Case:
    loc = 0x1000 + (r % 4) * 0x100
    size = [0, 1, 0x40, 0x100, 0xFFFF][r % 5]
    key = K(10 + r % 3)
    entries = [(K(10 + (r + i) % 3), 0x1000 + i * 0x100, 0x40) for i in range(r % 4)]
    page_count = r % 2
    pages: List[Tuple[int, bytes, int, int, int]] = []
    if page_count:
        # Distinct locations keep the normalize sort's order unique, and a
        # type 3 is left out so the found flag survives.
        for i in range(r % 3):
            kind = 1 + (r + i) % 7
            if kind == 3:
                kind = 6
            pages.append((0x80000001 + i, key, loc + 0x80 + i * 0x10, 0x40, kind))
    start = [0, 1, 0x10, size, size + 1, 0xFFFFFFFF][(r // 2) % 6]
    length = [0, 1, 0x10, 0x40, 0xFFFFFFFF, size][(r // 3) % 6]
    return (entries, page_count, pages, key, start, length, r % 4 != 0)


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
    entries, page_count, pages, key, start, length, out_entry = make_case(i)
    blob = bytearray(b"\xA5" * BLOB_LEN)
    # Allocator slot and disabled logger (level 0 at +8).
    blob[0x1000:0x1000 + 32] = bytes(32)
    blob[LOGGER:LOGGER + 32] = bytes(32)
    # The segment at 0. Its pointers are relative offsets; the ArgSpec relocs
    # rebase them on the oracle side and the probe reads them as offsets.
    blob[0x00:0x08] = (0x1000).to_bytes(8, "little")
    blob[0x08:0x10] = LOGGER.to_bytes(8, "little")
    blob[0x58:0x60] = ENTRIES.to_bytes(8, "little")
    blob[0x60:0x64] = len(entries).to_bytes(4, "little")
    blob[0x68:0x70] = PAGES.to_bytes(8, "little")
    blob[0x70:0x74] = page_count.to_bytes(4, "little")
    for index, (ekey, loc, size) in enumerate(entries):
        at = ENTRIES + index * 18
        blob[at:at + 18] = _entry_bytes(ekey, loc, size)
    if len(pages) > page_count * RECORDS_PER_PAGE:
        raise ValueError("fixture case has more records than pages")
    for index, (stored, pkey, loc, size, kind) in enumerate(pages):
        at = PAGES + index * RECORD_SIZE
        blob[at:at + 4] = stored.to_bytes(4, "little")
        blob[at + 4:at + 13] = pkey
        blob[at + 13:at + 18] = (loc & ((1 << 40) - 1)).to_bytes(5, "big")
        blob[at + 18:at + 22] = size.to_bytes(4, "little")
        blob[at + 22] = kind & 0xFF
        blob[at + 23] = 0xCC
    blob[KEY_AT:KEY_AT + 9] = key
    blob[REQUEST:REQUEST + 8] = start.to_bytes(8, "little")
    blob[REQUEST + 8:REQUEST + 12] = length.to_bytes(4, "little")
    blob[REQUEST + 12:REQUEST + 16] = (0xA5A5A5A5).to_bytes(4, "little")
    blob[OUT_RANGE:OUT_RANGE + 16] = b"\xA5" * 16
    blob[OUT_ENTRY:OUT_ENTRY + 16] = b"\xA5" * 16
    relocs = (0x00, 0x08, 0x58, 0x68)
    return [
        ArgSpec(kind="ptr", mem=bytes(blob), arena_offset=0, relocs=relocs),
        ArgSpec(kind="u64", bits=ARENA_BASE + KEY_AT),
        ArgSpec(kind="u64", bits=ARENA_BASE + REQUEST),
        ArgSpec(kind="u64", bits=ARENA_BASE + OUT_RANGE),
        ArgSpec(kind="u64", bits=ARENA_BASE + OUT_ENTRY if out_entry else 0),
    ]

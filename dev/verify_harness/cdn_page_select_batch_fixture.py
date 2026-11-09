"""Deterministic (index, keys, keyLen, count, outFound, outLocations) cases
for `fn/CdnIndexPageSelectBatch`.

The client's `CdnIndexPageSelectBatch` (`0x1424da910`) looks up many keys at
once, keeping one monotonic page cursor instead of binary-searching per key.
It takes the CDN index object (RCX), the caller's pointer array (RDX), the key
length (R8), the key count (R9), and two stack outputs: a byte "found" array
and a `u64` location array.

One blob holds the object at 0 and everything it points at:

    0x000  index object: +0x38 whole-file flag (0), +0x50 whole-file base,
           +0x5c location width, +0x5d location offset, +0x5e key width,
           +0x60 total entry count, +0x6c page size, +0x70 entry stride,
           +0x74 entries per page, +0x78 page count, +0x88 last-key array,
           +0x98 resident page pointer (written), +0xa0 current page (written)
    0x100  the query key buffers, 16 bytes apart
    0x400  the per-page last-key array (key width per page)
    0x1000 the page data: per_page * entry_stride bytes per page, each entry
           key bytes then the big-endian location at +keyWidth + locOff
    0x8000 the caller's pointer array (a separate pointee): u64 arena addresses
           of the query keys at 0x100
    0x9000 the found array (one byte per key, 0xA5 sentinel past `count`)
    0x9800 the location array (u64 per key, 0xBB sentinel past `count`)

The object's pointers are absolute arena addresses; the candidate subtracts
the fixed arena base. The fixture keeps its own fields' untouched bytes at a
sentinel so a stray write is visible.

Declared cases walk every branch of the window: the zero-count and zero-page
exits, a page with no entries, the found / not-found / keep-scanning record
compares, the location decode for 0, 1, 5 and 8 bytes, the page advance loop
(taken and exhausted), the same-page and next-page re-entry, duplicate and
unsorted keys, a key shorter than the key width (prefix compare), a zero key
length, a nonzero location offset, and both tail layouts: the vectorized
zeroing (`memset`) when the two outputs are far apart, and the scalar zero
loop when they overlap exactly.
"""

from __future__ import annotations

from typing import List, Optional, Sequence, Tuple

from emu_oracle import ArgSpec

ARENA = 0x2_0000_0000
INDEX_LEN = 0x8000
KEY_OFF = 0x100
KEY_SLOT = 16
LAST_OFF = 0x400
DATA_OFF = 0x1000
KEYS_ARENA = 0x8000
FOUND_ARENA = 0x9000
LOC_ARENA = 0x9800
FOUND_LEN = 0x100
LOC_LEN = 0x200
SENTINEL = 0xA5
OUT_FOUND_SENTINEL = 0xA5
OUT_LOC_SENTINEL = 0xBB

# One page = a list of (key, location value); the page's last key is its last
# entry's key.
Page = List[Tuple[bytes, int]]
Case = Tuple[int, int, int, int, int, List[Page], List[bytes], int, int, int]
# (keyWidth, locWidth, locOff, entryStride, perPage, pages, queries, keyLen,
#  foundArena, locationsArena)


def _key(n: int) -> bytes:
    return f"k{n:08d}".encode()


def _entry_stride(key_bytes: int, loc_off: int, loc_bytes: int) -> int:
    return max(16, ((key_bytes + loc_off + loc_bytes + 7) // 8) * 8)


def _put32(blob: bytearray, at: int, value: int) -> None:
    blob[at : at + 4] = (value & 0xFFFFFFFF).to_bytes(4, "little")


def _put64(blob: bytearray, at: int, value: int) -> None:
    blob[at : at + 8] = (value & 0xFFFFFFFFFFFFFFFF).to_bytes(8, "little")


def _case(
    queries: Sequence[bytes],
    pages: Sequence[Page],
    key_len: int = 9,
    key_bytes: int = 9,
    loc_bytes: int = 5,
    loc_off: int = 0,
    per_page: int = 4,
    found_arena: int = FOUND_ARENA,
    locations_arena: int = LOC_ARENA,
) -> Case:
    stride = _entry_stride(key_bytes, loc_off, loc_bytes)
    return (
        key_bytes,
        loc_bytes,
        loc_off,
        stride,
        per_page,
        list(pages),
        list(queries),
        key_len,
        found_arena,
        locations_arena,
    )


def _pages(a: int, b: int, per_page: int = 4, value: int = 0x1122334455) -> List[Page]:
    """One page of consecutive keys `a..=b`, each with its own location."""
    return [[(_key(k), value + k) for k in range(a, b + 1)]]


def _two_pages() -> List[Page]:
    return [
        [(_key(1), 0x1111111111), (_key(2), 0x2222222222), (_key(3), 0x3333333333), (_key(4), 0x4444444444)],
        [(_key(5), 0x5555555555), (_key(6), 0x6666666666), (_key(7), 0x7777777777)],
    ]


def _cases() -> List[Case]:
    one = _pages(1, 4)
    two = _two_pages()
    return [
        # No keys: the count == 0 exit, nothing is touched.
        _case([], [], per_page=4),
        # No pages, but keys: the whole page loop is skipped and the tail
        # zeroes all four (even count, the vectorized layout).
        _case([_key(1), _key(2), _key(3), _key(4)], []),
        # No pages, three keys: the tail's odd remainder takes the scalar step.
        _case([_key(1), _key(2), _key(3)], []),
        # No pages, one key: the scalar-only tail.
        _case([_key(1)], []),
        # No pages, four keys, the found array *after* the location array: the
        # client's first overlap test is taken instead of the second.
        _case([_key(1), _key(2), _key(3), _key(4)], [], found_arena=0x9A00),
        # A hit in the middle of a page: record 0 and 1 are below the key, the
        # hit is record 2, five location bytes are decoded.
        _case([_key(3)], one),
        # The same hit with a zero-width location: the decode is skipped.
        _case([_key(3)], one, loc_bytes=0),
        # One location byte: a single decode step, no loop back edge.
        _case([_key(3)], one, loc_bytes=1),
        # Eight location bytes: the loop's back edge and the top shift.
        _case([_key(8)], one, loc_bytes=8),
        # A key below every entry: the first record compare is above the key,
        # a miss that leaves the entry scan before it.
        _case([_key(0)], one),
        # A key above the page's last key: the page cursor advances once, the
        # second page holds it.
        _case([_key(5)], two),
        # A key above every page: the cursor advances past the last page and
        # the tail zeroes the key.
        _case([_key(9)], two),
        # Two keys on the same page: the record cursor continues.
        _case([_key(2), _key(3)], one),
        # Two keys on different pages: the pre-check advances the cursor.
        _case([_key(3), _key(5)], two),
        # A duplicate key: the second lookup resumes one record past the hit,
        # runs out of records and is reported missing.
        _case([_key(4), _key(4)], one),
        # Two pages' worth of keys across the boundary, then a miss.
        _case([_key(2), _key(3), _key(5), _key(9)], two),
        # A key longer than the key width: the compare is clamped to 9 bytes.
        _case([_key(6)], two, key_len=12),
        # A key shorter than the key width: only the prefix is compared, so a
        # query sharing four bytes with an entry is a hit.
        _case([_key(2)[:4] + b"zzzzz"], one, key_len=4),
        # A zero key length: every compare is equal, so record 0 of the first
        # page is the hit.
        _case([_key(0)], one, key_len=0),
        # Unsorted queries: after the hit the cursor stays, the next key scans
        # from the following record, runs out and is a miss.
        _case([_key(3), _key(1)], one),
        # A page whose entry count is zero (the last page starts at `total`).
        _case([_key(5)], [[(_key(1), 1), (_key(2), 2), (_key(3), 3), (_key(4), 4)], []]),
        # A nonzero location offset: the location sits after two spare bytes.
        _case([_key(2)], one, loc_off=2),
        # The location's high byte set (the top of five bytes).
        _case([_key(1)], [[(_key(1), 0xFFFFFFFFFF)]], per_page=4),
        # Two entries with equal keys: the first is the hit, the second is
        # reached by the resumed cursor.
        _case([_key(2), _key(2)], [[(_key(1), 1), (_key(2), 2), (_key(2), 3), (_key(3), 4)]]),
        # One entry per page: every key advances the cursor by one page.
        _case([_key(1), _key(2), _key(3)], [[(_key(1), 1)], [(_key(2), 2)], [(_key(3), 3)]], per_page=1),
        # A miss on the first record, a hit on the second, then the tail.
        _case([_key(2), _key(9)], one),
        # No pages, two keys: the smallest vectorized tail.
        _case([_key(1), _key(2)], []),
        # The two output arrays overlapping exactly (the found array is only
        # `count` bytes, so the shared region is the same on both sides).
        # The client's second overlap test fires and the scalar zero loop
        # runs once per key.
        _case([_key(1), _key(2), _key(3), _key(4)], [], found_arena=LOC_ARENA),
        _case([_key(1), _key(2), _key(3), _key(4), _key(5), _key(6)], [], found_arena=LOC_ARENA),
        # No pages, five keys: the memset loop runs twice and the scalar step
        # finishes the odd one.
        _case([_key(1), _key(2), _key(3), _key(4), _key(5)], []),
        # A short last page: `entries per page` is clamped by the total.
        _case([_key(7)], [[(_key(1), 1), (_key(2), 2), (_key(3), 3), (_key(4), 4)], [(_key(5), 5), (_key(6), 6), (_key(7), 7)]]),
    ]


SPECIALS = _cases()


def _build(case: Case) -> Tuple[bytes, List[int]]:
    (key_bytes, loc_bytes, loc_off, stride, per_page, pages, queries, _key_len,
     _found, _locations) = case
    blob = bytearray([SENTINEL]) * INDEX_LEN
    total = sum(len(page) for page in pages)
    if total > per_page * len(pages):
        raise ValueError("total exceeds the page capacity")
    key_at: List[int] = []
    for i, query in enumerate(queries):
        at = KEY_OFF + i * KEY_SLOT
        blob[at : at + key_bytes] = query[:key_bytes].ljust(key_bytes, b"\x00")
        key_at.append(at)
    for page_index, page in enumerate(pages):
        last = page[-1][0] if page else bytes(key_bytes)
        at = LAST_OFF + page_index * key_bytes
        blob[at : at + key_bytes] = last[:key_bytes].ljust(key_bytes, b"\x00")
        for record, (key, value) in enumerate(page):
            entry = DATA_OFF + page_index * per_page * stride + record * stride
            blob[entry : entry + key_bytes] = key[:key_bytes].ljust(key_bytes, b"\x00")
            at = entry + key_bytes + loc_off
            blob[at : at + loc_bytes] = (value & ((1 << (8 * loc_bytes)) - 1)).to_bytes(
                loc_bytes, "big"
            ) if loc_bytes else b""
    _put32(blob, 0x60, total)
    _put32(blob, 0x6C, per_page * stride)
    _put32(blob, 0x70, stride)
    _put32(blob, 0x74, per_page)
    _put32(blob, 0x78, len(pages))
    _put64(blob, 0x50, ARENA + DATA_OFF)
    _put64(blob, 0x88, ARENA + LAST_OFF)
    _put64(blob, 0x98, 0)
    _put32(blob, 0xA0, 0)
    blob[0x5C] = loc_bytes
    blob[0x5D] = loc_off
    blob[0x5E] = key_bytes
    blob[0x38] = 0
    return bytes(blob), key_at


def _random(r: int) -> Case:
    """A generated case: pages of sorted keys, queries on and off them."""
    key_bytes = 9
    loc_bytes = r % 9
    loc_off = (r // 9) % 3
    per_page = 1 + (r % 4)
    pages_n = 1 + (r // 4) % 3
    pages: List[Page] = []
    n = 0
    for p in range(pages_n):
        count = 1 + ((r >> (p + 1)) % per_page)
        page: Page = []
        for _ in range(count):
            n += 1 + (r % 3 == 0)
            page.append((_key(n), (r * 7919 + n * 104729) & 0xFFFFFFFFFF))
        pages.append(page)
    pool = [key for page in pages for key, _ in page]
    queries: List[bytes] = []
    for i in range(1 + (r % 6)):
        pick = (r * 13 + i * 7) % (len(pool) * 2)
        if pick < len(pool):
            queries.append(pool[pick])
        else:
            queries.append(_key(((r + i) * 3) % (n + 3)))
    key_len = (r // 2) % 13
    found_arena = 0x9A00 if (r // 3) % 4 == 0 else FOUND_ARENA
    return _case(
        queries,
        pages,
        key_len=key_len,
        key_bytes=key_bytes,
        loc_bytes=loc_bytes,
        loc_off=loc_off,
        per_page=per_page,
        found_arena=found_arena,
    )


def make_case(i: int) -> Case:
    if i < len(SPECIALS):
        return SPECIALS[i]
    return _random(i - len(SPECIALS))


def make_args(i: int) -> List[ArgSpec]:
    case = make_case(i)
    (_key_bytes, _loc_bytes, _loc_off, _stride, _per_page, _pages_, queries,
     key_len, found_arena, locations_arena) = case
    blob, key_at = _build(case)
    keys_blob = b"".join((ARENA + at).to_bytes(8, "little") for at in key_at)
    if len(keys_blob) > FOUND_ARENA - KEYS_ARENA:
        raise ValueError("keys array does not fit")
    # When the found array sits on the location array the client takes its
    # overlap path; give it exactly `count` bytes so the aliased region is the
    # same bytes on both sides of the comparison.
    found_len = len(queries) if found_arena == locations_arena else FOUND_LEN
    return [
        ArgSpec(kind="ptr", mem=blob, arena_offset=0),
        ArgSpec(kind="ptr", mem=keys_blob, arena_offset=KEYS_ARENA),
        ArgSpec(kind="u64", bits=key_len),
        ArgSpec(kind="u64", bits=len(queries)),
        ArgSpec(kind="ptr", mem=bytes([OUT_FOUND_SENTINEL]) * found_len, arena_offset=found_arena),
        ArgSpec(kind="ptr", mem=bytes([OUT_LOC_SENTINEL]) * LOC_LEN, arena_offset=locations_arena),
    ]

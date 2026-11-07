"""Deterministic inputs for the `fn/CdnIndexPageSelect` unit.

The client's CDN index page selection (`CdnIndexPageSelect`, exe
`0x1424da730`) takes the index object, the query key, its length, an offset
slot, a size slot (stack argument 5) and a 16-byte key slot (stack argument 6).
The object holds the footer fields at `+0x58..`, the last-key array pointer at
`+0x88`, the page-body pointer at `+0x98` and the current page at `+0xa0`:

    +0x38  whole-file flag (0; the streamed branch is out of coverage)
    +0x50  page bodies' base address
    +0x5c  offset width      +0x5d  size width      +0x5e  key width
    +0x5f  checksum width    +0x60  element count
    +0x6c  page size         +0x70  entry size      +0x74  entries per page
    +0x78  page count        +0x7c  last-key bytes  +0x80  checksum bytes
    +0x88  last-key array    +0x90  1               +0x98  page pointer
    +0xa0  current page      +0xa8  the object's own arena address

`+0xa8` is fixture plumbing: the selector never reads it, and it lets the probe
turn the absolute pointers into offsets in its copy (the same trick
`encoding_find_ckey_fixture.py` uses with the info struct's `+0x68`).

One arena buffer carries the object, the last-key array and the page bodies, so
the pointers the object holds are absolute arena addresses as in the client.
Each page body is `pageSize` bytes and its record `i` sits at `i * entrySize`;
a record is the key, the big-endian size and the big-endian offset field (for a
group index the field is the archive ordinal followed by the in-archive
offset). `entriesPerPage = pageSize / entrySize`, `pageCount` is the number of
page bodies and `elementCount` is the number of records, so the last page is
partly filled exactly as on a real index.

What the corpus pins, case by case:

* a key on page 0, in the middle page and on the last page;
* a key below every last key (page 0), above the last key (status 6, and the
  two whole-file object fields stay untouched), and between two pages' keys
  (the next page is selected and the scan misses);
* a key exactly equal to a page's last key (the early equal exit) and two
  pages with the *same* last key, where the early exit answers the later page
  and a pure lower bound would answer the earlier one;
* a key at a page's first record;
* `key_len` shorter than the key width, with the query's tail differing from
  the record's: the compare is `min(keyBytes, keyLen)` bytes and the 16-byte
  copy comes from the *record*, not the query;
* `pageCount == 0` (status 6, no read);
* duplicate records with the same key (the first record wins);
* a partly filled last page (the element-count cap stops the scan);
* a page with no elements left (`count == 0`, status 6);
* offset widths 4, 5, 6 and 0 (the loose-file flavour), size widths 4 and 0,
  and key widths 16 and 8 (the latter takes the zero-fill copy path).
"""

from __future__ import annotations

from typing import List, Optional, Sequence, Tuple

from emu_oracle import ARENA_BASE, ArgSpec

INDEX_OFF = 0
OBJECT_LEN = 0xB0
SELF_OFF = 0xA8
LAST_KEYS_OFF = 0x100
BODIES_OFF = 0x1000
PAGE_SIZE = 0x1000
KEY_OFF = 0xA000
KEY_BUF = 32
OUT_OFFSET_OFF = 0xA040
OUT_OFFSET_LEN = 8
OUT_SIZE_OFF = 0xA048
OUT_SIZE_LEN = 4
OUT_KEY16_OFF = 0xA050
OUT_KEY16_LEN = 16
OUT_FILL = 0xA5
MAX_PAGES = 6

# (index bytes, query key bytes, key length)
Case = Tuple[bytes, bytes, int]


def _key16(n: int, tail: int = 0) -> bytes:
    """A 16-byte key ordered by `n`: the counter big-endian in the first two
    bytes, so the byte order of the array is the numeric order."""
    k = bytearray(16)
    k[0] = (n >> 8) & 0xFF
    k[1] = n & 0xFF
    k[15] = tail & 0xFF
    return bytes(k)


def _query(n: int, tail: int = 0) -> bytes:
    return _key16(n, tail)


def _record(n: int, offset_width: int) -> Tuple[bytes, int, int]:
    """(key, size, offset field) for counter `n`.

    The size and the offset both vary with `n` so a scan that decodes the wrong
    record, the wrong width or the wrong byte order diverges. For a width above
    4 the field carries an archive ordinal in its high bytes, which is what the
    group indexes' 5- and 6-byte fields are.
    """
    size = 0x100 + n
    if offset_width > 4:
        ordinal = (n * 3) % (1 << (8 * (offset_width - 4)))
        offset = (ordinal << 32) | (0x1000 + n)
    else:
        offset = 0x1000 + n
    return _key16(n), size, offset


def _build(
    counters: Sequence[Sequence[int]],
    query: int,
    key_len: int,
    *,
    key_bytes: int = 16,
    size_width: int = 4,
    offset_width: int = 6,
    element_count: Optional[int] = None,
    dup_tail: int = 0,
) -> Case:
    """Build one index object from per-page key counters, plus the query."""
    entry_size = key_bytes + size_width + offset_width
    entries_per_page = PAGE_SIZE // entry_size
    page_count = len(counters)
    records = sum(len(page) for page in counters)
    mem = bytearray(BODIES_OFF + page_count * PAGE_SIZE)

    for page, page_counters in enumerate(counters):
        body_at = BODIES_OFF + page * PAGE_SIZE
        last = bytes(key_bytes)
        for i, n in enumerate(page_counters):
            key, size, offset = _record(n, offset_width)
            at = body_at + i * entry_size
            mem[at : at + key_bytes] = key[:key_bytes]
            p = at + key_bytes
            if size_width:
                mem[p : p + size_width] = size.to_bytes(size_width, "big")
                p += size_width
            if offset_width:
                mem[p : p + offset_width] = offset.to_bytes(offset_width, "big")
            last = key[:key_bytes]
        # The page's last key, as the footer's per-page array holds it.
        mem[LAST_KEYS_OFF + page * key_bytes : LAST_KEYS_OFF + (page + 1) * key_bytes] = last

    total = records if element_count is None else element_count
    mem[0x38] = 0  # whole-file: CdnIndexPageResident takes its pointer path
    mem[0x5C] = offset_width & 0xFF
    mem[0x5D] = size_width & 0xFF
    mem[0x5E] = key_bytes & 0xFF
    mem[0x5F] = 8
    mem[0x60:0x64] = (total & 0xFFFFFFFF).to_bytes(4, "little")
    mem[0x6C:0x70] = PAGE_SIZE.to_bytes(4, "little")
    mem[0x70:0x74] = entry_size.to_bytes(4, "little")
    mem[0x74:0x78] = entries_per_page.to_bytes(4, "little")
    mem[0x78:0x7C] = page_count.to_bytes(4, "little")
    mem[0x7C:0x80] = (page_count * key_bytes).to_bytes(4, "little")
    mem[0x80:0x84] = (page_count * 8).to_bytes(4, "little")
    mem[0x50:0x58] = (ARENA_BASE + BODIES_OFF).to_bytes(8, "little")
    mem[0x88:0x90] = (ARENA_BASE + LAST_KEYS_OFF).to_bytes(8, "little")
    mem[0x90] = 1
    mem[0x98:0xA0] = bytes(8)  # only the resident writes it
    mem[0xA0:0xA4] = (0xFFFFFFFF).to_bytes(4, "little")
    mem[0xA8:0xB0] = (ARENA_BASE + INDEX_OFF).to_bytes(8, "little")

    key = _query(query, dup_tail)
    assert 0 < key_len <= key_bytes and key_len <= 16, (key_len, key_bytes)
    return bytes(mem), key, key_len


def _counters(pages: int, per_page: int, start: int = 0x100, step: int = 2) -> List[List[int]]:
    """`pages` pages of ascending counters, `per_page` on each full page."""
    out: List[List[int]] = []
    n = start
    for _ in range(pages):
        page = []
        for _ in range(per_page):
            page.append(n)
            n += step
        out.append(page)
    return out


def _epp(key_bytes: int, size_width: int, offset_width: int) -> int:
    return PAGE_SIZE // (key_bytes + size_width + offset_width)


def _cases() -> List[Case]:
    out: List[Case] = []

    # Page 0's first record: the determinism case.
    one = _counters(1, _epp(16, 4, 6))
    out.append(_build(one, 0x100, 16))

    # Three full pages: a key on page 0, on the middle page, on the last page,
    # at a page's first record, and one between two pages (the next page is
    # selected, the scan misses).
    three = _counters(3, _epp(16, 4, 6))
    epp = _epp(16, 4, 6)
    out.append(_build(three, 0x100, 16))  # page 0's first record
    out.append(_build(three, 0x100 + 2 * epp, 16))  # page 1's first record
    out.append(_build(three, 0x100 + 2 * (2 * epp - 1), 16))  # page 1's last record
    out.append(_build(three, 0x100 + 2 * (3 * epp - 1), 16))  # the last record
    out.append(_build(three, 0x100 + 2 * epp - 1, 16))  # between page 0 and 1
    out.append(_build(three, 0x0, 16))  # below every last key -> page 0
    out.append(_build(three, 0xFFFE, 16))  # above every last key -> status 6
    out.append(_build(three, 0x100 + 2 * (epp - 1), 16))  # page 0's last key
    out.append(_build(three, 0x100 + 2 * (2 * epp - 1), 16))  # page 1's last key

    # Two pages with the same last key: the search's early equal exit answers
    # the later page (the lower-middle probe), where a pure lower bound would
    # answer page 0.
    same = [[0x100, 0x102, 0x110], [0x110, 0x110]]
    out.append(_build(same, 0x110, 16))
    # Duplicate records with one key: the first record wins.
    dup = [[0x120, 0x120, 0x122], [0x124]]
    out.append(_build(dup, 0x120, 16))

    # A key length shorter than the key width, with the record's tail
    # differing from the query's: the compare is the short prefix and the
    # 16-byte copy must come from the record.
    out.append(_build(three, 0x100, 4))
    out.append(_build(three, 0x100 + 2 * epp, 4))
    out.append(_build(three, 0x100 + 2 * (2 * epp) + 1, 4))  # a prefix that misses

    # No pages: status 6 and no read.
    out.append(_build([], 0x100, 16))
    out.append(_build([], 0x100, 4))

    # A partly filled last page: the element-count cap.
    part = [[0x100, 0x102, 0x104], [0x106, 0x108]]
    out.append(_build(part, 0x104, 16))  # page 0's last key (early equal)
    out.append(_build(part, 0x108, 16))  # the last page's only un-capped record
    out.append(_build(part, 0x10A, 16))  # above every last key

    # A page with no elements left: `elementCount - entriesPerPage*page` is 0,
    # so the scan runs zero records and the answer is status 6.
    out.append(_build(three, 0x100 + 2 * epp, 16, element_count=epp))

    # The subtraction wraps in 32 bits: a page past the element count scans the
    # whole page (the unsigned minimum picks entriesPerPage).
    out.append(_build(three, 0x100 + 2 * epp, 16, element_count=1))

    # Every offset width, with the same two-page corpus: 6 (u16 ordinal), 5
    # (u8 ordinal), 4 (offset only) and 0 (the loose-file flavour).
    for width in (4, 5, 6, 0):
        pages = _counters(2, _epp(16, 4, width), step=2)
        out.append(_build(pages, 0x100, 16, offset_width=width))
        out.append(_build(pages, 0x100 + 2 * len(pages[0]) - 2, 16, offset_width=width))

    # Size width 0: the size loop is skipped and the slot is written as 0.
    out.append(_build(one, 0x100, 16, size_width=0))

    # Key width 8: the copy is 8 bytes and the rest is zero-filled.
    eight = _counters(2, _epp(8, 4, 6))
    out.append(_build(eight, 0x100, 8, key_bytes=8))
    out.append(_build(eight, 0x100, 4, key_bytes=8))
    out.append(_build(eight, 0x100 + 2 * len(eight[0]), 8, key_bytes=8))
    return out


SPECIALS = _cases()


def _lcg(seed: int, n: int) -> int:
    x = seed & 0xFFFFFFFF
    for _ in range(n):
        x = (x * 1664525 + 1013904223) & 0xFFFFFFFF
    return x >> 24


def _sweep(r: int) -> Case:
    key_bytes = 16 if r % 5 else 8
    size_width = 4 if r % 7 else 0
    offset_width = (6, 5, 4, 0)[r % 4]
    pages = r % (MAX_PAGES + 1)
    step = 2 + (r % 3) * 2
    counters = _counters(pages, _epp(key_bytes, size_width, offset_width), step=step)
    total = sum(len(page) for page in counters)
    element_count = total
    flavour = r % 6
    if flavour == 1 and total:
        # A partly filled last page.
        element_count = max(1, total - 1 - (r % max(1, total)))
    elif flavour == 2 and total:
        # A page with no elements left (or a wrapping subtraction).
        element_count = max(1, _epp(key_bytes, size_width, offset_width) - (r % 2))

    key_len = 1 + (r % key_bytes)
    if total == 0:
        query = _lcg(0x51000 + r, 2) << 8
    else:
        flat = [n for page in counters for n in page]
        choice = r % 6
        if choice == 0:
            query = flat[r % len(flat)]  # a real record
        elif choice == 1:
            query = flat[-1] + step  # above every last key
        elif choice == 2:
            query = max(0, flat[0] - step)  # below every last key
        elif choice == 3:
            page = r % len(counters)
            query = counters[page][-1]  # a page's last key
        elif choice == 4:
            page = r % len(counters)
            query = counters[page][-1] + 1  # between pages
        else:
            query = flat[_lcg(0x60000 + r, 4) % len(flat)]
    # A query above the 16-bit counter space keeps the 32-byte buffer legal.
    query &= 0xFFFF
    return _build(
        counters,
        query,
        key_len,
        key_bytes=key_bytes,
        size_width=size_width,
        offset_width=offset_width,
        element_count=element_count,
        dup_tail=_lcg(0x70000 + r, 1),
    )


def make_case(i: int) -> Case:
    if i < len(SPECIALS):
        return SPECIALS[i]
    return _sweep(i - len(SPECIALS))


def make_args(i: int) -> List[ArgSpec]:
    mem, query, key_len = make_case(i)
    key = bytearray(KEY_BUF)
    key[: len(query)] = query
    return [
        ArgSpec(kind="ptr", mem=mem, arena_offset=INDEX_OFF),
        ArgSpec(kind="ptr", mem=bytes(key), arena_offset=KEY_OFF),
        ArgSpec(kind="u64", bits=key_len),
        ArgSpec(kind="ptr", mem=bytes([OUT_FILL]) * OUT_OFFSET_LEN, arena_offset=OUT_OFFSET_OFF),
        ArgSpec(kind="ptr", mem=bytes([OUT_FILL]) * OUT_SIZE_LEN, arena_offset=OUT_SIZE_OFF),
        ArgSpec(kind="ptr", mem=bytes([OUT_FILL]) * OUT_KEY16_LEN, arena_offset=OUT_KEY16_OFF),
    ]

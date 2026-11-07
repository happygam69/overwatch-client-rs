"""Deterministic inputs for the `fn/EncodingPageIndexSearch` unit.

The client's page-index binary search (`EncodingPageIndexSearch`, exe
`0x142498880`) takes the index base, the page count, the key width, the key and
the key length; the fifth argument travels on the stack. An index entry is the
page's first key (`width` bytes) followed by the page's 16-byte MD5, so the
stride is `width + 0x10`.

This module only builds the inputs. `pages` entries of `width + 16` bytes carry
the first keys the fixture chose, and a 32-byte key buffer carries the query in
its first `key_len` bytes, so a `key_len` of `width + 1` still has a byte to
spare and the compare never reads outside the buffer.

What the corpus pins, case by case:

* `pages == 0` returns `0xFFFFFFFF` without touching the index.
* a key below every page first key, one exactly at a page's first key, one
  between two pages and one above the last page: the miss, the equal hit, the
  `lo = mid + 1` edge and the `hi = mid` edge.
* an all-equal index, where the first probe is the lower-middle of `[0, pages)`
  and the equal compare returns it: `pages >> 1`, not page 0 and not the page
  after the middle. This pins `mid = ((hi - lo) >> 1) + lo`.
* two pages whose first keys share a prefix: with `key_len` shorter than the
  shared run the two entries compare equal, so the answer is the midpoint;
  with the full width the answer is page 0. That pins the search comparing
  exactly `key_len` bytes rather than the full width.
* a last page keyed `0xFF..FF`.
* `key_len` from 0 to `width + 1`.
"""

from __future__ import annotations

from typing import List, Tuple

from emu_oracle import ArgSpec

INDEX_OFF = 0
KEY_OFF = 0x800
KEY_BYTES = 32
HASH_BYTES = 16

# (index bytes, pages, width, key bytes, key_len)
Case = Tuple[bytes, int, int, bytes, int]

WIDTHS = (1, 2, 4, 8, 16)


def _entries(first_keys: List[bytes], width: int) -> bytes:
    """`len(first_keys)` index entries: the first key, then the page MD5."""
    out = bytearray()
    for page, first in enumerate(first_keys):
        assert len(first) == width, (len(first), width)
        out += first
        out += bytes([(0x80 + page) & 0xFF]) * HASH_BYTES
    return bytes(out)


def _build(
    first_keys: List[bytes],
    width: int,
    query: bytes,
    key_len: int,
) -> Case:
    index = _entries(first_keys, width)
    key = bytearray(KEY_BYTES)
    key[: len(query)] = query
    # The buffer is 32 bytes, so a compare length above the query length reads
    # the zero fill - and never past the buffer.
    assert key_len <= KEY_BYTES and len(query) <= KEY_BYTES, (len(query), key_len)
    return index, len(first_keys), width, bytes(key), key_len


def _repeated(value: int, width: int) -> bytes:
    return bytes([value & 0xFF]) * width


def _increasing(count: int, width: int, start: int = 0x10, step: int = 8) -> List[bytes]:
    return [_repeated(start + step * i, width) for i in range(count)]


def _cases() -> List[Case]:
    out: List[Case] = []

    # Case 0: the determinism case. One page, its first key exactly.
    out.append(_build([_repeated(0x40, 16)], 16, _repeated(0x40, 16), 16))

    # An all-equal index: the first probe is the lower-middle of [0, pages) and
    # the equal compare returns it, so the answer is `pages >> 1`. An even page
    # count separates the lower-middle from the upper-middle, and page 2 of 5
    # separates both from a coarser step. These come first because the mutation
    # stage scores only the first 48 cases.
    for pages in (2, 3, 5, 1, 17):
        keys = [_repeated(0x40, 16)] * pages
        out.append(_build(keys, 16, _repeated(0x40, 16), 16))
        out.append(_build(keys, 16, _repeated(0x3F, 16), 16))
        out.append(_build(keys, 16, _repeated(0x41, 16), 16))

    # A prefix-shared pair: equal over a short compare length, ordered over the
    # full width.
    for width in (2, 4, 8, 16):
        low = bytearray(_repeated(0x10, width))
        high = bytearray(low)
        high[width - 1] = 0x11
        keys = [bytes(low), bytes(high)]
        for key_len in range(1, width + 1):
            out.append(_build(keys, width, bytes(low), key_len))

    # No pages: the miss is 0xFFFFFFFF and the index is never read.
    out.append(_build([], 16, _repeated(0x40, 16), 16))
    out.append(_build([], 1, _repeated(0x40, 1), 0))
    out.append(_build([], 4, _repeated(0xFF, 4), 4))

    # One to seventeen pages of distinct first keys, probed below, at, between
    # and above them.
    for width in WIDTHS:
        for pages in (1, 2, 3, 5, 17):
            keys = _increasing(pages, width)
            last = keys[-1][0]
            middle = keys[pages // 2][0]
            queries = [
                _repeated(0x00, width),  # below every page
                keys[0],  # the first page exactly
                _repeated(keys[0][0] + 1, width),  # between page 0 and 1
                keys[pages - 1],  # the last page exactly
                _repeated(last + 1, width),  # above every page
            ]
            if pages >= 3:
                queries.append(_repeated(middle - 1, width))
                queries.append(_repeated(middle + 1, width))
            for query in queries:
                out.append(_build(keys, width, query, width))

    # A last page keyed 0xFF..FF.
    for width in WIDTHS:
        keys = _increasing(2, width) + [_repeated(0xFF, width)]
        out.append(_build(keys, width, _repeated(0xFF, width), width))
        out.append(_build(keys, width, _repeated(0xFE, width), width))
        out.append(_build(keys, width, keys[1], width))

    # The compare length swept from 0 to width + 1 against three pages.
    for width in WIDTHS:
        keys = _increasing(3, width, start=0x20, step=0x20)
        for key_len in range(0, width + 2):
            out.append(_build(keys, width, _repeated(0x40, width), key_len))
            out.append(_build(keys, width, _repeated(0x00, width), key_len))
    return out


SPECIALS = _cases()


def _lcg(seed: int, n: int) -> int:
    x = seed & 0xFFFFFFFF
    for _ in range(n):
        x = (x * 1664525 + 1013904223) & 0xFFFFFFFF
    return x >> 24


def _sweep(r: int) -> Case:
    width = WIDTHS[r % len(WIDTHS)]
    pages = r % 6
    step = 4 + (r % 5)
    keys = [_repeated(0x10 + step * i, width) for i in range(pages)]
    if pages >= 2 and r % 3 == 0:
        # Share a prefix with the previous page: the compare length decides
        # whether the two entries are equal.
        shared = bytearray(keys[pages - 1])
        shared[0] = keys[pages - 2][0]
        keys[pages - 1] = bytes(shared)
    key_len = r % (width + 2)
    query = bytes(_lcg(0x70000 + r, k) for k in range(width))
    if pages and r % 2 == 0:
        # Bias half the sweep onto a real page first key.
        target = bytearray(keys[(r >> 1) % pages])
        query = bytes(target)
        if r % 4 == 0 and key_len:
            target[0] = (target[0] + 1) & 0xFF
            query = bytes(target)
    return _build(keys, width, query, key_len)


def make_case(i: int) -> Case:
    if i < len(SPECIALS):
        return SPECIALS[i]
    return _sweep(i - len(SPECIALS))


def make_args(i: int) -> List[ArgSpec]:
    index, pages, width, key, key_len = make_case(i)
    return [
        ArgSpec(kind="ptr", mem=index, arena_offset=INDEX_OFF),
        ArgSpec(kind="u64", bits=pages),
        ArgSpec(kind="u64", bits=width),
        ArgSpec(kind="ptr", mem=key, arena_offset=KEY_OFF),
        ArgSpec(kind="u64", bits=key_len),
    ]

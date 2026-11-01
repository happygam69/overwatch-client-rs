"""Deterministic inputs for the `fn/EncodingFindEKey` unit.

The client's ekey lookup (`FUN_142498610`, exe `0x142498610`) takes the same
0x70-byte encoding-info struct as the ckey side, the key, the key length and an
out slot. This module builds a self-contained info buffer: the header fields at
their client offsets, the ekey page index after the struct, and the
concatenated page bodies after the index. The section pointers are absolute
arena addresses; `+0x68` holds the info buffer's own arena address (never read
by the lookup) so the probe can translate the pointers into offsets.

A record is `ekey[width]`, a big-endian `u32` espec index and a big-endian
`u40` stored size; an espec index of `0xFFFFFFFF` ends the page. The page
search compares the caller's full key length, while the record scan compares
`min(key_len, width)` bytes.
"""

from __future__ import annotations

from typing import List, Optional, Sequence, Tuple

from emu_oracle import ARENA_BASE, ArgSpec

INFO_OFF = 0
KEY_OFF = 0x400
OUT_OFF = 0x420
INDEX_OFF = 0x70


def _key(byte: int, width: int) -> bytes:
    return bytes([byte]) * width


def _entry(first: bytes, width: int, hash_fill: int) -> bytes:
    entry = bytearray([hash_fill]) * (width + 16)
    entry[:width] = first
    return bytes(entry)


def _build(
    pages: Sequence[Sequence[Tuple[bytes, int, bytes]]],
    width: int,
    page_bytes: int,
    query: bytes,
    key_len: int,
    *,
    after_terminator: Optional[Tuple[bytes, int, bytes]] = None,
) -> Tuple[bytes, bytes, int]:
    index_len = len(pages) * (width + 16)
    bodies_len = len(pages) * page_bytes
    info = bytearray(0x70)
    info[0x10] = 1
    info[0x11] = width
    info[0x12] = width
    info[0x1c:0x20] = len(pages).to_bytes(4, "little")
    info[0x20:0x24] = page_bytes.to_bytes(4, "little")
    info[0x40:0x48] = (ARENA_BASE + INDEX_OFF).to_bytes(8, "little")
    info[0x48:0x50] = (ARENA_BASE + INDEX_OFF + index_len).to_bytes(8, "little")
    info[0x68:0x70] = ARENA_BASE.to_bytes(8, "little")

    index = bytearray(index_len)
    bodies = bytearray(bodies_len)
    for page, records in enumerate(pages):
        first = records[0][0] if records else bytes(width)
        at = page * (width + 16)
        index[at : at + width + 16] = _entry(first, width, 0x80 + page)
        body_at = page * page_bytes
        p = 0
        for ekey, espec, size in records:
            bodies[body_at + p : body_at + p + width] = ekey
            bodies[body_at + p + width : body_at + p + width + 4] = espec.to_bytes(4, "big")
            bodies[body_at + p + width + 4 : body_at + p + width + 9] = size
            p += width + 9
        bodies[body_at + p : body_at + p + 4] = (0xFFFFFFFF).to_bytes(4, "big")
        p += 4
        if after_terminator is not None:
            ekey, espec, size = after_terminator
            bodies[body_at + p : body_at + p + width] = ekey
            bodies[body_at + p + width : body_at + p + width + 4] = espec.to_bytes(4, "big")
            bodies[body_at + p + width + 4 : body_at + p + width + 9] = size
    mem = bytes(info) + bytes(index) + bytes(bodies)
    assert len(mem) <= 0x400, len(mem)
    # A 16-byte key buffer makes a `len > key.len()` mutation observable at
    # len == 16; the width+1 quirk case needs one extra byte for the search.
    key = bytearray(max(16, len(query), key_len))
    key[: len(query)] = query
    return mem, bytes(key), key_len


def _size(value: int) -> bytes:
    return value.to_bytes(5, "big")


def _cases() -> List[Tuple[bytes, bytes, int]]:
    out: List[Tuple[bytes, bytes, int]] = []

    # Empty index.
    out.append(_build([], 16, 128, _key(0xA0, 16), 16))

    one = [[(_key(0xA0, 16), 0, _size(0x0102))]]
    for value in (0x9F, 0xA0, 0xA1, 0xFF):
        out.append(_build(one, 16, 128, _key(value, 16), 16))

    three = [
        [(_key(0xA0, 16), 0, _size(0x0100))],
        [(_key(0xB0, 16), 1, _size(0x0200)), (_key(0xB1, 16), 2, _size(0x0201))],
        [(_key(0xC0, 16), 3, _size(0x0300))],
    ]
    for value in (0x9F, 0xA0, 0xA1, 0xAF, 0xB0, 0xB1, 0xBF, 0xC0, 0xC1, 0xFF):
        out.append(_build(three, 16, 128, _key(value, 16), 16))

    # A record after the 0xFFFFFFFF terminator is unreachable.
    out.append(
        _build(
            [[(_key(0xA0, 16), 0, _size(1))]],
            16,
            128,
            _key(0xA1, 16),
            16,
            after_terminator=(_key(0xA1, 16), 4, _size(2)),
        )
    )
    # A terminator at the very first record: the page scan stops before any
    # key is compared.
    info, key, key_len = _build(
        [[(_key(0xA0, 16), 1, _size(1))]], 16, 128, _key(0xA0, 16), 16
    )
    index_len = 1 * (16 + 16)
    body_at = 0x70 + index_len
    patched = bytearray(info)
    patched[body_at + 16 : body_at + 20] = (0xFFFFFFFF).to_bytes(4, "big")
    out.append((bytes(patched), key, key_len))
    # A zero page size: the body start equals the body end and the client
    # returns the miss without entering the scan.
    zero_info = bytearray(0x70)
    zero_info[0x10] = 1
    zero_info[0x11] = 16
    zero_info[0x12] = 16
    zero_info[0x1c:0x20] = (1).to_bytes(4, "little")
    zero_info[0x20:0x24] = (0).to_bytes(4, "little")
    zero_info[0x40:0x48] = (ARENA_BASE + INDEX_OFF).to_bytes(8, "little")
    zero_info[0x48:0x50] = (ARENA_BASE + INDEX_OFF + 32).to_bytes(8, "little")
    zero_info[0x68:0x70] = ARENA_BASE.to_bytes(8, "little")
    zero_index = _entry(_key(0xA0, 16), 16, 0x80)
    out.append((bytes(zero_info) + zero_index, _key(0xA0, 16), 16))
    # Duplicate keys: the first record wins.
    out.append(
        _build(
            [[(_key(0xA0, 16), 1, _size(1)), (_key(0xA0, 16), 2, _size(2))]],
            16,
            128,
            _key(0xA0, 16),
            16,
        )
    )

    # Short compare lengths: the record scan uses min(key_len, width).
    out.append(_build(three, 16, 128, _key(0xA0, 16), 0))
    out.append(_build(three, 16, 128, b"\xA0", 1))
    out.append(_build(three, 16, 128, b"\xA0\x00", 2))

    # Full-length search, short scan: key_len = width + 1 compares the first
    # hash byte in the page search. Page 0's hash fill is 0x80, so a query equal
    # to page 0's first key plus 0x80 exact-hits page 0 and the scan compares
    # only the 16 key bytes.
    full = _key(0xA0, 16) + b"\x80"
    out.append(_build(three, 16, 128, full, 17))
    # The same extra byte against page 1's 0x81 hash must not exact-hit page 1;
    # the first key is different anyway.
    out.append(_build(three, 16, 128, _key(0xB0, 16) + b"\x81", 17))

    for width in (1, 2, 4, 8):
        pages = [[(_key(0xA0 + i, width), i, _size(0x100 + i))] for i in range(3)]
        for value in (0x9F, 0xA0, 0xA1, 0xAF, 0xB0, 0xB1, 0xFF):
            out.append(_build(pages, width, 128, _key(value, width), width))
        out.append(_build(pages, width, 128, b"\xA0", 1))
    return out


SPECIALS = _cases()


def _sweep(r: int) -> Tuple[bytes, bytes, int]:
    widths = (1, 2, 4, 8, 16)
    width = widths[r % len(widths)]
    pages_n = r % 4
    pages = []
    for page in range(pages_n):
        first = _key(0xA0 + 0x10 * page, width)
        second = _key(0xA8 + 0x10 * page, width)
        pages.append(
            [
                (first, page, _size(0x100 + page)),
                (second, page + 1, _size(0x180 + page)),
            ]
        )
    value = ((r * 53) & 0xFF) or 1
    key_len = r % (width + 2)
    return _build(pages, width, 128, _key(value, width), key_len)


def make_case(i: int) -> Tuple[bytes, bytes, int]:
    if i < len(SPECIALS):
        return SPECIALS[i]
    return _sweep(i - len(SPECIALS))


def make_args(i: int) -> List[ArgSpec]:
    info, key, key_len = make_case(i)
    return [
        ArgSpec(kind="ptr", mem=info, arena_offset=INFO_OFF),
        ArgSpec(kind="ptr", mem=key, arena_offset=KEY_OFF),
        ArgSpec(kind="u64", bits=key_len),
        ArgSpec(kind="ptr", mem=bytes(8), arena_offset=OUT_OFF),
    ]

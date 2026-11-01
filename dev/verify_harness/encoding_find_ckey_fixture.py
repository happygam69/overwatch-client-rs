"""Deterministic inputs for the `fn/EncodingFindCKey` unit.

The client's ckey lookup (`FUN_142498540`, exe `0x142498540`) takes the
0x70-byte encoding-info struct, the key, the key length and an out slot. This
module builds a self-contained info buffer: the header fields at their client
offsets, the ckey page index immediately after the 0x70-byte struct, and the
concatenated page bodies after the index. The section pointers are absolute
arena addresses, as in the client; `+0x68` holds the info buffer's own arena
address (the lookup never reads that slot) so the probe can translate the
pointers into offsets in its copy.

Every record is `u16 count`, `u32 BE decoded size`, `ckey[width]`, then
`count` ekeys of `ekey_width` bytes; a zero low count byte ends the page.
The out slot receives the record's absolute guest address, exactly as the
client writes it, and zero on a miss.
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
    pages: Sequence[Sequence[Tuple[bytes, Sequence[bytes]]]],
    ck_width: int,
    ek_width: int,
    page_bytes: int,
    query: bytes,
    key_len: int,
    *,
    terminator: bool = True,
    after_terminator: Optional[Tuple[bytes, Sequence[bytes]]] = None,
    count_high: int = 0,
) -> Tuple[bytes, bytes, int]:
    index_len = len(pages) * (ck_width + 16)
    bodies_len = len(pages) * page_bytes
    info = bytearray(0x70)
    info[0x10] = 1  # ready; the find function does not read it
    info[0x11] = ck_width
    info[0x12] = ek_width
    info[0x14:0x18] = len(pages).to_bytes(4, "little")
    info[0x18:0x1c] = page_bytes.to_bytes(4, "little")
    info[0x30:0x38] = (ARENA_BASE + INDEX_OFF).to_bytes(8, "little")
    info[0x38:0x40] = (ARENA_BASE + INDEX_OFF + index_len).to_bytes(8, "little")
    info[0x68:0x70] = ARENA_BASE.to_bytes(8, "little")

    index = bytearray(index_len)
    bodies = bytearray(bodies_len)
    for page, records in enumerate(pages):
        first = records[0][0] if records else bytes(ck_width)
        at = page * (ck_width + 16)
        index[at : at + ck_width + 16] = _entry(first, ck_width, 0x80 + page)
        body_at = page * page_bytes
        p = 0
        for record_key, ekeys in records:
            count = len(ekeys)
            bodies[body_at + p] = count & 0xFF
            bodies[body_at + p + 1] = count_high & 0xFF
            bodies[body_at + p + 2 : body_at + p + 6] = (0x1000 + page).to_bytes(4, "big")
            bodies[body_at + p + 6 : body_at + p + 6 + ck_width] = record_key
            q = p + 6 + ck_width
            for ekey in ekeys:
                bodies[body_at + q : body_at + q + ek_width] = ekey
                q += ek_width
            p = q
        if terminator:
            bodies[body_at + p] = 0
            # A nonzero high byte is not read by the client's low-byte test.
            bodies[body_at + p + 1] = 0x01
            p += 2
        if after_terminator is not None:
            record_key, ekeys = after_terminator
            bodies[body_at + p] = len(ekeys)
            bodies[body_at + p + 1] = 0
            bodies[body_at + p + 2 : body_at + p + 6] = (0x2000 + page).to_bytes(4, "big")
            bodies[body_at + p + 6 : body_at + p + 6 + ck_width] = record_key
            q = p + 6 + ck_width
            for ekey in ekeys:
                bodies[body_at + q : body_at + q + ek_width] = ekey
                q += ek_width
    mem = bytes(info) + bytes(index) + bytes(bodies)
    assert len(mem) <= 0x400, len(mem)
    key = bytearray(16)
    key[: len(query)] = query
    return mem, bytes(key), key_len


def _cases() -> List[Tuple[bytes, bytes, int]]:
    out: List[Tuple[bytes, bytes, int]] = []

    # Empty index.
    out.append(_build([], 16, 16, 128, _key(0x40, 16), 16))

    one = [[(_key(0x40, 16), [_key(0xA0, 16)])]]
    out.append(_build(one, 16, 16, 128, _key(0x3F, 16), 16))
    out.append(_build(one, 16, 16, 128, _key(0x40, 16), 16))
    out.append(_build(one, 16, 16, 128, _key(0x41, 16), 16))
    out.append(_build(one, 16, 16, 128, _key(0xFF, 16), 16))

    three = [
        [(_key(0x10, 16), [_key(0xA0, 16)])],
        [(_key(0x20, 16), [_key(0xB0, 16), _key(0xB1, 16)])],
        [(_key(0x40, 16), [_key(0xC0, 16)]), (_key(0x30, 16), [_key(0xC1, 16)])],
    ]
    for value in (0x0F, 0x10, 0x11, 0x1F, 0x20, 0x2F, 0x30, 0x3F, 0x40, 0x41, 0xFF):
        out.append(_build(three, 16, 16, 128, _key(value, 16), 16))

    # The low count byte ends the page; the record placed after the terminator
    # must not be reachable.
    out.append(
        _build(
            [[(_key(0x10, 16), [_key(0xA0, 16)])]],
            16,
            16,
            128,
            _key(0x20, 16),
            16,
            after_terminator=(_key(0x20, 16), [_key(0xD0, 16)]),
        )
    )
    # A nonzero high count byte is ignored: the low byte is one, and the next
    # record starts after one ekey, not 257.
    out.append(
        _build(
            [[(_key(0x10, 16), [_key(0xA0, 16)]), (_key(0x20, 16), [_key(0xB0, 16)])]],
            16,
            16,
            128,
            _key(0x20, 16),
            16,
            count_high=0x01,
        )
    )
    # A miss on the first record then a hit on the second: the loop-continue
    # edge and the following record are both exercised.
    out.append(
        _build(
            [[(_key(0x10, 16), [_key(0xA0, 16)]), (_key(0x11, 16), [_key(0xA1, 16)])]],
            16,
            16,
            128,
            _key(0x11, 16),
            16,
        )
    )
    # A zero page size: the body start equals the body end and the client
    # returns the miss without entering the scan.
    zero_info = bytearray(0x70)
    zero_info[0x10] = 1
    zero_info[0x11] = 16
    zero_info[0x12] = 16
    zero_info[0x14:0x18] = (1).to_bytes(4, "little")
    zero_info[0x18:0x1c] = (0).to_bytes(4, "little")
    zero_info[0x30:0x38] = (ARENA_BASE + INDEX_OFF).to_bytes(8, "little")
    zero_info[0x38:0x40] = (ARENA_BASE + INDEX_OFF + 32).to_bytes(8, "little")
    zero_info[0x68:0x70] = ARENA_BASE.to_bytes(8, "little")
    zero_index = _entry(_key(0x10, 16), 16, 0x80)
    out.append((bytes(zero_info) + zero_index, _key(0x10, 16), 16))
    # The last record ends exactly at the page end (no terminator): the
    # next-record comparison falls through to the miss.
    out.append(
        _build(
            [[(_key(0x10, 16), [_key(0xA0, 16)])]],
            16,
            16,
            38,
            _key(0x11, 16),
            16,
            terminator=False,
        )
    )
    # One spare byte after the last record: the loop-continue edge is taken,
    # then the zero-filled byte terminates the scan.
    out.append(
        _build(
            [[(_key(0x10, 16), [_key(0xA0, 16)])]],
            16,
            16,
            39,
            _key(0x11, 16),
            16,
            terminator=False,
        )
    )
    # Duplicate keys: the first record wins.
    out.append(
        _build(
            [[(_key(0x10, 16), [_key(0xA0, 16)]), (_key(0x10, 16), [_key(0xA1, 16)])]],
            16,
            16,
            128,
            _key(0x10, 16),
            16,
        )
    )
    # Short compare lengths: only the requested prefix is compared.
    out.append(_build(three, 16, 16, 128, _key(0x10, 16), 0))
    out.append(_build(three, 16, 16, 128, b"\x10", 1))
    out.append(_build(three, 16, 16, 128, b"\x10\x00", 2))
    out.append(_build(three, 16, 16, 128, b"\x11", 1))

    # Narrow keys: the same lookup at widths 1/2/4/8, with the ekey width equal
    # to the ckey width.
    for width in (1, 2, 4, 8):
        pages = [
            [(_key(0x10 + i, width), [_key(0xA0 + i, width)])] for i in range(3)
        ]
        for value, key_len in ((0x0F, width), (0x10, width), (0x11, width), (0x30, width)):
            out.append(_build(pages, width, width, 128, _key(value, width), key_len))
        out.append(_build(pages, width, width, 128, b"\x10", 1))
    return out


SPECIALS = _cases()


def _sweep(r: int) -> Tuple[bytes, bytes, int]:
    widths = (1, 2, 4, 8, 16)
    width = widths[r % len(widths)]
    pages_n = r % 4
    pages = []
    for page in range(pages_n):
        first = _key(0x10 + 0x10 * page, width)
        second = _key(0x18 + 0x10 * page, width)
        pages.append([(first, [_key(0xA0 + page, width)]), (second, [_key(0xB0 + page, width)])])
    value = ((r * 37) & 0xFF) or 1
    key_len = r % (width + 1)
    return _build(pages, width, width, 128, _key(value, width), key_len)


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

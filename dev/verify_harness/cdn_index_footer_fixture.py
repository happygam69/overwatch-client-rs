"""Deterministic inputs for the `fn/CdnIndexFooterParse` unit.

The client's `FUN_1424d9af0` (`0x1424d9af0`) takes a buffer, its length, the
whole index file's length and a 0x30-byte output struct. It reads the last 20
bytes as the footer, checks the footer's own MD5, then derives page size,
entry stride, entries per page, page count, last-key bytes, checksum bytes and
the exact file length. The oracle is the client's own bytes; this module only
builds inputs.

The first cases are the four real footers of this install's CDN config (the
archive group, patch group and two loose-file indexes), each with its real file
length. Then every gate at both edges: version 0/1/2, checksum size 0/16/17,
zero and maximum page-size KiB, zero entry components, element counts that
cross the page boundary, and lengths one byte either side of the formula.
"""

from __future__ import annotations

import hashlib
from typing import List, Tuple

from emu_oracle import ArgSpec

OUT_LEN = 0x30
DATA_LEN = 64
OUT_FILL = 0xA5
DATA_FILL = 0x5A


def _footer(
    version: int = 1,
    page_kb: int = 4,
    offset_bytes: int = 6,
    size_bytes: int = 4,
    key_bytes: int = 16,
    checksum_size: int = 8,
    elements: int = 1,
) -> bytes:
    header = bytes(
        [
            version & 0xFF,
            0,
            0,
            page_kb & 0xFF,
            offset_bytes & 0xFF,
            size_bytes & 0xFF,
            key_bytes & 0xFF,
            checksum_size & 0xFF,
            elements & 0xFF,
            (elements >> 8) & 0xFF,
            (elements >> 16) & 0xFF,
            (elements >> 24) & 0xFF,
        ]
    )
    digest = hashlib.md5(header + bytes(8)).digest()
    return header + digest[:8]


def _geometry(footer: bytes) -> Tuple[int, int, int, int]:
    """(page size, entry size, entries per page, page count), 32-bit."""
    page_size = (footer[3] << 10) & 0xFFFFFFFF
    entry_size = footer[4] + footer[5] + footer[6]
    epp = page_size // entry_size if entry_size else 0
    elements = int.from_bytes(footer[8:12], "little")
    pages = ((elements - 1 + epp) & 0xFFFFFFFF) // epp if epp else 0
    return page_size, entry_size, epp, pages


def _total(footer: bytes) -> int:
    page_size, _entry, _epp, pages = _geometry(footer)
    return (
        footer[7]
        + 20
        + ((footer[7] + footer[6] + page_size) * pages & 0xFFFFFFFF)
    ) & 0xFFFFFFFF


def _case(footer: bytes, total: int, prefix: int = 0) -> Tuple[bytes, int, int]:
    data = bytes([DATA_FILL]) * prefix + footer
    return data, len(data), total


def _cases() -> List[Tuple[bytes, int, int]]:
    out: List[Tuple[bytes, int, int]] = []

    # The four real index footers and their exact file lengths.
    real = [
        ("0100000406041008d847970055a616cfef3af752", 260_173_908),
        ("01000004050410081e591d00e8d1710213451589", 48_616_028),
        ("0100000400041008d66e0000f82dc775fc579b5b", 576_828),
        ("01000004000410084a110000909b13e32eae4b30", 90_668),
    ]
    for hex_footer, file_len in real:
        footer = bytes.fromhex(hex_footer)
        out.append(_case(footer, file_len))
        out.append(_case(footer, file_len, prefix=44))

    # Version 2 rejects; 0 and 1 accept.
    for version in (0, 1, 2, 3, 0xFF):
        footer = _footer(version=version, elements=1)
        out.append(_case(footer, _total(footer)))

    # Checksum size 17 rejects; 0, 1, 16 accept when the length matches.
    for ck in (0, 1, 16, 17, 0xFF):
        footer = _footer(checksum_size=ck, elements=1)
        out.append(_case(footer, _total(footer)))

    # Page size 0 rejects; 1 KiB and 255 KiB accept when the length matches.
    for page_kb in (0, 1, 2, 255):
        footer = _footer(page_kb=page_kb, elements=1)
        out.append(_case(footer, _total(footer)))

    # Entry components: any combination with a zero sum rejects; key width 0
    # with a non-zero size and offset still accepts (the leaf does not require
    # 16-byte keys).
    for off, size, key in [(0, 0, 0), (0, 0, 16), (0, 4, 16), (6, 0, 16), (6, 4, 0), (0, 0, 255)]:
        footer = _footer(offset_bytes=off, size_bytes=size, key_bytes=key, elements=1)
        out.append(_case(footer, _total(footer)))

    # Element counts around one page and a partial last page.
    for elements in (0, 1, 156, 157, 158, 314, 315, 0xFFFFFFFF):
        footer = _footer(elements=elements)
        out.append(_case(footer, _total(footer)))

    # The length gate: both edges are executed. The mismatch writes the same
    # six fields and only AL differs, so these cases contribute coverage but
    # not a memory-channel distinction (declared as a gap).
    footer = _footer(elements=158)
    total = _total(footer)
    out.append(_case(footer, total - 1))
    out.append(_case(footer, total + 1))
    out.append(_case(footer, 0))

    # A wrong header MD5 with an otherwise valid length, so the hash branch is
    # reached first; the output must keep the hash bytes zeroed on this path.
    footer = bytearray(_footer(elements=1))
    footer[12] ^= 1
    out.append(_case(bytes(footer), _total(bytes(footer))))
    return out


SPECIALS = _cases()


def _sweep(r: int) -> Tuple[bytes, int, int]:
    """A deterministic footer with a correct header hash after the pinned set."""
    state = (0xD0000 + r) & 0xFFFFFFFF

    def next_byte() -> int:
        nonlocal state
        state = (state * 1664525 + 1013904223) & 0xFFFFFFFF
        return state >> 24

    header = bytes(next_byte() for _ in range(12))
    digest = hashlib.md5(header + bytes(8)).digest()
    footer = header + digest[:8]
    # The length is derived from the same footer, so the sweep varies geometry
    # and element counts rather than only the invisible length status.
    total = _total(footer)
    return _case(footer, total, prefix=r % 20)[0:3]


def make_case(i: int) -> Tuple[bytes, int, int]:
    if i < len(SPECIALS):
        return SPECIALS[i]
    return _sweep(i - len(SPECIALS))


def make_args(i: int) -> List[ArgSpec]:
    data, length, total = make_case(i)
    data = data + bytes([DATA_FILL]) * (DATA_LEN - len(data))
    return [
        ArgSpec(kind="ptr", mem=data, arena_offset=0),
        ArgSpec(kind="u64", bits=length),
        ArgSpec(kind="u64", bits=total),
        ArgSpec(kind="ptr", mem=bytes([OUT_FILL]) * OUT_LEN, arena_offset=0x100),
    ]

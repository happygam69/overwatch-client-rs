"""Deterministic (key, file, len, out) cases for `fn/CdnIndexFileParse`.

The client's `CdnIndexFileParse` (`0x1424d9c90`) validates a whole CDN index
file in four stages and returns its `AL`: the footer block (`parse_index_footer`
- header MD5, version, checksum size, geometry, exact length), the terminal
checksum `MD5(last keys + checksums)`, the index's own key `MD5(terminal +
footer)[..16]`, and finally the per-page MD5 pass. Every stage's reject is
status 8; the page pass's status (0/3/8) is returned unchanged.

The corpus pins each stage with a file that is valid except for that one stage,
so a stage skipped or reordered changes an answer:

* a valid file (determinism case) and larger geometry variants;
* a header byte flipped after signing (header MD5), a signed version 2, a
  signed checksum size of 17, a signed page size of 0, a signed entry size of
  0, and a signed element count that implies a different page count;
* a broken terminal checksum (with the key recomputed so only that stage can
  reject), a wrong key, and a broken page body;
* lengths below the 20-byte footer and below the page bodies.

The `out` block starts as `0xA5` in every case, so the client's writes (the
20-byte footer copy, the derived fields, and the hash bytes it zeroes on a
header rejection) are all visible on the memory channel.
"""

from __future__ import annotations

import hashlib
from typing import List, Tuple

from emu_oracle import ArgSpec

FOOTER_SIZE = 20
OUT_OFFSET = 0xC000
KEY_OFFSET = 0x9000
SENTINEL = 0xA5


def _lcg_bytes(seed: int, n: int) -> bytes:
    """Version-independent deterministic bytes (a plain 32-bit LCG)."""
    x = seed & 0xFFFFFFFF
    out = bytearray(n)
    for i in range(n):
        x = (x * 1664525 + 1013904223) & 0xFFFFFFFF
        out[i] = x >> 24
    return bytes(out)


def _sign(footer: bytearray) -> None:
    """Footer bytes 12..20 = MD5(footer[0..12] + 8 zeros)[..8]."""
    header = bytes(footer[:12]) + bytes(8)
    footer[12:20] = hashlib.md5(header).digest()[:8]


def index_file(
    page_size: int = 1024,
    offset_bytes: int = 6,
    size_bytes: int = 4,
    key_bytes: int = 16,
    checksum_size: int = 8,
    pages: int = 2,
    elements: int | None = None,
    version: int = 1,
    seed: int = 0x10,
) -> Tuple[bytes, bytes]:
    """A complete index file and its own 16-byte key.

    The file is `[bodies][last keys][checksums][terminal][footer]`, exactly the
    layout `CdnIndexFileParse` validates.
    """
    entry_size = offset_bytes + size_bytes + key_bytes
    entries_per_page = page_size // entry_size
    if elements is None:
        elements = pages * entries_per_page
    bodies = _lcg_bytes(seed, page_size * pages)
    keys = _lcg_bytes(seed ^ 0x5A5A, pages * key_bytes)
    checksums = b"".join(
        hashlib.md5(bodies[p * page_size : (p + 1) * page_size]).digest()[:checksum_size]
        for p in range(pages)
    )
    terminal = hashlib.md5(keys + checksums).digest()[:checksum_size]
    footer = bytearray(FOOTER_SIZE)
    footer[0] = version
    footer[3] = page_size >> 10
    footer[4] = offset_bytes
    footer[5] = size_bytes
    footer[6] = key_bytes
    footer[7] = checksum_size
    footer[8:12] = elements.to_bytes(4, "little")
    _sign(footer)
    data = bodies + keys + checksums + terminal + bytes(footer)
    key = hashlib.md5(terminal + bytes(footer)).digest()[:16]
    return data, key


def _flip_byte(data: bytes, at: int) -> bytes:
    out = bytearray(data)
    out[at] ^= 0x01
    return bytes(out)


def _set_and_sign(data: bytes, edits: dict[int, int]) -> bytes:
    """Set footer bytes (offsets relative to the footer), then re-sign."""
    out = bytearray(data)
    base = len(out) - FOOTER_SIZE
    for at, value in edits.items():
        out[base + at] = value
    footer = bytearray(out[base:])
    _sign(footer)
    out[base:] = footer
    return bytes(out)


def _own_key(data: bytes) -> bytes:
    """`MD5(terminal + footer)[..16]` for the file as it now stands."""
    terminal = data[len(data) - FOOTER_SIZE - 8 : len(data) - FOOTER_SIZE]
    return hashlib.md5(terminal + data[len(data) - FOOTER_SIZE :]).digest()[:16]


# (key, file, len, out sentinel)
Case = Tuple[bytes, bytes, int, bytes]

_OUT = bytes([SENTINEL]) * 0x30

_VALID, _VALID_KEY = index_file()
_VALID_LEN = len(_VALID)

# A larger geometry: three pages of 2 KiB, 9-byte keys, 16-byte checksums.
_BIG, _BIG_KEY = index_file(page_size=2048, key_bytes=9, checksum_size=16, pages=3, seed=0x21)

# One page only.
_ONE, _ONE_KEY = index_file(page_size=1024, pages=1, seed=0x22)

# A file whose (signed) element count implies more pages than it holds: the
# exact-length check rejects.
_EL_MISMATCH = bytearray(_VALID)
_elements = int.from_bytes(_EL_MISMATCH[-12:-8], "little") + 200
_EL_MISMATCH[-12:-8] = _elements.to_bytes(4, "little")
_EL_MISMATCH = _set_and_sign(bytes(_EL_MISMATCH), {})

# A broken terminal, with the key recomputed from it so only the terminal
# stage can reject.
_BAD_TERMINAL = _flip_byte(_VALID, _VALID_LEN - FOOTER_SIZE - 8)
_BAD_TERMINAL_KEY = _own_key(_BAD_TERMINAL)

# A broken page body: every earlier stage still passes.
_BAD_PAGE = _flip_byte(_VALID, 0)

_WRONG_KEY = bytearray(_VALID_KEY)
_WRONG_KEY[0] ^= 0x01
_WRONG_KEY = bytes(_WRONG_KEY)

_SPECIAL_VERSION2, _KEY_VERSION2 = index_file(version=2)
_SPECIAL_VERSION2_LEN = len(_SPECIAL_VERSION2)
_SPECIAL_CK17, _KEY_CK17 = index_file(checksum_size=17)
_SPECIAL_CK17_LEN = len(_SPECIAL_CK17)

SPECIALS: List[Case] = [
    # Determinism case: a complete, self-consistent file.
    (_VALID_KEY, _VALID, _VALID_LEN, _OUT),
    # The larger geometry (three pages of 2 KiB, 9-byte keys, 16-byte checksums).
    (_BIG_KEY, _BIG, len(_BIG), _OUT),
    # One page: the page pass runs once.
    (_ONE_KEY, _ONE, len(_ONE), _OUT),
    # Header MD5: a header byte flipped after signing. The hash bytes in the
    # derived block stay zeroed.
    (_VALID_KEY, _flip_byte(_VALID, _VALID_LEN - FOOTER_SIZE), _VALID_LEN, _OUT),
    # Version 2 and checksum size 17, both correctly signed, so the header
    # check passes and the reject is the field check.
    (_KEY_VERSION2, _SPECIAL_VERSION2, _SPECIAL_VERSION2_LEN, _OUT),
    (_KEY_CK17, _SPECIAL_CK17, _SPECIAL_CK17_LEN, _OUT),
    # Page size 0 and entry size 0, signed after the edit.
    (_VALID_KEY, _set_and_sign(_VALID, {3: 0}), _VALID_LEN, _OUT),
    (_VALID_KEY, _set_and_sign(_VALID, {4: 0, 5: 0, 6: 0}), _VALID_LEN, _OUT),
    # A signed element count that implies more pages: the length check.
    (_VALID_KEY, _EL_MISMATCH, _VALID_LEN, _OUT),
    # A broken terminal with a matching key: only the terminal stage rejects.
    (_BAD_TERMINAL_KEY, _BAD_TERMINAL, _VALID_LEN, _OUT),
    # A wrong key: every earlier stage passes.
    (_WRONG_KEY, _VALID, _VALID_LEN, _OUT),
    # A broken page body: the last stage rejects with 8.
    (_VALID_KEY, _BAD_PAGE, _VALID_LEN, _OUT),
    # Shorter than the 20-byte footer: nothing is written to the derived block.
    (_VALID_KEY, _VALID, FOOTER_SIZE - 1, _OUT),
    (_VALID_KEY, _VALID, 0, _OUT),
    # Exactly a real file's footer, signed, but its element count implies many
    # pages, so the length check rejects.
    (_VALID_KEY, _VALID[_VALID_LEN - FOOTER_SIZE :], FOOTER_SIZE, _OUT),
]


def make_case(i: int) -> Case:
    """The i-th (key, file, len, out) case. Pure function of i."""
    if i < len(SPECIALS):
        return SPECIALS[i]
    r = i - len(SPECIALS)
    page_size = [1024, 2048, 4096][r % 3]
    key_bytes = [9, 16][(r // 3) % 2]
    checksum_size = [4, 8, 16][(r // 6) % 3]
    pages = [1, 2, 3, 4][(r // 18) % 4]
    offset_bytes = [0, 5, 6][(r // 72) % 3]
    data, key = index_file(
        page_size=page_size,
        offset_bytes=offset_bytes,
        key_bytes=key_bytes,
        checksum_size=checksum_size,
        pages=pages,
        seed=0x30000 + r,
    )
    length = len(data)
    if r % 5 == 0:
        # A header byte flipped after signing.
        data = _flip_byte(data, length - FOOTER_SIZE + (r % 12))
    elif r % 7 == 1:
        # A broken page body.
        data = _flip_byte(data, r % (page_size * pages))
    elif r % 11 == 2:
        # One byte short of the declared length: the terminal moves, so the
        # header check rejects first.
        length -= 1
    elif r % 13 == 3:
        # A wrong key.
        wrong = bytearray(key)
        wrong[r % 16] ^= 0x01
        key = bytes(wrong)
    return (key, data, length, _OUT)


def make_args(i: int) -> List[ArgSpec]:
    key, data, length, out = make_case(i)
    return [
        ArgSpec(kind="ptr", mem=key, arena_offset=KEY_OFFSET),
        ArgSpec(kind="ptr", mem=data, arena_offset=0),
        ArgSpec(kind="u64", bits=length),
        ArgSpec(kind="ptr", mem=out, arena_offset=OUT_OFFSET),
    ]

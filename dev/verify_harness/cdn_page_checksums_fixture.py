"""Deterministic (file, len, descriptor) cases for `fn/CdnIndexPageChecksums`.

The client's `CdnIndexPageChecksums` (`0x1424d9f20`) takes the whole index
file, the length the caller declares, and the 0x30-byte derived-footer block
`CdnIndexFooterParse` (`0x1424d9af0`) writes. It returns the client's `AL`:
0 every page matched, 3 `len` shorter than the page bodies, 8 a stored
per-page checksum disagreeing with its page.

The file is built the way the client's own writer lays it out (the order
`CdnIndexFileParse` validates): page bodies, the per-page last keys, the
per-page checksums, the terminal checksum, the 20-byte footer. `len` is the
whole-file length in a valid case, so the cases that vary it sit exactly on the
client's single gate (`0x1424d9f66 jae`: `len >= pageCount * pageSize`).

Cases are front-loaded because the mutation stage scores the first 48: the
determinism case, the loop shapes (one page, many), the gate's two edges, the
match and mismatch edges, and the checksum-size boundaries 0/8/16.
"""

from __future__ import annotations

import hashlib
from typing import List, Tuple

from emu_oracle import ArgSpec

FOOTER_SIZE = 20
# The descriptor's arena offset. The largest file here is under 8 KiB, so the
# two pointees never overlap.
DESC_OFFSET = 0x8000


def _lcg_bytes(seed: int, n: int) -> bytes:
    """Version-independent deterministic bytes (a plain 32-bit LCG)."""
    x = seed & 0xFFFFFFFF
    out = bytearray(n)
    for i in range(n):
        x = (x * 1664525 + 1013904223) & 0xFFFFFFFF
        out[i] = x >> 24
    return bytes(out)


def descriptor(
    page_size: int,
    page_count: int,
    key_bytes: int,
    checksum_size: int,
    last_key_bytes: int | None = None,
) -> bytes:
    """The 0x30-byte block at the client's offsets.

    `CdnIndexPageChecksums` reads `+0x07` (checksum size), `+0x14` (page size),
    `+0x20` (page count) and `+0x24` (last-key bytes). The other fields are
    filled the way `CdnIndexFooterParse` would, because a real descriptor has
    them - and because a candidate that read the wrong offset must diverge.
    """
    if last_key_bytes is None:
        last_key_bytes = page_count * key_bytes
    d = bytearray(0x30)
    d[0] = 1  # version
    d[4] = 6  # offset bytes
    d[5] = 4  # size bytes
    d[6] = key_bytes
    d[7] = checksum_size
    d[8:12] = (page_count * 7).to_bytes(4, "little")  # element count, plausible
    d[0x14:0x18] = page_size.to_bytes(4, "little")
    d[0x18:0x1c] = (key_bytes + 10).to_bytes(4, "little")  # entry size
    d[0x1c:0x20] = (page_size // (key_bytes + 10)).to_bytes(4, "little")
    d[0x20:0x24] = page_count.to_bytes(4, "little")
    d[0x24:0x28] = last_key_bytes.to_bytes(4, "little")
    d[0x28:0x2c] = (page_count * checksum_size).to_bytes(4, "little")
    return bytes(d)


def index_file(
    page_size: int,
    page_count: int,
    key_bytes: int,
    checksum_size: int,
    seed: int,
    last_key_bytes: int | None = None,
    bad_sum: int | None = None,
    bad_body: int | None = None,
) -> bytes:
    """A complete index file: bodies, last keys, checksums, terminal, footer."""
    if last_key_bytes is None:
        last_key_bytes = page_count * key_bytes
    bodies = _lcg_bytes(seed, page_size * page_count)
    keys = _lcg_bytes(seed ^ 0x5A5A, last_key_bytes)
    sums = b"".join(
        hashlib.md5(bodies[p * page_size : (p + 1) * page_size]).digest()[:checksum_size]
        for p in range(page_count)
    )
    if bad_sum is not None:
        corrupted = bytearray(sums)
        corrupted[bad_sum] ^= 0x01
        sums = bytes(corrupted)
    if bad_body is not None:
        corrupted = bytearray(bodies)
        corrupted[bad_body] ^= 0x01
        bodies = bytes(corrupted)
    terminal = hashlib.md5(keys + sums).digest()[:checksum_size]
    footer = bytes([0x11]) * FOOTER_SIZE
    return bodies + keys + sums + terminal + footer


# (file, len, descriptor). `len` is always <= len(file).
Case = Tuple[bytes, int, bytes]

_DET = index_file(64, 2, 16, 8, 0x10)
_DET_DESC = descriptor(64, 2, 16, 8)
_TWO_PAGE_BODIES = 64 * 2

_ONE = index_file(64, 1, 16, 8, 0x11)
_FOUR = index_file(96, 4, 16, 8, 0x12)
_FOUR_DESC = descriptor(96, 4, 16, 8)
_BAD_FIRST = index_file(64, 2, 16, 8, 0x10, bad_sum=0)
_BAD_LAST = index_file(96, 4, 16, 8, 0x12, bad_sum=4 * 8 - 1)
_BAD_BODY = index_file(64, 2, 16, 8, 0x10, bad_body=64 + 7)
_BAD_TAIL = index_file(64, 2, 16, 8, 0x10, bad_sum=8)
_ZERO_CK = index_file(64, 3, 16, 0, 0x13)
_CK16 = index_file(64, 2, 16, 16, 0x14)
_CK16_BAD_LAST_BYTE = index_file(64, 2, 16, 16, 0x14, bad_sum=15)
_KEYS9 = index_file(64, 3, 9, 8, 0x15)
_PAD_KEYS = index_file(64, 2, 9, 8, 0x16, last_key_bytes=32)
_BIG_PAGE = index_file(1024, 1, 16, 8, 0x17)

SPECIALS: List[Case] = [
    # Determinism case: two pages, everything matching.
    (_DET, len(_DET), _DET_DESC),
    # One page: the loop's back edge is not taken.
    (_ONE, len(_ONE), descriptor(64, 1, 16, 8)),
    # Four pages: the back edge is taken.
    (_FOUR, len(_FOUR), _FOUR_DESC),
    # The length gate: one byte short of the bodies is status 3 ...
    (_DET, _TWO_PAGE_BODIES - 1, _DET_DESC),
    (_DET, 0, _DET_DESC),
    # ... exactly the bodies passes, and the tail is still read.
    (_DET, _TWO_PAGE_BODIES, _DET_DESC),
    # A corrupt checksum on the first page: status 8, and the loop stops.
    (_BAD_FIRST, len(_BAD_FIRST), _DET_DESC),
    # A corrupt checksum on the *last* page: the loop reaches it.
    (_BAD_LAST, len(_BAD_LAST), _FOUR_DESC),
    # A corrupt page body breaks its own checksum.
    (_BAD_BODY, len(_BAD_BODY), _DET_DESC),
    # `len` stops at the bodies, but a mismatch past it is still reported: the
    # length gate covers the page bodies only.
    (_BAD_TAIL, _TWO_PAGE_BODIES, _DET_DESC),
    # checksumSize 0 compares nothing, so every page "matches".
    (_ZERO_CK, len(_ZERO_CK), descriptor(64, 3, 16, 0)),
    # checksumSize 16 compares the whole MD5, including its last byte.
    (_CK16, len(_CK16), descriptor(64, 2, 16, 16)),
    (_CK16_BAD_LAST_BYTE, len(_CK16_BAD_LAST_BYTE), descriptor(64, 2, 16, 16)),
    # keyBytes 9: the last-key array is 27 bytes for 3 pages.
    (_KEYS9, len(_KEYS9), descriptor(64, 3, 9, 8)),
    # lastKeyBytes is read from the descriptor, not recomputed from keyBytes:
    # 32 bytes of key array for 2 pages of 9-byte keys.
    (_PAD_KEYS, len(_PAD_KEYS), descriptor(64, 2, 9, 8, last_key_bytes=32)),
    # One page of 1 KiB bodies.
    (_BIG_PAGE, len(_BIG_PAGE), descriptor(1024, 1, 16, 8)),
    # Zero pages: status 0 before the loop (the client's `test r9d, r9d`).
    (b"", 0, descriptor(64, 0, 16, 8)),
]


def make_case(i: int) -> Case:
    """The i-th (file, len, descriptor) case. Pure function of i."""
    if i < len(SPECIALS):
        return SPECIALS[i]
    r = i - len(SPECIALS)
    page_size = [32, 64, 96, 128, 1024][r % 5]
    page_count = [1, 2, 3, 5][(r // 5) % 4]
    checksum_size = [0, 4, 8, 16][(r // 20) % 4]
    key_bytes = [9, 16][(r // 7) % 2]
    last_key_bytes = page_count * key_bytes + 4 * (r // 3 % 2)
    bad_sum = None
    if checksum_size > 0 and r % 6 == 0:
        bad_sum = (r * 37) % (page_count * checksum_size)
    body = index_file(
        page_size,
        page_count,
        key_bytes,
        checksum_size,
        0x30000 + r,
        last_key_bytes=last_key_bytes,
        bad_sum=bad_sum,
    )
    length = len(body) if r % 4 else page_size * page_count
    if r % 11 == 3:
        length = max(0, page_size * page_count - 1)
    return (
        body,
        length,
        descriptor(page_size, page_count, key_bytes, checksum_size, last_key_bytes),
    )


def make_args(i: int) -> List[ArgSpec]:
    body, length, desc = make_case(i)
    return [
        ArgSpec(kind="ptr", mem=body, arena_offset=0),
        ArgSpec(kind="u64", bits=length),
        ArgSpec(kind="ptr", mem=desc, arena_offset=DESC_OFFSET),
    ]

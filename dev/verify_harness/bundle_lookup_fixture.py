"""Deterministic synthetic bundle payloads for the `fn/BundleLookup` unit.

The client`s `BundleEntryLookup` (`0x141fd5820`) takes a tagged bundle request
handle (RDX) and a target GUID (R8). The handle`s low six bits are a tag; the
rest masks to a 64-byte-aligned object whose `+0` qword is the bundle payload
and whose `+8` dword carries the tag. The search reads `i32 count` at `+0`,
`u8 width` at `+4`, then `count` rows of `u64 GUID` plus a `width`-byte offset,
and returns `payload + offset`; offset `0`, an absent GUID, a width other than
1/2/4 and any failed handle check return `0`.

The fixture builds the one arena region the client`s first dereference expects:
64 bytes of handle object followed by a synthetic payload. Every byte is
synthetic - no bundle from the install is embedded, and no offset points at
real game data. The probe gets the same region and the payload`s guest base in
the client`s dead first argument slot (RCX), so it can return the client`s
`payload + offset` value even though its own payload lives at a host address.

Case order matters: the first 48 are the mutation corpus, so every branch the
search can take is pinned in that window.
"""

from __future__ import annotations

import random
import struct
from typing import List, Sequence, Tuple

from emu_oracle import ARENA_BASE, ArgSpec

OBJECT_LEN = 0x40
PAYLOAD_OFF = OBJECT_LEN
PAYLOAD_GUEST = ARENA_BASE + PAYLOAD_OFF

# --- handle modes ---------------------------------------------------------

_VALID = "valid"
_MINUS_ONE = "minus_one"
_ZERO_BASE = "zero_base"
_TAG_MISMATCH = "tag_mismatch"
_NULL_PAYLOAD = "null_payload"


def _payload(
    width: int,
    entries: Sequence[Tuple[int, int]],
    extra: bytes = b"",
) -> bytes:
    """`i32 count`, `u8 width`, then `count` GUID + `width`-byte offset rows."""
    out = bytearray(struct.pack("<iB", len(entries), width))
    for guid, offset in entries:
        out += struct.pack("<Q", guid)
        if width == 1:
            out += struct.pack("<B", offset)
        elif width == 2:
            out += struct.pack("<H", offset)
        elif width == 4:
            out += struct.pack("<I", offset)
        else:
            # No stride exists for this width; the client never reads the
            # rows. Pack `8 + width` bytes anyway so the table is well formed.
            out += struct.pack("<B", offset & 0xFF)
            out += b"\x00" * (width - 1)
    out += extra
    return bytes(out)


Case = Tuple[int, Sequence[Tuple[int, int]], int, str, int, bytes]


def _w4(count: int, start: int, step: int) -> List[Tuple[int, int]]:
    return [(start + i * step, 0x100 + i * 4) for i in range(count)]


_W4_8 = _w4(8, 0x10, 0x10)
_W2_8 = [(0x100 + i * 0x10, 0x200 + i * 4) for i in range(8)]
_W1_8 = [(0x10 + i * 0x10, 0x20 + i * 2) for i in range(8)]

_W4_3 = _w4(3, 0x10, 0x10)
_W2_3 = [(0x100, 0x200), (0x200, 0x204), (0x300, 0x208)]
_W1_2 = [(0x10, 0x20), (0x20, 0x22)]

_PINNED: List[Case] = [
    # Width 2, one row: found, absent before, absent after, offset 0. These
    # pin the loop-skipped path and both post-loop equality shapes.
    (2, [(0x100, 0x20)], 0x100, _VALID, 0x05, b""),
    (2, [(0x100, 0x20)], 0x0FF, _VALID, 0x05, b""),
    (2, [(0x100, 0x20)], 0x101, _VALID, 0x05, b""),
    (2, [(0x100, 0x00)], 0x100, _VALID, 0x05, b""),
    # The four handle failures return 0 before the search.
    (2, [(0x100, 0x20)], 0x100, _MINUS_ONE, 0x05, b""),
    (2, [(0x100, 0x20)], 0x100, _ZERO_BASE, 0x05, b""),
    (2, [(0x100, 0x20)], 0x100, _TAG_MISMATCH, 0x05, b""),
    (2, [(0x100, 0x20)], 0x100, _NULL_PAYLOAD, 0x05, b""),
    # Width 4: count 0, count 1, count 2, count 3.
    (4, [], 0x10, _VALID, 0x3F, b""),
    (4, _w4(1, 0x10, 0x10), 0x10, _VALID, 0x3F, b""),
    (4, _w4(2, 0x10, 0x10), 0x10, _VALID, 0x3F, b""),
    (4, _W4_3, 0x10, _VALID, 0x3F, b""),
    # Width 4, count 3: first, middle, last, before, between, between the
    # last two, after.
    (4, _W4_3, 0x20, _VALID, 0x3F, b""),
    (4, _W4_3, 0x30, _VALID, 0x3F, b""),
    (4, _W4_3, 0x08, _VALID, 0x3F, b""),
    (4, _W4_3, 0x18, _VALID, 0x3F, b""),
    (4, _W4_3, 0x28, _VALID, 0x3F, b""),
    (4, _W4_3, 0x38, _VALID, 0x3F, b""),
    # Width 4, count 8: first, middle, last, between, after.
    (4, _W4_8, 0x10, _VALID, 0x00, b""),
    (4, _W4_8, 0x50, _VALID, 0x00, b""),
    (4, _W4_8, 0x80, _VALID, 0x00, b""),
    (4, _W4_8, 0x78, _VALID, 0x00, b""),
    (4, _W4_8, 0x90, _VALID, 0x00, b""),
    # Unsorted tables follow the client's own walk, not a linear scan.
    (4, [(2, 0x11), (1, 0x22)], 2, _VALID, 0x11, b""),
    (4, [(1, 0x11), (2, 0x22), (1, 0x33), (2, 0x44)], 2, _VALID, 0x11, b""),
    (4, [(3, 0x11), (1, 0x22), (2, 0x33)], 2, _VALID, 0x11, b""),
    (4, [(5, 0x11), (5, 0x22), (1, 0x33), (9, 0x44), (2, 0x55)], 5, _VALID, 0x11, b""),
    # Duplicates: the lower bound is the first equal row.
    (4, [(0x10, 0x100), (0x20, 0x104), (0x20, 0x108), (0x30, 0x10C)], 0x20, _VALID, 0x2A, b""),
    (4, [(0x10, 0x100), (0x20, 0x00), (0x20, 0x108), (0x30, 0x10C)], 0x20, _VALID, 0x2A, b""),
    # GUID 0 and u64::MAX are ordinary table values.
    (4, [(0, 0x100), (0x10, 0x104)], 0, _VALID, 0x3F, b""),
    (4, [(0x10, 0x100), (0xFFFF_FFFF_FFFF_FFFF, 0x104)], 0xFFFF_FFFF_FFFF_FFFF, _VALID, 0x3F, b""),
    (4, [(0x10, 0x100), (0xFFFF_FFFF_FFFF_FFFE, 0x104)], 0xFFFF_FFFF_FFFF_FFFF, _VALID, 0x3F, b""),
    # Width 1: count 0, 1, 2, 8.
    (1, [], 0x10, _VALID, 0x07, b""),
    (1, [(0x10, 0x20)], 0x10, _VALID, 0x07, b""),
    (1, _W1_2, 0x10, _VALID, 0x07, b""),
    (1, _W1_2, 0x20, _VALID, 0x07, b""),
    (1, _W1_8, 0x50, _VALID, 0x07, b""),
    (1, _W1_8, 0x90, _VALID, 0x07, b""),
    # Width 2: count 0, count 3, count 8.
    (2, [], 0x100, _VALID, 0x05, b""),
    (2, _W2_3, 0x200, _VALID, 0x05, b""),
    (2, _W2_3, 0x300, _VALID, 0x05, b""),
    (2, _W2_3, 0x280, _VALID, 0x05, b""),
    (2, _W2_3, 0x380, _VALID, 0x05, b""),
    (2, _W2_8, 0x100, _VALID, 0x05, b""),
    (2, _W2_8, 0x140, _VALID, 0x05, b""),
    (2, _W2_8, 0x170, _VALID, 0x05, b""),
    (2, _W2_8, 0x138, _VALID, 0x05, b""),
    (2, _W2_8, 0x180, _VALID, 0x05, b""),
    # A width with no stride: the client's width gate returns 0.
    (3, [(0x10, 0x11), (0x20, 0x22), (0x30, 0x33)], 0x20, _VALID, 0x3F, b""),
    (0, [], 0x10, _VALID, 0x3F, b""),
    # Table at the very end of the payload: the last found row's offset byte
    # is the payload's final byte.
    (1, _W1_8, 0x80, _VALID, 0x07, b""),
]


def _generated(i: int) -> Case:
    """A deterministic case for index `i` beyond the pinned list."""
    rnd = random.Random(0x4255_4E44 ^ i)  # "BUND"
    width = (1, 2, 4)[i % 3]
    count = i % 9
    mode = _VALID
    if i % 29 == 0:
        mode = _TAG_MISMATCH
    elif i % 31 == 0:
        mode = _NULL_PAYLOAD

    if count == 0:
        return (width, [], rnd.getrandbits(64), mode, rnd.randrange(0x40), b"")

    guid = rnd.getrandbits(64)
    entries: List[Tuple[int, int]] = []
    for _ in range(count):
        guid = (guid + rnd.randrange(1, 0x1_0000)) & 0xFFFF_FFFF_FFFF_FFFF
        if width == 1:
            offset = rnd.randrange(1, 0x100)
        elif width == 2:
            offset = rnd.randrange(1, 0x1_0000)
        else:
            offset = rnd.randrange(1, 0x1_0000_0000)
        if i % 11 == 0:
            offset = 0
        entries.append((guid, offset))

    if i % 4 == 0:
        rnd.shuffle(entries)

    pick = i % 6
    if pick == 0:
        target = entries[0][0]
    elif pick == 1:
        target = entries[-1][0]
    elif pick == 2:
        target = entries[rnd.randrange(len(entries))][0]
    elif pick == 3:
        target = (entries[0][0] - rnd.randrange(1, 0x1000)) & 0xFFFF_FFFF_FFFF_FFFF
    elif pick == 4:
        target = (entries[-1][0] + rnd.randrange(1, 0x1000)) & 0xFFFF_FFFF_FFFF_FFFF
    else:
        first = min(guid for guid, _ in entries)
        target = (first + rnd.randrange(1, 0x1000)) & 0xFFFF_FFFF_FFFF_FFFF
    extra = b"" if i % 7 else bytes(rnd.randrange(0x100) for _ in range(16))
    return (width, entries, target, mode, rnd.randrange(0x40), extra)


def _handle(tag: int, mode: str) -> int:
    if mode == _MINUS_ONE:
        return 0xFFFF_FFFF_FFFF_FFFF
    if mode == _ZERO_BASE:
        return tag & 0x3F
    return ARENA_BASE | (tag & 0x3F)


def _region(payload: bytes, tag: int, mode: str) -> Tuple[bytes, Tuple[int, ...]]:
    obj = bytearray(OBJECT_LEN)
    if mode != _NULL_PAYLOAD:
        struct.pack_into("<Q", obj, 0, PAYLOAD_OFF)
    own_tag = (tag ^ 1) if mode == _TAG_MISMATCH else tag
    # The high 26 bits are set so the client's `& 0x3f` mask is exercised.
    struct.pack_into("<I", obj, 8, (own_tag & 0x3F) | 0xFFFF_FFC0)
    region = bytes(obj) + payload
    relocs: Tuple[int, ...] = () if mode == _NULL_PAYLOAD else (0,)
    return region, relocs


def make_args(i: int) -> List[ArgSpec]:
    """The i-th input: guest payload base, handle, target GUID, region."""
    if i < len(_PINNED):
        width, entries, target, mode, tag, extra = _PINNED[i]
    else:
        width, entries, target, mode, tag, extra = _generated(i)
    if mode == _NULL_PAYLOAD:
        payload = b""
    else:
        payload = _payload(width, entries, extra)
    region, relocs = _region(payload, tag, mode)
    return [
        ArgSpec(kind="u64", bits=PAYLOAD_GUEST),
        ArgSpec(kind="u64", bits=_handle(tag, mode)),
        ArgSpec(kind="u64", bits=target),
        ArgSpec(kind="ptr", mem=region, arena_offset=0, relocs=relocs),
    ]

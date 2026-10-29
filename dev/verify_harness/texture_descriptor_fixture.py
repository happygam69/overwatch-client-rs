"""Structured inputs for fn/TextureDescriptor.

The builder `0x142e2d900` reads a 0x20-byte texture header and writes the
descriptor qword and the object's flags dword. The fixture keeps every header
inside the loader's accepted set (`0x141dacf00`: format <= 0x71, mip <= 0x10,
width/height nonzero, payload_count < 5, base_mips <= mip_count) because the
probe calls the shipped `TextureHeader::parse`, which rejects the same set.

Two fields are pinned:

* `total_size` is 0. The builder adds it to a process-wide counter at
  `0x1442a36d8`; the unit declares that counter as state, and a zero add keeps
  the state channel identical without a probe-side emulation of a global.
* byte `+0x07` is 0. The builder's flags are only `flags | 0x100000`; the
  loader adds `0x4000` when this byte is 1, so the builder unit must not claim
  that later bit.

The out object is only 16 bytes: the descriptor at +0 and the flags at +8.
The builder writes more fields past +0x10 (pointers, -1 sentinels, total_size),
but those are bookkeeping, not descriptor parsing, and the header pointer is
placed away from the out object so the builder's sentinel writes cannot touch
the header between the reads and the comparison.
"""

from __future__ import annotations

import random
import struct
from typing import List

from emu_oracle import ArgSpec

HEADER = 0x20
OUT = 16
HEADER_ARENA_OFFSET = 0x100


def _header(
    flags: int,
    mip: int,
    fmt: int,
    surfaces: int,
    usage: int,
    payload: int,
    width: int,
    height: int,
    total: int,
    base: int,
    chunk: int,
    reserved: int,
) -> bytes:
    raw = struct.pack(
        "<HBBBBBBHHIBBB",
        flags & 0xFFFF,
        mip & 0xFF,
        fmt & 0xFF,
        surfaces & 0xFF,
        usage & 0xFF,
        payload & 0xFF,
        0,  # byte +0x07: the loader's 0x4000 bit, not the builder's
        width & 0xFFFF,
        height & 0xFFFF,
        total & 0xFFFF_FFFF,
        base & 0xFF,
        chunk & 0xFF,
        reserved & 0xFF,
    )
    return raw + b"\x00" * (HEADER - len(raw))


_SPECIALS = [
    # A typical BC1 asset: 64x32, three surfaces, two mips.
    _header(0x0001, 4, 0x47, 3, 2, 0, 64, 32, 0, 2, 0x08, 0),
    # Volume (flags bit 2), six depth slices, one mip.
    _header(0x0004, 1, 0x1E, 6, 31, 0, 1, 1, 0, 0, 0, 0),
    # Every field at its byte/word maximum that the loader accepts.
    _header(0xFFFF, 16, 0x71, 0, 0xFF, 4, 0xFFFF, 0xFFFF, 0, 16, 0xFF, 0xFF),
    # Exactly 16384 wide: the descriptor field is width-1 = 0x3fff.
    _header(0x0000, 0, 0x00, 1, 0, 0, 0x4000, 0x4000, 0, 0, 0, 0),
    # Payload chain (>1) takes the builder's second counter branch.
    _header(0x0001, 4, 0x47, 0, 0, 2, 3, 5, 0, 4, 0x0F, 0),
    # Volume plus a payload chain, surfaces wrap (0 -> 2048 in the consumer).
    _header(0x0004, 15, 0x43, 0xFF, 0x1F, 4, 2, 2, 0, 15, 0x08, 0x0F),
    # mip_count 16 stores 0 in the 4-bit descriptor field; base 0.
    _header(0x0000, 16, 0x00, 1, 0, 4, 0xFFFF, 1, 0, 0, 0xFF, 0xFF),
    # One axis one texel, the other 65535: both masks are exercised.
    _header(0x0004, 8, 0x71, 0, 0, 0, 1, 0xFFFF, 0, 8, 0, 0),
]


def _generate(i: int) -> bytes:
    rng = random.Random(7127 + i)
    mip = rng.randrange(0, 17)
    base = rng.randrange(0, mip + 1)
    return _header(
        rng.randrange(0, 0x1_0000),
        mip,
        rng.randrange(0, 0x72),
        rng.randrange(0, 0x100),
        rng.randrange(0, 0x100),
        rng.randrange(0, 5),
        rng.randrange(1, 0x1_0000),
        rng.randrange(1, 0x1_0000),
        0,
        base,
        rng.randrange(0, 0x100),
        rng.randrange(0, 0x100),
    )


def make_args(i: int) -> List[ArgSpec]:
    header = _SPECIALS[i] if i < len(_SPECIALS) else _generate(i)
    return [
        ArgSpec(kind="ptr", mem=b"\x00" * OUT, arena_offset=0),
        ArgSpec(kind="ptr", mem=header, arena_offset=HEADER_ARENA_OFFSET),
    ]

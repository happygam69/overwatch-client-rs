"""Structured inputs for fn/TextureDescriptorSizeWalk.

The client's wrapper `0x142e2dbb0` reads a 0x68-byte descriptor object: the
descriptor u64 at +0, the flags dword at +8 and the stored format byte at
+0x60. It shifts width/height/depth by `bias & 31`, clamps each at 1, passes
`mip - bias` (32-bit wrap) to `0x142e37070`, and returns the low 32 bits.

The fixture keeps `bias <= descriptor mip` because the wrapper's mip count is a
32-bit subtraction: a bias one above the mip makes the client's size loop run
almost 2^32 times, which is not a call the loader makes and cannot be emulated
inside the step budget. The bias domain 0..=mip still covers every shift the
wrapper applies, including 15.

Formats 0x72..=0xff are included even though the loader rejects them: the
wrapper itself does not validate, and both sides see the format switch's
default (0 bpp). The valid 0..=0x71 range is covered by every family boundary.
"""

from __future__ import annotations

import random
import struct
from typing import List

from emu_oracle import ArgSpec

OBJECT = 0x68


def _object(descriptor: int, flags: int, fmt: int) -> bytes:
    raw = bytearray(OBJECT)
    struct.pack_into("<Q", raw, 0, descriptor & 0xFFFF_FFFF_FFFF_FFFF)
    struct.pack_into("<I", raw, 8, flags & 0xFFFF_FFFF)
    raw[0x60] = fmt & 0xFF
    return bytes(raw)


def _descriptor(width: int, height: int, surfaces: int, mip: int) -> int:
    # width/height/surfaces are the stored field values (resource value - 1).
    return (
        (width & 0x3FFF)
        | ((height & 0x3FFF) << 14)
        | ((surfaces & 0x7FF) << 28)
        | ((mip & 0xF) << 39)
    )


def _case(width: int, height: int, surfaces: int, mip: int, flags: int, fmt: int, bias: int):
    return (
        _object(_descriptor(width, height, surfaces, mip), flags, fmt),
        bias,
    )


_VOLUME = 0x0010_0004
_PLAIN = 0x0010_0000
# Bit 3 is not the wrapper's volume flag. A case with it set and bit 2 clear
# must stay non-volume: a mutant that widens the mask to 0xc would swap the
# surface and depth fields here.
_PLAIN_BIT3 = 0x0010_0008
_VOLUME_BIT3 = 0x0010_000c

_SPECIALS = [
    # 1x1, one surface, one mip: the smallest non-zero walk.
    _case(0, 0, 0, 0, _PLAIN, 0x47, 0),
    # mip 15 at bias 15: the walk sees zero mips and returns zero.
    _case(0, 0, 0, 15, _PLAIN, 0x47, 15),
    # 16384x16384, 2048 surfaces, 16 mips: the largest descriptor fields.
    _case(0x3FFF, 0x3FFF, 0x7FF, 15, _PLAIN, 0x47, 0),
    _case(0x3FFF, 0x3FFF, 0x7FF, 15, _PLAIN, 0x47, 7),
    _case(0x3FFF, 0x3FFF, 0x7FF, 15, _PLAIN, 0x47, 15),
    # Volume: surfaces field is the depth, surface count is one.
    _case(0x3FFF, 0x3FFF, 0x7FF, 15, _VOLUME, 0x47, 7),
    _case(0, 0, 0x7FF, 15, _VOLUME, 0x43, 0),
    # Flags bit 3 alone is not volume; only bit 2 selects the volume fields.
    _case(0x3FFF, 0x3FFF, 0x3FF, 15, _PLAIN_BIT3, 0x47, 0),
    _case(0x3FFF, 0x3FFF, 0x7FF, 15, _VOLUME_BIT3, 0x47, 7),
    # The 1-bpp linear family.
    _case(0x3FFF, 0x3FFF, 0, 15, _PLAIN, 0x43, 0),
    # 12-bpp linear, and the mobile EAC ids the walk clamps at 1.
    _case(0x3FFF, 0x3FFF, 0, 15, _PLAIN, 0x71, 15),
    _case(0x3FFF, 0x3FFF, 0, 15, _PLAIN, 0x65, 0),
    _case(0x3FFF, 0x3FFF, 0, 15, _PLAIN, 0x68, 0),
    # Zero bits per pixel: the walk's total is scaled away.
    _case(0x3FFF, 0x3FFF, 0x7FF, 15, _PLAIN, 0x00, 0),
    # Outside the loader's accepted set: default bpp 0.
    _case(0x3FFF, 0x3FFF, 0x7FF, 15, _PLAIN, 0x72, 0),
    _case(0x3FFF, 0x3FFF, 0x7FF, 15, _PLAIN, 0xFF, 0),
]


def _generate(i: int):
    rng = random.Random(9137 + i)
    mip = rng.randrange(0, 16)
    bias = rng.randrange(0, mip + 1)
    return _case(
        rng.randrange(0, 0x4000),
        rng.randrange(0, 0x4000),
        rng.randrange(0, 0x800),
        mip,
        _VOLUME if rng.randrange(2) else _PLAIN,
        rng.randrange(0, 0x100),
        bias,
    )


def make_args(i: int) -> List[ArgSpec]:
    obj, bias = _SPECIALS[i] if i < len(_SPECIALS) else _generate(i)
    return [
        ArgSpec(kind="ptr", mem=obj),
        ArgSpec(kind="u64", bits=bias),
    ]

"""`MeshShapeFill` (`0x142cae540`) inputs: the 0x30-byte source buffer the
block unpacker writes, and the scale.

The client fills a 0x4c-byte shape record from the source: four lanes of the
scale, four lanes of its reciprocal (a `divss` by the constant 1.0 at
`0x142f7aae4`), the three data pointers, the four copied words and the one
float multiplied by the scale.
"""

from __future__ import annotations

import random
import struct

from emu_oracle import ARENA_BASE, ArgSpec

REC_OFF = 0x1000
SRC_OFF = 0x2000
REC_LEN = 0x4C
SRC_LEN = 0x30

# The scale cases: ordinary, the reciprocal's edge cases (zero and a
# denormal make the division inf/NaN), and a negative.
SCALES = (1.0, 2.0, 0.5, 0.25, -1.5, 3.0, 0.0, 1e-30, 1e30)


def _special(i: int):
    """(nodes, points, triangles, counts, words, the scaled float)."""
    table = [
        # the ordinary case: three pointers, four counts, one float
        (0x1111_2222_3333_4444, 0x5555_6666_7777_8888, 0x9999_AAAA_BBBB_CCCC,
         7, 8, 12, 4, 0.5),
        # all zeros: the empty mesh
        (0, 0, 0, 0, 0, 0, 0, 0.0),
        # one pointer at the maximum
        (0xFFFF_FFFF_FFFF_FFFF, 0xFFFF_FFFF_FFFF_FFFF, 0xFFFF_FFFF_FFFF_FFFF,
         0xFFFF_FFFF, 0xFFFF_FFFF, 0xFFFF_FFFF, 0xFFFF_FFFF, 3.4028235e38),
    ]
    return table[i % len(table)]


def _case(i: int):
    if i < 3:
        return _special(i)
    rng = random.Random(0x4D534846 ^ i)
    nodes = rng.getrandbits(64)
    points = rng.getrandbits(64)
    triangles = rng.getrandbits(64)
    counts = (rng.getrandbits(32), rng.getrandbits(32),
              rng.getrandbits(32), rng.getrandbits(32))
    value = struct.unpack("<f", struct.pack("<I", rng.getrandbits(32)))[0]
    return nodes, points, triangles, *counts, value


def build(i: int) -> bytes:
    """The 0x30-byte source buffer the client's unpacker would write."""
    nodes, points, triangles, c0, c1, c2, c3, value = _case(i)
    src = bytearray(SRC_LEN)
    struct.pack_into("<Q", src, 0x00, nodes)
    struct.pack_into("<Q", src, 0x08, points)
    struct.pack_into("<Q", src, 0x10, triangles)
    struct.pack_into("<I", src, 0x18, c0)
    struct.pack_into("<I", src, 0x1C, c1)
    struct.pack_into("<I", src, 0x20, c2)
    struct.pack_into("<I", src, 0x24, c3)
    struct.pack_into("<f", src, 0x28, value)
    return bytes(src)


def make_args(i: int):
    src = build(i)
    scale = SCALES[i % len(SCALES)]
    return [
        ArgSpec(kind="ptr", mem=bytes(REC_LEN), arena_offset=REC_OFF),
        ArgSpec(kind="ptr", mem=src, arena_offset=SRC_OFF),
        ArgSpec(kind="f32", value=scale),
    ]

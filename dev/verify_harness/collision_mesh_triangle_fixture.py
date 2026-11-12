"""Inputs for the `collision_mesh_triangle` unit (`0x142cae760`).

One case is one blob: the mesh record at its front, then the vertex array and
the triangle array behind it. The record's two pointers are absolute arena
addresses, so the oracle reads the vertices and the triangles out of the arena
and the probe reads the same bytes out of the blob.

The mesh record carries the scale quad at `+0x00`; a triangle record is
`0x20` bytes with its three vertex indices at `+0x00`, `+0x04` and `+0x08`
and its three layer words at `+0x18`, `+0x1a` and `+0x1c`. Every triangle in
the fixture points at a different vertex, so a wrong stride or a wrong index
shows up in the output.
"""

from __future__ import annotations

import random
import struct
from typing import List

from emu_oracle import ARENA_BASE, ArgSpec

BLOB_OFF = 0
OUT_OFF = 0x4000

RECORD_LEN = 0x50
VERTICES_OFF = 0x100
VERTEX_LEN = 0x10
TRIANGLES_OFF = 0x400
TRIANGLE_LEN = 0x20
BLOB_LEN = 0xA00

OUT_LEN = 0x86

VERTEX_COUNT = 8
TRIANGLE_COUNT = 6

# The scale the record carries: the vertices are multiplied by it.
SCALES = [
    (1.0, 1.0, 1.0, 1.0),
    (0.5, 0.5, 0.5, 0.5),
    (2.0, 1.5, -0.25, 1.0),
    (0.0, 0.0, 0.0, 0.0),
]


def _fbits(value: float) -> int:
    return struct.unpack("<I", struct.pack("<f", value))[0]


def _quad_bytes(values) -> bytes:
    return b"".join(struct.pack("<I", _fbits(v)) for v in values)


def _cases():
    rng = random.Random(23680)
    out = []
    for index in range(TRIANGLE_COUNT):
        out.append((index, index % len(SCALES)))
    # The same triangle through every scale, and a negative index.
    for scale in range(len(SCALES)):
        out.append((1, scale))
    out.append((TRIANGLE_COUNT - 1, 1))
    for _ in range(24):
        out.append((rng.randrange(0, TRIANGLE_COUNT), rng.randrange(0, len(SCALES))))
    return out


CASES = _cases()


def _blob_bytes(triangle: int, scale: int, rng: random.Random) -> bytes:
    blob = bytearray(BLOB_LEN)
    blob[0x00:0x10] = _quad_bytes(SCALES[scale])
    struct.pack_into("<Q", blob, 0x28, ARENA_BASE + BLOB_OFF + VERTICES_OFF)
    struct.pack_into("<Q", blob, 0x30, ARENA_BASE + BLOB_OFF + TRIANGLES_OFF)
    for index in range(VERTEX_COUNT):
        at = VERTICES_OFF + index * VERTEX_LEN
        blob[at : at + 0x10] = _quad_bytes(
            [
                rng.uniform(-4.0, 4.0),
                rng.uniform(-4.0, 4.0),
                rng.uniform(-4.0, 4.0),
                rng.uniform(-1.0, 1.0),
            ]
        )
    for index in range(TRIANGLE_COUNT):
        at = TRIANGLES_OFF + index * TRIANGLE_LEN
        # The three vertex indices: each triangle points at its own vertices.
        struct.pack_into("<i", blob, at + 0x00, index % VERTEX_COUNT)
        struct.pack_into("<i", blob, at + 0x04, (index + 1) % VERTEX_COUNT)
        struct.pack_into("<i", blob, at + 0x08, (index + 2) % VERTEX_COUNT)
        struct.pack_into("<H", blob, at + 0x18, 0x1000 + index)
        struct.pack_into("<H", blob, at + 0x1A, 0x2000 + index)
        struct.pack_into("<H", blob, at + 0x1C, 0x3000 + index)
    return bytes(blob)


def make_args(i: int) -> List[ArgSpec]:
    triangle, scale = CASES[i % len(CASES)]
    rng = random.Random(1000 + i)
    return [
        ArgSpec(kind="ptr", mem=_blob_bytes(triangle, scale, rng), arena_offset=BLOB_OFF),
        ArgSpec(kind="ptr", mem=bytes(OUT_LEN), arena_offset=OUT_OFF),
        ArgSpec(kind="u64", bits=triangle & 0xFFFFFFFF),
    ]

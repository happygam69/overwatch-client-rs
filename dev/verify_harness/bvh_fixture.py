"""Inputs for the BVH builder unit (`0x142ccb1b0`).

The builder takes `(state, faces, count, bins, prefix)`:

- `state` is the build state: `+0x30` the node array pointer, `+0x38` the used
  node count, `+0x3c` the capacity;
- `faces` is the `0x60`-stride face array (`+0x00` min, `+0x10` max, `+0x20`
  centroid, `+0x30` the bin scratch the builder writes, `+0x54` the id word);
- `count` is the face count;
- `bins` and `prefix` are the caller's scratch buffers, passed as raw
  addresses so the comparison channel stays on the node array.

The comparison target is the node array: `(2*count - 1) * 0x50` bytes, which
the fixture fills with a sentinel so an unwritten tail cannot pass.

The declared cases are chosen so every branch of the split rule runs: a spread
on each axis, one axis flat, all centroids inside a single bin, two clusters
that compete for the cheapest split, and a count small enough (5) that the
first split leaves two leaves.
"""

from __future__ import annotations

import struct
from typing import List, Tuple

from emu_oracle import ARENA_BASE, ArgSpec

STATE_AT = 0x100
FACES_AT = 0x200
BINS_AT = 0x2000
PREFIX_AT = 0x3000
NODES_AT = 0x8000
STATE_LEN = 0x40
FACE_LEN = 0x60
NODE_LEN = 0x50

SENTINEL = 0xCD


def _face(centroid: Tuple[float, float, float], half: Tuple[float, float, float], ident: float) -> bytes:
    out = bytearray([SENTINEL]) * FACE_LEN
    low = [centroid[k] - half[k] for k in range(3)]
    high = [centroid[k] + half[k] for k in range(3)]
    for k in range(4):
        struct.pack_into("<f", out, k * 4, low[k] if k < 3 else 1.0)
        struct.pack_into("<f", out, 0x10 + k * 4, high[k] if k < 3 else 1.0)
        struct.pack_into("<f", out, 0x20 + k * 4, centroid[k] if k < 3 else 1.0)
    struct.pack_into("<i", out, 0x30, -1)
    struct.pack_into("<f", out, 0x34, 0.5)
    struct.pack_into("<H", out, 0x54, int(ident) & 0xFFFF)
    return bytes(out)


def _grid(count: int, step: Tuple[float, float, float]) -> List[bytes]:
    return [
        _face(
            (0.5 + step[0] * i, 0.5 + step[1] * i, 0.5 + step[2] * i),
            (0.25, 0.25, 0.25),
            i + 1,
        )
        for i in range(count)
    ]


def _box(low, high, ident: int) -> bytes:
    centroid = tuple((low[k] + high[k]) / 2.0 for k in range(3))
    half = tuple((high[k] - low[k]) / 2.0 for k in range(3))
    return _face(centroid, half, ident)


# f32(0.005), the exact threshold the client compares the centroid spread with.
SPREAD_AT_THRESHOLD = struct.unpack("<f", struct.pack("<f", 0.005))[0]

DECLARED: List[List[bytes]] = [
    # spread on x only
    _grid(6, (1.0, 0.0, 0.0)),
    # spread on y only
    _grid(7, (0.0, 1.0, 0.0)),
    # spread on z only
    _grid(9, (0.0, 0.0, 1.0)),
    # all three axes spread, uneven counts
    _grid(11, (1.0, 2.0, 0.5)),
    # five faces: the first split leaves two leaves
    _grid(5, (1.0, 0.0, 0.0)),
    # every centroid inside a single bin on x: the degenerate SAH input
    [_face((0.5 + 0.0002 * i, 0.5, 0.5), (0.01, 0.01, 0.01), i) for i in range(8)],
    # two dense clusters far apart, competing for the cheapest split
    [_face((0.0, 0.0, 0.0), (0.05, 0.05, 0.05), i) for i in range(6)]
    + [_face((10.0, 0.0, 0.0), (0.05, 0.05, 0.05), 100 + i) for i in range(6)],
    # one huge face among small ones: the box area dominates the cost
    [_face((float(i), 0.5, 0.5), (0.1, 0.1, 0.1), i) for i in range(7)]
    + [_face((3.0, 0.5, 0.5), (5.0, 5.0, 5.0), 99)],
    # clustered path, box extent tie on x == y: the axis rule must fall to z.
    [_box((0.0, 0.0, 0.0), (1.0, 1.0, 2.0), i) for i in range(5)],
    # clustered path, box extent tie on x == z with a smaller y.
    [_box((0.0, 0.0, 0.0), (2.0, 1.0, 2.0), i) for i in range(5)],
    # centroid spread exactly f32(0.005): the split rule's own boundary.
    [_face((0.0, 0.5, 0.5), (0.001, 0.001, 0.001), i) for i in range(5)]
    + [_face((SPREAD_AT_THRESHOLD, 0.5, 0.5), (0.001, 0.001, 0.001), 9)],
]


def _random_faces(index: int) -> List[bytes]:
    state = (index * 2_654_435_761 + 7) & 0xFFFF_FFFF

    def next_bits() -> int:
        nonlocal state
        state = (state * 1_103_515_245 + 12345) & 0xFFFF_FFFF
        return state

    count = 5 + next_bits() % 12
    spread = 10.0 ** (next_bits() % 3 - 1)
    faces = []
    for i in range(count):
        centroid = tuple(round(((next_bits() >> 8) % 1000) / 1000.0 * spread, 6) for _ in range(3))
        half = tuple(round(0.01 + ((next_bits() >> 8) % 100) / 1000.0, 6) for _ in range(3))
        faces.append(_face(centroid, half, i))
    return faces


def make_args(i: int) -> List[ArgSpec]:
    if i < len(DECLARED):
        faces = DECLARED[i]
    else:
        faces = _random_faces(i)
    count = len(faces)
    capacity = 2 * count - 1
    nodes = bytes([SENTINEL]) * (capacity * NODE_LEN)

    state = bytearray([SENTINEL]) * STATE_LEN
    struct.pack_into("<Q", state, 0x30, ARENA_BASE + NODES_AT)
    struct.pack_into("<i", state, 0x38, 0)
    struct.pack_into("<i", state, 0x3C, capacity)

    return [
        ArgSpec(kind="ptr", mem=bytes(state), arena_offset=STATE_AT),
        ArgSpec(kind="ptr", mem=b"".join(faces), arena_offset=FACES_AT),
        ArgSpec(kind="u64", bits=count),
        ArgSpec(kind="u64", bits=ARENA_BASE + BINS_AT),
        ArgSpec(kind="u64", bits=ARENA_BASE + PREFIX_AT),
        ArgSpec(kind="ptr", mem=nodes, arena_offset=NODES_AT),
    ]

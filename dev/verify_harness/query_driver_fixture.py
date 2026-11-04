"""Inputs for the `query_driver` unit (`0x142cbd510`).

The driver takes four arguments — the island record, the query context, the
query block and the four-pass flag byte — and reaches everything else through
pointers. One arena blob holds the whole world and every pointer inside it is
an absolute arena address:

    +0x000 ctx         +0x00 record pointer, +0x08 the filter's first eight
                       bytes INLINE, +0x10 the callback, +0x18 its third
                       argument, +0x20 the sink (null, so no push)
    +0x030 island      the per-pass table: `+0x30 + 0x28*p` is pass p's
                       entry-array pointer and `+0x44 + 0x28*p` its root index
    +0x100 record      0x340 bytes of the world
    +0x440 entry array two 0xc0-byte shape entries
    +0x5c0 owners      two 0x50-byte owner records
    +0x660 poses       two 0x1c0-byte body poses
    +0xa00 type-0 pool two 0x20-byte sphere records
    +0xa40 type-1 pool two 0x30-byte capsule records
    +0xaa0 type-2 pool two 0x50-byte records (unread primitive)
    +0xb40 type-3 pool two 0x50-byte records (mesh)
    +0xbe0 type-4 pool two 0x50-byte records (unread primitive)
    +0xe00 mesh world  the BVH nodes, vertices and triangles a type-3 element
                       points at
    +0x1000 BVH        four passes of three 0x40-byte broadphase nodes
    +0x4000 chain      120 internal nodes plus 120 leaves: a deep walk that
                       still fits the client's inline 0x100-entry worklist
    +0xf000 block      the query block, above the blob

A node is `{min[4], max[4], shape id i32, pad, first child i32, second child
i32}`: `+0x2c == -1` makes it a leaf whose `+0x20` is the shape index, and
otherwise `+0x2c` is the second child and the first is the node's own index
plus one.

The callback slot points at `mov eax, N; ret` in the blob, so the hit policy's
verdict is a constant the fixture chooses. That matters here: the wrapper
`0x142cc1610` returns the callback's value on a hit, and the driver folds a hit
into the swept box only when that value is 2 and ends the pass when it is 1.
"""

from __future__ import annotations

import random
import struct
from typing import List

from emu_oracle import ARENA_BASE, ArgSpec
from mesh_fixture import standard_world

BLOB_OFF = 0x0000
# The block sits above the blob: a chain tree needs more than the 0x2000 bytes
# a three-node pass used to take, and the arena is 0x10000.
BLOCK_OFF = 0xF000

CTX_AT = 0x000
# The island pointer is the blob's own base, so its per-pass table starts at
# blob offset 0x30; the context occupies 0x00..0x28 and shares the low bytes.
ISLAND_AT = 0x000
RECORD_AT = 0x100
ENTRIES_AT = 0x440
OWNERS_AT = 0x5C0
POSES_AT = 0x660
POOLS_AT = [0xA00, 0xA40, 0xAA0, 0xB40, 0xBE0]
MESH_AT = 0xE00
NODES_AT = 0x1000
POINTS_AT = 0x1400
CALLBACK_AT = 0x1FFF
# A chain tree, one internal node per level. It stays under the client's
# inline 0x100-entry worklist on purpose: growth goes through realloc, whose
# allocator object is zero in the offline image.
CHAIN_AT = 0x4000
CHAIN_N = 120
CHAIN_LEN = 2 * CHAIN_N * 0x40
BLOB_LEN = 0xE000

ENTRY_COUNT = 2
ENTRY_STRIDE = 0xC0
OWNER_STRIDE = 0x50
POSE_STRIDE = 0x1C0
POOL_STRIDES = [0x20, 0x30, 0x50, 0x50, 0x50]

PASSES = 4
NODES_PER_PASS = 3
NODE_STRIDE = 0x40
# A root set may name the chain tree instead of a three-node pass; the value
# is out of range of the small arrays, so it can never be mistaken for one.
CHAIN_ROOT = 3
WORKLIST_CAPACITY = 0x100

BLOCK_LEN = 0x80
MASK_ALL = 0x0001


def _quad(values) -> bytes:
    return struct.pack("<4f", *values)


def _at(offset: int) -> int:
    return ARENA_BASE + BLOB_OFF + offset


def _block_at(offset: int) -> int:
    return ARENA_BASE + BLOCK_OFF + offset


# The five segment shapes the narrowphase unit already pins, so a driver hit is
# a hit the leaf has been proven to produce.
SEGMENTS = [
    ((0.0, 0.0, -5.0), (0.0, 0.0, 5.0), 10.0, 0.5, 1),  # straight through
    ((-5.0, 0.2, 0.0), (5.0, 0.2, 0.0), 10.0, 0.0, 1),  # through, off centre
    ((0.0, 5.0, 0.0), (0.0, 5.0, 5.0), 5.0, 0.0, 0),  # misses, far above
    ((0.0, 0.0, 0.0), (0.0, 0.0, 1.0), 1.0, 0.0, 1),  # starts inside
    ((0.5, 0.5, 5.0), (0.5, 0.5, -5.0), 10.0, 0.25, 1),  # oblique
    # Diagonal in the xz plane: the swept box's axis-aligned bound is far
    # larger than the swept box itself, which is what lets a node pass the
    # bound test and still miss the exact one.
    ((-4.0, -4.0, 0.0), (4.0, 4.0, 0.0), 10.0, 0.0, 1),
]

SHAPES = [
    ((0.0, 0.0, 0.0), 1.0),  # sphere
    ((0.0, 0.0, 0.0), 0.25),
]

CAPSULES = [
    ((0.0, 0.0, -1.0), (0.0, 0.0, 1.0), 0.5),
    ((-1.0, 0.0, 0.0), (1.0, 0.0, 0.0), 0.25),
]

QUATS = [
    (0.0, 0.0, 0.0, 1.0),
    (0.0, 0.0, 0.70710678, 0.70710678),
    (0.5, -0.5, 0.5, 0.5),
    (2.0, 0.0, 0.0, 2.0),
]

# The filter pair that always passes: `mask = a[1] & b[0]` non-zero and both
# `a[0] & b[1]` and the signed words clear. The driver's own logic is what the
# unit is about, so the layer filter stays permissive here.
FILTER_PASS = ((0x0001, 0x0001, 0, 0), (0x0001, 0x0001, 0))
FILTER_BLOCK = ((0x0002, 0x0002, 0, 0), (0x0002, 0x0002, 0))

# Bounds a leaf is given before the per-case margin, one per shape kind.
LEAF_BOUNDS = {
    0: ((-1.0, -1.0, -1.0), (1.0, 1.0, 1.0)),
    1: ((-1.25, -0.75, -1.25), (1.25, 0.75, 1.25)),
    3: ((-1.0, -1.0, -1.0), (6.0, 1.0, 1.0)),
}

ROOT_SETS = [
    (0, -1, 2, -1),  # full tree, empty, a bare leaf, empty
    (-1, -1, -1, -1),  # nothing anywhere: the fold never runs
    (1, 0, 2, 1),  # a leaf as a root, the full tree, a leaf, a leaf
    (2, 2, 0, 0),  # the same roots repeated across passes
    (CHAIN_ROOT, -1, -1, -1),  # the chain tree alone, on the first pass
    (CHAIN_ROOT, 0, CHAIN_ROOT, -1),  # the chain twice, either side of a tree
]

# Where the two leaves sit relative to the shape's own bounds. The diagonal
# segment plus a corner placement is what drives a node past the axis-aligned
# test and into the exact one.
SHIFTS = [
    (0.0, 0.0, 0.0),  # on the sweep
    (2.5, -2.5, 1.0),  # the diagonal's near corner
    (-2.5, 2.5, 1.0),  # the opposite corner
    (0.0, 0.0, 2.0),  # above the sweep
    (6.0, 6.0, 0.0),  # outside the swept bound entirely
]

FLAGS = [0xF, 0x3, 0x5, 0x0, 0xA, 0x1, 0x8, 0xC]
# Types 0 and 1 only. A type-3 leaf is left out on purpose: the mesh walk's
# support-point hull path (taken whenever the block carries points) is not
# verified and diverges, and a mesh leaf makes the emulator's result depend on
# the preceding call, so neither belongs in a corpus that is meant to pin the
# driver's own logic.
KIND_PAIRS = [(0, 1), (1, 0)]
VERDICTS = [0, 1, 2, 3]


def _cases():
    rng = random.Random(142_0510)
    out = []
    for i in range(144):
        segment = SEGMENTS[i % len(SEGMENTS)]
        roots = ROOT_SETS[i % len(ROOT_SETS)]
        flags = FLAGS[i % len(FLAGS)]
        kinds = KIND_PAIRS[i % len(KIND_PAIRS)]
        verdict = VERDICTS[i % len(VERDICTS)]
        shift = SHIFTS[i % len(SHIFTS)]
        filter_pair = FILTER_BLOCK if i % 11 == 0 else FILTER_PASS
        # The support-point count stays zero: a positive count switches every
        # primitive onto its hull path, which is not verified (see the notes),
        # so the driver's own logic is what this corpus measures. The pointer
        # is still a real address so a stray read would be visible.
        count = 0
        points = []
        quat = QUATS[i % len(QUATS)]
        margins = (rng.uniform(-0.6, 0.9), rng.uniform(-0.6, 0.9))
        out.append(
            (
                segment,
                roots,
                flags,
                kinds,
                verdict,
                filter_pair,
                points,
                quat,
                margins,
                shift,
            )
        )
    return out


CASES = _cases()


def _filter_bytes(a, b) -> bytes:
    mask, alt, signed, byte7 = a
    out = bytearray(0x10)
    struct.pack_into("<H", out, 0, mask)
    struct.pack_into("<H", out, 2, alt)
    struct.pack_into("<h", out, 4, signed)
    out[7] = byte7
    b_mask, b_alt, b_signed = b
    struct.pack_into("<H", out, 8, b_mask)
    struct.pack_into("<H", out, 10, b_alt)
    struct.pack_into("<h", out, 12, b_signed)
    return bytes(out)


def _block(segment, points, quat) -> bytes:
    start, end, motion, expansion, flag = segment
    block = bytearray(BLOCK_LEN)
    block[0x00:0x10] = _quad([start[0], start[1], start[2], 0.0])
    block[0x10:0x20] = _quad([end[0], end[1], end[2], 0.0])
    block[0x20:0x30] = _quad([motion, motion, motion, motion])
    struct.pack_into("<Q", block, 0x30, _at(POINTS_AT))
    struct.pack_into("<i", block, 0x38, len(points))
    block[0x40:0x50] = _quad(quat)
    block[0x50:0x60] = _quad([expansion, expansion, expansion, expansion])
    struct.pack_into("<H", block, 0x70, MASK_ALL)
    block[0x72] = flag
    return bytes(block)


def _points(points) -> bytes:
    out = bytearray()
    for point in points:
        out += _quad([point[0], point[1], point[2], 0.0])
    return bytes(out)


def _node(bounds_min, bounds_max, shape, first_child, second_child) -> bytes:
    node = bytearray(NODE_STRIDE)
    node[0x00:0x10] = _quad([bounds_min[0], bounds_min[1], bounds_min[2], 0.0])
    node[0x10:0x20] = _quad([bounds_max[0], bounds_max[1], bounds_max[2], 0.0])
    struct.pack_into("<i", node, 0x20, shape)
    struct.pack_into("<i", node, 0x2C, first_child)
    struct.pack_into("<i", node, 0x30, second_child)
    return bytes(node)


def _leaf_bounds(kind, margin, shift=(0.0, 0.0, 0.0)) -> tuple:
    low, high = LEAF_BOUNDS[kind]
    return (
        (low[0] - margin + shift[0], low[1] - margin + shift[1], low[2] - margin + shift[2]),
        (high[0] + margin + shift[0], high[1] + margin + shift[1], high[2] + margin + shift[2]),
    )


def make_args(i: int) -> List[ArgSpec]:
    (
        segment,
        roots,
        flags,
        kinds,
        verdict,
        filter_pair,
        points,
        quat,
        margins,
        shift,
    ) = CASES[i % len(CASES)]
    filter_bytes = _filter_bytes(*filter_pair)

    blob = bytearray(BLOB_LEN)

    # The context.
    struct.pack_into("<Q", blob, CTX_AT + 0x00, _at(RECORD_AT))
    blob[CTX_AT + 0x08 : CTX_AT + 0x10] = filter_bytes[:8]
    struct.pack_into("<Q", blob, CTX_AT + 0x10, _at(CALLBACK_AT - 5))
    struct.pack_into("<Q", blob, CTX_AT + 0x18, 0)
    struct.pack_into("<Q", blob, CTX_AT + 0x20, 0)  # no sink

    # `mov eax, verdict; ret`, the hit callback's stand-in.
    blob[CALLBACK_AT - 5] = 0xB8
    struct.pack_into("<I", blob, CALLBACK_AT - 4, verdict)
    blob[CALLBACK_AT] = 0xC3

    # The island's per-pass table.
    for pass_index in range(PASSES):
        at = ISLAND_AT + 0x30 + 0x28 * pass_index
        root = roots[pass_index]
        if root == CHAIN_ROOT:
            struct.pack_into("<Q", blob, at + 0x00, _at(CHAIN_AT))
            struct.pack_into("<i", blob, ISLAND_AT + 0x44 + 0x28 * pass_index, 0)
        else:
            struct.pack_into(
                "<Q",
                blob,
                at + 0x00,
                _at(NODES_AT + pass_index * NODES_PER_PASS * NODE_STRIDE),
            )
            struct.pack_into("<i", blob, ISLAND_AT + 0x44 + 0x28 * pass_index, root)

    # The world record.
    struct.pack_into("<Q", blob, RECORD_AT + 0x288, _at(POSES_AT))
    struct.pack_into("<Q", blob, RECORD_AT + 0x2A0, _at(OWNERS_AT))
    struct.pack_into("<Q", blob, RECORD_AT + 0x2B8, _at(ENTRIES_AT))
    for slot, base in enumerate((0x2D0, 0x2E8, 0x300, 0x318, 0x330)):
        struct.pack_into("<Q", blob, RECORD_AT + base, _at(POOLS_AT[slot]))
    blob[RECORD_AT + 0x191] = 0
    blob[RECORD_AT + 0x192] = 0
    struct.pack_into("<H", blob, RECORD_AT + 0x194, 0x1234)

    # Two entries, one per shape kind the case names.
    for entry in range(ENTRY_COUNT):
        at = ENTRIES_AT + entry * ENTRY_STRIDE
        struct.pack_into("<Q", blob, at + 0x00, _at(OWNERS_AT + entry * OWNER_STRIDE))
        struct.pack_into("<Q", blob, at + 0x10, _at(POSES_AT + entry * POSE_STRIDE))
        blob[at + 0x20] = kinds[entry]
        struct.pack_into("<i", blob, at + 0x24, 0)
        struct.pack_into("<i", blob, at + 0x50, entry)
        blob[at + 0x98 : at + 0x98 + 8] = filter_bytes[8:16]
        struct.pack_into("<I", blob, at + 0xA0, 0xA0 + entry)
        struct.pack_into("<H", blob, at + 0xA8, 0xA8 + entry)
        struct.pack_into("<I", blob, at + 0xAA, 0xAA00 + entry)
        struct.pack_into("<H", blob, at + 0xAE, 0xAE00 + entry)
        blob[at + 0xB0] = 0

    # Owners: quaternion at +0x00, position at +0x10. The first sits on the
    # origin so the shapes start where the segments aim.
    for owner in range(ENTRY_COUNT):
        at = OWNERS_AT + owner * OWNER_STRIDE
        blob[at : at + 0x10] = _quad(QUATS[0] if owner == 0 else quat)
        blob[at + 0x10 : at + 0x20] = _quad(
            (0.0, 0.0, 0.0, 0.0) if owner == 0 else (0.5, 0.25, -0.5, 0.0)
        )

    # Body poses: position at +0x10, quaternion at +0x20.
    for pose in range(ENTRY_COUNT):
        at = POSES_AT + pose * POSE_STRIDE
        blob[at + 0x10 : at + 0x20] = _quad((0.0, 0.0, 0.0, 0.0))
        blob[at + 0x20 : at + 0x30] = _quad(quat)

    # The per-type pools.
    for element in range(ENTRY_COUNT):
        for slot, base in enumerate(POOLS_AT):
            at = base + element * POOL_STRIDES[slot]
            if slot == 0:
                centre, radius = SHAPES[element]
                blob[at : at + 0x10] = _quad((centre[0], centre[1], centre[2], 0.0))
                struct.pack_into("<f", blob, at + 0x10, radius)
            elif slot == 1:
                p0, p1, radius = CAPSULES[element]
                blob[at : at + 0x10] = _quad((p0[0], p0[1], p0[2], 0.0))
                blob[at + 0x10 : at + 0x20] = _quad((p1[0], p1[1], p1[2], 0.0))
                struct.pack_into("<f", blob, at + 0x20, radius)
            elif slot == 3:
                blob[at : at + 0x10] = _quad((1.0, 1.0, 1.0, 1.0))
                blob[at + 0x10 : at + 0x20] = _quad((1.0, 1.0, 1.0, 1.0))
                struct.pack_into("<Q", blob, at + 0x20, _at(MESH_AT + 0x000))
                struct.pack_into("<Q", blob, at + 0x28, _at(MESH_AT + 0x100))
                struct.pack_into("<Q", blob, at + 0x30, _at(MESH_AT + 0x180))
            else:
                struct.pack_into("<f", blob, at, float(element + 1))

    # The mesh world the type-3 elements point at.
    blob[MESH_AT : MESH_AT + 0x200] = standard_world()

    # The support points. The count stays zero, but the array is present and
    # non-trivial so a query that read it anyway would answer differently.
    blob[POINTS_AT : POINTS_AT + 0x20] = _points([(0.0, 0.0, 0.0), (1.0, 1.0, 1.0)])

    # The chain tree: node i pushes its own leaf first and the next level
    # second, so the leaf stays pending while the walk descends. One pending
    # leaf per level is what overruns the client's inline worklist.
    wide = (-1.0e3, 1.0e3)
    for level in range(CHAIN_N):
        base = CHAIN_AT + level * NODE_STRIDE
        if level + 1 < CHAIN_N:
            children = (CHAIN_N + level, level + 1)
        else:
            children = (CHAIN_N + level, CHAIN_N + level - 1)
        blob[base : base + NODE_STRIDE] = _node(
            (wide[0], wide[0], wide[0]),
            (wide[1], wide[1], wide[1]),
            -1,
            children[0],
            children[1],
        )
        leaf = CHAIN_AT + (CHAIN_N + level) * NODE_STRIDE
        blob[leaf : leaf + NODE_STRIDE] = _node(
            (wide[0], wide[0], wide[0]),
            (wide[1], wide[1], wide[1]),
            level % ENTRY_COUNT,
            -1,
            0,
        )

    # The broadphase: three nodes per pass, a root and its two leaves.
    for pass_index in range(PASSES):
        base = NODES_AT + pass_index * NODES_PER_PASS * NODE_STRIDE
        low_a, high_a = _leaf_bounds(kinds[0], margins[0], shift)
        low_b, high_b = _leaf_bounds(kinds[1], margins[1], shift)
        low = tuple(min(low_a[k], low_b[k]) for k in range(3))
        high = tuple(max(high_a[k], high_b[k]) for k in range(3))
        blob[base : base + NODE_STRIDE] = _node(low, high, -1, 1, 2)
        blob[base + NODE_STRIDE : base + 2 * NODE_STRIDE] = _node(
            low_a, high_a, 0, -1, 0
        )
        blob[base + 2 * NODE_STRIDE : base + 3 * NODE_STRIDE] = _node(
            low_b, high_b, 1, -1, 0
        )

    return [
        ArgSpec(kind="ptr", mem=bytes(blob), arena_offset=BLOB_OFF),
        ArgSpec(kind="ptr", mem=bytes(blob[:0x28]), arena_offset=CTX_AT),
        ArgSpec(
            kind="ptr",
            mem=_block(segment, points, quat),
            arena_offset=BLOCK_OFF,
        ),
        ArgSpec(kind="u64", bits=flags),
    ]

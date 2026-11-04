"""Inputs for the `collision_narrowphase` unit (`0x142cc1610`).

The leaf takes four arguments — the query context, the output quad, the query
block and the shape index — and reaches everything else through pointers. So
one arena blob holds the whole world and every pointer inside it is an absolute
arena address:

    +0x000 ctx        +0x00 record pointer, +0x08 the filter's first eight
                      bytes INLINE (the leaf is handed `ctx + 8` itself),
                      +0x10 the callback, +0x18 its third argument, +0x20 the
                      sink
    +0x040 record     0x340 bytes of the world, holding the pointers at
                      +0x288/+0x2a0/+0x2b8 and the five per-type pool
                      pointers at +0x2d0..+0x330
    +0x400 entry array  two 0xc0-byte shape entries
    +0x580 owners       two 0x50-byte owner records
    +0x620 poses        two 0x1c0-byte body poses
    +0xa00 type-0 pool  two 0x20-byte sphere records
    +0xa40 type-1 pool  two 0x30-byte capsule records
    +0xaa0 type-2 pool  two 0x50-byte records (unread primitive)
    +0xb00 type-3 pool  two 0x50-byte records (mesh; needs a mesh world)
    +0xba0 type-4 pool  two 0x50-byte records (unread primitive)
    +0xd00 mesh world   the BVH nodes, vertices and triangles a type-3 element
                      points at

The corpus stays on types **0 and 1** — the two per-type primitives that are
verified — because the emulator would run the unread ones and there is nothing
to compare against. It varies the shape index, the pool index, both transform
branches (`entry[0xb0]`), the layer filter's three outcomes, and hits against
misses.

`ctx[0x20]` is the sink and is left null, so the wrapper skips its push
entirely; the callback slot points at `0x140c9f110`, the leaf that returns a
zero quad, so the call is a real call that writes nothing.
"""

from __future__ import annotations

import struct
from typing import List

from emu_oracle import ARENA_BASE, ArgSpec
from mesh_fixture import standard_world

BLOB_OFF = 0x0000
OUT_OFF = 0x2000
BLOCK_OFF = 0x2100

CTX_AT = 0x000
RECORD_AT = 0x040
ENTRIES_AT = 0x400
OWNERS_AT = 0x580
POSES_AT = 0x620
POOLS_AT = [0xA00, 0xA40, 0xAA0, 0xB00, 0xBA0]
POOL_STRIDES = [0x20, 0x30, 0x50, 0x50, 0x50]
MESH_AT = 0xD00
MESH_NODES = 0x000
MESH_VERTICES = 0x100
MESH_TRIANGLES = 0x180
BLOB_LEN = 0x1000

ENTRY_STRIDE = 0xC0
ENTRY_COUNT = 2
OWNER_STRIDE = 0x50
POSE_STRIDE = 0x1C0

# The hit callback is reached on every hit, before the quad is published, so it
# has to be a real call that returns. The blob's last byte is a lone `ret` and
# stands in for it: the emulator runs it, the wrapper continues, and the
# callback's return value only reaches the sink push, which a null sink skips.
# (The harness's own page of `ret` cannot be used - it is the stop address.)
CALLBACK_AT = BLOB_LEN - 1
CALLBACK = ARENA_BASE + CALLBACK_AT


def _quad(values) -> bytes:
    return struct.pack("<4f", *values)


def _at(offset: int) -> int:
    return ARENA_BASE + BLOB_OFF + offset


def _block(start, end, motion, expansion, flag) -> bytes:
    block = bytearray(0x78)
    block[0x00:0x10] = _quad([start[0], start[1], start[2], 0.0])
    block[0x10:0x20] = _quad([end[0], end[1], end[2], 0.0])
    block[0x20:0x30] = _quad([motion, motion, motion, motion])
    struct.pack_into("<i", block, 0x38, 0)
    block[0x50:0x60] = _quad([expansion, expansion, expansion, expansion])
    # The triangle/leaf mask the mesh walk tests against.
    struct.pack_into("<H", block, 0x70, MASK_ALL)
    block[0x72] = flag
    return bytes(block)


MASK_ALL = 0x0001


def _mesh_world() -> bytes:
    """The mesh unit's own standard world: two quads under an internal root,
    laid out as nodes at +0x00, vertices at +0x100 and triangles at +0x180.

    Reused rather than rebuilt so the BVH the wrapper walks here is the one
    `fn/MeshSweep` was verified against; quad B sits at x = 5..6 and is out of
    every segment's way, so it only exercises the walk's second child.
    """
    return standard_world()


# `ShapeQueryFilter(ctx+8, entry+0x98)` reads `a[0]`/`a[1]`/`a[2]` as three
# `u16`s and `a[7]` as a byte, and `b[0]`/`b[1]`/`b[2]` as three `u16`s:
#
#   mask = a[1] & b[0]                       # a's SECOND word against b's first
#   a[7] != 0            -> answer is (mask != 0)
#   mask == 0            -> reject
#   a[0] & b[1] == 0     -> reject            # a's first word against b's second
#   a[2] or b[2] non-zero:
#       |a[2]| == |b[2]| -> reject when either is < 1
#       else             -> reject when either is > 0
#
# So the pairs below are (a_mask, a_alt, a_signed, a_byte7) and
# (b_mask, b_alt, b_signed), and the two words that must meet are `a_alt` with
# `b_mask` and `a_mask` with `b_alt`.
FILTERS = [
    # Both signed words zero: the magnitude test is skipped entirely.
    ((0x00FF, 0x00FF, 0, 0), (0x00FF, 0x00FF, 0)),
    # Equal and positive magnitudes: accepted.
    ((0x00FF, 0x00FF, 2, 0), (0x00FF, 0x00FF, 2)),
    # Equal and negative magnitudes: rejected, because either being < 1 fails.
    ((0x00FF, 0x00FF, -2, 0), (0x00FF, 0x00FF, -2)),
    # Equal magnitudes of mixed sign: rejected.
    ((0x00FF, 0x00FF, 2, 0), (0x00FF, 0x00FF, -2)),
    # Unequal magnitudes, both negative: accepted.
    ((0x00FF, 0x00FF, -2, 0), (0x00FF, 0x00FF, -3)),
    # Unequal magnitudes, one positive: rejected.
    ((0x00FF, 0x00FF, -2, 0), (0x00FF, 0x00FF, 3)),
    # Unequal magnitudes, both positive: rejected, because either being > 0 fails.
    ((0x00FF, 0x00FF, 1, 0), (0x00FF, 0x00FF, 2)),
    # The first word pair misses.
    ((0x00FF, 0x0000, 0, 0), (0x00FF, 0x00FF, 0)),
    # The second word pair misses.
    ((0x0000, 0x00FF, 0, 0), (0x00FF, 0x00FF, 0)),
    # a[7] short circuit, mask non-zero: accepted with every other word zero.
    ((0x0000, 0x00FF, 0, 1), (0x00FF, 0x0000, 0)),
    # a[7] short circuit, mask zero: rejected.
    ((0x0000, 0x0000, 0, 1), (0x00FF, 0x00FF, 0)),
]

# Every shape sits at the origin of its own element, so the owner's position is
# what moves it. The segments then pass through or clearly beside the origin
# with margins far larger than the primitives' epsilon - the one tie-free way to
# keep the hit/miss boundary out of the corpus.
SEGMENTS = [
    ((0.0, 0.0, -5.0), (0.0, 0.0, 5.0), 10.0, 0.5, 1),  # straight through
    ((-5.0, 0.2, 0.0), (5.0, 0.2, 0.0), 10.0, 0.0, 1),  # through, off centre
    ((0.0, 5.0, 0.0), (0.0, 5.0, 5.0), 5.0, 0.0, 0),  # misses, far above
    ((0.0, 0.0, 0.0), (0.0, 0.0, 1.0), 1.0, 0.0, 1),  # starts inside
    ((0.5, 0.5, 5.0), (0.5, 0.5, -5.0), 10.0, 0.25, 1),  # oblique
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


KINDS = (0, 1, 3)


def _cases():
    cases = []
    for shape in (0, 1):
        for kind in (0, 1, 3):
            for pool_index in (0, 1):
                for attach in (0, 1):
                    for filter in FILTERS:
                        segment = SEGMENTS[len(cases) % len(SEGMENTS)]
                        quat = QUATS[len(cases) % len(QUATS)]
                        cases.append(
                            (shape, kind, pool_index, attach, filter, segment, quat)
                        )
    return cases


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


def make_args(i: int) -> List[ArgSpec]:
    shape, kind, pool_index, attach, filter_pair, segment, quat = CASES[i % len(CASES)]
    filter_bytes = _filter_bytes(*filter_pair)

    blob = bytearray(BLOB_LEN)

    # The context.
    struct.pack_into("<Q", blob, CTX_AT + 0x00, _at(RECORD_AT))
    blob[CTX_AT + 0x08 : CTX_AT + 0x10] = filter_bytes[:8]
    struct.pack_into("<Q", blob, CTX_AT + 0x10, CALLBACK)
    struct.pack_into("<Q", blob, CTX_AT + 0x18, 0)
    struct.pack_into("<Q", blob, CTX_AT + 0x20, 0)  # no sink

    # A lone `ret`, the stand-in hit callback.
    blob[CALLBACK_AT] = 0xC3

    # The world record.
    struct.pack_into("<Q", blob, RECORD_AT + 0x288, _at(POSES_AT))
    struct.pack_into("<Q", blob, RECORD_AT + 0x2A0, _at(OWNERS_AT))
    struct.pack_into("<Q", blob, RECORD_AT + 0x2B8, _at(ENTRIES_AT))
    for slot, base in enumerate((0x2D0, 0x2E8, 0x300, 0x318, 0x330)):
        struct.pack_into("<Q", blob, RECORD_AT + base, _at(POOLS_AT[slot]))
    blob[RECORD_AT + 0x191] = 1 if attach else 0
    blob[RECORD_AT + 0x192] = 0
    struct.pack_into("<H", blob, RECORD_AT + 0x194, 0x1234)

    # Both entries carry the same shape kind and the case's pool index; the
    # second one differs in its id fields so a shape-index mix-up is visible.
    for entry in range(ENTRY_COUNT):
        at = ENTRIES_AT + entry * ENTRY_STRIDE
        blob[at + 0x20] = kind
        struct.pack_into("<i", blob, at + 0x24, pool_index)
        struct.pack_into("<i", blob, at + 0x50, entry)
        blob[at + 0x98 : at + 0x98 + 8] = filter_bytes[8:16]
        struct.pack_into("<I", blob, at + 0xA0, 0xA0 + entry)
        struct.pack_into("<H", blob, at + 0xA8, 0xA8 + entry)
        struct.pack_into("<I", blob, at + 0xAA, 0xAA00 + entry)
        struct.pack_into("<H", blob, at + 0xAE, 0xAE00 + entry)
        blob[at + 0xB0] = attach

    # Owner records: quaternion at +0x00, position at +0x10. The first owner
    # sits on the origin so the shapes start where the segments aim.
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

    # The per-type pools. Only the case's kind is read, but every pool is
    # populated so a wrong pool base or stride shows up as a different hit.
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
                # The type-3 element is `{scale[4], 1/scale[4], root,
                # vertices, triangles}` with the three pointers absolute; the
                # world they name is the blob's own mesh region.
                blob[at : at + 0x10] = _quad((1.0, 1.0, 1.0, 1.0))
                blob[at + 0x10 : at + 0x20] = _quad((1.0, 1.0, 1.0, 1.0))
                struct.pack_into("<Q", blob, at + 0x20, _at(MESH_AT + MESH_NODES))
                struct.pack_into("<Q", blob, at + 0x28, _at(MESH_AT + MESH_VERTICES))
                struct.pack_into("<Q", blob, at + 0x30, _at(MESH_AT + MESH_TRIANGLES))
            else:
                # Unread primitives: keep the bytes non-zero and distinct so a
                # mis-selected pool is detectable, never executed.
                struct.pack_into("<f", blob, at, float(element + 1))

    # The mesh world the type-3 elements point at.
    blob[MESH_AT : MESH_AT + 0x200] = _mesh_world()

    start, end, motion, expansion, flag = segment
    return [
        ArgSpec(kind="ptr", mem=bytes(blob), arena_offset=BLOB_OFF),
        ArgSpec(kind="ptr", mem=bytes(0x20), arena_offset=OUT_OFF),
        ArgSpec(kind="ptr", mem=_block(start, end, motion, expansion, flag), arena_offset=BLOCK_OFF),
        ArgSpec(kind="u64", bits=shape),
    ]

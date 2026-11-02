"""Inputs for the `mesh_sweep` unit (`0x142caed30`, type 3).

The pool element is `{scale[4], 1/scale[4], root, vertices, triangles}`. The
pointee carries the element followed by its whole mesh world, and the three
baked pointers are arena addresses into that tail (`ARENA_BASE` is fixed, so
the fixture patches the absolute address in).

The standard mesh is two quads under an internal root: quad A spans the unit
square at z = 0 (triangles 0 and 1) and quad B spans x = 5..6 (triangles 2
and 3), both wound counter-clockwise so their normals point up.

The declared specials come first, so the mutation corpus covers them: the
diagonal split of quad A, the miss beside the mesh, the back face, a sweep
that starts past the plane, a zero-length query, the masked block, the masked
triangle, the degenerate triangle, two stacked faces and a rotated transform.
"""

from __future__ import annotations

import struct
from typing import List, Tuple

from emu_oracle import ARENA_BASE, ArgSpec

POOL_OFF = 0x4000
POOL_LEN = 0x50
WORLD_AT = 0x000
VERTICES_AT = 0x100
TRIANGLES_AT = 0x180
WORLD_LEN = 0x200

POOL_ADDR = ARENA_BASE + POOL_OFF
WORLD_ADDR = POOL_ADDR + POOL_LEN

OUT_OFF = 0x6000
BLOCK_OFF = 0x6100
XFORM_OFF = 0x6200

MASK_ALL = 0x0001


def _fbits(value: float) -> int:
    return struct.unpack("<I", struct.pack("<f", value))[0]


def _quad(values) -> bytes:
    lanes = list(values) + [0.0] * (4 - len(values))
    return b"".join(struct.pack("<f", v) for v in lanes[:4])


def node(low, high, child: int, mask: int = MASK_ALL, axis: int = 0, count: int = 0) -> bytes:
    rec = bytearray(0x20)
    rec[0x00:0x0C] = _quad(low)[:0x0C]
    rec[0x0C:0x18] = _quad(high)[:0x0C]
    struct.pack_into("<i", rec, 0x18, child)
    struct.pack_into("<H", rec, 0x1C, mask)
    rec[0x1E] = axis
    rec[0x1F] = count
    return bytes(rec)


def triangle(corners: Tuple[int, int, int], ids: Tuple[int, int] = (0, 0), mask: int = MASK_ALL) -> bytes:
    rec = bytearray(0x20)
    for slot, corner in enumerate(corners):
        struct.pack_into("<i", rec, slot * 4, corner)
    struct.pack_into("<H", rec, 0x18, ids[0])
    struct.pack_into("<H", rec, 0x1A, ids[1])
    struct.pack_into("<H", rec, 0x1C, mask)
    return bytes(rec)


QUAD_A = [(0.0, 0.0, 0.0), (1.0, 0.0, 0.0), (1.0, 1.0, 0.0), (0.0, 1.0, 0.0)]
QUAD_B = [(5.0, 0.0, 0.0), (6.0, 0.0, 0.0), (6.0, 1.0, 0.0), (5.0, 1.0, 0.0)]


def vertices(points) -> bytes:
    return b"".join(_quad([p[0], p[1], p[2], 0.0]) for p in points)


def standard_world(triangle_masks=None) -> bytes:
    """Two quads, an internal root and two leaves."""
    masks = triangle_masks or (MASK_ALL,) * 4
    nodes = (
        # The root: second child sits two nodes on (the quad-B leaf).
        node([-1.0, -1.0, -1.0], [7.0, 2.0, 1.0], 2, MASK_ALL, 0, 0)
        + node([0.0, 0.0, 0.0], [1.0, 1.0, 0.0], 0, MASK_ALL, 0, 2)
        + node([5.0, 0.0, 0.0], [6.0, 1.0, 0.0], 2, MASK_ALL, 0, 2)
    )
    tris = (
        triangle((0, 1, 2), (0x11, 0x21), masks[0])
        + triangle((0, 2, 3), (0x12, 0x22), masks[1])
        + triangle((4, 5, 6), (0x13, 0x23), masks[2])
        + triangle((4, 6, 7), (0x14, 0x24), masks[3])
    )
    world = bytearray(WORLD_LEN)
    world[WORLD_AT : WORLD_AT + len(nodes)] = nodes
    world[VERTICES_AT : VERTICES_AT + 0x80] = vertices(QUAD_A + QUAD_B)
    world[TRIANGLES_AT : TRIANGLES_AT + len(tris)] = tris
    return bytes(world)


def pool(scale: float, inverse: float, world: bytes) -> bytes:
    rec = bytearray(POOL_LEN)
    rec[0x00:0x10] = _quad([scale] * 4)
    rec[0x10:0x20] = _quad([inverse] * 4)
    # Relative to the pointee's own base; `relocs` rebase these to arena
    # addresses on the oracle side and to the local buffer on ours.
    struct.pack_into("<Q", rec, 0x20, POOL_LEN + WORLD_AT)
    struct.pack_into("<Q", rec, 0x28, POOL_LEN + VERTICES_AT)
    struct.pack_into("<Q", rec, 0x30, POOL_LEN + TRIANGLES_AT)
    rec[0x38:0x50] = bytes(0x18)
    return bytes(rec)


def block(start, end, motion: float = 1.0, expansion: float = 0.0, count: int = 0, mask: int = MASK_ALL) -> bytes:
    rec = bytearray(0x78)
    rec[0x00:0x10] = _quad(start)
    rec[0x10:0x20] = _quad(end)
    rec[0x20:0x30] = _quad([motion] * 4)
    rec[0x38:0x3C] = struct.pack("<i", count)
    rec[0x50:0x60] = _quad([expansion] * 4)
    struct.pack_into("<H", rec, 0x70, mask)
    return bytes(rec)


def xform(position, quat=(0.0, 0.0, 0.0, 1.0)) -> bytes:
    return _quad(position) + _quad(quat)


# `(scale, world, block, xform)` per case.
def _cases() -> List[Tuple[float, bytes, bytes, bytes]]:
    ident = xform([0.0, 0.0, 0.0])
    down = block([0.75, 0.9, 5.0, 0.0], [0.75, 0.9, -5.0, 0.0])
    world = standard_world()

    # A degenerate leaf: one triangle whose three corners coincide.
    degenerate = bytearray(WORLD_LEN)
    degenerate[WORLD_AT : WORLD_AT + 0x20] = node(
        [0.0, 0.0, 0.0], [0.0, 0.0, 0.0], 0, MASK_ALL, 0, 1
    )
    degenerate[VERTICES_AT : VERTICES_AT + 0x30] = vertices([(0.5, 0.5, 0.0)] * 3)
    degenerate[TRIANGLES_AT : TRIANGLES_AT + 0x20] = triangle((0, 1, 2), (0x31, 0x32))

    # Two stacked faces: the walk keeps the last triangle it accepts, which is
    # also the farther one, so a "closest hit" reading would diverge.
    stacked = bytearray(standard_world(triangle_masks=(MASK_ALL, MASK_ALL, 0x0002, 0x0002)))
    stacked[VERTICES_AT + 4 * 0x10 : VERTICES_AT + 8 * 0x10] = vertices(
        [(0.0, 0.0, -1.0), (1.0, 0.0, -1.0), (1.0, 1.0, -1.0), (0.0, 1.0, -1.0)]
    )
    stacked_block = block(
        [0.75, 0.9, 5.0, 0.0], [0.75, 0.9, -5.0, 0.0], 1.0, 0.0, 0, 0x0002
    )

    # Every node on layer 0x0001, so a block on layer 0x0002 skips the walk.
    layered = bytearray(world)
    for slot in range(3):
        struct.pack_into("<H", layered, WORLD_AT + slot * 0x20 + 0x1C, 0x0001)

    # Quad B moved off the diagonal rod: its box still overlaps the sweep's
    # axis-aligned hull but the oriented rod test rejects it.
    off_axis = bytearray(world)
    off_axis[VERTICES_AT + 4 * 0x10 : VERTICES_AT + 8 * 0x10] = vertices(
        [(3.0, -4.0, 0.0), (4.0, -4.0, 0.0), (4.0, -3.0, 0.0), (3.0, -3.0, 0.0)]
    )
    for slot in range(2):
        base = WORLD_AT + (2 + slot) * 0x20
        off_axis[base + 0x00 : base + 0x0C] = _quad([3.0, -4.0, -1.0])[:0x0C]
        off_axis[base + 0x0C : base + 0x18] = _quad([4.0, -3.0, 1.0])[:0x0C]

    def shifted(offset: float) -> bytes:
        """Quad B moved to [offset, offset + 1] on x, box and vertices."""
        out = bytearray(standard_world())
        out[VERTICES_AT + 4 * 0x10 : VERTICES_AT + 8 * 0x10] = vertices(
            [
                (offset, 0.0, 0.0),
                (offset + 1.0, 0.0, 0.0),
                (offset + 1.0, 1.0, 0.0),
                (offset, 1.0, 0.0),
            ]
        )
        for slot in range(2):
            base = WORLD_AT + (2 + slot) * 0x20
            out[base + 0x00 : base + 0x0C] = _quad([offset, 0.0, 0.0])[:0x0C]
            out[base + 0x0C : base + 0x18] = _quad([offset + 1.0, 1.0, 0.0])[:0x0C]
        return bytes(out)

    # A quarter turn about z: the unit square moves to the negative x strip.
    turned = xform([0.0, 0.0, 0.0], (0.0, 0.0, 0.7071067811865476, 0.7071067811865476))
    turned_block = block([-0.9, 0.75, 5.0, 0.0], [-0.9, 0.75, -5.0, 0.0])

    # One leaf holding two faces at different depths, nearest first: the client
    # keeps the last accepted triangle, so a motion budget that shrank after the
    # first acceptance would stop at the near face instead.
    two_deep = bytearray(WORLD_LEN)
    two_deep[WORLD_AT : WORLD_AT + 0x20] = node(
        [0.0, 0.0, 0.0], [1.0, 1.0, 3.0], 0, MASK_ALL, 0, 4
    )
    two_deep[VERTICES_AT : VERTICES_AT + 0x80] = vertices(
        [(0.0, 0.0, 3.0), (1.0, 0.0, 3.0), (1.0, 1.0, 3.0), (0.0, 1.0, 3.0)]
        + [(0.0, 0.0, 0.0), (1.0, 0.0, 0.0), (1.0, 1.0, 0.0), (0.0, 1.0, 0.0)]
    )
    two_deep[TRIANGLES_AT : TRIANGLES_AT + 0x80] = (
        triangle((0, 1, 2), (0x15, 0x25))
        + triangle((0, 2, 3), (0x16, 0x26))
        + triangle((4, 5, 6), (0x17, 0x27))
        + triangle((4, 6, 7), (0x18, 0x28))
    )

    return [
        # 0: the diagonal split of quad A - only triangle 1 covers (0.75, 0.9).
        (1.0, world, down, ident),
        # 1: the other side of the diagonal - only triangle 0 covers it.
        (1.0, world, block([0.2, 0.2, 5.0, 0.0], [0.2, 0.2, -5.0, 0.0]), ident),
        # 2: quad B, the far leaf; the root visits it second.
        (1.0, world, block([5.5, 0.25, 5.0, 0.0], [5.5, 0.25, -5.0, 0.0]), ident),
        # 3: beside both quads - a clean miss with both leaves considered.
        (1.0, world, block([3.0, 3.0, 5.0, 0.0], [3.0, 3.0, -5.0, 0.0]), ident),
        # 4: from below, so the sweep meets the back face (det < 0).
        (1.0, world, block([0.75, 0.9, -5.0, 0.0], [0.75, 0.9, 5.0, 0.0]), ident),
        # 5: starting past the plane and moving away (t < 0).
        (1.0, world, block([0.75, 0.9, 1.0, 0.0], [0.75, 0.9, 3.0, 0.0]), ident),
        # 6: a zero-length query lying in the plane (t == 0 exactly).
        (1.0, world, block([0.75, 0.9, 0.0, 0.0], [0.75, 0.9, 0.0, 0.0], 0.0), ident),
        # 7: the block's layer word intersects nothing.
        (1.0, world, block([0.75, 0.9, 5.0, 0.0], [0.75, 0.9, -5.0, 0.0], 1.0, 0.0, 0, 0x0002), ident),
        # 8: triangle 1 is masked out, so the covered side of the diagonal misses.
        (1.0, standard_world(triangle_masks=(MASK_ALL, 0x0002, MASK_ALL, MASK_ALL)), down, ident),
        # 9: a degenerate triangle leaves the scratch output empty.
        (1.0, bytes(degenerate), block([0.5, 0.5, 5.0, 0.0], [0.5, 0.5, -5.0, 0.0]), ident),
        # 10: the last accepted triangle wins over the nearest one.
        (1.0, bytes(stacked), stacked_block, ident),
        # 11: a rotated shape transform.
        (1.0, world, turned_block, turned),
        # 12: scale two with its reciprocal - the query halves on the way in.
        (2.0, world, block([1.5, 0.5, 5.0, 0.0], [1.5, 0.5, -5.0, 0.0]), ident),
        # 13: a translated shape transform.
        (1.0, world, down, xform([0.0, 0.0, 3.0])),
        # 14: a small expansion grows the node boxes by 0.02 but not the faces.
        (1.0, world, block([0.75, 0.9, 5.0, 0.0], [0.75, 0.9, -5.0, 0.0], 1.0, 0.004), ident),
        # 15: a sweep that only the expansion lets reach the leaves.
        (1.0, world, block([-1.002, 0.5, 5.0, 0.0], [-1.002, 0.5, -5.0, 0.0], 1.0, 0.004), ident),
        # 16: along +x, so the direction split's axis test swaps the children and
        # the plane is only grazed (det == 0).
        (1.0, world, block([-5.0, 0.5, 0.0, 0.0], [5.0, 0.5, 0.0, 0.0]), ident),
        # 17: the nodes are on another layer than the block.
        (1.0, bytes(layered), block([0.75, 0.9, 5.0, 0.0], [0.75, 0.9, -5.0, 0.0], 1.0, 0.0, 0, 0x0002), ident),
        # 18: along -x, the axis test's other side.
        (1.0, world, block([5.0, 0.5, 0.0, 0.0], [-5.0, 0.5, 0.0, 0.0]), ident),
        # 19: no BVH at all - the early return leaves the output untouched.
        (1.0, world, down, ident, True),
        # 20: a diagonal sweep that crosses quad A; the second leaf's box clears
        # the axis-aligned hull but not the oriented rod test.
        (1.0, bytes(off_axis), block([-5.0, -5.0, 0.0, 0.0], [5.0, 5.0, 0.0, 0.0]), ident),
        # 21: no expansion at all, and quad B sits just inside the 0.02 step -
        # the node margin decides whether its leaf is reached.
        (1.0, shifted(0.01), down, ident),
        # 22: expansion 0.004, so the step is 0.024 and quad B at 0.0245 is out.
        (1.0, shifted(0.0245), block([0.0, 0.5, 5.0, 0.0], [0.0, 0.5, -5.0, 0.0], 1.0, 0.004), ident),
        # 23: two faces in one leaf, nearest first - the budget does not shrink
        # after an acceptance, so the farther face wins as the last accepted.
        (1.0, bytes(two_deep), block([0.5, 0.5, 4.0, 0.0], [0.5, 0.5, -4.0, 0.0]), ident),
        # 24: no expansion and quad B 0.019 away: only the 0.02 step reaches its
        # leaf, so the leaf count separates `0 < exp` from `0 <= exp` and the
        # 0.02 addend from a zero one.
        (1.0, shifted(0.019), block([0.0, 0.5, 5.0, 0.0], [0.0, 0.5, -5.0, 0.0]), ident),
        # 25: expansion 0.004 with quad B 0.019 away - the 0.02 addend, not the
        # expansion alone, is what reaches the far leaf (0.024 > 0.019 > 0.004).
        (1.0, shifted(0.019), block([0.0, 0.5, 5.0, 0.0], [0.0, 0.5, -5.0, 0.0], 1.0, 0.004), ident),
    ]


CASES = _cases()


def make_args(i: int) -> List[ArgSpec]:
    case = CASES[i % len(CASES)]
    scale, world, query, shape = case[:4]
    no_root = bool(case[4]) if len(case) > 4 else False
    inverse = 1.0 / scale
    record = bytearray(pool(scale, inverse, world))
    if no_root:
        struct.pack_into("<Q", record, 0x20, 0)
    return [
        ArgSpec(
            kind="ptr",
            mem=bytes(record) + world,
            arena_offset=POOL_OFF,
            relocs=(0x28, 0x30) if no_root else (0x20, 0x28, 0x30),
        ),
        ArgSpec(kind="ptr", mem=bytes(0x44), arena_offset=OUT_OFF),
        ArgSpec(kind="ptr", mem=query, arena_offset=BLOCK_OFF),
        ArgSpec(kind="ptr", mem=shape, arena_offset=XFORM_OFF),
    ]

"""Inputs for the `mesh_gather` unit (`0x142caea70`).

The gather takes four things: the object record (`{scale, 1/scale, root,
vertices, triangles, ..., +0x44 stack guard}`), the caller's id container, the
eight-float query box and the layer mask. The pointee carries the record
followed by its whole mesh world, and the three baked pointers are blob offsets
that the oracle rebases into the arena; the container is its own pointee with
the client's header at `+0x200`/`+0x208`/`+0x20c` and the inline id array at
`+0x210`.

The tree is a *fixture* tree, built from the triangles it covers: the layout the
client walks is parent, adjacent subtree (`node + 0x20`), far subtree (at
`node + i32[+0x18] * 0x20`), so the ids come out adjacent-subtree-first and, in
a leaf, in triangle-index order.

The declared specials come first, so the mutation corpus covers them: the guard
word at both sides of its signed compare, the null root, each mask, the scaled
box, the sign-bit node test's NaN lane, a deep right-skewed tree (the walk's
order), a leaf boundary inside and outside the `0.005` tolerance, and a
degenerate triangle.
"""

from __future__ import annotations

import struct
from typing import List, Sequence, Tuple

from emu_oracle import ARENA_BASE, ArgSpec
from mesh_fixture import MASK_ALL, node, triangle, vertices

OBJECT_OFF = 0x4000
RECORD_LEN = 0x50
WORLD_AT = 0x000
VERTICES_AT = 0x200
TRIANGLES_AT = 0x400
WORLD_LEN = 0xA00

CONTAINER_OFF = 0x5000
CONTAINER_LEN = 0x410
CONTAINER_IDS_AT = 0x210
CONTAINER_CAP = 0x80

QUERY_OFF = 0x6000


def _quad(values) -> bytes:
    lanes = list(values) + [0.0] * (4 - len(values))
    return b"".join(struct.pack("<f", v) for v in lanes[:4])


def _box_of(points: Sequence[Sequence[float]]):
    lo = [min(float(p[k]) for p in points) for k in range(3)]
    hi = [max(float(p[k]) for p in points) for k in range(3)]
    return lo, hi


# A tree is ("leaf", [triangle indices]) or ("inner", adjacent, far): the
# adjacent subtree is walked first, so it is written first in the case list.
def _emit(tree, base: int, nodes: List[bytes], verts, tris, pad: float) -> int:
    """Write `tree` at node `base`; return the number of nodes it used."""
    if tree[0] == "leaf":
        covered = [i for t in tree[1] for i in tris[t]]
        lo, hi = _box_of([verts[i] for i in covered])
        nodes[base] = node(
            lo, hi, tree[1][0], MASK_ALL, 0, len(tree[1])
        )
        return 1
    _, adjacent, far = tree
    n_adj = _emit(adjacent, base + 1, nodes, verts, tris, pad)
    _emit(far, base + 1 + n_adj, nodes, verts, tris, pad)
    covered = _covered_triangles(tree)
    lo, hi = _box_of([verts[i] for t in covered for i in tris[t]])
    lo = [v - pad for v in lo]
    hi = [v + pad for v in hi]
    nodes[base] = node(lo, hi, 1 + n_adj, MASK_ALL, 0, 0)
    return 1 + n_adj + _emit_count(far)


def _emit_count(tree) -> int:
    if tree[0] == "leaf":
        return 1
    return 1 + _emit_count(tree[1]) + _emit_count(tree[2])


def _covered_triangles(tree) -> List[int]:
    if tree[0] == "leaf":
        return list(tree[1])
    return _covered_triangles(tree[1]) + _covered_triangles(tree[2])


def world_bytes(verts, tris, tree, tri_masks=None, node_masks=None) -> bytes:
    """Serialize `tree` over `verts`/`tris` into the gather's world blob."""
    nodes: List[bytes] = [b""] * 64
    count = _emit(tree, 0, nodes, verts, tris, 0.0)
    masks = tri_masks or (MASK_ALL,) * len(tris)
    records = bytearray()
    for slot, corners in enumerate(tris):
        records += triangle(tuple(corners), (slot, 0), masks[slot])
    if node_masks is not None:
        for slot in range(count):
            rec = bytearray(nodes[slot])
            struct.pack_into("<H", rec, 0x1C, node_masks[slot])
            nodes[slot] = bytes(rec)
    out = bytearray(WORLD_LEN)
    for slot in range(count):
        out[WORLD_AT + slot * 0x20 : WORLD_AT + (slot + 1) * 0x20] = nodes[slot]
    blob = vertices(verts)
    out[VERTICES_AT : VERTICES_AT + len(blob)] = blob
    out[TRIANGLES_AT : TRIANGLES_AT + len(records)] = records
    return bytes(out)


def record_world(world: bytes, inverse: float = 1.0, guard: int = 0x7F) -> bytes:
    rec = bytearray(RECORD_LEN)
    rec[0x00:0x10] = _quad([1.0] * 4)
    rec[0x10:0x20] = _quad([inverse] * 4)
    # Relative to the pointee's own base; `relocs` rebase them on the oracle
    # side and the probe reads them as offsets into the same blob.
    struct.pack_into("<Q", rec, 0x20, RECORD_LEN + WORLD_AT)
    struct.pack_into("<Q", rec, 0x28, RECORD_LEN + VERTICES_AT)
    struct.pack_into("<Q", rec, 0x30, RECORD_LEN + TRIANGLES_AT)
    struct.pack_into("<i", rec, 0x44, guard)
    return bytes(rec) + world


def container() -> bytes:
    blob = bytearray(b"\xcc" * CONTAINER_LEN)
    blob[0x200:0x208] = struct.pack("<Q", ARENA_BASE + CONTAINER_OFF + CONTAINER_IDS_AT)
    blob[0x208:0x20C] = struct.pack("<i", CONTAINER_CAP)
    blob[0x20C:0x210] = struct.pack("<i", 0)
    return bytes(blob)


# The standard mesh: three unit quads in a row, six triangles, twelve vertices.
def _quads(row: Sequence[Tuple[float, float]]) -> Tuple[List, List]:
    verts: List[Tuple[float, ...]] = []
    tris: List[Tuple[int, int, int]] = []
    for x, y in row:
        base = len(verts)
        verts += [
            (x, y, 0.0),
            (x + 1.0, y, 0.0),
            (x + 1.0, y + 1.0, 0.0),
            (x, y + 1.0, 0.0),
        ]
        tris += [(base, base + 1, base + 2), (base, base + 2, base + 3)]
    return verts, tris


STANDARD = _quads([(0.0, 0.0), (3.0, 0.0), (6.0, 0.0)])

# Root: adjacent leaf (triangles 0,1), far leaf (triangles 2..5).
TWO_LEAF = ("inner", ("leaf", [0, 1]), ("leaf", [2, 3, 4, 5]))

# A three-level tree: adjacent leaf, then an inner node whose adjacent leaf is
# triangles 2,3 and whose far leaf is 4,5. The far child of the root is the
# inner node, so the root's `+0x18` is 3 while the inner node's is 1.
THREE_LEVEL = (
    "inner",
    ("leaf", [0, 1]),
    ("inner", ("leaf", [2, 3]), ("leaf", [4, 5])),
)

# A right-skewed tree eight leaves deep: every far child is the next inner node
# and every adjacent child is one triangle, so the walk's stack grows one level
# per push and the ids come out leaf by leaf.
def _deep(leaves: int):
    tree = ("leaf", [leaves - 1])
    for i in range(leaves - 2, -1, -1):
        tree = ("inner", ("leaf", [i]), tree)
    return tree


def box(lo, hi) -> bytes:
    return _quad(list(lo)[:3] + [0.0]) + _quad(list(hi)[:3] + [0.0])


# The box-triangle unit's tolerance-band triangle: against the unit box its
# deciding edge lane measures 0.0045, inside the 0.005 tolerance, so the accept
# takes it. Translating it 0.008 along -x puts the same lane over the tolerance.
BAND_TRIANGLE = [
    (-1.0387098789215088, -0.5, 1.0),
    (0.961290180683136, 0.25, 1.25),
    (-1.2887098789215088, -1.75, 1.75),
]


def _band_case(dx: float):
    verts = [(x + dx, y, z) for x, y, z in BAND_TRIANGLE]
    world = world_bytes(verts, [(0, 1, 2)], ("leaf", [0]))
    return record_world(world), box([-1.0, -1.0, -1.0], [1.0, 1.0, 1.0])


def _cases():
    world = world_bytes(*STANDARD, TWO_LEAF)
    deep = world_bytes(*STANDARD, _deep(6))
    three = world_bytes(*STANDARD, THREE_LEVEL)

    near = box([0.25, 0.25, -0.5], [0.75, 0.75, 0.5])
    far_leaf = box([6.25, 0.25, -0.5], [6.75, 0.75, 0.5])
    everything = box([-1.0, -1.0, -1.0], [8.0, 2.0, 1.0])

    # A single leaf triangle, and the same triangle with the box just off it.
    single_verts = [(0.0, 0.0, 0.0), (1.0, 0.0, 0.0), (0.0, 1.0, 0.0)]
    single = world_bytes(single_verts, [(0, 1, 2)], ("leaf", [0]))

    cases = [
        # 0: the adjacent leaf only - the far leaf's box misses.
        (record_world(world), near, MASK_ALL),
        # 1: the far leaf only - the root pushes it first, the adjacent leaf
        # pops and misses.
        (record_world(world), far_leaf, MASK_ALL),
        # 2: both leaves; the ids come out adjacent-first, then index order.
        (record_world(world), everything, MASK_ALL),
        # 3: a clean miss.
        (record_world(world), box([20.0, 20.0, 20.0], [21.0, 21.0, 21.0]), MASK_ALL),
        # 4: the query mask misses every node mask.
        (record_world(world), everything, 0x0002),
        # 5: the triangles are on another layer - the nodes pass, the records
        # do not.
        (record_world(world_bytes(*STANDARD, TWO_LEAF, tri_masks=(0x0002, 0x0002, 0x0002, 0x0002, 0x0002, 0x0002))), everything, MASK_ALL),
        # 6: the leaf triangles split across two layers - only the first leaf's.
        (record_world(world_bytes(*STANDARD, TWO_LEAF, tri_masks=(0x0001, 0x0002, 0x0002, 0x0002, 0x0002, 0x0002))), everything, MASK_ALL),
        # 7: node masks - the root is 0x0001 and its children 0x0002.
        (record_world(world_bytes(*STANDARD, TWO_LEAF, node_masks=(0x0001, 0x0002, 0x0002))), everything, MASK_ALL),
        # 8: the guard word is exactly 0x80 - the walk returns before the root.
        (record_world(world, guard=0x80), everything, MASK_ALL),
        # 9: one below the guard.
        (record_world(world, guard=0x7F), everything, MASK_ALL),
        # 10: a negative guard word - the compare is signed, so the walk runs.
        (record_world(world, guard=-1), everything, MASK_ALL),
        # 11: no root - the early return.
        (record_world(world)[:0x20] + bytes(8) + record_world(world)[0x28:], everything, MASK_ALL),
        # 12: an identity multiplier, so the query box (0.55..0.6) lands inside
        # quad A.
        (record_world(world, inverse=1.0), box([0.55, 0.4, -0.5], [0.6, 0.6, 0.5]), MASK_ALL),
        # 13: the same query box with the record's `+0x10` quad at two, which
        # puts it at 1.1..1.2 - past quad A, so the gather keeps nothing.
        (record_world(world, inverse=2.0), box([0.55, 0.4, -0.5], [0.6, 0.6, 0.5]), MASK_ALL),
        # 14: `1/scale` negative flips both corners, so the local box is empty
        # in a different way: the sign test still decides.
        (record_world(world, inverse=-1.0), everything, MASK_ALL),
        # 15: a NaN in the low corner - the sign-bit test, not an ordered one.
        (record_world(world), box([float("nan"), 0.25, -0.5], [0.75, 0.75, 0.5]), MASK_ALL),
        # 16: a positive infinity in the high corner.
        (record_world(world), box([0.25, 0.25, -0.5], [float("inf"), 0.75, 0.5]), MASK_ALL),
        # 17: a three-level tree over everything: adjacent leaf first, then the
        # inner node's adjacent leaf, then its far leaf.
        (record_world(three), everything, MASK_ALL),
        # 18: a six-leaf right-skewed tree.
        (record_world(deep), everything, MASK_ALL),
        # 19: the same deep tree with only its last leaf under the box.
        (record_world(deep), far_leaf, MASK_ALL),
        # 20: one leaf triangle, box over it.
        (record_world(single), box([-0.25, -0.25, -0.5], [0.25, 0.25, 0.5]), MASK_ALL),
        # 21: the node box and the slabs pass and the triangle's deciding edge
        # lane is 0.0045 - inside the accept's tolerance, so it is collected.
        (*_band_case(0.0), MASK_ALL),
        # 22: the same triangle 0.008 further along -x, lane over 0.005: the
        # gather keeps nothing even though the node and slab tests still pass.
        (*_band_case(-0.008), MASK_ALL),
        # 23: a degenerate triangle inside the box: all three vertices equal.
        (
            record_world(
                world_bytes(
                    [(0.5, 0.5, 0.0)] * 3, [(0, 1, 2)], ("leaf", [0])
                )
            ),
            box([0.25, 0.25, -0.5], [0.75, 0.75, 0.5]),
            MASK_ALL,
        ),
        # 24: a leaf of six triangles whose boxes all overlap - the id order is
        # the index order and every mask is read.
        (
            record_world(world_bytes(*STANDARD, ("leaf", [0, 1, 2, 3, 4, 5]))),
            everything,
            MASK_ALL,
        ),
        # 25: the same leaf with every other triangle masked out.
        (
            record_world(
                world_bytes(
                    *STANDARD,
                    ("leaf", [0, 1, 2, 3, 4, 5]),
                    tri_masks=(0x0001, 0x0002, 0x0001, 0x0002, 0x0001, 0x0002),
                )
            ),
            everything,
            MASK_ALL,
        ),
    ]
    return cases


CASES = _cases()

# A deterministic sweep of random small meshes, trees and boxes. Half the
# boxes are built around a vertex so the walk actually collects; the rest vary
# the guard, the multiplier, the masks and the tree shape.
def _random_cases(count: int = 240):
    import random

    rng = random.Random(0x142C_AE_A70)
    out = []
    for _ in range(count):
        quads = rng.choice((1, 2, 2, 3))
        row = []
        x = 0.0
        for _ in range(quads):
            row.append((x, rng.uniform(-1.0, 1.0)))
            x += rng.uniform(1.0, 3.0)
        verts, tris = _quads(row)
        leaves = []
        slot = 0
        while slot < len(tris):
            take = rng.choice((1, 1, 2, 2, 3))
            leaves.append(("leaf", list(range(slot, min(slot + take, len(tris))))))
            slot += take
        tree = leaves[-1]
        for leaf in reversed(leaves[:-1]):
            tree = ("inner", leaf, tree)

        node_masks = None
        if rng.random() < 0.2:
            node_masks = tuple(rng.choice((0x0001, 0x0003, 0xFFFF)) for _ in range(16))
        tri_masks = None
        if rng.random() < 0.2:
            tri_masks = tuple(rng.choice((0x0001, 0x0002, 0xFFFF)) for _ in tris)

        if rng.random() < 0.75:
            anchor = rng.choice(verts)
            size = [rng.uniform(0.02, 0.5) for _ in range(3)]
            lo = [anchor[k] - size[k] * rng.random() for k in range(3)]
            hi = [lo[k] + size[k] for k in range(3)]
        else:
            lo = [rng.uniform(-2.0, 7.0) for _ in range(3)]
            hi = [lo[k] + rng.uniform(0.05, 3.0) for k in range(3)]

        inverse = rng.choice((1.0, 1.0, 1.0, 2.0, 0.5))
        guard = rng.choice((0x7F, 0, 3, -1))
        if rng.random() < 0.05:
            guard = 0x80
        out.append(
            (
                record_world(
                    world_bytes(verts, tris, tree, tri_masks=tri_masks, node_masks=node_masks),
                    inverse=inverse,
                    guard=guard,
                ),
                box(lo, hi),
                rng.choice((0x0001, 0x0001, 0x0003, 0xFFFF)),
            )
        )
    return out


for _case in _random_cases():
    CASES.append(_case)


def make_args(i: int) -> List[ArgSpec]:
    record, query, mask = CASES[i % len(CASES)]
    # A reloc slot is rebased unconditionally, so a null root must NOT be one:
    # rebasing zero would turn it into the pointee's own base address.
    root = struct.unpack_from("<Q", record, 0x20)[0]
    relocs = (0x28, 0x30) if root == 0 else (0x20, 0x28, 0x30)
    return [
        ArgSpec(
            kind="ptr",
            mem=record,
            arena_offset=OBJECT_OFF,
            relocs=relocs,
        ),
        ArgSpec(kind="ptr", mem=container(), arena_offset=CONTAINER_OFF),
        ArgSpec(kind="ptr", mem=query, arena_offset=QUERY_OFF),
        ArgSpec(kind="u64", bits=mask),
    ]

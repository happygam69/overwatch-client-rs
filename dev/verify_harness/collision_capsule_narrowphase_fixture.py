"""Inputs for the `collision_capsule_narrowphase` unit (`0x142cfc090`).

One case is one blob: the world record, the shape entries, the owner and pose
pools, the shape pools, the mesh pool and the runtime pool behind it. Every
pointer in it is an absolute arena address, so the oracle reads the world out
of the arena and the probe reads the same bytes out of the blob.

The narrowphase needs two worlds to be exercised: the **type-3** path, where
the shape's triangles come from the mesh's own records, and the **runtime**
path, where they come from a height field. The two cases differ in the shape
record the client is given, and in the shape's type byte.

The context - the candidate list the gather fills - is a second argument, and
its pool is a third: the client's pool is a heap block the narrowphase owns,
and the fixture puts it in the arena so both sides read the same bytes.

The three frames the narrowphase is given are the shape's own frame, the
**other** shape's frame and the sweep's direction, and the fixture keeps them
apart: the shape's frame carries the rotation the triangles are put through,
the other shape's frame carries the capsule's place in the world, and the
direction is only ever a sweep.
"""

from __future__ import annotations

import math
import random
import struct
from typing import List

from emu_oracle import ARENA_BASE, ArgSpec
from mesh_fixture import TRIANGLES_AT, VERTICES_AT, WORLD_AT, standard_world

BLOB_OFF = 0
CONTEXT_OFF = 0x4000
OUT_OFF = 0x4200
SHAPE_OFF = 0x4300
TRANSFORM_OFF = 0x4400
OTHER_OFF = 0x4500
DIRECTION_OFF = 0x4600
# The candidate pool: the client's own heap block. It sits on its own, clear
# of every other argument, because the gather writes a whole record per
# candidate and a pool that overlapped an argument would change the answer.
POOL_OFF = 0x5000

WORLD_LEN = 0x340
RECORD_AT = 0x340
RECORD_LEN = 0x50
ENTRIES_AT = 0x400
ENTRY_STRIDE = 0xC0
ENTRY_COUNT = 3
OWNERS_AT = 0x700
OWNER_STRIDE = 0x50
POSES_AT = 0x800
POSE_STRIDE = 0x1C0
SPHERES_AT = 0xC00
CAPSULES_AT = 0xC80
COUNTED_AT = 0xCD0
MESH_AT = 0xD80
RUNTIME_AT = 0xE80
MESH_WORLD_AT = 0x1000
RUNTIME_VERTICES_AT = 0x1200
BLOB_LEN = 0x2000

CONTEXT_LEN = 0x40
POOL_LEN = 0x20 * 64
OUT_LEN = 0x40
SHAPE_LEN = 0x28
FRAME_LEN = 0x20
DIRECTION_LEN = 0x10

MASK_ALL = 0x0001

# The entries: 0 the mesh shape, 1 the capsule, 2 the runtime shape.
MESH_ENTRY = 0
CAPSULE_ENTRY = 1
RUNTIME_ENTRY = 2

# The runtime's height field: four cells across, four down, a cell size of one.
RUNTIME_ACROSS = 4
RUNTIME_DOWN = 4
RUNTIME_CELL = 1.0
RUNTIME_VERTEX_COUNT = 32

# The capsule's own element: the segment it stands on and its radius.
CAPSULE_LOW = (0.0, 0.0, -0.9)
CAPSULE_HIGH = (0.0, 0.0, 0.9)
CAPSULE_RADIUS = 0.5

# The capsule's local box, and the margin the gather widens the query by.
CAPSULE_BOX = ((-0.5, -0.5, -1.4), (0.5, 0.5, 1.4))
CAPSULE_MARGIN = 6.0

# The context's own box when a case does not name one.
ZERO_BOX = ((0.0, 0.0, 0.0), (0.0, 0.0, 0.0))


def _fbits(value: float) -> int:
    return struct.unpack("<I", struct.pack("<f", value))[0]


def _quad_bytes(values) -> bytes:
    return b"".join(struct.pack("<I", _fbits(v)) for v in values)


def _quad(values) -> bytes:
    return _quad_bytes(list(values) + [0.0] * (4 - len(values)))


def _quaternion(axis, angle: float):
    """A unit quaternion from an axis and an angle, in the client's order."""
    length = math.sqrt(sum(v * v for v in axis)) or 1.0
    half = angle * 0.5
    scale = math.sin(half) / length
    return (axis[0] * scale, axis[1] * scale, axis[2] * scale, math.cos(half))


IDENTITY = (0.0, 0.0, 0.0, 1.0)


def _entry(kind: int, index: int, box, margin: float = 0.0, mask: int = MASK_ALL) -> bytes:
    """One shape entry: the type byte, the index, the local box and the rest."""
    entry = bytearray(ENTRY_STRIDE)
    entry[0x20] = kind
    struct.pack_into("<i", entry, 0x24, index)
    entry[0x00:0x10] = _quad(box[0])
    entry[0x10:0x20] = _quad(box[1])
    struct.pack_into("<H", entry, 0x98, mask)
    struct.pack_into("<I", entry, 0x88, _fbits(margin))
    # The world frame comes from the owner pool, entry 0.
    struct.pack_into("<i", entry, 0x50, 0)
    # The owner-attached state the transform reads.
    entry[0xB0] = 0
    return bytes(entry)


def _owner(position, quaternion) -> bytes:
    """One owner-pool entry: the quaternion quad and the position quad."""
    owner = bytearray(OWNER_STRIDE)
    owner[0x00:0x10] = _quad(quaternion)
    owner[0x10:0x20] = _quad(position)
    return bytes(owner)


def _mesh_record(scale: float = 1.0) -> bytes:
    """The type-3 mesh's own record: the scale, the BVH, the vertices."""
    record = bytearray(RECORD_LEN)
    record[0x00:0x10] = _quad([scale] * 4)
    record[0x10:0x20] = _quad([1.0 / scale] * 4)
    struct.pack_into("<Q", record, 0x20, ARENA_BASE + BLOB_OFF + MESH_WORLD_AT + WORLD_AT)
    struct.pack_into("<Q", record, 0x28, ARENA_BASE + BLOB_OFF + MESH_WORLD_AT + VERTICES_AT)
    struct.pack_into("<Q", record, 0x30, ARENA_BASE + BLOB_OFF + MESH_WORLD_AT + TRIANGLES_AT)
    return bytes(record)


def _runtime_record() -> bytes:
    """The runtime's own record: the grid and the vertex array."""
    record = bytearray(RECORD_LEN)
    struct.pack_into("<i", record, 0x00, RUNTIME_ACROSS)
    struct.pack_into("<i", record, 0x04, RUNTIME_DOWN)
    struct.pack_into("<I", record, 0x08, _fbits(RUNTIME_CELL))
    struct.pack_into("<Q", record, 0x38, ARENA_BASE + BLOB_OFF + RUNTIME_VERTICES_AT)
    return bytes(record)


def _runtime_vertices(height: float) -> bytes:
    """The height field's vertices: a height and a kind byte each.

    The heights are flat: the client's runtime path reads a cell's box from
    the corner heights, and a degenerate cell makes its closest-point solver
    answer with a NaN.
    """
    out = bytearray()
    for index in range(RUNTIME_VERTEX_COUNT):
        out += struct.pack("<I", _fbits(height))
        out += struct.pack("<H", 0x1000 + index)
        out += bytes([0x5A, 0x00])
    return bytes(out)


def _blob_bytes(runtime: bool, height: float, box, margin: float) -> bytes:
    blob = bytearray(BLOB_LEN)
    # The world record: the entries, the owners, the poses and the pools.
    struct.pack_into("<Q", blob, 0x2B8, ARENA_BASE + BLOB_OFF + ENTRIES_AT)
    struct.pack_into("<Q", blob, 0x2A0, ARENA_BASE + BLOB_OFF + OWNERS_AT)
    struct.pack_into("<Q", blob, 0x288, ARENA_BASE + BLOB_OFF + POSES_AT)
    struct.pack_into("<Q", blob, 0x2D0, ARENA_BASE + BLOB_OFF + SPHERES_AT)
    struct.pack_into("<Q", blob, 0x2E8, ARENA_BASE + BLOB_OFF + CAPSULES_AT)
    struct.pack_into("<Q", blob, 0x300, ARENA_BASE + BLOB_OFF + COUNTED_AT)
    struct.pack_into("<Q", blob, 0x318, ARENA_BASE + BLOB_OFF + MESH_AT)
    struct.pack_into("<Q", blob, 0x330, ARENA_BASE + BLOB_OFF + RUNTIME_AT)
    # The shape entries. The mesh shape carries the type-3 triangles; the
    # runtime shape carries the height field's. The capsule's box is the box
    # the gather sweeps, and it starts at zero: the runtime's own grid starts
    # at zero too, and the client's index arithmetic would run off the grid's
    # front for a box that starts below it.
    at = ENTRIES_AT + MESH_ENTRY * ENTRY_STRIDE
    blob[at : at + ENTRY_STRIDE] = _entry(
        3 if not runtime else 2, 0, ((0.0, 0.0, 0.0), (7.0, 7.0, 7.0)), 0.0
    )
    at = ENTRIES_AT + CAPSULE_ENTRY * ENTRY_STRIDE
    blob[at : at + ENTRY_STRIDE] = _entry(1, 0, box, margin)
    at = ENTRIES_AT + RUNTIME_ENTRY * ENTRY_STRIDE
    blob[at : at + ENTRY_STRIDE] = _entry(
        2, 0, ((0.0, 0.0, 0.0), (7.0, 7.0, 7.0)), 0.0
    )
    # The owner pool: the mesh shape's world frame, the identity.
    blob[OWNERS_AT : OWNERS_AT + OWNER_STRIDE] = _owner((0.0, 0.0, 0.0), IDENTITY)
    # The shape pools: the capsule the character sweeps with.
    capsule = bytearray(0x30)
    capsule[0x00:0x10] = _quad(CAPSULE_LOW)
    capsule[0x10:0x20] = _quad(CAPSULE_HIGH)
    struct.pack_into("<I", capsule, 0x20, _fbits(CAPSULE_RADIUS))
    blob[CAPSULES_AT : CAPSULES_AT + 0x30] = capsule
    # The mesh pool and the mesh world.
    blob[MESH_AT : MESH_AT + RECORD_LEN] = _mesh_record()
    world = standard_world()
    blob[MESH_WORLD_AT : MESH_WORLD_AT + len(world)] = world
    # The runtime pool and its height field.
    blob[RUNTIME_AT : RUNTIME_AT + RECORD_LEN] = _runtime_record()
    blob[RUNTIME_VERTICES_AT : RUNTIME_VERTICES_AT + RUNTIME_VERTEX_COUNT * 8] = (
        _runtime_vertices(height)
    )
    return bytes(blob)


def _context_bytes(count: int, old_box) -> bytes:
    """The context: the swept box, the count, the capacity and the pool."""
    context = bytearray(CONTEXT_LEN)
    struct.pack_into("<Q", context, 0x38, ARENA_BASE + POOL_OFF)
    # The capacity is the candidate pool's, and the client only grows it when
    # the count outgrows it - so the allocator is never reached.
    struct.pack_into("<i", context, 0x34, 64)
    # The count and the box the gather found last time. The client keeps the
    # old candidates when the new box still sits inside the old one, so a
    # negative count survives into the narrowphase's own loop guard.
    struct.pack_into("<i", context, 0x30, count)
    context[0x10:0x20] = _quad(old_box[0])
    context[0x20:0x30] = _quad(old_box[1])
    return bytes(context)


def _shape_bytes(runtime: bool) -> bytes:
    """The shape record: the mesh shape's index and the other shape's index."""
    shape = bytearray(SHAPE_LEN)
    struct.pack_into("<i", shape, 0x14, RUNTIME_ENTRY if runtime else MESH_ENTRY)
    struct.pack_into("<i", shape, 0x20, CAPSULE_ENTRY)
    return bytes(shape)


def _frame_bytes(position, quaternion) -> bytes:
    return _quad(position) + _quad(quaternion)


def _cases():
    """The declared cases.

    The capsule stands over the first quad of the mesh world and over the
    first cell of the height field, near enough for the closest-point solver
    to answer. The frames carry real rotations, so the shape's own frame and
    the other shape's frame are told apart, and the sweep's direction is
    never the same as either position.
    """
    cases = [
        # The mesh path, the capsule above the first quad and the sweep
        # pointing down at it.
        {
            "runtime": False,
            "height": 0.5,
            "transform": ((0.0, 0.0, 0.0), IDENTITY),
            "other": ((0.2, 0.2, 1.2), IDENTITY),
            "direction": (0.0, 0.0, -1.0),
        },
        # The mesh path with the shape's own frame turned about z.
        {
            "runtime": False,
            "height": 0.5,
            "transform": ((0.1, -0.2, 0.3), _quaternion((0.0, 0.0, 1.0), 0.7)),
            "other": ((0.2, 0.2, 1.2), IDENTITY),
            "direction": (0.0, 0.0, -1.0),
        },
        # The mesh path with the other shape's frame turned about x.
        {
            "runtime": False,
            "height": 0.5,
            "transform": ((0.0, 0.0, 0.0), IDENTITY),
            "other": ((0.25, 0.15, 1.15), _quaternion((1.0, 0.0, 0.0), -0.4)),
            "direction": (0.05, -0.05, -0.9),
        },
        # The mesh path with both frames turned.
        {
            "runtime": False,
            "height": 0.5,
            "transform": ((-0.3, 0.15, 0.05), _quaternion((0.3, 0.6, 0.2), 1.1)),
            "other": ((0.22, 0.18, 1.18), _quaternion((0.5, -0.4, 0.7), 0.55)),
            "direction": (0.12, 0.03, -0.9),
        },
        # The mesh path with the capsule clear of the quad: no contact.
        {
            "runtime": False,
            "height": 0.5,
            "transform": ((0.0, 0.0, 0.0), IDENTITY),
            "other": ((3.0, 3.0, 3.0), IDENTITY),
            "direction": (0.0, 0.0, -1.0),
        },
        # The runtime path, the capsule under the first cell of the field.
        {
            "runtime": True,
            "height": 0.5,
            "transform": ((0.0, 0.0, 0.0), IDENTITY),
            "other": ((0.2, 0.2, 1.2), IDENTITY),
            "direction": (1.0, 0.0, 0.0),
        },
        # The runtime path with the shape's own frame turned about z.
        {
            "runtime": True,
            "height": 0.5,
            "transform": ((0.0, 0.0, 0.0), _quaternion((0.0, 0.0, 1.0), 0.9)),
            "other": ((0.2, 0.2, 1.2), IDENTITY),
            "direction": (1.0, 0.0, 0.0),
        },
        # The runtime path with the other shape's frame turned about y.
        {
            "runtime": True,
            "height": 0.5,
            "transform": ((0.0, 0.0, 0.0), IDENTITY),
            "other": ((0.25, 0.2, 1.05), _quaternion((0.0, 1.0, 0.0), 0.35)),
            "direction": (0.9, 0.0, 0.0),
        },
        # The runtime path with both frames turned.
        {
            "runtime": True,
            "height": 0.5,
            "transform": ((0.15, 0.15, 0.0), _quaternion((0.4, 0.2, 0.9), 0.8)),
            "other": ((0.18, 0.24, 1.02), _quaternion((0.2, 0.8, 0.5), 0.45)),
            "direction": (0.9, 0.1, 0.0),
        },
        # The runtime path with the capsule clear of the field: no contact.
        {
            "runtime": True,
            "height": 0.5,
            "transform": ((0.0, 0.0, 0.0), IDENTITY),
            "other": ((3.0, 3.0, 3.0), IDENTITY),
            "direction": (1.0, 0.0, 0.0),
        },
        # The capsule's box clear of every triangle: the gather finds nothing
        # and the narrowphase answers before its loop.
        {
            "runtime": False,
            "height": 0.5,
            "transform": ((0.0, 0.0, 0.0), IDENTITY),
            "other": ((0.2, 0.2, 1.2), IDENTITY),
            "direction": (0.0, 0.0, -1.0),
            "box": ((8.0, 8.0, 8.0), (9.0, 9.0, 9.0)),
            "margin": 0.0,
        },
        # The same on the runtime path.
        {
            "runtime": True,
            "height": 0.5,
            "transform": ((0.0, 0.0, 0.0), IDENTITY),
            "other": ((0.2, 0.2, 1.2), IDENTITY),
            "direction": (1.0, 0.0, 0.0),
            "box": ((8.0, 8.0, 8.0), (9.0, 9.0, 9.0)),
            "margin": 0.0,
        },
        # The old box still holds the new one, so the gather keeps the old
        # candidates - and the count it keeps is negative, which sends the
        # narrowphase's own loop guard the other way.
        {
            "runtime": False,
            "height": 0.5,
            "transform": ((0.0, 0.0, 0.0), IDENTITY),
            "other": ((0.2, 0.2, 1.2), IDENTITY),
            "direction": (0.0, 0.0, -1.0),
            "count": -1,
            "old_box": ((-10.0, -10.0, -10.0), (10.0, 10.0, 10.0)),
        },
        # The same on the runtime path.
        {
            "runtime": True,
            "height": 0.5,
            "transform": ((0.0, 0.0, 0.0), IDENTITY),
            "other": ((0.2, 0.2, 1.2), IDENTITY),
            "direction": (1.0, 0.0, 0.0),
            "count": -1,
            "old_box": ((-10.0, -10.0, -10.0), (10.0, 10.0, 10.0)),
        },
        # The capsule's end point sits exactly on the quad's corner: the
        # solver's answer degenerates and the accept turns it down.
        {
            "runtime": False,
            "height": 0.5,
            "transform": ((0.0, 0.0, 0.0), IDENTITY),
            "other": ((0.0, 0.0, 0.9), IDENTITY),
            "direction": (0.0, 0.0, -1.0),
        },
        # The capsule's own frame carries a `NaN`, so the solver answers with
        # a zero residual: the first candidate sets the running minimum at
        # zero and the next one is not below it, which is the accept rule's
        # own other edge.
        {
            "runtime": False,
            "height": 0.5,
            "transform": ((0.0, 0.0, 0.0), IDENTITY),
            "other": ((float("nan"), 0.2, 1.2), IDENTITY),
            "direction": (0.0, 0.0, -1.0),
        },
    ]
    # A handful of random frames, so the solver sees a spread of rotations.
    rng = random.Random(23680)
    for _ in range(10):
        axis = (rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0))
        runtime = rng.choice((False, True))
        cases.append(
            {
                "runtime": runtime,
                "height": 0.5,
                "transform": (
                    (rng.uniform(-0.3, 0.3), rng.uniform(-0.3, 0.3), rng.uniform(-0.3, 0.3)),
                    _quaternion(axis, rng.uniform(-1.5, 1.5)),
                ),
                "other": (
                    (
                        rng.uniform(0.1, 0.35),
                        rng.uniform(0.1, 0.35),
                        rng.choice((1.2, 1.05)),
                    ),
                    _quaternion(axis, rng.uniform(-1.5, 1.5)),
                ),
                "direction": (
                    rng.uniform(-0.3, 0.3),
                    rng.uniform(-0.3, 0.3),
                    rng.uniform(-0.3, 0.3),
                ),
            }
        )
    return cases


CASES = _cases()


def make_args(i: int) -> List[ArgSpec]:
    case = CASES[i % len(CASES)]
    runtime = case["runtime"]
    return [
        ArgSpec(
            kind="ptr",
            mem=_context_bytes(case.get("count", 0), case.get("old_box", ZERO_BOX)),
            arena_offset=CONTEXT_OFF,
        ),
        ArgSpec(kind="ptr", mem=bytes(OUT_LEN), arena_offset=OUT_OFF),
        ArgSpec(
            kind="ptr",
            mem=_blob_bytes(
                runtime,
                case["height"],
                case.get("box", CAPSULE_BOX),
                case.get("margin", CAPSULE_MARGIN),
            ),
            arena_offset=BLOB_OFF,
        ),
        ArgSpec(kind="ptr", mem=_shape_bytes(runtime), arena_offset=SHAPE_OFF),
        ArgSpec(
            kind="ptr",
            mem=_frame_bytes(case["transform"][0], case["transform"][1]),
            arena_offset=TRANSFORM_OFF,
        ),
        ArgSpec(
            kind="ptr",
            mem=_frame_bytes(case["other"][0], case["other"][1]),
            arena_offset=OTHER_OFF,
        ),
        ArgSpec(kind="ptr", mem=_quad(case["direction"]), arena_offset=DIRECTION_OFF),
    ]

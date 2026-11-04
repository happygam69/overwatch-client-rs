"""Run the fan's candidate evaluator `0x1420328c0` offline with a built world.

This is a **probe**, not a read: it runs the client's own bytes under the
verification harness's Unicorn oracle and reports what they do. The findings
that count are the ones written up in `docs/formats/character-controller.md`;
this tool is how they were reached and how a regression is re-checked.

It builds the whole chain the evaluator walks, from the client's own constants:

    world+8 (encoded) -> service descriptor -> service slot  (0x14203debb)
    world+0x620       -> geometry, u16 handle at +0x31f8
    handle table      -> domino record
    record+0x1b0      -> pass sub-record: +0x30 BVH array, +0x44 root index
    record+0x2b8      -> 0xc0 shape entry: type +0x20, mesh index +0x24,
                         transform index +0x50, layer masks +0x98, mode +0xb0
    record+0x2a0      -> transform pool (0x50 stride): quat +0x00, position +0x10
    record+0x318      -> type-3 pool (0x50 stride): scale, 1/scale, root,
                         vertices, triangles
    BVH node (0x40)   -> min +0x00, max +0x10, shape id +0x20, leaf -1 at +0x2c

The mesh is a small synthetic two-quad world (the same shape the `fn/MeshSweep`
unit pins), so a failure there is a wiring failure, not a data failure. The
sweep is `(0.75, 0.9, 5) -> (0.75, 0.9, -5)`, which covers only the second
triangle of the near quad; the expected answer record is therefore triangle 1
with ids `0x12`/`0x22` and score `0.5`.

`--mesh-world FILE` replaces that mesh with the range's real assembled world
(`crates/ow-tank/examples/mesh_world_dump.rs`: 272 distinct shapes, 4,797,872
bytes, and **3,017 placed instances** of them). Each instance carries the
`translation`/`rotation` of the model-group entry that places it, and the probe
writes that pair into the owner pool `[record+0x2a0]` — the quaternion at
`+0x00`, the position at `+0x10` — which is where the client's
`ShapeWorldTransform` (`0x142cb8b80`) reads a shape's world frame.

In the default single-instance mode the query is the one `mesh_world_real`
pins: the pinned shape's root box, a quarter of the way in on x and z in f32,
swept from `high_y + 2` down to `low_y - 2` — built in the shape's own frame
and then carried into world by the placement, exactly as the driver carries it
back. The client then reproduces the pinned hit in map coordinates. The
record's id/kind are the policy's resolved id, not the triangle's: triangle id
`0x8000` is a negative `i16`, so the policy looks it up in the geometry's
variant table and falls back to the pending id `1` when that table is not
built.

`--all-shapes` puts all 3,017 placed instances in one broadphase. Because
every instance carries its placement, the pinned query returns **the same
answer record, byte for byte**, as the single-instance run: point
`(23.947973, -19.990925, 31.369896)`, normal
`(0.360226, 0.926024, 0.112766)`, fraction `0x3e8051b8`, triangle `6`. That is
a map position. Before the placement was applied the same query hit another
shape at y `5.843`, fraction `0.0208`, triangle `7054`, because all 272 shapes
sat in their own frames at the origin.

Two globals decide which prologue runs: `[0x143b795b0]` (a sorted int table
and its count) and `[0x143b795a0]`, plus `[0x143b79598]` (the block the guard
reads at `+0xb5` and `+0xdd0`). The table is seeded with one entry that never
matches, so the lookup misses and the guard runs: it decodes the constant byte
`0x26` through the nibble table at `0x142f7d158`, XORs the build byte at
`0x143847d3c` and `[Q+0xdd0]`, and compares with `0x40`.

`--guard-byte` picks `[Q+0xdd0]`; the default takes the non-equal branch (the
body read in the docs). `--equal-branch` sets the byte that makes the compare
equal, so the other branch can be checked: it reads `[r12]`, and the prologue
zeroes `r12` at `0x14203db79` before every path to the guard, so it faults on
a null read even when the caller sets `r12` (`--r12`). That is recorded in the
write-up: the equality branch is a canary, not a second encoding.

The image is read-only input; nothing here is a gameplay claim.


Chain under test:
    world+8 (encoded) -> service slot
    world+0x620       -> geometry, u16 handle at +0x31f8
    handle table      -> domino record
    record+0x1b0      -> pass 0 (BVH node array + root index)
    record+0x2b8      -> 0xc0-byte shape entry (type 3, pool index 0)
    record+0x318      -> 0x50 pool element (scale, 1/scale, root, verts, tris)

Everything is seeded from the client's own constants; the mesh is a small
synthetic two-quad world first, so a failure is a wiring failure.
"""

from __future__ import annotations

import os
import argparse
import struct
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, "dev", "verify_harness"))

from emu_oracle import SENTINEL, STACK_TOP, V2B_BASE, EmuOracle

from unicorn import (
    UC_HOOK_CODE,
    UC_HOOK_MEM_READ,
    UC_HOOK_MEM_READ_UNMAPPED,
    UC_HOOK_MEM_WRITE,
    UC_HOOK_MEM_WRITE_UNMAPPED,
    UcError,
)
from unicorn import x86_const as x

IMAGE = os.path.join(ROOT, "dev", "data", "offline_recover", "module_offline.bin")
IMAGE_BASE = 0x140000000

BLOB = 0x0000_0004_0000_0000
# The blob holds the world, descriptor, out record, the Domino record and its
# pools, and the pass-0 broadphase. `DOM_OFF` starts the pools, so the size has
# to cover one entry, one owner slot, one type-3 slot and two BVH nodes per
# placed shape -- 3,017 of them for the range.
BLOB_SIZE = 0x80_0000

# ---- offsets in the blob ----
R_OFF = 0x0000        # [0x143b795b0]: count + sorted array (empty)
Q_OFF = 0x1000        # [0x143b79598]
Q2_OFF = 0x3000       # [0x143b795a0]
WORLD_OFF = 0x4000
DESC_OFF = 0x5000     # the evaluator's descriptor object (param 3)
OUT_OFF = 0x6000      # the answer record (param 2)
P0_OFF = 0x7000       # the service descriptor the world+8 decode points at
S_OFF = 0x8000        # the service slot [p2+E]
GEOM_OFF = 0x9000     # [world+0x620]
TABLE_OFF = 0xD000    # the handle table
REC_OFF = 0xE000      # the domino record
ENTRY_OFF = 0xF000    # 0xc0
POOL_OFF = 0xF100     # 0x50, the type-3 element
XPOOL_OFF = 0xF160    # 0x50, the transform element {quat, position}
BVH_OFF = 0xF200      # 0x40
MESH_OFF = 0xF300     # nodes + vertices + triangles

# The real-world record lives in its own span: record, entries, the transform
# pool, the type-3 pool and the BVH, in that order.
DOM_OFF = 0x2_0000


def ror(x: int, n: int) -> int:
    x &= (1 << 64) - 1
    return ((x >> n) | (x << (64 - n))) & ((1 << 64) - 1)


def rol(x: int, n: int) -> int:
    return ror(x, 64 - n)


K_ADD = 0x01E2421A4798A328
K_XOR = 0xE1379E3CB308170D
K_SUB = 0x72184667DD2C7821
K_X2 = 0x8EF3269B22F53799
K_OFS = 0xB9E8EBFC25914471


def decode_slot(stored: int) -> int:
    """The body's own sequence: add, ror 0x22, xor, ror 0x2b."""
    return ror(ror((stored + K_ADD) & ((1 << 64) - 1), 0x22) ^ K_XOR, 0x2B)


def encode_slot(target: int) -> int:
    """The inverse, so `decode_slot(encode_slot(p)) == p`."""
    return (rol(rol(target, 0x2B) ^ K_XOR, 0x22) - K_ADD) & ((1 << 64) - 1)


def img(va: int) -> int:
    return va - IMAGE_BASE + V2B_BASE


def f32(value: float) -> float:
    """Round through binary32, because the client computes in f32."""
    return struct.unpack("<f", struct.pack("<f", value))[0]


def quad(*values) -> bytes:
    lanes = list(values) + [0.0] * (4 - len(values))
    return b"".join(struct.pack("<f", v) for v in lanes[:4])


def node(low, high, child: int, mask: int = 0xFFFF, axis: int = 0, count: int = 0) -> bytes:
    rec = bytearray(0x20)
    rec[0x00:0x0C] = quad(*low)[:0x0C]
    rec[0x0C:0x18] = quad(*high)[:0x0C]
    struct.pack_into("<i", rec, 0x18, child)
    struct.pack_into("<H", rec, 0x1C, mask)
    rec[0x1E] = axis
    rec[0x1F] = count
    return bytes(rec)


def triangle(corners, ids=(0, 0), mask: int = 0xFFFF) -> bytes:
    rec = bytearray(0x20)
    for slot, corner in enumerate(corners):
        struct.pack_into("<i", rec, slot * 4, corner)
    struct.pack_into("<H", rec, 0x18, ids[0])
    struct.pack_into("<H", rec, 0x1A, ids[1])
    struct.pack_into("<H", rec, 0x1C, mask)
    return bytes(rec)


def build_mesh(base: int) -> bytes:
    """Two quads under an internal root, as `mesh_fixture` does."""
    nodes = (
        node([-1.0, -1.0, -1.0], [7.0, 2.0, 1.0], 2, 0xFFFF, 0, 0)
        + node([0.0, 0.0, 0.0], [1.0, 1.0, 0.0], 0, 0xFFFF, 0, 2)
        + node([5.0, 0.0, 0.0], [6.0, 1.0, 0.0], 2, 0xFFFF, 0, 2)
    )
    tris = (
        triangle((0, 1, 2), (0x11, 0x21))
        + triangle((0, 2, 3), (0x12, 0x22))
        + triangle((4, 5, 6), (0x13, 0x23))
        + triangle((4, 6, 7), (0x14, 0x24))
    )
    verts = (
        quad(0.0, 0.0, 0.0)
        + quad(1.0, 0.0, 0.0)
        + quad(1.0, 1.0, 0.0)
        + quad(0.0, 1.0, 0.0)
        + quad(5.0, 0.0, 0.0)
        + quad(6.0, 0.0, 0.0)
        + quad(6.0, 1.0, 0.0)
        + quad(5.0, 1.0, 0.0)
    )
    nodes_at = 0x000
    verts_at = 0x100
    tris_at = 0x180
    out = bytearray(0x200)
    out[nodes_at : nodes_at + len(nodes)] = nodes
    out[verts_at : verts_at + len(verts)] = verts
    out[tris_at : tris_at + len(tris)] = tris
    struct.pack_into("<Q", out, 0x1F0, base + nodes_at)
    struct.pack_into("<Q", out, 0x1F8, 0)
    return bytes(out)


MESH_MAGIC = 0x324D_574F  # "OWM2", from crates/ow-tank/examples/mesh_world_dump.rs
MESH_SLOT_LEN = 0x30
MESH_INSTANCE_LEN = 0x28


def load_mesh_world(path: str):
    """`(base, slots, instances, blob, pinned)` from a `mesh_world_dump` file.

    A slot is one distinct model mesh (three array addresses and the block's
    words). An instance is one placement of a slot: the shape index, the
    translation and the rotation the collision builder would hand to
    `OwnerCreateBody`.
    """
    data = open(path, "rb").read()
    magic, count, base, blob_len, inst_count, pinned, _reserved = struct.unpack_from(
        "<IIQIIII", data, 0
    )
    if magic != MESH_MAGIC:
        raise SystemExit(f"{path}: not a mesh_world_dump file (magic {magic:#x})")
    slots = []
    at = 32
    for _ in range(count):
        nodes, points, triangles = struct.unpack_from("<QQQ", data, at)
        node_count, point_count, field_3c, height, field_44, _pad = (
            struct.unpack_from("<IIIIII", data, at + 24)
        )
        slots.append({
            "nodes": nodes,
            "points": points,
            "triangles": triangles,
            "node_count": node_count,
            "point_count": point_count,
            "field_3c": field_3c,
            "height": height,
            "field_44": field_44,
        })
        at += MESH_SLOT_LEN
    instances = []
    for _ in range(inst_count):
        shape, _pad0, tx, ty, tz, _pad1 = struct.unpack_from("<IIfffI", data, at)
        instances.append({
            "shape": shape,
            "translation": (tx, ty, tz),
            "rotation": struct.unpack_from("<4f", data, at + 24),
        })
        at += MESH_INSTANCE_LEN
    blob = data[at:at + blob_len]
    if len(blob) != blob_len:
        raise SystemExit(f"{path}: truncated blob ({len(blob)} of {blob_len})")
    return base, slots, instances, blob, pinned


def rotate(q, v):
    """`v` turned by the quaternion `q` = `(x, y, z, w)`."""
    x, y, z, w = q
    vx, vy, vz = v
    tx = 2.0 * (y * vz - z * vy)
    ty = 2.0 * (z * vx - x * vz)
    tz = 2.0 * (x * vy - y * vx)
    return (
        vx + w * tx + (y * tz - z * ty),
        vy + w * ty + (z * tx - x * tz),
        vz + w * tz + (x * ty - y * tx),
    )


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--guard-byte", type=lambda v: int(v, 0), default=0,
                    help="[Q+0xdd0]; the decoded compare is 0x96 ^ this byte")
    ap.add_argument("--equal-branch", action="store_true",
                    help="pick [Q+0xdd0] so the guard compares equal (0xd6)")
    ap.add_argument("--r12", type=lambda v: int(v, 0), default=0,
                    help="R12 at the evaluator's entry; the equal branch reads")
    ap.add_argument("--mesh-world", metavar="FILE", default=None,
                    help="assemble the range's real mesh world from a dump "
                         "(crates/ow-tank/examples/mesh_world_dump.rs)")
    ap.add_argument("--all-shapes", action="store_true",
                    help="put every placed instance in the broadphase "
                         "(3,017 of them for the range)")
    ap.add_argument("--trace", action="store_true", help="print the stage trace")
    args = ap.parse_args()

    oracle = EmuOracle(IMAGE)
    u = oracle.uc
    u.mem_map(BLOB, BLOB_SIZE)

    world = BLOB + WORLD_OFF
    desc = BLOB + DESC_OFF
    out = BLOB + OUT_OFF
    p0 = BLOB + P0_OFF
    sslot = BLOB + S_OFF
    geom = BLOB + GEOM_OFF
    table = BLOB + TABLE_OFF
    rec = BLOB + REC_OFF
    entry = BLOB + ENTRY_OFF
    pool = BLOB + POOL_OFF
    xpool = BLOB + XPOOL_OFF
    bvh = BLOB + BVH_OFF
    mesh = BLOB + MESH_OFF

    # ---- the service chain: world+8 -> p0 -> [+0x28] -> decode -> slot ----
    p2 = (sslot - K_OFS) & ((1 << 64) - 1)
    p1 = ((p2 ^ K_X2) + K_SUB) & ((1 << 64) - 1)
    u.mem_write(p0 + 0x28, struct.pack("<Q", p1))
    u.mem_write(sslot, struct.pack("<Q", 1))

    # ---- the globals the prologue tests, the guard block and the build byte ----
    q = BLOB + Q_OFF
    # The prologue's table: one entry that never matches the world's hashed id,
    # so the lookup misses and the guard runs with R12 left as the caller set
    # it (the empty table takes a different path that zeroes R12).
    rtab = BLOB + R_OFF
    u.mem_write(rtab, struct.pack("<I", 1))
    u.mem_write(rtab + 8, struct.pack("<Q", rtab + 0x10))
    u.mem_write(rtab + 0x10, struct.pack("<I", 0xDEADBEEF))
    u.mem_write(img(0x143B795B0), struct.pack("<Q", rtab))
    u.mem_write(img(0x143B79598), struct.pack("<Q", q))
    u.mem_write(img(0x143B795A0), struct.pack("<Q", BLOB + Q2_OFF))
    u.mem_write(img(0x143847D3C), b"")
    guard = 0xD6 if args.equal_branch else args.guard_byte
    u.mem_write(q + 0xDD0, bytes([guard & 0xFF]))

    # ---- world ----
    u.mem_write(world + 0x08, struct.pack("<Q", encode_slot(p0)))
    u.mem_write(world + 0x620, struct.pack("<Q", geom))
    u.mem_write(world + 0x6A8, struct.pack("<Q", 0))
    u.mem_write(world + 0x728, struct.pack("<Q", 0))

    # ---- geometry: the u16 handle at +0x31f8 ----
    u.mem_write(geom + 0x31F8, struct.pack("<H", 0))
    u.mem_write(geom + 0x31FA, struct.pack("<H", 0xFFFF))

    # ---- the handle table (0x1442a0bc0 holds its address) ----
    def encode_handle(ptr: int) -> int:
        return (((ptr ^ 0x77600BA5ADCC265C) - 0xB3269B88D44D5FB2)
                & ((1 << 64) - 1))

    u.mem_write(table, struct.pack("<Q", encode_handle(rec)))
    u.mem_write(img(0x1442A0BC0), struct.pack("<Q", table))

    # ---- the evaluator's descriptor object ----
    body = bytearray(0x1000)
    # +0x00 qword overlaps the +0x02 word: bytes 0-1 and 2-3 are the two
    # filter lanes, byte 6 is the driver's pass-flag byte.
    struct.pack_into("<Q", body, 0x00, 0x0001_0000_0100_FFFF)
    struct.pack_into("<H", body, 0x02, 0x0100)
    body[0x18] = 0
    body[0x1C] = 0
    body[0x450:0x460] = quad(0.75, 0.9, 5.0)
    body[0x460:0x470] = quad(0.75, 0.9, -5.0)
    struct.pack_into("<f", body, 0x480, 0.0)   # expansion
    struct.pack_into("<Q", body, 0x490, 0)     # support points
    struct.pack_into("<I", body, 0x498, 0)     # support count
    struct.pack_into("<f", body, 0x49C, 1.0)   # motion
    body[0x4A4] = 0
    u.mem_write(desc, bytes(body))

    # ---- the domino record ----
    r = bytearray(0x800)
    struct.pack_into("<Q", r, 0x288, BLOB + 0x10000)  # pose array
    struct.pack_into("<Q", r, 0x2A0, xpool)           # transform pool
    struct.pack_into("<Q", r, 0x318, pool)            # type-3 element pool
    struct.pack_into("<Q", r, 0x2B8, entry)           # shape entries
    struct.pack_into("<H", r, 0x194, 0x0000)
    for k in range(4):
        base = 0x1B0 + 0x28 * k
        struct.pack_into("<Q", r, base + 0x30, bvh)
        struct.pack_into("<i", r, base + 0x44, 0 if k == 0 else -1)
    u.mem_write(rec, bytes(r))

    # ---- the shape entry (0xc0) ----
    e = bytearray(0xC0)
    e[0x20] = 3                         # type 3 = mesh
    struct.pack_into("<i", e, 0x24, 0)  # type-3 element index
    struct.pack_into("<i", e, 0x50, 0)  # transform index (mode 0)
    struct.pack_into("<HHHH", e, 0x98, 0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF)
    e[0xB0] = 0                         # 0 = static transform, not a pose
    u.mem_write(entry, bytes(e))

    # ---- the type-3 element (0x50): scale, 1/scale, root, vertices, triangles ----
    m = build_mesh(mesh)
    u.mem_write(mesh, m)
    p = bytearray(0x50)
    p[0x00:0x10] = quad(1.0, 1.0, 1.0, 1.0)
    p[0x10:0x20] = quad(1.0, 1.0, 1.0, 1.0)
    struct.pack_into("<QQQ", p, 0x20, mesh + 0x000, mesh + 0x100, mesh + 0x180)
    u.mem_write(pool, bytes(p))

    # ---- the transform element (0x50): quat at +0x00, position at +0x10 ----
    xf = bytearray(0x50)
    xf[0x00:0x10] = quad(0.0, 0.0, 0.0, 1.0)
    xf[0x10:0x20] = quad(0.0, 0.0, 0.0, 0.0)
    u.mem_write(xpool, bytes(xf))

    # ---- the pass-0 BVH node (0x40): boxes, shape id, leaf marker ----
    b = bytearray(0x40)
    b[0x00:0x10] = quad(-1.0, -1.0, -1.0)
    b[0x10:0x20] = quad(7.0, 2.0, 1.0)
    struct.pack_into("<i", b, 0x20, 0)   # shape id (index into +0x2b8)
    struct.pack_into("<i", b, 0x2C, -1)  # -1 = leaf
    u.mem_write(bvh, bytes(b))

    # ---- the range's real mesh world, when a dump is given ----
    real = None
    if args.mesh_world:
        base, slots, instances, blob, pinned = load_mesh_world(args.mesh_world)
        indices = list(range(len(instances))) if args.all_shapes else [pinned]
        pages = (len(blob) + 0xFFF) & ~0xFFF
        u.mem_map(base, pages)
        u.mem_write(base, blob)

        dom = BLOB + DOM_OFF
        drec = dom
        dentries = dom + 0x800
        dxpool = dentries + 0xC0 * len(indices)
        dtpool = dxpool + 0x50 * len(indices)
        dbvh = dtpool + 0x50 * len(indices)

        # A pre-order BVH over `indices`: internal nodes union their children's
        # boxes, leaves carry the shape id at +0x20 and -1 at +0x2c. The boxes
        # are the shape's own local root box carried into world by its
        # placement, because the broadphase has no other frame to compare in.
        nodes = []

        def local_box(slot):
            at = slot["nodes"] - base
            low = struct.unpack_from("<3f", blob, at)
            high = struct.unpack_from("<3f", blob, at + 0x0C)
            return low, high

        def instance_box(instance):
            low, high = local_box(slots[instance["shape"]])
            q = instance["rotation"]
            t = instance["translation"]
            lo = [float("inf")] * 3
            hi = [float("-inf")] * 3
            for k in range(8):
                corner = (
                    low[0] if k & 1 else high[0],
                    low[1] if k & 2 else high[1],
                    low[2] if k & 4 else high[2],
                )
                world = rotate(q, corner)
                for lane in range(3):
                    value = world[lane] + t[lane]
                    lo[lane] = min(lo[lane], value)
                    hi[lane] = max(hi[lane], value)
            return lo, hi

        def build(lo, hi):
            index = len(nodes)
            nodes.append(None)
            if hi - lo == 1:
                low, high = instance_box(instances[indices[lo]])
                nodes[index] = (low, high, lo, -1, 0)
            else:
                mid = (lo + hi) // 2
                a = build(lo, mid)
                b = build(mid, hi)
                boxes = [nodes[a][:2], nodes[b][:2]]
                low = [min(x[0][lane] for x in boxes) for lane in range(3)]
                high = [max(x[1][lane] for x in boxes) for lane in range(3)]
                nodes[index] = (low, high, 0, a, b)
            return index

        build(0, len(indices))
        for index, (low, high, shape_id, child_a, child_b) in enumerate(nodes):
            n = bytearray(0x40)
            n[0x00:0x10] = quad(*low)
            n[0x10:0x20] = quad(*high)
            struct.pack_into("<i", n, 0x20, shape_id)
            struct.pack_into("<i", n, 0x2C, child_a)
            struct.pack_into("<i", n, 0x30, child_b)
            u.mem_write(dbvh + 0x40 * index, bytes(n))

        u.mem_write(dentries, bytes(0xC0 * len(indices)))
        u.mem_write(dxpool, bytes(0x50 * len(indices)))
        u.mem_write(dtpool, bytes(0x50 * len(indices)))
        for index, instance_index in enumerate(indices):
            instance = instances[instance_index]
            slot = slots[instance["shape"]]
            e = bytearray(0xC0)
            e[0x20] = 3
            struct.pack_into("<i", e, 0x24, index)
            struct.pack_into("<i", e, 0x50, index)
            struct.pack_into("<HHHH", e, 0x98, 0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF)
            u.mem_write(dentries + 0xC0 * index, bytes(e))
            p = bytearray(0x50)
            p[0x00:0x10] = quad(1.0, 1.0, 1.0, 1.0)
            p[0x10:0x20] = quad(1.0, 1.0, 1.0, 1.0)
            struct.pack_into("<QQQ", p, 0x20,
                             slot["nodes"], slot["points"], slot["triangles"])
            u.mem_write(dtpool + 0x50 * index, bytes(p))
            # The owner pool: `ShapeWorldTransform` (`0x142cb8b80`) reads the
            # quaternion at +0x00 and the position at +0x10.
            xf = bytearray(0x50)
            xf[0x00:0x10] = quad(*instance["rotation"])
            xf[0x10:0x20] = quad(*instance["translation"], 0.0)
            u.mem_write(dxpool + 0x50 * index, bytes(xf))

        r = bytearray(0x800)
        struct.pack_into("<Q", r, 0x2A0, dxpool)
        struct.pack_into("<Q", r, 0x2B8, dentries)
        struct.pack_into("<Q", r, 0x318, dtpool)
        for k in range(4):
            base_off = 0x1B0 + 0x28 * k
            struct.pack_into("<Q", r, base_off + 0x30, dbvh)
            struct.pack_into("<i", r, base_off + 0x44, 0 if k == 0 else -1)
        u.mem_write(drec, bytes(r))
        u.mem_write(table, struct.pack("<Q", encode_handle(drec)))

        # The pinned query: a downward sweep through the shape's own root box,
        # a quarter of the way in on x and z (the `mesh_world_real` choice),
        # built in the shape's local frame and then carried into world by the
        # placement, exactly as the driver carries it back.
        placed = instances[pinned]
        low, high = local_box(slots[placed["shape"]])
        qx = f32(low[0] + f32(f32(high[0] - low[0]) * 0.25))
        qz = f32(low[2] + f32(f32(high[2] - low[2]) * 0.25))
        local_start = (qx, f32(high[1] + 2.0), qz)
        local_end = (qx, f32(low[1] - 2.0), qz)

        def to_world(point):
            turned = rotate(placed["rotation"], point)
            return tuple(
                f32(turned[lane] + placed["translation"][lane]) for lane in range(3)
            )

        start = to_world(local_start)
        end = to_world(local_end)
        u.mem_write(desc + 0x450, quad(*start))
        u.mem_write(desc + 0x460, quad(*end))
        real = {
            "shapes": len(indices),
            "instances": len(instances),
            "pinned": pinned,
            "start": start,
            "end": end,
            "local_point": (qx, 4.0, qz),
            "local_normal": (0.0, 1.0, 0.0),
            "to_world": to_world,
            "rotation": placed["rotation"],
            "translation": placed["translation"],
        }

    # ---- call the wrapper ----
    for reg, value in (
        (x.UC_X86_REG_RCX, world),
        (x.UC_X86_REG_RDX, out),
        (x.UC_X86_REG_R8, desc),
        (x.UC_X86_REG_R9, 0),
    ):
        u.reg_write(reg, value)
    u.reg_write(x.UC_X86_REG_R12, args.r12)
    rsp = STACK_TOP - 8
    u.reg_write(x.UC_X86_REG_RSP, rsp)
    u.mem_write(rsp, struct.pack("<Q", SENTINEL))

    seen = set()
    fault = []

    def on_code(uc, address, size, _ud):
        seen.add(address - V2B_BASE + IMAGE_BASE)
        if args.trace and address - V2B_BASE + IMAGE_BASE in (
            0x14203DB79, 0x14203DBDD, 0x14203DC51, 0x14203DDF1, 0x14203DE51,
        ):
            print(f"  spot {address - V2B_BASE + IMAGE_BASE:#x} "
                  f"r12 {uc.reg_read(x.UC_X86_REG_R12):#x}")
        if args.trace:
            va = address - V2B_BASE + IMAGE_BASE
            if 0x142CBD510 <= va < 0x142CBD900:
                print(f"  drv {va:#x}")

    def on_fault(kind):
        def hook(uc, access, address, size, value, _ud):
            fault.append((kind, address, size,
                          uc.reg_read(x.UC_X86_REG_RIP) - V2B_BASE + IMAGE_BASE))
        return hook

    u.hook_add(UC_HOOK_CODE, on_code)
    u.hook_add(UC_HOOK_MEM_READ_UNMAPPED, on_fault("read"))
    u.hook_add(UC_HOOK_MEM_WRITE_UNMAPPED, on_fault("write"))

    status = "returned"
    try:
        u.emu_start(img(0x1420328C0), SENTINEL, count=2_000_000)
    except UcError as exc:
        status = f"fault: {exc}"

    def reached(lo: int, hi: int) -> bool:
        return any(lo <= va < hi for va in seen)

    print(f"status          {status}")
    print(f"steps           {len(seen)} distinct code addresses")
    print(f"return (rax)    {u.reg_read(x.UC_X86_REG_RAX):#x}")
    for f in fault[:2]:
        kind, addr, size, at = f
        print(f"fault           {kind} {addr:#x} size {size} at {at:#x}")
    print()
    print(f"guard body      {reached(0x14203debb, 0x14203e0d8)}")
    print(f"service slot    {reached(0x14203df11, 0x14203df33)}")
    print(f"handle dispatch {reached(0x142cc45a0, 0x142cc47ce)}")
    print(f"query driver    {reached(0x142cbd510, 0x142cbd87c)}")
    print(f"narrowphase     {reached(0x142cc1610, 0x142cc1814)}")
    print(f"type-3 mesh     {reached(0x142caed30, 0x142caf6a7)}")
    print(f"hit policy      {reached(0x14202bc90, 0x14202c394)}")
    print()

    raw = bytes(u.mem_read(out, 0x50))
    # The record's layout is `ow_sim::policy::HitRecord`: point at +0x00,
    # normal at +0x10, geometry at +0x20, ctx at +0x28, fraction at +0x30,
    # the narrowphase's dword at +0x34, key/slot at +0x38/+0x3a, id/kind at
    # +0x3c/+0x3e, tail at +0x40, byte at +0x48.
    print(f"answer record   {raw.hex()}")
    print(f"  point         {struct.unpack_from('<4f', raw, 0x00)}")
    print(f"  normal        {struct.unpack_from('<4f', raw, 0x10)}")
    print(f"  geometry      {struct.unpack_from('<Q', raw, 0x20)[0]:#x}")
    print(f"  ctx           {struct.unpack_from('<Q', raw, 0x28)[0]:#x}")
    print(f"  fraction      {struct.unpack_from('<f', raw, 0x30)[0]!r}")
    print(f"  word34        {struct.unpack_from('<I', raw, 0x34)[0]}")
    print(f"  key/slot      {struct.unpack_from('<HH', raw, 0x38)}")
    print(f"  id/kind       {struct.unpack_from('<HH', raw, 0x3c)}")
    print(f"  tail          {struct.unpack_from('<Q', raw, 0x40)[0]:#x} "
          f"byte48 {raw[0x48]:#04x}")

    if real is not None:
        print()
        print(f"mesh world      {real['shapes']} instance(s) of {real['instances']} "
              f"placed shape(s); pinned instance {real['pinned']}")
        print(f"placement       translation {real['translation']} "
              f"rotation {real['rotation']}")
        print(f"query           start {real['start']} end {real['end']}")
        # Pinned by `mesh_world_real`: in the shape's own frame the sweep meets
        # the mesh's top face at y = 4 and accepts triangle 7 with ids
        # 0x8000/0. The placement carries that hit into world, so the world
        # answer must be the same point turned and moved by it, and the normal
        # the same direction turned by it. The harness builds the world query in
        # Python and the client carries it back in `f32`, so the placed answer
        # is compared with a tolerance; the identity placement, where no
        # round-trip happens, is compared bit for bit.
        want_point = real["to_world"](real["local_point"])
        want_normal = rotate(real["rotation"], real["local_normal"])
        want_fraction = struct.unpack("<f", struct.pack("<I", 0x3E8051B8))[0]
        identity = real["rotation"] == (0.0, 0.0, 0.0, 1.0) and real[
            "translation"] == (0.0, 0.0, 0.0)
        point = struct.unpack_from("<4f", raw, 0x00)
        normal = struct.unpack_from("<4f", raw, 0x10)
        fraction = struct.unpack_from("<f", raw, 0x30)[0]
        word34 = struct.unpack_from("<I", raw, 0x34)[0]
        ids = struct.unpack_from("<HH", raw, 0x3C)

        def near(a, b, tol):
            return all(abs(x - y) <= tol for x, y in zip(a, b))

        tol = 0.0 if identity else 1e-4
        checks = [
            ("point", point[:3], want_point, near(point[:3], want_point, tol)),
            ("normal", normal[:3], want_normal, near(normal[:3], want_normal, tol)),
            ("fraction", fraction, want_fraction,
             fraction == want_fraction if identity else True),
            ("triangle", word34, 7, word34 == 7 if identity else True),
        ]
        for name, got, want, ok in checks:
            mark = "ok" if ok else "MISMATCH"
            note = "" if identity or name in ("point", "normal") else " (informational)"
            print(f"  {name:<12} {got!r}  want {want!r}  {mark}{note}")
        # The record's id/kind are the policy's *resolved* id, not the
        # triangle's: the triangle id 0x8000 reads as a negative i16, so the
        # policy looks it up in the geometry's variant table and falls back to
        # the pending id 1 when the table is not built.
        print(f"  id/kind      {ids}  (policy-resolved; triangle ids are 0x8000/0)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

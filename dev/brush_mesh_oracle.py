"""Oracle for stage 1 of the brush tree -> mesh conversion.

    python dev/brush_mesh_oracle.py

Runs `FUN_142cf0360` (`0x142cf0360`), the hull builder that `BrushTreeToMesh`
(`0x142cf23c0`) calls first, on synthetic brushes under the emulator and prints
everything it produces: the `0x30`-byte workspace vertices (whose first `0x18`
bytes are still the raw brush-vertex record at this point -- the position decode
happens later, in `BrushTreeToMesh`), the `0x60`-byte hulls, the per-node op
class, the edge-pair array and the sorted node order.

The Rust port (`ow_tank::brush_mesh::hull_stage`) is compared against this
output field for field. The builder allocates through the image's allocator hook,
so the probe installs the same bump-allocator stub `dev/brush_vertex_oracle.py`
uses at `0x1442a0158` and declares the region it hands out.

Nothing here is a gameplay claim and nothing is written to the image or the
install.
"""

from __future__ import annotations

import os
import struct
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "verify_harness"))

from brush_vertex_oracle import HOOK_RVA, hook_state, v2b  # noqa: E402
from emu_oracle import ARENA_BASE, ArgSpec, EmuOracle  # noqa: E402

IMAGE = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
    "dev", "data", "offline_recover", "module_offline.bin",
)

HULL_BUILD = 0x142CF0360
HEAP_OFF = 0x8000
HEAP_SIZE = 0x8000
CTX_OFF = 0x2000
RECORD_OFF = 0x2100
ARGS_OFF = 0x2200
VERTEX_OFF = 0x2300
NODE_OFF = 0x2800


def node(prev, nxt, vertex, next_ring, x, y, px, py, leaf, descending):
    out = bytearray(0x28)
    struct.pack_into("<i", out, 0x00, prev)
    struct.pack_into("<i", out, 0x04, nxt)
    struct.pack_into("<i", out, 0x08, vertex)
    struct.pack_into("<i", out, 0x0C, -1)
    struct.pack_into("<i", out, 0x10, next_ring)
    struct.pack_into("<i", out, 0x14, x)
    struct.pack_into("<i", out, 0x18, y)
    struct.pack_into("<i", out, 0x1C, px)
    struct.pack_into("<i", out, 0x20, py)
    out[0x24] = leaf
    out[0x25] = descending
    return bytes(out)


def vertex(index, x, y, divisor=1):
    out = bytearray(0x20)
    struct.pack_into("<q", out, 0x00, x)
    struct.pack_into("<q", out, 0x08, y)
    struct.pack_into("<q", out, 0x10, divisor)
    struct.pack_into("<I", out, 0x18, index)
    return bytes(out)


# The node records `0x142ced620` writes for a square and a triangle; they are
# the same values `brush::tests` pins, so the two oracles agree on the input.
SQUARE = [
    node(3, 1, 0, -1, 0, 0, 400, 0, 1, 1),
    node(0, 2, 1, -1, 400, 400, 400, 0, 1, 0),
    node(1, 3, 2, -1, 0, 400, 400, 400, 1, 0),
    node(2, 0, 3, -1, 0, 400, 0, 0, 1, 1),
]
SQUARE_VERTS = [(0, 0), (400, 0), (400, 400), (0, 400)]

TRIANGLE = [
    node(2, 1, 0, -1, 0, 0, 800, 0, 1, 1),
    node(0, 2, 1, -1, 200, 600, 800, 0, 1, 0),
    node(1, 0, 2, -1, 200, 600, 0, 0, 1, 1),
]
TRIANGLE_VERTS = [(0, 0), (800, 0), (200, 600)]

# Two rings: the first ring's node 0 carries `next_ring = 2`. Ring A is
# {0, 1}, ring B is {2, 3, 4}; the second ring is walked from node 2.
TWO_RINGS = [
    node(1, 1, 0, 2, 0, 0, 800, 800, 1, 1),
    node(0, 0, 1, -1, 800, 0, 0, 800, 1, 0),
    node(4, 3, 2, -1, 0, 400, 400, 400, 1, 1),
    node(2, 4, 3, -1, 400, 800, 400, 400, 1, 0),
    node(3, 2, 4, -1, 0, 800, 400, 800, 1, 0),
]
TWO_RING_VERTS = [(0, 0), (800, 0), (0, 400), (400, 800), (0, 800)]

# A ring whose ordering pass produced every op class: descending bytes
# alternate 1,0,1,0 so ops 2/3 appear whole and 0/4, 1/5 depend on the cross
# products below.
ZIGZAG = [
    node(4, 1, 0, -1, 0, 0, 400, 400, 1, 1),
    node(0, 2, 1, -1, 400, 400, 400, 0, 1, 0),
    node(1, 3, 2, -1, 400, 800, 0, 800, 1, 1),
    node(2, 4, 3, -1, 0, 800, 0, 400, 1, 0),
    node(3, 0, 4, -1, 0, 400, 0, 0, 1, 1),
]
ZIGZAG_VERTS = [(0, 0), (400, 400), (400, 800), (0, 800), (0, 400)]

CASES = [
    ("square", SQUARE, SQUARE_VERTS),
    ("triangle", TRIANGLE, TRIANGLE_VERTS),
    ("two rings", TWO_RINGS, TWO_RING_VERTS),
    ("zigzag", ZIGZAG, ZIGZAG_VERTS),
]


def ring_count(nodes: bytes, count: int, root: int) -> int:
    at = struct.unpack_from("<i", nodes, 0x10 + root * 0x28)[0]
    rings = 0
    while at != -1:
        rings += 1
        at = struct.unpack_from("<i", nodes, 0x10 + at * 0x28)[0]
    return rings


def build(oracle: EmuOracle, label, nodes, points, verbose=True):
    """Run `0x142cf0360` on one brush and return what it produced."""
    count = len(nodes)
    verts = b"".join(vertex(i, x, y) for i, (x, y) in enumerate(points))
    node_bytes = b"".join(nodes)

    rings = ring_count(node_bytes, count, 0)
    triangles = count - 2 + 2 * rings

    record = bytearray(0x28)
    struct.pack_into("<Q", record, 0x00, ARENA_BASE + VERTEX_OFF)
    struct.pack_into("<Q", record, 0x08, ARENA_BASE + NODE_OFF)
    struct.pack_into("<I", record, 0x10, count)
    struct.pack_into("<I", record, 0x14, count)
    struct.pack_into("<I", record, 0x18, 0)  # root
    struct.pack_into("<I", record, 0x20, 0xFFFFFFFF)

    ctx = bytearray(0x60)
    # What `BrushTreeToMesh` seeds before it calls this: the mesh vertex count
    # and the free-hull index (+0x30/+0x34), the hull count (+0x40), the node
    # count (+0x44) and the triangle count (+0x48).
    struct.pack_into("<I", ctx, 0x30, count)
    struct.pack_into("<I", ctx, 0x34, count)
    struct.pack_into("<I", ctx, 0x40, triangles * 3)
    struct.pack_into("<I", ctx, 0x44, count)
    struct.pack_into("<I", ctx, 0x48, triangles)

    args = bytearray(0x18)
    struct.pack_into("<Q", args, 0x00, ARENA_BASE + RECORD_OFF)
    struct.pack_into("<Q", args, 0x08, 0)  # no arena: use the global allocator
    struct.pack_into("<I", args, 0x10, 1)

    oracle.set_state(v2b(HOOK_RVA), hook_state())
    specs = [
        ArgSpec(kind="ptr", mem=bytes(ctx), arena_offset=CTX_OFF),
        ArgSpec(kind="ptr", mem=bytes(args), arena_offset=ARGS_OFF),
        # Not an argument the callee reads: it declares the heap window so the
        # allocated arrays come back through `pointee_after`.
        ArgSpec(kind="ptr", mem=bytes(HEAP_SIZE), arena_offset=HEAP_OFF),
        # The two arrays the record points at; placed after the window is
        # declared so their arena offsets are pinned by the record above.
        ArgSpec(kind="ptr", mem=verts, arena_offset=VERTEX_OFF),
        ArgSpec(kind="ptr", mem=node_bytes, arena_offset=NODE_OFF),
        ArgSpec(kind="ptr", mem=bytes(record), arena_offset=RECORD_OFF),
    ]
    res = oracle.call(v2b(HULL_BUILD), specs, ret_kind="void")
    if not res.ok:
        raise SystemExit(f"oracle fault on {label}: {res.fault}")
    after = res.pointee_after
    out_ctx = after[:0x60]
    heap = after[0x60 + 0x18 : 0x60 + 0x18 + HEAP_SIZE]
    ptrs = struct.unpack_from("<6Q", out_ctx, 0x00)
    return {
        "label": label,
        "count": count,
        "triangles": triangles,
        "rings": rings,
        "ctx": out_ctx,
        "ptrs": ptrs,
        "heap": heap,
    }


def dump(result):
    count = result["count"]
    ctx = result["ctx"]
    ws, hull, faces, order, pairs, groups = result["ptrs"]
    heap = result["heap"]
    base = HEAP_OFF  # heap window == ARENA + HEAP_OFF
    print(f"  {result['label']}: n={count} rings={result['rings']} "
          f"triangles={result['triangles']}")
    print(f"    next hull={struct.unpack_from('<I', ctx, 0x34)[0]} "
          f"faces={struct.unpack_from('<I', ctx, 0x38)[0]} "
          f"groups={struct.unpack_from('<I', ctx, 0x3C)[0]}")
    for name, ptr in (("ws", ws), ("hull", hull), ("faces", faces),
                      ("order", order), ("pairs", pairs), ("groups", groups)):
        if ptr == 0:
            print(f"    {name}: null")
        else:
            print(f"    {name}: heap+0x{ptr - ARENA_BASE - base:#x}")

    def at(ptr, off, n):
        start = ptr - ARENA_BASE - base + off
        return heap[start:start + n]

    for i in range(count):
        row = at(ws, i * 0x30, 0x30)
        x, y, d = struct.unpack_from("<3q", row, 0)
        hp = struct.unpack_from("<Q", row, 0x20)[0]
        op = struct.unpack_from("<I", row, 0x28)[0]
        assert hp == 0 or True
        print(f"    ws {i}: x={x} y={y} d={d} op={op} "
              f"hull=heap+0x{hp - ARENA_BASE - base:#x}")
    for i in range(count):
        row = at(hull, i * 0x60, 0x60)
        links = struct.unpack_from("<4Q", row, 0x00)
        prev, nxt = struct.unpack_from("<2Q", row, 0x28)
        vtx, closure, pair = struct.unpack_from("<3Q", row, 0x38)
        print(f"    hull {i}: tree=({links[0] - ARENA_BASE - base:#x},"
              f"{links[1] - ARENA_BASE - base:#x},"
              f"{links[2] - ARENA_BASE - base:#x}) colour={links[3] & 0xFFFFFFFF} "
              f"prev=heap+0x{prev - ARENA_BASE - base:#x} "
              f"next=heap+0x{nxt - ARENA_BASE - base:#x} "
              f"vertex=heap+0x{vtx - ARENA_BASE - base:#x} "
              f"closure={closure - ARENA_BASE - base:#x} "
              f"pair=heap+0x{pair - ARENA_BASE - base:#x}")
    for i in range(count):
        row = at(pairs, i * 0x10, 0x10)
        print(f"    pair {i}: {struct.unpack_from('<4i', row, 0)}")
    for i in range(count):
        ptr = struct.unpack_from("<Q", at(order, i * 8, 8), 0)[0]
        print(f"    order {i}: ws {(ptr - ws) // 0x30}")


def main() -> int:
    oracle = EmuOracle(IMAGE)
    print("hull builder 0x142cf0360:")
    for label, nodes, points in CASES:
        dump(build(oracle, label, nodes, points))
    print()
    print("whole conversion 0x142cf23c0:")
    for label, rings in MESH_RINGS:
        nodes, points = build_brush(rings)
        dump_mesh(mesh(oracle, label, nodes, points))
    return 0



# ---------------------------------------------------------------------------
# The whole conversion: `BrushTreeToMesh` (0x142cf23c0)
# ---------------------------------------------------------------------------

MESH_BUILD = 0x142CF23C0
OUT_OFF = 0x2900


def f32(v):
    return struct.unpack("<f", struct.pack("<f", v))[0]


def quantise(value, scale):
    """`0x142ced620`'s quantiser: roundf((v * scale) * 400) as two f32 steps."""
    x = f32(f32(value) * f32(scale))
    x = f32(x * f32(400.0))
    if x >= 0.0:
        return int(x + 0.5)
    return -int(-x + 0.5)


def ring_node(prev, nxt, vertex, next_ring, x, y, px, py, leaf, descending):
    return node(prev, nxt, vertex, next_ring, x, y, px, py, leaf, descending)


def build_brush(rings, scales=(1.0, 1.0)):
    """The node records `0x142ced620` writes for a list of rings of points.

    `rings` is a list of point lists; each ring becomes its own run of nodes and
    the previous ring's first node carries the `next_ring` link. The order of
    the nodes inside a ring is the ring's own order; the ordering pass may swap
    each node's pair, exactly as the client does.
    """
    points = [p for r in rings for p in r]
    n = len(points)
    xs = [quantise(p[0], scales[0]) for p in points]
    ys = [quantise(p[1], scales[1]) for p in points]

    starts = []
    at = 0
    for r in rings:
        starts.append(at)
        at += len(r)

    nodes = []
    for ri, r in enumerate(rings):
        base = starts[ri]
        m = len(r)
        for i in range(m):
            gi = base + i
            nodes.append({
                "prev": base + (i - 1) % m,
                "next": base + (i + 1) % m,
                "vertex": gi,
                "next_ring": starts[ri + 1] if (i == 0 and ri + 1 < len(rings)) else -1,
                "x": xs[gi], "y": ys[gi], "prev_x": 0, "prev_y": 0,
                "leaf": 1, "descending": 0,
            })
    for i in range(n):
        t = nodes[i]["prev"]
        nodes[t]["prev_x"] = nodes[i]["x"]
        nodes[t]["prev_y"] = nodes[i]["y"]
    for nd in nodes:
        if nd["y"] != nd["prev_y"]:
            greater = nd["y"] > nd["prev_y"]
        else:
            greater = nd["prev_x"] > nd["x"]
        if greater:
            nd["descending"] = 1
        elif nd["y"] == nd["prev_y"] and nd["prev_x"] == nd["x"]:
            raise SystemExit("degenerate ring: two consecutive points coincide")
        else:
            nd["x"], nd["prev_x"] = nd["prev_x"], nd["x"]
            nd["y"], nd["prev_y"] = nd["prev_y"], nd["y"]
            nd["descending"] = 0
    packed = [
        ring_node(nd["prev"], nd["next"], nd["vertex"], nd["next_ring"], nd["x"],
                  nd["y"], nd["prev_x"], nd["prev_y"], nd["leaf"], nd["descending"])
        for nd in nodes
    ]
    return packed, [(xs[i], ys[i]) for i in range(n)]


# The brushes the whole conversion is pinned on. The first two are the square
# and triangle the hull-stage cases use; the rest reach the ear clipping: a
# concave L, a five-point concave arrow, the same square wound clockwise, and a
# square with a triangular hole (two rings, which is where `next_ring` and the
# extra two triangles per ring come from).
MESH_RINGS = [
    ("square", [[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]]),
    ("triangle", [[(0.0, 0.0), (2.0, 0.0), (0.5, 1.5)]]),
    ("ell", [[(0.0, 0.0), (2.0, 0.0), (2.0, 1.0), (1.0, 1.0), (1.0, 2.0), (0.0, 2.0)]]),
    ("arrow", [[(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (1.0, 1.0), (0.0, 2.0)]]),
    ("triangle cw", [[(1.0, 0.0), (0.0, 0.0), (0.0, 1.0)]]),
    ("square with hole", [
        [(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)],
        [(1.0, 1.0), (2.0, 1.0), (1.5, 2.0)],
    ]),
]


def mesh(oracle: EmuOracle, label, nodes, points, variant=1):
    """Run `0x142cf23c0` on one brush and return the mesh it wrote."""
    count = len(nodes)
    verts = b"".join(vertex(i, x, y) for i, (x, y) in enumerate(points))
    node_bytes = b"".join(nodes)

    record = bytearray(0x28)
    struct.pack_into("<Q", record, 0x00, ARENA_BASE + VERTEX_OFF)
    struct.pack_into("<Q", record, 0x08, ARENA_BASE + NODE_OFF)
    struct.pack_into("<I", record, 0x10, count)
    struct.pack_into("<I", record, 0x14, count)
    struct.pack_into("<I", record, 0x18, 0)
    struct.pack_into("<I", record, 0x20, 0xFFFFFFFF)

    args = bytearray(0x18)
    struct.pack_into("<Q", args, 0x00, ARENA_BASE + RECORD_OFF)
    struct.pack_into("<Q", args, 0x08, 0)
    struct.pack_into("<I", args, 0x10, variant)

    oracle.set_state(v2b(HOOK_RVA), hook_state())
    specs = [
        ArgSpec(kind="ptr", mem=bytes(0x20), arena_offset=OUT_OFF),
        ArgSpec(kind="ptr", mem=bytes(args), arena_offset=ARGS_OFF),
        ArgSpec(kind="ptr", mem=bytes(HEAP_SIZE), arena_offset=HEAP_OFF),
        ArgSpec(kind="ptr", mem=verts, arena_offset=VERTEX_OFF),
        ArgSpec(kind="ptr", mem=node_bytes, arena_offset=NODE_OFF),
        ArgSpec(kind="ptr", mem=bytes(record), arena_offset=RECORD_OFF),
    ]
    import unicorn

    last = []

    def on_code(uu, addr, size, ud):
        last.append(addr)

    target = v2b(MESH_BUILD)
    oracle.uc.hook_add(unicorn.UC_HOOK_CODE, on_code)
    res = oracle.call(target, specs, ret_kind="void")
    if not res.ok:
        # The client faults on some inputs; the faulting address is part of the
        # finding, so report it instead of aborting the run.
        at = (last[-1] - target + MESH_BUILD) if last else 0
        return {"label": label, "variant": variant, "count": count,
                "vertex_count": 0, "triangle_count": 0, "vertices": [],
                "triangles": [], "fault": res.fault, "fault_at": at}
    after = res.pointee_after
    out = after[:0x20]
    heap = after[0x20 + 0x18 : 0x20 + 0x18 + HEAP_SIZE]
    vptr, vcount, tptr, tcount = struct.unpack_from("<QIIQ", out, 0)[0], 0, 0, 0
    vptr = struct.unpack_from("<Q", out, 0x00)[0]
    vcount = struct.unpack_from("<I", out, 0x08)[0]
    tptr = struct.unpack_from("<Q", out, 0x10)[0]
    tcount = struct.unpack_from("<I", out, 0x18)[0]

    def at(ptr, off, n):
        if ptr == 0:
            return b""
        start = ptr - ARENA_BASE - HEAP_OFF + off
        return heap[start:start + n]

    verts_out = []
    for i in range(vcount):
        bits = struct.unpack("<4I", at(vptr, i * 0x10, 0x10))
        verts_out.append(bits)
    tris_out = []
    for i in range(tcount):
        tris_out.append(struct.unpack("<6i", at(tptr, i * 0x18, 0x18)))
    return {
        "label": label,
        "variant": variant,
        "count": count,
        "vertex_count": vcount,
        "triangle_count": tcount,
        "vertices": verts_out,
        "triangles": tris_out,
    }


def dump_mesh(result):
    if result.get("fault"):
        print(f"  {result['label']} [{result['variant']}]: FAULT {result['fault']} "
              f"at 0x{result['fault_at']:x} (image address)")
        return
    print(f"  {result['label']} [{result['variant']}] n={result['count']} "
          f"vertices={result['vertex_count']} triangles={result['triangle_count']}")
    for i, bits in enumerate(result["vertices"]):
        print(f"    v{i}: " + " ".join(f"0x{b:08x}" for b in bits))
    for i, tri in enumerate(result["triangles"]):
        print(f"    t{i}: {tri}")

if __name__ == "__main__":
    raise SystemExit(main())

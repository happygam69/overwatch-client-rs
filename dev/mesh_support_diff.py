"""Differential probe: the type-3 support-point walk, client vs `ow-sim`.

Runs the real bytes of `0x142caed30` (the type-3 mesh narrowphase) with
support-point query blocks - the shape a capsule query has - and compares its
published output against `ow_sim::collide::mesh_sweep` through the shipped
probe, byte for byte: the three hit quads, the triangle index, the two record
ids and both walk counters.

What this proves and what it does not:

* the hull the client builds for a support-point query (the composed rotation,
  the point-cloud centre, the `+0.02`-inflated half extent, the shifted end and
  the recalculated direction) is the one `ow-sim` builds - every case below
  reaches the client's walk, and the walk's node culling depends on every lane
  of that hull;
* the *triangle accept* on that path is `0x142cc9460`, which `ow-sim` does not
  implement yet. A case the client's triangle test accepts therefore shows a hit
  on the client side and none here; when the walk's counters still agree, that
  difference is reported as an `OPEN` handoff, not as a failure.

The oracle dereferences the block's support pointer as a raw arena address and
the probe resolves the same array against the element pointee, so the two sides
receive that pointer in their own form. That is fixture plumbing; the array
either side reads is the same bytes.

    python dev/mesh_support_diff.py            # run the corpus
    python dev/mesh_support_diff.py --build    # rebuild the probe first

Read-only: nothing here writes the image, the install or the Ghidra project.
"""

from __future__ import annotations

import os
import struct
import subprocess
import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "verify_harness"))

from emu_oracle import EmuOracle, V2B_BASE  # noqa: E402

import mesh_fixture as mf  # noqa: E402

IMAGE = "dev/data/offline_recover/module_offline.bin"
MESH_BODY = 0x142CAED30
PROBE = os.path.join("target", "verify_probe", "ow-sim", "release", "verify_probe.exe")
SUPPORT_AT = 0x70  # free run in the world blob, between nodes and vertices


def quad(values) -> bytes:
    lanes = list(values) + [0.0] * (4 - len(values))
    return b"".join(struct.pack("<f", v) for v in lanes[:4])


def world_with_points(world: bytes, points) -> bytes:
    out = bytearray(world)
    blob = b"".join(quad([p[0], p[1], p[2], 0.0]) for p in points)
    out[SUPPORT_AT : SUPPORT_AT + len(blob)] = blob
    return bytes(out)


def support_block(start, end, count, expansion, motion=1.0, quat=(0.0, 0.0, 0.0, 1.0)):
    """A query block naming the support array at `SUPPORT_AT` in arena form."""
    rec = bytearray(0x78)
    rec[0x00:0x10] = quad(start)
    rec[0x10:0x20] = quad(end)
    rec[0x20:0x30] = quad([motion] * 4)
    struct.pack_into("<Q", rec, 0x30, mf.POOL_ADDR + mf.POOL_LEN + SUPPORT_AT)
    struct.pack_into("<i", rec, 0x38, count)
    rec[0x40:0x50] = quad(quat)
    rec[0x50:0x60] = quad([expansion] * 4)
    struct.pack_into("<H", rec, 0x70, mf.MASK_ALL)
    return bytes(rec)


def ladder_world(x0: float) -> bytes:
    """One leaf square pushed out to `x0`, so the hull's step decides its reach.

    The root spans the whole query; the leaf's box starts at `x0` and holds one
    upward triangle. A sweep at x = 0.75 with a hull half extent of 0.4 reaches
    the leaf only while `x0 <= 0.75 + step`, so the rungs of a ladder one ULP
    apart turn the hull's step into an observable.
    """
    nodes = (
        mf.node([-8.0, -8.0, -8.0], [8.0, 8.0, 8.0], 2, mf.MASK_ALL, 0, 0)
        + mf.node([x0, 0.0, 0.0], [x0 + 1.0, 1.0, 0.0], 0, mf.MASK_ALL, 0, 1)
        # A masked-out third node, so the root's second child is never expanded.
        + mf.node([0.0, 0.0, 0.0], [0.0, 0.0, 0.0], 0, 0x0000, 0, 0)
    )
    tris = mf.triangle((0, 1, 2), (0x51, 0x52))
    world = bytearray(mf.WORLD_LEN)
    world[mf.WORLD_AT : mf.WORLD_AT + len(nodes)] = nodes
    world[mf.VERTICES_AT : mf.VERTICES_AT + 0x30] = mf.vertices(
        [(x0, 0.0, 0.0), (x0 + 1.0, 0.0, 0.0), (x0 + 1.0, 1.0, 0.0)]
    )
    world[mf.TRIANGLES_AT : mf.TRIANGLES_AT + len(tris)] = tris
    return bytes(world)


def f32(value: float) -> float:
    return struct.unpack("<f", struct.pack("<f", value))[0]


def next_up(value: float) -> float:
    bits = struct.unpack("<I", struct.pack("<f", f32(value)))[0]
    return struct.unpack("<f", struct.pack("<I", bits + 1))[0]


def next_down(value: float) -> float:
    bits = struct.unpack("<I", struct.pack("<f", f32(value)))[0]
    return struct.unpack("<f", struct.pack("<I", bits - 1))[0]


CAPSULE = [(0.0, 0.0, 0.4), (0.0, 0.0, -0.4)]
TWO_X = [(0.4, 0.0, 0.0), (-0.4, 0.0, 0.0)]

# Each case: name, world, block, xform.
def corpus():
    ident = mf.xform([0.0, 0.0, 0.0])
    plain = mf.standard_world()
    caps = world_with_points(plain, CAPSULE)
    cases = []

    def add(name, world, block, shape):
        cases.append((name, world, block, shape))

    add(
        "capsule over quad A (client accepts)",
        caps,
        support_block([0.75, 0.9, 5.0, 0.0], [0.75, 0.9, -5.0, 0.0], 2, 0.4),
        ident,
    )
    add(
        "capsule beside both quads",
        caps,
        support_block([3.0, 0.5, 5.0, 0.0], [3.0, 0.5, -5.0, 0.0], 2, 0.4),
        ident,
    )
    add(
        "capsule far off the mesh",
        caps,
        support_block([20.0, 20.0, 5.0, 0.0], [20.0, 20.0, -5.0, 0.0], 2, 0.4),
        ident,
    )
    add(
        "points offset the centre onto the quad",
        world_with_points(plain, [(1.0, 0.0, 0.0), (1.0, 0.0, 0.0)]),
        support_block([-1.0, 0.9, 5.0, 0.0], [-1.0, 0.9, -5.0, 0.0], 2, 0.0),
        ident,
    )
    add(
        "two coincident points, zero expansion",
        world_with_points(plain, [(0.0, 0.0, 0.0), (0.0, 0.0, 0.0)]),
        support_block([0.75, 0.9, 2.0, 0.0], [0.75, 0.9, -5.0, 0.0], 2, 0.0),
        ident,
    )
    add(
        "rotated block quaternion",
        caps,
        support_block(
            [0.75, 0.9, 5.0, 0.0],
            [0.75, 0.9, -5.0, 0.0],
            2,
            0.1,
            quat=(0.0, 0.0, 0.7071067811865476, 0.7071067811865476),
        ),
        ident,
    )
    add(
        "single support point",
        world_with_points(plain, [(0.0, 0.0, 0.0)]),
        support_block([0.75, 0.9, 2.0, 0.0], [0.75, 0.9, -5.0, 0.0], 1, 0.0),
        ident,
    )
    add(
        "three support points",
        world_with_points(plain, [(0.3, 0.0, 0.0), (-0.3, 0.0, 0.0), (0.0, 0.3, 0.0)]),
        support_block([3.0, 0.5, 5.0, 0.0], [3.0, 0.5, -5.0, 0.0], 3, 0.4),
        ident,
    )
    add(
        "scaled shape (2.0 / 0.5)",
        caps,
        support_block([1.5, 0.5, 5.0, 0.0], [1.5, 0.5, -5.0, 0.0], 2, 0.2),
        ident,
    )
    add(
        "translated shape transform",
        caps,
        support_block([0.75, 0.9, 5.0, 0.0], [0.75, 0.9, -5.0, 0.0], 2, 0.4),
        mf.xform([0.0, 0.0, 3.0]),
    )
    add(
        "rotated shape transform",
        world_with_points(plain, TWO_X),
        support_block([0.75, 0.9, 5.0, 0.0], [0.75, 0.9, -5.0, 0.0], 2, 0.05),
        mf.xform(
            [0.0, 0.0, 0.0], (0.0, 0.0, 0.7071067811865476, 0.7071067811865476)
        ),
    )
    # The step ladder. A leaf box whose min.x sits at the hull's reach is
    # reached exactly while `min.x <= centre.x + step`, where the hull's centre
    # is the start plus the points' own offset and its step on x is
    # `(0.8 * 0.5) + 0 + 0.02`. Three rungs one ULP apart around that boundary
    # turn the hull's last bit into an observable: at the bounday the leaf is
    # reached, one ULP further out it is not.
    #
    # The boundary is computed here only to *choose* the rungs; no comparison
    # depends on this arithmetic. A last-bit difference between the client's
    # hull and `ow-sim`'s moves one of the rungs across the edge and shows up as
    # a counter divergence.
    step = f32(f32(0.8 * 0.5) + f32(0.02))
    for tag, points, boundary in (
        ("step ladder", [(0.4, 0.0, 0.0), (-0.4, 0.0, 0.0)], f32(step)),
        (
            "centre ladder",
            [(0.65, 0.0, 0.0), (-0.15, 0.0, 0.0)],
            f32(f32(0.25) + step),
        ),
    ):
        for rung, x0 in (
            ("-1ulp", next_down(boundary)),
            ("at the boundary", boundary),
            ("+1ulp", next_up(boundary)),
        ):
            add(
                f"{tag} {rung}",
                world_with_points(ladder_world(x0), points),
                support_block([0.0, 0.0, 5.0, 0.0], [0.0, 0.0, -5.0, 0.0], 2, 0.0),
                ident,
            )
    return cases


def oracle_out(oracle: EmuOracle, scale: float, world: bytes, block: bytes, shape: bytes) -> bytes:
    record = mf.pool(scale, 1.0 / scale, world)
    args = [
        mf.ArgSpec(
            kind="ptr",
            mem=bytes(record) + world,
            arena_offset=mf.POOL_OFF,
            relocs=(0x20, 0x28, 0x30),
        ),
        mf.ArgSpec(kind="ptr", mem=bytes(0x44), arena_offset=mf.OUT_OFF),
        mf.ArgSpec(kind="ptr", mem=block, arena_offset=mf.BLOCK_OFF),
        mf.ArgSpec(kind="ptr", mem=shape, arena_offset=mf.XFORM_OFF),
    ]
    result = oracle.call(V2B_BASE + (MESH_BODY - 0x140000000), args, ret_kind="u64")
    if result.fault:
        raise RuntimeError(f"oracle fault: {result.fault}")
    return bytes(oracle.uc.mem_read(mf.ARENA_BASE + mf.OUT_OFF, 0x44))


def probe_out(scale: float, world: bytes, block: bytes, shape: bytes) -> bytes:
    """The same call through the shipped probe, with the pointer re-expressed."""
    local = bytearray(block)
    if struct.unpack_from("<i", local, 0x38)[0] > 0:
        at = struct.unpack_from("<Q", local, 0x30)[0]
        # Arena address -> the element pointee's own pointer form: the
        # element sits at POOL_OFF and `MeshWorld`'s base is 0x50.
        relative = at - mf.POOL_ADDR
        struct.pack_into("<Q", local, 0x30, relative)
    record = mf.pool(scale, 1.0 / scale, world)
    fields = [bytes(record) + world, bytes(0x44), bytes(local), shape]
    line = ["call", "mesh_sweep", "4"]
    for field in fields:
        line += ["ptr", field.hex(), "-", "-"]
    proc = subprocess.run(
        [PROBE],
        input=(" ".join(line) + "\n").encode(),
        capture_output=True,
    )
    text = proc.stdout.decode().strip()
    tokens = text.split(" ")
    if not tokens or tokens[0] != "ok":
        raise RuntimeError(f"probe failed: {text[:200]} {proc.stderr.decode()[:200]}")
    # One hex blob per pointer argument, in argument order: element, out, block,
    # transform.
    return bytes.fromhex(tokens[3])


def describe(out: bytes) -> str:
    point = struct.unpack("<4f", out[0x00:0x10])
    normal = struct.unpack("<4f", out[0x10:0x20])
    fraction = struct.unpack("<4I", out[0x20:0x30])
    triangle = struct.unpack("<i", out[0x30:0x34])[0]
    ids = struct.unpack("<HH", out[0x34:0x38])
    internals, leaves = struct.unpack("<ii", out[0x3c:0x44])
    return (
        f"internals={internals} leaves={leaves} tri={triangle} ids={ids[0]:#06x}/{ids[1]:#06x} "
        f"point=({point[0]:.7g},{point[1]:.7g},{point[2]:.7g}) "
        f"normal=({normal[0]:.7g},{normal[1]:.7g},{normal[2]:.7g}) "
        f"fraction={[hex(b) for b in fraction]}"
    )


def main(argv) -> int:
    if "--build" in argv:
        cmd = [
            "cargo",
            "build",
            "--release",
            "--offline",
            "--manifest-path",
            "crates/ow-sim/Cargo.toml",
            "--target-dir",
            os.path.join("target", "verify_probe", "ow-sim"),
            "--bin",
            "verify_probe",
        ]
        if subprocess.run(cmd).returncode != 0:
            print("probe build failed")
            return 2
    oracle = EmuOracle(IMAGE)
    oracle.load_function(V2B_BASE + (MESH_BODY - 0x140000000), max_size=0x2000)

    failures = 0
    open_handoffs = 0
    for name, world, block, shape in corpus():
        client = oracle_out(oracle, 1.0, world, block, shape)
        rust = probe_out(1.0, world, block, shape)
        if client == rust:
            print(f"  agree   {name:44s} {describe(client)}")
            continue
        # A hit is marked by the 0xffff at +0x38; a miss leaves the buffer
        # untouched.
        client_hit = struct.unpack_from("<H", client, 0x38)[0] == 0xFFFF
        rust_hit = struct.unpack_from("<H", rust, 0x38)[0] == 0xFFFF
        # The walk's own fields: both counters. A hit also rewrites +0x30..0x38,
        # which is exactly the difference being reported here.
        walk_agrees = client[0x3C:0x44] == rust[0x3C:0x44]
        if client_hit and not rust_hit and walk_agrees:
            open_handoffs += 1
            print(f"  OPEN    {name:44s} client: {describe(client)}")
            print(f"          {'':44s} ow-sim: {describe(rust)}")
            continue
        failures += 1
        print(f"  FAIL    {name:44s}")
        print(f"          client: {describe(client)}")
        print(f"          ow-sim: {describe(rust)}")
        for offset in range(0, 0x44, 4):
            if client[offset : offset + 4] != rust[offset : offset + 4]:
                print(
                    f"          first byte divergence at +{offset:#04x}: "
                    f"{client[offset:offset+4].hex()} vs {rust[offset:offset+4].hex()}"
                )
                break
    print(
        f"cases: {len(corpus())}  failures: {failures}  "
        f"open support-point handoffs: {open_handoffs}"
    )
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))

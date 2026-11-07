"""Differential probe: the closest-point sub-simplex bodies, client vs `ow-sim`.

Runs the real bytes of `0x142cc8d80` (3-simplex) and `0x142cc7740` (4-simplex)
through `ow_sim::collide::simplex3` / `simplex4` via the shipped probe, over a
randomised corpus of simplex blocks, and compares the whole block and the
direction the routine publishes, byte for byte.

The corpus is seeded (a fixed generator, not a hand-picked list) so a
divergence is reproducible: `--seed N` re-runs a case.

Both bodies are transcribed, so a four-vertex divergence is a failure like any
other. `--only 3` / `--only 4` runs a single body.

    python dev/gjk_simplex_diff.py             # run the corpus
    python dev/gjk_simplex_diff.py --build     # rebuild the probe first
    python dev/gjk_simplex_diff.py --count 400

Read-only: nothing here writes the image, the install or the Ghidra project.
"""

from __future__ import annotations

import os
import random
import struct
import subprocess
import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "verify_harness"))

from emu_oracle import ARENA_BASE, ArgSpec, EmuOracle, V2B_BASE  # noqa: E402

IMAGE = "dev/data/offline_recover/module_offline.bin"
SIMPLEX3 = 0x142CC8D80
SIMPLEX4 = 0x142CC7740
STRIDE = 0x50
BLOCK_LEN = 0x150
OUT_OFF = 0x4100
SIM_OFF = 0x4200
PROBE = os.path.join("target", "verify_probe", "ow-sim", "release", "verify_probe.exe")


def quad(values) -> bytes:
    lanes = list(values) + [0.0] * (4 - len(values))
    return b"".join(struct.pack("<f", float(x)) for x in lanes[:4])


def build_block(vertices, count: int) -> bytes:
    rec = bytearray(BLOCK_LEN)
    for index, dw in enumerate(vertices):
        at = index * STRIDE
        rec[at + 0x00 : at + 0x10] = quad(dw)
        rec[at + 0x10 : at + 0x20] = quad([dw[0] + 0.25, dw[1] - 0.5, dw[2] + 1.0])
        rec[at + 0x20 : at + 0x30] = quad(dw)
        rec[at + 0x30 : at + 0x40] = quad([1.0, 1.0, 1.0, 1.0])
        struct.pack_into("<ii", rec, at + 0x40, index, index + 1)
    struct.pack_into("<i", rec, BLOCK_LEN - 0x10, count)
    return bytes(rec)


def describe(block: bytes, out: bytes) -> str:
    count = struct.unpack_from("<i", block, BLOCK_LEN - 0x10)[0]
    direction = struct.unpack_from("<4f", out, 0)
    parts = [f"count={count} dir={[round(v, 6) for v in direction]}"]
    for index in range(max(0, min(count, 4))):
        at = index * STRIDE
        bary = struct.unpack_from("<4f", block, at + 0x30)
        dw = struct.unpack_from("<4f", block, at)
        parts.append(f"v{index} dw={[round(v, 6) for v in dw[:3]]} bary={[round(v, 6) for v in bary]}")
    return " ".join(parts)


def oracle_run(oracle: EmuOracle, entry: int, block: bytes, out: bytes):
    args = [
        ArgSpec(kind="ptr", mem=out, arena_offset=OUT_OFF),
        ArgSpec(kind="ptr", mem=block, arena_offset=SIM_OFF),
    ]
    result = oracle.call(V2B_BASE + (entry - 0x140000000), args, ret_kind="u64")
    if result.fault:
        raise RuntimeError(f"oracle fault: {result.fault}")
    return (
        bytes(oracle.uc.mem_read(ARENA_BASE + OUT_OFF, len(out))),
        bytes(oracle.uc.mem_read(ARENA_BASE + SIM_OFF, len(block))),
        result.ret_bits & 0xFF,
    )


def probe_run(entry: str, block: bytes, out: bytes):
    line = ["call", entry, "2"]
    for field in (out, block):
        line += ["ptr", field.hex(), "-", "-"]
    proc = subprocess.run([PROBE], input=(" ".join(line) + "\n").encode(), capture_output=True)
    tokens = proc.stdout.decode().strip().split(" ")
    if not tokens or tokens[0] != "ok":
        raise RuntimeError(f"probe failed: {proc.stdout.decode()[:200]} {proc.stderr.decode()[:200]}")
    return (
        bytes.fromhex(tokens[2]),
        bytes.fromhex(tokens[3]),
        int(tokens[1], 16) if tokens[1] != "-" else 0,
    )


def corpus(seed: int, count: int):
    """A seeded walk over simplex shapes: far, straddling, degenerate, huge."""
    rng = random.Random(seed)
    out = []
    for case in range(count):
        count_vertices = 3 if case % 2 == 0 else 4
        scale = rng.choice([1e-3, 0.25, 1.0, 3.0, 1e3])
        vertices = []
        for _ in range(count_vertices):
            vertices.append(tuple(rng.uniform(-1.0, 1.0) * scale for _ in range(3)))
        shape = case % 6
        if shape == 1:  # everything on the +x side: a vertex region
            vertices = [(abs(v[0]) + 0.5, v[1], v[2]) for v in vertices]
        elif shape == 2:  # a flat simplex (all z equal): a face region
            plane = rng.uniform(-1.0, 1.0) * scale
            vertices = [(v[0], v[1], plane) for v in vertices]
        elif shape == 3:  # two coincident vertices
            vertices[1] = vertices[0]
        elif shape == 4:  # straddling the origin on every axis
            vertices = [
                (rng.uniform(0.5, 1.5) * s, rng.uniform(0.5, 1.5) * (1 if i % 2 else -1), rng.uniform(-1, 1))
                for i, (s, v) in enumerate(zip([1.0, -1.0, 1.0, -1.0], vertices))
            ]
        out.append((count_vertices, vertices))
    return out


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
    count = 60
    if "--count" in argv:
        count = int(argv[argv.index("--count") + 1])
    seed = 7
    if "--seed" in argv:
        seed = int(argv[argv.index("--seed") + 1])

    oracle = EmuOracle(IMAGE)
    oracle.load_function(V2B_BASE + (SIMPLEX3 - 0x140000000), max_size=0x800)
    oracle.load_function(V2B_BASE + (SIMPLEX4 - 0x140000000), max_size=0x1700)

    only = 0
    if "--only" in argv:
        only = int(argv[argv.index("--only") + 1])

    failures = 0
    tested = 0
    for index, (count_vertices, vertices) in enumerate(corpus(seed, count)):
        if only and count_vertices != only:
            continue
        tested += 1
        block = build_block(vertices, count_vertices)
        out = bytearray(0x40)
        entry = SIMPLEX3 if count_vertices == 3 else SIMPLEX4
        name = "gjk_simplex3" if count_vertices == 3 else "gjk_simplex4"
        client_out, client_block, client_ret = oracle_run(oracle, entry, block, bytes(out))
        rust_out, rust_block, rust_ret = probe_run(name, block, bytes(out))
        if client_out == rust_out and client_block == rust_block and client_ret == rust_ret:
            continue
        failures += 1
        print(f"  FAIL case {index} ({name}) vertices={vertices}")
        print(f"          client ret={client_ret} {describe(client_block, client_out)}")
        print(f"          ow-sim ret={rust_ret} {describe(rust_block, rust_out)}")
        for at in range(BLOCK_LEN):
            if client_block[at] != rust_block[at]:
                print(f"          first simplex divergence at +{at:#04x}")
                break
        else:
            print("          simplex blocks agree; the direction differs")
    print(f"cases: {tested}  failures: {failures}")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))

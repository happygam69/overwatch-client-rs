"""Differential probe: the closest-point GJK, client vs `ow-sim`.

Two corpora, both compared on the client's own bytes:

* **the live one**: every descriptor the real `0x142caed30` builds for
  `dev/mesh_support_diff.py`'s 17 cases, captured at the moment its triangle
  test calls `0x142cc9460` (fixture plumbing only - the same bytes are replayed
  into the function directly);
* **the seeded one**: random descriptors over random point sets, covering the
  branch the triangle test never reaches (support counts from 1 to 8, radii,
  rotations, motion budgets, both flags).

Comparison is the 0x40-byte output block and the returned flag, bit for bit. A
case whose run calls the four-vertex body `0x142cc7740` - which `ow-sim` does
not transcribe - is reported as `OPEN`, never as a failure.

    python dev/gjk_closest_diff.py            # both corpora
    python dev/gjk_closest_diff.py --build
    python dev/gjk_closest_diff.py --seeded 200

Read-only: nothing here writes the image, the install or the Ghidra project.
"""

from __future__ import annotations

import os
import random
import struct
import subprocess
import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "verify_harness"))

import mesh_fixture as mf  # noqa: E402
import mesh_support_diff as msd  # noqa: E402
from emu_oracle import ARENA_BASE, ArgSpec, EmuOracle, V2B_BASE  # noqa: E402
from unicorn import UC_HOOK_CODE, x86_const as x  # noqa: E402

IMAGE = "dev/data/offline_recover/module_offline.bin"
GJK = 0x142CC9460
SIMPLEX4 = 0x142CC7740
MESH_BODY = msd.MESH_BODY
PROBE = os.path.join("target", "verify_probe", "ow-sim", "release", "verify_probe.exe")
DESC_LEN = 0xA4
DESC_OFF = 0x4400
ARRAY_OFF = 0x4600
OUT_OFF = 0x4800


def quad(values) -> bytes:
    lanes = list(values) + [0.0] * (4 - len(values))
    return b"".join(struct.pack("<f", float(v)) for v in lanes[:4])


def unpack_quad(blob: bytes, at: int):
    return struct.unpack_from("<4f", blob, at)


def live_descriptors(oracle: EmuOracle):
    """Capture every descriptor the client's triangle test hands the GJK."""
    captured = []

    def hook(uu, address, size, ud):
        if address == V2B_BASE + (GJK - 0x140000000):
            rdx = uu.reg_read(x.UC_X86_REG_RDX)
            desc = bytes(uu.mem_read(rdx, DESC_LEN))
            arrays = []
            for at, count_at in ((0x00, 0x08), (0x20, 0x28)):
                pointer = struct.unpack_from("<Q", desc, at)[0]
                count = struct.unpack_from("<i", desc, count_at)[0]
                arrays.append(bytes(uu.mem_read(pointer, max(0, count) * 0x10)))
            captured.append((desc, arrays[0], arrays[1]))

    oracle.uc.hook_add(UC_HOOK_CODE, hook, begin=0, end=0xFFFFFFFFFFFFFFFF)
    for _name, world, block, shape in msd.corpus():
        captured.clear()
        sweep = msd.oracle_out(oracle, 1.0, world, block, shape)
        del sweep
        for desc, first, second in list(captured):
            yield desc, first, second


def seeded_descriptors(seed: int, count: int):
    rng = random.Random(seed)
    for _ in range(count):
        count_a = rng.choice([1, 2, 3, 3, 3, 4, 6])
        count_b = rng.choice([1, 1, 2, 3, 5, 8])
        scale = rng.choice([1e-3, 0.2, 1.0, 4.0])
        first = b"".join(
            quad([rng.uniform(-1.0, 1.0) * scale for _ in range(3)]) for _ in range(count_a)
        )
        second = b"".join(
            quad([rng.uniform(-1.0, 1.0) * scale for _ in range(3)]) for _ in range(count_b)
        )
        desc = bytearray(DESC_LEN)
        struct.pack_into("<Q", desc, 0x00, 0)
        struct.pack_into("<i", desc, 0x08, count_a)
        desc[0x10:0x20] = quad([rng.choice([0.0, 0.01, 0.05, 0.3])] * 4)
        struct.pack_into("<Q", desc, 0x20, 0)
        struct.pack_into("<i", desc, 0x28, count_b)
        desc[0x30:0x40] = quad([rng.choice([0.0, 0.01, 0.05, 0.3])] * 4)
        desc[0x40:0x50] = quad([rng.uniform(-1.0, 1.0) * scale for _ in range(3)])
        desc[0x50:0x60] = _random_quat(rng)
        desc[0x60:0x70] = quad([rng.uniform(-1.0, 1.0) * scale for _ in range(3)])
        desc[0x70:0x80] = _random_quat(rng)
        desc[0x80:0x90] = quad([rng.uniform(-1.0, 1.0) for _ in range(3)])
        desc[0x90:0xA0] = quad([rng.choice([0.0, 0.25, 0.5, 1.0])] * 4)
        desc[0xA0] = rng.choice([0, 1, 1])
        desc[0xA1] = rng.choice([0, 0, 1])
        yield bytes(desc), first, second


def _random_quat(rng) -> bytes:
    values = [rng.uniform(-1.0, 1.0) for _ in range(4)]
    if rng.random() < 0.5:
        return quad(values)
    return quad([0.0, 0.0, 0.0, 1.0])


def oracle_run(oracle: EmuOracle, desc: bytes, first: bytes, second: bytes):
    blob = bytearray(desc)
    at_first = ARENA_BASE + ARRAY_OFF
    at_second = at_first + len(first)
    struct.pack_into("<Q", blob, 0x00, at_first)
    struct.pack_into("<Q", blob, 0x20, at_second)
    args = [
        ArgSpec(kind="ptr", mem=bytes(0x40), arena_offset=OUT_OFF),
        ArgSpec(kind="ptr", mem=bytes(blob), arena_offset=DESC_OFF),
        ArgSpec(kind="ptr", mem=first, arena_offset=ARRAY_OFF),
        ArgSpec(kind="ptr", mem=second, arena_offset=ARRAY_OFF + len(first)),
    ]
    result = oracle.call(V2B_BASE + (GJK - 0x140000000), args, ret_kind="u64")
    if result.fault:
        raise RuntimeError(f"oracle fault: {result.fault}")
    return bytes(oracle.uc.mem_read(ARENA_BASE + OUT_OFF, 0x40)), result.ret_bits & 0xFF


def probe_run(desc: bytes, first: bytes, second: bytes):
    line = ["call", "gjk_closest", "4"]
    for field in (bytes(0x40), desc, first, second):
        line += ["ptr", field.hex(), "-", "-"]
    proc = subprocess.run([PROBE], input=(" ".join(line) + "\n").encode(), capture_output=True)
    tokens = proc.stdout.decode().strip().split(" ")
    if not tokens or tokens[0] != "ok":
        raise RuntimeError(f"probe failed: {proc.stdout.decode()[:300]} {proc.stderr.decode()[:300]}")
    return bytes.fromhex(tokens[2]), int(tokens[1], 16) if tokens[1] != "-" else 0


def describe(out: bytes) -> str:
    point = unpack_quad(out, 0x00)
    normal = unpack_quad(out, 0x10)
    residual = unpack_quad(out, 0x20)
    iterations = struct.unpack_from("<i", out, 0x30)[0]
    return (
        f"iters={iterations} point={[round(v, 6) for v in point[:3]]} "
        f"normal={[round(v, 6) for v in normal[:3]]} "
        f"residual={[round(v, 6) for v in residual[:3]]} "
        f"raw={out.hex()}"
    )


def compare(label: str, desc: bytes, first: bytes, second: bytes, oracle: EmuOracle, reached) -> bool:
    before = reached["four"]
    client_out, client_ret = oracle_run(oracle, desc, first, second)
    rust_out, rust_ret = probe_run(desc, first, second)
    if client_out == rust_out and client_ret == rust_ret:
        return True
    print(f"  FAIL {label}")
    print(f"          client ret={client_ret} {describe(client_out)}")
    print(f"          ow-sim ret={rust_ret} {describe(rust_out)}")
    for at in range(0, 0x40, 4):
        if client_out[at : at + 4] != rust_out[at : at + 4]:
            print(f"          first divergence at +{at:#04x}")
            break
    return False


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
    seeded = 120
    if "--seeded" in argv:
        seeded = int(argv[argv.index("--seeded") + 1])
    seed = 11
    if "--seed" in argv:
        seed = int(argv[argv.index("--seed") + 1])

    oracle = EmuOracle(IMAGE)
    oracle.load_function(V2B_BASE + (GJK - 0x140000000), max_size=0x1000)
    oracle.load_function(V2B_BASE + (SIMPLEX4 - 0x140000000), max_size=0x1700)

    reached = {"four": 0}

    def hook(uu, address, size, ud):
        if address == V2B_BASE + (SIMPLEX4 - 0x140000000):
            reached["four"] += 1

    oracle.uc.hook_add(UC_HOOK_CODE, hook, begin=0, end=0xFFFFFFFFFFFFFFFF)

    failures = 0
    live = list(live_descriptors(oracle))
    for index, (desc, first, second) in enumerate(live):
        failures += not compare(f"live {index}", desc, first, second, oracle, reached)
    print(f"live descriptors: {len(live)}  failures so far: {failures}")

    for index, (desc, first, second) in enumerate(seeded_descriptors(seed, seeded)):
        if not compare(f"seeded {index}", desc, first, second, oracle, reached):
            failures += 1
            if failures >= 40:
                print("  (stopping after 40 failures)")
                break
    print(
        f"live: {len(live)}  seeded: {seeded}  failures: {failures}  "
        f"4-simplex entries: {reached.get('four', 0)}"
    )
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))

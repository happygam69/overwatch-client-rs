"""Oracle for the brush's vertex decode, and a probe of the brush builder.

    python dev/brush_vertex_oracle.py            # the pinned cases
    python dev/brush_vertex_oracle.py --try-build # also try 0x142ced620

`FUN_142ced510` (`0x142ced510`) is the brush vertex decoder: it turns the
`0x20`-byte quantised vertex record into a `vec4` position. It takes one
pointer out and one pointer in and allocates nothing, so the oracle can run it
directly and the Rust port (`ow_tank::brush::vertex_position`) is compared
against the client's own bytes.

The builder `FUN_142ced620` allocates its two arrays through the image's
allocator hook (`0x142cadee0`, whose first word is a global function pointer),
so it can only run when that hook resolves; `--try-build` reports what happens
rather than assuming either way.

Nothing here is a gameplay claim and nothing is written to the image or the
install.
"""

from __future__ import annotations

import argparse
import os
import struct
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "verify_harness"))

from emu_oracle import ARENA_BASE, EmuOracle, V2B_BASE  # noqa: E402

# The image's allocator/free hooks: `0x142cadee0` and `0x142cadf00` tail-call a
# global function pointer when it is set, so a probe can install a stub. The
# stub region starts at the allocator hook word and holds both hook words, the
# bump pointer and the code, so it is one declared state region.
HOOK_RVA = 0x1442A0158
HEAP_BASE = ARENA_BASE + 0x8000


def _stub(cur_word: int) -> bytes:
    """A bump allocator: (size) -> aligned base, advancing the `CUR_WORD`."""
    return (
        b"\x48\xa1" + struct.pack("<Q", cur_word)   # mov rax, [cur]
        + b"\x48\x83\xc0\x0f"                        # add rax, 15
        + b"\x48\x83\xe0\xf0"                        # and rax, -16
        + b"\x48\x89\xc2"                            # mov rdx, rax
        + b"\x48\x03\xc1"                            # add rax, rcx
        + b"\x48\xa3" + struct.pack("<Q", cur_word)  # mov [cur], rax
        + b"\x48\x89\xd0"                            # mov rax, rdx
        + b"\xc3"                                    # ret
    )


def hook_state(n_bytes: int = 0x100) -> bytes:
    """The state snapshot: both hook words, the bump pointer and the stubs.

    The image is mapped at `V2B_BASE` under the emulator, so every address in
    here is the emulated one, not the current-image one.
    """
    region = v2b(HOOK_RVA)
    alloc = region + 0xA8
    free = region + 0xE8
    cur = region + 0x10
    state = bytearray(n_bytes)
    struct.pack_into("<Q", state, 0x00, alloc)
    struct.pack_into("<Q", state, 0x08, free)
    struct.pack_into("<Q", state, 0x10, HEAP_BASE)
    off = alloc - region
    code = _stub(cur)
    state[off : off + len(code)] = code
    state[free - region] = 0xC3
    return bytes(state)

IMAGE = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
    "dev", "data", "offline_recover", "module_offline.bin",
)
IMAGE_BASE = 0x140000000

VERTEX_DECODE = 0x142CED510
BRUSH_BUILD = 0x142CED620

OUT_OFF = 0x4000
VERTEX_OFF = 0x4100

# (x, y, divisor) triples. The first three are the shapes the builder emits
# (`divisor = 1`); the rest pin the integer-division decomposition that the
# expression uses for a general divisor, including its negative and
# out-of-range corners.
CASES = [
    (0, 0, 1),
    (1, 0, 1),
    (-1, 0, 1),
    (400, -400, 1),
    (1000, -1000, 1),
    (0x7FFFFFFF, -0x80000000, 1),
    (5, 7, 400),
    (-5, -7, 400),
    (1000, -1000, 400),
    (0x7FFFFFFF, -0x80000000, 400),
    (123456789, -987654321, 3),
]


def v2b(addr: int) -> int:
    return V2B_BASE + (addr - IMAGE_BASE)


def vertex_bytes(x: int, y: int, d: int, index: int = 0) -> bytes:
    out = bytearray(0x20)
    struct.pack_into("<q", out, 0, x)
    struct.pack_into("<q", out, 8, y)
    struct.pack_into("<q", out, 0x10, d)
    struct.pack_into("<I", out, 0x18, index)
    return bytes(out)


def decode(oracle: EmuOracle, x: int, y: int, d: int):
    from emu_oracle import ArgSpec

    args = [
        ArgSpec(kind="ptr", mem=bytes(16), arena_offset=OUT_OFF),
        ArgSpec(kind="ptr", mem=vertex_bytes(x, y, d), arena_offset=VERTEX_OFF),
    ]
    res = oracle.call(v2b(VERTEX_DECODE), args, ret_kind="void")
    if not res.ok:
        raise SystemExit(f"oracle fault on {(x, y, d)}: {res.fault}")
    out = res.pointee_after[:16]
    return struct.unpack("<4f", out), struct.unpack("<4I", out)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--try-build", action="store_true")
    opts = ap.parse_args()

    oracle = EmuOracle(IMAGE)
    print("vertex decode 0x142ced510:")
    for x, y, d in CASES:
        floats, bits = decode(oracle, x, y, d)
        print(
            f"  ({x:>12}, {y:>12}, d={d:>4}) -> "
            f"({floats[0]!r}, {floats[1]!r}, {floats[2]!r}, {floats[3]!r})  "
            f"bits {[hex(b) for b in bits]}"
        )

    if opts.try_build:
        print()
        print("brush builder 0x142ced620 (with a stubbed allocator):")
        oracle.set_state(v2b(HOOK_RVA), hook_state())
        for points, scales, label in BUILD_CASES:
            build(oracle, points, scales, label)
    return 0


# (point list, (sx, sy), label). The first is a square; the second is the same
# square wound the other way; the third is a triangle; the fourth repeats the
# last point so the ordering pass must reject it.
BUILD_CASES = [
    ([(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)], (1.0, 1.0), "square ccw"),
    ([(0.0, 1.0), (1.0, 1.0), (1.0, 0.0), (0.0, 0.0)], (1.0, 1.0), "square cw"),
    ([(0.0, 0.0), (2.0, 0.0), (0.5, 1.5)], (1.0, 1.0), "triangle"),
    ([(0.0, 0.0), (0.0, 0.0), (1.0, 1.0)], (1.0, 1.0), "degenerate"),
    ([(0.0, 0.0), (0.25, 0.0), (0.25, 0.25), (0.0, 0.25)], (0.5, 2.0), "scaled"),
    # Halfway values pin the rounding mode: 0.00125*400 = 0.5 exactly.
    ([(0.0, 0.0), (0.00125, 0.0), (0.00375, 0.00125)], (1.0, 1.0), "round half up"),
    ([(0.0, 0.0), (-0.00125, 0.0), (-0.00375, -0.00125)], (1.0, 1.0), "round half down"),
]


def build(oracle: EmuOracle, points, scales, label: str) -> None:
    from emu_oracle import ArgSpec

    n = len(points)
    scales_pointee = struct.pack("<4f", scales[0], scales[1], 0.0, 0.0)
    instances = b"".join(struct.pack("<4f", p[0], p[1], 0.0, 0.0) for p in points)
    heap = n * 0x20 + n * 0x28 + 0x40
    args = [
        ArgSpec(kind="ptr", mem=bytes(0x20), arena_offset=0x4200),
        ArgSpec(kind="ptr", mem=instances, arena_offset=0x4400),
        ArgSpec(kind="u64", bits=n),
        ArgSpec(kind="ptr", mem=scales_pointee, arena_offset=0x4500),
        # A fifth (stack) argument nobody reads: it declares the region the
        # stubbed allocator hands out, so `pointee_after` returns the arrays
        # the builder wrote. The arena write hook reports the bytes *before*
        # each store, so it cannot be used to rebuild the result.
        ArgSpec(kind="ptr", mem=bytes(heap), arena_offset=0x8000),
    ]
    res = oracle.call(v2b(BRUSH_BUILD), args, ret_kind="u64")
    print(f"  {label}: n={n} ok={res.ok} fault={res.fault} ret={res.ret_bits:#x}")
    # `pointee_after` is the pointees of the pointer arguments in order.
    nosee = 0x20 + len(instances) + len(scales_pointee)
    brush = res.pointee_after[:0x20]
    verts, nodes, vc, nc, root = struct.unpack_from("<QQIII", brush, 0)
    print(f"    brush header: verts={verts:#x} nodes={nodes:#x} vc={vc} nc={nc} root={root}")
    heap_bytes = res.pointee_after[nosee : nosee + heap]
    for base, count, stride, name in (
        (verts, vc, 0x20, "vertex"),
        (nodes, nc, 0x28, "node"),
    ):
        if base == 0:
            print(f"    {name}s: free")
            continue
        at = base - HEAP_BASE
        for i in range(count):
            off = at + i * stride
            row = heap_bytes[off : off + stride]
            if name == "vertex":
                print(
                    f"    vertex {i}: x={struct.unpack_from('<q', row, 0)[0]} "
                    f"y={struct.unpack_from('<q', row, 8)[0]} "
                    f"d={struct.unpack_from('<q', row, 0x10)[0]} "
                    f"index={struct.unpack_from('<I', row, 0x18)[0]}"
                )
            else:
                prev, nxt, vi, minus1, child = struct.unpack_from("<5i", row, 0)
                x, y, px, py = struct.unpack_from("<4i", row, 0x14)
                print(
                    f"    node {i}: prev={prev} next={nxt} vertex={vi} "
                    f"[{minus1} {child}] xy=({x},{y}) prev_xy=({px},{py}) "
                    f"one={row[0x24]} open={row[0x25]}"
                )


if __name__ == "__main__":
    raise SystemExit(main())

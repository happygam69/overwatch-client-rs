"""Inputs for the `collision_transform` unit (`0x142cb8b80`).

Five pointers, all pinned so the record can name the two pools by absolute
address (`ARENA_BASE` is fixed by `emu_oracle`):

* `entry`  - the `0xc0`-byte shape entry. `+0x50` is the pool slot (signed
  `i32`) and `+0xb0` the attach state: `1` reads the owner pose, anything else
  the owner pool.
* `out`    - the `0x20`-byte output: the position quad then the quaternion.
* `record` - the world. `+0x288` points at the pose array, `+0x2a0` at the
  owner pool, and `+0x191`/`+0x192` select the pose's composed branch.
* `owners` - the owner pool, `0x50` per element: quaternion at `+0x00`,
  position at `+0x10`.
* `poses`  - the pose array, `0x1c0` per element.

The cases walk every branch the leaf has: the two attach states, both
`+0x191`/`+0x192` combinations, every slot, and the composed branch on
quaternions that are unnormalised, zero-length, and sign-ambiguous against the
reference. Non-finite inputs are restricted to NaN, and only in the two fields
that propagate it rather than driving it into an invalid operation; the module
comment on `NAN` records why.
"""

from __future__ import annotations

import random
import struct
from typing import List

from emu_oracle import ARENA_BASE, ArgSpec

ENTRY_OFF = 0x4000
OUT_OFF = 0x4100
RECORD_OFF = 0x4200
OWNERS_OFF = 0x5000
POSES_OFF = 0x6000

ENTRY_LEN = 0xC0
OUT_LEN = 0x20
RECORD_LEN = 0x2A8
OWNER_STRIDE = 0x50
POSE_STRIDE = 0x1C0
SLOTS = 4

# Floats worth pinning in the pose fields: the normal range, the sign
# boundary, and the degenerate lengths the normaliser divides by.
FINITE = [
    0.0,
    -0.0,
    1.0,
    -1.0,
    0.5,
    -0.5,
    0.70710678,
    1e-8,
    1e8,
    -1e8,
]

# One NaN, canonical positive. The emulator's softfloat is not faithful to
# x86 in two ways that a richer NaN corpus would expose, so the corpus does
# not depend on either:
#   * an *invalid* operation (`inf * 0`, which the `(origin - pivot) * 0` term
#     reaches for an infinite origin) answers `0x7fc00000` in the emulator and
#     `0xffc00000` on hardware (the x86 QNaN indefinite);
#   * when both operands of an ADDPS/SUBPS are NaN the emulator does not always
#     return the destination's NaN the way hardware does, so two NaN operands
#     of different bits (which `negate` produces from a NaN reference) disagree.
# NaN is therefore confined to the quaternion, where every operand it meets is
# either the same canonical value or finite.
NAN = [float("nan")]


def _random_quad(rng: random.Random, pool: list) -> bytes:
    out = bytearray()
    for _ in range(4):
        if rng.random() < 0.35:
            out += struct.pack("<f", rng.choice(pool))
        else:
            out += struct.pack("<f", rng.uniform(-2.0, 2.0))
    return bytes(out)


def _case(state: int, slot: int, mode_191: int, mode_192: int, composed: bool, seed: int):
    """One case: (attach state, slot, the two record bytes, compose?, rng seed)."""
    return (state, slot, mode_191, mode_192, composed, seed)


def _cases():
    # The pool branch is taken by every state but 1, so pin 0 and 2 as well.
    cases = []
    for slot in range(SLOTS):
        for state in (0, 2, 1):
            for mode_191, mode_192 in ((0, 0), (1, 0), (1, 1), (0, 1)):
                # Only state 1 with +0x191 set and +0x192 clear composes.
                composed = state == 1 and mode_191 != 0 and mode_192 == 0
                cases.append(_case(state, slot, mode_191, mode_192, composed, slot * 31 + state))
    # A dense random corpus on top, so the composed arithmetic sees wide input.
    rng = random.Random(20261101)
    for i in range(240):
        state = 1 if i % 3 else (0 if i % 2 else 2)
        slot = rng.randrange(SLOTS)
        composed = state == 1
        cases.append(
            _case(state, slot, 1 if composed else 0, 0, composed, rng.randrange(1 << 30))
        )
    return cases


CASES = _cases()


def _pools(seed: int):
    rng = random.Random(seed)
    owners = bytearray(OWNER_STRIDE * SLOTS)
    for slot in range(SLOTS):
        base = slot * OWNER_STRIDE
        owners[base : base + 0x10] = _random_quad(rng, FINITE + NAN)
        owners[base + 0x10 : base + 0x20] = _random_quad(rng, FINITE + NAN)
    poses = bytearray(POSE_STRIDE * SLOTS)
    for slot in range(SLOTS):
        base = slot * POSE_STRIDE
        # +0x10 position, +0x20 quaternion (the verbatim copy's two quads).
        poses[base + 0x10 : base + 0x20] = _random_quad(rng, FINITE + NAN)
        poses[base + 0x20 : base + 0x30] = _random_quad(rng, FINITE + NAN)
        # The composed branch. Only the quaternion may be NaN (see NAN above).
        for offset in (0x50, 0x70, 0x80):
            poses[base + offset : base + offset + 0x10] = _random_quad(rng, FINITE)
        poses[base + 0x60 : base + 0x70] = _random_quad(rng, FINITE)
        poses[base + 0x90 : base + 0xA0] = _random_quad(rng, FINITE + NAN)
    return bytes(owners), bytes(poses)


def make_args(i: int) -> List[ArgSpec]:
    state, slot, mode_191, mode_192, _composed, seed = CASES[i % len(CASES)]
    owners, poses = _pools(seed)

    entry = bytearray(ENTRY_LEN)
    struct.pack_into("<i", entry, 0x50, slot)
    entry[0xB0] = state

    record = bytearray(RECORD_LEN)
    struct.pack_into("<Q", record, 0x288, ARENA_BASE + POSES_OFF)
    struct.pack_into("<Q", record, 0x2A0, ARENA_BASE + OWNERS_OFF)
    record[0x191] = mode_191
    record[0x192] = mode_192

    return [
        ArgSpec(kind="ptr", mem=bytes(entry), arena_offset=ENTRY_OFF),
        ArgSpec(kind="ptr", mem=bytes(OUT_LEN), arena_offset=OUT_OFF),
        ArgSpec(kind="ptr", mem=bytes(record), arena_offset=RECORD_OFF),
        ArgSpec(kind="ptr", mem=owners, arena_offset=OWNERS_OFF),
        ArgSpec(kind="ptr", mem=poses, arena_offset=POSES_OFF),
    ]

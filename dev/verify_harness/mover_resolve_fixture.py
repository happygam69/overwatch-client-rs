"""Inputs for the `mover_resolve_vector` unit (`0x140cb2220`).

The body is pinned in the arena. Its `+0x870` pointer names the 0x38-byte
record array and `+0x878` the count; `+0xe4` is the frame id the windows are
tested against, `+0x60c` the take-off lane (the lateral cap here), `+0xc0` the
direction the world lane is measured along and `+0x1150` the local vector.

`+0x968` (the 0x40-byte record count) is pinned to zero: that is what keeps the
client out of phase 5, so the whole remaining path - phases 1-4 and the closing
landing test - is compared. The state file word carries bit 24 (`0x1000000`), so
both transform helpers take their direct-copy path.
"""

from __future__ import annotations

import random
import struct
from typing import List

from emu_oracle import ARENA_BASE, ArgSpec

BODY_LEN = 0x1C00
REC_OFF = 0x2000
REC_LEN = 0x38

F32 = struct.unpack("<f", bytes.fromhex("0100807f"))[0]  # +NaN (quiet, positive)
INF = float("inf")

WORLD_SPACE = 0x0100_0000

# A `mov eax, 2 ; ret` stub and the fake vtable that points at it, carried
# inside the body itself (it is 0x1c00 bytes and the resolve touches none of
# this page), so a case can enter the phase-5 gate without extra arguments:
# `(*body->vtable)[0xa8](body)` returns 2, which is the client's "skip" value.
STUB_AT = 0x1A00
VT_AT = 0x1B00
REC40_AT = 0x1A40
# `mov eax, n ; ret`: n = 2 is the client's "skip the record passes" value.
STUB_2 = bytes.fromhex("b802000000c3")
STUB_0 = bytes.fromhex("b800000000c3")

# Case: (frame_id, vector, cap, direction, records, flags_extra, n40)
# records are (window_lo, window_hi, property, op, value, bound); n40 is the
# 0x40-byte record count, and 1 means "enter the phase-5 gate".
UP = (0.0, 1.0, 0.0, 0.0)
SIDE = (1.0, 0.0, 0.0, 0.0)


def _cases():
    # The discriminating cases come first: the mutation corpus only probes the
    # leading cases, so each phase, each op and both `0x25` outcomes appear there.
    # Case: (frame, vector, cap, direction, records, flags_extra, n40).

    def rec(prop, op, value, bound=0.0, lo=0, hi=8):
        return [(lo, hi, prop, op, value, bound)]

    no_rec = rec(0x14, 2, 9.0)  # a record that never matches

    return [
        # Zero records: the client returns before touching anything.
        (0, (1.0, 2.0, 3.0, 0.0), 6.0, UP, [], 0, 0),
        # The phase-5 gate reached with `n40 > 0` and the virtual answering 2,
        # so both gate sites execute and the passes are skipped.
        (0, (1.0, 2.0, 3.0, 0.0), 6.0, UP, rec(0x24, 0, 0.25), 0, 1),
        (0, (1.0, 2.0, 3.0, 0.0), 0.0, UP, no_rec, 0, 1),
        # Zero 0x38 records with the 0x40 count non-zero: the early return is
        # not taken and the stage runs on an empty record array.
        (0, (1.0, 2.0, 3.0, 0.0), 6.0, UP, [], 0, 1),
        # The gate opens (`eax != 2`) and both passes run over a record that
        # matches nothing, so they are no-ops the candidate need not model.
        (0, (1.0, 2.0, 3.0, 0.0), 6.0, UP, rec(0x24, 0, 0.25), 0, 2),
        (0, (1.0, 2.0, 3.0, 0.0), 0.0, UP, no_rec, 0, 2),
        # The guards exactly at their boundaries, where `>` and `>=` differ:
        # `x*x` is exactly the squared guard (`x = 1.25 * 2^-60`), and a lane of
        # `GUARD_LEN` squares to the value whose Newton root is exactly
        # `GUARD_LEN` - so `len > GUARD_LEN` is false on both stages.
        (0, (1.0842021724855044e-18, 0.0, 0.0, 0.0), 0.0, UP, no_rec, 0, 0),
        # The `len > GUARD_LEN` boundary with a record that *would* rescale, so
        # a `>` to `>=` flip is observable: the len is exactly the guard, so the
        # unmutated path skips and the mutant divides by it.
        (0, (1.1920928955078125e-05, 0.0, 0.0, 0.0), 0.0, UP, rec(0x22, 2, 2.0), 0, 0),
        (0, (1.1920928955078125e-05, 5.0, 0.0, 0.0), 0.0, UP, rec(0x23, 2, 2.0), 0, 0),
        # A squared length above the first guard but a root below the second, so
        # the two guards take opposite edges (phase 1, then phase 2).
        (0, (1e-6, 0.0, 0.0, 0.0), 0.0, UP, no_rec, 0, 0),
        (0, (1e-6, 5.0, 0.0, 0.0), 0.0, UP, no_rec, 0, 0),
        # No matching record: the phases are exact no-ops.
        (0, (1.0, 2.0, 3.0, 0.0), 6.0, UP, no_rec, 0, 0),
        # Property 0x22: the scale is `result / length`, every op.
        # The leading values are a length of 3 exactly, so the ratio is visible.
        (0, (3.0, 0.0, 0.0, 0.0), 0.0, UP, rec(0x22, 2, 2.0), 0, 0),
        (0, (3.0, 0.0, 0.0, 0.0), 0.0, UP, rec(0x22, 1, 2.0), 0, 0),
        (0, (3.0, 0.0, 0.0, 0.0), 0.0, UP, rec(0x22, 0, 1.0), 0, 0),
        (0, (3.0, 0.0, 0.0, 0.0), 0.0, UP, rec(0x22, 3, 1.0), 0, 0),
        (0, (3.0, 0.0, 0.0, 0.0), 0.0, UP, rec(0x22, 4, 1.0), 0, 0),
        (0, (3.0, 0.0, 0.0, 0.0), 0.0, UP, rec(0x22, 5, 1.0, 2.0), 0, 0),
        (0, (3.0, 0.0, 0.0, 0.0), 0.0, UP, rec(0x22, 6, 1.0), 0, 0),
        # Phase 1 scales all four lanes, including w.
        (0, (3.0, 0.0, 0.0, 4.0), 0.0, UP, rec(0x22, 2, 1.0), 0, 0),
        # Property 0x23 masks the y lane out of the length AND out of the lanes
        # it scales; y and w keep their phase-1 values.
        (0, (1.0, 2.0, 3.0, 4.0), 0.0, UP, rec(0x23, 2, 1.0), 0, 0),
        (0, (3.0, 0.0, 0.0, 4.0), 0.0, UP, rec(0x23, 2, 1.0), 0, 0),
        (0, (1.0, 2.0, 0.0, 9.0), 0.0, UP, rec(0x23, 1, 2.0), 0, 0),
        (0, (0.0, 1.0, 1.0, 0.0), 0.0, UP, rec(0x23, 2, 1.0), 0, 0),
        (0, (3.0, 4.0, 0.0, 0.0), 0.0, UP, rec(0x22, 2, 1.0) + rec(0x23, 1, 2.0), 0, 0),
        # Property 0x24 rewrites the y lane alone.
        (0, (1.0, 2.0, 3.0, 4.0), 0.0, UP, rec(0x24, 0, 1.0), 0, 0),
        (0, (1.0, 2.0, 3.0, 4.0), 0.0, UP, rec(0x24, 2, -0.5), 0, 0),
        # Property 0x25 acts on `y / cap` and only when it applied.
        (0, (1.0, 3.0, 2.0, 4.0), 6.0, UP, rec(0x25, 0, 1.0), 0, 0),
        (0, (1.0, 3.0, 2.0, 4.0), 6.0, UP, rec(0x25, 6, 1.0), 0, 0),
        (0, (1.0, 3.0, 2.0, 4.0), 6.0, UP, rec(0x25, 2, 0.0), 0, 0),
        # The cap guard: zero, below the guard, exactly the guard.
        (0, (1.0, 3.0, 2.0, 4.0), 0.0, UP, rec(0x25, 0, 1.0), 0, 0),
        (0, (1.0, 3.0, 2.0, 4.0), 1e-7, UP, rec(0x25, 0, 1.0), 0, 0),
        (0, (1.0, 3.0, 2.0, 4.0), 1.1920928955078125e-05, UP, rec(0x25, 0, 1.0), 0, 0),
        # A vector below the squared-length guard is never rescaled at all.
        (0, (1e-30, 0.0, 0.0, 0.0), 0.0, UP, rec(0x22, 2, 5.0), 0, 0),
        # The landing test: a rise above 0.004 along the direction, a rise below
        # it, and a fall.
        (0, (0.0, 1.0, 0.0, 0.0), 0.0, UP, rec(0x24, 0, 0.005), 0, 0),
        (0, (0.0, 1.0, 0.0, 0.0), 0.0, UP, rec(0x24, 0, 0.003), 0, 0),
        (0, (0.0, 1.0, 0.0, 0.0), 0.0, UP, rec(0x24, 0, -0.5), 0, 0),
        # The direction decides which lane the rise is measured on.
        (0, (1.0, 0.0, 0.0, 0.0), 0.0, SIDE, rec(0x22, 2, 2.0), 0, 0),
        # A state file word with the take-off bits already set, so the landing
        # transition takes its early return or its clear path.
        (0, (0.0, 1.0, 0.0, 0.0), 0.0, UP, rec(0x24, 0, 1.0), 0x0000_000c, 0),
        (0, (0.0, 1.0, 0.0, 0.0), 0.0, UP, rec(0x24, 0, 1.0), 0x0080_0000, 0),
        (0, (0.0, 1.0, 0.0, 0.0), 0.0, UP, rec(0x24, 0, 1.0), 0x0000_0008, 0),
    ]


CASES = _cases()
EDGE_POOL = [
    (0, (F32, 1.0, 1.0, 0.0), 6.0, UP, [(0, 8, 0x22, 0, 1.0, 0.0)], 0),
    (0, (1.0, INF, 0.0, 0.0), 6.0, UP, [(0, 8, 0x24, 0, 1.0, 0.0)], 0),
    (0, (-1.0, 2.0, 3.0, 4.0), 0.25, UP, [(0, 8, 0x25, 1, 2.0, 0.0)], 0),
    (3, (0.0, 1.0, 0.0, 0.0), 0.0, UP, [(3, 4, 0x24, 0, 0.01, 0.0)], 0),
    (3, (0.0, 1.0, 0.0, 0.0), 0.0, UP, [(4, 5, 0x24, 0, 0.01, 0.0)], 0),
]


def build_body(case, i: int) -> bytes:
    frame, vector, cap, direction, records, flags, n40 = case
    rng = random.Random(0x5245_534F ^ i)
    body = bytearray(rng.getrandbits(8) for _ in range(BODY_LEN))
    body[0xE4:0xE8] = frame.to_bytes(4, "little")
    body[0x10D0:0x10D4] = (WORLD_SPACE | flags).to_bytes(4, "little")
    body[0x60C:0x610] = struct.pack("<f", cap)
    struct.pack_into("<4f", body, 0xC0, *direction)
    struct.pack_into("<4f", body, 0x1150, *vector)
    body[0x968:0x96C] = (1 if n40 else 0).to_bytes(4, "little")
    if n40:
        # The gate is entered. `n40 == 1` answers 2 (skip), `n40 == 2` answers 0
        # (run the passes) with a 0x40-byte record that matches nothing, so the
        # passes are no-ops on both sides.
        stub = STUB_2 if n40 == 1 else STUB_0
        body[0:8] = (ARENA_BASE + VT_AT).to_bytes(8, "little")
        body[STUB_AT:STUB_AT + len(stub)] = stub
        body[VT_AT:VT_AT + 0x100] = bytes(0x100)
        body[VT_AT + 0xA8:VT_AT + 0xB0] = (ARENA_BASE + STUB_AT).to_bytes(8, "little")
        body[0x960:0x968] = (ARENA_BASE + REC40_AT).to_bytes(8, "little")
        rec = bytearray(0x40)
        struct.pack_into("<I", rec, 0x1C, 0x100)   # a window the frame id misses
        struct.pack_into("<I", rec, 0x20, 0x101)
        struct.pack_into("<i", rec, 0x28, 7)       # a mode nothing asks for
        struct.pack_into("<i", rec, 0x2C, 0)
        body[REC40_AT:REC40_AT + 0x40] = rec
    body[0x870:0x878] = (ARENA_BASE + REC_OFF).to_bytes(8, "little")
    body[0x878:0x87C] = len(records).to_bytes(4, "little")
    body[0x1A76] = 0
    body[0x10E0:0x10E4] = (0xAAAA_AAAA).to_bytes(4, "little")
    return bytes(body)


def build_records(records, i: int) -> bytes:
    rng = random.Random(0x524C_5658 ^ i)
    out = bytearray()
    for lo, hi, prop, op, value, bound in records:
        rec = bytearray(rng.getrandbits(8) for _ in range(REC_LEN))
        struct.pack_into("<I", rec, 0x1C, lo)
        struct.pack_into("<I", rec, 0x20, hi)
        struct.pack_into("<i", rec, 0x28, prop)
        struct.pack_into("<i", rec, 0x2C, op)
        struct.pack_into("<f", rec, 0x30, value)
        struct.pack_into("<f", rec, 0x34, bound)
        out += rec
    return bytes(out)


def make_args(i: int) -> List[ArgSpec]:
    case = CASES[i % len(CASES)]
    body = build_body(case, i)
    args = [
        ArgSpec(kind="ptr", mem=body, arena_offset=0),
        ArgSpec(kind="ptr", mem=build_records(case[4], i), arena_offset=REC_OFF),
    ]
    return args

"""Inputs for the `fn/MoverVectorRecords` unit (`0x140cb3b10`).

The body is pinned in the arena. Its `+0x960` pointer names the record array
(0x40 bytes each) and `+0x968` the count; `+0xe4` is the frame id the record
windows are tested against. The candidate receives the record bytes as its own
argument because it has no address space; that pointer sits in `r9`, which the
client function never reads.

The quad is a 16-byte `float*` on both sides, so it is the unit's second
channel: the harness compares all 16 bytes after the call.
"""

from __future__ import annotations

import random
import struct
from typing import List

from emu_oracle import ARENA_BASE, ArgSpec

BODY_LEN = 0x1C00
REC_OFF = 0x2000
QUAD_OFF = 0x3000
REC_LEN = 0x40

F32 = struct.unpack("<f", bytes.fromhex("0100807f"))[0]  # +NaN (quiet, positive)
INF = float("inf")

# One record: (window_lo, window_hi, mode, op, quad4).
Record = tuple

# Quads worth pinning: a plain one, a zero one, one with a NaN lane, one with an
# infinite lane, a signed-zero pair, and one whose length ties another.
_QUADS = [
    (1.0, 2.0, 3.0, 0.5),
    (0.0, 0.0, 0.0, 0.0),
    (F32, 1.0, 1.0, 1.0),
    (1.0, INF, 1.0, 1.0),
    (-0.0, 0.0, -0.0, 0.0),
    (1.0, 1.0, 1.0, 0.0),
    (3.0, 4.0, 0.0, 0.0),
]

# The record quads: shorter, longer, equal in length, NaN lane, zero.
_RECS = [
    (0.5, 0.5, 0.5, 0.25),
    (9.0, 9.0, 9.0, 9.0),
    (1.0, 2.0, 3.0, 0.5),
    (F32, F32, F32, F32),
    (0.0, 0.0, 0.0, 0.0),
]


def _cases():
    # The discriminating cases come first: the mutation corpus only probes the
    # leading cases.
    one = (0, 8, 0, 0, (10.0, 20.0, 30.0, 40.0))
    recs = lambda op, mode=0, quad=(10.0, 20.0, 30.0, 40.0): [  # noqa: E731
        (0, 8, mode, op, quad)
    ]
    cases = [
        # Window edges: id equal to lo (inside), to hi (outside), below, above.
        (5, 0, 2, [(5, 6, 0, 2, (7.0, 7.0, 7.0, 7.0))], (1.0, 2.0, 3.0, 4.0)),
        (6, 0, 2, [(5, 6, 0, 2, (7.0, 7.0, 7.0, 7.0))], (1.0, 2.0, 3.0, 4.0)),
        (4, 0, 2, [(5, 6, 0, 2, (7.0, 7.0, 7.0, 7.0))], (1.0, 2.0, 3.0, 4.0)),
        (7, 0, 2, [(5, 6, 0, 2, (7.0, 7.0, 7.0, 7.0))], (1.0, 2.0, 3.0, 4.0)),
        # A mode that does not match, and one that does beside it.
        (0, 0, 2, [(0, 8, 1, 2, (9.0, 9.0, 9.0, 9.0))], (1.0, 2.0, 3.0, 4.0)),
        (
            0,
            0,
            2,
            [(0, 8, 1, 2, (9.0, 9.0, 9.0, 9.0)), (0, 8, 0, 0, (1.0, 1.0, 1.0, 2.0))],
            (1.0, 2.0, 3.0, 4.0),
        ),
        # A skipped record that is not the last (the loop continues) and one
        # that is (the loop exits).
        (0, 0, 2, [(9, 10, 0, 2, (99.0, 99.0, 99.0, 99.0)), (0, 8, 0, 2, (7.0, 7.0, 7.0, 7.0))], (1.0, 1.0, 1.0, 1.0)),
        (0, 0, 2, [(0, 8, 0, 2, (7.0, 7.0, 7.0, 7.0)), (9, 10, 0, 2, (99.0, 99.0, 99.0, 99.0))], (1.0, 1.0, 1.0, 1.0)),
        # Each op once, and each with a mode that triggers the y restore.
        (0, 0, 2, recs(0), (1.0, 2.0, 3.0, 4.0)),
        (0, 0, 2, recs(1), (1.0, 2.0, 3.0, 4.0)),
        (0, 0, 2, recs(2), (1.0, 2.0, 3.0, 4.0)),
        (0, 0, 2, recs(3, quad=(0.5, 0.5, 0.5, 0.5)), (1.0, 2.0, 3.0, 4.0)),
        (0, 0, 2, recs(4, quad=(9.0, 9.0, 9.0, 9.0)), (1.0, 2.0, 3.0, 4.0)),
        (0, 0, 2, recs(5), (1.0, 2.0, 3.0, 4.0)),
        (0, 1, 2, recs(0, mode=1), (1.0, 2.0, 3.0, 4.0)),
        (0, 1, 2, recs(1, mode=1), (1.0, 2.0, 3.0, 4.0)),
        (0, 1, 2, recs(2, mode=1), (1.0, 2.0, 3.0, 4.0)),
        # Equal lengths decide nothing by length, so the branch order shows.
        (0, 0, 2, recs(3, quad=(-1.0, -2.0, -3.0, 0.0)), (1.0, 2.0, 3.0, 4.0)),
        (0, 0, 2, recs(4, quad=(-1.0, -2.0, -3.0, 0.0)), (1.0, 2.0, 3.0, 4.0)),
        # A NaN in the length: the two branches must disagree.
        (0, 0, 2, recs(3, quad=(F32, F32, F32, F32)), (1.0, 2.0, 3.0, 4.0)),
        (0, 0, 2, recs(4, quad=(F32, F32, F32, F32)), (1.0, 2.0, 3.0, 4.0)),
        # The empty array.
        (7, 0, 0, [], (1.0, 2.0, 3.0, 4.0)),
        (7, 0, 1, [], (1.0, 2.0, 3.0, 4.0)),
    ]
    for op in (0, 1, 2, 3, 4, 5, -1):
        for mode in (0, 1, 2, -1):
            for rec in _RECS:
                for quad in _QUADS:
                    cases.append((7, mode, op, [(0, 8, mode, op, rec)], quad))
    return cases


CASES = _cases()


def build_body(count: int, ident: int, i: int) -> bytes:
    rng = random.Random(0x5645_4354 ^ i)
    body = bytearray(rng.getrandbits(8) for _ in range(BODY_LEN))
    body[0xE4:0xE8] = ident.to_bytes(4, "little")
    body[0x960:0x968] = (ARENA_BASE + REC_OFF).to_bytes(8, "little")
    body[0x968:0x96C] = count.to_bytes(4, "little")
    return bytes(body)


def build_records(records, i: int) -> bytes:
    rng = random.Random(0x5245_4334 ^ i)
    out = bytearray()
    for lo, hi, mode, op, quad in records:
        rec = bytearray(rng.getrandbits(8) for _ in range(REC_LEN))
        struct.pack_into("<I", rec, 0x1C, lo)
        struct.pack_into("<I", rec, 0x20, hi)
        struct.pack_into("<i", rec, 0x28, mode)
        struct.pack_into("<i", rec, 0x2C, op)
        struct.pack_into("<4f", rec, 0x30, *quad)
        out += rec
    return bytes(out)


def make_args(i: int) -> List[ArgSpec]:
    ident, mode, _op, records, quad = CASES[i % len(CASES)]
    return [
        ArgSpec(kind="ptr", mem=build_body(len(records), ident, i), arena_offset=0),
        ArgSpec(kind="ptr", mem=struct.pack("<4f", *quad), arena_offset=QUAD_OFF),
        ArgSpec(kind="u64", bits=mode & 0xFFFF_FFFF),
        ArgSpec(
            kind="ptr",
            mem=build_records(records, i),
            arena_offset=REC_OFF,
        ),
    ]

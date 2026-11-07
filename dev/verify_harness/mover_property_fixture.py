"""Inputs for the `fn/MoverPropertyRecords` unit (`0x140cb3990`).

The body is pinned in the arena. Its `+0x870` pointer names the record array
(0x38 bytes each) and `+0x878` the count; `+0xe4` is the frame id the record
windows are tested against. The candidate receives the record bytes as its own
argument because it has no address space; the extra pointer sits in `r9`, which
the client function never reads.

The value is a `float*` on both sides, so the mutated float is the unit's second
channel: the harness compares the whole 4-byte pointee after the call.
"""

from __future__ import annotations

import random
import struct
from typing import List

from emu_oracle import ARENA_BASE, ArgSpec

BODY_LEN = 0x1C00
REC_OFF = 0x2000
VAL_OFF = 0x3000
REC_LEN = 0x38

# One record: (window_lo, window_hi, property, op, value, bound).
Record = tuple

F32 = struct.unpack("<f", bytes.fromhex("0100807f"))[0]  # +NaN (quiet, positive)
INF = float("inf")

# Ops 0..5 and an out-of-range pair, each with a distinct receiver value.
_OPS = [
    (0, 2.0, 5.0),  # add: record is the destination of ADDSS
    (1, 2.0, 5.0),  # mul: record is the destination of MULSS
    (2, 2.0, 5.0),  # assign
    (3, 2.0, 5.0),  # minss(current, value)
    (4, 2.0, 5.0),  # maxss(current, value)
    (5, 2.0, 5.0),  # clamp against bound
    (6, 2.0, 5.0),  # out of range: value kept, still "applied"
    (-1, 2.0, 5.0),  # signed out of range
]

# Values worth pinning against each op: a NaN payload, both infinities, signed
# zero, and the equal-operand case where MINSS/MAXSS return their source.
_VALUES = [0.0, -0.0, 1.0, -1.0, 2.0, 5.0, 1e30, -1e30, INF, -INF, F32]


def _cases():
    # The discriminating cases come first: the mutation corpus only probes the
    # leading cases, so every branch, every op and both window edges have to be
    # represented there.
    cases = [
        # frame_id equal to `lo` (inside), to `hi` (outside), below and above.
        (5, 0, 8, 2, [(5, 6, 0x14, 2, 9.0, 0.0)], 1.0),
        (6, 0, 8, 2, [(5, 6, 0x14, 2, 9.0, 0.0)], 1.0),
        (4, 0, 8, 2, [(5, 6, 0x14, 2, 9.0, 0.0)], 1.0),
        (7, 0, 8, 2, [(5, 6, 0x14, 2, 9.0, 0.0)], 1.0),
        # A property that does not match, and one that does beside it.
        (0, 0, 8, 2, [(0, 8, 0x15, 2, 9.0, 0.0)], 1.0),
        (0, 0, 8, 2, [(0, 8, 0x15, 2, 9.0, 0.0), (0, 8, 0x14, 0, 1.0, 0.0)], 1.0),
        # A skipped record that is not the last (the loop continues) and one
        # that is (the loop exits).
        (0, 0, 8, 2, [(9, 10, 0x14, 2, 99.0, 0.0), (0, 8, 0x14, 2, 3.0, 0.0)], 1.0),
        (0, 0, 8, 2, [(0, 8, 0x14, 2, 3.0, 0.0), (9, 10, 0x14, 2, 99.0, 0.0)], 1.0),
        # Table order decides, so the three ops compose.
        (
            0,
            0,
            8,
            2,
            [
                (0, 8, 0x14, 2, 4.0, 0.0),
                (0, 8, 0x14, 0, 1.0, 0.0),
                (0, 8, 0x14, 1, 2.0, 0.0),
            ],
            0.0,
        ),
        # Each op once with an argument the default cannot reproduce.
        (0, 0, 8, 0, [(0, 8, 0x14, 0, 2.0, 0.0)], 3.0),
        (0, 0, 8, 0, [(0, 8, 0x14, 1, 3.0, 0.0)], 3.0),
        (0, 0, 8, 0, [(0, 8, 0x14, 2, 5.0, 0.0)], 3.0),
        (0, 0, 8, 0, [(0, 8, 0x14, 3, 2.0, 0.0)], 3.0),
        (0, 0, 8, 0, [(0, 8, 0x14, 4, 2.0, 0.0)], 3.0),
        (0, 0, 8, 0, [(0, 8, 0x14, 5, 1.0, 9.0)], 20.0),
        (0, 0, 8, 0, [(0, 8, 0x14, 5, 1.0, 9.0)], -20.0),
        # An op outside the jump table is applied but changes nothing.
        (0, 0, 8, 0, [(0, 8, 0x14, 6, 2.0, 0.0)], 3.0),
        (0, 0, 8, 0, [(0, 8, 0x14, -1, 2.0, 0.0)], 3.0),
        # The empty array and a count that disagrees with the bytes.
        (7, 0, 0, 0, [], 3.0),
        (7, 0, 0, 0, [], 0.0),
    ]
    for op, arg, bound in _OPS:
        for value in _VALUES:
            cases.append((7, 7, 8, op, [(0, 8, 0x14, op, arg, bound)], value))
    return cases


CASES = _cases()


def build_body(count: int, ident: int, i: int) -> bytes:
    rng = random.Random(0x5052_4F50 ^ i)
    body = bytearray(rng.getrandbits(8) for _ in range(BODY_LEN))
    body[0xE4:0xE8] = ident.to_bytes(4, "little")
    body[0x870:0x878] = (ARENA_BASE + REC_OFF).to_bytes(8, "little")
    body[0x878:0x87C] = count.to_bytes(4, "little")
    return bytes(body)


def build_records(records, i: int) -> bytes:
    rng = random.Random(0x5241_4E44 ^ i)
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
    ident, _lo, _hi, _op, records, value = CASES[i % len(CASES)]
    value_bytes = struct.pack("<f", value)
    return [
        ArgSpec(kind="ptr", mem=build_body(len(records), ident, i), arena_offset=0),
        ArgSpec(kind="ptr", mem=value_bytes, arena_offset=VAL_OFF),
        ArgSpec(kind="u64", bits=0x14),
        ArgSpec(
            kind="ptr",
            mem=build_records(records, i),
            arena_offset=REC_OFF,
        ),
    ]

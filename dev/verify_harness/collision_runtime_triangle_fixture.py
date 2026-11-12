"""Inputs for the `collision_runtime_triangle` unit (`0x142ccf540`).

One case is one blob: the record at its front, then the height field's vertex
array, then the callback stub. The record's vertex pointer and its callback
pointer are absolute arena addresses, so the oracle reads the vertices out of
the arena and calls the stub there, and the probe reads the same bytes out of
the blob.

A vertex is eight bytes - a height and a kind byte at `+0x07` - and the
triangle index picks a quad of the grid. The declared cases walk every
diagonal the client's rule can pick: the kind byte `0` with both parities, the
kind byte `1`, and the kind byte `4`, which means there is no triangle at all.
The callback's own answer is pinned by the stub, which returns `0x1234`.
"""

from __future__ import annotations

import random
import struct
from typing import List

from emu_oracle import ARENA_BASE, ArgSpec

BLOB_OFF = 0
OUT_OFF = 0x4000

RECORD_LEN = 0x48
VERTICES_OFF = 0x100
VERTEX_LEN = 8
VERTEX_COUNT = 40
STUB_OFF = 0x400
# `mov eax, 0x1234; ret` - the callback's answer, in the client's own register.
STUB = bytes.fromhex("b834120000c3")
BLOB_LEN = 0x600

OUT_LEN = 0x84

# The cells per row. The client's index rule divides by `count - 1`, so the
# count must be at least two.
COUNT = 4
# The vertex stride, the grid's cell size.
STRIDE = 0.5

# The kind byte at the vertex's `+0x07`: `0` picks the diagonal by the parity
# of the cell, `1` always takes it and `4` means there is no triangle.
KINDS = [0, 1, 4]


def _fbits(value: float) -> int:
    return struct.unpack("<I", struct.pack("<f", value))[0]


def _cases():
    rng = random.Random(23680)
    out = []
    # Every kind against a spread of indices, so every diagonal and every
    # parity is reached.
    for kind in KINDS:
        for index in (0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 11, 12, 13, 17, 18, 23):
            out.append((kind, index))
    # The same index with and without the callback.
    for index in (0, 2, 5, 9):
        out.append((0, index))
    # The ordinary bulk.
    for _ in range(16):
        out.append((rng.choice(KINDS), rng.randrange(0, 24)))
    return out


CASES = _cases()


def _blob_bytes(kind: int, index: int, callback: bool, rng: random.Random) -> bytes:
    blob = bytearray(BLOB_LEN)
    struct.pack_into("<i", blob, 0x04, COUNT)
    struct.pack_into("<I", blob, 0x08, _fbits(STRIDE))
    struct.pack_into("<H", blob, 0x30, 0x0777)
    struct.pack_into("<Q", blob, 0x38, ARENA_BASE + BLOB_OFF + VERTICES_OFF)
    if callback:
        struct.pack_into("<Q", blob, 0x40, ARENA_BASE + BLOB_OFF + STUB_OFF)
    blob[STUB_OFF : STUB_OFF + len(STUB)] = STUB
    for vertex in range(VERTEX_COUNT):
        at = VERTICES_OFF + vertex * VERTEX_LEN
        blob[at : at + 4] = struct.pack("<I", _fbits(rng.uniform(-3.0, 3.0)))
        # The layer word at `+0x04` and the kind byte at `+0x07`.
        struct.pack_into("<H", blob, at + 4, 0x1000 + vertex)
        blob[at + 6] = 0x5A
        blob[at + 7] = kind
    return bytes(blob)


def make_args(i: int) -> List[ArgSpec]:
    kind, index = CASES[i % len(CASES)]
    callback = i % 3 != 0
    rng = random.Random(1000 + i)
    return [
        ArgSpec(kind="ptr", mem=_blob_bytes(kind, index, callback, rng), arena_offset=BLOB_OFF),
        ArgSpec(kind="ptr", mem=bytes(OUT_LEN), arena_offset=OUT_OFF),
        ArgSpec(kind="u64", bits=index & 0xFFFFFFFF),
    ]


def make_args_for(kind: int, index: int) -> List[ArgSpec]:
    """One case with an explicit kind and index (the dev probes use it)."""
    rng = random.Random(1000 + index)
    return [
        ArgSpec(kind="ptr", mem=_blob_bytes(kind, index, True, rng), arena_offset=BLOB_OFF),
        ArgSpec(kind="ptr", mem=bytes(OUT_LEN), arena_offset=OUT_OFF),
        ArgSpec(kind="u64", bits=index & 0xFFFFFFFF),
    ]

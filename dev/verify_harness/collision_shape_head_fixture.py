"""Inputs for the `collision_shape_head` unit (`0x142cb8950`).

One case is a shape entry, a head to write and one blob holding the world
record with the three shape pools behind it. The record's pool pointers are
absolute arena addresses, so the oracle reads the element out of the arena and
the probe reads the same bytes out of the blob.

The three pools are laid out at a `0x20`, a `0x30` and a `0x50` stride, and
each element's radius is a **different** number, so a wrong stride or a wrong
pool is caught by the head's radius rather than by the pointer alone.
"""

from __future__ import annotations

import random
import struct
from typing import List

from emu_oracle import ARENA_BASE, ArgSpec

ENTRY_OFF = 0x4000
OUT_OFF = 0x4100
# The blob sits at the arena's front, so the absolute addresses it carries
# are the arena base plus their own offset and the probe resolves them by
# subtracting the base.
BLOB_OFF = 0

WORLD_LEN = 0x340
POOL0_OFF = 0x400  # the sphere pool: world+0x2d0, a 0x20 stride
POOL1_OFF = 0x600  # the capsule pool: world+0x2e8, a 0x30 stride
POOL2_OFF = 0x900  # the counted pool: world+0x300, a 0x50 stride
BLOB_LEN = 0xE00

ENTRY_LEN = 0xC0

SPHERE_STRIDE = 0x20
CAPSULE_STRIDE = 0x30
COUNTED_STRIDE = 0x50

# The type byte at the entry's `+0x20`: the client compares it against zero
# and one, and every other value - the negative ones included - takes the
# counted branch.
TYPES = [0, 1, 2, 3, 4, 7, 255]


def _fbits(value: float) -> int:
    return struct.unpack("<I", struct.pack("<f", value))[0]


def _quad_bytes(values) -> bytes:
    return b"".join(struct.pack("<I", _fbits(v)) for v in values)


def _sphere_pool(count: int, rng: random.Random) -> bytes:
    out = bytearray()
    for index in range(count):
        element = bytearray(SPHERE_STRIDE)
        element[0x00:0x10] = _quad_bytes(
            [rng.uniform(-5, 5), rng.uniform(-5, 5), rng.uniform(-5, 5), 0.0]
        )
        # The radius the sphere builder splats out of the element's `+0x10`.
        struct.pack_into("<I", element, 0x10, _fbits(0.25 + index))
        out += element
    return bytes(out)


def _capsule_pool(count: int, rng: random.Random) -> bytes:
    out = bytearray()
    for index in range(count):
        element = bytearray(CAPSULE_STRIDE)
        element[0x00:0x10] = _quad_bytes(
            [rng.uniform(-5, 5), rng.uniform(-5, 5), rng.uniform(-5, 5), 0.0]
        )
        element[0x10:0x20] = _quad_bytes(
            [rng.uniform(-5, 5), rng.uniform(-5, 5), rng.uniform(-5, 5), 0.0]
        )
        # The radius the capsule builder splats out of the element's `+0x20`.
        struct.pack_into("<I", element, 0x20, _fbits(0.5 + index))
        out += element
    return bytes(out)


def _counted_pool(count: int, rng: random.Random) -> bytes:
    out = bytearray()
    for index in range(count):
        element = bytearray(COUNTED_STRIDE)
        # The points live behind the element's `+0x10`; the count is its
        # `+0x30`. The address is absolute and points into the blob.
        struct.pack_into("<Q", element, 0x10, ARENA_BASE + BLOB_OFF + 0xD00)
        struct.pack_into("<I", element, 0x30, 1 + index)
        element[0x40:0x44] = _quad_bytes([float(index), 0.0, 0.0, 0.0])
        out += element
    return bytes(out)


def _cases():
    rng = random.Random(142_8950)
    out = []
    # Every type against every index, with the pools' sizes following.
    for kind in TYPES:
        for index in range(4):
            out.append((kind, index))
    # A large index, so the stride arithmetic is exercised.
    for kind in TYPES:
        out.append((kind, 7))
    # The ordinary bulk.
    for _ in range(64):
        out.append((rng.choice(TYPES), rng.randrange(0, 8)))
    return out


CASES = _cases()


def _entry_bytes(kind: int, index: int) -> bytes:
    entry = bytearray(ENTRY_LEN)
    entry[0x20] = kind & 0xFF
    struct.pack_into("<i", entry, 0x24, index)
    return bytes(entry)


def _blob_bytes(kind: int, index: int, rng: random.Random) -> bytes:
    blob = bytearray(BLOB_LEN)
    struct.pack_into("<Q", blob, 0x2D0, ARENA_BASE + BLOB_OFF + POOL0_OFF)
    struct.pack_into("<Q", blob, 0x2E8, ARENA_BASE + BLOB_OFF + POOL1_OFF)
    struct.pack_into("<Q", blob, 0x300, ARENA_BASE + BLOB_OFF + POOL2_OFF)
    spheres = _sphere_pool(8, rng)
    capsules = _capsule_pool(8, rng)
    counted = _counted_pool(8, rng)
    blob[POOL0_OFF : POOL0_OFF + len(spheres)] = spheres
    blob[POOL1_OFF : POOL1_OFF + len(capsules)] = capsules
    blob[POOL2_OFF : POOL2_OFF + len(counted)] = counted
    return bytes(blob)


def make_args(i: int) -> List[ArgSpec]:
    kind, index = CASES[i % len(CASES)]
    rng = random.Random(1000 + i)
    return [
        ArgSpec(kind="ptr", mem=_entry_bytes(kind, index), arena_offset=ENTRY_OFF),
        # The head's word at `+0x0c` is the one the client never writes, so
        # the buffer starts with a pattern that proves it stayed put.
        ArgSpec(kind="ptr", mem=bytes([0xA5]) * 0x20, arena_offset=OUT_OFF),
        ArgSpec(kind="ptr", mem=_blob_bytes(kind, index, rng), arena_offset=BLOB_OFF),
    ]

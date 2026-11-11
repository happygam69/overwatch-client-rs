"""`MapChunkHeaderFixup` (`0x142e49360`) inputs: a pathing chunk's header
record with relative pointer slots, and the record's own address.

The client's dispatch (`FinishMapChunkAsset`, `0x141ff54e0`) sends the
pathing kind's header record through the fixup before it hands `+0x00` to
the STUD loader. The fixup converts the three self-relative slots at
`+0x00`, `+0x08` and `+0x18` to pointers by adding the record's own address,
and a stored zero stays zero.

The record keeps every slot in its file-relative form and the fixture passes
the record's arena base in the client's dead second argument slot, so the
oracle (which reads the base from RCX) and the probe (which is told it) write
the same absolute values and the whole record is compared without masking.
"""

from __future__ import annotations

import random
import struct

from emu_oracle import ARENA_BASE, ArgSpec

H_OFF = 0x1000
HEADER_LEN = 0x20

# The three slots the client converts, and the one it leaves alone.
SLOTS = (0x00, 0x08, 0x18)


def _special(i: int):
    """(values for +0x00, +0x08, +0x18, the untouched +0x10)."""
    table = [
        # all three zero: the null case, and the only one where every
        # conversion takes the `cmove` back to zero
        (0, 0, 0),
        # the blob pointer, a second pointer and a third: the ordinary case
        (0x20, 0x1000, 0x40),
        # one zero among two non-zero: pins that the zero rule is per slot
        (0x20, 0, 0x40),
        (0, 0x1000, 0),
        (0, 0, 0x40),
        # a value that wraps when the base is added
        (0xFFFF_FFFF_FFFF_FFF0, 0x10, 0x20),
    ]
    return table[i % len(table)]


def _case(i: int):
    if i < 6:
        return _special(i)
    rng = random.Random(0x48454144 ^ i)
    values = []
    for _ in SLOTS:
        values.append(rng.choice((0, rng.randrange(1, 0x1000), rng.getrandbits(64))))
    return tuple(values)


def build(i: int) -> bytes:
    """The header record: three relative slots and the one left alone."""
    values = _case(i)
    record = bytearray(HEADER_LEN)
    for at, value in zip(SLOTS, values):
        struct.pack_into("<Q", record, at, value)
    # +0x10 is not touched by the fixup: put something there that a spurious
    # write would change.
    struct.pack_into("<Q", record, 0x10, 0x0BAD_F00D_DEAD_BEEF)
    return bytes(record)


def make_args(i: int):
    record = build(i)
    return [
        ArgSpec(kind="ptr", mem=record, arena_offset=H_OFF),
        # Candidate-only: the record's own address, which the client uses as
        # the base. The oracle reads it from RCX.
        ArgSpec(kind="u64", bits=ARENA_BASE + H_OFF),
    ]

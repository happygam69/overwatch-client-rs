"""`ApplyCharacterControllerMove` (`0x142cd5370`) as a **steps** trace: the
character moved several frames in a row.

The single-call unit (`fn/ApplyCharacterControllerMove`) proves one move against
a record it is handed. This one calls the client three times in a row on the
same record, so the position the first move writes is the position the second
move integrates from - the actual movement loop, at the smallest size that has
the feedback in it.

The record, the handle table and the live entries all live in the unit's state
snapshot (pinned at the client's own handle-table global, exactly as the
single-call fixture has it), so every write a call makes is still there for the
next call, and the state channel compares the whole region after each step.

Each step's velocity differs, so the three moves land at three different
positions; a port that forgets to feed the updated record into the next move
diverges on step 1 while step 0 still matches.
"""

from __future__ import annotations

import struct
from math import inf, nan

from apply_character_move_fixture import CASES, STATE_BASE, STATE_HEX, STATE_LEN
from emu_oracle import ArgSpec

VELOCITY_OFF = 0x7000

# The steps of case `i`: three moves whose velocities differ, so the record's
# position has to change between them. The handle and the state base stay the
# same - only the velocity is new per step.
STEPS = [
    [(1.0, 0.0, 0.0, 0.0), (0.0, 2.0, 0.0, 0.0), (0.0, 0.0, 3.0, 0.0)],
    [(4.0, 0.0, 0.0, 0.0), (4.0, 0.0, 0.0, 0.0), (-4.0, 0.0, 0.0, 0.0)],
    [(0.5, 0.5, 0.5, 0.0), (1.5, -0.5, 2.5, 0.0), (0.0, 0.0, 0.0, 1.0)],
    [(10.0, 0.0, 0.0, 0.0), (0.0, 0.0, 0.0, 0.0), (10.0, 0.0, 0.0, 0.0)],
    # A move the client declines (inf xyz) between two real ones: the declined
    # call writes nothing, so the third move must integrate from the position
    # the first one left.
    [(3.0, 1.0, 0.0, 0.0), (inf, 0.0, 0.0, 0.0), (3.0, 1.0, 0.0, 0.0)],
    # The same with a NaN lane, then a w-only inf (w is not tested by the
    # client), so both decline lanes and the ignore lane are steps.
    [(0.0, nan, 0.0, 0.0), (1.0, 2.0, 3.0, 0.0), (0.0, 0.0, 0.0, inf)],
]


def _fbits(value: float) -> int:
    return struct.unpack("<I", struct.pack("<f", value))[0]


def make_steps(i: int):
    """The steps of corpus case `i`: a three-move sequence on one record."""
    handle = CASES[i % len(CASES)][0]
    velocities = STEPS[i % len(STEPS)]
    steps = []
    for velocity in velocities:
        steps.append(
            [
                ArgSpec(kind="u64", bits=handle),
                ArgSpec(
                    kind="ptr",
                    mem=b"".join(struct.pack("<I", _fbits(v)) for v in velocity),
                    arena_offset=VELOCITY_OFF,
                ),
                ArgSpec(kind="u64", bits=STATE_BASE),
            ]
        )
    return steps


def snapshot() -> bytes:
    data = bytes.fromhex(STATE_HEX)
    assert len(data) == STATE_LEN, (len(data), STATE_LEN)
    return data

"""State-mode fixture for fn/OrthonormalizeCameraBasis-state.

The same structured basis cases as the plain fixture, but the pointee lives
inside a declared state snapshot at a fixed guest address. The oracle preloads
the snapshot, writes the per-case basis at ``STATE_BASE + BASIS_OFF``, runs the
real function, and compares the whole state region after the call; the probe
translates the same guest address into its state buffer. This exercises the
state channel without pretending a client function is stateful.
"""

from __future__ import annotations

from typing import List

import orthonormalize_basis_fixture as plain
from emu_oracle import ArgSpec

STATE_BASE = 0x0000_2000_0000_0000
STATE_SIZE = 0x200
BASIS_OFF = 0x100


def make_args(i: int) -> List[ArgSpec]:
    mem = plain.make_args(i)[0].mem
    return [ArgSpec(kind="ptr", mem=mem, guest=STATE_BASE + BASIS_OFF)]

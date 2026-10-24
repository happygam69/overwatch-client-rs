"""Structured input factory for fn/AnimTimeWrap.

Why a factory: the generic random corpus leaves one mutation survivor on
this unit - `mask widen` flips the flag test from `& 1` to `& 3`, which is
only observable when the range's flag dword has bit 1 set (and bit 0 clear)
AND the wrap and clamp results differ for that t. Random draws over the
flag dword rarely land that combination with a discriminating t inside the
first cases the mutation stage samples. This factory pins those cases
deterministically at the FRONT of the corpus so the mutation stage always
sees them.

Shape of the range float[4]: [0]=min, [1]=max/period, [3] low bits are the
flag dword (bit 0 selects modular wrap).
"""

from __future__ import annotations

import struct
from typing import List

from emu_oracle import ArgSpec
from gen import focused_f32_edge_cases, generate_args, UnitArg

SPECS = [UnitArg("range", "ptr", 16), UnitArg("t", "f32")]


def _range_mem(min_v: float, max_v: float, slot2: float, flag: int) -> bytes:
    return (
        struct.pack("<f", min_v)
        + struct.pack("<f", max_v)
        + struct.pack("<f", slot2)
        + struct.pack("<I", flag & 0xFFFF_FFFF)
    )


def _case(min_v: float, max_v: float, flag: int, t: float) -> List[ArgSpec]:
    return [
        ArgSpec(kind="ptr", mem=_range_mem(min_v, max_v, 0.0, flag)),
        ArgSpec(kind="f32", value=t),
    ]


def make_args(i: int) -> List[ArgSpec]:
    # 0..15: deterministic flag-sweep cases. The discriminating triple is
    #   flag bit1 set (so a &3 mutant wraps where the real &1 clamps),
    #   t = 5.5, period = 2.0, min = 0.0:
    #     clamp -> max(0, min(5.5, 2.0)) = 2.0
    #     wrap  -> 5.5 - (int)(5.5/2)*2 = 5.5 - 2*2 = 1.5
    # Different answers, so any mask that takes the wrap path on bit1 dies.
    specials = [
        _case(0.0, 2.0, 0b00, 5.5),   # clamp path (bit0 clear): 2.0
        _case(0.0, 2.0, 0b01, 5.5),   # wrap path (bit0 set): 1.5
        _case(0.0, 2.0, 0b10, 5.5),   # bit1 only: real clamps (2.0); &3 mutant wraps (1.5)
        _case(0.0, 2.0, 0b11, 5.5),   # both bits: wrap either way
        _case(0.0, 2.0, 0b10, -1.5),  # bit1, negative t: clamp -> 0.0; wrap -> -1.5 - (int)(-0.75)*2 = -1.5
        _case(0.0, 2.0, 0b10, 2.0),   # bit1, t exactly max: both give 2.0 (agreement case)
        _case(1.0, 4.0, 0b10, 7.25),  # bit1, nonzero min: clamp -> 4.0; wrap -> 7.25 - 3*4? (int)(1.8125)=1 -> 7.25-4=3.25
        _case(0.0, 2.0, 0b100, 5.5),  # bit2 only: neither mask takes wrap; agreement
        _case(0.0, 2.0, 0x80000000, 5.5),  # sign bit only: agreement
        _case(0.0, 2.0, 0xFFFFFFFF, 5.5),  # all bits: wrap either way
        _case(-2.0, 2.0, 0b10, 3.5),  # negative min, bit1: clamp -> 2.0; wrap -> 3.5 - 1*2 = 1.5
        _case(0.0, 1.0, 0b10, 0.4),   # bit1, t inside range: both 0.4 (agreement)
        _case(0.0, 2.0, 0b10, float("nan")),  # bit1 + NaN t: clamp keeps NaN via maxss second-source; wrap is NaN
        _case(0.0, 2.0, 0b10, float("inf")),  # bit1 + inf: clamp -> 2.0; wrap -> cvttss2si(inf)=INT_MIN path
        _case(0.0, 0.0, 0b10, 5.5),   # degenerate min==max, bit1
        _case(0.0, 2.0, 0b10, 0.0),   # bit1 + zero t: both 0.0
    ]
    if i < len(specials):
        return specials[i]
    j = i - len(specials)
    # Next 22: the generic focused f32 edge pool with a zeroed range
    # (clamp-only), mirroring what the non-factory path used to do.
    edges = focused_f32_edge_cases(SPECS)
    if j < len(edges):
        return edges[j]
    # After that: the generic seeded random corpus.
    return generate_args(SPECS, 4241, j - len(edges))

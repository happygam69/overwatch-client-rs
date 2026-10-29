"""Structured inputs for fn/TextureFormatFamilyBpp.

The client's switch at `0x142e372b0` takes one 32-bit format value and two
output dwords. The fixture drives every value in 0..=0xff (the engine format
byte's domain), so every case range and both default gaps are covered. Its first
47 cases are the endpoint of every arm and both sides of every gap, because the
mutation stage only probes the first 48 cases. The outputs start at zero, which
is what the client leaves them at when no case matches; the probe writes the
shipped function's `(0, 0)` default into the same zeroed slots.
"""

from __future__ import annotations

from typing import List

from emu_oracle import ArgSpec


# Every arm endpoint first: the mutation stage only probes the first 48 cases,
# so the values that distinguish a one-step boundary change must be in that
# prefix, not merely somewhere in the 0..=0xff sweep.
_EDGES = [
    0x00, 0x01, 0x04, 0x05, 0x08, 0x09, 0x16, 0x17,
    0x30, 0x31, 0x3C, 0x3D, 0x42, 0x43, 0x44, 0x46,
    0x47, 0x49, 0x4A, 0x4C, 0x4D, 0x4F, 0x50, 0x52,
    0x53, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5B, 0x5C,
    0x5F, 0x61, 0x62, 0x64, 0x65, 0x67, 0x68, 0x6A,
    0x6B, 0x6D, 0x6E, 0x70, 0x71, 0x72, 0xFF,
]


def make_args(i: int) -> List[ArgSpec]:
    # Arm endpoints first, then the full 0..=0xff domain for the differential.
    fmt = _EDGES[i] if i < len(_EDGES) else (i - len(_EDGES)) & 0xFF
    return [
        ArgSpec(kind="u64", bits=fmt),
        ArgSpec(kind="ptr", mem=b"\x00" * 4),
        ArgSpec(kind="ptr", mem=b"\x00" * 4),
    ]

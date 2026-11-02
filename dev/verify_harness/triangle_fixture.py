"""Inputs for the `triangle_degenerate` unit (`0x142cd56e0`).

The leaf takes three `vec4` corners and returns a code in `AL`. The declared
cases come first so the mutation corpus sees them: a plain triangle, a
collinear one, two coincident corners, an area just under the `2.5e-5` floor,
an edge just under `0.0025`, a box just over `1e6`, `+inf`, `NaN`, `-inf`, an
`inf` confined to the fourth lane (the low-three-lane mask ignores it) and a
very small but usable triangle.

Every index past the declared cases draws a pseudo-random triangle from a
small integer generator, mixing magnitudes so the extent, edge and area tests
all get exercised with values that are neither round nor special.
"""

from __future__ import annotations

import struct
from typing import List, Tuple

from emu_oracle import ArgSpec

CORNER_A = 0x1000
CORNER_B = 0x1100
CORNER_C = 0x1200

INF = float("inf")
NAN = float("nan")


def _vec(values) -> bytes:
    out = bytearray(16)
    for index, value in enumerate(values):
        struct.pack_into("<f", out, index * 4, value)
    return bytes(out)


def _tri(a, b, c) -> Tuple[bytes, bytes, bytes]:
    return (_vec(a), _vec(b), _vec(c))


DECLARED = [
    # A plain unit triangle: every test passes.
    _tri((0.0, 0.0, 0.0, 0.0), (1.0, 0.0, 0.0, 0.0), (0.0, 1.0, 0.0, 0.0)),
    # Collinear: zero area but every edge long enough.
    _tri((0.0, 0.0, 0.0, 0.0), (1.0, 1.0, 1.0, 0.0), (2.0, 2.0, 2.0, 0.0)),
    # Two corners coincide: the edge test fires first.
    _tri((0.25, 0.5, 0.75, 0.0), (0.25, 0.5, 0.75, 0.0), (1.0, 0.0, 0.0, 0.0)),
    # Area just under 2.5e-5: a 0.005-wide sliver, edges well clear.
    _tri((0.0, 0.0, 0.0, 0.0), (1.0, 0.0, 0.0, 0.0), (0.5, 0.0049, 0.0, 0.0)),
    # Area just over 2.5e-5: the same sliver one step wider.
    _tri((0.0, 0.0, 0.0, 0.0), (1.0, 0.0, 0.0, 0.0), (0.5, 0.0051, 0.0, 0.0)),
    # Edge just under 0.0025 (squared 6.25e-6): the b/a edge.
    _tri((0.0, 0.0, 0.0, 0.0), (0.0024, 0.0, 0.0, 0.0), (0.0, 5.0, 0.0, 0.0)),
    # Edge just over 0.0025.
    _tri((0.0, 0.0, 0.0, 0.0), (0.0026, 0.0, 0.0, 0.0), (0.0, 5.0, 0.0, 0.0)),
    # A box just over a million on x: extent fires.
    _tri((0.0, 0.0, 0.0, 0.0), (1_000_001.0, 0.0, 0.0, 0.0), (0.0, 1.0, 0.0, 0.0)),
    # A box just under a million: it gets through to the area test.
    _tri((0.0, 0.0, 0.0, 0.0), (999_999.0, 0.0, 0.0, 0.0), (0.0, 1.0, 0.0, 0.0)),
    # The extent test is three scalar compares, one per lane; each needs its own
    # oversize case. Lane 1 (y) and lane 2 (z) here.
    _tri((0.0, 0.0, 0.0, 0.0), (1.0, 0.0, 0.0, 0.0), (0.0, 1_000_001.0, 0.0, 0.0)),
    _tri((0.0, 0.0, 0.0, 0.0), (1.0, 0.0, 0.0, 0.0), (0.0, 1.0, 1_000_001.0, 0.0)),
    # +inf on the first corner.
    _tri((INF, 0.0, 0.0, 0.0), (1.0, 0.0, 0.0, 0.0), (0.0, 1.0, 0.0, 0.0)),
    # NaN on the second corner.
    _tri((0.0, 0.0, 0.0, 0.0), (NAN, 0.0, 0.0, 0.0), (0.0, 1.0, 0.0, 0.0)),
    # -inf on the third corner, in the z lane.
    _tri((0.0, 0.0, 0.0, 0.0), (1.0, 0.0, 0.0, 0.0), (0.0, 1.0, -INF, 0.0)),
    # inf only in the fourth lane: the low-three-lane mask leaves it alone, and
    # every later test still passes.
    _tri((0.0, 0.0, 0.0, INF), (1.0, 0.0, 0.0, 0.0), (0.0, 1.0, 0.0, 0.0)),
    # A tiny but usable triangle.
    _tri((0.0, 0.0, 0.0, 0.0), (0.01, 0.0, 0.0, 0.0), (0.0, 0.01, 0.0, 0.0)),
    # Exact boundaries, one per threshold constant (the image's own f32 values:
    # 0x36d1b717 = 6.24999984e-06, 0x36d1b717's neighbour pair 2.49999994e-05,
    # 1e6 exact). Each kills a cmp-flip or a scaled constant mutant.
    # extent exactly 1e6 on x.
    _tri((0.0, 0.0, 0.0, 0.0), (1_000_000.0, 0.0, 0.0, 0.0), (0.0, 1.0, 0.0, 0.0)),
    # a/b edge squared exactly 6.24999984e-06 (length 0.00249999994).
    _tri(
        (0.0, 0.0, 0.0, 0.0),
        (0.0024999999441206455, 0.0, 0.0, 0.0),
        (0.0, 5.0, 0.0, 0.0),
    ),
    # cross length exactly 2.49999994e-05, with every edge above 0.0025.
    _tri(
        (0.0, 0.0, 0.0, 0.0),
        (1.0, 0.0, 0.0, 0.0),
        (0.0025, 2.499999936844688e-05, 0.0, 0.0),
    ),
    # cross length 3e-05: inside a 1.5x-widened area floor, outside the real one.
    _tri(
        (0.0, 0.0, 0.0, 0.0),
        (1.0, 0.0, 0.0, 0.0),
        (0.0025, 3.0e-05, 0.0, 0.0),
    ),
]


def _random_triangle(index: int) -> Tuple[bytes, bytes, bytes]:
    """A deterministic pseudo-random triangle, wider as the index grows."""
    state = (index * 2_654_435_761 + 12345) & 0xFFFF_FFFF

    def next_bits() -> int:
        nonlocal state
        state = (state * 1_103_515_245 + 12345) & 0xFFFF_FFFF
        return state

    scale = 10.0 ** (next_bits() % 7 - 3)
    points = []
    for _ in range(3):
        point = []
        for _ in range(3):
            raw = (next_bits() >> 8) % 2001 - 1000
            point.append(raw / 1000.0 * scale)
        point.append(0.0)
        points.append(tuple(point))
    return _tri(*points)


def make_args(i: int) -> List[ArgSpec]:
    if i < len(DECLARED):
        case = DECLARED[i]
    else:
        case = _random_triangle(i)
    a, b, c = case
    return [
        ArgSpec(kind="ptr", mem=a, arena_offset=CORNER_A),
        ArgSpec(kind="ptr", mem=b, arena_offset=CORNER_B),
        ArgSpec(kind="ptr", mem=c, arena_offset=CORNER_C),
    ]

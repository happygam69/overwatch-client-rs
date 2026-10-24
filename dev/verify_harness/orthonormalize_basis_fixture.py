"""Structured input factory for fn/OrthonormalizeCameraBasis.

Shape of the camera-basis pointee (byte offsets, matching the decompile):
  +0x10  row0.x  +0x14 row0.y  +0x18 row0.z  +0x1c row0.w
  +0x20  row1.x  +0x24 row1.y  +0x18 row1.z  +0x2c row1.w
  (total 0x30 bytes minimum; the function reads only these 8 floats)

The oracle:
  - early-outs (no stores) when |len0^2-1| < 0.008 AND |len1^2-1| < 0.008
    AND |dot| <= 0.004;
  - otherwise cross = row0 x row1 (3D), then row1 = normalize(cross x
    row0) via rsqrtps + Newton, and stores row0 back unchanged.

Cases pinned at the front so the mutation stage always sees:
  - already-orthonormal bases (early-out path);
  - unit-length but non-orthogonal (rebuild, clean cross);
  - non-unit-length rows (rebuild);
  - near-parallel rows (tiny cross, exercises the FLT_MIN fallback);
  - exactly-parallel rows (zero cross, first fallback);
  - a row that is nearly (0,0,0) so both fallbacks run;
  - NaN / Inf rows (IEEE compare behaviour on the early-out gates).
"""

from __future__ import annotations

import struct
from typing import List

from emu_oracle import ArgSpec

BASIS_LEN = 0x30


def _basis(r0, r1) -> bytes:
    """Pack two float4 rows into a 0x30-byte pointee (w lanes included)."""
    b = bytearray(BASIS_LEN)
    for i, v in enumerate(r0):
        struct.pack_into("<f", b, 0x10 + i * 4, v)
    for i, v in enumerate(r1):
        struct.pack_into("<f", b, 0x20 + i * 4, v)
    return bytes(b)


def make_args(i: int) -> List[ArgSpec]:
    specials = [
        # Already orthonormal: X axis and Y axis, w=0. Early-out, no stores.
        _basis((1.0, 0.0, 0.0, 0.0), (0.0, 1.0, 0.0, 0.0)),
        # Unit length, orthogonal in xyz, non-zero w. Still early-out
        # (w is ignored by the length/dot checks).
        _basis((1.0, 0.0, 0.0, 0.5), (0.0, 0.0, 1.0, -0.25)),
        # Unit length, not orthogonal: rebuild with a clean cross.
        _basis((1.0, 0.0, 0.0, 0.0), (0.70710678, 0.70710678, 0.0, 0.0)),
        # Non-unit-length rows: rebuild.
        _basis((2.0, 0.0, 0.0, 0.0), (0.0, 3.0, 0.0, 0.0)),
        # Near-parallel: cross is tiny but above FLT_MIN.
        _basis((1.0, 0.0, 0.0, 0.0), (1.0, 1e-4, 0.0, 0.0)),
        # Exactly parallel: first fallback (cross with +Y).
        _basis((1.0, 0.0, 0.0, 0.0), (1.0, 0.0, 0.0, 0.0)),
        # row0 = (0,0,0): both fallbacks (cross with +Y then +Z).
        _basis((0.0, 0.0, 0.0, 0.0), (0.0, 1.0, 0.0, 0.0)),
        # row0 along +Z, row1 = row0: fallback cross with +Y is degenerate
        # only if row0 is parallel to Y; here the first fallback works.
        _basis((0.0, 0.0, 1.0, 0.0), (0.0, 0.0, 1.0, 0.0)),
        # NaN in row0: every IEEE compare is false, so needs_rebuild is
        # false and the early-out path runs (no stores).
        _basis((float("nan"), 0.0, 0.0, 0.0), (0.0, 1.0, 0.0, 0.0)),
        # Inf in row1: length check sees inf-1 = inf >= 0.008, rebuild;
        # cross and Newton produce inf/NaN bits that must match.
        _basis((1.0, 0.0, 0.0, 0.0), (float("inf"), 0.0, 0.0, 0.0)),
        # Negative unit row (still unit length, orthogonal to +X).
        _basis((1.0, 0.0, 0.0, 0.0), (0.0, -1.0, 0.0, 0.0)),
        # Almost-unit row0 (just inside the 0.008 epsilon) and orthogonal
        # row1: early-out. len2 = 1.0079^2 ≈ 1.01586, |len2-1| ≈ 0.01586
        # wait - that's outside. Use 0.999 so len2=0.998001, diff=0.001999.
        _basis((0.999, 0.0, 0.0, 0.0), (0.0, 1.0, 0.0, 0.0)),
        # Just outside the epsilon: len2 = 1.01^2 = 1.0201, diff=0.0201.
        _basis((1.01, 0.0, 0.0, 0.0), (0.0, 1.0, 0.0, 0.0)),
        # Dot just inside 0.004: early-out.
        _basis((1.0, 0.0, 0.0, 0.0), (0.003, 0.9999955, 0.0, 0.0)),
        # Dot just outside 0.004: rebuild.
        _basis((1.0, 0.0, 0.0, 0.0), (0.005, 0.9999875, 0.0, 0.0)),
        # Denormal row0: length ~0, both fallbacks.
        _basis((1e-40, 0.0, 0.0, 0.0), (0.0, 1.0, 0.0, 0.0)),
    ]

    if i < len(specials):
        mem = specials[i]
    else:
        # Deterministic follow-on: rotate a few unit-ish bases and some
        # random-looking but fixed rotations so the corpus keeps moving.
        j = i - len(specials)
        angle = (j % 17) * (3.14159265 / 16.0)
        c, s = __import__("math").cos(angle), __import__("math").sin(angle)
        # row0 in the XY plane, row1 tilted toward Z - usually needs rebuild.
        r0 = (c, s, 0.0, float(j % 3) * 0.1)
        r1 = (-s * 0.5, c * 0.5, 0.8660254, 0.0)
        mem = _basis(r0, r1)

    return [ArgSpec(kind="ptr", mem=mem)]

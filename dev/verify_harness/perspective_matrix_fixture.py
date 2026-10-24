"""Structured input factory for fn/BuildPerspectiveMatrix.

Why a factory: the matrix entries are ratios of descriptor fields; a flat
random pointee almost always produces a near-zero or infinite intermediate
that collapses many mutants onto the same (NaN/inf) result. This factory
pins a non-degenerate descriptor shape that keeps every ratio finite and
distinct, so an operator swap or constant edit shows up in the out matrix.

Shape of the descriptor (byte offsets, matching the decompile):
  +0x4c  skew-x / centre-x terms used in m20/m21
  +0x50  skew-y
  +0x54  scale-x (m00 numerator multiplier)
  +0x58  scale-y (m11)
  +0x5c  near (a)
  +0x60  far  (b = far - near)
  +0x64  bias-x added into m20
  +0x68  bias-y added into m21
  +0x6c  m20 second term (uses + right-left span)
  +0x70  m21 second term
  +0x74  extent scale-x
  +0x78  extent scale-y
  +0x7c  extent origin-x
  +0x80  extent origin-y
  +0x84  extent max-x
  +0x88  extent max-y
  +0x8c  left   (c = right - left)
  +0x90  bottom (d = bottom - top)
  +0x94  right
  +0x98  top

out: 64 bytes (16 floats). reverse: u64 0 or 1.
"""

from __future__ import annotations

import struct
from typing import List

from emu_oracle import ArgSpec

DESC_LEN = 0xA0
OUT_LEN = 64


def _desc(
    *,
    near: float = 0.1,
    far: float = 100.0,
    left: float = -2.0,
    right: float = 2.0,
    bottom: float = -1.5,
    top: float = 1.5,
    sx: float = 1.25,
    sy: float = 0.875,
    skew_x: float = 0.05,
    skew_y: float = -0.02,
    cx: float = 0.1,
    cy: float = -0.1,
    bias_x: float = 0.3,
    bias_y: float = -0.4,
    m20b: float = 0.15,
    m21b: float = 0.25,
    ex: float = 1.1,
    ey: float = 0.9,
    ox: float = -0.5,
    oy: float = 0.5,
    mx: float = 3.5,
    my: float = 2.5,
) -> bytes:
    b = bytearray(DESC_LEN)
    def put_f32(off: int, v: float) -> None:
        struct.pack_into("<f", b, off, v)
    put_f32(0x4c, cx)
    put_f32(0x50, cy)
    put_f32(0x54, sx)
    put_f32(0x58, sy)
    put_f32(0x5c, near)
    put_f32(0x60, far)
    put_f32(0x64, bias_x)
    put_f32(0x68, bias_y)
    put_f32(0x6c, m20b)
    put_f32(0x70, m21b)
    put_f32(0x74, ex)
    put_f32(0x78, ey)
    put_f32(0x7c, ox)
    put_f32(0x80, oy)
    put_f32(0x84, mx)
    put_f32(0x88, my)
    put_f32(0x8c, left)
    put_f32(0x90, bottom)
    put_f32(0x94, right)
    put_f32(0x98, top)
    return bytes(b)


def make_args(i: int) -> List[ArgSpec]:
    # Front-load every (reverse, shape) pair so the mutation stage always
    # sees both branches of the near/far mapping with distinct ratios.
    specials = [
        _desc(), 0,
        _desc(), 1,
        # reverse-Z with a very small near/far gap: inv_b is huge, m22/m32
        # explode differently than the forward path.
        _desc(near=1.0, far=1.0001), 1,
        # forward path with the same tiny gap: f060/b is finite but large.
        _desc(near=1.0, far=1.0001), 0,
        # inverted order (far < near): negative span, both branches.
        _desc(near=10.0, far=1.0), 0,
        _desc(near=10.0, far=1.0), 1,
        # non-uniform extent: (mx-ox)*ex and (my-oy)*ey differ per axis.
        _desc(mx=4.0, ox=0.0, my=2.0, oy=0.0, ex=2.0, ey=0.5), 0,
        _desc(mx=4.0, ox=0.0, my=2.0, oy=0.0, ex=2.0, ey=0.5), 1,
        # zero-ish scales on one axis only (m00 or m11 becomes 0).
        _desc(sx=0.0), 0,
        _desc(sy=0.0), 1,
        # asymmetric bias / skew so m20 and m21 are independent.
        _desc(cx=1.0, cy=-2.0, bias_x=0.7, bias_y=-0.3, m20b=0.4, m21b=-0.6), 0,
        _desc(cx=1.0, cy=-2.0, bias_x=0.7, bias_y=-0.3, m20b=0.4, m21b=-0.6), 1,
    ]
    if i < len(specials) // 2:
        d, rev = specials[i * 2], specials[i * 2 + 1]
    else:
        j = i - len(specials) // 2
        # Deterministic but varied follow-on cases: rotate a few fields.
        near = 0.05 + (j % 7) * 0.1
        far = near + 1.0 + (j % 5) * 10.0
        span = 1.0 + (j % 3) * 0.5
        d = _desc(
            near=near,
            far=far,
            left=-span,
            right=span,
            bottom=-span * 0.5,
            top=span * 0.5,
            sx=1.0 + (j % 4) * 0.125,
            sy=1.0 - (j % 4) * 0.0625,
            skew_x=(j % 5) * 0.01,
            skew_y=-(j % 3) * 0.02,
            cx=(j % 7) * 0.01,
            cy=-(j % 5) * 0.01,
            bias_x=(j % 3) * 0.05,
            bias_y=-(j % 4) * 0.05,
            m20b=(j % 6) * 0.02,
            m21b=-(j % 6) * 0.03,
            ex=1.0 + (j % 2) * 0.25,
            ey=1.0 + (j % 3) * 0.125,
            ox=-(j % 4) * 0.1,
            oy=(j % 4) * 0.1,
            mx=2.0 + (j % 5) * 0.5,
            my=1.5 + (j % 5) * 0.25,
        )
        rev = j & 1

    return [
        ArgSpec(kind="ptr", mem=d),
        ArgSpec(kind="ptr", mem=b"\x00" * OUT_LEN),
        ArgSpec(kind="u64", bits=rev),
    ]

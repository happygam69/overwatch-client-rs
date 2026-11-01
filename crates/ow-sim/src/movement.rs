//! The read mover ramp leaf.
//!
//! `MoverRampVector` (`0x140ca75b0`) is the small leaf the mover's per-frame
//! path calls before it sends a velocity to the Domino character controller
//! (`0x140cc03d0` -> `0x142cbd1b0`) and again at the end of `MoverAdvance`
//! (`0x140ca7f30`). It has no branches; the only conditional is the
//! `TEST`/`CMOVZ` selection of the scale slot. Its read expression is:
//!
//! ```text
//! t      = minss(body + 0x10ec, 1.0)
//! scale  = (flags[+0x00] & 2) ? body + 0x5b4 : body + 0x5a8
//! ramp   = t * scale + flags[+0x2c]
//! out[i] = ramp * body[0xc0 + 4*i]         // four f32 lanes
//! ```
//!
//! The third argument is the caller's `body + 0x10d0` block: `+0x00` is the
//! low byte of the flags word and `+0x2c` is the per-frame scalar at
//! `body + 0x10fc`. The unit models that block as a separate 0x30-byte
//! pointee, so the body and the block are two controlled buffers.
//!
//! Every multiply and add is one f32 operation, in the order above. `minss`
//! is the SSE `MINSS` relation, not Rust's `f32::min`: if either operand is
//! NaN the second source operand wins, and equal zeros return the second
//! operand.

/// `timer` in the mover body.
pub const TIMER: usize = 0x10ec;
/// The four-lane direction at `body + 0xc0`.
pub const DIRECTION: usize = 0xc0;
/// The scale slot used when flag bit 1 is clear at `body + 0x10d0`.
pub const SCALE_CLEAR: usize = 0x5a8;
/// The scale slot used when flag bit 1 is set.
pub const SCALE_SET: usize = 0x5b4;
/// The caller's `body + 0x10d0` block: the flags byte is at `+0`, the scalar
/// at `+0x2c`.
pub const FLAGS_BLOCK: usize = 0x10d0;
/// The scalar added after `t * scale`, at `FLAGS_BLOCK + 0x2c`.
pub const RAMP_OFFSET: usize = 0x2c;
/// The flags bit that selects `SCALE_SET`.
pub const SCALE_BIT: u8 = 0x02;

/// The SSE `MINSS` relation: any NaN yields the source operand `b`, and
/// equal operands also yield `b`.
#[inline]
pub fn minss(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        b
    } else if a < b {
        a
    } else {
        b
    }
}

/// The SSE `MAXSS` relation: any NaN yields the source operand `b`, and
/// equal operands also yield `b`.
#[inline]
pub fn maxss(a: f32, b: f32) -> f32 {
    if a.is_nan() || b.is_nan() {
        b
    } else if a > b {
        a
    } else {
        b
    }
}

/// Quiet a NaN the way an SSE arithmetic instruction does.
#[inline]
fn sse_nan(v: f32) -> f32 {
    f32::from_bits(v.to_bits() | 0x0040_0000)
}

/// True for a signaling NaN (the quiet bit is clear).
#[inline]
fn is_signaling_nan(value: f32) -> bool {
    value.is_nan() && (value.to_bits() & 0x0040_0000) == 0
}

/// `MULSS`/`MULPS` NaN rule (measured on the target's instruction): a quiet
/// NaN first source wins; otherwise a quiet second source wins over a
/// signaling first; otherwise the signaling NaN is quieted. A lone NaN is
/// quieted whatever its position.
#[inline]
pub fn sse_mul(a: f32, b: f32) -> f32 {
    if a.is_nan() {
        if is_signaling_nan(a) && b.is_nan() && !is_signaling_nan(b) {
            sse_nan(b)
        } else {
            sse_nan(a)
        }
    } else if b.is_nan() {
        sse_nan(b)
    } else {
        a * b
    }
}

/// `ADDSS`/`ADDPS` operand rule, the same shape as [`sse_mul`].
#[inline]
pub fn sse_add(a: f32, b: f32) -> f32 {
    if a.is_nan() {
        if is_signaling_nan(a) && b.is_nan() && !is_signaling_nan(b) {
            sse_nan(b)
        } else {
            sse_nan(a)
        }
    } else if b.is_nan() {
        sse_nan(b)
    } else {
        a + b
    }
}

/// `MoverRampScalar` (`0x140ca75f0`): `minss(timer, 1) * scale`, with the
/// CMOVZ scale slot.
#[inline]
pub fn ramp_scale(timer: f32, flag_bit_set: bool, scale_clear: f32, scale_set: f32) -> f32 {
    let scale = if flag_bit_set { scale_set } else { scale_clear };
    sse_mul(minss(timer, 1.0), scale)
}

/// `MoverRampScalarOffset` (`0x140ca7570`): `ramp_scale + body[0x10fc]`.
#[inline]
pub fn ramp_offset(
    timer: f32,
    flag_bit_set: bool,
    scale_clear: f32,
    scale_set: f32,
    offset: f32,
) -> f32 {
    sse_add(
        ramp_scale(timer, flag_bit_set, scale_clear, scale_set),
        offset,
    )
}

/// The read ramp vector: `ramp_offset` broadcast and multiplied lane-wise by
/// `body[0xc0..0xcc]`. The scalar and lane multiplies follow the SSE operand
/// order, so a NaN result keeps the client's payload and sign.
#[inline]
pub fn ramp_vector(
    timer: f32,
    flag_bit_set: bool,
    scale_clear: f32,
    scale_set: f32,
    offset: f32,
    direction: [f32; 4],
) -> [f32; 4] {
    let ramp = ramp_offset(timer, flag_bit_set, scale_clear, scale_set, offset);
    [
        sse_mul(ramp, direction[0]),
        sse_mul(ramp, direction[1]),
        sse_mul(ramp, direction[2]),
        sse_mul(ramp, direction[3]),
    ]
}

/// The body fields `BuildMoverPhysicsCommand` (`0x140cb74a0`) reads.
///
/// `gate` is the result of the client's movement gate (`0x140ca7d80`); the
/// command builder calls it first and zeroes the command when it is false.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MoverCommandState {
    pub gate: bool,
    /// `+0x1a79`: the state byte.
    pub state_1a79: u8,
    /// `+0x652`: the movement-enabled byte.
    pub flag_652: bool,
    /// `+0x1a7a`: the rotated state byte.
    pub byte_1a7a: u8,
    /// `+0x1a7b`: the first state selector.
    pub byte_1a7b: bool,
    /// `+0x1a75`: the second state selector.
    pub byte_1a75: bool,
    /// `+0x1a60`: the integer state selector.
    pub state_1a60: i32,
    /// `+0x1a7f`: the air/ground selector.
    pub byte_1a7f: bool,
    /// `+0x23c`: the action word copied to the command's tail.
    pub action_23c: u16,
}

/// The read command builder: six written bytes. The caller's bytes 6..8 are
/// not written and stay as the caller left them.
///
/// The first word (`uVar8`) is selected by `+0x1a7b`, `+0x1a75` and
/// `+0x1a60`; the second word is the action block (`0x10a1`, `0x14a1` or
/// `0x14b9` plus the two masks); the third word is `+0x23c`. The gate-false
/// path writes four zero bytes and then the action word.
pub fn build_physics_command(s: &MoverCommandState) -> [u8; 6] {
    let mut first: u16 = 8;
    if s.byte_1a7b {
        first = 0x18;
    } else if s.byte_1a75 {
        first = 2;
    } else if s.state_1a60 != 0 {
        first = if s.state_1a60 == 1 { 0x10 } else { 0x18 };
    }

    let action = s.action_23c;
    if !s.gate {
        return [0, 0, 0, 0, action.to_le_bytes()[0], action.to_le_bytes()[1]];
    }

    let b = s.state_1a79.wrapping_add(0x59) ^ 0x71;
    let second = if ((b.wrapping_add(0x14) >> 1) == 0 && (b & 1) == 0) || !s.flag_652 {
        0x10a1
    } else {
        let mut word: u16 = if !s.byte_1a75 || s.state_1a60 == 4 {
            0x14b9
        } else {
            0x14a1
        };
        let rotated = s.byte_1a7a.rotate_right(1).wrapping_add(0x4d);
        if rotated.rotate_left(1) == 0xc5 {
            let mask = match s.state_1a60 {
                0 => Some(0xfff7u16),
                1 => Some(0xffefu16),
                _ => None,
            };
            if let Some(mask) = mask {
                word &= mask;
            }
        }
        if !s.byte_1a7f && (s.state_1a60 == 0 || s.state_1a60 == 1) {
            let clear = if s.state_1a60 == 0 { 8u16 } else { 0x10u16 };
            word &= !clear;
        }
        word
    };

    let first = first.to_le_bytes();
    let second = second.to_le_bytes();
    [
        first[0],
        first[1],
        second[0],
        second[1],
        action.to_le_bytes()[0],
        action.to_le_bytes()[1],
    ]
}

/// Read an `f32` from a raw client struct.
#[inline]
fn f32_at(bytes: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

#[inline]
fn set_f32(bytes: &mut [u8], offset: usize, value: f32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

/// The client's fixed-point quantiser: `floor(v * scale + 0.5) * inverse`.
///
/// It is the leaf at `0x140cae130` (pair values, scale 32768) and
/// `0x140cae050` (spring velocity, scale 1024). `CVTTSS2SI` truncates toward
/// zero; the `CMOVBE` subtracts one when the truncated float is above the
/// input, which makes the pair a floor.
#[inline]
pub fn quantize(value: f32, scale: f32, inverse: f32) -> f32 {
    let scaled = value * scale + 0.5;
    let truncated = scaled as i32;
    let rounded = if (truncated as f32) <= scaled {
        truncated
    } else {
        truncated - 1
    };
    (rounded as f32) * inverse
}

/// `FUN_140ca02d0` (`0x140ca02d0`): re-normalise the pair object's two values
/// against the clamp pair, then quantise both to 1/32768 and write the
/// effective values back. `object` is `body + 0x10d0`; `pair` is
/// `body + 0x688`.
#[allow(clippy::manual_clamp)] // The client's order and NaN behaviour, not f32::clamp.
pub fn pair_update(object: &mut [u8], pair: [f32; 2]) {
    let min = pair[0];
    let max = pair[1];
    let span = max - min;
    if span <= 1.1920929e-7 {
        object[0x34..0x3c].fill(0);
        set_f32(object, 0x2c, 0.0);
        set_f32(object, 0x30, 0.0);
        return;
    }

    let inv = 1.0 / span;
    let mut a = (f32_at(object, 0x2c) - min) * inv;
    if 1.0 <= a {
        a = 1.0;
    }
    if a <= 0.0 {
        a = 0.0;
    }
    set_f32(object, 0x34, a);

    let mut b = (f32_at(object, 0x30) - min) * inv;
    if 1.0 <= b {
        b = 1.0;
    }
    if b <= 0.0 {
        b = 0.0;
    }
    set_f32(object, 0x38, b);

    let a = quantize(f32_at(object, 0x34), 32768.0, 1.0 / 32768.0);
    let b = quantize(f32_at(object, 0x38), 32768.0, 1.0 / 32768.0);
    set_f32(object, 0x34, a);
    set_f32(object, 0x38, b);
    set_f32(object, 0x2c, span * f32_at(object, 0x34) + min);
    set_f32(object, 0x30, span * f32_at(object, 0x38) + min);
}

/// `MoverWishSpring` (`0x140cc0dc0`): the wish copy and the `+0x10fc` /
/// `+0x1100` spring.
///
/// `body` is the mover body (at least `0x110d` bytes); `input` is the
/// caller's input object; `dt` is the frame delta. `world_velocity` is the
/// value the client's `FUN_142cbcec0(body[0x1ad8])` supplies on the
/// non-matching state branch. The unit's fixture pins the matching branch
/// (state id `0xffffffff`), where `FUN_140ca5d00` is the identity because the
/// state lookup sentinel returns false.
pub fn wish_spring(body: &mut [u8], input: &[u8], dt: f32, world_velocity: [f32; 4]) {
    let flags = u32::from_le_bytes(body[0x10d0..0x10d4].try_into().unwrap());
    if input[0x98] != 0 && (flags & 0x0080_000c) == 0 {
        body[0x1140..0x1150].copy_from_slice(&input[0x60..0x70]);
    }

    let wish = [
        f32_at(body, 0x1140),
        f32_at(body, 0x1144),
        f32_at(body, 0x1148),
        f32_at(body, 0x114c),
    ];
    let local_48 = wish;

    let state = (
        u32::from_le_bytes(body[0x1520..0x1524].try_into().unwrap()),
        u32::from_le_bytes(body[0x1524..0x1528].try_into().unwrap()),
    );
    let current = (
        u32::from_le_bytes(body[0x78..0x7c].try_into().unwrap()),
        u32::from_le_bytes(body[0x7c..0x80].try_into().unwrap()),
    );

    let terms = if state == current {
        let local_58 = [f32_at(body, 0xc0), f32_at(body, 0xc4), f32_at(body, 0xc8)];
        let local_68 = [f32_at(body, 0x90), f32_at(body, 0x94), f32_at(body, 0x98)];
        let f2 = f32_at(body, 0x80);
        let f3 = f32_at(body, 0x84);
        let f4 = f32_at(body, 0x88);
        [
            ((f2 + local_68[0]) - wish[0]) * local_58[0],
            ((f3 + local_68[1]) - wish[1]) * local_58[1],
            ((f4 + local_68[2]) - wish[2]) * local_58[2],
        ]
    } else {
        let local_58 = local_48;
        [
            (world_velocity[0] - local_58[0]) * f32_at(body, 0xc0),
            (world_velocity[1] - local_58[1]) * f32_at(body, 0xc4),
            (world_velocity[2] - local_58[2]) * f32_at(body, 0xc8),
        ]
    };

    let ramp = ramp_scale(
        f32_at(body, TIMER),
        body[FLAGS_BLOCK] & SCALE_BIT != 0,
        f32_at(body, SCALE_CLEAR),
        f32_at(body, SCALE_SET),
    );
    let residual = (terms[1] + terms[0] + terms[2]) - ramp;
    let mut velocity = f32_at(body, 0x110c);

    let (mut spring, mut decel) = if residual.abs() <= 0.001953125 {
        set_f32(body, 0x110c, 0.0);
        (0.0, 0.0)
    } else {
        let k = f32_at(body, 0x5c0);
        let c = f32_at(body, 0x5c4);
        let filter = k * 6.2831855 * dt;
        velocity =
            (velocity - residual * dt * 39.47842 * k * k) / ((c + c + filter) * filter + 1.0);
        set_f32(body, 0x110c, velocity);
        let next = velocity * dt + residual;
        let old = f32_at(body, 0x10fc);
        let delta = (next - old).abs();
        let mut spring = next;
        if next.abs() <= 0.001953125 {
            spring = 0.0;
            if dt <= 1.1920929e-7 {
                set_f32(body, 0x110c, 0.0);
            } else {
                set_f32(body, 0x110c, (-1.0 / dt) * residual);
            }
        }
        let mut decel = f32_at(body, 0x1100);
        if decel != 0.0 {
            if delta <= decel.abs() {
                let signed = if decel <= 0.0 { -delta } else { delta };
                decel -= signed;
            } else {
                decel = 0.0;
            }
        }
        (spring, decel)
    };

    let min = f32_at(body, 0x688);
    let max = f32_at(body, 0x68c);
    if max <= spring {
        spring = max;
    }
    if spring <= min {
        spring = min;
    }
    set_f32(body, 0x10fc, spring);
    if max <= decel {
        decel = max;
    }
    if decel <= min {
        decel = min;
    }
    set_f32(body, 0x1100, decel);
    let pair = [f32_at(body, 0x688), f32_at(body, 0x68c)];
    pair_update(&mut body[0x10d0..], pair);
    let quantised = quantize(f32_at(body, 0x110c), 1024.0, 1.0 / 1024.0);
    set_f32(body, 0x110c, quantised);
}

/// `MoverWishForward` (`0x140cb5510`): the forward 4x4 column-major affine.
///
/// The client's SIMD order is `((y*c1 + x*c0) + z*c2) + c3`, one f32 per
/// operation; `v[3]` is overwritten by the transform's w lane and the input w
/// is not used.
pub fn forward_transform(v: &mut [f32; 4], matrix: &[f32; 16]) {
    let x = v[0];
    let y = v[1];
    let z = v[2];
    for lane in 0..4 {
        let a = y * matrix[4 + lane] + x * matrix[lane];
        let b = a + z * matrix[8 + lane];
        v[lane] = b + matrix[12 + lane];
    }
}

/// `FUN_140ca7700`: copy the input into the output, then apply the resolved
/// state transform when one exists.
///
/// The client copies first, resolves the state handle (`body+0x19b0` or
/// `FUN_140c9f000(body+0x10d0)`) and the service field (`[body[8]+0x28]`),
/// then calls `FUN_140cb5510`, which is the verified forward transform. The
/// lookup itself (`FUN_140cb83b0` / `FUN_140c71e10`) is a separate open stage;
/// `matrix` is its resolved output, `None` when the lookup declines.
pub fn wish_forward_copy(out: &mut [f32; 4], input: &[f32; 4], matrix: Option<&[f32; 16]>) {
    let v = *input;
    *out = v;
    if let Some(m) = matrix {
        // Same operation order as `forward_transform`; kept inline so the
        // mutation stage has the transform's arithmetic in this function.
        let x = v[0];
        let y = v[1];
        let z = v[2];
        for lane in 0..4 {
            let a = y * m[4 + lane] + x * m[lane];
            let b = a + z * m[8 + lane];
            out[lane] = b + m[12 + lane];
        }
    }
}

/// `SHUFPS a, b, imm` with the same lane selection the client uses.
#[inline]
fn shufps(a: [f32; 4], b: [f32; 4], imm: u8) -> [f32; 4] {
    [
        a[(imm & 0x3) as usize],
        a[((imm >> 2) & 0x3) as usize],
        b[((imm >> 4) & 0x3) as usize],
        b[((imm >> 6) & 0x3) as usize],
    ]
}

#[inline]
fn vmul4(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [a[0] * b[0], a[1] * b[1], a[2] * b[2], a[3] * b[3]]
}

#[inline]
fn vadd4(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3]]
}

#[inline]
fn vsub4(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2], a[3] - b[3]]
}

/// `XORPS` with the sign mask: negation that keeps NaN payloads and signed
/// zero bits exactly as the client's vector unit does.
#[inline]
fn vneg4(a: [f32; 4]) -> [f32; 4] {
    a.map(|value| f32::from_bits(value.to_bits() ^ 0x8000_0000))
}

/// `MoverWishInverse` (`0x140cb5350`): the inverse 4x4 affine.
///
/// The client computes `det = c0 . (c1 x c2)`, `R^-1` from the three cross
/// products, then `v' = R^-1 * (v - c3)`; the SIMD sequence is transcribed
/// lane for lane, including the `rcpps` + `(2 - rcp*x)*rcp` refinement. The
/// harness's Unicorn `rcpps` is exact IEEE `1/x` (probed), which Rust's `f32`
/// division reproduces bit for bit.
pub fn inverse_transform(v: &mut [f32; 4], matrix: &[f32; 16]) {
    let c0 = [matrix[0], matrix[1], matrix[2], matrix[3]];
    let c1 = [matrix[4], matrix[5], matrix[6], matrix[7]];
    let c2 = [matrix[8], matrix[9], matrix[10], matrix[11]];
    let c3 = [matrix[12], matrix[13], matrix[14], matrix[15]];

    let mut xmm2 = c0;
    let mut xmm5 = shufps(c2, c2, 0xd2);
    let xmm6 = shufps(c1, c1, 0xd2);
    let mut xmm7 = shufps(c0, c0, 0xd2);
    let xmm8 = shufps(c1, c1, 0xc9);
    let mut xmm9 = shufps(c2, c2, 0xc9);
    let mut xmm11 = shufps(c0, c0, 0xc9);
    let mut xmm0 = xmm9;
    let mut xmm4 = xmm5;

    xmm5 = vmul4(xmm5, xmm11);
    xmm4 = vmul4(xmm4, xmm8);
    xmm0 = vmul4(xmm0, xmm6);
    xmm9 = vmul4(xmm9, xmm7);
    xmm11 = vmul4(xmm11, xmm6);
    xmm4 = vsub4(xmm4, xmm0);
    xmm7 = vmul4(xmm7, xmm8);
    xmm9 = vsub4(xmm9, xmm5);
    xmm11 = vsub4(xmm11, xmm7);

    let mut xmm1 = vmul4(xmm4, xmm2);
    xmm0 = shufps(xmm1, xmm1, 0x00);
    xmm2 = shufps(xmm1, xmm1, 0x55);
    xmm2 = vadd4(xmm2, xmm0);
    xmm1 = shufps(xmm1, xmm1, 0xaa);
    xmm2 = vadd4(xmm2, xmm1);
    let det = xmm2;

    let rcp = [1.0 / det[0], 1.0 / det[1], 1.0 / det[2], 1.0 / det[3]];
    xmm0 = vmul4(rcp, det);
    let mut xmm3 = vsub4([2.0f32; 4], xmm0);
    xmm3 = vmul4(xmm3, rcp);

    xmm0 = shufps(c3, c3, 0x00);
    xmm9 = vmul4(xmm9, xmm3);
    let mut xmm10 = vmul4(xmm3, xmm4);
    xmm4 = shufps(c3, c3, 0x55);
    xmm11 = vmul4(xmm11, xmm3);
    let input = *v;
    xmm5 = xmm10;
    xmm1 = shufps(c3, c3, 0xaa);
    xmm2 = xmm10;
    xmm5 = shufps(xmm5, xmm9, 0x00);
    xmm5 = shufps(xmm5, xmm11, 0x08);
    xmm0 = vmul4(xmm0, xmm5);
    xmm2 = shufps(xmm2, xmm9, 0x55);
    xmm2 = shufps(xmm2, xmm11, 0x58);
    xmm4 = vmul4(xmm4, xmm2);
    xmm10 = shufps(xmm10, xmm9, 0xaa);
    xmm10 = shufps(xmm10, xmm11, 0xa8);
    xmm1 = vmul4(xmm1, xmm10);
    xmm4 = vadd4(xmm4, xmm0);
    xmm0 = shufps(input, input, 0x00);
    xmm0 = vmul4(xmm0, xmm5);
    xmm4 = vadd4(xmm4, xmm1);
    xmm1 = shufps(input, input, 0x55);
    xmm1 = vmul4(xmm1, xmm2);
    xmm3 = shufps(input, input, 0xaa);
    xmm4 = vneg4(xmm4);
    xmm3 = vmul4(xmm3, xmm10);
    xmm1 = vadd4(xmm1, xmm0);
    xmm1 = vadd4(xmm1, xmm3);
    xmm4 = vadd4(xmm4, xmm1);
    *v = xmm4;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(v: [f32; 4]) -> [u32; 4] {
        [
            v[0].to_bits(),
            v[1].to_bits(),
            v[2].to_bits(),
            v[3].to_bits(),
        ]
    }

    #[test]
    fn the_read_expression_is_pinned() {
        // timer 2.5 clamps to 1, bit 1 selects 7.0, offset 0.25, direction
        // (0.5, -1, 2, -0). Each product is one f32 multiply.
        let got = ramp_vector(2.5, true, 3.0, 7.0, 0.25, [0.5, -1.0, 2.0, -0.0]);
        assert_eq!(
            bits(got),
            bits([3.625, -7.25, 14.5, -0.0]),
            "read ramp bits"
        );
    }

    #[test]
    fn the_scale_bit_selects_the_read_slot() {
        assert_eq!(ramp_vector(1.0, false, 3.0, 7.0, 0.0, [1.0; 4]), [3.0; 4]);
        assert_eq!(ramp_vector(1.0, true, 3.0, 7.0, 0.0, [1.0; 4]), [7.0; 4]);
    }

    #[test]
    fn the_timer_clamps_at_one_and_never_rises() {
        for timer in [0.0, 0.25, 1.0, 1.5, 100.0] {
            let got = ramp_vector(timer, false, 2.0, 0.0, 0.0, [1.0; 4]);
            let expected = timer.min(1.0) * 2.0;
            assert_eq!(got[0], expected, "timer {timer}");
        }
    }

    #[test]
    fn minss_takes_the_second_operand_on_nan_and_equal_zeros() {
        assert_eq!(minss(f32::NAN, 1.0).to_bits(), 1.0f32.to_bits());
        assert_eq!(minss(1.0, f32::NAN).to_bits(), f32::NAN.to_bits());
        assert_eq!(minss(-0.0, 0.0).to_bits(), 0.0f32.to_bits());
        assert_eq!(minss(0.0, -0.0).to_bits(), (-0.0f32).to_bits());
        assert_eq!(minss(0.5, 1.0), 0.5);
        assert_eq!(minss(2.0, 1.0), 1.0);
    }

    #[test]
    fn the_sse_nan_operand_order_is_pinned() {
        let negative_nan = f32::from_bits(0xFFC0_0000);
        let positive_nan = f32::from_bits(0x7FC0_0000);
        let signaling = f32::from_bits(0x7FA0_0000);
        // One NaN: the NaN is quieted, whatever its position.
        assert_eq!(sse_mul(1.0, negative_nan).to_bits(), 0xFFC0_0000);
        assert_eq!(sse_mul(negative_nan, 1.0).to_bits(), 0xFFC0_0000);
        assert_eq!(sse_mul(1.0, signaling).to_bits(), 0x7FE0_0000);
        // Both quiet: the first source wins (measured on the target).
        assert_eq!(sse_mul(negative_nan, positive_nan).to_bits(), 0xFFC0_0000);
        assert_eq!(sse_add(negative_nan, positive_nan).to_bits(), 0xFFC0_0000);
        // A quiet NaN beats a signaling one in the other position.
        assert_eq!(sse_mul(positive_nan, signaling).to_bits(), 0x7FC0_0000);
        assert_eq!(sse_mul(signaling, positive_nan).to_bits(), 0x7FC0_0000);
    }

    #[test]
    fn the_command_builder_matches_the_read_branches() {
        let base = MoverCommandState {
            gate: true,
            state_1a79: 0x44,
            flag_652: true,
            action_23c: 0x1234,
            ..Default::default()
        };
        // Ground state, no modifiers: first = 8, action block = 0x10a1.
        assert_eq!(
            build_physics_command(&base),
            [0x08, 0x00, 0xa1, 0x10, 0x34, 0x12]
        );
        // Gate closed: four zero bytes, then the action word.
        assert_eq!(
            build_physics_command(&MoverCommandState {
                gate: false,
                ..base
            }),
            [0, 0, 0, 0, 0x34, 0x12]
        );
        // The 0x1a7b selector wins over 0x1a75 and 0x1a60.
        assert_eq!(
            build_physics_command(&MoverCommandState {
                byte_1a7b: true,
                byte_1a75: true,
                state_1a60: 4,
                ..base
            })[0..2],
            [0x18, 0x00]
        );
    }

    #[test]
    fn the_wish_copy_without_a_transform_is_a_copy() {
        let mut out = [9.0; 4];
        let input = [1.0, 2.0, 3.0, 4.0];
        wish_forward_copy(&mut out, &input, None);
        assert_eq!(out, input);
    }

    #[test]
    fn the_wish_copy_applies_the_forward_transform() {
        let mut m = [0.0f32; 16];
        m[0] = 1.0;
        m[5] = 1.0;
        m[10] = 1.0;
        m[15] = 1.0;
        m[12] = 1.0;
        m[13] = 2.0;
        m[14] = 3.0;
        let mut out = [0.0; 4];
        wish_forward_copy(&mut out, &[1.0, 2.0, 3.0, 0.0], Some(&m));
        assert_eq!(out, [2.0, 4.0, 6.0, 1.0]);
    }

    #[test]
    fn the_inverse_transform_undoes_a_rotation_and_translation() {
        // 90 degrees about z: forward maps (1,0,0) to (0,1,0); the inverse
        // subtracts the translation and rotates back.
        let mut m = [0.0f32; 16];
        m[0] = 0.0;
        m[1] = 1.0;
        m[4] = -1.0;
        m[5] = 0.0;
        m[10] = 1.0;
        m[12] = 1.0;
        m[13] = 2.0;
        m[14] = 3.0;
        m[15] = 1.0;
        let mut v = [1.0, 3.0, 3.0, 1.0];
        inverse_transform(&mut v, &m);
        assert_eq!(v, [1.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn the_inverse_transform_subtracts_the_translation() {
        let mut m = [0.0f32; 16];
        m[0] = 1.0;
        m[5] = 1.0;
        m[10] = 1.0;
        m[15] = 1.0;
        m[12] = 1.0;
        m[13] = 2.0;
        m[14] = 3.0;
        let mut v = [2.0, 4.0, 6.0, 1.0];
        inverse_transform(&mut v, &m);
        // The client transforms all four lanes: the row vectors carry a w
        // component derived from the shuffled columns, so w' is 3 here.
        assert_eq!(v, [1.0, 2.0, 3.0, 3.0]);
    }

    #[test]
    fn the_forward_transform_is_column_major() {
        let matrix: [f32; 16] = std::array::from_fn(|i| (i + 1) as f32);
        let mut v = [1.0, 2.0, 3.0, 4.0];
        forward_transform(&mut v, &matrix);
        // x*c0 + y*c1 + z*c2 + c3, lane by lane.
        assert_eq!(v, [51.0, 58.0, 65.0, 72.0]);
    }

    #[test]
    fn the_ramp_nan_channel_matches_the_client_bits() {
        // The i=248 fixture case: timer -2.5, scale_clear = -qNaN,
        // offset 2.0, direction (0x0C4C769C, -0.0, FLT_MAX, +qNaN).
        let negative_nan = f32::from_bits(0xFFC0_0000);
        let positive_nan = f32::from_bits(0x7FC0_0000);
        let got = ramp_vector(
            -2.5,
            false,
            negative_nan,
            0.0,
            2.0,
            [f32::from_bits(0x0C4C_769C), -0.0, f32::MAX, positive_nan],
        );
        // Lane 3 is the both-quiet-NaN multiply: the first source (the
        // negative scale) wins.
        assert_eq!(
            bits(got),
            [0xFFC0_0000, 0xFFC0_0000, 0xFFC0_0000, 0xFFC0_0000]
        );
    }
}

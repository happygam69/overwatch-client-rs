//! The collision sweep's bounds stage, read from `Overwatch.exe`.
//!
//! `0x142cd5540` is the first stage of the swept collision query
//! `0x142cbd510`. It builds a rotation from the query block's local
//! quaternion, transforms the block's local support points, folds them into a
//! centre and a half-extent, and writes three quads:
//!
//! ```text
//! start     = P0  + (max + min) * 0.5
//! end       = P1  + (max + min) * 0.5
//! expansion = off + (max - min) * 0.5
//! ```
//!
//! `min`/`max` are the lane-wise fold over the transformed support points.
//! The query block (`0x80` bytes, built by the handle dispatcher
//! `0x142cc45a0`) holds:
//!
//! | offset | field |
//! |---|---|
//! | `+0x00` | `P0`, the sweep's start position (vec4) |
//! | `+0x10` | `P1`, the sweep's end position (vec4) |
//! | `+0x30` | pointer to the local support points (vec4 each) |
//! | `+0x38` | point count (`i32`) |
//! | `+0x40` | local rotation quaternion `(x, y, z, w)` (vec4) |
//! | `+0x50` | `off`, the expansion base (vec4) |
//!
//! The arithmetic is the client's, operation for operation: `2q` is the
//! `ADDPS` of the quaternion with itself, the diagonal terms subtract the
//! absolute squares in the order `(1 - |2y^2|) - |2z^2|`, a point is
//! `(px*mx + py*my) + pz*mz` in that order per lane, and the fold is
//! `MINPS`/`MAXPS`, whose rule is "the second operand wins on NaN" - the same
//! rule `movement::minss`/`maxss` implement.
//!
//! The fourth lane of a rotated point is `(px + py + pz) * 0`: the diagonal's
//! fourth lane is the constant's zero and the absolute-value mask zeroes the
//! fourth lane. It is kept even though nothing consumes it, because the client
//! writes all four lanes and the harness compares them.

use crate::movement::{maxss, minss};

/// A four-lane value in the client's register layout.
pub type Quad = [f32; 4];

/// The constant the diagonal is built from (`0x1430a4bf0`): `(1, 1, 1, 0)`.
const DIAG_BASE: Quad = [1.0, 1.0, 1.0, 0.0];

/// The halving constant (`0x1435ecfc0`): `(0.5, 0.5, 0.5, 0.5)`.
const HALF: Quad = [0.5, 0.5, 0.5, 0.5];

fn f32_at(bytes: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn quad_at(bytes: &[u8], offset: usize) -> Quad {
    [
        f32_at(bytes, offset),
        f32_at(bytes, offset + 4),
        f32_at(bytes, offset + 8),
        f32_at(bytes, offset + 12),
    ]
}

/// `0x7fffffff` on lanes 0..2 and zero on lane 3 (the constant at
/// `0x1430a4b40`): the absolute-value mask the diagonal uses.
#[inline]
fn abs_lane(value: f32) -> f32 {
    f32::from_bits(value.to_bits() & 0x7fff_ffff)
}

/// The three per-lane multiplier quads the leaf builds from the block's
/// quaternion. `x * rows[0] + y * rows[1] + z * rows[2]` per lane rotates a
/// point; the quaternion is used as stored (the leaf never normalises it).
///
/// The diagonal lanes are `1 - 2y^2 - 2z^2`, `1 - 2x^2 - 2z^2`,
/// `1 - 2x^2 - 2y^2` and the fourth lane is the constant's `0`. The
/// off-diagonal terms are `2(wy + zx)`, `2(wz + yx)`, `2(wx + zy)` on the first
/// row and `2(zx - wy)`, `2(yx - wz)`, `2(zy - wx)` on the third.
pub fn rotation_rows(q: Quad) -> [Quad; 3] {
    // ADDPS: the leaf doubles the quaternion in place.
    let doubled = [q[0] + q[0], q[1] + q[1], q[2] + q[2], q[3] + q[3]];
    // MULPS: `2q * q`, lane for lane.
    let square = [
        doubled[0] * q[0],
        doubled[1] * q[1],
        doubled[2] * q[2],
        doubled[3] * q[3],
    ];
    let diagonal = [
        (DIAG_BASE[0] - abs_lane(square[1])) - abs_lane(square[2]),
        (DIAG_BASE[1] - abs_lane(square[0])) - abs_lane(square[2]),
        (DIAG_BASE[2] - abs_lane(square[0])) - abs_lane(square[1]),
        (DIAG_BASE[3] - 0.0) - 0.0,
    ];
    // T = (2z, 2y, 2z, 2w) * (x, x, y, y)
    let cross = [
        doubled[2] * q[0],
        doubled[1] * q[0],
        doubled[2] * q[1],
        doubled[3] * q[1],
    ];
    // W = (w, w, w, w) * (2y, 2z, 2x, 2w)
    let spin = [
        q[3] * doubled[1],
        q[3] * doubled[2],
        q[3] * doubled[0],
        q[3] * doubled[3],
    ];
    // ADDPS W + T, SUBPS T - W: the register order the leaf uses.
    let mut sum = [0.0f32; 4];
    let mut difference = [0.0f32; 4];
    for lane in 0..4 {
        sum[lane] = spin[lane] + cross[lane];
        difference[lane] = cross[lane] - spin[lane];
    }
    [
        [diagonal[0], sum[1], difference[0], diagonal[3]],
        [difference[1], diagonal[1], sum[2], diagonal[3]],
        [sum[0], difference[2], diagonal[2], diagonal[3]],
    ]
}

/// Rotate one local support point with the rows `rotation_rows` built.
///
/// The leaf's lane order is `(px*mx) + py*my`, then `+ pz*mz`; that order is
/// kept because a re-associated sum differs in the last bit.
#[inline]
pub fn transform_point(p: [f32; 3], rows: &[Quad; 3]) -> Quad {
    let mut out = [0.0f32; 4];
    for (lane, value) in out.iter_mut().enumerate() {
        let x = p[0] * rows[0][lane];
        let y = x + p[1] * rows[1][lane];
        *value = y + p[2] * rows[2][lane];
    }
    out
}

/// `0x142cd5540`: the sweep's start, end and expansion quads, in that order.
///
/// `block` is the query block; `points` is the local support-point array it
/// points at (`vec4` each, `block[0x38]` entries). A non-positive count leaves
/// the three outputs equal to `P0`, `P1` and `off` unchanged - the leaf skips
/// the transform entirely on that path.
pub fn sweep_bounds(block: &[u8], points: &[u8]) -> (Quad, Quad, Quad) {
    let p0 = quad_at(block, 0x00);
    let p1 = quad_at(block, 0x10);
    let count = i32::from_le_bytes(block[0x38..0x3c].try_into().unwrap());
    let offset = quad_at(block, 0x50);

    if count <= 0 {
        return (p0, p1, offset);
    }

    let rows = rotation_rows(quad_at(block, 0x40));
    let mut low = transform_point(
        [f32_at(points, 0), f32_at(points, 4), f32_at(points, 8)],
        &rows,
    );
    let mut high = low;
    for index in 1..count as usize {
        let base = index * 0x10;
        let point = transform_point(
            [
                f32_at(points, base),
                f32_at(points, base + 4),
                f32_at(points, base + 8),
            ],
            &rows,
        );
        for lane in 0..4 {
            low[lane] = minss(low[lane], point[lane]);
            high[lane] = maxss(high[lane], point[lane]);
        }
    }

    let mut centre = [0.0f32; 4];
    let mut half_extent = [0.0f32; 4];
    for lane in 0..4 {
        centre[lane] = (high[lane] + low[lane]) * HALF[lane];
        half_extent[lane] = (high[lane] - low[lane]) * HALF[lane];
    }

    let mut start = [0.0f32; 4];
    let mut end = [0.0f32; 4];
    let mut expansion = [0.0f32; 4];
    for lane in 0..4 {
        start[lane] = p0[lane] + centre[lane];
        end[lane] = p1[lane] + centre[lane];
        expansion[lane] = offset[lane] + half_extent[lane];
    }
    (start, end, expansion)
}

// ---------------------------------------------------------------------------
// The type-0 narrowphase (`0x142ccdf40`), no-support-point path.
// ---------------------------------------------------------------------------

/// The "is this vector non-degenerate" squared-length epsilon
/// (`0x143110818` and `0x143391030`, the same value splatted).
#[inline]
fn eps() -> f32 {
    f32::from_bits(0x057a_0000)
}

#[inline]
fn splat(v: f32) -> Quad {
    [v; 4]
}

#[inline]
fn rot3(v: Quad) -> Quad {
    // `SHUFPS v, v, 0xc9`: (y, z, x, w).
    [v[1], v[2], v[0], v[3]]
}

#[inline]
fn mul_lane(a: Quad, b: Quad) -> Quad {
    [a[0] * b[0], a[1] * b[1], a[2] * b[2], a[3] * b[3]]
}

#[inline]
fn add_lane(a: Quad, b: Quad) -> Quad {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3]]
}

#[inline]
fn sub_lane(a: Quad, b: Quad) -> Quad {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2], a[3] - b[3]]
}

/// Flip every lane's sign by XOR with the sign mask (`0x142f817a0`).
#[inline]
fn negate(v: Quad) -> Quad {
    [
        f32::from_bits(v[0].to_bits() ^ 0x8000_0000),
        f32::from_bits(v[1].to_bits() ^ 0x8000_0000),
        f32::from_bits(v[2].to_bits() ^ 0x8000_0000),
        f32::from_bits(v[3].to_bits() ^ 0x8000_0000),
    ]
}

/// The client's three-term fold: `(a.x*b.x + a.y*b.y) + a.z*b.z`, splatted.
///
/// The shuffles `0x99` then `0x55` pick `y`, `z` and add them to lane 0 in
/// that order; the result is broadcast with `SHUFPS 0x00`. The same fold
/// computes the squared length when both arguments are the same quad - the
/// product is taken before the fold, never the square of the fold.
#[inline]
fn dot3(a: Quad, b: Quad) -> Quad {
    let products = mul_lane(a, b);
    splat((products[0] + products[1]) + products[2])
}

/// True when the predicate holds on all four lanes (`CMPLTPS` + `MOVMSKPS`
/// compared with `0xf`): a NaN lane makes the predicate false.
#[inline]
fn all_lanes(mut predicate: impl FnMut(usize) -> bool) -> bool {
    (0..4).all(&mut predicate)
}

/// `SQRTPS`: one square root per lane, so the fourth lane stays independent.
#[inline]
fn per_lane_sqrt(v: Quad) -> Quad {
    [v[0].sqrt(), v[1].sqrt(), v[2].sqrt(), v[3].sqrt()]
}

/// `dot3(v, v)`: the squared length, splatted.
#[inline]
fn length_sq(v: Quad) -> Quad {
    dot3(v, v)
}

/// World -> local rotation.
///
/// `0x142cce0c0..0x142cce0fe`: the first stage subtracts `rot3(v) * q` from
/// `q_rot * v` and folds the quaternion's `w` term in, the second stage runs
/// the same cross/dot pair again, doubles it and adds `v`.
#[inline]
fn rotate_to_local(q: Quad, v: Quad) -> Quad {
    let qr = rot3(q);
    let cross = sub_lane(mul_lane(qr, v), mul_lane(rot3(v), q));
    let spin = mul_lane(splat(q[3]), v);
    let s = add_lane(rot3(cross), spin);
    let cross2 = sub_lane(mul_lane(s, qr), mul_lane(rot3(s), q));
    let doubled = add_lane(rot3(cross2), rot3(cross2));
    add_lane(doubled, v)
}

/// Local -> world rotation, contact-point flavour (`0x142cce317..0x142cce35a`).
#[inline]
fn rotate_to_world_point(q: Quad, v: Quad) -> Quad {
    let qr = rot3(q);
    let cross = sub_lane(mul_lane(rot3(v), q), mul_lane(v, qr));
    let spin = mul_lane(v, splat(q[3]));
    let s = add_lane(rot3(cross), spin);
    let cross2 = sub_lane(mul_lane(rot3(s), q), mul_lane(s, qr));
    let doubled = add_lane(rot3(cross2), rot3(cross2));
    add_lane(doubled, v)
}

/// Local -> world rotation, normal flavour (`0x142cce369..0x142cce3af`).
///
/// The same algebra as [`rotate_to_world_point`], with the products and the
/// first sum written in the client's other operand order - kept separate so
/// the NaN payload a signaling value carries survives identically.
#[inline]
fn rotate_to_world_normal(q: Quad, v: Quad) -> Quad {
    let qr = rot3(q);
    let cross = sub_lane(mul_lane(rot3(v), q), mul_lane(qr, v));
    let spin = mul_lane(splat(q[3]), v);
    let s = add_lane(spin, rot3(cross));
    let cross2 = sub_lane(mul_lane(rot3(s), q), mul_lane(s, qr));
    let doubled = add_lane(rot3(cross2), rot3(cross2));
    add_lane(doubled, v)
}

/* The record/transform/query layouts this leaf reads. */

/// A type-0 shape record: `{center at +0x00, radius at +0x10}`.
pub const SPHERE_RECORD_LEN: usize = 0x20;
/// A shape world transform: `{position at +0x00, world->local quaternion at +0x10}`.
pub const SHAPE_XFORM_LEN: usize = 0x20;

/// `0x142ccdf40` type 0 (sphere), the no-support-point path: a swept sphere
/// against the query segment.
///
/// `block` is the `0x78`-byte query block (`+0x00` start, `+0x10` end,
/// `+0x20` motion, `+0x38` support-point count, `+0x50` expansion,
/// `+0x72` the "hit while already inside" byte). Only the count `<= 0` path is
/// implemented: the count `> 0` path hands the block to `0x142cc9460`, which
/// is unread.
///
/// The three output quads are the world contact point, the world normal and
/// the hit fraction. `None` is a miss, on which the client leaves its output
/// buffer untouched.
pub fn sphere_sweep(record: &[u8], block: &[u8], xform: &[u8]) -> Option<[Quad; 3]> {
    let center = quad_at(record, 0x00);
    let radius = splat(f32_at(record, 0x10));
    let position = quad_at(xform, 0x00);
    let q = quad_at(xform, 0x10);
    let start = quad_at(block, 0x00);
    let end = quad_at(block, 0x10);
    let motion = quad_at(block, 0x20);
    let expansion = quad_at(block, 0x50);
    let hit_inside = block[0x72] != 0;

    // The fourth lane is carried through every step: the client's quads are
    // four lanes wide and the fourth one is `r + expansion.w`, not a copy.
    let reach = add_lane(radius, expansion);
    let reach_sq = mul_lane(reach, reach);

    // The start position in the shape's local frame, relative to its centre.
    let p = sub_lane(rotate_to_local(q, sub_lane(start, position)), center);
    let p_sq = length_sq(p);

    if all_lanes(|lane| p_sq[lane] < reach_sq[lane]) {
        // The sweep starts inside the expanded sphere.
        if !hit_inside {
            return None;
        }
        let delta = sub_lane(end, start);
        let delta_sq = length_sq(delta)[0];
        let normal = if delta_sq > eps() {
            mul_lane(delta, splat(1.0f32 / delta_sq.sqrt()))
        } else {
            delta
        };
        return Some([start, normal, [0.0; 4]]);
    }

    let delta = sub_lane(end, start);
    let d = rotate_to_local(q, delta);
    let d_sq = length_sq(d)[0];
    // `COMISS` + `JBE`: `<=` and unordered both land here.
    if d_sq.is_nan() || d_sq <= eps() {
        return None;
    }
    let len = d_sq.sqrt();
    let dir = mul_lane(d, splat(1.0f32 / len));

    // Closest approach of the line through `p` along `dir` to the local origin.
    let dot = dot3(dir, p)[0];
    let offset = add_lane(mul_lane(negate(splat(dot)), dir), p);
    let offset_sq = length_sq(offset);
    if all_lanes(|lane| reach_sq[lane] < offset_sq[lane]) {
        return None;
    }
    let half_chord = per_lane_sqrt(sub_lane(reach_sq, offset_sq));
    let t = sub_lane(negate(splat(dot)), half_chord);
    if all_lanes(|lane| t[lane] < 0.0) {
        return None;
    }
    let scaled_len = mul_lane(motion, splat(len));
    if all_lanes(|lane| scaled_len[lane] < t[lane]) {
        return None;
    }

    let hit = add_lane(mul_lane(t, dir), p);
    let hit_sq = length_sq(hit)[0];
    let normal = if hit_sq > eps() {
        mul_lane(hit, splat(1.0f32 / hit_sq.sqrt()))
    } else {
        negate(dir)
    };

    let contact = add_lane(mul_lane(normal, radius), center);
    let world_point = add_lane(rotate_to_world_point(q, contact), position);
    let world_normal = rotate_to_world_normal(q, normal);

    let fraction = if eps() < len.abs() {
        splat(1.0f32 / len)
    } else {
        splat(0.0)
    };
    let scaled = mul_lane(fraction, t);
    let mut out_fraction = [0.0f32; 4];
    for lane in 0..4 {
        out_fraction[lane] = crate::movement::minss(scaled[lane], motion[lane]);
    }
    Some([world_point, world_normal, out_fraction])
}

// ---------------------------------------------------------------------------
// The query's layer filter (`0x142cbb350`).
// ---------------------------------------------------------------------------

fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
}

fn i16_at(bytes: &[u8], offset: usize) -> i16 {
    u16_at(bytes, offset) as i16
}

/// The client's `NEG` + `CMOVS` magnitude: the negation when it went
/// non-negative, the original value otherwise (so `i16::MIN` keeps `0x8000`).
#[inline]
fn abs16(v: i16) -> u16 {
    let negated = v.wrapping_neg();
    if negated < 0 {
        v as u16
    } else {
        negated as u16
    }
}

/// `0x142cbb350`: tests a shape entry's filter words against the query's.
///
/// Both arguments are eight-byte word blocks. `a` is the query descriptor's
/// filter field and `b` is the shape's: `+0`/`+2` are two 16-bit masks, `+4`
/// a signed id and `+7` a mode byte.
///
/// * `a[7] != 0`: the test is the mask-only intersection `a[2] & b[0]`.
/// * otherwise `a[2] & b[0]` and `a[0] & b[2]` must both be non-zero, then the
///   ids decide. The zero pair passes. With equal magnitudes only two positive
///   ids pass; with different magnitudes `x` must not be positive and `y` must
///   not be positive. A negative id never matches itself - the client's
///   `TEST`/`JNZ` skips the equality shortcut whenever `x` is non-zero.
pub fn shape_filter(a: &[u8], b: &[u8]) -> bool {
    let mask = u16_at(a, 2) & u16_at(b, 0);
    if a[7] != 0 {
        return mask != 0;
    }
    if mask == 0 {
        return false;
    }
    if (u16_at(a, 0) & u16_at(b, 2)) == 0 {
        return false;
    }
    let x = i16_at(a, 4);
    let y = i16_at(b, 4);
    if x == 0 && y == 0 {
        return true;
    }
    if abs16(x) == abs16(y) {
        x > 0 && y > 0
    } else if x > 0 {
        false
    } else {
        y <= 0
    }
}

/// The parallel-sweep epsilon inside the capsule's analytic path
/// (`0x1435ecd80`).
#[inline]
fn eps_parallel() -> f32 {
    f32::from_bits(0x057a_0000)
}

/// A type-1 capsule record: `{P0 at +0x00, P1 at +0x10, radius at +0x20}`.
pub const CAPSULE_RECORD_LEN: usize = 0x30;

fn sphere_record(centre: Quad, radius: f32) -> [u8; SPHERE_RECORD_LEN] {
    let mut bytes = [0u8; SPHERE_RECORD_LEN];
    for (lane, value) in centre.iter().enumerate() {
        bytes[lane * 4..lane * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[0x10..0x14].copy_from_slice(&radius.to_le_bytes());
    bytes
}

/// `0x142ccd7b0` type 1 (capsule), the no-support-point path.
///
/// The leaf has three paths and this implements the two that do not need the
/// unread support-point routine: a degenerate capsule (`|P1-P0|^2 <= eps`)
/// recurses into the sphere leaf at the midpoint, and the analytic path folds
/// the sweep into the capsule's frame and runs the line-against-segment
/// closest approach. When the entry point lands outside the segment it also
/// recurses into the sphere leaf, at whichever endpoint the axis parameter
/// clamps to.
pub fn capsule_sweep(record: &[u8], block: &[u8], xform: &[u8]) -> Option<[Quad; 3]> {
    let p0 = quad_at(record, 0x00);
    let p1 = quad_at(record, 0x10);
    let radius = f32_at(record, 0x20);
    let radius4 = splat(radius);
    let axis = sub_lane(p1, p0);
    let axis_sq = length_sq(axis);

    // `COMISS` + `JA`: NaN and `<=` both take the sphere fallback.
    if axis_sq[0].is_nan() || axis_sq[0] <= eps() {
        let midpoint = mul_lane(add_lane(p0, p1), splat(0.5));
        let sphere = sphere_record(midpoint, radius);
        return sphere_sweep(&sphere, block, xform);
    }

    let position = quad_at(xform, 0x00);
    let q = quad_at(xform, 0x10);
    let start = quad_at(block, 0x00);
    let end = quad_at(block, 0x10);
    let motion = quad_at(block, 0x20);
    let expansion = quad_at(block, 0x50);
    let hit_inside = block[0x72] != 0;

    let length = axis_sq[0].sqrt();
    let length4 = splat(length);
    let axis_dir = mul_lane(axis, splat(1.0f32 / length));
    let reach = add_lane(radius4, expansion);
    let reach_sq = mul_lane(reach, reach);

    // The start in the capsule's local frame, relative to `P0`.
    let w = sub_lane(rotate_to_local(q, sub_lane(start, position)), p0);

    let along = dot3(axis_dir, w);
    let perpendicular = sub_lane(w, mul_lane(along, axis_dir));
    let perpendicular_sq = length_sq(perpendicular);

    if all_lanes(|lane| perpendicular_sq[lane] < reach_sq[lane]) {
        // Inside the infinite cylinder: clamp the axis parameter and test the
        // capped volume.
        let clamped = crate::movement::maxss(0.0, crate::movement::minss(along[0], length4[0]));
        let clamped4 = splat(clamped);
        let to_axis = sub_lane(w, mul_lane(clamped4, axis_dir));
        let to_axis_sq = length_sq(to_axis);
        if all_lanes(|lane| to_axis_sq[lane] < reach_sq[lane]) {
            if !hit_inside {
                return None;
            }
            let delta = sub_lane(end, start);
            let delta_sq = length_sq(delta)[0];
            let normal = if delta_sq > eps() {
                mul_lane(delta, splat(1.0f32 / delta_sq.sqrt()))
            } else {
                delta
            };
            return Some([start, normal, [0.0; 4]]);
        }
        let centre = add_lane(p0, mul_lane(clamped4, axis_dir));
        let sphere = sphere_record(centre, radius);
        return sphere_sweep(&sphere, block, xform);
    }

    let delta = sub_lane(end, start);
    let d = rotate_to_local(q, delta);
    let d_sq = length_sq(d)[0];
    if d_sq.is_nan() || d_sq <= eps() {
        return None;
    }
    let dir = mul_lane(d, splat(1.0f32 / d_sq.sqrt()));

    // Closest approach between the sweep line and the capsule's axis line.
    let c = dot3(dir, axis_dir);
    let one_minus_c2 = sub_lane(splat(1.0), mul_lane(c, c));
    if all_lanes(|lane| one_minus_c2[lane] < eps_parallel()) {
        return None;
    }
    let inv = {
        let mut out = [0.0f32; 4];
        for lane in 0..4 {
            out[lane] = if eps() < one_minus_c2[lane].abs() {
                1.0 / one_minus_c2[lane]
            } else {
                0.0
            };
        }
        out
    };
    let a = dot3(axis_dir, w);
    let b = dot3(dir, w);
    let sweep_parameter = mul_lane(sub_lane(mul_lane(c, a), b), inv);
    let axis_parameter = mul_lane(sub_lane(a, mul_lane(b, c)), inv);
    let offset = sub_lane(
        add_lane(mul_lane(sweep_parameter, dir), w),
        mul_lane(axis_parameter, axis_dir),
    );
    let offset_sq = length_sq(offset);
    if all_lanes(|lane| reach_sq[lane] < offset_sq[lane]) {
        return None;
    }

    let half_chord = per_lane_sqrt(mul_lane(sub_lane(reach_sq, offset_sq), inv));
    let entry = sub_lane(sweep_parameter, half_chord);
    let entry_axis = add_lane(mul_lane(entry, c), a);

    if all_lanes(|lane| entry_axis[lane] < 0.0) {
        let sphere = sphere_record(p0, radius);
        return sphere_sweep(&sphere, block, xform);
    }
    if all_lanes(|lane| length4[lane] < entry_axis[lane]) {
        let sphere = sphere_record(p1, radius);
        return sphere_sweep(&sphere, block, xform);
    }
    if all_lanes(|lane| entry[lane] < 0.0) {
        return None;
    }
    let budget = mul_lane(motion, splat(d_sq.sqrt()));
    if all_lanes(|lane| budget[lane] < entry[lane]) {
        return None;
    }

    let swept_point = add_lane(mul_lane(entry, dir), w);
    let axis_point = mul_lane(entry_axis, axis_dir);
    let raw_normal = sub_lane(swept_point, axis_point);
    let raw_sq = length_sq(raw_normal)[0];
    let normal = if raw_sq > eps() {
        mul_lane(raw_normal, splat(1.0f32 / raw_sq.sqrt()))
    } else {
        negate(dir)
    };

    let local_hit = sub_lane(add_lane(swept_point, p0), mul_lane(normal, expansion));
    let world_contact = add_lane(rotate_to_world_point(q, local_hit), position);
    let world_normal = rotate_to_world_normal(q, normal);
    let fraction = mul_lane(entry, splat(1.0f32 / d_sq.sqrt()));
    let mut out_fraction = [0.0f32; 4];
    for lane in 0..4 {
        out_fraction[lane] = crate::movement::minss(fraction[lane], motion[lane]);
    }
    Some([world_contact, world_normal, out_fraction])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(p0: Quad, p1: Quad, quat: Quad, off: Quad, count: i32) -> Vec<u8> {
        let mut bytes = vec![0u8; 0x80];
        let mut put = |offset: usize, quad: Quad| {
            for (lane, value) in quad.iter().enumerate() {
                bytes[offset + lane * 4..offset + lane * 4 + 4]
                    .copy_from_slice(&value.to_le_bytes());
            }
        };
        put(0x00, p0);
        put(0x10, p1);
        put(0x40, quat);
        put(0x50, off);
        bytes[0x38..0x3c].copy_from_slice(&count.to_le_bytes());
        bytes
    }

    fn points(list: &[[f32; 3]]) -> Vec<u8> {
        let mut bytes = vec![0u8; list.len() * 0x10];
        for (index, p) in list.iter().enumerate() {
            for (lane, value) in p.iter().enumerate() {
                let at = index * 0x10 + lane * 4;
                bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
            }
        }
        bytes
    }

    #[test]
    fn identity_quaternion_is_the_point_cloud_bounds() {
        let q = [0.0, 0.0, 0.0, 1.0];
        for (lane, value) in rotation_rows(q).iter().enumerate() {
            assert_eq!(value[0], if lane == 0 { 1.0 } else { 0.0 });
            assert_eq!(value[1], if lane == 1 { 1.0 } else { 0.0 });
            assert_eq!(value[2], if lane == 2 { 1.0 } else { 0.0 });
            assert_eq!(value[3], 0.0);
        }
    }

    #[test]
    fn quarter_turn_about_z_swaps_x_and_y() {
        let s = std::f32::consts::FRAC_1_SQRT_2;
        let rows = rotation_rows([0.0, 0.0, s, s]);
        let rotated = transform_point([1.0, 0.0, 0.0], &rows);
        assert!((rotated[0]).abs() < 1e-6);
        assert!((rotated[1] - 1.0).abs() < 1e-6);
        assert_eq!(rotated[3], 0.0);
    }

    #[test]
    fn one_point_has_no_extent() {
        let b = block(
            [1.0, 2.0, 3.0, 0.0],
            [1.0, 2.0, 8.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
            [0.1, 0.1, 0.1, 0.0],
            1,
        );
        let pts = points(&[[4.0, -5.0, 6.0]]);
        let (start, end, expansion) = sweep_bounds(&b, &pts);
        assert_eq!(start, [5.0, -3.0, 9.0, 0.0]);
        assert_eq!(end, [5.0, -3.0, 14.0, 0.0]);
        assert_eq!(expansion, [0.1, 0.1, 0.1, 0.0]);
    }

    #[test]
    fn zero_count_leaves_the_outputs_untouched() {
        let b = block(
            [1.0, 2.0, 3.0, 0.0],
            [4.0, 5.0, 6.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
            [7.0, 8.0, 9.0, 0.0],
            0,
        );
        let (start, end, expansion) = sweep_bounds(&b, &[]);
        assert_eq!(start, [1.0, 2.0, 3.0, 0.0]);
        assert_eq!(end, [4.0, 5.0, 6.0, 0.0]);
        assert_eq!(expansion, [7.0, 8.0, 9.0, 0.0]);
    }

    #[test]
    fn minps_keeps_the_second_operand_on_nan() {
        let b = block([0.0; 4], [0.0; 4], [0.0, 0.0, 0.0, 1.0], [0.0; 4], 2);
        let pts = points(&[[1.0, 1.0, 1.0], [f32::NAN, 2.0, 3.0]]);
        let (_, _, expansion) = sweep_bounds(&b, &pts);
        // `NaN * 0` is NaN, so one NaN coordinate spreads to every lane of the
        // rotated point; that point is then the second operand of MINPS/MAXPS,
        // which is the one that survives. The extent is NaN in all lanes.
        for lane in expansion {
            assert!(lane.is_nan());
        }
    }

    fn sphere_block(start: [f32; 3], end: [f32; 3], motion: f32, flag: u8) -> Vec<u8> {
        let mut bytes = vec![0u8; 0x78];
        for (lane, value) in start.iter().enumerate() {
            bytes[lane * 4..lane * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        for (lane, value) in end.iter().enumerate() {
            let at = 0x10 + lane * 4;
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        for lane in 0..4 {
            let at = 0x20 + lane * 4;
            bytes[at..at + 4].copy_from_slice(&motion.to_le_bytes());
        }
        bytes[0x72] = flag;
        bytes
    }

    fn sphere_record(center: [f32; 3], radius: f32) -> Vec<u8> {
        let mut bytes = vec![0u8; SPHERE_RECORD_LEN];
        for (lane, value) in center.iter().enumerate() {
            bytes[lane * 4..lane * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes[0x10..0x14].copy_from_slice(&radius.to_le_bytes());
        bytes
    }

    fn identity_xform(position: [f32; 3]) -> Vec<u8> {
        let mut bytes = vec![0u8; SHAPE_XFORM_LEN];
        for (lane, value) in position.iter().enumerate() {
            bytes[lane * 4..lane * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes[0x1c..0x20].copy_from_slice(&1.0f32.to_le_bytes());
        bytes
    }

    #[test]
    fn a_sweep_through_the_centre_hits_the_near_surface() {
        let record = sphere_record([0.0, 0.0, 0.0], 1.0);
        let block = sphere_block([0.0, 0.0, 10.0], [0.0, 0.0, -10.0], 1.0, 0);
        let xform = identity_xform([0.0, 0.0, 0.0]);
        let [point, normal, fraction] = sphere_sweep(&record, &block, &xform).unwrap();
        assert!((point[0]).abs() < 1e-6 && (point[1]).abs() < 1e-6);
        assert!((point[2] - 1.0).abs() < 1e-6);
        assert!((normal[2] - 1.0).abs() < 1e-6);
        assert!((fraction[0] - 0.45).abs() < 1e-6);
    }

    #[test]
    fn a_sweep_away_from_the_sphere_misses() {
        let record = sphere_record([0.0, 0.0, 0.0], 1.0);
        let block = sphere_block([0.0, 0.0, 10.0], [0.0, 0.0, 20.0], 1.0, 1);
        let xform = identity_xform([0.0, 0.0, 0.0]);
        assert!(sphere_sweep(&record, &block, &xform).is_none());
    }

    #[test]
    fn starting_inside_reports_only_when_the_flag_is_set() {
        let record = sphere_record([0.0, 0.0, 0.0], 2.0);
        let xform = identity_xform([0.0, 0.0, 0.0]);
        let inside = sphere_block([0.5, 0.0, 0.0], [0.5, 0.0, 5.0], 1.0, 0);
        assert!(sphere_sweep(&record, &inside, &xform).is_none());
        let flagged = sphere_block([0.5, 0.0, 0.0], [0.5, 0.0, 5.0], 1.0, 1);
        let [point, normal, fraction] = sphere_sweep(&record, &flagged, &xform).unwrap();
        assert_eq!(point, [0.5, 0.0, 0.0, 0.0]);
        assert!((normal[2] - 1.0).abs() < 1e-6);
        assert_eq!(fraction, [0.0; 4]);
    }

    #[test]
    fn the_filter_mask_mode_is_the_intersection() {
        let a = [0u8, 0, 0x0f, 0, 0, 0, 0, 1];
        let b = [0x0fu8, 0, 0, 0, 0, 0, 0, 0];
        assert!(shape_filter(&a, &b));
        let b2 = [0xf0u8, 0, 0, 0, 0, 0, 0, 0];
        assert!(!shape_filter(&a, &b2));
    }

    #[test]
    fn the_filter_id_rule_matches_the_client() {
        // Both masks intersect; the ids then decide.
        let make = |x: i16, m: u16| {
            let mut a = [0u8; 8];
            a[0..2].copy_from_slice(&m.to_le_bytes());
            a[2..4].copy_from_slice(&m.to_le_bytes());
            a[4..6].copy_from_slice(&x.to_le_bytes());
            a
        };
        let b = |y: i16| {
            let mut w = [0u8; 8];
            w[0..2].copy_from_slice(&1u16.to_le_bytes());
            w[2..4].copy_from_slice(&1u16.to_le_bytes());
            w[4..6].copy_from_slice(&y.to_le_bytes());
            w
        };
        assert!(shape_filter(&make(3, 1), &b(3)));
        assert!(!shape_filter(&make(3, 1), &b(-3)));
        // A negative id never matches itself: the equality shortcut is skipped
        // for a non-zero `x`.
        assert!(!shape_filter(&make(-3, 1), &b(-3)));
        // Different magnitudes: `x` must not be positive and `y` must not be
        // positive either.
        assert!(shape_filter(&make(-3, 1), &b(-5)));
        assert!(!shape_filter(&make(-3, 1), &b(5)));
        assert!(!shape_filter(&make(3, 1), &b(5)));
        // The zero pair passes; a zero against a non-zero falls into the
        // magnitude rule with `|0|`.
        assert!(shape_filter(&make(0, 1), &b(0)));
        assert!(shape_filter(&make(0, 1), &b(-5)));
        assert!(!shape_filter(&make(0, 1), &b(5)));
    }

    fn capsule_record(p0: [f32; 3], p1: [f32; 3], radius: f32) -> Vec<u8> {
        let mut bytes = vec![0u8; CAPSULE_RECORD_LEN];
        for (lane, value) in p0.iter().enumerate() {
            bytes[lane * 4..lane * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        for (lane, value) in p1.iter().enumerate() {
            let at = 0x10 + lane * 4;
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes[0x20..0x24].copy_from_slice(&radius.to_le_bytes());
        bytes
    }

    #[test]
    fn a_degenerate_capsule_is_a_sphere_at_the_midpoint() {
        let capsule = capsule_record([0.0, 0.0, -1.0], [0.0, 0.0, -1.0], 1.0);
        let sphere = sphere_record([0.0, 0.0, -1.0], 1.0);
        let block = sphere_block([0.0, 0.0, 10.0], [0.0, 0.0, -10.0], 1.0, 0);
        let xform = identity_xform([0.0, 0.0, 0.0]);
        assert_eq!(
            capsule_sweep(&capsule, &block, &xform),
            sphere_sweep(&sphere, &block, &xform)
        );
    }

    #[test]
    fn a_sweep_down_the_axis_is_bound_by_the_near_cap() {
        // The start sits inside the infinite cylinder but beyond the cap, so
        // the client recurses into the sphere leaf at the clamped axis point
        // P1.
        let capsule = capsule_record([0.0, 0.0, -2.0], [0.0, 0.0, 2.0], 1.0);
        let block = sphere_block([0.0, 0.0, 5.0], [0.0, 0.0, -5.0], 1.0, 0);
        let xform = identity_xform([0.0, 0.0, 0.0]);
        let [point, normal, _] = capsule_sweep(&capsule, &block, &xform).unwrap();
        // The cap sphere sits at P1 = (0, 0, 2) with radius 1, so its top is
        // the contact at t = 2 along the sweep.
        assert!((point[2] - 3.0).abs() < 1e-6, "point {point:?}");
        assert!((normal[2] - 1.0).abs() < 1e-6, "normal {normal:?}");
    }

    #[test]
    fn a_sweep_starting_inside_the_capped_volume_reports_at_the_start() {
        let capsule = capsule_record([0.0, 0.0, 0.0], [0.0, 0.0, 4.0], 1.0);
        let xform = identity_xform([0.0, 0.0, 0.0]);
        let inside = sphere_block([0.2, 0.0, 2.0], [2.0, 0.0, 2.0], 1.0, 1);
        let [point, _, fraction] = capsule_sweep(&capsule, &inside, &xform).unwrap();
        assert_eq!(point, [0.2, 0.0, 2.0, 0.0]);
        assert_eq!(fraction, [0.0; 4]);
        let flagged = sphere_block([0.2, 0.0, 2.0], [2.0, 0.0, 2.0], 1.0, 0);
        assert!(capsule_sweep(&capsule, &flagged, &xform).is_none());
    }
}

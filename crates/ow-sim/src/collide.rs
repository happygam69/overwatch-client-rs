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
//!
//! # The support-point branch of the type-3 body (`0x142caef15..0x142caf1a5`)
//!
//! A query block with a positive support-point count is not a point sweep: the
//! body first folds the block's transformed support points into a hull and then
//! walks the mesh with it. The transform composes two rotations - the block's own
//! quaternion (`block+0x40`) with the shape transform's conjugate - so each local
//! point maps through one matrix built from that product; the translation is the
//! shape transform's rotation applied to the sweep's start alone. The fold is
//! `MINPS`/`MAXPS` over the transformed points, and it replaces the sweep:
//!
//! ```text
//! start = centre = (max + min) * 0.5
//! end   = end + (centre - start)          // the same delta, so `end - start`
//!                                         // is unchanged
//! step  = (max - min) * 0.5 + expansion + 0.02
//! ```
//!
//! The `+0.02` here is unconditional (unlike the point sweep's, which only applies
//! when all four expansion lanes are positive), and it is added after the
//! expansion, not to it. `step` is what every bounding box the walk tests is
//! inflated by, so both the centre and the half extent steer the walk. The body
//! also recentres the transformed points in place for the block copy the triangle
//! test receives; that array is only read by `0x142cc9460`.
//!
//! The triangle test itself (`0x142cce430`) takes two paths: the analytic one
//! when the support-point count is zero and every lane of the block's scaled
//! expansion compares `<= 0.005`, and `0x142cc9460` otherwise. Only the analytic
//! path is implemented; see [`mesh_triangle`].

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
    // The whole budget: one sphere narrowphase per candidate, no triangle loop.
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
// The type-3 mesh primitive (`0x142caed10` wrapper, `0x142caed30` body).
// ---------------------------------------------------------------------------

/// A type-3 shape element: `{scale at +0x00, 1/scale at +0x10, BVH root at
/// +0x20, vertices at +0x28, triangles at +0x30}`.
pub const MESH_RECORD_LEN: usize = 0x50;
/// A BVH node: `{min xyz at +0x00, max xyz at +0x0c, i32 second-child or
/// triangle base at +0x18, u16 layer mask at +0x1c, u8 axis at +0x1e, u8 leaf
/// triangle count at +0x1f}`.
pub const MESH_NODE_LEN: usize = 0x20;
/// A mesh triangle record: three vertex indices at `+0x00`, two u16 ids at
/// `+0x18`, the layer mask at `+0x1c`.
pub const MESH_TRIANGLE_LEN: usize = 0x20;
/// A mesh vertex: one `vec4` (stride `0x10`).
pub const MESH_VERTEX_LEN: usize = 0x10;

/// The bytes the element's three pointers address, plus the address of the
/// first byte.
///
/// The client reads the pool element's `+0x20`, `+0x28` and `+0x30` slots as
/// raw pointers into the level's mesh block. The fixture and the harness hand
/// the same numbers over, so `base` is the guest address the blob starts at
/// and every read goes through `address - base`.
pub struct MeshWorld<'a> {
    pub base: u64,
    pub bytes: &'a [u8],
}

impl MeshWorld<'_> {
    fn slice(&self, address: u64, len: usize) -> &[u8] {
        let offset = address.wrapping_sub(self.base) as usize;
        assert!(
            offset
                .checked_add(len)
                .is_some_and(|end| end <= self.bytes.len()),
            "mesh world does not cover {len:#x} bytes at {address:#x}"
        );
        &self.bytes[offset..offset + len]
    }

    fn quad(&self, address: u64) -> Quad {
        quad_at(self.slice(address, 16), 0)
    }

    fn u16(&self, address: u64, offset: usize) -> u16 {
        u16_at(self.slice(address + offset as u64, 2), 0)
    }

    fn u8(&self, address: u64, offset: usize) -> u8 {
        self.slice(address + offset as u64, 1)[0]
    }

    fn i32(&self, address: u64, offset: usize) -> i32 {
        i32::from_le_bytes(
            self.slice(address + offset as u64, 4)
                .try_into()
                .expect("four bytes"),
        )
    }
}

/// The output a mesh hit leaves in the query buffer.
pub struct MeshHit {
    /// The world contact point (`out+0x00`).
    pub point: Quad,
    /// The world unit normal (`out+0x10`).
    pub normal: Quad,
    /// The hit fraction along the sweep (`out+0x20`).
    pub fraction: Quad,
    /// The triangle index that produced the hit (`out+0x30`, raw bits).
    pub triangle: i32,
    /// The triangle record's two `u16` ids (`out+0x34`, `out+0x36`).
    pub ids: [u16; 2],
}

/// One `mesh_sweep` call.
pub struct MeshSweep {
    /// The last accepted triangle, in world space. `None` leaves the client's
    /// output buffer untouched.
    pub hit: Option<MeshHit>,
    /// `out+0x3c`: every internal node the walk visited.
    pub internals: i32,
    /// `out+0x40`: every leaf the walk expanded.
    pub leaves: i32,
}

#[inline]
fn quad_min(a: Quad, b: Quad) -> Quad {
    [
        minss(a[0], b[0]),
        minss(a[1], b[1]),
        minss(a[2], b[2]),
        minss(a[3], b[3]),
    ]
}

#[inline]
fn quad_max(a: Quad, b: Quad) -> Quad {
    [
        maxss(a[0], b[0]),
        maxss(a[1], b[1]),
        maxss(a[2], b[2]),
        maxss(a[3], b[3]),
    ]
}

/// `ANDPS` with the `0x7fffffff` splat.
#[inline]
fn quad_abs(v: Quad) -> Quad {
    [
        f32::from_bits(v[0].to_bits() & 0x7fff_ffff),
        f32::from_bits(v[1].to_bits() & 0x7fff_ffff),
        f32::from_bits(v[2].to_bits() & 0x7fff_ffff),
        f32::from_bits(v[3].to_bits() & 0x7fff_ffff),
    ]
}

/// One lane's sign bit, as `MOVMSKPS` sees it (`-0.0` counts).
#[inline]
fn sign_bit(v: f32) -> bool {
    v.to_bits() & 0x8000_0000 != 0
}

/// `0x142cce430`: one triangle against the query block.
///
/// The leaf's first test is `CMPLEPS` of the block's scaled expansion against
/// the `0.005` splat at `0x1435ecda0`, accepted only when all four lanes
/// compare `<=` (an ordered compare: a `NaN` lane is not `<=`). Its second test
/// is `dword [block+0x38] == 0`. Both must hold for the analytic path below;
/// every other state builds a stack descriptor and calls `0x142cc9460`.
///
/// `count` is the query block's support-point count, forwarded untouched from
/// the block the walk copies. `0x142cc9460` is a GJK distance solver with a
/// cached simplex (itself calling `0x142cc8d80` for a triangle simplex and
/// `0x142cc7740` for a tetrahedron) and is **not implemented**; this function
/// therefore returns a miss on the path that reaches it, which is the one place
/// where the type-3 narrowphase still differs from the client.
fn mesh_triangle(
    vertices: [Quad; 3],
    start: Quad,
    end: Quad,
    motion: Quad,
    expansion: Quad,
    count: i32,
) -> Option<(Quad, Quad, Quad)> {
    // The client's fast path runs when every lane of the scaled expansion is
    // at most 0.005; anything else is the support-point (slow) path.
    if count != 0 || !all_lanes(|lane| expansion[lane] <= 0.005) {
        return None;
    }

    let [v0, v1, v2] = vertices;
    let p = sub_lane(v0, start);
    let q = sub_lane(v1, start);
    let r = sub_lane(v2, start);
    let d = sub_lane(end, start);

    // The client's vectorised cross products: `rot3(a) * rot3_inv(b) -
    // rot3_inv(a) * rot3(b)`, multiplied by `d` lane-wise before the fold.
    let a = dot3(cross(r, q), d);
    let b = dot3(cross(p, r), d);
    let c = dot3(cross(q, p), d);
    let plane = quad_min(quad_min(a, b), c);
    if all_lanes(|lane| plane[lane] < 0.0) {
        return None;
    }

    let e0 = sub_lane(v1, v0);
    let e1 = sub_lane(v2, v0);
    let normal = rot3(sub_lane(mul_lane(rot3(e1), e0), mul_lane(rot3(e0), e1)));

    // `det = -(normal . d)`; only a strictly smaller value misses.
    let det = negate(dot3(normal, d));
    if all_lanes(|lane| det[lane] < f32::from_bits(0x3400_0000)) {
        return None;
    }

    let numerator = negate(dot3(normal, p));
    let t = [
        numerator[0] / det[0],
        numerator[1] / det[1],
        numerator[2] / det[2],
        numerator[3] / det[3],
    ];
    if !all_lanes(|lane| 0.0 <= t[lane]) || !all_lanes(|lane| t[lane] <= motion[lane]) {
        return None;
    }

    let point = add_lane(mul_lane(t, d), start);
    let length = dot3(normal, normal)[0].sqrt();
    let unit = mul_lane(splat(1.0f32 / length), normal);
    Some((point, unit, t))
}

/// `cross(a, b)` in the client's operand order.
#[inline]
fn cross(a: Quad, b: Quad) -> Quad {
    let forward = mul_lane(rot3(a), rot3_inverted(b));
    let backward = mul_lane(rot3_inverted(a), rot3(b));
    sub_lane(forward, backward)
}

/// `SHUFPS v, v, 0xd2`: `(z, x, y, w)`.
#[inline]
fn rot3_inverted(v: Quad) -> Quad {
    [v[2], v[0], v[1], v[3]]
}

/// `SHUFPS dst, src, imm8`: lanes 0 and 1 are picked from `dst`, lanes 2 and 3
/// from `src`, each by the two-bit field `imm8 >> (2 * lane)`.
#[inline]
fn shufps(dst: Quad, src: Quad, imm: u8) -> Quad {
    let pick = |field: u8, from: &Quad| from[(field & 3) as usize];
    [
        pick(imm, &dst),
        pick(imm >> 2, &dst),
        pick(imm >> 4, &src),
        pick(imm >> 6, &src),
    ]
}

/// `XORPS` against a four-lane bit mask: a sign flip that carries a `NaN`
/// payload through unchanged and swaps `+0.0` with `-0.0`, which arithmetic
/// negation does not.
#[inline]
fn xor_mask(v: Quad, mask: [u32; 4]) -> Quad {
    [
        f32::from_bits(v[0].to_bits() ^ mask[0]),
        f32::from_bits(v[1].to_bits() ^ mask[1]),
        f32::from_bits(v[2].to_bits() ^ mask[2]),
        f32::from_bits(v[3].to_bits() ^ mask[3]),
    ]
}

/// `ANDPS` against a four-lane bit mask: an identity on the lanes whose mask is
/// all ones, and a zero on the lanes whose mask is zero.
#[inline]
fn and_mask(v: Quad, mask: [u32; 4]) -> Quad {
    [
        f32::from_bits(v[0].to_bits() & mask[0]),
        f32::from_bits(v[1].to_bits() & mask[1]),
        f32::from_bits(v[2].to_bits() & mask[2]),
        f32::from_bits(v[3].to_bits() & mask[3]),
    ]
}

/// `(-0, -0, -0, +0)` at `0x143110d50`: the conjugate mask. The conjugation is a
/// sign flip, not an arithmetic negation.
const CONJUGATE: [u32; 4] = [0x8000_0000, 0x8000_0000, 0x8000_0000, 0];
/// `(0, 0, -0, -0)` at `0x143110d70`, `(-0, 0, 0, -0)` at `0x143110d60` and
/// `(0, -0, 0, -0)` at `0x142f860e0`: the sign pattern of the composed
/// quaternion's three shuffle terms.
const PRODUCT_MASK_Z: [u32; 4] = [0, 0, 0x8000_0000, 0x8000_0000];
const PRODUCT_MASK_X: [u32; 4] = [0x8000_0000, 0, 0, 0x8000_0000];
const PRODUCT_MASK_Y: [u32; 4] = [0, 0x8000_0000, 0, 0x8000_0000];
/// `(0xffffffff, 0xffffffff, 0xffffffff, 0)` at `0x1430a4b40`: every bit kept on
/// lanes 0..2, lane 3 zeroed.
const KEEP_LOW_THREE: [u32; 4] = [0xffff_ffff, 0xffff_ffff, 0xffff_ffff, 0];

/// `0x142caef15..0x142caf055`: the support-point hull's affine map.
///
/// `q` is the shape transform's quaternion, `b` the query block's quaternion
/// (`block+0x40`) and `v` the sweep's start relative to the shape's position,
/// all before the mesh-space transform. The three returned rows are the
/// rotation of the composed quaternion `conj(q) * b`; the fourth return is that
/// same rotation applied to `v` alone. A point `p` of the block's local support
/// array maps to `((p.x * row0 + t) + p.y * row1) + p.z * row2` - the sums are in
/// that order in the leaf, which is why a re-associated form would differ in the
/// last bit.
fn support_hull_affine(q: Quad, b: Quad, v: Quad) -> ([Quad; 3], Quad) {
    // `xorps` with the conjugate mask: `c = conj(q)`.
    let c = xor_mask(q, CONJUGATE);
    // The composed quaternion, as four shuffle-and-sign products summed left to
    // right. `S` is `conj(q) * b`; the leaf never normalises it.
    let mut composed = mul_lane(xor_mask(shufps(c, c, 0x1b), PRODUCT_MASK_Z), splat(b[0]));
    composed = add_lane(composed, mul_lane(splat(b[3]), c));
    composed = add_lane(
        composed,
        mul_lane(xor_mask(shufps(c, c, 0x4e), PRODUCT_MASK_X), splat(b[1])),
    );
    composed = add_lane(
        composed,
        mul_lane(xor_mask(shufps(c, c, 0xb1), PRODUCT_MASK_Y), splat(b[2])),
    );
    let rot_c = shufps(c, c, 0xc9);

    // The translation is the same rotation applied to `v` alone: `2 * rot3(
    // rot3(s) * c - s * rot3(c)) + v` with `s = c.w * v + rot3(rot3(v) * c -
    // rot3(c) * v)`.
    let cross = rot3(sub_lane(mul_lane(rot3(v), c), mul_lane(rot_c, v)));
    let spin = add_lane(mul_lane(splat(c[3]), v), cross);
    let cross2 = rot3(sub_lane(mul_lane(rot3(spin), c), mul_lane(spin, rot_c)));
    let translation = add_lane(add_lane(cross2, cross2), v);

    // The rotation matrix of `composed`: the diagonal from `1 - (2s * s) -
    // (2s * s)`, the off-diagonal terms from `rot3(2s) * s.w` and the two
    // remaining `2s * s` shuffles. `2s` is the leaf's `ADDPS` of `s` with
    // itself; the products are the same terms `rotation_rows` builds, in the
    // leaf's own association, and the last lane keeps the diagonal constant's
    // zero.
    let doubled = add_lane(composed, composed);
    let square = mul_lane(doubled, composed);
    let diagonal = sub_lane(
        sub_lane(
            DIAG_BASE,
            and_mask(shufps(square, square, 0xc1), KEEP_LOW_THREE),
        ),
        and_mask(shufps(square, square, 0xda), KEEP_LOW_THREE),
    );
    let half = mul_lane(
        shufps(doubled, doubled, 0xe6),
        shufps(composed, composed, 0xd0),
    );
    let spin_term = mul_lane(rot3(doubled), splat(composed[3]));
    let plus = add_lane(spin_term, half);
    let minus = sub_lane(half, spin_term);

    // The rows are assembled from lane permutations of the diagonal and of the
    // two off-diagonal terms; the lane choices are the leaf's SHUFPS immediates.
    let second = shufps(shufps(plus, minus, 0x49), shufps(plus, minus, 0x49), 0x78);
    let third = shufps(shufps(plus, minus, 0xa0), shufps(plus, minus, 0xa0), 0x88);
    let row0 = shufps(
        shufps(diagonal, second, 0x4c),
        shufps(diagonal, second, 0x4c),
        0x78,
    );
    let row1 = {
        let partial = shufps(diagonal, second, 0xed);
        shufps(partial, partial, 0x72)
    };
    let row2 = shufps(third, diagonal, 0xe4);
    ([row0, row1, row2], translation)
}

/// `0x142caef15..0x142caf1a5`: fold the block's transformed support points into
/// the hull the walk continues with.
///
/// Each point is `((p.x * row0 + t) + p.y * row1) + p.z * row2`, scaled lane-wise
/// by the shape's inverse scale; the fold is `MINPS`/`MAXPS` over all four lanes
/// starting from the first point, whose rule is that the second operand wins on
/// a tie or a `NaN`. The results are the hull's start (the cloud's centre), its
/// end (the sweep's end moved by the same centre delta) and the inflation
/// `(max - min) * 0.5 + expansion + 0.02`, which is added to every bounding box
/// the walk tests. The in-place recentring of the transformed points exists only
/// for the block copy the triangle test receives and is not needed here.
fn support_hull(
    points: &[u8],
    count: usize,
    q: Quad,
    b: Quad,
    v: Quad,
    inverse: Quad,
    start: Quad,
    end: Quad,
    expansion: Quad,
) -> (Quad, Quad, Quad) {
    let (rows, translation) = support_hull_affine(q, b, v);
    let mut low = [0.0f32; 4];
    let mut high = [0.0f32; 4];
    for index in 0..count {
        let base = index * MESH_VERTEX_LEN;
        let mut point = add_lane(mul_lane(splat(f32_at(points, base)), rows[0]), translation);
        point = add_lane(point, mul_lane(splat(f32_at(points, base + 4)), rows[1]));
        point = add_lane(point, mul_lane(splat(f32_at(points, base + 8)), rows[2]));
        let point = mul_lane(point, inverse);
        if index == 0 {
            low = point;
            high = point;
        } else {
            for lane in 0..4 {
                low[lane] = minss(low[lane], point[lane]);
                high[lane] = maxss(high[lane], point[lane]);
            }
        }
    }
    let centre = mul_lane(add_lane(high, low), HALF);
    let step = add_lane(
        add_lane(mul_lane(sub_lane(high, low), HALF), expansion),
        splat(0.02),
    );
    let end = add_lane(end, sub_lane(centre, start));
    (centre, end, step)
}

/// `0x142caed30` type 3 (mesh): a swept query against a triangle BVH.
///
/// The count `> 0` branch (the support-point hull at
/// `0x142caef15..0x142caf1a5`) is implemented; what it hands each triangle to
/// (`0x142cc9460`) is not, so a support-point query reaches the client's walk
/// but not its triangle accept. The triangle callback argument is null on this
/// path (the wrapper `0x142caed10` zeroes it), so the walk never stops early.
pub fn mesh_sweep(record: &[u8], world: &MeshWorld, block: &[u8], xform: &[u8]) -> MeshSweep {
    let missed = MeshSweep {
        hit: None,
        internals: 0,
        leaves: 0,
    };
    let scale = quad_at(record, 0x00);
    let inverse = quad_at(record, 0x10);
    let root = read_pointer(record, 0x20);
    if root == 0 {
        return missed;
    }
    let vertices = read_pointer(record, 0x28);
    let triangles = read_pointer(record, 0x30);

    let position = quad_at(xform, 0x00);
    let q = quad_at(xform, 0x10);
    let start_world = quad_at(block, 0x00);
    let end_world = quad_at(block, 0x10);
    // The motion budget shrinks to the last accepted fraction (0x142caf5c1).
    let mut motion = quad_at(block, 0x20);
    let count = i32_at(block, 0x38);
    let mask = u16_at(block, 0x70);
    let expansion = mul_lane(quad_at(block, 0x50), inverse);

    let mut start = mul_lane(rotate_to_local(q, sub_lane(start_world, position)), inverse);
    let mut end = mul_lane(rotate_to_local(q, sub_lane(end_world, position)), inverse);
    let mut step = if all_lanes(|lane| 0.0 < expansion[lane]) {
        add_lane(splat(0.02), expansion)
    } else {
        expansion
    };

    if count > 0 {
        // The support-point hull replaces the sweep's start and end with the
        // point cloud's centre (and the end shifted by the same delta), and
        // the expansion with centre-relative half extent + expansion + 0.02.
        let points = world.slice(read_pointer(block, 0x30), count as usize * MESH_VERTEX_LEN);
        let (hull_start, hull_end, hull_step) = support_hull(
            points,
            count as usize,
            q,
            quad_at(block, 0x40),
            sub_lane(start_world, position),
            inverse,
            start,
            end,
            expansion,
        );
        start = hull_start;
        end = hull_end;
        step = hull_step;
    }

    let direction = sub_lane(end, start);
    let far = add_lane(mul_lane(motion, direction), start);
    // `lo`/`hi` are re-derived from the hit point after every acceptance
    // (0x142caf5d7), which tightens the node tests for the rest of the walk.
    let mut low = quad_min(start, far);
    let mut high = quad_max(start, far);
    let absolute = quad_abs(direction);

    let mut stack = vec![root];
    let mut sweep = missed;
    let mut hit: Option<MeshHit> = None;
    while let Some(node) = stack.pop() {
        if world.u16(node, 0x1c) & mask == 0 {
            continue;
        }
        let node_min = world.quad(node);
        let node_max = world.quad(node + 0x0c);
        let high_corner = add_lane(node_max, step);
        let low_corner = sub_lane(node_min, step);
        if (0..3).any(|lane| {
            sign_bit(high_corner[lane] - low[lane]) || sign_bit(high[lane] - low_corner[lane])
        }) {
            continue;
        }
        let sum = add_lane(high_corner, low_corner);
        let difference = sub_lane(high_corner, low_corner);
        let centre = sub_lane(start, mul_lane(sum, splat(0.5)));
        let half = mul_lane(difference, splat(0.5));
        let first = sub_lane(
            mul_lane(rot3(direction), centre),
            mul_lane(rot3(centre), direction),
        );
        let second = add_lane(
            mul_lane(rot3(absolute), half),
            mul_lane(rot3(half), absolute),
        );
        // `|rot3(t1)| - rot3(t2)`: the rotation is applied before the abs.
        let predicate = sub_lane(quad_abs(rot3(first)), rot3(second));
        if (0..3).any(|lane| 0.0 < predicate[lane]) {
            continue;
        }

        let children = world.i32(node, 0x18);
        let leaf = world.u8(node, 0x1f);
        if leaf != 0 {
            sweep.leaves += 1;
            let mut triangle = offset(triangles, i64::from(children) * 0x20);
            for index in 0..leaf {
                if world.u16(triangle, 0x1c) & mask != 0 {
                    let corner = |slot: usize| {
                        let vertex = world.i32(triangle, slot * 4);
                        world.quad(offset(vertices, i64::from(vertex) * 0x10))
                    };
                    if let Some((point, normal, fraction)) = mesh_triangle(
                        [corner(0), corner(1), corner(2)],
                        start,
                        end,
                        motion,
                        expansion,
                        count,
                    ) {
                        motion = fraction;
                        low = quad_min(start, point);
                        high = quad_max(start, point);
                        let local = mul_lane(point, scale);
                        hit = Some(MeshHit {
                            point: add_lane(rotate_to_world_point(q, local), position),
                            normal: rotate_to_world_normal(q, normal),
                            fraction,
                            triangle: children + i32::from(index),
                            ids: [world.u16(triangle, 0x18), world.u16(triangle, 0x1a)],
                        });
                    }
                }
                triangle = offset(triangle, MESH_TRIANGLE_LEN as i64);
            }
        } else {
            sweep.internals += 1;
            let second = offset(node, i64::from(children) * 0x20);
            let first = offset(node, MESH_NODE_LEN as i64);
            // `0 < d[axis]` on all four lanes picks the near child; the body
            // splats the axis lane and tests every lane, and every other axis
            // value reads a stack slot the body never writes.
            let visible = match world.u8(node, 0x1e) {
                0 => direction[0],
                1 => direction[1],
                _ => f32::NAN,
            };
            // The pair is pushed so that the pop order is: the child-field
            // node when `0 < d[axis]` is false, the adjacent one when true.
            let near_is_second = all_lanes(|_lane| 0.0 < visible);
            if near_is_second {
                stack.push(second);
                stack.push(first);
            } else {
                stack.push(first);
                stack.push(second);
            }
        }
    }
    sweep.hit = hit;
    sweep
}

fn read_pointer(record: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(record[offset..offset + 8].try_into().expect("eight bytes"))
}

fn i32_at(bytes: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("four bytes"))
}

fn offset(address: u64, delta: i64) -> u64 {
    address.wrapping_add(delta as u64)
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

/// `0x142cd56e0`: whether a collider triangle is unusable (`AL` set), in the
/// client's own return form.
///
/// The client returns `AL`: 1 when the triangle must be skipped, 0 when it is
/// usable. The `MOVMSKPS` result never reaches the caller - every rejecting
/// exit runs `mov al, 1` afterwards - so the compared channel is one bit.
///
/// The four tests run in this order: a corner coordinate that is `inf` or `NaN`
/// (`ANDPS` with `+inf`, `CMPEQPS` against `+inf`, low three lanes), a bounding
/// box wider than `1e6` on any axis, an edge shorter than `0.0025` (squared
/// length below `6.25e-6`), and twice the area below `2.5e-5`.
pub fn triangle_degenerate(a: Quad, b: Quad, c: Quad) -> u32 {
    for corner in [a, b, c] {
        for value in corner.iter().take(3) {
            if value.to_bits() & 0x7f80_0000 == 0x7f80_0000 {
                return 1;
            }
        }
    }

    let extent = sub_lane(quad_max(quad_max(a, b), c), quad_min(quad_min(a, b), c));
    for value in extent.iter().take(3) {
        if *value > 1_000_000.0 {
            return 1;
        }
    }

    for edge in [sub_lane(b, a), sub_lane(c, b), sub_lane(a, c)] {
        if length_sq(edge)[0] < 6.25e-6 {
            return 1;
        }
    }

    // `rot3(rot3(e1)*e2 - rot3(e2)*e1)` is `-cross(e1, e2)` for `e1 = c - a`
    // and `e2 = b - a`, so the normal below is the client's vector with the
    // opposite sign; only its length is tested, so the sign is unobservable.
    let normal = cross(sub_lane(b, a), sub_lane(c, a));
    if length_sq(normal)[0].sqrt() < 2.5e-5 {
        return 1;
    }
    0
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

    /*
     * The type-3 mesh primitive.
     *
     * The fixture is a two-quad, three-node mesh: an internal root whose two
     * children are leaves, the first holding a quad at the origin (triangles
     * 0 and 1) and the second a quad at x = 5 (triangles 2 and 3). Both quads
     * wind counter-clockwise in the xy plane, so their normals point up.
     */

    const MESH_BASE: u64 = 0x0000_0001_0000_0000;
    const NODES_AT: usize = 0x100;
    const VERTICES_AT: usize = 0x200;
    const TRIANGLES_AT: usize = 0x300;

    struct MeshFixture {
        record: Vec<u8>,
        world: Vec<u8>,
    }

    impl MeshFixture {
        fn sweep(&self, block: &[u8], xform: &[u8]) -> MeshSweep {
            let world = MeshWorld {
                base: MESH_BASE,
                bytes: &self.world,
            };
            mesh_sweep(&self.record, &world, block, xform)
        }
    }

    fn put_quad(bytes: &mut [u8], offset: usize, quad: Quad) {
        for (lane, value) in quad.iter().enumerate() {
            bytes[offset + lane * 4..offset + lane * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
    }

    fn put_node(
        bytes: &mut [u8],
        at: usize,
        low: [f32; 3],
        high: [f32; 3],
        child: i32,
        mask: u16,
        axis: u8,
        count: u8,
    ) {
        for (lane, value) in low.iter().enumerate() {
            bytes[at + lane * 4..at + lane * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        for (lane, value) in high.iter().enumerate() {
            let offset = at + 0x0c + lane * 4;
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes[at + 0x18..at + 0x1c].copy_from_slice(&child.to_le_bytes());
        bytes[at + 0x1c..at + 0x1e].copy_from_slice(&mask.to_le_bytes());
        bytes[at + 0x1e] = axis;
        bytes[at + 0x1f] = count;
    }

    fn put_triangle(bytes: &mut [u8], at: usize, corners: [i32; 3], ids: [u16; 2], mask: u16) {
        for (slot, corner) in corners.iter().enumerate() {
            bytes[at + slot * 4..at + slot * 4 + 4].copy_from_slice(&corner.to_le_bytes());
        }
        bytes[at + 0x18..at + 0x1a].copy_from_slice(&ids[0].to_le_bytes());
        bytes[at + 0x1a..at + 0x1c].copy_from_slice(&ids[1].to_le_bytes());
        bytes[at + 0x1c..at + 0x1e].copy_from_slice(&mask.to_le_bytes());
    }

    fn fixture(scale: f32, inverse: f32) -> MeshFixture {
        let mut world = vec![0u8; 0x400];
        let mut record = vec![0u8; MESH_RECORD_LEN];
        put_quad(&mut record, 0x00, [scale; 4]);
        put_quad(&mut record, 0x10, [inverse; 4]);
        record[0x20..0x28].copy_from_slice(&(MESH_BASE + NODES_AT as u64).to_le_bytes());
        record[0x28..0x30].copy_from_slice(&(MESH_BASE + VERTICES_AT as u64).to_le_bytes());
        record[0x30..0x38].copy_from_slice(&(MESH_BASE + TRIANGLES_AT as u64).to_le_bytes());

        let corners = [
            [0.0f32, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [5.0, 0.0, 0.0],
            [6.0, 0.0, 0.0],
            [6.0, 1.0, 0.0],
            [5.0, 1.0, 0.0],
        ];
        for (slot, corner) in corners.iter().enumerate() {
            put_quad(
                &mut world,
                VERTICES_AT + slot * MESH_VERTEX_LEN,
                [corner[0], corner[1], corner[2], 0.0],
            );
        }

        // The root straddles both quads; its second child sits two nodes on.
        put_node(
            &mut world,
            NODES_AT,
            [-1.0, -1.0, -1.0],
            [7.0, 2.0, 1.0],
            2,
            0x0001,
            0,
            0,
        );
        put_node(
            &mut world,
            NODES_AT + MESH_NODE_LEN,
            [0.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            0,
            0x0001,
            0,
            2,
        );
        put_node(
            &mut world,
            NODES_AT + 2 * MESH_NODE_LEN,
            [5.0, 0.0, 0.0],
            [6.0, 1.0, 0.0],
            2,
            0x0001,
            0,
            2,
        );

        put_triangle(&mut world, TRIANGLES_AT, [0, 1, 2], [0x11, 0x21], 0x0001);
        put_triangle(
            &mut world,
            TRIANGLES_AT + MESH_TRIANGLE_LEN,
            [0, 2, 3],
            [0x12, 0x22],
            0x0001,
        );
        put_triangle(
            &mut world,
            TRIANGLES_AT + 2 * MESH_TRIANGLE_LEN,
            [4, 5, 6],
            [0x13, 0x23],
            0x0001,
        );
        put_triangle(
            &mut world,
            TRIANGLES_AT + 3 * MESH_TRIANGLE_LEN,
            [4, 6, 7],
            [0x14, 0x24],
            0x0001,
        );
        MeshFixture { record, world }
    }

    fn mesh_block(
        start: Quad,
        end: Quad,
        motion: f32,
        expansion: f32,
        count: i32,
        mask: u16,
    ) -> Vec<u8> {
        let mut bytes = vec![0u8; 0x78];
        put_quad(&mut bytes, 0x00, start);
        put_quad(&mut bytes, 0x10, end);
        put_quad(&mut bytes, 0x20, [motion; 4]);
        bytes[0x38..0x3c].copy_from_slice(&count.to_le_bytes());
        put_quad(&mut bytes, 0x50, [expansion; 4]);
        bytes[0x70..0x72].copy_from_slice(&mask.to_le_bytes());
        bytes
    }

    /// The free run in the test world, after the four triangles.
    const SUPPORT_POINTS_AT: usize = 0x380;

    /// A query block with `count` support points: the pointer names the array the
    /// test world carries at [`SUPPORT_POINTS_AT`], and the block's own
    /// quaternion is passed through.
    fn mesh_block_with_points(
        start: Quad,
        end: Quad,
        motion: f32,
        expansion: f32,
        quat: Quad,
        count: i32,
        mask: u16,
    ) -> Vec<u8> {
        let mut bytes = mesh_block(start, end, motion, expansion, count, mask);
        bytes[0x30..0x38].copy_from_slice(&(MESH_BASE + SUPPORT_POINTS_AT as u64).to_le_bytes());
        put_quad(&mut bytes, 0x40, quat);
        bytes
    }

    fn mesh_world_with_points(points: &[[f32; 3]]) -> Vec<u8> {
        let mut world = fixture(1.0, 1.0).world;
        for (index, point) in points.iter().enumerate() {
            put_quad(
                &mut world,
                SUPPORT_POINTS_AT + index * MESH_VERTEX_LEN,
                [point[0], point[1], point[2], 0.0],
            );
        }
        world
    }

    #[test]
    fn a_mesh_sweep_down_onto_a_quad_reports_the_triangle_that_contains_it() {
        let mesh = fixture(1.0, 1.0);
        let block = mesh_block(
            [0.75, 0.9, 5.0, 0.0],
            [0.75, 0.9, -5.0, 0.0],
            1.0,
            0.004,
            0,
            0x0001,
        );
        let hit = mesh
            .sweep(&block, &identity_xform([0.0, 0.0, 0.0]))
            .hit
            .expect("the sweep crosses the quad");
        // `(0.75, 0.9)` lies above the quad's diagonal, so only triangle 1
        // covers it and triangle 0 is the miss that precedes the hit.
        assert_eq!(hit.triangle, 1);
        assert_eq!(hit.ids, [0x12, 0x22]);
        assert_eq!(hit.fraction, [0.5; 4]);
        assert_eq!(hit.point, [0.75, 0.9, 0.0, 0.0]);
        assert_eq!(hit.normal, [0.0, 0.0, 1.0, 0.0]);
    }

    #[test]
    fn a_mesh_walk_reports_the_nodes_it_expanded() {
        let mesh = fixture(1.0, 1.0);
        let block = mesh_block(
            [0.75, 0.9, 5.0, 0.0],
            [0.75, 0.9, -5.0, 0.0],
            1.0,
            0.004,
            0,
            0x0001,
        );
        let sweep = mesh.sweep(&block, &identity_xform([0.0, 0.0, 0.0]));
        // The root is internal; the second quad's box is outside the query's
        // reach, so only the first leaf is expanded.
        assert_eq!(sweep.internals, 1);
        assert_eq!(sweep.leaves, 1);
    }

    #[test]
    fn a_mesh_walk_skips_a_node_whose_layer_mask_is_clear() {
        let mesh = fixture(1.0, 1.0);
        let block = mesh_block(
            [0.75, 0.9, 5.0, 0.0],
            [0.75, 0.9, -5.0, 0.0],
            1.0,
            0.004,
            0,
            0x0002,
        );
        let sweep = mesh.sweep(&block, &identity_xform([0.0, 0.0, 0.0]));
        assert!(sweep.hit.is_none());
        assert_eq!(sweep.internals, 0);
        assert_eq!(sweep.leaves, 0);
    }

    #[test]
    fn a_mesh_walk_skips_a_triangle_whose_layer_mask_is_clear() {
        let mut mesh = fixture(1.0, 1.0);
        mesh.world
            [TRIANGLES_AT + MESH_TRIANGLE_LEN + 0x1c..TRIANGLES_AT + MESH_TRIANGLE_LEN + 0x1e]
            .copy_from_slice(&0x0002u16.to_le_bytes());
        let block = mesh_block(
            [0.75, 0.9, 5.0, 0.0],
            [0.75, 0.9, -5.0, 0.0],
            1.0,
            0.004,
            0,
            0x0001,
        );
        let sweep = mesh.sweep(&block, &identity_xform([0.0, 0.0, 0.0]));
        assert!(sweep.hit.is_none());
        assert_eq!(sweep.leaves, 1);
    }

    #[test]
    fn a_mesh_sweep_beside_the_quad_misses() {
        let mesh = fixture(1.0, 1.0);
        // The sweep falls between the two quads: inside the root's box, so the
        // root is expanded, but outside both leaves.
        let block = mesh_block(
            [3.0, 0.5, 5.0, 0.0],
            [3.0, 0.5, -5.0, 0.0],
            1.0,
            0.004,
            0,
            0x0001,
        );
        let sweep = mesh.sweep(&block, &identity_xform([0.0, 0.0, 0.0]));
        assert!(sweep.hit.is_none());
        assert_eq!(sweep.internals, 1);
        assert_eq!(sweep.leaves, 0);
    }

    #[test]
    fn a_mesh_shape_without_a_root_leaves_the_output_untouched() {
        let mut mesh = fixture(1.0, 1.0);
        mesh.record[0x20..0x28].copy_from_slice(&0u64.to_le_bytes());
        let block = mesh_block(
            [0.75, 0.9, 5.0, 0.0],
            [0.75, 0.9, -5.0, 0.0],
            1.0,
            0.004,
            0,
            0x0001,
        );
        let sweep = mesh.sweep(&block, &identity_xform([0.0, 0.0, 0.0]));
        assert!(sweep.hit.is_none());
        assert_eq!(sweep.internals, 0);
        assert_eq!(sweep.leaves, 0);
    }

    /// One `mesh_sweep` call over a world the caller supplies (so a test can put
    /// support points in it).
    fn sweep_world(world: &[u8], block: &[u8], xform: &[u8]) -> MeshSweep {
        let mesh = fixture(1.0, 1.0);
        let world = MeshWorld {
            base: MESH_BASE,
            bytes: world,
        };
        mesh_sweep(&mesh.record, &world, block, xform)
    }

    fn bits_of(quad: Quad) -> [u32; 4] {
        [
            quad[0].to_bits(),
            quad[1].to_bits(),
            quad[2].to_bits(),
            quad[3].to_bits(),
        ]
    }

    fn quad_of_bits(bits: [u32; 4]) -> Quad {
        [
            f32::from_bits(bits[0]),
            f32::from_bits(bits[1]),
            f32::from_bits(bits[2]),
            f32::from_bits(bits[3]),
        ]
    }

    /*
     * The support-point hull (`0x142caef15..0x142caf1a5`).
     *
     * The expected values are a golden vector: they come from a second,
     * independent float32 transcription of the branch's operation sequence made
     * during the read, not from this code. The walk-level proof that the client
     * builds the same hull is `dev/mesh_support_diff.py`, which compares these
     * counters against the real `0x142caed30`.
     */

    /// The affine map's rows and translation for a rotated block quaternion and a
    /// rotated shape transform, pinned bit for bit.
    #[test]
    fn the_support_hull_affine_is_the_read_map() {
        let q = quad_of_bits([0x3e4c_cccd, 0xbe99_999a, 0x3dcc_cccd, 0x3f6d_6796]);
        let b = quad_of_bits([0x3f00_0000, 0x3e80_0000, 0xbe00_0000, 0x3f51_d689]);
        let v = quad_of_bits([0x3fc0_0000, 0xc010_0000, 0x3f40_0000, 0x0000_0000]);
        let (rows, translation) = support_hull_affine(q, b, v);
        assert_eq!(
            bits_of(translation),
            [0x3fc0_0000, 0xc010_0000, 0x3f40_0000, 0x0000_0000]
        );
        assert_eq!(
            [bits_of(rows[0]), bits_of(rows[1]), bits_of(rows[2])],
            [
                [0x3eb7_ca19, 0xbec4_56f7, 0xbf59_d6e9, 0x0000_0000],
                [0x3f58_a1f4, 0x3f04_b2a0, 0x3dfc_ae44, 0x0000_0000],
                [0x3ec9_9c36, 0xbf43_ada4, 0x3f02_b504, 0x0000_0000],
            ]
        );
    }

    /// An identity block quaternion and an identity shape quaternion leave the
    /// rotation at the identity and the translation at the start: the hull is the
    /// plain point box then.
    #[test]
    fn an_identity_block_quaternion_is_the_identity_rotation() {
        let identity = [0.0, 0.0, 0.0, 1.0];
        let v = [1.0, 2.0, 3.0, 0.0];
        let (rows, translation) = support_hull_affine(identity, identity, v);
        assert_eq!(translation, v);
        assert_eq!(rows[0], [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(rows[1], [0.0, 1.0, 0.0, 0.0]);
        assert_eq!(rows[2], [0.0, 0.0, 1.0, 0.0]);
    }

    /// The fold's four outputs, pinned bit for bit on a two-point capsule-shaped
    /// cloud: the centre, the shifted end and the inflated step.
    #[test]
    fn the_support_hull_folds_two_points_into_the_walk_hull() {
        let q = quad_of_bits([0x3e4c_cccd, 0xbe99_999a, 0x3dcc_cccd, 0x3f6d_6796]);
        let b = quad_of_bits([0x3f00_0000, 0x3e80_0000, 0xbe00_0000, 0x3f51_d689]);
        let v = quad_of_bits([0x3fc0_0000, 0xc010_0000, 0x3f40_0000, 0x0000_0000]);
        let start = quad_of_bits([0x4060_0000, 0xbfa0_0000, 0x4000_0000, 0x0000_0000]);
        let end = quad_of_bits([0x4060_0000, 0xc114_0000, 0x4000_0000, 0x0000_0000]);
        let expansion = quad_of_bits([0x3ecc_cccd; 4]);
        let points = points(&[[0.0, 0.0, 0.0], [0.0, 0.0, 1.8]]);
        let (centre, shifted_end, step) =
            support_hull(&points, 2, q, b, v, [1.0; 4], start, end, expansion);
        assert_eq!(
            bits_of(centre),
            [0x3fed_5cbf, 0xc03c_0712, 0x3f9a_d175, 0x0000_0000]
        );
        assert_eq!(
            bits_of(shifted_end),
            [0x3fed_5cbf, 0xc12f_01c4, 0x3f9a_d175, 0x0000_0000]
        );
        assert_eq!(
            bits_of(step),
            [0x3f46_3e9c, 0x3f8d_d0b2, 0x3f61_2808, 0x3ed7_0a3e]
        );
    }

    /// A capsule-shaped query reaches the client's walk: the hull's centre and
    /// step decide which leaves the walk expands, and the counters are the ones
    /// the client's `0x142caed30` publishes for this query
    /// (`dev/mesh_support_diff.py`, case "capsule over quad A").
    ///
    /// The contact is **not** reproduced: the client's triangle test accepts here
    /// and `0x142cc9460` is not implemented, so this reports a miss where the
    /// client reports a hit. That is the one remaining deviation of the type-3
    /// narrowphase and it is the last assertion below.
    #[test]
    fn a_support_point_hull_reaches_the_client_walk() {
        let world = mesh_world_with_points(&[[0.0, 0.0, 0.4], [0.0, 0.0, -0.4]]);
        let block = mesh_block_with_points(
            [0.75, 0.9, 5.0, 0.0],
            [0.75, 0.9, -5.0, 0.0],
            1.0,
            0.4,
            [0.0, 0.0, 0.0, 1.0],
            2,
            0x0001,
        );
        let sweep = sweep_world(&world, &block, &identity_xform([0.0, 0.0, 0.0]));
        assert_eq!(sweep.internals, 1);
        assert_eq!(sweep.leaves, 1);
        assert!(sweep.hit.is_none(), "0x142cc9460 is unread");
    }

    /// Beside the quads the same capsule expands the root and clears both leaves;
    /// the counters are again the client's.
    #[test]
    fn a_support_point_hull_beside_the_mesh_clears_the_leaves() {
        let world = mesh_world_with_points(&[[0.0, 0.0, 0.4], [0.0, 0.0, -0.4]]);
        let block = mesh_block_with_points(
            [3.0, 0.5, 5.0, 0.0],
            [3.0, 0.5, -5.0, 0.0],
            1.0,
            0.4,
            [0.0, 0.0, 0.0, 1.0],
            2,
            0x0001,
        );
        let sweep = sweep_world(&world, &block, &identity_xform([0.0, 0.0, 0.0]));
        assert_eq!(sweep.internals, 1);
        assert_eq!(sweep.leaves, 0);
        assert!(sweep.hit.is_none());
    }

    /// The leaf reach sits exactly on `centre + step`: at that boundary the leaf
    /// is expanded and one ULP further out it is not. The two runs differ, which
    /// is what makes the step an observable; the client agrees on both
    /// (`dev/mesh_support_diff.py`, the step ladder).
    #[test]
    fn the_support_point_step_decides_the_leaf_reach() {
        let points = [[0.4, 0.0, 0.0], [-0.4, 0.0, 0.0]];
        let block = mesh_block_with_points(
            [0.0, 0.0, 5.0, 0.0],
            [0.0, 0.0, -5.0, 0.0],
            1.0,
            0.0,
            [0.0, 0.0, 0.0, 1.0],
            2,
            0x0001,
        );
        let at = f32::from_bits(0x3ed7_0a3e); // 0.4 + 0.02
        let mut leaves = Vec::new();
        for boundary in [at, f32::from_bits(at.to_bits() + 1)] {
            let mut world = mesh_world_with_points(&points);
            // A root whose second child sits two nodes on, one leaf square
            // starting at the boundary, and a masked-out third node.
            let mut nodes = vec![0u8; MESH_NODE_LEN * 3];
            put_node(
                &mut nodes,
                0,
                [-8.0, -8.0, -8.0],
                [8.0, 8.0, 8.0],
                2,
                0x0001,
                0,
                0,
            );
            put_node(
                &mut nodes,
                MESH_NODE_LEN,
                [boundary, 0.0, 0.0],
                [boundary + 1.0, 1.0, 0.0],
                0,
                0x0001,
                0,
                1,
            );
            put_node(
                &mut nodes,
                2 * MESH_NODE_LEN,
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0],
                0,
                0x0000,
                0,
                0,
            );
            world[NODES_AT..NODES_AT + nodes.len()].copy_from_slice(&nodes);
            put_quad(&mut world, VERTICES_AT, [boundary, 0.0, 0.0, 0.0]);
            put_quad(
                &mut world,
                VERTICES_AT + MESH_VERTEX_LEN,
                [boundary + 1.0, 0.0, 0.0, 0.0],
            );
            put_quad(
                &mut world,
                VERTICES_AT + 2 * MESH_VERTEX_LEN,
                [boundary + 1.0, 1.0, 0.0, 0.0],
            );
            put_triangle(&mut world, TRIANGLES_AT, [0, 1, 2], [0x51, 0x52], 0x0001);
            let sweep = sweep_world(&world, &block, &identity_xform([0.0, 0.0, 0.0]));
            leaves.push(sweep.leaves);
        }
        assert_eq!(leaves, vec![1, 0]);
    }

    #[test]
    fn the_mesh_scale_multiplies_the_local_hit() {
        // A shape scaled by two halves the query on the way in, so the world
        // hit of a sweep at x = 1.5 lands on the mesh's x = 0.75.
        let mesh = fixture(2.0, 0.5);
        let block = mesh_block(
            [1.5, 0.5, 5.0, 0.0],
            [1.5, 0.5, -5.0, 0.0],
            1.0,
            0.004,
            0,
            0x0001,
        );
        let hit = mesh
            .sweep(&block, &identity_xform([0.0, 0.0, 0.0]))
            .hit
            .expect("the enlarged quad covers (1.5, 0.5)");
        assert_eq!(hit.point, [1.5, 0.5, 0.0, 0.0]);
        // The local point is (0.75, 0.25), below the quad's diagonal.
        assert_eq!(hit.triangle, 0);
    }

    #[test]
    fn the_mesh_transform_moves_the_contact_into_world_space() {
        let mesh = fixture(1.0, 1.0);
        let block = mesh_block(
            [0.75, 0.9, 5.0, 0.0],
            [0.75, 0.9, -5.0, 0.0],
            1.0,
            0.004,
            0,
            0x0001,
        );
        let hit = mesh
            .sweep(&block, &identity_xform([0.0, 0.0, 3.0]))
            .hit
            .expect("the local quad still lies under the sweep");
        assert_eq!(hit.point, [0.75, 0.9, 3.0, 0.0]);
    }

    #[test]
    fn the_last_hit_triangle_wins() {
        // Two stacked, upward-facing quads both accept the same sweep; the walk
        // keeps the last triangle it tests, which is also the farther one.
        let mut mesh = fixture(1.0, 1.0);
        for slot in 4..8 {
            let corner = [
                [0.0f32, 0.0, -1.0],
                [1.0, 0.0, -1.0],
                [1.0, 1.0, -1.0],
                [0.0, 1.0, -1.0],
            ][slot - 4];
            put_quad(
                &mut mesh.world,
                VERTICES_AT + slot * MESH_VERTEX_LEN,
                [corner[0], corner[1], corner[2], 0.0],
            );
        }
        put_node(
            &mut mesh.world,
            NODES_AT + 2 * MESH_NODE_LEN,
            [0.0, 0.0, -1.0],
            [1.0, 1.0, 0.0],
            2,
            0x0003,
            0,
            2,
        );
        for (index, mask) in [0x0001u16, 0x0001, 0x0002, 0x0002].iter().enumerate() {
            let at = TRIANGLES_AT + index * MESH_TRIANGLE_LEN + 0x1c;
            mesh.world[at..at + 2].copy_from_slice(&mask.to_le_bytes());
        }
        for index in 0..3 {
            let at = NODES_AT + index * MESH_NODE_LEN + 0x1c;
            mesh.world[at..at + 2].copy_from_slice(&0x0003u16.to_le_bytes());
        }
        let block = mesh_block(
            [0.75, 0.9, 5.0, 0.0],
            [0.75, 0.9, -5.0, 0.0],
            1.0,
            0.004,
            0,
            0x0002,
        );
        let hit = mesh
            .sweep(&block, &identity_xform([0.0, 0.0, 0.0]))
            .hit
            .expect("both quads accept the sweep");
        assert_eq!(hit.triangle, 3);
        assert_eq!(hit.point[2], -1.0);
        assert!(hit.fraction[0] > 0.5);
    }
}

#[cfg(test)]
mod mesh_case0 {
    use super::*;

    #[test]
    fn the_python_fixture_case_hits() {
        let element = std::fs::read("../../dev/data/read/case0_element.bin").unwrap();
        let block = std::fs::read("../../dev/data/read/case0_block.bin").unwrap();
        let xform = std::fs::read("../../dev/data/read/case0_xform.bin").unwrap();
        let world = MeshWorld {
            base: 0x50,
            bytes: &element[0x50..],
        };
        let sweep = mesh_sweep(&element[..0x50], &world, &block, &xform);
        println!(
            "hit={:?} internals={} leaves={}",
            sweep
                .hit
                .as_ref()
                .map(|h| (h.triangle, h.point, h.normal, h.fraction)),
            sweep.internals,
            sweep.leaves
        );
        assert!(sweep.hit.is_some(), "expected a hit");
    }
}

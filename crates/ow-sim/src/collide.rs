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
//! expansion compares `<= 0.005`, and the closest-point solver `0x142cc9460`
//! otherwise. Both are implemented; the solver's four-vertex reduction
//! (`0x142cc7740`) is the one branch still open. See [`mesh_triangle`] and
//! [`closest_point`].

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
// The query driver's per-entry pruning (`0x142cbd510`, the walk's inner body).
// ---------------------------------------------------------------------------

/// `FLT_EPSILON` splatted, the driver's constant at `0x1435ecad0`.
const QUERY_EPSILON: f32 = f32::from_bits(0x3400_0000);

/// The separation constant splatted, the driver's constant at `0x1435ecaf0`.
const QUERY_SEPARATION: f32 = f32::from_bits(0x3ba3_d70a);

/// The halving constant the driver loads at `0x142f844e0` (`0x3f000000`).
/// The value is the same as [`HALF`]; the address is different because the
/// driver loads its own copy.
const QUERY_HALF: f32 = 0.5;

/// Everything one pass of the driver derives from the sweep before it walks
/// the shape list.
///
/// The driver builds it at `0x142cbd63a..0x142cbd667`, once per pass:
///
/// ```text
/// delta   = end - start                                  (0x142cbd647)
/// epsilon = |delta| + FLT_EPSILON                        (0x142cbd655)
/// reach   = start + motion * delta                       (0x142cbd651)
/// low     = MINPS(start, reach)                          (0x142cbd664)
/// high    = MAXPS(start, reach)                          (0x142cbd667)
/// ```
///
/// `start`, `end` and `expansion` are the three quads `CollisionSweepBounds`
/// (`0x142cd5540`, [`sweep_bounds`]) writes; `motion` is the query block's
/// `+0x20` splat, which the driver never re-derives inside a pass.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PassGeometry {
    /// The sweep's start quad, the pass's `MINPS` operand and its box corner.
    pub start: Quad,
    /// `end - start`, the direction the moving-box test uses.
    pub delta: Quad,
    /// `|delta| + FLT_EPSILON`, the per-lane motion tolerance.
    pub epsilon: Quad,
    /// `MINPS(start, start + motion*delta)`: the swept box's low corner.
    pub low: Quad,
    /// `MAXPS(start, start + motion*delta)`: the swept box's high corner.
    pub high: Quad,
}

/// The per-pass geometry, read from `0x142cbd63a..0x142cbd667`.
pub fn pass_geometry(start: Quad, end: Quad, motion: Quad) -> PassGeometry {
    let delta = sub_lane(end, start);
    let mut epsilon = [0.0f32; 4];
    for lane in 0..4 {
        epsilon[lane] = abs_lane(delta[lane]) + QUERY_EPSILON;
    }
    let reach = add_lane(mul_lane(motion, delta), start);
    let mut low = [0.0f32; 4];
    let mut high = [0.0f32; 4];
    for lane in 0..4 {
        // `MINPS start, reach` / `MAXPS start, reach`: the source (the reach)
        // wins on NaN, which is the `minss`/`maxss` rule.
        low[lane] = minss(start[lane], reach[lane]);
        high[lane] = maxss(start[lane], reach[lane]);
    }
    PassGeometry {
        start,
        delta,
        epsilon,
        low,
        high,
    }
}

/// The driver's first rejection (`0x142cbd6bf..0x142cbd6e3`): the expanded
/// shape box against the swept box, on x, y and z only.
///
/// ```text
/// high_expanded = entry.max + expansion
/// low_expanded  = entry.min - expansion
/// reject if the sign bit is set in ((high_expanded - low) | (high - low_expanded))
/// ```
///
/// The client ORs the two difference quads and reads `MOVMSKPS`, then tests
/// `AL & 7`. The sign bit, not the value: a `-0.0` difference rejects, exactly
/// as the client's `TEST` does. `true` means **rejected**.
pub fn aabb_reject(
    geometry: &PassGeometry,
    entry_min: Quad,
    entry_max: Quad,
    expansion: Quad,
) -> bool {
    let high_expanded = add_lane(entry_max, expansion);
    let low_expanded = sub_lane(entry_min, expansion);
    for lane in 0..3 {
        let a = high_expanded[lane] - geometry.low[lane];
        let b = geometry.high[lane] - low_expanded[lane];
        if sign_bit(a) || sign_bit(b) {
            return true;
        }
    }
    false
}

/// The driver's second rejection (`0x142cbd6e9..0x142cbd749`): the moving-box
/// separating-axis test, on x, y and z only.
///
/// With `mid = (max+min)*0.5`, `half = (max-min)*0.5` over the **expanded**
/// box, `b = start - mid` and `e = |delta| + FLT_EPSILON`:
///
/// ```text
/// cross = |rot3(rot3(delta)*b - rot3(b)*delta)|
/// sym   =    rot3(rot3(e)*half + rot3(half)*e)
/// reject if 0.005 < cross[i] - sym[i]  for any of x, y, z
/// ```
///
/// `rot3` is `SHUFPS v, v, 0xc9`, which the client applies twice (once to each
/// product, then again to the sum), so the three real lanes come out in the
/// order `(z, x, y)` of the plain cross product. The lane order is kept
/// because the client's `ANDPS`/`SUBPS` pair runs on it; the verdict is a
/// three-lane `OR`, so only the set of lanes matters, not their order.
/// `true` means **rejected**.
pub fn moving_box_reject(
    geometry: &PassGeometry,
    entry_min: Quad,
    entry_max: Quad,
    expansion: Quad,
) -> bool {
    let high_expanded = add_lane(entry_max, expansion);
    let low_expanded = sub_lane(entry_min, expansion);
    let mid = mul_lane(add_lane(high_expanded, low_expanded), splat(QUERY_HALF));
    let half = mul_lane(sub_lane(high_expanded, low_expanded), splat(QUERY_HALF));
    let b = sub_lane(geometry.start, mid);
    let cross = rot3(sub_lane(
        mul_lane(rot3(geometry.delta), b),
        mul_lane(rot3(b), geometry.delta),
    ));
    let sym = rot3(add_lane(
        mul_lane(rot3(geometry.epsilon), half),
        mul_lane(rot3(half), geometry.epsilon),
    ));
    let mut diff = [0.0f32; 4];
    for lane in 0..4 {
        diff[lane] = abs_lane(cross[lane]) - sym[lane];
    }
    for lane in 0..3 {
        if QUERY_SEPARATION < diff[lane] {
            return true;
        }
    }
    false
}

/// The driver's four-pass walk, `0x142cbd510`.
///
/// `pass_roots` is the shape index at `A + 0x44 + 0x28*pass` for the four
/// passes, `-1` meaning the pass has no shapes. `block` is the `0x80`-byte
/// query block; `points` is the local support-point array the block's `+0x30`
/// points at. `flags` is the driver's fourth argument (`R9B`): bit `0` runs
/// pass 0, bit `1` pass 1, and so on, and `SHR R15B,0x1` (`0x142cbd80f`)
/// shifts it once per pass whether the pass ran or not.
///
/// The pass order, read from the instruction stream:
///
/// 1. `CollisionSweepBounds` once per pass (`0x142cbd62c`); a clear flag bit
///    skips the pass entirely (`0x142cbd5e0`).
/// 2. [`pass_geometry`] from the three quads it wrote and the block's motion.
/// 3. The worklist starts as `[root]` (`0x142cbd692`) and is popped from the
///    end (`0x142cbd6a4`), so a pushed pair is visited `+0x30` first.
/// 4. [`aabb_reject`], then [`moving_box_reject`], then the leaf test on
///    `entry+0x2c == -1` (`0x142cbd756`).
/// 5. A leaf calls the narrowphase; `2` folds the hit into the running motion
///    and, when every lane improves, re-seats the swept box on
///    `start + hit*delta` (`0x142cbd791..0x142cbd7c3`); `1` ends the pass
///    (`0x142cbd7c9`); anything else continues.
/// 6. A branch pushes `entry+0x2c` then `entry+0x30` (`0x142cbd75f`).
///
/// Two folds, not one. `XMM6` is a per-lane running minimum over every `2`
/// answer (`0x142cbd7a1`), and it is what the driver returns
/// (`0x142cbd7ff`). `[RBP-0x40]` is the operand the four-lane test compares
/// against (`0x142cbd799`), and it is only rewritten on an all-lane hit
/// (`0x142cbd7ae`) - so after a partial hit the comparison operand stays at
/// the older, larger value while the minimum keeps shrinking. Every pass that
/// runs re-seeds the comparison operand from the returned minimum
/// (`0x142cbd603`), which is why the two only diverge within one pass.
///
/// The client's observable output is whatever the narrowphase wrote through
/// the descriptor (the answer record at `D[3]` and the event list at `D[4]`);
/// the driver itself writes nothing outside its frame.
pub fn query_driver(
    pass_roots: [i32; 4],
    block: &[u8],
    points: &[u8],
    flags: u8,
    world: &mut dyn QueryWorld,
) -> Quad {
    let motion = quad_at(block, 0x20);
    // `XMM6`: the running per-lane minimum, returned and carried across passes.
    let mut fold_min = motion;
    let mut flags = flags;

    for pass in 0..4 {
        if flags & 1 == 0 {
            flags >>= 1;
            continue;
        }
        flags >>= 1;
        // `0x142cbd603`: the comparison operand is re-seeded from `XMM0`, which
        // is the minimum the previous pass returned.
        let mut fold_cmp = fold_min;
        // `0x142cbd62c`: the three quads come from `CollisionSweepBounds`, not
        // straight from the block - the block's own `+0x00`/`+0x10` are that
        // call's inputs, and its `+0x50` is only the narrowphase's expansion.
        let (swept_start, swept_end, swept_expansion) = sweep_bounds(block, points);
        let mut geometry = pass_geometry(swept_start, swept_end, motion);
        let root = pass_roots[pass];
        if root == -1 {
            continue;
        }
        let mut worklist: Vec<i32> = Vec::with_capacity(0x100);
        worklist.push(root);
        'walk: while let Some(index) = worklist.pop() {
            let Some(entry) = world.entry(pass, index) else {
                continue;
            };
            let entry_min = quad_at(entry, 0x00);
            let entry_max = quad_at(entry, 0x10);
            if aabb_reject(&geometry, entry_min, entry_max, swept_expansion) {
                continue;
            }
            if moving_box_reject(&geometry, entry_min, entry_max, swept_expansion) {
                continue;
            }
            let child = i32::from_le_bytes(entry[0x2c..0x30].try_into().unwrap());
            if child == -1 {
                let id = i32::from_le_bytes(entry[0x20..0x24].try_into().unwrap());
                let mut hit = [0.0f32; 4];
                let answer = world.narrowphase(pass, id, &mut hit);
                match answer {
                    2 => {
                        // `0x142cbd799`: compare against the old operand, then
                        // `0x142cbd7a1` folds the minimum. `CMPLTPS` is false
                        // for a NaN lane, so a NaN never counts as improving.
                        let mut every_lane = true;
                        for lane in 0..4 {
                            let improves = hit[lane] < fold_cmp[lane];
                            if !improves {
                                every_lane = false;
                            }
                            fold_min[lane] = minss(fold_min[lane], hit[lane]);
                        }
                        if every_lane {
                            // `0x142cbd7ae`: the operand becomes the hit itself.
                            fold_cmp = hit;
                            let reach = add_lane(mul_lane(hit, geometry.delta), geometry.start);
                            for lane in 0..4 {
                                geometry.low[lane] = minss(geometry.start[lane], reach[lane]);
                                geometry.high[lane] = maxss(geometry.start[lane], reach[lane]);
                            }
                        }
                    }
                    1 => break 'walk,
                    _ => {}
                }
            } else {
                let second = i32::from_le_bytes(entry[0x30..0x34].try_into().unwrap());
                worklist.push(child);
                worklist.push(second);
            }
        }
    }
    fold_min
}

/// What [`query_driver`] needs from the world it walks.
///
/// The two methods are the driver's only two callees into the world:
/// `entry` is the shape-entry array at `[A + 0x30]`, reached as `[RSI-0x14]`
/// with `RSI` advanced by `0x28` per pass (`0x142cbd812`), so the array base
/// for pass `p` is `A + 0x30 + 0x28*p`. An entry is **`0x40` bytes**: the
/// driver indexes it with `SHL RBX,0x6` (`0x142cbd6bb`) and reads `+0x00`,
/// `+0x10`, `+0x20`, `+0x2c` and `+0x30`. `narrowphase` is `0x142cc1610`,
/// called `(ctx, &out_a, &out_b, id)` - the driver passes the hit quad as its
/// second output (`R8 = RBP-0x60`) and a second quad as its first
/// (`RDX = RSP+0x50`), which is the quad the fold reads. Its contract is
/// `1` = end the sweep, `2` = hit, anything else = continue.
pub trait QueryWorld {
    /// The shape entry `index` of `pass`'s array, `0x40` bytes.
    fn entry(&self, pass: usize, index: i32) -> Option<&[u8]>;
    /// `0x142cc1610`: transform, filter and dispatch one leaf shape.
    fn narrowphase(&mut self, pass: usize, id: i32, hit: &mut Quad) -> i32;
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

    /// Whether `address` addresses a byte of the blob. A pointer the fixture
    /// supplies that fails this is not a client state the walk can be read
    /// against, so the entry point reports a miss instead of panicking.
    fn covers(&self, address: u64) -> bool {
        address.wrapping_sub(self.base) < self.bytes.len() as u64
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
/// the block the walk copies, and `support` is that copy's point array - the
/// hull's transformed points with the centre subtracted, which is what the
/// client's descriptor points at (`0x142caf170`). `flag0` is the query block's
/// byte at `+0x72`, the descriptor's `+0xa0` flag.
///
/// Every other state - a support-point count of any size, or an expansion lane
/// above `0.005` - builds the descriptor at `0x142cce6e8` and calls the
/// closest-point solver `0x142cc9460`, whose output's residual is the hit
/// fraction and whose point and normal are the contact.
fn mesh_triangle(
    vertices: [Quad; 3],
    start: Quad,
    end: Quad,
    motion: Quad,
    expansion: Quad,
    count: i32,
    support: &[Quad],
    flag0: u8,
) -> Option<(Quad, Quad, Quad)> {
    // The client's fast path runs when every lane of the scaled expansion is
    // at most 0.005; anything else is the support-point (slow) path.
    if count != 0 || !all_lanes(|lane| expansion[lane] <= 0.005) {
        // The descriptor is built from the query block copy the walk made: the
        // identity rotation for the triangle, and for the second set either the
        // block's own start point (count zero, one point) or the recentred
        // support array at the block's start.
        let zero = [0.0f32; 4];
        let identity = [0.0, 0.0, 0.0, 1.0];
        let (points, translation) = if count > 0 {
            (support, start)
        } else {
            (&[start][..], zero)
        };
        let input = GjkInput {
            a: &vertices,
            b: points,
            shape_a: GjkShape {
                radius: splat(0.01),
                translation: zero,
                rotation: identity,
            },
            shape_b: GjkShape {
                radius: expansion,
                translation,
                rotation: identity,
            },
            direction: sub_lane(end, start),
            motion,
            flag0,
            flag1: 0,
        };
        let mut out = GjkOutput::default();
        if !closest_point(&input, &mut out) {
            return None;
        }
        return Some((out.point, out.normal, out.residual));
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
    buffer: &mut Vec<Quad>,
) -> (Quad, Quad, Quad) {
    let (rows, translation) = support_hull_affine(q, b, v);
    let mut low = [0.0f32; 4];
    let mut high = [0.0f32; 4];
    buffer.clear();
    for index in 0..count {
        let base = index * MESH_VERTEX_LEN;
        let mut point = add_lane(mul_lane(splat(f32_at(points, base)), rows[0]), translation);
        point = add_lane(point, mul_lane(splat(f32_at(points, base + 4)), rows[1]));
        point = add_lane(point, mul_lane(splat(f32_at(points, base + 8)), rows[2]));
        let point = mul_lane(point, inverse);
        buffer.push(point);
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
    // The client keeps the transformed points in a scratch buffer and subtracts
    // the centre from each (0x142caf170); that recentred array is what its
    // triangle test's descriptor points at.
    for point in buffer.iter_mut() {
        *point = sub_lane(*point, centre);
    }
    (centre, end, step)
}

/// `0x142caed30` type 3 (mesh): a swept query against a triangle BVH.
///
/// Both query shapes are the client's: the count `> 0` branch builds the
/// support-point hull (`0x142caef15..0x142caf1a5`) and keeps the recentred
/// points for the triangle test, and every triangle is tested by
/// [`mesh_triangle`], which reaches the closest-point solver
/// ([`closest_point`]) whenever the block carries support points or an
/// expansion lane above `0.005`. The triangle callback argument is null on this
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
    // The three pointers are fixture data. One outside the blob is not a state
    // the client can be in (it would read out of bounds), so report the miss
    // rather than panic inside the walk.
    if !world.covers(root) || !world.covers(vertices) || !world.covers(triangles) {
        return missed;
    }

    let position = quad_at(xform, 0x00);
    let q = quad_at(xform, 0x10);
    let start_world = quad_at(block, 0x00);
    let end_world = quad_at(block, 0x10);
    // The motion budget shrinks to the last accepted fraction (0x142caf5c1).
    let mut motion = quad_at(block, 0x20);
    let count = i32_at(block, 0x38);
    let mask = u16_at(block, 0x70);
    let expansion = mul_lane(quad_at(block, 0x50), inverse);

    // The recentred support points the walk keeps for the triangle test
    // (`0x142caf170`); empty when the query carries none.
    let mut support: Vec<Quad> = Vec::new();
    let flag0 = block[0x72];
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
            &mut support,
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
                        &support,
                        flag0,
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

/// `0x142cb8b80`: builds a shape entry's world frame.
///
/// The output is `[position, quaternion]` — the position quad first, then the
/// frame's quaternion — which is the `xform` the narrowphase primitives take
/// (`position` at `+0x00`, `quaternion` at `+0x10`).
///
/// `entry[0xb0]` selects the source. Anything but `1` — the owner-attached
/// state `ShapeAddToOwner` (`0x142cd4ef0`) sets — copies from the **owner
/// pool** `[record+0x2a0]`, element `entry[0x50]` at stride `0x50`: the
/// position is the element's `+0x10` quad and the quaternion its `+0x00`
/// quad. `1` — the body-attached state `ShapeAddToBody` (`0x142cb3940`) sets —
/// reads the **owner pose** `[record+0x288] + entry[0x50]*0x1c0` instead: with
/// `record[0x191] != 0` and `record[0x192] == 0` the pose is composed (see
/// [`compose_pose`]), otherwise its `+0x10` quad (position) and `+0x20` quad
/// (quaternion) are copied verbatim.
///
/// Both sources are pointers in the client. The caller resolves them and
/// passes them as `owners` (`[record+0x2a0]`) and `poses` (`[record+0x288]`);
/// `entry[0x50]` must index inside them.
pub fn shape_world_transform(
    entry: &[u8],
    record: &[u8],
    owners: &[u8],
    poses: &[u8],
) -> [Quad; 2] {
    let slot = i32_at(entry, 0x50) as usize;
    if entry[0xb0] != 1 {
        let base = slot * 0x50;
        return [quad_at(owners, base + 0x10), quad_at(owners, base)];
    }
    let base = slot * 0x1c0;
    if record[0x191] != 0 && record[0x192] == 0 {
        return compose_pose(&poses[base..base + 0x1c0]);
    }
    [quad_at(poses, base + 0x10), quad_at(poses, base + 0x20)]
}

/// The zero quad at `0x142f84a00`, which the client's sign-select and pivot
/// terms multiply by.
const ZERO_QUAD: Quad = [0.0, 0.0, 0.0, 0.0];

/// The composed-pose branch of [`shape_world_transform`]
/// (`0x142cb8bbf..0x142cb8cb9`): the owner pose rotated by its own quaternion.
///
/// `pose[+0x90]` is the quaternion the frame is normalised from and
/// `pose[+0x60]` the reference its sign is chosen against — the two are dot
/// producted, and a positive dot keeps `+0x60`, a negative one negates it.
/// `pose[+0x70]` is the offset carried into the frame and `pose[+0x50]` /
/// `pose[+0x80]` the two ends of the pivot term.
fn compose_pose(pose: &[u8]) -> [Quad; 2] {
    let q = quad_at(pose, 0x90);
    let reference = quad_at(pose, 0x60);
    // `q . reference`, broadcast to all four lanes by two horizontal adds
    // (`SHUFPS 0xb1`, `ADDPS`, `SHUFPS 0x4e`, `ADDPS`).
    let product = mul_lane(q, reference);
    let pairs = add_lane(shufps(product, product, 0xb1), product);
    let dot = add_lane(shufps(pairs, pairs, 0x4e), pairs);
    // `CMPLTPS zero, dot` then `ANDPS`/`ANDNPS`/`ORPS`: `reference` or its
    // sign flip, chosen per lane.
    let flipped = negate(reference);
    let aligned = [
        if dot[0] > 0.0 {
            reference[0]
        } else {
            flipped[0]
        },
        if dot[1] > 0.0 {
            reference[1]
        } else {
            flipped[1]
        },
        if dot[2] > 0.0 {
            reference[2]
        } else {
            flipped[2]
        },
        if dot[3] > 0.0 {
            reference[3]
        } else {
            flipped[3]
        },
    ];
    // `q + (aligned - q) * 0.0`. The client's select is a no-op that still
    // propagates a non-finite difference, so it is kept verbatim.
    let q = add_lane(mul_lane(sub_lane(aligned, q), ZERO_QUAD), q);
    // The normalised quaternion, written to the output's `+0x10` quad.
    let squared = mul_lane(q, q);
    let pairs = add_lane(shufps(squared, squared, 0xb1), squared);
    let length = add_lane(shufps(pairs, pairs, 0x4e), pairs);
    let inverse = splat(1.0 / length[0].sqrt());
    let normal = mul_lane(inverse, q);

    // `w*offset + (normal x offset)`, then the same shape again against the
    // normal, doubled and added to the offset, subtracted from the pivot.
    let offset = quad_at(pose, 0x70);
    let carried = sub_lane(
        mul_lane(rot3(offset), normal),
        mul_lane(rot3(normal), offset),
    );
    let crossed = add_lane(mul_lane(splat(normal[3]), offset), rot3(carried));
    let turned = sub_lane(
        mul_lane(rot3(crossed), normal),
        mul_lane(crossed, rot3(normal)),
    );
    let pivot = quad_at(pose, 0x80);
    let origin = add_lane(
        mul_lane(sub_lane(quad_at(pose, 0x50), pivot), ZERO_QUAD),
        pivot,
    );
    let swept = add_lane(add_lane(rot3(turned), rot3(turned)), offset);
    [sub_lane(origin, swept), normal]
}

/// What `0x142cc1610` concluded for one shape.
///
/// The client writes the same three codes as the sink record's status dword:
/// `0` when the layer filter rejects the shape, `2` when the per-type
/// primitive finds no hit, and `1` when it hits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Narrowphase {
    /// The layer filter said no. `out` is untouched.
    Filtered,
    /// The primitive found no hit. `out` is untouched.
    Rejected,
    /// The primitive hit. `out` holds the hit record's `+0x20` quad.
    Hit,
    /// The type byte names a primitive that has not been read (`2` and `4+`).
    UnreadType(i8),
}

/// The world `0x142cc1610` reaches: the record, the entry array at
/// `[record+0x2b8]`, the two transform pools, the layer filter's arguments,
/// the five per-type pools at `+0x2d0`/`+0x2e8`/`+0x300`/`+0x318`/`+0x330`,
/// and the memory the type-3 element's pointers live in.
pub struct NarrowphaseWorld<'a> {
    pub record: &'a [u8],
    pub entries: &'a [u8],
    pub owners: &'a [u8],
    pub poses: &'a [u8],
    pub filter: &'a [u8],
    pub pools: [&'a [u8]; 5],
    pub mesh: Option<MeshWorld<'a>>,
}

/// `0x142cc1610`: transform one shape entry, filter it, dispatch it on its type
/// byte, and hand back the hit quad.
///
/// The wrapper builds a `0x40`-byte hit record on its frame — `+0x00..+0x20`
/// zeroed, `+0x30 = -1`, `+0x34 = 0`, `+0x38 = 0xffff`, `+0x3a = 0`,
/// `+0x3c = 0` — passes it as the primitive's second argument, and then
/// publishes only its **`+0x20`** quad. Two fields are rewritten afterwards
/// from the entry: `+0x34` from `entry[0xaa]` **unless the type byte is 3 or
/// 4**, and `+0x38` from `entry[0xae]` unconditionally.
///
/// The primitive's arguments are `(element, out, block, xform)`: the element is
/// `pool[index]` with `index = entry[0x24]`, the out is that hit record, the
/// block is this function's third argument, and the xform is the frame just
/// built. Each of the verified primitives takes a subset of those in Rust form,
/// so the record is assembled here instead of being written in place.
///
/// The client does not bounds-check the shape index or the pool index; neither
/// is a state it can be in, so an out-of-range index reports
/// [`Narrowphase::Rejected`] rather than reading past the fixture, which is the
/// same convention `mesh_sweep` uses for a pointer outside its blob.
pub fn shape_narrowphase(
    world: &NarrowphaseWorld,
    block: &[u8],
    shape: i32,
    out: &mut Quad,
) -> Narrowphase {
    let base = shape as usize * 0xc0;
    let Some(entry) = world.entries.get(base..base + 0xc0) else {
        return Narrowphase::Rejected;
    };
    let frame = shape_world_transform(entry, world.record, world.owners, world.poses);
    if !shape_filter(world.filter, &entry[0x98..]) {
        return Narrowphase::Filtered;
    }

    let mut record = [0u8; 0x40];
    record[0x30..0x34].copy_from_slice(&(-1i32).to_le_bytes());
    record[0x38..0x3a].copy_from_slice(&0xffffu16.to_le_bytes());

    let mut xform = [0u8; 0x20];
    for (index, quad) in frame.iter().enumerate() {
        for (lane, value) in quad.iter().enumerate() {
            let at = index * 0x10 + lane * 4;
            xform[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
    }

    let kind = entry[0x20] as i8;
    let index = i32_at(entry, 0x24);
    let element = |slot: usize, stride: usize| -> Option<&[u8]> {
        let at = index as usize * stride;
        world.pools[slot].get(at..at + stride)
    };

    let hit = match kind {
        0 => match element(0, 0x20).and_then(|e| sphere_sweep(e, block, &xform)) {
            Some(quads) => {
                for (slot, quad) in quads.iter().enumerate() {
                    record[slot * 0x10..slot * 0x10 + 0x10].copy_from_slice(&quad_bytes(*quad));
                }
                true
            }
            None => false,
        },
        1 => match element(1, 0x30).and_then(|e| capsule_sweep(e, block, &xform)) {
            Some(quads) => {
                for (slot, quad) in quads.iter().enumerate() {
                    record[slot * 0x10..slot * 0x10 + 0x10].copy_from_slice(&quad_bytes(*quad));
                }
                true
            }
            None => false,
        },
        3 => {
            let (Some(mesh), Some(e)) = (world.mesh.as_ref(), element(3, 0x50)) else {
                return Narrowphase::Rejected;
            };
            match mesh_sweep(e, mesh, block, &xform).hit {
                Some(hit) => {
                    record[0x00..0x10].copy_from_slice(&quad_bytes(hit.point));
                    record[0x10..0x20].copy_from_slice(&quad_bytes(hit.normal));
                    record[0x20..0x30].copy_from_slice(&quad_bytes(hit.fraction));
                    record[0x30..0x34].copy_from_slice(&hit.triangle.to_le_bytes());
                    record[0x34..0x36].copy_from_slice(&hit.ids[0].to_le_bytes());
                    record[0x36..0x38].copy_from_slice(&hit.ids[1].to_le_bytes());
                    record[0x38..0x3a].copy_from_slice(&0xffffu16.to_le_bytes());
                    true
                }
                None => false,
            }
        }
        other => return Narrowphase::UnreadType(other),
    };
    if !hit {
        return Narrowphase::Rejected;
    }
    // `0x142cc1610`'s two id overwrites, applied after the primitive ran.
    if kind != 3 && kind != 4 {
        record[0x34..0x38].copy_from_slice(&entry[0xaa..0xae]);
    }
    record[0x38..0x3a].copy_from_slice(&entry[0xae..0xb0]);
    *out = quad_at(&record, 0x20);
    Narrowphase::Hit
}

fn quad_bytes(quad: Quad) -> [u8; 16] {
    let mut out = [0u8; 16];
    for (lane, value) in quad.iter().enumerate() {
        out[lane * 4..lane * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    out
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

// ---------------------------------------------------------------------------
// The closest-point GJK the type-3 triangle test falls into (`0x142cc9460`).
// ---------------------------------------------------------------------------

/// One simplex vertex: the client's 0x50-byte record.
///
/// `dw` is `w - p`, the Minkowski point the region tests work on; `p` is the
/// point taken from the first set and `w` the point from the second set moved
/// along the sweep by the running fraction; `bary` is the weight the
/// sub-simplex routines rewrite; `index` are the two support-array indices.
#[derive(Clone, Copy, Default)]
pub struct SimplexVertex {
    pub dw: Quad,
    pub p: Quad,
    pub w: Quad,
    pub bary: Quad,
    pub index: [i32; 2],
    pub pad: [u8; 8],
}

/// The cached simplex: four vertices and the live count at `+0x140`.
#[derive(Clone, Copy, Default)]
pub struct Simplex {
    pub vertex: [SimplexVertex; 4],
    pub count: i32,
}

/// One simplex vertex record's length (`0x50`).
pub const SIMPLEX_VERTEX_LEN: usize = 0x50;
/// The whole simplex block the sub-simplex routines take: four records plus
/// the count (`0x140`), `0x150` bytes.
pub const SIMPLEX_BYTES_LEN: usize = 0x150;

impl Simplex {
    /// Read the client's byte layout (`0x142cc8d80`/`0x142cc7740` take this
    /// block by pointer).
    pub fn from_bytes(bytes: &[u8]) -> Simplex {
        let mut out = Simplex::default();
        for (index, vertex) in out.vertex.iter_mut().enumerate() {
            let at = index * SIMPLEX_VERTEX_LEN;
            let quad = |offset: usize| -> Quad {
                let mut lane = [0.0f32; 4];
                for (slot, value) in lane.iter_mut().enumerate() {
                    *value = f32_at(bytes, at + offset + slot * 4);
                }
                lane
            };
            vertex.dw = quad(0x00);
            vertex.p = quad(0x10);
            vertex.w = quad(0x20);
            vertex.bary = quad(0x30);
            vertex.index = [
                i32::from_le_bytes(bytes[at + 0x40..at + 0x44].try_into().unwrap()),
                i32::from_le_bytes(bytes[at + 0x44..at + 0x48].try_into().unwrap()),
            ];
            vertex.pad.copy_from_slice(&bytes[at + 0x48..at + 0x50]);
        }
        out.count = i32::from_le_bytes(
            bytes[SIMPLEX_VERTEX_LEN * 4..SIMPLEX_VERTEX_LEN * 4 + 4]
                .try_into()
                .unwrap(),
        );
        out
    }

    /// Write the block back in the client's layout.
    pub fn write_to(&self, bytes: &mut [u8]) {
        for (index, vertex) in self.vertex.iter().enumerate() {
            let at = index * SIMPLEX_VERTEX_LEN;
            for (offset, quad) in [
                (0x00, vertex.dw),
                (0x10, vertex.p),
                (0x20, vertex.w),
                (0x30, vertex.bary),
            ] {
                for (slot, value) in quad.iter().enumerate() {
                    bytes[at + offset + slot * 4..at + offset + slot * 4 + 4]
                        .copy_from_slice(&value.to_le_bytes());
                }
            }
            bytes[at + 0x40..at + 0x44].copy_from_slice(&vertex.index[0].to_le_bytes());
            bytes[at + 0x44..at + 0x48].copy_from_slice(&vertex.index[1].to_le_bytes());
            bytes[at + 0x48..at + 0x50].copy_from_slice(&vertex.pad);
        }
        let at = SIMPLEX_VERTEX_LEN * 4;
        bytes[at..at + 4].copy_from_slice(&self.count.to_le_bytes());
    }
}

/// `(1, 1, 1, 1)` at `0x142fa62f0`: the dividend of every barycentric
/// reciprocal.
const ONE: Quad = [1.0, 1.0, 1.0, 1.0];

#[inline]
fn per_lane_div(a: Quad, b: Quad) -> Quad {
    [a[0] / b[0], a[1] / b[1], a[2] / b[2], a[3] / b[3]]
}

/// `0x142cc8d80`: reduce a 3-simplex to the face, edge or vertex nearest the
/// origin, rewriting the surviving vertices' barycentrics.
///
/// `a`, `b`, `c` are the three `dw` points; `u = b - a`, `v = c - b` and
/// `w = c - a` are the edges the region tests are written against. The five
/// scalars are the signed dot products the tests compare (`nd*` are the
/// negated forms the client keeps in its registers), and `x` is the face term
/// the three plane tests use: `rot3(rot3(w) * u - rot3(u) * w)`, which is
/// `-cross(w, u)` on the low three lanes. Returns false only when the face's
/// denominator is non-positive.
pub fn simplex3(out: &mut Quad, s: &mut Simplex) -> bool {
    let a = s.vertex[0].dw;
    let b = s.vertex[1].dw;
    let c = s.vertex[2].dw;

    let u = sub_lane(b, a);
    let v = sub_lane(c, b);
    let w = sub_lane(c, a);

    let rot_u = rot3(u);
    let ndu_a = negate(dot3(u, a));
    let du_b = dot3(u, b);
    let ndw_a = negate(dot3(w, a));
    let dw_c = dot3(w, c);
    let ndv_b = negate(dot3(v, b));
    let dv_c = dot3(v, c);

    let x = rot3(sub_lane(mul_lane(rot3(w), u), mul_lane(rot_u, w)));
    let face1 = dot3(
        rot3(sub_lane(mul_lane(rot3(c), b), mul_lane(rot3(b), c))),
        x,
    );
    let face2 = dot3(
        rot3(sub_lane(mul_lane(rot3(a), c), mul_lane(rot3(c), a))),
        x,
    );
    let face3 = dot3(
        rot3(sub_lane(mul_lane(rot3(b), a), mul_lane(rot3(a), b))),
        x,
    );

    // Vertex regions.
    if all_lanes(|lane| ndu_a[lane] <= 0.0) && all_lanes(|lane| ndw_a[lane] <= 0.0) {
        s.vertex[0].bary = ONE;
        s.count = 1;
        *out = negate(a);
        return true;
    }
    if all_lanes(|lane| du_b[lane] <= 0.0) && all_lanes(|lane| ndv_b[lane] <= 0.0) {
        s.vertex[1].bary = ONE;
        s.vertex[0] = s.vertex[1];
        s.count = 1;
        *out = negate(b);
        return true;
    }
    if all_lanes(|lane| dw_c[lane] <= 0.0) && all_lanes(|lane| dv_c[lane] <= 0.0) {
        s.vertex[2].bary = ONE;
        s.vertex[0] = s.vertex[2];
        s.count = 1;
        *out = negate(c);
        return true;
    }

    // Edge `a`-`b`.
    if all_lanes(|lane| 0.0 < du_b[lane])
        && all_lanes(|lane| 0.0 < ndu_a[lane])
        && all_lanes(|lane| face3[lane] <= 0.0)
    {
        let sum = add_lane(b, a);
        let inv = per_lane_div(ONE, add_lane(ndu_a, du_b));
        s.count = 2;
        s.vertex[1].bary = mul_lane(inv, ndu_a);
        s.vertex[0].bary = mul_lane(inv, du_b);
        let t = rot3(sub_lane(mul_lane(sum, rot_u), mul_lane(rot3(sum), u)));
        *out = rot3(sub_lane(mul_lane(t, rot_u), mul_lane(rot3(t), u)));
        return true;
    }

    // Edge `a`-`c`: the surviving pair is (slot 0 = `a`, slot 1 = `c`).
    if all_lanes(|lane| 0.0 < dw_c[lane])
        && all_lanes(|lane| 0.0 < ndw_a[lane])
        && all_lanes(|lane| face2[lane] <= 0.0)
    {
        let inv = per_lane_div(ONE, add_lane(ndw_a, dw_c));
        s.count = 2;
        s.vertex[2].bary = mul_lane(inv, ndw_a);
        s.vertex[0].bary = mul_lane(inv, dw_c);
        s.vertex[1].dw = c;
        s.vertex[1].p = s.vertex[2].p;
        s.vertex[1].w = s.vertex[2].w;
        s.vertex[1].bary = s.vertex[2].bary;
        s.vertex[1].index = s.vertex[2].index;
        let sum = add_lane(c, a);
        let rot_w = rot3(w);
        let t = rot3(sub_lane(mul_lane(rot_w, sum), mul_lane(rot3(sum), w)));
        *out = rot3(sub_lane(mul_lane(rot_w, t), mul_lane(rot3(t), w)));
        return true;
    }

    // Edge `b`-`c`: the surviving pair is (slot 0 = `c`, slot 1 = `b`); the
    // weights are written in the client's own order, slot 1 holding `c`'s.
    if all_lanes(|lane| 0.0 < dv_c[lane])
        && all_lanes(|lane| 0.0 < ndv_b[lane])
        && all_lanes(|lane| face1[lane] <= 0.0)
    {
        let inv = per_lane_div(ONE, add_lane(ndv_b, dv_c));
        s.count = 2;
        s.vertex[0].dw = c;
        s.vertex[2].bary = mul_lane(inv, ndv_b);
        s.vertex[1].bary = mul_lane(inv, dv_c);
        s.vertex[0].p = s.vertex[2].p;
        s.vertex[0].w = s.vertex[2].w;
        s.vertex[0].bary = mul_lane(inv, ndv_b);
        s.vertex[0].index = s.vertex[2].index;
        let sum = add_lane(c, b);
        let rot_v = rot3(v);
        let t = rot3(sub_lane(mul_lane(rot_v, sum), mul_lane(rot3(sum), v)));
        *out = rot3(sub_lane(mul_lane(t, rot_v), mul_lane(rot3(t), v)));
        return true;
    }

    // Face `a`-`b`-`c`: the weights are the three plane terms over their own
    // sum, and the direction is `x` or `-x`, whichever faces the origin.
    let denom = add_lane(add_lane(face2, face1), face3);
    if all_lanes(|lane| denom[lane] <= 0.0) {
        return false;
    }
    let inv = per_lane_div(ONE, denom);
    s.count = 3;
    let sum = add_lane(add_lane(b, a), c);
    s.vertex[1].bary = mul_lane(inv, face2);
    s.vertex[0].bary = mul_lane(inv, face1);
    s.vertex[2].bary = mul_lane(inv, face3);
    let face_dot = dot3(sum, x);
    *out = if all_lanes(|lane| 0.0 < negate(face_dot)[lane]) {
        x
    } else {
        negate(x)
    };
    true
}

/// The rotation matrix a quaternion's rows build, in the GJK's own operand
/// order (`0x142cc94f0..0x142cc9655` for the second quaternion and
/// `0x142cc957b..0x142cc9624` for the first).
///
/// The diagonal is `1 - (2q * q)`'s two shuffles subtracted from `(1,1,1,0)`;
/// the off-diagonal terms are `half = shufps(2q, 0xe6) * shufps(q, 0xd0)` and
/// `spin = rot3(2q) * q.w`, from which the three rows are assembled by the
/// leaf's shuffle immediates. This is the same construction
/// [`support_hull_affine`] folds into its composed rotation; the GJK applies it
/// to a shape's own quaternion, so the two are kept apart.
fn gjk_rotation_rows(q: Quad) -> [Quad; 3] {
    let doubled = add_lane(q, q);
    let square = mul_lane(doubled, q);
    let diagonal = sub_lane(
        sub_lane(
            DIAG_BASE,
            and_mask(shufps(square, square, 0xc1), KEEP_LOW_THREE),
        ),
        and_mask(shufps(square, square, 0xda), KEEP_LOW_THREE),
    );
    let half = mul_lane(shufps(doubled, doubled, 0xe6), shufps(q, q, 0xd0));
    let spin_term = mul_lane(rot3(doubled), splat(q[3]));
    let plus = add_lane(spin_term, half);
    let minus = sub_lane(half, spin_term);
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
    [row0, row1, row2]
}

/// `rows * p + t`, with the sums in the client's order
/// (`p.x * row0 + t + p.y * row1 + p.z * row2`).
#[inline]
fn gjk_transform(rows: &[Quad; 3], t: Quad, p: Quad) -> Quad {
    let mut out = mul_lane(splat(p[0]), rows[0]);
    out = add_lane(out, t);
    out = add_lane(out, mul_lane(splat(p[1]), rows[1]));
    add_lane(out, mul_lane(splat(p[2]), rows[2]))
}

/// One shape's half of the descriptor: the radius the caller passes (the GJK
/// floors it at `0.01`), its translation and its rotation quaternion.
#[derive(Clone, Copy, Default)]
pub struct GjkShape {
    pub radius: Quad,
    pub translation: Quad,
    pub rotation: Quad,
}

/// The descriptor the triangle test builds at `0x142cce6e8` and hands to
/// `0x142cc9460`.
///
/// In the client the two point arrays are pointers inside the descriptor; here
/// they are slices whose length is the descriptor's count. The two byte flags
/// are `+0xa0` (from the query block's `+0x72`) and `+0xa1` (the caller writes
/// zero); `direction` is the sweep's `end - start` and `motion` its budget.
#[derive(Clone, Copy)]
pub struct GjkInput<'a> {
    pub a: &'a [Quad],
    pub b: &'a [Quad],
    pub shape_a: GjkShape,
    pub shape_b: GjkShape,
    pub direction: Quad,
    pub motion: Quad,
    pub flag0: u8,
    pub flag1: u8,
}

/// What `0x142cc9460` writes through its first argument on acceptance.
#[derive(Clone, Copy, Default)]
pub struct GjkOutput {
    pub point: Quad,
    pub normal: Quad,
    pub residual: Quad,
    pub iterations: i32,
}

/// `0x142cc9460`: the closest-point GJK the triangle test takes whenever the
/// query block carries support points or an expansion lane above `0.005`.
///
/// The two point sets are expressed relative to the second set's first point
/// after each set's own rotation and translation; the loop then walks the
/// cached simplex toward the origin, stopping when the closest point's squared
/// length falls under `r * r` for `r = radius_a + radius_b - 0.01 + 0.00125`.
/// On a contact it publishes the contact point (the barycentric combination of
/// the first set's points, moved into the second set's frame and out by
/// `radius_a` along the normal), the unit normal, the residual vector and the
/// iteration count. Returning false leaves the caller's output untouched.
///
/// **One branch is open**: the four-vertex reduction `0x142cc7740` is not
/// transcribed, so a simplex that becomes a tetrahedron around the origin
/// reports a miss instead of a penetration contact. That state never arises in
/// the 17-query differential corpus, which is exact; see
/// `docs/formats/static-collision.md`.
pub fn closest_point(input: &GjkInput<'_>, out: &mut GjkOutput) -> bool {
    debug_assert!(!input.a.is_empty() && !input.b.is_empty());
    let rows_a = gjk_rotation_rows(input.shape_a.rotation);
    let rows_b = gjk_rotation_rows(input.shape_b.rotation);
    let rad_a = quad_max(input.shape_a.radius, splat(0.01));
    let rad_b = quad_max(input.shape_b.radius, splat(0.01));
    let t_a = input.shape_a.translation;
    let t_b = input.shape_b.translation;

    // Both arrays are taken relative to the second set's first point.
    let origin = gjk_transform(&rows_b, t_b, input.b[0]);
    let mut points_a = Vec::with_capacity(input.a.len());
    for point in input.a {
        points_a.push(sub_lane(gjk_transform(&rows_a, t_a, *point), origin));
    }
    let mut points_b = Vec::with_capacity(input.b.len());
    points_b.push([0.0; 4]);
    for point in input.b.iter().skip(1) {
        points_b.push(sub_lane(gjk_transform(&rows_b, t_b, *point), origin));
    }

    // The support searches measure from each array's own first point, so the
    // offset is constant and never re-derived (`0x142cc9743`).
    let probe = points_a[0];
    let zero = [0.0f32; 4];

    let mut extend = zero;
    let mut radius_offset = sub_lane(add_lane(rad_b, rad_a), splat(0.01));
    let mut radius = add_lane(splat(0.00125), radius_offset);
    let mut radius_sq = mul_lane(radius, radius);
    let mut best = splat(f32::MAX);
    let mut shrunk = false;
    let mut normal_hint = zero;
    let mut simplex = Simplex::default();
    let mut closest = sub_lane(points_b[0], probe);
    // `XMM3`: the direction the last reduction left behind. The output stage
    // takes the normal from this register, not from the closest point.
    let mut reduced = closest;
    let mut iterations: i32 = 0;

    loop {
        // The convergence test: `|closest|^2 < r^2` on every lane. The client
        // squares `XMM2` here, so from this point on the support searches use
        // `XMM3` - the direction the last reduction left - and not the closest
        // point, which is a parallel vector of a different length.
        let distance_sq = dot3(closest, closest);
        let mut to_output = false;
        if all_lanes(|lane| distance_sq[lane] < radius_sq[lane]) {
            if !all_lanes(|lane| extend[lane] == 0.0) {
                to_output = true;
            } else if input.flag1 == 0 {
                if input.flag0 == 0 {
                    return false;
                }
                to_output = true;
            } else if !all_lanes(|lane| best[lane] <= distance_sq[lane]) {
                // The distance improved on the running best: record it and
                // keep the radius (`0x142cc987d`).
                shrunk = true;
                best = distance_sq;
            } else {
                if !all_lanes(|lane| 0.0001 < distance_sq[lane]) {
                    return false;
                }
                let shortened = sub_lane(per_lane_sqrt(distance_sq), splat(0.005));
                shrunk = false;
                radius_offset = shortened;
                radius = add_lane(shortened, splat(0.00125));
                radius_sq = mul_lane(radius, radius);
            }
        }

        let mut penetration = false;
        if !to_output {
            // The two support searches (`0x142cc9885`): the first maximises
            // along `closest`, the second along its negation, both measured
            // from the arrays' own origins and both requiring a strictly
            // positive projection.
            let mut top_a = zero;
            let mut top_b = zero;
            let mut index_a = 0usize;
            let mut index_b = 0usize;
            for index in 1..input.a.len() {
                let along = dot3(sub_lane(points_a[index], probe), reduced);
                if all_lanes(|lane| top_a[lane] < along[lane]) {
                    top_a = along;
                    index_a = index;
                }
            }
            let backwards = negate(reduced);
            for index in 1..input.b.len() {
                let along = dot3(sub_lane(points_b[index], points_b[0]), backwards);
                if all_lanes(|lane| top_b[lane] < along[lane]) {
                    top_b = along;
                    index_b = index;
                }
            }
            let support_a = points_a[index_a];
            let support_b = points_b[index_b];

            // How far the new difference point lies beyond the radius along
            // the unit direction; at or below it the walk refines the sweep
            // fraction instead of growing the simplex (`0x142cc99e1`).
            // The client normalises `XMM3` - the direction the last reduction
            // left - not the closest point, which is a parallel vector of a
            // different length and rounds differently.
            let unit = mul_lane(splat(1.0f32 / length_sq(reduced)[0].sqrt()), reduced);
            normal_hint = unit;
            let advance = dot3(
                sub_lane(
                    sub_lane(support_a, support_b),
                    mul_lane(extend, input.direction),
                ),
                unit,
            );
            let reach = negate(advance);
            if all_lanes(|lane| radius[lane] < reach[lane]) && !shrunk {
                let slope = negate(dot3(unit, input.direction));
                if all_lanes(|lane| slope[lane] < f32::from_bits(0x3400_0000)) {
                    return false;
                }
                let step = add_lane(per_lane_div(sub_lane(reach, radius_offset), slope), extend);
                extend = step;
                if all_lanes(|lane| input.motion[lane] <= step[lane]) {
                    return false;
                }
            }

            // Append the support point, or re-project the cached ones when the
            // same pair arrives twice (`0x142cc9a50`).
            let shift = mul_lane(extend, input.direction);
            let mut duplicate = false;
            for slot in 0..simplex.count as usize {
                let vertex = &mut simplex.vertex[slot];
                let second = vertex.index[1] as usize;
                vertex.w = add_lane(points_b[second], shift);
                vertex.dw = sub_lane(vertex.w, vertex.p);
                if vertex.index[0] as usize == index_a && second == index_b {
                    duplicate = true;
                }
            }
            if !duplicate {
                let slot = simplex.count as usize;
                let mut vertex = SimplexVertex {
                    p: support_a,
                    w: add_lane(support_b, shift),
                    index: [index_a as i32, index_b as i32],
                    bary: ONE,
                    ..SimplexVertex::default()
                };
                vertex.dw = sub_lane(vertex.w, vertex.p);
                simplex.vertex[slot] = vertex;
                simplex.count += 1;
            }

            match simplex.count {
                1 => reduced = simplex.vertex[0].dw,
                2 => match reduce_segment(&mut simplex) {
                    Some(direction) => reduced = direction,
                    None => return false,
                },
                3 => {
                    let mut direction = zero;
                    if !simplex3(&mut direction, &mut simplex) {
                        return false;
                    }
                    reduced = negate(direction);
                }
                4 => {
                    let mut direction = zero;
                    if !simplex4(&mut direction, &mut simplex) {
                        return false;
                    }
                    reduced = negate(direction);
                }
                _ => closest = zero,
            }

            if simplex.count == 4 {
                if input.flag0 == 0 {
                    return false;
                }
                penetration = true;
            }
            if !penetration {
                // The closest point of the reduced simplex, from its own
                // barycentrics (`0x142cc9d69`).
                closest = simplex_point(&simplex, false, zero);
                iterations += 1;
                out.iterations = iterations;
                if iterations >= 0x14 {
                    to_output = true;
                }
            }
        }

        if !to_output && !penetration {
            continue;
        }

        // The output stage (`0x142cc9e3e`): the contact point, the normal and
        // the residual. The normal is the reduced direction, not the closest
        // point.
        let contact = simplex_point(&simplex, true, points_a[0]);
        let reduced_sq = dot3(reduced, reduced);
        let normal = if reduced_sq[0] > eps() {
            mul_lane(reduced, splat(1.0f32 / reduced_sq[0].sqrt()))
        } else {
            let direction_sq = dot3(input.direction, input.direction);
            if direction_sq[0] > eps() {
                negate(mul_lane(
                    input.direction,
                    splat(1.0f32 / direction_sq[0].sqrt()),
                ))
            } else {
                normal_hint
            }
        };
        out.point = add_lane(add_lane(contact, origin), mul_lane(normal, rad_a));
        out.normal = normal;
        out.iterations = iterations;
        out.residual = extend;
        return true;
    }
}

/// `0x142cc7740`: reduce a 4-simplex to its nearest face, edge or vertex.
///
/// **Open.** The body (`.pdata` `0x142cc7740..0x142cc8d72`, `0x1632` bytes) has
/// the same shape as [`simplex3`] one dimension up: an orientation fix-up that
/// swaps two vertices when the tetrahedron is negatively oriented, four vertex
/// regions, six edge regions, four face regions - each with the same
/// origin-relative dot tests and barycentric reciprocal the three-vertex body
/// uses - and an interior case that spreads `1 / sum(lambda)` over all four
/// vertices. Its three-term and four-term stack layout, its `lambda` slots and
/// its copy order are not transcribed, so this branch returns the miss the
/// triangle test reported before this change. It is reached only when the walk
/// appends a fourth support point that puts the origin inside the tetrahedron,
/// i.e. on penetration; the 17-case differential corpus calls it zero times.
/// `docs/formats/static-collision.md` records it as the one open piece.
pub fn simplex4(_out: &mut Quad, _s: &mut Simplex) -> bool {
    false
}

/// The two-vertex reduction the walk performs inline (`0x142cc9b8b`).
///
/// Returns the reduced simplex's direction - the value the output stage takes
/// the normal from - or `None` for the degenerate rejection at `0x142cc9c7f`.
/// The walk recomputes the closest point from the barycentrics immediately
/// after, so the direction and the closest point are different values.
fn reduce_segment(s: &mut Simplex) -> Option<Quad> {
    let first = s.vertex[0].dw;
    let second = s.vertex[1].dw;
    let edge = sub_lane(second, first);
    let at_first = negate(dot3(edge, first));
    if all_lanes(|lane| at_first[lane] <= 0.0) {
        s.vertex[0].bary = ONE;
        s.count = 1;
        return Some(first);
    }
    let at_second = dot3(edge, second);
    if all_lanes(|lane| at_second[lane] <= 0.0) {
        s.vertex[1].bary = ONE;
        s.vertex[0] = s.vertex[1];
        s.vertex[0].bary = ONE;
        s.count = 1;
        return Some(second);
    }
    let denominator = add_lane(at_second, at_first);
    if all_lanes(|lane| denominator[lane] <= 0.0) {
        return None;
    }
    // `1/denominator`, or zero where the denominator is not above `FLT_MIN`.
    let mut inv = [0.0f32; 4];
    for lane in 0..4 {
        if eps() < denominator[lane].abs() {
            inv[lane] = 1.0 / denominator[lane];
        }
    }
    s.count = 2;
    s.vertex[0].bary = mul_lane(inv, at_second);
    s.vertex[1].bary = mul_lane(inv, at_first);
    // `rot3(rot3(edge) * sum - rot3(sum) * edge)` and then the same form again,
    // negated into the direction slot (`0x142cc9ce3..0x142cc9d1f`).
    let sum = add_lane(second, first);
    let t = rot3(sub_lane(
        mul_lane(rot3(edge), sum),
        mul_lane(rot3(sum), edge),
    ));
    Some(negate(rot3(sub_lane(
        mul_lane(t, rot3(edge)),
        mul_lane(rot3(t), edge),
    ))))
}

/// The barycentric combination of a simplex's `dw` points (`from_p = false`)
/// or its `p` points (`from_p = true`), in the client's slot order.
///
/// `empty` is the value the client's accumulator already holds when the
/// simplex is empty: zero for the closest point (`0x142cc9d86`) and the first
/// relative `A` point for the contact sum, which is what `XMM9` was left with
/// before the walk ever ran (`0x142cc9799`).
fn simplex_point(s: &Simplex, from_p: bool, empty: Quad) -> Quad {
    if s.count <= 0 {
        return empty;
    }
    let mut out = [0.0f32; 4];
    for slot in 0..s.count.clamp(0, 4) as usize {
        let vertex = &s.vertex[slot];
        let point = if from_p { vertex.p } else { vertex.dw };
        out = add_lane(out, mul_lane(vertex.bary, point));
    }
    out
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

    // -----------------------------------------------------------------------
    // The query driver's pruning stage (`0x142cbd510`).
    // -----------------------------------------------------------------------

    const ZERO: Quad = [0.0, 0.0, 0.0, 0.0];

    #[test]
    fn pass_geometry_folds_the_swept_box_against_the_start() {
        // start (0,0,0), end (0,0,-10), motion 0.5 -> reach (0,0,-5).
        let g = pass_geometry(ZERO, [0.0, 0.0, -10.0, 0.0], splat(0.5));
        assert_eq!(g.delta, [0.0, 0.0, -10.0, 0.0]);
        assert_eq!(g.low, [0.0, 0.0, -5.0, 0.0]);
        assert_eq!(g.high, [0.0, 0.0, 0.0, 0.0]);
        assert_eq!(g.epsilon[0], QUERY_EPSILON);
        assert_eq!(g.epsilon[2], 10.0 + QUERY_EPSILON);
    }

    #[test]
    fn pass_geometry_keeps_the_reach_on_nan() {
        // MINPS/MAXPS return the source operand on NaN, so a NaN start yields
        // the reach in both corners, and a NaN reach yields the NaN.
        let g = pass_geometry([f32::NAN, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0], splat(1.0));
        assert!(g.low[0].is_nan());
        let g = pass_geometry([0.0, 0.0, 0.0, 0.0], [f32::NAN, 0.0, 0.0, 0.0], splat(1.0));
        assert!(g.high[0].is_nan());
    }

    #[test]
    fn aabb_accepts_an_overlap_and_rejects_a_gap() {
        let g = pass_geometry(ZERO, [0.0, 0.0, -10.0, 0.0], splat(1.0));
        let min = [-1.0, -1.0, -1.0, 0.0];
        let max = [1.0, 1.0, 1.0, 0.0];
        assert!(!aabb_reject(&g, min, max, ZERO));
        // Shift the box clear of the swept segment on y.
        let far = [10.0, 10.0, 10.0, 0.0];
        assert!(aabb_reject(&g, far, [11.0, 11.0, 11.0, 0.0], ZERO));
    }

    #[test]
    fn aabb_ignores_the_fourth_lane_and_reads_the_sign_bit() {
        let g = pass_geometry(ZERO, [0.0, 0.0, -10.0, 0.0], splat(1.0));
        // A gap on the w lane alone must not reject: the client masks AL & 7.
        let min = [-1.0, -1.0, -1.0, 100.0];
        let max = [1.0, 1.0, 1.0, 200.0];
        assert!(!aabb_reject(&g, min, max, ZERO));
        // A `-0.0` difference rejects even though it compares equal to zero:
        // the client's ORPS/MOVMSKPS pair reads the sign bit rather than
        // comparing values. `-0.0 + -0.0` is the only way the expansion stage
        // produces a negative zero, and `-0.0 - +0.0` keeps it.
        let touch = pass_geometry(ZERO, ZERO, splat(0.0));
        let min = [-0.0, -1.0, -1.0, 0.0];
        let max = [-0.0, 1.0, 1.0, 0.0];
        let neg_zero_expansion = [-0.0, 0.0, 0.0, 0.0];
        assert!(aabb_reject(&touch, min, max, neg_zero_expansion));
        // The same entry with a positive-zero expansion is accepted, which is
        // what proves the test is the sign bit and not a comparison.
        assert!(!aabb_reject(&touch, min, max, ZERO));
    }

    #[test]
    fn moving_box_accepts_a_box_the_sweep_passes_through() {
        // A sweep straight down -z through the middle of a unit box at the
        // origin: every axis overlaps, so no separating axis exists.
        let g = pass_geometry([0.0, 0.0, 5.0, 0.0], [0.0, 0.0, -5.0, 0.0], splat(1.0));
        let min = [-1.0, -1.0, -1.0, 0.0];
        let max = [1.0, 1.0, 1.0, 0.0];
        assert!(!moving_box_reject(&g, min, max, ZERO));
    }

    #[test]
    fn moving_box_rejects_a_box_the_sweep_misses() {
        let g = pass_geometry([0.0, 0.0, 5.0, 0.0], [0.0, 0.0, -5.0, 0.0], splat(1.0));
        // A thin box far out on x: the sweep never comes within reach.
        let min = [20.0, -1.0, -1.0, 0.0];
        let max = [21.0, 1.0, 1.0, 0.0];
        assert!(moving_box_reject(&g, min, max, ZERO));
    }

    #[test]
    fn moving_box_matches_the_client_lane_permutation() {
        // The client permutes both products by SHUFPS 0xc9 and again by 0xc9
        // after the subtract, so the three real lanes come out in (z, x, y)
        // order of the plain cross product. Pin the arithmetic directly.
        let g = pass_geometry([1.0, 2.0, 3.0, 0.0], [1.0, 2.0, -7.0, 0.0], splat(1.0));
        let min = [-1.0, -2.0, 0.0, 0.0];
        let max = [1.0, 2.0, 4.0, 0.0];
        let hi = add_lane(max, ZERO);
        let lo = sub_lane(min, ZERO);
        let mid = mul_lane(add_lane(hi, lo), splat(QUERY_HALF));
        let half = mul_lane(sub_lane(hi, lo), splat(QUERY_HALF));
        let b = sub_lane(g.start, mid);
        let cross = rot3(sub_lane(
            mul_lane(rot3(g.delta), b),
            mul_lane(rot3(b), g.delta),
        ));
        // delta = (0, 0, -10, 0), b = (1, 2, 1, 0).
        // rot3(delta)*b       = (0, -10, 0, 0) * (1, 2, 1, 0) = (0, -20, 0, 0)
        // rot3(b)*delta       = (2, 1, 1, 0) * (0, 0, -10, 0) = (0, 0, -10, 0)
        // difference          = (0, -20, 10, 0)
        // rot3(difference)    = (-20, 10, 0, 0)
        assert_eq!(cross[0], -20.0);
        assert_eq!(cross[1], 10.0);
        assert_eq!(cross[2], 0.0);
        let sym = rot3(add_lane(
            mul_lane(rot3(g.epsilon), half),
            mul_lane(rot3(half), g.epsilon),
        ));
        // epsilon = (eps, eps, 10+eps, eps), half = (1, 2, 2, 0)
        // rot3(eps)*half = (eps, 10+eps, eps, eps) * (1, 2, 2, 0)
        //                = (eps, 20+2eps, 2eps, 0)
        // rot3(half)*eps = (2, 2, 1, 0) * (eps, eps, 10+eps, eps)
        //                = (2eps, 2eps, 10+eps, 0)
        // sum            = (3eps, 20+4eps, 10+3eps, 0)
        // rot3(sum)      = (20+4eps, 10+3eps, 3eps, 0)
        assert_eq!(sym[0], 20.0 + 4.0 * QUERY_EPSILON);
        assert_eq!(sym[1], 10.0 + 3.0 * QUERY_EPSILON);
        assert_eq!(sym[2], 3.0 * QUERY_EPSILON);
    }

    #[test]
    fn moving_box_uses_the_separation_constant_strictly() {
        // A difference exactly at 0.005 must not reject: the client's test is
        // `CMPLTPS 0.005, diff`, not `<=`.
        assert_eq!(QUERY_SEPARATION, f32::from_bits(0x3ba3_d70a));
        let mut geometry = PassGeometry {
            start: ZERO,
            delta: [0.0, 0.0, 1.0, 0.0],
            epsilon: ZERO,
            low: ZERO,
            high: ZERO,
        };
        // Choose an entry whose cross term is exactly 0.005 on x and zero
        // elsewhere, with a zero symmetric term: half = 0 and epsilon = 0
        // make `sym` zero, so `cross` alone decides.
        geometry.delta = [0.0, 0.0, 0.005, 0.0];
        let min = [0.0, 0.0, -1.0, 0.0];
        let max = [0.0, 0.0, 1.0, 0.0];
        // b = start - mid = (0,0,0) - (0,0,0) = 0, so cross = 0: accepted.
        assert!(!moving_box_reject(&geometry, min, max, ZERO));
        geometry.start = [0.0, 0.005, 0.0, 0.0];
        // b = (0, 0.005, 0). rot3(delta) = (0, 0.005, 0), rot3(b) = (0.005, 0, 0).
        // rot3(delta)*b = (0, 0.005, 0) * (0, 0.005, 0) = (0, 2.5e-5, 0)
        // rot3(b)*delta = (0.005, 0, 0) * (0, 0, 0.005) = (0, 0, 0)
        // difference = (0, 2.5e-5, 0) -> cross = (2.5e-5, 0, 0)
        // 0.005 < 2.5e-5 is false: accepted.
        assert!(!moving_box_reject(&geometry, min, max, ZERO));
    }

    // -----------------------------------------------------------------------
    // The driver's walk (`0x142cbd510`).
    // -----------------------------------------------------------------------

    /// A synthetic shape-entry array: a two-child root and two leaves.
    ///
    /// Entry `0` is the root (`+0x2c = 1`, `+0x30 = 2`); entries `1` and `2`
    /// are leaves whose boxes straddle the sweep unless the caller moves the
    /// second one out of reach.
    struct TwoLeafWorld {
        entries: Vec<Vec<u8>>,
        /// The visit order, as `(pass, id)`.
        seen: Vec<(usize, i32)>,
        /// The answers `narrowphase` returns, as `(leaf id, answer)`.
        answers: Vec<(i32, i32)>,
        /// The hit quad a `2` answer writes.
        hit: Quad,
    }

    fn entry_bytes(min: Quad, max: Quad, id: i32, child: i32, second: i32) -> Vec<u8> {
        // `SHL RBX,0x6` at `0x142cbd6bb`: an entry is `0x40` bytes.
        let mut bytes = vec![0u8; 0x40];
        for (lane, value) in min.iter().enumerate() {
            bytes[lane * 4..lane * 4 + 4].copy_from_slice(&value.to_le_bytes());
            bytes[0x10 + lane * 4..0x10 + lane * 4 + 4].copy_from_slice(&max[lane].to_le_bytes());
        }
        bytes[0x20..0x24].copy_from_slice(&id.to_le_bytes());
        bytes[0x2c..0x30].copy_from_slice(&child.to_le_bytes());
        bytes[0x30..0x34].copy_from_slice(&second.to_le_bytes());
        bytes
    }

    impl TwoLeafWorld {
        fn new(hit: Quad, answers: Vec<(i32, i32)>) -> Self {
            Self {
                entries: vec![
                    entry_bytes([-50.0, -50.0, -50.0, 0.0], [50.0, 50.0, 50.0, 0.0], 0, 1, 2),
                    entry_bytes([-1.0, -1.0, -1.0, 0.0], [1.0, 1.0, 1.0, 0.0], 7, -1, 0),
                    entry_bytes([-1.0, -1.0, -1.0, 0.0], [1.0, 1.0, 1.0, 0.0], 9, -1, 0),
                ],
                seen: Vec::new(),
                answers,
                hit,
            }
        }

        /// Move the second leaf out of the sweep's reach.
        fn push_second_leaf_far(&mut self) {
            let far = entry_bytes(
                [100.0, 100.0, 100.0, 0.0],
                [101.0, 101.0, 101.0, 0.0],
                9,
                -1,
                0,
            );
            self.entries[2] = far;
        }
    }

    impl QueryWorld for TwoLeafWorld {
        fn entry(&self, _pass: usize, index: i32) -> Option<&[u8]> {
            usize::try_from(index)
                .ok()
                .and_then(|i| self.entries.get(i))
                .map(|v| v.as_slice())
        }

        fn narrowphase(&mut self, pass: usize, id: i32, hit: &mut Quad) -> i32 {
            self.seen.push((pass, id));
            *hit = self.hit;
            self.answers
                .iter()
                .find(|(leaf, _)| *leaf == id)
                .map_or(0, |(_, answer)| *answer)
        }
    }

    fn driver_block() -> Vec<u8> {
        let mut bytes = vec![0u8; 0x80];
        let put = |bytes: &mut Vec<u8>, offset: usize, quad: Quad| {
            for (lane, value) in quad.iter().enumerate() {
                bytes[offset + lane * 4..offset + lane * 4 + 4]
                    .copy_from_slice(&value.to_le_bytes());
            }
        };
        put(&mut bytes, 0x00, [0.0, 0.0, 5.0, 0.0]);
        put(&mut bytes, 0x10, [0.0, 0.0, -5.0, 0.0]);
        put(&mut bytes, 0x20, [1.0, 1.0, 1.0, 1.0]);
        put(&mut bytes, 0x40, [0.0, 0.0, 0.0, 1.0]);
        bytes
    }

    #[test]
    fn driver_visits_the_second_child_first() {
        let mut world = TwoLeafWorld::new([0.5, 0.5, 0.5, 0.5], vec![(7, 2), (9, 0)]);
        let hits = query_driver([0, -1, -1, -1], &driver_block(), &[], 0b0001, &mut world);
        // Pushed `+0x2c` then `+0x30`, popped from the end: leaf 9 is visited
        // before leaf 7.
        assert_eq!(world.seen, vec![(0, 9), (0, 7)]);
        // One `2` answer folds 0.5 into the returned motion.
        assert_eq!(hits, [0.5, 0.5, 0.5, 0.5]);
    }

    #[test]
    fn driver_prunes_a_leaf_the_swept_box_cannot_reach() {
        let mut world = TwoLeafWorld::new([0.5, 0.5, 0.5, 0.5], vec![(7, 2), (9, 2)]);
        world.push_second_leaf_far();
        let hits = query_driver([0, -1, -1, -1], &driver_block(), &[], 0b0001, &mut world);
        // Leaf 9 is popped first and rejected before the leaf test, so the
        // narrowphase never sees it.
        assert_eq!(world.seen, vec![(0, 7)]);
        assert_eq!(hits, [0.5, 0.5, 0.5, 0.5]);
    }

    #[test]
    fn driver_skips_passes_whose_flag_bit_is_clear() {
        let mut world = TwoLeafWorld::new([0.5, 0.5, 0.5, 0.5], vec![(7, 2), (9, 0)]);
        // Only bit 2 set: pass 2 runs with its own root.
        let hits = query_driver([-1, -1, 0, -1], &driver_block(), &[], 0b0100, &mut world);
        assert_eq!(world.seen, vec![(2, 9), (2, 7)]);
        assert_eq!(hits, [0.5, 0.5, 0.5, 0.5]);
        // No flag bits: nothing runs.
        world.seen.clear();
        let hits = query_driver([0, 0, 0, 0], &driver_block(), &[], 0, &mut world);
        assert!(world.seen.is_empty());
        // No pass ran, so the motion comes back untouched.
        assert_eq!(hits, [1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn driver_end_the_sweep_answer_stops_the_walk() {
        let mut world = TwoLeafWorld::new([0.5, 0.5, 0.5, 0.5], vec![(7, 1), (9, 2)]);
        let hits = query_driver([0, -1, -1, -1], &driver_block(), &[], 0b0001, &mut world);
        // Leaf 9 answers `2` and leaf 7 answers `1`, which ends the pass.
        assert_eq!(world.seen, vec![(0, 9), (0, 7)]);
        // The `1` answer ends the pass but the earlier `2` still folded.
        assert_eq!(hits, [0.5, 0.5, 0.5, 0.5]);
    }

    #[test]
    fn driver_folds_every_hit_into_the_running_motion() {
        // Both leaves answer `2`. Leaf 9 is visited first with 0.9, then leaf
        // 7 with 0.1: the fold takes the per-lane minimum, so the second hit
        // improves every lane and the swept box re-seats on it.
        struct Ordered {
            inner: TwoLeafWorld,
            hits: Vec<Quad>,
        }
        impl QueryWorld for Ordered {
            fn entry(&self, pass: usize, index: i32) -> Option<&[u8]> {
                self.inner.entry(pass, index)
            }
            fn narrowphase(&mut self, pass: usize, id: i32, hit: &mut Quad) -> i32 {
                self.inner.seen.push((pass, id));
                let next = self.hits.pop().unwrap_or([0.0; 4]);
                *hit = next;
                2
            }
        }
        let mut world = Ordered {
            inner: TwoLeafWorld::new([0.0; 4], vec![]),
            // Popped in reverse: leaf 9 gets 0.9, leaf 7 gets 0.1.
            hits: vec![[0.1, 0.1, 0.1, 0.1], [0.9, 0.9, 0.9, 0.9]],
        };
        let hits = query_driver([0, -1, -1, -1], &driver_block(), &[], 0b0001, &mut world);
        // 0.9 then 0.1: the per-lane minimum is the second hit.
        assert_eq!(hits, [0.1, 0.1, 0.1, 0.1]);
        assert_eq!(world.inner.seen, vec![(0, 9), (0, 7)]);
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
        let mut recentred = Vec::new();
        let (centre, shifted_end, step) = support_hull(
            &points,
            2,
            q,
            b,
            v,
            [1.0; 4],
            start,
            end,
            expansion,
            &mut recentred,
        );
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
    /// The contact the client's closest-point solver produces on the same
    /// query, pinned bit for bit: the second quad's triangle, its two record
    /// ids, the contact at the point cloud's centre pushed out to the capsule
    /// surface and the fraction the solver's residual leaves.
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
        let hit = sweep.hit.expect("the closest-point solver accepts here");
        assert_eq!(hit.triangle, 1);
        assert_eq!(hit.ids, [0x12, 0x22]);
        assert_eq!(
            bits_of(hit.point),
            [0x3f40_0000, 0x3f66_6666, 0x3c23_d70a, 0x0000_0000]
        );
        assert_eq!(
            bits_of(hit.normal),
            [0x0000_0000, 0x0000_0000, 0x3f80_0000, 0x0000_0000]
        );
        assert_eq!(bits_of(hit.fraction), [0x3ed7_0a3d; 4]);
    }

    /// The pinned translated contact: the same support-point query through a
    /// shape transform lifted by three units reproduces the client's hit
    /// exactly - the second quad's triangle, ids `0x12`/`0x22`, the point
    /// `(0.75, 0.9, 3.01)` and the fraction `0x3df5c290`.
    #[test]
    fn the_pinned_translated_support_point_contact_is_reproduced() {
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
        let sweep = sweep_world(&world, &block, &identity_xform([0.0, 0.0, 3.0]));
        let hit = sweep.hit.expect("the transparent shift keeps the contact");
        assert_eq!(hit.triangle, 1);
        assert_eq!(hit.ids, [0x12, 0x22]);
        assert_eq!(
            bits_of(hit.point),
            [0x3f40_0000, 0x3f66_6666, 0x4040_a3d7, 0x0000_0000]
        );
        assert_eq!(
            bits_of(hit.normal),
            [0x0000_0000, 0x0000_0000, 0x3f80_0000, 0x0000_0000]
        );
        assert_eq!(bits_of(hit.fraction), [0x3df5_c290; 4]);
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

    /// The composed branch multiplies `origin - pivot` by the zero quad, so an
    /// infinite origin reaches an *invalid* operation. x86 answers that with
    /// the QNaN floating-point indefinite, `0xffc00000`; the verify harness's
    /// emulator answers `0x7fc00000` instead (softfloat's default NaN), so this
    /// lane cannot be compared there and the hardware value is pinned here.
    #[test]
    #[cfg(target_arch = "x86_64")]
    fn composed_pose_infinite_origin_is_the_qnan_indefinite() {
        let mut pose = vec![0u8; 0x1c0];
        let put = |bytes: &mut Vec<u8>, offset: usize, quad: Quad| {
            for (lane, value) in quad.iter().enumerate() {
                bytes[offset + lane * 4..offset + lane * 4 + 4]
                    .copy_from_slice(&value.to_le_bytes());
            }
        };
        put(&mut pose, 0x50, [1.0, f32::INFINITY, 3.0, 4.0]);
        put(&mut pose, 0x60, [0.5; 4]);
        put(&mut pose, 0x70, [0.25; 4]);
        put(&mut pose, 0x80, [0.125; 4]);
        put(&mut pose, 0x90, [1.0, 0.0, 0.0, 0.0]);
        let frame = compose_pose(&pose);
        assert_eq!(
            frame[0][1].to_bits(),
            0xffc0_0000,
            "infinite origin through `* 0.0` must give the QNaN indefinite"
        );
    }
}

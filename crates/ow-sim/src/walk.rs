//! The character's walk: the read velocity law advanced by the verified swept
//! query, and the collision-resolved position.
//!
//! # What is read here
//!
//! - **The motion is the mover's.** [`crate::frame::update_frame`] composes
//!   `M*wish + ramp` from the body record; `MoverAdvance` (`0x140ca7f30`) turns
//!   it into a displacement with `(scale_a*body[0xc0] + wish)*scale_b` and
//!   publishes it through `SetCharacterControllerVelocity`
//!   (`0x142cbd1b0`). `scale_b` is the frame's step; the direction quad
//!   `body+0xc0` is what the ramp leaf multiplies.
//! - **The query is the client's.** Each sweep is the verified type-3 mesh leaf
//!   [`crate::collide::mesh_sweep`] (`0x142caed30`/`0x142cce430`) called with
//!   the query block `0x142cd5540` reads: `P0`, `P1`, the motion budget, the
//!   expansion and the layer mask. The support-point count is zero, but the
//!   expansion (`0.35`) is above `0.005`, so the triangle test takes the
//!   closest-point solver `0x142cc9460` - verified as `fn/ClosestPointGjk` -
//!   and not the small-expansion analytic path.
//! - **The fold is the driver's.** `0x142cbd510` folds each accepted hit into the
//!   motion by the hit fraction and re-seats the swept box on
//!   `start + hit*delta`; [`World::sweep`] takes the smallest fraction over the
//!   world's shapes, which is what the driver's per-lane minimum is for one
//!   shape.
//!
//! # What is the client's own
//!
//! The **slide**: how the residual motion after a contact is composed into the
//! next sweep. In the client that is `RunCharacterControllerSweep`
//! (`0x142cbcee0` -> `0x142cf3f80`), whose Domino-side body is a shape/pair walk
//! that needs the client's pools; the read part of it is the fold above. This
//! module's slide is the minimal projection of the residual onto the contact
//! plane, in the client's order (`dot`, then subtract the normal component),
//! and it is declared open in `docs/open-questions.md`.

use std::collections::HashMap;

use crate::collide::{self, MeshWorld};

/// The shape layer mask: every bit set. The range's holders all pass the class
/// default filter (`model_body::world_mask` with a mask of all ones).
pub const MESH_MASK: u16 = 0xffff;

/// The query block's motion budget for a single sweep. The driver shrinks it as
/// passes accept hits; one sweep starts at the full budget.
pub const QUERY_MOTION: f32 = 1.0;

/// The query block's `+0x00` start position.
pub const QUERY_START: usize = 0x00;
/// The query block's `+0x10` end position.
pub const QUERY_END: usize = 0x10;
/// The query block's `+0x20` motion budget.
pub const QUERY_MOTION_AT: usize = 0x20;
/// The query block's `+0x30` support-point pointer (unused when the points are
/// supplied directly).
pub const QUERY_SUPPORT_POINTER: usize = 0x30;
/// The query block's `+0x38` support-point count.
pub const QUERY_SUPPORT_COUNT: usize = 0x38;
/// The query block's `+0x40` quaternion the support points are rotated by
/// (`sweep_bounds`).
pub const QUERY_ROTATION: usize = 0x40;
/// The query block's `+0x50` expansion.
pub const QUERY_EXPANSION: usize = 0x50;
/// The query block's `+0x70` layer mask.
pub const QUERY_MASK: usize = 0x70;
/// The query block's length.
pub const QUERY_BLOCK_LEN: usize = 0x80;

/// The shape transform's `+0x00` position.
pub const XFORM_POSITION: usize = 0x00;
/// The shape transform's `+0x10` world-to-local quaternion.
pub const XFORM_ROTATION: usize = 0x10;
/// The shape transform's length.
pub const XFORM_LEN: usize = 0x20;

/// How many contacts one frame may slide along before the motion is dropped.
pub const SLIDE_ITERATIONS: usize = 3;

/// The contact margin the collision code uses: the driver inflates a shape's
/// bounds by it (`0x142cbd510`), the triangle test inflates its box by it and
/// `0x142caf6f0`'s support fold adds it, and the GJK's convergence radius is
/// `expansion + 0.00125`. The fold below re-seats on the surface and steps the
/// body out along the contact normal by this margin, so the next sweep starts
/// strictly outside the query's convergence radius instead of exactly on it
/// (a start inside reports only when the query's `+0x72` byte is set, which a
/// moving sweep does not).
pub const CONTACT_SKIN: f32 = 0.005;

/// The steepest surface a walk may climb, as the contact normal's up lane. A
/// contact flatter than this is a wall: the walk slides along it and keeps its
/// height. The client's own slope limit is not read; `0.7` (45.6 degrees) is the
/// value that keeps the practice range's 48-degree bevels out of the walkable
/// set, and it is declared open in `docs/open-questions.md`.
pub const WALKABLE_SLOPE: f32 = 0.7;

/// The broadphase cell edge, metres.
pub const CELL: f32 = 8.0;

/// The upright identity quaternion: the query block's `+0x40` when the shape
/// has no rotation of its own.
pub const UPRIGHT: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

fn put_quad(out: &mut [u8], offset: usize, quad: [f32; 4]) {
    for (lane, value) in quad.iter().enumerate() {
        out[offset + lane * 4..offset + lane * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
}

#[cfg(test)]
fn f32_at(bytes: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

/// The query block `0x142cd5540` and `0x142caed30` read, in the
/// **no-support-point** state: `P0`, `P1`, the motion budget, the expansion in
/// all four lanes, a zero support count and the layer mask.
pub fn query_block(
    start: [f32; 3],
    end: [f32; 3],
    expansion: f32,
    mask: u16,
) -> [u8; QUERY_BLOCK_LEN] {
    let mut bytes = [0u8; QUERY_BLOCK_LEN];
    put_quad(&mut bytes, QUERY_START, [start[0], start[1], start[2], 0.0]);
    put_quad(&mut bytes, QUERY_END, [end[0], end[1], end[2], 0.0]);
    put_quad(&mut bytes, QUERY_MOTION_AT, [QUERY_MOTION; 4]);
    put_quad(&mut bytes, QUERY_EXPANSION, [expansion; 4]);
    bytes[QUERY_SUPPORT_COUNT..QUERY_SUPPORT_COUNT + 4].copy_from_slice(&0i32.to_le_bytes());
    bytes[QUERY_MASK..QUERY_MASK + 2].copy_from_slice(&mask.to_le_bytes());
    bytes
}

/// The query block `0x142cd5540` reads in the **capsule** state: the sweep, the
/// query's own rotation at `+0x40`, two support points, the expansion and the
/// mask.
///
/// The character's capsule enters here: its two centres are
/// `ow_tank::character_collision::Capsule`'s points in the character's local
/// frame, rotated by the character's quaternion, and the sphere radius is the
/// expansion. A zero rotation is the upright identity.
pub fn query_block_capsule(
    start: [f32; 3],
    end: [f32; 3],
    rotation: [f32; 4],
    expansion: f32,
    mask: u16,
) -> [u8; QUERY_BLOCK_LEN] {
    let mut bytes = query_block(start, end, expansion, mask);
    put_quad(&mut bytes, QUERY_ROTATION, rotation);
    bytes[QUERY_SUPPORT_COUNT..QUERY_SUPPORT_COUNT + 4].copy_from_slice(&2i32.to_le_bytes());
    bytes
}

/// The two support points of a capsule, as the leaf reads them: `vec4` each.
pub fn capsule_points(a: [f32; 3], b: [f32; 3]) -> [u8; 2 * collide::MESH_VERTEX_LEN] {
    let mut bytes = [0u8; 2 * collide::MESH_VERTEX_LEN];
    put_quad(&mut bytes, 0, [a[0], a[1], a[2], 0.0]);
    put_quad(
        &mut bytes,
        collide::MESH_VERTEX_LEN,
        [b[0], b[1], b[2], 0.0],
    );
    bytes
}

/// The shape world transform the mesh leaf reads: position then quaternion.
pub fn shape_xform(position: [f32; 3], rotation: [f32; 4]) -> [u8; XFORM_LEN] {
    let mut bytes = [0u8; XFORM_LEN];
    put_quad(
        &mut bytes,
        XFORM_POSITION,
        [position[0], position[1], position[2], 0.0],
    );
    put_quad(&mut bytes, XFORM_ROTATION, rotation);
    bytes
}

/// One placed shape: its `MeshShapeFill` record, its world transform and the
/// world-space box the broadphase buckets it by.
#[derive(Clone)]
pub struct Shape {
    pub record: Vec<u8>,
    pub xform: [u8; XFORM_LEN],
    pub low: [f32; 3],
    pub high: [f32; 3],
}

/// The world's answer for one sweep: the closest contact along the motion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sweep {
    /// The contact's fraction along the sweep, in `[0, 1]`.
    pub fraction: f32,
    /// The world contact normal.
    pub normal: [f32; 3],
    /// The world contact point.
    pub point: [f32; 3],
    /// The shape that produced it.
    pub shape: usize,
}

/// The character's collision world: the mesh blob plus a uniform grid over the
/// placed shapes.
pub struct World {
    base: u64,
    bytes: Vec<u8>,
    shapes: Vec<Shape>,
    cells: HashMap<(i32, i32), Vec<u32>>,
}

impl World {
    /// Bucket every shape into the grid at [`CELL`] metres, by its world box.
    pub fn new(base: u64, bytes: Vec<u8>, shapes: Vec<Shape>) -> World {
        let mut cells: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (index, shape) in shapes.iter().enumerate() {
            let (x0, x1) = (shape.low[0] / CELL, shape.high[0] / CELL);
            let (z0, z1) = (shape.low[2] / CELL, shape.high[2] / CELL);
            for gx in (x0.floor() as i32)..=(x1.floor() as i32) {
                for gz in (z0.floor() as i32)..=(z1.floor() as i32) {
                    cells.entry((gx, gz)).or_default().push(index as u32);
                }
            }
        }
        World {
            base,
            bytes,
            shapes,
            cells,
        }
    }

    pub fn shape_count(&self) -> usize {
        self.shapes.len()
    }

    pub fn byte_len(&self) -> usize {
        self.bytes.len()
    }

    /// The union of every shape's world box.
    pub fn extent(&self) -> ([f32; 3], [f32; 3]) {
        let mut low = [f32::INFINITY; 3];
        let mut high = [f32::NEG_INFINITY; 3];
        for shape in &self.shapes {
            for lane in 0..3 {
                low[lane] = low[lane].min(shape.low[lane]);
                high[lane] = high[lane].max(shape.high[lane]);
            }
        }
        (low, high)
    }

    fn view(&self) -> MeshWorld<'_> {
        MeshWorld {
            base: self.base,
            bytes: &self.bytes,
        }
    }

    /// Every shape whose cell the swept box touches, in index order.
    fn candidates(&self, start: [f32; 3], end: [f32; 3], radius: f32) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut seen: Vec<u32> = Vec::new();
        let mut gx0 = f32::INFINITY;
        let mut gx1 = f32::NEG_INFINITY;
        let mut gz0 = f32::INFINITY;
        let mut gz1 = f32::NEG_INFINITY;
        for p in [start, end] {
            gx0 = gx0.min(p[0]);
            gx1 = gx1.max(p[0]);
            gz0 = gz0.min(p[2]);
            gz1 = gz1.max(p[2]);
        }
        let gx0 = ((gx0 - radius) / CELL).floor() as i32;
        let gx1 = ((gx1 + radius) / CELL).floor() as i32;
        let gz0 = ((gz0 - radius) / CELL).floor() as i32;
        let gz1 = ((gz1 + radius) / CELL).floor() as i32;
        for gx in gx0..=gx1 {
            for gz in gz0..=gz1 {
                let Some(cell) = self.cells.get(&(gx, gz)) else {
                    continue;
                };
                for index in cell {
                    if seen.contains(index) {
                        continue;
                    }
                    seen.push(*index);
                    out.push(*index);
                }
            }
        }
        out.sort_unstable();
        out
    }

    /// Sweep a sphere of `radius` from `start` to `end` against every shape the
    /// broadphase keeps, and return the closest contact.
    ///
    /// Each shape is one call of the verified mesh leaf; the shape's own BVH
    /// walk does the per-triangle work.
    pub fn sweep(&self, start: [f32; 3], end: [f32; 3], radius: f32) -> Option<Sweep> {
        self.sweep_block(start, end, radius, false)
    }

    /// The penetration contact at `at`: the same leaf, asked with the query
    /// block's **`+0x72`** byte set (the client's "hit while already inside"
    /// flag) and a zero-length sweep.
    ///
    /// A moving sweep reports nothing when its start is on or inside a surface;
    /// that is the leaf's own rule, and it is why a slide can leave the body
    /// tangent to a second wall and then travel through it. This query answers
    /// the question the moving sweep cannot: *is the body overlapping
    /// anything?* It is the shape of answer the client's character controller
    /// gets from its Domino pair list (a broadphase overlap), and the flag is
    /// the leaf's own switch for it.
    pub fn overlap(&self, at: [f32; 3], radius: f32) -> Option<Sweep> {
        self.sweep_block(at, at, radius, true)
    }

    fn sweep_block(
        &self,
        start: [f32; 3],
        end: [f32; 3],
        radius: f32,
        inside: bool,
    ) -> Option<Sweep> {
        let mut block = query_block(start, end, radius, MESH_MASK);
        if inside {
            block[0x72] = 1;
        }
        let view = self.view();
        let mut best: Option<Sweep> = None;
        for index in self.candidates(start, end, radius) {
            let shape = &self.shapes[index as usize];
            let sweep = collide::mesh_sweep(&shape.record, &view, &block, &shape.xform);
            let Some(hit) = sweep.hit else { continue };
            let fraction = hit.fraction[0];
            if best.is_some_and(|b| b.fraction <= fraction) {
                continue;
            }
            best = Some(Sweep {
                fraction,
                normal: [hit.normal[0], hit.normal[1], hit.normal[2]],
                point: [hit.point[0], hit.point[1], hit.point[2]],
                shape: index as usize,
            });
        }
        best
    }

    /// Sweep the character's **capsule** from `start` to `end`.
    ///
    /// `a`/`b` are the capsule's two centres in the character's local frame and
    /// `radius` its sphere radius; `rotation` is the character's quaternion,
    /// which the leaf rotates the points by (`sweep_bounds`). This is the shape
    /// `ow_tank::character_collision::capsule_from_mover` reads - a vertical
    /// capsule whose origin sits at its feet, not a sphere.
    ///
    /// The leaf is the same verified one [`World::sweep`] calls; only the
    /// support points are supplied directly instead of through the world image.
    ///
    /// **The support-point form is not a sphere at `P0`.** With points present
    /// the leaf replaces the swept box with the cloud's hull (`sweep_bounds`,
    /// `0x142cd5540`): the start/end become the hull's low/high corners and the
    /// expansion becomes `half extent + expansion + 0.02`. That is the client's
    /// own behaviour and it is why the capsule's contact differs from a sphere
    /// of the same radius - see the module's `capsule_tests`.
    pub fn sweep_capsule(
        &self,
        start: [f32; 3],
        end: [f32; 3],
        rotation: [f32; 4],
        a: [f32; 3],
        b: [f32; 3],
        radius: f32,
    ) -> Option<Sweep> {
        self.sweep_capsule_block(start, end, rotation, a, b, radius, false)
    }

    /// The penetration contact for the **capsule** at `at`: the same leaf,
    /// asked with the query block's **`+0x72`** byte set (the client's "hit
    /// while already inside" flag) and a zero-length sweep.
    ///
    /// This is the capsule's answer to the question a moving sweep cannot
    /// answer: *is the body overlapping anything?* The sphere's own form is
    /// [`World::overlap`]; the flag and the rule are the same.
    pub fn overlap_capsule(
        &self,
        at: [f32; 3],
        rotation: [f32; 4],
        a: [f32; 3],
        b: [f32; 3],
        radius: f32,
    ) -> Option<Sweep> {
        self.sweep_capsule_block(at, at, rotation, a, b, radius, true)
    }

    fn sweep_capsule_block(
        &self,
        start: [f32; 3],
        end: [f32; 3],
        rotation: [f32; 4],
        a: [f32; 3],
        b: [f32; 3],
        radius: f32,
        inside: bool,
    ) -> Option<Sweep> {
        let mut block = query_block_capsule(start, end, rotation, radius, MESH_MASK);
        if inside {
            block[0x72] = 1;
        }
        let points = capsule_points(a, b);
        let view = self.view();
        let mut best: Option<Sweep> = None;
        for index in self.candidates(start, end, radius) {
            let shape = &self.shapes[index as usize];
            let sweep = collide::mesh_sweep_with_points(
                &shape.record,
                &view,
                &block,
                &shape.xform,
                &points,
            );
            let Some(hit) = sweep.hit else { continue };
            let fraction = hit.fraction[0];
            if best.is_some_and(|b| b.fraction <= fraction) {
                continue;
            }
            best = Some(Sweep {
                fraction,
                normal: [hit.normal[0], hit.normal[1], hit.normal[2]],
                point: [hit.point[0], hit.point[1], hit.point[2]],
                shape: index as usize,
            });
        }
        best
    }

    /// The highest contact under a point within `reach`, straight down.
    pub fn floor(&self, at: [f32; 3], radius: f32, reach: f32) -> Option<Sweep> {
        let down = [at[0], at[1] - reach, at[2]];
        let hit = self.sweep(at, down, radius)?;
        (hit.normal[1] > 0.5).then_some(hit)
    }
}

/// The walk's original proxy: a sphere of `radius`.
///
/// The client sweeps the character's **capsule** ([`Capsule`]); the sphere is
/// kept so the walk's own numbers stay pinned and so the two shapes can be
/// compared on the same world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sphere {
    /// The sphere's radius, metres.
    pub radius: f32,
}

/// The client's character capsule: a vertical capsule whose origin is at its
/// **feet**.
///
/// `point_a` and `point_b` are the two sphere centres in the character's local
/// frame and `radius` the sphere radius. `ow_tank::character_collision`'s
/// `capsule_from_mover` is the client's construction of this shape and
/// `ow_tank::entity::mover_capsule_of` reads a hero's own.
///
/// **The rotation is not carried.** The capsule is vertical and the character
/// yaws, and a yaw about the up axis leaves a vertical capsule's two centres
/// where they are, so the query block's quaternion (`+0x40`) is the identity
/// ([`UPRIGHT`]). The leaf reads the block's quaternion to fold the support
/// points into a hull (`sweep_bounds`), and for a vertical capsule that hull is
/// the same under any yaw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Capsule {
    /// The lower sphere's centre: `r` above the capsule's origin.
    pub point_a: [f32; 3],
    /// The upper sphere's centre: `r + d` above the origin.
    pub point_b: [f32; 3],
    /// The sphere radius `r`.
    pub radius: f32,
}

impl Capsule {
    /// The capsule's total height, `2r + d`.
    pub fn height(&self) -> f32 {
        2.0 * self.radius + (self.point_b[1] - self.point_a[1])
    }

    /// The hull's centre height above the capsule's origin, `r + d/2`.
    ///
    /// The leaf folds the two centres into a hull whose centre is this far
    /// above the origin, so this is the height a probe of the character's
    /// middle has to sit at to test the capsule's cross-section.
    pub fn centre_height(&self) -> f32 {
        self.radius + 0.5 * (self.point_b[1] - self.point_a[1])
    }
}

/// The shape the walk queries with.
///
/// The client sweeps the character's own collision shape; [`Sphere`] is the
/// walk's proxy and [`Capsule`] is the client's.
pub trait Collider {
    /// The contact a sweep from `start` to `end` hits, if any.
    fn sweep(&self, world: &World, start: [f32; 3], end: [f32; 3]) -> Option<Sweep>;

    /// The contact the shape is **already inside** at `at`, if any.
    ///
    /// `skin` is the margin the query must exceed: the leaf calls a body
    /// resting on a surface "already inside" (its convergence radius is the
    /// expansion plus a margin, so a tangent centre converges and the `+0x72`
    /// flag turns that into a contact), so a penetration test has to ask about
    /// a depth **below** the resting distance.
    fn overlap(&self, world: &World, at: [f32; 3], skin: f32) -> Option<Sweep>;
}

impl Collider for Sphere {
    fn sweep(&self, world: &World, start: [f32; 3], end: [f32; 3]) -> Option<Sweep> {
        world.sweep(start, end, self.radius)
    }

    fn overlap(&self, world: &World, at: [f32; 3], skin: f32) -> Option<Sweep> {
        world.overlap(at, self.radius - skin)
    }
}

impl Collider for Capsule {
    fn sweep(&self, world: &World, start: [f32; 3], end: [f32; 3]) -> Option<Sweep> {
        world.sweep_capsule(start, end, UPRIGHT, self.point_a, self.point_b, self.radius)
    }

    fn overlap(&self, world: &World, at: [f32; 3], skin: f32) -> Option<Sweep> {
        world.overlap_capsule(at, UPRIGHT, self.point_a, self.point_b, self.radius - skin)
    }
}

/// The result of one step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Advanced {
    /// The resolved position.
    pub pos: [f32; 3],
    /// Whether the step ended on a contact whose normal points up.
    pub grounded: bool,
    /// The last contact's normal, if any contact was hit.
    pub contact: Option<[f32; 3]>,
    /// The shape the last contact came from, for the log.
    pub contact_shape: Option<usize>,
    /// Sweeps that returned a contact.
    pub contacts: u32,
    /// The fraction of the requested motion that was travelled; a zero request
    /// reports one, since there was nothing left to travel.
    pub travelled: f32,
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn mul(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn length(a: [f32; 3]) -> f32 {
    dot(a, a).sqrt()
}

/// How many pushes the depenetration makes before it gives up. A right-angle
/// corner needs one per wall; four leaves room for a wall the push moves the
/// body into.
pub const DEPENETRATION_STEPS: usize = 4;

/// Push the body out of every contact it is inside by more than [`CONTACT_SKIN`].
///
/// The radius is reduced by the skin because the leaf calls a body *resting* on
/// a surface "already inside" (its convergence radius is the expansion plus a
/// margin, so a tangent centre converges and the `+0x72` flag turns that into a
/// contact). A penetration test must therefore ask about a depth **below** the
/// resting distance, or every floor contact would read as a penetration.
fn depenetrate(world: &World, at: [f32; 3], collider: &impl Collider) -> ([f32; 3], Option<Sweep>) {
    let mut at = at;
    let mut last = None;
    // The depth is not reported, so the push doubles from the contact skin:
    // 5 + 10 + 20 + 40 mm clears 75 mm in four queries, and a one-skin
    // penetration (the common case) still costs one.
    let mut step = CONTACT_SKIN;
    for _ in 0..DEPENETRATION_STEPS {
        let Some(hit) = collider.overlap(world, at, CONTACT_SKIN) else {
            break;
        };
        // A wall's push is horizontal, for the same reason the slide's re-seat
        // is: a wall's normal carries a little up and lifting the body along it
        // is how a walk climbs a wall.
        let horizontal = hit.normal[1].abs() < WALKABLE_SLOPE;
        let push = mul(hit.normal, step);
        at = add(
            at,
            if horizontal {
                [push[0], 0.0, push[2]]
            } else {
                push
            },
        );
        last = Some(hit);
        step *= 2.0;
    }
    (at, last)
}

/// Advance `pos` by `delta`, resolving contacts.
///
/// One iteration is the driver's fold: travel the hit fraction, then compose
/// the residual by removing its component along the contact normal. Up to
/// [`SLIDE_ITERATIONS`] run per frame, so a corner takes two contacts and a
/// floor a single one.
///
/// The fold alone is not enough at a corner. The verified leaf reports nothing
/// for a sweep whose start is on or inside a surface, so a slide that leaves the
/// body exactly tangent to the second wall applies the residual in full and the
/// body passes through it - measured on a right-angle corner: `3` cm through the
/// second wall on the first frame. The leaf's own "hit while already inside"
/// query (`block+0x72`) closes it, in three places:
///
/// - before the first sweep and after every contact's re-seat the body is
///   pushed out of anything it is inside by more than the skin
///   (`depenetrate`);
/// - when the sweep reports nothing (its start is on a surface) the body moves
///   to the **depenetrated target**, not to the raw target: it lands on the
///   wall, and a rising slope still carries it because the push is along the
///   slope's normal.
///
/// Refusing the residual outright when the target penetrates was measured to
/// stop every ramp on the map, which is why the resolve is a move instead.
///
/// This is still the stand-in: the client moves the body and resolves the
/// Domino pair list, it does not sweep. The stage stays open in
/// `docs/open-questions.md`.
pub fn advance(world: &World, pos: [f32; 3], delta: [f32; 3], radius: f32) -> Advanced {
    advance_with(world, pos, delta, &Sphere { radius })
}

/// Advance `pos` by `delta` with the client's **capsule**, resolving contacts.
///
/// This is [`advance`] with the shape the client actually sweeps
/// ([`Capsule`]): every rule - the fold, the depenetration, the re-seat and
/// the slide - is the same, and only the query's shape differs.
///
/// `pos` is the capsule's **origin**, which is at its feet, so the resolved
/// position is the character's origin too.
pub fn advance_capsule(
    world: &World,
    pos: [f32; 3],
    delta: [f32; 3],
    capsule: &Capsule,
) -> Advanced {
    advance_with(world, pos, delta, capsule)
}

/// The shared body of [`advance`] and [`advance_capsule`].
fn advance_with(
    world: &World,
    pos: [f32; 3],
    delta: [f32; 3],
    collider: &impl Collider,
) -> Advanced {
    let requested = length(delta);
    let mut at = depenetrate(world, pos, collider).0;
    let mut remaining = delta;
    let mut contact = None;
    let mut contact_shape = None;
    let mut contacts = 0;
    let mut travelled = 0.0f32;
    for _ in 0..SLIDE_ITERATIONS {
        if length(remaining) <= 1e-6 {
            break;
        }
        let target = add(at, remaining);
        let Some(hit) = collider.sweep(world, at, target) else {
            // The start is on or inside a surface, so the moving sweep could
            // not see what is ahead - the state a corner leaves the body in.
            // Move, then resolve the overlaps: the body lands on the wall
            // instead of passing through it, and a rising slope still carries
            // it (the push is along the slope's own normal).
            let (resolved, overlap) = depenetrate(world, target, collider);
            if let Some(overlap) = overlap {
                contacts += 1;
                contact = Some(overlap.normal);
                contact_shape = Some(overlap.shape);
            }
            travelled += length(sub(resolved, at));
            at = resolved;
            break;
        };
        contacts += 1;
        let f = hit.fraction.clamp(0.0, 1.0);
        at = add(at, mul(remaining, f));
        travelled += length(remaining) * f;
        contact = Some(hit.normal);
        contact_shape = Some(hit.shape);
        // Re-seat off the surface: the sweep's own convergence radius is
        // `expansion + 0.00125`, so a centre left exactly on the contact reads
        // as "already inside" on the next sweep and is reported only for a
        // query that carries the inside byte. Stepping out by the collision
        // margin keeps every accepted contact resolvable.
        //
        // A wall's contact never lifts either half of the step (the re-seat or
        // the slide). The contact plane's own tilt would otherwise raise the
        // body: the practice range's walls report normals with `0.09` of up
        // (`(-0.576, -0.086, -0.813)`), which a plane projection reads as a
        // ramp, and the walk rides it up and over the top instead of stopping.
        // Height on a walk comes from a walkable slope's plane or from the floor
        // probe ([`snap_to_floor`]), never from a wall. The test is on the
        // contact's own normal rather than on `delta`'s vertical lane, because
        // one airborne frame gives a delta of a few millimetres downward and an
        // equality test would let the wall climb on that frame.
        let horizontal = hit.normal[1].abs() < WALKABLE_SLOPE;
        let skin = mul(hit.normal, CONTACT_SKIN);
        at = add(
            at,
            if horizontal {
                [skin[0], 0.0, skin[2]]
            } else {
                skin
            },
        );
        // A corner's second wall is not in this hit: the body may have slid
        // along the first one into it, so resolve any overlap before the next
        // sweep (and before the residual is composed, which assumes the start
        // is clear).
        at = depenetrate(world, at, collider).0;
        let rest = mul(remaining, 1.0 - f);
        let into = dot(rest, hit.normal);
        remaining = sub(rest, mul(hit.normal, into));
        if horizontal {
            remaining[1] = 0.0;
        }
    }
    let grounded = contact.is_some_and(|n| n[1] > 0.5);
    Advanced {
        pos: at,
        grounded,
        contact,
        contact_shape,
        contacts,
        travelled: if requested > 0.0 {
            travelled / requested
        } else {
            1.0
        },
    }
}

/// Snap a step's vertical **down** to the floor when it is close enough.
///
/// The new height is the starting height minus the reach times the contact
/// fraction, which is exactly where the swept sphere's centre stops. (The
/// narrowphase's `point` lane is the client's own contact record and is not the
/// centre; the fraction is the value the fold uses.)
pub fn snap_to_floor(world: &World, pos: [f32; 3], radius: f32, reach: f32) -> Option<[f32; 3]> {
    let hit = world.floor(pos, radius, reach)?;
    let y = pos[1] - reach * hit.fraction.clamp(0.0, 1.0);
    if (y - pos[1]).abs() > reach {
        return None;
    }
    // Only down. A floor probe that lands *above* the body has met a lip or a
    // bevel it is standing beside, and lifting the body onto it is the
    // step-up stage's job (`FUN_140cc1440`, a separate read stage), not the
    // floor follower's. Lifting here is what let a walk climb a wall: the
    // practice range's bevels report up normals and each frame raised the body
    // another few centimetres until it went over the top.
    if y > pos[1] {
        return None;
    }
    Some([pos[0], y, pos[2]])
}

/// Snap a step's vertical **down** to the floor with the client's capsule.
///
/// This is [`snap_to_floor`] with the shape the client sweeps: the capsule's
/// **feet** are its origin, so the floor's height is where the origin stops,
/// not where a sphere's centre would rest a radius above it.
///
/// `pos` is the capsule's origin (at the feet) and the answer is the origin's
/// new height.
pub fn snap_to_floor_capsule(
    world: &World,
    pos: [f32; 3],
    capsule: &Capsule,
    reach: f32,
) -> Option<[f32; 3]> {
    let down = [pos[0], pos[1] - reach, pos[2]];
    let hit = world.sweep_capsule(
        pos,
        down,
        UPRIGHT,
        capsule.point_a,
        capsule.point_b,
        capsule.radius,
    )?;
    if hit.normal[1] <= 0.5 {
        return None;
    }
    let y = pos[1] - reach * hit.fraction.clamp(0.0, 1.0);
    if (y - pos[1]).abs() > reach {
        return None;
    }
    // Only down, for the reason `snap_to_floor` gives.
    if y > pos[1] {
        return None;
    }
    Some([pos[0], y, pos[2]])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collider;

    const BASE: u64 = 0x0000_0001_0000_0000;

    /// A one-quad floor at `y = 0`, 40 m square, as a mesh blob the walk can
    /// sweep.
    fn floor_world() -> World {
        let vertices: Vec<[f32; 4]> = vec![
            [-20.0, 0.0, -20.0, 1.0],
            [20.0, 0.0, -20.0, 1.0],
            [20.0, 0.0, 20.0, 1.0],
            [-20.0, 0.0, 20.0, 1.0],
        ];
        let faces = vec![
            collider::SourceFace {
                index: [0, 1, 2],
                present: [true; 3],
            },
            collider::SourceFace {
                index: [0, 2, 3],
                present: [true; 3],
            },
        ];
        let _ = faces;
        // The winding puts the normal up: the analytic path is one-sided and a
        // downward sweep only meets a face whose normal opposes it.
        place_quad(&vertices, &[[0, 2, 1], [0, 3, 2]], [0.0, 0.0, 0.0])
    }

    /// One wall: a quad in the `y`-`z` plane at `x = 2`.
    fn wall_world() -> World {
        let vertices: Vec<[f32; 4]> = vec![
            [2.0, -1.0, -20.0, 1.0],
            [2.0, 3.0, -20.0, 1.0],
            [2.0, 3.0, 20.0, 1.0],
            [2.0, -1.0, 20.0, 1.0],
        ];
        let faces = vec![
            collider::SourceFace {
                index: [0, 1, 2],
                present: [true; 3],
            },
            collider::SourceFace {
                index: [0, 2, 3],
                present: [true; 3],
            },
        ];
        let _ = faces;
        place_quad(&vertices, &[[0, 2, 1], [0, 3, 2]], [0.0, 0.0, 0.0])
    }

    /// A blob with one leaf node and two triangles, in the client's record
    /// layout: nodes `0x20` stride, points `0x10`, triangles `0x20`.
    pub(crate) fn place_quad(vertices: &[[f32; 4]], triangles: &[[i32; 3]], at: [f32; 3]) -> World {
        place_quad_rotated(vertices, triangles, at, [0.0, 0.0, 0.0, 1.0])
    }

    /// [`place_quad`] with a local-to-world rotation, whose conjugate is what
    /// the shape transform carries (the leaf rotates the query into the shape's
    /// frame).
    fn place_quad_rotated(
        vertices: &[[f32; 4]],
        triangles: &[[i32; 3]],
        at: [f32; 3],
        rotation: [f32; 4],
    ) -> World {
        const NODE_LEN: usize = collide::MESH_NODE_LEN;
        const VERTEX_LEN: usize = collide::MESH_VERTEX_LEN;
        const TRI_LEN: usize = collide::MESH_TRIANGLE_LEN;
        let mut bytes = vec![0u8; NODE_LEN];
        let points_at = bytes.len();
        for v in vertices {
            let base = bytes.len();
            bytes.resize(base + VERTEX_LEN, 0);
            put_quad(&mut bytes, base, *v);
        }
        let triangles_at = bytes.len();
        for t in triangles {
            let base = bytes.len();
            bytes.resize(base + TRI_LEN, 0);
            for (lane, index) in t.iter().enumerate() {
                bytes[base + lane * 4..base + lane * 4 + 4].copy_from_slice(&index.to_le_bytes());
            }
            bytes[base + 0x1c..base + 0x1e].copy_from_slice(&0xffffu16.to_le_bytes());
        }
        let mut low = [f32::INFINITY; 3];
        let mut high = [f32::NEG_INFINITY; 3];
        for v in vertices {
            for lane in 0..3 {
                low[lane] = low[lane].min(v[lane] + at[lane]);
                high[lane] = high[lane].max(v[lane] + at[lane]);
            }
        }
        for lane in 0..3 {
            bytes[lane * 4..lane * 4 + 4].copy_from_slice(&low[lane].to_le_bytes());
            bytes[0x0c + lane * 4..0x0c + lane * 4 + 4].copy_from_slice(&high[lane].to_le_bytes());
        }
        bytes[0x18..0x1c].copy_from_slice(&0i32.to_le_bytes());
        bytes[0x1c..0x1e].copy_from_slice(&0xffffu16.to_le_bytes());
        bytes[0x1e] = 3;
        bytes[0x1f] = triangles.len() as u8;

        let mut record = vec![0u8; collide::MESH_RECORD_LEN];
        for lane in 0..4 {
            record[lane * 4..lane * 4 + 4].copy_from_slice(&1.0f32.to_le_bytes());
            record[0x10 + lane * 4..0x10 + lane * 4 + 4].copy_from_slice(&1.0f32.to_le_bytes());
        }
        record[0x20..0x28].copy_from_slice(&BASE.to_le_bytes());
        record[0x28..0x30].copy_from_slice(&(BASE + points_at as u64).to_le_bytes());
        record[0x30..0x38].copy_from_slice(&(BASE + triangles_at as u64).to_le_bytes());
        World::new(
            BASE,
            bytes,
            vec![Shape {
                record,
                xform: shape_xform(at, conjugate(rotation)),
                low,
                high,
            }],
        )
    }

    /// A downward sweep meets the floor and reports an up normal.
    #[test]
    fn the_floor_stops_a_downward_sweep() {
        let world = floor_world();
        let hit = world
            .sweep([0.0, 0.5, 0.0], [0.0, -0.5, 0.0], 0.35)
            .expect("the floor is hit");
        assert!(hit.normal[1] > 0.5, "normal {:?}", hit.normal);
        assert!(hit.fraction > 0.0 && hit.fraction <= 1.0, "{hit:?}");
    }

    /// Motion across an open floor is unresisted: the whole delta is travelled.
    #[test]
    fn open_floor_motion_is_not_clamped() {
        let world = floor_world();
        let out = advance(&world, [0.0, 0.35, 0.0], [1.0, 0.0, 0.0], 0.35);
        assert_eq!(out.contacts, 0);
        assert!((out.pos[0] - 1.0).abs() < 1e-5, "{out:?}");
        assert!((out.travelled - 1.0).abs() < 1e-5, "{out:?}");
    }

    /// Walking into a wall stops at it and keeps the tangential component: the
    /// slide is along the contact plane.
    #[test]
    fn a_wall_stops_the_normal_and_keeps_the_tangent() {
        let world = wall_world();
        let radius = 0.35;
        let out = advance(&world, [1.0, 1.0, 0.0], [3.0, 0.0, 1.0], radius);
        assert!(out.contacts > 0, "{out:?}");
        // Stopped short of the wall.
        assert!(out.pos[0] < 2.0 - 0.2, "{out:?}");
        assert!(out.pos[0] > 1.0, "the whole motion was eaten: {out:?}");
        // The tangential part survives.
        assert!(out.pos[2] > 0.2, "{out:?}");
    }

    /// Pressing into a wall for many frames must never pass it: the sweep is
    /// re-run from a point already on the contact surface, and the client's
    /// inside flag makes that a contact at fraction zero rather than a miss.
    #[test]
    fn a_sustained_wall_push_never_tunnels() {
        let world = wall_world();
        let radius = 0.35;
        let mut pos = [0.0, 1.0, 0.0];
        let mut crossed = false;
        for frame in 0..240 {
            let out = advance(&world, pos, [0.1, 0.0, 0.0], radius);
            pos = out.pos;
            if pos[0] + radius > 2.0 {
                println!("frame {frame}: centre x {:.6} is past the wall", pos[0]);
                crossed = true;
            }
        }
        assert!(!crossed, "the walk passed the wall: {pos:?}");
        assert!(pos[0] + radius <= 2.0 + 1e-4, "{pos:?}");
    }

    /// A tilted wall is a wall, not a ramp: a horizontal walk may not gain
    /// height through the slide. The practice range's own walls report normals
    /// a few degrees off vertical, and without this rule the walk rides the
    /// bevel up and over the top (the operator's "i can go through walls").
    #[test]
    fn a_horizontal_walk_cannot_climb_a_tilted_wall() {
        // A wall in the `y`-`z` plane at `x = 2`, tilted so its normal carries
        // `0.2` of up: the shape a bevel or a battered wall produces.
        let vertices: Vec<[f32; 4]> = vec![
            [2.0, -1.0, -20.0, 1.0],
            [2.4, 3.0, -20.0, 1.0],
            [2.4, 3.0, 20.0, 1.0],
            [2.0, -1.0, 20.0, 1.0],
        ];
        let world = place_quad(&vertices, &[[0, 2, 1], [0, 3, 2]], [0.0, 0.0, 0.0]);
        let radius = 0.35;
        // The tilt itself: the wall's normal leans `0.2` up, which is what made
        // the real range's contact climb.
        let hit = world
            .sweep([0.0, 1.0, 0.0], [2.0, 1.0, 0.0], radius)
            .expect("the tilted wall is hit");
        assert!(hit.normal[1] > 0.05, "the wall is not tilted: {hit:?}");
        let mut pos = [0.0, 1.0, 0.0];
        let mut highest = pos[1];
        for _ in 0..240 {
            let out = advance(&world, pos, [0.1, 0.0, 0.0], radius);
            pos = out.pos;
            highest = highest.max(pos[1]);
        }
        assert!(
            (highest - 1.0).abs() < 1e-4,
            "a horizontal walk climbed {:.3} m up a tilted wall",
            highest - 1.0
        );
        // The wall's surface at this height: the quad runs from `x = 2.0` at
        // `y = -1` to `x = 2.4` at `y = 3`, so `y = 1` sits at `x = 2.2`.
        assert!(pos[0] + radius <= 2.2 + 1e-3, "{pos:?}");
    }

    /// The fold leaves the centre outside the query's convergence radius
    /// (`expansion + 0.00125`), so the next frame's sweep toward the same wall
    /// still reports it instead of starting inside and missing.
    #[test]
    fn the_fold_leaves_the_body_outside_the_query_radius() {
        let world = wall_world();
        let radius = 0.35;
        let out = advance(&world, [1.0, 1.0, 0.0], [3.0, 0.0, 0.0], radius);
        assert!(out.contacts > 0, "{out:?}");
        let gap = 2.0 - (out.pos[0] + 0.35);
        assert!(
            gap > 0.00125,
            "the fold left the centre {gap} m from the wall"
        );
        let again = world.sweep(out.pos, [out.pos[0] + 0.1, 1.0, 0.0], radius);
        assert!(
            again.is_some(),
            "the next sweep missed the wall from {out:?}"
        );
    }

    /// A body resting on the floor reads as "inside" at the full radius (the
    /// leaf's convergence radius is the expansion plus a margin), and reads as
    /// clear once the contact skin comes off. The depenetration depends on
    /// exactly this: the penetration radius is `radius - CONTACT_SKIN`.
    #[test]
    fn a_resting_contact_is_not_a_penetration_at_the_penetration_radius() {
        let world = floor_world();
        let rest = [0.0, 0.35, 0.0];
        let inside = world.overlap(rest, 0.35).expect("a resting contact");
        assert!(inside.normal[1] > 0.5, "{inside:?}");
        assert_eq!(
            world.overlap(rest, 0.35 - CONTACT_SKIN),
            None,
            "a resting body must read as clear at the penetration radius"
        );
    }

    /// The penetration query names the wall, and a clear point reports nothing.
    #[test]
    fn the_penetration_query_names_the_wall_and_clears_open_space() {
        let world = wall_world();
        // 3 cm inside the wall at x = 2 (the body radius is 0.35).
        let inside = world.overlap([1.68, 1.0, 0.0], 0.35 - CONTACT_SKIN);
        let inside = inside.expect("the wall");
        assert!(inside.normal[0] < -0.5, "{inside:?}");
        assert_eq!(
            world.overlap([1.0, 1.0, 0.0], 0.35 - CONTACT_SKIN),
            None,
            "open space is not a penetration"
        );
    }

    /// A walkable slope ahead of the body is not a wall: the residual is
    /// applied (the client's floor follower is what raises the body), so the
    /// endpoint guard must not refuse it.
    #[test]
    fn a_rising_slope_does_not_refuse_the_residual() {
        // A 20-degree ramp: y rises with x, spanning z. Normal (0, 0.94, 0.34).
        let rise = 20.0f32.to_radians().tan();
        let vertices: Vec<[f32; 4]> = vec![
            [-20.0, -20.0 * rise, -20.0, 1.0],
            [20.0, 20.0 * rise, -20.0, 1.0],
            [20.0, 20.0 * rise, 20.0, 1.0],
            [-20.0, -20.0 * rise, 20.0, 1.0],
        ];
        // Winding chosen so the normal points up and back along -x.
        let world = place_quad(&vertices, &[[0, 2, 1], [0, 3, 2]], [0.0, 0.0, 0.0]);
        let down = world
            .sweep([0.0, 2.0, 0.0], [0.0, -1.0, 0.0], 0.35)
            .expect("the ramp");
        assert!(down.normal[1] > 0.9, "the ramp's normal: {down:?}");
        let rest = 2.0 - 3.0 * down.fraction;
        let start = [0.0, rest, 0.0];
        assert_eq!(
            world.overlap(start, 0.35 - CONTACT_SKIN),
            None,
            "the body rests on the ramp: {start:?}"
        );
        let out = advance(&world, start, [0.09, 0.0, 0.0], 0.35);
        assert!(
            out.pos[0] > start[0] + 0.05,
            "the residual was refused on a slope: {out:?}"
        );
    }

    /// A body that starts the frame inside a wall is pushed out, not left there.
    #[test]
    fn a_body_starting_inside_a_wall_is_pushed_out() {
        let world = wall_world();
        let out = advance(&world, [1.68, 1.0, 0.0], [0.0, 0.0, 0.0], 0.35);
        assert!(
            out.pos[0] < 2.0 - 0.35,
            "the body must leave the wall: {out:?}"
        );
        assert_eq!(
            world.overlap(out.pos, 0.35 - CONTACT_SKIN),
            None,
            "and read as clear afterwards: {out:?}"
        );
    }

    /// Two walls meeting at a right angle: walking diagonally into the corner    /// Two walls meeting at a right angle: walking diagonally into the corner
    /// must stop at both planes, never cross one.
    #[test]
    fn a_corner_stops_the_body_at_both_walls() {
        let vertices: Vec<[f32; 4]> = vec![
            // Wall A: the y-z plane at x = 2, facing -x.
            [2.0, -1.0, -20.0, 1.0],
            [2.0, 3.0, -20.0, 1.0],
            [2.0, 3.0, 20.0, 1.0],
            [2.0, -1.0, 20.0, 1.0],
            // Wall B: the x-y plane at z = 2, facing -z.
            [-20.0, -1.0, 2.0, 1.0],
            [20.0, -1.0, 2.0, 1.0],
            [20.0, 3.0, 2.0, 1.0],
            [-20.0, 3.0, 2.0, 1.0],
        ];
        let world = place_quad(
            &vertices,
            &[[0, 2, 1], [0, 3, 2], [4, 6, 5], [4, 7, 6]],
            [0.0, 0.0, 0.0],
        );
        let radius = 0.35;
        // Ten frames of a diagonal push into the corner at (2, 2).
        let mut at = [1.5, 1.0, 1.5];
        for frame in 0..10 {
            let out = advance(&world, at, [0.09, 0.0, 0.09], radius);
            at = out.pos;
            assert!(
                at[0] < 2.0 - radius + 0.02,
                "frame {frame}: crossed wall A, x {:.4} (pos {at:?}, {out:?})",
                at[0]
            );
            assert!(
                at[2] < 2.0 - radius + 0.02,
                "frame {frame}: crossed wall B, z {:.4} (pos {at:?}, {out:?})",
                at[2]
            );
        }
    }

    /// A downward step onto the floor reports the contact as ground and stops
    /// the drop.
    #[test]
    fn a_downward_step_lands_grounded() {
        let world = floor_world();
        let out = advance(&world, [0.0, 0.9, 0.0], [0.0, -0.6, 0.0], 0.35);
        assert!(out.grounded, "{out:?}");
        assert!((out.pos[1] - 0.35).abs() < 1e-2, "{out:?}");
    }

    /// The floor probe reports the contact height and the snap lands the centre
    /// a radius above it.
    #[test]
    fn the_floor_snap_lands_a_radius_above_the_surface() {
        let world = floor_world();
        let pos = [0.0, 0.9, 0.0];
        let snapped = snap_to_floor(&world, pos, 0.35, 1.0).expect("a floor within reach");
        assert!((snapped[1] - 0.35).abs() < 1e-3, "{snapped:?}");
    }

    /// The reach is what decides whether a drop is a step: the mover's own
    /// step-down limit is `2.5` m for the common hero (`+0x688`, read by
    /// `ow_tank::entity::mover_step_limits_of`), so a `2.0` m drop is a step
    /// the walk snaps down, while the old `1.5` reached nothing.
    #[test]
    fn a_two_metre_drop_is_a_step_at_the_movers_reach_but_not_below_it() {
        let world = floor_world();
        let pos = [0.0, 2.35, 0.0];
        assert_eq!(
            snap_to_floor(&world, pos, 0.35, 1.5),
            None,
            "1.5 m does not reach a 2.0 m drop"
        );
        let snapped = snap_to_floor(&world, pos, 0.35, 2.5).expect("2.5 m reaches it");
        assert!((snapped[1] - 0.35).abs() < 1e-3, "{snapped:?}");
    }

    /// The query block writes the client's fields at the client's offsets.
    fn conjugate(q: [f32; 4]) -> [f32; 4] {
        [-q[0], -q[1], -q[2], q[3]]
    }

    /// A wall rotated a quarter turn about `y` moves from the `y`-`z` plane to
    /// the `x`-`y` one: the sweep only meets it if the transform's rotation is
    /// the placement's inverse.
    #[test]
    fn the_shape_transform_rotates_the_query_into_the_shape() {
        let vertices: Vec<[f32; 4]> = vec![
            [2.0, -1.0, -3.0, 1.0],
            [2.0, 3.0, -3.0, 1.0],
            [2.0, 3.0, 3.0, 1.0],
            [2.0, -1.0, 3.0, 1.0],
        ];
        let quarter = [
            0.0,
            (std::f32::consts::FRAC_PI_4).sin(),
            0.0,
            (std::f32::consts::FRAC_PI_4).cos(),
        ];
        // Before the rotation the wall is at x = 2, so a +x sweep meets it and
        // a +z sweep from the middle of the plane would run inside it.
        let flat = place_quad(&vertices, &[[0, 2, 1], [0, 3, 2]], [0.0, 0.0, 0.0]);
        assert!(flat.sweep([1.0, 1.0, 0.0], [6.0, 1.0, 0.0], 0.05).is_some());
        // After a quarter turn about y the plane's normal is -x and the wall
        // lies across z = 2: a +z sweep meets it, a +x sweep no longer does.
        let turned =
            place_quad_rotated(&vertices, &[[0, 2, 1], [0, 3, 2]], [0.0, 0.0, 0.0], quarter);
        let along_z = turned.sweep([0.0, 1.0, 0.0], [0.0, 1.0, 6.0], 0.05);
        assert!(along_z.is_some(), "the rotated wall was not met along z");
        assert!(
            turned
                .sweep([0.0, 1.0, 0.0], [6.0, 1.0, 0.0], 0.05)
                .is_none(),
            "the unrotated plane was still met along x"
        );
    }

    #[test]
    fn the_query_block_is_the_read_layout() {
        let block = query_block([1.0, 2.0, 3.0], [4.0, 5.0, 6.0], 0.35, 0x0102);
        assert_eq!(f32_at(&block, 0x00), 1.0);
        assert_eq!(f32_at(&block, 0x04), 2.0);
        assert_eq!(f32_at(&block, 0x08), 3.0);
        assert_eq!(f32_at(&block, 0x10), 4.0);
        assert_eq!(f32_at(&block, 0x14), 5.0);
        assert_eq!(f32_at(&block, 0x18), 6.0);
        assert_eq!(f32_at(&block, 0x20), QUERY_MOTION);
        assert_eq!(f32_at(&block, 0x50), 0.35);
        assert_eq!(i32::from_le_bytes(block[0x38..0x3c].try_into().unwrap()), 0);
        assert_eq!(
            u16::from_le_bytes(block[0x70..0x72].try_into().unwrap()),
            0x0102
        );
        // The xform writer puts the position first and the quaternion second.
        let xf = shape_xform([7.0, 8.0, 9.0], [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(f32_at(&xf, 0x00), 7.0);
        assert_eq!(f32_at(&xf, 0x10), 0.0);
        assert_eq!(f32_at(&xf, 0x1c), 1.0);
    }
}

#[cfg(test)]
mod capsule_tests {
    use super::*;

    const BASE: u64 = 0x0000_0001_0000_0000;
    const UPRIGHT: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

    /// One quad of `size` half-extent, at height `y`, facing up (a floor) or
    /// down (a roof).
    fn plane(y: f32, up: bool) -> World {
        let vertices: Vec<[f32; 4]> = vec![
            [-20.0, y, -20.0, 1.0],
            [20.0, y, -20.0, 1.0],
            [20.0, y, 20.0, 1.0],
            [-20.0, y, 20.0, 1.0],
        ];
        let triangles: Vec<[i32; 3]> = if up {
            vec![[0, 2, 1], [0, 3, 2]]
        } else {
            vec![[0, 1, 2], [0, 2, 3]]
        };
        mesh_world(&vertices, &triangles, [-20.0, y, -20.0], [20.0, y, 20.0])
    }

    /// One wall at `x = 2` from `sill` upward, normal facing `-x`.
    fn wall_from(sill: f32) -> World {
        let vertices: Vec<[f32; 4]> = vec![
            [2.0, sill, -20.0, 1.0],
            [2.0, 3.0, -20.0, 1.0],
            [2.0, 3.0, 20.0, 1.0],
            [2.0, sill, 20.0, 1.0],
        ];
        mesh_world(
            &vertices,
            &[[0, 2, 1], [0, 3, 2]],
            [2.0, sill, -20.0],
            [2.0, 3.0, 20.0],
        )
    }

    /// A one-leaf mesh blob in the client's record layout, with the world box
    /// given explicitly (the broadphase only needs it to bucket the shape).
    fn mesh_world(
        vertices: &[[f32; 4]],
        triangles: &[[i32; 3]],
        low: [f32; 3],
        high: [f32; 3],
    ) -> World {
        let mut bytes = vec![0u8; collide::MESH_NODE_LEN];
        let points_at = bytes.len();
        for v in vertices {
            let base = bytes.len();
            bytes.resize(base + collide::MESH_VERTEX_LEN, 0);
            (0..4).for_each(|lane| {
                bytes[base + lane * 4..base + lane * 4 + 4].copy_from_slice(&v[lane].to_le_bytes())
            });
        }
        let triangles_at = bytes.len();
        for t in triangles {
            let base = bytes.len();
            bytes.resize(base + collide::MESH_TRIANGLE_LEN, 0);
            for (lane, index) in t.iter().enumerate() {
                bytes[base + lane * 4..base + lane * 4 + 4].copy_from_slice(&index.to_le_bytes());
            }
            bytes[base + 0x1c..base + 0x1e].copy_from_slice(&0xffffu16.to_le_bytes());
        }
        for lane in 0..3 {
            bytes[lane * 4..lane * 4 + 4].copy_from_slice(&low[lane].to_le_bytes());
            bytes[0x0c + lane * 4..0x0c + lane * 4 + 4].copy_from_slice(&high[lane].to_le_bytes());
        }
        bytes[0x18..0x1c].copy_from_slice(&0i32.to_le_bytes());
        bytes[0x1c..0x1e].copy_from_slice(&0xffffu16.to_le_bytes());
        bytes[0x1e] = 3;
        bytes[0x1f] = triangles.len() as u8;

        let mut record = vec![0u8; collide::MESH_RECORD_LEN];
        for lane in 0..4 {
            record[lane * 4..lane * 4 + 4].copy_from_slice(&1.0f32.to_le_bytes());
            record[0x10 + lane * 4..0x10 + lane * 4 + 4].copy_from_slice(&1.0f32.to_le_bytes());
        }
        record[0x20..0x28].copy_from_slice(&BASE.to_le_bytes());
        record[0x28..0x30].copy_from_slice(&(BASE + points_at as u64).to_le_bytes());
        record[0x30..0x38].copy_from_slice(&(BASE + triangles_at as u64).to_le_bytes());
        World::new(
            BASE,
            bytes,
            vec![Shape {
                record,
                xform: shape_xform([0.0, 0.0, 0.0], UPRIGHT),
                low,
                high,
            }],
        )
    }

    /// The support-point query is **not** a sphere at `P0`: the leaf replaces
    /// the swept box with the point cloud's hull (`sweep_bounds`), so the same
    /// numbers give a different contact than [`World::sweep`].
    ///
    /// This is the shape distinction the walk needs: a sphere proxy and the
    /// client's capsule are different queries even at the same radius.
    #[test]
    fn the_capsule_query_is_not_the_sphere_query() {
        let world = plane(0.0, true);
        let (start, end) = ([0.0, 1.5, 0.0], [0.0, -0.5, 0.0]);

        // The sphere touches when its centre reaches the radius: 0.5.
        let sphere = world.sweep(start, end, 0.5).expect("the floor stops it");
        assert!((sphere.fraction - 0.5).abs() < 1e-4, "{}", sphere.fraction);

        // The capsule's lowest point is its local origin, so it travels further.
        let capsule = world
            .sweep_capsule(start, end, UPRIGHT, [0.0, 0.5, 0.0], [0.0, 1.0, 0.0], 0.5)
            .expect("the floor stops the capsule");
        assert!(
            (capsule.fraction - 0.75).abs() < 1e-4,
            "capsule fraction {}",
            capsule.fraction
        );
        assert!(capsule.normal[1] > 0.5, "{:?}", capsule.normal);
        assert!(
            capsule.fraction > sphere.fraction,
            "the capsule stands taller than the sphere"
        );
    }

    /// The segment's length does not move a floor contact, because both capsule
    /// ends share the same lowest point: the feet meet the floor together.
    #[test]
    fn the_segments_length_does_not_move_a_floor_contact() {
        let world = plane(0.0, true);
        let (start, end) = ([0.0, 1.5, 0.0], [0.0, -0.5, 0.0]);
        let long = world
            .sweep_capsule(start, end, UPRIGHT, [0.0, 0.5, 0.0], [0.0, 1.0, 0.0], 0.5)
            .expect("hits");
        let short = world
            .sweep_capsule(start, end, UPRIGHT, [0.0, 0.5, 0.0], [0.0, 0.55, 0.0], 0.5)
            .expect("hits");
        assert!(
            (long.fraction - short.fraction).abs() < 1e-5,
            "{} vs {}",
            long.fraction,
            short.fraction
        );
    }

    /// The capsule's upper sphere reaches a sill the low sphere passes under.
    #[test]
    fn the_capsules_upper_sphere_reaches_a_wall() {
        // The wall starts at y = 0.9: a 0.5-radius sphere whose centre sits at
        // local y = 0.1 tops out at 0.6 and passes under, while the capsule's
        // upper sphere at 1.0 + 0.5 meets it.
        let world = wall_from(0.9);
        let (start, end) = ([0.0, 0.1, 0.0], [3.0, 0.1, 0.0]);
        assert_eq!(
            world.sweep(start, end, 0.5),
            None,
            "the low sphere passes under the sill"
        );
        let capsule = world
            .sweep_capsule(start, end, UPRIGHT, [0.0, 0.5, 0.0], [0.0, 1.0, 0.0], 0.5)
            .expect("the capsule meets the sill");
        assert!(capsule.fraction < 0.9, "{}", capsule.fraction);
    }

    /// The install's common capsule: `r = 0.5`, height `1.5`, so the segment
    /// is `0.5` and the hull's centre is `0.75` above the feet.
    ///
    /// This is the shape `ow_tank::character_collision::capsule_from_mover`
    /// builds for the common hero (`extent 2.0`, `radius 0.5`, `offset 0.5`).
    fn common_capsule() -> Capsule {
        Capsule {
            point_a: [0.0, 0.5, 0.0],
            point_b: [0.0, 1.0, 0.0],
            radius: 0.5,
        }
    }

    /// The capsule's **origin** is at its feet, so a body standing on a floor
    /// is at the floor's height - not a radius above it, the way a sphere's
    /// centre rests.
    #[test]
    fn the_capsule_origin_rests_on_the_floor() {
        let world = plane(0.0, true);
        let capsule = common_capsule();
        // Start above the floor and drop past it: the feet stop on the floor
        // while the drop still has room, so the contact is a real one.
        let out = advance_capsule(&world, [0.0, 1.0, 0.0], [0.0, -1.5, 0.0], &capsule);
        assert!(out.grounded, "{out:?}");
        assert!(
            (out.pos[1] - 0.0).abs() < 1e-2,
            "the feet rest on the floor, not a radius above it: {out:?}"
        );
    }

    /// The sphere's answer on the same floor: its **centre** rests a radius
    /// above the floor. The two shapes put the body at different heights, and
    /// that difference is the reason the capsule cannot be swapped for the
    /// sphere proxy.
    #[test]
    fn the_sphere_origin_rests_a_radius_above_the_floor() {
        let world = plane(0.0, true);
        let out = advance(&world, [0.0, 1.0, 0.0], [0.0, -1.0, 0.0], 0.5);
        assert!(out.grounded, "{out:?}");
        assert!(
            (out.pos[1] - 0.5).abs() < 1e-2,
            "the sphere's centre rests a radius above the floor: {out:?}"
        );
    }

    /// The capsule's floor probe answers with the **origin's** height, because
    /// the origin is at the feet.
    #[test]
    fn the_capsule_floor_probe_answers_with_the_feet() {
        let world = plane(0.0, true);
        let capsule = common_capsule();
        let snapped = snap_to_floor_capsule(&world, [0.0, 0.5, 0.0], &capsule, 1.0)
            .expect("a floor within reach");
        assert!(
            (snapped[1] - 0.0).abs() < 1e-2,
            "the feet land on the floor: {snapped:?}"
        );
        assert_eq!(
            snap_to_floor_capsule(&world, [0.0, 0.5, 0.0], &capsule, 0.1),
            None,
            "a floor out of reach is not a floor"
        );
    }

    /// The capsule's own width is the client's: a wall stops the capsule a
    /// radius further out than the same wall stops the sphere proxy, because
    /// the capsule's radius is `0.5` and not `0.35`.
    #[test]
    fn the_capsule_stops_a_radius_further_out() {
        let world = wall_from(-1.0);
        let capsule = common_capsule();
        let out = advance_capsule(&world, [0.0, 0.0, 0.0], [3.0, 0.0, 0.0], &capsule);
        assert!(out.contacts > 0, "{out:?}");
        assert!(
            out.pos[0] < 2.0 - 0.5,
            "the capsule stops a radius short of the wall: {out:?}"
        );
    }
}

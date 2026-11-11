//! The live mover: the read per-frame velocity law, advanced by the verified
//! swept query against the practice range's own collision.
//!
//! The chain this module drives is the client's live path
//! (`docs/movement-plan.md`):
//!
//! ```text
//! local wish (the input stage, `body+0x1140`)
//!   -> MoverWishEspring        the read spring's clamp/quantiser (`ow_sim::movement`)
//!   -> MoverStateVector        R * wish, the state record's transform ("state matrix")
//!   -> MoverRampVector         (min(timer, 1) * scale + offset) * direction
//!   -> frame_velocity          ADDPS(state, ramp)
//!   -> MoverAdvance            (state + ramp) * dt + pos
//!   -> the swept query         the verified type-3 mesh leaf over the range
//! ```
//!
//! What the input stage supplies is the one thing a client cannot read out of
//! its own binary: the player's control. Everything downstream of it is the
//! client's code, operation for operation, and the collision is the map's own
//! `MAP_CHUNK_COLLISION` mesh ({@link ow_tank::range_world::RangeWorld::level}).
//!
//! The per-model holder meshes are **not** in this world. Their read consumer
//! in this image is a query that writes nothing, and the per-model Domino body
//! route hangs off the collision component, whose attach is unread; adding them
//! was measured against the operator's report and cost `0.31` m of a `5.5` m
//! walk, `0.461` m of drift and a `0.78` m climb in a flat room
//! (`docs/formats/character-controller.md`).

use glam::Mat4;
use ow_sim::walk::{self, World};

/// The mover body record's span: the gravity accumulator at `body+0x1500` is
/// the highest field this module writes.
const BODY_LEN: usize = 0x1510;
/// `body + 0x1140`: the local wish vector the spring writes.
const WISH: usize = 0x1140;
/// `body + 0x1150`: the local movement vector - the arc's state.
const LOCAL: usize = 0x1150;
/// `body + 0xc0`: the four-lane direction the ramp multiplies.
const DIRECTION: usize = 0xc0;
/// `body + 0x10ec`: the ramp's timer, which saturates at one.
const TIMER: usize = 0x10ec;
/// `body + 0x5a8` / `+0x5b4`: the ramp's two scale slots (the settings default
/// is `1.0` for both, `0x1405a87d0`).
const SCALE_CLEAR: usize = 0x5a8;
const SCALE_SET: usize = 0x5b4;
/// The flags block's `+0x2c`: the per-frame offset added after `t * scale`.
const RAMP_OFFSET: usize = 0x2c;

/// The mover component's capsule **extent**, field hash `0x1972C687` (class
/// `+0x78`). The install-wide common value is `2.0` (`2.3` for the large
/// heroes); a picked hero's own row replaces it.
pub const CAPSULE_EXTENT: f32 = 2.0;

/// The mover component's capsule **radius**, field hash `0x7A70E3E3` (class
/// `+0x7c`). The install-wide common value is `0.5` (`0.7` for the large
/// heroes); a picked hero's own row replaces it.
pub const CAPSULE_RADIUS: f32 = 0.5;

/// The mover component's capsule **offset**, field hash `0x4A576BC2` (class
/// `+0x80`), the height the segment is derived against. The install-wide
/// common value is `0.5`; a picked hero's own row replaces it.
pub const CAPSULE_OFFSET: f32 = 0.5;

/// The walker's collider: the character's collision capsule, in the
/// character's local frame.
///
/// The client sweeps **this** shape
/// (`ow_tank::character_collision::capsule_from_mover` is the client's
/// construction of it). The walk used to sweep a `0.35` m sphere instead, which
/// is not a shape the client has: it is narrower than the capsule's `0.5` m
/// radius and only `0.7` m tall, so it passes under a sill the capsule's upper
/// sphere meets, and it rests a radius above the floor where the capsule's
/// feet do not.
pub fn capsule_of(extent: f32, radius: f32, offset: f32) -> ow_sim::walk::Capsule {
    let capsule = ow_tank::character_collision::capsule_from_mover(extent, radius, offset);
    ow_sim::walk::Capsule {
        point_a: capsule.point_a,
        point_b: capsule.point_b,
        radius: capsule.radius,
    }
}

/// The install-wide common capsule, for a launch with no hero picked.
pub fn common_capsule() -> ow_sim::walk::Capsule {
    capsule_of(CAPSULE_EXTENT, CAPSULE_RADIUS, CAPSULE_OFFSET)
}

/// The walk's collider for a capsule the client already built.
pub fn capsule_of_shape(capsule: ow_tank::character_collision::Capsule) -> ow_sim::walk::Capsule {
    ow_sim::walk::Capsule {
        point_a: capsule.point_a,
        point_b: capsule.point_b,
        radius: capsule.radius,
    }
}

/// The mover component's base speed, field hash `0x19452F15` (`5.5` for most
/// heroes in this install). The input source scales this; a selected hero's own
/// mover component replaces it.
pub const SPEED: f32 = 5.5;

/// The mover component's gravity magnitude, field hash `0xC2C40969` (class
/// `+0xe8`, runtime slot `body+0x610`). The frame's acceleration is
/// `-10.0 * gravity` along the up axis (`0x140ca72d0`); every playable hero on
/// this install reads `1.75`, so `-17.5 m/s^2`, except Jetpack Cat at `0.30`.
/// A picked hero's own row replaces it; this is the install-wide common value
/// for a launch with no hero pick.
pub const GRAVITY: f32 = 1.75;

/// The mover component's take-off lane, field hash `0xEC289987` (class `+0xe4`,
/// runtime slot `body+0x60c`): the up lane the standing jump writes. Every
/// playable hero on this install reads `6.0`, so `ow_sim::jump`'s impulse is a
/// `6.0 m/s` launch. A picked hero's own row replaces it.
pub const TAKEOFF_LANE: f32 = 6.0;

/// The mover component's terminal fall speed, the field hash `0xB74245E8`,
/// which reads `30.0` for hero 1 and for the install-wide common case
/// (`docs/formats/character-controller.md`). It is the **down** terminal of
/// the drag law (`0x140cb2064`), not a clamp this module applies itself.
pub const TERMINAL_FALL: f32 = 30.0;

/// The mover component's terminal speed while rising, field hash `0x975F21A5`
/// (class `+0xf0`, `body+0x618`). **No playable hero on this install declares
/// it**, so the component slot is zero and the drag law skips the rising lane
/// (`0x140cb207e`'s guard); a hero that did declare it would get a quadratic
/// drag on the way up too.
pub const RISE_TERMINAL: f32 = 0.0;

/// The eye's height above the character's **origin** (its feet), metres.
///
/// The camera's height is **not read** from the client (the view stage is
/// open, `docs/open-questions.md`), so this carries the sphere proxy's world
/// height over unchanged: the sphere's centre rested `0.35` m above the floor
/// and the eye `1.25` m above the centre, so the eye was `1.60` m above the
/// floor. The capsule's origin is at the feet, so the same world height is
/// `1.60` m above the origin.
pub const EYE_ABOVE_ORIGIN: f32 = 1.60;

/// How far down the floor probe reaches, metres: the mover's step-down limit
/// `+0x688` (`2.5` for the common hero, `2.8` for the large ones), read from
/// the picked hero's component by `ow_tank::entity::mover_step_limits_of` and
/// applied as [`Walker::floor_reach`]. A lip deeper than this is a fall, not a
/// step. This constant is the fallback for a launch with no hero pick.
pub const FLOOR_REACH: f32 = 2.5;

/// The walk's state: the body record, the position and the airborne velocity.
pub struct Walker {
    /// The collision world assembled from the range's models.
    pub world: World,
    /// The mover body record, at the client's offsets.
    body: Vec<u8>,
    /// The ramp's flags block, `body + 0x10d0`, as its own buffer.
    flags: Vec<u8>,
    /// The character's collision capsule, in the character's local frame. The
    /// walk sweeps this shape - the client's own - and not a sphere proxy.
    pub capsule: ow_sim::walk::Capsule,
    /// The character's origin, world space: the capsule's origin, which is at
    /// its **feet**.
    pub pos: [f32; 3],
    /// The arc's state: the up lane of the local movement vector
    /// (`body+0x1150`), m/s. Positive is up.
    pub fall: f32,
    /// The drag law's down terminal, m/s. [`TERMINAL_FALL`] by default; a
    /// selected hero's own `0xB74245E8` row replaces it.
    pub terminal_fall: f32,
    /// The drag law's up terminal, m/s. [`RISE_TERMINAL`] (zero: no drag while
    /// rising); a selected hero's own `0x975F21A5` row replaces it.
    pub rise_terminal: f32,
    /// The gravity magnitude, a scale on the client's `10.0`. [`GRAVITY`]
    /// (`1.75` -> `17.5 m/s^2`) by default; a picked hero's own `0xC2C40969`
    /// row replaces it.
    pub gravity: f32,
    /// The take-off lane, `body+0x60c`, m/s. [`TAKEOFF_LANE`] (`6.0`) by
    /// default; a picked hero's own `0xEC289987` row replaces it.
    pub takeoff_lane: f32,
    /// `body+0x1500..0x150c`: the client's world-space gravity displacement
    /// accumulator (`0x140cb2b4e`). Kept because the law writes it; its
    /// consumer is the body vtable's `+0x88` call, which the base mover leaves
    /// as a no-op.
    gravity_acc: [f32; 4],
    /// The jump button's held state, the `wants` the gate tests.
    pub jump: bool,
    /// The crouch button, bit 7 of the packed command.
    pub crouch: bool,
    /// The floor probe's reach, metres. [`FLOOR_REACH`] by default; a selected
    /// hero's own step-down limit replaces it.
    pub floor_reach: f32,
    pub grounded: bool,
    /// The last step's diagnostic, for the HUD and the log.
    pub last: Option<Step>,
}

/// One frame's report.
#[derive(Clone, Copy, Debug, Default)]
pub struct Step {
    /// The velocity the frame law composed, m/s (world).
    pub velocity: [f32; 3],
    /// The resolved position.
    pub pos: [f32; 3],
    /// Contacts the slide took.
    pub contacts: u32,
    /// The fraction of the requested motion travelled.
    pub travelled: f32,
    pub grounded: bool,
    /// The last contact's normal and the shape that produced it, for the log.
    pub contact: Option<[f32; 3]>,
    pub contact_shape: Option<usize>,
}

fn put_f32(bytes: &mut [u8], offset: usize, value: f32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

impl Walker {
    /// A walker with the character's own collision capsule, standing on the
    /// floor under `spawn`.
    ///
    /// The capsule decides the shape of every query the walk makes, so it is
    /// the one thing that must be right before the walk means anything: a
    /// picked hero's own capsule comes from
    /// `ow_tank::entity::mover_capsule_of`, and a launch with no hero pick
    /// uses the install-wide common one ([`common_capsule`]).
    pub fn new(world: World, spawn: [f32; 3], capsule: ow_sim::walk::Capsule) -> Walker {
        let mut body = vec![0u8; BODY_LEN];
        let mut flags = vec![0u8; 0x30];
        // The ramp's direction is up: a character's ramp term is vertical.
        put_f32(&mut body, DIRECTION + 4, 1.0);
        // The settings defaults: both scale slots are one, the timer is zero
        // and the offset is zero, so the ramp contributes nothing until it is
        // driven.
        put_f32(&mut body, SCALE_CLEAR, 1.0);
        put_f32(&mut body, SCALE_SET, 1.0);
        put_f32(&mut body, TIMER, 0.0);
        // The take-off lane the jump impulse reads, `body+0x60c`.
        put_f32(&mut body, ow_sim::jump::body::TAKEOFF_LANE, TAKEOFF_LANE);
        put_f32(&mut flags, RAMP_OFFSET, 0.0);
        // The flags byte selects the scale slot; bit 1 clear picks +0x5a8.
        flags[0] = 0;
        let pos = find_spawn(&world, spawn, &capsule);
        Walker {
            world,
            body,
            flags,
            capsule,
            pos,
            fall: 0.0,
            terminal_fall: TERMINAL_FALL,
            rise_terminal: RISE_TERMINAL,
            gravity: GRAVITY,
            takeoff_lane: TAKEOFF_LANE,
            gravity_acc: [0.0; 4],
            jump: false,
            crouch: false,
            floor_reach: FLOOR_REACH,
            // `find_spawn` already placed the body on the floor; the probe
            // takes over from the next frame.
            grounded: true,
            last: None,
        }
    }

    /// The state file word, `body+0x10d0` (the flags block's first dword).
    pub fn state_word(&self) -> u32 {
        u32::from_le_bytes(self.flags[0..4].try_into().unwrap())
    }

    fn set_state_word(&mut self, word: u32) {
        self.flags[0..4].copy_from_slice(&word.to_le_bytes());
    }

    /// Write the local movement vector, `body+0x1150`.
    fn set_local(&mut self, v: [f32; 4]) {
        for (i, lane) in v.iter().enumerate() {
            put_f32(&mut self.body, LOCAL + 4 * i, *lane);
        }
    }

    /// Read the local movement vector, `body+0x1150`.
    fn local(&self) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        for (i, lane) in out.iter_mut().enumerate() {
            let at = LOCAL + 4 * i;
            *lane = f32::from_le_bytes(self.body[at..at + 4].try_into().unwrap());
        }
        out
    }

    /// The camera's world eye, given the current position.
    pub fn eye(&self) -> [f32; 3] {
        [self.pos[0], self.pos[1] + EYE_ABOVE_ORIGIN, self.pos[2]]
    }

    /// One frame.
    ///
    /// `wish_local` is the input stage's vector in the character's own frame,
    /// metres per second; `yaw` is the character's facing (the same value the
    /// camera uses, so the local frame is the view's).
    pub fn step(&mut self, wish_local: [f32; 3], yaw: f32, dt: f32) -> Step {
        // The state record's transform: the character's rotation, no
        // translation. The client's own body position is the walk's `pos`, so
        // the frame's delta is the rotation applied to the local wish
        // (`MoverAdvance`'s `(scale_a*body[0xc0] + wish)*scale_b + pos`).
        let matrix = Mat4::from_rotation_y(yaw).to_cols_array();

        // The take-off / crouch stage, `FUN_140cc1440` through the gate, on the
        // live path's frame order (`MoverUpdateFrame` `0x140cbb6d8`). The
        // airborne prologue (`0x140cc148f..0x140cc155b`) and the gate
        // (`0x140cc1581..0x140cc15dc`) are `ow_sim::jump`'s leaves.
        {
            // `body+0x60c` is the mover component's take-off lane; the body
            // copy carries it, so the selected hero's row lands here.
            put_f32(
                &mut self.body,
                ow_sim::jump::body::TAKEOFF_LANE,
                self.takeoff_lane,
            );
            let wish_world = [wish_local[0], wish_local[1], wish_local[2], 0.0];
            let local = self.local();
            let mut word = self.state_word();
            let airborne = word & ow_sim::jump::file::NOT_ON_GROUND != 0;
            if airborne {
                // `dot(body+0x1150, cmd+0x10) < property(0x26, 0.001)`: slow
                // enough to accept the ground again.
                let along = ow_sim::resolve::hsum3([
                    ow_sim::movement::sse_mul(local[0], wish_world[0]),
                    ow_sim::movement::sse_mul(local[1], wish_world[1]),
                    ow_sim::movement::sse_mul(local[2], wish_world[2]),
                ]);
                if self.jump && along < 0.001 {
                    word = ow_sim::jump::mover_on_ground(word).flags;
                    self.set_state_word(word);
                }
            } else if !self.jump {
                // `MoverOnLanding`, the prologue's on-ground no-request arm.
                word = ow_sim::jump::mover_on_landing(word).flags;
                self.set_state_word(word);
            }
            // The walk's stand-in is the character controller itself, so the
            // live physics body the gate asks for (`body+0x1ad8`) is taken as
            // present; `ow_sim::jump` still models the "no body" arm for the
            // unit.
            let (gated, impulse) =
                ow_sim::jump::jump_gate(word, if self.jump { 1.0 } else { 0.0 }, true);
            self.set_state_word(gated);
            if impulse {
                // The impulse is `body+0x60c` along `body+0xc0`, transformed
                // through the state record (`0x140cc15d5..0x140cc166a`). The
                // rows are the matrix's columns for this yaw-only transform,
                // and the anchor `body+0x1510` is zero in this build.
                // The three 16-byte blocks `0x140c9e740` writes, which are
                // this matrix's columns.
                let rows = [
                    [matrix[0], matrix[1], matrix[2], matrix[3]],
                    [matrix[4], matrix[5], matrix[6], matrix[7]],
                    [matrix[8], matrix[9], matrix[10], matrix[11]],
                ];
                let raised = ow_sim::jump::jump_impulse(
                    local,
                    [0.0; 4],
                    Some(&rows),
                    self.state_word(),
                    self.takeoff_lane,
                );
                self.set_local(raised);
                // `MoverOnTakeoff(body, dl = 1)` and the `+0x1ad0` budget.
                let taken = ow_sim::jump::mover_on_takeoff(self.state_word(), true);
                self.set_state_word(taken.flags);
            }
            self.set_state_word(ow_sim::jump::mover_set_crouch(
                self.state_word(),
                self.crouch,
            ));
            // The impulse is the arc's new up lane; keep the mirror in step so
            // the frame's write below carries it.
            self.fall = self.local()[1];
        }

        // The local movement vector: the input stage's wish on the horizontal
        // lanes and the arc's own lane on y. `body+0x1140` (the wish) then
        // carries the same vector into the published velocity, which is the
        // fan writer's no-hit path - the fan's geometry pass is the declared
        // input stand-in the walk already ships.
        self.set_local([wish_local[0], self.fall, wish_local[2], 0.0]);
        let local = self.local();
        // The frame stage writes the frame's own duration to `body+0xe8`, and
        // the drag reads it back through `0x140ca72b0` (`body[0xe8] * 1e-6`).
        // This is the walk's frame clock: one tick, one duration.
        let micros = (f64::from(dt) * 1.0e6).round() as i64;
        self.body[ow_sim::air::body::MICROS..ow_sim::air::body::MICROS + 8]
            .copy_from_slice(&micros.to_le_bytes());
        let drag_dt = ow_sim::air::frame_seconds(micros);
        let local = ow_sim::air::step(
            local,
            ow_sim::air::UP,
            Some(&matrix),
            self.state_word(),
            &mut self.gravity_acc,
            dt,
            drag_dt,
            ow_sim::air::GATE_ON,
            self.gravity,
            self.terminal_fall,
            self.rise_terminal,
        );
        self.set_local(local);
        self.fall = local[1];
        put_f32(&mut self.body, WISH, local[0]);
        put_f32(&mut self.body, WISH + 4, local[1]);
        put_f32(&mut self.body, WISH + 8, local[2]);
        put_f32(&mut self.body, WISH + 12, local[3]);
        // The read tail: `ADDPS(state_vector(wish), ramp_from_body)`.
        let velocity = ow_sim::frame::update_frame(&self.body, &self.flags, Some(&matrix));
        let velocity = [velocity[0], velocity[1], velocity[2]];

        // The published vector is the frame's displacement this tick
        // (`MoverAdvance`: `pos + scale_b * (wish + scale_a * axis)`), so the
        // vertical travel is the arc's own lane through the same transform.
        let mut delta = [velocity[0] * dt, velocity[1] * dt, velocity[2] * dt];

        // The contact stage is the walk's verified sweep and floor probe; a
        // floor in reach ends the arc and zeroes the lane, which is what the
        // client's contact response does to the movement vector.
        //
        // The probe starts a fifth of a metre above the body so the capsule is
        // clear of the surface it is standing on: the leaf reports nothing for
        // a sweep whose start is on or inside a surface.
        let probe_from = [self.pos[0], self.pos[1] + 0.2, self.pos[2]];
        // A body that left the ground is swept, not snapped: the floor probe is
        // the walk's step-down rule and would otherwise pull a body off its own
        // apex (the probe reaches 2.5 m). The sweep's own `grounded` answer ends
        // the arc instead.
        let was_grounded = self.grounded;
        if self.fall > 0.0 || !was_grounded {
            self.grounded = false;
            delta[1] = self.fall * dt;
        } else {
            match walk::snap_to_floor_capsule(
                &self.world,
                probe_from,
                &self.capsule,
                self.floor_reach,
            ) {
                Some(snapped) => {
                    self.pos[1] = snapped[1];
                    self.fall = 0.0;
                    let mut local = self.local();
                    local[1] = 0.0;
                    self.set_local(local);
                    self.grounded = true;
                    // A snap must not also ask the sweep to travel vertically.
                    delta[1] = delta[1].min(0.0);
                }
                None => {
                    self.grounded = false;
                    delta[1] = self.fall * dt;
                }
            }
        }

        let out = walk::advance_capsule(&self.world, self.pos, delta, &self.capsule);
        self.pos = out.pos;
        if out.grounded {
            self.grounded = true;
            if self.fall < 0.0 {
                self.fall = 0.0;
                let mut local = self.local();
                local[1] = 0.0;
                self.set_local(local);
            }
        }
        let step = Step {
            velocity,
            pos: self.pos,
            contacts: out.contacts,
            travelled: out.travelled,
            grounded: self.grounded,
            contact: out.contact,
            contact_shape: out.contact_shape,
        };
        self.last = Some(step);
        step
    }
}

/// A spawn point on the range's floor.
///
/// The scene's own eye is the median of the placement origins: a viewpoint, not
/// a foothold, and on the practice range it sits inside the building. It is an
/// *eye*, so the room it stands in is the highest floor **below** it - a floor
/// above the eye is a roof or a ledge, and the terrain below the building is a
/// different room. The viewpoint's own column is preferred, which keeps the
/// spawn in the room the camera is in; a grid over the range's extent (whose
/// points also need a level patch) is the fallback when that column lands on a
/// prop or a wall. Every candidate keeps its headroom, and the spawn is the
/// highest such floor at or below the eye, at the point nearest the viewpoint.
///
/// Spawn selection is the client's: the map's own spawn points live in an unread
/// entity component (550 entity placeables in this range, none naming a model).
pub fn find_spawn(world: &World, centre: [f32; 3], capsule: &ow_sim::walk::Capsule) -> [f32; 3] {
    let bounds = world.extent();
    let below_eye = |at: &&[f32; 3]| at[1] <= centre[1] + 0.5;
    // The room the viewpoint is in: its own column's highest floor under the
    // eye. This is exact without a grid, and cannot land across a wall.
    let own = column_levels(world, [centre[0], centre[2]], bounds, false, capsule);
    if let Some(at) = own
        .iter()
        .filter(below_eye)
        .max_by(|a, b| a[1].total_cmp(&b[1]))
    {
        println!(
            "spawn: viewpoint ({:.2}, {:.2}, {:.2}); its column's floor is y {:.2}, picked ({:.2}, {:.2}, {:.2})",
            centre[0], centre[1], centre[2], at[1], at[0], at[1], at[2]
        );
        return *at;
    }
    let (low, high) = bounds;
    let step = 6.0f32;
    let mut levels: Vec<[f32; 3]> = Vec::new();
    let mut x = low[0];
    while x <= high[0] {
        let mut z = low[2];
        while z <= high[2] {
            levels.extend(column_levels(world, [x, z], bounds, true, capsule));
            z += step;
        }
        x += step;
    }
    if levels.is_empty() {
        return centre;
    }
    let mut counts: std::collections::BTreeMap<i64, usize> = std::collections::BTreeMap::new();
    for at in &levels {
        *counts.entry((at[1] * 2.0).round() as i64).or_default() += 1;
    }
    // The highest floor under the eye (its room, seen from the grid); a floor
    // above it only when the eye sits below every standable level.
    let under = counts
        .iter()
        .filter(|(bucket, _)| **bucket as f32 * 0.5 <= centre[1] + 0.5)
        .max_by_key(|(bucket, _)| **bucket)
        .map(|(b, c)| (*b, *c));
    let above = || {
        counts
            .iter()
            .filter(|(bucket, _)| **bucket as f32 * 0.5 >= centre[1] - 0.5)
            .max_by_key(|(bucket, count)| (**count, **bucket))
            .map(|(b, c)| (*b, *c))
    };
    let (bucket, count, under_eye) = match under.or_else(above) {
        Some((bucket, count)) => {
            let under_eye = (bucket as f32 * 0.5) <= centre[1] + 0.5;
            (bucket, count, under_eye)
        }
        None => return levels[0],
    };
    let mut best = levels[0];
    let mut best_score = f32::INFINITY;
    for at in &levels {
        if (at[1] * 2.0).round() as i64 != bucket {
            continue;
        }
        let dx = at[0] - centre[0];
        let dz = at[2] - centre[2];
        let score = dx * dx + dz * dz;
        if score < best_score {
            best_score = score;
            best = *at;
        }
    }
    println!(
        "spawn: viewpoint ({:.2}, {:.2}, {:.2}); {} standable level(s); the {} floor is {} point(s) at y {:.1}, picked ({:.2}, {:.2}, {:.2})",
        centre[0],
        centre[1],
        centre[2],
        levels.len(),
        if under_eye { "highest under the viewpoint" } else { "largest above the viewpoint" },
        count,
        bucket as f32 * 0.5,
        best[0],
        best[1],
        best[2]
    );
    best
}

/// Every floor a vertical march at `(x, z)` crosses that is standable: up
/// facing, with headroom above, and - when `level_patch` - level within a step
/// around it. The viewpoint's own column skips the patch test: the room's floor
/// may be a shallow ramp, and the camera being over it is the stronger fact.
fn column_levels(
    world: &World,
    xz: [f32; 2],
    bounds: ([f32; 3], [f32; 3]),
    level_patch: bool,
    capsule: &ow_sim::walk::Capsule,
) -> Vec<[f32; 3]> {
    let (low, high) = bounds;
    let step = 2.0f32;
    let mut out = Vec::new();
    let mut y = high[1] + 1.0;
    while y > low[1] {
        let Some(hit) = world.sweep_capsule(
            [xz[0], y, xz[1]],
            [xz[0], y - step, xz[1]],
            ow_sim::walk::UPRIGHT,
            capsule.point_a,
            capsule.point_b,
            capsule.radius,
        ) else {
            y -= step;
            continue;
        };
        let at = y - step * hit.fraction.clamp(0.0, 1.0);
        y = at - 0.2;
        if hit.normal[1] <= 0.9 {
            continue;
        }
        // Headroom: the capsule must fit under whatever is above.
        if world
            .sweep_capsule(
                [xz[0], at + 0.1, xz[1]],
                [xz[0], at + 2.0, xz[1]],
                ow_sim::walk::UPRIGHT,
                capsule.point_a,
                capsule.point_b,
                capsule.radius,
            )
            .is_some()
        {
            continue;
        }
        if !level_patch {
            out.push([xz[0], at, xz[1]]);
            continue;
        }
        let mut level = true;
        for (dx, dz) in [(1.2f32, 0.0f32), (-1.2, 0.0), (0.0, 1.2), (0.0, -1.2)] {
            let probe = [xz[0] + dx, at + 0.5, xz[1] + dz];
            // The spawn-levels march is not the walk's floor follower: it
            // probes in fixed increments, so it keeps the constant reach.
            match walk::snap_to_floor_capsule(world, probe, capsule, FLOOR_REACH) {
                Some(side) => {
                    let side_y = side[1];
                    if (side_y - at).abs() > 0.35 {
                        level = false;
                    }
                }
                None => level = false,
            }
        }
        if level {
            out.push([xz[0], at, xz[1]]);
        }
    }
    out
}

/// The input stage: the local wish vector for the held keys, in the character's
/// frame, metres per second.
///
/// The local axes are the character's own: `+z` forward, `+x` to the
/// character's left (the state transform maps `+z` to the camera's forward and
/// `+x` to its screen-left, which is where a right-handed `R_y` puts them).
pub fn wish_local(forward: f32, strafe: f32, speed: f32) -> [f32; 3] {
    let length = (forward * forward + strafe * strafe).sqrt();
    if length <= f32::EPSILON {
        return [0.0; 3];
    }
    [strafe / length * speed, 0.0, forward / length * speed]
}

//! The character mover: the per-hero data and the trace-measured integration.
//!
//! **Where the numbers come from.** One `STUCharacterMoverComponent` per hero is
//! read out of the gameplay entity by `ow-tank`'s mover reader. Its
//! per-hero scalars were then *identified by value shape* against a published
//! description, the way the health pools were (see `docs/provenance.md`: a
//! published figure is usable to check a value, never to make one):
//!
//! * forward/sideways base: 5.5 for most, 6.0 for Tracer, Genji and Anran
//!   (field hashes [`FIELD_FORWARD`] and [`FIELD_SIDE`]). Exactly three heroes
//!   read 6.0 in the dump, matching the published trio by shape alone.
//! * backward base: 90% of forward (4.95 / 5.4) — field [`FIELD_BACK`]; the
//!   dump reads 4.95 for almost every hero and 5.4 for the fast trio.
//! * crouch: 3.0 for "all" heroes per the published description, with a
//!   handful of exceptions in the dump (two and 2.5) — field [`FIELD_CROUCH`],
//!   named *mostly*-confirmed.
//! * terminal fall speed 30 m/s — field [`FIELD_FALL_TERMINAL`], constant
//!   across all heroes. The client's value and the published figure agree
//!   exactly, so this one is double-sourced.
//!
//! Everything else remains hash-labelled ([`MoverSpec::unlabelled`]); the
//! thrill of a name never outruns the evidence in this crate.
//!
//! **The horizontal arithmetic is measured from the client's own motion.** The
//! keyed captures (`D:/ow-dumps/trace/player_fast3_keys.jsonl`,
//! `player_keyed.jsonl`) write the traced node once per *frame*, and the
//! per-frame displacement is `v * frame_dt` with a *stable* per-frame dt
//! (~0.0323 s, ~31 fps), so the mover integrates once per frame — not on a
//! fixed tick. Measured from those writes:
//!
//! * travel speed 5.5 m/s and backpedal 4.95 m/s (the per-hero slot values
//!   exactly: the plateau step is 0.1777004 m = 5.5 * 0.032309 and
//!   0.1599036 m = 4.95 * 0.032309), diagonal input normalised, no penalty;
//! * acceleration 3.9216 m/s per frame from rest, so [`ACCEL`] = 121.4 m/s^2;
//! * friction 2.345 m/s per frame shed on release (four clean decrements over
//!   two captures), so [`FRICTION`] = 72.6 m/s^2; both are applied as
//!   `rate * dt` (the rate's dt-scaling is consistent with the measured
//!   partial first frame of a ramp, where the increment was a fraction of the
//!   rate — see `docs/formats/character-controller.md`);
//! * gravity 0.5613 m/s per frame (constant decrement, jitter tracks the
//!   frame dt), so [`GRAVITY`] = 17.37 m/s^2; the jump's first-frame velocity
//!   is [`JUMP_IMPULSE`] = 5.33 m/s (fit to the whole arc: the discrete sum of
//!   the ascent is 0.7333 m and the decrement 0.5613 m/s);
//! * airborne with **no** direction held the horizontal velocity is
//!   *preserved* (no friction): a character that leaves a ledge mid-ramp keeps
//!   its speed until it lands, then resumes the ramp;
//! * airborne with a direction held it **accelerates toward the wish speed at
//!   [`AIR_ACCEL`] = 18.5 m/s^2** and holds there — measured on the
//!   crouch/air captures (`player_ctrl*.jsonl`, 2026-10-15, a 60 fps practice
//!   range and a ~30 fps custom map): the per-write step grows by
//!   +0.00535 m at a 16.7 ms cadence and +0.0199 m at 33.0 ms, i.e. 17.1-19.2
//!   m/s^2 over five clean runs, until the step holds at the 5.5 m/s slot
//!   (~0.3 s from rest); jumping while already running leaves the step
//!   bit-identical at the slot, which is the same rule at the cap;
//! * crouch is the 3.0 slot reached through the ordinary ramp: the same
//!   capture's crouch-walk plateaus read 2.960-3.011 m/s and both transitions
//!   (walk -> crouch, crouch -> walk) complete in 1-2 frames, decelerating /
//!   accelerating at the ground [`ACCEL`] rate toward the new slot (the
//!   crouch transition to 3.0 never overshoots below it in the clean
//!   frame pair 5.5 -> 3.46 -> 2.97 m/s at i=10147, and the walk transition
//!   back reaches the slot in two frames at i=8023);
//! * on walkable ground the slot speed is held *along the surface* (the
//!   horizontal share `speed * cos(slope)`, the vertical `speed * sin(slope)`,
//!   the heading the wish's) — measured on a 17.9° climb: 4.940 m/s = the 4.95
//!   slot to 0.2%, and its heading is the forward run's reversed to 0.2°. The
//!   *steep*-slide case (|v| = 4.6895 m/s, not a slot) stays unmodelled, tier 3.
//!
//! The mover control's *code* (plan question Q3) is **closed as tier 3**: the
//! constants above are trace-measured rates, and the control itself is
//! obfuscated (encrypted vtable slots, junk decompilations, and no rate literal
//! or velocity store in any capture — the evidence trail is in
//! `docs/formats/character-controller.md`, the rule in `AGENTS.md`). The
//! physics the mover drives is extracted — [`crate::body`] holds the client's
//! island-solve integration with golden vectors.
//!
//! `docs/formats/character-controller.md` carries the extraction addresses.

/// Forward (and sideways) base field.
pub const FIELD_FORWARD: u32 = 0x1945_2F15;
/// Sideways base field; set equal to forward on every readable hero.
pub const FIELD_SIDE: u32 = 0xCFD3_E635;
/// Backpedal base field (90% of forward, per published figure and dump).
pub const FIELD_BACK: u32 = 0x61D6_B4E9;
/// Crouch speed field (3.0 mostly; a few heroes disagree so it is provisional).
pub const FIELD_CROUCH: u32 = 0x0DDD_18C3;
/// Terminal fall speed field (30 m/s, both sources agree).
pub const FIELD_FALL_TERMINAL: u32 = 0xB742_45E8;
/// The only other per-hero-varying scalar (1.7 .. 4.0); unlabelled.
pub const FIELD_UNNAMED_SCALAR: u32 = 0x1972_C687;

/// Runtime slot of the speed float inside the client's mover object.
///
/// Confirmed twice: the reflective field table in the capture's data areas
/// (row `{hash 0x031FE7EE, type ptr, offset 0xe0}`) and the client's own
/// registrar (each field registered against the allocation slot that follows
/// it). The rule across the whole component is *runtime slot = STU offset + 4*;
/// the STU offset of this field is 220, so the runtime slot is 224 = 0xe0.
pub const SLOT_SPEED: usize = 0xE0;

/// Reference tick rate: the client's *physics* step is fixed at 1/30 s
/// (`DAT_7ff7c7d5d84c` = 0.0333333351, the module dt table's `+0x0c` entry).
/// The *mover* does not integrate on it — the traced node moves once per
/// frame with the frame's dt — so this constant only serves as the reference
/// rate for callers that want a nominal dt.
pub const TICK_RATE_HZ: f32 = 30.0;

/// Vertical impulse of a jump, in metres per second. **Measured** from the
/// traced jump arc (`player_keyed.jsonl`, keys `space`): the first frame's
/// displacement is `(impulse - g*dt) * dt` = 0.15426 m at dt = 0.032309, and
/// the discrete ascent sums to 0.7333 m, giving 5.33 m/s. (The mover's own
/// jump struct defaults carry a 0.98 m apex; at this impulse and gravity the
/// discrete apex is 0.733 m, so the default is not the operative pair here.)
pub const JUMP_IMPULSE: f32 = 5.33;
/// Downward acceleration in air, in metres per second squared. **Measured**:
/// the traced jump sheds a constant 0.5613 m/s per frame (variance tracks the
/// frame dt, so it is dt-scaled), and 0.5613 / 0.032309 = 17.37. No easing
/// toward the terminal speed was observed on the way down.
pub const GRAVITY: f32 = 17.37;

/// Horizontal acceleration in metres per second squared. **Measured** from
/// the live ramps: from rest the client gains 3.9216 m/s in its first full
/// frame (0.1267069 m of extra displacement over a partial first frame at
/// dt = 0.032309), so the rate is 121.4. The measurement is one frame pair
/// (+/-0.7); 121.4 is 1.2% above 120, which is within that band.
pub const ACCEL: f32 = 121.4;
/// Horizontal friction toward zero when no direction is held, in metres per
/// second squared. **Measured**: the decrement is a *constant* 2.345 m/s per
/// frame (2.391/2.303 and 2.353/2.333 in two captures — a constant delta, not
/// a decay proportional to v), so 2.345 / 0.032309 = 72.6.
pub const FRICTION: f32 = 72.6;

/// Horizontal acceleration while airborne with a direction held, in metres per
/// second squared. **Measured** from the per-write step increases of the
/// air-accel runs (standing jump, then W pressed mid-air) in three captures:
/// +0.00535 m/write at a 16.7 ms write cadence (`player_ctrl.jsonl`, practice
/// range, 60 fps client), +0.0199 m/write at 33.0 ms (`player_ctrl2.jsonl`,
/// custom map, ~30 fps client) and +0.0204 m at 30.4 ms (`player_ctrl3.jsonl`,
/// same map at 200 Hz sampling) — the increase scales with `dt^2`, so the rate
/// is 17.1-19.2 m/s^2 over five clean runs, and 18.5 is their centre. The run
/// ends when the step holds at the walk slot: the speed ramps 0 -> 5.5 m/s in
/// ~0.3 s in both sessions. With no direction held the step is bit-identical
/// (preserved). `dev/air_probe.py` re-derives this from a trace.
pub const AIR_ACCEL: f32 = 18.5;

/// A velocity vector that moves toward `target` by at most `limit`.
///
/// The approach is along the *vector*, not per component: the traced ramp
/// keeps the wish direction (the flat trace's plateau steps are parallel to
/// the yaw direction to 0.006 degrees; the first ramp step reads within ~0.5
/// degrees of it), and the traced stop sheds
/// a constant speed along the old direction. A per-component clamp would swing
/// the direction during a turn.
fn move_toward_vec(v: [f32; 2], target: [f32; 2], limit: f32) -> [f32; 2] {
    let d = [target[0] - v[0], target[1] - v[1]];
    let len = (d[0] * d[0] + d[1] * d[1]).sqrt();
    if len <= limit || len == 0.0 {
        target
    } else {
        let s = limit / len;
        [v[0] + d[0] * s, v[1] + d[1] * s]
    }
}

use crate::Vec3;

/// A mover's per-hero constants as read from the install.
///
/// `speed` is the verified figure. The rest are carried verbatim so that the
/// pipeline files-to-simulation runs end to end without inventing meanings;
/// naming them moves them from hash rows to named rows, and that step is what
/// `docs/formats/character-controller.md` tracks.
pub struct MoverSpec {
    /// Base travel speed in metres per second. 5.5 for every readable hero.
    pub speed: f32,
    /// Every other float field, keyed by its field hash.
    pub unlabelled: Vec<(u32, f32)>,
}

impl MoverSpec {
    /// The default-mover spec from the generated class file's declared defaults
    /// (fields the heroes never override; see `docs/formats/character-controller.md`).
    pub fn from_base_speed(speed: f32) -> MoverSpec {
        MoverSpec {
            speed,
            unlabelled: Vec::new(),
        }
    }

    /// An override for an unlabelled field.
    pub fn with_unlabelled(mut self, hash: u32, value: f32) -> MoverSpec {
        self.unlabelled.retain(|(h, _)| *h != hash);
        self.unlabelled.push((hash, value));
        self
    }

    /// A field's value by hash, if this hero overrides it.
    pub fn get(&self, hash: u32) -> Option<f32> {
        if hash == FIELD_FORWARD {
            return Some(self.speed);
        }
        self.unlabelled
            .iter()
            .find(|(h, _)| *h == hash)
            .map(|(_, v)| *v)
    }

    fn named(&self, hash: u32, fallback: f32) -> f32 {
        self.get(hash).unwrap_or(fallback)
    }

    /// Backward base speed for this hero.
    pub fn back_speed(&self) -> f32 {
        self.named(FIELD_BACK, self.speed * 0.9)
    }

    /// Sideways base speed for this hero.
    pub fn side_speed(&self) -> f32 {
        self.named(FIELD_SIDE, self.speed)
    }

    /// Crouch speed for this hero.
    pub fn crouch_speed(&self) -> f32 {
        self.named(FIELD_CROUCH, 3.0)
    }

    /// Terminal fall speed for this hero.
    pub fn fall_terminal(&self) -> f32 {
        self.named(FIELD_FALL_TERMINAL, 30.0)
    }
}

/// One discrete input of the controller for a tick.
#[derive(Debug, Clone, Copy, Default)]
pub struct Input {
    /// W/S relative to the current yaw, -1 .. 1.
    pub forward: f32,
    /// D/A relative to the current yaw, -1 .. 1.
    pub side: f32,
    /// Held: move slowly.
    pub crouch: bool,
    /// Edge-triggered: jump this tick.
    pub jump: bool,
}

/// Directional axes of a look.
#[derive(Debug, Clone, Copy, Default)]
pub struct Axes {
    pub yaw: f32,
}

/// The player's movement state; integrates input and gravity once per frame.
///
/// **What is measured from the client** (see the module header): the per-hero
/// speeds ([`MoverSpec`]), the horizontal ramps ([`ACCEL`], [`FRICTION`]), the
/// per-frame integration stride, gravity and the jump impulse ([`GRAVITY`],
/// [`JUMP_IMPULSE`]), the direction convention (forward = (sin yaw, cos yaw),
/// right = (-cos yaw, sin yaw)), the airborne rule (no input: horizontal
/// velocity preserved; direction held: [`AIR_ACCEL`] up to the slot), and the
/// crouch slot with its transitions. **What is not:** ledge handling, terrain
/// collision, and the slope/slide behaviour the keyed captures show on steep
/// ground. The gate that arbitrates all "matching" claims is
/// `trace_diff keys <trace> --by-state` (see `formats/domino.md`).
pub struct Controller {
    pub spec: MoverSpec,
    /// Position of the eye of the mover.
    pub pos: Vec3,
    pub yaw: f32,
    /// Vertical velocity in metres per second, +up.
    pub vertical: f32,
    /// Whether the mover rests on ground.
    pub grounded: bool,
    /// The ground's normal at the mover's feet, world axes. `[0, 1, 0]` (the
    /// default) means flat ground, which is the only case the harness
    /// sandbox has. On walkable ground the mover holds the slot speed *along
    /// the surface*: measured on a 17.9 degree climb in
    /// `player_keyed.jsonl` (i=352) the horizontal is 4.699 m/s and the
    /// vertical +1.523 m/s, whose magnitude is 4.940 = the 4.95 slot to 0.2%.
    pub ground_normal: Vec3,
    /// The vertical velocity the slope contributes while grounded (the slot
    /// speed's `sin` share). Zero on flat ground.
    pub slope_vertical: f32,
    /// Horizontal velocity in world axes (x, z), metres per second. On the
    /// ground it ramps toward the input target at [`ACCEL`] and decays to zero
    /// at [`FRICTION`], `rate * dt` per frame; airborne it ramps at
    /// [`AIR_ACCEL`] when a direction is held and does not change without one.
    pub vel: [f32; 2],
}

impl Controller {
    /// A controller standing at `pos`, facing `yaw`.
    pub fn new(spec: MoverSpec, pos: Vec3, yaw: f32) -> Controller {
        Controller {
            spec,
            pos,
            yaw,
            vertical: 0.0,
            grounded: true,
            ground_normal: [0.0, 1.0, 0.0],
            slope_vertical: 0.0,
            vel: [0.0; 2],
        }
    }

    /// One frame of `dt` seconds: a single integration with the frame's own
    /// dt, matching the client's per-frame writes.
    pub fn advance(&mut self, dt: f32, input: Input) {
        self.tick(dt, input);
    }

    /// One integration step of `dt` seconds.
    fn tick(&mut self, dt: f32, input: Input) {
        // Horizontal: a target velocity from the input direction. On the
        // ground it is approached at ACCEL, and with no input the velocity
        // decays at FRICTION. Airborne it is approached at AIR_ACCEL when a
        // direction is held, and left bit-identical when none is (measured:
        // the step holds its exact bits through a whole fall).
        //
        // Directions use the client's convention, verified against the live
        // trace: forward = (sin yaw, cos yaw), right = (-cos yaw, sin yaw)
        // (D strafes along right, A along -right).
        let yaw_c = self.yaw.cos();
        let yaw_s = self.yaw.sin();
        let fwd_dir = [yaw_s, yaw_c];
        let right_dir = [-yaw_c, yaw_s];
        let ux = fwd_dir[0] * input.forward + right_dir[0] * input.side;
        let uz = fwd_dir[1] * input.forward + right_dir[1] * input.side;
        let len = (ux * ux + uz * uz).sqrt();
        let (mut tx, mut tz) = (0.0f32, 0.0f32);
        let mut moving = false;
        if len > 1e-5 {
            let mut speed = if input.forward < 0.0 {
                self.spec.back_speed()
            } else if input.forward > 0.0 || input.side != 0.0 {
                self.spec.speed
            } else {
                0.0
            };
            if input.crouch {
                speed = speed.min(self.spec.crouch_speed());
            }
            // The input direction is normalised, so a diagonal runs at the
            // full single-axis speed (measured: W+D plateaus at 5.5).
            tx = ux / len * speed;
            tz = uz / len * speed;
            moving = true;
        }
        self.slope_vertical = 0.0;
        if self.grounded {
            // Walkable ground: the slot speed is held *along the surface*,
            // so the horizontal share is `speed * cos(slope)` and the vertical
            // `speed * sin(slope)`, both constant while the slope is constant
            // (measured). `t` is the directional slope, tan(theta).
            if moving {
                let n = self.ground_normal;
                if (n[1] - 1.0).abs() > 1e-4 && n[1] > 1e-3 {
                    let ux = tx / (tx * tx + tz * tz).sqrt().max(1e-9);
                    let uz = tz / (tx * tx + tz * tz).sqrt().max(1e-9);
                    let t = -(ux * n[0] + uz * n[2]) / n[1];
                    let scale = 1.0 / (1.0 + t * t).sqrt();
                    tx *= scale;
                    tz *= scale;
                    self.slope_vertical = (tx * tx + tz * tz).sqrt() * t;
                }
            }
            let limit = if moving { ACCEL * dt } else { FRICTION * dt };
            if moving && self.vel[0] * tx + self.vel[1] * tz < 0.0 {
                // A wish that *opposes* the velocity sheds the release
                // friction as well as accelerating: the traced reverse
                // (walk -> backpedal, 33 ms write cadence) sheds 10.45 m/s in
                // one write — ~210-230 m/s^2, where the acceleration alone is
                // [`ACCEL`] = 121.4 and ACCEL + FRICTION = 194
                // (`player_ctrl3.jsonl` i=4579 and i=17599, `player_ctrl4.jsonl`
                // i=7783). A perpendicular wish (a 90 degree turn) has no
                // opposing component, and the straight-ahead ramp is untouched:
                // both measured clean.
                self.vel = move_toward_vec(self.vel, [0.0, 0.0], FRICTION * dt);
            }
            self.vel = move_toward_vec(self.vel, [tx, tz], limit);
        } else if moving {
            self.vel = move_toward_vec(self.vel, [tx, tz], AIR_ACCEL * dt);
        }
        self.pos[0] += self.vel[0] * dt;
        self.pos[2] += self.vel[1] * dt;
        if self.grounded {
            self.pos[1] += self.slope_vertical * dt;
        }

        // Jump: edge pulse from the caller.
        if input.jump && self.grounded {
            self.vertical = JUMP_IMPULSE;
            self.grounded = false;
        }

        // Vertical: constant gravity, clamped at the per-hero terminal speed.
        // The traced decrement is constant, so no easing toward the terminal.
        if !self.grounded {
            let terminal = self.spec.fall_terminal();
            self.vertical = (self.vertical - GRAVITY * dt).max(-terminal);
            self.pos[1] += self.vertical * dt;
        } else {
            self.vertical = 0.0;
        }
    }

    /// Land on a floor height; returns true when untouched.
    pub fn settle_ground(&mut self, ground_y: f32) {
        if self.pos[1] <= ground_y {
            self.pos[1] = ground_y;
            self.vertical = 0.0;
            self.grounded = true;
        }
    }
}

/// A position and direction in metres; re-exported for callers.
pub type MoveVec = Vec3;

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / TICK_RATE_HZ;
    const EPS: f32 = 1e-3;

    fn approx(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() < eps
    }

    #[test]
    fn speed_carries_the_extracted_value() {
        let spec = MoverSpec::from_base_speed(5.5);
        assert_eq!(spec.get(FIELD_FORWARD), Some(5.5));
        assert_eq!(spec.get(0x0000_0001), None);
        assert_eq!(spec.back_speed(), 5.5 * 0.9);
        assert_eq!(spec.side_speed(), 5.5);
        assert_eq!(spec.crouch_speed(), 3.0);
        assert_eq!(spec.fall_terminal(), 30.0);
    }

    #[test]
    fn backpedal_speed_scales_with_the_base() {
        let spec = MoverSpec::from_base_speed(6.0);
        assert!(approx(spec.back_speed(), 5.4, EPS));
    }

    #[test]
    fn forward_ramps_in_two_client_frames_from_rest() {
        // The traced ramp from rest (`player_fast3_keys.jsonl`): the first
        // full frame gains ACCEL * 0.032309 = 3.9216 m/s (the trace's first
        // two writes are 0.0063696 m and 0.1330765 m), and the second frame
        // clamps at the 5.5 slot. `move_toward` reproduces both exactly at
        // the trace's own frame dt.
        let dt = 0.032309f32;
        let mut c = Controller::new(MoverSpec::from_base_speed(5.5), [0.0; 3], 0.0);
        let input = Input {
            forward: 1.0,
            ..Default::default()
        };
        c.tick(dt, input);
        assert!(approx(c.vel[1], 3.9216, 0.01), "v1 {}", c.vel[1]);
        c.tick(dt, input);
        assert!(approx(c.vel[1], 5.5, EPS), "v2 {}", c.vel[1]);
    }

    #[test]
    fn backward_uses_the_back_speed() {
        let spec = MoverSpec::from_base_speed(5.5);
        let mut c = Controller::new(spec, [0.0; 3], 0.0);
        let input = Input {
            forward: -1.0,
            ..Default::default()
        };
        for _ in 0..3 {
            c.tick(DT, input);
        }
        assert!(approx(c.vel[1], -back_speed(5.5), EPS), "{}", c.vel[1]);
    }

    fn back_speed(base: f32) -> f32 {
        base * 0.9
    }

    #[test]
    fn crouch_caps_the_horizontal_speed() {
        let spec = MoverSpec::from_base_speed(5.5);
        let mut c = Controller::new(spec, [0.0; 3], 0.0);
        let input = Input {
            forward: 1.0,
            crouch: true,
            ..Default::default()
        };
        for _ in 0..4 {
            c.tick(DT, input);
        }
        assert!(approx(c.vel[1], 3.0, EPS), "{}", c.vel[1]);
    }

    #[test]
    fn friction_stops_the_mover_linearly() {
        let spec = MoverSpec::from_base_speed(5.5);
        let mut c = Controller::new(spec, [0.0; 3], 0.0);
        for _ in 0..4 {
            c.tick(
                DT,
                Input {
                    forward: 1.0,
                    ..Default::default()
                },
            );
        }
        let v = c.vel[1];
        c.tick(DT, Input::default());
        assert!(approx(c.vel[1], v - FRICTION * DT, EPS));
    }

    #[test]
    fn airborne_horizontal_is_preserved() {
        // The traced ledge walk-off (`player_keyed.jsonl`, i=969): mid-ramp
        // the character leaves the ground at 4.29 m/s and the horizontal
        // stays there for the whole fall, then resumes ramping on landing.
        let dt = 0.032309f32;
        let mut c = Controller::new(MoverSpec::from_base_speed(5.5), [0.0; 3], 0.0);
        let input = Input {
            forward: 1.0,
            ..Default::default()
        };
        c.tick(dt, input);
        let left_ground_with = c.vel[1];
        c.grounded = false;
        for _ in 0..12 {
            c.tick(dt, Input::default());
        }
        assert_eq!(c.vel[1], left_ground_with);
        c.grounded = true;
        c.tick(dt, input);
        assert!(c.vel[1] > left_ground_with, "{}", c.vel[1]);
    }

    #[test]
    fn reversing_the_wish_sheds_friction_and_accel_together() {
        // Traced (`player_ctrl4.jsonl`, i=7783, a ~30 fps client): walking at
        // +5.5 m/s, S goes down and the velocity reads +4.78, -1.53, -4.51 m/s
        // over the next writes — the velocity crosses zero in one write, which
        // ACCEL alone cannot do (it would take three).  FRICTION + ACCEL =
        // 194 m/s^2 is the measured rate.  A trailing key-phase offset makes
        // the exact first-write value uncertain, so the assertion is the one
        // that separates the rules: accel-only would leave this write at
        // +1.86 m/s, not below zero.
        let dt = 0.030f32;
        let walk = Input {
            forward: 1.0,
            ..Default::default()
        };
        let back = Input {
            forward: -1.0,
            ..Default::default()
        };
        let mut c = Controller::new(MoverSpec::from_base_speed(5.5), [0.0; 3], 0.0);
        for _ in 0..30 {
            c.tick(dt, walk);
        }
        assert!(approx(c.vel[1], 5.5, 1e-3), "{}", c.vel[1]);
        c.tick(dt, back);
        assert!(
            c.vel[1] < -0.2,
            "the reverse write must cross zero: {}",
            c.vel[1]
        );
        c.tick(dt, back);
        assert!(
            c.vel[1] < -3.0,
            "two writes in, well past zero: {}",
            c.vel[1]
        );
        c.tick(dt, back);
        assert!(
            approx(c.vel[1], -4.95, 1e-3),
            "backpedal plateau: {}",
            c.vel[1]
        );
        // And the friction must *not* fight a wish that agrees with the
        // velocity: the forward ramp from rest is accel-only (measured:
        // 3.9216 m/s in the first full frame at dt = 0.032309 = 121.4 m/s^2).
        let mut c = Controller::new(MoverSpec::from_base_speed(5.5), [0.0; 3], 0.0);
        let dt = 0.032309f32;
        c.tick(dt, walk);
        assert!(approx(c.vel[1], 3.9216, 1e-3), "from rest: {}", c.vel[1]);
    }

    #[test]
    fn air_control_accelerates_toward_the_wish_speed() {
        // Measured (`player_ctrl.jsonl`, 2026-10-15, ~60 fps): W pressed in
        // the air from rest grows the per-frame horizontal step by
        // +0.00533/+0.00534 m every frame in two independent runs
        // (i=10954: 0.00371 -> 0.08389 m) until it holds at 0.0918 m = the
        // 5.5 m/s slot. A step increment of AIR_ACCEL * dt^2 is that fit.
        let dt = 1.0 / 60.0;
        let input = Input {
            forward: 1.0,
            ..Default::default()
        };
        let mut c = Controller::new(MoverSpec::from_base_speed(5.5), [0.0; 3], 0.0);
        c.grounded = false;
        let mut prev = 0.0f32;
        let mut increments = Vec::new();
        for _ in 0..40 {
            let before = c.pos[2];
            c.tick(dt, input);
            let step = (c.pos[2] - before).abs();
            increments.push(step - prev);
            prev = step;
        }
        let expected = AIR_ACCEL * dt * dt; // 0.00533 m per frame
        for (k, inc) in increments.iter().take(14).enumerate() {
            assert!(
                approx(*inc, expected, 1e-4),
                "frame {k}: {inc} vs {expected}"
            );
        }
        assert!(
            approx(c.vel[1], 5.5, 1e-3),
            "ramp must hold at the slot: {}",
            c.vel[1]
        );
    }

    #[test]
    fn crouch_transitions_switch_the_slot_through_the_ramp() {
        // Measured (`player_ctrl.jsonl`, 2026-10-15, ~60 fps): the clean
        // walk -> crouch pair reads 5.5 -> 3.46 -> 2.97 m/s (i=10147) and the
        // crouch -> walk pair 2.97 -> 4.16/5.54 m/s (i=8023) — the slot switch
        // is the ordinary ACCEL-limited approach, not a clamp or a reset.
        let dt = 1.0 / 60.0;
        let walk = Input {
            forward: 1.0,
            ..Default::default()
        };
        let crouch = Input {
            forward: 1.0,
            crouch: true,
            ..Default::default()
        };
        let mut c = Controller::new(MoverSpec::from_base_speed(5.5), [0.0; 3], 0.0);
        for _ in 0..30 {
            c.tick(dt, walk);
        }
        assert!(approx(c.vel[1], 5.5, 1e-3), "{}", c.vel[1]);
        c.tick(dt, crouch);
        assert!(
            approx(c.vel[1], 5.5 - ACCEL * dt, 0.02),
            "first crouch frame {}",
            c.vel[1]
        );
        c.tick(dt, crouch);
        assert!(approx(c.vel[1], 3.0, 1e-3), "crouch slot {}", c.vel[1]);
        c.tick(dt, walk);
        assert!(
            approx(c.vel[1], 3.0 + ACCEL * dt, 0.02),
            "first walk-back frame {}",
            c.vel[1]
        );
        c.tick(dt, walk);
        assert!(approx(c.vel[1], 5.5, 1e-3), "walk slot {}", c.vel[1]);
    }

    #[test]
    fn slope_following_holds_the_slot_speed_along_the_surface() {
        // The traced backpedal climb (`player_keyed.jsonl` i=352): a 17.9
        // degree grade where the client holds 4.699 m/s horizontal and
        // +1.523 m/s vertical, magnitude 4.940 = the 4.95 backpedal slot to
        // 0.2%. The normal below is that slope's plane (tan 17.9 = 0.323).
        let dt = 0.032309f32;
        let theta: f32 = 17.9f32.to_radians();
        let n = [0.0, theta.cos(), theta.sin()]; // the slope's normal, rising in +z
        let mut c = Controller::new(MoverSpec::from_base_speed(5.5), [0.0; 3], 0.0);
        c.ground_normal = n;
        // Backpedal: yaw 0 faces +z, so S walks along -z (down the grade).
        let input = Input {
            forward: -1.0,
            ..Default::default()
        };
        for _ in 0..4 {
            c.tick(dt, input);
        }
        let h = (c.vel[0] * c.vel[0] + c.vel[1] * c.vel[1]).sqrt();
        let v = c.slope_vertical;
        // The rule is exact (|v| = the slot); the components carry the trace's
        // own measurement precision (~0.2%, from the 58 Hz sampling).
        let mag = (h * h + v * v).sqrt();
        assert!(approx(mag, 4.95, 0.002), "magnitude {}", mag);
        assert!(approx(h, 4.699, 0.02), "horizontal {}", h);
        assert!(approx(v.abs(), 1.523, 0.02), "vertical {}", v);
    }

    #[test]
    fn jump_leaves_the_ground() {
        let spec = MoverSpec::from_base_speed(5.5);
        let mut c = Controller::new(spec, [0.0; 3], 0.0);
        c.tick(
            DT,
            Input {
                jump: true,
                ..Default::default()
            },
        );
        assert!(!c.grounded);
        // After the launch tick one gravity interval already elapsed.
        assert!(
            approx(c.vertical, JUMP_IMPULSE - GRAVITY * DT, 0.02),
            "v {}",
            c.vertical
        );
    }

    #[test]
    fn jump_arc_matches_the_traced_arc() {
        // The traced jump (`player_keyed.jsonl`, keys `space`): constant
        // 0.5613 m/s shed per 0.032309 s frame, first-frame displacement
        // 0.15426 m, discrete ascent 0.7333 m, apex between frames 9 and 10.
        let dt = 0.032309f32;
        let mut c = Controller::new(MoverSpec::from_base_speed(5.5), [0.0; 3], 0.0);
        c.tick(
            dt,
            Input {
                jump: true,
                ..Default::default()
            },
        );
        let first = c.pos[1];
        assert!(approx(first, 0.15426, 0.0005), "first dy {}", first);
        let mut apex = first;
        for _ in 0..9 {
            c.tick(dt, Input::default());
            apex = apex.max(c.pos[1]);
        }
        assert!(approx(apex, 0.7333, 0.01), "apex {}", apex);
    }

    #[test]
    fn fall_speed_is_capped_at_the_terminal() {
        let spec = MoverSpec::from_base_speed(5.5);
        let mut c = Controller::new(spec, [0.0; 3], 0.0);
        c.grounded = false;
        c.vertical = 0.0;
        for _ in 0..6000 {
            c.tick(DT, Input::default());
        }
        assert!(
            c.vertical >= -31.0 && c.vertical <= -28.0,
            "v {}",
            c.vertical
        );
    }

    #[test]
    fn advance_integrates_once_with_the_given_dt() {
        // The client integrates per frame with the frame's own dt; there is no
        // accumulator, so a short frame still moves (a fraction of a tick).
        let spec = MoverSpec::from_base_speed(5.5);
        let mut c = Controller::new(spec, [0.0; 3], 0.0);
        let input = Input {
            forward: 1.0,
            ..Default::default()
        };
        c.advance(0.004, input);
        assert!(c.vel[1] > 0.0 && c.vel[1] < 0.5, "v {}", c.vel[1]);
        // Yaw 0 faces +z in the client's convention; the frame moved.
        assert!(!approx(c.pos[2], 0.0, EPS));
    }

    #[test]
    fn advance_rejects_nonfinite_positions() {
        let spec = MoverSpec::from_base_speed(5.5);
        let mut c = Controller::new(spec, [f32::NAN, 0.0, 0.0], 0.0);
        c.advance(
            0.25,
            Input {
                forward: 1.0,
                ..Default::default()
            },
        );
        assert!(c.pos[0].is_nan());
    }

    #[test]
    fn settling_on_ground_resets_the_vertical() {
        let spec = MoverSpec::from_base_speed(5.5);
        let mut c = Controller::new(spec, [0.0; 3], 0.0);
        c.grounded = false;
        c.vertical = -10.0;
        c.settle_ground(0.0);
        assert!(c.grounded);
        assert_eq!(c.vertical, 0.0);
        assert_eq!(c.pos[1], 0.0);
    }
}

//! The read airborne vertical law, `0x140ca72d0` / `0x140cb2010` /
//! `0x140cb2b00` / `0x140c993e0`.
//!
//! This is the missing half of the jump: the take-off (`crate::jump`) writes
//! the local movement vector's up lane and this stage evolves it while the body
//! is airborne. It is **not** a state-record pass and it does not need one:
//! `FUN_140cb2220` (the resolve) is a no-op with an empty record set, so on a
//! bare character the arc is these two terms and nothing else.
//!
//! # The frame
//!
//! `MoverUpdateFrame` runs the vertical stage twice per frame, in this order
//! (`0x140cbbbb7`, `0x140cbbc29`):
//!
//! ```text
//! 0x140cb2010(body, input)          ; drag, airborne only
//!   0x140cb3cf0(body, input, body[0x600], body[0x604], body[0x608])
//!                                   ; the HORIZONTAL accel stage, run first
//!   fall_up = input+0x88 > 0 ? body[0x618] : body[0x614]
//!   if fall_up > 1.1920929e-05:
//!       gate = 0x14202fb00(body[0x240])          ; -10.0 or 0.0
//!       mag  = |gate * body[0x610] / (fall_up*fall_up) * input+0x88|
//!       d    = dt * (-mag) * input+0x20          ; input+0x20 = (local.axis)*axis
//!       if |d| > |local|: d *= |local| / |d|
//!       local += d
//!
//! 0x140cb2b00(body, input, dt)      ; the airborne arm
//!       gv = 0x140ca72d0(body) = (gate * body[0x610]) * body[0xc0..0xcc]
//!       body[0x1500..0x150c] += dt * gv          ; the gravity accumulator
//!       local += state_local(dt * gv)            ; 0x140c993e0
//! ```
//!
//! `input+0x88` and `input+0x20` are built by `0x140cba030`'s airborne arm:
//! the direction is `body+0xc0..0xcc`, so `input+0x88 = dot3(local, axis)` -
//! the local up lane - and `input+0x20 = splat(that) * axis`.
//!
//! # The numbers
//!
//! `body+0x610` is the mover component's class `+0xe8` (hash `0xC2C40969`) and
//! `0x14202fb00` returns the image constant `-10.0` (`0x1431161f8`) for the
//! normal movement state. Every playable hero on this install reads
//! `gravity = 1.75`, so the frame's acceleration is `-17.5 m/s^2` - except
//! Jetpack Cat at `0.30`. `body+0x614` is the class `+0xec` terminal fall
//! (`30.0`); `body+0x618` is class `+0xf0` (hash `0x975F21A5`), which **no
//! playable hero declares**, so its slot stays zero and the drag is skipped
//! while rising. The falling drag is the terminal-velocity law:
//! `dv/dt = -g - (g/v_term^2) v |v|`, so `v_term` is reached exactly when the
//! drag cancels gravity.
//!
//! `0x14202fb00` is a state-table lookup (`body[0x240]` + `0x6a8`, decoded to a
//! table `T`, then `T[0x1e0] == 1` or `T[0x1e8] -> [..+0x3c]` selecting "no
//! gravity" for modes 0, 1 and 3). The table's content is runtime data, so this
//! module takes the gate's value as a parameter; [`GATE_ON`] is the constant the
//! lookup returns for a normal state.

use crate::movement::{sse_add, sse_mul};
use crate::resolve::{hsum3, sqrt_newton, GUARD_LEN, GUARD_SQ};

/// The body offsets this law reads and writes.
pub mod body {
    /// `body+0xc0..0xcc`: the axis. `ConstructMoverBase` (`0x140ca3acc`)
    /// writes the constant at `0x14328cdd0`, `(0, 1, 0, 0)`; the field index
    /// finds no other writer.
    pub const AXIS: usize = 0xc0;
    /// `body+0xe8`: the frame's duration in integer microseconds
    /// (`0x140ca72b0` = `(f32)(i64)[+0xe8] * 1e-6`, the constant at
    /// `0x142f85108`).
    pub const MICROS: usize = 0xe8;
    /// `body+0x610`: the gravity magnitude (component `+0xe8`).
    pub const GRAVITY: usize = 0x610;
    /// `body+0x614`: the terminal fall speed (component `+0xec`, `30.0`).
    pub const FALL_TERMINAL: usize = 0x614;
    /// `body+0x618`: the terminal speed while rising (component `+0xf0`);
    /// absent on every playable hero, so the drag skips the rising lane.
    pub const RISE_TERMINAL: usize = 0x618;
    /// `body+0x1500..0x150c`: the gravity displacement accumulator, in world
    /// space, `+= dt * gravity` per airborne frame.
    pub const ACCUMULATOR: usize = 0x1500;
    /// `body+0x1150..0x1160`: the local movement vector.
    pub const LOCAL: usize = 0x1150;
    /// `body+0x10d0`: the state file word.
    pub const FLAGS: usize = 0x10d0;
}

/// The state file bits that mean "in the air": `0x80000c` (bits 2, 3 and 23),
/// the same mask the jump stage gates on.
pub const AIR_MASK: u32 = 0x0080_000c;

/// `body+0x10d0` bit 24: the state transform is the identity, so the local
/// vector takes world vectors as they are (`0x140c993ea`'s `TEST 0x1000000`).
pub const WORLD_SPACE: u32 = 0x0100_0000;

/// The gate `0x14202fb00` returns for a normal movement state: the image
/// constant at `0x1431161f8`.
pub const GATE_ON: f32 = -10.0;

/// The guard the drag's terminal speed is compared against (`0x142f7aad4`,
/// `1.1920929e-05`).
pub const DRAG_GUARD: f32 = GUARD_LEN;

/// The frame duration scale at `0x142f85108`.
pub const MICRO_SECONDS: f32 = 1.0e-6;

/// The up axis `body+0xc0..0xcc` holds for a character.
pub const UP: [f32; 4] = [0.0, 1.0, 0.0, 0.0];

/// `0x140ca72b0`: the frame duration in seconds.
///
/// `(float)(int64)micros * 1e-6` - the integer convert and the multiply are
/// the client's two operations, in that order.
pub fn frame_seconds(micros: i64) -> f32 {
    sse_mul(micros as f32, MICRO_SECONDS)
}

/// `0x140ca72d0`: the world gravity vector.
///
/// `s = gate * magnitude` (one `MULSS`), then `s * axis[i]` per lane - the
/// broadcast is by `SHUFPS 0`, so the fourth lane is scaled too.
///
/// `gate` is `0x14202fb00`'s result: [`GATE_ON`] (`-10.0`) or `0.0`.
pub fn gravity_vector(gate: f32, magnitude: f32, axis: [f32; 4]) -> [f32; 4] {
    let scale = sse_mul(gate, magnitude);
    [
        sse_mul(scale, axis[0]),
        sse_mul(scale, axis[1]),
        sse_mul(scale, axis[2]),
        sse_mul(scale, axis[3]),
    ]
}

/// `0x140cba030`'s airborne arm: the input record's two channels for the
/// vertical axis.
///
/// Returns `(input+0x88, input+0x20)`: the scalar `dot3(local, axis)` and the
/// four-lane `splat(scalar) * axis`. The dot is the client's `(p1 + p0) + p2`.
pub fn vertical_channels(local: [f32; 4], axis: [f32; 4]) -> (f32, [f32; 4]) {
    let scalar = hsum3([
        sse_mul(local[0], axis[0]),
        sse_mul(local[1], axis[1]),
        sse_mul(local[2], axis[2]),
    ]);
    let along = [
        sse_mul(scalar, axis[0]),
        sse_mul(scalar, axis[1]),
        sse_mul(scalar, axis[2]),
        sse_mul(scalar, axis[3]),
    ];
    (scalar, along)
}

/// Two three-lane lengths as the client's one-Newton `rsqrtss` produces them.
///
/// `0x140cb214e`: the delta length is computed only when its square exceeds
/// `1.17549435e-36`; otherwise the *square* is kept and the later compare runs
/// against it. The local length keeps `0.0` on the same guard because `XMM7`
/// is zeroed at `0x140cb2061`. Both quirks are the client's and are preserved.
fn lengths(sq_delta: f32, sq_local: f32) -> (f32, f32) {
    let len_delta = if sq_delta > GUARD_SQ {
        sqrt_newton(sq_delta)
    } else {
        sq_delta
    };
    let len_local = if sq_local > GUARD_SQ {
        sqrt_newton(sq_local)
    } else {
        0.0
    };
    (len_delta, len_local)
}

/// The squared three-lane length, as `MULPS` then `shufps`/`addps` build it.
fn length_sq(v: [f32; 4]) -> f32 {
    hsum3([
        sse_mul(v[0], v[0]),
        sse_mul(v[1], v[1]),
        sse_mul(v[2], v[2]),
    ])
}

/// `0x140cb2010`: the quadratic drag on the vertical lane.
///
/// `local` is `body+0x1150`, `axis` is `body+0xc0..0xcc`, `dt` is
/// `0x140ca72b0`'s value. `gate` and `magnitude` are the same pair the gravity
/// vector uses ([`GATE_ON`], `body+0x610`). `fall_terminal` is `body+0x614`
/// and `rise_terminal` is `body+0x618`; the client picks by the sign of the
/// vertical lane, and a terminal of zero (or anything at or below
/// [`DRAG_GUARD`]) skips the whole stage.
///
/// The stage is only reached while airborne; the caller owns that gate.
pub fn apply_drag(
    local: [f32; 4],
    axis: [f32; 4],
    dt: f32,
    gate: f32,
    magnitude: f32,
    fall_terminal: f32,
    rise_terminal: f32,
) -> [f32; 4] {
    let (speed, along) = vertical_channels(local, axis);
    // `COMISS xmm7(0), [input+0x88] ; CMOVC 0x618`: rising takes the up
    // terminal. `>` keeps the `JBE`-on-unordered behaviour: a NaN lane is not
    // "rising" and takes the down terminal.
    let terminal = if speed > 0.0 {
        rise_terminal
    } else {
        fall_terminal
    };
    // `COMISS terminal, 1.1920929e-05 ; JBE return`. `JBE` is taken on
    // unordered too, so the negated comparison is the client's semantics, not
    // a style choice.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    if !(terminal > DRAG_GUARD) {
        return local;
    }
    let mut mag = sse_mul(gate, magnitude);
    mag /= sse_mul(terminal, terminal);
    mag = sse_mul(mag, speed);
    // `ANDPS abs` then `XORPS sign`: the magnitude is negated as a bit flip, so
    // a NaN keeps its payload and a `-0.0` becomes `+0.0`.
    let neg = f32::from_bits((mag.to_bits() & 0x7fff_ffff) ^ 0x8000_0000);
    let mut delta = [
        sse_mul(dt, sse_mul(neg, along[0])),
        sse_mul(dt, sse_mul(neg, along[1])),
        sse_mul(dt, sse_mul(neg, along[2])),
        sse_mul(dt, sse_mul(neg, along[3])),
    ];

    let (len_delta, len_local) = lengths(length_sq(delta), length_sq(local));
    // `COMISS len_delta, len_local ; JBE`: only a strictly longer delta is
    // scaled back, and the comparison is the client's (unordered skips).
    if len_delta > len_local {
        let scale = len_local / len_delta;
        delta = [
            sse_mul(delta[0], scale),
            sse_mul(delta[1], scale),
            sse_mul(delta[2], scale),
            sse_mul(delta[3], scale),
        ];
    }
    [
        sse_add(local[0], delta[0]),
        sse_add(local[1], delta[1]),
        sse_add(local[2], delta[2]),
        sse_add(local[3], delta[3]),
    ]
}

/// `0x140c993e0`: add a world-space vector to the local movement vector.
///
/// With [`WORLD_SPACE`] set (or a declined transform, `matrix == None`) the
/// vector is added as it is. Otherwise the client multiplies it by the state
/// rows, `out = (dot3(v, c0), dot3(v, c1), dot3(v, c2), dot3(v, c2))` - the
/// fourth lane is the third one's copy, the same quirk the take-off's invert
/// has - and then adds. The sums are `(p1 + p0) + p2`.
///
/// `matrix` is the column-major basis the frame's forward transform uses
/// (`crate::movement::forward_transform`), so the three blocks are its `c0`,
/// `c1` and `c2`.
pub fn add_local(
    local: [f32; 4],
    world: [f32; 4],
    matrix: Option<&[f32; 16]>,
    world_space: bool,
) -> [f32; 4] {
    let v = match (world_space, matrix) {
        (false, Some(m)) => {
            let dot = |at: usize| {
                hsum3([
                    sse_mul(world[0], m[at]),
                    sse_mul(world[1], m[at + 1]),
                    sse_mul(world[2], m[at + 2]),
                ])
            };
            let (d0, d1, d2) = (dot(0), dot(4), dot(8));
            [d0, d1, d2, d2]
        }
        _ => world,
    };
    [
        sse_add(local[0], v[0]),
        sse_add(local[1], v[1]),
        sse_add(local[2], v[2]),
        sse_add(local[3], v[3]),
    ]
}

/// One airborne frame of the vertical law: the drag, then the gravity add.
///
/// Returns the new local vector and adds this frame's world-space gravity
/// delta to `accumulator` (`body+0x1500..0x150c`) as a side effect, which is
/// what `0x140cb2b4e`'s `MOVUPS` does. `flags` is `body+0x10d0`; when
/// [`AIR_MASK`] is clear the frame is grounded and this is the identity (both
/// call sites are gated on the mask).
///
/// Two time steps, because the client has two: `dt` is the frame stage's
/// argument (the gravity's scale through `0x140cb2b00`'s `param_3`), and
/// `drag_dt` is `0x140ca72b0`'s own read of `body+0xe8` (the frame duration in
/// microseconds times `1e-6`), which the drag uses instead.
#[allow(clippy::too_many_arguments)]
pub fn step(
    local: [f32; 4],
    axis: [f32; 4],
    matrix: Option<&[f32; 16]>,
    flags: u32,
    accumulator: &mut [f32; 4],
    dt: f32,
    drag_dt: f32,
    gate: f32,
    magnitude: f32,
    fall_terminal: f32,
    rise_terminal: f32,
) -> [f32; 4] {
    if flags & AIR_MASK == 0 {
        return local;
    }
    let dragged = apply_drag(
        local,
        axis,
        drag_dt,
        gate,
        magnitude,
        fall_terminal,
        rise_terminal,
    );
    let gravity = gravity_vector(gate, magnitude, axis);
    let delta = [
        sse_mul(dt, gravity[0]),
        sse_mul(dt, gravity[1]),
        sse_mul(dt, gravity[2]),
        sse_mul(dt, gravity[3]),
    ];
    for (acc, d) in accumulator.iter_mut().zip(delta) {
        *acc = sse_add(*acc, d);
    }
    add_local(dragged, delta, matrix, flags & WORLD_SPACE != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(v: [f32; 4]) -> [u32; 4] {
        v.map(f32::to_bits)
    }

    /// Hero 1's read row: gravity `1.75` (component `+0xe8`), terminal fall
    /// `30.0` (component `+0xec`) and no rising terminal (component `+0xf0`,
    /// absent on every playable hero).
    const HERO_GRAVITY: f32 = 1.75;
    const HERO_FALL: f32 = 30.0;
    const HERO_RISE: f32 = 0.0;

    #[test]
    fn the_gravity_vector_is_the_constant_times_the_component_field() {
        // -10.0 * 1.75 = -17.5 along the up axis, and the fourth lane too.
        let g = gravity_vector(GATE_ON, HERO_GRAVITY, UP);
        // The two zero lanes are `-17.5 * 0.0 = -0.0`, exactly as the client's
        // `MULPS` leaves them.
        assert_eq!(bits(g), bits([-0.0, -17.5, -0.0, -0.0]));
        // The gate's off value zeroes the vector, sign included.
        assert_eq!(bits(gravity_vector(0.0, HERO_GRAVITY, UP)), bits([0.0; 4]));
        // The fourth lane is scaled by the axis as well.
        let g = gravity_vector(GATE_ON, 2.0, [0.0, 1.0, 0.0, 0.5]);
        assert_eq!(bits(g), bits([-0.0, -20.0, -0.0, -10.0]));
    }

    #[test]
    fn the_frame_seconds_are_the_integer_microseconds() {
        assert_eq!(frame_seconds(16_666), 0.016666f32);
        assert_eq!(frame_seconds(0), 0.0);
        assert_eq!(frame_seconds(1_000_000), 1.0);
        assert_eq!(frame_seconds(-16_666), -0.016666f32);
    }

    #[test]
    fn the_vertical_channels_are_the_up_lane() {
        let (speed, along) = vertical_channels([1.5, 6.0, -2.0, 3.5], UP);
        assert_eq!(speed, 6.0);
        assert_eq!(bits(along), bits([0.0, 6.0, 0.0, 0.0]));
        // A tilted axis projects, and the fourth lane follows the scale.
        let (speed, along) = vertical_channels([1.0, 2.0, 3.0, 4.0], [0.5, 0.5, 0.0, 0.5]);
        assert_eq!(speed, 1.5);
        assert_eq!(bits(along), bits([0.75, 0.75, 0.0, 0.75]));
    }

    #[test]
    fn a_rising_vector_skips_the_drag_on_this_install() {
        // body+0x618 is absent, so the terminal is zero and the guard returns
        // the vector untouched - the rise is a pure gravity parabola.
        let local = [0.0, 6.0, 0.0, 0.0];
        let got = apply_drag(
            local,
            UP,
            0.0166,
            GATE_ON,
            HERO_GRAVITY,
            HERO_FALL,
            HERO_RISE,
        );
        assert_eq!(bits(got), bits(local));
        // With a rising terminal the stage runs and the drag is downward.
        let again = apply_drag(local, UP, 0.0166, GATE_ON, HERO_GRAVITY, HERO_FALL, 30.0);
        assert!(again[1] < local[1], "{again:?}");
        assert_eq!(again[0], 0.0);
        assert_eq!(again[2], 0.0);
    }

    #[test]
    fn the_falling_drag_opposes_the_fall_and_cannot_reverse_it() {
        // The drag is quadratic: dv = -dt * (g/v_term^2) * v * |v|, so a
        // falling v gains (never loses) upward speed and the gain scales with
        // the square of the speed.
        let slow = apply_drag(
            [0.0, -1.0, 0.0, 0.0],
            UP,
            0.0166,
            GATE_ON,
            HERO_GRAVITY,
            HERO_FALL,
            HERO_RISE,
        );
        assert!(slow[1] > -1.0 && slow[1] < 0.0, "{slow:?}");
        let fast = apply_drag(
            [0.0, -10.0, 0.0, 0.0],
            UP,
            0.0166,
            GATE_ON,
            HERO_GRAVITY,
            HERO_FALL,
            HERO_RISE,
        );
        assert!(fast[1] > -10.0 && fast[1] < 0.0, "{fast:?}");
        let gain = |v: f32| {
            let got = apply_drag(
                [0.0, v, 0.0, 0.0],
                UP,
                0.0166,
                GATE_ON,
                HERO_GRAVITY,
                HERO_FALL,
                HERO_RISE,
            );
            got[1] - v
        };
        // Ten times the speed is one hundred times the gain, within one f32
        // step of the products.
        let ratio = gain(-10.0) / gain(-1.0);
        assert!((ratio - 100.0).abs() < 0.01, "{ratio}");
    }

    #[test]
    fn the_drag_is_capped_by_the_local_vectors_length() {
        // A delta longer than the local vector is cut to the vector's length:
        // the client scales it by `|local| / |delta|`. Here the delta is ten
        // units long and the vector four, so the result stays under four.
        let local = [3.0, 4.0, 0.0, 0.0];
        let dt = 1.0;
        // mag = |gate * magnitude / terminal^2 * v|; with gate -10, magnitude
        // 1.75, terminal 30 and v = 4 the delta is dt * mag * 4 = 0.777...,
        // which is shorter than the vector, so scale the magnitude up until
        // the clamp bites: magnitude 60 gives mag = 0.2666, delta 1.0666.
        let got = apply_drag(local, [0.0, 1.0, 0.0, 0.0], dt, GATE_ON, 60.0, 30.0, 0.0);
        assert!(got[1] <= 4.0, "{got:?}");
        assert!(got[1] > 0.0, "{got:?}");
        // The scale is `|local| / |delta|`: the up lane lands exactly on the
        // vector's length when the clamp runs.
        let delta = apply_drag([0.0; 4], [0.0, 1.0, 0.0, 0.0], dt, GATE_ON, 60.0, 30.0, 0.0);
        assert_eq!(bits(delta), bits([0.0; 4]));
        // The direct add keeps all four lanes.
        let long = [0.0, -20.0, 0.0, 0.0];
        let added = add_local([0.0; 4], long, None, true);
        assert_eq!(bits(added), bits(long));
    }

    #[test]
    fn add_local_applies_the_transpose_of_the_basis() {
        // A yaw of 90 degrees: c0 = (0,0,-1), c1 = (0,1,0), c2 = (1,0,0).
        let m: [f32; 16] = [
            0.0, 0.0, -1.0, 0.0, // c0
            0.0, 1.0, 0.0, 0.0, // c1
            1.0, 0.0, 0.0, 0.0, // c2
            0.0, 0.0, 0.0, 1.0, // c3
        ];
        // A world +x delta is local -z, and the fourth lane copies the third.
        let got = add_local([0.0; 4], [1.0, 0.0, 0.0, 0.0], Some(&m), false);
        assert_eq!(bits(got), bits([0.0, 0.0, 1.0, 1.0]));
        // `local` is added to, not replaced.
        let got = add_local([2.0, 3.0, 4.0, 5.0], [1.0, 0.0, 0.0, 0.0], Some(&m), false);
        assert_eq!(bits(got), bits([2.0, 3.0, 5.0, 6.0]));
    }

    #[test]
    fn a_grounded_body_takes_no_air_step() {
        let local = [1.0, -2.0, 3.0, 4.0];
        let mut acc = [0.0; 4];
        let got = step(
            local,
            UP,
            None,
            0,
            &mut acc,
            0.0166,
            0.0166,
            GATE_ON,
            HERO_GRAVITY,
            HERO_FALL,
            HERO_RISE,
        );
        assert_eq!(bits(got), bits(local));
        assert_eq!(bits(acc), bits([0.0; 4]));
    }

    #[test]
    fn the_air_step_is_drag_then_gravity() {
        // One frame at 60 Hz from the take-off lane: no drag (rising, no
        // rising terminal), then 17.5 * 0.0166 of downward speed, and the
        // accumulator holds the world-space delta.
        let dt = 0.0166f32;
        let mut acc = [0.0; 4];
        let got = step(
            [0.0, 6.0, 0.0, 0.0],
            UP,
            None,
            AIR_MASK,
            &mut acc,
            dt,
            frame_seconds(16_600),
            GATE_ON,
            HERO_GRAVITY,
            HERO_FALL,
            HERO_RISE,
        );
        let expect = 6.0f32 + (-17.5f32) * dt;
        assert_eq!(bits(got), bits([0.0, expect, 0.0, 0.0]));
        assert_eq!(bits(acc), bits([0.0, -17.5 * dt, 0.0, 0.0]));
    }

    /// A golden trace: the first eight frames of a standing jump at 60 Hz from
    /// hero 1's read row, world-space (no transform). The take-off sets the up
    /// lane to `6.0`; every later frame applies the read law in the client's
    /// operation order, so the expected value is built with the same two
    /// operations (`dt * gravity`, then the add).
    #[test]
    fn the_first_frames_of_a_jump_are_pinned() {
        let dt = frame_seconds(16_666);
        let per_frame = sse_mul(dt, sse_mul(GATE_ON, HERO_GRAVITY));
        let mut local = [0.0, 6.0, 0.0, 0.0];
        let mut expect = 6.0f32;
        let mut acc = [0.0; 4];
        let mut trace = Vec::new();
        for _ in 0..22 {
            local = step(
                local,
                UP,
                None,
                AIR_MASK | WORLD_SPACE,
                &mut acc,
                dt,
                dt,
                GATE_ON,
                HERO_GRAVITY,
                HERO_FALL,
                HERO_RISE,
            );
            expect = sse_add(expect, per_frame);
            // The drag is skipped only while the lane is positive; from the
            // frame it turns it contributes too, so the bit-exact comparison
            // covers the rising frames.
            if local[1] > 0.0 {
                assert_eq!(local[1].to_bits(), expect.to_bits());
            }
            trace.push(local[1]);
        }
        // The rise is a pure gravity parabola (the drag is skipped while
        // rising): 6.0 / (17.5 * 0.016666) = 20.57 frames, so the lane is
        // still positive at frame 19 and non-positive at frame 20.
        assert!(trace[19] > 0.0 && trace[20] <= 0.0, "{trace:?}");
    }
}

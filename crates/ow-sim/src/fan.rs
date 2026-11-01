//! The read fan solver inside the wish writer `FUN_140cb86c0`
//! (`0x140cb86c0`).
//!
//! The writer produces the input object's wish vector by searching a small
//! set of candidate directions around the mover. This module implements the
//! parts of that search that are read exactly:
//!
//! * the seed `s = dot(position - M*wish, dir)`, plus the second term
//!   `dot(query - (position - ramp), dir)` added when the state id is not the
//!   sentinel;
//! * the `+0.1` bias added to the seed before the search;
//! * the fan's speed (`min(|velocity.xyz|, max3(scale slots)) * 1.1 *
//!   (frame_time * helper)`, floored by `body+0x5cc`);
//! * the fan's radius (`max(body+0x65c, 0.35) * body+0x5b8 - 0.015`);
//! * the candidate rotation (the pentagon's 72° step and the `2π/N` step);
//! * the acceptance rules of both search passes.
//!
//! What is **not** here, because it is not read: the evaluation
//! `0x1420328c0` that scores a candidate (77 call sites, a general
//! candidate test), the two generic helpers `0x1426cd3d0` and `0x1426de990`
//! that feed the speed and the score threshold, the back-transform's role,
//! and the three per-pass record layouts. Those are inputs to this module,
//! not outputs: [`accepts_pass1`] and [`accepts_pass2`] take the evaluator's
//! score and height as arguments rather than inventing them.
//!
//! Every constant below is read from the current image. The addresses in the
//! comments are the instruction that loads it.

use crate::movement::{forward_transform, maxss, minss, sse_add, sse_mul, DIRECTION};

/// `body + 0x10d0`: the flags block. Bit 1 selects the second set of scale
/// slots and the second speed cap.
pub const FLAGS_BLOCK: usize = 0x10d0;
/// The wish vector, `body + 0x1140`.
pub const WISH: usize = 0x1140;
/// `body + 0x1150`: the four-lane velocity the speed reads.
pub const VELOCITY: usize = 0x1150;
/// `body + 0x5c8`: the turn limit in degrees (`0x140ca7400`).
pub const TURN_LIMIT: usize = 0x5c8;
/// `body + 0x5d0..0x5d8`: the speed cap triple used when flags bit 1 is clear.
pub const SPEED_CAP_CLEAR: [usize; 3] = [0x5d0, 0x5d4, 0x5d8];
/// `body + 0x5dc..0x5e4`: the speed cap triple used when flags bit 1 is set.
pub const SPEED_CAP_SET: [usize; 3] = [0x5dc, 0x5e0, 0x5e4];
/// `body + 0xe8`: the frame counter; `* 1e-6` gives seconds.
pub const FRAME_COUNTER: usize = 0xe8;
/// `body + 0x5cc`: the floor under the fan's speed.
pub const SPEED_FLOOR: usize = 0x5cc;
/// `body + 0x5b8`: the radius scale.
pub const RADIUS_SCALE: usize = 0x5b8;
/// `body + 0x65c`: the radius input, floored at `0.35`.
pub const RADIUS_INPUT: usize = 0x65c;
/// `body + 0x5bc`: the candidate count of the fine fan.
pub const FAN_COUNT: usize = 0x5bc;
/// `body + 0x610`: the first gate term (`> 0` means active).
pub const ACTIVE: usize = 0x610;

/// `0x140cb8975`: the bias added to the seed before the search.
pub const SEED_BIAS: f32 = 0.1;
/// `0x140cb884c` (`0x3f8ccccd`): the speed gain.
pub const SPEED_GAIN: f32 = 1.1;
/// `0x142f85108` (`0x358637bd`): the frame counter's seconds scale.
pub const FRAME_SECONDS: f32 = 1e-6;
/// `0x1431102c0` (`0x3fc90f77`): the turn limit's upper clamp.
pub const TURN_LIMIT_MAX: f32 = 1.570_784_4;
/// `0x142f844b8` (`0x3c8efa35`): degrees to radians.
pub const DEG_TO_RAD: f32 = 0.017_453_292;
/// `0x143110820` (`0x3c75c28f`): the radius margin and the candidate `y`.
pub const RADIUS_MARGIN: f32 = 0.015;
/// `0x143058fac` (`0x3eb33333`): the radius input's floor.
pub const RADIUS_FLOOR: f32 = 0.35;
/// `0x142f7aad0` (`0x3c80000`): the squared-length threshold below which the
/// speed is zero.
pub const LENGTH_EPSILON: f32 = f32::from_bits(0x0080_0000);
/// `0x143110824` (`0x3e9e3779`): cos(72°), the pentagon's rotation step.
pub const PENTAGON_COS: f32 = 0.309_016_97;
/// `0x143110828` (`0x3f737871`): sin(72°).
pub const PENTAGON_SIN: f32 = 0.951_056_54;
/// `0x142f88c18` (`0x40c90fdb`): the fine fan's full turn.
pub const TWO_PI: f32 = 6.283_185_5;

#[inline]
fn f32_at(bytes: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

#[inline]
fn lanes_at(bytes: &[u8], offset: usize) -> [f32; 4] {
    [
        f32_at(bytes, offset),
        f32_at(bytes, offset + 4),
        f32_at(bytes, offset + 8),
        f32_at(bytes, offset + 12),
    ]
}

/// `0x140ca7400`: `clamp(body[0x5c8] * (π/180), 0, 1.5707844)`.
///
/// The client forms `minss(v, 1.5707844)` then `maxss(that, 0)`, so a NaN
/// takes the clamp operand.
pub fn turn_limit(body: &[u8]) -> f32 {
    let v = sse_mul(f32_at(body, TURN_LIMIT), DEG_TO_RAD);
    let v = minss(v, TURN_LIMIT_MAX);
    maxss(v, 0.0)
}

/// `0x140ca7430`: the maximum of the three cap slots, selected by flags bit 1.
///
/// The client compares `a >= b`, `a >= c`, then `b >= a`, `b >= c`, and
/// returns `c` when neither pair holds. `COMISS` + `SETNC` treats an
/// unordered comparison as false, so a NaN never wins and the last slot is
/// the fallback.
pub fn speed_cap(body: &[u8], flag_bit_set: bool) -> f32 {
    let slots = if flag_bit_set {
        SPEED_CAP_SET
    } else {
        SPEED_CAP_CLEAR
    };
    let a = f32_at(body, slots[0]);
    let b = f32_at(body, slots[1]);
    let c = f32_at(body, slots[2]);
    let ge = |x: f32, y: f32| !x.is_nan() && !y.is_nan() && x >= y;
    if ge(a, b) && ge(a, c) {
        a
    } else if ge(b, a) && ge(b, c) {
        b
    } else {
        c
    }
}

/// `0x140ca72b0`: `(f32)(i64)body[0xe8] * 1e-6` — the frame counter in
/// seconds.
pub fn frame_time(body: &[u8]) -> f32 {
    let ticks = i64::from_le_bytes(body[FRAME_COUNTER..FRAME_COUNTER + 8].try_into().unwrap());
    sse_mul(ticks as f32, FRAME_SECONDS)
}

/// The speed the fan's radius uses, read at `0x140cb87d6..0x140cb8874`.
///
/// ```text
/// len2 = (y*y + x*x) + z*z                       ; body[0x1150].xyz
/// len  = len2 > 1.17549435e-36
///        ? len2 * ((3 - len2*rcp*rcp) * (rcp*0.5))  with rcp = rsqrtss(len2)
///        : 0
/// speed = minss(len, speed_cap)
/// speed = speed * 1.1
/// speed = speed * (frame_time * helper_e)
/// speed = maxss(speed, body[0x5cc])
/// ```
///
/// The length is the client's `rsqrtss` plus one Newton step, not `sqrt`, so
/// the last bits are the approximation's.
pub fn fan_speed(body: &[u8], flag_bit_set: bool, helper_e: f32) -> f32 {
    let v = lanes_at(body, VELOCITY);
    let x = sse_mul(v[0], v[0]);
    let y = sse_mul(v[1], v[1]);
    let z = sse_mul(v[2], v[2]);
    let len2 = sse_add(sse_add(y, x), z);
    let len = if len2 > LENGTH_EPSILON {
        let rcp = 1.0f32 / len2.sqrt();
        let rcp2 = sse_mul(rcp, rcp);
        let half = sse_mul(rcp, 0.5);
        let inner = sse_mul(sse_add(3.0, -sse_mul(len2, rcp2)), half);
        sse_mul(len2, inner)
    } else {
        0.0
    };
    let capped = minss(len, speed_cap(body, flag_bit_set));
    let gained = sse_mul(capped, SPEED_GAIN);
    let scaled = sse_mul(gained, sse_mul(frame_time(body), helper_e));
    maxss(scaled, f32_at(body, SPEED_FLOOR))
}

/// The fan's radius, read at `0x140cb927a..0x140cb92e2`:
/// `maxss(body[0x65c], 0.35) * body[0x5b8] - 0.015`.
pub fn fan_radius(body: &[u8]) -> f32 {
    let base = maxss(f32_at(body, RADIUS_INPUT), RADIUS_FLOOR);
    sse_add(sse_mul(base, f32_at(body, RADIUS_SCALE)), -RADIUS_MARGIN)
}

/// The seed of the search, read at `0x140cb8766..0x140cb87ba`:
/// `((dy*diry) + (dx*dirx)) + (dz*dirz)` where `d = position - M*wish`.
///
/// `matrix` is the resolved state transform; `None` is the client's
/// no-transform path, where the wish is used as it stands.
pub fn seed(body: &[u8], position: [f32; 4], matrix: Option<&[f32; 16]>) -> f32 {
    let mut wish = lanes_at(body, WISH);
    if let Some(m) = matrix {
        forward_transform(&mut wish, m);
    }
    let dir = lanes_at(body, DIRECTION);
    let mut acc = 0.0f32;
    // The client multiplies lane-wise, then adds y, x, z in that order.
    let mut lanes = [0.0f32; 4];
    for lane in 0..4 {
        lanes[lane] = sse_mul(sse_add(position[lane], -wish[lane]), dir[lane]);
    }
    acc = sse_add(acc, sse_add(lanes[1], lanes[0]));
    acc = sse_add(acc, lanes[2]);
    acc
}

/// The second seed term, read at `0x140cb88f9..0x140cb892c`:
/// `dot(query - (position - ramp), dir)`, added to the seed when the state id
/// is not the sentinel.
pub fn seed_state_term(body: &[u8], position: [f32; 4], ramp: [f32; 4], query: [f32; 4]) -> f32 {
    let dir = lanes_at(body, DIRECTION);
    let mut lanes = [0.0f32; 4];
    for lane in 0..4 {
        let base = sse_add(position[lane], -ramp[lane]);
        lanes[lane] = sse_mul(sse_add(query[lane], -base), dir[lane]);
    }
    sse_add(sse_add(lanes[1], lanes[0]), lanes[2])
}

/// The seed as the search uses it: `s + 0.1` (`0x140cb8975`).
pub fn biased_seed(seed: f32) -> f32 {
    sse_add(seed, SEED_BIAS)
}

/// Rotate a 2D pair by `(cos, sin)`, in the client's operation order
/// (`0x140cb983c..0x140cb9863`).
///
/// ```text
/// a' = a*cos - b*sin
/// b' = a*sin + b*cos
/// ```
pub fn rotate_pair(pair: [f32; 2], cos: f32, sin: f32) -> [f32; 2] {
    let [a, b] = pair;
    let as_ = sse_mul(a, sin);
    let bc = sse_mul(b, cos);
    let ac = sse_mul(a, cos);
    let bs = sse_mul(b, sin);
    [sse_add(ac, -bs), sse_add(as_, bc)]
}

/// The pentagon fan's offsets: the start pair rotated by 72° four times, read
/// from the loop at `0x140cb9842` (five candidates, `ebx < 5`).
pub fn pentagon_offsets(start: [f32; 2]) -> [[f32; 2]; 5] {
    let mut out = [[0.0f32; 2]; 5];
    let mut pair = start;
    for slot in out.iter_mut() {
        *slot = pair;
        pair = rotate_pair(pair, PENTAGON_COS, PENTAGON_SIN);
    }
    out
}

/// The fine fan's step angle: `2π / N`, `N = body[0x5bc]`
/// (`0x140cb9322..0x140cb9332`).
///
/// Returns `None` for a non-positive count, which is the client's own
/// `jle` guard before the loop.
pub fn ring_step(body: &[u8]) -> Option<f32> {
    let n = i32::from_le_bytes(body[FAN_COUNT..FAN_COUNT + 4].try_into().unwrap());
    if n <= 0 {
        return None;
    }
    Some(TWO_PI / n as f32)
}

/// The fine fan's offsets: the start pair rotated by the step angle `n` times.
pub fn ring_offsets(start: [f32; 2], n: usize) -> Vec<[f32; 2]> {
    if n == 0 {
        return Vec::new();
    }
    let (sin, cos) = (TWO_PI / n as f32).sin_cos();
    let mut out = Vec::with_capacity(n);
    let mut pair = start;
    for _ in 0..n {
        out.push(pair);
        pair = rotate_pair(pair, cos, sin);
    }
    out
}

/// The first pass's acceptance, read at `0x140cb90f1..0x140cb91d8`:
/// `|score| < best_score` **and** `y > best_y`, both strict.
///
/// The pass starts with `best_score = +FLT_MAX` and `best_y = -FLT_MAX`
/// (`0x142f844d8` / `0x142fb8e58`), so the first candidate that passes the
/// evaluator always wins.
pub fn accepts_pass1(score: f32, y: f32, best_score: f32, best_y: f32) -> bool {
    score.abs() < best_score && y > best_y
}

/// The fine pass's acceptance, read at `0x140cb9be5..0x140cb9c16`:
/// `|score| < best_score` only — it improves the first pass's best and does
/// not test the height.
pub fn accepts_pass2(score: f32, best_score: f32) -> bool {
    score.abs() < best_score
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(len: usize) -> Vec<u8> {
        vec![0u8; len]
    }

    fn put(body: &mut [u8], offset: usize, value: f32) {
        body[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    #[test]
    fn turn_limit_converts_and_clamps() {
        let mut b = body(0x1100);
        put(&mut b, TURN_LIMIT, 45.0);
        assert_eq!(turn_limit(&b), 45.0 * DEG_TO_RAD);
        put(&mut b, TURN_LIMIT, 1000.0);
        assert_eq!(turn_limit(&b), TURN_LIMIT_MAX);
        put(&mut b, TURN_LIMIT, -5.0);
        assert_eq!(turn_limit(&b), 0.0);
    }

    #[test]
    fn speed_cap_takes_the_maximum_of_three() {
        let mut b = body(0x1100);
        put(&mut b, SPEED_CAP_CLEAR[0], 1.0);
        put(&mut b, SPEED_CAP_CLEAR[1], 3.0);
        put(&mut b, SPEED_CAP_CLEAR[2], 2.0);
        assert_eq!(speed_cap(&b, false), 3.0);
        put(&mut b, SPEED_CAP_SET[0], 4.0);
        put(&mut b, SPEED_CAP_SET[1], 5.0);
        put(&mut b, SPEED_CAP_SET[2], 6.0);
        assert_eq!(speed_cap(&b, true), 6.0);
    }

    #[test]
    fn speed_cap_falls_back_to_the_last_slot_on_nan() {
        let mut b = body(0x1100);
        put(&mut b, SPEED_CAP_CLEAR[0], f32::NAN);
        put(&mut b, SPEED_CAP_CLEAR[1], 3.0);
        put(&mut b, SPEED_CAP_CLEAR[2], 2.0);
        // a is unordered, b wins both comparisons: the client's second test
        // is `b >= a` (false for NaN) so it falls through to c.
        assert_eq!(speed_cap(&b, false), 2.0);
    }

    #[test]
    fn frame_time_is_ticks_times_a_microsecond() {
        let mut b = body(0x1100);
        b[FRAME_COUNTER..FRAME_COUNTER + 8].copy_from_slice(&16_666i64.to_le_bytes());
        assert_eq!(frame_time(&b), 16_666.0 * FRAME_SECONDS);
    }

    #[test]
    fn fan_speed_uses_the_velocity_length_and_the_floor() {
        let mut b = body(0x1200);
        put(&mut b, VELOCITY, 3.0);
        put(&mut b, SPEED_CAP_CLEAR[0], 10.0);
        put(&mut b, SPEED_CAP_CLEAR[1], 10.0);
        put(&mut b, SPEED_CAP_CLEAR[2], 10.0);
        b[FRAME_COUNTER..FRAME_COUNTER + 8].copy_from_slice(&1_000_000i64.to_le_bytes());
        // len = 3, cap = 10, gain 1.1, frame 1.0 s, helper 1.0 -> 3.3
        let speed = fan_speed(&b, false, 1.0);
        assert!((speed - 3.3).abs() < 1e-5, "{speed}");
        // A zero velocity falls to the floor.
        put(&mut b, VELOCITY, 0.0);
        put(&mut b, VELOCITY + 4, 0.0);
        put(&mut b, VELOCITY + 8, 0.0);
        put(&mut b, SPEED_FLOOR, 0.5);
        assert_eq!(fan_speed(&b, false, 1.0), 0.5);
    }

    #[test]
    fn fan_radius_floors_scales_and_shrinks() {
        let mut b = body(0x1100);
        put(&mut b, RADIUS_INPUT, 0.1);
        put(&mut b, RADIUS_SCALE, 2.0);
        // max(0.1, 0.35) * 2 - 0.015
        assert_eq!(fan_radius(&b), 0.35 * 2.0 - RADIUS_MARGIN);
        put(&mut b, RADIUS_INPUT, 1.0);
        assert_eq!(fan_radius(&b), 2.0 - RADIUS_MARGIN);
    }

    #[test]
    fn seed_projects_the_delta_onto_the_direction() {
        let mut b = body(0x1200);
        put(&mut b, DIRECTION, 1.0);
        put(&mut b, DIRECTION + 4, 1.0);
        put(&mut b, DIRECTION + 8, 1.0);
        put(&mut b, WISH, 1.0);
        put(&mut b, WISH + 4, 2.0);
        put(&mut b, WISH + 8, 3.0);
        let position = [10.0, 10.0, 10.0, 0.0];
        // delta = (9, 8, 7); dot with (1,1,1) = 24
        assert_eq!(seed(&b, position, None), 24.0);
    }

    #[test]
    fn seed_uses_the_transformed_wish() {
        let mut b = body(0x1200);
        put(&mut b, DIRECTION, 1.0);
        put(&mut b, WISH, 1.0);
        let mut m = [0.0f32; 16];
        m[0] = 1.0;
        m[5] = 1.0;
        m[10] = 1.0;
        m[12] = 100.0;
        let position = [5.0, 0.0, 0.0, 0.0];
        // M*wish = 101; delta.x = -96
        assert_eq!(seed(&b, position, Some(&m)), -96.0);
    }

    #[test]
    fn state_term_subtracts_the_ramp_before_projecting() {
        let mut b = body(0x1200);
        put(&mut b, DIRECTION, 1.0);
        let position = [1.0, 0.0, 0.0, 0.0];
        let ramp = [0.5, 0.0, 0.0, 0.0];
        let query = [2.0, 0.0, 0.0, 0.0];
        // 2 - (1 - 0.5) = 1.5
        assert_eq!(seed_state_term(&b, position, ramp, query), 1.5);
    }

    #[test]
    fn biased_seed_adds_a_tenth() {
        assert_eq!(biased_seed(0.0), 0.1);
    }

    #[test]
    fn rotate_pair_is_the_client_rotation() {
        let rotated = rotate_pair([1.0, 0.0], 0.0, 1.0);
        assert_eq!(rotated, [0.0, 1.0]);
        let rotated = rotate_pair([1.0, 0.0], 1.0, 0.0);
        assert_eq!(rotated, [1.0, 0.0]);
    }

    #[test]
    fn pentagon_rotates_by_72_degrees_five_times() {
        let offsets = pentagon_offsets([1.0, 0.0]);
        assert_eq!(offsets[0], [1.0, 0.0]);
        // One 72° step from (1, 0) is (cos 72°, sin 72°).
        assert!((offsets[1][0] - PENTAGON_COS).abs() < 1e-6);
        assert!((offsets[1][1] - PENTAGON_SIN).abs() < 1e-6);
        // Five 72° steps return to the start.
        let back = rotate_pair(offsets[4], PENTAGON_COS, PENTAGON_SIN);
        assert!((back[0] - 1.0).abs() < 1e-5);
        assert!(back[1].abs() < 1e-5);
    }

    #[test]
    fn ring_step_is_two_pi_over_the_count() {
        let mut b = body(0x1100);
        b[FAN_COUNT..FAN_COUNT + 4].copy_from_slice(&8i32.to_le_bytes());
        assert_eq!(ring_step(&b), Some(TWO_PI / 8.0));
        b[FAN_COUNT..FAN_COUNT + 4].copy_from_slice(&0i32.to_le_bytes());
        assert_eq!(ring_step(&b), None);
    }

    #[test]
    fn ring_offsets_wrap_after_a_full_turn() {
        let offsets = ring_offsets([1.0, 0.0], 4);
        assert_eq!(offsets.len(), 4);
        let back = rotate_pair(offsets[3], (TWO_PI / 4.0).cos(), (TWO_PI / 4.0).sin());
        assert!((back[0] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn the_first_pass_needs_a_better_score_and_a_higher_y() {
        assert!(accepts_pass1(1.0, 0.0, f32::MAX, -f32::MAX));
        assert!(accepts_pass1(-0.5, 1.0, 1.0, 0.0));
        assert!(!accepts_pass1(1.0, 1.0, 1.0, 0.0)); // score not strictly lower
        assert!(!accepts_pass1(0.5, 0.0, 1.0, 0.0)); // y not strictly higher
    }

    #[test]
    fn the_fine_pass_only_needs_a_better_score() {
        assert!(accepts_pass2(-0.5, 1.0));
        assert!(!accepts_pass2(1.0, 1.0));
    }
}

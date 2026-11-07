//! The per-frame movement-vector resolve, `FUN_140cb2220` (`0x140cb2220`),
//! read from `Overwatch.exe`.
//!
//! This is the only per-frame producer of `body+0x1150`. It is **not** a
//! gravity integrator: it is a property chain that re-spaces the vector, a
//! lateral cap, and (behind a virtual gate) two record passes that are still
//! open. What is here is phases 1-4, which are read end to end and verified.
//!
//! ```text
//! sq   = hsum3(v * v)                       ((p1+p0)+p2, lanes 0..2 only)
//! len  = sqrt via RSQRTSS + one Newton step
//! if sq > 1.17549435e-36 && len > 1.1920929e-05:
//!     v *= property(0x22, len) / len
//! sq   = hsum3((v.x, 0, v.z, v.w) squared)   <- the y lane is masked out
//! len  = the same sequence
//! if sq > 1.17549435e-36 && len > 1.1920929e-05:
//!     v.x *= property(0x23, len) / len;  v.z *= property(0x23, len) / len
//! v.y  = property(0x24, v.y)                (the y lane alone)
//! cap  = body+0x60c
//! if cap > 1.1920929e-05:
//!     y  = v.y / cap
//!     y  = property(0x25, y)                (used only when the query applied)
//!     v.y = y * cap
//! ```
//!
//! Phase 5 (the `0x40`-byte record passes, `mode 1` then `mode 0`, both behind
//! `(*body->vtable)[0xa8](body) != 2`) and the closing landing test are
//! documented in `docs/formats/character-controller.md`, "The per-frame vector
//! resolve", and are not implemented: their hit path is a five-way plane
//! projection dispatch that is only read at the skeleton level.

use crate::modifiers::apply_property_records;
use crate::movement::{sse_add, sse_mul};

/// The body's live byte span.
pub const BODY_LEN: usize = 0x1c00;

/// The guard the client compares the **squared** length against
/// (`0x142f7aad0` = `0x03c80000`, `1.17549435e-36`).
pub const GUARD_SQ: f32 = f32::from_bits(0x03c8_0000);
/// The guard it compares the **square root** against (`0x142f7aad4` =
/// `0x37480000`, `1.1920929e-05`).
pub const GUARD_LEN: f32 = f32::from_bits(0x3748_0000);

/// The body offsets this stage touches.
pub mod body {
    /// The direction the world lane is measured along, and the axis the
    /// take-off impulse rises by (`(0,1,0,0)` from `ConstructMoverBase`).
    pub const DIRECTION: usize = 0xc0;
    /// The frame id the record windows are tested against.
    pub const FRAME_ID: usize = 0xe4;
    /// The take-off lane; here it is the lateral cap, not an impulse.
    pub const TAKEOFF_LANE: usize = 0x60c;
    /// The 0x38-byte record array and its count.
    pub const RECORDS: usize = 0x870;
    pub const RECORD_COUNT: usize = 0x878;
    /// The state file word.
    pub const FLAGS: usize = 0x10d0;
    /// The local movement vector.
    pub const VECTOR: usize = 0x1150;
}

/// `RSQRTSS` as the client's machine computes it.
///
/// Measured against the image in the oracle: `f32(1.0 / f32(sqrt(x)))`, a
/// float sqrt rounded first and a float reciprocal rounded second. 220 of 220
/// sampled inputs match this form; the double-precision forms
/// (`f32(1/sqrt(x))` from a `f64` sqrt) differ in 44 of 220 because they round
/// once instead of twice. Grabbing the value from the emulator is what settled
/// this - the hardware approximation a real `RSQRTSS` produces is *not* what
/// the oracle executes.
pub fn rsqrtss(x: f32) -> f32 {
    1.0 / x.sqrt()
}

/// The client's one-Newton-step square root, `0x140cb232e..0x140cb2357`.
///
/// `sq * ((3 - sq*r*r) * (0.5*r))` with `r = RSQRTSS(sq)`, every operation in
/// `f32` and in the client's order. It is not `sq.sqrt()` in general, which is
/// why the sequence is reproduced instead of replaced.
pub fn sqrt_newton(sq: f32) -> f32 {
    let r = rsqrtss(sq);
    let rr = sse_mul(r, r);
    let half_r = sse_mul(r, 0.5);
    let t = sse_mul(sq, rr);
    let u = 3.0 - t;
    let v = sse_mul(u, half_r);
    sse_mul(sq, v)
}

/// `hsum3` of the lanes squared: `((p1 + p0) + p2)`.
fn length_sq(v: [f32; 4]) -> f32 {
    sse_add(
        sse_add(sse_mul(v[1], v[1]), sse_mul(v[0], v[0])),
        sse_mul(v[2], v[2]),
    )
}

/// The horizontal sum of three lanes: `((p1 + p0) + p2)`, which is the
/// client's `SHUFPS 0` / `0x55` / `0xaa` pair of `ADDPS`.
pub fn hsum3(p: [f32; 3]) -> f32 {
    sse_add(sse_add(p[1], p[0]), p[2])
}

/// Phases 1-4 of `FUN_140cb2220` on the local movement vector.
///
/// `cap` is `body+0x60c`. `property(kind, value)` is the record query the
/// client makes through `FUN_140cb3990` - [`apply_property_records`] with the
/// body's record array and frame id - and its second element is the client's
/// `al`, which only the `0x25` call tests.
pub fn resolve_local(
    vector: [f32; 4],
    cap: f32,
    property: &mut dyn FnMut(u32, f32) -> (f32, bool),
) -> [f32; 4] {
    let mut v = vector;

    // Phase 1, `0x140cb2302..0x140cb2384`: the squared-length guard first, then
    // the square-root guard, then the scale by `property(0x22) / len`.
    let sq = length_sq(v);
    if sq > GUARD_SQ {
        let len = sqrt_newton(sq);
        if len > GUARD_LEN {
            let scale = property(0x22, len).0 / len;
            for lane in v.iter_mut() {
                *lane = sse_mul(*lane, scale);
            }
        }
    }

    // Phase 2, `0x140cb2388..0x140cb2451`: property `0x23` against the length of
    // `(x, 0, z, w)` - the shuffle dance at `0x140cb2399..0x140cb23b5` builds
    // exactly that, with the **y lane zeroed**. So the length is `x^2 + z^2`,
    // the y lane is not scaled, and the assembled vector keeps its phase-1 y.
    // The dance is not the identity: dropping the `movss xmm7,xmm6` at
    // `0x140cb2399` makes it look like one, which is how the first cut of this
    // function got y wrong.
    let masked = hsum3([sse_mul(v[0], v[0]), 0.0, sse_mul(v[2], v[2])]);
    if masked > GUARD_SQ {
        let len = sqrt_newton(masked);
        if len > GUARD_LEN {
            let scale = property(0x23, len).0 / len;
            v[0] = sse_mul(v[0], scale);
            v[2] = sse_mul(v[2], scale);
        }
    }

    // Phase 3, `0x140cb2468`: property `0x24` on the y lane alone. The result
    // is used whether or not a record matched.
    v[1] = property(0x24, v[1]).0;

    // Phase 4, `0x140cb246d..0x140cb24f5`: the lateral cap. The y lane is
    // divided by the take-off lane, property `0x25` acts on that quotient, and
    // the lane is multiplied back - so a record can only move y, and only when
    // its query applied.
    if cap > GUARD_LEN {
        let y = v[1] / cap;
        let (scaled, applied) = property(0x25, y);
        if applied {
            v[1] = sse_mul(scaled, cap);
        }
    }

    v
}

/// The closing landing test of `0x140cb260e`: `MoverOnLanding` runs when the
/// world lane of the resolved vector rose by more than `0.00400000019`
/// (`0x3b83126f`) above the vector the frame started with.
pub fn landing_rise(before: [f32; 4], after: [f32; 4], direction: [f32; 4]) -> bool {
    let before_along = hsum3([
        sse_mul(direction[0], before[0]),
        sse_mul(direction[1], before[1]),
        sse_mul(direction[2], before[2]),
    ]);
    let after_along = hsum3([
        sse_mul(direction[0], after[0]),
        sse_mul(direction[1], after[1]),
        sse_mul(direction[2], after[2]),
    ]);
    after_along - before_along > f32::from_bits(0x3b83_126f)
}

/// The record query closure over a body: `FUN_140cb3990` with the body's own
/// record array and frame id.
pub fn body_properties(records: &[u8], frame_id: u32) -> impl FnMut(u32, f32) -> (f32, bool) + '_ {
    move |kind, value| apply_property_records(records, frame_id, kind as i32, value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_kind: u32, value: f32) -> (f32, bool) {
        (value, false)
    }

    /// With no record applied, `property(len) / len` is exactly `1.0`, so the
    /// phases are exact no-ops whatever the square root returned - which is why
    /// the guard constants and the sequence matter only when a record matches.
    #[test]
    fn no_record_leaves_the_vector_untouched() {
        let v = [1.0f32, 2.0, 3.0, 4.0];
        let mut p = none;
        assert_eq!(resolve_local(v, 6.0, &mut p), v);
    }

    /// A property that returns exactly what it was given also leaves the
    /// vector alone, bit for bit.
    #[test]
    fn identity_properties_are_exact() {
        let v = [0.1f32, -2.5, 1e-3, 7.0];
        let mut p = |_k: u32, x: f32| (x, true);
        assert_eq!(resolve_local(v, 6.0, &mut p), v);
    }

    /// `property(0x22)` is applied as a *ratio to the length*, so assigning a
    /// length of `2` to a vector of length `3` scales every lane by
    /// `2 / length`, where `length` is the client's Newton root - which is one
    /// ulp below `3.0`, so the answer is one ulp above `2.0`. That is the law,
    /// not a rounding nuisance.
    #[test]
    fn property_22_scales_to_the_given_length() {
        let v = [3.0f32, 0.0, 0.0, 0.0];
        let mut p = |k: u32, x: f32| if k == 0x22 { (2.0, true) } else { (x, true) };
        let out = resolve_local(v, 6.0, &mut p);
        let expected = 3.0 * (2.0 / sqrt_newton(9.0));
        assert_eq!(out, [expected, 0.0, 0.0, 0.0]);
        assert_eq!(out[0], f32::from_bits(0x4000_0001));
    }

    /// Phase 3 touches the y lane and nothing else.
    #[test]
    fn property_24_rewrites_only_the_y_lane() {
        let v = [1.0f32, 2.0, 3.0, 4.0];
        let mut p = |k: u32, x: f32| if k == 0x24 { (5.0, true) } else { (x, true) };
        assert_eq!(resolve_local(v, 6.0, &mut p), [1.0, 5.0, 3.0, 4.0]);
    }

    /// Phase 4 divides y by the cap, lets `0x25` act, and multiplies back. With
    /// a cap of `6` and a `0x25` that doubles the quotient, y goes from `3` to
    /// `(3/6*2)*6 = 6`.
    #[test]
    fn property_25_acts_inside_the_cap() {
        let v = [1.0f32, 3.0, 2.0, 4.0];
        let mut p = |k: u32, x: f32| {
            if k == 0x25 {
                (x * 2.0, true)
            } else {
                (x, true)
            }
        };
        assert_eq!(resolve_local(v, 6.0, &mut p), [1.0, 6.0, 2.0, 4.0]);
    }

    /// The `0x25` result is used only when the query applied: the client tests
    /// `al` at `0x140cb24b9` and keeps the pre-cap vector otherwise.
    #[test]
    fn property_25_is_ignored_when_the_query_did_not_apply() {
        let v = [1.0f32, 3.0, 2.0, 4.0];
        let mut p = |k: u32, x: f32| if k == 0x25 { (99.0, false) } else { (x, true) };
        assert_eq!(resolve_local(v, 6.0, &mut p), v);
    }

    /// A zero or negative cap skips phase 4 entirely (`COMISS` + `JBE`).
    #[test]
    fn a_cap_below_the_guard_skips_the_lateral_stage() {
        let v = [1.0f32, 3.0, 2.0, 4.0];
        let mut p = |k: u32, x: f32| if k == 0x25 { (0.0, true) } else { (x, true) };
        assert_eq!(resolve_local(v, 0.0, &mut p), v);
        assert_eq!(resolve_local(v, 1.0e-6, &mut p), v);
        // A cap exactly at the guard is still skipped: the compare is `JBE`.
        assert_eq!(resolve_local(v, GUARD_LEN, &mut p), v);
    }

    /// A vector shorter than the guard is not rescaled, so a record that would
    /// have changed it is not consulted at all.
    #[test]
    fn a_short_vector_skips_the_length_stages() {
        let v = [1.0e-30f32, 0.0, 0.0, 0.0];
        let mut seen = Vec::new();
        let mut p = |k: u32, x: f32| {
            seen.push(k);
            (x, true)
        };
        resolve_local(v, 0.0, &mut p);
        // Only 0x24 is consulted; the 0x22/0x23 stages were guarded out.
        assert_eq!(seen, vec![0x24]);
    }

    /// The order is `0x22`, `0x23`, `0x24`, `0x25`, which is what makes a
    /// record chain behave as the client's does.
    #[test]
    fn the_property_order_is_the_clients() {
        let mut seen = Vec::new();
        let mut p = |k: u32, x: f32| {
            seen.push(k);
            (x, true)
        };
        resolve_local([1.0, 2.0, 3.0, 4.0], 6.0, &mut p);
        assert_eq!(seen, vec![0x22, 0x23, 0x24, 0x25]);
    }

    /// `RSQRTSS` is `1 / f32_sqrt`, not a hardware approximation and not a
    /// double-precision recap: `RSQRTSS(2)` in the oracle is `0.7071067690849304`,
    /// which is the f32 `1/sqrt(2)`, while `f32(1/sqrt(2))` from a `f64` root is
    /// `0.70710677`.
    #[test]
    fn rsqrt_is_the_oracles_value() {
        assert_eq!(rsqrtss(2.0), f32::from_bits(0x3f35_04f3));
        assert_eq!(rsqrtss(4.0), 0.5);
        assert_eq!(rsqrtss(1.0), 1.0);
        assert_eq!(rsqrtss(0.0), f32::INFINITY);
    }

    /// The sequence is **not** `sq.sqrt()`: on `9.0` it returns one ulp below
    /// `3.0`. Measured against the image in the oracle (a 74-input sample
    /// through a hand-assembled copy of the client's own instruction sequence),
    /// this function and the oracle agree bit for bit, so the sequence is
    /// reproduced rather than replaced by `sqrt`.
    #[test]
    fn the_newton_root_reproduces_the_law() {
        // One ulp below 3.0 - the sequence's own result, not an error.
        assert_eq!(sqrt_newton(9.0), f32::from_bits(0x403f_ffff));
        assert_ne!(sqrt_newton(9.0), 9.0f32.sqrt());
        assert_eq!(sqrt_newton(1.0), 1.0);
        assert_eq!(sqrt_newton(4.0), 2.0);
        // The oracle's `RSQRTSS(2)` is `1/f32_sqrt(2)`, so the composed root of
        // 2 lands on the correctly rounded value.
        assert_eq!(sqrt_newton(2.0), std::f32::consts::SQRT_2);
    }

    /// The landing test is a rise of more than `0.004` along the direction.
    #[test]
    fn the_landing_test_is_a_rise() {
        let up = [0.0f32, 1.0, 0.0, 0.0];
        assert!(!landing_rise(
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 1.003, 0.0, 0.0],
            up
        ));
        assert!(landing_rise(
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 1.005, 0.0, 0.0],
            up
        ));
        assert!(!landing_rise(
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.5, 0.0, 0.0],
            up
        ));
    }
}

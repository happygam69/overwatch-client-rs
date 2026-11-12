//! The client's character-controller resolve, read from `Overwatch.exe`.
//!
//! The chain is four bodies, in this order:
//!
//! ```text
//! MoverAdvance 0x140ca7f30
//!   -> builds the sweep block and calls RunCharacterControllerSweep
//! CollisionSolver 0x142cf3f80            (the character path)
//!   -> the pair walk fills the contact planes
//!   -> step_stage 0x142cf5020            (the planes push the target out)
//!   -> CharacterPositionResolve 0x142cd5270
//!        -> CharacterBodyUpdate 0x142cd4610
//!             -> the narrowphase gives each contact a factor
//!             -> the motion is scaled by the smallest factor
//! ```
//!
//! # The sweep block (`0x140ca7f30` fills it, `0x142cbcee0` copies it)
//!
//! `MoverAdvance` builds a block and `RunCharacterControllerSweep` copies it
//! into the one the solver reads, so there are two layouts and the copy
//! shifts the tail by `0x10`:
//!
//! | the advance's block | the solver's block | value |
//! |---|---|---|
//! | `+0x00` | `+0x00` | the frame's **displacement** (four lanes) |
//! | - | `+0x10` | zero |
//! | `+0x10` | `+0x20` | the frame duration |
//! | `+0x14` | `+0x24` | the push limit: `_DAT_143872ebc` (1.0) times `body+0x628` times the frame duration |
//! | `+0x18` | `+0x28` | the epsilon, `0.005` (`0x3ba3d70a`) |
//! | `+0x1c` | `+0x2c` | the iteration cap: `min(step_state[0x94], body+0x1a88)` |
//! | `+0x20` | `+0x30` | one byte from `step_state[0x9a]` |
//!
//! The solver's requested position is the **displacement added to the body's
//! current position**: `requested = old + block[0x00..0x10]` (`0x142cf4229`,
//! `0x142cf422d`; the block's `+0x10` is zero, so the `dt * block[+0x10]` term
//! the same two instructions multiply by contributes nothing).
//!
//! # The step stage (`0x142cf5020`)
//!
//! A leaf: it calls nothing. It walks the contact planes and pushes the
//! requested position out of every plane it penetrates, until the push is
//! smaller than the epsilon or the 200-iteration cap is reached.
//!
//! The plane record is `0x40` bytes: the normal and the offset at `+0x00`, the
//! push limit at `+0x10` and the accumulated push at `+0x20`. The solver's pair
//! walk writes one per overlapping shape (`0x142cf46dd`): the limit is one
//! float splatted over four lanes and the accumulated push starts at zero.
//!
//! # The resolve (`0x142cd4610`)
//!
//! The motion is `requested - body[0x10]`, and the factor is the lane-wise
//! minimum over the contacts' factors (`minps`, `0x142cd4a79`), each of which
//! starts at one. The new position is `body[0x10] + motion * factor` - the
//! motion is **scaled**, not projected: the client has no residual slide. A
//! lane that comes out infinite (or NaN) discards the whole update and keeps
//! the old position (`0x142cd4ac0`).
//!
//! # The fixed point (`0x142cf3f80`)
//!
//! The solver runs the step stage and the resolve until the resolved position
//! is within `eps` of the stepped one: the correction's three lanes are
//! squared and summed and compared against `eps * eps` (`0x142cf49bb`,
//! `0x142cf49d8`). The bound is the block's `+0x2c` (the solver reads it as
//! `param_3[0xb]`, `cmp edx, dword ptr [rsi + 0x2c]` at `0x142cf49f0`).
//!
//! # What is proved
//!
//! [`step_stage`] is **verified** against the client's bytes as
//! `fn/CharacterStepStage`: 160 differential cases (the output position quad,
//! the round count and the plane records' accumulated pushes, byte for byte),
//! 100% branch coverage of the seven reachable branch sites and 8/8 mutants
//! killed. [`resolve_motion`], [`factor_min`] and [`resolve_loop`] are
//! transcribed from the read above and covered by unit tests; the resolve
//! itself (`0x142cd4610`) is not a leaf, so it has no differential unit yet.

use crate::collide::Quad;
use crate::movement::{maxss, minss};

/// The epsilon the solver and the step stage converge to (`0x3ba3d70a`):
/// `0.005` metres.
pub const RESOLVE_EPS: f32 = f32::from_bits(0x3ba3_d70a);

/// The skin the step stage adds to the signed distance before it pushes
/// (`0x1435ed220`, `0x3ba3d70a`): `0.005` metres.
pub const STEP_SKIN: f32 = f32::from_bits(0x3ba3_d70a);

/// The step stage's iteration cap (`cmp r11d, 0xc8` at `0x142cf5367`): 200
/// rounds.
pub const STEP_ITERATIONS: u32 = 200;

/// One contact plane: the client's `0x40`-byte record.
///
/// `normal` is the plane's normal and its offset in one quad - the signed
/// distance of a point `p` is `dot(normal.xyz, p.xyz) - normal.w`. `limit` caps
/// how far the plane may push and `accumulated` is how far it has pushed so
/// far; both are four-lane values because the client writes them as quads.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Plane {
    /// The plane's normal (lanes 0..2) and its offset (lane 3).
    pub normal: Quad,
    /// The push limit, per lane.
    pub limit: Quad,
    /// The accumulated push, per lane.
    pub accumulated: Quad,
}

impl Plane {
    /// A plane with no limit and no accumulated push: the state the solver's
    /// pair walk writes before the step stage runs.
    pub fn new(normal: Quad, limit: f32) -> Plane {
        Plane {
            normal,
            limit: [limit; 4],
            accumulated: [0.0; 4],
        }
    }
}

/// The client's three-lane dot product: `(a0*b0 + a1*b1) + a2*b2`, the order
/// the SSE sequence at `0x142cf50fa` folds in.
fn dot3(a: Quad, b: Quad) -> f32 {
    (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]
}

/// The absolute value the client takes with `ANDPS` and the mask at
/// `0x142f7aaf0` (`0x7fffffff`): the sign bit is cleared and nothing else.
#[inline]
fn absolute(value: f32) -> f32 {
    f32::from_bits(value.to_bits() & 0x7fff_ffff)
}

/// `0x142cf5020`: the client's step stage.
///
/// The requested position is pushed out of every plane it penetrates. A plane's
/// push is
///
/// ```text
/// distance = dot(normal.xyz, position.xyz) - normal.w
/// candidate = -(distance + 0.005) + accumulated
/// accumulated = max(0, min(candidate, limit))
/// push = accumulated - the old accumulated
/// position = position + push * normal
/// ```
///
/// and the stage repeats until the largest push over the four lanes is below
/// `eps`, or the 200-round cap is reached. The accumulated push survives the
/// rounds - that is what makes the stage converge - and the count of rounds
/// that did *not* converge is what the stage returns.
///
/// The planes are consumed: their `accumulated` field carries the push the
/// stage applied, so a caller that rebuilds the list (the solver's pair walk
/// does, every round of its own loop) starts from zero again.
pub fn step_stage(requested: Quad, planes: &mut [Plane], eps: f32) -> (Quad, u32) {
    if planes.is_empty() {
        // `0x142cf5051`: no contacts, the target is already the answer.
        return (requested, 0);
    }
    let mut position = requested;
    let mut rounds = 0u32;
    loop {
        let mut largest = [0.0f32; 4];
        for plane in planes.iter_mut() {
            let distance = dot3(plane.normal, position) - plane.normal[3];
            let mut accumulated = [0.0f32; 4];
            for lane in 0..4 {
                // `-(distance + 0.005) + accumulated`: the sign flip is the
                // client's `XORPS` with the mask at `0x142f817a0`.
                let candidate = -(distance + STEP_SKIN) + plane.accumulated[lane];
                // `minps` then `maxps`: the limit caps the push and the zero
                // stops a plane that is behind the point from pulling it in.
                accumulated[lane] = maxss(0.0, minss(candidate, plane.limit[lane]));
            }
            let mut push = [0.0f32; 4];
            for lane in 0..4 {
                push[lane] = accumulated[lane] - plane.accumulated[lane];
                plane.accumulated[lane] = accumulated[lane];
            }
            for lane in 0..4 {
                // The client's `MULPS`/`ADDPS` pair writes all four lanes, the
                // fourth one included.
                position[lane] += push[lane] * plane.normal[lane];
                largest[lane] = maxss(largest[lane], absolute(push[lane]));
            }
        }
        // `0x142cf53a4`: the round ends when every lane's push is under the
        // epsilon. The client's test is `<`, so a push of exactly the epsilon
        // keeps the stage going.
        if largest.iter().all(|value| *value < eps) {
            break;
        }
        rounds += 1;
        if rounds >= STEP_ITERATIONS {
            break;
        }
    }
    (position, rounds)
}

/// The factor accumulator: the lane-wise minimum over the contacts'
/// factors.
///
/// The client's `minps` at `0x142cd4a79` keeps the smaller of the two and
/// takes the contact's value on a NaN, which is what [`minss`] does.
pub fn factor_min(factor: Quad, contact: Quad) -> Quad {
    let mut out = [0.0f32; 4];
    for lane in 0..4 {
        out[lane] = minss(factor[lane], contact[lane]);
    }
    out
}

/// `0x142cd4610`: the client's resolve, once the contacts are known.
///
/// The motion is scaled by the factor - the client does **not** project the
/// leftover motion off the contact normal, so there is no slide here. A lane
/// that comes out infinite or NaN discards the whole update and the old
/// position stands (`0x142cd4ac0`).
pub fn resolve_motion(current: Quad, requested: Quad, factor: Quad) -> Quad {
    let mut out = [0.0f32; 4];
    for lane in 0..4 {
        out[lane] = current[lane] + (requested[lane] - current[lane]) * factor[lane];
    }
    // The client's test is the exponent field, so a NaN counts as infinite
    // (`(bits & 0x7f800000) == 0x7f800000`), and only the three position
    // lanes are looked at.
    if (0..3).any(|lane| (out[lane].to_bits() & 0x7f80_0000) == 0x7f80_0000) {
        return current;
    }
    out
}

/// The solver's convergence test (`0x142cf49bb`): the correction's three lanes
/// are squared and summed, and the sum is compared against `eps * eps`.
pub fn converged(resolved: Quad, requested: Quad, eps: f32) -> bool {
    let mut total = 0.0f32;
    for lane in 0..3 {
        let difference = resolved[lane] - requested[lane];
        total += difference * difference;
    }
    total < eps * eps
}

/// What the solver's character path publishes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Resolved {
    /// Where the body ended up.
    pub position: Quad,
    /// The number of rounds the fixed point took.
    pub iterations: u32,
}

/// `0x142cf3f80`: the solver's character path, the fixed-point resolve.
///
/// `probe` is the contact collection: it is called with the body's current
/// position and returns the contact planes and the contact factor for that
/// position. The solver's own order is the pair walk (the planes) first, the
/// step stage second and the narrowphase (the factor) third, so a caller that
/// has both in one query may return them together - the planes are what the
/// step stage reads and the factor is what the resolve reads.
///
/// The loop ends when the resolved position is within `eps` of the stepped one
/// or when the cap is reached; the count of rounds that did not converge is
/// what the solver publishes at `body+0x664`.
pub fn resolve_loop<F>(start: Quad, requested: Quad, cap: u32, mut probe: F) -> Resolved
where
    F: FnMut(Quad) -> (Vec<Plane>, Quad),
{
    if cap == 0 {
        // `0x142cf42d0`: the cap is the loop's own bound, so a zero cap skips
        // the whole path and the body keeps its position.
        return Resolved {
            position: start,
            iterations: 0,
        };
    }
    let mut position = start;
    let mut iterations = 0u32;
    loop {
        let (mut planes, factor) = probe(position);
        let (stepped, _) = step_stage(requested, &mut planes, RESOLVE_EPS);
        let resolved = resolve_motion(position, stepped, factor);
        if converged(resolved, stepped, RESOLVE_EPS) {
            return Resolved {
                position: resolved,
                iterations,
            };
        }
        position = resolved;
        iterations += 1;
        if iterations >= cap {
            return Resolved {
                position,
                iterations,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() <= 1.0e-6
    }

    fn quad_close(a: Quad, b: Quad) -> bool {
        (0..4).all(|lane| close(a[lane], b[lane]))
    }

    /// A floor at `y = 0`, its normal pointing up, and a body above it.
    fn floor() -> Plane {
        // The plane's offset: `dot(n.xyz, p) - n.w` is zero at `y = 0`, so
        // `n.w = 0` with the normal `(0, 1, 0)`.
        Plane::new([0.0, 1.0, 0.0, 0.0], 1.0)
    }

    #[test]
    fn no_planes_leaves_the_target_alone() {
        let requested = [1.0, 2.0, 3.0, 0.0];
        let (position, rounds) = step_stage(requested, &mut [], RESOLVE_EPS);
        assert!(quad_close(position, requested));
        assert_eq!(rounds, 0);
    }

    #[test]
    fn a_plane_the_point_is_outside_of_does_not_push() {
        // The point is above the floor, so the distance is positive and the
        // candidate is negative: the zero clamp keeps the push at nothing.
        let requested = [0.0, 1.0, 0.0, 0.0];
        let mut planes = vec![floor()];
        let (position, rounds) = step_stage(requested, &mut planes, RESOLVE_EPS);
        assert!(quad_close(position, requested));
        assert_eq!(rounds, 0);
    }

    #[test]
    fn a_plane_the_point_is_inside_of_pushes_it_out() {
        // The point is 0.2 m below the floor: the push is 0.2 - 0.005.
        let requested = [0.0, -0.2, 0.0, 0.0];
        let mut planes = vec![floor()];
        let (position, rounds) = step_stage(requested, &mut planes, RESOLVE_EPS);
        assert!(close(position[1], -0.005), "position {position:?}");
        assert_eq!(rounds, 1, "the first round pushed, the second did not");
        assert!(close(planes[0].accumulated[0], 0.195));
    }

    #[test]
    fn the_push_is_capped_by_the_limit() {
        // The limit caps the *total* push, not the push of one round: the
        // accumulated value is an integrator, so a plane that is already at
        // its limit pushes nothing more.
        let requested = [0.0, -1.0, 0.0, 0.0];
        let mut planes = vec![Plane::new([0.0, 1.0, 0.0, 0.0], 0.01)];
        let (position, rounds) = step_stage(requested, &mut planes, RESOLVE_EPS);
        assert!(close(position[1], -0.99), "position {position:?}");
        assert_eq!(rounds, 1);
        assert!(
            close(planes[0].accumulated[0], 0.01),
            "{:?}",
            planes[0].accumulated
        );
    }

    #[test]
    fn the_step_keeps_the_fourth_lane() {
        // The client multiplies and adds all four lanes, so the fourth lane
        // moves too.
        let requested = [0.0, -0.2, 0.0, 1.0];
        let mut planes = vec![Plane::new([0.0, 1.0, 0.0, 0.0], 1.0)];
        let (position, _) = step_stage(requested, &mut planes, RESOLVE_EPS);
        assert!(close(position[3], 1.0), "position {position:?}");
    }

    #[test]
    fn the_factor_is_the_smallest_of_the_contacts() {
        let mut factor = [1.0, 1.0, 1.0, 1.0];
        factor = factor_min(factor, [0.5, 0.8, 0.2, 1.0]);
        factor = factor_min(factor, [0.4, 0.9, 0.7, 1.0]);
        assert!(quad_close(factor, [0.4, 0.8, 0.2, 1.0]));
    }

    #[test]
    fn the_motion_is_scaled_by_the_factor() {
        let current = [0.0, 0.0, 0.0, 0.0];
        let requested = [10.0, 0.0, -4.0, 0.0];
        let resolved = resolve_motion(current, requested, [0.25, 1.0, 0.5, 1.0]);
        assert!(quad_close(resolved, [2.5, 0.0, -2.0, 0.0]));
    }

    #[test]
    fn an_infinite_lane_keeps_the_old_position() {
        let current = [1.0, 2.0, 3.0, 0.0];
        let requested = [f32::INFINITY, 2.0, 3.0, 0.0];
        let resolved = resolve_motion(current, requested, [1.0; 4]);
        assert!(quad_close(resolved, current));
    }

    #[test]
    fn a_nan_lane_keeps_the_old_position() {
        let current = [1.0, 2.0, 3.0, 0.0];
        let requested = [f32::NAN, 2.0, 3.0, 0.0];
        let resolved = resolve_motion(current, requested, [1.0; 4]);
        assert!(quad_close(resolved, current));
    }

    #[test]
    fn the_fixed_point_stops_at_the_epsilon() {
        // A probe that always reports a contact at half the motion: the body
        // creeps towards the target and never arrives.
        let requested = [10.0, 0.0, 0.0, 0.0];
        let resolved = resolve_loop([0.0; 4], requested, 8, |_position| (vec![], [0.5; 4]));
        assert_eq!(resolved.iterations, 8, "the cap ends the loop");
        assert!(resolved.position[0] < requested[0]);
        assert!(resolved.position[0] > 0.0);
    }

    #[test]
    fn a_free_body_reaches_the_target_in_one_round() {
        let requested = [10.0, 0.0, 0.0, 0.0];
        let resolved = resolve_loop([0.0; 4], requested, 8, |_position| (vec![], [1.0; 4]));
        assert!(quad_close(resolved.position, requested));
        assert_eq!(resolved.iterations, 0);
    }

    #[test]
    fn a_zero_cap_never_moves_the_body() {
        let requested = [10.0, 0.0, 0.0, 0.0];
        let resolved = resolve_loop([1.0, 0.0, 0.0, 0.0], requested, 0, |_position| {
            (vec![], [1.0; 4])
        });
        assert!(quad_close(resolved.position, [1.0, 0.0, 0.0, 0.0]));
        assert_eq!(resolved.iterations, 0);
    }
}

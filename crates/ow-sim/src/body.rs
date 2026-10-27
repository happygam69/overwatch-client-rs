//! The Domino body and the integration order the island solve applies to it.
//!
//! The operation order is read from the client, not chosen:
//!
//! ```text
//! t1 = gravityScale * mass              (one f32 multiply)
//! t4 = invMass * dt                     (precomputed 4-lane product)
//! t2 = t1 * g_i                         (mulps)
//! t3 = t2 + force_i                     (addps)
//! t5 = t3 * t4                          (mulps)
//! v' = t5 + v_i                         (addps)
//! f  = clamp(1 - damping * dt, 0, 1)    (mul, sub, minps, maxps)
//! v''= f * v'                           (mulps)
//! ```
//!
//! Then positions: `x += v'' * dt` (multiply, then add), with the quaternion
//! update and renormalisation after it (`q *= 1/sqrt(|q|^2)` when
//! `|q|^2 > 1.17e-35`).
//!
//! Damping is linear (`clamp(1 - d*dt, 0, 1)`), not Box2D's `1/(1+dt*d)`, and
//! the gravity/force term is formed as
//! `(g*scale*mass + force) * invMass * dt` in that order, so the roundings are
//! reproducible.
//!
//! `min`/`max` follow the SSE operand semantics (`minss`/`maxss`: the second
//! operand wins only when strictly less/greater), not Rust's NaN behaviour, so
//! a NaN velocity is clamped the way the client clamps it.
//!
use crate::Vec3;

/// `minss a, b` — returns `b` only when it is strictly less than `a`.
#[inline]
fn min_client(a: f32, b: f32) -> f32 {
    if b < a {
        b
    } else {
        a
    }
}

/// `maxss a, b` — returns `b` only when it is strictly greater than `a`.
#[inline]
fn max_client(a: f32, b: f32) -> f32 {
    if b > a {
        b
    } else {
        a
    }
}

/// The damping factor the client multiplies a velocity by:
/// `clamp(1 - damping * dt, 0, 1)`, in the client's order (multiply, subtract,
/// min against 1, max against 0).
pub fn damping_factor(damping: f32, dt: f32) -> f32 {
    let f = 1.0 - damping * dt;
    let f = min_client(f, 1.0);
    max_client(0.0, f)
}

/// One velocity integration step, exactly as the client forms it.
#[allow(clippy::too_many_arguments)]
pub fn integrate_velocity(
    v: Vec3,
    force: Vec3,
    gravity: Vec3,
    mass: f32,
    inv_mass: f32,
    gravity_scale: f32,
    linear_damping: f32,
    dt: f32,
) -> Vec3 {
    let t1 = gravity_scale * mass;
    let t4 = inv_mass * dt;
    let factor = damping_factor(linear_damping, dt);
    let mut out = [0.0f32; 3];
    for i in 0..3 {
        let t2 = t1 * gravity[i];
        let t3 = t2 + force[i];
        let t5 = t3 * t4;
        let vp = t5 + v[i];
        out[i] = factor * vp;
    }
    out
}

/// `x += v * dt`, with the multiply before the add, per the client's write.
pub fn integrate_position(pos: Vec3, vel: Vec3, dt: f32) -> Vec3 {
    [
        vel[0] * dt + pos[0],
        vel[1] * dt + pos[1],
        vel[2] * dt + pos[2],
    ]
}

/// Quaternion renormalisation: `q *= 1/sqrt(|q|²)` when `|q|²` exceeds the
/// client's threshold (bits `0x057a0000`, read at exec `0x7ff7566e39a8`).
pub fn normalize_quaternion(q: [f32; 4]) -> [f32; 4] {
    const THRESHOLD: f32 = f32::from_bits(0x057a_0000);
    let len2 = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
    if THRESHOLD < len2 {
        let inv = 1.0 / len2.sqrt();
        [q[0] * inv, q[1] * inv, q[2] * inv, q[3] * inv]
    } else {
        q
    }
}

/// A body with the fields the integration touches, using the client's runtime
/// layout names (`docs/formats/domino.md`, dmBody).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Body {
    pub pos: Vec3,
    pub vel: Vec3,
    pub force: Vec3,
    pub mass: f32,
    pub inv_mass: f32,
    pub gravity_scale: f32,
    pub linear_damping: f32,
    pub quat: [f32; 4],
    /// Accumulated sleep time, seconds (client: dmBody.sleepTime).
    pub sleep_time: f32,
}

impl Body {
    /// A body of `mass` at rest, gravity scale 1, no damping.
    pub fn new(pos: Vec3, mass: f32) -> Body {
        Body {
            pos,
            vel: [0.0; 3],
            force: [0.0; 3],
            mass,
            inv_mass: if mass > 0.0 { 1.0 / mass } else { 0.0 },
            gravity_scale: 1.0,
            linear_damping: 0.0,
            quat: [0.0, 0.0, 0.0, 1.0],
            sleep_time: 0.0,
        }
    }

    /// The client's integration order for this body: integrate velocities,
    /// then damp them, then integrate the position.
    pub fn step(&mut self, gravity: Vec3, dt: f32) {
        self.vel = integrate_velocity(
            self.vel,
            self.force,
            gravity,
            self.mass,
            self.inv_mass,
            self.gravity_scale,
            self.linear_damping,
            dt,
        );
        self.pos = integrate_position(self.pos, self.vel, dt);
        self.quat = normalize_quaternion(self.quat);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(v: f32) -> u32 {
        v.to_bits()
    }

    /// Golden vectors computed independently (a Python reference of the same
    /// assembly order, `docs/formats/domino.md` § Q2/Q4): if this crate ever
    /// reassociates or fuses the arithmetic, these bits change.
    #[test]
    fn integration_matches_the_clients_operation_order() {
        let cases: [(
            [f32; 3],
            [f32; 3],
            [f32; 3],
            f32,
            f32,
            f32,
            f32,
            f32,
            [u32; 3],
        ); 3] = [
            (
                [5.5, -1.25, 0.0],
                [10.0, 0.0, -4.0],
                [0.0, -20.0, 0.0],
                75.0,
                1.0f32 / 75.0,
                1.0,
                0.1,
                1.0f32 / 60.0,
                [0x40af_c714, 0xbfca_5432, 0xba68_a0e8],
            ),
            (
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 0.0],
                [0.0, -30.0, 0.0],
                200.0,
                1.0f32 / 200.0,
                1.0,
                0.0,
                1.0f32 / 60.0,
                [0x0000_0000, 0xbf00_0000, 0x0000_0000],
            ),
            (
                [-3.3, 5.72, 1.0e-7],
                [123.456, -7.5, 0.25],
                [0.0, -20.0, 0.0],
                55.5,
                1.0f32 / 55.5,
                0.5,
                0.3,
                1.0f32 / 30.0,
                [0xc04c_63d7, 0x40aa_81c4, 0x391b_f926],
            ),
        ];
        for (i, (v, force, g, mass, inv_mass, scale, damping, dt, want)) in
            cases.into_iter().enumerate()
        {
            let out = integrate_velocity(v, force, g, mass, inv_mass, scale, damping, dt);
            for k in 0..3 {
                assert_eq!(
                    bits(out[k]),
                    want[k],
                    "case {i} component {k}: got {:08x} ({}) want {:08x} ({})",
                    bits(out[k]),
                    out[k],
                    want[k],
                    f32::from_bits(want[k]),
                );
            }
        }
    }

    /// A body dropped under gravity reaches the analytic Δv = g·dt only when
    /// mass and invMass cancel as written: 75 · (−20) · (1/75) · (1/60).
    #[test]
    fn gravity_cancels_mass_and_inv_mass() {
        let mut body = Body::new([0.0, 100.0, 0.0], 75.0);
        body.step([0.0, -20.0, 0.0], 1.0 / 60.0);
        // 20/60 = 1/3; the float product is one rounding from that.
        assert!((body.vel[1] + 1.0 / 3.0).abs() < 1e-7, "{:?}", body.vel);
    }

    #[test]
    fn damping_is_linear_and_clamped() {
        // The formula the client uses: 1 - d*dt, clamped to [0, 1].
        assert_eq!(damping_factor(0.0, 1.0 / 60.0), 1.0);
        assert_eq!(damping_factor(0.1, 1.0 / 60.0), 1.0 - 0.1 * (1.0 / 60.0));
        // A damping large enough to overshoot clamps at zero, not negative.
        assert_eq!(damping_factor(1000.0, 1.0 / 60.0), 0.0);
    }

    /// The lock the golden vectors exist for: a reassociated form gives
    /// different bits on adversarial inputs, so the test is not vacuous.
    #[test]
    fn a_reassociated_integration_drifts() {
        let (v, force, g) = ([5.5, -1.25, 0.0], [10.0, 0.0, -4.0], [0.0, -20.0, 0.0]);
        let (mass, inv_mass, scale, damping, dt) =
            (75.0f32, 1.0f32 / 75.0, 1.0f32, 0.1f32, 1.0f32 / 60.0);
        let client = integrate_velocity(v, force, g, mass, inv_mass, scale, damping, dt);
        // v + dt * (g*scale + force*invMass), the "textbook" association.
        let mut alt = [0.0f32; 3];
        for i in 0..3 {
            alt[i] = v[i] + dt * (g[i] * scale + force[i] * inv_mass);
        }
        let mut differs = false;
        for i in 0..3 {
            if bits(client[i]) != bits(alt[i]) {
                differs = true;
            }
        }
        assert!(
            differs,
            "the alternate association must differ, else the golden test proves nothing: {:?} vs {:?}",
            client, alt
        );
    }

    #[test]
    fn position_integration_multiplies_then_adds() {
        let out = integrate_position([1.0, 2.0, 3.0], [5.5, -0.5, 0.0], 1.0 / 60.0);
        for i in 0..3 {
            let want = [1.0 + 5.5 * (1.0 / 60.0), 2.0 - 0.5 * (1.0 / 60.0), 3.0];
            assert_eq!(
                bits(out[i]),
                bits([5.5f32, -0.5, 0.0][i] * (1.0 / 60.0) + [1.0f32, 2.0, 3.0][i]),
                "component {i}"
            );
            assert!((out[i] - want[i]).abs() < 1e-6);
        }
    }

    #[test]
    fn quaternion_stays_normalized() {
        let q = normalize_quaternion([0.0, 0.0, 0.0, 1.0]);
        assert_eq!(q, [0.0, 0.0, 0.0, 1.0]);
        let q = normalize_quaternion([0.0, 0.0, 0.0, 2.0]);
        assert!((q[3] - 1.0).abs() < 1e-6, "{q:?}");
    }
}

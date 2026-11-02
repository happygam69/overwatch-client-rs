//! The exact client-side arithmetic this project has read from
//! `Overwatch.exe`.
//!
//! There is no fitted gameplay here. `body` is the Domino integration order
//! the island solve applies; `camera` is the basis orthonormaliser; `movement`
//! holds the read mover ramp, command, wish-spring and transform leaves;
//! `state` is the state-record transform lookup; `frame` is the per-frame
//! velocity law those leaves compose into; `collide` is the first stage of the
//! swept collision query, and `policy` is the hit callback that query calls
//! back into. The arithmetic is checked by the differential harness. The rest
//! of the mover's input→velocity path (Domino dispatch) and the map ingest the
//! policy reads are still open (`docs/open-questions.md`), so they are not
//! implemented in this crate.

pub mod body;
pub mod camera;
pub mod collide;
pub mod collider;
pub mod fan;
pub mod frame;
pub mod gate;
pub mod modifiers;
pub mod movement;
pub mod policy;
pub mod settings;
pub mod shapes;
pub mod state;
pub mod world;

/// A position and direction in metres.
pub type Vec3 = [f32; 3];

pub fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub fn scale(a: Vec3, s: f32) -> Vec3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

pub fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn length(a: Vec3) -> f32 {
    dot(a, a).sqrt()
}

pub fn normalize(a: Vec3) -> Vec3 {
    let len = length(a);
    if len <= f32::EPSILON {
        [0.0, 0.0, 1.0]
    } else {
        scale(a, 1.0 / len)
    }
}

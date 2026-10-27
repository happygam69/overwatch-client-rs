//! The exact client-side arithmetic this project has read from
//! `Overwatch.exe`.
//!
//! There is no fitted gameplay here. `body` is the Domino integration order
//! the island solve applies; `camera` is the basis orthonormaliser. Both are
//! checked by the differential harness. The mover's input→velocity path is
//! still open (`docs/open-questions.md`), so it is not implemented in this
//! crate.

pub mod body;
pub mod camera;

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

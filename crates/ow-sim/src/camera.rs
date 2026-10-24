//! Camera-basis arithmetic read from the client.
//!
//! This module holds the *shipped* implementation used by the view path, not
//! a harness copy. The harness probe (`src/bin/verify_probe.rs`) imports these
//! functions directly, so a `cargo` tier-2 unit verifies the code the viewer
//! actually links.
//!
//! `orthonormalize_camera_basis` is the leaf at exe `0x142069c60` (v2b
//! `0x7ff7c6d89c60`), read from the complete current-build image. It reads two
//! float4 basis rows and rebuilds row 1 when the pair is off unit length or
//! non-orthogonal. The operation order below is the image's, not a tidied
//! equivalent: the harness compares bit patterns.

use std::hint::black_box;

/// `rsqrtps` as Unicorn emulates it: an f32 square root, then an f32
/// reciprocal. The host CPU's hardware `rsqrtps` is a low-precision
/// approximation with different bits, so the harness oracle (Unicorn) and
/// this function agree only on the precise form.
#[inline]
fn rsqrtps_unicorn(x: f32) -> f32 {
    // black_box stops LLVM folding this into a hardware rsqrtps/rsqrtss.
    let s = black_box(x).sqrt();
    black_box(1.0f32 / s)
}

/// 3D cross of `(ax, ay, az) x (bx, by, bz)`; the w lanes are ignored.
#[inline]
fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Length-squared of the xyz lanes, with the image's horizontal-add
/// association: `(y*y + x*x) + z*z`.
#[inline]
fn len2_xyz(v: [f32; 3]) -> f32 {
    (v[1] * v[1] + v[0] * v[0]) + v[2] * v[2]
}

/// Re-orthonormalize two camera basis rows in place.
///
/// `desc` is the pointee (at least `0x30` bytes); row 0 lives at `+0x10` and
/// row 1 at `+0x20`. Already-orthonormal input writes nothing. On the rebuild
/// path row 0 is stored back unchanged and row 1 becomes the normalised
/// `(row0 x row1) x row0`, with `row1.w = 0`.
// The `!(x >= floor)` form is deliberate: the image's `comiss`+`jae` falls
// through on unordered (NaN) as well as on a value below the floor, which
// `x < floor` does not model. Clippy's readable-comparison lint would change
// the behaviour on NaN.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn orthonormalize_camera_basis(desc: &mut [u8]) {
    assert!(desc.len() >= 0x30, "camera basis pointee too small");

    let f32_at = |off: usize| -> f32 {
        let mut b = [0u8; 4];
        b.copy_from_slice(&desc[off..off + 4]);
        f32::from_le_bytes(b)
    };
    let set_f32 = |desc: &mut [u8], off: usize, v: f32| {
        desc[off..off + 4].copy_from_slice(&v.to_le_bytes());
    };

    let row0 = [f32_at(0x10), f32_at(0x14), f32_at(0x18), f32_at(0x1c)];
    let row1 = [f32_at(0x20), f32_at(0x24), f32_at(0x28), f32_at(0x2c)];

    let len0 = len2_xyz([row0[0], row0[1], row0[2]]);
    let len1 = len2_xyz([row1[0], row1[1], row1[2]]);
    let dot = row0[0] * row1[0] + row0[1] * row1[1] + row0[2] * row1[2];

    // Rebuild when either length is off by >= 0.008, or |dot| > 0.004.
    let needs_rebuild =
        (len0 - 1.0).abs() >= 0.008f32 || (len1 - 1.0).abs() >= 0.008f32 || dot.abs() > 0.004f32;

    if !needs_rebuild {
        return;
    }

    let mut c = cross3([row0[0], row0[1], row0[2]], [row1[0], row1[1], row1[2]]);
    // comiss + jae skips the fallback only when len2 >= FLT_MIN and the
    // compare is ordered. NaN/Inf lengths are unordered, so `!(>=)` is true
    // for them as well as for values below the floor - match the machine.
    if !(len2_xyz(c) >= 1.17549435e-36f32) {
        c = cross3([row0[0], row0[1], row0[2]], [0.0, 1.0, 0.0]);
        if !(len2_xyz(c) >= 1.17549435e-36f32) {
            c = cross3([row0[0], row0[1], row0[2]], [0.0, 0.0, 1.0]);
        }
    }

    set_f32(desc, 0x10, row0[0]);
    set_f32(desc, 0x14, row0[1]);
    set_f32(desc, 0x18, row0[2]);
    set_f32(desc, 0x1c, row0[3]);

    let c2 = cross3(c, [row0[0], row0[1], row0[2]]);
    let c2len2 = len2_xyz(c2);
    let y = rsqrtps_unicorn(c2len2);
    let yy = black_box(y * y);
    let y_half = black_box(y * 0.5f32);
    let t = black_box(yy * c2len2);
    let s = black_box(3.0f32 - t);
    let t = black_box(s * y_half);
    set_f32(desc, 0x20, t * c2[0]);
    set_f32(desc, 0x24, t * c2[1]);
    set_f32(desc, 0x28, t * c2[2]);
    set_f32(desc, 0x2c, 0.0f32 * t);
}

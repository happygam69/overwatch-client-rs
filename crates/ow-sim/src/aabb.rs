//! The segment-AABB transform at `0x142697ac0`.
//!
//! One call takes two 4-wide endpoints of an axis-aligned segment and a column
//! matrix, and writes the segment's two AABB corners in the matrix's frame:
//! `out[0..4]` is the low corner and `out[4..8]` the high one, lane by lane.
//!
//! Read from the body, not from the C. The client's own order, which the
//! transcription keeps, is:
//!
//! ```text
//! c = (high + low) * 0.5                 ; addps, mulps by the 0.5 vector
//! e = (high - low) * 0.5                 ; subps, mulps
//! ext = (e.y * |m1| + e.x * |m0|) + e.z * |m2|      ; andps abs mask
//! cen = ((c.x * m0 + t) + c.y * m1) + c.z * m2
//! out[0..4] = cen - ext                 ; subps
//! out[4..8] = cen + ext                 ; addps
//! ```
//!
//! `e.y * |m1|` is added **first** and `e.x * |m0|` second (`0x142697b0c` is the
//! `mulps` on the `0x55` broadcast, `0x142697b19` the one on the `0x00`
//! broadcast, `0x142697b27` the `addps`), so the Rust keeps that grouping: a
//! single-valued sum reordered would change the low bits. `shufps ... 0x55`
//! broadcasts lane 1, `0xaa` lane 2, `0x00` lane 0; the `0x20` mask copy at
//! `0x142697b15` reuses the abs mask for row 2.
//!
//! The two constants the body reads are image data: `0x142f844e0` is
//! `0.5` four times and `0x142f7aaf0` is `0x7fffffff` four times (the abs
//! mask, applied with `andps`). Lane 3 participates: the client never masks a
//! lane off, so a caller that leaves `low[3]`/`high[3]` unrelated still gets a
//! lane-3 result computed from it.
//!
//! The body returns its first argument in `rax` (`mov rax, rcx` at
//! `0x142697acc`, never overwritten); see the unit's `acknowledgedGaps` for why
//! the Rust entry returns nothing.

/// `0x142697ac0`: the segment's AABB in the matrix's frame.
///
/// `m` is 16 floats read as four 4-wide columns: `m[0..4]` row 0, `m[4..8]`
/// row 1, `m[8..12]` row 2, `m[12..16]` the translation. `seg` is the two
/// endpoints, `seg[0..4]` and `seg[4..8]`.
pub fn segment_aabb_transform(m: &[f32; 16], seg: &[f32; 8]) -> [f32; 8] {
    // Scalar per lane, not intrinsics: each lane of the client's `addps`/`mulps`
    // sequence is independent, so the same operations in the same order round
    // identically - and a scalar form gives the mutation stage something to
    // edit. The one thing this form cannot reproduce is the *sign* of a NaN
    // when two NaNs meet: the instruction keeps the destination's, and scalar
    // Rust leaves that choice to the backend. That case is not comparable in
    // this harness either (the emulator clears the sign), and the unit's corpus
    // excludes NaN operands for exactly that reason.
    let half = 0.5f32;
    // c and e are computed on all four lanes, exactly as the client does.
    let mut c = [0.0f32; 4];
    let mut e = [0.0f32; 4];
    for lane in 0..4 {
        c[lane] = (seg[4 + lane] + seg[lane]) * half;
        e[lane] = (seg[4 + lane] - seg[lane]) * half;
    }
    // `andps` with `0x7fffffff`: the extent takes the magnitude, sign and all.
    let abs = |v: f32| f32::from_bits(v.to_bits() & 0x7fff_ffff);

    let mut out = [0.0f32; 8];
    for lane in 0..4 {
        let m0 = m[lane];
        let m1 = m[4 + lane];
        let m2 = m[8 + lane];
        let t = m[12 + lane];
        // (e.y * |m1| + e.x * |m0|) + e.z * |m2|
        let ext = (e[1] * abs(m1) + e[0] * abs(m0)) + e[2] * abs(m2);
        // ((c.x * m0 + t) + c.y * m1) + c.z * m2
        let cen = ((c[0] * m0 + t) + c[1] * m1) + c[2] * m2;
        out[lane] = cen - ext;
        out[4 + lane] = cen + ext;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::segment_aabb_transform;

    /// Identity columns: the corners are the segment's own midpoint plus or
    /// minus its half-extent. Lane 3 carries the translation's `w`, which the
    /// client never masks off, so both corners end at `1.0` there.
    #[test]
    fn identity_columns_keep_the_segment_shape() {
        let m = [
            1.0, 0.0, 0.0, 0.0, // column 0
            0.0, 1.0, 0.0, 0.0, // column 1
            0.0, 0.0, 1.0, 0.0, // column 2
            0.0, 0.0, 0.0, 1.0, // translation
        ];
        let seg = [0.0, 0.0, 0.0, 0.0, 2.0, 4.0, 6.0, 0.0];
        let out = segment_aabb_transform(&m, &seg);
        assert_eq!(out, [0.0, 0.0, 0.0, 1.0, 2.0, 4.0, 6.0, 1.0]);
    }

    /// A mirrored axis negates the centre and keeps the extent positive, so the
    /// corners stay low-then-high instead of swapping: the `andps` is why.
    #[test]
    fn mirrored_columns_keep_the_corner_order() {
        let m = [
            -1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        let seg = [0.0, 0.0, 0.0, 0.0, 2.0, 4.0, 6.0, 0.0];
        let out = segment_aabb_transform(&m, &seg);
        assert_eq!(out, [-2.0, 0.0, 0.0, 1.0, 0.0, 4.0, 6.0, 1.0]);
    }
}

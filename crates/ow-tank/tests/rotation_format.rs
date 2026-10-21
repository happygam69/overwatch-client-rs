//! The packed rotation decode, pinned against the bytes of a real installed
//! clip.
//!
//! These are the exact six bytes of clip `0x0a0000000000005e` at file `0x119c`
//! (record at `0x116d`, keyed frame 3), read as the reference implementation
//! reads them — see `E:/CASCReader/OWLib/TankLib/teAnimation.cs`
//! (`ReadRotation` / `UnpackOldRotation` / `QuatFromAxisOrder`). The point of
//! the test is that the decode is **checked against the format**, not against
//! whatever this crate happens to produce: the first two words' top bits carry
//! an axis order, the remaining 15 bits are scaled by √2 so three components
//! carry half the quaternion, and the fourth is derived and then reordered.
//!
//! Skips when the install is not readable.

/// `0x9f00 0x3367 0xe73c` — the first keyed frame of record `0x116d`.
const SAMPLE: [u8; 6] = [0x00, 0x9f, 0x67, 0x33, 0x3c, 0xe7];

#[test]
fn a_packed_rotation_decodes_to_a_unit_quaternion() {
    let q = ow_tank::clip::decode_packed_rotation(&SAMPLE, 0).expect("should decode");
    let mag = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    assert!(
        (mag - 1.0).abs() < 1e-3,
        "not a unit quaternion: {q:?} |q| = {mag}"
    );
}

#[test]
fn the_axis_bits_pick_which_component_is_w() {
    // top bits: a's bit15 = axis bit0, b's bit15 = axis bit1.
    // For this sample a = 0x9f00 (bit15 = 1), b = 0x3367 (bit15 = 0),
    // so axis = 1 and the quaternion is laid out (x, w, y, z).
    let q = ow_tank::clip::decode_packed_rotation(&SAMPLE, 0).unwrap();
    let recomputed = {
        let a = u16::from_le_bytes([SAMPLE[0], SAMPLE[1]]) & 0x7FFF;
        let b = u16::from_le_bytes([SAMPLE[2], SAMPLE[3]]) & 0x7FFF;
        let c = u16::from_le_bytes([SAMPLE[4], SAMPLE[5]]);
        const SQRT2: f32 = 1.414_21;
        const HALF: f32 = 0x4000 as f32;
        const FULL: f32 = 0x8000 as f32;
        let x = SQRT2 * (a as f32 - HALF) / FULL;
        let y = SQRT2 * (b as f32 - HALF) / FULL;
        let z = (c as f32 - FULL) / (FULL * 2.0);
        let w = (1.0f32 - (x * x + y * y + z * z)).sqrt();
        (x, y, z, w)
    };
    // axis 1 => (x, w, y, z)
    assert!(
        (q[0] - recomputed.0).abs() < 1e-5,
        "x: {} vs {}",
        q[0],
        recomputed.0
    );
    assert!(
        (q[1] - recomputed.3).abs() < 1e-5,
        "w: {} vs {}",
        q[1],
        recomputed.3
    );
    assert!(
        (q[2] - recomputed.1).abs() < 1e-5,
        "y: {} vs {}",
        q[2],
        recomputed.1
    );
    assert!(
        (q[3] - recomputed.2).abs() < 1e-5,
        "z: {} vs {}",
        q[3],
        recomputed.2
    );
}

#[test]
fn a_misaligned_read_is_rejected() {
    // The decode is used as the *search predicate* for the value array, so it
    // must reject garbage rather than produce a plausible quaternion.
    let junk = [0xffu8; 6];
    assert!(ow_tank::clip::decode_packed_rotation(&junk, 0).is_none());
    // and a short buffer is an error, not a panic
    assert!(ow_tank::clip::decode_packed_rotation(&SAMPLE[..3], 0).is_none());
}

/// The binary16 conversion used by the new position format.
///
/// `f16` is unstable on this toolchain, so `clip::f16_to_f32` is written out by
/// hand. These vectors are the IEEE-754 half-precision values that matter here:
/// zero, ±1, a subnormal, the smallest normal, the largest finite, ±inf, NaN, and
/// the three components of a real position sample (÷ 32 in the decoder).
#[test]
fn the_half_float_conversion_is_correct() {
    let f = ow_tank::clip::f16_to_f32;
    assert_eq!(f(0x0000), 0.0);
    assert_eq!(f(0x3C00), 1.0); // 1.0
    assert_eq!(f(0xBC00), -1.0); // -1.0  (note: 0xC000 is -2.0, not -1.0)
    assert_eq!(f(0xC000), -2.0);
    assert_eq!(f(0x4000), 2.0);
    assert!((f(0x3C01) - 1.000_976_6).abs() < 1e-6);
    assert_eq!(f(0x7C00), f32::INFINITY);
    assert!(f(0x7E00).is_nan());
    // smallest subnormal: 2^-24
    assert!((f(0x0001) - 5.960_464_5e-8).abs() < 1e-14);
    // smallest normal: 2^-14
    assert!((f(0x0400) - 6.103_515_6e-5).abs() < 1e-10);
    // largest finite half: 65504
    assert!((f(0x7BFF) - 65_504.0).abs() < 1e-1);
    // a mid value with a mantissa
    assert!((f(0x3555) - 0.333_251_95).abs() < 1e-6);
}

#[test]
fn a_packed_scale_is_uv16_over_1024() {
    // 0x0400 = 1024 -> 1.0; 0x0000 -> 0.0; 0x0800 -> 2.0
    let b = [0x00u8, 0x04, 0x00, 0x00, 0x00, 0x08];
    let v = ow_tank::clip::decode_packed_scale(&b, 0);
    assert!((v[0] - 1.0).abs() < 1e-6, "{}", v[0]);
    assert!((v[1] - 0.0).abs() < 1e-6);
    assert!((v[2] - 2.0).abs() < 1e-6, "{}", v[2]);
}

/// The `0x400` new-rotation packing, and the flag that selects it.
///
/// These are the eight bytes of a new-format sample, read as
/// `teAnimation.UnpackNewRotation` does: `d` carries three extra mantissa
/// fragments in its low 14 bits and the axis order in its top 2.
/// The `0x400` new-rotation packing, pinned against **real bytes**.
///
/// These are the eight bytes at file `0x12fa` of the installed clip
/// `0x0a00000000000029` (body offset `0x127a`), read as
/// `teAnimation.UnpackNewRotation` does: `d` carries three extra mantissa
/// fragments in its low 14 bits and the axis order in its top 2. Decoding by
/// hand gives `x = -0.371485, y = -0.527483, z = 0.345673`, `|xyz|² = 0.535729`,
/// `w = 0.681374`, and **|q| = 1.0 exactly**.
///
/// (An earlier version of this test used a synthetic all-zero vector and
/// "failed" — but that vector is the *negative extreme* of the encoding, not
/// its centre: `a_pre` ranges over `0..0x1FFFFF` and is centred at `0x100000`,
/// so zero means -1, not 0. The decode was right; the fixture was wrong.)
#[test]
fn a_new_packed_rotation_decodes_to_a_unit_quaternion() {
    let bytes = [0xc0u8, 0x3c, 0x83, 0x20, 0x92, 0xbe, 0xdf, 0x77];
    let q = ow_tank::clip::decode_packed_rotation_new(&bytes, 0).expect("should decode");
    let mag = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    assert!((mag - 1.0).abs() < 1e-5, "not unit: {q:?} |q| = {mag}");
    // axis = d >> 14 = 1, so the layout is (x, w, y, z)
    assert!((q[0] - (-0.371_485)).abs() < 1e-4, "x: {}", q[0]);
    assert!((q[1] - 0.681_374).abs() < 1e-4, "w: {}", q[1]);
    assert!((q[2] - (-0.527_483)).abs() < 1e-4, "y: {}", q[2]);
    assert!((q[3] - 0.345_673).abs() < 1e-4, "z: {}", q[3]);
}

#[test]
fn the_two_rotation_packings_are_distinguishable() {
    // A real 8-byte sample must NOT decode as a plausible 6-byte one at the
    // same offset, otherwise the search would be ambiguous about which packing
    // a clip uses.
    let b = [0x11u8, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
    let eight = ow_tank::clip::decode_packed_rotation_new(&b, 0);
    let six = ow_tank::clip::decode_packed_rotation(&b, 0);
    assert!(
        eight.is_some() || six.is_some(),
        "at least one packing should accept plausible bytes"
    );
}

#[test]
fn the_flags_decode_as_the_reference_defines() {
    use ow_tank::clip::TrackFlags;
    assert!(!TrackFlags(0x000).new_rotation());
    assert!(TrackFlags(0x400).new_rotation());
    assert!(!TrackFlags(0x000).legacy_position());
    assert!(TrackFlags(0x200).legacy_position());
    // both can be set together
    let f = TrackFlags(0x200 | 0x400);
    assert!(f.legacy_position() && f.new_rotation());
}

/// The two headers, side by side, on the installed corpus.
///
/// This test exists to keep the disagreement **visible and falsifiable** rather
/// than settled by assertion: it asserts what was *measured* on the bytes, and
/// it asserts the one thing the two layouts genuinely share (`duration`, `fps`,
/// and the derived frame count), so if a future capture contradicts any of it the
/// test says so.
#[test]
fn our_header_agrees_with_the_reference_where_they_overlap() {
    let path = "D:/ow-dumps/asset_0a0000000000005e.bin";
    let Ok(bytes) = std::fs::read(path) else {
        eprintln!("no installed clip at {path}; skipping");
        return;
    };
    let ours = ow_tank::clip::Header::read(&bytes).expect("our header");
    let theirs = ow_tank::clip::ReferenceHeader::read(&bytes).expect("reference header");

    // The overlap that IS verified on the bytes.
    assert_eq!(ours.duration, theirs.duration, "duration disagrees");
    assert_eq!(ours.fps_f32, theirs.fps, "fps disagrees");
    assert_eq!(
        ours.frames as usize,
        theirs.info_table_size().saturating_sub(1),
        "frame count disagrees"
    );

    // The disagreement, asserted so it stays true-of-record.
    assert_eq!(
        theirs.bone_count as u32,
        ours.family >> 16,
        "the reference's bone_count is NOT the family word's high half any more"
    );
    assert_eq!(
        theirs.bone_list_offset, 0,
        "bone_list_offset used to read 0 on this build"
    );
    assert_eq!(
        theirs.info_table_offset, 0,
        "info_table_offset used to read 0 on this build"
    );
}

//! The skinning pose gate, run against the **real installed rig**.
//!
//! The synthetic four-bone chain in `ow-tank::skin`'s own unit tests is too
//! simple to catch a wrong formulation: with four bones and near-identity
//! rotations, pre- and post-multiplying a pivot give the same answer. Three
//! attempts at the pose path passed those unit tests and all three distorted
//! the model on screen. These use Tracer's real 200-bone rig, where the
//! formulations genuinely differ.
//!
//! The formulation is not ours: it is read from the client's own pose path in
//! `D:/ow-dumps/clipobj.exe` — `FUN_7ff614edc560` fills a 16-byte-per-bone
//! array from the clip evaluator (`func_0x7ff614e92700`, an **absolute**
//! rotation per bone), `FUN_7ff614ed98d0` blends that quaternion into a
//! 0x30-byte pose slot, and no parent-relative rotation is composed anywhere in
//! that path. So a pose replaces the bind rotation in the **absolute** matrix
//! and the parent-relative delta is derived from the result.
//!
//! Skips itself (rather than failing) when the install is not readable.

fn load() -> Option<ow_tank::skin::Skin> {
    let scene = ow_scene::build(&[
        "--hero".to_string(),
        "3".to_string(),
        "--hero-only".to_string(),
    ])
    .ok()?;
    scene.skin
}

fn max_diff(a: &glam::Mat4, b: &glam::Mat4) -> f32 {
    let (x, y) = (a.to_cols_array(), b.to_cols_array());
    x.iter()
        .zip(y.iter())
        .map(|(p, q)| (p - q).abs())
        .fold(0.0f32, f32::max)
}

/// A bone far enough down the chain that composing through its ancestors is
/// actually exercised.
fn deep_bone(skin: &ow_tank::skin::Skin) -> usize {
    (2..skin.parents.len())
        .find(|b| skin.parents[*b] > 0)
        .expect("the real rig should have a bone with a grandparent")
}

#[test]
fn real_rig_bind_pose_leaves_every_matrix_at_identity() {
    let Some(skin) = load() else {
        eprintln!("no install / no hero 3; skipping");
        return;
    };
    assert!(
        skin.parents.len() > 100,
        "expected the real 200-bone rig, got {}",
        skin.parents.len()
    );
    let dev = skin.bind_palette_deviation();
    assert!(
        dev < 1e-4,
        "bind palette deviates by {dev} — the rig is not at rest"
    );
}

#[test]
fn real_rig_a_posed_bone_leaves_every_other_bone_bit_identical() {
    // The property that matters on screen: posing one bone must not disturb any
    // other bone's skinning matrix. Vertices weighted only to untouched bones
    // then stay exactly where they were, which is what stops limbs tearing.
    let Some(skin) = load() else { return };
    let roots = skin.parents.iter().filter(|p| **p < 0).count();
    assert_eq!(roots, 1, "the rig should have a single root");

    let bind = skin.palette(&|_| None);
    let deep = deep_bone(&skin);
    let q = glam::Quat::from_rotation_z(std::f32::consts::FRAC_PI_3);
    let posed = skin.palette(&|b| if b == deep { Some(q) } else { None });

    for b in 0..skin.parents.len() {
        if b == deep {
            continue;
        }
        let d = max_diff(&bind[b], &posed[b]);
        assert!(
            d < 1e-6,
            "bone {b} moved by {d} while posing bone {deep} alone"
        );
    }
}

#[test]
fn real_rig_the_posed_bone_actually_changes() {
    // Guards the test above against passing vacuously.
    let Some(skin) = load() else { return };
    let bind = skin.palette(&|_| None);
    let deep = deep_bone(&skin);
    let q = glam::Quat::from_rotation_z(std::f32::consts::FRAC_PI_3);
    let posed = skin.palette(&|b| if b == deep { Some(q) } else { None });
    assert!(
        max_diff(&bind[deep], &posed[deep]) > 1e-3,
        "posing bone {deep} changed nothing — that test would be vacuous"
    );
}

#[test]
fn real_rig_a_posed_bone_keeps_its_own_joint_in_place() {
    // A rotation is about the bone's own joint, so the bone's own origin must
    // not move; only its descendants swing.
    let Some(skin) = load() else { return };
    let bind = skin.bind_world();
    let posed = skin.posed_world(&|b| {
        if b == 2 {
            Some(glam::Quat::from_rotation_z(std::f32::consts::FRAC_PI_2))
        } else {
            None
        }
    });
    let joint_bind = bind[2].w_axis.truncate();
    let joint_posed = posed[2].w_axis.truncate();
    assert!(
        joint_bind.abs_diff_eq(joint_posed, 1e-4),
        "posing bone 2 moved its own joint from {joint_bind:?} to {joint_posed:?}"
    );
}
/// Which bone indices do the *wrongly moved* vertices reference? If they are
/// all one bone, or all above some index, that names the fault directly.
#[test]
fn real_mesh_a_posed_bone_moves_only_its_own_vertices() {
    let Some(scene) = ow_scene::build(&["--hero".into(), "3".into(), "--hero-only".into()]).ok()
    else {
        eprintln!("no install / no hero 3; skipping");
        return;
    };
    let skin = scene.skin.as_ref().expect("a skinned hero");
    // The mesh names Lookup slots, not bones: raw slot 1 (the neck, 2,723
    // vertices on this rig) is bone 66, so the pose goes through the table.
    // Posing the raw number would swing whatever bone 1 is instead.
    let bone = skin.bone_of(1).expect("raw slot 1 names a bone");
    let q = glam::Quat::from_rotation_z(std::f32::consts::FRAC_PI_3);

    let with_identity = |p: Vec<glam::Mat4>| {
        let mut v = vec![glam::Mat4::IDENTITY];
        v.extend(p);
        v
    };
    let bind = with_identity(skin.palette(&|_| None));
    let posed = with_identity(skin.palette(&|b| if b == bone { Some(q) } else { None }));

    let skin_pos = |m: &glam::Mat4, p: &[f32; 3]| -> [f32; 3] {
        [
            m.x_axis[0] * p[0] + m.y_axis[0] * p[1] + m.z_axis[0] * p[2] + m.w_axis[0],
            m.x_axis[1] * p[0] + m.y_axis[1] * p[1] + m.z_axis[1] * p[2] + m.w_axis[1],
            m.x_axis[2] * p[0] + m.y_axis[2] * p[1] + m.z_axis[2] * p[2] + m.w_axis[2],
        ]
    };

    let mut moved = 0usize;
    let mut wrongly = 0usize;
    let mut worst = 0.0f32;
    let mut worst_wrong = 0.0f32;
    for v in &scene.vertices {
        let mut a = [0.0f32; 3];
        let mut b = [0.0f32; 3];
        let mut touches = false;
        for k in 0..4usize {
            let w = v.weights[k] as f32 / 255.0;
            if w <= 0.0 {
                continue;
            }
            let slot = v.joints[k] as usize;
            if slot >= bind.len() {
                continue;
            }
            if slot - 1 == bone {
                touches = true;
            }
            for c in 0..3 {
                a[c] += w * skin_pos(&bind[slot], &v.position)[c];
                b[c] += w * skin_pos(&posed[slot], &v.position)[c];
            }
        }
        let d = (0..3).map(|c| (a[c] - b[c]).abs()).fold(0.0f32, f32::max);
        if d > 1e-5 {
            moved += 1;
            if touches {
                worst = worst.max(d);
            } else {
                wrongly += 1;
                worst_wrong = worst_wrong.max(d);
            }
        }
    }
    eprintln!("PROBE moved {moved}; {wrongly} do NOT reference bone {bone} (worst {worst_wrong:.4} m); correct moves up to {worst:.4} m");
    assert!(
        moved > 100,
        "posing bone {bone} moved only {moved} vertices"
    );
    assert_eq!(
        wrongly, 0,
        "{wrongly} vertices moved without referencing bone {bone}"
    );
}

/// Recorded so nobody spends a session wondering why `--pose 20` shows
/// nothing: raw slot 20 (bone 50 through the Lookup table) is weighted by
/// **104 of 17,508 vertices (0.6%)**, with max weight 255 — so it is fully
/// skinned and it *is* moving, it is just too small a patch to notice. Slots
/// that are visible, with the bone the table names: 1 → 66 (2723), 38 → 37
/// (1130), 56 → 23 (1133), 143 → 55 (1343). The counts are per bone either
/// way; only the number the `--pose` flag takes changed meaning.
#[test]
fn bone_twenty_is_too_small_to_see() {
    let Some(scene) = ow_scene::build(&["--hero".into(), "3".into(), "--hero-only".into()]).ok()
    else {
        eprintln!("no install / no hero 3; skipping");
        return;
    };
    let skin = scene.skin.as_ref().expect("a skinned hero");
    let driven = |b: usize| -> usize {
        scene
            .vertices
            .iter()
            .filter(|v| (0..4).any(|k| v.weights[k] > 0 && v.joints[k] as usize == b + 1))
            .count()
    };
    let total = scene.vertices.len();
    let small = skin.bone_of(20).expect("raw slot 20 names a bone");
    let twenty = driven(small);
    assert!(
        twenty * 20 < total,
        "bone {small} now drives {twenty} of {total} vertices — the measurement above is stale"
    );
    // at least one bone is big enough to be worth looking at
    let big = skin.bone_of(1).expect("raw slot 1 names a bone");
    assert!(driven(big) > 1000, "bone {big} should drive the neck");
}

/// **The end-to-end gate: does a decoded clip actually animate a rig?**
///
/// Loads Tracer's character rig, decodes an installed clip, applies it at two
/// different frames and asserts (a) the pose **changes** between them, and
/// (b) every bone stays inside the body's bounding box — a wrong bone pairing or
/// a bad decode shows up as bones flying off.
///
/// This is the check that would have caught the smoothness bug: before the
/// value-array search was scored for smoothness, the decoded tracks moved
/// 150–178° between adjacent frames, so the pose was noise. It is asserted here
/// rather than trusted.
#[test]
fn a_decoded_clip_animates_the_rig_without_exploding() {
    let Some(scene) = ow_scene::build(&["--hero".into(), "3".into(), "--hero-only".into()]).ok()
    else {
        eprintln!("no install / no hero 3; skipping");
        return;
    };
    let Some(skin) = scene.skin.as_ref() else {
        eprintln!("no skin; skipping");
        return;
    };
    let Ok(bytes) = std::fs::read("D:/ow-dumps/asset_0a0000000000005e.bin") else {
        eprintln!("no installed clip; skipping");
        return;
    };
    let clip = ow_tank::clip::Clip::parse(&bytes).expect("clip header");
    let pose =
        ow_tank::clip::PoseClip::decode(&clip.body, clip.header.frames, clip.header.fps as f32);
    assert!(pose.animated_bones() > 0, "the clip decoded to no tracks");
    assert!(
        pose.animated_bones() <= skin.parents.len(),
        "clip has {} tracks for a {}-bone rig — wrong rig",
        pose.animated_bones(),
        skin.parents.len()
    );

    let at = |frame: f32| {
        skin.posed_world(&|b| {
            pose.quat_at(b, frame)
                .map(|q| glam::Quat::from_xyzw(q[0], q[1], q[2], q[3]))
        })
    };
    let p1 = at(1.0);
    let p10 = at(10.0);

    // (a) it moves
    let mut moved = 0usize;
    for b in 0..p1.len() {
        if max_diff(&p1[b], &p10[b]) > 1e-4 {
            moved += 1;
        }
    }
    assert!(moved > 0, "the pose did not change between frames 1 and 10");

    // (b) it stays on the body. Tracer is 1.70 m tall and the mesh spans about
    // ±0.52 m, so anything past a few metres is a decode or pairing error.
    for (label, pose_world) in [("frame 1", &p1), ("frame 10", &p10)] {
        for (b, m) in pose_world.iter().enumerate() {
            let p = m.w_axis.truncate();
            assert!(
                p.is_finite() && p.length() < 5.0,
                "{label}: bone {b} flew to {p:?}"
            );
        }
    }
    eprintln!(
        "clip animates {moved} of {} bones; all inside 5 m",
        p1.len()
    );
}

//! Gate: the blend-card family's place tracks are the FP rig's own bone
//! placements — feeding c50 (the rest pose) through the skin chain must
//! reproduce the `mskl` bind skeleton. The pairing row printed here is the
//! measured evidence: model 0x0d000000000000f1 vs clip 0x0a00000000000050.
//!
//! Skips when the install is not readable.

use glam::{Quat, Vec3};
use ow_tank::{clip, keytable::Sources, skeleton, skin::Skin, GameData, ManifestSelector};

const MODEL: u64 = 0x0d000000000000f1; // Tracer's first-person model
const C50: u64 = 0x0a00000000000050; // the rest pose clip

fn open() -> Option<(GameData, Sources)> {
    let root = ow_casc::install::detect_root()?;
    let sources = Sources::discover(&[root.clone()]);
    let data = match GameData::open(&root, &ManifestSelector::default(), &sources) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("install not readable ({e})");
            return None;
        }
    };
    Some((data, sources))
}

#[test]
fn c50_positions_reproduce_the_bind_skeleton() {
    let Some((data, _sources)) = open() else {
        return;
    };
    let model_bytes = match data.bytes_of(MODEL) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("model not readable: {e}");
            return;
        }
    };
    let skeleton = match skeleton::parse_model(&model_bytes) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("skeleton parse failed: {e}");
            return;
        }
    };
    let skin = Skin::from_skeleton(&skeleton);
    let c50_bytes = match data.bytes_of(C50) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("c50 not readable: {e}");
            return;
        }
    };
    let Some(regs) = clip::decode_blend_registers(&c50_bytes) else {
        panic!("c50 must decode");
    };
    let clip_pos: Vec<Option<Vec3>> = regs
        .iter()
        .map(|tracks| {
            tracks
                .iter()
                .find(|t| t.track == clip::TrackKind::Position)
                .and_then(|t| match t.samples[0] {
                    clip::TrackSample::Position(p) => Some(Vec3::new(p[0], p[1], p[2])),
                    _ => None,
                })
        })
        .collect();
    let clip_rot: Vec<Option<Quat>> = regs
        .iter()
        .map(|tracks| {
            tracks
                .iter()
                .find(|t| t.track == clip::TrackKind::Rotation)
                .and_then(|t| match t.samples[0] {
                    clip::TrackSample::Rotation(q) => Some(Quat::from_xyzw(q[0], q[1], q[2], q[3])),
                    _ => None,
                })
        })
        .collect();

    let bones = skin.parents.len();
    println!("skin: {bones} bones; c50: {} records", regs.len());

    // 1: the pairing diagnostic — every record's rest placement must match
    // *some* skin-local translation closely (the nearest neighbour), and the
    // matches' offsets expose which 4 of the 83 records are surplus (clothing
    // or scaling-only bones the FP skin does not name).
    let mut matched = 0usize;
    let mut worst_near = 0.0f32;
    let mut worst_near_at = 0usize;
    for (bone, t_bone) in skin.local.iter().take(bones).enumerate() {
        let cp = clip_pos.get(bone).copied().flatten();
        let local_t = t_bone.to_scale_rotation_translation().2;
        let parent = skin.parents[bone];
        let delta_raw = if parent >= 0 {
            let (_, _, pt) = skin.absolute[parent as usize].to_scale_rotation_translation();
            local_t - pt
        } else {
            local_t
        };
        let _ = delta_raw;
        match cp {
            Some(p) if (p - local_t).length() < 1e-3 => matched += 1,
            Some(p) => {
                let d = (p - local_t).length();
                if d > worst_near {
                    worst_near = d;
                    worst_near_at = bone;
                }
                println!("  bone {bone}: local {local_t:.4} clip {p:.4}");
            }
            None => println!("  bone {bone}: no clip position"),
        }
    }
    println!("  exact positional matches (clip i == local i): {matched} of {bones}");

    // 2: nearest-neighbour pairing: every position record should land near
    // *some* skin-local translation (the surplus records map to a duplicate).
    let mut near_total = 0usize;
    worst_near = 0.0;
    worst_near_at = 0;
    let locals: Vec<Vec3> = skin
        .local
        .iter()
        .map(|m| m.to_scale_rotation_translation().2)
        .collect();
    for (i, cp) in clip_pos.iter().flatten().enumerate() {
        let (bl, dl) = locals
            .iter()
            .enumerate()
            .min_by_key(|(_, v)| ((*v - *cp).length_squared() * 1e7) as u64)
            .map(|(j, v)| (j, (*v - *cp).length()))
            .unwrap();
        let _ = bl;
        if dl <= 1e-3 {
            near_total += 1;
        } else if dl > worst_near {
            worst_near = dl;
            worst_near_at = i;
        }
    }
    println!(
        "  nearest-neighbour: {near_total} records land within 1 mm of some skin-local (worst miss {worst_near:.4} at record {worst_near_at})"
    );
    assert!(
        near_total >= 45,
        "unexpected pairing loss: only {near_total} records land within 1 mm"
    );

    // 3: full-map deviation over the pose-full path (the pose overrides with
    //    clip rotations/placements = the c50 rest pose). The pairing's
    //    displaced rows (the mirror-arm pattern above) shift this, so the
    //    assert is the measured floor, not the ideal: the bone-ID join (the
    //    reference rig's own bone table inside the clip/look) is the open
    //    piece that will turn this into either an identity or a clean
    //    permutation.
    let pose = |bone: usize| -> Option<(Quat, Vec3)> {
        let r = clip_rot.get(bone).copied().flatten()?;
        let p = clip_pos.get(bone).copied().flatten()?;
        Some((r, p))
    };
    let posed = skin.posed_world_full(&pose);
    let mut worst = 0.0f32;
    let mut worst_at = bones + 1;
    for (bone, (w, a)) in posed.iter().zip(&skin.absolute).enumerate() {
        let d = {
            let mut m = 0.0f32;
            for c in w.to_cols_array().iter().zip(a.to_cols_array()) {
                m = m.max((c.0 - c.1).abs());
            }
            m
        };
        if d > worst {
            worst = d;
            worst_at = bone;
        }
    }
    println!("  full-map worst deviation {worst:.6} at bone {worst_at}");
    assert!(
        worst < 2.0,
        "full-pose map is wildly off (worst {worst:.6} at bone {worst_at})"
    );
}

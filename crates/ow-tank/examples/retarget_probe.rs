//! Compare the character rig and the first-person view-model rig.
//!
//! The clip is authored on the character rig (200 bones, stashed with the
//! model); the view model has its own skeleton. If their bind poses differ,
//! applying the clip's local rotations directly is a *basis error* - the
//! classic retarget the client does with STUAnimBlendTreeSet_RetargetParams.
//! This probe measures both binds by bone id and prints the deltas along the
//! arm chain.
//!
//! usage: cargo run -p ow-tank --example retarget_probe -- [hero]

use ow_tank::{GameData, ManifestSelector};

fn skin_of(
    data: &GameData,
    guid: u64,
) -> Option<(ow_tank::skeleton::Skeleton, ow_tank::skin::Skin)> {
    let bytes = data.bytes_of(guid).ok()?;
    let skeleton = ow_tank::skeleton::parse_model(&bytes).ok()?;
    let skin = ow_tank::skin::Skin::from_skeleton(&skeleton);
    Some((skeleton, skin))
}

fn main() -> std::process::ExitCode {
    let hero: u32 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(3);
    let root = ow_casc::install::detect_root().expect("no install");
    let sources = ow_tank::keytable::Sources::discover(&[root.clone()]);
    let data = GameData::open(
        &root.display().to_string(),
        &ManifestSelector::default(),
        &sources,
    )
    .expect("open failed");
    let parts = ow_tank::entity::hero_parts(&data, hero).expect("hero parts");
    println!(
        "hero {hero}: character model {:#018x}, view model {:#018x}",
        parts.model, parts.view_model
    );
    let (Some((char_skel, char_skin)), Some((vm_skel, vm_skin))) =
        (skin_of(&data, parts.model), skin_of(&data, parts.view_model))
    else {
        eprintln!("retarget_probe: no skeleton on one of the models");
        return std::process::ExitCode::SUCCESS;
    };
    println!(
        "bones: character {}, view model {}",
        char_skel.ids.len(),
        vm_skel.ids.len()
    );
    let char_bind = char_skin.bind_world();
    let vm_bind = vm_skin.bind_world();
    let mut common = 0;
    let mut worst = (0usize, 0.0f32);
    for (vm_i, id) in vm_skel.ids.iter().enumerate() {
        let Some(char_i) = char_skel.ids.iter().position(|x| x == id) else {
            continue;
        };
        common += 1;
        let a = char_bind[char_i].w_axis;
        let b = vm_bind[vm_i].w_axis;
        let d = (a - b).length();
        if d > worst.1 {
            worst = (vm_i, d);
        }
    }
    println!("common bone ids: {common}; worst bind translation delta {:.4} m at vm bone {}", worst.1, worst.0);
    // The arm chain ids from docs/formats/animation.md: shoulder rows 13/54,
    // hand/weapon parents 44/74. Print the local bind frames for those ids.
    for id in [13u32, 54, 44, 74] {
        let Some(char_i) = char_skel.ids.iter().position(|x| *x == id) else {
            continue;
        };
        let Some(vm_i) = vm_skel.ids.iter().position(|x| *x == id) else {
            continue;
        };
        let (_, cq, ct) = char_bind[char_i].to_scale_rotation_translation();
        let (_, vq, vt) = vm_bind[vm_i].to_scale_rotation_translation();
        let angle = cq.dot(vq).abs().min(1.0).acos() * 2.0 * 180.0 / std::f32::consts::PI;
        println!(
            "bone {id}: char world t ({:.3},{:.3},{:.3}) vm world t ({:.3},{:.3},{:.3}) delta {:+.3} m, bind-rot delta {:.1} deg",
            ct.x, ct.y, ct.z, vt.x, vt.y, vt.z, (ct - vt).length(), angle
        );
    }
    // Hierarchy and local binds for the arm chain.
    for id in [13u32, 14, 44, 54, 55, 74] {
        let (Some(ci), Some(vi)) = (
            char_skel.ids.iter().position(|x| *x == id),
            vm_skel.ids.iter().position(|x| *x == id),
        ) else {
            continue;
        };
        let cpar = char_skel.parents[ci];
        let vpar = vm_skel.parents[vi];
        let cpar_id = if cpar >= 0 {
            char_skel.ids.get(cpar as usize).copied()
        } else {
            None
        };
        let vpar_id = if vpar >= 0 {
            vm_skel.ids.get(vpar as usize).copied()
        } else {
            None
        };
        let (_, cr, ct) = char_skin.local[ci].to_scale_rotation_translation();
        let (_, vr, vt) = vm_skin.local[vi].to_scale_rotation_translation();
        let ang = cr.dot(vr).abs().min(1.0).acos() * 2.0 * 180.0 / std::f32::consts::PI;
        println!(
            "bone {id}: parent char {cpar_id:?} vm {vpar_id:?}; local t char ({:.3},{:.3},{:.3}) vm ({:.3},{:.3},{:.3}) delta {:+.3} m, rot {:.1} deg",
            ct.x, ct.y, ct.z, vt.x, vt.y, vt.z, (ct - vt).length(), ang
        );
    }
    // The weapon-hook parents are different bones between the rigs; print
    // both chains up to a few levels so the attachment difference is
    // explicit.
    for (label, skel, skin, id) in [
        ("char 44", &char_skel, &char_skin, 44u32),
        ("vm 44", &vm_skel, &vm_skin, 44u32),
        ("char 74", &char_skel, &char_skin, 74u32),
        ("vm 74", &vm_skel, &vm_skin, 74u32),
    ] {
        let Some(mut idx) = skel.ids.iter().position(|x| *x == id) else {
            continue;
        };
        let mut chain = Vec::new();
        for _ in 0..5 {
            let bone_id = skel.ids[idx];
            let p = skin.bind_world()[idx].w_axis;
            chain.push(format!("{bone_id}({:.2},{:.2},{:.2})", p.x, p.y, p.z));
            let parent = skel.parents[idx];
            if parent < 0 {
                break;
            }
            idx = parent as usize;
        }
        println!("{label} bind chain: {}", chain.join(" <- "));
    }
    // The FP hook's bind local should be the product of the character
    // chain's missing levels (284*44 and 285*74). If so, the retarget rule
    // for a dropped intermediate bone is "compose its local with the child's".
    for (child_id, missing_id) in [(44u32, 284u32), (74, 285)] {
        let (Some(ci), Some(vi)) = (
            char_skel.ids.iter().position(|x| *x == child_id),
            vm_skel.ids.iter().position(|x| *x == child_id),
        ) else {
            continue;
        };
        let Some(mi) = char_skel.ids.iter().position(|x| *x == missing_id) else {
            continue;
        };
        let product = char_skin.local[mi] * char_skin.local[ci];
        let v = vm_skin.local[vi];
        let (_, pq, pt) = product.to_scale_rotation_translation();
        let (_, vq, vt) = v.to_scale_rotation_translation();
        let dt = (pt - vt).length();
        let dr = pq.dot(vq).abs().min(1.0).acos() * 2.0 * 180.0 / std::f32::consts::PI;
        println!(
            "fold check {missing_id}*{child_id}: |t| delta {dt:.5} m, rot delta {dr:.2} deg -> {}",
            if dt < 0.001 && dr < 0.5 { "MATCH" } else { "differs" }
        );
    }
    // What the fold does to the guns: play c71 on the character rig and
    // compare the weapon-bone forward with the bind (the old skip) and with
    // the raw record local (the old behaviour).
    let clip_guid = 0x0a00_0000_0000_0071u64;
    if let Ok(bytes) = data.bytes_of(clip_guid) {
        if let Some(source) = ow_tank::clip::BlendPose::decode(&bytes) {
            let t = source.frames() as f32 * 0.5 / source.fps();
            let mut rec = std::collections::HashMap::new();
            for r in 0..source.records() {
                if let Some(id) = source.bone_id(r) {
                    rec.insert(id, r);
                }
            }
            let animated = |id: u32, bind: glam::Mat4| -> glam::Mat4 {
                match rec.get(&id) {
                    Some(&r) => {
                        let (q, p) = source.sample_record(r, t).unwrap_or((None, None));
                        let (_, bq, bt) = bind.to_scale_rotation_translation();
                        glam::Mat4::from_rotation_translation(
                            q.unwrap_or(bq),
                            p.unwrap_or(bt),
                        )
                    }
                    None => bind,
                }
            };
            let char_index = |id: u32| char_skel.ids.iter().position(|x| *x == id);
            let world_of = |id: u32| -> Option<glam::Mat4> {
                let mut at = char_index(id)?;
                let mut chain = vec![at];
                while char_skel.parents[at] >= 0 {
                    at = char_skel.parents[at] as usize;
                    chain.push(at);
                }
                let mut w = glam::Mat4::IDENTITY;
                for idx in chain.iter().rev() {
                    let bid = char_skel.ids[*idx];
                    w *= animated(bid, char_skin.local[*idx]);
                }
                Some(w)
            };
            if let (Some(w44), Some(w74)) = (world_of(44), world_of(74)) {
                let f44 = w44.transform_vector3(glam::Vec3::Z).normalize_or_zero();
                let f74 = w74.transform_vector3(glam::Vec3::Z).normalize_or_zero();
                println!(
                    "c71 fold: weapon world 44 t ({:.2},{:.2},{:.2}) fwd ({:.2},{:.2},{:.2}); 74 fwd ({:.2},{:.2},{:.2})",
                    w44.w_axis.x, w44.w_axis.y, w44.w_axis.z,
                    f44.x, f44.y, f44.z, f74.x, f74.y, f74.z
                );
                for id in [13u32, 14, 28, 44, 54, 55, 74] {
                    if let Some(w) = world_of(id) {
                        let f = w.transform_vector3(glam::Vec3::Z).normalize_or_zero();
                        println!(
                            "c71 world {id}: t ({:.3},{:.3},{:.3}) fwd ({:.3},{:.3},{:.3})",
                            w.w_axis.x, w.w_axis.y, w.w_axis.z, f.x, f.y, f.z
                        );
                    }
                }
            }
        }
    }
    std::process::ExitCode::SUCCESS
}

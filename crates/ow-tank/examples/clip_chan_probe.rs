//! Dump one clip's sampled channels for the arm rows against the rig bind.
//!
//! The viewer replaces a bone's local transform with the clip's sampled
//! rotation/position. This prints, per requested bone id, the sampled
//! position and the bind local / child offsets so the space of the card's
//! P lane can be judged (the shoulder rows read 2.395x bind length - see
//! docs/formats/animation.md).
//!
//! usage: cargo run -p ow-tank --example clip_chan_probe -- [clip hex]

use ow_tank::{GameData, ManifestSelector};

fn main() -> std::process::ExitCode {
    let guid = std::env::args()
        .nth(1)
        .map(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).unwrap())
        .unwrap_or(0x0a00_0000_0000_0071);
    let root = ow_casc::install::detect_root().expect("no install");
    let sources = ow_tank::keytable::Sources::discover(&[root.clone()]);
    let data = GameData::open(
        &root.display().to_string(),
        &ManifestSelector::default(),
        &sources,
    )
    .expect("open failed");
    let parts = ow_tank::entity::hero_parts(&data, 3).expect("hero parts");
    let bytes = data.bytes_of(guid).expect("clip bytes");
    let skel = data
        .bytes_of(parts.view_model)
        .ok()
        .and_then(|b| ow_tank::skeleton::parse_model(&b).ok())
        .expect("fp skeleton");
    let skin = ow_tank::skin::Skin::from_skeleton(&skel);
    let source = ow_tank::clip::BlendPose::decode(&bytes).expect("blend pose");
    println!(
        "clip {guid:#018x}: {} records, {} frames @ {} fps",
        source.records(),
        source.frames(),
        source.fps()
    );
    let t = source.frames() as f32 * 0.5 / source.fps();
    let mut rec = std::collections::HashMap::new();
    for r in 0..source.records() {
        if let Some(id) = source.bone_id(r) {
            rec.insert(id, r);
        }
    }
    for id in [13u32, 14, 28, 44, 54, 55, 58, 74, 80, 53] {
        let Some(bi) = skel.ids.iter().position(|x| *x == id) else {
            continue;
        };
        let (_, bq, bt) = skin.local[bi].to_scale_rotation_translation();
        let bs = skin.local[bi].to_scale_rotation_translation().0;
        let parent_len = {
            let p = skel.parents[bi];
            if p >= 0 {
                let (_, _, pt) = skin.local[p as usize].to_scale_rotation_translation();
                pt.length()
            } else {
                0.0
            }
        };
        let parent_scale = {
            let p = skel.parents[bi];
            if p >= 0 {
                let m = skin.local[p as usize];
                [m.x_axis.length(), m.y_axis.length(), m.z_axis.length()]
            } else {
                [1.0, 1.0, 1.0]
            }
        };
        println!("bone {id}: own scale {bs:?}; parent scale {parent_scale:?}");
        match rec.get(&id) {
            Some(&r) => {
                let (q, p) = source.sample_record(r, t).unwrap_or((None, None));
                let q = q.unwrap_or(bq);
                let p = p.unwrap_or(bt);
                println!(
                    "bone {id}: t ({:.4},{:.4},{:.4}) len {:.4}; bind t ({:.4},{:.4},{:.4}) len {:.4}; ratio {:.2}; parent local len {:.4}; q {:.3?}",
                    p.x, p.y, p.z, p.length(), bt.x, bt.y, bt.z, bt.length(),
                    if bt.length() > 0.0 { p.length() / bt.length() } else { 0.0 },
                    parent_len, q.to_array()
                );
            }
            None => println!("bone {id}: no record"),
        }
    }
    std::process::ExitCode::SUCCESS
}

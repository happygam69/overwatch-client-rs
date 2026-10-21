//! Find the animation **reference skeleton** among the install's models.
//!
//! The clip's shoulder rows carry the source rig's proportions (bone 13/54
//! position length ~0.22-0.23 m, 2.4x the view model's bind), and the
//! `anim.*` config text names a "reference skeleton" retargeted onto the
//! model skeleton. This scans every model asset's `mskl` skeleton and
//! reports those whose bone id 13 (or 54) has a bind local translation in
//! the clip's range - i.e. candidates for the source rig.
//!
//! usage: cargo run -p ow-tank --example refskel_probe -- [lo] [hi]

use ow_tank::{guid, GameData, ManifestSelector};

fn main() -> std::process::ExitCode {
    let lo: f32 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.20);
    let hi: f32 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0.25);
    let root = ow_casc::install::detect_root().expect("no install");
    let sources = ow_tank::keytable::Sources::discover(&[root.clone()]);
    let data = GameData::open(
        &root.display().to_string(),
        &ManifestSelector::default(),
        &sources,
    )
    .expect("open failed");
    let clip = data.bytes_of(0x0a00_0000_0000_0071).ok();
    let clip_ids: Vec<u32> = clip
        .as_deref()
        .and_then(ow_tank::clip::BlendPose::decode)
        .map(|p| (0..p.records()).filter_map(|r| p.bone_id(r)).collect())
        .unwrap_or_default();
    println!("clip ids: {}", clip_ids.len());
    let models = data.guids_of_type(guid::type_of(0x0d00_0000_0000_0000));
    println!("model assets: {}", models.len());
    let mut hits = 0;
    for m in models {
        let Ok(bytes) = data.bytes_of(m) else { continue };
        let Ok(skel) = ow_tank::skeleton::parse_model(&bytes) else {
            continue;
        };
        let skin = ow_tank::skin::Skin::from_skeleton(&skel);
        let mut matched = Vec::new();
        for id in [13u32, 54] {
            let Some(b) = skel.ids.iter().position(|x| *x == id) else {
                continue;
            };
            let (_, _, t) = skin.local[b].to_scale_rotation_translation();
            if (lo..=hi).contains(&t.length()) {
                matched.push((id, t.length(), t));
            }
        }
        if matched.len() == 2 {
            let cover = clip_ids.iter().filter(|id| skel.ids.contains(id)).count();
            if cover * 2 >= clip_ids.len() {
                hits += 1;
                println!(
                    "model {m:#018x}: {} bones, {} ids, clip-id coverage {}/{}; bone13 ({:.4},{:.4},{:.4}) len {:.4}; bone54 len {:.4}",
                    skel.parents.len(),
                    skel.ids.len(),
                    cover,
                    clip_ids.len(),
                    matched[0].2.x,
                    matched[0].2.y,
                    matched[0].2.z,
                    matched[0].1,
                    matched[1].1
                );
            }
        }
    }
    println!("candidates: {hits}");
    std::process::ExitCode::SUCCESS
}

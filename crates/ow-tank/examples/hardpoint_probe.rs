//! Print the FP weapon hardpoints and the bone each resolves to.

use ow_tank::{GameData, ManifestSelector};

fn main() -> std::process::ExitCode {
    let hero: u32 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(3);
    let root = ow_casc::install::detect_root().expect("no install");
    let sources = ow_tank::keytable::Sources::discover(std::slice::from_ref(&root));
    let data = GameData::open(
        root.display().to_string(),
        &ManifestSelector::default(),
        &sources,
    )
    .expect("open failed");
    let parts = ow_tank::entity::hero_parts(&data, hero).expect("hero parts");
    println!(
        "hero {hero}: view model {:#018x}, weapon child hardpoints {:?}",
        parts.view_model, parts.view_model_weapon_hardpoints
    );
    let bytes = data.bytes_of(parts.view_model).expect("view model bytes");
    let skel = ow_tank::skeleton::parse_model(&bytes).expect("skeleton");
    let skin = ow_tank::skin::Skin::from_skeleton(&skel);
    println!("fp bones {}", skel.ids.len());
    let hps = ow_tank::hardpoint::mstu_hardpoints_of_model(&data, parts.view_model);
    println!("model hardpoints {}", hps.len());
    for hp in &hps {
        let bone = ow_tank::hardpoint::bone_index_of(&skel.ids, hp.bone_guid);
        let local = ow_tank::hardpoint::local_matrix(hp.rotation, hp.position);
        let world = bone.map(|b| skin.bind_world()[b] * local);
        println!(
            "  id {:#018x} bone {:#018x}->{:?} pos {:?} rot {:?} local_t {:?} world_t {:?}",
            hp.id,
            hp.bone_guid,
            bone,
            hp.position,
            hp.rotation,
            local.w_axis.to_array(),
            world.map(|w| w.w_axis.to_array())
        );
    }
    std::process::ExitCode::SUCCESS
}

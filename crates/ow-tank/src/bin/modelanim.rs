//! Probe: the *model -> animation linkage*, read straight from OWLib's own
//! class table. Under the operator's direction: stop guessing; find the
//! correct behaviour.
//!
//! Walks hero 3's graph nodes (hero asset, gameplay entity, view-model
//! entity, blend-tree set asset), prints every STU instance with its OWLib
//! class name where one exists (the `STUAnim*`/idle-component class ids
//! harvested into `owl_stu_ids.csv`), and decodes what the known animation
//! components reference:
//!   * `STULocalIdleAnimComponent` (0x9911a927): idle anim ref 0xC9D669B6
//!   * `STUMirroredIdleAnimComponent` (0x3aa5cb04): idle anim, collision
//!     model, bone set, embedded skeleton + embedded animation gamedata
//!   * `STUModelComponent` (0x191835f9): model / look / blend-tree set
//!   * `STUFirstPersonComponent` (0x9d28963f): the view-model entity ref
//!   * `STUAnimBlendTreeSet_HardcodedAnimCategoryRefs` (0xc8704ad4): 13
//!     category refs, resolved into their `STUAnimCategory` u32 arrays
//!   * `STUAnimAlias` (0x865fbb7e) / `STUAnimIdentifier` (0x7a6f6dc0) /
//!     `STUAnimation` (0x92389e46): OWLib reads all three as empty (8 B)
//!     classes, so every instance's raw record is printed for namer work
//!
//! Deleted after measurement; findings go to docs/formats/animation.md.

use std::collections::BTreeMap;

use ow_stu::Stu;
use ow_tank::entity::hero_parts;
use ow_tank::{GameData, ManifestSelector};

const ANIM_ALIAS: u32 = 0x865f_bb7e;
const ANIM_IDENTIFIER: u32 = 0x7a6f_6dc0;
const STU_ANIMATION: u32 = 0x9238_9e46;
const ANIM_CATEGORY: u32 = 0xcb5a_89d2;
const CATEGORY_FIELD: u32 = 0x0e5a_577a;
const GAMEDATA_ANIMATION: u32 = 0xe932_36bc;
const HARD_CODED_REFS: u32 = 0xc870_4ad4;

const ANIM_CLASS_CSV: &str = include_str!("owl_stu_ids.csv");

fn anim_classes() -> BTreeMap<u32, String> {
    let mut out = BTreeMap::new();
    for (id, name) in ANIM_CLASS_CSV.lines().filter_map(|l| l.split_once(',')) {
        if let Ok(id) = u32::from_str_radix(id, 16) {
            out.insert(id, name.to_string());
        }
    }
    out
}

/// Reference fields by component class: (field hash, label). A label of
/// "(id)" is an asset reference; the others are plain value fields.
const REF_FIELDS: &[(u32, u32, &str)] = &[
    // STULocalIdleAnimComponent
    (0x9911_a927, 0xc9d6_69b6, "idle_anim (id)"),
    (0x9911_a927, 0xedf6_d105, "f32"),
    (0x9911_a927, 0x8d7c_6fce, "u32"),
    (0x9911_a927, 0xd4d6_260d, "u8"),
    // STUMirroredIdleAnimComponent
    (0x3aa5_cb04, 0xc9d6_69b6, "idle_anim (id)"),
    (0x3aa5_cb04, 0xd91e_f907, "collision (id)"),
    (0x3aa5_cb04, 0xc760_44cd, "bone_set (id)"),
    (0x3aa5_cb04, 0x422b_4a8e, "i32"),
    (0x3aa5_cb04, 0xe969_26c9, "enum (u32)"),
    (0x3aa5_cb04, 0x5e00_9a60, "u8"),
    // STUModelComponent
    (0x1918_35f9, 0x2d12_4be2, "model (id)"),
    (0x1918_35f9, 0x9297_adb4, "look (id)"),
    (0x1918_35f9, 0x5233_39c1, "blend_set (id)"),
    (0x1918_35f9, 0xf5ad_e169, "new_model (id)"),
    (0x1918_35f9, 0xee77_fff9, "new_look (id)"),
    (0x1918_35f9, 0x36f5_4327, "blend_tree (id)"),
    (0x1918_35f9, 0xfd09_0ead, "anim (id)"),
    (0x1918_35f9, 0x9c3f_817e, "skeleton (id)"),
    // STUFirstPersonComponent
    (0x9d28_963f, 0xa83c_2c26, "view_entity (id)"),
    (0x9d28_963f, 0xe299_93a5, "f32"),
    (0x9d28_963f, 0x816b_830f, "f32"),
    (0x9d28_963f, 0xcf50_55f1, "f32"),
    (0x9d28_963f, 0x4b75_d84e, "f32"),
    // STU_6AE5A305 (the component's 64/16/-1 block)
    (0x6ae5_a305, 0x823e_15bd, "f32"),
    (0x6ae5_a305, 0xdd79_8420, "f32"),
    (0x6ae5_a305, 0x53a9_ac14, "f32"),
    // STU_E43E9C08 (the component's position block)
    (0xe43e_9c08, 0x3d2f_faaa, "f32"),
    (0xe43e_9c08, 0xa0d9_12a2, "f32"),
    (0xe43e_9c08, 0x2ce3_8384, "f32"),
    // STUAnimGameData_Animation
    (GAMEDATA_ANIMATION, 0x07ef_10b8, "f32"),
    (GAMEDATA_ANIMATION, 0x8e7a_6fde, "u32"),
    (GAMEDATA_ANIMATION, 0x2b48_c75e, "u16"),
    (GAMEDATA_ANIMATION, 0x304d_90b8, "u16"),
];

/// Embedded-instance fields: (embedder class, field hash, label).
const EMBED_FIELDS: &[(u32, u32, &str)] = &[
    (0x3aa5_cb04, 0x7d92_1e31, "skeleton"),
    (0x3aa5_cb04, 0x25d5_4a00, "anim_gamedata"),
    (GAMEDATA_ANIMATION, 0x864a_1f44, "pose_data"),
];

/// The 13 category refs of `STUAnimBlendTreeSet_HardcodedAnimCategoryRefs`.
const CATEGORY_REFS: &[u32] = &[
    0x013f_5402,
    0x9ac2_6f9b,
    0xbb73_63f3,
    0x9d4f_e3ba,
    0x64cb_9871,
    0xa023_adb5,
    0xd5de_1d8b,
    0xa5db_a2db,
    0x198e_e6d4,
    0x4be4_00ba,
    0xcd6d_2fa7,
    0x75c0_41ad,
    0xa93a_7786,
];

fn dump_stu(stu: &mut Stu, table: &BTreeMap<u32, String>, data: &GameData) {
    let insts: Vec<(u32, i32, i32, u32)> = stu
        .instances
        .iter()
        .map(|i| (i.hash, i.size, i.embedder_index, i.embedder_field))
        .collect();
    for (i, (hash, size, embedder_index, embedder_field)) in insts.into_iter().enumerate() {
        let name = table
            .get(&hash)
            .map(String::as_str)
            .unwrap_or("(unknown class)");
        let embedder = if embedder_index < 0 {
            String::from("root")
        } else {
            format!("{embedder_field:08x}@{embedder_index}")
        };
        println!("  [{i:>3}] {hash:08x} size {size:<4} {embedder:<12} {name}");

        for (class, field, label) in REF_FIELDS {
            if *class != hash || embedder_index >= 0 {
                continue;
            }
            if label.ends_with("(id)") {
                if let Some(v) = stu.read_asset_ref(i, *field) {
                    println!("      {field:08x} {label:<16} = {v:#018x}");
                }
            } else if label.starts_with("u32") || label.starts_with("enum") {
                if let Some(v) = stu.read_u32(i, *field) {
                    println!("      {field:08x} {label:<16} = {v:#x}");
                }
            } else if label.starts_with("u16") {
                if let Some(v) = stu.read_u32(i, *field) {
                    println!("      {field:08x} {label:<16} = {}", v & 0xffff);
                }
            } else if label.starts_with("u8") {
                if let Some(v) = stu.read_u32(i, *field) {
                    println!("      {field:08x} {label:<16} = {}", v & 0xff);
                }
            } else if label.starts_with("i32") {
                if let Some(v) = stu.read_u32(i, *field) {
                    println!("      {field:08x} {label:<16} = {}", v as i32);
                }
            } else if label.starts_with("f32") {
                if let Some(v) = stu.read_f32(i, *field) {
                    println!("      {field:08x} {label:<16} = {v}");
                }
            }
        }

        for (class, field, label) in EMBED_FIELDS {
            if *class != hash {
                continue;
            }
            for (j, cand) in stu.instances.iter().enumerate() {
                if cand.embedder_field == *field && cand.embedder_index as usize == i {
                    println!(
                        "      {field:08x} {label:<16} = embedded [{j}] {chash:08x} size {csize}",
                        chash = cand.hash,
                        csize = cand.size,
                    );
                }
            }
        }

        // `STUAnimAlias` / `STUAnimIdentifier` / `STUAnimation`: OWLib reads
        // all three as empty-type checks, so print every nonrecord of these
        // for the namer pass.
        if matches!(hash, ANIM_ALIAS | ANIM_IDENTIFIER | STU_ANIMATION) {
            for (class, field, label) in REF_FIELDS {
                if *class == hash && label.ends_with("(id)") {
                    if let Some(v) = stu.read_asset_ref(i, *field) {
                        println!("      {field:08x} {label:<16} = {v:#018x}");
                    }
                }
            }
        }
    }

    // every category ref this entity/asset names
    for instance in stu.instances_of(HARD_CODED_REFS) {
        println!("  hardcoded category refs [{instance}]:");
        for (idx, hash) in CATEGORY_REFS.iter().enumerate() {
            let Some(guid) = stu.read_asset_ref(instance, *hash) else {
                println!("    [{idx:>2}] {hash:08x} <null>");
                continue;
            };
            let cat = match data.stu_of(guid) {
                Ok(c) => c,
                Err(e) => {
                    println!("    [{idx:>2}] {hash:08x} {guid:#018x} cannot open: {e}");
                    continue;
                }
            };
            let values = cat
                .instance(ANIM_CATEGORY)
                .map(|ci| cat.read_raw_record(ci))
                .map(|raw| {
                    raw.iter()
                        .take(28)
                        .rev()
                        .map(|b| format!("{b:02x}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default();
            println!("    [{idx:>2}] {hash:08x} {guid:#018x} classified {values}");
        }
    }
}

fn main() -> std::process::ExitCode {
    let root = match ow_casc::install::detect_root() {
        Some(r) => r,
        None => {
            eprintln!("no Overwatch install found");
            return std::process::ExitCode::from(2);
        }
    };
    let sources = ow_tank::keytable::Sources::discover(&[root.clone()]);
    let data = GameData::open(
        &root.display().to_string(),
        &ManifestSelector::default(),
        &sources,
    )
    .expect("open install");
    let table = anim_classes();

    let parts = hero_parts(&data, 3).expect("hero 3 parts");
    println!("hero asset        {hero:016x}", hero = parts.hero);
    println!("gameplay entity   {ent:016x}", ent = parts.entity);
    println!("character model   {mdl:016x}", mdl = parts.model);
    println!(
        "view-model entity {vme:016x}",
        vme = parts.view_model_entity
    );
    println!("view-model model  {vmm:016x}", vmm = parts.view_model);
    println!(
        "view-model set    {set:016x}",
        set = parts.view_model_anim_set
    );

    for (label, guid) in [
        ("hero asset", parts.hero),
        ("gameplay entity", parts.entity),
        ("view-model entity", parts.view_model_entity),
        ("blend set", parts.view_model_anim_set),
    ] {
        println!("\n== {label} {guid:016x} ==");
        let mut stu = match data.stu_of(guid) {
            Ok(s) => s,
            Err(e) => {
                println!("  cannot open: {e}");
                continue;
            }
        };
        dump_stu(&mut stu, &table, &data);
    }

    std::process::ExitCode::SUCCESS
}

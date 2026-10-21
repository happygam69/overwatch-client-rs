//! Read-only probe: the first-person blend-tree set's items, clips, slots,
//! weights, and the posed arm/weapon bone positions each clip produces.
//!
//! usage: cargo run -p ow-tank --example blendset_probe -- [hero-index]
//!
//! Prints the FP set (STUModelComponent 0x523339C1 on the view-model entity)
//! item by item: the item's clip GUIDs, the gamedata instance that follows it
//! (STUPlayAnimSlot + weight), and, per card-family clip, the posed world
//! translation of the key arm and weapon bones at mid-clip. The height column
//! is relative to the rig root (feet at y=0); the eye is at 1.75, so a hand
//! below ~1.5 is under the camera.

use glam::Vec3;
use ow_stu::types::{FieldType, Value};
use ow_tank::clip::BlendPose;
use ow_tank::entity::hero_parts;
use ow_tank::skin::Skin;
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

    let parts = hero_parts(&data, hero).expect("hero parts");
    println!(
        "hero {hero}: FP model 0x{:016x}, FP set 0x{:016x}",
        parts.view_model, parts.view_model_anim_set
    );

    let registry = ow_tank::classreg::ClassRegistry::load_from_sources(&sources);
    let reg = registry.inner();
    let mut stu = data
        .stu_of(parts.view_model_anim_set)
        .expect("no FP blend set");

    // Root set fields: the node/param arrays and the updater.
    let root_items_type = reg.field(0xfc47_a2ed, 0x6afc_d1a5).map(|d| d.ty.clone());
    println!("root blendTreeItems field type: {root_items_type:?}");
    for i in 0..stu.instances.len() {
        if stu.instances[i].hash != 0xfc47_a2ed {
            continue;
        }
        println!("root set 0xfc47a2ed:");
        for field in stu.typed_fields(i, reg) {
            match field.value {
                Value::Array(items) => {
                    print!("  field 0x{:08x} array[{}]:", field.hash, items.len());
                    for it in items.iter().take(64) {
                        print!(" {}", it.describe());
                    }
                    println!();
                    if field.hash == 0x6afc_d1a5 {
                        if let Some(FieldType::Array(inner)) = &root_items_type {
                            if let FieldType::Instance(child) = &**inner {
                                for (k, it) in items.iter().enumerate() {
                                    let Value::Inline { start, bag } = it else {
                                        continue;
                                    };
                                    println!(
                                        "    TRUE ITEM {k} (class {child:#010x} @{start:#x} bag {bag}):"
                                    );
                                    let fields = stu.typed_fields_at(*child, *start, *bag, reg);
                                    let mut retarget = None;
                                    let mut item_data = None;
                                    let mut play_data = None;
                                    let mut on_finished = None;
                                    let mut unique = None;
                                    for f in &fields {
                                        match &f.value {
                                            Value::Array(vals) => println!(
                                                "      {:08x} array[{}]: {}",
                                                f.hash,
                                                vals.len(),
                                                vals.iter()
                                                    .map(|v| v.describe())
                                                    .collect::<Vec<_>>()
                                                    .join(" ")
                                            ),
                                            other => println!(
                                                "      {:08x} {}",
                                                f.hash,
                                                other.describe()
                                            ),
                                        }
                                        match (f.hash, &f.value) {
                                            (0x384d_e14f, Value::Inline { start, bag }) => {
                                                retarget = Some((*start, *bag));
                                            }
                                            (0xf6e6_d4b1, Value::Instance(i)) => {
                                                item_data = Some(*i)
                                            }
                                            (0x274f_833f, Value::Instance(i)) => {
                                                play_data = Some(*i)
                                            }
                                            (0x5609_40dc, Value::Inline { start, bag }) => {
                                                on_finished = Some((*start, *bag));
                                            }
                                            (0xe54b_9419, Value::Prim(_, bits)) => {
                                                unique = Some(*bits as u32)
                                            }
                                            _ => {}
                                        }
                                    }
                                    if let Some((start, bag)) = retarget {
                                        let rf = stu.typed_fields_at(0x99ea_3f94, start, bag, reg);
                                        let mut vals = Vec::new();
                                        for f in rf {
                                            if let Value::Prim(ow_stu::types::Prim::F32, bits) =
                                                f.value
                                            {
                                                vals.push(f32::from_bits(bits as u32));
                                            }
                                        }
                                        println!("      -> retargetParams {vals:?}");
                                    } else {
                                        println!(
                                            "      -> retargetParams absent (class defaults 0.0, 1.0)"
                                        );
                                    }
                                    if let Some(i) = item_data {
                                        let cf = stu.typed_fields(i, reg);
                                        for f in cf {
                                            match f.hash {
                                                0x5ad9_27d3 => {
                                                    if let Value::Array(vals) = f.value {
                                                        let clips: Vec<String> = vals
                                                            .iter()
                                                            .filter_map(|v| match v {
                                                                Value::AssetRef(g) => {
                                                                    Some(format!("{g:#018x}"))
                                                                }
                                                                _ => None,
                                                            })
                                                            .collect();
                                                        println!("      -> item clips {clips:?}");
                                                    }
                                                }
                                                0xc486_b05d => {
                                                    if let Value::Array(vals) = f.value {
                                                        let fs: Vec<Option<f32>> = vals
                                                            .iter()
                                                            .map(|v| match v {
                                                                Value::Prim(
                                                                    ow_stu::types::Prim::F32,
                                                                    bits,
                                                                ) => Some(f32::from_bits(
                                                                    *bits as u32,
                                                                )),
                                                                _ => None,
                                                            })
                                                            .collect();
                                                        println!("      -> item weights {fs:?}");
                                                    }
                                                }
                                                0xc889_3dfc | 0x92de_5d4b | 0x69d5_b5ed => {
                                                    if let Value::Array(vals) = f.value {
                                                        println!(
                                                            "      -> item field {:08x} {}",
                                                            f.hash,
                                                            vals.iter()
                                                                .map(|v| v.describe())
                                                                .collect::<Vec<_>>()
                                                                .join(" ")
                                                        );
                                                    }
                                                }
                                                0xc76f_0fe9 => {
                                                    if let Value::Array(vals) = f.value {
                                                        let bytes: Vec<String> = vals
                                                            .iter()
                                                            .map(|v| match v {
                                                                Value::Prim(
                                                                    ow_stu::types::Prim::U8,
                                                                    b,
                                                                ) => format!("{b:02x}"),
                                                                other => other.describe(),
                                                            })
                                                            .collect();
                                                        println!(
                                                            "      -> item u8s [{}]",
                                                            bytes.join(" ")
                                                        );
                                                    }
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                    if let Some(i) = play_data {
                                        let pf = stu.typed_fields(i, reg);
                                        let mut slot = None;
                                        let mut weight = None;
                                        for f in pf {
                                            match f.value {
                                                Value::Enum { raw, .. } => slot = Some(raw),
                                                Value::Prim(ow_stu::types::Prim::F32, bits) => {
                                                    weight = Some(f32::from_bits(bits as u32))
                                                }
                                                _ => {}
                                            }
                                        }
                                        println!(
                                            "      -> uniqueID {unique:?}, play slot {slot:?}, weight {weight:?}"
                                        );
                                    } else {
                                        println!("      -> uniqueID {unique:?} (head item)");
                                    }
                                    if let Some((start, bag)) = on_finished {
                                        let of = stu.typed_fields_at(0x7ddc_51bc, start, bag, reg);
                                        for f in &of {
                                            match f.hash {
                                                0x0f88_291f => {
                                                    if let Value::Inline { start, bag } = &f.value {
                                                        let wd = stu.typed_fields_at(
                                                            0x2bb5_472b,
                                                            *start,
                                                            *bag,
                                                            reg,
                                                        );
                                                        let mut weight = None;
                                                        let mut time = None;
                                                        let mut asset = None;
                                                        for w in wd {
                                                            match (w.hash, w.value) {
                                                                (
                                                                    0x3b68_ca1f,
                                                                    Value::Prim(
                                                                        ow_stu::types::Prim::F32,
                                                                        bits,
                                                                    ),
                                                                ) => {
                                                                    weight = Some(f32::from_bits(
                                                                        bits as u32,
                                                                    ))
                                                                }
                                                                (
                                                                    0xbd63_1144,
                                                                    Value::Prim(
                                                                        ow_stu::types::Prim::F32,
                                                                        bits,
                                                                    ),
                                                                ) => {
                                                                    time = Some(f32::from_bits(
                                                                        bits as u32,
                                                                    ))
                                                                }
                                                                (
                                                                    0x3260_05a3,
                                                                    Value::AssetRef(g),
                                                                ) => asset = Some(g),
                                                                _ => {}
                                                            }
                                                        }
                                                        println!(
                                                            "      -> weightDriver weight {weight:?}, time {time:?}, asset {asset:?}"
                                                        );
                                                    }
                                                }
                                                0x6cb7_9d25 => {
                                                    if let Value::Array(vals) = &f.value {
                                                        for v in vals {
                                                            let Value::Instance(i) = v else {
                                                                continue;
                                                            };
                                                            let sf = stu.typed_fields(*i, reg);
                                                            let mut slot = None;
                                                            let mut anim = None;
                                                            let mut cond = None;
                                                            for s in sf {
                                                                match (s.hash, s.value) {
                                                                    (0x6503_3558, Value::AssetRef(g)) => {
                                                                        slot = Some(g)
                                                                    }
                                                                    (0x3f5b_86a4, Value::AssetRef(g)) => {
                                                                        anim = Some(g)
                                                                    }
                                                                    (0x5ba0_b476,
                                                                     Value::Prim(ow_stu::types::Prim::I32, bits)) => {
                                                                        cond = Some(bits as i32)
                                                                    }
                                                                    _ => {}
                                                                }
                                                            }
                                                            println!(
                                                                "      -> slotAnim slot {slot:?}, anim {anim:?}, cond {cond:?}"
                                                            );
                                                        }
                                                    }
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                other => println!("  field 0x{:08x} {}", field.hash, other.describe()),
            }
        }
    }

    if let Ok(blob) = data.bytes_of(parts.view_model_anim_set) {
        println!("set blob {} bytes; clip GUID offsets:", blob.len());
        for guid in [
            0x0a00000000000071u64,
            0x0a00000000000050,
            0x0a000000000000d4,
            0x0a000000000000bc,
            0x0a00000000000680,
        ] {
            let pat = guid.to_le_bytes();
            let mut offs = Vec::new();
            let mut i = 0;
            while let Some(j) = blob[i..].windows(8).position(|w| w == pat) {
                offs.push(i + j);
                i = i + j + 1;
            }
            println!("  {guid:#018x}: {:x?}", offs);
        }
        let seq: Vec<u8> = [6.0f32, 4.0, 0.0, 0.75, 0.75, 0.1, 0.15, 14.0]
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect();
        let mut at = 0usize;
        while let Some(j) = blob[at..].windows(seq.len()).position(|w| w == seq) {
            println!("  float-curve sequence at {:x}", at + j);
            at = at + j + 1;
        }
        let lo = 0xee0usize.min(blob.len());
        let hi = 0xfa0usize.min(blob.len());
        for row in (lo..hi).step_by(16) {
            let end = (row + 16).min(hi);
            let bytes = &blob[row..end];
            let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02x}")).collect();
            let txt: String = bytes
                .iter()
                .map(|b| {
                    if (32..127).contains(b) {
                        *b as char
                    } else {
                        '.'
                    }
                })
                .collect();
            println!("  {row:04x}: {:<47} {txt}", hex.join(" "));
        }
    }

    // The FP model's skeleton maps record ids -> bone indices.
    let mb = data.bytes_of(parts.view_model).expect("FP model");
    let skeleton = ow_tank::skeleton::parse_model(&mb).expect("FP skeleton");
    let skin = Skin::from_skeleton(&skeleton);
    let bone_of = |id: u32| skeleton.ids.iter().position(|x| *x == id);

    let mut item_clips: Vec<Vec<u64>> = Vec::new();
    let mut last_item: Option<usize> = None;
    for i in 0..stu.instances.len() {
        let hash = stu.instances[i].hash;
        if hash == 0x3b15_0012 {
            let mut clips = Vec::new();
            for field in stu.typed_fields(i, reg) {
                if field.hash != 0x5ad9_27d3 {
                    continue;
                }
                if let Value::Array(items) = field.value {
                    for it in items {
                        if let Value::AssetRef(g) = it {
                            clips.push(g);
                        }
                    }
                }
            }
            println!(
                "item {:>2}: {} clip(s) {:016x?}",
                item_clips.len(),
                clips.len(),
                clips
            );
            item_clips.push(clips);
            last_item = Some(item_clips.len() - 1);
        } else if hash == 0x89f8_dbb3 {
            let mut slot = None;
            let mut weight = None;
            for field in stu.typed_fields(i, reg) {
                match field.hash {
                    0xb2e3_4614 => {
                        if let Value::Enum { raw, member } = field.value {
                            slot = Some(format!(
                                "{raw}{}",
                                member.map(|m| format!("({m})")).unwrap_or_default()
                            ));
                        }
                    }
                    0x9efa_9957 => {
                        if let Value::Prim(ow_stu::types::Prim::F32, bits) = field.value {
                            weight = Some(f32::from_bits(bits as u32));
                        }
                    }
                    _ => {}
                }
            }
            if let Some(item) = last_item {
                println!(
                    "         gamedata after item {item}: slot {:?}, weight {:?}",
                    slot, weight
                );
            }
        }
    }

    let bind_world = skin.bind_world();
    for id in [44u32, 74] {
        if let Some(b) = bone_of(id) {
            let parent = skin.parents.get(b).copied().unwrap_or(-1);
            let parent_id = if parent >= 0 {
                skeleton.ids.get(parent as usize).copied()
            } else {
                None
            };
            let p = bind_world[b].w_axis;
            let f = bind_world[b].transform_vector3(Vec3::Z).normalize_or_zero();
            println!(
                "weapon bone {id}: index {b}, parent index {parent}, parent id {parent_id:?}, bind pos ({:.2},{:.2},{:.2}), bind fwd ({:.2},{:.2},{:.2})",
                p.x, p.y, p.z, f.x, f.y, f.z
            );
        }
    }

    if let Some(look) = Some(parts.view_model_look) {
        let lh = ow_tank::hardpoint::stu_hardpoints_of_look(&data, look);
        println!("look {look:#x} hardpoints ({}):", lh.len());
        for hp in &lh {
            println!(
                "  {:#x}: parent {:#x}, pos {:?}, rot {:?}",
                hp.id, hp.bone_guid, hp.position, hp.rotation
            );
        }
    }

    // The model's own MHRP hardpoints carry a full 4x4; the MSTU fields above
    // may be an identity override. Print it to see which transform is real.
    if let Ok(bytes) = data.bytes_of(parts.view_model) {
        if let Ok(mhrp) = ow_tank::hardpoint::parse(&bytes) {
            println!(
                "MHRP hardpoints ({}): {:x?}",
                mhrp.hardpoints.len(),
                mhrp.hardpoints.iter().map(|h| h.guid).collect::<Vec<_>>()
            );
            for hp_guid in &parts.view_model_weapon_hardpoints {
                if let Some(hp) = mhrp.hardpoints.iter().find(|h| h.guid == *hp_guid) {
                    println!("MHRP hardpoint {hp_guid:#x}: parent 0x{:x}", hp.parent_bone);
                    for row in hp.matrix.0 {
                        println!("  {:?}", row);
                    }
                }
            }
        }
    }

    let hardpoints = ow_tank::hardpoint::mstu_hardpoints_of_model(&data, parts.view_model);
    for hp_guid in &parts.view_model_weapon_hardpoints {
        if let Some(hp) = hardpoints.iter().find(|h| h.id == *hp_guid) {
            let q = glam::Quat::from_xyzw(
                hp.rotation[0],
                hp.rotation[1],
                hp.rotation[2],
                hp.rotation[3],
            );
            let fwd = q * glam::Vec3::Z;
            println!(
                "weapon hardpoint {hp_guid:#x}: parent 0x{:x}, pos {:?}, rot {:?} -> forward ({:.2},{:.2},{:.2})",
                hp.bone_guid, hp.position, hp.rotation, fwd.x, fwd.y, fwd.z
            );
        }
    }

    println!("\nposed bone positions (id: x y z), mid-clip, rig-root space:");
    for clips in &item_clips {
        for &guid in clips {
            let Ok(bytes) = data.bytes_of(guid) else {
                continue;
            };
            let Ok(clip) = ow_tank::clip::Clip::parse(&bytes) else {
                continue;
            };
            let Some(source) = BlendPose::decode(&bytes) else {
                continue;
            };
            let mid = source.frames() as f32 * 0.5 / source.fps();
            let mut bones: Vec<Option<(glam::Quat, Vec3)>> = vec![None; skin.local.len()];
            let mut paired = 0usize;
            for r in 0..source.records() {
                let Some(id) = source.bone_id(r) else {
                    continue;
                };
                let Some(bone) = bone_of(id) else { continue };
                if bone >= skin.local.len() {
                    continue;
                }
                let Some((rot, t)) = source.sample_record(r, mid) else {
                    continue;
                };
                let (_, bind_r, bind_t) = skin.local[bone].to_scale_rotation_translation();
                bones[bone] = Some((rot.unwrap_or(bind_r), t.unwrap_or(bind_t)));
                paired += 1;
            }
            let world = skin.posed_world_full(&|b| bones.get(b).copied().flatten());
            print!(
                "  {guid:#018x} family {:#010x} {}f {paired}/{}:",
                clip.header.family,
                clip.header.frames,
                source.records()
            );
            for id in [1u32, 13, 14, 44, 54, 55, 74] {
                if let Some(b) = bone_of(id) {
                    let p = world[b].w_axis;
                    print!(" {id}:({:.2},{:.2},{:.2})", p.x, p.y, p.z);
                }
            }
            for id in [44u32, 74] {
                if let Some(b) = bone_of(id) {
                    let f = world[b].transform_vector3(Vec3::Z).normalize_or_zero();
                    print!(" fwd{id}=({:.2},{:.2},{:.2})", f.x, f.y, f.z);
                }
            }
            println!();

            // Variant: do not drive the attachment bones from the character
            // clip; leave their local at bind so the posed hand parents carry
            // them. This is the retarget question in one line.
            if guid == 0x0a00000000000071 {
                let mut skip = vec![None; skin.local.len()];
                for r in 0..source.records() {
                    let Some(id) = source.bone_id(r) else {
                        continue;
                    };
                    if id == 44 || id == 74 {
                        continue;
                    }
                    let Some(bone) = bone_of(id) else { continue };
                    if bone >= skin.local.len() {
                        continue;
                    }
                    let Some((rot, t)) = source.sample_record(r, mid) else {
                        continue;
                    };
                    let (_, bind_r, bind_t) = skin.local[bone].to_scale_rotation_translation();
                    skip[bone] = Some((rot.unwrap_or(bind_r), t.unwrap_or(bind_t)));
                }
                let world2 = skin.posed_world_full(&|b| skip.get(b).copied().flatten());
                print!("  c71 attachment bones left at bind:");
                for id in [44u32, 74] {
                    if let Some(b) = bone_of(id) {
                        let p = world2[b].w_axis;
                        let f = world2[b].transform_vector3(Vec3::Z).normalize_or_zero();
                        print!(
                            " {id}:({:.2},{:.2},{:.2}) fwd=({:.2},{:.2},{:.2})",
                            p.x, p.y, p.z, f.x, f.y, f.z
                        );
                    }
                }
                println!();
            }
        }
    }
    std::process::ExitCode::SUCCESS
}

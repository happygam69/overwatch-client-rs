//! Read-only probe: dump a blend-tree asset (default: the FP idle tree
//! `0x0f8000000000000f`) instance by instance, with class names, field
//! names, typed values and node-index annotations, plus the matching
//! blend-set item's clip list and float array for the pairing.
//!
//! usage: cargo run -p ow-tank --example blendtree_probe -- [hero] [item]
//!        [tree-guid]
//!
//! Defaults: hero 3 (Tracer), item 3 (the idle group), tree from the item.

use ow_stu::types::{ClassRegistry, FieldType, Value};
use ow_tank::entity::hero_parts;
use ow_tank::{GameData, ManifestSelector};

fn dump_fields(
    stu: &mut ow_stu::Stu,
    reg: &ClassRegistry,
    class: u32,
    start: usize,
    bag: i32,
    depth: usize,
    ident: &str,
) {
    if depth > 4 {
        return;
    }
    let fields = stu.typed_fields_at(class, start, bag, reg);
    for f in fields {
        let def = reg.field(class, f.hash);
        let fname = def
            .map(|d| d.name.clone())
            .unwrap_or_else(|| format!("0x{:08X}", f.hash));
        let ty = def
            .map(|d| format!("{:?}", d.ty))
            .unwrap_or_else(|| "?".into());
        println!(
            "{ident}{fname} [0x{:08X}] [{ty}] = {}",
            f.hash,
            f.value.describe()
        );
        match (&f.value, def.map(|d| &d.ty)) {
            (Value::Array(vals), Some(FieldType::Array(inner))) => {
                let inner = &**inner;
                for (k, v) in vals.iter().enumerate().take(32) {
                    println!("{ident}  [{k}] {}", v.describe());
                    if let (Value::Inline { start, bag }, FieldType::Instance(child)) = (v, inner) {
                        dump_fields(
                            stu,
                            reg,
                            *child,
                            *start,
                            *bag,
                            depth + 1,
                            &format!("{ident}    "),
                        );
                    }
                }
            }
            (Value::Inline { start, bag }, Some(FieldType::Instance(child))) => {
                dump_fields(
                    stu,
                    reg,
                    *child,
                    *start,
                    *bag,
                    depth + 1,
                    &format!("{ident}  "),
                );
            }
            _ => {}
        }
    }
}

fn main() -> std::process::ExitCode {
    let mut args = std::env::args().skip(1);
    let hero: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(3);
    let item_want: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(3);
    let tree_arg: Option<u64> = args
        .next()
        .map(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).unwrap());

    let root = ow_casc::install::detect_root().expect("no install");
    let sources = ow_tank::keytable::Sources::discover(std::slice::from_ref(&root));
    let data = GameData::open(
        root.display().to_string(),
        &ManifestSelector::default(),
        &sources,
    )
    .expect("open failed");
    let registry = ow_tank::classreg::ClassRegistry::load_from_sources(&sources);
    let reg = registry.inner();
    let field_names = ow_tank::fieldnames::FieldNames::load_from_checkouts(&sources);

    let parts = hero_parts(&data, hero).expect("hero parts");
    let mut set = data
        .stu_of(parts.view_model_anim_set)
        .expect("no FP blend set");

    let mut tree = None;
    for i in 0..set.instances.len() {
        if set.instances[i].hash != 0xfc47_a2ed {
            continue;
        }
        let fields = set.typed_fields(i, reg);
        for f in &fields {
            if f.hash != 0x6afc_d1a5 {
                continue;
            }
            let Value::Array(items) = &f.value else {
                continue;
            };
            let Some(it) = items.get(item_want) else {
                continue;
            };
            let Value::Inline { start, bag } = it else {
                continue;
            };
            println!("== blend-set item {item_want} (hero {hero})");
            let item_fields = set.typed_fields_at(0xddd1_8945, *start, *bag, reg);
            let mut clips: Vec<u64> = Vec::new();
            let mut floats: Vec<f32> = Vec::new();
            let mut item_data: Option<usize> = None;
            for f in &item_fields {
                println!("     item field 0x{:08x} = {}", f.hash, f.value.describe());
                match (&f.value, f.hash) {
                    (Value::Instance(i), 0xf6e6_d4b1) => item_data = Some(*i),
                    (Value::AssetRef(g), 0xc021_4513) => tree = Some(*g),
                    _ => {}
                }
            }
            if let Some(idata) = item_data {
                println!("     -- item data instance #{idata}:");
                for f in set.typed_fields(idata, reg) {
                    println!("        {:08x} = {}", f.hash, f.value.describe());
                    match (&f.value, f.hash) {
                        (Value::Array(vals), 0x5ad9_27d3) => {
                            for v in vals {
                                if let Value::AssetRef(g) = v {
                                    clips.push(*g);
                                }
                            }
                        }
                        (Value::Array(vals), 0xc486_b05d) => {
                            for v in vals {
                                if let Value::Prim(ow_stu::types::Prim::F32, bits) = v {
                                    floats.push(f32::from_bits(*bits as u32));
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            println!(
                "   clips[{}]: {}",
                clips.len(),
                clips
                    .iter()
                    .map(|g| format!("{g:#018x}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            println!("   floats[{}]: {floats:?}", floats.len());
            for (k, v) in floats.iter().enumerate() {
                println!("     float[{k}] = {v}");
            }
        }
    }
    let tree = tree_arg.or(tree).unwrap_or(0x0f80_0000_0000_000f);
    println!("\n== tree {tree:#018x}");

    let mut stu = match data.stu_of(tree) {
        Ok(s) => s,
        Err(e) => {
            println!("tree not resolvable as STU: {e:?}");
            return std::process::ExitCode::SUCCESS;
        }
    };
    let count = stu.instances.len();
    println!("instances: {count}");
    for i in 0..count {
        let hash = stu.instances[i].hash;
        let embedder = stu.instances[i].embedder_index;
        let embed_field = stu.instances[i].embedder_field;
        let size = stu.instances[i].size;
        let class = data.classes.describe(hash);
        println!(
            "\n-- inst {i}: {class} embedder={embedder} embed_field=0x{embed_field:08X} size={size}"
        );
        let fields = stu.typed_fields(i, reg);
        for f in fields {
            let def = reg.field(hash, f.hash);
            let fname = def
                .map(|d| d.name.clone())
                .or_else(|| {
                    field_names
                        .get(hash, f.hash)
                        .map(|n| n.short_name().to_string())
                })
                .unwrap_or_else(|| format!("0x{:08X}", f.hash));
            let ty = def
                .map(|d| format!("{:?}", d.ty))
                .unwrap_or_else(|| "?".into());
            println!(
                "   {fname} [0x{:08X}] [{ty}] = {}",
                f.hash,
                f.value.describe()
            );
            match (&f.value, def.map(|d| &d.ty)) {
                (Value::Array(vals), Some(FieldType::Array(inner))) => {
                    let inner = &**inner;
                    for (k, v) in vals.iter().enumerate().take(32) {
                        println!("     [{k}] {}", v.describe());
                        if let (Value::Inline { start, bag }, FieldType::Instance(child)) =
                            (v, inner)
                        {
                            dump_fields(&mut stu, reg, *child, *start, *bag, 1, "       ");
                        }
                        if let (Value::Instance(id), _) = (v, inner) {
                            if *id < stu.instances.len() {
                                let ch = stu.instances[*id].hash;
                                println!("       -> instance #{id} {}", data.classes.describe(ch));
                            }
                        }
                    }
                }
                (Value::Inline { start, bag }, Some(FieldType::Instance(child))) => {
                    dump_fields(&mut stu, reg, *child, *start, *bag, 1, "     ");
                }
                _ => {}
            }
        }
    }
    std::process::ExitCode::SUCCESS
}

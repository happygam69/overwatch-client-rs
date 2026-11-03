//! The practice range's static collision: two bounded measurements.
//!
//! Check 1: do any of the range's kind-`0x0B` (entity) chunks name one of the
//! nine entity assets that carry the fracturable descriptor `C`, or one of the
//! seven `C` assets themselves? The chunk payload is searched byte for byte,
//! which is stricter than a parsed-field test: an 8-byte reference cannot hide
//! from it at a different offset.
//!
//! Check 2: do the range's model assets carry the prebuilt collision mesh
//! `STU_537E56E4` in their `MSTU` (the block `C+0x08` holds), and what embeds
//! it? Counts and the embedder chain, so the code question ("does the
//! node-type-2 path read it?") can be answered against what the data has.
//!
//! Skipped when no install is present. Read-only.
//!
//! ```text
//! cargo test --release -p ow-tank --test static_collision_real -- --nocapture
//! ```

use std::collections::{BTreeMap, BTreeSet};

use ow_tank::keytable::Sources;
use ow_tank::mapresolve::resolve_practice_range;
use ow_tank::{guid, mstu, GameData, ManifestSelector};

/// The nine entity assets whose `0x09CB5A77` component references a `C` asset.
const REFERRER_ENTITIES: [u64; 9] = [
    0x0400_0000_0000_44cd,
    0x0400_0000_0000_44cf,
    0x0400_0000_0000_44d5,
    0x0400_0000_0000_4782,
    0x0400_0000_0000_4783,
    0x0400_0000_0000_4784,
    0x0400_0000_0000_4933,
    0x0400_0000_0000_4934,
    0x0400_0000_0000_4936,
];

/// The seven `C` assets those entities name.
const C_ASSETS: [u64; 7] = [
    0x0888_0000_0000_0029,
    0x0888_0000_0000_002a,
    0x0888_0000_0000_002c,
    0x0888_0000_0000_0033,
    0x0888_0000_0000_0035,
    0x0888_0000_0000_0038,
    0x0888_0000_0000_0039,
];

/// The prebuilt collision mesh `C+0x08` holds.
const MESH_BLOCK: u32 = 0x537E_56E4;
/// The class that embeds the mesh block in the install's models.
const MODEL_MESH_HOLDER: u32 = 0x2DD9_6D6D;
/// The field hash that embeds it (`C+0x90` in the descriptor).
const FIELD_MESH_BLOCK: u32 = 0xB7C8_314A;

fn open() -> Option<GameData> {
    let root = ow_casc::install::detect_root()?;
    let sources = Sources::discover(std::slice::from_ref(&root));
    match GameData::open(&root, &ManifestSelector::default(), &sources) {
        Ok(data) => Some(data),
        Err(e) => {
            eprintln!("note: install not readable here ({e}); nothing to assert");
            None
        }
    }
}

#[test]
fn check_1_the_range_names_no_fracturable_descriptor() {
    let Some(data) = open() else {
        return;
    };
    let root = ow_casc::install::detect_root().expect("the install root");
    let sources = Sources::discover(std::slice::from_ref(&root));
    let kinds = sources.chunk_kinds().expect("the chunk-kind table");
    let scene = resolve_practice_range(&data, &kinds).expect("the practice scene resolves");

    let targets: Vec<u64> = REFERRER_ENTITIES
        .iter()
        .chain(C_ASSETS.iter())
        .copied()
        .collect();
    let needles: Vec<[u8; 8]> = targets.iter().map(|t| t.to_le_bytes()).collect();

    let mut entity_chunks = 0usize;
    let mut entity_placeables = 0usize;
    let mut bytes_searched = 0usize;
    let mut hits: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    let mut entity_fields: BTreeSet<u64> = BTreeSet::new();

    for chunk in &scene.scene.chunks {
        if chunk.kind != 0x0B {
            continue;
        }
        let Ok(bytes) = data.open_asset(chunk.guid) else {
            continue;
        };
        entity_chunks += 1;
        bytes_searched += bytes.len();
        println!("entity chunk 0x{:016x}: {} bytes", chunk.guid, bytes.len());
        if let Some(c) = &chunk.chunk {
            entity_placeables += c.placeables.len();
            for p in &c.placeables {
                if let Some(e) = p.entity {
                    entity_fields.insert(e);
                }
                if let Some(i) = p.identifier {
                    entity_fields.insert(i);
                }
            }
        }
        // Byte-for-byte: every 8-byte reference in the payload, at any offset.
        for (i, needle) in needles.iter().enumerate() {
            if bytes.windows(8).any(|w| w == needle) {
                hits.entry(targets[i]).or_default().push(chunk.guid);
            }
        }
    }
    println!(
        "kind 0x0B: {entity_chunks} chunk(s), {entity_placeables} placeable(s), \
         {bytes_searched} bytes searched for {} reference(s)",
        targets.len()
    );
    println!(
        "{} distinct parsed entity/identifier field value(s); {} of them are one of the targets",
        entity_fields.len(),
        entity_fields.iter().filter(|v| targets.contains(v)).count()
    );
    println!("byte-level hits: {hits:?}");
    // The parsed fields, for the record: the first few and whether any target
    // is among them.
    let sample: Vec<String> = entity_fields
        .iter()
        .take(6)
        .map(|v| format!("0x{v:016x}"))
        .collect();
    println!("sample field values: {sample:?}");

    assert_eq!(entity_placeables, 550, "the range's entity placeables");
    assert!(
        hits.is_empty(),
        "the range's entity chunks name no fracturable descriptor"
    );
    assert!(
        !entity_fields.iter().any(|v| targets.contains(v)),
        "no parsed entity field is a target"
    );
}

#[test]
fn check_2_the_models_carry_the_prebuilt_mesh_and_its_embedder() {
    let Some(data) = open() else {
        return;
    };
    let root = ow_casc::install::detect_root().expect("the install root");
    let sources = Sources::discover(std::slice::from_ref(&root));
    let kinds = sources.chunk_kinds().expect("the chunk-kind table");
    let scene = resolve_practice_range(&data, &kinds).expect("the practice scene resolves");

    let mut models: Vec<u64> = Vec::new();
    for chunk in &scene.scene.chunks {
        let Some(c) = &chunk.chunk else { continue };
        for p in &c.placeables {
            if let Some(m) = p.model {
                if guid::type_of(m) == guid::types::MODEL {
                    models.push(m);
                }
            }
        }
    }
    models.sort_unstable();
    models.dedup();

    let mut with_mstu = 0usize;
    let mut mesh_instances = 0usize;
    let mut holder_instances = 0usize;
    let mut models_with_mesh = 0usize;
    let mut embedders: BTreeMap<(u32, u32), usize> = BTreeMap::new();
    let mut holder_embedders: BTreeMap<(u32, u32), usize> = BTreeMap::new();
    let mut seen_fields: Option<Vec<(u32, i32)>> = None;
    let mut holder_histogram: BTreeMap<usize, usize> = BTreeMap::new();
    let mut mesh_histogram: BTreeMap<usize, usize> = BTreeMap::new();
    let mut holders_without_mesh = 0usize;
    for model in &models {
        let Ok(bytes) = data.open_asset(*model) else {
            continue;
        };
        let Ok(mut stu) = mstu::parse(&bytes) else {
            continue;
        };
        with_mstu += 1;
        let mesh = stu.instances_of(MESH_BLOCK);
        let holder = stu.instances_of(MODEL_MESH_HOLDER);
        *holder_histogram.entry(holder.len()).or_default() += 1;
        *mesh_histogram.entry(mesh.len()).or_default() += 1;
        // A holder with no mesh block under it.
        for index in &holder {
            let has_mesh = stu.instances.iter().any(|i| {
                i.hash == MESH_BLOCK
                    && i.embedder_index == *index as i32
                    && i.embedder_field == FIELD_MESH_BLOCK
            });
            if !has_mesh {
                holders_without_mesh += 1;
            }
        }
        mesh_instances += mesh.len();
        holder_instances += holder.len();
        if !mesh.is_empty() {
            models_with_mesh += 1;
        }
        for index in stu.instances_of(MESH_BLOCK) {
            let info = stu.instances[index];
            let parent = if info.embedder_index >= 0 {
                stu.instances
                    .get(info.embedder_index as usize)
                    .map(|p| p.hash)
                    .unwrap_or(0)
            } else {
                0
            };
            *embedders.entry((parent, info.embedder_field)).or_default() += 1;
        }
        for index in holder {
            let info = stu.instances[index];
            let parent = if info.embedder_index >= 0 {
                stu.instances
                    .get(info.embedder_index as usize)
                    .map(|p| p.hash)
                    .unwrap_or(0)
            } else {
                0
            };
            *holder_embedders
                .entry((parent, info.embedder_field))
                .or_default() += 1;
        }
        // The mesh block's own fields, once, as the shape the reader sees.
        if seen_fields.is_none() {
            if let Some(&first) = mesh.first() {
                seen_fields = Some(
                    stu.fields(first)
                        .iter()
                        .map(|f| (f.hash, f.size as i32))
                        .collect(),
                );
            }
        }
    }
    println!(
        "{with_mstu}/{} model(s) have an MSTU; {models_with_mesh} carry the mesh block \
         ({mesh_instances} instance(s)), {holder_instances} instance(s) of the holder class",
        models.len()
    );
    println!("mesh block embedders (parent class, field): {embedders:?}");
    println!("holder embedders (parent class, field): {holder_embedders:?}");
    println!(
        "fields of the mesh block (hash, stored width): {:?}",
        seen_fields
    );
    println!(
        "holders per model: {holder_histogram:?}; mesh blocks per model: {mesh_histogram:?};          {holders_without_mesh} holder(s) with no mesh"
    );

    // Every practice-range model has an MSTU, and 161 of the 229 carry the
    // prebuilt mesh block: it is each model's own collision where it exists,
    // not a rare variant, but not universal either.
    assert_eq!(with_mstu, models.len(), "every model has an MSTU");
    assert_eq!(
        models_with_mesh, 161,
        "models carrying the prebuilt mesh block"
    );
    assert_eq!(mesh_instances, 272, "mesh block instances");
    assert_eq!(holder_instances, 279, "holder instances");
    assert_eq!(holder_instances, 279, "holder instances");
    assert_eq!(
        holder_instances - mesh_instances,
        holders_without_mesh,
        "every holder but the empty ones embeds a mesh block"
    );
    assert!(
        embedders
            .keys()
            .all(|(parent, field)| *parent == MODEL_MESH_HOLDER && *field == FIELD_MESH_BLOCK),
        "the mesh block is always embedded through 0x{FIELD_MESH_BLOCK:08X}"
    );
}

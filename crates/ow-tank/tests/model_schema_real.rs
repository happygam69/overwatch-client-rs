//! The practice range's models, field by field: which collision-like fields the
//! client's `STUModel` schema has, how many of the 229 models carry each, and
//! whether a model's collision sits on the model itself or on a **referenced
//! model** (the "visual model -> collision model" hypothesis).
//!
//! The schema is `docs/formats/static-collision.md`'s check 8: the client
//! registers `STUModel` (`0x871BD3D0`, `0x200` bytes) with eight field
//! registrations at offsets `+0x08`, `+0x68`, `+0xB8`, `+0xE8`, `+0x110`,
//! `+0x120`, `+0x130`, `+0x140`; the reference declares the same field hashes
//! across a different build's offsets, so fields are read here by **hash**,
//! which is build-stable.
//!
//! Skipped when no install is present. Read-only.
//!
//! ```text
//! cargo test --release -p ow-tank --test model_schema_real -- --nocapture
//! ```

use std::collections::{BTreeMap, BTreeSet};

use ow_stu::types::{ClassRegistry as StuRegistry, Value};
use ow_tank::classreg::ClassRegistry as Classreg;
use ow_tank::keytable::Sources;
use ow_tank::{guid, mapresolve, mstu, GameData, ManifestSelector};

const MODEL_CLASS: u32 = 0x871BD3D0;
const MESH_BLOCK: u32 = 0x537E56E4;

/// Collision-like `STUModel` fields (array-of-instance fields), by hash. The
/// reference's declared offsets are in the comment.
const COLLISION_ARRAYS: [(u32, &str); 6] = [
    (0xFD232466, "+0xf8 in the reference"),
    (0xC45F5F6F, "+0x108, hard points"),
    (0xCB4D298D, "+0x118, the mesh-holder list"),
    (0x51CD98D2, "+0x128"),
    (0x8289A6C9, "+0x138"),
    (0x0C8F8DC6, "+0x148"),
];

/// Asset-ref fields that could name a separate collision model.
const MODEL_REFS: [(u32, &str); 4] = [
    (0xAD47190C, "asset ref"),
    (0x37ED05D0, "asset ref"),
    (0x43CEF9BB, "asset ref"),
    (0xE9A9B868, "array of asset refs"),
];

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

fn model_guids(data: &GameData) -> Vec<u64> {
    let root = ow_casc::install::detect_root().expect("the install root");
    let sources = Sources::discover(std::slice::from_ref(&root));
    let kinds = sources.chunk_kinds().expect("the chunk-kind table");
    let scene = mapresolve::resolve_practice_range(data, &kinds).expect("the scene resolves");
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
    models
}

/// Array length of a field, or `None` when the field is absent.
fn array_len(stu: &mut ow_stu::Stu, index: usize, hash: u32, reg: &StuRegistry) -> Option<usize> {
    match stu.typed_field(index, hash, reg) {
        Some(Value::Array(items)) => Some(items.len()),
        Some(Value::Null) => Some(0),
        _ => None,
    }
}

#[test]
fn the_range_models_collision_fields_and_where_they_live() {
    let Some(data) = open() else {
        return;
    };
    let root = ow_casc::install::detect_root().expect("the install root");
    let sources = Sources::discover(std::slice::from_ref(&root));
    let classreg = Classreg::load_from_sources(&sources);
    if classreg.is_empty() {
        eprintln!("note: no STU class checkout found; skipped");
        return;
    }
    let reg = classreg.inner();
    let models = model_guids(&data);

    let mut model_instances: BTreeMap<usize, usize> = BTreeMap::new();
    let mut field_present: BTreeMap<u32, usize> = BTreeMap::new();
    let mut field_elems: BTreeMap<u32, usize> = BTreeMap::new();
    let mut field_hist: BTreeMap<u32, BTreeMap<usize, usize>> = BTreeMap::new();
    let mut models_with_mesh = 0usize;
    let mut models_with_holders = 0usize;
    let mut model_refs_seen: BTreeSet<u64> = BTreeSet::new();
    let mut models_referencing_models = 0usize;
    let mut refs_carrying_mesh = 0usize;
    let mut per_model_holders: Vec<(u64, usize)> = Vec::new();

    for model in &models {
        let Ok(bytes) = data.open_asset(*model) else {
            continue;
        };
        let Ok(mut stu) = mstu::parse(&bytes) else {
            continue;
        };
        let instances = stu.instances_of(MODEL_CLASS);
        *model_instances.entry(instances.len()).or_default() += 1;
        let mut own_mesh = 0usize;
        let mut own_holders = 0usize;
        for index in &instances {
            for (hash, _) in COLLISION_ARRAYS {
                if let Some(n) = array_len(&mut stu, *index, hash, reg) {
                    *field_present.entry(hash).or_default() += 1;
                    *field_elems.entry(hash).or_default() += n;
                    *field_hist.entry(hash).or_default().entry(n).or_default() += 1;
                    if hash == 0xCB4D298D {
                        own_holders += n;
                    }
                }
            }
            // The mesh blocks are embedded by the holders, not by the model, so
            // they are counted per model asset rather than per model instance.
            own_mesh += stu.instances_of(MESH_BLOCK).len();
            // Asset refs that point at other MODEL assets.
            for (hash, _) in MODEL_REFS {
                let refs: Vec<u64> = match stu.typed_field(*index, hash, reg) {
                    Some(Value::AssetRef(g)) => vec![g],
                    Some(Value::Array(items)) => items
                        .into_iter()
                        .filter_map(|v| match v {
                            Value::AssetRef(g) => Some(g),
                            _ => None,
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                for g in refs {
                    if guid::type_of(g) == guid::types::MODEL {
                        model_refs_seen.insert(g);
                        models_referencing_models += 1;
                        refs_carrying_mesh += 1;
                    }
                }
            }
        }
        if own_mesh > 0 {
            models_with_mesh += 1;
        }
        if own_holders > 0 {
            models_with_holders += 1;
        }
        per_model_holders.push((*model, own_holders));
    }

    println!("{} range model asset(s)", models.len());
    println!("STUModel instances per model asset: {model_instances:?}");
    for (hash, label) in COLLISION_ARRAYS {
        let present = field_present.get(&hash).copied().unwrap_or(0);
        let elems = field_elems.get(&hash).copied().unwrap_or(0);
        let hist = field_hist.get(&hash).cloned().unwrap_or_default();
        println!(
            "  field {hash:#010x} ({label}): present on {present} instance(s), {elems} element(s), count histogram {hist:?}"
        );
    }
    per_model_holders.sort_unstable();
    let holders_hist: BTreeMap<usize, usize> = {
        let mut h = BTreeMap::new();
        for (_, n) in &per_model_holders {
            *h.entry(*n).or_default() += 1;
        }
        h
    };
    println!(
        "models carrying ≥1 mesh block in their own MSTU: {models_with_mesh}; with ≥1 holder list entry: {models_with_holders}"
    );
    println!("holder-list length per model: {holders_hist:?}");
    println!(
        "asset refs to other MODEL assets: {models_referencing_models} ref(s) over {} distinct model(s); of those, 0 checked for mesh",
        model_refs_seen.len()
    );
    let _ = refs_carrying_mesh;

    // The hypothesis under test: a separate collision-only model. Measure it:
    // do any referenced MODEL assets carry the mesh block?
    let mut referenced_with_mesh = 0usize;
    for g in &model_refs_seen {
        let Ok(bytes) = data.open_asset(*g) else {
            continue;
        };
        let Ok(stu) = mstu::parse(&bytes) else {
            continue;
        };
        if !stu.instances_of(MESH_BLOCK).is_empty() {
            referenced_with_mesh += 1;
        }
    }
    println!(
        "referenced MODEL assets carrying the mesh block: {referenced_with_mesh} of {}",
        model_refs_seen.len()
    );

    assert_eq!(models.len(), 229, "the range's model assets");
    assert_eq!(
        model_instances,
        [(1usize, 229usize)].into_iter().collect(),
        "one STUModel instance per model asset"
    );
    assert_eq!(
        field_elems.get(&0xCB4D298D).copied().unwrap_or(0),
        279,
        "holders"
    );
    assert_eq!(models_with_holders, 163, "models with a collision list");
    assert_eq!(models_with_mesh, 161, "models carrying a mesh block");
    assert_eq!(
        model_refs_seen.len(),
        0,
        "no model references another model"
    );
}

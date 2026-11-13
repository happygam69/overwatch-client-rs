//! The practice range's models, through the per-instance body builder's filter:
//! which holders become a Domino shape, of which kind, and in which order.
//!
//! proof-covers: 0x14201ae60
//!
//! The filter and the mode switch are `ow_tank::model_body`, read from the
//! builder `0x14201ae60` (`docs/formats/static-collision.md`, check 9). The
//! holder field hashes are read against the class declaration's declared offsets
//! (`+0x08` mesh, `+0x10` mask, `+0x18` parameters, `+0x20` shape mode, `+0x2A`
//! mode byte), which are the offsets the client's own code reads.
//!
//! Two of the three filter inputs are runtime state in the general case, and
//! this test says what it can prove about each:
//!
//! - the holder's **mask** (`+0x10`) is carried by every holder and is all ones,
//!   so the mask test passes whatever the component's two mask words hold;
//! - the **mode byte** (`+0x2A`) and the **shape mode** (`+0x20`) are not carried
//!   by any holder payload, so the client reads the class's default, which is
//!   zero for both (enums with no initializer). The filter byte `0` is what the
//!   builder requires, and shape mode `0` is the mesh arm.
//!
//! Skipped when no install is present. Read-only.
//!
//! ```text
//! cargo test --release -p ow-tank --test model_body_real -- --nocapture
//! ```

use std::collections::BTreeMap;

use ow_stu::types::{ClassRegistry as StuRegistry, DefaultValue, Value};
use ow_tank::classreg::ClassRegistry as Classreg;
use ow_tank::keytable::Sources;
use ow_tank::model_body::{self, Shape};
use ow_tank::{guid, mapresolve, mstu, GameData, ManifestSelector};

const MESH_BLOCK: u32 = 0x537E56E4;
const FIELD_HOLDERS: u32 = 0xCB4D298D;
const FIELD_MESH_BLOCK: u32 = 0xB7C8314A;
/// `holder+0x10`: the mask word.
const FIELD_HOLDER_MASK: u32 = 0xBD18CD91;
/// `holder+0x20`: the shape mode.
const FIELD_HOLDER_SHAPE_MODE: u32 = 0xD833F59A;
/// `holder+0x2A`: the filter byte.
const FIELD_HOLDER_MODE_BYTE: u32 = 0x89AABA94;
/// The holder's declared class size, which is what the client allocates.
const HOLDER_SIZE: u32 = 0x30;
/// The model whose plan is pinned bit-exactly.
const PINNED_MODEL: u64 = 0x0d0000000000078c;

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

/// One scalar field's value and whether the payload carries it. A field the
/// payload omits reads as the class's declared default, which is what the loader
/// leaves in place.
fn field_u64(
    stu: &mut ow_stu::Stu,
    holder: usize,
    hash: u32,
    reg: &StuRegistry,
) -> Option<(u64, bool)> {
    match stu.typed_field(holder, hash, reg)? {
        Value::Prim(_, bits) => Some((bits, true)),
        Value::Null => Some((0, true)),
        Value::Missing => {
            let class = stu.instances.get(holder).map(|i| i.hash)?;
            let def = reg.field(class, hash)?;
            match def.default {
                DefaultValue::None => Some((0, false)),
                DefaultValue::Bool(b) => Some((u64::from(b), false)),
                DefaultValue::Int(i) => Some((i as u64, false)),
                DefaultValue::EnumMember(_) => Some((0, false)),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Every holder the model's `0xCB4D298D` array names, in array order.
fn holder_instances(stu: &mut ow_stu::Stu, model: usize, reg: &StuRegistry) -> Vec<usize> {
    match stu.typed_field(model, FIELD_HOLDERS, reg) {
        Some(Value::Array(items)) => items
            .into_iter()
            .filter_map(|v| match v {
                Value::Instance(i) => Some(i),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The mesh block the holder embeds through field `0xB7C8314A`.
fn holder_has_mesh(stu: &ow_stu::Stu, holder: usize) -> bool {
    stu.instances.iter().any(|i| {
        i.hash == MESH_BLOCK
            && i.embedder_index == holder as i32
            && i.embedder_field == FIELD_MESH_BLOCK
    })
}

/// One holder, as the builder's tests see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HolderRow {
    model: u64,
    slot: usize,
    mask: u64,
    mesh: u64,
    mode_byte: u8,
    shape_mode: i32,
    shape: Option<Shape>,
}

#[test]
fn the_range_models_holder_plan() {
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
    let holder_class = reg.class(0x2DD96D6D).expect("the holder class is declared");
    assert_eq!(holder_class.size, HOLDER_SIZE, "the holder's declared size");
    // The declared offsets are the ones the client's code reads; this pins the
    // hash-to-offset mapping the value reads below depend on.
    let declared: Vec<(u32, u32)> = [
        (0xB7C8314A, 0x08u32),
        (0xBD18CD91, 0x10),
        (0x9B0424EB, 0x18),
        (0xD833F59A, 0x20),
        (0x30F925F4, 0x24),
        (0x1DE83A05, 0x28),
        (0x89AABA94, 0x2A),
    ]
    .to_vec();
    for (hash, offset) in declared {
        let field = reg
            .field(0x2DD96D6D, hash)
            .unwrap_or_else(|| panic!("the holder declares {hash:#010x}"));
        assert_eq!(
            field.v1_offset, offset,
            "the declared offset of {hash:#010x} is the offset the client reads"
        );
    }
    assert_eq!(holder_class.fields.len(), 7, "the holder's field count");

    let models = model_guids(&data);
    let mut rows: Vec<HolderRow> = Vec::new();
    let mut mask_carried = 0usize;
    let mut byte_carried = 0usize;
    let mut shape_mode_carried = 0usize;
    for model in &models {
        let Ok(bytes) = data.open_asset(*model) else {
            continue;
        };
        let Ok(mut stu) = mstu::parse(&bytes) else {
            continue;
        };
        for index in stu.instances_of(0x871BD3D0) {
            for (slot, holder) in holder_instances(&mut stu, index, reg)
                .into_iter()
                .enumerate()
            {
                let (mask, mask_present) =
                    field_u64(&mut stu, holder, FIELD_HOLDER_MASK, reg).unwrap_or((0, false));
                let (mode_byte, byte_present) =
                    field_u64(&mut stu, holder, FIELD_HOLDER_MODE_BYTE, reg).unwrap_or((0, false));
                let (shape_mode, mode_present) =
                    field_u64(&mut stu, holder, FIELD_HOLDER_SHAPE_MODE, reg).unwrap_or((0, false));
                mask_carried += usize::from(mask_present);
                byte_carried += usize::from(byte_present);
                shape_mode_carried += usize::from(mode_present);
                let mesh = u64::from(holder_has_mesh(&stu, holder));
                // The component's two mask words are runtime state; an all-ones
                // holder mask makes the mask test independent of them, which is
                // why u64::MAX is a sound argument here only when the mask
                // measured below is all ones (asserted further down).
                let shape = model_body::selected_shape(
                    mode_byte as u8,
                    mesh,
                    mask,
                    u64::MAX,
                    shape_mode as i32,
                );
                rows.push(HolderRow {
                    model: *model,
                    slot,
                    mask,
                    mesh,
                    mode_byte: mode_byte as u8,
                    shape_mode: shape_mode as i32,
                    shape,
                });
            }
        }
    }

    let mesh_holders = rows.iter().filter(|r| r.mesh != 0).count();
    let shape_meshes = rows.iter().filter(|r| r.shape == Some(Shape::Mesh)).count();
    let by_model: BTreeMap<u64, usize> = {
        let mut m = BTreeMap::new();
        for row in &rows {
            if row.shape.is_some() {
                *m.entry(row.model).or_default() += 1;
            }
        }
        m
    };
    let mut shapes: BTreeMap<Option<Shape>, usize> = BTreeMap::new();
    for row in &rows {
        *shapes.entry(row.shape).or_default() += 1;
    }
    println!(
        "{} range model asset(s); {} holder(s)",
        models.len(),
        rows.len()
    );
    println!(
        "payload carries the field: mask {mask_carried}, mode byte {byte_carried}, shape mode {shape_mode_carried} of {}",
        rows.len()
    );
    println!("shape histogram: {shapes:?}");
    println!(
        "models with at least one shape: {}/{}; mesh shapes per instantiation of those models: {}",
        by_model.len(),
        models.len(),
        shape_meshes
    );

    assert_eq!(models.len(), 229, "model assets named by the range");
    assert_eq!(rows.len(), 279, "holders over those models");
    assert_eq!(mesh_holders, 272, "holders with a mesh block");
    assert_eq!(mask_carried, 279, "every holder carries its mask word");
    assert!(
        rows.iter().all(|r| r.mask == u64::MAX),
        "every holder's mask is all ones, so the mask test never excludes one"
    );
    assert_eq!(
        (byte_carried, shape_mode_carried),
        (0, 0),
        "no holder carries the filter byte or the shape mode: both are the class default"
    );
    assert!(
        rows.iter().all(|r| r.mode_byte == 0 && r.shape_mode == 0),
        "the defaulted filter byte and shape mode are zero"
    );
    assert_eq!(
        shapes.get(&Some(Shape::Mesh)).copied().unwrap_or(0),
        272,
        "every one of the 272 mesh holders is a type-3 mesh shape"
    );
    assert_eq!(
        shapes.get(&None).copied().unwrap_or(0),
        7,
        "the seven holders without a mesh block are skipped"
    );
    assert_eq!(
        by_model.len(),
        161,
        "models with at least one shape, against 161 models carrying the mesh block"
    );

    // One model's plan, exactly: model 0x0d0000000000078c has one holder, the
    // slot the mesh-block check pinned as a 4-unit box, and it becomes a mesh.
    let plan: Vec<HolderRow> = rows
        .iter()
        .copied()
        .filter(|r| r.model == PINNED_MODEL)
        .collect();
    assert_eq!(
        plan,
        vec![HolderRow {
            model: PINNED_MODEL,
            slot: 0,
            mask: u64::MAX,
            mesh: 1,
            mode_byte: 0,
            shape_mode: 0,
            shape: Some(Shape::Mesh),
        }],
        "the pinned model's holder plan"
    );
}

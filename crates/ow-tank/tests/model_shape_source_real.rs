//! The practice range through the **shape-source assembler's** filter: how many
//! of the range's holders it takes, and into which of its four arrays.
//!
//! The filter and the arm set are `ow_tank::model_shape_source`, read from
//! `0x14209fc90` (`docs/formats/static-collision.md`, check 15). It walks the
//! *same* holder list the per-instance body builder walks (the model's field
//! `0xCB4D298D` at `+0x140`), so the holder enumeration here is the one
//! `model_body_real` uses, and the holder field hashes are the class
//! declaration's own offsets.
//!
//! The answer is a **zero**: the assembler takes the holders whose mode byte
//! (`+0x2A`, field `0x89AABA94`) is `1`, no holder of the range carries that
//! field at all, and the class default is `0`, so the four counts sum to zero
//! and the client returns before it allocates (`0x14209ff8a`). The mesh test
//! alone passes for 272 holders, so the mode byte is the only reason the plan is
//! empty -- that is pinned too.
//!
//! Skipped when no install is present. Read-only.
//!
//! ```text
//! cargo test --release -p ow-tank --test model_shape_source_real -- --nocapture
//! ```

use ow_stu::types::{ClassRegistry as StuRegistry, DefaultValue, Value};
use ow_tank::classreg::ClassRegistry as Classreg;
use ow_tank::keytable::Sources;
use ow_tank::model_body::Shape;
use ow_tank::model_shape_source::{self, Plan};
use ow_tank::{guid, mapresolve, mstu, GameData, ManifestSelector};

const MESH_BLOCK: u32 = 0x537E56E4;
const FIELD_HOLDERS: u32 = 0xCB4D298D;
const FIELD_MESH_BLOCK: u32 = 0xB7C8314A;
/// `holder+0x20`: the shape mode.
const FIELD_HOLDER_SHAPE_MODE: u32 = 0xD833F59A;
/// `holder+0x2A`: the mode byte this consumer filters on.
const FIELD_HOLDER_MODE_BYTE: u32 = 0x89AABA94;
/// The holder's declared class size, which is what the client allocates.
const HOLDER_SIZE: u32 = 0x30;

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

#[test]
fn the_range_takes_no_holder_into_the_shape_source() {
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
    // The two field hashes this test reads are declared at the offsets the
    // client's assembler reads.
    for (hash, offset) in [
        (FIELD_HOLDER_MODE_BYTE, 0x2Au32),
        (FIELD_HOLDER_SHAPE_MODE, 0x20),
    ] {
        let field = reg
            .field(0x2DD96D6D, hash)
            .unwrap_or_else(|| panic!("the holder declares {hash:#010x}"));
        assert_eq!(
            field.v1_offset, offset,
            "the declared offset of {hash:#010x}"
        );
    }

    let models = model_guids(&data);
    let mut holders: Vec<(u8, u64, i32)> = Vec::new();
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
            for holder in holder_instances(&mut stu, index, reg) {
                let (mode_byte, byte_present) =
                    field_u64(&mut stu, holder, FIELD_HOLDER_MODE_BYTE, reg).unwrap_or((0, false));
                let (shape_mode, shape_present) =
                    field_u64(&mut stu, holder, FIELD_HOLDER_SHAPE_MODE, reg).unwrap_or((0, false));
                byte_carried += usize::from(byte_present);
                shape_mode_carried += usize::from(shape_present);
                let mesh = u64::from(holder_has_mesh(&stu, holder));
                holders.push((mode_byte as u8, mesh, shape_mode as i32));
            }
        }
    }

    let plan: Plan = model_shape_source::plan(holders.iter().copied());
    let mesh_holders = holders.iter().filter(|(_, mesh, _)| *mesh != 0).count();
    println!(
        "{} range model asset(s); {} holder(s)",
        models.len(),
        holders.len()
    );
    println!(
        "payload carries the field: mode byte {byte_carried}, shape mode {shape_mode_carried} of {}",
        holders.len()
    );
    println!("holders with a mesh block: {mesh_holders}");
    println!("assembler plan: {:?}", plan.arms());

    assert_eq!(models.len(), 229, "model assets named by the range");
    assert_eq!(holders.len(), 279, "holders over those models");
    // The mesh test alone passes for 272 of them, so the empty plan below is the
    // mode byte's doing and not an artefact of the mesh test.
    assert_eq!(mesh_holders, 272, "holders carrying the mesh block");
    assert_eq!(
        byte_carried, 0,
        "no holder carries the mode byte, so every one reads the class default"
    );
    assert!(
        holders.iter().all(|(byte, _, _)| *byte == 0),
        "the defaulted mode byte is zero"
    );
    assert_eq!(shape_mode_carried, 0, "no holder carries the shape mode");
    assert!(
        plan.is_empty(),
        "the assembler takes no holder, so it returns before allocating"
    );
    assert_eq!(plan.total(), 0, "the sum the client tests at 0x14209ff8a");
    for (shape, count) in plan.arms() {
        assert_eq!(count, 0, "{shape:?}");
    }
    assert_eq!(plan.count(Shape::Mesh), 0);
    // A mode byte of 1 would be taken for every mesh holder whose shape mode is
    // 0: the counterfactual shows the pin above is about the data, not the code.
    let counterfactual: Plan = model_shape_source::plan(
        holders
            .iter()
            .map(|(_, mesh, shape_mode)| (1u8, *mesh, *shape_mode)),
    );
    assert_eq!(
        counterfactual.count(Shape::Mesh),
        272,
        "with the mode byte at 1 every mesh holder is a mesh record"
    );
    assert_eq!(counterfactual.total(), 272);
}

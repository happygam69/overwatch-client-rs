//! The model's prebuilt collision mesh (`STU_537E56E4`) as a practice-range
//! model carries it, and the shape record the client builds from it.
//!
//! The consumer of `STUModel`'s collision list is `FUN_14210bac0`
//! (`docs/formats/static-collision.md`): it walks the list, unpacks each
//! `STU_537E56E4` with `0x141b53af0`, fills a shape record with `MeshShapeFill`
//! (`0x142cae540`) and hands that record to the collision query `0x142caf6c0`.
//! This test reads the same three arrays out of the install and pins them, plus
//! the shape record built from them at unit scale.
//!
//! The record's three data-pointer lanes are runtime addresses and cannot be
//! read from the asset, so the test feeds the arrays' file offsets there and
//! asserts only the lanes the client derives by count and by scale.
//!
//! Skipped when no install is present. Read-only.
//!
//! ```text
//! cargo test --release -p ow-tank --test model_mesh_real -- --nocapture
//! ```

use std::collections::BTreeSet;

use ow_stu::types::{ClassRegistry as StuRegistry, Value};
use ow_tank::classreg::ClassRegistry as Classreg;
use ow_tank::keytable::Sources;
use ow_tank::mesh_shape::{self, MeshSource};
use ow_tank::{guid, mapresolve, mstu, GameData, ManifestSelector};

const MESH_BLOCK: u32 = 0x537E56E4;
const FIELD_NODES: u32 = 0x9B2111C5;
const FIELD_POINTS: u32 = 0x88FCECD7;
const FIELD_TRIANGLES: u32 = 0x0D50490E;
const FIELD_NODE_COUNT: u32 = 0xF91B8EDD;
const FIELD_38B71436: u32 = 0x38B71436;
const FIELD_HEIGHT: u32 = 0xA16426C6;
const FIELD_5BD33E19: u32 = 0x5BD33E19;

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

/// One `STU_537E56E4` block, read through the class's declared fields.
#[derive(Debug, Clone)]
struct MeshBlock {
    model: u64,
    nodes_len: usize,
    points: Vec<[f32; 4]>,
    indices: Vec<u32>,
    node_count: u32,
    field_3c: u32,
    height: u32,
    field_44_bits: u32,
}

fn u32_field(stu: &mut ow_stu::Stu, index: usize, hash: u32, reg: &StuRegistry) -> u32 {
    match stu.typed_field(index, hash, reg) {
        Some(Value::Prim(_, bits)) => bits as u32,
        _ => 0,
    }
}

fn array_items(stu: &mut ow_stu::Stu, index: usize, hash: u32, reg: &StuRegistry) -> Vec<Value> {
    match stu.typed_field(index, hash, reg) {
        Some(Value::Array(items)) => items,
        _ => Vec::new(),
    }
}

fn blocks_of(stu: &mut ow_stu::Stu, reg: &StuRegistry, model: u64) -> Vec<MeshBlock> {
    let mut out = Vec::new();
    for index in stu.instances_of(MESH_BLOCK) {
        let nodes_len = array_items(stu, index, FIELD_NODES, reg).len();
        let points: Vec<[f32; 4]> = array_items(stu, index, FIELD_POINTS, reg)
            .into_iter()
            .filter_map(|v| match v {
                Value::Struct(f) if f.len() >= 4 => Some([f[0], f[1], f[2], f[3]]),
                _ => None,
            })
            .collect();
        let indices: Vec<u32> = array_items(stu, index, FIELD_TRIANGLES, reg)
            .into_iter()
            .filter_map(|v| match v {
                Value::Prim(_, bits) => Some(bits as u32),
                _ => None,
            })
            .collect();
        out.push(MeshBlock {
            model,
            nodes_len,
            node_count: u32_field(stu, index, FIELD_NODE_COUNT, reg),
            field_3c: u32_field(stu, index, FIELD_38B71436, reg),
            height: u32_field(stu, index, FIELD_HEIGHT, reg),
            field_44_bits: u32_field(stu, index, FIELD_5BD33E19, reg),
            points,
            indices,
        });
    }
    out
}

#[test]
fn the_range_models_prebuilt_mesh_and_its_shape_record() {
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
    let kinds = sources.chunk_kinds().expect("the chunk-kind table");
    let scene = mapresolve::resolve_practice_range(&data, &kinds).expect("the scene resolves");

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

    let mut with_blocks = 0usize;
    let mut blocks_total = 0usize;
    let mut node_counts: BTreeSet<u32> = BTreeSet::new();
    let mut first: Option<MeshBlock> = None;
    for model in &models {
        let Ok(bytes) = data.open_asset(*model) else {
            continue;
        };
        let Ok(mut stu) = mstu::parse(&bytes) else {
            continue;
        };
        let blocks = blocks_of(&mut stu, reg, *model);
        if blocks.is_empty() {
            continue;
        }
        with_blocks += 1;
        blocks_total += blocks.len();
        for b in &blocks {
            node_counts.insert(b.node_count);
        }
        if first.is_none() {
            first = blocks.into_iter().next();
        }
    }
    println!(
        "{with_blocks} of {} model(s) carry the mesh block; {blocks_total} block(s); nodeCount values {node_counts:?}",
        models.len()
    );

    let Some(block) = first else {
        eprintln!("note: no practice-range model carries the mesh block");
        return;
    };
    println!(
        "model 0x{:016x}: nodes {} bytes, points {}, indices {}, nodeCount {}, +0x3c {}, height {}, +0x44 bits {:08x}",
        block.model,
        block.nodes_len,
        block.points.len(),
        block.indices.len(),
        block.node_count,
        block.field_3c,
        block.height,
        block.field_44_bits
    );
    let head: Vec<String> = block
        .points
        .iter()
        .take(4)
        .map(|p| {
            format!(
                "{:08x} {:08x} {:08x} {:08x}",
                p[0].to_bits(),
                p[1].to_bits(),
                p[2].to_bits(),
                p[3].to_bits()
            )
        })
        .collect();
    println!("  first point bits: {head:?}");
    println!(
        "  first indices: {:?}",
        block.indices.iter().take(12).collect::<Vec<_>>()
    );

    // The shape record the client builds from this block, at unit scale. The
    // three pointer lanes carry the arrays' file offsets here (runtime addresses
    // are not in the asset); every other lane is the client's own value.
    let source = MeshSource {
        nodes: block.nodes_len as u64,
        points: block.points.len() as u64,
        triangles: block.indices.len() as u64,
        node_count: block.node_count,
        point_count: block.points.len() as u32,
        field_3c: block.field_3c,
        height: block.height,
        field_44: f32::from_bits(block.field_44_bits),
    };
    // The measured block: a unit box (points 0..4) with 9 nodes.
    assert_eq!(block.model, 0x0d00_0000_0000_078c);
    assert_eq!(block.node_count, 9, "nodeCount");
    assert_eq!(block.nodes_len, 288, "node array bytes (9 x 32)");
    assert_eq!(block.points.len(), 8, "points");
    assert_eq!(block.indices.len(), 384, "triangle array bytes (24 x 16)");
    assert_eq!(block.field_3c, 12);
    assert_eq!(block.height, 5);
    assert_eq!(block.field_44_bits, 0x42c0_0000, "96.0f");
    let point_bits: Vec<[u32; 4]> = block.points.iter().map(|p| p.map(f32::to_bits)).collect();
    assert_eq!(
        &point_bits[..4],
        &[
            [0x0000_0000, 0x0000_0000, 0x4080_0000, 0x0000_0000],
            [0x4080_0000, 0x0000_0000, 0x4080_0000, 0x0000_0000],
            [0x0000_0000, 0x4080_0000, 0x4080_0000, 0x0000_0000],
            [0x4080_0000, 0x4080_0000, 0x4080_0000, 0x0000_0000],
        ],
        "the block's first four points"
    );
    assert_eq!(
        &block.indices[..12],
        &[4, 0, 0, 0, 5, 0, 0, 0, 6, 0, 0, 0],
        "the triangle array's first twelve words"
    );
    assert_eq!(
        (with_blocks, blocks_total),
        (161, 272),
        "the range's blocks"
    );

    let record = mesh_shape::shape_record(&source, 1.0);
    println!("shape record (19 dwords): {record:08x?}");
    assert_eq!(
        &record[0..8],
        &[1.0f32.to_bits(); 8],
        "unit scale and its reciprocal"
    );
    assert_eq!(record[14], block.node_count, "+0x38 nodeCount");
    assert_eq!(record[15], block.points.len() as u32, "+0x3c point count");
    assert_eq!(record[16], block.field_3c, "+0x40 the 0x3c word");
    assert_eq!(record[17], block.height, "+0x44 height");
    assert!(!block.points.is_empty(), "the block carries points");
}

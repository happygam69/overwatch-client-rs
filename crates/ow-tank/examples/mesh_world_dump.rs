//! Dump the range's assembled mesh world for the offline evaluator probe.
//!
//! `ow_tank::mesh_world` assembles the mesh shapes the range's models carry
//! into one guest-address buffer. `dev/probe_candidate_evaluator.py` needs that
//! buffer, each shape's array addresses, and **where each shape is placed**, so
//! it can build the Domino record the client's collision query walks.
//!
//! ## Why the placement is here
//!
//! The client's collision builder reads a placed model's transform from the
//! scene component and hands it to `OwnerCreateBody`, whose config is two
//! quads: the quaternion first, the position second. The component's transform
//! is the same `(translation, scale, quaternion)` triple the map chunk stores
//! for every placed model, and `ShapeWorldTransform` (`0x142cb8b80`) reads that
//! body back as the shape's world frame. The range places its 229 models
//! through 2,449 model-group entries, so the world image is one shape instance
//! per (model, entry), each with the entry's translation and rotation.
//!
//! ## File format (magic `OWM2`)
//!
//! ```text
//! u32 magic "OWM2", u32 shape count, u64 base, u32 blob length,
//! u32 instance count, u32 pinned instance, u32 reserved
//! shape count x { u64 nodes, u64 points, u64 triangles,
//!                 u32 node_count, u32 point_count, u32 field_3c, u32 height,
//!                 f32 field_44 bits, u32 pad }
//! instance count x { u32 shape index, u32 pad, f32 translation[3], f32 pad,
//!                    f32 rotation[4] }
//! blob bytes
//! ```
//!
//! `pinned` is the index of the first instance placed from model
//! `0x0d0000000000078c`, the block `mesh_world_real` pins bit for bit. The
//! addresses are absolute guest addresses, so the probe maps the blob at
//! `base` and uses the slot addresses unchanged.
//!
//! The install is read only. The output path is gitignored:
//!
//! ```text
//! cargo run -p ow-tank --example mesh_world_dump -- dev/data/mesh_world.bin
//! ```

use std::collections::BTreeMap;
use std::io::Write;

use ow_stu::types::{ClassRegistry as StuRegistry, Prim, Value};
use ow_tank::classreg::ClassRegistry as Classreg;
use ow_tank::keytable::Sources;
use ow_tank::mapchunk::Body;
use ow_tank::mesh_shape::MeshSource;
use ow_tank::mesh_world::{MeshArrays, MeshWorld};
use ow_tank::{guid, mapresolve, model_body, mstu, GameData, ManifestSelector};

const MESH_BLOCK: u32 = 0x537E56E4;
const FIELD_NODES: u32 = 0x9B2111C5;
const FIELD_POINTS: u32 = 0x88FCECD7;
const FIELD_TRIANGLES: u32 = 0x0D50490E;
const FIELD_NODE_COUNT: u32 = 0xF91B8EDD;
const FIELD_38B71436: u32 = 0x38B71436;
const FIELD_HEIGHT: u32 = 0xA16426C6;
const FIELD_5BD33E19: u32 = 0x5BD33E19;

const FIELD_HOLDERS: u32 = 0xCB4D298D;
const FIELD_HOLDER_MESH: u32 = 0xB7C8314A;
const FIELD_HOLDER_MASK: u32 = 0xBD18CD91;
const FIELD_HOLDER_SHAPE_MODE: u32 = 0xD833F59A;
const FIELD_HOLDER_MODE_BYTE: u32 = 0x89AABA94;
const STU_MODEL: u32 = 0x871BD3D0;

const PINNED_MODEL: u64 = 0x0d00_0000_0000_078c;
const WORLD_BASE: u64 = 0x0000_0001_0000_0000;

const MAGIC: u32 = 0x324D_574F; // "OWM2"

/// One placed model: its asset, and the entry transform the collision builder
/// would hand to `OwnerCreateBody`.
#[derive(Debug, Clone, Copy)]
struct Placement {
    model: u64,
    translation: [f32; 3],
    rotation: [f32; 4],
}

fn u32_field(stu: &mut ow_stu::Stu, index: usize, hash: u32, reg: &StuRegistry) -> Option<u32> {
    match stu.typed_field(index, hash, reg)? {
        Value::Prim(_, bits) => Some(bits as u32),
        Value::Defaulted(inner) => match *inner {
            Value::Prim(_, bits) => Some(bits as u32),
            _ => None,
        },
        _ => None,
    }
}

fn array_items(stu: &mut ow_stu::Stu, index: usize, hash: u32, reg: &StuRegistry) -> Vec<Value> {
    let value = stu.typed_field(index, hash, reg).map(|v| match v {
        Value::Defaulted(inner) => *inner,
        other => other,
    });
    match value {
        Some(Value::Array(items)) => items
            .into_iter()
            .map(|v| match v {
                Value::Defaulted(inner) => *inner,
                other => other,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn bytes_field(stu: &mut ow_stu::Stu, index: usize, hash: u32, reg: &StuRegistry) -> Vec<u8> {
    array_items(stu, index, hash, reg)
        .into_iter()
        .filter_map(|v| match v {
            Value::Prim(Prim::U8, bits) => Some(bits as u8),
            _ => None,
        })
        .collect()
}

fn pointbytes_field(stu: &mut ow_stu::Stu, index: usize, hash: u32, reg: &StuRegistry) -> Vec<u8> {
    let mut out = Vec::new();
    for item in array_items(stu, index, hash, reg) {
        let Value::Struct(lanes) = item else { continue };
        for lane in lanes {
            out.extend_from_slice(&lane.to_le_bytes());
        }
    }
    out
}

fn block_arrays(
    stu: &mut ow_stu::Stu,
    index: usize,
    reg: &StuRegistry,
) -> (MeshArrays, MeshSource) {
    let arrays = MeshArrays {
        nodes: bytes_field(stu, index, FIELD_NODES, reg),
        points: pointbytes_field(stu, index, FIELD_POINTS, reg),
        triangles: bytes_field(stu, index, FIELD_TRIANGLES, reg),
    };
    let words = MeshSource {
        nodes: 0,
        points: 0,
        triangles: 0,
        node_count: u32_field(stu, index, FIELD_NODE_COUNT, reg).unwrap_or(0),
        point_count: arrays.point_count() as u32,
        field_3c: u32_field(stu, index, FIELD_38B71436, reg).unwrap_or(0),
        height: u32_field(stu, index, FIELD_HEIGHT, reg).unwrap_or(0),
        field_44: f32::from_bits(u32_field(stu, index, FIELD_5BD33E19, reg).unwrap_or(0)),
    };
    (arrays, words)
}

fn holder_instances(stu: &mut ow_stu::Stu, model: usize, reg: &StuRegistry) -> Vec<usize> {
    match stu.typed_field(model, FIELD_HOLDERS, reg) {
        Some(Value::Array(items)) => items
            .into_iter()
            .filter_map(|v| match v.unwrap_defaulted() {
                Value::Instance(i) => Some(*i),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn holder_mesh(stu: &ow_stu::Stu, holder: usize) -> Option<usize> {
    stu.instances.iter().position(|i| {
        i.hash == MESH_BLOCK
            && i.embedder_index == holder as i32
            && i.embedder_field == FIELD_HOLDER_MESH
    })
}

fn field_u64(stu: &mut ow_stu::Stu, index: usize, hash: u32, reg: &StuRegistry) -> u64 {
    match stu.typed_field(index, hash, reg) {
        Some(value) => match value.unwrap_defaulted() {
            Value::Prim(_, bits) => *bits,
            _ => 0,
        },
        None => 0,
    }
}

/// Every placed model of the range, in chunk order.
///
/// A model-group placeable's own transform is identity; its groups carry looks
/// and its entries carry the world transforms, one per placed instance. A
/// single-model placeable carries its transform on the placeable itself.
fn model_placements(data: &GameData) -> Vec<Placement> {
    let root = ow_casc::install::detect_root().expect("the install root");
    let sources = Sources::discover(std::slice::from_ref(&root));
    let kinds = sources.chunk_kinds().expect("the chunk-kind table");
    let scene = mapresolve::resolve_practice_range(data, &kinds).expect("the scene resolves");
    let mut out: Vec<Placement> = Vec::new();
    for chunk in &scene.scene.chunks {
        let Some(c) = &chunk.chunk else { continue };
        for p in &c.placeables {
            let Some(model) = p.model else { continue };
            if guid::type_of(model) != guid::types::MODEL {
                continue;
            }
            match &p.body {
                Body::ModelGroup(group) => {
                    for g in &group.groups {
                        for entry in &g.entries {
                            out.push(Placement {
                                model,
                                translation: entry.translation,
                                rotation: entry.rotation,
                            });
                        }
                    }
                }
                Body::Model(_) | Body::SingleModel(_) => out.push(Placement {
                    model,
                    translation: p.translation,
                    rotation: p.rotation,
                }),
                _ => {}
            }
        }
    }
    out
}

/// One model's selected holders' arrays, in holder order.
fn model_shapes(data: &GameData, model: u64, reg: &StuRegistry) -> Vec<(MeshArrays, MeshSource)> {
    let mut out = Vec::new();
    let Ok(bytes) = data.open_asset(model) else {
        return out;
    };
    let Ok(mut stu) = mstu::parse(&bytes) else {
        return out;
    };
    for instance in stu.instances_of(STU_MODEL) {
        for holder in holder_instances(&mut stu, instance, reg) {
            let mode_byte = field_u64(&mut stu, holder, FIELD_HOLDER_MODE_BYTE, reg) as u8;
            let mask = field_u64(&mut stu, holder, FIELD_HOLDER_MASK, reg);
            let shape_mode = field_u64(&mut stu, holder, FIELD_HOLDER_SHAPE_MODE, reg) as i32;
            let Some(block) = holder_mesh(&stu, holder) else {
                continue;
            };
            if !model_body::holder_selected(mode_byte, 1, mask, u64::MAX) {
                continue;
            }
            assert_eq!(
                model_body::shape_for(shape_mode),
                Some(model_body::Shape::Mesh),
                "the range's holders are shape mode 0"
            );
            out.push(block_arrays(&mut stu, block, reg));
        }
    }
    out
}

fn main() -> std::io::Result<()> {
    let out_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "dev/data/mesh_world.bin".to_string());

    let root = ow_casc::install::detect_root().expect("the install root");
    let sources = Sources::discover(std::slice::from_ref(&root));
    let data =
        GameData::open(&root, &ManifestSelector::default(), &sources).expect("the install opens");
    let classreg = Classreg::load_from_sources(&sources);
    assert!(!classreg.is_empty(), "no STU class checkout found");
    let reg = classreg.inner();

    let placements = model_placements(&data);

    // One shape entry per (model, selected holder); each model is parsed once.
    let mut by_model: BTreeMap<u64, Vec<usize>> = BTreeMap::new();
    let mut world = MeshWorld::new(WORLD_BASE);
    let mut distinct: Vec<u64> = placements.iter().map(|p| p.model).collect();
    distinct.sort_unstable();
    distinct.dedup();
    for model in &distinct {
        let ids = model_shapes(&data, *model, reg)
            .iter()
            .map(|(arrays, words)| world.push(arrays, words))
            .collect();
        by_model.insert(*model, ids);
    }

    let mut instances: Vec<(usize, [f32; 3], [f32; 4])> = Vec::new();
    let mut pinned: Option<usize> = None;
    let mut from_pinned = 0usize;
    for placement in &placements {
        for shape in &by_model[&placement.model] {
            if placement.model == PINNED_MODEL {
                from_pinned += 1;
                if pinned.is_none() {
                    pinned = Some(instances.len());
                }
            }
            instances.push((*shape, placement.translation, placement.rotation));
        }
    }

    let pinned = pinned.expect("the pinned model is placed in the range");
    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC.to_le_bytes());
    out.extend_from_slice(&(world.len() as u32).to_le_bytes());
    out.extend_from_slice(&world.base().to_le_bytes());
    out.extend_from_slice(&(world.bytes().len() as u32).to_le_bytes());
    out.extend_from_slice(&(instances.len() as u32).to_le_bytes());
    out.extend_from_slice(&(pinned as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for slot in world.shapes() {
        out.extend_from_slice(&slot.nodes.to_le_bytes());
        out.extend_from_slice(&slot.points.to_le_bytes());
        out.extend_from_slice(&slot.triangles.to_le_bytes());
        out.extend_from_slice(&slot.node_count.to_le_bytes());
        out.extend_from_slice(&slot.point_count.to_le_bytes());
        out.extend_from_slice(&slot.field_3c.to_le_bytes());
        out.extend_from_slice(&slot.height.to_le_bytes());
        out.extend_from_slice(&slot.field_44.to_bits().to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
    }
    for (shape, translation, rotation) in &instances {
        out.extend_from_slice(&(*shape as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        for lane in translation {
            out.extend_from_slice(&lane.to_le_bytes());
        }
        out.extend_from_slice(&0u32.to_le_bytes());
        for lane in rotation {
            out.extend_from_slice(&lane.to_le_bytes());
        }
    }
    out.extend_from_slice(world.bytes());

    if let Some(dir) = std::path::Path::new(&out_path).parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::File::create(&out_path)?.write_all(&out)?;

    println!(
        "wrote {out_path}: {} shape(s), {} instance(s), {} bytes, base 0x{:x}, \
         pinned instance {pinned} (model 0x{PINNED_MODEL:016x} contributed {from_pinned})",
        world.len(),
        instances.len(),
        world.bytes().len(),
        world.base()
    );
    Ok(())
}

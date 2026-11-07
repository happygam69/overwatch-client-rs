//! The practice range assembled as a collision world: the model meshes the
//! install ships, placed where the map chunk puts them.
//!
//! This is the ingest the movement step reads (`docs/formats/static-collision.md`,
//! checks 9 and 12): a placed model's collision is the `STU_537E56E4` block its
//! holders carry (`0x14201ae60`'s holder loop, `ow_tank::model_body`), the mesh
//! arrays are the block's own `MeshArrays` (`ow_tank::mesh_world`), and the
//! placement is the map chunk's entry transform, the same
//! `(translation, scale, rotation)` triple the scene instancer uses.
//!
//! Two things are deliberately absent:
//!
//! - the Domino body/shape handles (`OwnerCreateBody`, `ShapeAddToBody`). They
//!   allocate from a running client's pools; the client has no pools, so the
//!   shapes are a list here and `ow_sim::walk` sweeps it directly with the
//!   verified type-3 narrowphase.
//! - the descriptor's mask/filter words. The range's holders all pass
//!   `holder_selected` with the world mask all ones, so the shape's layer is
//!   the all-ones default the class sets (`model_body::world_mask`).
//!
//! Read-only over the install.

use crate::chunkkind;
use crate::collision_level;
use crate::keytable::Sources;
use crate::mesh_shape::MeshSource;
use crate::mesh_world::{MeshArrays, MeshWorld};
use crate::{guid, mapchunk::Body, mapresolve, model_body, mstu, GameData};
use glam::{Mat4, Quat, Vec3};
use ow_stu::types::{ClassRegistry as StuRegistry, Prim, Value};

/// The mesh block every collision holder embeds (`STU_537E56E4`).
pub const MESH_BLOCK: u32 = 0x537E_56E4;
/// The `STUModel` type (`0x871BD3D0`).
pub const STU_MODEL: u32 = 0x871B_D3D0;

// `STUModel+0x140`, the holder array (field `0xCB4D298D`).
const FIELD_HOLDERS: u32 = 0xCB4D_298D;
/// `holder+0x08`, the embedded mesh block.
const FIELD_HOLDER_MESH: u32 = 0xB7C8_314A;
/// `holder+0x10`, the holder's mask word.
const FIELD_HOLDER_MASK: u32 = 0xBD18_CD91;
/// `holder+0x20`, the shape mode.
const FIELD_HOLDER_SHAPE_MODE: u32 = 0xD833_F59A;
/// `holder+0x2a`, the filter byte.
const FIELD_HOLDER_MODE_BYTE: u32 = 0x89AA_BA94;

// The mesh block's arrays and words.
const FIELD_NODES: u32 = 0x9B21_11C5;
const FIELD_POINTS: u32 = 0x88FC_ECD7;
const FIELD_TRIANGLES: u32 = 0x0D50_490E;
const FIELD_NODE_COUNT: u32 = 0xF91B_8EDD;
const FIELD_38B71436: u32 = 0x38B7_1436;
const FIELD_HEIGHT: u32 = 0xA164_26C6;
const FIELD_5BD33E19: u32 = 0x5BD3_3E19;

/// One placed model: its asset and the transform the map chunk gives it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub model: u64,
    pub translation: [f32; 3],
    pub scale: [f32; 3],
    pub rotation: [f32; 4],
}

/// One placed mesh shape: which of the model's blocks, and where it sits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacedShape {
    /// Index into [`RangeWorld::mesh`]'s shape list.
    pub shape: usize,
    pub model: u64,
    pub translation: [f32; 3],
    pub scale: [f32; 3],
    pub rotation: [f32; 4],
    /// The shape's mesh points' box in the shape's own frame.
    pub low: [f32; 3],
    pub high: [f32; 3],
}

impl PlacedShape {
    /// The world-space box the broadphase buckets: the local box's eight
    /// corners through the placement.
    pub fn world_bounds(&self) -> ([f32; 3], [f32; 3]) {
        let m = Mat4::from_scale_rotation_translation(
            Vec3::from_array(self.scale),
            quat(self.rotation),
            Vec3::from_array(self.translation),
        );
        let mut low = [f32::INFINITY; 3];
        let mut high = [f32::NEG_INFINITY; 3];
        for corner in 0..8 {
            let p = Vec3::new(
                if corner & 1 == 0 {
                    self.low[0]
                } else {
                    self.high[0]
                },
                if corner & 2 == 0 {
                    self.low[1]
                } else {
                    self.high[1]
                },
                if corner & 4 == 0 {
                    self.low[2]
                } else {
                    self.high[2]
                },
            );
            let q = m.transform_point3(p);
            let lanes = [q.x, q.y, q.z];
            for lane in 0..3 {
                low[lane] = low[lane].min(lanes[lane]);
                high[lane] = high[lane].max(lanes[lane]);
            }
        }
        (low, high)
    }

    /// The descriptor's scale (`+0x68`): `maxss(scale, 0.01)` when the three
    /// components agree within `0.01`, else `1.0` (`0x14201c434`).
    pub fn descriptor_scale(&self) -> f32 {
        model_body::mesh_descriptor_scale(self.scale[0], self.scale[1], self.scale[2])
    }
}

fn quat(q: [f32; 4]) -> Quat {
    Quat::from_xyzw(q[0], q[1], q[2], q[3])
}

/// The range's collision world: one mesh blob and every placed instance of it.
pub struct RangeWorld {
    pub mesh: MeshWorld,
    pub placed: Vec<PlacedShape>,
    /// How many of `placed` are the map's own level collider (the
    /// `MAP_CHUNK_COLLISION` body), as opposed to a model's holder mesh.
    pub level_shapes: usize,
    /// The map's spawn-room trigger volume, when it has one. This is the
    /// viewer's declared default: the client's spawn selector is unread, and
    /// the practice map ships no `MAP_PLACEABLE_SPAWN_VOLUME` placeables.
    pub spawn_room: Option<mapresolve::SpawnRoom>,
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

/// The practice range's resolved chunk set, opened once per caller.
fn practice_scene(data: &GameData, sources: &Sources) -> Result<mapresolve::Scene, String> {
    let kinds = sources
        .chunk_kinds()
        .map_err(|e| format!("the chunk-kind table: {e}"))?;
    mapresolve::resolve_practice_range(data, &kinds)
        .map(|p| p.scene)
        .map_err(|e| format!("the practice range does not resolve: {e}"))
}

/// Every placed model of the resolved scene, in chunk order.
///
/// A model-group placeable's own transform is identity; its groups carry looks
/// and its entries carry the world transforms, one per placed instance. A
/// single-model placeable carries its transform on the placeable itself.
pub fn placements(data: &GameData, sources: &Sources) -> Result<Vec<Placement>, String> {
    let scene = practice_scene(data, sources)?;
    Ok(placements_from(&scene))
}

fn placements_from(scene: &mapresolve::Scene) -> Vec<Placement> {
    let mut out: Vec<Placement> = Vec::new();
    for chunk in &scene.chunks {
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
                                scale: entry.scale,
                                rotation: entry.rotation,
                            });
                        }
                    }
                }
                Body::Model(_) | Body::SingleModel(_) => out.push(Placement {
                    model,
                    translation: p.translation,
                    scale: p.scale,
                    rotation: p.rotation,
                }),
                _ => {}
            }
        }
    }
    out
}

/// What a model's holders are, before the selection filter drops any: one row
/// per (holder, model) in holder order.
///
/// The range's collision is not only meshes: a holder's shape mode selects the
/// descriptor type (`0` mesh, `1` point cloud, `2` capsule, `4` sphere,
/// `model_body::shape_for`), and the filter also drops a holder whose mask word
/// is zero (`model_body::holder_selected` with the world mask all ones). A world
/// that carries only meshes is therefore missing whatever the other modes
/// describe, which is what [`RangeWorld::build`] used to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HolderRow {
    pub model: u64,
    /// `holder+0x20`, the shape mode.
    pub shape_mode: i32,
    /// `holder+0x2a`, the filter byte.
    pub mode_byte: u8,
    /// `holder+0x10`, the mask word.
    pub mask: u64,
    /// Whether the holder embeds a mesh block at `+0x08`.
    pub has_mesh: bool,
    /// Whether the filter keeps it (`model_body::holder_selected`, world mask
    /// all ones).
    pub selected: bool,
    /// The descriptor type the client builds for it, when the mode is known.
    pub shape: Option<model_body::Shape>,
}

/// Every holder of every distinct model the scene places, in model order.
pub fn holder_rows(
    data: &GameData,
    sources: &Sources,
    reg: &StuRegistry,
) -> Result<Vec<HolderRow>, String> {
    let placements = placements(data, sources)?;
    let mut distinct: Vec<u64> = placements.iter().map(|p| p.model).collect();
    distinct.sort_unstable();
    distinct.dedup();
    let mut out = Vec::new();
    for model in distinct {
        let bytes = data
            .open_asset(model)
            .map_err(|e| format!("model {model:#x}: {e}"))?;
        let mut stu = mstu::parse(&bytes).map_err(|e| format!("model {model:#x}: {e}"))?;
        for instance in stu.instances_of(STU_MODEL) {
            for holder in holder_instances(&mut stu, instance, reg) {
                let mode_byte = field_u64(&mut stu, holder, FIELD_HOLDER_MODE_BYTE, reg) as u8;
                let mask = field_u64(&mut stu, holder, FIELD_HOLDER_MASK, reg);
                let shape_mode = field_u64(&mut stu, holder, FIELD_HOLDER_SHAPE_MODE, reg) as i32;
                let has_mesh = holder_mesh(&stu, holder).is_some();
                out.push(HolderRow {
                    model,
                    shape_mode,
                    mode_byte,
                    mask,
                    has_mesh,
                    selected: model_body::holder_selected(
                        mode_byte,
                        u64::from(has_mesh),
                        mask,
                        u64::MAX,
                    ),
                    shape: model_body::shape_for(shape_mode),
                });
            }
        }
    }
    Ok(out)
}

/// One model's selected collision holders' mesh arrays, in holder order.
fn model_shapes(
    data: &GameData,
    model: u64,
    reg: &StuRegistry,
) -> Result<Vec<(MeshArrays, MeshSource)>, String> {
    let mut out = Vec::new();
    let bytes = data
        .open_asset(model)
        .map_err(|e| format!("model 0x{model:016x}: {e}"))?;
    let mut stu = mstu::parse(&bytes).map_err(|e| format!("model 0x{model:016x}: {e}"))?;
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
            // The range's holders are shape mode 0: the model mesh, descriptor
            // type 3. Any other mode is a different descriptor type whose mesh
            // arrays are not these, so it is not a shape this world can carry.
            // Say so: a silent skip is how a map loses its capsule and sphere
            // holders without anyone noticing.
            if model_body::shape_for(shape_mode) != Some(model_body::Shape::Mesh) {
                println!(
                    "collision: model {model:#018x} holder {shape_mode} is not a mesh holder (descriptor type {}); not swept",
                    model_body::shape_for(shape_mode)
                        .map(|shape| shape.descriptor_type())
                        .unwrap_or(0xff)
                );
                continue;
            }
            out.push(block_arrays(&mut stu, block, reg));
        }
    }
    Ok(out)
}

/// The mesh points' box, in the shape's frame.
fn local_bounds(bytes: &[u8], offset: usize, count: usize) -> ([f32; 3], [f32; 3]) {
    let mut low = [f32::INFINITY; 3];
    let mut high = [f32::NEG_INFINITY; 3];
    for i in 0..count {
        let at = offset + i * 0x10;
        for lane in 0..3 {
            let v = f32::from_le_bytes(bytes[at + lane * 4..at + lane * 4 + 4].try_into().unwrap());
            low[lane] = low[lane].min(v);
            high[lane] = high[lane].max(v);
        }
    }
    (low, high)
}

impl RangeWorld {
    /// Assemble the range: every placed model's mesh holders into one blob, one
    /// [`PlacedShape`] per (placement, mesh holder).
    pub fn build(data: &GameData, sources: &Sources, reg: &StuRegistry) -> Result<Self, String> {
        let scene = practice_scene(data, sources)?;
        let spawn_room = mapresolve::spawn_room(&scene, data);
        let placements = placements_from(&scene);
        let mut distinct: Vec<u64> = placements.iter().map(|p| p.model).collect();
        distinct.sort_unstable();
        distinct.dedup();

        let mut world = MeshWorld::new(WORLD_BASE);
        let mut by_model: std::collections::BTreeMap<u64, Vec<(usize, [f32; 3], [f32; 3])>> =
            std::collections::BTreeMap::new();
        for model in &distinct {
            let mut ids = Vec::new();
            for (arrays, words) in model_shapes(data, *model, reg)? {
                let index = world.push(&arrays, &words);
                let (low, high) = local_bounds(&arrays.points, 0, arrays.point_count());
                ids.push((index, low, high));
            }
            if !ids.is_empty() {
                by_model.insert(*model, ids);
            }
        }

        let mut placed = Vec::new();
        for placement in &placements {
            let Some(shapes) = by_model.get(&placement.model) else {
                continue;
            };
            for (shape, low, high) in shapes {
                placed.push(PlacedShape {
                    shape: *shape,
                    model: placement.model,
                    translation: placement.translation,
                    scale: placement.scale,
                    rotation: placement.rotation,
                    low: *low,
                    high: *high,
                });
            }
        }
        // The map's own level collider: the `MAP_CHUNK_COLLISION` body is one
        // mesh record in world coordinates (`ow_tank::collision_level`), so it
        // is placed at the origin with no rotation. The per-model holder meshes
        // are coarse blocking volumes; this is the ground, the floors and the
        // walls the map is walked on.
        let mut level_shapes = 0;
        let kinds = sources
            .chunk_kinds()
            .map_err(|e| format!("the chunk-kind table: {e}"))?;
        let scene = mapresolve::resolve_practice_range(data, &kinds)
            .map_err(|e| format!("the practice range does not resolve: {e}"))?;
        for chunk in &scene.scene.chunks {
            if chunk.kind != chunkkind::KIND_COLLISION
                && chunk.kind != chunkkind::KIND_ADDITIVE_COLLISION
            {
                continue;
            }
            let payload = match data.open_asset(chunk.guid) {
                Ok(bytes) => bytes,
                Err(e) => return Err(format!("collision chunk {:#018x}: {e}", chunk.guid)),
            };
            let level = collision_level::parse(&payload)
                .map_err(|e| format!("collision chunk {:#018x}: {e}", chunk.guid))?;
            let index = world.push(&level.arrays, &level.words);
            placed.push(PlacedShape {
                shape: index,
                model: chunk.guid,
                translation: [0.0; 3],
                scale: [1.0; 3],
                rotation: [0.0, 0.0, 0.0, 1.0],
                low: level.root.0,
                high: level.root.1,
            });
            level_shapes += 1;
            println!(
                "level collision: {} sector(s), {} vertices, {} nodes, {} triangles, root ({:.1}, {:.1}, {:.1}) .. ({:.1}, {:.1}, {:.1}), mask {:#06x}",
                level.sectors,
                level.arrays.point_count(),
                level.words.node_count,
                level.words.field_3c,
                level.root.0[0],
                level.root.0[1],
                level.root.0[2],
                level.root.1[0],
                level.root.1[1],
                level.root.1[2],
                level.mask
            );
        }
        if placed.is_empty() {
            return Err("the range's models carry no collision mesh holder".into());
        }
        Ok(RangeWorld {
            mesh: world,
            placed,
            level_shapes,
            spawn_room,
        })
    }

    /// The map's own level collider bodies, in chunk order: the
    /// `MAP_CHUNK_COLLISION` / `MAP_CHUNK_ADDITIVE_COLLISION` meshes, which are
    /// already in world coordinates.
    ///
    /// These are the static map's collision. The per-model holder meshes
    /// ([`RangeWorld::placed`] before them) are the *models'* own meshes, whose
    /// read consumer in this image is a query
    /// (`FUN_14210bac0` -> `0x142caf6c0`, which writes nothing persistent), and
    /// whose Domino body route is reached from the collision **component**,
    /// whose attach is decided by an unread engine driver
    /// (`docs/formats/static-collision.md`, checks 9-12). Using them as walking
    /// collision is the open question; the measurement is in
    /// `docs/formats/character-controller.md`.
    pub fn level(&self) -> &[PlacedShape] {
        &self.placed[self.placed.len() - self.level_shapes..]
    }

    /// The shapes' total point count, for a load report.
    pub fn point_count(&self) -> usize {
        self.mesh
            .shapes()
            .iter()
            .map(|slot| slot.point_count as usize)
            .sum()
    }
}

/// Where the blob lives in the guest's address space. The client's addresses
/// are heap allocations; any base works because every record pointer is derived
/// from it and the walk only subtracts it.
pub const WORLD_BASE: u64 = 0x0000_0001_0000_0000;

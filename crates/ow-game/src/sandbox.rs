//! The Bevy side: turning the sim and the install's geometry into a scene.
//!
//! Everything here is shell work — meshes, materials, transforms, text. The
//! numbers it reads come from `ow-sim` (the sandbox state) and `ow-scene` (the
//! map), and it never computes simulation state itself.

use bevy::prelude::*;
use bevy::render::mesh::{Indices, Mesh, PrimitiveTopology};
use bevy::render::render_asset::RenderAssetUsages;
use ow_scene::Vertex as SceneVertex;

/// The standing surface. Flat is the test range; `Map` is the install's own
/// collider, so the sandbox stands on real geometry.
pub enum Ground {
    Flat(f32),
    Map(ow_scene::Collider),
}

impl Ground {
    pub fn at(&self, x: f32, z: f32, from: f32) -> f32 {
        match self {
            Ground::Flat(y) => *y,
            Ground::Map(c) => c.ground_at(x, z, from),
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Ground::Flat(_) => "flat test range",
            Ground::Map(_) => "install map",
        }
    }
}

/// A bot's scene entity, indexed by its position in `Sim::world.combatants`.
#[derive(Component)]
pub struct BotIndex(pub usize);

/// The player camera.
#[derive(Component)]
pub struct PlayerCam;

/// The HUD text.
#[derive(Component)]
pub struct Hud;

/// Build a Bevy mesh from plain vertex/index arrays.
pub fn mesh_of(positions: &[[f32; 3]], normals: &[[f32; 3]], indices: &[u32]) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions.to_vec());
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals.to_vec());
    mesh.insert_indices(Indices::U32(indices.to_vec()));
    mesh
}

/// A flat ground grid with a few pillars, so movement and hitscan have
/// something measurable to work against without an install.
pub fn test_range_meshes() -> (Mesh, Mesh) {
    const HALF: f32 = 100.0;
    let positions = vec![
        [-HALF, 0.0, -HALF],
        [HALF, 0.0, -HALF],
        [HALF, 0.0, HALF],
        [-HALF, 0.0, HALF],
    ];
    let normals = vec![[0.0, 1.0, 0.0]; 4];
    let indices = vec![0u32, 2, 1, 0, 3, 2];
    let floor = mesh_of(&positions, &normals, &indices);

    // Four pillars at the compass points, 12 m out.
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut indices = Vec::new();
    for (i, (x, z)) in [(-12.0f32, 0.0f32), (12.0, 0.0), (0.0, -12.0), (0.0, 12.0)]
        .into_iter()
        .enumerate()
    {
        let base = positions.len() as u32;
        let (hw, h) = (0.6f32, 3.0f32);
        let bottom = [
            [x - hw, 0.0, z - hw],
            [x + hw, 0.0, z - hw],
            [x + hw, 0.0, z + hw],
            [x - hw, 0.0, z + hw],
        ];
        let top = [
            [x - hw, h, z - hw],
            [x + hw, h, z - hw],
            [x + hw, h, z + hw],
            [x - hw, h, z + hw],
        ];
        for p in bottom.iter().chain(top.iter()) {
            positions.push(*p);
            normals.push([0.0, 1.0, 0.0]);
        }
        // Sides.
        for k in 0..4u32 {
            let (a, b) = (k, (k + 1) % 4);
            indices.extend_from_slice(&[base + a, base + b, base + 4 + b]);
            indices.extend_from_slice(&[base + a, base + 4 + b, base + 4 + a]);
        }
        // Cap.
        indices.extend_from_slice(&[base + 4, base + 5, base + 6]);
        indices.extend_from_slice(&[base + 4, base + 6, base + 7]);
        let _ = i;
    }
    (floor, mesh_of(&positions, &normals, &indices))
}

/// A 0.8 x 1.8 x 0.8 box, the bot stand-in until hit shapes are extracted
/// (docs/rewrite-plan.md Q8).
// FIT-COM-07: bots: stand-in boxes, no client AI
pub fn bot_box() -> Mesh {
    let (hx, hy, hz) = (0.4f32, 0.9f32, 0.4f32);
    let positions = vec![
        [-hx, -hy, -hz],
        [hx, -hy, -hz],
        [hx, hy, -hz],
        [-hx, hy, -hz],
        [-hx, -hy, hz],
        [hx, -hy, hz],
        [hx, hy, hz],
        [-hx, hy, hz],
    ];
    let normals = vec![
        [0.0, 0.0, -1.0],
        [0.0, 0.0, -1.0],
        [0.0, 0.0, -1.0],
        [0.0, 0.0, -1.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, 1.0],
    ];
    let indices = vec![
        0, 1, 2, 0, 2, 3, // -z
        5, 4, 7, 5, 7, 6, // +z
        4, 0, 3, 4, 3, 7, // -x
        1, 5, 6, 1, 6, 2, // +x
        3, 2, 6, 3, 6, 7, // +y
        4, 5, 1, 4, 1, 0, // -y
    ];
    mesh_of(&positions, &normals, &indices)
}

/// Bake one map scene into Bevy meshes: one mesh per draw (a model), one
/// entity per placement. The shared vertex buffer is split per draw so each
/// mesh owns exactly the vertices its indices touch.
pub fn bake_map(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    scene: &ow_scene::Scene,
) -> usize {
    let material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.55, 0.55, 0.58),
        perceptual_roughness: 0.95,
        cull_mode: None,
        ..default()
    });
    let mut spawned = 0usize;
    for draw in &scene.draws {
        let first = draw.first_index as usize;
        let last = first + draw.index_count as usize;
        let mut remap = std::collections::HashMap::new();
        let mut positions: Vec<[f32; 3]> = Vec::new();
        let mut normals: Vec<[f32; 3]> = Vec::new();
        let mut indices: Vec<u32> = Vec::with_capacity(draw.index_count as usize);
        for &index in &scene.indices[first..last] {
            let global = (index as i64 + draw.base_vertex as i64) as u32;
            let local = *remap.entry(global).or_insert_with(|| {
                let v: SceneVertex = scene.vertices[global as usize];
                positions.push(v.position);
                normals.push(v.normal);
                (positions.len() - 1) as u32
            });
            indices.push(local);
        }
        let mesh = meshes.add(mesh_of(&positions, &normals, &indices));
        let start = draw.instance_start as usize;
        let end = start + draw.instance_count as usize;
        for instance in &scene.instances[start..end] {
            commands.spawn(PbrBundle {
                mesh: mesh.clone(),
                material: material.clone(),
                transform: Transform::from_matrix(Mat4::from_cols_array_2d(&instance.model)),
                ..default()
            });
            spawned += 1;
        }
    }
    spawned
}

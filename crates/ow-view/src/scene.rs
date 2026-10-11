//! Turning map chunks and models into one buffer of triangles.
//!
//! A map is a set of chunks; a chunk is a set of placements; a placement is a
//! model and a transform. So the scene is built by walking that chain, and the
//! same model is usually placed many times, which is why models are cached by
//! GUID rather than re-read per placement.

use std::collections::HashMap;

use glam::{Mat4, Quat, Vec3};

use ow_tank::mapchunk::{Kind, MapChunk};
use ow_tank::{guid, GameData, ManifestSelector};

use crate::Vertex;

pub struct Scene {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub placements: usize,
    pub models_loaded: usize,
    /// Where the camera starts, and which way it faces.
    pub eye: Vec3,
    pub yaw: f32,
}

impl Scene {
    fn empty() -> Scene {
        Scene {
            vertices: Vec::new(),
            indices: Vec::new(),
            placements: 0,
            models_loaded: 0,
            eye: Vec3::new(0.0, 2.0, 0.0),
            yaw: 0.0,
        }
    }
}

fn parse_vec3(text: &str) -> Option<Vec3> {
    let parts: Vec<f32> = text
        .split(',')
        .map(|p| p.trim().parse().ok())
        .collect::<Option<Vec<_>>>()?;
    if parts.len() != 3 {
        return None;
    }
    Some(Vec3::new(parts[0], parts[1], parts[2]))
}

pub fn build(args: &[String]) -> Result<Scene, String> {
    let flag = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };

    let root = ow_casc::install::detect_root().ok_or(
        "no Overwatch install found; set OW_INSTALL to the folder containing .build.info",
    )?;
    let sources = ow_tank::keytable::Sources::discover(&[root.clone()]);
    let install_root = root.display().to_string();
    let data = GameData::open(&install_root, &ManifestSelector::default(), &sources)
        .map_err(|e| e.to_string())?;

    let mut scene = Scene::empty();

    if args.iter().any(|a| a == "--list") {
        let count: usize = flag("--count").and_then(|v| v.parse().ok()).unwrap_or(40);
        let guids = data.guids_of_type(0x0BC);
        println!("{} map chunk assets; the first {count}:", guids.len());
        let resolves = |g: u64| data.registry().get(g).is_some();
        for guid in guids.iter().take(count) {
            let Ok(bytes) = data.bytes_of(*guid) else {
                continue;
            };
            if let Ok((kind, chunk)) = MapChunk::detect(&bytes, &resolves) {
                let hits = chunk.resolvable_models(&resolves);
                if hits > 0 {
                    println!(
                        "  0x{guid:016x}  {:>8} bytes  {}  {hits} models resolve  eye guess {}",
                        bytes.len(),
                        kind.label(),
                        eye_guess(&chunk)
                            .map(|v| format!("({:.0}, {:.0}, {:.0})", v.x, v.y, v.z))
                            .unwrap_or_else(|| "-".into())
                    );
                }
            }
        }
        std::process::exit(0);
    }

    // Which chunks to draw. Explicitly listed, or everything the install has --
    // which is far too much for one buffer, so the count is capped and the cap
    // is printed rather than silently applied.
    let chunk_guids: Vec<u64> = match flag("--chunks") {
        Some(list) => list
            .split(',')
            .map(|s| {
                u64::from_str_radix(s.trim().trim_start_matches("0x"), 16)
                    .map_err(|_| format!("{s:?} is not a GUID"))
            })
            .collect::<Result<_, _>>()?,
        None => {
            let cap: usize = flag("--max-chunks").and_then(|v| v.parse().ok()).unwrap_or(12);
            let guids = data.guids_of_type(0x0BC);
            println!(
                "no --chunks given; taking the first {cap} of {} map chunks",
                guids.len()
            );
            guids.into_iter().take(cap).collect()
        }
    };

    let resolves = |g: u64| data.registry().get(g).is_some();
    let mut models: HashMap<u64, ow_tank::model::Model> = HashMap::new();
    let mut chunk_kinds: HashMap<u64, Kind> = HashMap::new();
    let mut bounds_min = Vec3::splat(f32::MAX);
    let mut bounds_max = Vec3::splat(f32::MIN);
    // Where the placements are, which is where the map's contents are -- unlike
    // the centre of the bounding box, which for a map with a tall ceiling and a
    // deep pit can land inside a wall or under the floor.
    let mut origins: Vec<Vec3> = Vec::new();
    let mut model_failures = 0usize;

    for guid in &chunk_guids {
        let Ok(bytes) = data.bytes_of(*guid) else {
            eprintln!("  chunk 0x{guid:016x}: not readable");
            continue;
        };
        let Ok((kind, chunk)) = MapChunk::detect(&bytes, &resolves) else {
            eprintln!("  chunk 0x{guid:016x}: not a readable map chunk");
            continue;
        };
        chunk_kinds.insert(*guid, kind);
        let mut placed = 0usize;

        for placeable in &chunk.placeables {
            // Where the placements are is where the map's contents are; the
            // centre of the bounding box can sit in a wall or under the floor.
            origins.push(Vec3::from_array(placeable.translation));
            let Some(model_guid) = placeable.model.or(placeable.entity) else {
                continue;
            };
            if guid::type_of(model_guid) != guid::types::MODEL {
                continue;
            }
            if !models.contains_key(&model_guid) {
                let Ok(model_bytes) = data.bytes_of(model_guid) else {
                    model_failures += 1;
                    continue;
                };
                let loader = |g: u64| data.bytes_of(g).ok();
                match ow_tank::model::parse(&model_bytes, &loader) {
                    Ok(model) => {
                        models.insert(model_guid, model);
                    }
                    Err(_) => {
                        model_failures += 1;
                        continue;
                    }
                }
            }
            let Some(model) = models.get(&model_guid) else {
                continue;
            };

            let transform = placement_matrix(placeable);
            let base = scene.vertices.len() as u32;
            for mesh in &model.meshes {
                for (i, position) in mesh.positions.iter().enumerate() {
                    let world = transform.transform_point3(Vec3::from_array(*position));
                    let normal = mesh
                        .normals
                        .get(i)
                        .copied()
                        .map(Vec3::from_array)
                        .unwrap_or(Vec3::Y);
                    // The model's normals are in model space, so the placement
                    // has to rotate them; scaling is ignored, which only
                    // matters for lighting on squashed props.
                    let normal = transform.transform_vector3(normal).normalize_or(Vec3::Y);
                    scene.vertices.push(Vertex {
                        position: world.to_array(),
                        normal: normal.to_array(),
                    });
                    bounds_min = bounds_min.min(world);
                    bounds_max = bounds_max.max(world);
                }
                for index in &mesh.indices {
                    scene.indices.push(base + index);
                }
            }
            placed += 1;
        }
        scene.placements += placed;
        println!(
            "  chunk 0x{guid:016x}: {} placeables, {placed} drawn",
            chunk.placeables.len()
        );
    }

    scene.models_loaded = models.len();
    if model_failures > 0 {
        eprintln!("{model_failures} model(s) could not be read");
    }
    if scene.indices.is_empty() {
        return Err("no geometry: none of those chunks placed a readable model".into());
    }

    // Start the camera among the placements, facing along whichever horizontal
    // axis the geometry is longer in.
    let size = bounds_max - bounds_min;
    let centroid = if origins.is_empty() {
        (bounds_min + bounds_max) * 0.5
    } else {
        origins.iter().copied().sum::<Vec3>() / origins.len() as f32
    };
    scene.eye = match flag("--at").and_then(|v| parse_vec3(&v)) {
        Some(v) => v,
        None => centroid,
    };
    scene.yaw = if size.x >= size.z { 0.0 } else { std::f32::consts::FRAC_PI_2 };
    println!(
        "bounds ({:.0}, {:.0}, {:.0}) .. ({:.0}, {:.0}, {:.0}); eye ({:.0}, {:.0}, {:.0})",
        bounds_min.x, bounds_min.y, bounds_min.z,
        bounds_max.x, bounds_max.y, bounds_max.z,
        scene.eye.x, scene.eye.y, scene.eye.z
    );
    Ok(scene)
}

/// Where a chunk's geometry is centred, used to guess a viewpoint.
fn eye_guess(chunk: &MapChunk) -> Option<Vec3> {
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    let mut any = false;
    for p in &chunk.placeables {
        if p.model.is_none() {
            continue;
        }
        let t = Vec3::from_array(p.translation);
        min = min.min(t);
        max = max.max(t);
        any = true;
    }
    any.then(|| (min + max) * 0.5)
}

/// A placement's transform: translate, rotate, scale.
///
/// The rotation is stored as a quaternion in x, y, z, w order, which is what
/// the file holds; the tooling's `teQuat` is the same order.
fn placement_matrix(p: &ow_tank::mapchunk::Placeable) -> Mat4 {
    let translation = Vec3::from_array(p.translation);
    let scale = Vec3::from_array(p.scale);
    let raw = Quat::from_xyzw(
        p.rotation[0],
        p.rotation[1],
        p.rotation[2],
        p.rotation[3],
    );
    // A placement with no rotation stores four zeroes, which is not a unit
    // quaternion; normalising it would produce NaN and lose the whole model.
    let rotation = if raw.length_squared() > 1e-6 {
        raw.normalize()
    } else {
        Quat::IDENTITY
    };
    Mat4::from_scale_rotation_translation(scale, rotation, translation)
}

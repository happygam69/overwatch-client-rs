//! Turning map chunks and models into something the GPU can draw.
//!
//! A map is a set of chunks; a chunk is a set of placements; a placement is a
//! model and a transform. The obvious way to build that is to expand every
//! placement into its own copy of the model's vertices -- and it is the wrong
//! way, because a map places the same prop hundreds of times. Expanding one map
//! reached 17 GB of vertices.
//!
//! So each model is stored once and drawn once per placement, with the
//! transform supplied per instance. Instances are grouped by model, which keeps
//! it to one draw call per distinct model rather than one per placement.

use std::collections::HashMap;

use glam::{Mat4, Quat, Vec3};

use ow_tank::mapchunk::MapChunk;
use ow_tank::{guid, GameData, ManifestSelector};

use crate::Vertex;

/// A model's slice of the shared vertex and index buffers.
#[derive(Debug, Clone, Copy)]
pub struct Draw {
    pub first_index: u32,
    pub index_count: u32,
    /// Added to every index, so one buffer can hold every model.
    pub base_vertex: i32,
    pub instance_start: u32,
    pub instance_count: u32,
    /// The model this came from, for reporting.
    #[allow(dead_code)]
    pub model: u64,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Instance {
    pub model: [[f32; 4]; 4],
}

pub struct Scene {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub instances: Vec<Instance>,
    pub draws: Vec<Draw>,
    pub placements: usize,
    pub models_loaded: usize,
    /// Where the camera starts, and which way it faces.
    pub eye: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    /// Every placement origin, for picking viewpoints.
    origins: Vec<Vec3>,
}

impl Scene {
    fn empty() -> Scene {
        Scene {
            vertices: Vec::new(),
            indices: Vec::new(),
            instances: Vec::new(),
            draws: Vec::new(),
            placements: 0,
            models_loaded: 0,
            eye: Vec3::new(0.0, 2.0, 0.0),
            yaw: 0.0,
            pitch: -0.1,
            origins: Vec::new(),
        }
    }

    /// A spread of viewpoints through the placements, for looking a map over.
    pub fn tour_points(&self, count: usize) -> Vec<Vec3> {
        if self.origins.is_empty() {
            return vec![self.eye];
        }
        let step = (self.origins.len() / count.max(1)).max(1);
        self.origins.iter().step_by(step).take(count).copied().collect()
    }

    /// A copy of the scene's geometry seen from somewhere else.
    pub fn view_from(eye: Vec3, yaw: f32) -> Scene {
        let mut scene = Scene::empty();
        scene.eye = eye;
        scene.yaw = yaw;
        scene.pitch = -0.15;
        scene
    }

    pub fn triangle_count(&self) -> usize {
        self.draws
            .iter()
            .map(|d| (d.index_count / 3) as usize * d.instance_count as usize)
            .sum()
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
    let resolves = |g: u64| data.registry().get(g).is_some();

    if args.iter().any(|a| a == "--list") {
        let count: usize = flag("--count").and_then(|v| v.parse().ok()).unwrap_or(40);
        let guids = data.guids_of_type(0x0BC);
        println!("{} map chunk assets; the first {count}:", guids.len());
        for guid in guids.iter().take(count) {
            let Ok(bytes) = data.bytes_of(*guid) else {
                continue;
            };
            if let Ok((kind, chunk)) = MapChunk::detect(&bytes, &resolves) {
                let hits = chunk.resolvable_models(&resolves);
                if hits > 0 {
                    println!(
                        "  0x{guid:016x}  {:>8} bytes  {}  {hits} models resolve  centre {}",
                        bytes.len(),
                        kind.label(),
                        chunk_centre(&chunk)
                            .map(|v| format!("({:.0}, {:.0}, {:.0})", v.x, v.y, v.z))
                            .unwrap_or_else(|| "-".into())
                    );
                }
            }
        }
        std::process::exit(0);
    }

    // Which chunks to draw.
    let chunk_guids: Vec<u64> = if let Some(path) = flag("--chunks-from") {
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("could not read {path}: {e}"))?;
        text.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|l| {
                u64::from_str_radix(l.trim_start_matches("0x"), 16)
                    .map_err(|_| format!("{l:?} is not a GUID"))
            })
            .collect::<Result<Vec<_>, _>>()?
    } else if let Some(path) = flag("--chunk-file") {
        // A map chunk read out of a running game's memory rather than out of
        // the manifest. Its bytes are already decoded, so there is no asset to
        // look up and no GUID to know.
        let bytes = std::fs::read(&path).map_err(|e| format!("could not read {path}: {e}"))?;
        println!("{path}: {} bytes", bytes.len());
        let mut geometry: HashMap<u64, Loaded> = HashMap::new();
        let mut order: Vec<u64> = Vec::new();
        let mut origins: Vec<Vec3> = Vec::new();
        let mut bounds_min = Vec3::splat(f32::MAX);
        let mut bounds_max = Vec3::splat(f32::MIN);
        let mut model_failures = 0usize;
        let loader = |g: u64| data.bytes_of(g).ok();
        let (_, chunk) = MapChunk::detect(&bytes, &resolves).map_err(|e| e.to_string())?;
        println!("{}", chunk.summary());
        for placeable in &chunk.placeables {
            let Some(model_guid) = placeable.model.or(placeable.entity) else {
                continue;
            };
            origins.push(Vec3::from_array(placeable.translation));
            if guid::type_of(model_guid) != guid::types::MODEL {
                continue;
            }
            if !geometry.contains_key(&model_guid) {
                let Ok(model_bytes) = data.bytes_of(model_guid) else {
                    model_failures += 1;
                    continue;
                };
                let Ok(model) = ow_tank::model::parse(&model_bytes, &loader) else {
                    model_failures += 1;
                    continue;
                };
                let vertex_start = scene.vertices.len() as u32;
                let index_start = scene.indices.len() as u32;
                let fine: Vec<&ow_tank::model::Mesh> =
                    model.meshes.iter().filter(|m| m.lod <= 0).collect();
                let use_meshes: Vec<&ow_tank::model::Mesh> =
                    if fine.is_empty() { model.meshes.iter().collect() } else { fine };
                for mesh in use_meshes {
                    let base = scene.vertices.len() as u32 - vertex_start;
                    for (i, position) in mesh.positions.iter().enumerate() {
                        let normal = mesh.normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]);
                        scene.vertices.push(Vertex { position: *position, normal });
                    }
                    for index in &mesh.indices {
                        scene.indices.push(base + index);
                    }
                }
                let index_count = scene.indices.len() as u32 - index_start;
                if index_count == 0 {
                    scene.vertices.truncate(vertex_start as usize);
                    scene.indices.truncate(index_start as usize);
                    continue;
                }
                order.push(model_guid);
                geometry.insert(
                    model_guid,
                    Loaded { vertex_start, index_start, index_count, instances: Vec::new() },
                );
            }
            let matrix = placement_matrix(placeable);
            if let Some(entry) = geometry.get_mut(&model_guid) {
                entry.instances.push(Instance { model: matrix.to_cols_array_2d() });
                scene.placements += 1;
            }
            let origin = Vec3::from_array(placeable.translation);
            bounds_min = bounds_min.min(origin - Vec3::splat(8.0));
            bounds_max = bounds_max.max(origin + Vec3::splat(8.0));
        }
        for model_guid in &order {
            let Some(entry) = geometry.get(model_guid) else { continue };
            if entry.index_count == 0 || entry.instances.is_empty() {
                continue;
            }
            let instance_start = scene.instances.len() as u32;
            scene.instances.extend_from_slice(&entry.instances);
            scene.draws.push(Draw {
                first_index: entry.index_start,
                index_count: entry.index_count,
                base_vertex: entry.vertex_start as i32,
                instance_start,
                instance_count: entry.instances.len() as u32,
                model: *model_guid,
            });
        }
        scene.models_loaded = order.len();
        scene.origins = origins;
        if model_failures > 0 {
            eprintln!("{model_failures} model(s) could not be read");
        }
        if scene.draws.is_empty() {
            return Err("no geometry in that chunk".into());
        }
        let size = bounds_max - bounds_min;
        let centroid = if scene.origins.is_empty() {
            (bounds_min + bounds_max) * 0.5
        } else {
            let mut xs: Vec<f32> = scene.origins.iter().map(|v| v.x).collect();
            let mut ys: Vec<f32> = scene.origins.iter().map(|v| v.y).collect();
            let mut zs: Vec<f32> = scene.origins.iter().map(|v| v.z).collect();
            xs.sort_by(f32::total_cmp);
            ys.sort_by(f32::total_cmp);
            zs.sort_by(f32::total_cmp);
            let mid = scene.origins.len() / 2;
            Vec3::new(xs[mid], ys[mid], zs[mid])
        };
        scene.eye = flag("--at").and_then(|v| parse_vec3(&v)).unwrap_or(centroid);
        scene.yaw = if size.x >= size.z { 0.0 } else { std::f32::consts::FRAC_PI_2 };
        println!(
            "{} distinct models, {} placements; eye ({:.0}, {:.0}, {:.0})",
            scene.models_loaded, scene.placements, scene.eye.x, scene.eye.y, scene.eye.z
        );
        return Ok(scene);
    } else if let Some(list) = flag("--chunks") {
        list.split(',')
            .map(|s| {
                u64::from_str_radix(s.trim().trim_start_matches("0x"), 16)
                    .map_err(|_| format!("{s:?} is not a GUID"))
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        let cap: usize = flag("--max-chunks").and_then(|v| v.parse().ok()).unwrap_or(12);
        let guids = data.guids_of_type(0x0BC);
        println!(
            "no --chunks given; taking the first {cap} of {} map chunks",
            guids.len()
        );
        guids.into_iter().take(cap).collect()
    };

    // A set of chunks gathered from a running game is not a map: it holds
    // whatever that session touched. Chunks belonging to the same map have
    // their contents in the same place, so grouping by proximity recovers the
    // maps without the map-to-chunk link, which is not in the data this
    // program can read.
    let chunk_guids = if args.iter().any(|a| a == "--clusters") || flag("--cluster").is_some() {
        let clusters = cluster_chunks(&data, &chunk_guids, &resolves);
        if args.iter().any(|a| a == "--clusters") {
            println!(
                "{} cluster(s) over {} chunks",
                clusters.len(),
                chunk_guids.len()
            );
            for (i, c) in clusters.iter().enumerate() {
                println!(
                    "  [{i:>3}] {:>5} chunks  centre ({:>7.0}, {:>7.0}, {:>7.0})  span ({:.0}, {:.0}, {:.0})",
                    c.chunks.len(),
                    c.centre.x,
                    c.centre.y,
                    c.centre.z,
                    c.span.x,
                    c.span.y,
                    c.span.z
                );
            }
            std::process::exit(0);
        }
        let wanted: usize = flag("--cluster").and_then(|v| v.parse().ok()).unwrap_or(0);
        clusters
            .into_iter()
            .nth(wanted)
            .map(|c| c.chunks)
            .unwrap_or_else(|| {
                eprintln!("no cluster {wanted}");
                std::process::exit(1);
            })
    } else {
        chunk_guids
    };

    // Model geometry, stored once. `model_at` says where each one landed in the
    // shared buffers; `by_model` accumulates the placements that use it.
    struct Loaded {
        vertex_start: u32,
        index_start: u32,
        index_count: u32,
        instances: Vec<Instance>,
    }
    let mut geometry: HashMap<u64, Loaded> = HashMap::new();
    let mut order: Vec<u64> = Vec::new();
    let mut bounds_min = Vec3::splat(f32::MAX);
    let mut bounds_max = Vec3::splat(f32::MIN);
    let mut origins: Vec<Vec3> = Vec::new();
    let mut model_failures = 0usize;
    let mut chunk_failures = 0usize;
    let mut entity_placements = 0usize;

    let loader = |g: u64| data.bytes_of(g).ok();

    for guid in &chunk_guids {
        let Ok(bytes) = data.bytes_of(*guid) else {
            chunk_failures += 1;
            continue;
        };
        let Ok((kind, chunk)) = MapChunk::detect(&bytes, &resolves) else {
            chunk_failures += 1;
            continue;
        };
        let _ = kind;

        for placeable in &chunk.placeables {
            let Some(model_guid) = placeable.model.or(placeable.entity) else {
                continue;
            };
            if placeable.entity.is_some() {
                entity_placements += 1;
            }
            if guid::type_of(model_guid) != guid::types::MODEL {
                continue;
            }
            if !geometry.contains_key(&model_guid) {
                let Ok(model_bytes) = data.bytes_of(model_guid) else {
                    model_failures += 1;
                    continue;
                };
                let Ok(model) = ow_tank::model::parse(&model_bytes, &loader) else {
                    model_failures += 1;
                    continue;
                };
                let vertex_start = scene.vertices.len() as u32;
                let index_start = scene.indices.len() as u32;
                // Only the finest level of detail. A model carries every LOD,
                // and drawing all of them at once inflated one map to 676M
                // triangles. Submeshes tagged -1 belong to every level.
                let base_meshes: Vec<&ow_tank::model::Mesh> = {
                    let fine: Vec<_> = model.meshes.iter().filter(|m| m.lod <= 0).collect();
                    if fine.is_empty() {
                        model.meshes.iter().collect()
                    } else {
                        fine
                    }
                };
                for mesh in base_meshes {
                    let base = scene.vertices.len() as u32 - vertex_start;
                    for (i, position) in mesh.positions.iter().enumerate() {
                        let normal = mesh.normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]);
                        scene.vertices.push(Vertex {
                            position: *position,
                            normal,
                        });
                    }
                    for index in &mesh.indices {
                        scene.indices.push(base + index);
                    }
                }
                let index_count = scene.indices.len() as u32 - index_start;
                if index_count == 0 {
                    // Roll back an empty model so it does not leave a gap.
                    scene.vertices.truncate(vertex_start as usize);
                    scene.indices.truncate(index_start as usize);
                    geometry.insert(
                        model_guid,
                        Loaded {
                            vertex_start,
                            index_start,
                            index_count: 0,
                            instances: Vec::new(),
                        },
                    );
                    continue;
                }
                order.push(model_guid);
                geometry.insert(
                    model_guid,
                    Loaded {
                        vertex_start,
                        index_start,
                        index_count,
                        instances: Vec::new(),
                    },
                );
            }

            let matrix = placement_matrix(placeable);
            let entry = geometry.get_mut(&model_guid).expect("just inserted");
            entry.instances.push(Instance {
                model: matrix.to_cols_array_2d(),
            });
            scene.placements += 1;

            let origin = Vec3::from_array(placeable.translation);
            origins.push(origin);
            // Approximate the model's extent so the bounds cover the geometry,
            // not just the origins.
            let reach = 8.0;
            bounds_min = bounds_min.min(origin - Vec3::splat(reach));
            bounds_max = bounds_max.max(origin + Vec3::splat(reach));
        }
    }

    // Flatten into one instance buffer, grouped so each model is one draw.
    for model_guid in &order {
        let Some(entry) = geometry.get(model_guid) else {
            continue;
        };
        if entry.index_count == 0 || entry.instances.is_empty() {
            continue;
        }
        let instance_start = scene.instances.len() as u32;
        scene.instances.extend_from_slice(&entry.instances);
        scene.draws.push(Draw {
            first_index: entry.index_start,
            index_count: entry.index_count,
            base_vertex: entry.vertex_start as i32,
            instance_start,
            instance_count: entry.instances.len() as u32,
            model: *model_guid,
        });
    }

    scene.models_loaded = order.len();
    if model_failures > 0 {
        eprintln!("{model_failures} model(s) could not be read");
    }
    if chunk_failures > 0 {
        eprintln!("{chunk_failures} chunk(s) could not be read");
    }
    if entity_placements > 0 {
        println!("{entity_placements} entity placements drawn as their model");
    }
    if scene.draws.is_empty() {
        return Err("no geometry: none of those chunks placed a readable model".into());
    }

    scene.origins = origins;
    let origins = &scene.origins;
    let size = bounds_max - bounds_min;
    // The median, not the mean. A map's placements are mostly where the map
    // is, and a handful of far-away ones drag a mean off into empty space --
    // which is what put the camera inside a wall.
    let centroid = if origins.is_empty() {
        (bounds_min + bounds_max) * 0.5
    } else {
        let mut xs: Vec<f32> = origins.iter().map(|v| v.x).collect();
        let mut ys: Vec<f32> = origins.iter().map(|v| v.y).collect();
        let mut zs: Vec<f32> = origins.iter().map(|v| v.z).collect();
        xs.sort_by(f32::total_cmp);
        ys.sort_by(f32::total_cmp);
        zs.sort_by(f32::total_cmp);
        let mid = origins.len() / 2;
        Vec3::new(xs[mid], ys[mid], zs[mid])
    };
    scene.eye = match flag("--at").and_then(|v| parse_vec3(&v)) {
        Some(v) => v,
        None => centroid,
    };
    scene.yaw = if size.x >= size.z {
        0.0
    } else {
        std::f32::consts::FRAC_PI_2
    };
    // An overhead view, for seeing a map's layout rather than standing in it.
    if args.iter().any(|a| a == "--overview") {
        let centre = (bounds_min + bounds_max) * 0.5;
        let height = size.length() * 0.9;
        scene.eye = Vec3::new(centre.x, bounds_max.y + height, centre.z);
        scene.yaw = 0.0;
        scene.pitch = -1.2;
        println!("overview from ({:.0}, {:.0}, {:.0})", scene.eye.x, scene.eye.y, scene.eye.z);
    }
    println!(
        "{} distinct models, {} placements, {} draws",
        scene.models_loaded,
        scene.placements,
        scene.draws.len()
    );
    println!(
        "bounds ({:.0}, {:.0}, {:.0}) .. ({:.0}, {:.0}, {:.0}); eye ({:.0}, {:.0}, {:.0})",
        bounds_min.x, bounds_min.y, bounds_min.z,
        bounds_max.x, bounds_max.y, bounds_max.z,
        scene.eye.x, scene.eye.y, scene.eye.z
    );
    Ok(scene)
}

/// Where a chunk's placements are centred, used to guess a viewpoint.
fn chunk_centre(chunk: &MapChunk) -> Option<Vec3> {
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

struct Cluster {
    chunks: Vec<u64>,
    centre: Vec3,
    span: Vec3,
}

/// Group chunks whose contents overlap in space.
///
/// Two chunks belong to the same map when their bounding boxes are close; a
/// single-link union over that relation recovers the maps. The threshold is
/// generous because a map's chunks tile it, so neighbours touch rather than
/// overlap, while different maps sit hundreds of units apart.
fn cluster_chunks(
    data: &GameData,
    guids: &[u64],
    resolves: &dyn Fn(u64) -> bool,
) -> Vec<Cluster> {
    const NEIGHBOURHOOD: f32 = 600.0;

    let mut boxes: Vec<(Vec3, Vec3)> = Vec::new();
    let mut kept: Vec<u64> = Vec::new();
    for guid in guids {
        let Ok(bytes) = data.bytes_of(*guid) else {
            continue;
        };
        let Ok((_, chunk)) = MapChunk::detect(&bytes, resolves) else {
            continue;
        };
        let mut min = Vec3::splat(f32::MAX);
        let mut max = Vec3::splat(f32::MIN);
        for p in &chunk.placeables {
            let t = Vec3::from_array(p.translation);
            min = min.min(t);
            max = max.max(t);
        }
        if min.x > max.x {
            continue;
        }
        boxes.push((min, max));
        kept.push(*guid);
    }

    let n = kept.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(parent: &mut Vec<usize>, mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for i in 0..n {
        for j in i + 1..n {
            let (amin, amax) = boxes[i];
            let (bmin, bmax) = boxes[j];
            // Distance between the two boxes, zero when they overlap.
            let gap = (amin - bmax).max(bmin - amax).max(Vec3::ZERO);
            if gap.length() <= NEIGHBOURHOOD {
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                if ri != rj {
                    parent[ri] = rj;
                }
            }
        }
    }

    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..n {
        let root = find(&mut parent, i);
        groups.entry(root).or_default().push(i);
    }
    let mut out: Vec<Cluster> = groups
        .into_values()
        .map(|members| {
            let mut min = Vec3::splat(f32::MAX);
            let mut max = Vec3::splat(f32::MIN);
            for &m in &members {
                min = min.min(boxes[m].0);
                max = max.max(boxes[m].1);
            }
            Cluster {
                chunks: members.iter().map(|m| kept[*m]).collect(),
                centre: (min + max) * 0.5,
                span: max - min,
            }
        })
        .collect();
    out.sort_by_key(|c| std::cmp::Reverse(c.chunks.len()));
    out
}

/// A placement's transform: translate, rotate, scale.
///
/// The rotation is stored as a quaternion in x, y, z, w order, which is what
/// the file holds.
fn placement_matrix(p: &ow_tank::mapchunk::Placeable) -> Mat4 {
    let translation = Vec3::from_array(p.translation);
    let scale = Vec3::from_array(p.scale);
    let raw = Quat::from_xyzw(p.rotation[0], p.rotation[1], p.rotation[2], p.rotation[3]);
    // A placement with no rotation stores four zeroes, which is not a unit
    // quaternion; normalising it would produce NaN and lose the whole model.
    let rotation = if raw.length_squared() > 1e-6 {
        raw.normalize()
    } else {
        Quat::IDENTITY
    };
    Mat4::from_scale_rotation_translation(scale, rotation, translation)
}

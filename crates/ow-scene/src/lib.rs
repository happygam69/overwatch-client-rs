//! Map geometry, assembled from the install's own map chunks and models.
//!
//! This is the data half of the viewers: it decodes map chunks, resolves the
//! models they place, and produces one vertex/index buffer plus per-placement
//! instances -- everything a renderer needs, with no renderer in it. `ow-client`
//! draws the buffer.

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    /// The model's first texcoord set. The vertex data carries two; the second
    /// is a lightmap/detail set nothing reads yet, so it is not carried here.
    pub uv: [f32; 2],
    /// Four entries into the scene's joint palette, **offset by one**: slot 0
    /// is the identity, so an unskinned vertex (a map prop) is
    /// `[1, 0, 0, 0]` with weights `[255, 0, 0, 0]` and the shader leaves it
    /// where it is without any second code path.
    pub joints: [u8; 4],
    /// Blend weights, `255` = 1.0 (the file stores them summing to exactly 255).
    pub weights: [u8; 4],
}

impl Default for Vertex {
    fn default() -> Vertex {
        Vertex {
            position: [0.0; 3],
            normal: [0.0; 3],
            uv: [0.0; 2],
            joints: [1, 0, 0, 0],
            weights: [255, 0, 0, 0],
        }
    }
}

// Turning map chunks and models into something the GPU can draw.
//
// A map is a set of chunks; a chunk is a set of placements; a placement is a
// model and a transform. The obvious way to build that is to expand every
// placement into its own copy of the model's vertices -- and it is the wrong
// way, because a map places the same prop hundreds of times. Expanding one map
// reached 17 GB of vertices.
//
// So each model is stored once and drawn once per placement, with the
// transform supplied per instance. Instances are grouped by model, which keeps
// it to one draw call per distinct model rather than one per placement.

use std::collections::HashMap;

use glam::{Mat4, Quat, Vec3};

use ow_tank::mapchunk::{MapChunk, Placeable};
use ow_tank::{guid, mapresolve, GameData, ManifestSelector};

/// The viewer's eye height above a floor.
///
/// It is the *viewer's* constant, not a client reading — the traced node is the
/// feet, so the client's own eye height is not measured here. Both the harness
/// and a view model's anchor use this one number so they cannot drift.
// FIT-CAM-05: eye height: assumed (the client's own was not measured)
pub const EYE_HEIGHT: f32 = 1.75;

/// A model's slice of the shared vertex and index buffers.
#[derive(Debug, Clone, Copy)]
pub struct Draw {
    pub first_index: u32,
    pub index_count: u32,
    /// Added to every index, so one buffer can hold every model.
    pub base_vertex: i32,
    pub instance_start: u32,
    pub instance_count: u32,
    /// Index into [`Scene::textures`]; 0 is the untextured white slot, which is
    /// what every map prop gets while the material keys are missing.
    pub texture: u32,
    /// The model this came from, for reporting.
    #[allow(dead_code)]
    pub model: u64,
}

/// A decoded texture, ready to be uploaded.
///
/// The bytes are RGBA8 because that is what both viewers can take; the source
/// format is kept for the report (`docs/formats/materials.md` has the table of
/// what the install ships).
#[derive(Debug, Clone)]
pub struct SceneTexture {
    pub guid: u64,
    pub width: u16,
    pub height: u16,
    pub rgba: Vec<u8>,
    pub format: String,
}

impl SceneTexture {
    /// A UV checker, for making "no material data" unmistakable: it is visibly
    /// *not* a texture, and it doubles as a UV reference (squares are 8 px on a
    /// 64 px tile, so tiling and seams are readable).
    pub fn uv_checker() -> SceneTexture {
        const N: usize = 64;
        const SQUARE: usize = 8;
        let mut rgba = vec![0u8; N * N * 4];
        for y in 0..N {
            for x in 0..N {
                let dark = ((x / SQUARE) + (y / SQUARE)) % 2 == 0;
                let v = if dark { 90 } else { 190 };
                let at = (y * N + x) * 4;
                rgba[at] = v;
                rgba[at + 1] = v;
                rgba[at + 2] = v;
                rgba[at + 3] = 255;
            }
        }
        SceneTexture {
            guid: 0,
            width: N as u16,
            height: N as u16,
            rgba,
            format: "uv checker (no material data)".into(),
        }
    }

    /// The 1×1 white slot every untextured draw points at.
    pub fn white() -> SceneTexture {
        SceneTexture {
            guid: 0,
            width: 1,
            height: 1,
            rgba: vec![255, 255, 255, 255],
            format: "white".into(),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Instance {
    pub model: [[f32; 4]; 4],
}

pub struct Scene {
    /// The placeholder bound where a draw has no readable material: white by
    /// default, a UV checker under `--uv-check` so an untextured surface cannot
    /// be mistaken for a textured one.
    // FIT-PRE-01: materials: white placeholder where the key is missing
    pub placeholder: SceneTexture,
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub instances: Vec<Instance>,
    pub draws: Vec<Draw>,
    /// Slot 0 is always white; a prop that has no readable material points at
    /// it, and the viewers say so rather than tinting it to look finished.
    pub textures: Vec<SceneTexture>,
    pub placements: usize,
    pub models_loaded: usize,
    /// Where the camera starts, and which way it faces.
    pub eye: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    /// The instance index a hero model was placed at, so a viewer can move it
    /// (the player's own body, once animation drives it) without rebuilding.
    pub hero_instance: Option<usize>,
    /// Where a **sandbox** scene (built with `--no-map`) puts its player
    /// standing on the ground: `x`, the ground height, `z`. A map scene leaves
    /// this `None`, and the harness derives a spawn from the geometry — which is
    /// meaningless for a plane and four boxes, whose vertex median lands on a
    /// box top instead of the floor.
    pub sandbox_spawn: Option<[f32; 3]>,
    /// The instance a **view model** was placed at, when one was: a first-person
    /// pass moves this every frame (see [`Scene::view_model_matrix`]).
    pub view_model_instance: Option<usize>,
    /// The instances the view model's **weapons** were placed at, with the
    /// view-model bone each hangs off and the hardpoint's local frame. Tracer
    /// carries one in each hand: the FP entity's child array names the weapon
    /// twice (two adjacent hardpoints) and the FP model's MSTU carries a
    /// hardpoint on each hand bone (ids 14 and 55).
    pub weapon_attachments: Vec<(usize, usize, Mat4)>,
    /// The FP skeleton's bone ids, so a weapon attachment's bone index can be
    /// mapped to the character rig by id.
    pub view_model_ids: Vec<u32>,
    /// The character rig and the c71 idle pose, used for the two weapon
    /// instances: the guns are authored on the character rig and their FP
    /// parent chain differs at 44/74.
    pub character_skin: Option<ow_tank::skin::Skin>,
    pub character_ids: Vec<u32>,
    // FIT-ANI-04: weapon attachment: read structure, chosen frame (c71 idle)
    pub character_weapon_pose: Option<ow_tank::clip::BlendPose>,
    /// The hero model and texture that were placed, for the HUD.
    pub hero_model: u64,
    pub hero_texture_guid: u64,
    /// An animation clip loaded with `--clip <guid>`, parsed as far as the
    /// verified parts go ([`ow_tank::clip::Clip`]). Kept so the pass that
    /// evaluates its payload has the data at hand; nothing consumes its body yet.
    pub clip: Option<ow_tank::clip::Clip>,
    pub clip_guid: u64,
    /// The joint palette the vertex shader reads: slot 0 is always the
    /// identity (what an unskinned vertex uses), then one matrix per bone. In
    /// bind pose they are all the identity, which is why the file's own bind
    /// pose and a posed skeleton go down the same path. A hero owns the tail
    /// it uploads (base 1 on an empty scene, or appended past the map models'
    /// pool); map models share the identity pool at base 1, which is exact in
    /// bind pose and keeps every joint index valid.
    pub joints: Vec<[[f32; 4]; 4]>,
    /// Where the hero's palette starts inside `joints`: 1 when the scene was
    /// empty (the hero replaced the placeholder), or the identity pool's size
    /// when the map models grew it first. A per-frame re-pose must write its
    /// matrices at this base, not at 1, or the hero reads map-identity slots
    /// and the pool slots take hero bones.
    pub hero_joint_base: u32,
    /// The pose applied, as `(bone, axis, degrees)`, for the report.
    pub pose: Vec<(usize, char, f32)>,
    /// The skin the hero was drawn with, for a viewer that wants to pose it.
    pub skin: Option<ow_tank::skin::Skin>,
    /// The **model asset GUID** the hero's skin was built from, when a
    /// hero/view-model was placed. The animation feed pairs a clip's bone IDs
    /// (`Skeleton::ids`) against this model's skeleton, so the record→bone
    /// join can name them (the blend-card family's records carry per-bone
    /// pose blocks).
    pub hero_model_guid: Option<u64>,
    /// Every placement origin, for picking viewpoints.
    origins: Vec<Vec3>,
}

impl Scene {
    pub fn empty() -> Scene {
        Scene {
            placeholder: SceneTexture::white(),
            vertices: Vec::new(),
            indices: Vec::new(),
            instances: Vec::new(),
            draws: Vec::new(),
            textures: vec![SceneTexture::white()],
            placements: 0,
            models_loaded: 0,
            eye: Vec3::new(0.0, 2.0, 0.0),
            yaw: 0.0,
            pitch: -0.1,
            hero_instance: None,
            sandbox_spawn: None,
            view_model_instance: None,
            weapon_attachments: Vec::new(),
            view_model_ids: Vec::new(),
            character_skin: None,
            character_ids: Vec::new(),
            character_weapon_pose: None,
            hero_model: 0,
            hero_texture_guid: 0,
            clip: None,
            clip_guid: 0,
            joints: vec![Mat4::IDENTITY.to_cols_array_2d()],
            hero_joint_base: 1,
            pose: Vec::new(),
            skin: None,
            hero_model_guid: None,
            origins: Vec::new(),
        }
    }

    /// Where the player's own body belongs for a given eye and look.
    ///
    /// The body's transform **is the character's**, and the client's own
    /// configuration is what says so: all twelve `firstPerson.*` offsets default
    /// to `0.0` (a packed defaults array at v2b `0x7ff7c89d1ae0`, one float per
    /// var — read by `dev/fp_table.py`), so the view model is drawn where the
    /// character is drawn, with nothing added. Two details follow from the same
    /// read of the model rather than from a guess:
    ///
    /// * **Origin at the feet.** The rig's own bind pose puts the feet at y = 0
    ///   and the neck at **1.7104 m** (`ow-tank-skel --guid 0x0d00000000000137`)
    ///   — the constant both rigs share. With `EYE_HEIGHT` 1.75 the eyes sit
    ///   4 cm above that bone, which is anatomically right and a check on the
    ///   constant rather than an input to it.
    /// * **No half turn.** The rig's forward is its **+Z**, like the character
    ///   model's: its weapon bone is at `(0, 1.7104, +0.40)` in the model's own
    ///   space. An earlier version of this function added π (from a note that the
    ///   *mesh* extends to −z), which is what pointed the arms backwards.
    ///
    /// The first-person view model is **camera-relative**, not a world prop:
    /// it is the camera's own transform moved down to the rig's feet, so the
    /// camera's position, yaw **and pitch** carry the arms and weapon with it.
    /// The client pitches the whole rig per bone through the first-person
    /// animation (`camera.FirstPersonAnimationWeight`, default 1.0); the arms
    /// model here has no legs, so rotating it about the eye is the equivalent
    /// whole-rig attitude, and no pose translation ever reaches the camera.
    ///
    /// * **No half turn.** The rig's forward is its **+Z**, like the character
    ///   model's: its weapon bone is at `(0, 1.7104, +0.40)` in the model's own
    ///   space. An earlier version of this function added π (from a note that the
    ///   *mesh* extends to −z), which is what pointed the arms backwards.
    // FIT-CAM-06: first-person pitch: whole-rig rotation fitted
    pub fn view_model_matrix(eye: Vec3, yaw: f32, pitch: f32) -> Mat4 {
        let rotation = Quat::from_rotation_y(yaw) * Quat::from_rotation_x(-pitch);
        let camera = Mat4::from_rotation_translation(rotation, eye);
        camera * Mat4::from_translation(Vec3::new(0.0, -EYE_HEIGHT, 0.0))
    }

    /// The texture slot for a GUID, adding it to the table if it is new.
    fn texture_slot(&mut self, guid: u64) -> u32 {
        if guid == 0 {
            return 0;
        }
        if let Some(i) = self.textures.iter().position(|t| t.guid == guid) {
            return i as u32;
        }
        self.textures.push(SceneTexture::white());
        (self.textures.len() - 1) as u32
    }

    /// A spread of viewpoints through the placements, for looking a map over.
    ///
    /// The camera stays at the map's eye height: many placement origins are on
    /// walls, ceilings or the ground under the room, and a tour that used the
    /// origin's own y axis spent half its views inside geometry. The x/z still
    /// comes from the placements, so the tour walks the map rather than
    /// clustering at the start.
    pub fn tour_points(&self, count: usize) -> Vec<Vec3> {
        if self.origins.is_empty() {
            return vec![self.eye];
        }
        let step = (self.origins.len() / count.max(1)).max(1);
        self.origins
            .iter()
            .step_by(step)
            .take(count)
            .map(|v| Vec3::new(v.x, self.eye.y, v.z))
            .collect()
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

/// Point the camera's look at a world position.
///
/// Shared by the map path and the `--no-map` path: both need it, and the hero's
/// placement turns the model to the camera's heading, so it has to run first.
fn aim(scene: &mut Scene, target: Vec3) {
    let d = target - scene.eye;
    if d.length_squared() > 1e-6 {
        scene.yaw = d.x.atan2(d.z);
        scene.pitch = (d.y / d.length()).asin();
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

/// Skinning coverage for one geometry load, reported collider-style.
#[derive(Default)]
struct SkinStats {
    models: usize,
    vertices: usize,
    unmapped_lanes: usize,
}

/// The joint/weight lanes for one vertex of a skinned mesh.
///
/// Raw indices go through the skeleton's Lookup table ([`ow_tank::skin`]):
/// a bound lane points at `base + bone` in the scene palette and an unbound
/// one (past the table, or through its `0xFFFF` hole) is zeroed, so the
/// shader's all-zero guard holds the vertex in bind pose instead of dragging
/// it onto a guessed bone. An unskinned vertex keeps the identity default.
///
/// The submesh `BoneIdOffset` term the reference adds before the Lookup is not
/// surfaced by the model reader, so raws map directly; `docs/formats/animation.md`
/// records that as the remaining gap.
fn skinned_lanes(
    skin: &ow_tank::skin::Skin,
    base: u32,
    mesh: &ow_tank::model::Mesh,
    vertex: usize,
    stats: &mut SkinStats,
) -> ([u8; 4], [u8; 4]) {
    let fallback = ([1u8, 0, 0, 0], [255u8, 0, 0, 0]);
    let (Some(raw), Some(weights)) = (
        mesh.bone_indices.get(vertex),
        mesh.blend_weights.get(vertex),
    ) else {
        return fallback;
    };
    if weights.iter().all(|w| *w == 0) {
        return fallback;
    }
    stats.vertices += 1;
    let mut joints = [0u8; 4];
    let mut out_weights = *weights;
    for lane in 0..4 {
        if skin.bone_of(raw[lane]).is_some() {
            joints[lane] = skin.joint_slot(base, raw[lane]);
        } else {
            joints[lane] = 0;
            out_weights[lane] = 0;
            stats.unmapped_lanes += 1;
        }
    }
    (joints, out_weights)
}

/// Grow the scene's joint palette with identity slots up to and including
/// `need`.
///
/// Map models have no posed palette of their own, and in bind pose every
/// skinning matrix is the identity, so they share one pool of identity slots
/// at palette base 1 rather than one palette each. The pool never exceeds the
/// 256 slots a `u8` joint index can name; lanes past it fall back per lane.
fn ensure_identity_pool(scene: &mut Scene, need: u32) {
    while (scene.joints.len() as u32) <= need.min(255) && scene.joints.len() < 256 {
        scene.joints.push(Mat4::IDENTITY.to_cols_array_2d());
    }
}

/// The highest palette slot a model's skinned meshes name, if any lane maps.
///
/// `None` is "no palette needed": no skeleton, `--no-skin`, or no mapped lane
/// anywhere — the model draws on the identity default like any other prop.
fn model_palette_need(
    skin: Option<&ow_tank::skin::Skin>,
    meshes: &[&ow_tank::model::Mesh],
) -> Option<u32> {
    let skin = skin?;
    let mut top = 0u32;
    let mut any = false;
    for mesh in meshes {
        if !mesh.skinned {
            continue;
        }
        for raw in &mesh.bone_indices {
            for lane in raw {
                if let Some(bone) = skin.bone_of(*lane) {
                    top = top.max(1 + bone as u32);
                    any = true;
                }
            }
        }
    }
    any.then_some(top)
}

/// A model's skeleton the way the hero path builds it: the file's own
/// hierarchy, or the cloth fake hierarchy where the model carries cloth.
fn model_skin(model_bytes: &[u8]) -> Option<ow_tank::skin::Skin> {
    let skeleton = ow_tank::skeleton::parse_model(model_bytes).ok()?;
    Some(match ow_tank::cloth::parse(model_bytes) {
        Ok(cloth) => ow_tank::skin::Skin::from_skeleton_with_cloth(&skeleton, &cloth),
        Err(_) => ow_tank::skin::Skin::from_skeleton(&skeleton),
    })
}

/// One submesh of a model loaded once and drawn once per placement group.
struct MapMeshRange {
    first_index: u32,
    index_count: u32,
    base_vertex: i32,
    material_id: u64,
}

/// A model's drawable submeshes, already appended to the scene's shared
/// vertex/index buffers.
struct MapModelGeometry {
    meshes: Vec<MapMeshRange>,
}

/// Append one model's finest LOD to the scene and return its submesh ranges.
///
/// Map models have no posed palette of their own; a skinned one shares the
/// scene's bind-pose identity pool, exactly as the previous chunk path did.
fn map_model_geometry(
    scene: &mut Scene,
    data: &GameData,
    model_guid: u64,
    no_skin: bool,
    skin_stats: &mut SkinStats,
) -> Result<Option<MapModelGeometry>, String> {
    let bytes = data
        .open_asset(model_guid)
        .map_err(|e| format!("model 0x{model_guid:016x}: {e}"))?;
    let loader = |g: u64| data.open_asset(g).ok();
    let model = ow_tank::model::parse(&bytes, &loader)
        .map_err(|e| format!("model 0x{model_guid:016x}: {e}"))?;
    let use_meshes = model.finest_level();
    let skin = if no_skin { None } else { model_skin(&bytes) };
    let joint_base = match model_palette_need(skin.as_ref(), &use_meshes) {
        Some(top) => {
            ensure_identity_pool(scene, top);
            skin_stats.models += 1;
            1u32
        }
        None => 0,
    };

    let mut meshes = Vec::new();
    for mesh in use_meshes {
        let first_index = scene.indices.len() as u32;
        let base = scene.vertices.len() as u32;
        for (i, position) in mesh.positions.iter().enumerate() {
            let normal = mesh.normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]);
            let uv = mesh
                .uv_sets
                .first()
                .and_then(|set| set.get(i))
                .copied()
                .unwrap_or([0.0, 0.0]);
            let (joints, weights) = if joint_base > 0 && mesh.skinned {
                skinned_lanes(
                    skin.as_ref().expect("pool implies a skeleton"),
                    joint_base,
                    mesh,
                    i,
                    skin_stats,
                )
            } else {
                ([1u8, 0, 0, 0], [255u8, 0, 0, 0])
            };
            scene.vertices.push(Vertex {
                position: *position,
                normal,
                uv,
                joints,
                weights,
            });
        }
        for index in &mesh.indices {
            scene.indices.push(*index);
        }
        let index_count = scene.indices.len() as u32 - first_index;
        if index_count == 0 {
            scene.vertices.truncate(base as usize);
            continue;
        }
        meshes.push(MapMeshRange {
            first_index,
            index_count,
            base_vertex: base as i32,
            material_id: model.materials.get(mesh.material).copied().unwrap_or(0),
        });
    }
    Ok((!meshes.is_empty()).then_some(MapModelGeometry { meshes }))
}

/// Is a decoded texture a flat fill (no colour information)?
///
/// The same rule the hero path applies: at least two colour channels must have
/// structure, or the surface stays on the placeholder instead of being painted
/// with a mask or a solid colour that reads as if it were textured.
fn texture_has_colour(texture: &SceneTexture) -> bool {
    let n = (texture.rgba.len() / 4).max(1) as f64;
    let structured = (0..3usize)
        .filter(|c| {
            let mean = texture
                .rgba
                .chunks_exact(4)
                .map(|p| p[*c] as f64)
                .sum::<f64>()
                / n;
            let variance = texture
                .rgba
                .chunks_exact(4)
                .map(|p| {
                    let d = p[*c] as f64 - mean;
                    d * d
                })
                .sum::<f64>()
                / n;
            variance.sqrt() >= 2.0
        })
        .count();
    structured >= 2
}

/// Totals for the map material pass, printed once instead of per look.
#[derive(Default)]
struct MaterialStats {
    looks: usize,
    materials: usize,
    unreadable: usize,
    no_colour: usize,
    flat: usize,
    ambiguous: usize,
}

/// Resolve a look's material ids to texture slots.
///
/// The join is the model's `MDLC` slot value against the look's `MaterialID`,
/// then the material's first sRGB texture. Every failure is a reported reason,
/// never a guessed neighbour.
fn look_texture_slots(
    scene: &mut Scene,
    data: &GameData,
    look: u64,
    report: &mut Vec<String>,
    stats: &mut MaterialStats,
) -> std::collections::HashMap<u64, u32> {
    let materials = match ow_tank::material::materials_of_look(data, look) {
        Ok(m) => m,
        Err(e) => {
            report.push(format!("look 0x{look:016x}: {e}"));
            return std::collections::HashMap::new();
        }
    };
    stats.looks += 1;
    stats.materials += materials.len();
    let mut seen: std::collections::HashMap<u64, u64> = std::collections::HashMap::new();
    let mut ambiguous: std::collections::HashSet<u64> = std::collections::HashSet::new();
    for m in &materials {
        match seen.get(&m.id) {
            None => {
                seen.insert(m.id, m.material);
            }
            Some(&prev) if prev != m.material => {
                ambiguous.insert(m.id);
            }
            _ => {}
        }
    }
    stats.ambiguous += ambiguous.len();
    let mut out = std::collections::HashMap::new();
    let mut unreadable = 0usize;
    let mut no_colour = 0usize;
    let mut flat = 0usize;
    for m in &materials {
        if ambiguous.contains(&m.id) || out.contains_key(&m.id) {
            continue;
        }
        if data.open_asset(m.material).is_err() {
            unreadable += 1;
            continue;
        }
        match ow_tank::material::base_texture_of_material(data, m.material) {
            Some(texture) => {
                let slot = texture_slot_loaded(scene, data, texture, report);
                if texture_has_colour(&scene.textures[slot as usize]) {
                    out.insert(m.id, slot);
                } else {
                    flat += 1;
                }
            }
            None => no_colour += 1,
        }
    }
    stats.unreadable += unreadable;
    stats.no_colour += no_colour;
    stats.flat += flat;
    report.push(format!(
        "look 0x{look:016x}: {} materials; {unreadable} unreadable, {no_colour} with no sRGB base colour, {flat} flat fills, {} textured id(s), {} ambiguous id(s)",
        materials.len(),
        out.len(),
        ambiguous.len(),
    ));
    out
}

/// Frame a map scene: the median placement origin, with `--at`, `--look-at`
/// and `--overview` exactly as the previous map path defined them.
fn frame_map(
    scene: &mut Scene,
    args: &[String],
    origins: &[Vec3],
    bounds: Option<([f32; 3], [f32; 3])>,
) {
    let (min, max) = bounds
        .map(|(a, b)| (Vec3::from_array(a), Vec3::from_array(b)))
        .unwrap_or_else(|| {
            let mut min = Vec3::splat(f32::MAX);
            let mut max = Vec3::splat(f32::MIN);
            for o in origins {
                min = min.min(*o);
                max = max.max(*o);
            }
            (min, max)
        });
    let size = max - min;
    let centroid = if origins.is_empty() {
        (min + max) * 0.5
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
    scene.eye = arg(args, "--at").and_then(parse_vec3).unwrap_or(centroid);
    scene.yaw = if size.x >= size.z {
        0.0
    } else {
        std::f32::consts::FRAC_PI_2
    };
    if let Some(target) = arg(args, "--look-at").and_then(parse_vec3) {
        aim(scene, target);
    }
    if args.iter().any(|a| a == "--overview") {
        let centre = (min + max) * 0.5;
        let height = size.length() * 0.9;
        scene.eye = Vec3::new(centre.x, max.y + height, centre.z);
        scene.yaw = 0.0;
        scene.pitch = -1.2;
        println!(
            "overview from ({:.0}, {:.0}, {:.0})",
            scene.eye.x, scene.eye.y, scene.eye.z
        );
    }
}

fn arg<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

/// Build the practice-range scene through the map resolver.
///
/// The chunk set is the resolver's own (`mapresolve::resolve_scene`): the
/// header selects a variation, the variation names a type-`0x002` scene map,
/// and that map asset stores some chunk references and synthesises the rest
/// from the client's table. No chunk is guessed by kind or proximity.
fn build_practice_range(
    scene: &mut Scene,
    data: &GameData,
    sources: &ow_tank::keytable::Sources,
    args: &[String],
) -> Result<(), String> {
    let kinds = sources
        .chunk_kinds()
        .map_err(|e| format!("the client's chunk-kind table: {e}"))?;
    let resolved = mapresolve::resolve_practice_range(data, &kinds).map_err(|e| e.to_string())?;
    let range = &resolved.range;
    println!(
        "practice range: header 0x{:016x}, mapName {:?}, base map {}, mode 0x{:016x} ({}), \
         variation {} of {} selected, scene map 0x{:016x}",
        range.header,
        range.map_name,
        range
            .base_map
            .map(|g| format!("0x{g:016x}"))
            .unwrap_or_else(|| "-".into()),
        range.variation.game_mode,
        range.mode_name,
        range.variation.index,
        range.variation_count,
        range.scene_map,
    );
    if range.matching_variations.len() > 1 {
        println!(
            "  note: {} variations name the same mode display name; header order picks {} \
             (the client's selector is still open)",
            range.matching_variations.len(),
            range.variation.index
        );
    }
    println!(
        "  chunks: {} requested, {} opened, {} unregistered, {} unopened; \
         {} placeables, {} model refs, {} entity refs",
        resolved.scene.requested.len(),
        resolved.scene.chunks.len(),
        resolved.scene.unregistered.len(),
        resolved.scene.unopened.len(),
        resolved.scene.placeables,
        resolved.scene.models,
        resolved.scene.entities,
    );
    if !resolved.scene.unregistered.is_empty() {
        return Err(format!(
            "the practice scene is incomplete: {} requested chunk(s) are not registered: {}",
            resolved.scene.unregistered.len(),
            resolved
                .scene
                .unregistered
                .iter()
                .map(|g| format!("0x{g:016x}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if !resolved.scene.unopened.is_empty() {
        return Err(format!(
            "the practice scene is incomplete: {} requested chunk(s) are registered but did not open: {}",
            resolved.scene.unopened.len(),
            resolved.scene
                .unopened
                .iter()
                .map(|g| format!("0x{g:016x}"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let mut chunks: Vec<(u64, u8, usize, usize)> = resolved
        .scene
        .chunks
        .iter()
        .map(|c| (c.guid, c.kind, c.size, c.placeables()))
        .collect();
    chunks.sort_unstable();
    for (guid, kind, size, placeables) in &chunks {
        println!("  chunk 0x{guid:016x} kind 0x{kind:02x}: {size} B, {placeables} placeables");
    }
    if let Some((min, max)) = resolved.scene.bounds {
        println!(
            "  bounds ({:.5}, {:.5}, {:.5}) .. ({:.5}, {:.5}, {:.5})",
            min[0], min[1], min[2], max[0], max[1], max[2]
        );
    }

    let mut placeables: Vec<&Placeable> = Vec::new();
    for chunk in &resolved.scene.chunks {
        if let Some(parsed) = &chunk.chunk {
            placeables.extend(parsed.placeables.iter());
        }
    }
    build_map_placeables(scene, data, &placeables, args)
}

/// Instance the model placeables from a resolved scene.
fn build_map_placeables(
    scene: &mut Scene,
    data: &GameData,
    placeables: &[&Placeable],
    args: &[String],
) -> Result<(), String> {
    let no_skin = args.iter().any(|a| a == "--no-skin");
    let mut order: Vec<(u64, u64)> = Vec::new();
    let mut groups: HashMap<(u64, u64), Vec<Instance>> = HashMap::new();
    let mut origins: Vec<Vec3> = Vec::new();
    for placeable in placeables {
        match &placeable.body {
            ow_tank::mapchunk::Body::ModelGroup(group) => {
                let Some(model) = placeable.model else {
                    continue;
                };
                if guid::type_of(model) != guid::types::MODEL {
                    continue;
                }
                for g in &group.groups {
                    let look = g.look.unwrap_or(0);
                    let key = (model, look);
                    let instances = match groups.entry(key) {
                        std::collections::hash_map::Entry::Vacant(e) => {
                            order.push(key);
                            e.insert(Vec::new())
                        }
                        std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
                    };
                    for entry in &g.entries {
                        instances.push(Instance {
                            model: transform_matrix(entry.translation, entry.scale, entry.rotation)
                                .to_cols_array_2d(),
                        });
                        origins.push(Vec3::from_array(entry.translation));
                    }
                }
            }
            ow_tank::mapchunk::Body::Model(_) | ow_tank::mapchunk::Body::SingleModel(_) => {
                let Some(model) = placeable.model else {
                    continue;
                };
                if guid::type_of(model) != guid::types::MODEL {
                    continue;
                }
                let look = placeable.look.unwrap_or(0);
                let key = (model, look);
                let instances = match groups.entry(key) {
                    std::collections::hash_map::Entry::Vacant(e) => {
                        order.push(key);
                        e.insert(Vec::new())
                    }
                    std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
                };
                instances.push(Instance {
                    model: placement_matrix(placeable).to_cols_array_2d(),
                });
                origins.push(Vec3::from_array(placeable.translation));
            }
            _ => {}
        }
    }
    if order.is_empty() {
        return Err("the resolved scene has no type-0x00C model placeable".into());
    }

    let mut geometry: HashMap<u64, MapModelGeometry> = HashMap::new();
    let mut skin_stats = SkinStats::default();
    let mut model_failures = 0usize;
    for (model, _) in &order {
        if geometry.contains_key(model) {
            continue;
        }
        match map_model_geometry(scene, data, *model, no_skin, &mut skin_stats) {
            Ok(Some(geometry_of_model)) => {
                geometry.insert(*model, geometry_of_model);
            }
            Ok(None) => {
                model_failures += 1;
            }
            Err(e) => {
                eprintln!("{e}");
                model_failures += 1;
            }
        }
    }

    let mut look_slots: HashMap<u64, std::collections::HashMap<u64, u32>> = HashMap::new();
    let mut report: Vec<String> = Vec::new();
    let mut material_stats = MaterialStats::default();
    let mut textured_submeshes = 0usize;
    let mut placeholder_submeshes = 0usize;
    for key in &order {
        let (model, look) = *key;
        let Some(model_geometry) = geometry.get(&model) else {
            continue;
        };
        let instances = groups.get(key).expect("the group was inserted above");
        let instance_start = scene.instances.len() as u32;
        scene.instances.extend_from_slice(instances);
        scene.placements += instances.len();
        look_slots.entry(look).or_insert_with(|| {
            look_texture_slots(scene, data, look, &mut report, &mut material_stats)
        });
        let slots = &look_slots[&look];
        for mesh in &model_geometry.meshes {
            let texture = slots.get(&mesh.material_id).copied().unwrap_or(0);
            if texture == 0 {
                placeholder_submeshes += 1;
            } else {
                textured_submeshes += 1;
            }
            scene.draws.push(Draw {
                first_index: mesh.first_index,
                index_count: mesh.index_count,
                base_vertex: mesh.base_vertex,
                instance_start,
                instance_count: instances.len() as u32,
                texture,
                model,
            });
        }
    }

    scene.models_loaded = geometry.len();
    scene.origins = origins;
    if model_failures > 0 {
        eprintln!("{model_failures} model(s) could not be read");
    }
    if args.iter().any(|a| a == "--verbose") {
        for line in &report {
            println!("{line}");
        }
    }
    println!(
        "  materials: {} look(s), {} material entr(ies); {} unreadable, {} with no sRGB base colour, {} flat fill(s), {} ambiguous id(s); {} texture(s) decoded, {} submesh draw(s) textured, {placeholder_submeshes} on the white placeholder",
        material_stats.looks,
        material_stats.materials,
        material_stats.unreadable,
        material_stats.no_colour,
        material_stats.flat,
        material_stats.ambiguous,
        scene.textures.len().saturating_sub(1),
        textured_submeshes,
    );
    if skin_stats.models > 0 || skin_stats.unmapped_lanes > 0 {
        eprintln!(
            "skin: {} model(s) share the bind-pose identity pool, {} skinned vertices, {} unmapped lane(s) held at identity",
            skin_stats.models, skin_stats.vertices, skin_stats.unmapped_lanes
        );
    }
    println!(
        "  geometry: {} distinct models, {} placements, {} draws",
        scene.models_loaded,
        scene.placements,
        scene.draws.len(),
    );
    let mut bounds_min = Vec3::splat(f32::MAX);
    let mut bounds_max = Vec3::splat(f32::MIN);
    for origin in &scene.origins {
        bounds_min = bounds_min.min(*origin);
        bounds_max = bounds_max.max(*origin);
    }
    if !scene.origins.is_empty() {
        let reach = Vec3::splat(8.0);
        bounds_min -= reach;
        bounds_max += reach;
        println!(
            "  model-instance origins ({:.5}, {:.5}, {:.5}) .. ({:.5}, {:.5}, {:.5})",
            bounds_min.x + 8.0,
            bounds_min.y + 8.0,
            bounds_min.z + 8.0,
            bounds_max.x - 8.0,
            bounds_max.y - 8.0,
            bounds_max.z - 8.0,
        );
    }

    frame_map(
        scene,
        args,
        &scene.origins.clone(),
        Some((bounds_min.to_array(), bounds_max.to_array())),
    );
    Ok(())
}

pub fn build(args: &[String]) -> Result<Scene, String> {
    let flag = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };

    let root = ow_casc::install::detect_root()
        .ok_or("no Overwatch install found; set OW_INSTALL to the folder containing .build.info")?;
    let sources = ow_tank::keytable::Sources::discover(std::slice::from_ref(&root));
    let install_root = root.display().to_string();
    let data = GameData::open(&install_root, &ManifestSelector::default(), &sources)
        .map_err(|e| e.to_string())?;

    let mut scene = Scene::empty();
    if args.iter().any(|a| a == "--uv-check") {
        scene.placeholder = SceneTexture::uv_checker();
        scene.textures[0] = SceneTexture::uv_checker();
    }
    let resolves = |g: u64| data.registry().get(g).is_some();

    // `--clip <guid>`: load an animation clip and parse its container, so the
    // harness carries real clip data (frames, rate, duration, the per-rig track
    // list) instead of nothing. The **pose payload is not decoded** — the record
    // layout is rig-dependent and the client's per-kind handlers are the place
    // that reads it (`ow-tank::clip` carries the full account) — so this reports
    // what it read and the scene keeps it for the pass that will evaluate it.
    if let Some(g) = flag("--clip") {
        let guid = u64::from_str_radix(g.trim_start_matches("0x"), 16).unwrap_or(0);
        match data.open_asset(guid) {
            Ok(bytes) => match ow_tank::clip::Clip::parse(&bytes) {
                Ok(c) => {
                    println!(
                        "clip 0x{guid:016x}: {} B, content 0x{:08x}, {} frames @ {} fps, {:.3} s, \
                         id 0x{:08x}, slot {}, first section {} words, body {} B — payload not decoded",
                        bytes.len(),
                        c.header.family,
                        c.header.frames,
                        c.header.fps,
                        c.header.duration,
                        c.header.id,
                        c.header.slot,
                        c.first_section.len(),
                        c.body.len()
                    );
                    scene.clip = Some(c);
                    scene.clip_guid = guid;
                }
                Err(e) => println!("clip 0x{guid:016x}: {e}"),
            },
            Err(e) => println!("clip 0x{guid:016x}: {e}"),
        }
    }

    // `--list-maps`: every readable map header and its variation modes. This is
    // the data-driven answer to "which map is which"; it does not select one.
    if args.iter().any(|a| a == "--list-maps") {
        let maps = mapresolve::list_maps(&data);
        println!("{} readable type-0x09F map headers", maps.len());
        for map in &maps {
            println!(
                "0x{:016x}  {}  ({} variation(s))",
                map.header,
                map.map_name,
                map.variations.len()
            );
            for v in &map.variations {
                println!(
                    "    [{:>2}] mode 0x{:016x}  {:<32}  scene 0x{:016x}",
                    v.index,
                    v.game_mode,
                    v.mode_name
                        .as_deref()
                        .unwrap_or("<no readable display name>"),
                    v.scene_map,
                );
            }
        }
        std::process::exit(0);
    }

    // Which world to build. The default is the install's practice range,
    // selected through the map header (no chunk seed, no synthetic course). The
    // chunk flags remain explicit diagnostics; `--no-map` is the explicit
    // stand-in course for viewer work.
    let one_of = |names: &[&str]| args.iter().any(|a| names.contains(&a.as_str()));
    let legacy_map = one_of(&[
        "--chunks",
        "--chunks-from",
        "--chunk-file",
        "--cluster",
        "--clusters",
        "--list",
    ]);
    let no_map = args.iter().any(|a| a == "--no-map") || args.iter().any(|a| a == "--hero-only");
    let practice = args.iter().any(|a| a == "--practice-range") || (!no_map && !legacy_map);
    if no_map {
        build_sandbox(&mut scene)?;
        // Aim before placing the hero: the placement turns the model to the
        // camera's heading, so the look has to be known first. (This used to be
        // applied only on the map path, which silently ignored `--look-at` here.)
        if let Some(target) = flag("--look-at").and_then(|v| parse_vec3(&v)) {
            aim(&mut scene, target);
        }
        if let Some(hero_index) = flag("--hero").and_then(|v| v.parse::<u32>().ok()) {
            let mut report = Vec::new();
            let view_model = args.iter().any(|a| a == "--view-model");
            let at = flag("--hero-at")
                .and_then(|v| parse_vec3(&v))
                .or(if view_model {
                    None
                } else {
                    Some(Vec3::new(0.0, 0.0, -8.0))
                });
            match place_hero(&mut scene, &data, hero_index, at, false, args, &mut report) {
                Ok(()) => {
                    for line in report {
                        println!("{line}");
                    }
                }
                Err(e) => eprintln!("hero {hero_index}: {e}"),
            }
        }
        return Ok(scene);
    }

    if practice {
        build_practice_range(&mut scene, &data, &sources, args)?;
        if let Some(hero_index) = flag("--hero").and_then(|v| v.parse::<u32>().ok()) {
            let mut report = Vec::new();
            let at = flag("--hero-at").and_then(|v| parse_vec3(&v));
            match place_hero(&mut scene, &data, hero_index, at, false, args, &mut report) {
                Ok(()) => {
                    for line in report {
                        println!("{line}");
                    }
                }
                Err(e) => eprintln!("hero {hero_index}: {e}"),
            }
        }
        return Ok(scene);
    }

    if args.iter().any(|a| a == "--list") {
        let count: usize = flag("--count").and_then(|v| v.parse().ok()).unwrap_or(40);
        let guids = data.guids_of_type(0x0BC);
        println!("{} map chunk assets; the first {count}:", guids.len());
        for guid in guids.iter().take(count) {
            let Ok(bytes) = data.open_asset(*guid) else {
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

    // `--hero-only` draws the hero model and nothing else: a map's first chunks
    // are thousands of props, and a hero placed at the origin is buried inside
    // them. This is the mode for looking at a model.
    let hero_only = args.iter().any(|a| a == "--hero-only");

    // Which chunks to draw.
    let chunk_guids: Vec<u64> = if hero_only {
        Vec::new()
    } else if let Some(path) = flag("--chunks-from") {
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
        let mut skin_stats = SkinStats::default();
        let no_skin = args.iter().any(|a| a == "--no-skin");
        let loader = |g: u64| data.open_asset(g).ok();
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
            if let std::collections::hash_map::Entry::Vacant(e) = geometry.entry(model_guid) {
                let Ok(model_bytes) = data.open_asset(model_guid) else {
                    model_failures += 1;
                    continue;
                };
                let Ok(model) = ow_tank::model::parse(&model_bytes, &loader) else {
                    model_failures += 1;
                    continue;
                };
                let vertex_start = scene.vertices.len() as u32;
                let index_start = scene.indices.len() as u32;
                let use_meshes = model.finest_level();
                // A skinned map model shares the scene's identity pool at base
                // 1, which is exact in bind pose and keeps its joint indices
                // valid; anything else draws on the identity default as before.
                let skin = if no_skin {
                    None
                } else {
                    model_skin(&model_bytes)
                };
                let joint_base = match model_palette_need(skin.as_ref(), &use_meshes) {
                    Some(top) => {
                        ensure_identity_pool(&mut scene, top);
                        skin_stats.models += 1;
                        1u32
                    }
                    None => 0,
                };
                for mesh in use_meshes {
                    let base = scene.vertices.len() as u32 - vertex_start;
                    for (i, position) in mesh.positions.iter().enumerate() {
                        let normal = mesh.normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]);
                        let uv = mesh
                            .uv_sets
                            .first()
                            .and_then(|set| set.get(i))
                            .copied()
                            .unwrap_or([0.0, 0.0]);
                        let (joints, weights) = if joint_base > 0 && mesh.skinned {
                            skinned_lanes(
                                skin.as_ref().expect("pool implies a skeleton"),
                                joint_base,
                                mesh,
                                i,
                                &mut skin_stats,
                            )
                        } else {
                            ([1u8, 0, 0, 0], [255u8, 0, 0, 0])
                        };
                        scene.vertices.push(Vertex {
                            position: *position,
                            normal,
                            uv,
                            joints,
                            weights,
                        });
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
                e.insert(Loaded {
                    vertex_start,
                    index_start,
                    index_count,
                    instances: Vec::new(),
                });
            }
            let matrix = placement_matrix(placeable);
            if let Some(entry) = geometry.get_mut(&model_guid) {
                entry.instances.push(Instance {
                    model: matrix.to_cols_array_2d(),
                });
                scene.placements += 1;
            }
            let origin = Vec3::from_array(placeable.translation);
            bounds_min = bounds_min.min(origin - Vec3::splat(8.0));
            bounds_max = bounds_max.max(origin + Vec3::splat(8.0));
        }
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
                texture: 0,
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
        scene.eye = flag("--at")
            .and_then(|v| parse_vec3(&v))
            .unwrap_or(centroid);
        scene.yaw = if size.x >= size.z {
            0.0
        } else {
            std::f32::consts::FRAC_PI_2
        };
        println!(
            "{} distinct models, {} placements; eye ({:.0}, {:.0}, {:.0})",
            scene.models_loaded, scene.placements, scene.eye.x, scene.eye.y, scene.eye.z
        );
        if skin_stats.models > 0 || skin_stats.unmapped_lanes > 0 {
            eprintln!(
                "skin: {} model(s) share the bind-pose identity pool, {} skinned vertices, {} unmapped lane(s) held at identity",
                skin_stats.models, skin_stats.vertices, skin_stats.unmapped_lanes
            );
        }
        return Ok(scene);
    } else if let Some(list) = flag("--chunks") {
        list.split(',')
            .map(|s| {
                u64::from_str_radix(s.trim().trim_start_matches("0x"), 16)
                    .map_err(|_| format!("{s:?} is not a GUID"))
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        return Err(
            "no map selected: the default is --practice-range; name an explicit diagnostic \
             with --chunks, --chunks-from, --chunk-file or --cluster"
                .into(),
        );
    };

    // A set of chunks gathered from a running game is not a map: it holds
    // whatever that session touched. Chunks belonging to the same map have
    // their contents in the same place, so grouping by proximity recovers the
    // maps without the map-to-chunk link, which is not in the data this
    // program can read.
    let chunk_guids = if hero_only {
        Vec::new()
    } else if args.iter().any(|a| a == "--clusters") || flag("--cluster").is_some() {
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
    let mut skin_stats = SkinStats::default();
    let no_skin = args.iter().any(|a| a == "--no-skin");

    let loader = |g: u64| data.open_asset(g).ok();

    for guid in &chunk_guids {
        let Ok(bytes) = data.open_asset(*guid) else {
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
                let Ok(model_bytes) = data.open_asset(model_guid) else {
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
                let base_meshes = model.finest_level();
                // As in the `--chunk-file` path: a skinned model shares the
                // identity pool at base 1, exact in bind pose.
                let skin = if no_skin {
                    None
                } else {
                    model_skin(&model_bytes)
                };
                let joint_base = match model_palette_need(skin.as_ref(), &base_meshes) {
                    Some(top) => {
                        ensure_identity_pool(&mut scene, top);
                        skin_stats.models += 1;
                        1u32
                    }
                    None => 0,
                };
                for mesh in base_meshes {
                    let base = scene.vertices.len() as u32 - vertex_start;
                    for (i, position) in mesh.positions.iter().enumerate() {
                        let normal = mesh.normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]);
                        let uv = mesh
                            .uv_sets
                            .first()
                            .and_then(|set| set.get(i))
                            .copied()
                            .unwrap_or([0.0, 0.0]);
                        let (joints, weights) = if joint_base > 0 && mesh.skinned {
                            skinned_lanes(
                                skin.as_ref().expect("pool implies a skeleton"),
                                joint_base,
                                mesh,
                                i,
                                &mut skin_stats,
                            )
                        } else {
                            ([1u8, 0, 0, 0], [255u8, 0, 0, 0])
                        };
                        scene.vertices.push(Vertex {
                            position: *position,
                            normal,
                            uv,
                            joints,
                            weights,
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
            texture: 0,
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
    if skin_stats.models > 0 || skin_stats.unmapped_lanes > 0 {
        eprintln!(
            "skin: {} model(s) share the bind-pose identity pool, {} skinned vertices, {} unmapped lane(s) held at identity",
            skin_stats.models, skin_stats.vertices, skin_stats.unmapped_lanes
        );
    }
    if entity_placements > 0 {
        println!("{entity_placements} entity placements drawn as their model");
    }
    if scene.draws.is_empty() && !hero_only {
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
        None if hero_only => Vec3::new(2.4, 1.6, 2.4),
        None => centroid,
    };
    scene.yaw = if size.x >= size.z {
        0.0
    } else {
        std::f32::consts::FRAC_PI_2
    };
    // Look at a point: the honest way to aim a camera at a model whose position
    // the command line just named.
    if let Some(target) = flag("--look-at").and_then(|v| parse_vec3(&v)) {
        aim(&mut scene, target);
    }
    // An overhead view, for seeing a map's layout rather than standing in it.
    if args.iter().any(|a| a == "--overview") {
        let centre = (bounds_min + bounds_max) * 0.5;
        let height = size.length() * 0.9;
        scene.eye = Vec3::new(centre.x, bounds_max.y + height, centre.z);
        scene.yaw = 0.0;
        scene.pitch = -1.2;
        println!(
            "overview from ({:.0}, {:.0}, {:.0})",
            scene.eye.x, scene.eye.y, scene.eye.z
        );
    }
    println!(
        "{} distinct models, {} placements, {} draws",
        scene.models_loaded,
        scene.placements,
        scene.draws.len()
    );
    if scene.placements > 0 {
        println!(
            "bounds ({:.0}, {:.0}, {:.0}) .. ({:.0}, {:.0}, {:.0}); eye ({:.0}, {:.0}, {:.0})",
            bounds_min.x,
            bounds_min.y,
            bounds_min.z,
            bounds_max.x,
            bounds_max.y,
            bounds_max.z,
            scene.eye.x,
            scene.eye.y,
            scene.eye.z
        );
    } else {
        println!(
            "no map geometry; eye ({:.1}, {:.1}, {:.1})",
            scene.eye.x, scene.eye.y, scene.eye.z
        );
    }

    // A hero model, when one was asked for. It is placed after the map's eye is
    // known so it can stand in front of the camera by default.
    if let Some(hero_index) = flag("--hero").and_then(|v| v.parse::<u32>().ok()) {
        let mut report = Vec::new();
        // `--hero-only` stands a *character* model on the origin so it can be
        // framed; a view model is placed by the camera instead, so it keeps the
        // first-person placement unless `--hero-at` names a spot.
        let at = flag("--hero-at").and_then(|v| parse_vec3(&v)).or(
            if hero_only && !args.iter().any(|a| a == "--view-model") {
                Some(Vec3::ZERO)
            } else {
                None
            },
        );
        match place_hero(
            &mut scene,
            &data,
            hero_index,
            at,
            hero_only,
            args,
            &mut report,
        ) {
            Ok(()) => {
                for line in report {
                    println!("{line}");
                }
            }
            Err(e) => eprintln!("hero {hero_index}: {e}"),
        }
    }
    Ok(scene)
}

/// Decode a texture into `scene.textures`, returning its slot. The slot's GUID
/// records the load even when the decode fails, so a retry does not re-read the
/// install.
fn texture_slot_loaded(
    scene: &mut Scene,
    data: &GameData,
    texture_guid: u64,
    report: &mut Vec<String>,
) -> u32 {
    let slot = scene.texture_slot(texture_guid);
    if texture_guid == 0 || scene.textures[slot as usize].guid != 0 {
        return slot;
    }
    let loader = |g: u64| data.open_asset(g).ok();
    match data
        .open_asset(texture_guid)
        .map_err(|e| e.to_string())
        .and_then(|b| {
            ow_tank::texture::parse_asset(&b, texture_guid, &loader).map_err(|e| e.to_string())
        }) {
        Ok(t) => {
            // The mean colour of the decode: a white or blank texture would show
            // up here, so "this submesh is painted with a real texture" is a
            // number rather than an impression.
            let pixels = (t.rgba.len() / 4).max(1) as u64;
            let mean_of =
                |c: usize| t.rgba.chunks_exact(4).map(|px| px[c] as u64).sum::<u64>() / pixels;
            let sd_of = |c: usize, mean: u64| {
                let variance = t
                    .rgba
                    .chunks_exact(4)
                    .map(|px| {
                        let d = px[c] as f64 - mean as f64;
                        d * d
                    })
                    .sum::<f64>()
                    / pixels as f64;
                variance.sqrt()
            };
            let mean = [mean_of(0), mean_of(1), mean_of(2)];
            let sd = [sd_of(0, mean[0]), sd_of(1, mean[1]), sd_of(2, mean[2])];
            let flat = sd.iter().cloned().fold(0.0f64, f64::max) < 2.0;
            report.push(format!(
                "texture 0x{texture_guid:016x}: {}x{} {:?} ({} mip{}), mean RGB ({}, {}, {}), sd ({:.1}, {:.1}, {:.1}){}",
                t.width,
                t.height,
                t.format,
                t.mip_count,
                if t.mip_count == 1 { "" } else { "s" },
                mean[0],
                mean[1],
                mean[2],
                sd[0],
                sd[1],
                sd[2],
                if flat { " — a flat fill, which carries no colour" } else { "" }
            ));
            scene.textures[slot as usize] = SceneTexture {
                guid: texture_guid,
                width: t.width,
                height: t.height,
                rgba: t.rgba,
                format: format!("{:?}", t.format),
            };
        }
        Err(e) => {
            report.push(format!("texture 0x{texture_guid:016x}: {e}"));
            // Keep the GUID so the failure is remembered, and give the slot a
            // readable stand-in rather than a stretched neighbour.
            scene.textures[slot as usize] = SceneTexture {
                guid: texture_guid,
                format: "unreadable".into(),
                ..SceneTexture::white()
            };
        }
    }
    slot
}

/// The test course's boxes, as `(min, max)`, around a spawn.
///
/// `--no-map` draws these as the test course. The course runs along +z with
/// one pillar behind the spawn.
pub fn sandbox_boxes(spawn: [f32; 3], ground: f32) -> Vec<([f32; 3], [f32; 3])> {
    let (x, z) = (spawn[0], spawn[2]);
    let at = |dx: f32, dz: f32, w: f32, h: f32, d: f32| {
        (
            [x + dx - w / 2.0, ground, z + dz - d / 2.0],
            [x + dx + w / 2.0, ground + h, z + dz + d / 2.0],
        )
    };
    vec![
        // 0.5 m lip: walk up it.
        at(0.0, 6.0, 4.0, 0.5, 4.0),
        // 0.9 m block: needs the jump (apex ~0.98).
        at(0.0, 12.0, 2.0, 0.9, 2.0),
        // A wall to walk into.
        at(0.0, 18.0, 6.0, 2.5, 0.5),
        // A pillar behind the spawn.
        at(0.0, -6.0, 0.8, 3.0, 0.8),
    ]
}

/// Draw a box: four sides, a top and a bottom, normals outward and UVs planar
/// per face, so the placeholder texture reads (it is a checker with a repeat
/// address mode, which is what makes a face's UV scale visible).
fn push_box(scene: &mut Scene, min: [f32; 3], max: [f32; 3], texture: u32) {
    let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
        // (normal, corners counter-clockwise seen from outside)
        (
            [0.0, 0.0, 1.0],
            [
                [min[0], min[1], max[2]],
                [max[0], min[1], max[2]],
                [max[0], max[1], max[2]],
                [min[0], max[1], max[2]],
            ],
        ),
        (
            [0.0, 0.0, -1.0],
            [
                [max[0], min[1], min[2]],
                [min[0], min[1], min[2]],
                [min[0], max[1], min[2]],
                [max[0], max[1], min[2]],
            ],
        ),
        (
            [1.0, 0.0, 0.0],
            [
                [max[0], min[1], max[2]],
                [max[0], min[1], min[2]],
                [max[0], max[1], min[2]],
                [max[0], max[1], max[2]],
            ],
        ),
        (
            [-1.0, 0.0, 0.0],
            [
                [min[0], min[1], min[2]],
                [min[0], min[1], max[2]],
                [min[0], max[1], max[2]],
                [min[0], max[1], min[2]],
            ],
        ),
        (
            [0.0, 1.0, 0.0],
            [
                [min[0], max[1], max[2]],
                [max[0], max[1], max[2]],
                [max[0], max[1], min[2]],
                [min[0], max[1], min[2]],
            ],
        ),
        (
            [0.0, -1.0, 0.0],
            [
                [min[0], min[1], min[2]],
                [max[0], min[1], min[2]],
                [max[0], min[1], max[2]],
                [min[0], min[1], max[2]],
            ],
        ),
    ];
    let first_index = scene.indices.len() as u32;
    let base = scene.vertices.len() as u32;
    for (normal, corners) in faces {
        let fbase = scene.vertices.len() as u32;
        // UVs from the face's own extent, so a 1 m face shows one tile.
        let span = (0..3)
            .map(|a| (max[a] - min[a]).abs())
            .fold(0.0f32, f32::max)
            .max(1.0);
        for (i, corner) in corners.iter().enumerate() {
            let uv = match i {
                0 => [0.0, 0.0],
                1 => [1.0, 0.0],
                2 => [1.0, 1.0],
                _ => [0.0, 1.0],
            };
            // Identity slot 0 (see `build_sandbox`): the sandbox is the one
            // scene whose hero palette replaces the placeholder, so the
            // base-1 default would ride the hero's bone 0.
            scene.vertices.push(Vertex {
                position: *corner,
                normal,
                uv: [uv[0] * span, uv[1] * span],
                joints: [0, 0, 0, 0],
                ..Vertex::default()
            });
        }
        scene.indices.push(fbase);
        scene.indices.push(fbase + 1);
        scene.indices.push(fbase + 2);
        scene.indices.push(fbase);
        scene.indices.push(fbase + 2);
        scene.indices.push(fbase + 3);
    }
    let index_count = scene.indices.len() as u32 - first_index;
    scene.instances.push(Instance {
        model: Mat4::IDENTITY.to_cols_array_2d(),
    });
    scene.draws.push(Draw {
        first_index,
        index_count,
        base_vertex: base as i32,
        instance_start: (scene.instances.len() - 1) as u32,
        instance_count: 1,
        texture,
        model: 0,
    });
}

/// Build the stand-in course: a ground plane and [`sandbox_boxes`], drawn with
/// the placeholder texture.
///
/// This is `--no-map`: the sandbox the operator tests movement in, without
/// loading 2,666 map models first. The plane is 80 m across so a walk or a jump
/// cannot reach its edge in a normal session.
fn build_sandbox(scene: &mut Scene) -> Result<(), String> {
    let ground = 0.0f32;
    let half = 40.0f32;
    let first_index = scene.indices.len() as u32;
    let base = scene.vertices.len() as u32;
    for corner in [
        [-half, ground, -half],
        [half, ground, -half],
        [half, ground, half],
        [-half, ground, half],
    ] {
        scene.vertices.push(Vertex {
            position: corner,
            normal: [0.0, 1.0, 0.0],
            // One checker tile per metre, so the plane shows scale.
            uv: [corner[0] * 0.25, corner[2] * 0.25],
            // Identity slot 0, not the `[1, 0, 0, 0]` map-prop default: the
            // sandbox course is the one scene where a placed hero *replaces*
            // the palette (nothing grew an identity pool first), so a
            // base-1 vertex would be skinned by the hero's bone 0 once
            // `--anim` drives it.
            joints: [0, 0, 0, 0],
            ..Vertex::default()
        });
    }
    // Wound so the front face is *up*: the other order points the normal down
    // and back-face culling hides the whole floor (which is how this looked
    // first: a course of boxes floating over nothing).
    scene
        .indices
        .extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
    scene.instances.push(Instance {
        model: Mat4::IDENTITY.to_cols_array_2d(),
    });
    scene.draws.push(Draw {
        first_index,
        index_count: 6,
        base_vertex: base as i32,
        instance_start: (scene.instances.len() - 1) as u32,
        instance_count: 1,
        texture: 0,
        model: 0,
    });
    for (min, max) in sandbox_boxes([0.0, 0.0, 0.0], ground) {
        push_box(scene, min, max, 0);
    }
    // Stand where the collision sandbox expects the player: on the plane, at
    // the course's start, looking down it (+z). The spawn is *stated* rather
    // than derived — a vertex median over a plane and four boxes lands on a box
    // top, which put the player 4.2 m up before this existed.
    scene.sandbox_spawn = Some([0.0, ground, 0.0]);
    scene.eye = Vec3::new(0.0, ground + EYE_HEIGHT, 0.0);
    scene.yaw = 0.0;
    scene.pitch = -0.05;
    println!(
        "sandbox: an 80x80 m ground plane at y = {ground} and {} box(es); no map geometry",
        scene.draws.len() - 1
    );
    Ok(())
}

/// Append one model's geometry to the scene, one draw per submesh, each with the
/// texture its *material* resolves to.
///
/// This is the hero path: a model placed by the viewer rather than by a chunk.
/// A submesh's material is the model's own material id (`MDLC`), looked up in
/// the look the caller passes — the chain `docs/formats/materials.md` closes.
/// Materials whose assets are not decryptable on this install (most of them:
/// type `0x008` is 16 of 2000 readable, and the hero's biggest submesh is one
/// of the blocked ones) leave their submesh on the white placeholder, and the
/// report says how many.
fn add_model(
    scene: &mut Scene,
    data: &GameData,
    model_guid: u64,
    matrix: Mat4,
    look: u64,
    no_skin: bool,
    skin: Option<&ow_tank::skin::Skin>,
    joint_base: u32,
    rigid: bool,
    report: &mut Vec<String>,
) -> Result<usize, String> {
    let bytes = data
        .open_asset(model_guid)
        .map_err(|e| format!("model 0x{model_guid:016x}: {e}"))?;
    let loader = |g: u64| data.open_asset(g).ok();
    let model = ow_tank::model::parse(&bytes, &loader)
        .map_err(|e| format!("model 0x{model_guid:016x}: {e}"))?;

    // The look's id → texture map.
    let look_materials = if look != 0 {
        match ow_tank::material::materials_of_look(data, look) {
            Ok(m) => m,
            Err(e) => {
                report.push(format!("look 0x{look:016x}: {e}"));
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };
    // The MDLC slot value meets the look on the material id; an id the look
    // names twice on *different* materials is ambiguous and stays on the
    // placeholder rather than picking one (`material_of_id` says the same).
    let mut ambiguous_ids: Vec<u64> = Vec::new();
    {
        let mut seen: std::collections::HashMap<u64, u64> = std::collections::HashMap::new();
        for m in &look_materials {
            match seen.get(&m.id) {
                None => {
                    seen.insert(m.id, m.material);
                }
                Some(&prev) if prev != m.material && !ambiguous_ids.contains(&m.id) => {
                    ambiguous_ids.push(m.id);
                }
                _ => {}
            }
        }
    }
    for id in &ambiguous_ids {
        report.push(format!(
            "look 0x{look:016x}: id 0x{id:016x} names different materials — ambiguous, left on the placeholder"
        ));
    }
    let mut slot_by_id: std::collections::HashMap<u64, u32> = std::collections::HashMap::new();
    let mut seen_material: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let mut unreadable_materials = 0usize;
    let mut data_map_materials = 0usize;
    let mut flat_colour = 0usize;
    for m in &look_materials {
        if ambiguous_ids.contains(&m.id) || !seen_material.insert(m.material) {
            continue;
        }
        if data.open_asset(m.material).is_err() {
            // The material asset itself is not decryptable on this install
            // (`key … is not present`), which is where most of the hero's
            // surfaces stop — the body included.
            unreadable_materials += 1;
            continue;
        }
        // Base colour = the material's first **sRGB** texture: colour data is
        // stored sRGB and data maps are not. A material whose list has no sRGB
        // entry paints nothing rather than painting a normal map as colour.
        match ow_tank::material::base_texture_of_material(data, m.material) {
            Some(t) => {
                let slot = texture_slot_loaded(scene, data, t, report);
                // A **flat fill** carries no colour information: painting it
                // makes a black or grey patch that reads as "this part is
                // painted" when the honest statement is "this material's colour
                // is not readable here". Measured on Tracer: the forearm plates'
                // only texture is a solid black 128x128 BC1 (sd 0.0), and the
                // patch it painted is why this rule exists.
                // A *colour* map has structure in at least two channels. A
                // single-channel one is a mask: Reaper's only readable texture
                // is 512x512 with sd (8.2, 0.0, 0.0), and painting it would have
                // made him black with a red glow rather than textured.
                let colour_channels = {
                    let t = &scene.textures[slot as usize];
                    let n = (t.rgba.len() / 4).max(1) as f64;
                    (0..3usize)
                        .filter(|c| {
                            let mean =
                                t.rgba.chunks_exact(4).map(|p| p[*c] as f64).sum::<f64>() / n;
                            let variance = t
                                .rgba
                                .chunks_exact(4)
                                .map(|p| {
                                    let d = p[*c] as f64 - mean;
                                    d * d
                                })
                                .sum::<f64>()
                                / n;
                            variance.sqrt() >= 2.0
                        })
                        .count()
                };
                let is_flat = colour_channels < 2;
                if is_flat {
                    flat_colour += 1;
                } else {
                    slot_by_id.insert(m.id, slot);
                }
            }
            None => {
                data_map_materials += 1;
                let maps = ow_tank::material::textures_with_format(data, m.material)
                    .map(|v| v.len())
                    .unwrap_or(0);
                report.push(format!(
                    "  material 0x{:016x} (id 0x{:016x}): {maps} texture(s), none stored as sRGB — data maps, not base colour",
                    m.material, m.id
                ));
            }
        }
    }

    let instance = scene.instances.len();
    scene.instances.push(Instance {
        model: matrix.to_cols_array_2d(),
    });

    let vertex_base = scene.vertices.len() as u32;
    let mut total_vertices = 0u32;
    let mut total_indices = 0u32;
    let mut textured_meshes = 0usize;
    let mut placeholder_meshes = 0usize;
    let mut skin_stats = SkinStats::default();
    let use_meshes = model.finest_level();
    for mesh in &use_meshes {
        let first_index = scene.indices.len() as u32;
        // `base` is this submesh's first vertex, absolute in the shared buffer;
        // the indices stay **relative to the submesh** and `base_vertex` adds
        // the offset at draw time, which is the convention the map path uses.
        // Writing absolute indices here *and* setting `base_vertex` counted the
        // offset twice: every submesh after the first read the wrong vertices,
        // and the last ones (the head) read past the buffer and vanished.
        let base = scene.vertices.len() as u32;
        for (i, position) in mesh.positions.iter().enumerate() {
            let normal = mesh.normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]);
            let uv = mesh
                .uv_sets
                .first()
                .and_then(|set| set.get(i))
                .copied()
                .unwrap_or([0.0, 0.0]);
            // Bone indices go through the skeleton's Lookup table onto the
            // palette at `joint_base`, so slot 0 stays the identity an
            // unskinned vertex points at. A submesh that carries no blend
            // weights — 6 of Tracer's 9 — keeps that identity: taking its
            // (empty) weights as authoritative would skin it with the zero
            // matrix and collapse it onto the origin, which is how the head
            // disappeared once. Lanes the Lookup cannot map are zeroed, so
            // the shader's all-zero guard holds them in bind pose.
            let (joints, weights) = if rigid {
                // A rigid attachment (the FP weapon) is drawn in its own bind
                // mesh under the instance placement: palette slot 0 is the
                // identity, so every lane points there.
                ([0u8, 0, 0, 0], [255u8, 0, 0, 0])
            } else if mesh.skinned && !no_skin {
                match skin {
                    Some(skin) => skinned_lanes(skin, joint_base, mesh, i, &mut skin_stats),
                    // No skeleton: the identity default, which is bind-exact.
                    // The old path wrote the raw indices here, which indexed
                    // past a one-slot palette.
                    None => ([1u8, 0, 0, 0], [255u8, 0, 0, 0]),
                }
            } else {
                ([1u8, 0, 0, 0], [255u8, 0, 0, 0])
            };
            scene.vertices.push(Vertex {
                position: *position,
                normal,
                uv,
                joints,
                weights,
            });
        }
        for index in &mesh.indices {
            scene.indices.push(*index);
        }
        let index_count = scene.indices.len() as u32 - first_index;
        if index_count == 0 {
            scene.vertices.truncate(base as usize);
            continue;
        }
        let id = model.materials.get(mesh.material).copied().unwrap_or(0);
        let slot = slot_by_id.get(&id).copied().unwrap_or(0);
        if slot == 0 {
            placeholder_meshes += 1;
        } else {
            textured_meshes += 1;
        }
        // The *scene's* copy of the vertices, so a collapse in this path (not in
        // the model reader) is visible as a zero-extent extent.
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for v in &scene.vertices[base as usize..] {
            for a in 0..3 {
                lo[a] = lo[a].min(v.position[a]);
                hi[a] = hi[a].max(v.position[a]);
            }
        }
        let skinned_here = !rigid && mesh.skinned && !no_skin && skin.is_some();
        report.push(format!(
            "  submesh {:>2}: material id 0x{id:016x}, {} tris, extent ({:.3},{:.3},{:.3})..({:.3},{:.3},{:.3}), {}skinned -> {}",
            report.len(),
            index_count / 3,
            lo[0], lo[1], lo[2], hi[0], hi[1], hi[2],
            if skinned_here { "" } else { "un" },
            if slot == 0 {
                "placeholder (no readable material)".to_string()
            } else {
                let t = &scene.textures[slot as usize];
                format!("texture 0x{:016x} ({}x{} {})", t.guid, t.width, t.height, t.format)
            }
        ));
        total_vertices += scene.vertices.len() as u32 - base;
        total_indices += index_count;
        scene.draws.push(Draw {
            first_index,
            index_count,
            base_vertex: base as i32,
            instance_start: instance as u32,
            instance_count: 1,
            texture: slot,
            model: model_guid,
        });
    }
    if total_indices == 0 {
        scene.vertices.truncate(vertex_base as usize);
        return Err(format!("model 0x{model_guid:016x} has no drawable submesh"));
    }
    report.push(format!(
        "model 0x{model_guid:016x}: {total_vertices} vertices, {} triangles; {textured_meshes} of {} submeshes have a base-colour texture, {placeholder_meshes} on the placeholder — of {} look materials {unreadable_materials} are not readable on this install, {data_map_materials} name no colour map, {flat_colour} name only a flat fill and {} id(s) are ambiguous, so those surfaces are *untextured by design*; {} skinned vertices, {} unmapped lane(s) held at identity",
        total_indices / 3,
        use_meshes.len(),
        look_materials.len(),
        ambiguous_ids.len(),
        skin_stats.vertices,
        skin_stats.unmapped_lanes,
    ));
    Ok(instance)
}

/// Place a hero's character model in the scene (`--hero N`).
///
/// The model is drawn in its **bind pose** — the pose its vertex data is stored
/// in — and nothing animates it: the client's pose evaluator is not in any
/// standing capture, so a clip-driven pose is a later, capture-gated read (see
/// `docs/formats/animation.md`).
fn place_hero(
    scene: &mut Scene,
    data: &GameData,
    hero_index: u32,
    at: Option<Vec3>,
    hero_only: bool,
    args_vec: &[String],
    report: &mut Vec<String>,
) -> Result<(), String> {
    let parts = ow_tank::entity::hero_parts(data, hero_index).map_err(|e| e.to_string())?;
    // The view model is a separate entity with its own model *and its own look*;
    // `--view-model` draws that one instead of the character.
    let view_model = args_vec.iter().any(|a| a == "--view-model");
    // `--first-person` asks for the body's own placement — the player's own body,
    // which is where the client draws it — instead of the inspection framing.
    let first_person = args_vec.iter().any(|a| a == "--first-person");
    let (model_guid, look_guid) = if view_model {
        (parts.view_model, parts.view_model_look)
    } else {
        (parts.model, parts.look)
    };
    if model_guid == 0 {
        return Err(format!(
            "hero {hero_index} names no {}",
            if view_model {
                "view model"
            } else {
                "character model"
            }
        ));
    }
    let eye = scene.eye;
    let forward = Vec3::new(scene.yaw.sin(), 0.0, scene.yaw.cos());
    let (origin, matrix) = if let Some(at) = at {
        // An explicit position: the model keeps its own facing (its forward is
        // its **+Z**, measured by rendering it from ±z) and the camera is aimed
        // at it with `--look-at`.
        (at, Mat4::from_rotation_translation(Quat::IDENTITY, at))
    } else if view_model && first_person {
        // The player's own body: the character's transform, because the client
        // adds nothing to it (see `view_model_matrix`).
        let feet = Vec3::new(eye.x, eye.y - EYE_HEIGHT, eye.z);
        (feet, Scene::view_model_matrix(eye, scene.yaw, scene.pitch))
    } else if view_model {
        // Inspection framing. A view model's own placement is *inside* the
        // camera — its neck bone is 4 cm below the eye — so a static shot of it
        // would be the inside of its head. Put it in front like any other model,
        // and let `--first-person` ask for the body's own placement instead.
        let origin = Vec3::new(
            eye.x + forward.x * 2.0,
            eye.y - EYE_HEIGHT,
            eye.z + forward.z * 2.0,
        );
        (
            origin,
            Mat4::from_rotation_translation(
                Quat::from_rotation_y(scene.yaw + std::f32::consts::PI),
                origin,
            ),
        )
    } else {
        // A character model: a few metres in front, feet on the camera's floor,
        // turned to face the camera (a half turn, because its forward is +Z).
        let origin = Vec3::new(
            eye.x + forward.x * 4.0,
            eye.y - EYE_HEIGHT,
            eye.z + forward.z * 4.0,
        );
        (
            origin,
            Mat4::from_rotation_translation(
                Quat::from_rotation_y(scene.yaw + std::f32::consts::PI),
                origin,
            ),
        )
    };
    // The skeleton and the pose. In bind pose the palette is all identity, so
    // an unposed hero and a posed one take the same path — and the vertices the
    // file stores in bind pose are what the identity leaves alone.
    //
    // A model with a `CLTH` chunk takes the reference's *fake hierarchy*
    // (`Cloth::fake_hierarchy`), which is the parent list the exporters use for
    // a cloth rig; a model without one keeps `mskl`'s own list, so the default
    // path is unchanged.
    let skin = data
        .open_asset(model_guid)
        .ok()
        .and_then(|b| model_skin(&b));
    // The hero owns the palette tail it uploads: on an empty scene that is
    // the whole palette at base 1, and past map models' identity pool it is
    // appended, so their base-1 indices keep working.
    let mut joint_base = 1u32;
    if let Some(skin) = &skin {
        let pose = parse_pose(args_vec, report);
        let overrides: std::collections::HashMap<usize, glam::Quat> = pose
            .iter()
            .map(|(bone, axis, deg)| {
                let rot = match axis {
                    'x' => glam::Quat::from_rotation_x(deg.to_radians()),
                    'y' => glam::Quat::from_rotation_y(deg.to_radians()),
                    _ => glam::Quat::from_rotation_z(deg.to_radians()),
                };
                (*bone, rot)
            })
            .collect();
        let palette = skin.palette(&|bone| overrides.get(&bone).copied());
        let cells: Vec<[[f32; 4]; 4]> = palette.iter().map(|m| m.to_cols_array_2d()).collect();
        if scene.joints.len() <= 1 {
            scene.joints = std::iter::once(glam::Mat4::IDENTITY.to_cols_array_2d())
                .chain(cells.iter().copied())
                .collect();
        } else {
            joint_base = scene.joints.len() as u32;
            scene.joints.extend(cells.iter().copied());
        }
        scene.hero_joint_base = joint_base;
        scene.pose = pose;
        report.push(format!(
            "skin: {} bones, palette of {} matrices at base {joint_base} (bind-pose deviation {:.2e}, file's inverse-bind array {}{})",
            skin.local.len(),
            cells.len() + 1,
            skin.bind_palette_deviation(),
            if skin.file_inverse_trusted { "used" } else { "not used (see ow-tank-skel --bind-check)" },
            if skin.cloth_reparented.is_empty() {
                String::new()
            } else {
                format!(", {} cloth-reparented bone(s)", skin.cloth_reparented.len())
            }
        ));
    }

    let no_skin = args_vec.iter().any(|a| a == "--no-skin");
    let instance = add_model(
        scene,
        data,
        model_guid,
        matrix,
        look_guid,
        no_skin,
        skin.as_ref(),
        joint_base,
        false,
        report,
    )?;
    scene.skin = skin;
    if view_model {
        if let Ok(bytes) = data.open_asset(parts.model) {
            if let Ok(cskel) = ow_tank::skeleton::parse_model(&bytes) {
                scene.character_ids = cskel.ids.clone();
                scene.character_skin = model_skin(&bytes);
            }
        }
        if let Ok(bytes) = data.open_asset(0x0a00_0000_0000_0071u64) {
            scene.character_weapon_pose = ow_tank::clip::BlendPose::decode(&bytes);
        }
    }
    let mut weapons: Vec<(usize, usize, Mat4)> = Vec::new();
    if view_model && parts.view_model_weapon != 0 {
        // Tracer carries one pistol in each hand. The FP entity's child array
        // names the weapon child twice, with hardpoints `0x0dc…0a` and
        // `0x0dc…0b` (the typed child-definition read). Those hang off the FP
        // skeleton's two weapon bones (ids 44 and 74) — not the hand bones,
        // and not the centre weapon bone (126), which were the two wrong
        // first cuts.
        if let Ok(bytes) = data.open_asset(model_guid) {
            if let Ok(fp_skel) = ow_tank::skeleton::parse_model(&bytes) {
                scene.view_model_ids = fp_skel.ids.clone();
                let hardpoints = ow_tank::hardpoint::mstu_hardpoints_of_model(data, model_guid);
                for hp_guid in &parts.view_model_weapon_hardpoints {
                    let Some(hp) = hardpoints.iter().find(|h| h.id == *hp_guid) else {
                        eprintln!("weapon hardpoint 0x{hp_guid:016x}: not on the model");
                        continue;
                    };
                    let Some(bone) = ow_tank::hardpoint::bone_index_of(&fp_skel.ids, hp.bone_guid)
                    else {
                        eprintln!("weapon hardpoint 0x{hp_guid:016x}: no bone");
                        continue;
                    };
                    let local = ow_tank::hardpoint::local_matrix(hp.rotation, hp.position);
                    let world = match &scene.skin {
                        Some(s) => s.bind_world()[bone] * local,
                        None => local,
                    };
                    match add_model(
                        scene,
                        data,
                        parts.view_model_weapon,
                        matrix * world,
                        parts.view_model_weapon_look,
                        true,
                        None,
                        0,
                        true,
                        report,
                    ) {
                        Ok(i) => weapons.push((i, bone, local)),
                        Err(e) => eprintln!(
                            "weapon 0x{:016x} (hardpoint 0x{hp_guid:016x}): {e}",
                            parts.view_model_weapon
                        ),
                    }
                }
            }
        }
    }
    // In hero-only mode the camera is aimed at the model's chest, so a shot of
    // this scene frames the hero rather than the empty world — unless the
    // command line already said where to look, which it must win over a guess.
    if hero_only && args_vec.iter().all(|a| a != "--look-at") {
        let target = origin + Vec3::new(0.0, 1.1, 0.0);
        let d = target - scene.eye;
        if d.length_squared() > 1e-6 {
            scene.yaw = d.x.atan2(d.z);
            scene.pitch = (d.y / d.length()).asin();
        }
    }
    if view_model {
        scene.view_model_instance = Some(instance);
        scene.weapon_attachments = weapons;
    } else {
        scene.hero_instance = Some(instance);
    }
    scene.hero_model = model_guid;
    scene.hero_texture_guid = parts.texture;
    // The animation feed pairs a clip's bone ids against this model's
    // skeleton (`Skeleton::ids`), so the record→bone join must name it.
    scene.hero_model_guid = Some(model_guid);
    report.push(format!(
        "hero {hero_index}: asset 0x{:016x}, entity 0x{:016x}, view-model entity 0x{:016x}{}, {} placed at ({:.1}, {:.1}, {:.1})",
        parts.hero,
        parts.entity,
        parts.view_model_entity,
        if parts.view_model != 0 {
            format!(" (view model 0x{:016x})", parts.view_model)
        } else {
            String::new()
        },
        if view_model {
            if first_person {
                "view model at the player's own transform"
            } else {
                "view model (inspection framing; --first-person for its own placement)"
            }
        } else {
            "character model"
        },
        origin.x,
        origin.y,
        origin.z
    ));
    Ok(())
}

/// Parse `--pose <bone>:<axis>:<deg>[,<bone>:<axis>:<deg>…]`.
///
/// An explicit pose is what makes the skinning path visible before the clips
/// are found: the clips are a data question (`docs/formats/animation.md`), while
/// everything the shader needs is here.
fn parse_pose(args: &[String], report: &mut Vec<String>) -> Vec<(usize, char, f32)> {
    let Some(spec) = args
        .iter()
        .position(|a| a == "--pose")
        .and_then(|i| args.get(i + 1))
        .cloned()
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for part in spec.split(',') {
        let bits: Vec<&str> = part.trim().split(':').collect();
        if bits.len() != 3 {
            report.push(format!("--pose {part:?}: wants <bone>:<x|y|z>:<degrees>"));
            continue;
        }
        let (Ok(bone), Ok(deg)) = (bits[0].parse::<usize>(), bits[2].parse::<f32>()) else {
            report.push(format!("--pose {part:?}: bone and degrees must be numbers"));
            continue;
        };
        let axis = bits[1].chars().next().unwrap_or('x').to_ascii_lowercase();
        out.push((bone, axis, deg));
    }
    out
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
fn cluster_chunks(data: &GameData, guids: &[u64], resolves: &dyn Fn(u64) -> bool) -> Vec<Cluster> {
    const NEIGHBOURHOOD: f32 = 600.0;

    let mut boxes: Vec<(Vec3, Vec3)> = Vec::new();
    let mut kept: Vec<u64> = Vec::new();
    for guid in guids {
        let Ok(bytes) = data.open_asset(*guid) else {
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
    fn find(parent: &mut [usize], mut i: usize) -> usize {
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
    // Largest first, with a centre tiebreak so two same-sized clusters keep
    // one order across runs: `into_values` is HashMap-ordered, and `--cluster
    // N` has to name the same cluster every run.
    out.sort_by(|a, b| {
        b.chunks
            .len()
            .cmp(&a.chunks.len())
            .then_with(|| {
                a.centre
                    .x
                    .partial_cmp(&b.centre.x)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| {
                a.centre
                    .z
                    .partial_cmp(&b.centre.z)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    out
}

/// A transform from a placeable or a model-group entry: translate, rotate,
/// scale.
///
/// The rotation is stored as a quaternion in x, y, z, w order, which is what
/// the file holds.
fn transform_matrix(translation: [f32; 3], scale: [f32; 3], rotation: [f32; 4]) -> Mat4 {
    let translation = Vec3::from_array(translation);
    let scale = Vec3::from_array(scale);
    let raw = Quat::from_xyzw(rotation[0], rotation[1], rotation[2], rotation[3]);
    // A placement with no rotation stores four zeroes, which is not a unit
    // quaternion; normalising it would produce NaN and lose the whole model.
    let rotation = if raw.length_squared() > 1e-6 {
        raw.normalize()
    } else {
        Quat::IDENTITY
    };
    Mat4::from_scale_rotation_translation(scale, rotation, translation)
}

/// A placement's transform: translate, rotate, scale.
fn placement_matrix(p: &ow_tank::mapchunk::Placeable) -> Mat4 {
    transform_matrix(p.translation, p.scale, p.rotation)
}

/// Downward collision against the map geometry, coarse but real.
///
/// The grid buckets triangles by their xz extent at four-metre cells, so a
/// ground query touches the triangles near a column rather than every triangle
/// of a map. This is *not* a full character controller's collision -- walls,
/// ledges and slopes are not resolved -- but the floor a mover settles on comes
/// from the geometry the install shipped, not from a flat plane.
pub struct Collider {
    cells: std::collections::HashMap<(i32, i32), Vec<[Vec3; 3]>>,
}

impl Collider {
    /// A collider with no triangles: ground falls back to the flat floor.
    pub fn empty() -> Collider {
        Collider {
            cells: std::collections::HashMap::new(),
        }
    }

    /// Bucket every triangle of the scene into grid cells.
    pub fn build(scene: &Scene) -> Collider {
        let cell = 4.0f32;
        let mut cells: std::collections::HashMap<(i32, i32), Vec<[Vec3; 3]>> =
            std::collections::HashMap::new();
        let len = scene.vertices.len();
        let mut model_failures = 0usize;
        for idx in scene.indices.chunks_exact(3) {
            // An index past the vertex list would panic below; skip the
            // triangle and report it like unreadable models elsewhere.
            if (idx[0] as usize) >= len || (idx[1] as usize) >= len || (idx[2] as usize) >= len {
                model_failures += 1;
                continue;
            }
            let a = scene.vertices[idx[0] as usize].position;
            let b = scene.vertices[idx[1] as usize].position;
            let c = scene.vertices[idx[2] as usize].position;
            let tri = [
                Vec3::from_array(a),
                Vec3::from_array(b),
                Vec3::from_array(c),
            ];
            let min_x = a[0].min(b[0]).min(c[0]);
            let max_x = a[0].max(b[0]).max(c[0]);
            let min_z = a[2].min(b[2]).min(c[2]);
            let max_z = a[2].max(b[2]).max(c[2]);
            let gx0 = (min_x / cell).floor() as i32;
            let gx1 = (max_x / cell).floor() as i32;
            let gz0 = (min_z / cell).floor() as i32;
            let gz1 = (max_z / cell).floor() as i32;
            for gx in gx0..=gx1 {
                for gz in gz0..=gz1 {
                    cells.entry((gx, gz)).or_default().push(tri);
                }
            }
        }
        if model_failures > 0 {
            eprintln!("{model_failures} triangle(s) with bad indices skipped");
        }
        Collider { cells }
    }

    /// Highest triangle top under `(x, z)` below `from`, or the fallback floor.
    pub fn ground_at(&self, x: f32, z: f32, from: f32) -> f32 {
        const CELL: f32 = 4.0;
        let key = ((x / CELL).floor() as i32, (z / CELL).floor() as i32);
        const FALLBACK: f32 = 2.0;
        let mut best = FALLBACK;
        if let Some(tris) = self.cells.get(&key) {
            for tri in tris {
                if let Some(h) = tri_top_y(tri, x, z) {
                    // The mover stands on surfaces below its eye.
                    if h <= from && h > best {
                        best = h;
                    }
                }
            }
        }
        best
    }

    /// Cells touched, for a load report.
    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }
}

/// Barycentric top height of a triangle at an `(x, z)` point, if the point is
/// inside its xz projection.
fn tri_top_y(tri: &[Vec3; 3], x: f32, z: f32) -> Option<f32> {
    let [a, b, c] = tri;
    let (x1, z1) = (b[0] - a[0], b[2] - a[2]);
    let (x2, z2) = (c[0] - a[0], c[2] - a[2]);
    let denom = x1 * z2 - z1 * x2;
    if denom.abs() < 1e-8 {
        return None;
    }
    let (dx, dz) = (x - a[0], z - a[2]);
    let u = (dx * z2 - dz * x2) / denom;
    let v = (x1 * dz - z1 * dx) / denom;
    if u < -0.02 || v < -0.02 || u + v > 1.02 {
        return None;
    }
    Some(a[1] + u * (b[1] - a[1]) + v * (c[1] - a[1]))
}

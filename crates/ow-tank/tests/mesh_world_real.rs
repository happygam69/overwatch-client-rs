//! The range's model meshes in one world image, and a swept query through it.
//!
//! proof-covers: 0x142caed30 0x14201ae60
//!
//! Two things are pinned here:
//!
//! 1. one real block (`STU_537E56E4` of model `0x0d0000000000078c`) laid out by
//!    [`ow_tank::mesh_world`] and swept by the **read** type-3 narrowphase
//!    `ow_sim::collide::mesh_sweep` (`0x142caed30`), down to the hit's point,
//!    normal, fraction, triangle index and ids;
//! 2. every mesh shape the range's models carry -- one per holder the per-model
//!    body builder selects (`model_body::holder_selected`, `0x14201ae60`) --
//!    assembled into one `MeshWorld`, with each shape's record addressing its own
//!    arrays.
//!
//! The arrays are the block's own bytes: `0x141b53af0` passes the three pointers
//! on and the client converts nothing, so the assembly appends what the asset
//! stores. The strides are `mesh_world`'s, read from `0x142caed30`.
//!
//! **What the query is, and what it is not.** `MeshTriangleTest` (`0x142cce430`)
//! takes the read triangle path only when the query has **no support points**
//! and every lane of its scaled expansion is at most `0.005`; every other state
//! hands the triangle to `0x142cc9460`, verified as `fn/ClosestPointGjk` but not
//! called by this test. So this test drives the swept-point query, not a capsule:
//! the block's support-point count is `0` and its expansion base is `0`. The
//! capsule's support-point handoff into the solver is still **open** (`docs/formats/static-collision.md`, check 16).
//!
//! **What the query's mask is.** The layer mask is a runtime value in the client
//! -- the mover's or shape's collision layer -- and is not model-asset data, so
//! the test asks for every layer (`u16::MAX`). The distinct layer masks the
//! assembled nodes and triangles actually carry are logged so the reader can see
//! what the mask passed.
//!
//! **Where the shapes are.** The scene's placeable transform is not read, so
//! every shape is queried in its own local space: the sweep's start and end are
//! derived from the shape's own root node box, with a sweep length that is the
//! test's, not the client's.
//!
//! **In what order.** The assembly is in the test's order -- model GUIDs sorted,
//! then each model's holder list in array order -- because the client's order is
//! its scene graph, which is not read. Shape indices below are therefore this
//! file's, not the client's.
//!
//! Skipped when no install is present. Read-only.
//!
//! ```text
//! cargo test --release -p ow-tank --test mesh_world_real -- --nocapture
//! ```

use std::collections::BTreeSet;

use ow_sim::collide::{mesh_sweep, sweep_bounds, MeshWorld as NarrowphaseWorld};
use ow_stu::types::{ClassRegistry as StuRegistry, Prim, Value};
use ow_tank::classreg::ClassRegistry as Classreg;
use ow_tank::keytable::Sources;
use ow_tank::mesh_shape::MeshSource;
use ow_tank::mesh_world::{
    node_bounds, query_block, shape_xform, MeshArrays, MeshWorld, IDENTITY_ROTATION, NODE_LEN,
    NODE_MASK, TRIANGLE_LEN, TRIANGLE_MASK, VERTEX_LEN,
};
use ow_tank::{guid, mapresolve, model_body, mstu, GameData, ManifestSelector};

const MESH_BLOCK: u32 = 0x537E56E4;
const FIELD_NODES: u32 = 0x9B2111C5;
const FIELD_POINTS: u32 = 0x88FCECD7;
const FIELD_TRIANGLES: u32 = 0x0D50490E;
const FIELD_NODE_COUNT: u32 = 0xF91B8EDD;
const FIELD_38B71436: u32 = 0x38B71436;
const FIELD_HEIGHT: u32 = 0xA16426C6;
const FIELD_5BD33E19: u32 = 0x5BD33E19;

/// `STUModel`'s holder list, field `0xCB4D298D` at `+0x140`.
const FIELD_HOLDERS: u32 = 0xCB4D298D;
/// The holder's embedded mesh block, field `0xB7C8314A` at `+0x08`.
const FIELD_HOLDER_MESH: u32 = 0xB7C8314A;
/// The holder's mask word, field `0xBD18CD91` at `+0x10`.
const FIELD_HOLDER_MASK: u32 = 0xBD18CD91;
/// The holder's shape mode, field `0xD833F59A` at `+0x20`.
const FIELD_HOLDER_SHAPE_MODE: u32 = 0xD833F59A;
/// The holder's filter byte, field `0x89AABA94` at `+0x2A`.
const FIELD_HOLDER_MODE_BYTE: u32 = 0x89AABA94;
/// The `STUModel` class the holder list hangs off.
const STU_MODEL: u32 = 0x871BD3D0;

/// The block the first half of this test pins: the range model whose block
/// `model_mesh_real` already measures.
const PINNED_MODEL: u64 = 0x0d00_0000_0000_078c;

/// The guest base of the assembled world. Any value works: every pointer in a
/// record is derived from it, and the narrowphase only subtracts it.
const WORLD_BASE: u64 = 0x0000_0001_0000_0000;

/// The test's sweep length beyond the shape's own box, in each direction.
const SWEEP_MARGIN: f32 = 2.0;

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

/// Every model asset the range's placeables name.
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

/// The block's node or triangle array: the class declares both as byte arrays
/// (`Array(Prim(U8))`), so one element is one byte.
fn bytes_field(stu: &mut ow_stu::Stu, index: usize, hash: u32, reg: &StuRegistry) -> Vec<u8> {
    array_items(stu, index, hash, reg)
        .into_iter()
        .filter_map(|v| match v {
            Value::Prim(Prim::U8, bits) => Some(bits as u8),
            _ => None,
        })
        .collect()
}

/// The block's point array: `Array(Struct(Vec3A))`, four float lanes each.
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

/// One `STU_537E56E4` instance as the client would hand it on: the three arrays
/// plus the block's own scalar words.
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

/// Every holder the model's `0xCB4D298D` array names, in array order.
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

/// The instance of `STU_537E56E4` the holder embeds through field `0xB7C8314A`.
fn holder_mesh(stu: &ow_stu::Stu, holder: usize) -> Option<usize> {
    stu.instances.iter().position(|i| {
        i.hash == MESH_BLOCK
            && i.embedder_index == holder as i32
            && i.embedder_field == FIELD_HOLDER_MESH
    })
}

/// A scalar's raw bits, with a defaulted value read as its stored default and an
/// absent field as the class's own default (`0` for every field read here).
fn field_u64(stu: &mut ow_stu::Stu, index: usize, hash: u32, reg: &StuRegistry) -> u64 {
    match stu.typed_field(index, hash, reg) {
        Some(value) => match value.unwrap_defaulted() {
            Value::Prim(_, bits) => *bits,
            _ => 0,
        },
        None => 0,
    }
}

/// One point inside a box, away from its centre so a downward sweep does not run
/// along a box diagonal: the low corner plus a quarter of each extent. This is
/// the test's choice of query position, not the client's.
fn quarter_point(low: [f32; 3], high: [f32; 3]) -> (f32, f32) {
    (
        low[0] + (high[0] - low[0]) * 0.25,
        low[2] + (high[2] - low[2]) * 0.25,
    )
}

/// One shape's sweep with a query block and a world transform the caller built.
fn sweep_shape(
    world: &MeshWorld,
    index: usize,
    block: &[u8],
    xform: &[u8],
) -> Option<ow_sim::collide::MeshHit> {
    let record = world.record(index, 1.0);
    let narrowphase = NarrowphaseWorld {
        base: world.base(),
        bytes: world.bytes(),
    };
    mesh_sweep(&record, &narrowphase, block, xform).hit
}

/// The union of every shape's root node box, the frame one line is drawn in.
fn union_box(world: &MeshWorld) -> ([f32; 3], [f32; 3]) {
    let mut low = [f32::INFINITY; 3];
    let mut high = [f32::NEG_INFINITY; 3];
    for slot in world.shapes() {
        let at = world.offset_of(slot.nodes).expect("inside");
        let (shape_low, shape_high) = node_bounds(&world.bytes()[at..at + NODE_LEN]);
        for lane in 0..3 {
            low[lane] = low[lane].min(shape_low[lane]);
            high[lane] = high[lane].max(shape_high[lane]);
        }
    }
    (low, high)
}

/// The distinct layer masks the assembled nodes and triangles carry.
fn layer_masks(world: &MeshWorld) -> (BTreeSet<u16>, BTreeSet<u16>) {
    let mut nodes = BTreeSet::new();
    let mut triangles = BTreeSet::new();
    for index in 0..world.len() {
        let arrays = world.arrays(index);
        // Both arrays are read by stride, so the mask at each record's `+0x1c`
        // is every `0x20`-th pair of bytes.
        for record in arrays.nodes.chunks(NODE_LEN) {
            nodes.insert(u16::from_le_bytes(
                record[NODE_MASK..NODE_MASK + 2].try_into().unwrap(),
            ));
        }
        for record in arrays.triangles.chunks(TRIANGLE_LEN) {
            triangles.insert(u16::from_le_bytes(
                record[TRIANGLE_MASK..TRIANGLE_MASK + 2].try_into().unwrap(),
            ));
        }
    }
    (nodes, triangles)
}

/// Step 1-3: one real block, its record, and the swept hit, bit-exact.
#[test]
fn the_pinned_model_mesh_sweeps_to_an_exact_hit() {
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

    let bytes = data.open_asset(PINNED_MODEL).expect("the model opens");
    let mut stu = mstu::parse(&bytes).expect("the model parses as MSTU");
    let blocks = stu.instances_of(MESH_BLOCK);
    assert_eq!(blocks.len(), 1, "the pinned model carries one mesh block");

    let (arrays, words) = block_arrays(&mut stu, blocks[0], reg);
    println!(
        "model 0x{PINNED_MODEL:016x}: {} node byte(s) ({} node(s) of 0x{NODE_LEN:x}), \
         {} point byte(s) ({} point(s)), {} triangle byte(s) ({} record(s) of 0x{TRIANGLE_LEN:x})",
        arrays.nodes.len(),
        arrays.node_count(),
        arrays.points.len(),
        arrays.point_count(),
        arrays.triangles.len(),
        arrays.triangle_count(),
    );
    println!(
        "block words: nodeCount {} +0x3c {} height {} +0x44 bits {:08x}",
        words.node_count,
        words.field_3c,
        words.height,
        words.field_44.to_bits()
    );

    let mut world = MeshWorld::new(WORLD_BASE);
    let index = world.push(&arrays, &words);
    println!(
        "world: base 0x{:x}, {} byte(s), shape {index} nodes 0x{:x} points 0x{:x} triangles 0x{:x}",
        world.base(),
        world.bytes().len(),
        world.shape(index).nodes,
        world.shape(index).points,
        world.shape(index).triangles,
    );

    // The record `MeshShapeFill` writes at unit scale, with the world's
    // addresses in its three pointer lanes.
    let record = world.record(index, 1.0);
    let dwords: Vec<u32> = record[..0x4c]
        .chunks(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
        .collect();
    println!("shape record (19 dwords): {dwords:08x?}");
    assert_eq!(&dwords[0..4], &[1.0f32.to_bits(); 4], "+0x00 scale lanes");
    assert_eq!(&dwords[4..8], &[1.0f32.to_bits(); 4], "+0x10 inverse lanes");
    let slot = *world.shape(index);
    let address = |lane: usize| u64::from(dwords[lane]) | (u64::from(dwords[lane + 1]) << 32);
    assert_eq!(address(8), slot.nodes, "+0x20 nodes");
    assert_eq!(address(10), slot.points, "+0x28 points");
    assert_eq!(address(12), slot.triangles, "+0x30 triangles");
    assert_eq!(dwords[14], 9, "+0x38 nodeCount");
    assert_eq!(dwords[15], 8, "+0x3c point count");
    assert_eq!(dwords[16], 12, "+0x40 the block's 0x3c word");
    assert_eq!(dwords[17], 5, "+0x44 height");
    assert_eq!(dwords[18], 96.0f32.to_bits(), "+0x48 scale * 0x5BD33E19");

    // The query: a downward sweep through the box, in the shape's own frame.
    let (low, high) = node_bounds(&world.bytes()[..NODE_LEN]);
    let (x, z) = quarter_point(low, high);
    let block = query_block(
        [x, high[1] + SWEEP_MARGIN, z],
        [x, low[1] - SWEEP_MARGIN, z],
        1.0,
        0.0,
        u16::MAX,
    );
    println!(
        "query: start ({x}, {}, {z}) end ({x}, {}, {z}) expansion 0 count 0 mask 0xffff",
        high[1] + SWEEP_MARGIN,
        low[1] - SWEEP_MARGIN
    );
    // `0x142cd5540` on this path returns its own three inputs unchanged, so the
    // block the narrowphase reads is exactly what `sweep_bounds` produces.
    let (start, end, expansion) = sweep_bounds(&block, &[]);
    assert_eq!(start, [x, high[1] + SWEEP_MARGIN, z, 0.0]);
    assert_eq!(end, [x, low[1] - SWEEP_MARGIN, z, 0.0]);
    assert_eq!(expansion, [0.0; 4]);

    let xform = shape_xform([0.0; 3], IDENTITY_ROTATION);
    let narrowphase = NarrowphaseWorld {
        base: world.base(),
        bytes: world.bytes(),
    };
    let sweep = mesh_sweep(&record, &narrowphase, &block, &xform);
    println!(
        "walk: {} internal node(s), {} leaf/leaves",
        sweep.internals, sweep.leaves
    );
    let hit = sweep.hit.expect("the downward sweep crosses the box");
    println!(
        "hit: point {:?} normal {:?} fraction {:?} triangle {} ids {:?}",
        hit.point, hit.normal, hit.fraction, hit.triangle, hit.ids
    );

    // Pinned from the read narrowphase against this block. The box's node says
    // its top is at y = 4.01, the mesh's own top face is at y = 4, and the sweep
    // is 2 + 4.02 + 2 units long, so it meets the face a quarter of the way
    // along; the accepted triangle is the leaf's base plus its index, and its
    // two ids come straight out of the triangle record at `+0x18` and `+0x1a`.
    assert_eq!(high[1], 4.01, "the root node's high y");
    assert_eq!(hit.point, [x, 4.0, z, 0.0], "the contact point");
    assert_eq!(hit.normal, [0.0, 1.0, 0.0, 0.0], "the face normal");
    assert_eq!(
        hit.fraction,
        [f32::from_bits(0x3e80_51b8); 4],
        "the hit fraction"
    );
    assert_eq!(hit.triangle, 7, "the triangle the walk accepted");
    assert_eq!(hit.ids, [0x8000, 0], "the triangle record's two ids");
    assert_eq!(sweep.leaves, 1, "one leaf was expanded");
    assert_eq!(sweep.internals, 4, "four internal nodes were tested");
}

/// Step 4: every mesh shape the range carries, in one world, and one sweep.
#[test]
fn the_ranges_mesh_shapes_assemble_into_one_world() {
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

    let mut world = MeshWorld::new(WORLD_BASE);
    // The model each shape came from, one entry per shape, in push order.
    let mut owners: Vec<u64> = Vec::new();
    // The first shape's arrays as the asset stored them, to compare the world
    // against after the assembly.
    let mut first_block: Option<MeshArrays> = None;
    let mut mesh_models = 0usize;
    let mut holders = 0usize;
    let mut triangle_words_agree = 0usize;
    for model in &models {
        let Ok(bytes) = data.open_asset(*model) else {
            continue;
        };
        let Ok(mut stu) = mstu::parse(&bytes) else {
            continue;
        };
        let mut pushed = 0usize;
        for instance in stu.instances_of(STU_MODEL) {
            for holder in holder_instances(&mut stu, instance, reg) {
                holders += 1;
                let mode_byte = field_u64(&mut stu, holder, FIELD_HOLDER_MODE_BYTE, reg) as u8;
                let mask = field_u64(&mut stu, holder, FIELD_HOLDER_MASK, reg);
                let shape_mode = field_u64(&mut stu, holder, FIELD_HOLDER_SHAPE_MODE, reg) as i32;
                let Some(block) = holder_mesh(&stu, holder) else {
                    // The builder's `[holder+0x08] != 0` test fails here.
                    assert!(
                        !model_body::holder_selected(mode_byte, 0, mask, u64::MAX),
                        "a holder without a mesh must not be selected"
                    );
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
                let (arrays, words) = block_arrays(&mut stu, block, reg);
                if first_block.is_none() {
                    first_block = Some(arrays.clone());
                }
                if words.field_3c as usize * TRIANGLE_LEN == arrays.triangles.len() {
                    triangle_words_agree += 1;
                }
                world.push(&arrays, &words);
                owners.push(*model);
                pushed += 1;
            }
        }
        if pushed > 0 {
            mesh_models += 1;
        }
    }

    println!(
        "{} model asset(s) named by the range, {mesh_models} carrying a selected mesh shape",
        models.len()
    );
    println!(
        "{} holder(s), {} shape(s) assembled, buffer {} byte(s) from base 0x{:x}",
        holders,
        world.len(),
        world.bytes().len(),
        world.base(),
    );
    println!(
        "blocks where the 0x38B71436 word times 0x{TRIANGLE_LEN:x} equals the triangle array's byte length: {triangle_words_agree} of {}",
        world.len()
    );
    let (node_masks, triangle_masks) = layer_masks(&world);
    println!("layer masks in the data: nodes {node_masks:?}, triangles {triangle_masks:?}");

    assert_eq!(models.len(), 229, "model assets named by the range");
    assert_eq!(mesh_models, 161, "models that carry a mesh block");
    assert_eq!(world.len(), 272, "one shape per selected holder");
    // The block's `0x38B71436` word is the triangle count and `0x20` the record
    // stride: the two agree on every block the range carries.
    assert_eq!(
        triangle_words_agree,
        world.len(),
        "the block's triangle count times the stride is the array's byte length"
    );
    assert_eq!(world.len(), owners.len());
    // Every record addresses its own arrays, inside the one buffer.
    for index in 0..world.len() {
        let slot = *world.shape(index);
        let record = world.record(index, 1.0);
        let address = |lane: usize| {
            let low = u32::from_le_bytes(record[lane * 4..lane * 4 + 4].try_into().unwrap());
            let high = u32::from_le_bytes(record[lane * 4 + 4..lane * 4 + 8].try_into().unwrap());
            u64::from(low) | (u64::from(high) << 32)
        };
        assert_eq!(address(8), slot.nodes, "shape {index}: +0x20 is its nodes");
        assert_eq!(
            address(10),
            slot.points,
            "shape {index}: +0x28 is its points"
        );
        assert_eq!(
            address(12),
            slot.triangles,
            "shape {index}: +0x30 is its triangles"
        );
        assert!(world.offset_of(slot.nodes).is_some());
        assert!(world.offset_of(slot.points).is_some());
        assert!(world.offset_of(slot.triangles).is_some());
        let arrays = world.arrays(index);
        assert_eq!(
            arrays.nodes.len(),
            slot.node_count as usize * NODE_LEN,
            "shape {index}: the node array is nodeCount records"
        );
        assert_eq!(
            arrays.points.len(),
            slot.point_count as usize * VERTEX_LEN,
            "shape {index}: the point array is the header's count of vertices"
        );
        assert_eq!(
            arrays.triangles.len() % TRIANGLE_LEN,
            0,
            "shape {index}: the triangle array is whole records"
        );
        assert_eq!(
            slot.triangles - slot.nodes,
            (arrays.nodes.len() + arrays.points.len()) as u64,
            "shape {index}: nodes then points then triangles, with no padding"
        );
    }
    // The assembly is byte-identical to what the asset stores.
    assert_eq!(
        world.arrays(0),
        first_block.expect("at least one shape"),
        "the first shape round-trips byte for byte"
    );

    // One downward sweep through every shape. All shapes share one line -- the
    // union of their root boxes -- so the fractions are comparable and the
    // smallest is the nearest surface along it. The shapes are in their own
    // local frames: the scene's placement transform is not read.
    let (union_low, union_high) = union_box(&world);
    let (x, z) = quarter_point(union_low, union_high);
    let block = query_block(
        [x, union_high[1] + SWEEP_MARGIN, z],
        [x, union_low[1] - SWEEP_MARGIN, z],
        1.0,
        0.0,
        u16::MAX,
    );
    let xform = shape_xform([0.0; 3], IDENTITY_ROTATION);
    println!(
        "one line through the unplaced assembly: start ({x}, {}, {z}) end ({x}, {}, {z}), \
         union box {:?}..{:?}",
        union_high[1] + SWEEP_MARGIN,
        union_low[1] - SWEEP_MARGIN,
        union_low,
        union_high
    );
    let (start, end, expansion) = sweep_bounds(&block, &[]);
    assert_eq!(start, [x, union_high[1] + SWEEP_MARGIN, z, 0.0]);
    assert_eq!(end, [x, union_low[1] - SWEEP_MARGIN, z, 0.0]);
    assert_eq!(expansion, [0.0; 4]);

    let mut hit_count = 0usize;
    let mut first: Option<(usize, ow_sim::collide::MeshHit)> = None;
    for index in 0..world.len() {
        let Some(hit) = sweep_shape(&world, index, &block, &xform) else {
            continue;
        };
        hit_count += 1;
        if first
            .as_ref()
            .is_none_or(|(_, earlier)| hit.fraction[0] < earlier.fraction[0])
        {
            first = Some((index, hit));
        }
    }
    println!("{hit_count} of {} shape(s) produced a hit", world.len());
    let (index, hit) = first.expect("at least one shape is hit");
    println!(
        "first hit: shape {index}, model 0x{:016x}, point {:?}, normal {:?}, fraction {:?}, \
         triangle {} ids {:?}",
        owners[index], hit.point, hit.normal, hit.fraction, hit.triangle, hit.ids
    );

    // Pinned from the assembled world. The nearest surface along the line is a
    // box top at y = 100, and the contact is the point where the line meets it.
    assert_eq!(hit.point, [x, 100.0, z, 0.0], "the nearest contact point");
    assert_eq!(hit.normal, [0.0, 1.0, 0.0, 0.0], "the face normal");
    assert_eq!(
        hit.fraction,
        [f32::from_bits(0x3c8a_edfa); 4],
        "the nearest hit's fraction"
    );
    assert_eq!(hit.triangle, 14, "the triangle the walk accepted");
    assert_eq!(owners[index], 0x0d00_0000_0000_10f5, "the owning model");
    assert_eq!(index, 49, "the shape in assembly order");
}

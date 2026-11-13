//! The collision descriptor `C` (registered class `0x564B1018`) out of the
//! install, and a real brush decoded from it.
//!
//! proof-covers: 0x142ced620 0x142cf23c0
//!
//! `C` is not stored in a model: all 32,278 readable model assets in this
//! install were scanned and none mentions the class hash. It is reached as the
//! loaded target of an asset reference: nine entity assets carry an instance of
//! `0x09CB5A77` whose field `0xA7ED6024` is the
//! `teStructuredDataAssetRef<STU_564B1018>`. Each target is a type-`0x112`
//! container with two chunks, `FRAC` and `FSTU`; `FSTU` is a `{i64 offset,
//! i64 size}` header followed by an STU blob holding exactly one instance, `C`.
//!
//! `C`'s instance points are the `Vec3A` array `0x88FCECD7` of the geometry
//! block `0xB7C8314A` — the same array `GeometryBuild` (`0x14213c440`) reads at
//! `C+0x98` into the ring builder `0x142ced620`. This test pins the ring and the
//! mesh those points produce, bit for bit.
//!
//! Skipped when no install is present. Read-only.
//!
//! ```text
//! cargo test --release -p ow-tank --test model_collision_real -- --nocapture
//! ```

use ow_stu::types::{ClassRegistry as StuRegistry, FieldType, Value};
use ow_tank::classreg::ClassRegistry as Classreg;
use ow_tank::keytable::Sources;
use ow_tank::{brush, brush_convert, guid, mapresolve, GameData, ManifestSelector};

const CLASS_HASH: u32 = 0x564B1018;
const REFERRER: u32 = 0x09CB5A77;
const REF_FIELD: u32 = 0xA7ED6024;
const FIELD_GEOMETRY_BLOCK: u32 = 0xB7C8314A;
const FIELD_POINTS: u32 = 0x88FCECD7;
/// `GeometryBuild` stores `1` in the word past its two-argument struct, which
/// selects `FUN_142ceeee0` — the emission variant `brush_convert` calls
/// variant 1.
const EMISSION_VARIANT: i32 = 1;

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

/// The `FSTU` chunk's STU blob: `{i64 offset, i64 size}` then the blob, the same
/// header shape `MSTU` uses.
///
/// The header rule here is *observed* on the seven assets (the offset is 16 on
/// all of them and the slice parses as STU), not read from a client function;
/// this test only uses it to slice to the blob, and nothing in `ow-tank` depends
/// on it.
fn fstu_stu(bytes: &[u8]) -> Option<(i64, i64, ow_stu::Stu)> {
    let chunks = ow_tank::chunks::Chunks::parse(bytes).ok()?;
    let payload = chunks.payload(bytes, b"FSTU")?;
    if payload.len() < 16 {
        return None;
    }
    let off = i64::from_le_bytes(payload[0..8].try_into().unwrap());
    let size = i64::from_le_bytes(payload[8..16].try_into().unwrap());
    if off < 0 || size < 0 {
        return None;
    }
    let (o, s) = (off as usize, size as usize);
    let blob = payload.get(o..o.checked_add(s)?)?;
    Some((off, size, ow_stu::Stu::parse(blob.to_vec()).ok()?))
}

/// The C assets the install's entity assets name, in GUID order.
fn c_assets(data: &GameData) -> Vec<u64> {
    let registry = data.registry();
    let mut targets: Vec<u64> = Vec::new();
    for g in data.guids_of_type(0x003) {
        let Ok(bytes) = data.open_asset(g) else {
            continue;
        };
        if !bytes.windows(4).any(|w| w == REFERRER.to_le_bytes())
            || !bytes.windows(4).any(|w| w == REF_FIELD.to_le_bytes())
        {
            continue;
        }
        let Ok(mut stu) = ow_stu::Stu::parse(bytes) else {
            continue;
        };
        for index in stu.instances_of(REFERRER) {
            if let Some(t) = stu.read_asset_ref(index, REF_FIELD) {
                if registry.get(t).is_some() {
                    targets.push(t);
                }
            }
        }
    }
    targets.sort_unstable();
    targets.dedup();
    targets
}

/// The instance points of a C instance: the geometry block's `Vec3A` array.
fn instance_points(
    stu: &mut ow_stu::Stu,
    index: usize,
    reg: &StuRegistry,
) -> Option<Vec<[f32; 4]>> {
    let Some(Value::Inline { start, bag }) = stu.typed_field(index, FIELD_GEOMETRY_BLOCK, reg)
    else {
        return None;
    };
    let Some(FieldType::Instance(inner_class)) = reg
        .field(CLASS_HASH, FIELD_GEOMETRY_BLOCK)
        .map(|f| f.ty.clone())
    else {
        return None;
    };
    let Some(Value::Array(items)) = stu.typed_field_at(inner_class, start, bag, FIELD_POINTS, reg)
    else {
        return None;
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        match item {
            Value::Struct(f) if f.len() >= 4 => out.push([f[0], f[1], f[2], f[3]]),
            _ => return None,
        }
    }
    Some(out)
}

#[test]
fn the_collision_descriptor_is_reachable_through_its_asset_reference() {
    let Some(data) = open() else {
        return;
    };
    let root = ow_casc::install::detect_root().expect("the install root");
    let sources = Sources::discover(std::slice::from_ref(&root));
    let classreg = Classreg::load_from_sources(&sources);
    if classreg.is_empty() {
        eprintln!("note: no STU class checkout found; the payload walk is skipped");
        return;
    }
    let reg = classreg.inner();

    let targets = c_assets(&data);
    println!(
        "{} distinct C asset(s): {}",
        targets.len(),
        targets
            .iter()
            .map(|t| format!("0x{t:016x}"))
            .collect::<Vec<_>>()
            .join(" ")
    );
    assert_eq!(targets.len(), 7, "the C assets the referrers name");

    // The container shape and C's instance, on every one of them. Two payload
    // sizes exist, matching the two ring shapes: 0x29/0x2c/0x35/0x38 carry the
    // rectangle, 0x2a/0x33/0x39 the slanted quad.
    let mut shapes: Vec<(u64, i32, i64, i64, usize)> = Vec::new();
    for target in &targets {
        let bytes = data.open_asset(*target).expect("the C asset opens");
        let chunks = ow_tank::chunks::Chunks::parse(&bytes).expect("chunked");
        assert_eq!(chunks.summary(), "FRACv1 FSTUv1", "0x{target:016x} chunks");
        let (off, size, mut stu) = fstu_stu(&bytes).expect("FSTU holds an STU blob");
        assert_eq!(off, 16, "0x{target:016x} FSTU header offset");

        let hits = stu.instances_of(CLASS_HASH);
        assert_eq!(hits.len(), 1, "0x{target:016x} carries exactly one C");
        let index = hits[0];
        let info = stu.instances[index];
        assert_eq!(info.embedder_field, 0, "C is a top-level instance");
        println!(
            "0x{target:016x} type 0x{:03X} {} bytes: C payload {} bytes, {} field(s)",
            guid::type_of(*target),
            bytes.len(),
            info.size,
            stu.fields(index).len()
        );

        let points = instance_points(&mut stu, index, reg).expect("the geometry block's points");
        assert_eq!(points.len(), 4, "0x{target:016x} is a four-point ring");
        shapes.push((*target, info.size, off, size, points.len()));
    }
    shapes.sort_unstable();
    for (asset, payload, off, size, n) in &shapes {
        println!("0x{asset:016x}: FSTU header ({off}, {size}), C payload {payload}, {n} point(s)");
    }
    // The measured table, asset by asset: two payload sizes, one ring each.
    let expected: Vec<(u64, i32, i64, i64, usize)> = vec![
        (0x0888_0000_0000_0029, 1394, 16, 42206, 4),
        (0x0888_0000_0000_002a, 1398, 16, 42082, 4),
        (0x0888_0000_0000_002c, 1394, 16, 42206, 4),
        (0x0888_0000_0000_0033, 1398, 16, 42082, 4),
        (0x0888_0000_0000_0035, 1394, 16, 42206, 4),
        (0x0888_0000_0000_0038, 1394, 16, 42206, 4),
        (0x0888_0000_0000_0039, 1398, 16, 42082, 4),
    ];
    assert_eq!(shapes, expected, "the C assets and their instances");

    // One instance in full: the asset named by entity 0x04000000000044cd.
    let bytes = data
        .open_asset(0x0888_0000_0000_0029)
        .expect("the 0x29 asset");
    let (_, _, mut stu) = fstu_stu(&bytes).expect("FSTU");
    let index = stu.instances_of(CLASS_HASH)[0];
    let layout: Vec<(usize, usize, u32)> = stu
        .fields(index)
        .iter()
        .map(|f| (f.offset, f.size, f.hash))
        .collect();
    println!("0x0888000000000029 field layout: {layout:?}");
    let points = instance_points(&mut stu, index, reg).expect("points");
    for (i, p) in points.iter().enumerate() {
        println!(
            "  point {i}: {:?} bits {:08x} {:08x}",
            p,
            p[0].to_bits(),
            p[1].to_bits()
        );
    }
    // The field hashes and widths are the client's own read set: the geometry
    // block, the fracture-piece array, and one scalar.
    assert!(layout.iter().any(|f| f.2 == FIELD_GEOMETRY_BLOCK));
    assert!(layout.iter().any(|f| f.2 == 0x20DE3871));
}

#[test]
fn the_real_brush_ring_and_mesh_are_pinned() {
    let Some(data) = open() else {
        return;
    };
    let root = ow_casc::install::detect_root().expect("the install root");
    let sources = Sources::discover(std::slice::from_ref(&root));
    let classreg = Classreg::load_from_sources(&sources);
    if classreg.is_empty() {
        eprintln!("note: no STU class checkout found; the ring pin is skipped");
        return;
    }
    let reg = classreg.inner();

    let targets = c_assets(&data);
    assert!(!targets.is_empty(), "the install names C");

    // The scale GeometryBuild passes the ring builder is a single float from
    // the object at `geometry+0x88` (`[+0xe0]`), clamped up to 1.0 and copied
    // into both lanes. That object's producer is not read (see the doc), so the
    // ring is pinned at the clamp's floor, 1.0.
    const SCALE: f32 = 1.0;

    let mut rings: Vec<(u64, Vec<i64>, Vec<i64>)> = Vec::new();
    for target in &targets {
        let bytes = data.open_asset(*target).expect("the C asset opens");
        let (_, _, mut stu) = fstu_stu(&bytes).expect("FSTU");
        let index = stu.instances_of(CLASS_HASH)[0];
        let points = instance_points(&mut stu, index, reg).expect("points");
        let brush = brush::build_from_instances(&points, [SCALE, SCALE])
            .expect("the four points form a ring");
        let xs: Vec<i64> = brush.vertices.iter().map(|v| v.x).collect();
        let ys: Vec<i64> = brush.vertices.iter().map(|v| v.y).collect();
        let mesh =
            brush_convert::tree_to_mesh_variant(&brush, EMISSION_VARIANT).expect("the ring meshes");
        assert_eq!(mesh.vertices.len(), 4, "0x{target:016x} vertices");
        assert_eq!(
            mesh.triangles,
            vec![[3, -1, 0, -1, 1, 3], [3, 2, 1, -1, 2, -1]],
            "0x{target:016x} triangles"
        );
        println!(
            "0x{target:016x}: vertices x {xs:?} y {ys:?}; mesh {} vertices, {} triangles: {:?}",
            mesh.vertices.len(),
            mesh.triangles.len(),
            mesh.triangles
        );
        rings.push((*target, xs, ys));
    }

    // Two authored shapes: a rectangle and a slanted quad, both quantised with
    // the ring builder's `roundf((v * scale) * 400)` and then truncated.
    let rectangle = (vec![0i64, 0, 800, 800], vec![400i64, 9, 9, 400]);
    let slanted = (vec![0i64, 0, 795, 795], vec![400i64, 9, -399, 2]);
    for (asset, xs, ys) in &rings {
        let want = match *asset {
            0x0888_0000_0000_002a | 0x0888_0000_0000_0033 | 0x0888_0000_0000_0039 => &slanted,
            _ => &rectangle,
        };
        assert_eq!((xs, ys), (&want.0, &want.1), "0x{asset:016x} ring");
    }
    // And the mesh: four vertices, two triangles, the same index and half-edge
    // mark pattern for both shapes.
    for (asset, _, _) in &rings {
        println!("  ring asset 0x{asset:016x}");
    }
}

#[test]
fn the_practice_range_places_no_model_that_carries_the_descriptor() {
    let Some(data) = open() else {
        return;
    };
    let root = ow_casc::install::detect_root().expect("the install root");
    let sources = Sources::discover(std::slice::from_ref(&root));
    let kinds = sources.chunk_kinds().expect("the chunk-kind table");
    let scene = mapresolve::resolve_practice_range(&data, &kinds).expect("the scene resolves");

    let mut placed: Vec<u64> = Vec::new();
    for chunk in &scene.scene.chunks {
        let Some(c) = &chunk.chunk else { continue };
        for p in &c.placeables {
            if let Some(m) = p.model {
                if guid::type_of(m) == guid::types::MODEL {
                    placed.push(m);
                }
            }
            if let Some(e) = p.entity {
                placed.push(e);
            }
        }
    }
    placed.sort_unstable();
    placed.dedup();
    println!(
        "practice range: {} chunk(s), {} placeable(s), {} distinct model/entity GUID(s)",
        scene.scene.chunks.len(),
        scene.scene.placeables,
        placed.len()
    );
    let held: std::collections::BTreeSet<u64> = placed.iter().copied().collect();
    for target in c_assets(&data) {
        assert!(
            !held.contains(&target),
            "the practice range does not place 0x{target:016x}"
        );
    }
    // And no placed model carries the class hash anywhere in its bytes.
    let needle = CLASS_HASH.to_le_bytes();
    let mut scanned = 0usize;
    for g in &placed {
        if guid::type_of(*g) != guid::types::MODEL {
            continue;
        }
        let Ok(bytes) = data.open_asset(*g) else {
            continue;
        };
        scanned += 1;
        assert!(
            !bytes.windows(4).any(|w| w == needle),
            "model 0x{g:016x} mentions C"
        );
    }
    println!("{scanned} placed model(s) scanned: none mentions 0x{CLASS_HASH:08X}");
}

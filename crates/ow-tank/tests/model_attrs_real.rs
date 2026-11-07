//! Read real models' vertex attributes: how many texcoord sets and colour sets
//! they carry, what their submesh flags say, and what cloth's fake hierarchy
//! changes about their rigs.
//!
//! The invariants asserted are length-consistency of the attribute arrays and
//! the fake hierarchy's in-range writes; the histograms are printed for the
//! operator because their *values* are the finding.
//!
//! Skipped when no install is present.

use ow_tank::keytable::Sources;
use ow_tank::{guid, model, GameData, ManifestSelector};

/// The model the notes name (hero 3's character model; MODEL type 0x00C).
const TARGET: u64 = 0x0d00000000000109;

#[test]
fn real_models_carry_their_attributes_and_cloth_reparents() {
    let Some(root) = ow_casc::install::detect_root() else {
        return;
    };
    let sources = Sources::discover(std::slice::from_ref(&root));
    let data = match GameData::open(&root, &ManifestSelector::default(), &sources) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("note: install not readable here ({e}); nothing to assert");
            return;
        }
    };
    let loader = |g: u64| data.bytes_of(g).ok();

    // The scan: the first models in the manifest plus every hero's character
    // model, where layered materials and cloth actually live.
    let guids = data.guids_of_type(guid::types::MODEL);
    let mut targets: Vec<u64> = guids.iter().copied().take(60).collect();
    let mut hero_models = 0usize;
    for hero in 0..48u32 {
        if let Ok(p) = ow_tank::entity::hero_parts(&data, hero) {
            if p.model != 0 {
                hero_models += 1;
                if !targets.contains(&p.model) {
                    targets.push(p.model);
                }
            }
        }
    }
    if !targets.contains(&TARGET) && data.bytes_of(TARGET).is_ok() {
        targets.push(TARGET);
    }
    eprintln!(
        "model attrs: {} model(s) in the manifest, {hero_models} of 48 heroes name one, scanning {}",
        guids.len(),
        targets.len()
    );

    let mut scanned = 0usize;
    let mut mesh_total = 0usize;
    let mut elements_total = 0usize;
    let mut stride_mismatch = 0usize;
    let mut unknown_formats: std::collections::BTreeMap<u8, usize> = Default::default();
    let mut with_two_uv_sets = 0usize;
    let mut with_colors = 0usize;
    let mut uv_hist: std::collections::BTreeMap<usize, usize> = Default::default();
    let mut flag_hist: std::collections::BTreeMap<u8, usize> = Default::default();
    let mut bad = Vec::new();
    let mut cloth_models = 0usize;
    let mut cloth_bones = 0usize;
    let mut first_cloth: Option<String> = None;
    let mut bounds_models = 0usize;
    let mut no_bounds = 0usize;
    let mut worst_box = 0.0f32;
    let mut worst_sphere = 0.0f32;
    let mut worst_slack = 0.0f32;
    let mut bounds_bad = Vec::new();

    for guid in &targets {
        let Ok(bytes) = data.bytes_of(*guid) else {
            continue;
        };
        let Ok(m) = model::parse(&bytes, &loader) else {
            continue;
        };
        scanned += 1;

        // The element record and the descriptor carry the stream stride in two
        // places: `packed >> 22` inside each element, and `+0x08`/`+0x09` of
        // the vertex-buffer descriptor. `MrnmHandler` publishes the descriptor
        // copy at runtime `+0x18`/`+0x19`, and the reader splits the packed
        // dword the same way the client's own writer builds it
        // (`0x14210c450`), so a disagreement here would mean the split is
        // wrong. Format bytes the reader does not decode are counted, not
        // hidden: a model that carries one must not parse silently.
        if let Ok(elements) = model::vertex_elements(&bytes) {
            for e in &elements {
                elements_total += e.buffers;
                if e.stride != 0 && e.stride != e.descriptor_stride as u16 {
                    stride_mismatch += 1;
                    if stride_mismatch <= 5 {
                        eprintln!(
                            "0x{guid:016x}: semantic {} stream {} stride {} vs descriptor {}",
                            e.semantic, e.stream, e.stride, e.descriptor_stride
                        );
                    }
                }
                if e.format > 0x0c {
                    *unknown_formats.entry(e.format).or_default() += 1;
                }
            }
        }

        // The `MDLC` bound (thirteen floats: min, max, sphere centre, radius).
        // Two claims are checked against the decoded vertices, which is what
        // makes the field meaning a measurement rather than a reading of the
        // copy loop:
        //   1. containment -- every vertex of every level is inside the box and
        //      inside the sphere;
        //   2. tightness -- `min`/`max` are the finest level's extents, not some
        //      larger volume (a 4x4-matrix reading would put a translation in
        //      one of these lanes and fail here).
        match m.bounds {
            Some(b) => {
                bounds_models += 1;
                let mut lo = [f32::MAX; 3];
                let mut hi = [f32::MIN; 3];
                for mesh in m.finest_level() {
                    for p in &mesh.positions {
                        for a in 0..3 {
                            lo[a] = lo[a].min(p[a]);
                            hi[a] = hi[a].max(p[a]);
                        }
                    }
                }
                if lo[0] <= hi[0] {
                    for a in 0..3 {
                        let slack = (b.min[a] - lo[a]).abs().max((b.max[a] - hi[a]).abs());
                        worst_slack = worst_slack.max(slack);
                        if slack > 0.05 {
                            bounds_bad.push(format!(
                                "0x{guid:016x} axis {a}: bound [{:.4}, {:.4}] vs decoded [{:.4}, {:.4}]",
                                b.min[a], b.max[a], lo[a], hi[a]
                            ));
                        }
                    }
                }
                for mesh in &m.meshes {
                    for p in &mesh.positions {
                        if !b.contains(*p) {
                            let over = (0..3)
                                .map(|a| (b.min[a] - p[a]).max(p[a] - b.max[a]))
                                .fold(f32::MIN, f32::max);
                            worst_box = worst_box.max(over);
                            if over > 1e-3 {
                                bounds_bad.push(format!(
                                    "0x{guid:016x}: vertex {p:?} is {over:.4} outside the box"
                                ));
                            }
                        }
                        let excess = b.sphere_distance(*p) - b.radius;
                        worst_sphere = worst_sphere.max(excess);
                        if excess > 1e-3 {
                            bounds_bad.push(format!(
                                "0x{guid:016x}: vertex {p:?} is {excess:.4} outside the sphere"
                            ));
                        }
                    }
                }
            }
            None => no_bounds += 1,
        }

        for mesh in &m.meshes {
            mesh_total += 1;
            *uv_hist.entry(mesh.uv_sets.len()).or_default() += 1;
            *flag_hist.entry(mesh.flags).or_default() += 1;
            if mesh.uv_sets.len() >= 2 {
                with_two_uv_sets += 1;
            }
            if !mesh.colors.is_empty() {
                with_colors += 1;
            }
            for (set, uvs) in mesh.uv_sets.iter().enumerate() {
                if uvs.len() != mesh.positions.len() {
                    bad.push(format!(
                        "0x{guid:016x} mat {}: uv set {set} has {} row(s) for {} position(s)",
                        mesh.material,
                        uvs.len(),
                        mesh.positions.len()
                    ));
                }
            }
            for (set, colors) in mesh.colors.iter().enumerate() {
                if colors.len() != mesh.positions.len() {
                    bad.push(format!(
                        "0x{guid:016x} mat {}: colour set {set} has {} row(s) for {} position(s)",
                        mesh.material,
                        colors.len(),
                        mesh.positions.len()
                    ));
                }
            }
        }
        // Cloth, on the same bytes: which bones its fake hierarchy re-parents.
        if let (Ok(sk), Ok(cloth)) = (
            ow_tank::skeleton::parse_model(&bytes),
            ow_tank::cloth::parse(&bytes),
        ) {
            let (hierarchy, reparented) = cloth.fake_hierarchy(&sk);
            if !reparented.is_empty() {
                cloth_models += 1;
                cloth_bones += reparented.len();
                for &b in &reparented {
                    assert!(
                        b < hierarchy.len(),
                        "0x{guid:016x}: bone {b} re-parented outside {} bones",
                        hierarchy.len()
                    );
                    assert_ne!(
                        hierarchy[b] as usize, b,
                        "0x{guid:016x}: bone {b} re-parented to itself"
                    );
                    assert!(
                        hierarchy[b] >= 0,
                        "0x{guid:016x}: bone {b} re-parented to {}",
                        hierarchy[b]
                    );
                }
                if first_cloth.is_none() {
                    let pairs: Vec<String> = reparented
                        .iter()
                        .take(6)
                        .map(|&b| format!("{b}->{}", hierarchy[b]))
                        .collect();
                    first_cloth = Some(format!(
                        "0x{guid:016x}: {} of {} bones re-parented ({}); {} piece(s)",
                        reparented.len(),
                        sk.parents.len(),
                        pairs.join(", "),
                        cloth.pieces.len()
                    ));
                }
            }
        }
    }

    eprintln!("  scanned {scanned} model(s), {mesh_total} submesh(es)");
    eprintln!(
        "  vertex elements: {elements_total} record(s), {stride_mismatch} packed-vs-descriptor stride mismatch(es), unknown formats {unknown_formats:?}"
    );
    assert_eq!(
        stride_mismatch, 0,
        "an element's packed stride disagrees with its descriptor's stride"
    );
    eprintln!("  uv sets per submesh: {uv_hist:?}");
    eprintln!("  submeshes with colours: {with_colors}; with 2+ uv sets: {with_two_uv_sets}");
    eprintln!("  submesh flags seen: {flag_hist:?}");
    if let Some(line) = &first_cloth {
        eprintln!("  first cloth model: {line}");
    }
    eprintln!("  cloth: {cloth_models} model(s) re-parent {cloth_bones} bone(s) total");
    eprintln!(
        "  bounds: {bounds_models} model(s) carry one, {no_bounds} without; \
         worst box overrun {worst_box:.6}, worst sphere overrun {worst_sphere:.6}, \
         worst extent slack {worst_slack:.6}"
    );
    assert!(scanned > 0, "no model asset parsed");
    assert!(
        bad.is_empty(),
        "{} length mismatches, first: {}",
        bad.len(),
        bad[0]
    );
    assert!(
        bounds_bad.is_empty(),
        "{} bound mismatch(es), first: {}",
        bounds_bad.len(),
        bounds_bad[0]
    );

    // The named model's per-submesh detail, small enough to read.
    match data.bytes_of(TARGET) {
        Ok(bytes) => match model::parse(&bytes, &loader) {
            Ok(m) => {
                println!(
                    "target 0x{TARGET:016x}: {} submesh(es), {} vertices, {} triangles",
                    m.meshes.len(),
                    m.vertex_count(),
                    m.triangle_count()
                );
                for (i, mesh) in m.meshes.iter().enumerate().take(16) {
                    println!(
                        "  [{i:>2}] mat {:<2} lod {:>3}  {} verts  uv sets {}  colors {}  flags 0x{:02x}",
                        mesh.material,
                        mesh.lod,
                        mesh.vertex_count(),
                        mesh.uv_sets.len(),
                        mesh.colors.len(),
                        mesh.flags
                    );
                }
                if let Ok(cloth) = ow_tank::cloth::parse(&bytes) {
                    if let Ok(sk) = ow_tank::skeleton::parse_model(&bytes) {
                        let (hierarchy, reparented) = cloth.fake_hierarchy(&sk);
                        println!(
                            "  cloth: {} piece(s), {} of {} bones re-parented",
                            cloth.pieces.len(),
                            reparented.len(),
                            sk.parents.len()
                        );
                        for &b in reparented.iter().take(10) {
                            println!(
                                "    bone {b:>3}: parent {} -> {}",
                                sk.parents.get(b).copied().unwrap_or(-1),
                                hierarchy.get(b).copied().unwrap_or(-1)
                            );
                        }
                    } else {
                        println!("  cloth present but the skeleton did not parse");
                    }
                } else {
                    println!("  no cloth chunk");
                }
            }
            Err(e) => eprintln!("target 0x{TARGET:016x} did not parse: {e}"),
        },
        Err(e) => eprintln!("target 0x{TARGET:016x} is not in this manifest: {e}"),
    }
}

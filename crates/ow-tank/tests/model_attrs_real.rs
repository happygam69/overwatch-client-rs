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
    let sources = Sources::discover(&[root.clone()]);
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
    let mut with_two_uv_sets = 0usize;
    let mut with_colors = 0usize;
    let mut uv_hist: std::collections::BTreeMap<usize, usize> = Default::default();
    let mut flag_hist: std::collections::BTreeMap<u8, usize> = Default::default();
    let mut bad = Vec::new();
    let mut cloth_models = 0usize;
    let mut cloth_bones = 0usize;
    let mut first_cloth: Option<String> = None;

    for guid in &targets {
        let Ok(bytes) = data.bytes_of(*guid) else {
            continue;
        };
        let Ok(m) = model::parse(&bytes, &loader) else {
            continue;
        };
        scanned += 1;
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
    eprintln!("  uv sets per submesh: {uv_hist:?}");
    eprintln!("  submeshes with colours: {with_colors}; with 2+ uv sets: {with_two_uv_sets}");
    eprintln!("  submesh flags seen: {flag_hist:?}");
    if let Some(line) = &first_cloth {
        eprintln!("  first cloth model: {line}");
    }
    eprintln!("  cloth: {cloth_models} model(s) re-parent {cloth_bones} bone(s) total");
    assert!(scanned > 0, "no model asset parsed");
    assert!(
        bad.is_empty(),
        "{} length mismatches, first: {}",
        bad.len(),
        bad[0]
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

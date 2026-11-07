//! Read a model asset: its meshes, extents, texcoords and vertex attributes.
//!
//! This is the "log, don't look" tool for the geometry the viewers draw. The
//! questions it answers, in the order they came up: how big is this model (is
//! the character 1.7 m or 100 m?), which submeshes are LODs, does a texcoord
//! set exist and is it in [0, 1], and which vertex semantics a skinned mesh
//! carries where `ow-tank::model` only decodes position, normal and uv.
//!
//! ```text
//! ow-tank-model --guid 0x0d000000000002b1
//! ow-tank-model --hero 3            # the hero's character model
//! ow-tank-model --guid 0x… --vertices 8
//! ```
//!
//! Read-only.

use ow_tank::{guid, model, GameData, ManifestSelector};

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };

    let root = match ow_casc::install::detect_root() {
        Some(r) => r,
        None => {
            eprintln!("no Overwatch install found; set OW_INSTALL");
            return std::process::ExitCode::from(2);
        }
    };
    let sources = ow_tank::keytable::Sources::discover(std::slice::from_ref(&root));
    let data = match GameData::open(
        root.display().to_string(),
        &ManifestSelector::default(),
        &sources,
    ) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("open failed: {e}");
            return std::process::ExitCode::from(2);
        }
    };

    let target = if let Some(hero) = flag("--hero").and_then(|v| v.parse::<u32>().ok()) {
        match ow_tank::entity::hero_parts(&data, hero) {
            Ok(p) if p.model != 0 => {
                println!(
                    "hero {hero}: model 0x{:016x}, texture 0x{:016x}, view model 0x{:016x}",
                    p.model, p.texture, p.view_model
                );
                p.model
            }
            Ok(_) => {
                eprintln!("hero {hero} names no model");
                return std::process::ExitCode::from(1);
            }
            Err(e) => {
                eprintln!("hero {hero}: {e}");
                return std::process::ExitCode::from(1);
            }
        }
    } else if let Some(g) = flag("--guid") {
        match u64::from_str_radix(g.trim_start_matches("0x"), 16) {
            Ok(v) => v,
            Err(_) => {
                eprintln!("--guid wants a hex GUID");
                return std::process::ExitCode::from(2);
            }
        }
    } else {
        eprintln!("usage: ow-tank-model (--guid 0x… | --hero N) [--vertices N]");
        return std::process::ExitCode::from(2);
    };

    let bytes = match data.bytes_of(target) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("load failed: {e}");
            return std::process::ExitCode::from(1);
        }
    };
    println!(
        "model 0x{target:016x}: {} bytes, type 0x{:03x}",
        bytes.len(),
        guid::type_of(target)
    );

    let loader = |g: u64| data.bytes_of(g).ok();
    let model = match model::parse(&bytes, &loader) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("parse failed: {e}");
            return std::process::ExitCode::from(1);
        }
    };

    // Extents over the finest LOD, which is what the viewers draw.
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    let mut uv_min = [f32::MAX; 2];
    let mut uv_max = [f32::MIN; 2];
    let mut lod_counts: std::collections::BTreeMap<i8, (usize, usize)> = Default::default();
    for mesh in &model.meshes {
        let e = lod_counts.entry(mesh.lod).or_default();
        e.0 += 1;
        e.1 += mesh.triangle_count();
    }
    for mesh in model.finest_level() {
        for p in &mesh.positions {
            for i in 0..3 {
                min[i] = min[i].min(p[i]);
                max[i] = max[i].max(p[i]);
            }
        }
        for uv in mesh.uv_sets.first().into_iter().flatten() {
            uv_min[0] = uv_min[0].min(uv[0]);
            uv_min[1] = uv_min[1].min(uv[1]);
            uv_max[0] = uv_max[0].max(uv[0]);
            uv_max[1] = uv_max[1].max(uv[1]);
        }
    }
    let size = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
    println!(
        "  {} submeshes, {} vertices, {} triangles",
        model.meshes.len(),
        model.vertex_count(),
        model.triangle_count()
    );
    println!(
        "  extents (finest lod): min ({:.3}, {:.3}, {:.3}) max ({:.3}, {:.3}, {:.3}); size ({:.3}, {:.3}, {:.3})",
        min[0], min[1], min[2], max[0], max[1], max[2], size[0], size[1], size[2]
    );
    println!(
        "  uv range: ({:.3}, {:.3}) .. ({:.3}, {:.3})",
        uv_min[0], uv_min[1], uv_max[0], uv_max[1]
    );
    println!("  materials: {} (MDLC man ids)", model.materials.len());
    for (lod, (meshes, tris)) in &lod_counts {
        println!("  lod {lod:>3}: {meshes} submesh(es), {tris} triangles");
    }

    // The bind pose, as the vertices store it: the model is *already* posed, so
    // the highest vertex is the head and its height is the model's height.
    println!(
        "  bind pose: {:.4} m tall, feet at y = {:.4}, centre x {:.4}",
        size[1],
        min[1],
        (min[0] + max[0]) * 0.5
    );

    // Per-submesh detail: a material id per submesh, its UV range, and whether
    // its UVs fit one texture ("[0, 1]") or need a wrap. The range is over the
    // **first** texcoord set (what a single-set reader uses); the count says
    // how many the submesh actually carries.
    println!("  submeshes of the finest level:");
    for (i, mesh) in model.finest_level().iter().enumerate() {
        let mut uv_min = [f32::MAX; 2];
        let mut uv_max = [f32::MIN; 2];
        for uv in mesh.uv_sets.first().into_iter().flatten() {
            uv_min[0] = uv_min[0].min(uv[0]);
            uv_min[1] = uv_min[1].min(uv[1]);
            uv_max[0] = uv_max[0].max(uv[0]);
            uv_max[1] = uv_max[1].max(uv[1]);
        }
        if mesh.uv_sets.is_empty() {
            uv_min = [0.0; 2];
            uv_max = [0.0; 2];
        }
        let id = model.materials.get(mesh.material).copied().unwrap_or(0);
        // Where the submesh *is*, so a small one can be found and looked at
        // rather than assumed hidden.
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for p in &mesh.positions {
            for a in 0..3 {
                lo[a] = lo[a].min(p[a]);
                hi[a] = hi[a].max(p[a]);
            }
        }
        println!(
            "    [{i:>2}] mat {:<2} id 0x{id:016x} lod {:>3}  {} verts, {} tris  uv sets {}, uv0 ({:.3}, {:.3})..({:.3}, {:.3})  colors {}, flags 0x{:02x}  at ({:>6.3},{:>6.3},{:>6.3}) size ({:.3},{:.3},{:.3})",
            mesh.material,
            mesh.lod,
            mesh.vertex_count(),
            mesh.triangle_count(),
            mesh.uv_sets.len(),
            uv_min[0],
            uv_min[1],
            uv_max[0],
            uv_max[1],
            mesh.colors.len(),
            mesh.flags,
            (lo[0] + hi[0]) * 0.5,
            (lo[1] + hi[1]) * 0.5,
            (lo[2] + hi[2]) * 0.5,
            hi[0] - lo[0],
            hi[1] - lo[1],
            hi[2] - lo[2]
        );
    }

    // The skin attributes, measured rather than assumed: what the weight sums
    // are, which bone indices appear, and whether every vertex is influenced.
    if args.iter().any(|a| a == "--skin") {
        let mut max_index = 0u32;
        let mut min_index = u32::MAX;
        let mut min_sum = u32::MAX;
        let mut max_sum = 0u32;
        let mut unweighted = 0usize;
        let mut influenced = [0usize; 5];
        let mut total = 0usize;
        // The two readings that decided which semantic is which: the weights
        // must sum to a constant (255 here), and the indices must repeat across
        // a region (a per-region bone set) rather than look like fractions.
        let mut weight_samples = Vec::new();
        for mesh in model.finest_level() {
            for (i, w) in mesh.blend_weights.iter().enumerate() {
                let sum: u32 = w.iter().map(|v| *v as u32).sum();
                if sum == 0 {
                    unweighted += 1;
                    continue;
                }
                total += 1;
                min_sum = min_sum.min(sum);
                max_sum = max_sum.max(sum);
                influenced[w.iter().filter(|v| **v > 0).count()] += 1;
                if weight_samples.len() < 5 {
                    weight_samples.push(*w);
                }
                if let Some(j) = mesh.bone_indices.get(i) {
                    for k in 0..4 {
                        if w[k] == 0 {
                            continue;
                        }
                        max_index = max_index.max(j[k] as u32);
                        min_index = min_index.min(j[k] as u32);
                    }
                }
            }
        }
        println!("  skin: {total} weighted vertices, {unweighted} without weights");
        println!(
            "    influences per vertex: 1:{}, 2:{}, 3:{}, 4:{}",
            influenced[1], influenced[2], influenced[3], influenced[4]
        );
        println!("    weight sum range {min_sum}..{max_sum} (u8 weights, 255 = 1.0); samples {weight_samples:?}");

        println!(
            "    bone index range {}..{}",
            if min_index == u32::MAX { 0 } else { min_index },
            max_index
        );
    }

    // Which bone is which, without names: the bone's bind-pose *position* (from
    // the composed hierarchy) and how many vertices it drives. This is the tool
    // for choosing a bone to pose — `ow-view --pose <bone>:x:45`.
    if args.iter().any(|a| a == "--bones") {
        match ow_tank::skeleton::parse_model(&bytes) {
            Ok(sk) => {
                let skin = ow_tank::skin::Skin::from_skeleton(&sk);
                let world = skin.bind_world();
                let mut driven = vec![0usize; world.len()];
                for mesh in model.finest_level() {
                    for (i, w) in mesh.blend_weights.iter().enumerate() {
                        let best = (0..4).max_by_key(|k| w[*k]).unwrap_or(0);
                        if w[best] == 0 {
                            continue;
                        }
                        if let Some(j) = mesh.bone_indices.get(i) {
                            let b = j[best] as usize;
                            if b < driven.len() {
                                driven[b] += 1;
                            }
                        }
                    }
                }
                // Whether the bind poses compose to the mesh's scale: if they
                // are absolute model-space transforms, summing them along the
                // hierarchy inflates the height, which is a measurable tell.
                let local_max_y = sk
                    .bind_pose
                    .iter()
                    .map(|b| b.translation[1])
                    .fold(f32::MIN, f32::max);
                let world_max_y = world
                    .iter()
                    .map(|m| m.transform_point3(glam::Vec3::ZERO).y)
                    .fold(f32::MIN, f32::max);
                println!(
                    "  rig scale: largest bind-pose translation y {local_max_y:.3}, largest composed head y {world_max_y:.3} (mesh is 1.70 m tall)"
                );
                let min_vertices: usize = flag("--min").and_then(|v| v.parse().ok()).unwrap_or(200);
                println!("  bones driving at least {min_vertices} vertices:");
                for b in 0..world.len() {
                    if driven[b] < min_vertices {
                        continue;
                    }
                    let p = world[b].transform_point3(glam::Vec3::ZERO);
                    println!(
                        "    bone {b:>3} parent {:>4}  head ({:>7.3}, {:>7.3}, {:>7.3})  drives {} vertices",
                        skin.parents.get(b).copied().unwrap_or(-1),
                        p.x,
                        p.y,
                        p.z,
                        driven[b]
                    );
                }
            }
            Err(e) => println!("  skeleton: {e}"),
        }
    }

    // What cloth's fake hierarchy does to this rig: which bones
    // `CreateFakeHierarchy` re-parents, and off which driver. The reference
    // exporters replace the model's hierarchy with this for any model with a
    // `CLTH` chunk; `ow-scene` does the same via `Skin::from_skeleton_with_cloth`.
    if args.iter().any(|a| a == "--cloth") {
        match (
            ow_tank::skeleton::parse_model(&bytes),
            ow_tank::cloth::parse(&bytes),
        ) {
            (Ok(sk), Ok(cloth)) => {
                let (hierarchy, reparented) = cloth.fake_hierarchy(&sk);
                println!(
                    "  cloth: {} piece(s), {} of {} bones re-parented",
                    cloth.pieces.len(),
                    reparented.len(),
                    sk.parents.len()
                );
                for &b in &reparented {
                    println!(
                        "    bone {b:>3}: parent {} -> {}",
                        sk.parents.get(b).copied().unwrap_or(-1),
                        hierarchy.get(b).copied().unwrap_or(-1)
                    );
                }
            }
            (Ok(_), Err(e)) => println!("  cloth: {e}"),
            (Err(e), _) => println!("  skeleton: {e}"),
        }
    }

    // The model's own structured data (`MSTU`), read as STU: the chunk carries
    // its own field hashes, and *those* are what de-obfuscate asset references
    // inside it (`ow_stu::deobfuscate` needs the field hash as well as the
    // asset's checksum, which is why a raw qword scan finds nothing).
    if args.iter().any(|a| a == "--mstu") {
        match ow_tank::chunks::Chunks::parse(&bytes) {
            Ok(chunks) => {
                let mut found = false;
                for tag in [b"MSTU", b"MESD", b"MDLC"] {
                    let Some(payload) = chunks.payload(&bytes, tag) else {
                        continue;
                    };
                    found = true;
                    println!(
                        "  chunk {} ({} B): trying to read it as STU",
                        String::from_utf8_lossy(tag),
                        payload.len()
                    );
                    match ow_stu::Stu::parse(payload.to_vec()) {
                        Ok(mut stu) => {
                            println!("    {} instance(s)", stu.instances.len());
                            for i in 0..stu.instances.len() {
                                let hash = stu.instances[i].hash;
                                println!(
                                    "      [{i:>2}] type 0x{hash:08x}  {} field(s)",
                                    stu.fields(i).len()
                                );
                                if !args.iter().any(|a| a == "--fields") {
                                    continue;
                                }
                                for f in stu.fields(i).to_vec() {
                                    let Some((at, len)) = stu.field_bytes(i, f.hash) else {
                                        continue;
                                    };
                                    let base = stu.payload_start(i);
                                    let mut shown = format!("{len} B");
                                    if len == 8 {
                                        if let Some(g) = stu.read_asset_ref(i, f.hash) {
                                            if g != 0 {
                                                shown = format!(
                                                    "guid 0x{g:016x} type 0x{:03x} ({})",
                                                    ow_tank::guid::type_of(g),
                                                    ow_tank::guid::types::label(
                                                        ow_tank::guid::type_of(g)
                                                    )
                                                    .unwrap_or("?")
                                                );
                                            }
                                        }
                                        if shown == "8 B" {
                                            if let Some(v) = stu.u64_at(base + at) {
                                                shown = format!("u64 0x{v:016x}");
                                            }
                                        }
                                    } else if len == 4 {
                                        if let Some(v) = stu.u32_at(base + at) {
                                            shown = format!("u32 {v}");
                                        }
                                    }
                                    println!(
                                        "        +{at:#07x} h 0x{:08x} len {len}: {shown}",
                                        f.hash
                                    );
                                }
                            }
                        }
                        Err(e) => println!("    not a STU blob: {e:?}"),
                    }
                }
                if !found {
                    println!("  no MSTU/MESD/MDLC chunk");
                }
            }
            Err(e) => println!("  chunks: {e:?}"),
        }
    }

    let elements = model::vertex_elements(&bytes).unwrap_or_default();
    println!("  vertex elements (semantic, index, format, stream, offset, stride, buffers):");
    for e in elements {
        println!(
            "    {:>3}, {}, format {:#04x}, stream {}, +{:<2} stride {:<2}  x{}",
            e.semantic, e.index, e.format, e.stream, e.offset, e.stride, e.buffers
        );
    }

    if let Some(n) = flag("--vertices").and_then(|v| v.parse::<usize>().ok()) {
        let mesh = &model.meshes[0];
        println!(
            "  first {n} vertex/vertices of submesh 0 (material {}):",
            mesh.material
        );
        for i in 0..n.min(mesh.positions.len()) {
            let uv = |set: usize| {
                mesh.uv_sets
                    .get(set)
                    .and_then(|s| s.get(i))
                    .copied()
                    .unwrap_or([0.0, 0.0])
            };
            let mut line = format!(
                "    {:>3}: pos ({:>9.4}, {:>9.4}, {:>9.4})  normal ({:>6.3}, {:>6.3}, {:>6.3})  uv0 ({:>8.4}, {:>8.4})",
                i,
                mesh.positions[i][0],
                mesh.positions[i][1],
                mesh.positions[i][2],
                mesh.normals[i][0],
                mesh.normals[i][1],
                mesh.normals[i][2],
                uv(0)[0],
                uv(0)[1]
            );
            if mesh.uv_sets.len() > 1 {
                line.push_str(&format!("  uv1 ({:>8.4}, {:>8.4})", uv(1)[0], uv(1)[1]));
            }
            if let Some(c) = mesh.colors.first().and_then(|s| s.get(i)).copied() {
                line.push_str(&format!(
                    "  color ({:.3}, {:.3}, {:.3}, {:.3})",
                    c[0], c[1], c[2], c[3]
                ));
            }
            println!("{line}");
        }
    }
    std::process::ExitCode::SUCCESS
}

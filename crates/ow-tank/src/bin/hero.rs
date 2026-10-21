//! Dump a hero's asset and entity STU fields, resolving GUID-typed fields to
//! their target type.
//!
//! The animation/view-model work starts by asking *which* assets a hero points
//! at: its model (the skeleton and geometry), its first-person model, its
//! animation set.  Those live in STU component fields, some of them unnamed, so
//! this tool prints every field of every component instance with its value and,
//! for GUID fields, the target's type id (0x00C = model, 0x00D = ...) so a model
//! reference is visible even without a field name.
//!
//! ```text
//! ow-tank-hero --hero 2            # the hero asset plus its gameplay entity
//! ow-tank-hero --from 0x0400000000000022 --components
//! ```
//!
//! Read-only: opens the install the same way the rest of the tree does.

use ow_tank::{guid, GameData, ManifestSelector};

/// Type ids that name their target usefully when printed.
fn type_name(t: u16) -> &'static str {
    match t {
        0x00C => "MODEL",
        0x004 => "TEXTURE?",
        0x03F => "STATESCRIPT?",
        0x02E => "HERO",
        0x040 => "ENTITY",
        0x058 => "GRAPH",
        _ => "?",
    }
}

/// Materials, readable materials, materials with a real colour map, and the
/// model's submesh count — for any model + look pair.
fn coverage(data: &GameData, look: u64) -> (usize, usize, usize, usize) {
    if look == 0 {
        return (0, 0, 0, 0);
    }
    let Ok(materials) = ow_tank::material::materials_of_look(data, look) else {
        return (0, 0, 0, 0);
    };
    let mut readable = 0usize;
    let mut colour = 0usize;
    for m in &materials {
        if data.bytes_of(m.material).is_err() {
            continue;
        }
        readable += 1;
        if let Some(t) = ow_tank::material::base_texture_of_material(data, m.material) {
            if let Ok(bytes) = data.bytes_of(t) {
                if let Ok(tex) =
                    ow_tank::texture::parse_asset(&bytes, t, &|g| data.bytes_of(g).ok())
                {
                    let n = (tex.rgba.len() / 4).max(1) as f64;
                    let structured = (0..3usize)
                        .filter(|c| {
                            let mean =
                                tex.rgba.chunks_exact(4).map(|p| p[*c] as f64).sum::<f64>() / n;
                            let variance = tex
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
                    if structured >= 2 {
                        colour += 1;
                    }
                }
            }
        }
    }
    (materials.len(), readable, colour, 0)
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    if args.is_empty() {
        eprintln!("usage: ow-tank-hero (--hero N | --from 0xGUID) [--fields]");
        return std::process::ExitCode::from(2);
    }

    let root = match ow_casc::install::detect_root() {
        Some(r) => r,
        None => {
            eprintln!("no Overwatch install found; set OW_INSTALL");
            return std::process::ExitCode::from(2);
        }
    };
    let sources = ow_tank::keytable::Sources::discover(&[root.clone()]);
    let data = match GameData::open(
        &root.display().to_string(),
        &ManifestSelector::default(),
        &sources,
    ) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("open failed: {e}");
            return std::process::ExitCode::from(2);
        }
    };

    // A census of what a hero can actually be *painted* with: its look's
    // materials, how many of them this install can read, and how many of those
    // name an sRGB (colour) texture. The answer for hero 3 was zero, which is
    // why its body draws untextured; this says whether any hero is better off.
    if args.iter().any(|a| a == "--textures") {
        let limit: u32 = flag("--heroes").and_then(|v| v.parse().ok()).unwrap_or(40);
        let mut heroes = data.guids_of_type(guid::types::HERO);
        heroes.sort_unstable();
        println!(
            "{:>5}  {:>9}  {:>9}  {:>7}  {:>8}  {}",
            "hero", "materials", "readable", "colour", "submesh", "what it is"
        );
        for g in heroes {
            let index = guid::index_of(g);
            if index >= limit {
                continue;
            }
            let Ok(parts) = ow_tank::entity::hero_parts(&data, index) else {
                continue;
            };
            if parts.look == 0 {
                continue;
            }
            let Ok(materials) = ow_tank::material::materials_of_look(&data, parts.look) else {
                continue;
            };
            let mut readable = 0usize;
            let mut colour = 0usize;
            for m in &materials {
                if data.bytes_of(m.material).is_err() {
                    continue;
                }
                readable += 1;
                // The same rule the viewer applies: an sRGB texture *with*
                // structure. A flat fill (sd < 2) paints nothing, and a normal
                // map painted as colour is worse than leaving it blank.
                if let Some(t) = ow_tank::material::base_texture_of_material(&data, m.material) {
                    if let Ok(bytes) = data.bytes_of(t) {
                        if let Ok(tex) =
                            ow_tank::texture::parse_asset(&bytes, t, &|g2| data.bytes_of(g2).ok())
                        {
                            let n = (tex.rgba.len() / 4).max(1) as f64;
                            let sd = |c: usize| {
                                let mean =
                                    tex.rgba.chunks_exact(4).map(|p| p[c] as f64).sum::<f64>() / n;
                                (tex.rgba
                                    .chunks_exact(4)
                                    .map(|p| {
                                        let d = p[c] as f64 - mean;
                                        d * d
                                    })
                                    .sum::<f64>()
                                    / n)
                                    .sqrt()
                            };
                            // A *colour* map varies in at least two channels.
                            // One channel with structure is a mask: Reaper's
                            // only readable texture is 512x512 with sd
                            // (8.2, 0.0, 0.0) — an emissive/mask, not albedo.
                            if (0..3).filter(|c| sd(*c) >= 2.0).count() >= 2 {
                                colour += 1;
                                if colour == 1 && args.iter().any(|a| a == "--verbose") {
                                    println!(
                                        "        first colour map: 0x{t:016x} {}x{} {:?}",
                                        tex.width, tex.height, tex.format
                                    );
                                }
                            } else if args.iter().any(|a| a == "--verbose") {
                                println!(
                                    "        flat fill: 0x{t:016x} {}x{} {:?}",
                                    tex.width, tex.height, tex.format
                                );
                            }
                        }
                    }
                }
            }
            let submeshes = data
                .bytes_of(parts.model)
                .ok()
                .and_then(|b| ow_tank::model::parse(&b, &|g2| data.bytes_of(g2).ok()).ok())
                .map(|m| m.finest_level().len())
                .unwrap_or(0);
            // The view model is a separate entity with its own model *and its
            // own look*: what it is painted with is decided there, not on the
            // character.
            let mut vm = String::from("view model: none");
            if parts.view_model != 0 {
                let (vm_total, vm_readable, vm_colour, vm_meshes) =
                    coverage(&data, parts.view_model_look);
                let anim_set = if parts.view_model_anim_set != 0 {
                    format!(" anim set 0x{:016x}", parts.view_model_anim_set)
                } else {
                    String::new()
                };
                vm = format!(
                    "view model 0x{:016x}: {vm_readable}/{vm_total} materials readable, {vm_colour} colour, {vm_meshes} submeshes{anim_set}",
                    parts.view_model
                );
            }
            println!(
                "{index:>5}  {:>9}  {readable:>9}  {colour:>7}  {submeshes:>8}  model 0x{:016x}, look 0x{:016x}  |  {vm}",
                materials.len(),
                parts.model,
                parts.look
            );
        }
        return std::process::ExitCode::SUCCESS;
    }

    let mut roots: Vec<(String, u64)> = Vec::new();
    if let Some(g) = flag("--from") {
        match u64::from_str_radix(g.trim_start_matches("0x"), 16) {
            Ok(v) => roots.push(("from".into(), v)),
            Err(_) => {
                eprintln!("--from wants a hex GUID");
                return std::process::ExitCode::from(2);
            }
        }
    } else if let Some(hero) = flag("--hero").and_then(|v| v.parse::<u32>().ok()) {
        let mut heroes = data.guids_of_type(guid::types::HERO);
        heroes.sort_unstable();
        let Some(hg) = heroes.into_iter().find(|g| guid::index_of(*g) == hero) else {
            eprintln!("hero index {hero} not found");
            return std::process::ExitCode::from(1);
        };
        roots.push(("hero asset".into(), hg));
        let mut stu = match data.stu_of(hg) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("hero {hero}: {e}");
                return std::process::ExitCode::from(1);
            }
        };
        let entity = stu
            .instance(ow_tank::classes::HERO)
            .and_then(|h| stu.read_asset_ref(h, ow_tank::classes::hero::GAMEPLAY_ENTITY));
        if let Some(e) = entity {
            roots.push(("gameplay entity".into(), e));
        }
    } else {
        eprintln!("select --hero N or --from 0xGUID");
        return std::process::ExitCode::from(2);
    }

    for (label, g) in &roots {
        println!(
            "{label}: 0x{g:016x}  type 0x{:03x} index {}{}",
            guid::type_of(*g),
            guid::index_of(*g),
            if guid::type_of(*g) == guid::types::MODEL {
                "  (MODEL)"
            } else {
                ""
            }
        );
        let mut stu = match data.stu_of(*g) {
            Ok(s) => s,
            Err(e) => {
                println!("  stu: {e}");
                continue;
            }
        };
        let n = stu.instances.len();
        println!("  {n} component instance(s)");
        for i in 0..n {
            let hash = stu.instances[i].hash;
            let data_len = stu.data(i).len();
            let fields = stu.fields(i).len();
            println!("    [{i:2}] type 0x{hash:08x}  payload {data_len} B  {fields} field(s)");
            if args.iter().any(|a| a == "--fields") {
                let layout = stu.fields(i).to_vec();
                let base = stu.payload_start(i);
                for f in layout {
                    let (at, len) = match stu.field_bytes(i, f.hash) {
                        Some(v) => v,
                        None => continue,
                    };
                    let mut shown = String::new();
                    // `read_asset_ref` does the instance's own offset
                    // arithmetic; `asset_ref_at` wants an absolute offset and
                    // silently decodes noise on a relative one (bug found by
                    // the field dump reading nonsense types).
                    if len == 8 {
                        if let Some(g2) = stu.read_asset_ref(i, f.hash) {
                            if g2 != 0 {
                                shown = format!(
                                    "guid 0x{g2:016x} type 0x{:03x} {}",
                                    guid::type_of(g2),
                                    type_name(guid::type_of(g2))
                                );
                            }
                        }
                    }
                    if shown.is_empty() && len == 4 {
                        if let Some(v) = stu.u32_at(base + at) {
                            shown = format!("u32 {v} (0x{v:x})");
                        }
                    } else if shown.is_empty() && len == 8 {
                        if let Some(v) = stu.u64_at(base + at) {
                            shown = format!("u64 0x{v:016x} type 0x{:03x}", guid::type_of(v));
                        }
                    } else if shown.is_empty() {
                        shown = format!("{len} B");
                    }
                    println!("        +{at:#07x} h 0x{:08x} len {len}: {shown}", f.hash);
                }
            }
        }
    }
    std::process::ExitCode::SUCCESS
}

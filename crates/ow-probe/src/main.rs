//! Probe that reads gameplay data out of a local Overwatch installation.
//!
//! The point of this binary is to prove the data path works end to end against
//! a real install: container, manifest, registry, structured data.
//!
//! ```text
//! ow-probe info             what install was found, and its build
//! ow-probe types            asset counts per type
//! ow-probe heroes           every hero, with a resolved asset reference
//! ow-probe hero <index>     one hero in detail
//! ow-probe classmap         how many STU class names were recovered
//! ```

use std::process::ExitCode;

use ow_tank::classes;
use ow_tank::guid::{self, types};
use ow_tank::keytable::Sources;
use ow_tank::{GameData, ManifestSelector};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("info");

    match run(command, &args[1..]) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(command: &str, rest: &[String]) -> Result<(), String> {
    let selector = ManifestSelector::default();

    let Some(root) = ow_casc::install::detect_root() else {
        return Err(
            "no Overwatch install found; set OW_INSTALL to the folder containing .build.info"
                .to_string(),
        );
    };
    let install_root = root.display().to_string();
    let sources = Sources::discover(&[root]);
    println!("install: {install_root}");
    if let Some(path) = &sources.tactlib_checkout {
        println!("container library: {}", path.display());
    }
    if let Some(path) = &sources.owlib_checkout {
        println!("asset library: {}", path.display());
    }

    // `buckets` diagnoses the container alone, so it must not depend on the
    // manifest layer succeeding.
    if command == "buckets" {
        return buckets(&install_root);
    }

    let data = GameData::open(&install_root, &selector, &sources).map_err(|e| e.to_string())?;

    match command {
        "info" => info(&data),
        "types" => type_histogram(&data),
        "heroes" => heroes(&data),
        "hero" => {
            let index: u32 = rest
                .first()
                .ok_or("usage: ow-probe hero <index>")?
                .parse()
                .map_err(|_| "hero index must be a number".to_string())?;
            hero_detail(&data, index)
        }
        "classmap" => {
            println!("STU class names known: {}", data.classes.len());
            println!(
                "  {} -> {}",
                guid::types::label(guid::types::HERO).unwrap_or("?"),
                data.classes.describe(classes::HERO)
            );
            Ok(())
        }
        "settings" => settings(&data, rest),
        "scan" => scan(&data, rest),
        "dump" => dump(&data, rest),
        "grep" => grep(&data, rest),
        "mapchunk" => map_chunk(&data, rest),
        "size" => by_size(&data, rest),
        "raw" => raw(&data, rest),
        "package" => package(&data, rest),
        "graph" => graph(&data, rest),
        "model" => model(&data, rest),
        "health" => health(&data),
        "weapons" => weapons(&data),
        "weapon-params" => weapon_params(&data, rest),
        other => Err(format!(
            "unknown command {other:?}; try info, types, heroes, hero <index>, \
             health, weapons, weapon-params [hero], settings [n], scan, classmap, buckets"
        )),
    }
}

/// Read the resource graph: what package an asset belongs to, and what else
/// ships with it.
///
/// ```text
/// ow-probe graph                       # summary
/// ow-probe graph <guid>                # the package holding that asset
/// ```
fn graph(data: &GameData, rest: &[String]) -> Result<(), String> {
    let graph = ow_tank::trg::open(data).map_err(|e| e.to_string())?;
    println!(
        "resource graph: build {}, version {}, {} assets in {} packages",
        graph.build_version,
        graph.version,
        graph.memberships.len(),
        graph
            .memberships
            .iter()
            .map(|m| m.package_key)
            .collect::<std::collections::HashSet<_>>()
            .len()
    );

    let Some(guid_arg) = rest.iter().find(|a| !a.starts_with("--")) else {
        let mut counts: std::collections::BTreeMap<u16, usize> =
            std::collections::BTreeMap::new();
        for m in &graph.memberships {
            *counts.entry(guid::type_of(m.asset)).or_default() += 1;
        }
        println!("types in the graph:");
        for (kind, count) in counts.iter().rev().take(20) {
            println!("  type 0x{kind:03X}  {count:>6}");
        }
        return Ok(());
    };
    let wanted = u64::from_str_radix(guid_arg.trim_start_matches("0x"), 16)
        .map_err(|_| format!("{guid_arg:?} is not a GUID"))?;

    let Some(mine) = graph.membership(wanted) else {
        println!("0x{wanted:016x} is not in the graph");
        return Ok(());
    };
    let package = graph.package_of(wanted);
    println!(
        "0x{wanted:016x} is in package 0x{:016x} with {} asset(s)",
        mine.package_key,
        package.len()
    );
    println!("  types: {}", ow_tank::trg::ResourceGraph::describe(&package));

    let mut chunks = 0usize;
    let mut others = 0usize;
    for asset in &package {
        match guid::type_of(*asset) {
            0x0BC => {
                chunks += 1;
                if chunks <= 30 {
                    println!("  0x{asset:016x}  map chunk");
                }
            }
            _ => {
                others += 1;
                if others <= 20 {
                    println!(
                        "  0x{asset:016x}  type 0x{:03X}  {}",
                        guid::type_of(*asset),
                        if data.registry().get(*asset).is_some() { "present" } else { "absent" }
                    );
                }
            }
        }
    }
    println!("  {chunks} map chunk(s), {others} other(s)");
    Ok(())
}

/// Find the package holding an asset, and list what else is in it.
///
/// ```text
/// ow-probe package <guid> [--index N]
/// ```
fn package(data: &GameData, rest: &[String]) -> Result<(), String> {
    let packages = ow_tank::packages::open(data).map_err(|e| e.to_string())?;
    println!("{} packages", packages.len());

    if let Some(at) = rest.iter().position(|a| a == "--index") {
        let index: usize = rest
            .get(at + 1)
            .ok_or("--index needs a value")?
            .parse()
            .map_err(|_| "--index must be a number".to_string())?;
        let p = packages.load(data, index).map_err(|e| e.to_string())?;
        println!("package {index}: 0x{:016x}, {} records", p.guid, p.records.len());
        println!("  types: {}", ow_tank::packages::describe(&p));
        for guid in p.records.iter().take(60) {
            println!("  0x{guid:016x}  type 0x{:03X}", guid::type_of(*guid));
        }
        return Ok(());
    }

    let guid_arg = rest
        .iter()
        .find(|a| !a.starts_with("--"))
        .ok_or("usage: ow-probe package <guid>")?;
    let wanted = u64::from_str_radix(guid_arg.trim_start_matches("0x"), 16)
        .map_err(|_| format!("{guid_arg:?} is not a GUID"))?;

    let found = packages.find(data, wanted);
    if found.is_empty() {
        println!("0x{wanted:016x} is in no package");
        return Ok(());
    }
    for (index, p) in found {
        println!(
            "package {index}: 0x{:016x}, {} records",
            p.guid,
            p.records.len()
        );
        println!("  types: {}", ow_tank::packages::describe(&p));
        let mut chunks = 0usize;
        for guid in &p.records {
            if guid::type_of(*guid) == 0x0BC {
                chunks += 1;
                if chunks <= 40 {
                    println!("  0x{guid:016x}  map chunk");
                }
            }
        }
        println!("  {chunks} map chunk(s)");
    }
    Ok(())
}

/// Write an asset's decoded bytes to a file.
///
/// ```text
/// ow-probe raw <guid> <path>
/// ```
fn raw(data: &GameData, rest: &[String]) -> Result<(), String> {
    let guid_arg = rest.first().ok_or("usage: ow-probe raw <guid> <path>")?;
    let path = rest.get(1).ok_or("usage: ow-probe raw <guid> <path>")?;
    let guid = u64::from_str_radix(guid_arg.trim_start_matches("0x"), 16)
        .map_err(|_| format!("{guid_arg:?} is not a GUID"))?;
    let bytes = data.bytes_of(guid).map_err(|e| e.to_string())?;
    std::fs::write(path, &bytes).map_err(|e| format!("could not write {path}: {e}"))?;
    println!("wrote {} bytes to {path}", bytes.len());
    Ok(())
}

/// List registry records with a given stored size.
///
/// A chunk read out of memory has no GUID, but it does have a length. If the
/// manifest's size field is the decoded length, that length names the asset --
/// which is how a chunk lifted from a running game is tied back to its GUID.
///
/// ```text
/// ow-probe size 252808 [--type 0x0BC]
/// ```
fn by_size(data: &GameData, rest: &[String]) -> Result<(), String> {
    let size: u32 = rest
        .iter()
        .find(|a| !a.starts_with("--"))
        .ok_or("usage: ow-probe size <bytes> [--type 0xNNN]")?
        .parse()
        .map_err(|_| "size must be a number".to_string())?;
    let type_filter: Option<u16> = rest
        .iter()
        .position(|a| a == "--type")
        .and_then(|i| rest.get(i + 1))
        .and_then(|v| u16::from_str_radix(v.trim_start_matches("0x"), 16).ok());

    let mut hits = 0usize;
    for record in data.registry().records() {
        if record.size != size {
            continue;
        }
        let kind = guid::type_of(record.guid);
        if let Some(want) = type_filter {
            if kind != want {
                continue;
            }
        }
        println!("  0x{:016x}  type 0x{kind:03X}  size {}", record.guid, record.size);
        hits += 1;
        if hits >= 40 {
            println!("  (stopping at 40)");
            break;
        }
    }
    println!("{hits} record(s) of {size} bytes");
    Ok(())
}

/// Read a model asset and report the geometry it holds.
///
/// ```text
/// ow-probe model <guid>
/// ow-probe model --sweep [--count N]
/// ```
fn model(data: &GameData, rest: &[String]) -> Result<(), String> {
    let load = |g: u64| data.bytes_of(g).ok();
    let count: usize = rest
        .iter()
        .position(|a| a == "--count")
        .and_then(|i| rest.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(40);

    if rest.iter().any(|a| a == "--sweep") {
        let guids = data.guids_of_type(types::MODEL);
        println!("{} model assets", guids.len());
        let mut with_geometry = 0usize;
        let mut empty = 0usize;
        let mut unreadable = 0usize;
        let mut triangles = 0usize;
        for guid in guids.iter().take(count) {
            let Ok(bytes) = data.bytes_of(*guid) else {
                unreadable += 1;
                continue;
            };
            match ow_tank::model::parse(&bytes, &load) {
                Ok(m) if m.triangle_count() > 0 => {
                    with_geometry += 1;
                    triangles += m.triangle_count();
                    if with_geometry <= 6 {
                        println!(
                            "  0x{guid:016x}  {} meshes, {} vertices, {} triangles, {} materials",
                            m.meshes.len(),
                            m.vertex_count(),
                            m.triangle_count(),
                            m.materials.len()
                        );
                    }
                }
                Ok(_) => empty += 1,
                Err(e) => {
                    empty += 1;
                    if empty <= 3 {
                        println!("  0x{guid:016x}  {e}");
                    }
                }
            }
        }
        println!(
            "{with_geometry} with geometry, {empty} without, {unreadable} unreadable; \
             {triangles} triangles total"
        );
        return Ok(());
    }

    let guid_arg = rest
        .iter()
        .find(|a| !a.starts_with("--"))
        .ok_or("usage: ow-probe model <guid>")?;
    let guid = u64::from_str_radix(guid_arg.trim_start_matches("0x"), 16)
        .map_err(|_| format!("{guid_arg:?} is not a GUID"))?;
    let bytes = data.bytes_of(guid).map_err(|e| e.to_string())?;
    let chunks = ow_tank::chunks::Chunks::parse(&bytes).map_err(|e| e.to_string())?;
    println!("0x{guid:016x}: {} bytes", bytes.len());
    println!("chunks: {}", chunks.summary());

    let m = ow_tank::model::parse(&bytes, &load).map_err(|e| e.to_string())?;
    println!(
        "{} meshes, {} vertices, {} triangles, {} materials",
        m.meshes.len(),
        m.vertex_count(),
        m.triangle_count(),
        m.materials.len()
    );
    for (i, mesh) in m.meshes.iter().enumerate() {
        println!(
            "  [{i:>3}] {} verts, {} tris, material {}, lod {}, radius {:.2}",
            mesh.vertex_count(),
            mesh.triangle_count(),
            mesh.material,
            mesh.lod,
            mesh.radius()
        );
        if i < 2 {
            for p in mesh.positions.iter().take(4) {
                println!("        pos ({:.3}, {:.3}, {:.3})", p[0], p[1], p[2]);
            }
        }
    }
    Ok(())
}

/// Read a map chunk: what it places, and whether those references resolve.
///
/// ```text
/// ow-probe mapchunk <guid> [--limit N]
/// ow-probe mapchunk --sweep [--count N]
/// ```
fn map_chunk(data: &GameData, rest: &[String]) -> Result<(), String> {
    let resolves = |g: u64| data.registry().get(g).is_some();
    let limit: usize = rest
        .iter()
        .position(|a| a == "--limit")
        .and_then(|i| rest.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);

    if rest.iter().any(|a| a == "--sweep") {
        // Walk map chunks and report how well each one reads. A chunk whose
        // references resolve is one this program understands; a sweep shows
        // whether that is most of them or almost none.
        let count: usize = rest
            .iter()
            .position(|a| a == "--count")
            .and_then(|i| rest.get(i + 1))
            .and_then(|v| v.parse().ok())
            .unwrap_or(60);
        let guids = data.guids_of_type(0x0BC);
        println!("{} map chunk assets", guids.len());
        let mut model_ok = 0usize;
        let mut entity_ok = 0usize;
        let mut failed = 0usize;
        for guid in guids.iter().take(count) {
            let Ok(bytes) = data.bytes_of(*guid) else {
                failed += 1;
                continue;
            };
            match ow_tank::mapchunk::MapChunk::detect(&bytes, &resolves) {
                Ok((kind, chunk)) => {
                    let hits = match kind {
                        ow_tank::mapchunk::Kind::Model => chunk.resolvable_models(&resolves),
                        ow_tank::mapchunk::Kind::Entity => chunk
                            .placeables
                            .iter()
                            .filter(|p| p.entity.map(|g| resolves(g)).unwrap_or(false))
                            .count(),
                        _ => 0,
                    };
                    if hits > 0 {
                        println!(
                            "  0x{guid:016x}  {}  {}  {hits} refs resolve  {}",
                            kind.label(),
                            chunk.header.placeable_count,
                            chunk.summary()
                        );
                        if kind == ow_tank::mapchunk::Kind::Model {
                            model_ok += 1;
                        } else {
                            entity_ok += 1;
                        }
                    } else {
                        failed += 1;
                    }
                }
                Err(_) => failed += 1,
            }
        }
        println!("{model_ok} model chunks, {entity_ok} entity chunks, {failed} unreadable");
        return Ok(());
    }

    let guid_arg = rest
        .iter()
        .find(|a| !a.starts_with("--"))
        .ok_or("usage: ow-probe mapchunk <guid> [--limit N]")?;
    let guid = u64::from_str_radix(guid_arg.trim_start_matches("0x"), 16)
        .map_err(|_| format!("{guid_arg:?} is not a GUID"))?;
    let bytes = data.bytes_of(guid).map_err(|e| e.to_string())?;
    println!("0x{guid:016x}: {} bytes", bytes.len());

    let (kind, chunk) = ow_tank::mapchunk::MapChunk::detect(&bytes, &resolves)
        .map_err(|e| e.to_string())?;
    println!(
        "read as {} placeables: {}",
        kind.label(),
        chunk.summary()
    );
    println!(
        "  placeable count {} at {:#x}, instance data at {:#x}",
        chunk.header.placeable_count,
        chunk.header.placeable_offset,
        chunk.header.instance_data_offset
    );
    if rest.iter().any(|a| a == "--hex") {
        for p in chunk.placeables.iter().take(limit) {
            let start = p.offset;
            let end = (start + 96).min(bytes.len());
            println!("  --- placeable {} at {start:#x} (size {})", p.index, p.size);
            for row in bytes[start..end].chunks(16) {
                let hexs: Vec<String> = row.iter().map(|b| format!("{b:02x}")).collect();
                let text: String = row
                    .iter()
                    .map(|b| if (32..127).contains(b) { *b as char } else { '.' })
                    .collect();
                println!("      {:<47}  {text}", hexs.join(" "));
            }
        }
        return Ok(());
    }
    for p in chunk.placeables.iter().take(limit) {
        let target = p
            .model
            .or(p.entity)
            .map(|g| format!("0x{g:016x} type 0x{:03X}", guid::type_of(g)))
            .unwrap_or_else(|| "-".to_string());
        println!(
            "  [{:>4}] size {:>5}  {target:<28} at ({:.1}, {:.1}, {:.1}) scale ({:.2}, {:.2}, {:.2})",
            p.index, p.size, p.translation[0], p.translation[1], p.translation[2],
            p.scale[0], p.scale[1], p.scale[2]
        );
    }
    Ok(())
}

/// Find assets whose raw bytes contain a string.
///
/// Asset names are not shipped, so the only way to locate a thing whose name is
/// known -- "Practice Range" -- is to look for the name inside the data. This
/// searches decoded asset bytes, so it sees strings that are not in any field
/// this program knows how to read.
///
/// ```text
/// ow-probe grep "Practice Range" [--types 0x09F,0x0C5] [--limit 20]
/// ```
fn grep(data: &GameData, rest: &[String]) -> Result<(), String> {
    let needle = rest
        .iter()
        .find(|a| !a.starts_with("--"))
        .ok_or("usage: ow-probe grep <text> [--types 0xNNN,...] [--limit N]")?
        .clone();
    let mut type_ids: Vec<u16> = Vec::new();
    let mut limit = 20usize;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "--types" => {
                i += 1;
                let value = rest.get(i).ok_or("--types needs a value")?;
                for part in value.split(',') {
                    type_ids.push(
                        u16::from_str_radix(part.trim_start_matches("0x"), 16)
                            .map_err(|_| format!("bad type {part:?}"))?,
                    );
                }
            }
            "--limit" => {
                i += 1;
                limit = rest.get(i).ok_or("--limit needs a value")?.parse().unwrap_or(limit);
            }
            _ => {}
        }
        i += 1;
    }

    let histogram = data.type_histogram();
    let chosen: Vec<u16> = if type_ids.is_empty() {
        let mut small: Vec<(u16, usize)> = histogram
            .iter()
            .filter(|(_, count)| **count <= 20_000)
            .map(|(k, v)| (*k, *v))
            .collect();
        small.sort_by_key(|(_, count)| *count);
        small.into_iter().map(|(t, _)| t).collect()
    } else {
        type_ids
    };

    let bytes_needle = needle.as_bytes();
    println!("searching {} type(s) for {needle:?}", chosen.len());
    let mut hits = 0usize;
    let mut scanned = 0usize;
    for type_id in chosen {
        for guid in data.guids_of_type(type_id) {
            let Ok(bytes) = data.bytes_of(guid) else {
                continue;
            };
            scanned += 1;
            if let Some(at) = bytes
                .windows(bytes_needle.len())
                .position(|w| w == bytes_needle)
            {
                let from = at.saturating_sub(24);
                let to = (at + bytes_needle.len() + 40).min(bytes.len());
                let context = String::from_utf8_lossy(&bytes[from..to]);
                println!("  0x{guid:016x}  type 0x{type_id:03X}  at {at:#x}: {context:?}");
                hits += 1;
                if hits >= limit {
                    println!("  (stopping at {limit})");
                    return Ok(());
                }
            }
        }
    }
    println!("{hits} hit(s) in {scanned} asset(s)");
    Ok(())
}

/// Print one asset's structured data: its instances, their classes, and the
/// fields each instance actually carries.
///
/// Field layouts come from the instance's own field bag, so this shows what the
/// file holds rather than what a generated class claims. That distinction is the
/// point: a class layout from another build silently reads the wrong bytes.
///
/// ```text
/// ow-probe dump 0x079000000000005b [--strings] [--refs]
/// ```
fn dump(data: &GameData, rest: &[String]) -> Result<(), String> {
    let guid_arg = rest
        .iter()
        .find(|a| !a.starts_with("--"))
        .ok_or("usage: ow-probe dump <guid> [--strings] [--refs]")?;
    let guid = u64::from_str_radix(guid_arg.trim_start_matches("0x"), 16)
        .map_err(|_| format!("{guid_arg:?} is not a GUID"))?;
    let show_strings = rest.iter().any(|a| a == "--strings");
    let show_refs = rest.iter().any(|a| a == "--refs");
    // An inline array is not a pointer to a GUID: it is a run of elements
    // stored inside the instance, and its elements are asset references.
    let array_field: Option<u32> = rest
        .iter()
        .position(|a| a == "--array")
        .and_then(|i| rest.get(i + 1))
        .and_then(|v| u32::from_str_radix(v.trim_start_matches("0x"), 16).ok());

    let bytes = data.bytes_of(guid).map_err(|e| e.to_string())?;
    println!("0x{guid:016x}: {} bytes", bytes.len());
    if rest.iter().any(|a| a == "--raw") {
        for (i, row) in bytes.chunks(16).enumerate() {
            let hexs: Vec<String> = row.iter().map(|b| format!("{b:02x}")).collect();
            let text: String = row
                .iter()
                .map(|b| if (32..127).contains(b) { *b as char } else { '.' })
                .collect();
            println!("  {:06x}  {:<47}  {text}", i * 16, hexs.join(" "));
        }
        return Ok(());
    }
    if let Ok(chunks) = ow_tank::chunks::Chunks::parse(&bytes) {
        println!("chunked: {}", chunks.summary());
        return Ok(());
    }

    let mut stu = ow_stu::Stu::parse(bytes).map_err(|e| e.to_string())?;
    println!("{} instances", stu.instances.len());

    for index in 0..stu.instances.len() {
        let info = stu.instances[index];
        let class = data.classes.describe(info.hash);
        let embedded = if info.embedder_index >= 0 {
            format!(" in {}(0x{:08X})", info.embedder_index, info.embedder_field)
        } else {
            String::new()
        };
        println!(
            "  [{index:>4}] 0x{:08X} {class:<34} {size:>5} bytes{embedded}",
            info.hash,
            size = info.size
        );
        if !show_strings && !show_refs && array_field.is_none() {
            continue;
        }
        if let Some(field) = array_field {
            let elements = stu.inline_elements(index, field);
            println!("          0x{field:08X} array of {} element(s)", elements.len());
            for (bag, offset) in elements {
                let _ = bag;
                let at = stu.payload_start(index) + offset;
                if let Some(target) = stu.asset_ref_at(at, field) {
                    let known = data.registry().get(target).is_some();
                    println!(
                        "            0x{target:016x}  type 0x{:03X}  {}",
                        guid::type_of(target),
                        if known { "present" } else { "not in this install" }
                    );
                }
            }
        }
        let fields: Vec<ow_stu::FieldInfo> = stu.fields(index).to_vec();
        for field in fields {
            if show_strings && field.size == 4 {
                if let Some(text) = stu.read_string(index, field.hash) {
                    if !text.is_empty() && text.chars().all(|c| c.is_ascii_graphic() || c == ' ') {
                        println!("          0x{:08X} = {text:?}", field.hash);
                    }
                }
            }
            if show_refs && field.size == 8 {
                // A reference is only worth printing if it lands on an asset
                // that exists; anything else is a number that happens to be
                // eight bytes wide.
                let at = stu.payload_start(index) + field.offset;
                if let Some(target) = stu.asset_ref_at(at, field.hash) {
                    if data.registry().get(target).is_some() {
                        println!(
                            "          0x{:08X} -> 0x{target:016x}  {}",
                            field.hash,
                            guid::types::label(guid::type_of(target)).unwrap_or("asset")
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

/// Find assets holding an instance of a STU class, and read a field from it.
///
/// Asset names are not in the manifest, so an asset has to be identified by
/// what it contains. This is the tool for that: give it a class hash and it
/// reports which assets hold one, optionally reading a `teString` field so the
/// result can be recognised rather than guessed at.
///
/// ```text
/// ow-probe scan 0x02514B6A --string 0x506FA8D8 --types 0x0C5,0x054
/// ```
fn scan(data: &GameData, rest: &[String]) -> Result<(), String> {
    let mut class_hash: Option<u32> = None;
    let mut string_field: Option<u32> = None;
    let mut type_ids: Vec<u16> = Vec::new();
    let mut limit = 40usize;
    // With --refs, report every asset reference each instance holds, and what
    // type each points at. That is how a class's links are found without
    // knowing its layout: the ones that resolve are the real ones.
    let show_refs = rest.iter().any(|a| a == "--refs");

    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i].as_str();
        let hex = |s: &str| -> Result<u32, String> {
            u32::from_str_radix(s.trim_start_matches("0x"), 16)
                .map_err(|_| format!("{s:?} is not a hexadecimal number"))
        };
        match arg {
            "--string" => {
                i += 1;
                string_field = Some(hex(rest.get(i).ok_or("--string needs a value")?)?);
            }
            "--types" => {
                i += 1;
                let value = rest.get(i).ok_or("--types needs a value")?;
                for part in value.split(',') {
                    let raw = hex(part)?;
                    type_ids.push(
                        u16::try_from(raw).map_err(|_| format!("type {part} out of range"))?,
                    );
                }
            }
            "--limit" => {
                i += 1;
                limit = rest.get(i).ok_or("--limit needs a value")?.parse().unwrap_or(limit);
            }
            // A flag this command does not consume is ignored rather than
            // mistaken for the class hash.
            other if other.starts_with("--") => {}
            other => class_hash = Some(hex(other)?),
        }
        i += 1;
    }

    let class_hash = class_hash.ok_or("usage: ow-probe scan <class-hash> [--string <field>] [--types 0xNNN,...]")?;

    // Without a type list, sweep the types small enough to walk whole.
    let histogram = data.type_histogram();
    let chosen: Vec<u16> = if type_ids.is_empty() {
        let mut small: Vec<(u16, usize)> = histogram
            .iter()
            .filter(|(_, count)| **count <= 20_000)
            .map(|(k, v)| (*k, *v))
            .collect();
        small.sort_by_key(|(_, count)| *count);
        small.into_iter().map(|(t, _)| t).collect()
    } else {
        type_ids
    };

    println!(
        "scanning {} type(s) for class 0x{class_hash:08X}{}",
        chosen.len(),
        string_field
            .map(|f| format!(", reading string field 0x{f:08X}"))
            .unwrap_or_default()
    );

    let mut found = 0usize;
    let mut assets_scanned = 0usize;
    for type_id in chosen {
        let guids = data.guids_of_type(type_id);
        for guid in guids {
            let Ok(bytes) = data.bytes_of(guid) else {
                continue;
            };
            let Ok(mut stu) = ow_stu::Stu::parse(bytes) else {
                continue;
            };
            assets_scanned += 1;
            let instances = stu.instances_of(class_hash);
            if instances.is_empty() {
                continue;
            }
            for instance in instances {
                found += 1;
                let text = string_field
                    .and_then(|field| stu.read_string(instance, field))
                    .unwrap_or_default();
                println!(
                    "  0x{guid:016x}  type 0x{type_id:03X}  instance {instance:<5} {text}"
                );
                if show_refs {
                    let fields: Vec<ow_stu::FieldInfo> = stu.fields(instance).to_vec();
                    for field in fields {
                        if field.size != 8 {
                            continue;
                        }
                        let at = stu.payload_start(instance) + field.offset;
                        if let Some(target) = stu.asset_ref_at(at, field.hash) {
                            if data.registry().get(target).is_some() {
                                println!(
                                    "        0x{:08X} -> 0x{target:016x}  type 0x{:03X}",
                                    field.hash,
                                    guid::type_of(target)
                                );
                            }
                        }
                    }
                }
                if found >= limit {
                    println!("  (stopping at {limit})");
                    return Ok(());
                }
            }
        }
    }

    println!("{found} instance(s) in {assets_scanned} readable asset(s)");
    Ok(())
}

/// The per-hero tuning values.
///
/// These are the multipliers a player can move in a custom game, not base combat
/// statistics. They are read here because they exercise the whole path and reveal
/// the vocabulary of tunable quantities per hero.
fn settings(data: &GameData, rest: &[String]) -> Result<(), String> {
    let limit: usize = rest
        .first()
        .map(|s| s.parse().unwrap_or(25))
        .unwrap_or(25);

    let assets = ow_tank::settings::read_all(data).map_err(|e| e.to_string())?;
    if assets.is_empty() {
        return Err("no hero-settings asset was readable".into());
    }

    for (asset, floats) in &assets {
        println!(
            "settings asset 0x{asset:016x} (index {}) -- {} float settings",
            guid::index_of(*asset),
            floats.len()
        );

        let defaulted = floats.iter().filter(|f| f.bounds_are_defaulted()).count();
        println!(
            "  {defaulted} of {} leave min/max at the class default",
            floats.len()
        );
        let tuned = floats
            .iter()
            .filter(|f| (f.default - 1.0).abs() > 1e-6)
            .count();
        println!("  {tuned} have a default other than 1.0");
        println!();

        println!(
            "{:>5}  {:>9}  {:>9}  {:>9}  {:>8}  {:<18}  bounds",
            "inst", "default", "min", "max", "step", "identifier"
        );
        for f in floats.iter().take(limit) {
            let bounds = if f.bounds_are_defaulted() {
                "defaulted"
            } else if f.explicit[1] && f.explicit[2] {
                "authored"
            } else {
                "partial"
            };
            println!(
                "{:>5}  {:>9.4}  {:>9.4}  {:>9.4}  {:>8.4}  0x{:016x}  {bounds}",
                f.instance,
                f.default,
                f.min,
                f.max,
                f.step,
                f.identifier.unwrap_or(0)
            );
        }
        if floats.len() > limit {
            println!("... {} more", floats.len() - limit);
        }
    }
    Ok(())
}

/// Diagnostic: what the bucket index claims, before anything else can fail.
fn buckets(install_root: &str) -> Result<(), String> {
    let install = ow_casc::Install::open(install_root).map_err(|e| e.to_string())?;
    let store = ow_casc::Store::open(install).map_err(|e| e.to_string())?;
    let install = store.install();
    println!("index keys   {}", store.key_count());

    for key_name in ["encoding", "root"] {
        let Some(values) = install.build_config.get(key_name) else {
            println!("{key_name}: absent from build config");
            continue;
        };
        for (slot, value) in values.iter().enumerate() {
            if value.len() != 32 {
                continue;
            }
            let mut bytes = [0u8; 16];
            for (i, b) in bytes.iter_mut().enumerate() {
                *b = u8::from_str_radix(&value[i * 2..i * 2 + 2], 16).map_err(|e| e.to_string())?;
            }
            match store.location_of(&bytes) {
                Some(location) => {
                    let path = install
                        .data_dir()
                        .join(format!("data.{:03}", location.archive));
                    let file_len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    let fits = location.offset + u64::from(location.size) <= file_len;
                    println!(
                        "{key_name}[{slot}] {} -> archive {} offset 0x{:x} size {} \
                         (file {} bytes, fits={fits})",
                        &value[..16],
                        location.archive,
                        location.offset,
                        location.size,
                        file_len
                    );
                }
                None => println!("{key_name}[{slot}] {} -> not in the bucket index", &value[..16]),
            }
        }
    }
    Ok(())
}

fn info(data: &GameData) -> Result<(), String> {
    let install = data.store().install();
    println!("version      {}", install.build_info.version);
    println!(
        "branch       {} (product {})",
        install.build_info.branch, install.build_info.product
    );
    let build_name = install
        .build_config
        .get("build-name")
        .and_then(|v| v.first())
        .cloned()
        .unwrap_or_default();
    println!("build name   {build_name}");
    let _ = data.registry().header.build_version;
    println!("index keys   {}", data.store().key_count());
    println!("keyring keys {}", install.keyring.len());
    println!(
        "registry     {} records from {} manifest(s)",
        data.registry().records.len(),
        data.manifests.len()
    );
    for name in &data.manifests {
        println!("  {name}");
    }
    Ok(())
}

fn type_histogram(data: &GameData) -> Result<(), String> {
    let histogram = data.type_histogram();
    println!("{} distinct asset types", histogram.len());
    let mut rows: Vec<(u16, usize)> = histogram.iter().map(|(k, v)| (*k, *v)).collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1));

    println!("\ntop 12:");
    for (type_id, count) in rows.iter().take(12) {
        println!("  type 0x{type_id:03X}  {count:>8}");
    }

    println!("\ncombat-relevant:");
    for type_id in [
        types::TEXTURE,
        types::MODEL,
        types::STU_GENERIC_SETTINGS,
        types::HERO,
        types::LOADOUT,
        types::GAME_MODE,
        types::TALENT,
        types::LOOTBOX,
    ] {
        let count = histogram.get(&type_id).copied().unwrap_or(0);
        let label = types::label(type_id).unwrap_or("unknown");
        println!("  type 0x{type_id:03X}  {count:>8}  {label}");
    }
    Ok(())
}

/// Hero GUIDs sit on the fixed `0x02E00000` prefix; index 2 is the first.
fn hero_guids(data: &GameData) -> Vec<u64> {
    let mut guids = data.guids_of_type(types::HERO);
    guids.sort_unstable();
    guids
}

fn heroes(data: &GameData) -> Result<(), String> {
    let guids = hero_guids(data);
    println!("{} heroes", guids.len());
    println!(
        "{:>6}  {:<18}  {:<18}  {:<8}  colour",
        "index", "guid", "gameplay entity", "entity#"
    );

    let mut resolved = 0usize;
    let mut unreadable: Vec<(u32, String)> = Vec::new();
    for guid in &guids {
        // One asset that cannot be opened must not hide the rest: a few are
        // encrypted with keys the installation's own keyring does not carry.
        let mut stu = match data.stu_of(*guid) {
            Ok(stu) => stu,
            Err(e) => {
                unreadable.push((guid::index_of(*guid), e.to_string()));
                continue;
            }
        };
        let Some(hero) = stu.instance(classes::HERO) else {
            continue;
        };
        let entity = stu.read_asset_ref(hero, classes::hero::GAMEPLAY_ENTITY);
        let colour = stu.read_rgba(hero, classes::hero::HERO_COLOR);
        let in_registry = entity
            .map(|e| data.registry().get(e).is_some())
            .unwrap_or(false);
        if in_registry {
            resolved += 1;
        }
        println!(
            "{:>6}  0x{:016x}  0x{:016x}  {:<8}  {}",
            guid::index_of(*guid),
            guid,
            entity.unwrap_or(0),
            entity.map(guid::index_of).unwrap_or(0),
            colour
                .map(|c| format!("({:.3}, {:.3}, {:.3}, {:.3})", c[0], c[1], c[2], c[3]))
                .unwrap_or_else(|| "-".into())
        );
    }

    println!(
        "\n{resolved} of {} hero references resolve to a registered asset",
        guids.len()
    );
    if !unreadable.is_empty() {
        println!(
            "{} hero asset(s) could not be opened (set OW_SUPPORT_KEYRING to a \
             community keyring file to read them):",
            unreadable.len()
        );
        for (index, why) in &unreadable {
            println!("  index {index}: {why}");
        }
    }
    Ok(())
}

fn hero_detail(data: &GameData, index: u32) -> Result<(), String> {
    let guid = hero_guids(data)
        .into_iter()
        .find(|g| guid::index_of(*g) == index)
        .ok_or_else(|| format!("no hero with index {index}"))?;

    let bytes = data.bytes_of(guid).map_err(|e| e.to_string())?;
    let mut stu = data.stu_of(guid).map_err(|e| e.to_string())?;

    println!("hero         0x{guid:016x} (index {index})");
    println!("stored size  {} bytes", bytes.len());
    println!("instances    {}", stu.instances.len());
    for (i, info) in stu.instances.iter().enumerate() {
        println!(
            "  [{i}] {} size={} embedded_by={}",
            data.classes.describe(info.hash),
            info.size,
            info.embedder_index
        );
    }

    let Some(hero) = stu.instance(classes::HERO) else {
        return Err("no STUHero instance in this asset".into());
    };

    let fields = stu.fields(hero).to_vec();
    println!("\nSTUHero fields ({}):", fields.len());
    for field in fields.iter().take(60) {
        println!(
            "  0x{:08X} offset={:>5} size={:>5}",
            field.hash, field.offset, field.size
        );
    }

    println!();
    if let Some(entity) = stu.read_asset_ref(hero, classes::hero::GAMEPLAY_ENTITY) {
        let kind = guid::type_of(entity);
        let label = types::label(kind).unwrap_or("unmapped type");
        let present = data.registry().get(entity).is_some();
        println!("gameplay entity 0x{entity:016x} (type 0x{kind:03X} {label}, registry={present})");
    }
    if let Some(colour) = stu.read_rgba(hero, classes::hero::HERO_COLOR) {
        println!(
            "colour          ({:.4}, {:.4}, {:.4}, {:.4})",
            colour[0], colour[1], colour[2], colour[3]
        );
    }
    if let Some(size) = stu.read_u32(hero, classes::hero::HERO_SIZE) {
        println!("hero size enum  {size}");
    }
    if let Some(gender) = stu.read_u32(hero, classes::hero::GENDER) {
        println!("gender enum     {gender}");
    }
    Ok(())
}

/// Per-hero health pools, read from each hero's gameplay entity schema.
///
/// This is the first base combat data: not a tuning multiplier but the pool the
/// hero actually spawns with. Armour and shields are absent for heroes that have
/// none, which is a real zero rather than a failed reading.
fn health(data: &GameData) -> Result<(), String> {
    use ow_tank::entity::{pool, HealthPools};

    println!(
        "{:>6}  {:>7}  {:>7}  {:>8}  {:>7}",
        "index", "health", "armour", "shields", "total"
    );

    let mut read = 0usize;
    let mut missing = 0usize;
    let mut totals: Vec<i32> = Vec::new();

    for guid in hero_guids(data) {
        let entity = {
            let mut stu = match data.stu_of(guid) {
                Ok(stu) => stu,
                Err(_) => {
                    missing += 1;
                    continue;
                }
            };
            let Some(hero) = stu.instance(classes::HERO) else {
                missing += 1;
                continue;
            };
            match stu.read_asset_ref(hero, classes::hero::GAMEPLAY_ENTITY) {
                Some(entity) => entity,
                None => {
                    missing += 1;
                    continue;
                }
            }
        };

        let entries = match ow_tank::entity::schema_of(data, entity) {
            Ok(entries) => entries,
            Err(_) => {
                missing += 1;
                continue;
            }
        };
        let show = |token: u32| {
            entries
                .get(&token)
                .and_then(|v| v.as_i32())
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".to_string())
        };
        let pools = HealthPools {
            health: entries.get(&pool::HEALTH).and_then(|v| v.as_i32()),
            armour: entries.get(&pool::ARMOUR).and_then(|v| v.as_i32()),
            shields: entries.get(&pool::SHIELDS).and_then(|v| v.as_i32()),
        };
        if let Some(total) = pools.total() {
            totals.push(total);
        }
        read += 1;

        println!(
            "{:>6}  {:>7}  {:>7}  {:>8}  {:>7}",
            guid::index_of(guid),
            show(pool::HEALTH),
            show(pool::ARMOUR),
            show(pool::SHIELDS),
            pools
                .total()
                .map(|t| t.to_string())
                .unwrap_or_else(|| "-".into())
        );
    }

    println!("\n{read} heroes read, {missing} not readable");
    if !totals.is_empty() {
        totals.sort_unstable();
        println!(
            "totals min {} max {} median {}",
            totals.first().unwrap(),
            totals.last().unwrap(),
            totals[totals.len() / 2]
        );
    }
    Ok(())
}

/// Each hero's weapons, and the state-script graph each one uses.
///
/// The graph is where a weapon's numbers live, but a graph is a program rather
/// than a record, so this prints the graph reference and checks it resolves.
/// Reading damage and fire rate out of the graph is a separate problem.
fn weapons(data: &GameData) -> Result<(), String> {
    println!(
        "{:>6}  {:>5}  {:<18}  {:>7}  {}",
        "index", "slot", "graph", "type", "present"
    );

    let mut heroes_with_weapons = 0usize;
    let mut graphs = 0usize;
    let mut present = 0usize;
    let mut empty_slots = 0usize;
    let mut unreadable = 0usize;

    for guid in hero_guids(data) {
        let entity = {
            let mut stu = match data.stu_of(guid) {
                Ok(stu) => stu,
                Err(_) => {
                    unreadable += 1;
                    continue;
                }
            };
            let Some(hero) = stu.instance(classes::HERO) else {
                unreadable += 1;
                continue;
            };
            match stu.read_asset_ref(hero, classes::hero::GAMEPLAY_ENTITY) {
                Some(entity) => entity,
                None => {
                    unreadable += 1;
                    continue;
                }
            }
        };

        let weapons = match ow_tank::weapons::weapons_of(data, entity) {
            Ok(weapons) => weapons,
            Err(_) => {
                unreadable += 1;
                continue;
            }
        };
        if weapons.is_empty() {
            continue;
        }
        heroes_with_weapons += 1;

        for weapon in &weapons {
            if weapon.is_empty() {
                empty_slots += 1;
                continue;
            }
            if let Some(graph) = weapon.graph {
                graphs += 1;
                let ok = ow_tank::weapons::graph_is_present(data, graph);
                if ok {
                    present += 1;
                }
                println!(
                    "{:>6}  {:>5}  0x{graph:016x}  {:>7}  {ok}",
                    guid::index_of(guid),
                    weapon.slot,
                    format!("0x{:03X}", guid::type_of(graph))
                );
            }
        }
    }

    println!(
        "\n{heroes_with_weapons} heroes with weapons, {graphs} graph references \
         ({present} present), {empty_slots} empty slots, {unreadable} heroes unreadable"
    );
    Ok(())
}

/// Named parameters inside each hero's weapon graphs.
///
/// A literal is only meaningful once the field that owns it is named, so this
/// joins every embedded literal in a graph to its owning field and keeps only
/// the ones the reference types name. Anything still unnamed is dropped rather
/// than printed as a hash, because a hash is not an answer.
fn weapon_params(data: &GameData, rest: &[String]) -> Result<(), String> {
    if data.fields.is_empty() {
        return Err("no field names available; set OW_OWLIB to a checkout of the \
                    reference types so literals can be named"
            .into());
    }
    let only: Option<u32> = rest.first().and_then(|s| s.parse().ok());

    println!(
        "{} field names loaded; scanning weapon graphs",
        data.fields.len()
    );
    let mut named_total = 0usize;
    let mut graphs = 0usize;
    let mut heroes = 0usize;

    for guid in hero_guids(data) {
        let index = guid::index_of(guid);
        if let Some(want) = only {
            if index != want {
                continue;
            }
        }
        let entity = {
            let mut stu = match data.stu_of(guid) {
                Ok(stu) => stu,
                Err(_) => continue,
            };
            let Some(hero) = stu.instance(classes::HERO) else {
                continue;
            };
            match stu.read_asset_ref(hero, classes::hero::GAMEPLAY_ENTITY) {
                Some(entity) => entity,
                None => continue,
            }
        };
        let weapons = match ow_tank::weapons::weapons_of(data, entity) {
            Ok(weapons) => weapons,
            Err(_) => continue,
        };

        let mut hero_printed = false;
        for weapon in &weapons {
            let Some(graph) = weapon.graph else {
                continue;
            };
            graphs += 1;
            let params = match ow_tank::graph::params_for_graph(data, graph) {
                Ok(params) => params,
                Err(_) => continue,
            };
            if params.is_empty() {
                continue;
            }
            if !hero_printed {
                println!("\n=== hero {index}");
                hero_printed = true;
                heroes += 1;
            }
            println!("  slot {} graph 0x{graph:016x}", weapon.slot);
            let mut seen = std::collections::BTreeSet::new();
            for param in &params {
                let rendered = match param.value {
                    ow_tank::entity::Value::Float(v) => format!("{v}"),
                    ow_tank::entity::Value::Int(v) => format!("{v}"),
                    ow_tank::entity::Value::Bool(v) => format!("{v}"),
                    ow_tank::entity::Value::Unread(_) => continue,
                };
                // The same parameter repeats across a graph's states; collapse
                // identical readings so the output is readable.
                let key = format!("{} = {}", param.describe(), rendered);
                if !seen.insert(key.clone()) {
                    continue;
                }
                named_total += 1;
                println!("      {key}");
            }
        }
    }

    println!("\n{heroes} heroes, {graphs} graphs, {named_total} named parameters");
    Ok(())
}

//! Follow a map placement's `look` reference, and see whether it reaches a
//! texture.
//!
//! The MDLC material u64s do not resolve as assets (`docs/formats/materials.md`
//! records that measurement), so the map side has to answer the question
//! instead: a placeable carries a *second* asset reference next to its model,
//! tentatively called `look`, and a map's own material override would live
//! there. This walks it.
//!
//! ```text
//! ow-tank-look --census 40         # how many placements name a look, and what
//! ow-tank-look --walk 0x…          # follow one asset's references downward
//! ow-tank-look --why 0x…           # every step of the read chain, named
//! ow-tank-look --cdn-fetch 0x…     # as --why, with OW_CDN=1 set first
//! ow-tank-look --cdn-key 0x…      # one raw 16-byte key: route, then read
//! ```
//!
//! Read-only: opens the install the same way the rest of the tree does. With
//! `--cdn-fetch`, the run may *download* assets from Blizzard's CDN (writes
//! only under `target/cdn-cache`), never the game install.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use ow_tank::{guid, GameData, ManifestSelector};

fn type_name(t: u16) -> &'static str {
    match t {
        guid::types::TEXTURE => "TEXTURE",
        guid::types::MODEL => "MODEL",
        guid::types::TEX_PAYLOAD => "tex-payload",
        0x008 => "material?",
        0x00D => "?00D",
        0x058 => "graph",
        0x040 => "entity",
        0x03F => "statescript",
        0x0BC => "map-chunk",
        0x0B3 => "material-data?",
        _ => "?",
    }
}

fn hex16(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|a| a == "--help") {
        eprintln!(
            "usage: ow-tank-look (--census [N] | --walk 0xGUID [--depth N] \
             | --why 0xGUID | --cdn-fetch 0xGUID | --cdn-key 0xKEY)"
        );
        return std::process::ExitCode::from(2);
    }
    let flag = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };

    // `--cdn-fetch` is `--why` with fetching on for the whole run: the CDN
    // client is built inside `Install::open`, so the switch must be in the
    // environment *before* GameData::open.
    if args.iter().any(|a| a == "--cdn-fetch") {
        std::env::set_var("OW_CDN", "1");
    }

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
    let resolves = |g: u64| data.registry().get(g).is_some();

    // A raw hexdump with every 8-byte-aligned qword's type decoded: the "print
    // both candidates" tool for an asset whose shape does not match the reader
    // (a material that is not a STU, a header that is not 32 bytes).
    if let Some(g) = flag("--hex") {
        let Ok(guid) = u64::from_str_radix(g.trim_start_matches("0x"), 16) else {
            eprintln!("--hex wants a hex GUID");
            return std::process::ExitCode::from(2);
        };
        let limit: usize = flag("--len").and_then(|v| v.parse().ok()).unwrap_or(0x100);
        let bytes = match data.bytes_of(guid) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("0x{guid:016x}: {e}");
                return std::process::ExitCode::from(1);
            }
        };
        println!(
            "0x{guid:016x} type 0x{:03x}: {} bytes",
            ow_tank::guid::type_of(guid),
            bytes.len()
        );
        for (i, chunk) in bytes.chunks(16).take(limit.max(16) / 16).enumerate() {
            let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
            let mut kinds = Vec::new();
            for j in (0..16).step_by(8) {
                let Some(slice) = chunk.get(j..j + 8) else {
                    continue;
                };
                let v = u64::from_le_bytes(slice.try_into().unwrap());
                let t = ow_tank::guid::type_of(v);
                if t != 1 && t != 0x801 && t != 0x201 {
                    kinds.push(format!(
                        "+{:#x}: type 0x{t:03x} idx {}",
                        i + j,
                        ow_tank::guid::index_of(v)
                    ));
                }
            }
            println!("  {:#06x}  {}   {}", i, hex.join(" "), kinds.join(", "));
        }
        return std::process::ExitCode::SUCCESS;
    }

    // Scan every 8-byte-aligned qword of an asset for *resolvable asset
    // references*: the trick that found a map placement's look, applied to a
    // container this project cannot parse yet (the hero's `0x0D4` skin prefabs
    // are `EFCT`/`ECMP` chunks, not STU).
    if let Some(g) = flag("--scan") {
        let Ok(guid) = u64::from_str_radix(g.trim_start_matches("0x"), 16) else {
            eprintln!("--scan wants a hex GUID");
            return std::process::ExitCode::from(2);
        };
        let bytes = match data.bytes_of(guid) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("0x{guid:016x}: {e}");
                return std::process::ExitCode::from(1);
            }
        };
        let only =
            flag("--type").and_then(|v| u16::from_str_radix(v.trim_start_matches("0x"), 16).ok());
        let mut found: Vec<(usize, u64, u16)> = Vec::new();
        for at in (0..bytes.len().saturating_sub(8)).step_by(8) {
            let v = u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
            let t = ow_tank::guid::type_of(v);
            if let Some(want) = only {
                if t != want {
                    continue;
                }
            } else if guid::types::label(t).is_none() {
                continue;
            }
            if resolves(v) {
                found.push((at, v, t));
            }
        }
        println!(
            "0x{guid:016x} type 0x{:03x}: {} B, {} resolvable reference(s)",
            ow_tank::guid::type_of(guid),
            bytes.len(),
            found.len()
        );
        let mut seen = std::collections::BTreeSet::new();
        for (at, v, t) in &found {
            let line = format!("  +{at:#06x} 0x{v:016x} type 0x{t:03x}");
            println!("{line}");
            seen.insert(*t);
        }
        println!("  types seen: {seen:?}");
        return std::process::ExitCode::SUCCESS;
    }

    // Every manifest the root file names, with its keys: the list the client
    // chooses from (content, text, speech, resource graph *and* asset packages).
    if args.iter().any(|a| a == "--root") {
        let filter = flag("--filter").unwrap_or_default().to_lowercase();
        match data.store().root_file() {
            Ok(text) => {
                let mut shown = 0usize;
                for line in text.lines() {
                    if line.starts_with('#') || line.trim().is_empty() {
                        continue;
                    }
                    if !filter.is_empty() && !line.to_lowercase().contains(&filter) {
                        continue;
                    }
                    let fields: Vec<&str> = line.split('|').map(str::trim).collect();
                    if fields.len() < 7 {
                        continue;
                    }
                    println!("{:<52} ckey {} {} B", fields[5], fields[1], fields[3]);
                    shown += 1;
                    if shown >= 40 {
                        println!("  … (stopped at 40)");
                        break;
                    }
                }
                println!("{shown} entry/entries shown (filter {filter:?})");
            }
            Err(e) => println!("root file: {e}"),
        }
        return std::process::ExitCode::SUCCESS;
    }

    if args.iter().any(|a| a == "--index-stats") {
        let _ = data.registry();
        let (buckets, from_archives, added) = ow_casc::container::index_stats();
        println!(
            "index: {buckets} bucket entries, {from_archives} read from per-archive indexes,              {added} of them new (keys the bucket tables did not have); {} total",
            data.store().key_count()
        );
        for line in ow_casc::container::shmem_report() {
            println!("  shmem: {line}");
        }
        return std::process::ExitCode::SUCCESS;
    }

    // One raw 16-byte key through the store: the route first (which index
    // knows it, where the bytes would come from), then the actual read. The
    // tool for keys that are not GUIDs — an encoding key, a group entry.
    // Without --cdn-fetch the read is local-only; with it, the store may fetch.
    if let Some(k) = flag("--cdn-key") {
        let k = k.trim_start_matches("0x").to_lowercase();
        let mut key = [0u8; 16];
        let parsed = k.len() == 32
            && k.as_bytes().chunks(2).enumerate().all(|(i, pair)| {
                match std::str::from_utf8(pair)
                    .ok()
                    .and_then(|p| u8::from_str_radix(p, 16).ok())
                {
                    Some(b) => {
                        key[i] = b;
                        true
                    }
                    None => false,
                }
            });
        if !parsed {
            eprintln!("--cdn-key wants 32 hex digits");
            return std::process::ExitCode::from(2);
        }
        match data.store().cdn_route_of(&key) {
            Some(route) => println!("key {k}: CDN route: {route}"),
            None => {
                println!("key {k}: in no local index (neither group, nor patch group, nor loose)")
            }
        }
        match data.store().open_ekey(&key) {
            Ok(b) => println!(
                "read: {} B, first 4 {:?}",
                b.len(),
                String::from_utf8_lossy(&b[..b.len().min(4)])
            ),
            Err(e) => println!("read failed: {e}"),
        }
        return std::process::ExitCode::SUCCESS;
    }

    // Why an asset cannot be read: every step of the chain, named.
    if let Some(g) = flag("--why").or_else(|| flag("--cdn-fetch")) {
        let Ok(guid) = u64::from_str_radix(g.trim_start_matches("0x"), 16) else {
            eprintln!("--why wants a hex GUID");
            return std::process::ExitCode::from(2);
        };
        let Some(record) = data.registry().get(guid) else {
            println!(
                "0x{guid:016x}: not in the root manifest — the installation never registered it"
            );
            return std::process::ExitCode::SUCCESS;
        };
        println!(
            "0x{guid:016x} type 0x{:03x}: in the root manifest, declared {} B, content key {}",
            ow_tank::guid::type_of(guid),
            record.size,
            hex16(&record.content_key)
        );
        for line in data.store().encoding_report() {
            println!("  encoding manifest: {line}");
        }
        // Does any *other* asset share this content key? If one of them reads,
        // then the key is resolvable after all and this asset's failure is our
        // resolution, not the install.
        {
            let same: Vec<u64> = data
                .registry()
                .records
                .iter()
                .filter(|r| r.content_key == record.content_key && r.guid != guid)
                .map(|r| r.guid)
                .take(4)
                .collect();
            match same.len() {
                0 => println!("  no other asset shares this content key"),
                n => {
                    println!("  {n} other asset(s) share this content key:");
                    for g in same {
                        println!("    0x{g:016x} type 0x{:03x}", ow_tank::guid::type_of(g));
                    }
                }
            }
        }
        // Is the *content* key itself an index key? If the modern archive
        // indexes were content-keyed for some archives, the file would be
        // readable with no encoding entry at all.
        match data.store().location_of(&record.content_key) {
            Some(l) => println!(
                "  content key as an index key: FOUND -> archive data.{:03} at {:#x}, {} B",
                l.archive, l.offset, l.size
            ),
            None => println!("  content key as an index key: not indexed"),
        }
        match data.store().cdn_route_of(&record.content_key) {
            Some(route) => println!("  CDN route for the content key: {route}"),
            None => println!(
                "  CDN route for the content key: in no local index \
                 (neither the group nor the loose file index)"
            ),
        }
        let raw = data.store().encoding_blob_contains(&record.content_key);
        println!(
            "  raw byte search in the decoded encoding manifest(s): {}",
            if raw.is_empty() {
                "absent".to_string()
            } else {
                format!("present in manifest(s) {raw:?} — the page lookup is what misses")
            }
        );
        match data.store().ekeys_per_manifest(&record.content_key) {
            Ok(per_manifest) => {
                for (i, ekeys) in per_manifest.iter().enumerate() {
                    println!("  encoding manifest {i}: {} ekey(s)", ekeys.len());
                    for ekey in ekeys {
                        let location = data.store().location_of(ekey);
                        match location {
                            Some(l) => println!(
                                "    ekey {} -> local: archive data.{:03} at {:#x}, {} B",
                                hex16(ekey),
                                l.archive,
                                l.offset,
                                l.size
                            ),
                            None => match data.store().cdn_route_of(ekey) {
                                Some(route) => {
                                    println!("    ekey {} -> not local; CDN: {route}", hex16(ekey))
                                }
                                None => println!(
                                    "    ekey {} -> NOT in the local bucket index and NOT in \
                                     the CDN indexes either (group, patch group, loose)",
                                    hex16(ekey)
                                ),
                            },
                        }
                    }
                }
            }
            Err(e) => println!("  encoding lookup failed: {e}"),
        }
        for key in ["install", "download"] {
            match data.store().find_ckey_in(key, &record.content_key) {
                Ok(hits) if hits.is_empty() => {
                    println!("  {key} manifest: content key not found")
                }
                Ok(hits) => {
                    for (which, ekey) in hits {
                        let local = match data.store().location_of(&ekey) {
                            Some(l) => format!(
                                "local: archive data.{:03} at {:#x}, {} B",
                                l.archive, l.offset, l.size
                            ),
                            None => "NOT in the local bucket index".to_string(),
                        };
                        println!(
                            "  {key} manifest #{which}: ekey {} -> {local}",
                            hex16(&ekey)
                        );
                    }
                }
                Err(e) => println!("  {key} manifest: {e}"),
            }
        }
        return std::process::ExitCode::SUCCESS;
    }

    if let Some(g) = flag("--walk") {
        let Ok(start) = u64::from_str_radix(g.trim_start_matches("0x"), 16) else {
            eprintln!("--walk wants a hex GUID");
            return std::process::ExitCode::from(2);
        };
        let depth: usize = flag("--depth").and_then(|v| v.parse().ok()).unwrap_or(3);
        walk(&data, &resolves, start, depth);
        return std::process::ExitCode::SUCCESS;
    }

    // Dump a model's MDLC payload: the material array, and every aligned u32
    // run nearby, so the slot-id array's offset is read rather than guessed.
    if let Some(g) = flag("--mdlc") {
        let Ok(g) = u64::from_str_radix(g.trim_start_matches("0x"), 16) else {
            eprintln!("--mdlc wants a hex GUID");
            return std::process::ExitCode::from(2);
        };
        let Ok(bytes) = data.bytes_of(g) else {
            eprintln!("model {g:#x} is not readable");
            return std::process::ExitCode::from(1);
        };
        let loader = |x: u64| data.bytes_of(x).ok();
        let model = match ow_tank::model::parse(&bytes, &loader) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("parse failed: {e}");
                return std::process::ExitCode::from(1);
            }
        };
        println!(
            "model 0x{g:016x}: {} u64 material(s), {} mesh(es)",
            model.materials.len(),
            model.meshes.len()
        );
        for (i, m) in model.materials.iter().enumerate() {
            println!(
                "  [{i}] 0x{m:016x}  low32 0x{:08x} hi32 0x{:08x}",
                *m as u32,
                m >> 32
            );
        }
        for (i, mesh) in model.meshes.iter().enumerate() {
            println!(
                "  mesh {i}: {} verts, {} tris, material index {}, lod {}",
                mesh.positions.len(),
                mesh.indices.len() / 3,
                mesh.material,
                mesh.lod
            );
        }
        let chunks = ow_tank::chunks::Chunks::parse(&bytes).expect("chunked");
        if let Some(mdlc) = chunks.payload(&bytes, b"MDLC") {
            println!("MDLC payload {} B", mdlc.len());
            let base = bytes.len() - mdlc.len();
            println!("  (payload starts at file offset {base:#x})");
            for row in 0..(mdlc.len() / 16) {
                let at = row * 16;
                let words: Vec<String> = (0..4)
                    .map(|i| {
                        u32::from_le_bytes(mdlc[at + i * 4..at + i * 4 + 4].try_into().unwrap())
                            .to_string()
                    })
                    .collect();
                println!("  +{at:#05x}: {}", words.join(", "));
            }
        }
        return std::process::ExitCode::SUCCESS;
    }

    // The decisive experiment: for each placement, does the model's material
    // slot list meet the look's material ids? If the u32 array in the MDLC and
    // STUModelMaterial's MaterialID are the same id space, the join is real.
    if let Some(n) = flag("--pairs")
        .map(|v| v.parse::<usize>().unwrap_or(6))
        .or(Some(0))
        .filter(|n| *n > 0)
    {
        let guids = data.guids_of_type(0x0BC);
        let mut shown = 0usize;
        for g in guids.iter() {
            if shown >= n {
                break;
            }
            let Ok(bytes) = data.bytes_of(*g) else {
                continue;
            };
            let Ok((kind, chunk)) = ow_tank::mapchunk::MapChunk::detect(&bytes, &resolves) else {
                continue;
            };
            if kind != ow_tank::mapchunk::Kind::Model {
                continue;
            }
            for p in &chunk.placeables {
                if shown >= n {
                    break;
                }
                let (Some(model_g), Some(look_g)) = (p.model, p.look) else {
                    continue;
                };
                println!("\n== placement {} of 0x{g:016x}", p.index);
                println!("  model 0x{model_g:016x}   look 0x{look_g:016x}");
                match data.bytes_of(model_g) {
                    Err(e) => println!("  model unreadable: {e}"),
                    Ok(mb) => {
                        let loader = |x: u64| data.bytes_of(x).ok();
                        match ow_tank::model::parse(&mb, &loader) {
                            Err(e) => println!("  model parse: {e}"),
                            Ok(model) => {
                                let idx: Vec<usize> =
                                    model.meshes.iter().map(|m| m.material).collect();
                                println!(
                                    "  model: {} material(s), {} mesh(es), mesh material indices {:?}",
                                    model.materials.len(),
                                    model.meshes.len(),
                                    idx
                                );
                                for (i, m) in model.materials.iter().enumerate() {
                                    println!("     [{i}] 0x{m:016x}");
                                }
                            }
                        }
                    }
                }
                match ow_tank::material::materials_of_look(&data, look_g) {
                    Err(e) => println!("  look: {e}"),
                    Ok(mats) => {
                        println!("  look: {} material(s)", mats.len());
                        for m in &mats {
                            let textures =
                                ow_tank::material::textures_of_material(&data, m.material)
                                    .unwrap_or_default();
                            println!(
                                "     material 0x{:016x} (type 0x{:03x}) id 0x{:016x} (low32 {}) \
                                 -> {} texture(s) {:016x?}",
                                m.material,
                                guid::type_of(m.material),
                                m.id,
                                m.id as u32,
                                textures.len(),
                                textures
                            );
                        }
                    }
                }
                shown += 1;
            }
        }
        return std::process::ExitCode::SUCCESS;
    }

    // The join, counted: how often does a model's MDLC material id appear among
    // its look's material ids? That is the whole basis for texturing a prop.
    if let Some(n) = flag("--join").map(|v| v.parse::<usize>().unwrap_or(200)) {
        let guids = data.guids_of_type(0x0BC);
        let mut seen = 0usize;
        let mut looks_ok = 0usize;
        let mut models = 0usize;
        let mut by_index = 0usize;
        let mut by_set = 0usize;
        let mut no_ids = 0usize;
        let mut materials_resolve = 0usize;
        let mut materials_total = 0usize;
        let mut with_textures = 0usize;
        let mut textures_total = 0usize;
        'outer: for g in guids.iter() {
            let Ok(bytes) = data.bytes_of(*g) else {
                continue;
            };
            let Ok((kind, chunk)) = ow_tank::mapchunk::MapChunk::detect(&bytes, &resolves) else {
                continue;
            };
            if kind != ow_tank::mapchunk::Kind::Model {
                continue;
            }
            for p in &chunk.placeables {
                if seen >= n {
                    break 'outer;
                }
                let (Some(model_g), Some(look_g)) = (p.model, p.look) else {
                    continue;
                };
                seen += 1;
                let Ok(mb) = data.bytes_of(model_g) else {
                    continue;
                };
                let loader = |x: u64| data.bytes_of(x).ok();
                let Ok(model) = ow_tank::model::parse(&mb, &loader) else {
                    continue;
                };
                models += 1;
                let Ok(mats) = ow_tank::material::materials_of_look(&data, look_g) else {
                    continue;
                };
                looks_ok += 1;
                if mats.is_empty() {
                    no_ids += 1;
                    continue;
                }
                for (i, id) in model.materials.iter().enumerate() {
                    if mats.get(i).map(|m| m.id == *id).unwrap_or(false) {
                        by_index += 1;
                    }
                    if mats.iter().any(|m| m.id == *id) {
                        by_set += 1;
                    }
                }
                for m in &mats {
                    materials_total += 1;
                    if data.bytes_of(m.material).is_ok() {
                        materials_resolve += 1;
                        let t = ow_tank::material::textures_of_material(&data, m.material)
                            .unwrap_or_default();
                        if !t.is_empty() {
                            with_textures += 1;
                            textures_total += t.len();
                        }
                    }
                }
            }
        }
        println!(
            "{seen} placement(s): {models} model read, {looks_ok} look read, \
             {no_ids} look with no materials"
        );
        println!(
            "  model material ids matching the look's id AT THE SAME INDEX: {by_index}; \
             matching ANY of the look's ids: {by_set}"
        );
        println!(
            "  look materials: {materials_total} total, {materials_resolve} readable, \
             {with_textures} naming a texture ({textures_total} textures)"
        );
        return std::process::ExitCode::SUCCESS;
    }

    // One look asset, element by element, with the raw words of each element so
    // the field layout can be read off the bytes instead of argued about.
    if let Some(g) = flag("--look") {
        let Ok(g) = u64::from_str_radix(g.trim_start_matches("0x"), 16) else {
            eprintln!("--look wants a hex GUID");
            return std::process::ExitCode::from(2);
        };
        let stu = match data.stu_of(g) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("look {g:#x}: {e}");
                return std::process::ExitCode::from(1);
            }
        };
        let Some(instance) = stu.instance(ow_tank::material::MODEL_LOOK) else {
            println!("0x{g:016x} has no STUModelLook instance");
            return std::process::ExitCode::SUCCESS;
        };
        let base = stu.payload_start(instance);
        println!("look 0x{g:016x} instance {instance}, payload at {base:#x}");
        for (bag, at) in stu.inline_elements(instance, 0xbafdafba) {
            println!("  element bag {bag} at {at:#x}");
            for field in stu.layout_at(at, bag) {
                let raw = stu.u64_at(at + field.offset).unwrap_or(0);
                let raw2 = stu.u64_at(at + field.offset + 8).unwrap_or(0);
                let first = stu.asset_ref_at(at + field.offset, field.hash).unwrap_or(0);
                let second = stu
                    .asset_ref_at(at + field.offset + 8, field.hash)
                    .unwrap_or(0);
                println!(
                    "    +{:#04x} h 0x{:08x} len {}: raw 0x{raw:016x} 0x{raw2:016x}                      -> first half type 0x{:03x} idx {}, second half type 0x{:03x} idx {}",
                    field.offset,
                    field.hash,
                    field.size,
                    guid::type_of(first),
                    guid::index_of(first),
                    guid::type_of(second),
                    guid::index_of(second),
                );
            }
        }
        println!("  materials as the reader has them:");
        for m in ow_tank::material::materials_of_look(&data, g).unwrap_or_default() {
            println!(
                "    material 0x{:016x} (type 0x{:03x}) id 0x{:016x}",
                m.material,
                guid::type_of(m.material),
                m.id
            );
        }
        return std::process::ExitCode::SUCCESS;
    }

    // How much key material the install actually carries. The count decides
    // whether an unreadable asset means "not in this install" or "not in this
    // build's keyring at all".
    if args.iter().any(|a| a == "--keyring") {
        let install = match ow_casc::Install::open_with_support_keyring(
            &root,
            sources.support_keyring.as_deref(),
        ) {
            Ok(i) => i,
            Err(e) => {
                eprintln!("install: {e}");
                return std::process::ExitCode::from(1);
            }
        };
        println!(
            "build {} version {}; keyring {} entr(ies); support keyring {:?}",
            install.build_info.branch,
            install.build_info.version,
            install.keyring.len(),
            sources.support_keyring,
        );
        return std::process::ExitCode::SUCCESS;
    }

    let count: usize = args
        .iter()
        .position(|a| a == "--census")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(40);

    // How much of a given asset type is actually readable? A manifest entry is
    // not a decodable asset: the content key lives in the keytable, and a
    // missing key reads as "not present in this installation".
    if let Some(t) = flag("--probe-type") {
        let Ok(t) = u16::from_str_radix(t.trim_start_matches("0x"), 16) else {
            eprintln!("--probe-type wants a hex type id");
            return std::process::ExitCode::from(2);
        };
        let guids = data.guids_of_type(t);
        let sample = guids.len().min(count);
        // Spread the sample across the whole type rather than taking a prefix:
        // readability differs by package, and a prefix can sit entirely inside
        // one of them.
        let step = (guids.len() / sample.max(1)).max(1);
        let picked: Vec<u64> = guids.iter().copied().step_by(step).take(sample).collect();
        let mut ok = 0usize;
        let mut errors: BTreeMap<String, usize> = BTreeMap::new();
        let mut sizes: BTreeMap<usize, usize> = BTreeMap::new();
        let mut failing: Vec<u64> = Vec::new();
        for g in &picked {
            match data.bytes_of(*g) {
                Ok(b) => {
                    ok += 1;
                    *sizes.entry(b.len()).or_default() += 1;
                }
                Err(e) => {
                    let text = e.to_string();
                    let brief = text.split(" is not present").next().unwrap_or(&text);
                    *errors.entry(brief.to_string()).or_default() += 1;
                    if failing.len() < 3 {
                        failing.push(*g);
                    }
                }
            }
        }
        println!(
            "type 0x{t:03x} {}: {} asset(s) in the registry; {sample} tried (every {step}th):              {ok} readable",
            type_name(t),
            guids.len()
        );
        for (e, n) in &errors {
            println!("  {n:>6} error(s): {e}");
        }
        if !sizes.is_empty() {
            let mut v: Vec<(usize, usize)> = sizes.into_iter().collect();
            v.sort_by_key(|(n, _)| std::cmp::Reverse(*n));
            println!("  readable sizes (top 10): {:?}", &v[..v.len().min(10)]);
        }
        let readable: Vec<u64> = picked
            .iter()
            .copied()
            .filter(|g| data.bytes_of(*g).is_ok())
            .take(5)
            .collect();
        println!("  first readable: {readable:#016x?}");
        if failing.is_empty() {
            println!("  first failing: (none)");
        } else {
            let shown: Vec<String> = failing.iter().map(|g| format!("0x{g:016x}")).collect();
            println!("  first failing: {}", shown.join(" "));
        }
        return std::process::ExitCode::SUCCESS;
    }

    // Where inside a model placeable does a *resolvable* 8-byte reference sit?
    // Scanning every aligned qword of every placeable and tallying the offsets
    // that land on real assets finds the reference fields without guessing the
    // layout.
    if args.iter().any(|a| a == "--refs") {
        let guids = data.guids_of_type(0x0BC);
        let mut by_offset: BTreeMap<usize, (usize, BTreeMap<u16, usize>)> = BTreeMap::new();
        let mut placeable_shapes: BTreeMap<usize, usize> = BTreeMap::new();
        let mut scanned = 0usize;
        for g in guids.iter() {
            let Ok(bytes) = data.bytes_of(*g) else {
                continue;
            };
            let Ok((kind, chunk)) = ow_tank::mapchunk::MapChunk::detect(&bytes, &resolves) else {
                continue;
            };
            if kind != ow_tank::mapchunk::Kind::Model {
                continue;
            }
            scanned += 1;
            if scanned > count {
                break;
            }
            for p in &chunk.placeables {
                *placeable_shapes.entry(p.size).or_default() += 1;
                let body = p.offset + 0x10;
                let end = (p.offset + p.size).min(bytes.len());
                let mut at = 0usize;
                while body + at + 8 <= end {
                    let q = u64::from_le_bytes(bytes[body + at..body + at + 8].try_into().unwrap());
                    if q != 0 && resolves(q) {
                        let e = by_offset.entry(at).or_default();
                        e.0 += 1;
                        *e.1.entry(guid::type_of(q)).or_default() += 1;
                    }
                    at += 8;
                }
            }
        }
        println!("scanned {scanned} model chunk(s)");
        println!("placeable sizes: {placeable_shapes:?}");
        println!("8-byte references that resolve, by offset within the placeable body:");
        for (at, (n, types)) in &by_offset {
            let shown: Vec<String> = types
                .iter()
                .take(4)
                .map(|(t, c)| format!("0x{t:03x}×{c}"))
                .collect();
            println!("  +{at:#06x}  {n:>7}  {}", shown.join(" "));
        }
        return std::process::ExitCode::SUCCESS;
    }

    let guids = data.guids_of_type(0x0BC);
    let mut placeables = 0usize;
    let mut with_model = 0usize;
    let mut with_look = 0usize;
    let mut looks: BTreeMap<u64, usize> = BTreeMap::new();
    // Which field slots of a placeable carry a look at all.
    let mut slots: BTreeMap<usize, usize> = BTreeMap::new();
    let mut chunk_errors = 0usize;

    for guid_ in guids.iter().take(count) {
        let Ok(bytes) = data.bytes_of(*guid_) else {
            chunk_errors += 1;
            continue;
        };
        let Ok((_, chunk)) = ow_tank::mapchunk::MapChunk::detect(&bytes, &resolves) else {
            chunk_errors += 1;
            continue;
        };
        for p in &chunk.placeables {
            placeables += 1;
            // Which byte offset the look sits at, relative to the placeable,
            // so the layout is visible in the numbers rather than in prose.
            if let Some(l) = p.look {
                with_look += 1;
                *looks.entry(l).or_default() += 1;
                // Where in the placeable the reference sits, so the layout is
                // visible as a number rather than only in prose.
                let body = &bytes[p.offset..(p.offset + p.size).min(bytes.len())];
                let needle = l.to_le_bytes();
                if let Some(at) = body.windows(8).position(|w| w == needle) {
                    *slots.entry(at).or_default() += 1;
                }
            }
            if p.model.is_some() {
                with_model += 1;
            }
        }
    }

    println!(
        "{count} chunk(s) ({chunk_errors} unreadable): {placeables} placements, \
         {with_model} with a model, {with_look} with a look"
    );
    let mut by_type: BTreeMap<u16, usize> = BTreeMap::new();
    let mut resolvable = 0usize;
    for (g, n) in &looks {
        *by_type.entry(guid::type_of(*g)).or_default() += n;
        if resolves(*g) {
            resolvable += 1;
        }
    }
    println!(
        "{} distinct look GUID(s); {resolvable} of them resolve; by type:",
        looks.len()
    );
    for (t, n) in &by_type {
        println!("  type 0x{t:03x} {:<12} {n} placement(s)", type_name(*t));
    }
    println!("look byte offsets within the placeable:");
    for (at, n) in &slots {
        println!("  +{at:#06x}  {n}");
    }
    println!("the first 20 looks:");
    for (g, n) in looks.iter().take(20) {
        println!(
            "  0x{g:016x}  type 0x{:03x} {:<12} {n:>5}x  {}",
            guid::type_of(*g),
            type_name(guid::type_of(*g)),
            if resolves(*g) { "resolves" } else { "-" }
        );
    }
    std::process::ExitCode::SUCCESS
}

/// Breadth-first over an asset's outgoing references: STU fields whose 8-byte
/// value decodes as a resolvable asset, and a model's material u64s.
fn walk(data: &GameData, resolves: &dyn Fn(u64) -> bool, start: u64, depth: usize) {
    let mut queue: VecDeque<(u64, usize)> = VecDeque::new();
    let mut seen: BTreeSet<u64> = BTreeSet::new();
    let mut textures: BTreeSet<u64> = BTreeSet::new();
    queue.push_back((start, 0));
    seen.insert(start);

    while let Some((g, d)) = queue.pop_front() {
        let indent = "  ".repeat(d);
        let t = guid::type_of(g);
        let exists = data.registry().get(g).is_some();
        println!(
            "{indent}0x{g:016x} type 0x{t:03x} {:<12} {}",
            type_name(t),
            if exists { "" } else { "(not in the manifest)" }
        );
        if t == guid::types::TEXTURE {
            textures.insert(g);
        }
        if d >= depth || !exists {
            continue;
        }

        let mut refs: Vec<u64> = Vec::new();
        if t == guid::types::MODEL {
            if let Ok(bytes) = data.bytes_of(g) {
                let loader = |x: u64| data.bytes_of(x).ok();
                if let Ok(model) = ow_tank::model::parse(&bytes, &loader) {
                    refs.extend(model.materials.iter().copied());
                    for m in &model.materials {
                        let cd = *m;
                        println!(
                            "{indent}  (mdlc material 0x{cd:016x} low32 0x{:08x})",
                            cd as u32
                        );
                    }
                }
            }
        } else {
            match data.bytes_of(g) {
                Err(e) => println!("{indent}  bytes unavailable: {e}"),
                Ok(bytes) if bytes.len() < 8 => {
                    println!("{indent}  {} B (too short to read)", bytes.len())
                }
                Ok(bytes) => {
                    // What kind of file is it at all? A STU starts with the
                    // CRC64 the reader checks; anything else is a chunked blob
                    // and its first four bytes are a tag.
                    let head = &bytes[..bytes.len().min(64)];
                    let csum = u64::from_le_bytes(head[..8].try_into().unwrap());
                    println!(
                        "{indent}  {} B, first8 0x{csum:016x} first4 {:?}",
                        bytes.len(),
                        String::from_utf8_lossy(&head[..4])
                    );
                    match data.stu_of(g) {
                        Ok(mut stu) => {
                            let n = stu.instances.len();
                            println!("{indent}  STU with {n} instance(s)");
                            let verbose = std::env::args().any(|a| a == "--fields");
                            for i in 0..n {
                                if verbose {
                                    let hash = stu.instances[i].hash;
                                    let _base = stu.payload_start(i);
                                    println!("{indent}    [{i}] type 0x{hash:08x}");
                                }
                                let fields = stu.fields(i).to_vec();
                                for f in fields {
                                    let Some((at, len)) = stu.field_bytes(i, f.hash) else {
                                        continue;
                                    };
                                    if verbose {
                                        let base = stu.payload_start(i);
                                        let v = stu.u64_at(base + at).unwrap_or(0);
                                        println!(
                                            "{indent}      +{at:#06x} h 0x{:08x} len {len}: \
                                             0x{v:016x}{}",
                                            f.hash,
                                            if len == 8 && guid::type_of(v) != 0 {
                                                format!(
                                                    " type 0x{:03x} {}",
                                                    guid::type_of(v),
                                                    type_name(guid::type_of(v))
                                                )
                                            } else {
                                                String::new()
                                            }
                                        );
                                    }
                                    if len != 8 {
                                        continue;
                                    }
                                    let Some(v) = stu.read_asset_ref(i, f.hash) else {
                                        continue;
                                    };
                                    if v == 0 {
                                        continue;
                                    }
                                    let vt = guid::type_of(v);
                                    if resolves(v) || vt == guid::types::TEXTURE {
                                        refs.push(v);
                                    }
                                }
                            }
                        }
                        Err(e) => println!("{indent}  not a STU: {e}"),
                    }
                    if !refs.is_empty() {
                        let mut summary: BTreeMap<u16, usize> = BTreeMap::new();
                        for r in &refs {
                            *summary.entry(guid::type_of(*r)).or_default() += 1;
                        }
                        let shown: Vec<String> = summary
                            .iter()
                            .map(|(t, n)| format!("0x{t:03x} {name}×{n}", name = type_name(*t)))
                            .collect();
                        println!(
                            "{indent}  {} asset reference(s): {}",
                            refs.len(),
                            shown.join(", ")
                        );
                    }
                }
            }
        }

        for r in refs {
            if seen.insert(r) {
                queue.push_back((r, d + 1));
            }
        }
    }
    if textures.is_empty() {
        println!("no texture reached");
    } else {
        println!("{} texture(s) reached:", textures.len());
        for t in &textures {
            println!("  0x{t:016x}");
        }
    }
}

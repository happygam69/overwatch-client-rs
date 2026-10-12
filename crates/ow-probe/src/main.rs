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
        "first" => first_of_type(&data, rest),
        "guids" => guids_cmd(&data, rest),
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
        "materials" => materials(&data, rest),
        "mdl-values" => mdl_values(&data, rest),
        "resolve-material" => resolve_material(&data, rest),
        "size" => by_size(&data, rest),
        "raw" => raw(&data, rest),
        "package" => package(&data, rest),
        "graph" => graph(&data, rest),
        "near" => near(&data, rest),
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

/// Test whether MDLC material values are an asset GUID under a transform.
///
/// Three families are decided here, and none of them needs a known
/// (value, GUID) pair:
///
/// * **Permutation with a constant inside:** `value = P(guid ^ c)`.
/// * **Permutation with a constant outside:** `value = P(guid) ^ c`.
///   For the right `P` there is one `c` that maps *every* material value onto
///   *some* asset GUID. So for each `P`, `c` is derived from the first value
///   against every candidate GUID, and the second and third values must land
///   on real GUIDs under the same `c`. Three values decide: a wrong `P` leaves
///   each `c` a 64-bit needle in a haystack of 1.5 million.
/// * **A known hash of the GUID:** `value = H(guid)`. Each candidate `H` runs
///   over every material GUID and its outputs are intersected with the
///   observed values.
///
/// A hit prints the scheme, the permutation, the constant and a matched GUID
/// with its type, so the result can be re-derived rather than trusted.
///
/// ```text
/// ow-probe resolve-material 0xe91779d26205ab24
/// ow-probe resolve-material --values <mdl-values output>
/// ```
fn resolve_material(data: &GameData, rest: &[String]) -> Result<(), String> {
    // Values to explain: a file of mdl-values output, or one on the command
    // line. Fewer than three distinct values cannot decide anything -- with
    // one value every permutation "matches" its own invented constant -- so
    // the shortfall is made up from models in the registry.
    let mut values: Vec<u64> = Vec::new();
    if let Some(path) = rest
        .iter()
        .position(|a| a == "--values")
        .and_then(|i| rest.get(i + 1))
    {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        for line in text.lines() {
            for token in line.split_whitespace().skip(1) {
                if let Ok(v) = u64::from_str_radix(token.trim_start_matches("0x"), 16) {
                    if v != 0 {
                        values.push(v);
                    }
                }
            }
        }
    } else {
        let wanted = u64::from_str_radix(
            rest.first()
                .ok_or("usage: ow-probe resolve-material <value> | --values <file>")?
                .trim_start_matches("0x"),
            16,
        )
        .map_err(|_| "not a number".to_string())?;
        values.push(wanted);
    }
    values.sort_unstable();
    values.dedup();

    let mut borrowed = 0usize;
    if values.len() < 3 {
        'augment: for guid in data.guids_of_type(types::MODEL) {
            let Ok(bytes) = data.bytes_of(guid) else { continue };
            let Ok(chunks) = ow_tank::chunks::Chunks::parse(&bytes) else {
                continue;
            };
            let Some(mdlc) = chunks.payload(&bytes, b"MDLC") else {
                continue;
            };
            if mdlc.len() < 106 {
                continue;
            }
            let offset = i64::from_le_bytes(mdlc[8..16].try_into().unwrap());
            let count = u16::from_le_bytes(mdlc[104..106].try_into().unwrap());
            if offset <= 0 || offset as usize + count as usize * 8 > mdlc.len() {
                continue;
            }
            let base = offset as usize;
            for i in 0..count as usize {
                let v = u64::from_le_bytes(
                    mdlc[base + i * 8..base + i * 8 + 8].try_into().unwrap(),
                );
                if v != 0 {
                    values.push(v);
                }
            }
            values.sort_unstable();
            values.dedup();
            borrowed += 1;
            if values.len() >= 24 || (values.len() >= 3 && borrowed >= 32) {
                break 'augment;
            }
        }
    }
    if values.len() < 3 {
        return Err(
            "fewer than three distinct material values; this test cannot decide".to_string(),
        );
    }

    // Every GUID in the registry, as the confirmation set: a transform that
    // lands on any real asset is interesting even when the type is not the
    // one expected, and the type of the hit says what the scheme really is.
    let known: std::collections::HashSet<u64> =
        data.registry().records().iter().map(|r| r.guid).collect();
    let probes: Vec<u64> = values.iter().copied().take(3).collect();
    println!(
        "{} distinct value(s) ({} borrowed from registry models), {} GUID(s) to confirm against",
        values.len(),
        borrowed,
        known.len()
    );

    struct Perm {
        name: &'static str,
        fwd: Box<dyn Fn(u64) -> u64>,
        inv: Box<dyn Fn(u64) -> u64>,
    }

    let mut list: Vec<Perm> = Vec::new();
    let mut push = |name: &'static str, fwd: fn(u64) -> u64, inv: fn(u64) -> u64| {
        list.push(Perm {
            name,
            fwd: Box::new(fwd),
            inv: Box::new(inv),
        })
    };
    fn byte_rev(v: u64) -> u64 {
        v.swap_bytes()
    }
    fn bit_rev(v: u64) -> u64 {
        let mut out = 0u64;
        for i in 0..64 {
            if v & (1 << i) != 0 {
                out |= 1 << (63 - i);
            }
        }
        out
    }
    fn half_swap(v: u64) -> u64 {
        (v << 32) | (v >> 32)
    }
    fn word_swap(v: u64) -> u64 {
        let b = v.to_le_bytes();
        u64::from_le_bytes([b[4], b[5], b[6], b[7], b[0], b[1], b[2], b[3]])
    }
    // The STU reference permutation (TankLib's Deobfuscate, whose swap list is
    // published), forward and undone. The undo replays the swaps in reverse
    // order; each individual swap is its own inverse.
    fn stu_fwd(v: u64) -> u64 {
        let mut b = v.to_le_bytes();
        b.swap(0, 3);
        b.swap(7, 1);
        b.swap(2, 6);
        b.swap(4, 5);
        u64::from_le_bytes(b)
    }
    fn stu_inv(v: u64) -> u64 {
        let mut b = v.to_le_bytes();
        b.swap(4, 5);
        b.swap(2, 6);
        b.swap(7, 1);
        b.swap(0, 3);
        u64::from_le_bytes(b)
    }
    fn id(v: u64) -> u64 {
        v
    }
    push("identity", id, id);
    push("byte reverse", byte_rev, byte_rev);
    push("bit reverse", bit_rev, bit_rev);
    push("half swap", half_swap, half_swap);
    push("word swap", word_swap, word_swap);
    push("stu swap", stu_fwd, stu_inv);
    for shift in 1..64u32 {
        list.push(Perm {
            name: Box::leak(format!("rol {shift}").into_boxed_str()),
            fwd: Box::new(move |v: u64| v.rotate_left(shift)),
            inv: Box::new(move |v: u64| v.rotate_right(shift)),
        });
    }
    // Compositions with the STU swap, since the STU scheme is the one
    // reference this project has actually seen work.
    list.push(Perm {
        name: "stu swap then byte reverse",
        fwd: Box::new(|v| byte_rev(stu_fwd(v))),
        inv: Box::new(|v| stu_inv(byte_rev(v))),
    });
    list.push(Perm {
        name: "stu swap then half swap",
        fwd: Box::new(|v| half_swap(stu_fwd(v))),
        inv: Box::new(|v| stu_inv(half_swap(v))),
    });

    // Candidate types for what the value is an obfuscated GUID *of*. The
    // chain in the docs says material (0x008); material data (0x0B3) and the
    // texture (0x004) sit next to it, and guessing the type costs nothing.
    let candidate_types = [0x008u16, 0x0B3, 0x004];

    let mut found = false;

    // Family A: value = P(guid ^ c)  <=>  Q(value) ^ c = guid.
    for type_id in candidate_types {
        let guids = data.guids_of_type(type_id);
        if guids.is_empty() {
            continue;
        }
        for p in &list {
            let q = &p.inv;
            let q0 = q(probes[0]);
            for g in &guids {
                let c = q0 ^ g;
                if probes[1..].iter().all(|v| known.contains(&(q(*v) ^ c))) {
                    let guid = q(probes[1]) ^ c;
                    println!(
                        "MATCH value = {}(guid ^ {c:#018x})\n\
                         \x20       type 0x{:03X}: {guid:#018x} explains all {} values",
                        p.name,
                        guid::type_of(guid),
                        probes.len()
                    );
                    found = true;
                    break;
                }
            }
            if found {
                break;
            }
        }
        if found {
            break;
        }
    }

    // Family B: value = P(guid) ^ c  <=>  Q(value ^ c) = guid.
    if !found {
        for type_id in candidate_types {
            let guids = data.guids_of_type(type_id);
            if guids.is_empty() {
                continue;
            }
            for p in &list {
                for g in &guids {
                    let c = (p.fwd)(*g) ^ probes[0];
                    if probes[1..]
                        .iter()
                        .all(|v| known.contains(&(p.inv)(*v ^ c)))
                    {
                        let guid = (p.inv)(probes[1] ^ c);
                        println!(
                            "MATCH value = {}(guid) ^ {c:#018x}\n\
                             \x20       type 0x{:03X}: {guid:#018x} explains all {} values",
                            p.name,
                            guid::type_of(guid),
                            probes.len()
                        );
                        found = true;
                        break;
                    }
                }
                if found {
                    break;
                }
            }
            if found {
                break;
            }
        }
    }

    // Family C: value = H(guid) for a known 64-bit hash. Each candidate runs
    // over every material GUID; landing on every observed value is the test.
    if !found {
        fn crc64_forward(data: &[u8], init: u64, invert: bool) -> u64 {
            // Non-reflected CRC64 (ECMA polynomial), as an independent variant
            // of the reflected one ow-stu ships.
            let mut crc = init;
            for b in data {
                crc ^= (*b as u64) << 56;
                for _ in 0..8 {
                    crc = if crc & (1 << 63) != 0 {
                        (crc << 1) ^ 0x42F0_E129_2C6E_9ED5
                    } else {
                        crc << 1
                    };
                }
            }
            if invert { !crc } else { crc }
        }
        fn crc64_reflected(data: &[u8], init: u64, invert: bool) -> u64 {
            const POLY: u64 = 0x95AC_9329_AC4B_C9B5;
            let mut crc = init;
            for b in data {
                let mut c = (crc ^ *b as u64) & 0xFF;
                for _ in 0..8 {
                    c = if c & 1 != 0 { (c >> 1) ^ POLY } else { c >> 1 };
                }
                crc = (crc >> 8) ^ c;
            }
            if invert { !crc } else { crc }
        }
        fn fnv1a(data: &[u8]) -> u64 {
            let mut h = 0xcbf2_9ce4_8422_2325u64;
            for b in data {
                h ^= *b as u64;
                h = h.wrapping_mul(0x100_0000_01b3);
            }
            h
        }
        fn fnv1(data: &[u8]) -> u64 {
            let mut h = 0xcbf2_9ce4_8422_2325u64;
            for b in data {
                h = h.wrapping_mul(0x100_0000_01b3);
                h ^= *b as u64;
            }
            h
        }
        fn djb2(data: &[u8]) -> u64 {
            let mut h = 5381u64;
            for b in data {
                h = h.wrapping_mul(33).wrapping_add(*b as u64);
            }
            h
        }
        fn sdbm(data: &[u8]) -> u64 {
            let mut h = 0u64;
            for b in data {
                h = (*b as u64)
                    .wrapping_add(h << 6)
                    .wrapping_add(h << 16)
                    .wrapping_sub(h);
            }
            h
        }

        let observed: std::collections::HashSet<u64> = values.iter().copied().collect();
        let materials = data.guids_of_type(0x008);
        let hashers: Vec<(&str, Box<dyn Fn(&[u8]) -> u64>)> = vec![
            ("stucrc64", Box::new(|d: &[u8]| ow_stu::crc::crc64(d))),
            ("crc64-forward/0", Box::new(|d: &[u8]| crc64_forward(d, 0, false))),
            ("crc64-forward/~0", Box::new(|d: &[u8]| crc64_forward(d, !0, false))),
            ("crc64-forward/0/inv", Box::new(|d: &[u8]| crc64_forward(d, 0, true))),
            ("crc64-forward/~0/inv", Box::new(|d: &[u8]| crc64_forward(d, !0, true))),
            ("crc64-refl/0", Box::new(|d: &[u8]| crc64_reflected(d, 0, false))),
            ("crc64-refl/~0", Box::new(|d: &[u8]| crc64_reflected(d, !0, false))),
            ("crc64-refl/0/inv", Box::new(|d: &[u8]| crc64_reflected(d, 0, true))),
            ("crc64-refl/~0/inv", Box::new(|d: &[u8]| crc64_reflected(d, !0, true))),
            ("fnv1a", Box::new(fnv1a)),
            ("fnv1", Box::new(fnv1)),
            ("djb2", Box::new(djb2)),
            ("sdbm", Box::new(sdbm)),
        ];

        'outer: for (label, h) in &hashers {
            for order in ["le", "be"] {
                let mut landed: Vec<u64> = Vec::new();
                for g in &materials {
                    let mut bytes = g.to_le_bytes();
                    if order == "be" {
                        bytes.reverse();
                    }
                    if observed.contains(&h(&bytes)) {
                        landed.push(*g);
                    }
                }
                if landed.len() == values.len() {
                    println!(
                        "MATCH value = {label}({order} guid); first guid {:#018x}",
                        landed[0]
                    );
                    found = true;
                    break 'outer;
                } else if !landed.is_empty() {
                    println!(
                        "  {label}/{order}: {} of {} values land -- partial, not a match",
                        landed.len(),
                        values.len()
                    );
                }
            }
        }
    }

    if !found {
        println!(
            "no match: {} permutations x {} candidate types, plus {} hash variants",
            list.len(),
            candidate_types.len(),
            13
        );
    }
    Ok(())
}

/// Collect MDLC material arrays from many models, to see how they are encoded.
///
/// Four values on one model can be ruled out or explained, but only one model
/// can be a coincidence. If the same mask applies to all models the values line
/// up; if it varies by position or by asset, the pattern shows it.
///
/// ```text
/// ow-probe materials [--count 40] [--type 0x00C]
/// ```
fn materials(data: &GameData, rest: &[String]) -> Result<(), String> {
    let count: usize = rest
        .iter()
        .position(|a| a == "--count")
        .and_then(|i| rest.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(40);
    let type_id: u16 = rest
        .iter()
        .position(|a| a == "--type")
        .and_then(|i| rest.get(i + 1))
        .and_then(|v| u16::from_str_radix(v.trim_start_matches("0x"), 16).ok())
        .unwrap_or(types::MODEL);

    let guids = data.guids_of_type(type_id);
    let mut shown = 0usize;
    for guid in &guids {
        if shown >= count {
            break;
        }
        let Ok(bytes) = data.bytes_of(*guid) else { continue };
        let Ok(chunks) = ow_tank::chunks::Chunks::parse(&bytes) else {
            continue;
        };
        let Some(mdlc) = chunks.payload(&bytes, b"MDLC") else {
            continue;
        };
        if mdlc.len() < 106 {
            continue;
        }
        let offset = i64::from_le_bytes(mdlc[8..16].try_into().unwrap());
        let count = u16::from_le_bytes(mdlc[104..106].try_into().unwrap());
        if offset <= 0 || offset as usize + count as usize * 8 > mdlc.len() {
            continue;
        }
        let base = offset as usize;
        let mut values = Vec::new();
        for i in 0..count as usize {
            values.push(u64::from_le_bytes(mdlc[base + i * 8..base + i * 8 + 8].try_into().unwrap()));
        }
        println!(
            "0x{guid:016x}  {count} material(s) at {offset}: {}",
            values
                .iter()
                .enumerate()
                .map(|(i, v)| format!("[{i}]{v:#018x}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        shown += 1;
    }
    println!("{shown} model(s)");
    Ok(())
}

/// Print the MDLC material values of every model named in a file.
///
/// One process, many models: the values are wanted for cross-referencing
/// against a memory capture, and reopening the container per model is the
/// difference between seconds and minutes. The file holds one GUID per line.
///
/// ```text
/// ow-probe mdl-values models.txt
/// ```
fn mdl_values(data: &GameData, rest: &[String]) -> Result<(), String> {
    let path = rest.first().ok_or("usage: ow-probe mdl-values <file>")?;
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let mut models = 0usize;
    let mut values = 0usize;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Ok(guid) = u64::from_str_radix(line.trim_start_matches("0x"), 16) else {
            continue;
        };
        let Ok(bytes) = data.bytes_of(guid) else { continue };
        let Ok(chunks) = ow_tank::chunks::Chunks::parse(&bytes) else {
            continue;
        };
        let Some(mdlc) = chunks.payload(&bytes, b"MDLC") else {
            continue;
        };
        if mdlc.len() < 106 {
            continue;
        }
        let offset = i64::from_le_bytes(mdlc[8..16].try_into().unwrap());
        let count = u16::from_le_bytes(mdlc[104..106].try_into().unwrap());
        if offset <= 0 || offset as usize + count as usize * 8 > mdlc.len() {
            continue;
        }
        let base = offset as usize;
        let mut row = Vec::with_capacity(count as usize);
        for i in 0..count as usize {
            row.push(u64::from_le_bytes(
                mdlc[base + i * 8..base + i * 8 + 8].try_into().unwrap(),
            ));
        }
        println!(
            "0x{guid:016x} {}",
            row.iter()
                .map(|v| format!("{v:#018x}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        models += 1;
        values += row.len();
    }
    println!("{models} model(s), {values} material value(s)");
    Ok(())
}

/// Find the map chunks that sit where a known chunk sits.
///
/// Chunks belonging to one map tile it, so they overlap in space and the map is
/// recoverable by proximity -- no map-to-chunk link needed. The known chunk
/// came out of a running game; the rest of its map is whatever is near it.
///
/// ```text
/// ow-probe near <chunk-file> [--radius 600] [--limit 80]
/// ```
fn near(data: &GameData, rest: &[String]) -> Result<(), String> {
    let path = rest
        .iter()
        .find(|a| !a.starts_with("--"))
        .ok_or("usage: ow-probe near <chunk-file> [--radius N]")?;
    let radius: f32 = rest
        .iter()
        .position(|a| a == "--radius")
        .and_then(|i| rest.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(600.0);
    let limit: usize = rest
        .iter()
        .position(|a| a == "--limit")
        .and_then(|i| rest.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(80);

    let bytes = std::fs::read(path).map_err(|e| format!("could not read {path}: {e}"))?;
    let resolves = |g: u64| data.registry().get(g).is_some();
    let (_, seed) = ow_tank::mapchunk::MapChunk::detect(&bytes, &resolves)
        .map_err(|e| e.to_string())?;
    let (min, max) = bounds(&seed);
    let seed_centre = median(&seed);
    println!(
        "seed: {} placeables, median ({:.0}, {:.0}, {:.0}), box ({:.0}, {:.0}, {:.0}) .. ({:.0}, {:.0}, {:.0})",
        seed.placeables.len(),
        seed_centre.x, seed_centre.y, seed_centre.z,
        min.x, min.y, min.z, max.x, max.y, max.z
    );
    let centre = seed_centre;

    let guids = data.guids_of_type(0x0BC);
    println!("{} map chunks to check", guids.len());
    let mut hits: Vec<(u64, usize, f32, [f32; 3])> = Vec::new();
    let mut decoded = 0usize;
    for (i, guid) in guids.iter().enumerate() {
        if i % 5000 == 0 && i > 0 {
            eprintln!("  ... {i} checked, {} near", hits.len());
        }
        let Ok(raw) = data.bytes_of(*guid) else { continue };
        let Ok((_, chunk)) = ow_tank::mapchunk::MapChunk::detect(&raw, &resolves) else {
            continue;
        };
        decoded += 1;
        if chunk.placeables.is_empty() {
            continue;
        }
        // The median, not the box. A chunk carries a handful of far-away
        // placements -- skybox pieces, distant props -- and their box covers
        // the whole map, which makes every chunk a neighbour of every other.
        let c = median(&chunk);
        let distance = (c - centre).length();
        if distance <= radius {
            hits.push((*guid, chunk.placeables.len(), distance, c.to_array()));
        }
    }
    hits.sort_by(|a, b| a.2.total_cmp(&b.2));
    println!("{decoded} chunks decoded; {} within {radius:.0}", hits.len());
    for (guid, count, distance, centre) in hits.iter().take(limit) {
        println!(
            "  0x{guid:016x}  {count:>5} placeables  {distance:>8.1} away  centre ({:.0}, {:.0}, {:.0})",
            centre[0], centre[1], centre[2]
        );
    }
    Ok(())
}

/// Where a chunk's placements mostly are, robust to a few far-away ones.
fn median(chunk: &ow_tank::mapchunk::MapChunk) -> glam::Vec3 {
    let mut xs: Vec<f32> = chunk.placeables.iter().map(|p| p.translation[0]).collect();
    let mut ys: Vec<f32> = chunk.placeables.iter().map(|p| p.translation[1]).collect();
    let mut zs: Vec<f32> = chunk.placeables.iter().map(|p| p.translation[2]).collect();
    xs.sort_by(f32::total_cmp);
    ys.sort_by(f32::total_cmp);
    zs.sort_by(f32::total_cmp);
    let mid = chunk.placeables.len() / 2;
    glam::Vec3::new(xs[mid], ys[mid], zs[mid])
}

fn bounds(chunk: &ow_tank::mapchunk::MapChunk) -> (glam::Vec3, glam::Vec3) {
    let mut min = glam::Vec3::splat(f32::MAX);
    let mut max = glam::Vec3::splat(f32::MIN);
    for p in &chunk.placeables {
        let t = glam::Vec3::from_array(p.translation);
        min = min.min(t);
        max = max.max(t);
    }
    (min, max)
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

    if rest.iter().any(|a| a == "--mdlc") {
        let payload = chunks
            .payload(&bytes, b"MDLC")
            .ok_or("no MDLC chunk")?;
        println!("MDLC: {} bytes", payload.len());
        for (i, row) in payload.chunks(16).take(10).enumerate() {
            let hexs: Vec<String> = row.iter().map(|b| format!("{b:02x}")).collect();
            println!("  {:04x}  {:<47}", i * 16, hexs.join(" "));
        }
        println!(
            "  offset@8 = {}  count@104 = {}",
            i64::from_le_bytes(payload[8..16].try_into().unwrap()),
            u16::from_le_bytes(payload[104..106].try_into().unwrap())
        );
        return Ok(());
    }

    let m = ow_tank::model::parse(&bytes, &load).map_err(|e| e.to_string())?;
    println!(
        "{} meshes, {} vertices, {} triangles, {} materials",
        m.meshes.len(),
        m.vertex_count(),
        m.triangle_count(),
        m.materials.len()
    );
    for material in m.materials.iter().take(8) {
        println!(
            "  material 0x{material:016x}  type 0x{:03X}  {}",
            guid::type_of(*material),
            if data.registry().get(*material).is_some() { "present" } else { "absent" }
        );
    }
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
        // A model's own structured data is a chunk of the same file, and its
        // references are STU references -- readable, unlike the model chunk's
        // obfuscated material array.
        if let Some(payload) = chunks.payload(&bytes, b"MSTU") {
            // The chunk leads with the offset and length of the structured data
            // inside it; the data is not the whole payload.
            let (offset, size) = if payload.len() >= 16 {
                (
                    i64::from_le_bytes(payload[0..8].try_into().unwrap()),
                    i64::from_le_bytes(payload[8..16].try_into().unwrap()),
                )
            } else {
                (0, payload.len() as i64)
            };
            println!("MSTU: data at {offset} for {size} bytes of {} ", payload.len());
            let slice = if offset >= 0 && size > 0 && (offset + size) as usize <= payload.len() {
                payload[offset as usize..(offset + size) as usize].to_vec()
            } else {
                payload.to_vec()
            };
            match ow_stu::Stu::parse(slice) {
                Ok(mut stu) => {
                    println!("MSTU: {} instance(s)", stu.instances.len());
                    for index in 0..stu.instances.len() {
                        let info = stu.instances[index];
                        println!(
                            "  [{index:>3}] 0x{:08X} {}",
                            info.hash,
                            data.classes.describe(info.hash)
                        );
                        let fields: Vec<ow_stu::FieldInfo> = stu.fields(index).to_vec();
                        for field in fields {
                            if field.size != 8 {
                                continue;
                            }
                            let at = stu.payload_start(index) + field.offset;
                            if let Some(target) = stu.asset_ref_at(at, field.hash) {
                                let kind = guid::type_of(target);
                                let known = data.registry().get(target).is_some();
                                // The stored word travels with the decoded one:
                                // a field declared as a plain `ulong` in the
                                // reference tooling is *not* obfuscated, so its
                                // raw value is the thing to compare against the
                                // MDLC material array, not the deobfuscated one.
                                let raw = stu.u64_at(at).unwrap_or(0);
                                // Print the type even when it does not resolve:
                                // a type that is plausible but absent from the
                                // install is a different problem from a type that
                                // is nonsense, and the two look the same when
                                // both are silently dropped.
                                if kind < 0x400 || known {
                                    println!(
                                        "        0x{:08X} -> 0x{target:016x}  type 0x{kind:03X}  {}  raw 0x{raw:016x}",
                                        field.hash,
                                        if known { "present" } else { "absent" }
                                    );
                                }
                            }
                        }
                    }
                }
                Err(e) => println!("MSTU did not parse as structured data: {e}"),
            }
        }
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

/// Every asset GUID in the registry, one per line, optionally one type.
///
/// For feeding membership tests against a candidate transform: the answer to
/// "is this decoded value an asset" needs the whole registry, not one type —
/// a decode can land on a different type than the caller expected.
///
/// ```text
/// ow-probe guids            every GUID
/// ow-probe guids 0x008      only that type
/// ```
fn guids_cmd(data: &GameData, rest: &[String]) -> Result<(), String> {
    if let Some(arg) = rest.first() {
        let type_id: u16 = arg
            .trim_start_matches("0x")
            .parse()
            .map_err(|_| "type must be a number".to_string())?;
        for guid in data.guids_of_type(type_id) {
            println!("0x{guid:016x}");
        }
        return Ok(());
    }
    for record in data.registry().records() {
        println!("0x{:016x}", record.guid);
    }
    Ok(())
}

/// The first few GUIDs of an asset type, for feeding `dump`.
///
/// With `--readable`, only GUIDs whose bytes are actually installed are
/// listed: a manifest holds every asset whether or not this install has
/// downloaded it, and a GUID whose content key is absent fails at `dump`.
///
/// ```text
/// ow-probe first 0x008 [count]
/// ow-probe first 0x008 [count] --readable
/// ```
fn first_of_type(data: &GameData, rest: &[String]) -> Result<(), String> {
    let readable = rest.iter().any(|a| a == "--readable");
    let type_arg = rest
        .iter()
        .find(|a| !a.starts_with("--") && a.parse::<usize>().is_err())
        .or_else(|| rest.first())
        .ok_or("usage: ow-probe first <type> [count] [--readable]")?;
    let type_id: u16 = type_arg
        .trim_start_matches("0x")
        .parse()
        .map_err(|_| "type must be a number".to_string())?;
    let count: usize = rest
        .iter()
        .find(|a| !a.starts_with("--") && a.parse::<usize>().is_ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    let guids = data.guids_of_type(type_id);
    println!("{} asset(s) of type 0x{type_id:03X}", guids.len());
    let mut shown = 0usize;
    let mut probed = 0usize;
    for guid in &guids {
        if shown >= count {
            break;
        }
        if readable {
            probed += 1;
            if data.bytes_of(*guid).is_err() {
                continue;
            }
            if shown == 0 {
                println!("(first {probed} probed for readability)");
            }
        }
        println!("0x{guid:016x}");
        shown += 1;
    }
    if readable {
        println!("{shown} readable of {probed} probed");
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
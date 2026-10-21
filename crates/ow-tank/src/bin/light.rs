//! Read lighting manifests (type `0x0BD`): header and chunk list.
//!
//! This is the "log, don't look" tool for the lighting manifest. The questions
//! it answers: which magic the assets carry (the reference reader does not name
//! its value, so it was read off the install), how many chunks a manifest names,
//! and what a chunk record looks like.
//!
//! ```text
//! ow-tank-light                 # census the first 200 manifests
//! ow-tank-light --limit 20      # fewer
//! ow-tank-light --guid 0x0bd…   # one asset, every field printed
//! ```
//!
//! Read-only.

use ow_tank::{lighting, GameData, ManifestSelector};

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

    let limit: usize = flag("--limit").and_then(|v| v.parse().ok()).unwrap_or(200);

    if args.iter().any(|a| a == "--hexdump") {
        let Some(g) = flag("--guid") else {
            eprintln!("--hexdump wants --guid");
            return std::process::ExitCode::from(2);
        };
        let v = match u64::from_str_radix(g.trim_start_matches("0x"), 16) {
            Ok(v) => v,
            Err(_) => {
                eprintln!("--guid wants a hex GUID");
                return std::process::ExitCode::from(2);
            }
        };
        let bytes = match data.bytes_of(v) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("load failed: {e}");
                return std::process::ExitCode::from(1);
            }
        };
        println!("0x{v:016x}: {} bytes", bytes.len());
        let n = flag("--bytes")
            .and_then(|v| usize::from_str_radix(v.trim_start_matches("0x"), 16).ok())
            .unwrap_or(bytes.len());
        for (row, chunk) in bytes[..n.min(bytes.len())].chunks(16).enumerate() {
            let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
            println!("{:04x}  {}", row * 16, hex.join(" "));
        }
        return std::process::ExitCode::SUCCESS;
    }

    let guids = match flag("--guid") {
        Some(g) => match u64::from_str_radix(g.trim_start_matches("0x"), 16) {
            Ok(v) => vec![v],
            Err(_) => {
                eprintln!("--guid wants a hex GUID");
                return std::process::ExitCode::from(2);
            }
        },
        None => data.guids_of_type(lighting::TYPE),
    };

    let mut parsed = 0usize;
    let mut open_failed = 0usize;
    let mut parse_failed = 0usize;
    let mut magics: std::collections::BTreeMap<u32, usize> = Default::default();
    let mut counts: std::collections::BTreeMap<u32, usize> = Default::default();
    let mut unknown1: std::collections::BTreeMap<u32, usize> = Default::default();
    let mut unknown2: std::collections::BTreeMap<u32, usize> = Default::default();
    let mut unknown45: std::collections::BTreeMap<(u32, u32), usize> = Default::default();
    let mut chunk_types: std::collections::BTreeMap<u16, usize> = Default::default();
    let mut size_ok = 0usize;
    let mut exact_len = 0usize;
    let mut zero_ok = 0usize;
    let mut shown = 0usize;
    for guid_v in guids.iter().take(limit) {
        let bytes = match data.bytes_of(*guid_v) {
            Ok(b) => b,
            Err(_) => {
                open_failed += 1;
                continue;
            }
        };
        match lighting::parse(&bytes) {
            Ok(manifest) => {
                parsed += 1;
                let h = &manifest.header;
                *magics.entry(h.magic).or_insert(0) += 1;
                *counts.entry(h.chunk_count).or_insert(0) += 1;
                *unknown1.entry(h.unknown1).or_insert(0) += 1;
                *unknown2.entry(h.unknown2).or_insert(0) += 1;
                *unknown45.entry((h.unknown4, h.unknown5)).or_insert(0) += 1;
                for c in &manifest.chunks {
                    *chunk_types
                        .entry(ow_tank::guid::type_of(c.guid))
                        .or_insert(0) += 1;
                }
                if h.data_size as usize == bytes.len() - 8 {
                    size_ok += 1;
                }
                if lighting::HEADER_SIZE + manifest.chunks.len() * lighting::CHUNK_SIZE
                    == bytes.len()
                {
                    exact_len += 1;
                }
                if h.unknown3 == 0 && h.unknown6 == 0 && h.unknown7 == 0 {
                    zero_ok += 1;
                }
                if shown < 5 || args.iter().any(|a| a == "--all") {
                    shown += 1;
                    println!(
                        "0x{guid_v:016x}: {} bytes, data_size {}, unknown1 {}, unknown2 {}, \
                         unknown4/5 {}/{}, chunk_count {}, unknown3/6/7 {}/{}/{}",
                        bytes.len(),
                        h.data_size,
                        h.unknown1,
                        h.unknown2,
                        h.unknown4,
                        h.unknown5,
                        h.chunk_count,
                        h.unknown3,
                        h.unknown6,
                        h.unknown7
                    );
                    for (i, c) in manifest.chunks.iter().enumerate().take(4) {
                        println!(
                            "  [{i:>3}] type 0x{:03x} guid 0x{:016x}",
                            ow_tank::guid::type_of(c.guid),
                            c.guid
                        );
                    }
                }
            }
            Err(e) => {
                parse_failed += 1;
                if parse_failed <= 3 || args.iter().any(|a| a == "--raw") {
                    let head: Vec<String> =
                        bytes.iter().take(96).map(|b| format!("{b:02x}")).collect();
                    eprintln!("0x{guid_v:016x}: {e}; head {}", head.join(" "));
                }
            }
        }
    }

    println!(
        "{} manifest(s) in the registry; {parsed} parsed of {} tried \
         ({open_failed} would not open, {parse_failed} did not parse)",
        guids.len(),
        guids.len().min(limit)
    );
    println!("  magics: {magics:08x?}");
    println!("  chunk counts: {counts:?}");
    println!("  unknown1 (constant?): {unknown1:?}");
    println!("  unknown2: {unknown2:?}");
    println!("  unknown4/5 pairs: {unknown45:?}");
    println!("  chunk guid types: {chunk_types:03x?}");
    println!(
        "  invariants: data_size == len - 8: {size_ok}/{parsed}; \
         40 + 8*count == len: {exact_len}/{parsed}; unknown3/6/7 == 0: {zero_ok}/{parsed}"
    );
    std::process::ExitCode::SUCCESS
}

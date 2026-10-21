//! Inspect shader assets (types `0x085` group, `0x087` code; `0x086` instance
//! is empty in this build's registry).
//!
//! ```text
//! ow-tank-shader                       # invariant census, 30 openable assets per type
//! ow-tank-shader --limit 200
//! ow-tank-shader --types               # registry histogram around the shader types
//! ow-tank-shader --guid 0x…            # one asset, layout decoded
//! ow-tank-shader --guid 0x… --hexdump  # raw bytes
//! ```
//!
//! Read-only.

use ow_tank::{guid, shader, GameData, ManifestSelector};

const TYPE_GROUP: u16 = 0x085;
const TYPE_INSTANCE: u16 = 0x086;
const TYPE_CODE: u16 = 0x087;

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    data.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn u64_at(data: &[u8], at: usize) -> Option<u64> {
    data.get(at..at + 8)
        .map(|s| u64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))
}

fn openable(data: &GameData, type_id: u16, want: usize, scan_cap: usize) -> Vec<(u64, Vec<u8>)> {
    let guids = data.guids_of_type(type_id);
    let mut out = Vec::with_capacity(want);
    for g in guids.iter().take(scan_cap) {
        if out.len() == want {
            break;
        }
        if let Ok(bytes) = data.bytes_of(*g) {
            out.push((*g, bytes));
        }
    }
    out
}

struct Report {
    ok: bool,
    issues: Vec<String>,
    lines: Vec<String>,
}

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

    if args.iter().any(|a| a == "--types") {
        let histogram = data.type_histogram();
        for (type_id, count) in &histogram {
            if (0x38..=0x50).contains(type_id) || (0x84..=0x90).contains(type_id) {
                println!("type 0x{type_id:03x}: {count}");
            }
        }
        return std::process::ExitCode::SUCCESS;
    }

    let limit: usize = flag("--limit").and_then(|v| v.parse().ok()).unwrap_or(30);

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
        println!(
            "0x{v:016x}: type 0x{:03x}, {} bytes",
            guid::type_of(v),
            bytes.len()
        );
        let n = flag("--bytes")
            .and_then(|v| usize::from_str_radix(v.trim_start_matches("0x"), 16).ok())
            .unwrap_or(0x100);
        for (row, chunk) in bytes[..n.min(bytes.len())].chunks(16).enumerate() {
            let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
            let as_u16: Vec<String> = chunk
                .chunks(2)
                .map(|w| {
                    format!(
                        "{:>5}",
                        u16::from_le_bytes([w[0], w.get(1).copied().unwrap_or(0)])
                    )
                })
                .collect();
            println!(
                "{:06x}  {:<47}  {}",
                row * 16,
                hex.join(" "),
                as_u16.join(" ")
            );
        }
        return std::process::ExitCode::SUCCESS;
    }

    if args.iter().any(|a| a == "--guid") {
        let Some(g) = flag("--guid") else {
            unreachable!()
        };
        let Ok(v) = u64::from_str_radix(g.trim_start_matches("0x"), 16) else {
            eprintln!("--guid wants a hex GUID");
            return std::process::ExitCode::from(2);
        };
        let bytes = match data.bytes_of(v) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("load failed: {e}");
                return std::process::ExitCode::from(1);
            }
        };
        let report = match guid::type_of(v) {
            TYPE_GROUP => analyze_group(&bytes),
            TYPE_CODE => analyze_code(&bytes),
            other => {
                println!("0x{v:016x}: type 0x{other:03x}, {} bytes", bytes.len());
                return std::process::ExitCode::SUCCESS;
            }
        };
        println!("0x{v:016x}: {} bytes", bytes.len());
        for line in &report.lines {
            println!("  {line}");
        }
        if !report.issues.is_empty() {
            println!("  issues: {}", report.issues.join("; "));
        }
        return std::process::ExitCode::SUCCESS;
    }

    // Census.
    for (type_id, label) in [
        (TYPE_GROUP, "group"),
        (TYPE_INSTANCE, "instance"),
        (TYPE_CODE, "code"),
    ] {
        let all = data.guids_of_type(type_id);
        let assets = openable(&data, type_id, limit, 400);
        println!(
            "{label} (0x{type_id:03x}): {} in registry; {} openable of first 400",
            all.len(),
            assets.len()
        );
        if assets.is_empty() {
            println!("  NOT FOUND");
            continue;
        }
        let mut ok_count = 0usize;
        let mut module_ok = 0usize;
        let mut violations: std::collections::BTreeMap<String, usize> = Default::default();
        for (i, (g, bytes)) in assets.iter().enumerate() {
            let report = match type_id {
                TYPE_GROUP => analyze_group(bytes),
                TYPE_CODE => analyze_code(bytes),
                _ => continue,
            };
            let parsed = match type_id {
                TYPE_GROUP => shader::parse_group(bytes).is_ok(),
                TYPE_CODE => shader::parse_code(bytes).is_ok(),
                _ => false,
            };
            if parsed {
                module_ok += 1;
            }
            if i < 3 {
                println!("  0x{g:016x}: {} bytes", bytes.len());
                for line in &report.lines {
                    println!("    {line}");
                }
            }
            if report.ok {
                ok_count += 1;
            }
            for issue in report.issues {
                *violations.entry(issue).or_insert(0) += 1;
            }
        }
        println!(
            "  invariants hold on {ok_count}/{}; module parse succeeded on {module_ok}/{}{}",
            assets.len(),
            assets.len(),
            if violations.is_empty() {
                String::new()
            } else {
                format!("; violations: {violations:?}")
            }
        );
    }
    std::process::ExitCode::SUCCESS
}

/// Group layout, read off the install. Header is 0xb8 bytes; N = u32 @0x64.
/// Offsets: X @0x08 (24-byte records × N), A @0x10 ({u32, u32} × N),
/// B @0x58 (u32 × N, padded to 8), C @0x68 (u64 code GUIDs × N),
/// D @0x70 (u64 × N), E @0x78 (u64 source GUIDs × N), F @0x80 (u64 × N).
fn analyze_group(bytes: &[u8]) -> Report {
    let len = bytes.len();
    let mut issues = Vec::new();
    let mut lines = Vec::new();
    let Some(h8) = u64_at(bytes, 0x08) else {
        return Report {
            ok: false,
            issues: vec!["too short for a header".into()],
            lines,
        };
    };
    let a0 = u64_at(bytes, 0x10).unwrap_or(u64::MAX) as usize;
    let b0 = u64_at(bytes, 0x58).unwrap_or(u64::MAX) as usize;
    let c0 = u64_at(bytes, 0x68).unwrap_or(u64::MAX) as usize;
    let d0 = u64_at(bytes, 0x70).unwrap_or(u64::MAX) as usize;
    let e0 = u64_at(bytes, 0x78).unwrap_or(u64::MAX) as usize;
    let f0 = u64_at(bytes, 0x80).unwrap_or(u64::MAX) as usize;
    let num = u32_at(bytes, 0x64).unwrap_or(u32::MAX) as usize;
    let x0 = u64_at(bytes, 0x00).unwrap_or(u64::MAX);
    let null18 = u64_at(bytes, 0x18).unwrap_or(0);
    let src = u64_at(bytes, 0x98).unwrap_or(0);
    let u60 = u32_at(bytes, 0x60).unwrap_or(0);

    let checks = [
        ("X not at 0xb8", h8 as usize != 0xB8),
        ("X != 24*N", a0.checked_sub(h8 as usize) != Some(24 * num)),
        ("A != 8*N", b0.checked_sub(a0) != Some(8 * num)),
        (
            "B !~ 4*N",
            b0 > c0 || (c0 - b0 != 4 * num && c0 - b0 != 4 * num + 4),
        ),
        ("C != 8*N", d0.checked_sub(c0) != Some(8 * num)),
        ("D != 8*N", e0.checked_sub(d0) != Some(8 * num)),
        ("E != 8*N", f0.checked_sub(e0) != Some(8 * num)),
        ("F != EOF-8*N", f0 > len || len - f0 != 8 * num),
    ];
    for (what, bad) in &checks {
        if *bad {
            issues.push((*what).to_string());
        }
    }
    let mut c_types: std::collections::BTreeMap<u16, usize> = Default::default();
    for i in 0..num.min(1024) {
        if let Some(c) = u64_at(bytes, c0 + i * 8) {
            *c_types.entry(guid::type_of(c)).or_insert(0) += 1;
        }
    }
    let mut e_types: std::collections::BTreeMap<u16, usize> = Default::default();
    for i in 0..num.min(1024) {
        if let Some(c) = u64_at(bytes, e0 + i * 8) {
            *e_types.entry(guid::type_of(c)).or_insert(0) += 1;
        }
    }
    lines.push(format!(
        "N={num}, offsets X@{h8} A@{a0} B@{b0} C@{c0} D@{d0} E@{e0} F@{f0}, len {len}"
    ));
    lines.push(format!(
        "unknown0 0x{x0:016x} (type 0x{:03x}), null18 0x{null18:016x}, u60 0x{u60:08x}, \
         src 0x{src:016x} (type 0x{:03x})",
        guid::type_of(x0),
        guid::type_of(src)
    ));
    lines.push(format!(
        "C types {c_types:03x?}; E types {e_types:03x?}; C[0] 0x{:016x} E[0] 0x{:016x}",
        u64_at(bytes, c0).unwrap_or(0),
        u64_at(bytes, e0).unwrap_or(0)
    ));
    Report {
        ok: issues.is_empty(),
        issues,
        lines,
    }
}

/// Code layout, read off the install: gzip payload at u64 @0x70, compressed
/// size u32 @0x84, uncompressed size u32 @0x88, header 0x110 bytes.
fn analyze_code(bytes: &[u8]) -> Report {
    let len = bytes.len();
    let mut issues = Vec::new();
    let mut lines = Vec::new();
    let data_at = u64_at(bytes, 0x70).unwrap_or(u64::MAX) as usize;
    let compressed = u32_at(bytes, 0x84).unwrap_or(u32::MAX) as usize;
    let uncompressed = u32_at(bytes, 0x88).unwrap_or(u32::MAX) as usize;
    let zero0 = u64_at(bytes, 0x00).unwrap_or(0);
    let u08 = u64_at(bytes, 0x08).unwrap_or(0);
    let u48 = u64_at(bytes, 0x48).unwrap_or(0);
    let at = data_at;
    let gzip = bytes.get(at..at + 2) == Some(&[0x1f, 0x8b]);
    let head_ok = at < len && len < 1 << 24;
    if !gzip {
        issues.push("no gzip magic at @0x70".into());
    }
    if !head_ok || compressed != len - at {
        issues.push("compressed size != len - data offset".into());
    }
    let mut inflated: Option<usize> = None;
    if gzip && head_ok {
        use std::io::Read;
        let mut decoder = flate2::read::GzDecoder::new(&bytes[at..]);
        let mut plain = Vec::new();
        match decoder.read_to_end(&mut plain) {
            Ok(_) => {
                if plain.len() == uncompressed {
                    inflated = Some(plain.len());
                } else {
                    issues.push(format!(
                        "inflated {} != declared {uncompressed}",
                        plain.len()
                    ));
                }
            }
            Err(e) => issues.push(format!("inflate failed: {e}")),
        }
    }
    lines.push(format!(
        "zero0 0x{zero0:016x}, u08 0x{u08:016x}, u48 0x{u48:016x}, data@{at} gzip={gzip}, \
         compressed {compressed}, uncompressed {uncompressed}{}",
        match inflated {
            Some(d) => format!(" (inflated ok {d})"),
            None => String::new(),
        }
    ));
    Report {
        ok: issues.is_empty(),
        issues,
        lines,
    }
}

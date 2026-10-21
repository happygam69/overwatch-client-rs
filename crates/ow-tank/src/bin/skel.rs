//! Look for the skeleton inside a model asset's chunks.
//!
//! `docs/formats/animation.md` records the runtime bone tables (0xA0-stride
//! nodes, 0xF8-stride hierarchy with `-1` roots, a 0x18-stride third table) but
//! not which *chunk* carries them; the model's `MRNM` top level does not (its
//! header is the mesh header `model.rs` already parses).  This tool lists every
//! chunk with its size and scans the candidates for skeleton-shaped content: an
//! array of parent indices with `-1` roots, a run of 0xA0-stride records, or a
//! block of bone names.
//!
//! ```text
//! ow-tank-skel --guid 0x0d00000000000109
//! ```
//!
//! Read-only.

use ow_tank::{chunks::Chunks, guid, GameData, ManifestSelector};

/// Read a stored record as a 3×4 affine: three rows of `[x y z t]`, which is
/// how DirectX stores a transform, padded to a 4×4 for the comparison.
fn three_by_four(m: &[[f32; 4]; 4]) -> glam::Mat4 {
    glam::Mat4::from_cols_array_2d(&[
        [m[0][0], m[1][0], m[2][0], 0.0],
        [m[0][1], m[1][1], m[2][1], 0.0],
        [m[0][2], m[1][2], m[2][2], 0.0],
        [m[0][3], m[1][3], m[2][3], 1.0],
    ])
}

fn u16_at(d: &[u8], at: usize) -> Option<u16> {
    d.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
}
fn u32_at(d: &[u8], at: usize) -> Option<u32> {
    d.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}
fn i32_at(d: &[u8], at: usize) -> Option<i32> {
    u32_at(d, at).map(|v| v as i32)
}
fn f32_at(d: &[u8], at: usize) -> Option<f32> {
    u32_at(d, at).map(f32::from_bits)
}

/// Runs of printable ASCII, at least `min` characters.
fn ascii_runs(d: &[u8], min: usize, limit: usize) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, &b) in d.iter().enumerate() {
        let printable = (0x20..0x7f).contains(&b);
        match (printable, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) if i - s >= min => {
                out.push((s, String::from_utf8_lossy(&d[s..i]).to_string()));
                start = None;
            }
            (false, Some(_)) => start = None,
            _ => {}
        }
        if out.len() >= limit {
            break;
        }
    }
    out
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let Some(g) =
        flag("--guid").and_then(|v| u64::from_str_radix(v.trim_start_matches("0x"), 16).ok())
    else {
        eprintln!("usage: ow-tank-skel --guid 0xGUID [--strings N] [--chunk TAG]");
        return std::process::ExitCode::from(2);
    };
    let strings: usize = flag("--strings").and_then(|v| v.parse().ok()).unwrap_or(0);

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

    println!("model 0x{g:016x} (type 0x{:03x})", guid::type_of(g));
    let bytes = match data.bytes_of(g) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("load failed: {e}");
            return std::process::ExitCode::from(1);
        }
    };
    let chunks = match Chunks::parse(&bytes) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("not a chunked asset: {e:?}");
            return std::process::ExitCode::from(1);
        }
    };
    println!("{} B, {} chunk(s)", bytes.len(), chunks.entries.len());
    for (i, c) in chunks.entries.iter().enumerate() {
        let indent = "  ".repeat(c.depth);
        let owns = if c.has_children() {
            " (owns nested chunks)"
        } else {
            ""
        };
        let tag = c.tag_str();
        let ver = c.version;
        let size = c.size;
        let off = c.offset;
        println!("  [{i:2}] {indent}{tag} v{ver}  {size} B at +{off:#x}{owns}");
    }

    // Try to read a chunk payload as an STU blob and print its GUID fields.
    if let Some(tag) = flag("--stu") {
        let want = tag.clone().into_bytes();
        match chunks.entries.iter().find(|c| c.tag == want[..]) {
            Some(c) => {
                let mut d = &bytes[c.offset..(c.offset + c.size).min(bytes.len())];
                // A model chunk that wraps an STU carries (offset, size) first
                // (the reference's ModelSTUHeader); a slice that parses as-is
                // is used directly.
                if let (Some(off), Some(size)) = (
                    d.get(0..8)
                        .map(|b| i64::from_le_bytes(b.try_into().unwrap())),
                    d.get(8..16)
                        .map(|b| i64::from_le_bytes(b.try_into().unwrap())),
                ) {
                    let (off, size) = (off.max(0) as usize, size.max(0) as usize);
                    if off + size <= d.len() && size > 0 {
                        println!("{tag}: inner STU at +{off:#x}, {size} B");
                        d = &d[off..off + size];
                    }
                }
                match ow_stu::Stu::parse(d.to_vec()) {
                    Ok(mut stu) => {
                        println!("{} parsed as STU: {} instance(s)", tag, stu.instances.len());
                        let instances = stu.instances.len();
                        for i in 0..instances {
                            let n = stu.fields(i).len();
                            let hash = stu.instances[i].hash;
                            println!("  [{i:2}] type 0x{hash:08x}  {n} field(s)");
                            let layout = stu.fields(i).to_vec();
                            for f in layout {
                                if let Some(g2) = stu.read_asset_ref(i, f.hash) {
                                    if g2 != 0 {
                                        println!(
                                            "        h 0x{:08x}: guid 0x{g2:016x} type 0x{:03x}",
                                            f.hash,
                                            guid::type_of(g2)
                                        );
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => println!("{tag} is not an STU blob: {e:?}"),
                }
            }
            None => println!("no {tag} chunk"),
        }
    }

    // Vertex element semantics per mesh: which attributes the file carries and
    // which the reader decodes (it knows 0 position, 1 normal, 8 colour,
    // 9 texcoord; a skinned mesh must also carry bone indices and weights).
    if args.iter().any(|a| a == "--elements") {
        match ow_tank::model::vertex_semantics(&bytes, &|g2| data.bytes_of(g2).ok()) {
            Ok(sem) => {
                println!("vertex attributes (semantic, index, format, submeshes):");
                for (semantic, index, format, n) in sem {
                    let name = match semantic {
                        0 => "position",
                        1 => "normal",
                        8 => "colour",
                        9 => "texcoord",
                        10 => "?10 (blend weights?)",
                        11 => "?11 (blend indices?)",
                        12 => "?12",
                        13 => "?13",
                        15 => "?15",
                        _ => "?",
                    };
                    println!("  semantic {semantic:3}  index {index}  format 0x{format:02x}  x{n}   {name}");
                }
            }
            Err(e) => println!("model parse: {e}"),
        }
    }

    // The bind-pose check: the file's inverted-bind array against the hierarchy
    // composed from the bind poses. They describe the same rest pose, so a
    // correct parse makes every `world(b) · inverse_bind(b)` the identity — the
    // property skinning depends on, and the one that shows a wrong matrix order.
    if args.iter().any(|a| a == "--bind-check") {
        match ow_tank::skeleton::parse_model(&bytes) {
            Ok(s) => {
                let has_file = s.inverse_bind.len() == s.bind_pose.len();
                println!(
                    "inverse-bind array: {} matrices, bind poses {}",
                    s.inverse_bind.len(),
                    s.bind_pose.len()
                );
                if let Some(first) = s.inverse_bind.first() {
                    println!("  bone 0 as stored: {first:?}");
                }
                let skin = ow_tank::skin::Skin::from_skeleton(&s);
                println!(
                    "  file array trusted: {}   bind-pose palette deviation: {:.6}",
                    skin.file_inverse_trusted,
                    skin.bind_palette_deviation()
                );
                // Every plausible reading of the stored 64-byte record,
                // printed rather than argued about: a 3x4 affine (row-major,
                // as DirectX stores it) padded out, its transpose, and the
                // column-major 4x4. Each is compared against both the composed
                // bind *world* matrices and their inverses.
                if has_file {
                    type Build = fn(&[[f32; 4]; 4]) -> glam::Mat4;
                    let candidates: [(&str, Build); 3] = [
                        ("3x4 row-major", three_by_four),
                        ("3x4 row-major transposed", |m| three_by_four(m).transpose()),
                        ("4x4 column-major", |m| glam::Mat4::from_cols_array_2d(m)),
                    ];
                    let world = skin.bind_world();
                    let composed = skin.composed_inverse_bind();
                    let diff = |set: &[glam::Mat4], want: &[glam::Mat4]| {
                        set.iter()
                            .zip(want)
                            .map(|(a, b)| {
                                a.to_cols_array()
                                    .iter()
                                    .zip(b.to_cols_array().iter())
                                    .map(|(p, q)| (p - q).abs())
                                    .fold(0.0f32, f32::max)
                            })
                            .fold(0.0f32, f32::max)
                    };
                    for (name, build) in candidates {
                        let set: Vec<glam::Mat4> = s.inverse_bind.iter().map(build).collect();
                        println!(
                            "  {name:>26}: vs bind world {:.6}, vs inverse bind {:.6}, vs bind locals {:.6}",
                            diff(&set, &world),
                            diff(&set, &composed),
                            diff(&set, &skin.local)
                        );
                    }
                }
            }
            Err(e) => println!("skeleton parse: {e}"),
        }
    }

    // The skeleton, from the mskl chunk, via the library parser.
    if args.iter().any(|a| a == "--skeleton") {
        match ow_tank::skeleton::parse_model(&bytes) {
            Ok(s) => {
                println!(
                    "skeleton: {} bones (abs), {} simple, {} cloth, {} ids, {} remap",
                    s.bones_abs,
                    s.bones_simple,
                    s.bones_cloth,
                    s.ids.len(),
                    s.remap.len()
                );
                println!("  forest: {}   roots: {:?}", s.is_forest(), s.roots());
                let mut depths = std::collections::BTreeMap::new();
                for i in 0..s.parents.len() {
                    if let Some(d) = s.depth(i) {
                        *depths.entry(d).or_insert(0usize) += 1;
                    }
                }
                let mut dist: Vec<(usize, usize)> = depths.into_iter().collect();
                dist.sort();
                println!("  depth histogram (depth: bones): {dist:?}");
                let maxd = dist.last().map(|d| d.0).unwrap_or(0);
                println!("  max depth {maxd}");
                for (i, b) in s.bind_pose.iter().enumerate().take(10) {
                    println!(
                        "  bone {i:3} parent {:4}  t ({:8.4},{:8.4},{:8.4})  q ({:6.3},{:6.3},{:6.3},{:6.3})  s ({:5.3},{:5.3},{:5.3}){}",
                        s.parents.get(i).copied().unwrap_or(-2),
                        b.translation[0], b.translation[1], b.translation[2],
                        b.orientation[0], b.orientation[1], b.orientation[2], b.orientation[3],
                        b.scale[0], b.scale[1], b.scale[2],
                        if s.ids.get(i).copied().unwrap_or(0) != 0 { format!("  id 0x{:08x}", s.ids[i]) } else { String::new() }
                    );
                }
                let mut bone_lengths: Vec<f32> = Vec::new();
                let children = s.children();
                for (i, kids) in children.iter().enumerate() {
                    for k in kids {
                        let a = s.bind_pose[i].translation;
                        let b = s.bind_pose[*k].translation;
                        bone_lengths.push(
                            ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2) + (b[2] - a[2]).powi(2))
                                .sqrt(),
                        );
                    }
                }
                if !bone_lengths.is_empty() {
                    bone_lengths.sort_by(|a, b| a.partial_cmp(b).unwrap());
                    let med = bone_lengths[bone_lengths.len() / 2];
                    println!(
                        "  parent-child distances: min {:.4} median {:.4} max {:.4} m (n = {})",
                        bone_lengths[0],
                        med,
                        bone_lengths[bone_lengths.len() - 1],
                        bone_lengths.len()
                    );
                }
            }
            Err(e) => println!("skeleton: {e}"),
        }
    }

    // Optional raw dump of one chunk: u32 / f32 / u16 columns.
    if let (Some(tag), Some(off), Some(len)) = (
        flag("--hex"),
        flag("--at").and_then(|v| usize::from_str_radix(v.trim_start_matches("0x"), 16).ok()),
        flag("--len").and_then(|v| usize::from_str_radix(v.trim_start_matches("0x"), 16).ok()),
    ) {
        let want = tag.clone().into_bytes();
        if let Some(c) = chunks.entries.iter().find(|c| c.tag == want[..]) {
            let d = &bytes[c.offset..(c.offset + c.size).min(bytes.len())];
            let end = (off + len).min(d.len());
            let mut a = off & !0xf;
            while a < end {
                let u = (0..4)
                    .map(|k| {
                        u32_at(d, a + k * 4)
                            .map(|v| format!("{v:>10}"))
                            .unwrap_or_default()
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                let f = (0..4)
                    .map(|k| {
                        f32_at(d, a + k * 4)
                            .map(|v| {
                                if v.is_finite() && v.abs() > 1e-6 && v.abs() < 1e6 {
                                    format!("{v:>9.3}")
                                } else {
                                    "        .".into()
                                }
                            })
                            .unwrap_or_default()
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                let h = (0..8)
                    .map(|k| {
                        u16_at(d, a + k * 2)
                            .map(|v| format!("{v:04x}"))
                            .unwrap_or_default()
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                println!("  +{a:#07x}  u32 {u}   f32 {f}  u16 {h}");
                a += 0x10;
            }
        } else {
            println!("no {tag} chunk");
        }
    }

    // Per chunk: skeleton-shaped scans.
    for (i, c) in chunks.entries.iter().enumerate() {
        let d = &bytes[c.offset..(c.offset + c.size).min(bytes.len())];
        let tag = c.tag_str();
        if strings > 0 {
            let runs = ascii_runs(d, 3, strings);
            if !runs.is_empty() {
                println!(
                    "  [{i}] {tag}: {} printable run(s), first {}:",
                    runs.len(),
                    runs.len().min(strings)
                );
                for (at, s) in runs.iter().take(strings) {
                    println!("        +{at:#08x} {s:?}");
                }
            }
        }
        // parent-index scan: a run of i32 in [-1, 4096) at a plausible stride
        for stride in [4usize, 8, 0x18, 0xa0, 0xf8] {
            let n = d.len().saturating_sub(stride) / stride;
            if n < 32 {
                continue;
            }
            let mut best = 0usize;
            let mut best_at = 0usize;
            let mut run = 0usize;
            for k in 0..n {
                let v = i32_at(d, k * stride + 4).unwrap_or(i32::MIN);
                if (-1..4096).contains(&v) {
                    run += 1;
                    if run > best {
                        best = run;
                        best_at = (k + 1 - run) * stride + 4;
                    }
                } else {
                    run = 0;
                }
            }
            if best >= 32 {
                println!(
                    "  [{i}] {tag}: {best} consecutive in-range i32 at stride {stride:#x} starting +{best_at:#x}"
                );
            }
        }
        // affine-matrix scan: 0x40-stride records whose last row is (0,0,0,1)
        // and whose 3x3 is finite.  A skeleton's bind pose is such an array.
        {
            let stride = 0x40usize;
            let n = d.len().saturating_sub(stride) / stride;
            let affine = |k: usize| -> bool {
                let at = k * stride;
                let last = (
                    f32_at(d, at + 0x30),
                    f32_at(d, at + 0x34),
                    f32_at(d, at + 0x38),
                    f32_at(d, at + 0x3c),
                );
                let m = (
                    f32_at(d, at),
                    f32_at(d, at + 4),
                    f32_at(d, at + 8),
                    f32_at(d, at + 0x10),
                    f32_at(d, at + 0x14),
                    f32_at(d, at + 0x18),
                    f32_at(d, at + 0x20),
                    f32_at(d, at + 0x24),
                    f32_at(d, at + 0x28),
                );
                // Translation may sit in the last row *or* the last column, so
                // the test is the shape, not the convention: sixteen finite
                // floats whose bottom-right element is 1.
                let _ = last;
                let ok_last = matches!(last, (Some(_), Some(_), Some(_), Some(e))
                    if (e - 1.0).abs() < 1e-3);
                let ok_m = matches!(m, (Some(a), Some(b), Some(c), Some(e), Some(f), Some(g2), Some(h), Some(i2), Some(j))
                    if [a, b, c, e, f, g2, h, i2, j].iter().all(|v| v.is_finite() && v.abs() < 100.0));
                ok_last && ok_m
            };
            let mut best = (0usize, 0usize);
            let mut run = 0usize;
            let mut start = 0usize;
            for k in 0..n {
                if affine(k) {
                    if run == 0 {
                        start = k * stride;
                    }
                    run += 1;
                    if run > best.0 {
                        best = (run, start);
                    }
                } else {
                    run = 0;
                }
            }
            if best.0 >= 8 {
                println!(
                    "  [{i}] {tag}: {} affine 4x4 matrices at stride 0x40 starting +{:#x}",
                    best.0, best.1
                );
                for k in 0..best.0.min(6) {
                    let at = best.1 + k * stride;
                    let t = (
                        f32_at(d, at + 0x30),
                        f32_at(d, at + 0x34),
                        f32_at(d, at + 0x38),
                    );
                    let r = (f32_at(d, at), f32_at(d, at + 0x10), f32_at(d, at + 0x20));
                    println!(
                        "        bone {k:3}  translation ({:8.3},{:8.3},{:8.3})  basis x ({:7.3},{:7.3},{:7.3})",
                        t.0.unwrap_or(f32::NAN), t.1.unwrap_or(f32::NAN), t.2.unwrap_or(f32::NAN),
                        r.0.unwrap_or(f32::NAN), r.1.unwrap_or(f32::NAN), r.2.unwrap_or(f32::NAN)
                    );
                }
            }
        }
        // parent-array scan: a run of i32 whose values are all < N with a few
        // roots (-1).  That is the hierarchy table's source shape
        // (animation.md), and nothing else in a model produces it by accident.
        {
            let n = d.len() / 4;
            let vals: Vec<i32> = (0..n)
                .map(|k| i32_at(d, k * 4).unwrap_or(i32::MIN))
                .collect();
            let mut k = 0usize;
            while k < vals.len() {
                if !(-1..8192).contains(&vals[k]) {
                    k += 1;
                    continue;
                }
                let start = k;
                while k < vals.len() && (-1..8192).contains(&vals[k]) {
                    k += 1;
                }
                let run = &vals[start..k];
                // try each prefix length that could be a bone count
                for len in (16..=run.len().min(512)).rev() {
                    let pref = &run[..len];
                    let max = pref.iter().copied().max().unwrap_or(-1);
                    let roots = pref.iter().filter(|v| **v == -1).count();
                    if (1..=8).contains(&roots) && max < len as i32 {
                        println!(
                            "  [{i}] {tag}: possible bone parent array: {len} entries at +{:#x}, {roots} root(s), max index {max}",
                            start * 4
                        );
                        let show: Vec<String> =
                            pref.iter().take(12).map(|v| v.to_string()).collect();
                        println!("        first {}: {}", show.len(), show.join(" "));
                        break;
                    }
                }
            }
        }
        // position-triple scan for the node table (0xA0 / 0x80 strides)
        for stride in [0x80usize, 0xa0] {
            let n = d.len().saturating_sub(stride) / stride;
            if n < 16 {
                continue;
            }
            let mut best = (0usize, 0usize);
            let mut run = 0usize;
            let mut start = 0usize;
            for k in 0..n {
                let at = k * stride;
                let (x, y, z) = (f32_at(d, at), f32_at(d, at + 4), f32_at(d, at + 8));
                let ok = matches!((x, y, z), (Some(x), Some(y), Some(z))
                    if x.is_finite() && y.is_finite() && z.is_finite()
                        && x.abs() < 200.0 && y.abs() < 200.0 && z.abs() < 200.0);
                if ok {
                    if run == 0 {
                        start = at;
                    }
                    run += 1;
                    if run > best.0 {
                        best = (run, start);
                    }
                } else {
                    run = 0;
                }
            }
            if best.0 >= 16 {
                println!(
                    "  [{i}] {tag}: {} consecutive plausible triples at stride {stride:#x} starting +{:#x}",
                    best.0, best.1
                );
            }
        }
    }
    std::process::ExitCode::SUCCESS
}

//! Bone indices: what the mesh stores, what the skeleton's Lookup holds, and
//! which rule joins them on the install's own data.
//!
//! The reference (`OverwatchModel.cs`) joins them as
//! `Lookup[raw + submesh.BoneIdOffset]`, with `0xFFFF` when the sum is past the
//! table. This probe measures the four candidate rules against the geometry
//! itself: the bind pose in `mskl` is **model space**, so a vertex's dominant
//! influence should name a bone whose bind translation is near that vertex.
//! The correct rule wins by a wide margin; a permuted bone set does not.
//!
//! usage:
//!   cargo run -p ow-tank --example bonemap_probe                 # four pinned rigs
//!   cargo run -p ow-tank --example bonemap_probe -- --census N   # first N model assets
//!   cargo run -p ow-tank --example bonemap_probe -- --all        # every model asset
//!   cargo run -p ow-tank --example bonemap_probe -- 0xGUID ...
//!
//! The census counts, over the install, every property the rule depends on:
//! how the Lookup is shaped, whether any `0xFFFF` hole exists, whether
//! `remap_count` is exactly the largest raw index plus one, whether any
//! submesh carries a nonzero `BoneIdOffset`, and which element formats the
//! blend-index attribute uses.

use ow_tank::{guid, GameData, ManifestSelector};

const DEFAULT_MODELS: [u64; 4] = [
    0x0d00_0000_0000_0109, // Tracer's character model
    0x0d00_0000_0000_00f1, // Tracer's first-person model
    0x0d00_0000_0000_06a4, // a view model
    0x0d00_0000_0000_0413, // the pistol
];

#[derive(Default)]
struct Dist {
    d: Vec<f32>,
}

impl Dist {
    fn new() -> Dist {
        Dist { d: Vec::new() }
    }
    fn summary(&self) -> String {
        if self.d.is_empty() {
            return "no vertices".into();
        }
        let mut v = self.d.clone();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mean = v.iter().sum::<f32>() / v.len() as f32;
        let at = |q: f32| v[((v.len() as f32 - 1.0) * q) as usize];
        format!(
            "n={:6}  med {:.4}  p90 {:.4}  max {:.4}  mean {:.3}",
            v.len(),
            at(0.5),
            at(0.9),
            at(1.0),
            mean
        )
    }
}

fn u16_at(d: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([d[at], d[at + 1]])
}

/// Everything one model says about the bone-index join.
#[derive(Default)]
struct Facts {
    bones_abs: usize,
    lookup_len: usize,
    unbound: usize,
    out_of_range: usize,
    distinct: usize,
    identity: bool,
    max_raw: Option<u16>,
    max_raw_plus_off: Option<u16>,
    offsets_nonzero: usize,
    offset_max: u16,
    vertices: usize,
    /// Vertices whose rule named no bone.
    unbound_raw: usize,
    unbound_off: usize,
    unbound_lookup: usize,
    unbound_lookup_off: usize,
    /// The dominant-influence distance sums per rule.
    dist: [Dist; 4],
}

fn facts_of(bytes: &[u8], data: &GameData) -> Option<Facts> {
    let skel = ow_tank::skeleton::parse_model(bytes).ok()?;
    let model = ow_tank::model::parse(bytes, &|id| data.bytes_of(id).ok()).ok()?;
    let lookup = &skel.remap;
    let mut f = Facts {
        bones_abs: skel.parents.len(),
        lookup_len: lookup.len(),
        unbound: lookup.iter().filter(|v| **v == 0xffff).count(),
        out_of_range: lookup
            .iter()
            .filter(|v| **v != 0xffff && **v as usize >= skel.bind_pose.len())
            .count(),
        identity: lookup.iter().enumerate().all(|(i, v)| *v as usize == i),
        dist: [Dist::new(), Dist::new(), Dist::new(), Dist::new()],
        ..Facts::default()
    };
    let mut sorted = lookup.clone();
    sorted.sort_unstable();
    sorted.dedup();
    f.distinct = sorted.len();

    for mesh in &model.meshes {
        if !mesh.skinned || mesh.bone_indices.is_empty() {
            continue;
        }
        if mesh.bone_id_offset != 0 {
            f.offsets_nonzero += 1;
        }
        f.offset_max = f.offset_max.max(mesh.bone_id_offset);
        for (v, raws) in mesh.bone_indices.iter().enumerate() {
            let w = mesh.blend_weights.get(v).copied().unwrap_or([0; 4]);
            for (lane, raw) in raws.iter().enumerate() {
                if w[lane] == 0 {
                    continue;
                }
                f.max_raw = Some(f.max_raw.map_or(*raw, |m| m.max(*raw)));
                let with = raw.saturating_add(mesh.bone_id_offset);
                f.max_raw_plus_off = Some(f.max_raw_plus_off.map_or(with, |m| m.max(with)));
            }
            let lane = (0..4).max_by_key(|l| w[*l]).unwrap();
            if w[lane] == 0 {
                continue;
            }
            f.vertices += 1;
            let raw = raws[lane];
            let off = raw as usize + mesh.bone_id_offset as usize;
            let mut c = [
                Some(raw as usize),
                Some(off),
                if lookup.is_empty() {
                    Some(raw as usize)
                } else {
                    lookup.get(raw as usize).map(|b| *b as usize)
                },
                if lookup.is_empty() {
                    Some(off)
                } else {
                    lookup.get(off).map(|b| *b as usize)
                },
            ];
            for (r, cand) in c.iter_mut().enumerate() {
                if cand.map(|b| b >= skel.bind_pose.len()).unwrap_or(true) {
                    *cand = None;
                }
                match cand {
                    Some(b) => {
                        let t = skel.bind_pose[*b].translation;
                        let p = mesh.positions[v];
                        let d = glam::Vec3::new(p[0] - t[0], p[1] - t[1], p[2] - t[2]).length();
                        f.dist[r].d.push(d);
                    }
                    None => match r {
                        0 => f.unbound_raw += 1,
                        1 => f.unbound_off += 1,
                        2 => f.unbound_lookup += 1,
                        _ => f.unbound_lookup_off += 1,
                    },
                }
            }
        }
    }
    Some(f)
}

/// The 8-influence variant: does any submesh declare a second four-wide blend
/// index set, and is the second set ever weighted?
fn multi_set_report(bytes: &[u8]) -> Option<(usize, usize, usize, usize, u32, u32, u16)> {
    let chunks = ow_tank::chunks::Chunks::parse(bytes).ok()?;
    let d = chunks.payload(bytes, b"MRNM")?;
    let i64_at = |at: usize| -> Option<i64> {
        Some(i64::from_le_bytes(d.get(at..at + 8)?.try_into().ok()?))
    };
    let u32_at = |at: usize| -> Option<u32> {
        Some(u32::from_le_bytes(d.get(at..at + 4)?.try_into().ok()?))
    };
    let u8_at = |at: usize| -> Option<u8> { d.get(at).copied() };
    let u16_at = |at: usize| -> Option<u16> {
        Some(u16::from_le_bytes(d.get(at..at + 2)?.try_into().ok()?))
    };
    let vbo_base = i64_at(0x00)? as usize;
    let sm_base = i64_at(0x10)? as usize;
    let sm_count = u32_at(0x1c)? as usize;
    let vbo_count = u8_at(0x22)? as usize;
    let mut multi_submeshes = 0usize;
    let mut weighted_second = 0usize;
    let mut weighted_vertices = 0usize;
    let mut max_second_weight = 0u8;
    let mut sum_lo = u32::MAX;
    let mut sum_hi = 0u32;
    let mut max_raw_both = 0u16;
    for s in 0..sm_count {
        let sb = sm_base + s * 0x80;
        let vb = u8_at(sb + 0x76)? as usize;
        if vb >= vbo_count {
            continue;
        }
        let vb_at = vbo_base + vb * 0x60;
        let elem_ptr = i64_at(vb_at + 0x10)? as usize;
        let elem_count = u8_at(vb_at + 0x0a)? as usize;
        let flags = u8_at(vb_at + 0x0b)?;
        if flags & 0x10 != 0 {
            continue;
        }
        let strides = [u8_at(vb_at + 0x08)? as usize, u8_at(vb_at + 0x09)? as usize];
        let bases = [
            i64_at(vb_at + 0x50).filter(|v| *v > 0)? as usize,
            i64_at(vb_at + 0x58)? as usize,
        ];
        // The elements this submesh's buffer declares.
        let mut idx0 = None;
        let mut idx1 = None;
        let mut w0 = None;
        let mut w1 = None;
        for e in 0..elem_count {
            let at = elem_ptr + e * 8;
            let sem = u8_at(at)?;
            let index = u8_at(at + 1)?;
            let stream = u8_at(at + 3)? as usize;
            let packed = u32_at(at + 4)?;
            let offset = ((packed >> 12) & 1023) as usize;
            match (sem, index) {
                (4, 0) => idx0 = Some((offset, stream)),
                (4, 1) => idx1 = Some((offset, stream)),
                (5, 0) => w0 = Some((offset, stream)),
                (5, 1) => w1 = Some((offset, stream)),
                _ => {}
            }
        }
        let (Some((o0, st0)), Some((o1, st1)), Some((ow0, stw0)), Some((ow, stw))) =
            (idx0, idx1, w0, w1)
        else {
            continue;
        };
        if idx0.is_none() || o1 >= strides[st1] || ow + 4 > strides[stw] {
            continue;
        }
        multi_submeshes += 1;
        let vertex_start = u32_at(sb + 0x3c)? as usize;
        let vertices = u16_at(sb + 0x68)? as usize;
        for v in 0..vertices {
            let vat = bases[st1] + (vertex_start + v) * strides[st1] + o1;
            let wat = bases[stw] + (vertex_start + v) * strides[stw] + ow;
            let vat0 = bases[st0] + (vertex_start + v) * strides[st0] + o0;
            let wat0 = bases[stw0] + (vertex_start + v) * strides[stw0] + ow0;
            let mut any = false;
            let mut sum = 0u32;
            for i in 0..4 {
                let w = u8_at(wat + i)?;
                sum += w as u32;
                max_second_weight = max_second_weight.max(w);
                any |= w != 0;
                sum += u8_at(wat0 + i)? as u32;
                max_raw_both = max_raw_both.max(u16_at(vat0 + i * 2)?);
                max_raw_both = max_raw_both.max(u16_at(vat + i * 2)?);
            }
            sum_lo = sum_lo.min(sum);
            sum_hi = sum_hi.max(sum);
            if any {
                weighted_second += 1;
            }
            weighted_vertices += 1;
        }
    }
    Some((
        multi_submeshes,
        weighted_second,
        weighted_vertices,
        max_second_weight as usize,
        sum_lo,
        sum_hi,
        max_raw_both,
    ))
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let census = args.iter().any(|a| a == "--census" || a == "--all");
    let all = args.iter().any(|a| a == "--all");
    let limit: usize = args
        .iter()
        .position(|a| a == "--census")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(usize::MAX);

    let root = ow_casc::install::detect_root().expect("no install");
    let sources = ow_tank::keytable::Sources::discover(std::slice::from_ref(&root));
    let data = GameData::open(
        root.display().to_string(),
        &ManifestSelector::default(),
        &sources,
    )
    .expect("open failed");

    if args.iter().any(|a| a == "--multi") {
        let mut guids = data.guids_of_type(guid::type_of(0x0d00_0000_0000_0000));
        guids.truncate(limit);
        let mut models = 0usize;
        let mut sum_lo = u32::MAX;
        let mut sum_hi = 0u32;
        let mut max_raw_any = 0u16;
        let mut lookup_short = 0usize;
        for g in &guids {
            let Ok(bytes) = data.bytes_of(*g) else {
                continue;
            };
            if let Some((subs, weighted, verts, maxw, lo, hi, maxraw)) = multi_set_report(&bytes) {
                if subs > 0 {
                    models += 1;
                    sum_lo = sum_lo.min(lo);
                    sum_hi = sum_hi.max(hi);
                    max_raw_any = max_raw_any.max(maxraw);
                    if let Ok(skel) = ow_tank::skeleton::parse_model(&bytes) {
                        if maxraw as usize >= skel.remap.len() {
                            lookup_short += 1;
                        }
                    }
                    if models <= 12 {
                        println!(
                            "0x{g:016x}: {subs} submeshes with a second blend-index set,                              {weighted}/{verts} vertices weighted there, max weight {maxw},                                  8-lane weight sum {lo}..{hi}, max raw over both sets {maxraw}"
                        );
                    }
                }
            }
        }
        println!(
            "models with a second (index 1) blend-index set: {models}; 8-lane weight sum              {sum_lo}..{sum_hi}; max raw over both sets {max_raw_any}; models whose second-set              raw index falls past the Lookup: {lookup_short}"
        );
        return std::process::ExitCode::SUCCESS;
    }

    if !census {
        let guids: Vec<u64> = {
            let a: Vec<u64> = args
                .iter()
                .filter_map(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
                .collect();
            if a.is_empty() {
                DEFAULT_MODELS.to_vec()
            } else {
                a
            }
        };
        for g in guids {
            println!("\n=== model 0x{g:016x} ===");
            let Ok(bytes) = data.bytes_of(g) else {
                println!("  load failed");
                continue;
            };
            let Some(f) = facts_of(&bytes, &data) else {
                println!("  not a skinned model this reader parses");
                continue;
            };
            let payload = ow_tank::chunks::Chunks::parse(&bytes)
                .ok()
                .and_then(|c| c.payload(&bytes, b"mskl").map(|p| p.to_vec()));
            if let Some(d) = payload.as_deref().filter(|d| d.len() >= 132) {
                println!(
                    "  header: bones_abs {} simple {} cloth {} remap_count {} id_count(116) {} count(130) {}",
                    u16_at(d, 108),
                    u16_at(d, 110),
                    u16_at(d, 112),
                    u16_at(d, 114),
                    u16_at(d, 116),
                    u16_at(d, 130)
                );
            }
            let skel = ow_tank::skeleton::parse_model(&bytes).expect("skeleton");
            let head: Vec<String> = skel.remap.iter().take(20).map(|v| v.to_string()).collect();
            println!(
                "  skeleton: {} bones, lookup {} ({} distinct, unbound {}, out of range {}, identity {}), max raw {:?}",
                f.bones_abs,
                f.lookup_len,
                f.distinct,
                f.unbound,
                f.out_of_range,
                f.identity,
                f.max_raw
            );
            println!("  lookup[0..20] = {}", head.join(" "));
            if let Some(d) = payload.as_deref() {
                // The two tables the client packs after the Lookup, at the
                // offsets `MsklTableBuild` copies from: +0x58 (bones_abs u16)
                // and +0x60 (remap_count x 8 bytes).
                let i64at = |at: usize| i64::from_le_bytes(d[at..at + 8].try_into().unwrap());
                let (t2, t3) = (i64at(88) as usize, i64at(96) as usize);
                if t2 > 0 && t2 + f.bones_abs * 2 <= d.len() {
                    let v: Vec<u16> = (0..f.bones_abs).map(|i| u16_at(d, t2 + i * 2)).collect();
                    let head: Vec<String> = v.iter().take(20).map(|x| x.to_string()).collect();
                    println!("  payload +88 [0..20] = {}", head.join(" "));
                    // Is it the inverse of the Lookup?
                    let mut inv_ok = 0usize;
                    for (slot, bone) in skel.remap.iter().enumerate() {
                        if v.get(*bone as usize).map(|s| *s as usize) == Some(slot) {
                            inv_ok += 1;
                        }
                    }
                    println!(
                        "  +88 inverse of the Lookup on {}/{} slots",
                        inv_ok,
                        skel.remap.len()
                    );
                }
                // The other arrays the client packs (`MsklTableBuild`), so the
                // record shapes are visible on real data.
                let a40 = i64at(40) as usize;
                if a40 > 0 && a40 + 32 * f.bones_abs <= d.len() {
                    let v: Vec<String> = (0..8)
                        .map(|i| {
                            u32::from_le_bytes(d[a40 + i * 4..a40 + i * 4 + 4].try_into().unwrap())
                                .to_string()
                        })
                        .collect();
                    println!("  payload +40 bone0 as u32[8] = {}", v.join(" "));
                }
                let a48 = i64at(48) as usize;
                if a48 > 0 && a48 + 4 * f.bones_abs <= d.len() {
                    let v: Vec<String> = (0..8)
                        .map(|i| {
                            u32::from_le_bytes(d[a48 + i * 4..a48 + i * 4 + 4].try_into().unwrap())
                                .to_string()
                        })
                        .collect();
                    println!("  payload +48 u32[0..8] = {}", v.join(" "));
                }
                let a64 = i64at(64) as usize;
                if a64 > 0 && a64 + 4 * f.bones_abs <= d.len() {
                    let v: Vec<String> = (0..8)
                        .map(|i| {
                            u32::from_le_bytes(d[a64 + i * 4..a64 + i * 4 + 4].try_into().unwrap())
                                .to_string()
                        })
                        .collect();
                    println!("  payload +64 u32[0..8] = {}", v.join(" "));
                }
                let a72 = i64at(72) as usize;
                let c130 = u16_at(d, 130) as usize;
                if a72 > 0 && a72 + 2 * c130 <= d.len() {
                    let v: Vec<String> = (0..c130.min(16))
                        .map(|i| u16_at(d, a72 + i * 2).to_string())
                        .collect();
                    let all: Vec<u16> = (0..c130).map(|i| u16_at(d, a72 + i * 2)).collect();
                    let max = all.iter().copied().max().unwrap_or(0);
                    let eq_parent = (0..c130)
                        .filter(|i| {
                            skel.parents.get(i + 1).copied().map(|p| p as i32)
                                == Some(all[*i] as i32)
                        })
                        .count();
                    println!(
                        "  payload +72 u16[0..16] = {} (count {}, max {}, equals parent[i+1] on {})",
                        v.join(" "),
                        c130,
                        max,
                        eq_parent
                    );
                }
                if t3 > 0 && t3 + f.lookup_len * 8 <= d.len() {
                    let pairs: Vec<String> = (0..f.lookup_len.min(12))
                        .map(|i| {
                            let a = u32::from_le_bytes(
                                d[t3 + i * 8..t3 + i * 8 + 4].try_into().unwrap(),
                            );
                            let b = u32::from_le_bytes(
                                d[t3 + i * 8 + 4..t3 + i * 8 + 8].try_into().unwrap(),
                            );
                            format!("({a},{b})")
                        })
                        .collect();
                    println!("  payload +96 pairs[0..12] = {}", pairs.join(" "));
                }
            }
            if let Ok(elements) = ow_tank::model::vertex_elements(&bytes) {
                let sem4: Vec<String> = elements
                    .iter()
                    .filter(|e| e.semantic == 4)
                    .map(|e| {
                        format!(
                            "{{index {}, fmt {:#04x}, off {}}}",
                            e.index, e.format, e.offset
                        )
                    })
                    .collect();
                println!(
                    "  semantic 4 elements ({}): {}",
                    sem4.len(),
                    sem4.join(", ")
                );
            }
            println!(
                "  submeshes with a nonzero BoneIdOffset: {} (max {})",
                f.offsets_nonzero, f.offset_max
            );
            for (name, r) in [
                "raw                      ",
                "raw + BoneIdOffset       ",
                "Lookup[raw]              ",
                "Lookup[raw + BoneIdOffset]",
            ]
            .iter()
            .zip(&f.dist)
            {
                println!("  {name}: {}", r.summary());
            }
            println!(
                "  vertices with no bone: raw {}, raw+off {}, lookup {}, lookup(raw+off) {}",
                f.unbound_raw, f.unbound_off, f.unbound_lookup, f.unbound_lookup_off
            );
        }
        return std::process::ExitCode::SUCCESS;
    }

    // Census over model assets.
    let gtype = guid::type_of(0x0d00_0000_0000_0000);
    let mut guids = data.guids_of_type(gtype);
    let total = guids.len();
    if !all {
        guids.truncate(limit);
    }
    println!("census: {} of {} model assets", guids.len(), total);
    let mut seen = 0usize;
    let mut skinned = 0usize;
    let mut with_lookup = 0usize;
    let mut identity = 0usize;
    let mut permuted = 0usize;
    let mut not_permutation = 0usize;
    let mut unbound = 0usize;
    let mut out_of_range = 0usize;
    let mut fit_raw = 0usize;
    let mut fit_off = 0usize;
    let mut fit_short = 0usize;
    let mut offsets_nonzero_models = 0usize;
    let mut offset_max = 0u16;
    let mut lookup_lens: Vec<usize> = Vec::new();
    let mut fmt_counts: std::collections::BTreeMap<(u8, u8), usize> = Default::default();
    let mut count_mismatch = [0usize; 3];
    let mut inverse_exact = 0usize;
    let mut inverse_holes_ok = 0usize;
    let mut vertices = 0usize;
    let mut vertices_unbound_raw = 0usize;
    let mut vertices_unbound_lookup = 0usize;
    let mut worst = Vec::new();
    for (n, g) in guids.iter().enumerate() {
        let Ok(bytes) = data.bytes_of(*g) else {
            continue;
        };
        if n % 500 == 0 {
            eprintln!("  {n}/{}", guids.len());
        }
        if let Ok(elements) = ow_tank::model::vertex_elements(&bytes) {
            for e in elements.iter().filter(|e| e.semantic == 4) {
                *fmt_counts.entry((e.index, e.format)).or_default() += 1;
            }
        }
        let Some(f) = facts_of(&bytes, &data) else {
            continue;
        };
        seen += 1;
        if f.vertices == 0 {
            continue;
        }
        skinned += 1;
        if let Some(d) = ow_tank::chunks::Chunks::parse(&bytes)
            .ok()
            .and_then(|c| c.payload(&bytes, b"mskl").map(|p| p.to_vec()))
            .filter(|d| d.len() >= 132)
        {
            if u16_at(&d, 110) as usize != u16_at(&d, 116) as usize {
                count_mismatch[0] += 1;
            }
            if u16_at(&d, 110) as usize != u16_at(&d, 130) as usize {
                count_mismatch[1] += 1;
            }
            if u16_at(&d, 116) as usize != u16_at(&d, 130) as usize {
                count_mismatch[2] += 1;
            }
            // The inverse table at +88: exact inverse of the Lookup, and
            // 0xFFFF exactly where a bone has no slot?
            let bones_abs = u16_at(&d, 108) as usize;
            let at = i64::from_le_bytes(d[88..96].try_into().unwrap()).max(0) as usize;
            if at > 0 && at + bones_abs * 2 <= d.len() {
                let Ok(skel) = ow_tank::skeleton::parse_model(&bytes) else {
                    continue;
                };
                let inv: Vec<u16> = (0..bones_abs).map(|i| u16_at(&d, at + i * 2)).collect();
                let exact = skel
                    .remap
                    .iter()
                    .enumerate()
                    .all(|(slot, bone)| inv.get(*bone as usize).copied() == Some(slot as u16));
                if exact {
                    inverse_exact += 1;
                }
                let holes = inv.iter().filter(|v| **v == 0xffff).count();
                if holes == bones_abs.saturating_sub(skel.remap.len()) {
                    inverse_holes_ok += 1;
                }
            }
        }
        if f.lookup_len == 0 {
            continue;
        }
        with_lookup += 1;
        lookup_lens.push(f.lookup_len);
        if f.identity {
            identity += 1;
        }
        if f.distinct == f.lookup_len && f.unbound == 0 && f.out_of_range == 0 {
            permuted += 1;
        } else {
            not_permutation += 1;
        }
        unbound += f.unbound;
        out_of_range += f.out_of_range;
        vertices += f.vertices;
        vertices_unbound_raw += f.unbound_raw;
        vertices_unbound_lookup += f.unbound_lookup;
        if f.offsets_nonzero > 0 {
            offsets_nonzero_models += 1;
            offset_max = offset_max.max(f.offset_max);
        }
        let Some(max_raw) = f.max_raw else { continue };
        let max_off = f.max_raw_plus_off.unwrap_or(max_raw);
        if max_raw as usize == f.lookup_len - 1 {
            fit_raw += 1;
        } else if max_off as usize == f.lookup_len - 1 {
            fit_off += 1;
        } else {
            fit_short += 1;
            if worst.len() < 40 {
                worst.push(format!(
                    "0x{g:016x}: lookup {}, max raw {max_raw}, max raw+off {max_off}, bones {}",
                    f.lookup_len, f.bones_abs
                ));
            }
        }
    }
    lookup_lens.sort_unstable();
    println!("parsed {seen} models ({skinned} with skinned meshes, {with_lookup} with a Lookup)");
    println!(
        "lookup: identity {identity}, permutation {permuted}, other {not_permutation}; \
         {} unbound entries, {} out-of-range entries",
        unbound, out_of_range
    );
    println!(
        "lookup length: min {}, med {}, max {}",
        lookup_lens.first().copied().unwrap_or(0),
        lookup_lens.get(lookup_lens.len() / 2).copied().unwrap_or(0),
        lookup_lens.last().copied().unwrap_or(0)
    );
    println!(
        "remap_count == max raw + 1: {fit_raw} models; == max(raw+offset) + 1: {fit_off} models; \
         neither: {fit_short}"
    );
    println!("models with a nonzero BoneIdOffset: {offsets_nonzero_models} (max {offset_max})");
    println!("semantic 4 (index, format) -> elements: {fmt_counts:?}");
    println!(
        "count mismatches: bones_simple != id_count(116) {}, bones_simple != count(130) {}, \
         id_count(116) != count(130) {}",
        count_mismatch[0], count_mismatch[1], count_mismatch[2]
    );
    println!(
        "dominant-influence vertices {vertices}: {} name no bone through the raw index,          {} through the Lookup",
        vertices_unbound_raw, vertices_unbound_lookup
    );
    println!(
        "inverse table at mskl +88: exact inverse of the Lookup on {inverse_exact} models,          0xFFFF exactly on the unmapped bones on {inverse_holes_ok}"
    );
    for w in &worst {
        println!("  {w}");
    }
    std::process::ExitCode::SUCCESS
}

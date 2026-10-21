//! Find which installed asset carries a given STU instance type.
//!
//! The animation hunt needs the reverse of the usual lookup. The components that
//! name an idle animation are known by *hash* (`STUMirroredIdleAnimComponent`
//! 0x3AA5CB04, `STULocalIdleAnimComponent` 0x9911A927) and the clip data is an
//! **inline** instance inside them (`STUAnimGameData_Animation` 0xE93236BC, with
//! a `STUAnimGameData_Skeleton` 0x2BA91EC5 beside it), so "where is the clip" is
//! really "which asset lists that instance type" — a question no field name
//! answers, because the field hashes are unnamed in the reference types.
//!
//! ```text
//! ow-tank-anim --census                       # installed assets per type
//! ow-tank-anim --scan 0x003 --limit 2000      # assets of that type that name a target instance
//! ow-tank-anim --walk 0x0400000000000022 --depth 3
//! ow-tank-anim --bones 0x0a0000000000005e     # clip body: bone tables, quaternion data
//! ow-tank-anim --bones all --limit 200        # the same over every installed clip, summarised
//! ```
//!
//! `--scan` opens every installed asset of the named types and prints the ones
//! whose instance list contains any target hash, with those instances' fields
//! (GUID fields resolved to their target type, so an asset reference is visible
//! even when its field has no name). `--walk` follows asset references from one
//! GUID and reports the instance types it reaches, which is how a component gets
//! attributed to an owner. `--bones` runs the clip-body finders
//! (`clip::find_bone_tables`, `clip::find_float_regions`) over a clip and
//! prints the bone-table/float-data pairing — the forensic check for the
//! quaternion pose decode.
//!
//! Read-only: opens the install the same way the rest of the tree does.

use std::collections::{BTreeMap, HashSet, VecDeque};

use ow_tank::clip::{self, Clip};
use ow_tank::{guid, GameData, ManifestSelector};

/// The asset type of animation clips (from the census: 28,972 installed).
const ANIM_CLIP_TYPE: u16 = 0x006;

/// The instance types this tool hunts by default, with the names the reference
/// checkout gives them (`TankLib/STU/Types/*.cs`).
const DEFAULT_HASHES: [(u32, &str); 5] = [
    (0x3AA5CB04, "STUMirroredIdleAnimComponent"),
    (0x9911A927, "STULocalIdleAnimComponent"),
    (0xE93236BC, "STUAnimGameData_Animation"),
    (0x2BA91EC5, "STUAnimGameData_Skeleton"),
    (0x65DD9C84, "STUAnimGameData_AnimationData"),
];

fn label(hash: u32) -> &'static str {
    DEFAULT_HASHES
        .iter()
        .find(|(h, _)| *h == hash)
        .map(|(_, n)| *n)
        .unwrap_or("")
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
        eprintln!("usage: ow-tank-anim (--census | --scan TYPES | --walk GUID | --bones GUID|all) [--depth N] [--limit N] [--hashes 0x…,0x…] [--fields]");
        return std::process::ExitCode::from(2);
    }

    let hashes: Vec<u32> = match flag("--hashes") {
        Some(list) => list
            .split(',')
            .filter_map(|h| u32::from_str_radix(h.trim().trim_start_matches("0x"), 16).ok())
            .collect(),
        None => DEFAULT_HASHES.iter().map(|(h, _)| *h).collect(),
    };
    let limit: usize = flag("--limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(usize::MAX);
    let fields = args.iter().any(|a| a == "--fields");

    if let Some(p) = flag("--parse") {
        // Parse a clip chunk the operator extracted themselves (a path, never
        // bundled data) and print the container the corpus established.
        let bytes = match std::fs::read(&p) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("read {p} failed: {e}");
                return std::process::ExitCode::from(1);
            }
        };
        match ow_tank::clip::Clip::parse(&bytes) {
            Ok(c) => {
                println!(
                    "{p}: {} B  family 0x{:08x}  frames {}  fps {}  duration {:.4} (implied {:.4})  rate {:.4}  period {:.5}  id 0x{:08x}  slot {}",
                    bytes.len(),
                    c.header.family,
                    c.header.frames,
                    c.header.fps,
                    c.header.duration,
                    c.header.implied_duration(),
                    c.header.rate,
                    c.header.frame_period,
                    c.header.id,
                    c.header.slot
                );
                print!("  sections:");
                for (i, v) in c.sections.iter().enumerate() {
                    if *v != 0 && *v != -1 {
                        print!(" [{i}]={v}");
                    }
                }
                println!();
                println!("  first section: {} words", c.first_section.len());
                println!("  second section at {}", c.second_at);
                println!("  body: {} B", c.body.len());
            }
            Err(e) => {
                eprintln!("{p}: {e}");
                return std::process::ExitCode::from(1);
            }
        }
        return std::process::ExitCode::SUCCESS;
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

    if args.iter().any(|a| a == "--census") {
        let hist = data.type_histogram();
        let total: usize = hist.values().sum();
        println!("{total} installed asset(s) across {} type(s)", hist.len());
        let mut rows: Vec<(u16, usize)> = hist.into_iter().collect();
        rows.sort_by(|a, b| b.1.cmp(&a.1));
        for (t, n) in rows.iter().take(40) {
            println!("  0x{t:03x}  {n}");
        }
        return std::process::ExitCode::SUCCESS;
    }

    if let Some(g) = flag("--dump") {
        let Ok(guid) = u64::from_str_radix(g.trim_start_matches("0x"), 16) else {
            eprintln!("--dump wants a hex GUID");
            return std::process::ExitCode::from(2);
        };
        let instance: Option<usize> = flag("--instance").and_then(|v| v.parse().ok());
        if instance.is_none() {
            // No instance named: dump every instance's fields, which is what turns a
            // list of rig entities into a table (model → clip).
            let Ok(mut stu) = data.stu_of(guid) else {
                eprintln!("0x{guid:016x}: not a readable STU");
                return std::process::ExitCode::from(1);
            };
            println!(
                "0x{guid:016x} type 0x{:03x}  {} instance(s)",
                guid::type_of(guid),
                stu.instances.len()
            );
            for i in 0..stu.instances.len() {
                println!(
                    "  [{i:2}] 0x{:08x} {}",
                    stu.instances[i].hash,
                    label(stu.instances[i].hash)
                );
                dump_fields(&mut stu, i);
            }
            return std::process::ExitCode::SUCCESS;
        }
        let hex_len: usize = flag("--hex").and_then(|v| v.parse().ok()).unwrap_or(256);
        dump(&data, guid, instance, hashes.as_slice(), hex_len);
        return std::process::ExitCode::SUCCESS;
    }

    if let Some(t) = flag("--list") {
        // Enumerate installed assets of one type, optionally writing their
        // decrypted bytes out: a chunk format with no reference reader is
        // legible by comparing several samples, not by staring at one.
        let Ok(type_id) = u16::from_str_radix(t.trim_start_matches("0x"), 16) else {
            eprintln!("--list wants a type number");
            return std::process::ExitCode::from(2);
        };
        let guids = data.guids_of_type(type_id);
        println!("type 0x{type_id:03x}: {} asset(s)", guids.len());
        let write = args.iter().any(|a| a == "--raw");
        let mut written = 0usize;
        for guid in guids.iter().take(limit) {
            match data.bytes_of(*guid) {
                Ok(bytes) => {
                    println!("  0x{guid:016x}  {} B", bytes.len());
                    if write {
                        let out = format!("D:/ow-dumps/asset_{guid:016x}.bin");
                        match std::fs::write(&out, &bytes) {
                            Ok(()) => written += 1,
                            Err(e) => eprintln!("    write {out} failed: {e}"),
                        }
                    }
                }
                Err(e) => println!("  0x{guid:016x}  unreadable: {e}"),
            }
        }
        if write {
            println!("wrote {written} file(s) to D:/ow-dumps/");
        }
        return std::process::ExitCode::SUCCESS;
    }

    if let Some(g) = flag("--raw") {
        // Write an asset's decrypted bytes to a file so a script can chew on a
        // chunk format the reference has no reader for (the animation asset is
        // one). The path must be outside the repository: game bytes never get
        // committed.
        let Ok(guid) = u64::from_str_radix(g.trim_start_matches("0x"), 16) else {
            eprintln!("--raw wants a hex GUID");
            return std::process::ExitCode::from(2);
        };
        let out = flag("--out").unwrap_or_else(|| format!("D:/ow-dumps/asset_{guid:016x}.bin"));
        match data.bytes_of(guid) {
            Ok(bytes) => {
                if let Err(e) = std::fs::write(&out, &bytes) {
                    eprintln!("write {out} failed: {e}");
                    return std::process::ExitCode::from(1);
                }
                println!(
                    "0x{guid:016x} type 0x{:03x}: {} B -> {out}",
                    guid::type_of(guid),
                    bytes.len()
                );
            }
            Err(e) => {
                eprintln!("0x{guid:016x}: {e}");
                return std::process::ExitCode::from(1);
            }
        }
        return std::process::ExitCode::SUCCESS;
    }

    if let Some(types) = flag("--scan") {
        let types: Vec<u16> = types
            .split(',')
            .filter_map(|t| u16::from_str_radix(t.trim().trim_start_matches("0x"), 16).ok())
            .collect();
        for t in types {
            let guids = data.guids_of_type(t);
            println!(
                "=== type 0x{t:03x}: {} installed asset(s){}",
                guids.len(),
                if guids.len() > limit {
                    format!(", scanning {limit}")
                } else {
                    String::new()
                }
            );
            let mut opened = 0usize;
            let mut hits = 0usize;
            let mut skips: BTreeMap<String, usize> = BTreeMap::new();
            for guid in guids.iter().take(limit) {
                let mut stu = match data.stu_of(*guid) {
                    Ok(s) => s,
                    Err(e) => {
                        *skips.entry(e.to_string()).or_default() += 1;
                        continue;
                    }
                };
                opened += 1;
                let found: Vec<usize> = (0..stu.instances.len())
                    .filter(|i| hashes.contains(&stu.instances[*i].hash))
                    .collect();
                if found.is_empty() {
                    continue;
                }
                hits += 1;
                println!(
                    "  0x{guid:016x}  {} instance(s), {} of interest",
                    stu.instances.len(),
                    found.len()
                );
                for i in found {
                    let hash = stu.instances[i].hash;
                    let note = if fields {
                        String::new()
                    } else {
                        format!("  ({} field(s); --fields to dump)", stu.fields(i).len())
                    };
                    println!("      [{i:2}] 0x{hash:08x} {}{note}", label(hash));
                    if fields {
                        dump_fields(&mut stu, i);
                    }
                }
            }
            println!("  scanned {opened} asset(s), {hits} with a target instance");
            if !skips.is_empty() {
                let skipped: usize = skips.values().sum();
                println!("  {skipped} did not open:");
                for (why, n) in skips.iter().take(4) {
                    println!("      {n} × {why}");
                }
            }
        }
        return std::process::ExitCode::SUCCESS;
    }

    if let Some(which) = flag("--channels") {
        let guids: Vec<u64> = if which.eq_ignore_ascii_case("all") {
            data.guids_of_type(ANIM_CLIP_TYPE)
                .iter()
                .copied()
                .take(limit)
                .collect()
        } else {
            let Ok(g) = u64::from_str_radix(which.trim_start_matches("0x"), 16) else {
                eprintln!("--channels wants a hex GUID or 'all'");
                return std::process::ExitCode::from(2);
            };
            vec![g]
        };
        let (mut unread, mut bad, mut parsed, mut clips_with, mut chans) =
            (0usize, 0usize, 0usize, 0usize, 0usize);
        let mut keys_total = 0usize;
        let mut shown = 0usize;
        for guid_v in &guids {
            let bytes = match data.bytes_of(*guid_v) {
                Ok(b) => b,
                Err(_) => {
                    unread += 1;
                    continue;
                }
            };
            let clip = match Clip::parse(&bytes) {
                Ok(c) => c,
                Err(_) => {
                    bad += 1;
                    continue;
                }
            };
            parsed += 1;
            let cs = clip::decode_channels(&clip.body, clip.header.frames);
            if cs.is_empty() {
                continue;
            }
            clips_with += 1;
            chans += cs.len();
            for c in &cs {
                keys_total += c.keys.len();
                if shown < 6 {
                    println!(
                        "0x{guid_v:016x} family 0x{:08x}, {} frames @ {} fps",
                        clip.header.family, clip.header.frames, clip.header.fps
                    );
                    let dense = c
                        .bones
                        .iter()
                        .enumerate()
                        .all(|(i, b)| *b as usize == i + 1);
                    let bones_desc = if dense {
                        format!("1..={}", c.bones.len())
                    } else {
                        format!(
                            "{} sparse, 1..={} (gaps: {:?})",
                            c.bones.len(),
                            c.bones.last().copied().unwrap_or(0),
                            {
                                let mut g: Vec<u8> = c
                                    .bones
                                    .iter()
                                    .enumerate()
                                    .filter(|(i, b)| **b as usize != i + 1)
                                    .map(|(_, b)| *b)
                                    .collect();
                                if g.len() > 6 {
                                    g.truncate(6);
                                    g.push(0xfe);
                                }
                                g
                            }
                        )
                    };
                    println!(
                        "  channel @+0x{:x}: {} bones [{}], {} keyed frames {:?}, {} rotations, values @+0x{:x}",
                        c.offset,
                        c.bones.len(),
                        bones_desc,
                        c.keys.len(),
                        c.keys,
                        c.rotations.len(),
                        c.values_offset
                    );
                    if let Some(r) = c.rotations.first() {
                        let (x, y, z, w) = r.quat();
                        let m = (x * x + y * y + z * z + w * w).sqrt();
                        println!(
                            "    key {}: q = ({:.6}, {:.6}, {:.6}, {:.6})  |q| = {:.6}",
                            c.keys[0], x, y, z, w, m
                        );
                    }
                    shown += 1;
                }
            }
        }
        println!(
            "channels: {} unreadable, {} bad header, {} parsed, {} with >=1 channel, \
             {} channels total, {} keyed frames total",
            unread, bad, parsed, clips_with, chans, keys_total
        );
        {
            // Bone-count histogram over every decoded channel: this is how a
            // clip is matched to a rig, since a channel's table names the bones
            // it drives and the largest bone index bounds the rig it belongs to.
            let mut hist: std::collections::BTreeMap<usize, usize> = Default::default();
            let mut clips: Vec<u64> = Vec::new();
            for guid_v in &guids {
                let Ok(bytes) = data.bytes_of(*guid_v) else {
                    continue;
                };
                let Ok(clip) = Clip::parse(&bytes) else {
                    continue;
                };
                let cs = clip::decode_channels(&clip.body, clip.header.frames);
                if cs.is_empty() {
                    continue;
                }
                for c in &cs {
                    *hist.entry(c.bones.len()).or_default() += 1;
                }
                clips.push(*guid_v);
            }
            let mut v: Vec<_> = hist.into_iter().collect();
            v.sort_by_key(|(n, c)| (std::cmp::Reverse(*c), *n));
            // Both headers, printed together so the disagreement stays visible
            // in the tool output and not only in the docs.
            if let Some(g0) = guids.first() {
                if let Ok(b) = data.bytes_of(*g0) {
                    if let (Some(a), Some(c)) =
                        (clip::Header::read(&b), clip::ReferenceHeader::read(&b))
                    {
                        println!(
                            "header ours:       family {:#010x} frames {} fps {} dur {}",
                            a.family, a.frames, a.fps, a.duration
                        );
                        println!(
                            "header reference:  bone_count {} dur {} fps {} frame_count {}",
                            c.bone_count,
                            c.duration,
                            c.fps,
                            c.info_table_size().saturating_sub(1)
                        );
                        println!(
                            "header offsets:    reference bone_list {:#x} info_table {:#x} some_table {:#x} (family>>16 = {})",
                            c.bone_list_offset, c.info_table_offset, c.some_table_offset,
                            a.family >> 16
                        );
                    }
                }
            }
            println!("bone-count histogram over decoded channels (bones: channels), top 20:");
            for (n, c) in v.iter().take(20) {
                println!("  {n:4} bones: {c:6}");
            }
            println!("{} clips carry at least one decoded channel", clips.len());
            let mut fam: std::collections::BTreeMap<u32, usize> = Default::default();
            for g in &guids {
                let Ok(b) = data.bytes_of(*g) else { continue };
                let Ok(c) = Clip::parse(&b) else { continue };
                *fam.entry(c.header.family).or_default() += 1;
            }
            println!("distinct families: {}", fam.len());
            // The OWLib-format decoder (packed 6-byte rotations) measured over
            // the whole corpus. This is the format the reference implementation
            // reads and the one verified byte-for-byte against a known clip.
            let (mut new_rotation_clips, mut new_rotation_tracks) = (0usize, 0usize);
            let mut rt_clips = 0usize;
            let mut rt_tracks = 0usize;
            let mut rt_keys = 0usize;
            for g in &guids {
                let Ok(b) = data.bytes_of(*g) else { continue };
                let Ok(c) = Clip::parse(&b) else { continue };
                let ts = clip::decode_rotation_tracks(&c.body, c.header.frames);
                let ts2 = clip::decode_rotation_tracks_new(&c.body, c.header.frames);
                if ts.is_empty() {
                    continue;
                }
                if !ts2.is_empty() {
                    new_rotation_clips += 1;
                    new_rotation_tracks += ts2.len();
                }
                if ts.is_empty() {
                    continue;
                }
                rt_clips += 1;
                rt_tracks += ts.len();
                rt_keys += ts.iter().map(|t| t.keys.len()).sum::<usize>();
            }
            println!(
                "owlib-format rotation tracks (legacy 6B): {} clips, {} tracks, {} keyed frames",
                rt_clips, rt_tracks, rt_keys
            );
            println!(
                "owlib-format rotation tracks (new 8B 0x400): {} clips, {} tracks",
                new_rotation_clips, new_rotation_tracks
            );
            let (mut vt_clips, mut vt_scale, mut vt_pos, mut vt_keys) =
                (0usize, 0usize, 0usize, 0usize);
            for g in &guids {
                let Ok(b) = data.bytes_of(*g) else { continue };
                let Ok(c) = Clip::parse(&b) else { continue };
                let vs = clip::decode_vec_tracks(&c.body, c.header.frames);
                if vs.is_empty() {
                    continue;
                }
                vt_clips += 1;
                for t in &vs {
                    match t.track {
                        clip::TrackKind::Scale => vt_scale += 1,
                        _ => vt_pos += 1,
                    }
                    vt_keys += t.keys.len();
                }
            }
            println!(
                "owlib-format position/scale tracks: {} clips, {} scale, {} position, {} keyed frames",
                vt_clips, vt_scale, vt_pos, vt_keys
            );
            let mut fv: Vec<_> = fam.into_iter().collect();
            fv.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
            println!("family histogram over parsed clips:");
            for (f, c) in fv.iter().take(15) {
                println!("  0x{f:08x}: {c} clips");
            }
        }
        return std::process::ExitCode::SUCCESS;
    }

    if let Some(bones) = flag("--bones") {
        let which: Vec<u64> = if bones.eq_ignore_ascii_case("all") {
            data.guids_of_type(ANIM_CLIP_TYPE)
                .iter()
                .copied()
                .take(limit)
                .collect()
        } else {
            let Ok(g) = u64::from_str_radix(bones.trim_start_matches("0x"), 16) else {
                eprintln!("--bones wants a hex GUID or 'all'");
                return std::process::ExitCode::from(2);
            };
            vec![g]
        };
        let mut stats = BoneStats::default();
        let mut shown = 0usize;
        for guid_v in which {
            let bytes = match data.bytes_of(guid_v) {
                Ok(b) => b,
                Err(e) => {
                    stats.unread += 1;
                    if shown < 3 && !bones.eq_ignore_ascii_case("all") {
                        eprintln!("0x{guid_v:016x}: {e}");
                    }
                    continue;
                }
            };
            let clip = match Clip::parse(&bytes) {
                Ok(c) => c,
                Err(e) => {
                    stats.bad_header += 1;
                    if shown < 10 {
                        println!("  0x{guid_v:016x}: header refused: {e}");
                        shown += 1;
                    }
                    continue;
                }
            };
            let tables = clip::find_bone_tables(&clip.body);
            let regions = clip::find_float_regions(&clip.body);
            let pairs = clip::pair_bone_tables_with_floats(&clip.body, &tables, &regions);
            stats.parsed += 1;
            stats.with_bones += usize::from(!pairs.is_empty());
            for (n, count) in table_histogram(&tables) {
                *stats.bone_counts.entry(n).or_default() += count;
            }
            if pairs.is_empty() || bones.eq_ignore_ascii_case("all") {
                continue;
            }
            if shown < 5 {
                println!(
                    "0x{guid_v:016x} family 0x{:08x}, {} frames @ {} fps, body {} B",
                    clip.header.family,
                    clip.header.frames,
                    clip.header.fps,
                    clip.body.len()
                );
                for (off, count, floats_off, rotations) in &pairs {
                    println!(
                        "  bone table @+0x{:x} ({} bones) -> floats @+0x{:x}, first bone q = {:?} (|q| {:.6})",
                        off,
                        count,
                        floats_off,
                        rotations
                            .first()
                            .map(clip::BoneRotation::quat)
                            .unwrap_or_default(),
                        rotations
                            .first()
                            .map(|r| (r.x * r.x + r.y * r.y + r.z * r.z + r.w() * r.w()).sqrt())
                            .unwrap_or(0.0)
                    );
                }
                shown += 1;
            }
        }
        println!(
            "clips: {} unreadable, {} bad header, {} parsed, {} with a bone table + quaternion pair",
            stats.unread, stats.bad_header, stats.parsed, stats.with_bones
        );
        if !stats.bone_counts.is_empty() {
            println!("bone-count histogram (over every table, every clip):");
            for (n, count) in stats.bone_counts.iter().take(20) {
                println!("  {count:4} x {n} bones");
            }
        }
        return std::process::ExitCode::SUCCESS;
    }

    if let Some(start) = flag("--walk") {
        let Ok(root_guid) = u64::from_str_radix(start.trim_start_matches("0x"), 16) else {
            eprintln!("--walk wants a hex GUID");
            return std::process::ExitCode::from(2);
        };
        let depth: usize = flag("--depth").and_then(|v| v.parse().ok()).unwrap_or(2);
        walk(&data, root_guid, depth, &hashes, fields, limit);
        return std::process::ExitCode::SUCCESS;
    }

    eprintln!("nothing to do: pass --census, --scan, --walk or --bones");
    std::process::ExitCode::from(2)
}

/// Sweep counters for `--bones`.
#[derive(Default)]
struct BoneStats {
    unread: usize,
    bad_header: usize,
    parsed: usize,
    with_bones: usize,
    bone_counts: BTreeMap<usize, usize>,
}

/// Histogram each table's bone count over the whole sweep.
fn table_histogram(tables: &[clip::BoneTable]) -> Vec<(usize, usize)> {
    let mut by_count: BTreeMap<usize, usize> = BTreeMap::new();
    for t in tables {
        *by_count.entry(t.bone_count as usize).or_default() += 1;
    }
    by_count.into_iter().collect()
}

/// Print one instance's fields, resolving GUID fields to their target type.
fn dump_fields(stu: &mut ow_stu::Stu, i: usize) {
    let layout = stu.fields(i).to_vec();
    let base = stu.payload_start(i);
    for f in layout {
        let Some((at, len)) = stu.field_bytes(i, f.hash) else {
            continue;
        };
        let mut shown = String::new();
        if len == 8 {
            if let Some(g) = stu.read_asset_ref(i, f.hash) {
                if g != 0 {
                    shown = format!(
                        "guid 0x{g:016x} type 0x{:03x} index {}",
                        guid::type_of(g),
                        guid::index_of(g)
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
        println!("          +{at:#07x} h 0x{:08x} len {len}: {shown}", f.hash);
    }
}

/// Print one asset's whole instance list, and the bytes of one instance.
///
/// The pose data is a blob of arrays referenced by offset, so the hex is the
/// evidence: the field dump gives the offsets, this gives what they point at.
fn dump(data: &GameData, guid: u64, instance: Option<usize>, hashes: &[u32], hex_len: usize) {
    let Ok(mut stu) = data.stu_of(guid) else {
        eprintln!("0x{guid:016x}: not a readable STU");
        return;
    };
    println!(
        "0x{guid:016x} type 0x{:03x}: {} instance(s)",
        guid::type_of(guid),
        stu.instances.len()
    );
    for i in 0..stu.instances.len() {
        let hash = stu.instances[i].hash;
        println!(
            "  [{i:2}] 0x{hash:08x} {}{}{}",
            label(hash),
            if hashes.contains(&hash) {
                "  <-- target"
            } else {
                ""
            },
            if instance == Some(i) {
                "  <-- dumping"
            } else {
                ""
            }
        );
    }
    let Some(i) = instance else { return };
    println!("--- instance {i} fields");
    dump_fields(&mut stu, i);
    let start = stu.payload_start(i);
    let data_len = stu.data(i).len();
    println!("--- instance {i} payload {data_len} B at +0x{start:x}");
    let bytes = stu.data(i).to_vec();
    for (row, chunk) in bytes.chunks(16).enumerate() {
        if row * 16 >= hex_len {
            break;
        }
        let text: String = chunk
            .iter()
            .map(|b| {
                if (0x20..0x7f).contains(b) {
                    *b as char
                } else {
                    '.'
                }
            })
            .collect();
        println!(
            "  +{:#06x}  {}  |{text}|",
            row * 16,
            chunk
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
    }
}

/// Follow asset references from one GUID, reporting the instance types reached.
///
/// The point is attribution: a component's own asset can be one hop from the
/// hero's entity through an embedded child-entity definition, and the reference
/// types name neither the field nor the child.
fn walk(data: &GameData, start: u64, depth: usize, hashes: &[u32], fields: bool, limit: usize) {
    let mut seen: HashSet<u64> = HashSet::new();
    let mut queue: VecDeque<(u64, usize)> = VecDeque::new();
    queue.push_back((start, 0));
    let mut reached = 0usize;
    while let Some((guid, d)) = queue.pop_front() {
        if !seen.insert(guid) || reached >= limit {
            continue;
        }
        reached += 1;
        let ty = guid::type_of(guid);
        println!(
            "{}0x{guid:016x}  type 0x{ty:03x}  depth {d}",
            "  ".repeat(d)
        );
        let Ok(mut stu) = data.stu_of(guid) else {
            println!("{}  (not a readable STU)", "  ".repeat(d));
            continue;
        };
        let mut hist: BTreeMap<u32, usize> = BTreeMap::new();
        for inst in &stu.instances {
            *hist.entry(inst.hash).or_default() += 1;
        }
        for (hash, n) in &hist {
            let mark = if hashes.contains(hash) {
                "  <-- target"
            } else {
                ""
            };
            println!(
                "{}  0x{hash:08x} x{n}{}{mark}",
                "  ".repeat(d),
                if label(*hash).is_empty() {
                    String::new()
                } else {
                    format!(" {}", label(*hash))
                }
            );
        }
        // Collect the asset references this asset carries, before dropping `stu`.
        let mut refs: Vec<u64> = Vec::new();
        for i in 0..stu.instances.len() {
            for f in stu.fields(i).to_vec() {
                if let Some(g) = stu.read_asset_ref(i, f.hash) {
                    if g != 0 && !seen.contains(&g) {
                        refs.push(g);
                    }
                }
            }
        }
        for g in refs {
            if d + 1 <= depth {
                queue.push_back((g, d + 1));
            } else if depth > 0 {
                println!("{}  (ref 0x{g:016x} beyond depth)", "  ".repeat(d));
            }
        }
        if fields {
            for i in 0..stu.instances.len() {
                if hashes.contains(&stu.instances[i].hash) {
                    dump_fields(&mut stu, i);
                }
            }
        }
    }
    println!("walked {reached} asset(s)");
}

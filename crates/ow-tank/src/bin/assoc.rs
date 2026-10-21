//! Associate assets OWLib-style and optionally export JSON just like TankTonka.
//!
//! Every mode reports how a read went, not just what it found. An asset whose
//! bytes are not on this machine prints `(unreadable: ...)` rather than an
//! empty line, and `--out` writes its own tally at the end.

use std::io::Write;
use std::path::{Path, PathBuf};

use ow_tank::{assoc, GameData, ManifestSelector};

#[derive(Debug)]
struct TypeReport {
    type_id: u16,
    records: usize,
    complete: usize,
    unreadable: Vec<u64>,
    stu_fallback: Vec<u64>,
    dropped_by_membership: usize,
    files_written: usize,
    write_failures: Vec<String>,
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };

    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        eprintln!("usage: ow-tank-assoc (--asset 0xGUID | --type 0xTYPE | --out DIR) [--types 0x..,..] [--limit N]");
        eprintln!("  --limit caps how many assets --type or --out reads. Without it a full");
        eprintln!("  --out is every asset of every type (over a million), which takes minutes.");
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

    if let Some(hex) = flag("--asset") {
        return run_single(&data, hex);
    }

    if let Some(out_dir) = flag("--out") {
        let limit: Option<usize> = flag("--limit").and_then(|v| v.parse().ok());
        return export(&data, &out_dir, flag("--types").as_deref(), limit);
    }

    // default: --type summary
    if let Some(thex) = flag("--type") {
        let t = match u16::from_str_radix(thex.trim_start_matches("0x"), 16) {
            Ok(v) => v,
            Err(_) => {
                eprintln!("--type wants a hex type");
                return std::process::ExitCode::from(2);
            }
        };
        let limit: usize = flag("--limit").and_then(|v| v.parse().ok()).unwrap_or(20);
        // Built once, then handed to every asset below. Rebuilding it per asset
        // cost a flat ~65 ms on each one whatever the file's size.
        let known = assoc::known_guids(&data);
        for g in data.guids_of_type(t).into_iter().take(limit) {
            let record = assoc::process_asset(&data, g, &known);
            print!("{g:#018x}:");
            for r in &record.references {
                print!(" {r:#018x}");
            }
            // Say why there is nothing, so an unreadable asset is not mistaken
            // for an asset that references nothing.
            match record.status.detail() {
                Some(detail) => println!(" ({}: {detail})", record.status.kind_str()),
                None if !record.status.is_complete() => println!(" ({})", record.status.kind_str()),
                None => println!(),
            }
        }
        return std::process::ExitCode::SUCCESS;
    }

    eprintln!("select --asset, --type, or --out");
    std::process::ExitCode::from(2)
}

fn run_single(data: &GameData, hex: String) -> std::process::ExitCode {
    let guid = match u64::from_str_radix(hex.trim_start_matches("0x"), 16) {
        Ok(g) => g,
        Err(_) => {
            eprintln!("--asset wants a hex guid");
            return std::process::ExitCode::from(2);
        }
    };
    let known = assoc::known_guids(data);
    let record = assoc::process_asset(data, guid, &known);
    println!(
        "asset {} (type 0x{:03x}): {} reference(s), read as {}",
        assoc::guid_string(guid),
        ow_tank::guid::type_of(guid),
        record.references.len(),
        record.status
    );
    for r in &record.references {
        println!("  {}", assoc::guid_string(*r));
    }
    if record.dropped_by_membership > 0 {
        println!(
            "  {} candidate value(s) were not GUIDs in this install and were dropped",
            record.dropped_by_membership
        );
    }
    // A read that did not happen is not a success.
    if record.status.is_complete() {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::from(1)
    }
}

/// Write a file, and say so if it could not be written. The old code threw the
/// result away, so a full disk produced a directory of nothing and no message.
fn write_file(path: &Path, text: &str) -> Result<(), String> {
    let mut f = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    f.write_all(text.as_bytes())
        .map_err(|e| format!("{}: {e}", path.display()))?;
    f.flush().map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(())
}

fn export(
    data: &GameData,
    out_dir: &str,
    types_arg: Option<&str>,
    limit: Option<usize>,
) -> std::process::ExitCode {
    let started = std::time::Instant::now();
    let types: Vec<u16> = if let Some(list) = types_arg {
        list.split(',')
            .filter_map(|x| u16::from_str_radix(x.trim().trim_start_matches("0x"), 16).ok())
            .collect()
    } else {
        assoc::all_type_ids(data)
    };

    // Creating the output directory is a precondition for everything below. If
    // it fails, every per-asset write would fail too, and each of those would
    // be counted separately; say it once and stop instead.
    let out_path = PathBuf::from(out_dir);
    if let Err(e) = std::fs::create_dir_all(&out_path) {
        eprintln!("could not create {}: {e}", out_path.display());
        return std::process::ExitCode::from(2);
    }
    let mut problems = 0usize;

    // Flatten every requested type into ONE work list, so the workers share the
    // load. The previous shape was one thread per type, which made the wall time
    // equal to the *largest* type rather than the total: type 0x008 alone is
    // 99,392 assets, and on a 12-core machine that left eleven cores idle for the
    // better part of an hour. A single queue fixes the skew, whatever the type
    // mix.
    let mut work: Vec<(u16, u64)> = Vec::new();
    for &t in types.iter() {
        if assoc::is_known_payload_type(t) {
            continue;
        }
        for g in data.guids_of_type(t) {
            work.push((t, g));
            if let Some(n) = limit {
                if work.len() >= n {
                    break;
                }
            }
        }
        if let Some(n) = limit {
            if work.len() >= n {
                break;
            }
        }
    }
    let total_work = work.len();
    if total_work == 0 {
        eprintln!("no assets to read for the requested type(s)");
        return std::process::ExitCode::from(1);
    }

    // Create every per-type directory up front. The workers write files by
    // path and none of them creates directories, so a missing directory here
    // turns every write for that type into a failure.
    for t in types.iter().copied() {
        let dir = out_path.join(format!("{t:03x}")).join("assets");
        if let Err(e) = std::fs::create_dir_all(&dir) {
            eprintln!("could not create {}: {e}", dir.display());
            problems += 1;
        }
    }

    // One membership set per worker rather than one per asset: it has over a
    // million entries and building it is a flat ~45 ms charge.
    //
    // `GameData` is deliberately not `Sync` -- the CASC store keeps its open
    // file handles in `RefCell`s -- so it cannot be shared between threads.
    // Each worker therefore opens its own view of the installation once and
    // keeps it. That is a fixed cost per worker (~3 s to open, plus the set),
    // paid once, and it is what buys the parallelism: the previous shape paid
    // it per *type* and then left every worker idle behind the largest one.
    let root = match ow_casc::install::detect_root() {
        Some(r) => r,
        None => {
            eprintln!("no Overwatch install found; set OW_INSTALL");
            return std::process::ExitCode::from(2);
        }
    };

    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(total_work)
        .max(1);

    let cursor = std::sync::atomic::AtomicUsize::new(0);
    let work = &work;
    let cursor = &cursor;
    let out_path = &out_path;

    // Each worker *returns* its tallies rather than writing into a shared
    // accumulator, so nothing is borrowed across the spawn boundary and the
    // workers never take a lock. Results are folded together afterwards.
    let mut results: Vec<(
        std::collections::HashMap<u16, TypeAcc>,
        Vec<String>,
        Option<String>,
    )> = Vec::new();

    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                let root = root.clone();
                scope.spawn(move || {
                    let sources = ow_tank::keytable::Sources::discover(&[root.clone()]);
                    let data = match GameData::open(
                        &root.display().to_string(),
                        &ManifestSelector::default(),
                        &sources,
                    ) {
                        Ok(d) => d,
                        Err(e) => {
                            return (
                                std::collections::HashMap::new(),
                                Vec::new(),
                                Some(e.to_string()),
                            )
                        }
                    };
                    let known = assoc::known_guids(&data);
                    let mut acc: std::collections::HashMap<u16, TypeAcc> =
                        std::collections::HashMap::new();
                    let mut write_failures: Vec<String> = Vec::new();
                    loop {
                        let i = cursor.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if i >= work.len() {
                            break;
                        }
                        let (t, g) = work[i];
                        read_one(&data, t, g, &known, out_path, &mut acc, &mut write_failures);
                    }
                    (acc, write_failures, None)
                })
            })
            .collect();
        for handle in handles {
            match handle.join() {
                Ok(r) => results.push(r),
                Err(panic) => eprintln!("a worker stopped early: {}", panic_message(&panic)),
            }
        }
    });

    // Fold the workers' per-type tallies into one report each.
    let mut by_type: std::collections::BTreeMap<u16, TypeReport> =
        std::collections::BTreeMap::new();
    let mut write_failures: Vec<String> = Vec::new();
    let mut problems = 0usize;
    for (a, wf, open_err) in results {
        if let Some(e) = open_err {
            problems += 1;
            eprintln!("a worker could not open the installation: {e}");
        }
        write_failures.extend(wf);
        for (t, ta) in a {
            let e = by_type.entry(t).or_insert_with(|| TypeReport {
                type_id: t,
                records: 0,
                complete: 0,
                unreadable: Vec::new(),
                stu_fallback: Vec::new(),
                dropped_by_membership: 0,
                files_written: 0,
                write_failures: Vec::new(),
            });
            e.records += ta.records;
            e.complete += ta.complete;
            e.dropped_by_membership += ta.dropped;
            e.files_written += ta.written;
            e.unreadable.extend(ta.unreadable);
            e.stu_fallback.extend(ta.stu_fallback);
        }
    }

    let mut all_unreadable: Vec<u64> = Vec::new();
    let mut all_fallback: Vec<u64> = Vec::new();
    let mut total_records = 0usize;
    let mut total_complete = 0usize;
    let mut total_dropped = 0usize;
    let mut total_written = 0usize;
    let mut empty_types = 0usize;

    for report in by_type.values() {
        total_records += report.records;
        total_complete += report.complete;
        total_dropped += report.dropped_by_membership;
        total_written += report.files_written;
        all_unreadable.extend(report.unreadable.iter().copied());
        all_fallback.extend(report.stu_fallback.iter().copied());
        if report.records == 0 {
            empty_types += 1;
        }
        if report.unreadable.is_empty()
            && report.stu_fallback.is_empty()
            && report.write_failures.is_empty()
        {
            continue;
        }
        eprintln!(
            "type 0x{:03x}: {} record(s), {} unreadable, {} structured-data parse \
             failure(s) byte-scanned instead, {} candidate value(s) dropped, \
             {} file(s) written, {} write failure(s)",
            report.type_id,
            report.records,
            report.unreadable.len(),
            report.stu_fallback.len(),
            report.dropped_by_membership,
            report.files_written,
            report.write_failures.len(),
        );
    }

    all_unreadable.sort_unstable();
    all_unreadable.dedup();
    all_fallback.sort_unstable();
    all_fallback.dedup();

    // missing.json now carries every asset whose bytes could not be read. Those
    // used to be dropped on the floor or reported as empty.
    for (name, list) in [
        ("missing.json", &all_unreadable),
        ("stu_fallback.json", &all_fallback),
    ] {
        let path = out_path.join(name);
        match write_file(&path, &assoc::write_named_missing_json(list)) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("could not write {}: {e}", path.display());
                problems += 1;
            }
        }
    }
    for e in write_failures.iter().take(20) {
        eprintln!("  write failed: {e}");
    }
    if write_failures.len() > 20 {
        eprintln!(
            "  ... and {} more write failures",
            write_failures.len() - 20
        );
    }

    println!(
        "summary: {total_records} record(s) over {} type(s), of which {total_complete} were read \
         as the asset's own reference list; {total_written} file(s) written; \
         {} asset(s) unreadable (missing.json); {} asset(s) structured-data parse \
         failure(s) byte-scanned instead (stu_fallback.json); {total_dropped} candidate value(s) \
         dropped for not being a GUID in this install; {empty_types} type(s) produced nothing; \
         {} write failure(s).",
        by_type.len(),
        all_unreadable.len(),
        all_fallback.len(),
        write_failures.len(),
    );
    // Always say how long it took, and what it cost per asset. A run that is
    // slower than this should be a surprise to the reader, not a mystery.
    println!(
        "read {total_work} asset(s) on {workers} thread(s) in {:.1} s ({:.1} ms/asset)",
        started.elapsed().as_secs_f64(),
        started.elapsed().as_secs_f64() * 1000.0 / total_work as f64,
    );

    if empty_types > 0 || problems > 0 {
        std::process::ExitCode::from(1)
    } else {
        std::process::ExitCode::SUCCESS
    }
}

/// One asset's tallies, kept per type so the end-of-run summary can still be
/// reported per type even though the work is now shared across all of them.
#[derive(Default)]
struct TypeAcc {
    records: usize,
    complete: usize,
    dropped: usize,
    written: usize,
    unreadable: Vec<u64>,
    stu_fallback: Vec<u64>,
}

/// Read one asset and write its record. Every outcome is tallied, so a type that
/// produced nothing is distinguishable from a type that was never visited.
fn read_one(
    data: &GameData,
    type_id: u16,
    guid: u64,
    known: &std::collections::HashSet<u64>,
    out_path: &Path,
    acc: &mut std::collections::HashMap<u16, TypeAcc>,
    write_failures: &mut Vec<String>,
) {
    let record = assoc::process_asset(data, guid, known);

    let type_dir = out_path.join(format!("{type_id:03x}")).join("assets");
    let ta = acc.entry(type_id).or_default();
    ta.records += 1;
    if record.status.is_complete() {
        ta.complete += 1;
    }
    ta.dropped += record.dropped_by_membership;
    match record.status {
        assoc::AssetStatus::Unreadable(_) => ta.unreadable.push(guid),
        assoc::AssetStatus::StuParseFailed(_) => ta.stu_fallback.push(guid),
        _ => {}
    }

    let path = type_dir.join(format!("{}.json", assoc::guid_string(record.guid)));
    match write_file(&path, &assoc::asset_record_json(&record)) {
        Ok(()) => ta.written += 1,
        Err(e) => write_failures.push(e),
    }
}

/// A panic payload is an opaque value; pull out its message if it is one of
/// the two shapes `panic!` produces, and say so plainly if it is not.
fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "thread panicked with a non-string payload".to_string()
    }
}

//! A read-only byte census of the install's CASC store.
//!
//! The numbers answer three questions the sample test in
//! `crates/ow-casc/tests/blte_real.rs` only answers for chunks: how many bytes
//! each BLTE block mode accounts for, how much of the install is unreadable
//! without a keyring key, and how many records that touches.
//!
//! Only the index entries, each record's frame header and each chunk's first
//! bytes are read. No payload is decoded, nothing is written, and no temporary
//! files are created.
//!
//! ```text
//! cargo run --release -p ow-casc --bin census
//! cargo run --release -p ow-casc --bin census -- --stride 32
//! ```
//!
//! `OW_INSTALL` overrides the detected install root. A stride keeps one in
//! every N unique salt-0 keys (the same selection the install test makes) and
//! labels the report as a sample.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use ow_casc::blte::{self, FRAME_TYPE_EXTENDED, FRAME_TYPE_NORMAL};
use ow_casc::index::{
    archive_name, bucket_of, parse_bucket_file, parse_segment_file_name, Location, SegmentEntries,
    ARCHIVE_MASK, ARCHIVE_SHIFT, DATA_HEADER_SIZE,
};
use ow_casc::{Install, Store};

/// Bytes read at the start of every record: the data header, the BLTE magic
/// and the whole frame header of a small record.
const PREFIX_LEN: usize = 96;
/// A chunk's head covers the mode byte and an `E` header (1 + 1 + 8 + 1 + IV).
const CHUNK_HEAD: usize = 16;
/// Records between progress lines on stderr.
const PROGRESS_EVERY: u64 = 100_000;

/// The report's mode order: stored, zlib, encrypted, LZ4, and anything else.
const MODE_NAMES: [&str; 5] = ["N", "Z", "E", "4", "other"];

fn mode_index(mode: u8) -> usize {
    match mode {
        b'N' => 0,
        b'Z' => 1,
        b'E' => 2,
        b'4' => 3,
        _ => 4,
    }
}

#[derive(Clone, Copy, Default)]
struct ModeStats {
    chunks: u64,
    stored: u64,
    decoded: u64,
}

impl ModeStats {
    fn add(&mut self, stored: u32, decoded: u32) {
        self.chunks += 1;
        self.stored += u64::from(stored);
        self.decoded += u64::from(decoded);
    }
}

#[derive(Default)]
struct Census {
    /// Distinct salt-0 keys the index lists, before the stride is applied.
    unique_keys: u64,
    records: u64,
    header_only: u64,
    short_records: u64,
    not_blte: u64,
    direct: u64,
    read_failures: u64,
    parse_failures: u64,
    frame_normal: u64,
    frame_extended: u64,
    stored_total: u64,
    modes: [ModeStats; 5],
    e_chunks: u64,
    e_key_present: u64,
    e_key_misses: u64,
    missing_key_ids: BTreeSet<u64>,
    records_any_e: u64,
    records_undecodable: u64,
}

/// Open `data/data.NNN` files, cached like the client's archive handles.
struct ArchiveReader {
    data_dir: PathBuf,
    files: HashMap<u32, File>,
}

impl ArchiveReader {
    fn new(data_dir: PathBuf) -> ArchiveReader {
        ArchiveReader {
            data_dir,
            files: HashMap::new(),
        }
    }

    /// Read `buf.len()` bytes starting `offset` bytes into the record, following
    /// the client's 2^30 archive split.
    fn read_at(&mut self, location: Location, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        let mut at = location.packed() + offset;
        let mut filled = 0usize;
        while filled < buf.len() {
            let archive = (at >> ARCHIVE_SHIFT) as u32;
            let archive_offset = at & ARCHIVE_MASK;
            if !self.files.contains_key(&archive) {
                let file = File::open(self.data_dir.join(archive_name(archive)))?;
                self.files.insert(archive, file);
            }
            let file = self.files.get_mut(&archive).expect("archive just opened");
            let take = (buf.len() - filled).min((ARCHIVE_MASK + 1 - archive_offset) as usize);
            file.seek(SeekFrom::Start(archive_offset))?;
            file.read_exact(&mut buf[filled..filled + take])?;
            filled += take;
            at += take as u64;
        }
        Ok(())
    }
}

/// What one chunk contributed: whether it is encrypted, and whether the
/// keyring has its key.
#[derive(Clone, Copy, Default)]
struct ChunkFlags {
    encrypted: bool,
    missing_key: bool,
}

/// Classify one chunk from its first bytes and add it to the totals.
///
/// `decoded` is the frame header's declared size; the direct container has no
/// frame table, so its single chunk passes 0 there.
fn classify_chunk(
    census: &mut Census,
    reader: &mut ArchiveReader,
    keyring: &BTreeMap<u64, [u8; 16]>,
    location: Location,
    prefix: &[u8],
    at: u64,
    stored: u32,
    decoded: u32,
) -> io::Result<ChunkFlags> {
    let mut flags = ChunkFlags::default();
    let head_len = (stored as usize).min(CHUNK_HEAD);
    if head_len == 0 {
        // A zero-stored chunk has no mode byte to read.
        census.parse_failures += 1;
        return Ok(flags);
    }
    let mut head = [0u8; CHUNK_HEAD];
    let from_prefix = at as usize;
    if from_prefix + head_len <= prefix.len() {
        head[..head_len].copy_from_slice(&prefix[from_prefix..from_prefix + head_len]);
    } else {
        reader.read_at(location, at, &mut head[..head_len])?;
    }

    census.modes[mode_index(head[0])].add(stored, decoded);
    if head[0] != b'E' {
        return Ok(flags);
    }
    flags.encrypted = true;
    census.e_chunks += 1;
    // `E` header: mode, key-name length (8), key id (LE u64), IV length, IV.
    if head_len < 10 || head[1] != 8 {
        census.parse_failures += 1;
        flags.missing_key = true;
        return Ok(flags);
    }
    let mut id = [0u8; 8];
    id.copy_from_slice(&head[2..10]);
    let key_id = u64::from_le_bytes(id);
    if keyring.contains_key(&key_id) {
        census.e_key_present += 1;
    } else {
        census.e_key_misses += 1;
        census.missing_key_ids.insert(key_id);
        flags.missing_key = true;
    }
    Ok(flags)
}

/// One record: the data header, the frame header, then each chunk's head.
fn scan_record(
    census: &mut Census,
    reader: &mut ArchiveReader,
    keyring: &BTreeMap<u64, [u8; 16]>,
    location: Location,
) -> io::Result<()> {
    let size = u64::from(location.size);
    census.stored_total += size;

    let prefix_len = (size as usize).min(PREFIX_LEN);
    let mut prefix = [0u8; PREFIX_LEN];
    if prefix_len < 8 {
        census.short_records += 1;
        return Ok(());
    }
    reader.read_at(location, 0, &mut prefix[..prefix_len])?;
    let prefix = &prefix[..prefix_len];

    // The same test the install makes: a record may start with the BLTE magic,
    // or carry the 30-byte data header first.
    let blte_at = if prefix[0..4] == blte::BLTE_MAGIC {
        0u64
    } else if size == u64::from(DATA_HEADER_SIZE) {
        // Header-only empty file: the client returns a zero-length range.
        census.header_only += 1;
        return Ok(());
    } else if prefix.len() >= 34 {
        if prefix[30..34] != blte::BLTE_MAGIC {
            census.not_blte += 1;
            return Ok(());
        }
        u64::from(DATA_HEADER_SIZE)
    } else {
        let mut magic = [0u8; 4];
        reader.read_at(location, u64::from(DATA_HEADER_SIZE), &mut magic)?;
        if magic != blte::BLTE_MAGIC {
            census.not_blte += 1;
            return Ok(());
        }
        u64::from(DATA_HEADER_SIZE)
    };

    let mut size_bytes = [0u8; 8];
    let header_at = blte_at as usize;
    if header_at + 8 <= prefix.len() {
        size_bytes.copy_from_slice(&prefix[header_at..header_at + 8]);
    } else {
        reader.read_at(location, blte_at, &mut size_bytes)?;
    }
    let header_size = u64::from(u32::from_be_bytes([
        size_bytes[4],
        size_bytes[5],
        size_bytes[6],
        size_bytes[7],
    ]));

    if header_size == 0 {
        // The direct form: one chunk straight after the 8-byte container
        // prefix, with no frame table to declare a decoded size.
        census.direct += 1;
        let chunk_at = blte_at + 8;
        if chunk_at >= size {
            census.parse_failures += 1;
            return Ok(());
        }
        let flags = classify_chunk(
            census,
            reader,
            keyring,
            location,
            prefix,
            chunk_at,
            (size - chunk_at) as u32,
            0,
        )?;
        if flags.encrypted {
            census.records_any_e += 1;
        }
        if flags.missing_key {
            census.records_undecodable += 1;
        }
        return Ok(());
    }

    if blte_at + header_size > size {
        census.parse_failures += 1;
        return Ok(());
    }
    let header;
    let header_bytes: &[u8] = if header_at + header_size as usize <= prefix.len() {
        &prefix[header_at..header_at + header_size as usize]
    } else {
        header = {
            let mut buf = vec![0u8; header_size as usize];
            reader.read_at(location, blte_at, &mut buf)?;
            buf
        };
        &header
    };

    let frame = match blte::parse_frame_header(header_bytes, None) {
        Ok(frame) => frame,
        Err(_) => {
            census.parse_failures += 1;
            return Ok(());
        }
    };
    match frame.frame_type {
        FRAME_TYPE_NORMAL => census.frame_normal += 1,
        FRAME_TYPE_EXTENDED => census.frame_extended += 1,
        _ => {}
    }

    let mut chunk_at = blte_at + frame.header_size as u64;
    let mut any_e = false;
    let mut missing_key = false;
    for chunk in &frame.chunks {
        if chunk_at + u64::from(chunk.stored) > size {
            census.parse_failures += 1;
            break;
        }
        let flags = classify_chunk(
            census,
            reader,
            keyring,
            location,
            prefix,
            chunk_at,
            chunk.stored,
            chunk.decoded,
        )?;
        any_e |= flags.encrypted;
        missing_key |= flags.missing_key;
        chunk_at += u64::from(chunk.stored);
    }
    if any_e {
        census.records_any_e += 1;
    }
    if missing_key {
        census.records_undecodable += 1;
    }
    Ok(())
}

/// Every non-temporary `<bucket:02x><segment:08x>.idx` in `dir`.
fn idx_files(dir: &Path) -> io::Result<Vec<(u8, u32, PathBuf)>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        let Some(parsed) = parse_segment_file_name(name) else {
            continue;
        };
        if parsed.temporary {
            continue;
        }
        out.push((parsed.bucket, parsed.segment, entry.path()));
    }
    out.sort_by_key(|(bucket, segment, _)| (*bucket, *segment));
    Ok(out)
}

fn selected_path(files: &[(u8, u32, PathBuf)], bucket: u8, segment: u32) -> Option<PathBuf> {
    files
        .iter()
        .find(|(file_bucket, file_segment, _)| *file_bucket == bucket && *file_segment == segment)
        .map(|(_, _, path)| path.clone())
}

fn usage() -> String {
    "usage: census [--stride N]\n\
     \n\
     A read-only byte census of the install's CASC store. Every record's frame\n\
     header and chunk mode bytes are read; no payload is decoded and the install\n\
     is never written.\n\
     \n\
     --stride N   keep one in every N unique salt-0 keys (default 1: all of them)\n\
     OW_INSTALL   overrides the detected install root\n"
        .to_string()
}

fn parse_stride() -> Result<Option<u64>, String> {
    let mut stride = 1u64;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--stride" => {
                let value = args.next().ok_or("--stride needs a number")?;
                stride = value
                    .parse()
                    .map_err(|_| format!("--stride {value}: not a number"))?;
            }
            "--help" | "-h" => return Ok(None),
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    if stride == 0 {
        return Err("--stride must be at least 1".to_string());
    }
    Ok(Some(stride))
}

fn print_report(root: &Path, stride: u64, census: &Census, elapsed: Duration) {
    let sampled = stride > 1;
    let kind = if sampled {
        format!(
            "sampled scan (stride {stride}: one in every {stride} unique salt-0 keys; these numbers are a sample)"
        )
    } else {
        "full scan (stride 1: every unique salt-0 key)".to_string()
    };
    println!("ow-casc byte census");
    println!("install: {}", root.display());
    println!("mode: {kind}");
    println!();
    println!("unique salt-0 keys: {}", census.unique_keys);
    println!("records scanned: {}", census.records);
    println!("  header-only 30-byte records: {}", census.header_only);
    println!("  records shorter than 8 bytes: {}", census.short_records);
    println!("  direct container (no frame header): {}", census.direct);
    println!("  not BLTE: {}", census.not_blte);
    println!("  read failures: {}", census.read_failures);
    println!("  parse failures: {}", census.parse_failures);
    println!(
        "stored bytes of scanned records (sum of index entry sizes): {}",
        census.stored_total
    );
    println!();
    println!("frame types:");
    println!("  0x0F normal: {}", census.frame_normal);
    println!("  0x10 extended: {}", census.frame_extended);
    println!("  absent (direct): {}", census.direct);
    println!();
    println!("chunk modes:");
    println!(
        "  {:<6}{:>12}{:>20}{:>20}",
        "mode", "chunks", "stored bytes", "decoded bytes"
    );
    let mut total = ModeStats::default();
    for (index, stats) in census.modes.iter().enumerate() {
        total.chunks += stats.chunks;
        total.stored += stats.stored;
        total.decoded += stats.decoded;
        println!(
            "  {:<6}{:>12}{:>20}{:>20}",
            MODE_NAMES[index], stats.chunks, stats.stored, stats.decoded
        );
    }
    println!(
        "  {:<6}{:>12}{:>20}{:>20}",
        "total", total.chunks, total.stored, total.decoded
    );
    println!();
    println!("E chunks:");
    println!("  chunks: {}", census.e_chunks);
    println!("  keyring hits: {}", census.e_key_present);
    println!("  keyring misses: {}", census.e_key_misses);
    println!(
        "  distinct missing key ids: {}",
        census.missing_key_ids.len()
    );
    println!("  records with any E chunk: {}", census.records_any_e);
    println!(
        "  records undecodable because a key is missing: {}",
        census.records_undecodable
    );
    println!();
    println!("elapsed: {:.1} s", elapsed.as_secs_f64());
}

fn run(stride: u64) -> Result<(), String> {
    let Some(root) = ow_casc::install::detect_root() else {
        return Err("no Overwatch install found; set OW_INSTALL to the install root".to_string());
    };
    let data_dir = root.join("data").join("casc").join("data");
    let files =
        idx_files(&data_dir).map_err(|error| format!("scan {}: {error}", data_dir.display()))?;
    let install = Install::open(&root).map_err(|error| format!("open install: {error}"))?;
    let keyring = install.keyring.clone();
    let store = Store::open(install).map_err(|error| format!("open store: {error}"))?;
    let data_dir = store.install().data_dir();
    let selected = store.segment_numbers();

    let started = Instant::now();
    let mut reader = ArchiveReader::new(data_dir);
    let mut census = Census::default();

    for bucket in 0u8..16 {
        let Some(segment) = selected[usize::from(bucket)] else {
            continue;
        };
        let Some(path) = selected_path(&files, bucket, segment) else {
            return Err(format!(
                "bucket {bucket:02x} selected segment {segment:08x} has no file"
            ));
        };
        let file = parse_bucket_file(&path, Some(bucket))
            .map_err(|error| format!("parse {}: {error}", path.display()))?;
        let entries = SegmentEntries::with_pages(file.entries, file.pages);

        let mut at = 0usize;
        while at < entries.len() {
            let key = entries.as_slice()[at].0;
            let mut end = at + 1;
            while end < entries.len() && entries.as_slice()[end].0 == key {
                end += 1;
            }
            at = end;
            if bucket_of(&key, 0) != bucket {
                // Salt-1 archive placeholder: the read path does not resolve it.
                continue;
            }
            census.unique_keys += 1;
            if census.unique_keys % stride != 0 {
                continue;
            }
            let Some(location) = store.location_of(&key) else {
                return Err(format!("salt-0 key {key:02x?} does not resolve"));
            };
            census.records += 1;
            match scan_record(&mut census, &mut reader, &keyring, location) {
                Ok(()) => {}
                Err(_) => census.read_failures += 1,
            }
            if census.records % PROGRESS_EVERY == 0 {
                eprintln!(
                    "census: {} records, {:.0} s",
                    census.records,
                    started.elapsed().as_secs_f64()
                );
            }
        }
    }

    let elapsed = started.elapsed();
    print_report(&root, stride, &census, elapsed);
    Ok(())
}

fn main() -> ExitCode {
    let stride = match parse_stride() {
        Ok(Some(stride)) => stride,
        Ok(None) => {
            print!("{}", usage());
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            eprintln!("census: {message}");
            eprint!("{}", usage());
            return ExitCode::from(2);
        }
    };
    match run(stride) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("census: {message}");
            ExitCode::FAILURE
        }
    }
}

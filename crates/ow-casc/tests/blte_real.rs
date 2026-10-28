//! A read-only census of the BLTE containers in the install.
//!
//! The install holds 1.1 million records in 86 GB of archives, so the census
//! takes a deterministic sample: every 32nd unique salt-0 entry. Each sampled
//! entry is located through the same index the client reads, then only the
//! record's frame header and each chunk's first bytes are fetched. That covers
//! frame types `0x0F`/`0x10`, the direct (zero header-size) form, outer chunk
//! modes `N`/`Z`/`E`/`4`, and the inner mode of an `E` chunk whose key is in
//! the install's keyring. A smaller stride sample is decoded in full with
//! `blte::decode`.
//!
//! Skipped when no install is present. Read-only.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use ow_casc::blte::{self, FrameHeader, FRAME_TYPE_EXTENDED, FRAME_TYPE_NORMAL};
use ow_casc::index::{
    archive_name, bucket_of, parse_bucket_file, parse_segment_file_name, Location, SegmentEntries,
    ARCHIVE_MASK, ARCHIVE_SHIFT, DATA_HEADER_SIZE,
};
use ow_casc::{Install, Store};

fn idx_files(dir: &Path) -> Vec<(u8, u32, PathBuf)> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).expect("read data/") {
        let entry = entry.expect("data/ entry");
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
    out
}

fn selected_path(files: &[(u8, u32, PathBuf)], bucket: u8, segment: u32) -> Option<PathBuf> {
    files
        .iter()
        .find(|(file_bucket, file_segment, _)| *file_bucket == bucket && *file_segment == segment)
        .map(|(_, _, path)| path.clone())
}

/// Read `buf.len()` bytes starting `offset` bytes into the record, following
/// the client's 2^30 archive split.
fn read_record_at(
    files: &mut HashMap<u32, File>,
    data_dir: &Path,
    location: Location,
    offset: u64,
    buf: &mut [u8],
) {
    let mut at = location.packed() + offset;
    let mut filled = 0usize;
    while filled < buf.len() {
        let archive = (at >> ARCHIVE_SHIFT) as u32;
        let archive_offset = at & ARCHIVE_MASK;
        let file = files.entry(archive).or_insert_with(|| {
            File::open(data_dir.join(archive_name(archive))).expect("open data archive")
        });
        let take = (buf.len() - filled).min((ARCHIVE_MASK + 1 - archive_offset) as usize);
        file.seek(SeekFrom::Start(archive_offset))
            .expect("seek data archive");
        file.read_exact(&mut buf[filled..filled + take])
            .expect("read data archive");
        filled += take;
        at += take as u64;
    }
}

/// One in this many unique salt-0 keys is inspected.
const SAMPLE_STRIDE: usize = 32;
/// One in this many inspected keys is decoded in full.
const DECODE_STRIDE: usize = 1024;

#[derive(Default)]
struct Census {
    entries: usize,
    empty_records: usize,
    non_blte: usize,
    direct: usize,
    normal: usize,
    extended: usize,
    chunks: usize,
    outer_n: usize,
    outer_z: usize,
    outer_e: usize,
    outer_lz4: usize,
    outer_other: usize,
    inner_n: usize,
    inner_z: usize,
    inner_e: usize,
    inner_lz4: usize,
    inner_other: usize,
    lz4_total: usize,
    encrypted_missing_key: usize,
    missing_keys: std::collections::BTreeSet<u64>,
    decoded_sample: usize,
    decoded_missing_key: usize,
    decoded_bytes: u64,
}

fn outer_mode(
    census: &mut Census,
    files: &mut HashMap<u32, File>,
    data_dir: &Path,
    location: Location,
    chunk_at: u64,
    stored: u64,
    keyring: &std::collections::BTreeMap<u64, [u8; 16]>,
    chunk_index: usize,
) {
    let mut mode = [0u8; 1];
    read_record_at(files, data_dir, location, chunk_at, &mut mode);
    census.chunks += 1;
    match mode[0] {
        b'N' => census.outer_n += 1,
        b'Z' => census.outer_z += 1,
        b'E' => {
            census.outer_e += 1;
            // The E header is at most 1+1+8+1+8+1 bytes; 32 bytes also covers
            // the first 16 bytes of ciphertext.
            let available = stored.min(32) as usize;
            let mut header = [0u8; 32];
            read_record_at(
                files,
                data_dir,
                location,
                chunk_at,
                &mut header[..available],
            );
            let key_name_len = header[1] as usize;
            assert_eq!(key_name_len, 8, "E chunk key name length");
            let mut id = [0u8; 8];
            id.copy_from_slice(&header[2..10]);
            let key_id = u64::from_le_bytes(id);
            let iv_len = header[10] as usize;
            assert!(iv_len <= 8, "E chunk iv length");
            let enc_type = header[11 + iv_len];
            assert_eq!(enc_type, b'S', "E chunk encryption type");
            let cipher_at = 1 + 1 + key_name_len + 1 + iv_len + 1;
            assert!(cipher_at < available, "E chunk has no ciphertext byte");
            let mut iv = [0u8; 8];
            iv[..iv_len].copy_from_slice(&header[11..11 + iv_len]);
            let ordinal = chunk_index as u32;
            let low = u32::from_le_bytes([iv[0], iv[1], iv[2], iv[3]]) ^ ordinal;
            iv[0..4].copy_from_slice(&low.to_le_bytes());
            let Some(key) = keyring.get(&key_id) else {
                census.missing_keys.insert(key_id);
                census.encrypted_missing_key += 1;
                return;
            };
            let cipher_end = (cipher_at + 16).min(available);
            let plain = blte::salsa20(key, &iv, &header[cipher_at..cipher_end]);
            match plain[0] {
                b'N' => census.inner_n += 1,
                b'Z' => census.inner_z += 1,
                b'E' => census.inner_e += 1,
                b'4' => census.inner_lz4 += 1,
                _ => census.inner_other += 1,
            }
        }
        b'4' => {
            census.outer_lz4 += 1;
        }
        _ => census.outer_other += 1,
    }
}

#[test]
fn sampled_install_blte_containers_match_the_parsed_census() {
    let Some(root) = ow_casc::install::detect_root() else {
        eprintln!("note: no Overwatch install here; nothing to assert");
        return;
    };
    let dir = root.join("data").join("casc").join("data");
    let files = idx_files(&dir);
    let install = Install::open(&root).expect("open install");
    let keyring = install.keyring.clone();
    let store = Store::open(install).expect("open store");
    let data_dir = store.install().data_dir();
    let selected = store.segment_numbers();

    let started = std::time::Instant::now();
    let mut archives: HashMap<u32, File> = HashMap::new();
    let mut census = Census::default();
    let mut decode_keys: Vec<[u8; 9]> = Vec::new();
    let mut unique = 0usize;

    for bucket in 0u8..16 {
        let segment = selected[usize::from(bucket)].expect("selected segment");
        let path = selected_path(&files, bucket, segment).expect("selected path");
        let file = parse_bucket_file(&path, Some(bucket)).expect("valid selected segment");
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
            unique += 1;
            if unique % SAMPLE_STRIDE != 0 {
                continue;
            }
            census.entries += 1;

            let location = store.location_of(&key).expect("salt-0 key resolves");
            let size = u64::from(location.size);

            let mut magic = [0u8; 4];
            read_record_at(&mut archives, &data_dir, location, 0, &mut magic);
            let blte_at = if magic == blte::BLTE_MAGIC {
                0
            } else if size == u64::from(DATA_HEADER_SIZE) {
                census.empty_records += 1;
                continue;
            } else {
                read_record_at(
                    &mut archives,
                    &data_dir,
                    location,
                    u64::from(DATA_HEADER_SIZE),
                    &mut magic,
                );
                if magic != blte::BLTE_MAGIC {
                    census.non_blte += 1;
                    continue;
                }
                u64::from(DATA_HEADER_SIZE)
            };

            let mut prefix = [0u8; 12];
            read_record_at(&mut archives, &data_dir, location, blte_at, &mut prefix);
            let header_size = u32::from_be_bytes([prefix[4], prefix[5], prefix[6], prefix[7]]);
            if header_size == 0 {
                census.direct += 1;
                outer_mode(
                    &mut census,
                    &mut archives,
                    &data_dir,
                    location,
                    blte_at + 8,
                    size - blte_at - 8,
                    &keyring,
                    0,
                );
                if census.entries % DECODE_STRIDE == 0 {
                    decode_keys.push(key);
                }
                continue;
            }

            assert!(
                blte_at + u64::from(header_size) <= size,
                "frame header fits the record"
            );
            let mut header = vec![0u8; header_size as usize];
            read_record_at(&mut archives, &data_dir, location, blte_at, &mut header);
            let frame: FrameHeader =
                blte::parse_frame_header(&header, None).expect("install frame header parses");
            match frame.frame_type {
                FRAME_TYPE_NORMAL => census.normal += 1,
                FRAME_TYPE_EXTENDED => census.extended += 1,
                other => panic!("unexpected frame type {other:#04x}"),
            }

            let mut chunk_at = blte_at + u64::from(header_size);
            for (index, chunk) in frame.chunks.iter().enumerate() {
                assert!(
                    chunk_at + u64::from(chunk.stored) <= size,
                    "chunk {index} runs past the record"
                );
                outer_mode(
                    &mut census,
                    &mut archives,
                    &data_dir,
                    location,
                    chunk_at,
                    u64::from(chunk.stored),
                    &keyring,
                    index,
                );
                chunk_at += u64::from(chunk.stored);
            }
            assert_eq!(
                chunk_at, size,
                "chunks cover the record after the frame header"
            );
            if census.entries % DECODE_STRIDE == 0 {
                decode_keys.push(key);
            }
        }
    }
    let scan_elapsed = started.elapsed();

    for key in &decode_keys {
        let stored = store.read_stored(key).expect("read sample record");
        match blte::decode(&stored, &keyring) {
            Ok(decoded) => {
                census.decoded_sample += 1;
                census.decoded_bytes += decoded.len() as u64;
            }
            // The build's keyring omits a few keys; `BlteBlockRead` reports
            // the same condition as status 13.
            Err(blte::Error::MissingKey(_)) => census.decoded_missing_key += 1,
            Err(error) => panic!("decode sample record: {error}"),
        }
    }

    census.lz4_total = census.outer_lz4 + census.inner_lz4;
    eprintln!("blte census scan took {scan_elapsed:?}");
    eprintln!(
        "blte census: {} entries ({} empty, {} non-BLTE); frame headers: {} direct, \
         {} type 0x0f, {} type 0x10, {} chunks; outer modes N/Z/E/4/other = \
         {}/{}/{}/{}/{}; E inner N/Z/E/4/other = {}/{}/{}/{}/{}; LZ4 total {}; \
         {} E chunks with no key ({} distinct); decoded sample {} records / {} bytes ({} no key)",
        census.entries,
        census.empty_records,
        census.non_blte,
        census.direct,
        census.normal,
        census.extended,
        census.chunks,
        census.outer_n,
        census.outer_z,
        census.outer_e,
        census.outer_lz4,
        census.outer_other,
        census.inner_n,
        census.inner_z,
        census.inner_e,
        census.inner_lz4,
        census.inner_other,
        census.lz4_total,
        census.encrypted_missing_key,
        census.missing_keys.len(),
        census.decoded_sample,
        census.decoded_bytes,
        census.decoded_missing_key,
    );
    assert_eq!(
        census.entries,
        census.empty_records + census.non_blte + census.direct + census.normal + census.extended
    );
    assert_eq!(
        census.chunks,
        census.outer_n + census.outer_z + census.outer_e + census.outer_lz4 + census.outer_other
    );
    assert_eq!(
        census.outer_e,
        census.inner_n
            + census.inner_z
            + census.inner_e
            + census.inner_lz4
            + census.inner_other
            + census.encrypted_missing_key
    );

    // The full scan (one-off, stride 1) found 1,146,326 unique salt-0 keys:
    // 1,146,325 type-0x0F containers and one known non-BLTE orphan payload.
    // No direct container, no type 0x10 and no outer LZ4 block occurs in this
    // install; the 43,061 E chunks' keys are not in its 63-key keyring, so
    // their inner modes (including a possible LZ4) are unreadable here.
    assert_eq!(unique, 1_146_326);
    assert_eq!(census.entries, 35_822);
    assert_eq!(census.empty_records, 0);
    assert_eq!(census.non_blte, 0);
    assert_eq!(census.direct, 0);
    assert_eq!(census.normal, 35_822);
    assert_eq!(census.extended, 0);
    assert_eq!(census.chunks, 54_490);
    assert_eq!(census.outer_n, 9_154);
    assert_eq!(census.outer_z, 44_054);
    assert_eq!(census.outer_e, 1_282);
    assert_eq!(census.outer_lz4, 0);
    assert_eq!(census.outer_other, 0);
    assert_eq!(census.inner_n, 0);
    assert_eq!(census.inner_z, 0);
    assert_eq!(census.inner_e, 0);
    assert_eq!(census.inner_lz4, 0);
    assert_eq!(census.inner_other, 0);
    assert_eq!(census.encrypted_missing_key, 1_282);
    assert_eq!(census.lz4_total, 0);
    assert_eq!(census.decoded_sample, 34);
    assert_eq!(census.decoded_bytes, 15_144_112);
    assert_eq!(census.decoded_missing_key, 0);
}

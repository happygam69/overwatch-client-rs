//! The key→bucket fold against the installed CASC store.
//!
//! proof-covers: 0x142501b40
//!
//! `DynamicKeyStateLookup` (`0x142501b40`) computes
//! `((xor9 >> 4) ^ xor9) + salt) & 0xf` and selects one of sixteen bucket
//! files. This test reads every entry of every `.idx` file in the operator's
//! install and compares the computed bucket with the file's header bucket.
//!
//! Two salt domains meet in the file. The 1,147,132 real keys use salt 0 and
//! all match. The 1,488 archive-header placeholders (93 archives × 16 slots,
//! a 30-byte record at `30 * bucket`) use salt 1 and all match that. No entry
//! matches neither. The placement is asserted structurally, so the exception
//! is named rather than excluded.
//!
//! Skipped when no install is present. Read-only.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use ow_casc::index::{bucket_of, parse_bucket_file, parse_segment_file_name};
use ow_casc::{Install, Store};

/// Build 2.25.0.1.154504: every real key matches salt 0; the replicated
/// 30-byte archive-header records match salt 1.
const REAL_KEYS: usize = 1_147_132;
const HEADER_RECORDS: usize = 1_488;

fn idx_files(dir: &Path) -> Vec<(u8, u32, std::path::PathBuf)> {
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

#[test]
fn every_real_key_hashes_to_its_idx_file_bucket() {
    let Some(root) = ow_casc::install::detect_root() else {
        eprintln!("note: no Overwatch install here; nothing to assert");
        return;
    };
    let dir = root.join("data").join("casc").join("data");
    let files = idx_files(&dir);
    assert_eq!(files.len(), 16, "expected one .idx segment per bucket");

    let mut salt_zero = 0usize;
    let mut salt_one = 0usize;
    let mut unmatched = 0usize;
    let mut header_keys = HashSet::new();
    let mut bucket_seen = [false; 16];

    for (file_bucket, _segment, path) in &files {
        bucket_seen[usize::from(*file_bucket)] = true;
        let file = parse_bucket_file(path, Some(*file_bucket)).expect("valid bucket file");
        for (key, location) in file.entries {
            if bucket_of(&key, 0) == *file_bucket {
                salt_zero += 1;
            } else if bucket_of(&key, 1) == *file_bucket {
                salt_one += 1;
                // The placeholder is the 30-byte record at slot `file_bucket`
                // of an archive, not a hash-addressed encoding key.
                assert_eq!(location.size, 30, "header record size");
                assert_eq!(
                    location.offset,
                    u64::from(*file_bucket) * 30,
                    "header record slot"
                );
                assert_eq!(&key[2..4], [0x00, 0x28], "header record tag");
                assert!(header_keys.insert(key), "header key appears once");
            } else {
                unmatched += 1;
            }
            // The client adds the 30-byte data header to the packed location
            // before selecting the archive. No install entry crosses a 2^30
            // boundary, so the payload archive is the entry's archive.
            assert_eq!(
                location.data_archive(),
                location.archive,
                "record crosses a 2^30 archive boundary"
            );
            assert!(location.archive <= 0x3fe, "client archive range");
        }
    }

    eprintln!(
        "CASC bucket hash: {salt_zero} salt-0 keys, {salt_one} salt-1 header records, \
         {unmatched} unmatched"
    );
    assert_eq!(unmatched, 0, "every entry matches one of the two salts");
    assert_eq!(salt_zero, REAL_KEYS);
    assert_eq!(salt_one, HEADER_RECORDS);
    assert_eq!(header_keys.len(), HEADER_RECORDS);
    assert!(bucket_seen.iter().all(|seen| *seen), "all 16 buckets read");
}

#[test]
fn the_store_selects_the_highest_segment_of_each_bucket() {
    let Some(root) = ow_casc::install::detect_root() else {
        eprintln!("note: no Overwatch install here; nothing to assert");
        return;
    };
    let dir = root.join("data").join("casc").join("data");
    let files = idx_files(&dir);
    let mut highest = [None; 16];
    for (bucket, segment, _) in &files {
        let slot = &mut highest[usize::from(*bucket)];
        if slot.is_none_or(|current| *segment >= current) {
            *slot = Some(*segment);
        }
    }

    let store = Store::open(Install::open(&root).expect("open install")).expect("open store");
    assert_eq!(store.segment_numbers(), highest);
    assert_eq!(store.stats().bucket_files, 16);

    // The map is keyed by the 9-byte key, so a bucket that lists the same key
    // twice (the install has 806 such pairs: one entry at the 30-byte header,
    // one at the BLTE payload) contributes one map entry.
    let mut unique = 0usize;
    for bucket in 0..16u8 {
        let Some(segment) = highest[usize::from(bucket)] else {
            continue;
        };
        let path = files
            .iter()
            .find(|(file_bucket, file_segment, _)| {
                *file_bucket == bucket && *file_segment == segment
            })
            .expect("selected segment")
            .2
            .clone();
        let keys: HashSet<[u8; 9]> = parse_bucket_file(&path, Some(bucket))
            .expect("valid selected bucket")
            .entries
            .into_iter()
            .map(|(key, _)| key)
            .collect();
        unique += keys.len();
    }
    assert_eq!(store.key_count(), unique);
}

//! The client's index-entry search and duplicate choice against the install.
//!
//! `DynamicIndexFindEntry` (`0x1425111a0`) lower-bound searches the 18-byte
//! entries, expands the equal range and decodes it. The first equal entry is
//! the chosen one. This install has 806 keys listed twice: the first entry is
//! at the 30-byte data header, the second at `offset + 30` with `size - 30`.
//! The client chooses the first; this test asserts that on every duplicate and
//! that every other real key still resolves to its own segment entry.
//!
//! Skipped when no install is present. Read-only.

use std::fs;
use std::path::{Path, PathBuf};

use ow_casc::index::{bucket_of, parse_bucket_file, parse_segment_file_name, SegmentEntries};
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

#[test]
fn find_range_and_duplicate_choice_match_the_install() {
    let Some(root) = ow_casc::install::detect_root() else {
        eprintln!("note: no Overwatch install here; nothing to assert");
        return;
    };
    let dir = root.join("data").join("casc").join("data");
    let files = idx_files(&dir);
    let store = Store::open(Install::open(&root).expect("open install")).expect("open store");
    let selected = store.segment_numbers();

    let mut real_keys = 0usize;
    let mut placeholders = 0usize;
    let mut duplicate_groups = 0usize;
    let mut entries_total = 0usize;

    for bucket in 0u8..16 {
        let segment = selected[usize::from(bucket)].expect("selected segment");
        let path = selected_path(&files, bucket, segment).expect("selected path");
        let entries = SegmentEntries::new(
            parse_bucket_file(&path, Some(bucket)).expect("valid selected segment"),
        );
        entries_total += entries.len();

        // The client's search assumes the file's non-decreasing key order.
        for pair in entries.as_slice().windows(2) {
            assert!(
                pair[0].0 <= pair[1].0,
                "bucket {bucket:02x} segment {segment:08x} is not sorted"
            );
        }

        let mut at = 0usize;
        while at < entries.len() {
            let key = entries.as_slice()[at].0;
            let mut end = at + 1;
            while end < entries.len() && entries.as_slice()[end].0 == key {
                end += 1;
            }
            let range = entries.find_range(&key);
            assert_eq!(range, at..end, "bucket {bucket:02x} key {key:02x?}");

            let first = entries.as_slice()[at].1;
            let resolved = store.location_of(&key);
            if bucket_of(&key, 0) == bucket {
                real_keys += end - at;
                assert_eq!(resolved, Some(first), "salt-0 key {key:02x?}");
            } else {
                // The 1,488 archive placeholders were written with salt 1;
                // the read lookup uses salt 0 and does not find them.
                placeholders += end - at;
                assert!(bucket_of(&key, 1) == bucket, "placeholder salt 1");
                assert_eq!(resolved, None, "salt-1 placeholder must not resolve");
            }

            if end - at > 1 {
                duplicate_groups += 1;
                assert_eq!(end - at, 2, "the install's duplicate pairs are two");
                let second = entries.as_slice()[at + 1].1;
                // The second entry is the same stored record at its payload,
                // not a different record.
                assert_eq!(first.offset + 30, second.offset, "payload offset");
                assert_eq!(first.size, second.size + 30, "payload size");
                assert_eq!(resolved, Some(first), "first duplicate wins");
                let stored = store.read_stored(&key).expect("duplicate record is local");
                assert_eq!(stored.len(), first.size as usize);
                let blte_at = if stored.starts_with(b"BLTE") {
                    0
                } else {
                    assert_eq!(&stored[30..34], b"BLTE", "header then BLTE");
                    30
                };
                assert!(stored.len() > blte_at + 4);
            }

            at = end;
        }
    }

    eprintln!(
        "index find: {entries_total} entries, {real_keys} salt-0 keys, \
         {placeholders} salt-1 placeholders, {duplicate_groups} duplicate pairs"
    );
    assert_eq!(entries_total, 1_148_620);
    assert_eq!(real_keys, 1_147_132);
    assert_eq!(placeholders, 1_488);
    assert_eq!(duplicate_groups, 806);
    assert_eq!(store.entry_count(), entries_total);
    assert_eq!(store.key_count(), 1_147_814);
}

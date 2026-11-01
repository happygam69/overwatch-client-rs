//! The archive page cache on real stored records.
//!
//! `DynamicContainerRead` (`0x1424f48d0`) asks the key state for
//! `(0x1e, size - 0x1e)` and passes the resolved absolute range to
//! `DynamicContainerReadPage` (`0x1424fcd20`). That function's reader uses the
//! 16-slot / 0x10000-byte page cache: an unaligned range is split into a head
//! fragment, a run of whole pages and a tail fragment; the head and tail come
//! from (or load) one page slot each, and the whole-page run is a single direct
//! read because the cache's non-blocking flag is set.
//!
//! This test proves three things on the install:
//!
//! 1. a small record's first read is one whole-page read and its second read
//!    is a cache hit with no archive I/O, and both return the archive's bytes;
//! 2. a record larger than a page is read as exactly the client's plan (head
//!    page, one direct whole-page run, tail page) and returns the archive's
//!    bytes;
//! 3. after 17 distinct pages the 16-slot cache has evicted the first, so
//!    re-reading it is a miss and reads the page again.
//!
//! Skipped when no install is present. Read-only.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use ow_casc::index::{
    self, archive_name, bucket_of, parse_bucket_file, parse_segment_file_name, Location,
    SegmentEntries, ARCHIVE_MASK, ARCHIVE_SHIFT, DATA_HEADER_SIZE,
};
use ow_casc::pagecache::{Plan, PAGE_SIZE};
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

/// One archive read at an absolute stored-record offset.
fn direct_read(data_dir: &Path, at: u64, len: usize) -> Vec<u8> {
    let archive = (at >> ARCHIVE_SHIFT) as u32;
    let offset = at & ARCHIVE_MASK;
    let mut file = File::open(data_dir.join(archive_name(archive))).expect("open data archive");
    file.seek(SeekFrom::Start(offset)).expect("seek");
    let mut out = vec![0u8; len];
    file.read_exact(&mut out).expect("read archive bytes");
    out
}

fn payload_start(location: Location) -> u64 {
    location.packed() + u64::from(DATA_HEADER_SIZE)
}

fn payload_len(location: Location) -> u32 {
    location.size - DATA_HEADER_SIZE
}

#[test]
fn real_stored_reads_follow_the_client_page_plan() {
    let Some(root) = ow_casc::install::detect_root() else {
        eprintln!("note: no Overwatch install here; nothing to assert");
        return;
    };
    let dir = root.join("data").join("casc").join("data");
    let files = idx_files(&dir);
    let install = Install::open(&root).expect("open install");
    let store = Store::open(install).expect("open store");
    let data_dir = store.install().data_dir();
    let selected = store.segment_numbers();

    // Collect the cases while walking the selected segments: a small payload
    // that fits one page, a payload larger than two pages, and 17 small keys
    // whose first page differs.
    let mut small: Option<([u8; 9], Location)> = None;
    let mut large: Option<([u8; 9], Location)> = None;
    let mut distinct: Vec<([u8; 9], Location)> = Vec::new();
    let mut distinct_pages: HashSet<u64> = HashSet::new();

    'buckets: for bucket in 0u8..16 {
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
            if bucket_of(&key, 0) != bucket || key[0] == 0 {
                continue;
            }
            let Some(location) = store.location_of(&key) else {
                continue;
            };
            if location.size <= DATA_HEADER_SIZE || location.archive > index::MAX_ARCHIVE {
                continue;
            }
            let len = payload_len(location);
            let start = payload_start(location);
            let in_page = (start & u64::from(PAGE_SIZE - 1)) as u32;
            if small.is_none() && in_page != 0 && len < PAGE_SIZE && in_page + len <= PAGE_SIZE {
                small = Some((key, location));
            }
            if large.is_none() && len >= 2 * PAGE_SIZE && in_page != 0 {
                large = Some((key, location));
            }
            if distinct.len() < 17 {
                let page = start & !u64::from(PAGE_SIZE - 1);
                if len < PAGE_SIZE && in_page + len <= PAGE_SIZE && distinct_pages.insert(page) {
                    distinct.push((key, location));
                }
            }
            if small.is_some() && large.is_some() && distinct.len() == 17 {
                break 'buckets;
            }
        }
    }

    let (small_key, small_location) = small.expect("a small stored record");
    let (small_key, small_location) = (&small_key, small_location);
    let small_len = payload_len(small_location) as usize;
    let small_start = payload_start(small_location);
    let small_page = (small_start & ARCHIVE_MASK) & !u64::from(PAGE_SIZE - 1);
    let small_bytes = direct_read(&data_dir, small_start, small_len);

    store.set_page_cache_tracing(true);
    store.clear_page_cache_trace();
    let first = store.read_payload(small_key).expect("read small payload");
    assert_eq!(first, small_bytes, "small payload bytes");
    let trace = store.page_cache_trace();
    assert_eq!(
        trace.len(),
        1,
        "first read is one whole-page read: {trace:?}"
    );
    assert!(trace[0].page, "the only read is a page load");
    assert_eq!(trace[0].archive, (small_start >> ARCHIVE_SHIFT) as u32);
    assert_eq!(trace[0].offset, small_page);
    assert_eq!(trace[0].len, PAGE_SIZE);

    store.clear_page_cache_trace();
    let second = store
        .read_payload(small_key)
        .expect("read small payload again");
    assert_eq!(second, small_bytes, "small payload bytes on a hit");
    assert!(
        store.page_cache_trace().is_empty(),
        "a page-cache hit must issue no archive I/O: {:?}",
        store.page_cache_trace()
    );

    // A payload larger than two pages: exactly the client's plan. The middle
    // whole-page run is one direct read, not one page load per page.
    let (large_key, large_location) = large.expect("a stored record larger than two pages");
    let (large_key, large_location) = (&large_key, large_location);
    let large_len = payload_len(large_location);
    let large_start = payload_start(large_location);
    let large_bytes = direct_read(&data_dir, large_start, large_len as usize);
    store.clear_page_cache_trace();
    let large_payload = store.read_payload(large_key).expect("read large payload");
    assert_eq!(large_payload, large_bytes, "large payload bytes");
    let trace = store.page_cache_trace();
    let plan = Plan::new(large_start & ARCHIVE_MASK, large_len);
    let mut expected = Vec::new();
    if let Some(head) = plan.head {
        expected.push((true, head.page, PAGE_SIZE));
    }
    if let Some(middle) = plan.middle {
        expected.push((false, middle.page, middle.len));
    }
    if let Some(tail) = plan.tail {
        expected.push((true, tail.page, PAGE_SIZE));
    }
    let actual: Vec<(bool, u64, u32)> = trace
        .iter()
        .map(|op| (op.page, op.offset, op.len))
        .collect();
    assert_eq!(actual, expected, "large read follows the client's plan");
    let archive = (large_start >> ARCHIVE_SHIFT) as u32;
    assert!(trace.iter().all(|op| op.archive == archive));
    if plan.middle.is_some() {
        let middle_ops = trace.iter().filter(|op| !op.page).count();
        assert_eq!(middle_ops, 1, "the whole-page run is one direct read");
    }
    let large_ops = trace.len();

    // The cache holds 16 pages. Seventeen distinct pages evict the first; its
    // re-read is a miss and loads the page again. The earlier reads are
    // dropped first so the 17 pages are the only candidates.
    store.clear_page_cache();
    for (key, _) in &distinct {
        store.read_payload(key).expect("read distinct-page payload");
    }
    let first_page = (payload_start(distinct[0].1) & ARCHIVE_MASK) & !u64::from(PAGE_SIZE - 1);
    let resident = store.page_cache_resident();
    assert_eq!(resident.len(), 16, "the cache keeps 16 page slots");
    assert!(
        !resident.iter().any(|(_, page, _, _)| *page == first_page),
        "the 17th distinct page evicts the first"
    );
    store.clear_page_cache_trace();
    store
        .read_payload(&distinct[0].0)
        .expect("re-read evicted page");
    let trace = store.page_cache_trace();
    assert_eq!(
        trace,
        vec![ow_casc::pagecache::ReadOp {
            page: true,
            archive: (payload_start(distinct[0].1) >> ARCHIVE_SHIFT) as u32,
            offset: first_page,
            len: PAGE_SIZE,
        }],
        "an evicted page is loaded again"
    );

    eprintln!(
        "page cache: small key size {} B (payload {small_len} B, page {small_page:#x}); \
         large key size {} B (payload {large_len} B, {large_ops} trace ops); \
         {} resident slots after {} distinct pages",
        small_location.size,
        large_location.size,
        store.page_cache_resident().len(),
        distinct.len()
    );
}

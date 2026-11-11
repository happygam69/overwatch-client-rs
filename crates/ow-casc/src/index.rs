//! The bucket index: maps a 9-byte encoding key to a location in a data file.
//!
//! `data/<bucket:02X>*.idx` holds 16 buckets. Each file starts with a
//! `BlockSizeAndHash` section header (`i32` byte count, `i32` hash), a 16-byte
//! file header, padding to the 16-byte-aligned end of that section, then the
//! entry block's own `BlockSizeAndHash` and that many bytes of fixed 18-byte
//! entries:
//!
//! ```text
//! EKey[9]  | location[5, big-endian] | size u32 LE
//! ```
//!
//! The two fields disagree on endianness, which is easy to get wrong: the
//! 40-bit location splits into `archive = value >> 30` and
//! `offset = value & (2^30 - 1)` read big-endian, while `size` is a plain
//! little-endian `u32`. `size` counts the *whole* stored record, including the
//! 30-byte data header when the payload is wrapped in one.
//!
//! Files are preallocated, so a zero-padded tail follows the entries; the entry
//! count comes from the block header, never from the file size.
//!
//! Bucket selection is the client's `DynamicKeyStateLookup` (`0x142501b40`)
//! exactly: XOR the nine key bytes, fold the high nibble into the low one,
//! add the caller's salt, and keep four bits. Every lookup caller passes a
//! salt of 0; [`bucket_of`] keeps the parameter because the client has it.
//!
//! A bucket's file name is `<bucket:02x><segment:08x>.idx`. The client's
//! `DynamicIndexSegmentOpen` (`0x14250e400`) builds it from the bucket and the
//! segment number in its per-bucket table; its directory scan
//! (`DynamicIndexScanSegments`) parses the same shape and keeps the highest segment per
//! bucket. [`parse_segment_file_name`] is that parser and [`segment_name`] is
//! the formatter.

use std::cell::OnceCell;
use std::cmp::Ordering;
use std::fmt;
use std::fs;
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};

/// The leading `BlockSizeAndHash` section header.
pub const SECTION_HEADER: usize = 8;
/// The 16-byte file header that follows it (version, bucket, widths, ...).
pub const FILE_HEADER_SIZE: usize = 16;
pub const ENTRY_SIZE: usize = 18;
/// Where entries begin in the usual file: a 16-byte section, padding to 32,
/// then the entry block's 8-byte header. Real files can declare another
/// section size; `parse_bucket_file` computes the offset from it.
pub const ENTRIES_OFFSET: usize = 40;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Malformed(PathBuf, &'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "io error: {e}"),
            Error::Malformed(p, m) => write!(f, "malformed index {}: {m}", p.display()),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

type Result<T> = std::result::Result<T, Error>;

/// Where a stored record lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Location {
    /// Ordinal of `data/data.NNN` holding the record.
    pub archive: u32,
    /// Byte offset of the record within that archive.
    pub offset: u64,
    /// Stored length, including any 30-byte data header.
    pub size: u32,
}

impl Location {
    /// The client's 40-bit virtual offset: the index entry's five big-endian
    /// bytes read as `(archive << 30) | offset`.
    pub fn packed(&self) -> u64 {
        (u64::from(self.archive) << ARCHIVE_SHIFT) | self.offset
    }

    /// The archive the client's payload read selects.
    ///
    /// `DynamicContainerRead` (`0x1424f48d0`) adds the 30-byte data header to
    /// the packed location before `DynamicContainerReadPage`
    /// (`0x1424fcd20`) shifts it right by 30. On this installation no record
    /// crosses a 2^30 boundary, so this is the entry's own archive for every
    /// key; the arithmetic is kept because it is the client's expression.
    pub fn data_archive(&self) -> u32 {
        ((self.packed() + u64::from(DATA_HEADER_SIZE)) >> ARCHIVE_SHIFT) as u32
    }

    /// Split a packed `archive << 30 | offset` value the way the client's
    /// decoders do. The 18-byte entry and the 24-byte page record carry the
    /// same five big-endian bytes, so both paths end at this decomposition.
    pub fn from_packed(packed: u64, size: u32) -> Location {
        Location {
            archive: (packed >> ARCHIVE_SHIFT) as u32,
            offset: packed & ARCHIVE_MASK,
            size,
        }
    }
}

/// The client's 10-bit archive field: `location >> 30`.
pub const ARCHIVE_SHIFT: u32 = 30;
/// The low 30 bits of the packed location.
pub const ARCHIVE_MASK: u64 = (1u64 << ARCHIVE_SHIFT) - 1;
/// `DynamicContainerReadPage` rejects an archive above this before opening it.
pub const MAX_ARCHIVE: u32 = 0x3fe;
/// The data header `DynamicContainerRead` skips before reading a payload.
pub const DATA_HEADER_SIZE: u32 = 0x1e;

/// One record in a `.idx` page table.
///
/// The record is 24 bytes: a `u32` stored hash at `+0`, a 9-byte key at `+4`,
/// a 5-byte big-endian location at `+13`, a 4-byte little-endian size at
/// `+18`, a one-byte type at `+22`, and one pad byte. The stored hash is
/// `DynamicIndexKeyHash(record + 4, 0x13, 0) | 0x80000000`; zero marks the
/// first empty slot, not a valid record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageRecord {
    pub stored_hash: u32,
    pub key: [u8; 9],
    pub location: Location,
    /// The decoder's type byte. See [`SegmentEntries::find`] for the chosen
    /// entry effect; the interval-buffer effect is not modelled.
    pub kind: u8,
}

/// One 0x200-byte page holds 21 24-byte records; the last 8 bytes are pad.
pub const PAGE_SIZE: usize = 0x200;
pub const PAGE_RECORD_COUNT: usize = 21;
pub const PAGE_RECORD_SIZE: usize = 24;
/// The client's parser requires at least this much page-table space.
pub const PAGE_TABLE_MIN_BYTES: usize = 0x7800;

/// The page table that follows the 18-byte entry block.
///
/// `DynamicIndexFindEntry` (`0x1425111a0`) scans pages and records in file
/// order and stops at the first record whose stored hash is zero. There is no
/// hash-based page selection: the stored hash is a validity check, and the
/// scan is linear. [`PageTable::matches`] returns only the records before the
/// first zero, exactly the prefix the client can reach.
#[derive(Debug, Clone, Default)]
pub struct PageTable {
    page_count: usize,
    records: Vec<PageRecord>,
    first_empty: Option<(usize, usize)>,
    nonzero_records: usize,
}

impl PageTable {
    /// Construct a table for a synthetic fixture. `records` must be the prefix
    /// before `first_empty` (or the whole table when no slot is empty).
    pub fn from_records(
        page_count: usize,
        records: Vec<PageRecord>,
        first_empty: Option<(usize, usize)>,
    ) -> PageTable {
        let nonzero_records = records.len();
        PageTable {
            page_count,
            records,
            first_empty,
            nonzero_records,
        }
    }

    pub fn page_count(&self) -> usize {
        self.page_count
    }

    /// Records before the first zero stored hash, in file order.
    pub fn records(&self) -> &[PageRecord] {
        &self.records
    }

    /// `(page, record)` of the first zero stored hash, if any.
    pub fn first_empty(&self) -> Option<(usize, usize)> {
        self.first_empty
    }

    /// Number of non-zero stored hashes across every page and record, including
    /// any after the first zero (which the client never reads).
    pub fn nonzero_records(&self) -> usize {
        self.nonzero_records
    }

    pub fn total_slots(&self) -> usize {
        self.page_count * PAGE_RECORD_COUNT
    }

    /// The records for `key` in the client's scan order: page by page, record
    /// by record, stopping at the first zero hash.
    pub fn matches<'a>(&'a self, key: &[u8; 9]) -> impl Iterator<Item = &'a PageRecord> + 'a {
        let key = *key;
        self.records.iter().filter(move |record| record.key == key)
    }

    /// The table's byte form, as [`find_entry`] walks it: `page_count`
    /// 0x200-byte pages, the retained records in their slots (they are the
    /// sequential prefix before the first zero), and zero everywhere else,
    /// so the first zero is at [`Self::first_empty`] and a full table has
    /// none. Pad bytes are never read by the scan or the decode.
    fn page_bytes(&self) -> Vec<u8> {
        let mut bytes = vec![0u8; self.page_count * PAGE_SIZE];
        for (slot, record) in self.records.iter().enumerate() {
            let at = (slot / PAGE_RECORD_COUNT) * PAGE_SIZE
                + (slot % PAGE_RECORD_COUNT) * PAGE_RECORD_SIZE;
            bytes[at..at + 4].copy_from_slice(&record.stored_hash.to_le_bytes());
            bytes[at + 4..at + 13].copy_from_slice(&record.key);
            bytes[at + 13..at + 18].copy_from_slice(&record.location.packed().to_be_bytes()[3..8]);
            bytes[at + 18..at + 22].copy_from_slice(&record.location.size.to_le_bytes());
            bytes[at + 22] = record.kind;
        }
        bytes
    }

    /// Parse the client's page table from a loaded segment.
    ///
    /// The first zero stored hash stops the client's validation and lookup;
    /// this parser validates the records before it, retains only that prefix,
    /// and still counts non-zero hashes after it for diagnostics.
    fn parse(
        data: &[u8],
        offset: usize,
        page_count: usize,
    ) -> std::result::Result<PageTable, &'static str> {
        let mut records = Vec::new();
        let mut first_empty = None;
        let mut nonzero_records = 0usize;
        for page in 0..page_count {
            for record in 0..PAGE_RECORD_COUNT {
                let at = offset + page * PAGE_SIZE + record * PAGE_RECORD_SIZE;
                let bytes = data
                    .get(at..at + PAGE_RECORD_SIZE)
                    .ok_or("page-table record runs past the end of the file")?;
                let stored = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                if stored == 0 {
                    if first_empty.is_none() {
                        first_empty = Some((page, record));
                    }
                    continue;
                }
                nonzero_records += 1;
                if first_empty.is_some() {
                    // The client stopped reading here; do not validate or
                    // retain records after its terminating zero.
                    continue;
                }
                let payload = &bytes[4..4 + 19];
                let expected = key_hash(payload, 0) | 0x8000_0000;
                if stored != expected {
                    return Err("page-table record hash mismatch");
                }
                let mut key = [0u8; 9];
                key.copy_from_slice(&payload[0..9]);
                let mut packed = 0u64;
                for byte in &payload[9..14] {
                    packed = (packed << 8) | u64::from(*byte);
                }
                let size = u32::from_le_bytes([payload[14], payload[15], payload[16], payload[17]]);
                records.push(PageRecord {
                    stored_hash: stored,
                    key,
                    location: Location {
                        archive: (packed >> ARCHIVE_SHIFT) as u32,
                        offset: packed & ARCHIVE_MASK,
                        size,
                    },
                    kind: payload[18],
                });
            }
        }
        Ok(PageTable {
            page_count,
            records,
            first_empty,
            nonzero_records,
        })
    }
}

/// A parsed `.idx` file: the sorted 18-byte entries and the page table.
#[derive(Debug, Clone, Default)]
pub struct BucketFile {
    pub entries: Vec<([u8; 9], Location)>,
    pub pages: PageTable,
}

/// The 18-byte entries of one segment, in the file's order.
///
/// `DynamicIndexFindEntry` (`0x1425111a0`) binary-searches this array and
/// expands the equal range. The entries are not a map: this install lists 806
/// keys twice, and the client's chosen entry is the **first** one in file
/// order (see [`SegmentEntries::find`]).
#[derive(Debug, Clone, Default)]
pub struct SegmentEntries {
    entries: Vec<([u8; 9], Location)>,
    pages: PageTable,
    /// The segment's byte form, built once and reused.
    ///
    /// The bytes are a pure function of `entries` and `pages` - the same
    /// 18-byte records and the same page table the client keeps in memory
    /// after it opens the segment. `key_state` used to rebuild them on every
    /// call, which made a per-key sweep quadratic in the segment size: the
    /// install test resolves 1.1M keys and copied tens of gigabytes.
    bytes: OnceCell<SegmentBytes>,
}

/// The segment's byte forms: the sorted 18-byte entries, the page table
/// bytes and the page count.
#[derive(Debug, Clone, Default)]
struct SegmentBytes {
    entries: Vec<u8>,
    page_table: Vec<u8>,
    page_count: u32,
}

/// The segment's byte forms, built once per segment.
///
/// The layout is the client's: `EKey[9] | location[5, big-endian] |
/// size u32 LE` per entry, then the page table's own bytes.
fn build_segment_bytes(entries: &[([u8; 9], Location)], pages: &PageTable) -> SegmentBytes {
    let mut bytes = Vec::with_capacity(entries.len() * ENTRY_SIZE);
    for (ekey, location) in entries {
        bytes.extend_from_slice(ekey);
        bytes.extend_from_slice(&location.packed().to_be_bytes()[3..8]);
        bytes.extend_from_slice(&location.size.to_le_bytes());
    }
    SegmentBytes {
        entries: bytes,
        page_table: pages.page_bytes(),
        page_count: pages.page_count() as u32,
    }
}

impl SegmentEntries {
    pub fn new(entries: Vec<([u8; 9], Location)>) -> SegmentEntries {
        SegmentEntries {
            entries,
            pages: PageTable::default(),
            bytes: OnceCell::new(),
        }
    }

    pub fn with_pages(entries: Vec<([u8; 9], Location)>, pages: PageTable) -> SegmentEntries {
        SegmentEntries {
            entries,
            pages,
            bytes: OnceCell::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn as_slice(&self) -> &[([u8; 9], Location)] {
        &self.entries
    }

    pub fn pages(&self) -> &PageTable {
        &self.pages
    }

    /// The client's binary search and equal-range expansion.
    ///
    /// `DynamicIndexFindEntry` keeps `lo = 0`, `hi = count`; each step compares
    /// `memcmp(key, entry[lo + (hi - lo) / 2], 9)`. A smaller key moves `hi` to
    /// the midpoint, a larger key moves `lo` past it. On equality it records
    /// the midpoint, walks backward while the previous entry has the same nine
    /// bytes, walks forward while the next entry has them, and decodes
    /// `[first, one-past-last)`. An absent key therefore yields an empty range
    /// at its insertion point; the returned range is the client's exact range,
    /// not `std::slice::binary_search`'s single-index result.
    pub fn find_range(&self, key: &[u8; 9]) -> Range<usize> {
        let entries = &self.entries;
        let mut lo = 0usize;
        let mut hi = entries.len();
        while lo != hi {
            let mid = (lo + hi) / 2;
            match key.cmp(&entries[mid].0) {
                Ordering::Equal => {
                    lo = mid;
                    hi = mid + 1;
                    while lo > 0 && entries[lo - 1].0 == *key {
                        lo -= 1;
                    }
                    while hi < entries.len() && entries[hi].0 == *key {
                        hi += 1;
                    }
                    return lo..hi;
                }
                Ordering::Less => hi = mid,
                Ordering::Greater => lo = mid + 1,
            }
        }
        lo..hi
    }

    /// The entry the client resolves, without the interval side effects.
    ///
    /// This is [`SegmentEntries::key_state`]'s chosen entry: the first equal
    /// 18-byte entry (type 1 for a singleton, type 2 for the first of several),
    /// then every matching page record in page/record order through
    /// `DynamicIndexEntrySink` (`0x142520590`). Page type 1 or 2 replaces the
    /// chosen entry, type 3 clears it, and types 4-7 leave it alone. The
    /// interval buffer only matters to [`SegmentEntries::resolve_range`], so
    /// this method is the chosen half of the same state.
    pub fn find(&self, key: &[u8; 9]) -> Option<Location> {
        self.key_state(key).chosen().map(ChosenEntry::to_location)
    }

    /// Build the key state in the client's decode order.
    ///
    /// This is [`find_entry`] (`DynamicIndexFindEntry`, `0x1425111a0`) over
    /// the segment's byte forms: the sorted 18-byte entries, the page table
    /// bytes, and the fresh key state the caller builds with
    /// `DynamicIndexFindContextInit` (`0x142520520`). The out page/record
    /// words are not part of the shipped state, so they are discarded here.
    pub fn key_state(&self, key: &[u8; 9]) -> KeyState {
        let bytes = self
            .bytes
            .get_or_init(|| build_segment_bytes(&self.entries, &self.pages));
        let mut state = KeyState::default();
        let _ = find_entry(
            &bytes.entries,
            &bytes.page_table,
            bytes.page_count,
            key,
            &mut state,
        );
        state
    }

    /// `DynamicIndexResolveRange` (`0x1425115d0`) over this segment.
    ///
    /// The request is `(start, len)` in stored-record bytes: `start` is the
    /// caller's offset, not an absolute location. `len == 0xffffffff` means
    /// "to the end of the chosen entry". The result's status codes are the
    /// client's: 0 found, 2 no entry, 3 not resident, 4 past the entry's end.
    pub fn resolve_range(&self, key: &[u8; 9], start: u64, len: u32) -> ResolveResult {
        self.key_state(key).resolve(start, len)
    }

    /// Number of distinct keys, using the client's non-decreasing order.
    pub fn unique_key_count(&self) -> usize {
        if self.entries.is_empty() {
            0
        } else {
            1 + self
                .entries
                .windows(2)
                .filter(|pair| pair[0].0 != pair[1].0)
                .count()
        }
    }
}

/// The two output words `DynamicIndexFindEntry` (`0x1425111a0`) writes
/// through its optional out arguments: the page index and the record index
/// of the first zero stored hash in the page scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FindEntryOut {
    pub page: u32,
    pub record: u32,
}

/// `DynamicIndexFindEntry` (`0x1425111a0`): find a 9-byte key in one segment
/// and decode it into a fresh key state.
///
/// Measured extent `0x1425111a0..0x142511539` (921 bytes through the `ret`
/// at `0x142511538`; the `.pdata` root is only `0x1425111a0..0x1425111c4`,
/// 0x24 bytes, and the body continues past it). Signature
/// `(segment, key, key_state, out_page, out_record) -> void`
/// (RCX segment, RDX key, R8 key state, R9 out page, stack out record).
/// The segment supplies the 18-byte entries at `+0x58`, the entry count at
/// `+0x60`, the page-table base at `+0x68` and the page count at `+0x70`;
/// this byte-view form takes those four directly. The key state is the
/// one `DynamicIndexFindContextInit` (`0x142520520`) builds (found at
/// `+0x10`, chosen at `+0x18`, interval buffer at `+0x28` with its base
/// at `+0x128`, count at `+0x130`, capacity 16 at `+0x134`) - in
/// production the caller builds it, this function never calls that init.
///
/// The walk (`0x142511219..0x1425112d5`) is a lower bound with
/// `lo = 0, hi = count`: `mid = (lo + hi) >> 1` in 32-bit arithmetic
/// (`lea eax, [r15+r13]` / `shr eax, 1` at `0x142511221`), `memcmp(key,
/// entry[mid], 9)` through `0x142efecf0`. A negative result moves `hi` to
/// `mid`, a positive one moves `lo` to `mid + 1`, and the loop ends when
/// `lo == hi` (`cmp r13d, r15d` / `jne` at `0x142511257`). On equality
/// the midpoint is recorded, the backward walk (`0x142511280`) steps
/// while the previous entry's first eight bytes and ninth byte match,
/// and the forward walk (`0x1425112b0`) steps while the next entry's do;
/// the decode range is `[lo, hi)`, empty at the insertion point for an
/// absent key. An empty entry count skips the walk (`test esi, esi` /
/// `je` at `0x142511219`) and decodes the empty range.
///
/// The page scan (`0x142511318..0x14251145b`) runs when the page count
/// at segment `+0x70` is non-zero (`cmp [rax+0x70], 0` / `jbe` at
/// `0x14251131b`): for each page, for each of 21 records, it stops at
/// the first record whose stored hash (the `u32` at `page*0x200 +
/// record*0x18`) is zero, and otherwise appends the record's 19-byte
/// payload (at `+4`) to a pointer array when its key matches, by the
/// same first-byte / qword / ninth-byte compare. The out words are
/// that first zero's `(page, record)`, `(page_count, 21)` when every
/// slot is non-zero, and `(0, 0)` when the page count is zero. The
/// pointer array lives on the stack with capacity 32; growing it past
/// 32 matches calls the allocator vtable at `[segment]` (`+0x18` alloc,
/// `+0x30` free) and is the declared gap of `fn/DynamicIndexFindEntry`.
///
/// The decode is the call to `DynamicIndexDecodeEntries`
/// (`0x1425209a0`) at `0x1425114a3` with the equal range as the 18-byte
/// source, the collected payloads as the pointer-array source, and a
/// null contiguous range. Each entry/page payload decodes the 5-byte
/// big-endian location at `+9` and the little-endian `u32` size at
/// `+14`. The two out words are written only when their pointer is
/// non-null (`test rax, rax` / `je` at `0x1425114e0` and `0x1425114f5`).
pub fn find_entry(
    entries: &[u8],
    page_table: &[u8],
    page_count: u32,
    key: &[u8; 9],
    state: &mut KeyState,
) -> FindEntryOut {
    let count = entries.len() / ENTRY_SIZE;
    let mut lo = 0usize;
    let mut hi = count;
    while lo != hi {
        let mid = ((lo as u32).wrapping_add(hi as u32) >> 1) as usize;
        let entry_key = &entries[mid * ENTRY_SIZE..mid * ENTRY_SIZE + 9];
        match key.as_slice().cmp(entry_key) {
            Ordering::Equal => {
                lo = mid;
                hi = mid + 1;
                while lo > 0 && &entries[(lo - 1) * ENTRY_SIZE..(lo - 1) * ENTRY_SIZE + 9] == key {
                    lo -= 1;
                }
                while hi < count && &entries[hi * ENTRY_SIZE..hi * ENTRY_SIZE + 9] == key.as_slice()
                {
                    hi += 1;
                }
                break;
            }
            Ordering::Less => hi = mid,
            Ordering::Greater => lo = mid + 1,
        }
    }
    let mut indirect: Vec<[u8; 19]> = Vec::new();
    let mut out = FindEntryOut { page: 0, record: 0 };
    if page_count != 0 {
        let mut first_empty = None;
        'pages: for page in 0..page_count {
            for record in 0..PAGE_RECORD_COUNT {
                let at = page as usize * PAGE_SIZE + record * PAGE_RECORD_SIZE;
                let stored = u32::from_le_bytes([
                    page_table[at],
                    page_table[at + 1],
                    page_table[at + 2],
                    page_table[at + 3],
                ]);
                if stored == 0 {
                    first_empty = Some((page, record as u32));
                    break 'pages;
                }
                if &page_table[at + 4..at + 13] == key.as_slice() {
                    let mut payload = [0u8; 19];
                    payload.copy_from_slice(&page_table[at + 4..at + 23]);
                    indirect.push(payload);
                }
            }
        }
        out = match first_empty {
            Some((page, record)) => FindEntryOut { page, record },
            None => FindEntryOut {
                page: page_count,
                record: PAGE_RECORD_COUNT as u32,
            },
        };
    }
    state.decode_entries(&entries[lo * ENTRY_SIZE..hi * ENTRY_SIZE], &indirect, &[]);
    out
}

/// Number of buckets in a dynamic index.
pub const BUCKET_COUNT: usize = 16;

/// One 16-byte key-state interval event.
///
/// The client's buffer at key-state `+0x128` holds `{u64 start, i32 delta}`
/// records with four bytes of padding. `DynamicIndexAppendInterval`
/// (`0x142520700`) writes the pair and `DynamicIndexNormalizeIntervals`
/// (`0x1425208d0`) rewrites the buffer in place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntervalEvent {
    /// Event position in packed `archive << 30 | offset` space.
    pub start: u64,
    /// Signed coverage change. Resident intervals use `+1`/`-1`; a
    /// non-resident interval uses the two `0x40000000` magnitudes.
    pub delta: i32,
}

/// The delta `DynamicIndexAppendInterval` writes at a non-resident interval's
/// start. Its end event uses the negated value, `+0x40000000`.
pub const NON_RESIDENT_DELTA: i32 = -0x4000_0000;

/// The key state's interval buffer.
///
/// `DynamicIndexAppendInterval` (`0x142520700`) adds two events per range:
/// `(start, +1)` and `(start + len, -1)` for a resident range, or
/// `(start, -0x40000000)` and `(start + len, +0x40000000)` for a non-resident
/// one. Only the non-resident append normalizes immediately; the resident
/// appends are normalized once by `DynamicIndexDecodeEntries`.
///
/// The end position is `start + (u32)len` as a 64-bit add. The deltas are
/// 32-bit wrapping values: the normalize sweep and the resident check both add
/// them with wrap-around, exactly as the client's `ADD` instructions do.
#[derive(Debug, Clone, Default)]
pub struct IntervalSet {
    /// The client's buffer, as the slots it has ever written. The logical
    /// buffer is `events[..count]`; slots at `count..` are the stale tail
    /// the client's normalize leaves behind (see [`IntervalSet::normalize`]).
    events: Vec<IntervalEvent>,
    /// The client's count at key-state `+0x130`.
    count: usize,
}

impl IntervalSet {
    pub fn new() -> IntervalSet {
        IntervalSet::default()
    }

    /// Build a set from raw events without normalizing. The client's
    /// normalize is the subject of the arithmetic tests; production callers
    /// must use [`IntervalSet::append`] so the buffer reaches the same shape.
    pub fn from_events(events: Vec<IntervalEvent>) -> IntervalSet {
        let count = events.len();
        IntervalSet { events, count }
    }

    /// The logical buffer: the first `count` events, as the client reads it.
    pub fn events(&self) -> &[IntervalEvent] {
        &self.events[..self.count]
    }

    /// Every slot the buffer has ever written, including the stale tail at
    /// `count..` that the client's normalize leaves in place. The client's
    /// byte-level buffer is exactly this sequence of records.
    pub fn physical_events(&self) -> &[IntervalEvent] {
        &self.events
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn len(&self) -> usize {
        self.count
    }

    /// The client's count clear (`mov dword ptr [obj+0x130], 0`): the count
    /// goes to zero and the buffer slots stay, exactly as the client's do.
    pub fn clear(&mut self) {
        self.count = 0;
    }

    /// `DynamicIndexAppendInterval` (`0x142520700`): append the range's two
    /// events, and normalize immediately for a non-resident range.
    ///
    /// The client's capacity check grows the buffer when
    /// `capacity <= count + 1`; the Rust `Vec` has no observable capacity
    /// contract, and the initial client capacity of 16 is not part of any
    /// decode result.
    pub fn append(&mut self, start: u64, len: u32, resident: bool) {
        let delta = if resident { 1 } else { NON_RESIDENT_DELTA };
        self.write_event(IntervalEvent { start, delta });
        self.write_event(IntervalEvent {
            start: start.wrapping_add(u64::from(len)),
            delta: delta.wrapping_neg(),
        });
        if !resident {
            self.normalize();
        }
    }

    /// Write one event at the count and increment the count, as the client's
    /// append does: a slot the buffer has written before is overwritten, a
    /// slot past the high-water mark extends it.
    fn write_event(&mut self, event: IntervalEvent) {
        if self.count < self.events.len() {
            self.events[self.count] = event;
        } else {
            self.events.push(event);
        }
        self.count += 1;
    }

    /// Sort by start, sum equal-start deltas, and keep only coverage changes.
    ///
    /// `DynamicIndexNormalizeIntervals` (`0x1425208d0`) calls the client's
    /// unstable 16-byte sort (`FUN_14251ff30`) on the start word, walks the
    /// equal-start groups summing their deltas with 32-bit wrap, and emits
    /// `(start, +1)` when coverage goes from `<= 0` to `> 0`, or
    /// `(start, -1)` when it goes from `> 0` to `<= 0`. The group's start is
    /// the first event's start. Events that do not cross a boundary are
    /// dropped; the written count is the number of emitted events.
    pub fn normalize(&mut self) {
        self.events[..self.count].sort_unstable_by_key(|event| event.start);
        let mut out = Vec::with_capacity(self.count);
        let mut coverage: i32 = 0;
        let mut at = 0usize;
        while at < self.count {
            let start = self.events[at].start;
            let mut sum = coverage.wrapping_add(self.events[at].delta);
            at += 1;
            while at < self.count && self.events[at].start == start {
                sum = sum.wrapping_add(self.events[at].delta);
                at += 1;
            }
            let delta = if coverage > 0 {
                (sum <= 0).then_some(-1)
            } else {
                (sum > 0).then_some(1)
            };
            if let Some(delta) = delta {
                out.push(IntervalEvent { start, delta });
            }
            coverage = sum;
        }
        // The client compacts the emitted prefix forward over the sorted
        // buffer and stores the emitted count; the sorted records at
        // `new_count..old_count` stay in place as the tail. Copying `out`
        // over the front and keeping the rest of the Vec is that compaction.
        self.events[..out.len()].copy_from_slice(&out);
        self.count = out.len();
    }

    /// `DynamicIndexRangeIsResident` (`0x142520c60`) on a normalized buffer.
    ///
    /// Walk events while `event.start <= start`, adding each delta to the
    /// coverage. At the end of the buffer the client returns `flag ^ 1`.
    /// Otherwise, with `resident` (the client's flag 1) it answers "the whole
    /// request is inside resident coverage": coverage must be `> 0` and
    /// `start + len <= next_event.start`. With `resident == false` (flag 0) it
    /// answers the complementary case "the whole request is in an uncovered
    /// gap": coverage must be `<= 0` and `start + len <= next_event.start`.
    /// The end comparison is inclusive, and the start comparison consumes an
    /// event whose start equals the request's start. The client's flag-mismatch
    /// return is non-zero with a zero low byte; its callers test only the low
    /// byte, so this returns the low-byte answer as a bool.
    pub fn range_is_resident(&self, start: u64, len: u32, resident: bool) -> bool {
        let end = start.wrapping_add(u64::from(len));
        let mut coverage: i32 = 0;
        let mut at = 0usize;
        while at < self.count {
            if start < self.events[at].start {
                break;
            }
            coverage = coverage.wrapping_add(self.events[at].delta);
            at += 1;
        }
        if at == self.count {
            return !resident;
        }
        if coverage > 0 {
            if !resident {
                return false;
            }
        } else if resident {
            return false;
        }
        end <= self.events[at].start
    }
}

/// The client's chosen entry, the 16-byte decode result copied to `out_entry`.
///
/// `DynamicIndexResolveRange` copies all 16 bytes, but the decoder only
/// initializes the packed location (8) and the size (4); the trailing dword is
/// never read back. [`ChosenEntry`] therefore carries it only so byte-level
/// copies (the sink's `movups` on types 1/2, at `0x1425205c5`/`0x1425205ec`)
/// and the type-3 clear, which writes only the location and size and leaves
/// the trailing dword in place, can be modelled exactly.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ChosenEntry {
    /// Packed `archive << 30 | offset` location, as the five big-endian entry
    /// or page-record bytes decode.
    pub location: u64,
    /// Stored size, including the 30-byte data header when present.
    pub size: u32,
    /// The record's trailing dword (the `movups` at `0x1425205c5` copies it
    /// along with the location and size). Never read back as a value.
    pub trailing: u32,
}

impl ChosenEntry {
    pub fn to_location(self) -> Location {
        Location::from_packed(self.location, self.size)
    }
}

/// The chosen entry one 18-byte index entry decodes to: the 5-byte
/// big-endian location at `+9` and the little-endian u32 size at `+14`.
/// The key bytes at `+0..9` are never read by the decoder.
fn decode_entry_18(record: &[u8]) -> ChosenEntry {
    decode_entry_common(record)
}

/// The chosen entry one 19-byte payload record decodes to: the same
/// location at `+9` and size at `+14`. The type byte at `+18` is the
/// sink's argument, not part of the entry.
fn decode_entry_19(record: &[u8]) -> ChosenEntry {
    decode_entry_common(record)
}

fn decode_entry_common(record: &[u8]) -> ChosenEntry {
    let mut packed = 0u64;
    for byte in &record[9..14] {
        packed = (packed << 8) | u64::from(*byte);
    }
    ChosenEntry {
        location: packed,
        size: u32::from_le_bytes([record[14], record[15], record[16], record[17]]),
        // The 18/19-byte records carry no trailing dword; the decoder's
        // stack slot for it is never written (see fn/DynamicIndexDecodeEntries:
        // the oracle observes it as zero, and the fixture pins that).
        trailing: 0,
    }
}

/// The key state built by one lookup: the chosen entry plus its interval
/// buffer, in the client's decode order.
///
/// This is the stack context `DynamicIndexFindContextInit` (`0x142520520`)
/// creates: found flag at `+0x10`, chosen entry at `+0x18`, interval buffer at
/// `+0x128`, count at `+0x130`, capacity at `+0x134`.
#[derive(Debug, Clone, Default)]
pub struct KeyState {
    found: bool,
    chosen: ChosenEntry,
    intervals: IntervalSet,
}

impl KeyState {
    pub fn found(&self) -> bool {
        self.found
    }

    pub fn chosen(&self) -> Option<ChosenEntry> {
        self.found.then_some(self.chosen)
    }

    /// The chosen entry as stored, even when the state is unfound: the
    /// type-3 sink zeroes it and every other sink either replaces it or
    /// leaves it, so an unfound state's entry is still observable in the
    /// client's bytes at key-state `+0x18`.
    pub fn raw_chosen(&self) -> ChosenEntry {
        self.chosen
    }

    pub fn intervals(&self) -> &IntervalSet {
        &self.intervals
    }

    /// Mutable access to the interval buffer, for the verify probe's
    /// inverted-range case (end < begin), which the slice-based
    /// [`Self::decode_entries`] cannot represent: the probe drives
    /// clear/sink/normalize directly there.
    pub fn intervals_mut(&mut self) -> &mut IntervalSet {
        &mut self.intervals
    }

    /// A key state with the caller's found flag and chosen entry, and an
    /// empty interval buffer. `DynamicIndexDecodeEntries` preserves the
    /// found flag and chosen entry it is handed (only the interval count
    /// is cleared), so a caller that builds the state itself starts here.
    pub fn from_initial(found: bool, chosen: ChosenEntry) -> KeyState {
        KeyState {
            found,
            chosen,
            intervals: IntervalSet::default(),
        }
    }

    /// `DynamicIndexDecodeEntries` (`0x1425209a0`): decode three record
    /// sources into this state, in order, then normalize once.
    ///
    /// The sources are the client's, by their stride and decode:
    ///
    /// - `entries`: the sorted equal range, 18-byte records (`key[9]`,
    ///   5-byte big-endian location, u32 little-endian size). The count is
    ///   `(end - begin) / 18`; one record is sink type 1, more are type 2
    ///   for the first and type 6 for each later record. Nothing is
    ///   decoded for a null or empty range.
    /// - `indirect`: the records a pointer array points at, each 19 bytes
    ///   (`key[9]`, location, size, type). The type byte is the sink's;
    ///   the caller resolves the pointers, the client dereferences them.
    /// - `contiguous`: the same 19-byte records back to back.
    ///
    /// The interval count is cleared first; the found flag and the chosen
    /// entry are preserved until a sink changes them. The final normalize
    /// is the tail-jump to `DynamicIndexNormalizeIntervals` (`0x1425208d0`).
    pub fn decode_entries(&mut self, entries: &[u8], indirect: &[[u8; 19]], contiguous: &[u8]) {
        self.intervals.clear();
        let mut records = entries.chunks_exact(ENTRY_SIZE);
        match entries.len() / ENTRY_SIZE {
            0 => {}
            1 => {
                let entry = decode_entry_18(records.next().expect("one 18-byte entry"));
                self.sink(1, entry);
            }
            _ => {
                let entry = decode_entry_18(records.next().expect("the first entry"));
                self.sink(2, entry);
                for record in records {
                    self.sink(6, decode_entry_18(record));
                }
            }
        }
        for record in indirect {
            self.sink(record[18], decode_entry_19(record));
        }
        for record in contiguous.chunks_exact(19) {
            self.sink(record[18], decode_entry_19(record));
        }
        self.intervals.normalize();
    }

    /// Whether the absolute range is covered by the resident intervals.
    /// [`IntervalSet::range_is_resident`] with the client's flag 1.
    pub fn is_resident(&self, start: u64, len: u32) -> bool {
        self.intervals.range_is_resident(start, len, true)
    }

    /// `DynamicIndexEntrySink` (`0x142520590`), one type byte at a time.
    ///
    /// The type byte arrives in `DL`; a `dec`/`cmp 6`/`ja` split sends
    /// anything above 7 to the default exit (`0x1425206a2`), and a jump table
    /// at `0x1425206a8` dispatches types 1-7 to their blocks. Type 6 shares
    /// the resident-append tail with type 1 (jump-table entry `0x1425205d3`),
    /// without the found/copy/clear prefix.
    ///
    /// - 1: found, chosen (all 16 bytes, `movups` at `0x1425205c5`), clear
    ///   the intervals, append the interval (resident);
    /// - 2: found, chosen (all 16 bytes), clear the intervals;
    /// - 3: clear found, the chosen location and size, and the intervals;
    ///   the trailing dword is *not* written and is preserved;
    /// - 4: (after the log call the fixture's disabled logger returns from)
    ///   clear the intervals, append the *current* chosen entry (resident);
    /// - 5: (after the same log call) clear the intervals;
    /// - 6: append the interval (resident);
    /// - 7: append the interval (non-resident; normalizes immediately).
    ///
    /// Any other type is ignored. Type 4 does not set the found flag: a
    /// type-4 record with no earlier chosen entry appends the zero entry and
    /// leaves the state unfound, exactly as the client's sink does.
    pub fn sink(&mut self, kind: u8, entry: ChosenEntry) {
        match kind {
            1 => {
                self.found = true;
                self.chosen = entry;
                self.intervals.clear();
                self.intervals.append(entry.location, entry.size, true);
            }
            2 => {
                self.found = true;
                self.chosen = entry;
                self.intervals.clear();
            }
            3 => {
                self.found = false;
                // `mov qword [rbx+0x18], 0` and `mov [rbx+0x20], 0` only: the
                // trailing dword at +0x24 is left in place.
                self.chosen.location = 0;
                self.chosen.size = 0;
                self.intervals.clear();
            }
            4 => {
                self.intervals.clear();
                self.intervals
                    .append(self.chosen.location, self.chosen.size, true);
            }
            5 => self.intervals.clear(),
            6 => self.intervals.append(entry.location, entry.size, true),
            7 => self.intervals.append(entry.location, entry.size, false),
            _ => {}
        }
    }

    /// `DynamicIndexResolveRange`'s body (`0x1425115d0`) after the lookup.
    ///
    /// The bounds are against the chosen entry's stored size:
    /// `start > size` is status 4; `len == 0xffffffff` becomes
    /// `size - start`; and `start + len > size` is status 4. The output range
    /// is the absolute `chosen.location + start` with the (possibly replaced)
    /// length and is written before the resident check. A non-zero length is
    /// then checked with [`KeyState::is_resident`]; a failure is status 3 and
    /// leaves the entry unwritten. Success writes the chosen entry. A zero
    /// length skips the resident check and succeeds.
    pub fn resolve(&self, start: u64, len: u32) -> ResolveResult {
        if !self.found {
            return ResolveResult::NoEntry;
        }
        let size = u64::from(self.chosen.size);
        if start > size {
            return ResolveResult::PastEnd;
        }
        let len = if len == u32::MAX {
            self.chosen.size.wrapping_sub(start as u32)
        } else {
            len
        };
        if start + u64::from(len) > size {
            return ResolveResult::PastEnd;
        }
        let range = ResolvedRange {
            start: self.chosen.location.wrapping_add(start),
            len,
        };
        if len != 0 && !self.is_resident(range.start, range.len) {
            return ResolveResult::NotResident { range };
        }
        ResolveResult::Found {
            range,
            entry: self.chosen,
        }
    }
}

/// The 8-byte location plus 4-byte length `DynamicIndexResolveRange` writes to
/// its `out_range` argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedRange {
    /// Absolute packed `archive << 30 | offset`.
    pub start: u64,
    pub len: u32,
}

/// The client's resolve status word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveStatus {
    /// 0: the range and entry were written.
    Found,
    /// 2: `DynamicIndexFindEntry` did not set the found flag.
    NoEntry,
    /// 3: the range is inside the entry but not inside resident coverage.
    NotResident,
    /// 4: the request runs past the chosen entry's stored size.
    PastEnd,
}

impl ResolveStatus {
    pub fn code(self) -> u8 {
        match self {
            ResolveStatus::Found => 0,
            ResolveStatus::NoEntry => 2,
            ResolveStatus::NotResident => 3,
            ResolveStatus::PastEnd => 4,
        }
    }
}

/// What one `DynamicIndexResolveRange` call writes and returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResolveOutcome {
    /// The client's status byte (0 found, 2 no entry, 3 not resident, 4 past
    /// the chosen entry's end).
    pub status: u8,
    /// The 12 bytes the client stores through `out_range` (a u64 start and a
    /// u32 length). Present for status 0 and 3: it is written before the
    /// resident check.
    pub range: Option<ResolvedRange>,
    /// The 16 bytes the client stores through `out_entry`. Present only for
    /// status 0.
    pub entry: Option<ChosenEntry>,
}

/// `DynamicIndexResolveRange` (`0x1425115d0`): the whole call, from the fresh
/// key state to the free.
///
/// Measured extent `0x1425115d0..0x1425116f2` (291 bytes through the `ret` at
/// `0x1425116f1`), the declared `coverageWindow`. Signature
/// `(segment, key, request, out_range, out_entry) -> status` (RCX segment, RDX
/// key, R8 request, R9 out range, stack out entry). The client:
///
/// 1. builds a key state on its own stack with `DynamicIndexFindContextInit`
///    (`0x142520520`) from the segment's `+0` and `+8` words (the interval
///    allocator and logger, read only by the grow and log paths);
/// 2. calls `DynamicIndexFindEntry` (`0x1425111a0`) with both out words null
///    (`xor r9d, r9d` at `0x1425111621` and the zero store at
///    `0x1425111624`), which fills the found byte at `+0x10`, the chosen entry
///    at `+0x18` and the interval buffer;
/// 3. `cmp byte ptr [rsp+0x50], sil` / `jne` (`0x1425111634`): a zero found
///    byte returns 2;
/// 4. `cmp rcx, r8` / `ja` (`0x142511165f`): `request.start > chosen.size`
///    (the u32 at chosen `+8`, zero-extended) returns 4;
/// 5. `cmp edx, -1` / `jne` (`0x1425111668`): a length of `0xffffffff` becomes
///    `size - start` in 32-bit arithmetic;
/// 6. `cmp rax, r8` / `ja` (`0x1425111676`): `start + len > size` returns 4;
/// 7. the two out-range stores (`0x1425111680`, `0x1425111687`): the u32 length
///    at `+8` and the absolute `chosen.location + start` at `+0`;
/// 8. `test edx, edx` / `je` (`0x142511168a`): a zero length skips the
///    resident check;
/// 9. `DynamicIndexRangeIsResident` (`0x142520c60`) with its flag set to 1 and
///    the out-range record; a zero answer returns 3 (`0x14251116a0`);
/// 10. `test rbp, rbp` / `je` (`0x14251116ae`): the 16-byte chosen entry is
///     written only when `out_entry` is non-null;
/// 11. `DynamicIndexFindContextFree` (`0x142520570`), which frees the interval
///     buffer only when it left the inline one (the declared grow gap).
///
/// Status 0 is the fall-through value: `xor esi, esi` before the lookup is the
/// only place the return is cleared, so the success path returns the zero it
/// started with. The key state is fresh per call, so this wrapper and
/// production's [`SegmentEntries::resolve_range`] agree: both look the key up
/// and then call [`KeyState::resolve`], where the whole bounds and residency
/// body lives.
pub fn resolve_range(
    entries: &[u8],
    page_table: &[u8],
    page_count: u32,
    key: &[u8; 9],
    start: u64,
    len: u32,
) -> ResolveOutcome {
    let mut state = KeyState::default();
    let _ = find_entry(entries, page_table, page_count, key, &mut state);
    match state.resolve(start, len) {
        ResolveResult::Found { range, entry } => ResolveOutcome {
            status: ResolveStatus::Found.code(),
            range: Some(range),
            entry: Some(entry),
        },
        ResolveResult::NotResident { range } => ResolveOutcome {
            status: ResolveStatus::NotResident.code(),
            range: Some(range),
            entry: None,
        },
        ResolveResult::NoEntry => ResolveOutcome {
            status: ResolveStatus::NoEntry.code(),
            ..ResolveOutcome::default()
        },
        ResolveResult::PastEnd => ResolveOutcome {
            status: ResolveStatus::PastEnd.code(),
            ..ResolveOutcome::default()
        },
    }
}

/// What `DynamicIndexResolveRange` (`0x1425115d0`) returns.
///
/// The range is present for [`ResolveStatus::Found`] and
/// [`ResolveStatus::NotResident`] because the client writes `out_range` before
/// the resident check. The entry is present only on success.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveResult {
    Found {
        range: ResolvedRange,
        entry: ChosenEntry,
    },
    NoEntry,
    NotResident {
        range: ResolvedRange,
    },
    PastEnd,
}

impl ResolveResult {
    pub fn status(self) -> ResolveStatus {
        match self {
            ResolveResult::Found { .. } => ResolveStatus::Found,
            ResolveResult::NoEntry => ResolveStatus::NoEntry,
            ResolveResult::NotResident { .. } => ResolveStatus::NotResident,
            ResolveResult::PastEnd => ResolveStatus::PastEnd,
        }
    }

    pub fn range(self) -> Option<ResolvedRange> {
        match self {
            ResolveResult::Found { range, .. } | ResolveResult::NotResident { range } => {
                Some(range)
            }
            ResolveResult::NoEntry | ResolveResult::PastEnd => None,
        }
    }

    pub fn entry(self) -> Option<ChosenEntry> {
        match self {
            ResolveResult::Found { entry, .. } => Some(entry),
            _ => None,
        }
    }
}

/// The client's key→bucket fold (`DynamicKeyStateLookup`, `0x142501b40`).
///
/// It XORs the nine key bytes into one byte `b`, computes `b ^ (b >> 4)`,
/// adds the caller's low salt byte and keeps the low four bits. The client's
/// third argument is the salt; all three callers of `DynamicKeyStateLookup`
/// pass 0. The additions are 32-bit in the client and cannot overflow a byte
/// before the mask.
pub fn bucket_of(key: &[u8; 9], salt: u8) -> u8 {
    let mut folded = 0u8;
    for byte in key {
        folded ^= *byte;
    }
    (folded ^ (folded >> 4)).wrapping_add(salt) & 0x0f
}

/// A parsed `<bucket:02x><segment:08x>.idx` file name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentName {
    pub bucket: u8,
    pub segment: u32,
    /// The client's `._idx` temporary marker: deleted in a read-write store
    /// and ignored by a read-only one.
    pub temporary: bool,
}

/// Parse a file name with the client's exact rule (`DynamicIndexScanSegments`).
///
/// The name is ten hex digits, a `.`, and `idx` (ASCII case-insensitive).
/// The ten digits are the bucket and segment: the first two are a hex byte
/// below 16 (so the first digit is always `0`), the last eight are the
/// segment. A `_` between the dot and `idx` marks a temporary file. Any other
/// character in the ten-digit prefix, any other length, and any other suffix
/// is rejected. The suffix compare is case-insensitive.
pub fn parse_segment_file_name(name: &str) -> Option<SegmentName> {
    let bytes = name.as_bytes();
    if bytes.len() < 14 {
        return None;
    }
    let mut digits = [0u8; 10];
    for (slot, byte) in digits.iter_mut().zip(&bytes[..10]) {
        *slot = hex_nibble(*byte)?;
    }
    let bucket = (digits[0] << 4) | digits[1];
    if usize::from(bucket) >= BUCKET_COUNT {
        return None;
    }
    let mut segment = 0u32;
    for digit in &digits[2..] {
        segment = (segment << 4) | u32::from(*digit);
    }
    if bytes[10] != b'.' {
        return None;
    }
    let (temporary, suffix_at) = if bytes[11] == b'_' {
        (true, 12)
    } else {
        (false, 11)
    };
    if bytes.len() != suffix_at + 3 || !bytes[suffix_at..].eq_ignore_ascii_case(b"idx") {
        return None;
    }
    Some(SegmentName {
        bucket,
        segment,
        temporary,
    })
}

/// The name `DynamicIndexSegmentOpen` (`0x14250e400`) builds for a bucket and
/// segment number: `0` + one lowercase hex digit + eight lowercase hex digits
/// + `.idx`. The install's `0c0000014b.idx` pins the lowercase form.
pub fn segment_name(bucket: u8, segment: u32) -> String {
    debug_assert!(usize::from(bucket) < BUCKET_COUNT);
    format!("{bucket:02x}{segment:08x}.idx")
}

/// The name `DynamicContainerOpenArchive` (`0x14250c8f0`) builds for an
/// archive ordinal: `data.%03d`.
pub fn archive_name(archive: u32) -> String {
    format!("data.{archive:03}")
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Parse one bucket file. `expect_bucket` is verified against the header.
pub fn parse_bucket_file(path: &Path, expect_bucket: Option<u8>) -> Result<BucketFile> {
    let data = fs::read(path)?;

    // `DynamicIndexSegmentParse` (`0x14250fcf0`) checks in this order: the
    // 8-byte section header fits; the section body fits; the section hash
    // matches; the section is at least 0x10 bytes; the bucket matches; then
    // the version and widths. The section size is read as a u32 and every
    // offset is formed in 64 bits, so a huge declared size cannot wrap.
    if data.len() < SECTION_HEADER {
        return Err(Error::Malformed(
            path.to_path_buf(),
            "file shorter than its section header",
        ));
    }
    let section_size = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    let header_end = SECTION_HEADER + section_size as usize;
    if data.len() < header_end {
        return Err(Error::Malformed(
            path.to_path_buf(),
            "section body runs past the end of the file",
        ));
    }

    // The section's second word is a 32-bit hash of the section body, and the
    // entry block's second word is a rolling hash over the 18-byte entries.
    // The client checks both before using the segment (`0x14250fcf0`); this is
    // the same hash, verified against all sixteen files of the reference
    // install. Rejecting a mismatch is what keeps a corrupt index from
    // resolving to arbitrary bytes.
    let mut state = [0u32; 2];
    hash_index(&data[8..header_end], &mut state);
    let stored = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    if state[0] != stored {
        return Err(Error::Malformed(
            path.to_path_buf(),
            "file header hash mismatch",
        ));
    }

    // The client returns status 5 for a short section or a bad version/widths
    // and status 9 for the structural and hash failures. `ow-casc` keeps one
    // `Malformed` error because callers branch only on success; the mapping
    // is recorded in `docs/formats/casc.md`.
    if section_size < 0x10 {
        return Err(Error::Malformed(
            path.to_path_buf(),
            "section header is smaller than 16 bytes",
        ));
    }

    let bucket = data[10];
    if let Some(expected) = expect_bucket {
        if bucket != expected {
            return Err(Error::Malformed(
                path.to_path_buf(),
                "bucket number disagrees with filename",
            ));
        }
    }

    let version = u16::from_le_bytes([data[8], data[9]]);
    let span_size = data[12];
    let span_offs = data[13];
    let ekey_bytes = data[14];

    // The client's parser (`0x14250fcf0`) accepts any version >= 7 and
    // checks only the three field widths it hardcodes: 4-byte size, 5-byte
    // location, 9-byte key. It never looks at the extra-bytes or offset-bits
    // bytes, so neither do we; a v8 file with the same widths reads the same.
    if version < 7 || span_size != 4 || span_offs != 5 || ekey_bytes != 9 {
        return Err(Error::Malformed(
            path.to_path_buf(),
            "unsupported index header",
        ));
    }

    // The section header's byte count positions the entry block. The client
    // rounds `section_size + 0x17` down to a 16-byte boundary; the entry
    // block's own 8-byte BlockSizeAndHash follows, then the entries. The usual
    // file has section_size 16 -> entry header at 32 -> entries at 40. The
    // client also stores the u64 at header + 0x10 as the maximum addressable
    // size; the parse never checks it, and the index lookup/resolve paths do
    // not read it.
    let entry_header = (section_size as usize + 0x17) & !0xf;
    if entry_header >= data.len() {
        return Err(Error::Malformed(
            path.to_path_buf(),
            "entry block header starts past the end of the file",
        ));
    }
    let remaining = data.len() - entry_header;
    if remaining < SECTION_HEADER {
        return Err(Error::Malformed(
            path.to_path_buf(),
            "entry block header runs past the end of the file",
        ));
    }
    let block_size = u32::from_le_bytes([
        data[entry_header],
        data[entry_header + 1],
        data[entry_header + 2],
        data[entry_header + 3],
    ]);
    if u64::from(block_size) + SECTION_HEADER as u64 > remaining as u64 {
        return Err(Error::Malformed(
            path.to_path_buf(),
            "entry block runs past the end of the file",
        ));
    }

    // The client divides the declared block size by 18 and ignores any
    // remainder: `count = block_size / 0x12`, with no divisibility check. A
    // trailing partial entry is never hashed and never decoded.
    let count = block_size as usize / ENTRY_SIZE;
    let entries_offset = entry_header + SECTION_HEADER;
    let end = entries_offset + count * ENTRY_SIZE;
    if data.len() < end {
        return Err(Error::Malformed(
            path.to_path_buf(),
            "entries run past the end of the file",
        ));
    }

    let mut state = [0u32; 2];
    for entry in data[entries_offset..end].chunks_exact(ENTRY_SIZE) {
        hash_index(entry, &mut state);
    }
    let stored = u32::from_le_bytes([
        data[entry_header + 4],
        data[entry_header + 5],
        data[entry_header + 6],
        data[entry_header + 7],
    ]);
    if state[0] != stored {
        return Err(Error::Malformed(
            path.to_path_buf(),
            "entry block hash mismatch",
        ));
    }

    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let e = &data[entries_offset + i * ENTRY_SIZE..entries_offset + (i + 1) * ENTRY_SIZE];
        let mut ekey = [0u8; 9];
        ekey.copy_from_slice(&e[0..9]);

        let mut packed: u64 = 0;
        for b in &e[9..14] {
            packed = (packed << 8) | u64::from(*b);
        }
        let size = u32::from_le_bytes([e[14], e[15], e[16], e[17]]);

        out.push((
            ekey,
            Location {
                archive: (packed >> 30) as u32,
                offset: packed & ((1 << 30) - 1),
                size,
            },
        ));
    }
    // The page table starts at the first 0x1000 boundary after the entry
    // block plus 0x1007, exactly as `DynamicIndexSegmentParse` computes it
    // from the entry block size and the entry header offset. The client
    // requires at least 0x7800 bytes (60 pages) and derives the page count by
    // shifting the remainder right by 9.
    let page_table_offset = (block_size as usize + entry_header + 0x1007) & !0xfff;
    if page_table_offset >= data.len() {
        return Err(Error::Malformed(
            path.to_path_buf(),
            "page table starts past the end of the file",
        ));
    }
    let remaining = data.len() - page_table_offset;
    if remaining < PAGE_TABLE_MIN_BYTES {
        return Err(Error::Malformed(
            path.to_path_buf(),
            "page table is shorter than 60 pages",
        ));
    }
    let page_count = remaining / PAGE_SIZE;
    let pages = PageTable::parse(&data, page_table_offset, page_count)
        .map_err(|message| Error::Malformed(path.to_path_buf(), message))?;
    Ok(BucketFile {
        entries: out,
        pages,
    })
}

/// The client's dynamic-index hash, exactly (`DynamicIndexHash`,
/// `0x142512c60`).
///
/// It is a two-word 32-bit state hash over 12-byte blocks. The same function
/// hashes the file header (state starts at zero) and each 18-byte entry in
/// turn (the state carries between entries); `state[0]` is compared with the
/// stored hash. Verified against all sixteen `.idx` files of the reference
/// install: the file-header hash and the complete entry-block rolling hash
/// both match. Public so the verification probe calls this implementation
/// rather than a copy.
pub fn hash_index(data: &[u8], state: &mut [u32; 2]) {
    let mut a = (data.len() as u32)
        .wrapping_add(0xdead_beef)
        .wrapping_add(state[0]);
    let mut b = state[1].wrapping_add(a);
    let mut c = a;

    // A zero-length segment is the client's switch case 0: it stores the two
    // running words and returns before the final mix. The parsed index never
    // hashes an empty slice, but the function's contract includes it.
    if data.is_empty() {
        state[0] = b;
        state[1] = a;
        return;
    }

    let mut pos = 0usize;
    // The client's loop runs floor((len - 1) / 12) blocks: the last 1..=12
    // bytes are the switch tail, including a whole twelve-byte group when the
    // length is a multiple of 12. The group is added to the running words, not
    // mixed as a block.
    while pos + 12 < data.len() {
        let w0 = u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
        let w1 = u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]);
        let w2 = u32::from_le_bytes([data[pos + 8], data[pos + 9], data[pos + 10], data[pos + 11]]);
        b = b.wrapping_add(w2);
        c = b.rotate_left(4) ^ w0.wrapping_sub(b).wrapping_add(c);
        let i5 = a.wrapping_add(w1).wrapping_add(b);
        let i12 = i5.wrapping_add(c);
        c = c.rotate_left(6) ^ a.wrapping_add(w1).wrapping_sub(c);
        a = c.rotate_left(8) ^ i5.wrapping_sub(c);
        let i6 = i12.wrapping_add(c);
        c = a.rotate_left(16) ^ i12.wrapping_sub(a);
        let i5 = i6.wrapping_add(a);
        a = c.rotate_left(19) ^ i6.wrapping_sub(c);
        c = c.wrapping_add(i5);
        b = a.rotate_left(4) ^ i5.wrapping_sub(a);
        a = a.wrapping_add(c);
        pos += 12;
    }

    let tail = &data[pos..];
    match tail.len() {
        0 => unreachable!("a non-empty input leaves a 1..=12 byte tail"),
        1 => c = c.wrapping_add(u32::from(tail[0])),
        2 => c = c.wrapping_add(le(tail, 0, 2)),
        3 => c = c.wrapping_add(le(tail, 0, 3)),
        4 => c = c.wrapping_add(le(tail, 0, 4)),
        5 => {
            a = a.wrapping_add(u32::from(tail[4]));
            c = c.wrapping_add(le(tail, 0, 4));
        }
        6 => {
            a = a.wrapping_add(le(tail, 4, 2));
            c = c.wrapping_add(le(tail, 0, 4));
        }
        7 => {
            a = a.wrapping_add(le(tail, 4, 3));
            c = c.wrapping_add(le(tail, 0, 4));
        }
        8 => {
            a = a.wrapping_add(le(tail, 4, 4));
            c = c.wrapping_add(le(tail, 0, 4));
        }
        9 => {
            a = a.wrapping_add(le(tail, 4, 4));
            b = b.wrapping_add(u32::from(tail[8]));
            c = c.wrapping_add(le(tail, 0, 4));
        }
        10 => {
            a = a.wrapping_add(le(tail, 4, 4));
            b = b.wrapping_add(le(tail, 8, 2));
            c = c.wrapping_add(le(tail, 0, 4));
        }
        11 => {
            a = a.wrapping_add(le(tail, 4, 4));
            b = b.wrapping_add(le(tail, 8, 3));
            c = c.wrapping_add(le(tail, 0, 4));
        }
        12 => {
            a = a.wrapping_add(le(tail, 4, 4));
            b = b.wrapping_add(le(tail, 8, 4));
            c = c.wrapping_add(le(tail, 0, 4));
        }
        _ => unreachable!("the block loop leaves at most twelve bytes"),
    }

    b = (b ^ a).wrapping_sub(a.rotate_left(14));
    c = (c ^ b).wrapping_sub(b.rotate_left(11));
    a = (a ^ c).wrapping_sub(c.rotate_left(25));
    b = (b ^ a).wrapping_sub(a.rotate_left(16));
    c = (c ^ b).wrapping_sub(b.rotate_left(4));
    c = (c ^ a).wrapping_sub(c.rotate_left(14));
    state[0] = (b ^ c).wrapping_sub(c.rotate_left(24));
    state[1] = c;
}

fn le(data: &[u8], at: usize, len: usize) -> u32 {
    let mut value = 0u32;
    for i in 0..len {
        value |= u32::from(data[at + i]) << (8 * i);
    }
    value
}

/// The client's per-record dynamic-index key hash, exactly
/// (`DynamicIndexKeyHash`, `0x142513230`).
///
/// It is the lookup3 `hashlittle` word hash: the three running words start at
/// `0xdeadbeef + len + init` (all 32-bit wrapping adds), twelve-byte groups are
/// mixed, and the 1..=12 byte tail is added before the final mix. The returned
/// word is the final `c`. A zero-length key is the client's `case 0`: it
/// returns `c` before the final mix.
///
/// `DynamicIndexSegmentParse` (`0x14250fcf0`) calls it once per 24-byte page
/// record as `key_hash(&record[4..23], 0)`, then compares the result ORed with
/// `0x80000000` against the record's first dword. The client selects one of
/// three load paths from the key pointer's low two bits (dword, word and byte
/// loads); all three assemble the same twelve bytes, so this byte-wise
/// implementation is the function's return contract on every alignment.
///
/// A page record is 4-byte stored hash, 19-byte key, one pad byte. The install's
/// page tables are zero-filled, so this function has no stored record hash to
/// agree with; the unit tests pin the client's own outputs instead.
pub fn key_hash(key: &[u8], init: u32) -> u32 {
    use std::num::Wrapping;

    let seed = Wrapping(key.len() as u32) + Wrapping(0xdead_beef) + Wrapping(init);
    let mut a = seed;
    let mut b = seed;
    let mut c = seed;

    let mut pos = 0usize;
    while key.len() - pos > 12 {
        let w0 = Wrapping(le(key, pos, 4));
        let w1 = Wrapping(le(key, pos + 4, 4));
        let w2 = Wrapping(le(key, pos + 8, 4));
        a += w0;
        b += w1;
        c += w2;
        a = (a - c) ^ Wrapping(c.0.rotate_left(4));
        c += b;
        b = (b - a) ^ Wrapping(a.0.rotate_left(6));
        a += c;
        c = (c - b) ^ Wrapping(b.0.rotate_left(8));
        b += a;
        a = (a - c) ^ Wrapping(c.0.rotate_left(16));
        c += b;
        b = (b - a) ^ Wrapping(a.0.rotate_left(19));
        a += c;
        c = (c - b) ^ Wrapping(b.0.rotate_left(4));
        b += a;
        pos += 12;
    }

    let tail = &key[pos..];
    match tail.len() {
        0 => return c.0,
        1 => a += Wrapping(u32::from(tail[0])),
        2 => a += Wrapping(le(tail, 0, 2)),
        3 => a += Wrapping(le(tail, 0, 3)),
        4 => a += Wrapping(le(tail, 0, 4)),
        5 => {
            a += Wrapping(le(tail, 0, 4));
            b += Wrapping(u32::from(tail[4]));
        }
        6 => {
            a += Wrapping(le(tail, 0, 4));
            b += Wrapping(le(tail, 4, 2));
        }
        7 => {
            a += Wrapping(le(tail, 0, 4));
            b += Wrapping(le(tail, 4, 3));
        }
        8 => {
            a += Wrapping(le(tail, 0, 4));
            b += Wrapping(le(tail, 4, 4));
        }
        9 => {
            a += Wrapping(le(tail, 0, 4));
            b += Wrapping(le(tail, 4, 4));
            c += Wrapping(u32::from(tail[8]));
        }
        10 => {
            a += Wrapping(le(tail, 0, 4));
            b += Wrapping(le(tail, 4, 4));
            c += Wrapping(le(tail, 8, 2));
        }
        11 => {
            a += Wrapping(le(tail, 0, 4));
            b += Wrapping(le(tail, 4, 4));
            c += Wrapping(le(tail, 8, 3));
        }
        12 => {
            a += Wrapping(le(tail, 0, 4));
            b += Wrapping(le(tail, 4, 4));
            c += Wrapping(le(tail, 8, 4));
        }
        _ => unreachable!("the block loop leaves at most twelve bytes"),
    }

    c = (c ^ b) - Wrapping(b.0.rotate_left(14));
    a = (a ^ c) - Wrapping(c.0.rotate_left(11));
    b = (b ^ a) - Wrapping(a.0.rotate_left(25));
    c = (c ^ b) - Wrapping(b.0.rotate_left(16));
    a = (a ^ c) - Wrapping(c.0.rotate_left(4));
    b = (b ^ a) - Wrapping(a.0.rotate_left(14));
    c = (c ^ b) - Wrapping(b.0.rotate_left(24));
    c.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bucket_file(version: u16, bucket: u8, entries: &[([u8; 9], u64, u32)]) -> Vec<u8> {
        let mut out = vec![0u8; ENTRIES_OFFSET];
        out[0..4].copy_from_slice(&16u32.to_le_bytes());
        out[8..10].copy_from_slice(&version.to_le_bytes());
        out[10] = bucket;
        out[12] = 4;
        out[13] = 5;
        out[14] = 9;
        out[15] = 30;
        out[32..36].copy_from_slice(&((entries.len() * ENTRY_SIZE) as i32).to_le_bytes());
        for (key, packed, size) in entries {
            out.extend_from_slice(key);
            out.extend_from_slice(&packed.to_be_bytes()[3..8]);
            out.extend_from_slice(&size.to_le_bytes());
        }
        let mut state = [0u32; 2];
        for entry in out[ENTRIES_OFFSET..].chunks_exact(ENTRY_SIZE) {
            hash_index(entry, &mut state);
        }
        out[36..40].copy_from_slice(&state[0].to_le_bytes());
        let mut state = [0u32; 2];
        hash_index(&out[8..24], &mut state);
        out[4..8].copy_from_slice(&state[0].to_le_bytes());
        paste_page_table(&mut out, entries.len() * ENTRY_SIZE, &[]);
        out
    }

    fn page_table_offset(entry_block_size: usize) -> usize {
        (entry_block_size + 32 + 0x1007) & !0xfff
    }

    /// Resize a synthetic file to the client's 60-page minimum and write page
    /// records at `(page, record)`. The rest of the page table stays zero.
    fn paste_page_table(
        out: &mut Vec<u8>,
        entry_block_size: usize,
        pages: &[(usize, usize, [u8; 9], u64, u32, u8)],
    ) {
        let off = page_table_offset(entry_block_size);
        out.resize(off + PAGE_TABLE_MIN_BYTES, 0);
        for (page, record, key, packed, size, kind) in pages {
            let at = off + page * PAGE_SIZE + record * PAGE_RECORD_SIZE;
            out[at + 4..at + 13].copy_from_slice(key);
            out[at + 13..at + 18].copy_from_slice(&packed.to_be_bytes()[3..8]);
            out[at + 18..at + 22].copy_from_slice(&size.to_le_bytes());
            out[at + 22] = *kind;
            let stored = key_hash(&out[at + 4..at + 23], 0) | 0x8000_0000;
            out[at..at + 4].copy_from_slice(&stored.to_le_bytes());
        }
    }

    fn bucket_file_with_pages(
        version: u16,
        bucket: u8,
        entries: &[([u8; 9], u64, u32)],
        pages: &[(usize, usize, [u8; 9], u64, u32, u8)],
    ) -> Vec<u8> {
        let mut out = bucket_file(version, bucket, entries);
        // `bucket_file` already padded with zero pages; overwrite the records
        // requested by the fixture and recompute each stored hash.
        paste_page_table(&mut out, entries.len() * ENTRY_SIZE, pages);
        out
    }

    fn write_temp(name: &str, bytes: &[u8]) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("ow-casc-idx-{}-{name}.idx", std::process::id()));
        fs::write(&path, bytes).expect("write synthetic bucket");
        path
    }

    /// Known answers from the client/real files. The 16-byte header is the
    /// real `0300000159.idx` file header and `0xc52f303d` is the hash the
    /// client stored at file offset 4. The two entry states are the rolling
    /// values after the first and second real entries of that file.
    #[test]
    fn matches_the_client_hash_on_known_vectors() {
        fn bytes(hex: &str) -> Vec<u8> {
            hex.as_bytes()
                .chunks_exact(2)
                .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
                .collect()
        }
        let mut state = [0u32; 2];
        hash_index(&bytes("070003000405091e000000c0ff000000"), &mut state);
        assert_eq!(state[0], 0xc52f_303d);

        let mut state = [0u32; 2];
        hash_index(&bytes("00001d9bbc07919efd0fd56f31a2b4220000"), &mut state);
        assert_eq!(state, [0xef6c_3f29, 0xd114_251e]);
        hash_index(&bytes("00003400342943950309a1666c77513a0000"), &mut state);
        assert_eq!(state, [0x758e_09f9, 0x9c58_4663]);
    }

    /// A zero-length segment is the client's switch case 0: it stores the two
    /// running words and returns before the final mix. `parse_bucket_file`
    /// never hashes an empty slice, but the function's contract includes it;
    /// the values are the client's own output for the same inputs.
    #[test]
    fn a_zero_length_hash_returns_the_running_words_unmixed() {
        let mut state = [0u32; 2];
        hash_index(&[], &mut state);
        assert_eq!(state, [0xdead_beef, 0xdead_beef]);

        let mut state = [0xdead_beef, 0x1234_5678];
        hash_index(&[], &mut state);
        assert_eq!(state, [0xcf8f_d456, 0xbd5b_7dde]);
    }

    /// `DynamicIndexKeyHash` (`0x142513230`) known answers, read from the
    /// client's bytes: the empty early return (`case 0` returns `c`), every
    /// tail length from 1 to 12, the 12-byte block loop, and the real 19-byte
    /// page-key shape. The install's page tables are zero-filled, so there is
    /// no stored page-record hash; these are the client's outputs for the same
    /// inputs. `00..18` is also a real 19-byte span of `0300000159.idx`'s first
    /// entry block (the page tables themselves are all zero). The client reads
    /// `init` through `R8D`, so `key_hash` takes a `u32`; the fixture exercises
    /// the oracle's truncation of a 64-bit init word.
    #[test]
    fn matches_the_client_key_hash_on_known_vectors() {
        let seq = |n: usize| -> Vec<u8> { (0..n).map(|i| i as u8).collect() };
        let cases: &[(Vec<u8>, u32, u32)] = &[
            (Vec::new(), 0, 0xdead_beef),
            (Vec::new(), 1, 0xdead_bef0),
            (Vec::new(), 0xffff_ffff, 0xdead_beee),
            (vec![0u8; 19], 0, 0x9100_686e),
            (vec![0u8; 19], 0xffff_ffff, 0x3428_e3f5),
            (seq(19), 0, 0x74ab_ad21),
            (seq(19), 1, 0x6d33_d311),
            (seq(19), 0xffff_ffff, 0x8036_c457),
            (b"hello, world!".to_vec(), 0x1234_5678, 0xb416_2ac5),
            (seq(24), 0, 0x9c0a_dd53),
            (seq(25), 0, 0x3a88_2244),
            (seq(8), 0, 0xd4cc_b93c),
            (seq(9), 0, 0xeca5_1474),
            (seq(10), 0, 0x610f_c1e3),
            (seq(14), 0, 0x9609_e9d3),
            (seq(15), 0, 0x63a6_e503),
            (seq(16), 0, 0x5e84_eafc),
            (seq(17), 0, 0x8c35_32cb),
            (seq(18), 0, 0xd732_003d),
            (seq(23), 0, 0xa5ff_3c58),
            // The first 19 bytes of the real file's entry block.
            (
                hex_bytes("00001d9bbc07919efd0fd56f31a2b422000000"),
                0,
                0x6de3_52b4,
            ),
            (
                hex_bytes("00003400342943950309a1666c77513a000000"),
                0,
                0x1994_5f92,
            ),
        ];
        for (key, init, expected) in cases {
            assert_eq!(
                key_hash(key, *init),
                *expected,
                "key {key:02x?} init {init:#x}"
            );
        }
    }

    /// The Rust slice has no alignment path; the same 19 bytes at four address
    /// offsets must hash identically. The oracle unit runs the client's three
    /// pointer paths (16-bit aligned, even-not-4, odd) and compares each.
    #[test]
    fn key_hash_does_not_depend_on_slice_alignment() {
        let seed = seq_bytes(19);
        let expected = key_hash(&seed, 0);
        for offset in 0..4 {
            let mut buf = [0u8; 32];
            buf[offset..offset + 19].copy_from_slice(&seed);
            assert_eq!(key_hash(&buf[offset..offset + 19], 0), expected);
        }
    }

    fn hex_bytes(hex: &str) -> Vec<u8> {
        hex.as_bytes()
            .chunks_exact(2)
            .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
            .collect()
    }

    fn seq_bytes(n: usize) -> Vec<u8> {
        (0..n).map(|i| i as u8).collect()
    }

    /// The client's block loop runs `floor((len - 1) / 12)`, so a trailing
    /// whole twelve-byte group is added to the running words, not mixed as a
    /// block. The `.idx` files only hash 16- and 18-byte spans, so this shape
    /// has no file vector; these are the client's own outputs for `00..0b`
    /// and `00..17` with the state at zero.
    #[test]
    fn a_whole_twelve_byte_tail_is_added_not_mixed() {
        let twelve: Vec<u8> = (0u8..12).collect();
        let mut state = [0u32; 2];
        hash_index(&twelve, &mut state);
        assert_eq!(state, [0x5e4a_a593, 0x0cb6_e7e9]);

        let twenty_four: Vec<u8> = (0u8..24).collect();
        let mut state = [0u32; 2];
        hash_index(&twenty_four, &mut state);
        assert_eq!(state, [0x9c0a_dd53, 0x9f41_1002]);
    }

    #[test]
    fn reads_entries_and_both_endiannesses() {
        let key = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99];
        let archive = 5u64;
        let offset = 0x1234_5678u64;
        let packed = (archive << 30) | offset;
        let path = write_temp("entries", &bucket_file(7, 3, &[(key, packed, 0x1122_3344)]));
        let file = parse_bucket_file(&path, Some(3)).expect("parses");
        let entries = &file.entries;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, key);
        assert_eq!(
            entries[0].1,
            Location {
                archive: 5,
                offset: 0x1234_5678,
                size: 0x1122_3344,
            }
        );
        fs::remove_file(&path).ok();
    }

    /// The client accepts version >= 7; it only hardcodes the 4/5/9 widths.
    #[test]
    fn accepts_a_newer_version_with_the_same_widths() {
        let key = [7u8; 9];
        let path = write_temp("v8", &bucket_file(8, 0, &[(key, 1 << 30, 7)]));
        let file = parse_bucket_file(&path, Some(0)).expect("v8 parses");
        assert_eq!(file.entries[0].1.archive, 1);
        assert_eq!(file.entries[0].1.offset, 0);
        fs::remove_file(&path).ok();
    }

    /// The entry block is positioned by the section header's byte count, not
    /// by a fixed 0x28. The client aligns `sectionSize + 0x17` to 16 bytes;
    /// this file declares a 32-byte section and must still read.
    #[test]
    fn finds_the_entry_block_from_the_section_size() {
        let key = [0x42u8; 9];
        let section = 32usize;
        let entry_header = (section + 0x17) & !0xf;
        let mut data = vec![0u8; entry_header + 8 + ENTRY_SIZE];
        data[0..4].copy_from_slice(&(section as u32).to_le_bytes());
        data[8..10].copy_from_slice(&7u16.to_le_bytes());
        data[10] = 2;
        data[12] = 4;
        data[13] = 5;
        data[14] = 9;
        data[entry_header..entry_header + 4].copy_from_slice(&(ENTRY_SIZE as i32).to_le_bytes());
        let at = entry_header + 8;
        data[at..at + 9].copy_from_slice(&key);
        data[at + 9..at + 14].copy_from_slice(&(1u64 << 30).to_be_bytes()[3..8]);
        data[at + 14..at + 18].copy_from_slice(&99u32.to_le_bytes());
        let mut state = [0u32; 2];
        hash_index(&data[at..at + ENTRY_SIZE], &mut state);
        data[entry_header + 4..entry_header + 8].copy_from_slice(&state[0].to_le_bytes());
        let mut state = [0u32; 2];
        hash_index(&data[8..8 + section], &mut state);
        data[4..8].copy_from_slice(&state[0].to_le_bytes());
        let page_table_offset = (ENTRY_SIZE + entry_header + 0x1007) & !0xfff;
        data.resize(page_table_offset + PAGE_TABLE_MIN_BYTES, 0);

        let path = write_temp("section", &data);
        let file = parse_bucket_file(&path, Some(2)).expect("parses");
        assert_eq!(file.entries[0].0, key);
        assert_eq!(file.entries[0].1.archive, 1);
        assert_eq!(file.entries[0].1.size, 99);
        fs::remove_file(&path).ok();
    }

    /// The client floors `block_size / 0x12`; it does not require the declared
    /// entry block to be a multiple of 18. The five trailing bytes here are
    /// part of the block but are neither hashed nor decoded.
    #[test]
    fn accepts_an_entry_block_with_a_partial_trailing_entry() {
        let key = [0x2au8; 9];
        let mut file = bucket_file(7, 3, &[(key, 1 << 30, 41)]);
        file[32..36].copy_from_slice(&23u32.to_le_bytes());
        file[ENTRIES_OFFSET + ENTRY_SIZE..ENTRIES_OFFSET + 23].fill(0xa5);
        // `(23 + 32 + 0x1007) & ~0xfff` is still 0x1000, so the fixture's page
        // table stays at the offset the parser computes.
        let path = write_temp("block-tail", &file);
        let parsed = parse_bucket_file(&path, Some(3)).expect("floored entry count");
        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.entries[0].0, key);
        assert_eq!(parsed.entries[0].1.size, 41);
        fs::remove_file(&path).ok();
    }

    /// The first structural check is the 8-byte section header; a shorter
    /// file is malformed before any field is read.
    #[test]
    fn rejects_a_file_shorter_than_the_section_header() {
        let path = write_temp("short", &[0u8; 4]);
        assert!(parse_bucket_file(&path, None).is_err());
        fs::remove_file(&path).ok();
    }

    /// The section hash covers the declared section body and is checked before
    /// the version and widths, so a changed version byte is a hash failure.
    #[test]
    fn rejects_a_section_hash_mismatch() {
        let key = [0x2bu8; 9];
        let mut file = bucket_file(7, 3, &[(key, 1 << 30, 41)]);
        file[8] ^= 0xff;
        let path = write_temp("section-hash", &file);
        assert!(parse_bucket_file(&path, Some(3)).is_err());
        fs::remove_file(&path).ok();
    }

    /// The entry block's rolling hash covers exactly the floored entry count;
    /// a changed entry byte is rejected before any entry is decoded.
    #[test]
    fn rejects_an_entry_block_hash_mismatch() {
        let key = [0x2cu8; 9];
        let mut file = bucket_file(7, 3, &[(key, 1 << 30, 41)]);
        file[ENTRIES_OFFSET] ^= 0xff;
        let path = write_temp("entry-hash", &file);
        assert!(parse_bucket_file(&path, Some(3)).is_err());
        fs::remove_file(&path).ok();
    }

    /// The client requires `block_size + 8 <= remaining`; one byte more is
    /// rejected even though the section and entry hashes are still valid.
    #[test]
    fn rejects_an_entry_block_that_runs_past_the_file() {
        let key = [0x2du8; 9];
        let mut file = bucket_file(7, 3, &[(key, 1 << 30, 41)]);
        let block_size = (file.len() - ENTRIES_OFFSET + 1) as u32;
        file[32..36].copy_from_slice(&block_size.to_le_bytes());
        let path = write_temp("block-bound", &file);
        assert!(parse_bucket_file(&path, Some(3)).is_err());
        fs::remove_file(&path).ok();
    }

    /// `page_count = remaining >> 9` floors; a partial trailing page is
    /// ignored and does not add a page to the table.
    #[test]
    fn page_count_floors_a_partial_trailing_page() {
        let key = [0x2eu8; 9];
        let mut file = bucket_file(7, 3, &[(key, 1 << 30, 41)]);
        file.extend_from_slice(&[0u8; 0x123]);
        let path = write_temp("partial-page", &file);
        let parsed = parse_bucket_file(&path, Some(3)).expect("partial page is ignored");
        assert_eq!(parsed.pages.page_count(), 60);
        fs::remove_file(&path).ok();
    }

    #[test]
    fn rejects_an_older_version_and_a_foreign_bucket() {
        let key = [7u8; 9];
        let path = write_temp("v6", &bucket_file(6, 0, &[(key, 0, 7)]));
        assert!(parse_bucket_file(&path, Some(0)).is_err());
        fs::remove_file(&path).ok();

        let path = write_temp("bucket", &bucket_file(7, 1, &[(key, 0, 7)]));
        assert!(parse_bucket_file(&path, Some(2)).is_err());
        fs::remove_file(&path).ok();
    }

    /// The client's fold (`0x142501b40`): XOR the nine bytes, fold the high
    /// nibble, add the salt, mask. `all 0xff` and the real `0300000159.idx`
    /// key are both pinned; salt 1 moves every bucket by one.
    #[test]
    fn bucket_of_is_the_client_fold() {
        assert_eq!(bucket_of(&[0u8; 9], 0), 0);
        // Nine 0xff bytes XOR to 0xff; 0xff ^ 0x0f = 0xf0.
        assert_eq!(bucket_of(&[0xff; 9], 0), 0);
        assert_eq!(bucket_of(&[0xff; 9], 1), 1);
        assert_eq!(bucket_of(&[0xff; 9], 0x1f), 0x0f);

        // XOR 0x11 ^ 0x22 ^ ... ^ 0x99 = 0x11; 0x11 ^ 0x01 = 0x10.
        let key = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99];
        assert_eq!(bucket_of(&key, 0), 0);
        assert_eq!(bucket_of(&key, 0x0f), 0x0f);
        assert_eq!(bucket_of(&key, 0x10), 0);

        // The first entry of the real 0300000159.idx: XOR 0xcf, fold 0xc3,
        // bucket 3, which is that file's header bucket.
        let real = [0x00, 0x00, 0x1d, 0x9b, 0xbc, 0x07, 0x91, 0x9e, 0xfd];
        assert_eq!(bucket_of(&real, 0), 3);
    }

    /// The client's file-name parser (`DynamicIndexScanSegments`): exactly ten hex
    /// digits, a dot, then `idx`, optionally with the `_` temporary marker.
    #[test]
    fn parses_segment_file_names_like_the_client() {
        assert_eq!(
            parse_segment_file_name("0300000159.idx"),
            Some(SegmentName {
                bucket: 3,
                segment: 0x159,
                temporary: false
            })
        );
        assert_eq!(
            parse_segment_file_name("0c0000014b.idx"),
            Some(SegmentName {
                bucket: 12,
                segment: 0x14b,
                temporary: false
            })
        );
        assert_eq!(
            parse_segment_file_name("0a0000FFFF.IDX"),
            Some(SegmentName {
                bucket: 10,
                segment: 0xffff,
                temporary: false
            })
        );
        assert_eq!(
            parse_segment_file_name("0300000020._idx"),
            Some(SegmentName {
                bucket: 3,
                segment: 0x20,
                temporary: true
            })
        );
        assert_eq!(
            parse_segment_file_name("0030000159.idx"),
            Some(SegmentName {
                bucket: 0,
                segment: 0x3000_0159,
                temporary: false
            })
        );

        for rejected in [
            "data.000",
            "0300000159.idx.bak",
            "030000015.idx",
            "1030000159.idx",
            "x300000159.idx",
            "0300000159.id",
            "0300000159._id",
            "0300000159.zidx",
        ] {
            assert_eq!(
                parse_segment_file_name(rejected),
                None,
                "{rejected} must not parse"
            );
        }

        assert_eq!(segment_name(3, 0x159), "0300000159.idx");
        assert_eq!(segment_name(12, 0x14b), "0c0000014b.idx");
        assert_eq!(segment_name(15, 0), "0f00000000.idx");
        assert_eq!(archive_name(7), "data.007");
        assert_eq!(archive_name(1022), "data.1022");
    }

    /// The payload archive is the packed location plus the 30-byte data
    /// header, shifted by 30. A synthetic entry whose offset is within 29
    /// bytes of the 2^30 boundary carries into the next archive; no install
    /// entry does.
    #[test]
    fn payload_archive_is_the_packed_location_plus_the_header() {
        let location = Location {
            archive: 7,
            offset: 0x1234,
            size: 30,
        };
        assert_eq!(location.packed(), (7 << 30) | 0x1234);
        assert_eq!(location.data_archive(), 7);

        let boundary = Location {
            archive: 7,
            offset: (1 << 30) - 29,
            size: 30,
        };
        assert_eq!(boundary.data_archive(), 8);
    }

    fn key_at(first: u8, last: u8) -> [u8; 9] {
        let mut key = [0u8; 9];
        key[0] = first;
        key[8] = last;
        key
    }

    fn entry(key: [u8; 9], offset: u64, size: u32) -> ([u8; 9], Location) {
        (
            key,
            Location {
                archive: 1,
                offset,
                size,
            },
        )
    }

    fn synthetic_entries() -> SegmentEntries {
        SegmentEntries::new(vec![
            entry(key_at(1, 0), 0x100, 40),
            entry([2u8; 9], 0x200, 50),
            entry([2u8; 9], 0x232, 20),
            entry([0xff; 9], 0x300, 60),
        ])
    }

    /// The client's lower-bound walk and equal-range expansion
    /// (`DynamicIndexFindEntry`, `0x1425111a0`): present keys return the whole
    /// equal group, absent keys return the insertion point with an empty range.
    #[test]
    fn find_range_is_the_client_equal_range() {
        let entries = synthetic_entries();

        assert_eq!(entries.find_range(&[2u8; 9]), 1..3);
        assert_eq!(entries.find(&[2u8; 9]), Some(entries.as_slice()[1].1));
        assert_eq!(entries.find_range(&key_at(1, 0)), 0..1);
        assert_eq!(entries.find_range(&[1u8; 9]), 1..1);
        assert_eq!(entries.find_range(&[0xff; 9]), 3..4);
        assert_eq!(entries.find_range(&[0u8; 9]), 0..0);
        assert_eq!(entries.find_range(&key_at(1, 1)), 1..1);
        assert_eq!(entries.find_range(&key_at(3, 0)), 3..3);
        assert_eq!(entries.find(&key_at(1, 1)), None);

        let empty = SegmentEntries::new(Vec::new());
        assert_eq!(empty.find_range(&[0u8; 9]), 0..0);
        assert_eq!(empty.find(&[0xff; 9]), None);
        assert_eq!(empty.unique_key_count(), 0);

        // The extreme keys: the all-zero key is the first entry, the
        // all-ones key is the last. A key between them is absent.
        let edge = SegmentEntries::new(vec![
            entry([0u8; 9], 0x500, 70),
            entry([0xff; 9], 0x600, 80),
        ]);
        assert_eq!(edge.find_range(&[0u8; 9]), 0..1);
        assert_eq!(edge.find_range(&[0xff; 9]), 1..2);
        assert_eq!(edge.find_range(&key_at(0, 1)), 1..1);

        // The duplicate pair is the header entry first, the payload second.
        let pair =
            SegmentEntries::new(vec![entry([7u8; 9], 0x400, 90), entry([7u8; 9], 0x41e, 60)]);
        assert_eq!(pair.find_range(&[7u8; 9]), 0..2);
        assert_eq!(pair.find(&[7u8; 9]).unwrap().offset, 0x400);
        assert_eq!(pair.find(&[7u8; 9]).unwrap().size, 90);
        assert_eq!(pair.unique_key_count(), 1);
    }

    /// The search must agree with a linear first/last scan on every key,
    /// including duplicates and absent keys. A deterministic LCG supplies a
    /// sorted corpus; the install itself is checked in
    /// `tests/index_find_real.rs`.
    #[test]
    fn find_range_agrees_with_a_linear_scan() {
        let mut state = 0x1234_5678u32;
        let mut keys: Vec<[u8; 9]> = Vec::new();
        for _ in 0..400 {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let mut key = [0u8; 9];
            for byte in &mut key {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                *byte = (state >> 24) as u8;
            }
            keys.push(key);
        }
        keys.sort_unstable();
        let entries = SegmentEntries::new(
            keys.iter()
                .enumerate()
                .map(|(i, key)| entry(*key, i as u64 * 0x100, 30))
                .collect(),
        );

        let mut queries = keys.clone();
        queries.push([0u8; 9]);
        queries.push([0xff; 9]);
        for query in queries {
            let mut first = 0usize;
            while first < keys.len() && keys[first] < query {
                first += 1;
            }
            let mut last = first;
            while last < keys.len() && keys[last] == query {
                last += 1;
            }
            assert_eq!(entries.find_range(&query), first..last, "key {query:02x?}");
        }
    }

    fn page(
        page: usize,
        record: usize,
        key: [u8; 9],
        offset: u64,
        size: u32,
        kind: u8,
    ) -> (usize, usize, [u8; 9], u64, u32, u8) {
        (page, record, key, (1u64 << 30) | offset, size, kind)
    }

    fn filler(page: usize, record: usize) -> [u8; 9] {
        let mut key = [0x80u8; 9];
        key[1] = page as u8;
        key[2] = record as u8;
        key[3] = 0;
        key
    }

    /// The page table is parsed from its real 24-byte records and scanned in
    /// page/record order. A page type 1/2 replaces the sorted-entry choice, a
    /// type 3 clears it, and a type 6 does not change it.
    #[test]
    fn page_table_parses_and_selects_in_page_order() {
        let sorted = [0x40u8; 9];
        let clear = [0x41u8; 9];
        let interval = [0x42u8; 9];
        let first = [0x10u8; 9];
        let middle = [0x11u8; 9];
        let last = [0x12u8; 9];
        let entries = vec![
            entry(sorted, 0x700, 90),
            entry(clear, 0x710, 91),
            entry(interval, 0x720, 92),
        ];
        let pages = {
            let mut pages = Vec::new();
            for record in 0..=20 {
                let key = match record {
                    0 => first,
                    10 => middle,
                    _ => filler(0, record),
                };
                let kind = if record == 10 { 1 } else { 2 };
                let offset = if record == 10 {
                    0x200
                } else {
                    0x100 + record as u64
                };
                pages.push(page(0, record, key, offset, 40, kind));
            }
            for record in 0..=20 {
                let key = if record == 20 {
                    last
                } else {
                    filler(1, record)
                };
                let offset = if record == 20 {
                    0x300
                } else {
                    0x200 + record as u64
                };
                pages.push(page(1, record, key, offset, 50, 2));
            }
            pages.push(page(2, 0, sorted, 0x400, 70, 2));
            pages.push(page(2, 1, clear, 0x500, 80, 3));
            pages.push(page(2, 2, interval, 0x600, 90, 6));
            pages
        };
        let file = bucket_file_with_pages(7, 3, &[], &pages);
        let parsed = parse_bucket_file(&write_temp("pages", &file), Some(3)).expect("parses");
        assert_eq!(parsed.pages.page_count(), 60);
        assert_eq!(parsed.pages.total_slots(), 60 * 21);
        assert_eq!(parsed.pages.records().len(), 45);
        assert_eq!(parsed.pages.nonzero_records(), 45);
        assert_eq!(parsed.pages.first_empty(), Some((2, 3)));

        let entries = SegmentEntries::with_pages(entries, parsed.pages);
        assert_eq!(entries.find(&first).unwrap().offset, 0x100);
        assert_eq!(entries.find(&middle).unwrap().offset, 0x200);
        assert_eq!(entries.find(&last).unwrap().offset, 0x300);
        // A page hit replaces the sorted entry, and a later type 3 clears it.
        assert_eq!(entries.find(&sorted).unwrap().offset, 0x400);
        assert_eq!(entries.find(&clear), None);
        // A type 6 page record never changes the chosen sorted entry.
        assert_eq!(entries.find(&interval).unwrap().offset, 0x720);

        // The page record type byte is retained exactly.
        assert_eq!(
            entries
                .pages()
                .matches(&first)
                .map(|record| record.kind)
                .collect::<Vec<_>>(),
            vec![2]
        );
        assert_eq!(
            entries
                .pages()
                .matches(&interval)
                .map(|record| record.kind)
                .collect::<Vec<_>>(),
            vec![6]
        );
    }

    /// A zero stored hash ends the client's page scan. A record after the zero
    /// is not retained and cannot resolve, even though it is non-zero and is
    /// counted in the raw diagnostic.
    #[test]
    fn a_zero_page_hash_stops_the_scan() {
        let first = [0x20u8; 9];
        let after = [0x21u8; 9];
        let file = bucket_file_with_pages(
            7,
            3,
            &[],
            &[
                page(0, 0, first, 0x100, 40, 2),
                page(0, 2, after, 0x200, 50, 2),
            ],
        );
        let parsed = parse_bucket_file(&write_temp("page-zero", &file), Some(3)).expect("parses");
        assert_eq!(parsed.pages.first_empty(), Some((0, 1)));
        assert_eq!(parsed.pages.records().len(), 1);
        assert_eq!(parsed.pages.nonzero_records(), 2);

        let entries = SegmentEntries::with_pages(Vec::new(), parsed.pages);
        assert_eq!(entries.find(&first).unwrap().offset, 0x100);
        assert_eq!(entries.find(&after), None);
    }

    /// The client requires at least 0x7800 page-table bytes; a shorter
    /// remainder is a malformed segment, not an empty page table.
    #[test]
    fn a_short_page_table_is_rejected() {
        let mut file = bucket_file(7, 3, &[]);
        let off = page_table_offset(0);
        file.truncate(off + PAGE_SIZE);
        assert!(parse_bucket_file(&write_temp("page-short", &file), Some(3)).is_err());
    }

    /// A page record's stored hash is `DynamicIndexKeyHash(payload, 0) | bit
    /// 31`; a mismatch is a malformed segment, as in `DynamicIndexSegmentParse`.
    #[test]
    fn a_page_record_hash_must_match_its_payload() {
        let key = [0x30u8; 9];
        let mut file = bucket_file_with_pages(7, 3, &[], &[page(0, 0, key, 0x100, 40, 2)]);
        let page_at = page_table_offset(0);
        file[page_at] ^= 0xff;
        assert!(parse_bucket_file(&write_temp("page-hash", &file), Some(3)).is_err());
    }

    /// Page records are decoded in page/record order. A later type 2 replaces
    /// an earlier type 2; a later type 6 never replaces a type 2.
    #[test]
    fn page_collision_uses_decode_order() {
        let key = [0x60u8; 9];
        let location = |offset| Location {
            archive: 1,
            offset,
            size: 40,
        };
        let record = |offset, kind| PageRecord {
            stored_hash: 1,
            key,
            location: location(offset),
            kind,
        };
        let pages = PageTable::from_records(
            1,
            vec![record(0x100, 2), record(0x200, 2), record(0x300, 6)],
            Some((0, 3)),
        );
        let entries = SegmentEntries::with_pages(Vec::new(), pages);
        assert_eq!(entries.find(&key).unwrap().offset, 0x200);

        let pages =
            PageTable::from_records(1, vec![record(0x100, 6), record(0x200, 2)], Some((0, 2)));
        let entries = SegmentEntries::with_pages(Vec::new(), pages);
        assert_eq!(entries.find(&key).unwrap().offset, 0x200);
    }

    /// The page lookup is independent of the file's 60-page minimum: a
    /// one-page table and a many-page table use the same page/record order.
    #[test]
    fn page_counts_one_and_many_share_the_lookup_order() {
        let one = [0x50u8; 9];
        let many = [0x51u8; 9];
        let one_page = PageTable::from_records(
            1,
            vec![PageRecord {
                stored_hash: 1,
                key: one,
                location: Location {
                    archive: 1,
                    offset: 0x100,
                    size: 40,
                },
                kind: 2,
            }],
            None,
        );
        let entries = SegmentEntries::with_pages(Vec::new(), one_page);
        assert_eq!(entries.find(&one).unwrap().offset, 0x100);

        let many_pages = PageTable::from_records(
            4,
            vec![PageRecord {
                stored_hash: 1,
                key: many,
                location: Location {
                    archive: 2,
                    offset: 0x200,
                    size: 50,
                },
                kind: 1,
            }],
            None,
        );
        let entries = SegmentEntries::with_pages(Vec::new(), many_pages);
        assert_eq!(entries.find(&many).unwrap().offset, 0x200);
        assert_eq!(entries.pages().page_count(), 4);
    }

    fn packed(archive: u64, offset: u64) -> u64 {
        (archive << ARCHIVE_SHIFT) | offset
    }

    fn events(set: &IntervalSet) -> Vec<(u64, i32)> {
        set.events()
            .iter()
            .map(|event| (event.start, event.delta))
            .collect()
    }

    /// `DynamicIndexAppendInterval`/`DynamicIndexNormalizeIntervals` on one
    /// interval, two adjacent intervals, two overlapping intervals and a gap.
    /// Adjacent and overlapping ranges collapse to one resident interval; a
    /// gap keeps its two crossings.
    #[test]
    fn interval_normalize_merges_adjacent_and_overlapping_ranges() {
        let mut one = IntervalSet::new();
        one.append(100, 50, true);
        one.normalize();
        assert_eq!(events(&one), vec![(100, 1), (150, -1)]);
        assert!(one.range_is_resident(100, 50, true));
        assert!(!one.range_is_resident(100, 51, true));
        assert!(!one.range_is_resident(99, 1, true));

        let mut adjacent = IntervalSet::new();
        adjacent.append(100, 50, true);
        adjacent.append(150, 50, true);
        adjacent.normalize();
        assert_eq!(events(&adjacent), vec![(100, 1), (200, -1)]);
        assert!(adjacent.range_is_resident(100, 100, true));

        let mut overlapping = IntervalSet::new();
        overlapping.append(100, 50, true);
        overlapping.append(120, 80, true);
        overlapping.normalize();
        assert_eq!(events(&overlapping), vec![(100, 1), (200, -1)]);
        assert!(overlapping.range_is_resident(100, 100, true));

        let mut gap = IntervalSet::new();
        gap.append(100, 50, true);
        gap.append(200, 50, true);
        gap.normalize();
        assert_eq!(events(&gap), vec![(100, 1), (150, -1), (200, 1), (250, -1)]);
        assert!(gap.range_is_resident(100, 50, true));
        assert!(!gap.range_is_resident(150, 50, true));
        assert!(gap.range_is_resident(150, 50, false));
        assert!(gap.range_is_resident(160, 40, false));
        assert!(!gap.range_is_resident(160, 41, false));
        assert!(!gap.range_is_resident(160, 40, true));
    }

    /// Equal-start events are summed with 32-bit wrap before the crossing
    /// decision. Two adjacent appends already exercise the grouping; the raw
    /// constructor pins the wrap: `i32::MAX + 1` becomes `i32::MIN` and does
    /// not emit a `+1`, while `i32::MAX` alone does.
    #[test]
    fn normalize_sums_equal_starts_with_i32_wrapping() {
        let mut not_crossing = IntervalSet::from_events(vec![
            IntervalEvent {
                start: 0,
                delta: i32::MAX,
            },
            IntervalEvent { start: 0, delta: 1 },
        ]);
        not_crossing.normalize();
        assert!(not_crossing.is_empty());

        let mut crossing = IntervalSet::from_events(vec![IntervalEvent {
            start: 0,
            delta: i32::MAX,
        }]);
        crossing.normalize();
        assert_eq!(events(&crossing), vec![(0, 1)]);

        let mut two_minima = IntervalSet::from_events(vec![
            IntervalEvent {
                start: 0,
                delta: i32::MIN,
            },
            IntervalEvent {
                start: 0,
                delta: i32::MIN,
            },
        ]);
        two_minima.normalize();
        assert!(two_minima.is_empty());

        // A descending run of raw events must sort first; the client's sort
        // compares only the start word.
        let mut unsorted = IntervalSet::from_events(vec![
            IntervalEvent {
                start: 300,
                delta: -1,
            },
            IntervalEvent {
                start: 100,
                delta: 1,
            },
        ]);
        unsorted.normalize();
        assert_eq!(events(&unsorted), vec![(100, 1), (300, -1)]);
    }

    /// A type-7 non-resident interval uses the client's `-0x40000000` /
    /// `+0x40000000` pair. Alone it normalizes to no events; inside a resident
    /// interval it splits that interval in two.
    #[test]
    fn non_resident_deltas_cut_a_hole_in_resident_coverage() {
        let mut alone = IntervalSet::new();
        alone.append(100, 50, false);
        assert!(
            alone.is_empty(),
            "a lone non-resident interval emits nothing"
        );

        let mut split = IntervalSet::new();
        split.append(0, 10, true);
        split.append(2, 3, false);
        assert_eq!(events(&split), vec![(0, 1), (2, -1), (5, 1), (10, -1)]);
        assert!(split.range_is_resident(0, 2, true));
        assert!(!split.range_is_resident(2, 1, true));
        assert!(split.range_is_resident(2, 1, false));
        assert!(split.range_is_resident(5, 5, true));
        assert!(!split.range_is_resident(9, 2, true));
    }

    /// `DynamicIndexRangeIsResident` (`0x142520c60`) consumes an event whose
    /// start equals the request's start, compares the end inclusively, returns
    /// `flag ^ 1` at the end of the buffer, and has the two flag branches.
    #[test]
    fn resident_check_uses_the_client_start_and_end_rules() {
        let mut set = IntervalSet::new();
        set.append(100, 50, true);
        set.normalize();

        // The start event is consumed: coverage is 1 at exactly 100.
        assert!(set.range_is_resident(100, 0, true));
        // The end comparison is inclusive: 149 + 1 == 150 is still covered.
        assert!(set.range_is_resident(149, 1, true));
        assert!(!set.range_is_resident(149, 2, true));
        // Before the first event the request is wholly in a gap.
        assert!(set.range_is_resident(0, 100, false));
        assert!(!set.range_is_resident(0, 101, false));
        assert!(!set.range_is_resident(0, 100, true));
        // At or past the last event the buffer ends: flag 1 is false and
        // flag 0 is true, without an end comparison.
        assert!(!set.range_is_resident(150, 0, true));
        assert!(set.range_is_resident(150, 0, false));
        assert!(!set.range_is_resident(10_000, 0, true));
        assert!(set.range_is_resident(10_000, 0, false));

        // A coverage sum that wraps from i32::MAX to i32::MIN must be read as
        // non-positive, exactly as the client's ADD does.
        let wrapped = IntervalSet::from_events(vec![
            IntervalEvent {
                start: 0,
                delta: i32::MAX,
            },
            IntervalEvent { start: 0, delta: 1 },
            IntervalEvent {
                start: 10,
                delta: 0,
            },
        ]);
        assert!(wrapped.range_is_resident(5, 0, false));
        assert!(!wrapped.range_is_resident(5, 0, true));
    }

    fn single_entry(key: [u8; 9], location: u64, size: u32) -> SegmentEntries {
        SegmentEntries::new(vec![(key, Location::from_packed(location, size))])
    }

    /// The resolve statuses and bounds, on a singleton entry: found, past the
    /// end, no entry, the `0xffffffff` length, exact start and exact end.
    #[test]
    fn resolve_range_matches_the_client_statuses_and_bounds() {
        let key = [0x71u8; 9];
        let location = packed(1, 0x1000);
        let entries = single_entry(key, location, 0x100);

        assert_eq!(
            entries.resolve_range(&key, 0x1e, 0x100 - 0x1e),
            ResolveResult::Found {
                range: ResolvedRange {
                    start: location + 0x1e,
                    len: 0xe2,
                },
                entry: ChosenEntry {
                    location,
                    size: 0x100,
                    trailing: 0,
                },
            }
        );

        // The whole stored record, including the data header, is covered by
        // the singleton's type-1 interval.
        assert_eq!(
            entries.resolve_range(&key, 0, 0x100),
            ResolveResult::Found {
                range: ResolvedRange {
                    start: location,
                    len: 0x100,
                },
                entry: ChosenEntry {
                    location,
                    size: 0x100,
                    trailing: 0,
                },
            }
        );

        // Exact end, zero length, and `0xffffffff` all behave as the client.
        assert!(matches!(
            entries.resolve_range(&key, 0x100, 0),
            ResolveResult::Found {
                range: ResolvedRange { len: 0, .. },
                ..
            }
        ));
        assert!(matches!(
            entries.resolve_range(&key, 0x100, u32::MAX),
            ResolveResult::Found {
                range: ResolvedRange { len: 0, .. },
                ..
            }
        ));
        assert_eq!(
            entries.resolve_range(&key, 0xff, u32::MAX),
            ResolveResult::Found {
                range: ResolvedRange {
                    start: location + 0xff,
                    len: 1,
                },
                entry: ChosenEntry {
                    location,
                    size: 0x100,
                    trailing: 0,
                },
            }
        );
        assert_eq!(
            entries.resolve_range(&key, 0x101, 0),
            ResolveResult::PastEnd
        );
        assert_eq!(
            entries.resolve_range(&key, 0x100, 1),
            ResolveResult::PastEnd
        );
        assert_eq!(
            entries.resolve_range(&key, 0x101, u32::MAX),
            ResolveResult::PastEnd
        );
        assert_eq!(
            entries.resolve_range(&[0x72u8; 9], 0, 0),
            ResolveResult::NoEntry
        );
        assert_eq!(entries.find(&[0x72u8; 9]), None);
    }

    /// A duplicate pair decodes as type 2 (chosen, no interval) then type 6
    /// (interval). The chosen header entry covers nothing itself; the second
    /// entry's interval is exactly the payload range `DynamicContainerRead`
    /// asks for (`+0x1e`, `size - 0x1e`).
    #[test]
    fn duplicate_pair_chooses_the_header_and_covers_the_payload() {
        let key = [0x73u8; 9];
        let header = packed(1, 0x2000);
        let entries = SegmentEntries::new(vec![
            (key, Location::from_packed(header, 0x100)),
            (key, Location::from_packed(header + 0x1e, 0xe2)),
        ]);
        let state = entries.key_state(&key);
        assert_eq!(
            state.chosen(),
            Some(ChosenEntry {
                location: header,
                size: 0x100,
                trailing: 0,
            })
        );
        assert_eq!(
            events(state.intervals()),
            vec![(header + 0x1e, 1), (header + 0x100, -1)]
        );
        assert_eq!(
            entries.resolve_range(&key, 0x1e, 0xe2),
            ResolveResult::Found {
                range: ResolvedRange {
                    start: header + 0x1e,
                    len: 0xe2,
                },
                entry: ChosenEntry {
                    location: header,
                    size: 0x100,
                    trailing: 0,
                },
            }
        );
        // The 30-byte header itself is not in the interval set.
        assert_eq!(
            entries.resolve_range(&key, 0, 0x1e),
            ResolveResult::NotResident {
                range: ResolvedRange {
                    start: header,
                    len: 0x1e,
                },
            }
        );
    }

    fn page_record(key: [u8; 9], location: u64, size: u32, kind: u8) -> PageRecord {
        PageRecord {
            stored_hash: 1,
            key,
            location: Location::from_packed(location, size),
            kind,
        }
    }

    /// Every page type through `DynamicIndexEntrySink` (`0x142520590`), with
    /// the chosen entry and interval buffer checked after the final normalize.
    #[test]
    fn entry_sink_page_types_build_the_client_state() {
        let key = [0x74u8; 9];
        let sorted = packed(1, 0x3000);
        let page1 = packed(1, 0x4000);
        let page2 = packed(1, 0x5000);

        // Type 1: chosen plus a resident interval.
        let pages =
            PageTable::from_records(1, vec![page_record(key, page1, 0x40, 1)], Some((0, 1)));
        let entries = SegmentEntries::with_pages(Vec::new(), pages);
        let state = entries.key_state(&key);
        assert_eq!(state.chosen().unwrap().location, page1);
        assert_eq!(
            events(state.intervals()),
            vec![(page1, 1), (page1 + 0x40, -1)]
        );
        assert!(state.is_resident(page1, 0x40));

        // Type 2: chosen, no interval, so a non-empty request is not resident.
        let pages =
            PageTable::from_records(1, vec![page_record(key, page2, 0x40, 2)], Some((0, 1)));
        let entries = SegmentEntries::with_pages(Vec::new(), pages);
        let state = entries.key_state(&key);
        assert_eq!(state.chosen().unwrap().location, page2);
        assert!(state.intervals().is_empty());
        assert_eq!(
            entries.resolve_range(&key, 0, 1),
            ResolveResult::NotResident {
                range: ResolvedRange {
                    start: page2,
                    len: 1,
                },
            }
        );
        assert!(matches!(
            entries.resolve_range(&key, 0, 0),
            ResolveResult::Found { .. }
        ));

        // Type 3: the chosen entry is cleared and the type-1 interval is gone.
        let pages = PageTable::from_records(
            1,
            vec![
                page_record(key, page1, 0x40, 1),
                page_record(key, page2, 0x40, 3),
            ],
            Some((0, 2)),
        );
        let entries =
            SegmentEntries::with_pages(vec![(key, Location::from_packed(sorted, 0x80))], pages);
        let state = entries.key_state(&key);
        assert!(!state.found());
        assert!(state.intervals().is_empty());
        assert_eq!(entries.find(&key), None);
        assert_eq!(entries.resolve_range(&key, 0, 0), ResolveResult::NoEntry);

        // Type 4: clear the buffer and mark the current chosen entry resident;
        // type 5 then clears the buffer again.
        let pages = PageTable::from_records(
            1,
            vec![
                page_record(key, page2, 0x40, 2),
                page_record(key, page2, 0x40, 4),
            ],
            Some((0, 2)),
        );
        let entries = SegmentEntries::with_pages(Vec::new(), pages);
        let state = entries.key_state(&key);
        assert_eq!(state.chosen().unwrap().location, page2);
        assert_eq!(
            events(state.intervals()),
            vec![(page2, 1), (page2 + 0x40, -1)]
        );
        assert!(state.is_resident(page2, 0x40));

        let pages = PageTable::from_records(
            1,
            vec![
                page_record(key, page1, 0x40, 1),
                page_record(key, page2, 0x40, 5),
            ],
            Some((0, 2)),
        );
        let entries = SegmentEntries::with_pages(Vec::new(), pages);
        let state = entries.key_state(&key);
        assert_eq!(state.chosen().unwrap().location, page1);
        assert!(state.intervals().is_empty());
        assert_eq!(
            entries.resolve_range(&key, 0, 1),
            ResolveResult::NotResident {
                range: ResolvedRange {
                    start: page1,
                    len: 1,
                },
            }
        );

        // A type 4 with no earlier chosen entry leaves the state unfound and
        // appends the zero entry, whose two events cancel.
        let pages =
            PageTable::from_records(1, vec![page_record(key, page2, 0x40, 4)], Some((0, 1)));
        let entries = SegmentEntries::with_pages(Vec::new(), pages);
        let state = entries.key_state(&key);
        assert!(!state.found());
        assert!(state.intervals().is_empty());
        assert_eq!(entries.resolve_range(&key, 0, 0), ResolveResult::NoEntry);

        // Type 6 only adds an interval; it never sets the chosen entry.
        let pages =
            PageTable::from_records(1, vec![page_record(key, page2, 0x40, 6)], Some((0, 1)));
        let entries = SegmentEntries::with_pages(Vec::new(), pages);
        let state = entries.key_state(&key);
        assert!(!state.found());
        assert_eq!(
            events(state.intervals()),
            vec![(page2, 1), (page2 + 0x40, -1)]
        );
        assert_eq!(entries.resolve_range(&key, 0, 0), ResolveResult::NoEntry);
    }

    /// A type-7 page record removes a hole from the chosen entry's coverage.
    /// The final normalize runs after the page records, as
    /// `DynamicIndexDecodeEntries` does.
    #[test]
    fn type_7_page_record_splits_the_chosen_interval() {
        let key = [0x75u8; 9];
        let sorted = packed(1, 0x6000);
        let hole = sorted + 0x40;
        let pages = PageTable::from_records(1, vec![page_record(key, hole, 0x20, 7)], Some((0, 1)));
        let entries =
            SegmentEntries::with_pages(vec![(key, Location::from_packed(sorted, 0x100))], pages);
        let state = entries.key_state(&key);
        assert_eq!(state.chosen().unwrap().location, sorted);
        assert_eq!(
            events(state.intervals()),
            vec![
                (sorted, 1),
                (hole, -1),
                (hole + 0x20, 1),
                (sorted + 0x100, -1)
            ]
        );
        assert!(state.is_resident(sorted, 0x40));
        assert!(!state.is_resident(hole, 1));
        assert!(state.is_resident(hole + 0x20, 0x100 - 0x60));
        assert_eq!(
            entries.resolve_range(&key, 0x50, 1),
            ResolveResult::NotResident {
                range: ResolvedRange {
                    start: hole + 0x10,
                    len: 1,
                },
            }
        );
        assert!(matches!(
            entries.resolve_range(&key, 0x70, 1),
            ResolveResult::Found { .. }
        ));
    }
}

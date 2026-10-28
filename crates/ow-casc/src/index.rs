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
}

impl SegmentEntries {
    pub fn new(entries: Vec<([u8; 9], Location)>) -> SegmentEntries {
        SegmentEntries {
            entries,
            pages: PageTable::default(),
        }
    }

    pub fn with_pages(entries: Vec<([u8; 9], Location)>, pages: PageTable) -> SegmentEntries {
        SegmentEntries { entries, pages }
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

    /// The entry the client resolves.
    ///
    /// The 18-byte search gives the first equal entry, exactly as
    /// `DynamicIndexDecodeEntries` (`0x1425209a0`) gives a single equal entry
    /// type 1 and the first of several type 2. Either way the chosen location
    /// is that first entry; type 1 additionally merges the entry's interval,
    /// which this method does not model (the interval/residency path is open).
    /// The client then decodes the page-table matches, in page/record order,
    /// through the same entry sink: page type 1 or 2 replaces the chosen entry,
    /// type 3 clears it, and types 4-7 leave the chosen entry alone (they
    /// affect only the interval buffer or metrics). This method models that
    /// chosen-entry effect; the interval buffer is not part of the read lookup.
    pub fn find(&self, key: &[u8; 9]) -> Option<Location> {
        let range = self.find_range(key);
        let mut chosen = if range.is_empty() {
            None
        } else {
            Some(self.entries[range.start].1)
        };
        for record in self.pages.matches(key) {
            match record.kind {
                1 | 2 => chosen = Some(record.location),
                3 => chosen = None,
                _ => {}
            }
        }
        chosen
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

/// Number of buckets in a dynamic index.
pub const BUCKET_COUNT: usize = 16;

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
    if data.len() < SECTION_HEADER + FILE_HEADER_SIZE {
        return Err(Error::Malformed(
            path.to_path_buf(),
            "file shorter than its header",
        ));
    }

    // The section header's byte count positions the entry block. The client
    // (`0x14250fcf0`) requires it to be at least 0x10 and rounds
    // `sectionSize + 0x17` down to a 16-byte boundary; the entry block's own
    // 8-byte BlockSizeAndHash follows, then the entries. The usual file has
    // sectionSize 16 -> entry header at 32 -> entries at 40.
    let section_size = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    if section_size < 0x10 {
        return Err(Error::Malformed(
            path.to_path_buf(),
            "section header is smaller than 16 bytes",
        ));
    }
    let entry_header = (section_size as usize + 0x17) & !0xf;
    if data.len() < entry_header + SECTION_HEADER {
        return Err(Error::Malformed(
            path.to_path_buf(),
            "entry block header runs past the end of the file",
        ));
    }
    let entries_offset = entry_header + SECTION_HEADER;

    // The section's second word is a 32-bit hash of the section body, and the
    // entry block's second word is a rolling hash over the 18-byte entries.
    // The client checks both before using the segment (`0x14250fcf0`); this is
    // the same hash, verified against all sixteen files of the reference
    // install. Rejecting a mismatch is what keeps a corrupt index from
    // resolving to arbitrary bytes.
    let header_end = 8 + section_size as usize;
    if data.len() < header_end {
        return Err(Error::Malformed(
            path.to_path_buf(),
            "section body runs past the end of the file",
        ));
    }
    let mut state = [0u32; 2];
    hash_index(&data[8..header_end], &mut state);
    let stored = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    if state[0] != stored {
        return Err(Error::Malformed(
            path.to_path_buf(),
            "file header hash mismatch",
        ));
    }

    let version = u16::from_le_bytes([data[8], data[9]]);
    let bucket = data[10];
    let span_size = data[12];
    let span_offs = data[13];
    let ekey_bytes = data[14];

    // The client's parser (`FUN_14250fcf0`) accepts any version >= 7 and
    // checks only the three field widths it hardcodes: 4-byte size, 5-byte
    // location, 9-byte key. It never looks at the extra-bytes or offset-bits
    // bytes, so neither do we; a v8 file with the same widths reads the same.
    if version < 7 || span_size != 4 || span_offs != 5 || ekey_bytes != 9 {
        return Err(Error::Malformed(
            path.to_path_buf(),
            "unsupported index header",
        ));
    }
    if let Some(expected) = expect_bucket {
        if bucket != expected {
            return Err(Error::Malformed(
                path.to_path_buf(),
                "bucket number disagrees with filename",
            ));
        }
    }

    let block_size = i32::from_le_bytes([
        data[entry_header],
        data[entry_header + 1],
        data[entry_header + 2],
        data[entry_header + 3],
    ]);
    if block_size < 0 || block_size as usize % ENTRY_SIZE != 0 {
        return Err(Error::Malformed(
            path.to_path_buf(),
            "entry block size is not a multiple of 18",
        ));
    }
    let count = block_size as usize / ENTRY_SIZE;

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
}

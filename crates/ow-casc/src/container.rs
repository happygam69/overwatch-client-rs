//! Ties the store together: bucket index, data archives, BLTE, encoding file.
//!
//! Lookup path for a content key is
//! `encoding file -> encoding key -> bucket index -> data.NNN -> BLTE`.
//!
//! The bucket index is the client's dynamic index, not a search over every
//! `.idx` file: the key's nine bytes pick one of sixteen buckets
//! ([`index::bucket_of`]), that bucket has exactly one selected segment (the
//! highest `<bucket><segment>.idx` number found by the directory scan), and
//! the segment's entry supplies the archive ordinal and offset. `ecache/` and
//! `indices/` are not local content stores: the first holds chunk manifests,
//! the second CDN routes, and merging either into a local lookup produced
//! wrong reads.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::io::{ErrorKind, Read, Seek, SeekFrom};
use std::path::Path;

use crate::blte;
use crate::bytes;
use crate::encoding::EncodingFile;
use crate::index::{self, Location};
use crate::install::{Error as InstallError, Install};
use crate::pagecache::{self, PageCache};

#[derive(Debug)]
pub enum Error {
    Install(InstallError),
    Io(std::io::Error),
    Index(index::Error),
    Blte(blte::Error),
    Encoding(crate::encoding::Error),
    /// The key is not present in this installation.
    KeyNotFound(String),
    Parse(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Install(e) => write!(f, "{e}"),
            Error::Io(e) => write!(f, "io error: {e}"),
            Error::Index(e) => write!(f, "{e}"),
            Error::Blte(e) => write!(f, "{e}"),
            Error::Encoding(e) => write!(f, "{e}"),
            Error::KeyNotFound(k) => write!(f, "key {k} is not present in this installation"),
            Error::Parse(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<InstallError> for Error {
    fn from(e: InstallError) -> Self {
        Error::Install(e)
    }
}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}
impl From<index::Error> for Error {
    fn from(e: index::Error) -> Self {
        Error::Index(e)
    }
}
impl From<blte::Error> for Error {
    fn from(e: blte::Error) -> Self {
        Error::Blte(e)
    }
}
impl From<crate::encoding::Error> for Error {
    fn from(e: crate::encoding::Error) -> Self {
        Error::Encoding(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// One bucket's selected `.idx` segment.
///
/// The client keeps exactly one segment per bucket: the highest
/// `<bucket:02x><segment:08x>.idx` number its directory scan finds
/// (`DynamicIndexScanSegments`). `segment` is the number in the file name and `entries`
/// is that file's key table; keys are never merged across segments or
/// buckets.
#[derive(Debug)]
struct BucketSegment {
    segment: u32,
    entries: index::SegmentEntries,
}

/// The sixteen bucket maps, selected by the client's filename scan.
#[derive(Debug)]
struct BucketIndex {
    buckets: [Option<BucketSegment>; index::BUCKET_COUNT],
}

impl BucketIndex {
    /// Scan `dir` and load the highest segment of each bucket.
    ///
    /// The client's read-only open (`DynamicIndexOpen`, mode 2/3) requires all
    /// sixteen buckets; a missing one is an error, not a reason to merge
    /// another segment. `parse_bucket_file` checks the header's bucket byte
    /// against the bucket the name selected, so a misnamed file is rejected.
    fn open_dir(dir: &Path) -> Result<BucketIndex> {
        let found = discover_segments(dir)?;
        let mut buckets: [Option<BucketSegment>; index::BUCKET_COUNT] =
            std::array::from_fn(|_| None);
        for (bucket, found) in found.into_iter().enumerate() {
            let Some(found) = found else {
                return Err(Error::Parse(format!(
                    "bucket {bucket:02x} has no .idx segment in {}; the client's read-only open requires all 16",
                    dir.display()
                )));
            };
            let file = index::parse_bucket_file(&found.path, Some(bucket as u8))?;
            buckets[bucket] = Some(BucketSegment {
                segment: found.segment,
                entries: index::SegmentEntries::with_pages(file.entries, file.pages),
            });
        }
        Ok(BucketIndex { buckets })
    }

    fn location_of(&self, key: &[u8; 9]) -> Option<Location> {
        let bucket = usize::from(index::bucket_of(key, 0));
        self.buckets[bucket].as_ref()?.entries.find(key)
    }

    /// The client's resolve for the bucket `bucket_of(key, 0)` selects.
    fn resolve(&self, key: &[u8; 9], start: u64, len: u32) -> Option<index::ResolveResult> {
        let bucket = usize::from(index::bucket_of(key, 0));
        Some(
            self.buckets[bucket]
                .as_ref()?
                .entries
                .resolve_range(key, start, len),
        )
    }

    fn key_count(&self) -> usize {
        self.buckets
            .iter()
            .flatten()
            .map(|bucket| bucket.entries.unique_key_count())
            .sum()
    }

    fn entry_count(&self) -> usize {
        self.buckets
            .iter()
            .flatten()
            .map(|bucket| bucket.entries.len())
            .sum()
    }

    fn segment_count(&self) -> usize {
        self.buckets.iter().flatten().count()
    }

    fn segment_numbers(&self) -> [Option<u32>; index::BUCKET_COUNT] {
        std::array::from_fn(|bucket| self.buckets[bucket].as_ref().map(|b| b.segment))
    }
}

/// One `.idx` file the directory scan selected.
struct DiscoveredSegment {
    segment: u32,
    path: std::path::PathBuf,
}

/// Scan `dir` for `<bucket:02x><segment:08x>.idx` names and keep the highest
/// segment number of each bucket (`DynamicIndexScanSegments`).
///
/// The client's scan deletes a lower segment and any `._idx` temporary in a
/// read-write store, and ignores both in a read-only one; no segment is ever
/// merged with another. The highest wins, and an equal number replaces.
fn discover_segments(dir: &Path) -> Result<[Option<DiscoveredSegment>; index::BUCKET_COUNT]> {
    let mut found: [Option<DiscoveredSegment>; index::BUCKET_COUNT] = std::array::from_fn(|_| None);
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        // `FUN_1426a16b0` reports directories separately and the scan skips
        // them; a directory named like a segment must not be opened.
        if entry.file_type()?.is_dir() {
            continue;
        }
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        let Some(parsed) = index::parse_segment_file_name(name) else {
            continue;
        };
        if parsed.temporary {
            continue;
        }
        let slot = &mut found[usize::from(parsed.bucket)];
        if slot
            .as_ref()
            .is_some_and(|current| current.segment > parsed.segment)
        {
            continue;
        }
        *slot = Some(DiscoveredSegment {
            segment: parsed.segment,
            path: entry.path(),
        });
    }
    Ok(found)
}

/// Read-only view over an installation's store.
pub struct Store {
    install: Install,
    index: BucketIndex,
    /// Open pack files, keyed by archive ordinal (`data/data.NNN`). This is
    /// the client's archive-handle cache (`DynamicContainerOpenArchive`).
    archives: RefCell<HashMap<u32, fs::File>>,
    /// The client's 16-slot `0x10000`-byte page cache. `DynamicContainerRead`
    /// reads its payload through it; the 30-byte data header is our own read
    /// for the decoder (the client skips the header and never reads it).
    page_cache: RefCell<PageCache>,
    /// The decoded encoding **manifests**: this build config names more than
    /// one (`encoding = <a> <b>`), and an asset's content key can be in either.
    /// Loading only the first reported half the install as "key … is not present
    /// in this installation", which is our own message for "the encoding table
    /// has no entry for this key" — not a missing decryption key at all.
    encoding: RefCell<Option<Vec<Vec<u8>>>>,
    /// What happened to each encoding manifest the build config named.
    encoding_report: RefCell<Vec<String>>,
}

impl Store {
    pub fn open(install: Install) -> Result<Store> {
        let index = BucketIndex::open_dir(&install.data_dir())?;
        Ok(Store {
            install,
            index,
            archives: RefCell::new(HashMap::new()),
            page_cache: RefCell::new(PageCache::new()),
            encoding: RefCell::new(None),
            encoding_report: RefCell::new(Vec::new()),
        })
    }

    pub fn install(&self) -> &Install {
        &self.install
    }

    /// Number of encoding keys the bucket index knows about.
    pub fn key_count(&self) -> usize {
        self.index.key_count()
    }

    /// Number of 18-byte entries across the selected segments.
    ///
    /// This is larger than [`Store::key_count`] when a key is listed twice.
    pub fn entry_count(&self) -> usize {
        self.index.entry_count()
    }

    /// The selected segment number of each bucket, in bucket order.
    pub fn segment_numbers(&self) -> [Option<u32>; index::BUCKET_COUNT] {
        self.index.segment_numbers()
    }

    /// Record every archive read the page cache issues. Diagnostics only; a
    /// long-lived store keeps this off.
    pub fn set_page_cache_tracing(&self, enabled: bool) {
        self.page_cache.borrow_mut().set_tracing(enabled);
    }

    /// The archive reads the page cache issued since the last clear.
    pub fn page_cache_trace(&self) -> Vec<pagecache::ReadOp> {
        self.page_cache.borrow().trace().to_vec()
    }

    pub fn clear_page_cache_trace(&self) {
        self.page_cache.borrow_mut().clear_trace();
    }

    /// Empty the page cache. Diagnostic/test helper; the read-only path never
    /// calls the client's close/reopen maintenance.
    pub fn clear_page_cache(&self) {
        self.page_cache.borrow_mut().clear();
    }

    /// Resident page slots as `(archive, page offset, stored bytes, stamp)`.
    pub fn page_cache_resident(&self) -> Vec<(u32, u64, u32, u64)> {
        self.page_cache.borrow().resident()
    }

    /// What the local index layer found, for `look --index-stats`.
    ///
    /// `indices/*.index` are counted but never turned into locations: they
    /// describe the CDN, with its own archive numbering and offsets.
    pub fn stats(&self) -> StoreStats {
        StoreStats {
            keys: self.index.key_count(),
            bucket_files: self.index.segment_count(),
            data_packs: pack_count(&self.install.data_dir()),
            cdn_indexes: index_file_count(&self.install.indices_dir()),
        }
    }

    pub fn location_of(&self, ekey: &[u8]) -> Option<Location> {
        let key: [u8; 9] = ekey.get(0..9)?.try_into().ok()?;
        self.index.location_of(&key)
    }

    /// Resolve a key's request against the client's key state.
    ///
    /// `DynamicKeyStateLookup` (`0x142501b40`) selects the salt-0 bucket and
    /// calls `DynamicIndexResolveRange` (`0x1425115d0`) with the caller's
    /// `(start, len)` request. The result's status codes are 0 found, 2 no
    /// entry, 3 not resident, 4 past the entry's end. This is the read path's
    /// lookup; it returns `None` only when the salt-0 bucket has no segment,
    /// which a read-only open does not allow.
    pub fn resolve_range(&self, ekey: &[u8], start: u64, len: u32) -> Result<index::ResolveResult> {
        let key: [u8; 9] = ekey
            .get(0..9)
            .and_then(|key| key.try_into().ok())
            .ok_or_else(|| Error::KeyNotFound(hex(ekey)))?;
        self.index
            .resolve(&key, start, len)
            .ok_or_else(|| Error::Parse("the salt-0 bucket has no segment".into()))
    }

    /// Read and resolve the payload the client's `DynamicContainerRead`
    /// (`0x1424f48d0`) asks for.
    ///
    /// A full-record read is the caller's `(offset = 0, len = size)`, so the
    /// key-state request is `(0x1e, size - 0x1e)` and the absolute output
    /// range is `(packed + 0x1e, size - 0x1e)`. That range must be resident:
    /// the install's duplicate keys rely on exactly that, because the chosen
    /// header entry has no interval of its own and the second entry's interval
    /// is the payload range the request asks for.
    ///
    /// `DynamicContainerReadPage` (`0x1424fcd20`) then rejects an archive
    /// number above `0x3fe`, and rejects a range whose first and last byte are
    /// not in the same archive (status 10); it does **not** split a range at a
    /// 2^30 boundary. No entry in this install crosses, so the check is
    /// reached only by a corrupt entry.
    fn read_payload_of(&self, ekey: &[u8]) -> Result<(Location, Vec<u8>)> {
        let key: [u8; 9] = ekey
            .get(0..9)
            .and_then(|key| key.try_into().ok())
            .ok_or_else(|| Error::KeyNotFound(hex(ekey)))?;
        let bucket = usize::from(index::bucket_of(&key, 0));
        let Some(segment) = self.index.buckets[bucket].as_ref() else {
            return Err(Error::Parse(format!(
                "the salt-0 bucket {bucket:02x} has no segment"
            )));
        };
        let state = segment.entries.key_state(&key);
        let Some(chosen) = state.chosen() else {
            return Err(Error::KeyNotFound(hex(ekey)));
        };
        let payload_len = chosen
            .size
            .checked_sub(index::DATA_HEADER_SIZE)
            .ok_or_else(|| {
                Error::Parse(format!(
                    "key {}: stored record is {} B, shorter than the 30-byte data header",
                    hex(ekey),
                    chosen.size
                ))
            })?;
        let resolved = state.resolve(u64::from(index::DATA_HEADER_SIZE), payload_len);
        if resolved.status() != index::ResolveStatus::Found {
            return Err(Error::Parse(format!(
                "key {}: client resolve status {} for the {payload_len}-byte payload range",
                hex(ekey),
                resolved.status().code()
            )));
        }
        let location = chosen.to_location();
        let range = resolved.range().ok_or_else(|| {
            Error::Parse(format!(
                "key {}: resolve status 0 without an output range",
                hex(ekey)
            ))
        })?;

        let start = range.start;
        let len = range.len;
        let archive = (start >> index::ARCHIVE_SHIFT) as u32;
        if archive > index::MAX_ARCHIVE {
            return Err(Error::Parse(format!(
                "data archive {archive} is beyond the client's 0..=1022 range"
            )));
        }
        let last = if len == 0 {
            archive
        } else {
            ((start + u64::from(len) - 1) >> index::ARCHIVE_SHIFT) as u32
        };
        if last != archive {
            return Err(Error::Parse(format!(
                "key {}: payload range {start:#x}+{len} crosses a 2^30 archive boundary (client status 10)",
                hex(ekey)
            )));
        }

        // The index entry's size field is file data. The client allocates it
        // and discovers a short read afterwards; this port checks the archive
        // first, so a corrupt entry cannot ask for a multi-gigabyte allocation
        // before the read fails.
        let offset_in_archive = start & index::ARCHIVE_MASK;
        if len != 0 {
            let archive_len = {
                let mut archives = self.archives.borrow_mut();
                let file = open_archive(&mut archives, &self.install, archive)?;
                file.metadata()?.len()
            };
            if offset_in_archive.saturating_add(u64::from(len)) > archive_len {
                return Err(Error::Parse(format!(
                    "key {}: {len}-byte payload at {offset_in_archive:#x} is past the {archive_len}-byte archive {archive}",
                    hex(ekey)
                )));
            }
        }

        let mut out = vec![0u8; len as usize];
        let read = self.read_archive_span(archive, offset_in_archive, len, &mut out)?;
        out.truncate(read as usize);
        Ok((location, out))
    }

    /// Read the payload bytes `DynamicContainerRead` returns: the stored
    /// record **without** its 30-byte data header. This is the client's exact
    /// range and goes through the archive page cache.
    pub fn read_payload(&self, ekey: &[u8]) -> Result<Vec<u8>> {
        Ok(self.read_payload_of(ekey)?.1)
    }

    /// Read the stored (still BLTE-encoded) record for an encoding key,
    /// 30-byte data header included.
    ///
    /// The client never reads the data header: `DynamicContainerRead` adds
    /// `0x1e` to the caller's offset and the `.cmf` reader passes offset 0.
    /// This wrapper exists for `blte::decode` and the install tests, and reads
    /// the header with one un-cached 30-byte read before concatenating the
    /// client's payload. The payload read itself is [`Store::read_payload`],
    /// the client's range through the page cache.
    pub fn read_stored(&self, ekey: &[u8]) -> Result<Vec<u8>> {
        let (location, payload) = self.read_payload_of(ekey)?;
        let mut out = vec![0u8; index::DATA_HEADER_SIZE as usize];
        // The record starts at `packed`; `DynamicContainerRead` adds 0x1e for
        // the payload. A range that did not cross an archive boundary keeps
        // the header in the same archive as the payload.
        let header_archive = (location.packed() >> index::ARCHIVE_SHIFT) as u32;
        let header_offset = location.packed() & index::ARCHIVE_MASK;
        let read = self.read_archive_span_direct(header_archive, header_offset, &mut out)?;
        if read as usize != out.len() {
            return Err(Error::Parse(format!(
                "key {}: data header short read ({read} of {} B)",
                hex(ekey),
                out.len()
            )));
        }
        out.extend_from_slice(&payload);
        Ok(out)
    }

    /// Read one archive span through the client's page cache.
    ///
    /// The range is archive-local and must not cross the 2^30 boundary; the
    /// caller has already applied `DynamicContainerReadPage`'s checks. A
    /// whole-range bypass (aligned range, closed cache) is one direct read.
    /// Short reads are returned as the count, not as an error; the page cache
    /// zero-fills a fragment the archive did not supply, as the completion
    /// `0x1424f9bf0` does.
    fn read_archive_span(
        &self,
        archive: u32,
        offset: u64,
        len: u32,
        out: &mut [u8],
    ) -> Result<u32> {
        if len == 0 {
            return Ok(0);
        }
        let mut archives = self.archives.borrow_mut();
        let file = open_archive(&mut archives, &self.install, archive)?;
        let mut cache = self.page_cache.borrow_mut();
        let outcome = cache.read(archive, offset, len, out, |_, at, buf| {
            file.seek(SeekFrom::Start(at))?;
            file.read(buf)
        })?;
        match outcome {
            pagecache::Outcome::Served(bytes) => Ok(bytes),
            pagecache::Outcome::Bypass => {
                file.seek(SeekFrom::Start(offset))?;
                Ok(file.read(&mut out[..len as usize])? as u32)
            }
        }
    }

    /// One direct archive read, outside the page cache.
    ///
    /// Used only for the 30-byte data header, which the client's read path
    /// never touches.
    fn read_archive_span_direct(&self, archive: u32, offset: u64, out: &mut [u8]) -> Result<u32> {
        let mut archives = self.archives.borrow_mut();
        let file = open_archive(&mut archives, &self.install, archive)?;
        file.seek(SeekFrom::Start(offset))?;
        Ok(file.read(out)? as u32)
    }

    /// Decode a stored record for an encoding key.
    ///
    /// Local first, always. When `OW_CDN` enabled fetching and the record is
    /// not locally resolvable — no index entry, or the entry names an archive
    /// file that is not on disk — the record is fetched from Blizzard's CDN
    /// (group entry as a Range GET, else a loose whole-file GET) and decoded
    /// the same way. A decode failure of *locally present* bytes never falls
    /// back: the bytes are here; a keyring problem is not a CDN problem.
    pub fn open_ekey(&self, ekey: &[u8]) -> Result<Vec<u8>> {
        let stored = match self.read_payload(ekey) {
            Ok(stored) => stored,
            Err(Error::KeyNotFound(_)) => match self.fetch_stored_remotely(ekey) {
                Some(stored) => stored,
                None => return Err(Error::KeyNotFound(hex(ekey))),
            },
            Err(Error::Io(e)) if e.kind() == ErrorKind::NotFound => {
                match self.fetch_stored_remotely(ekey) {
                    Some(stored) => stored,
                    None => return Err(Error::Io(e)),
                }
            }
            Err(e) => return Err(e),
        };
        Ok(blte::decode(&stored, &self.install.keyring)?)
    }

    /// Where the CDN would find a key: the group entry (an archive slice) or
    /// the loose-file index (a whole file). Purely local reads — it opens the
    /// index files this install already has, in `indices/`, and says what the
    /// route would be. `None` means no local index knows the key.
    pub fn cdn_route_of(&self, key: &[u8; 16]) -> Option<CdnRoute> {
        let config = self.install.cdn_config.as_ref()?;

        if let Some(route) = self.group_route(config, "archive-group", "archives", false, key) {
            return Some(route);
        }
        if let Some(route) =
            self.group_route(config, "patch-archive-group", "patch-archives", true, key)
        {
            return Some(route);
        }

        let loose_hash = config.get_str("file-index").ok()?;
        let index_key = parse_hex_key(loose_hash).ok()?;
        let mut index = PageIndex::open_with_key(
            &self
                .install
                .indices_dir()
                .join(format!("{loose_hash}.index")),
            Some(&index_key),
        )
        .ok()?;
        match index.loose_lookup(key) {
            Ok(Some(size)) => Some(CdnRoute::Loose { size }),
            _ => None,
        }
    }

    /// One group index (`archive-group`, or the patch one): find the key, map
    /// the entry's archive ordinal through the matching hash list.
    fn group_route(
        &self,
        config: &crate::install::Config,
        group_key: &str,
        list_key: &str,
        patch: bool,
        key: &[u8; 16],
    ) -> Option<CdnRoute> {
        let group_hash = config.get_str(group_key).ok()?;
        let list = config.get(list_key)?;
        let index_key = parse_hex_key(group_hash).ok()?;
        let mut index = PageIndex::open_with_key(
            &self
                .install
                .indices_dir()
                .join(format!("{group_hash}.index")),
            Some(&index_key),
        )
        .ok()?;
        let (ordinal, offset, size) = match index.group_lookup(key) {
            Ok(Some(entry)) => entry,
            Ok(None) => return None,
            Err(e) => {
                eprintln!("cdn: {group_hash}.index: {e}");
                return None;
            }
        };
        match list.get(ordinal as usize) {
            Some(hash) => Some(CdnRoute::Group {
                archive_hash: hash.clone(),
                offset,
                size,
                patch,
            }),
            None => {
                eprintln!(
                    "cdn: {group_hash}.index names {list_key} ordinal {ordinal}, \
                     but the list has {} entries",
                    list.len()
                );
                None
            }
        }
    }

    /// Fetch one stored record's bytes from the CDN, if a route exists.
    ///
    /// Order mirrors the reference's `TryOpenEKeyFromRemote`: the group
    /// (archived) route first, then the loose-file route.
    fn fetch_stored_remotely(&self, ekey: &[u8]) -> Option<Vec<u8>> {
        let cdn = self.install.cdn.as_ref()?;
        let key: [u8; 16] = ekey.get(0..16)?.try_into().ok()?;
        match self.cdn_route_of(&key) {
            Some(CdnRoute::Group {
                archive_hash,
                offset,
                size,
                patch,
            }) => {
                let what = if patch { "patch archive" } else { "archive" };
                match cdn.fetch_range(&archive_hash, offset, u64::from(size)) {
                    Ok(bytes) => {
                        eprintln!(
                            "cdn: {} resolved from {what} {} at {offset:#x} ({size} B)",
                            hex(&key),
                            &archive_hash[..archive_hash.len().min(12)]
                        );
                        Some(bytes)
                    }
                    Err(e) => {
                        eprintln!("cdn: archive slice for {} failed: {e}", hex(&key));
                        None
                    }
                }
            }
            Some(CdnRoute::Loose { .. }) => match cdn.fetch_data(&hex(&key), None) {
                Ok(bytes) => {
                    eprintln!(
                        "cdn: {} resolved as a loose file ({} B)",
                        hex(&key),
                        bytes.len()
                    );
                    Some(bytes)
                }
                Err(e) => {
                    eprintln!("cdn: loose fetch for {} failed: {e}", hex(&key));
                    None
                }
            },
            None => None,
        }
    }

    /// Load the decoded encoding manifest the build config names.
    ///
    /// A build-config line is **`<content key> <encoding key>`**: the
    /// reference's `BuildConfig.GetFileRecord` reads `vals[0]` into
    /// `ContentKey` and `vals[1]` into `EncodingKey` (`BuildConfigParse`,
    /// `0x1424ccc00`, the `encoding` case, stores them at build-config
    /// `+0x1a0` and `+0x1b4`).
    /// Only the encoding key names a stored artifact; the content key is the
    /// same file's content identity. On this install the two sizes in
    /// `encoding-size` (`111,058,223 109,882,544`) are exactly the decoded
    /// and stored sizes of that one file.
    ///
    /// The encoding key is therefore tried first. The other value is only a
    /// fallback, for a build that writes the pair in the other order (or a
    /// one-value line); it is never loaded as a second table.
    fn ensure_encoding(&self) -> Result<()> {
        if self.encoding.borrow().is_some() {
            return Ok(());
        }
        let values = self
            .install
            .build_config
            .get("encoding")
            .map(|v| v.to_vec())
            .ok_or_else(|| Error::Parse("build config has no encoding key".into()))?;
        let mut blobs: Vec<Vec<u8>> = Vec::new();
        let mut report = Vec::new();
        let mut opened: Option<&String> = None;

        // The reference reads the second value as the encoding key. For a
        // one-value line that value is all there is.
        let order: Vec<&String> = if values.len() >= 2 {
            vec![&values[1], &values[0]]
        } else {
            values.iter().collect()
        };
        for value in &order {
            let Ok(key) = parse_hex_key(value) else {
                report.push(format!("{value}: not a key"));
                continue;
            };
            match self.open_ekey(&key) {
                Ok(blob) => {
                    report.push(format!(
                        "{value}: opened as an encoding key, {} B",
                        blob.len()
                    ));
                    blobs.push(blob);
                    opened = Some(value);
                    break;
                }
                Err(_) => report.push(format!("{value}: not an encoding key")),
            }
        }
        if values.len() >= 2 && !blobs.is_empty() && opened != Some(&values[0]) {
            // Say what the other half is, so the diagnostic cannot be read as
            // "a second manifest failed to open".
            report.push(format!(
                "{}: content key of the same encoding file",
                values[0]
            ));
        }
        if blobs.is_empty() {
            return Err(Error::Parse("no encoding manifest could be read".into()));
        }
        self.encoding_report.borrow_mut().clone_from(&report);
        *self.encoding.borrow_mut() = Some(blobs);
        Ok(())
    }

    /// Search a build-config manifest (by its config key) for a content key.
    ///
    /// The **install manifest** lists exactly the files present on disk, as
    /// `ekey[16] ckey[16]` pairs. A content key found here has a locally
    /// installable encoding even when the *root* manifest's variant does not
    /// cover it, which is the difference between "not in this installation" and
    /// "not in the manifest I happened to read".
    pub fn find_ckey_in(
        &self,
        config_key: &str,
        ckey: &[u8; 16],
    ) -> Result<Vec<(usize, [u8; 16])>> {
        let hashes = self
            .install
            .build_config
            .get(config_key)
            .map(|v| v.to_vec())
            .unwrap_or_default();
        let mut out = Vec::new();
        for (i, hash) in hashes.iter().enumerate() {
            let Ok(ekey) = parse_hex_key(hash) else {
                continue;
            };
            let Ok(blob) = self.open_ekey(&ekey) else {
                continue;
            };
            // The last valid 32-byte window starts at `len - 32`; an
            // exclusive range stopped one short of it and never matched the
            // manifest's final entry.
            if blob.len() >= 32 {
                for at in 0..=blob.len() - 32 {
                    if &blob[at + 16..at + 32] == ckey {
                        let mut ekey = [0u8; 16];
                        ekey.copy_from_slice(&blob[at..at + 16]);
                        out.push((i, ekey));
                        break;
                    }
                }
            }
        }
        Ok(out)
    }

    /// What happened to each encoding manifest the build config named.
    pub fn encoding_report(&self) -> Vec<String> {
        self.encoding_report.borrow().clone()
    }

    /// Does the decoded encoding manifest contain these 16 bytes *anywhere*?
    ///
    /// The page-hashed lookup can miss while the bytes are present, which is the
    /// difference between "the build's table has no entry" and "our lookup of it
    /// is wrong".
    pub fn encoding_blob_contains(&self, needle: &[u8; 16]) -> Vec<usize> {
        let borrowed = self.encoding.borrow();
        let Some(blobs) = borrowed.as_ref() else {
            return Vec::new();
        };
        blobs
            .iter()
            .enumerate()
            .filter(|(_, b)| b.windows(16).any(|w| w == needle))
            .map(|(i, _)| i)
            .collect()
    }

    /// Which encoding keys each encoding manifest gives for a content key.
    ///
    /// This is the step a "key … is not present in this installation" usually
    /// stands for, and *seeing* it separates "the build's encoding table has no
    /// entry for this content key" from "the entry exists but its file is not in
    /// the local store".
    pub fn ekeys_per_manifest(&self, ckey: &[u8; 16]) -> Result<Vec<Vec<[u8; 16]>>> {
        self.ensure_encoding()?;
        let borrowed = self.encoding.borrow();
        let blobs = borrowed.as_ref().expect("just loaded");
        let mut out = Vec::new();
        for blob in blobs {
            out.push(EncodingFile::parse(blob)?.ekeys_for_ckey(ckey));
        }
        Ok(out)
    }

    /// Decode a content key: encoding file to encoding key to record.
    pub fn open_ckey(&self, ckey: &[u8; 16]) -> Result<Vec<u8>> {
        self.ensure_encoding()?;
        // Borrow rather than clone: the decoded encoding manifests are ~100 MB
        // here, and a bundle index opens tens of thousands of assets.
        let blobs = self.encoding.borrow();
        let blobs = blobs.as_ref().expect("just loaded");
        self.open_ckey_using(ckey, blobs)
    }

    /// Resolve a content key against an explicit set of decoded encoding
    /// manifests. Separate from [`Store::open_ckey`] so that loading the
    /// encoding manifests themselves can resolve one half through the other
    /// without re-entering `ensure_encoding` (which recursed until the stack
    /// overflowed, when this was one function).
    fn open_ckey_using(&self, ckey: &[u8; 16], blobs: &[Vec<u8>]) -> Result<Vec<u8>> {
        let mut had_ekey = false;
        let mut last = None;
        for blob in blobs {
            let encoding = EncodingFile::parse(blob)?;
            let ekeys = encoding.ekeys_for_ckey(ckey);
            if ekeys.is_empty() {
                continue;
            }
            had_ekey = true;
            for ekey in &ekeys {
                match self.open_ekey(ekey) {
                    Ok(data) => return Ok(data),
                    Err(e) => last = Some(e),
                }
            }
        }
        if !had_ekey {
            return Err(Error::KeyNotFound(hex(ckey)));
        }
        Err(last.unwrap_or(Error::KeyNotFound(hex(ckey))))
    }

    /// The root file, a text list of the installation's files.
    ///
    /// One entry per line:
    /// `#FILEID | MD5 | CHUNK_ID | PRIORITY | MPRIORITY | FILENAME | INSTALLPATH`
    pub fn root_file(&self) -> Result<String> {
        let ckey = self
            .install
            .build_config
            .get_str("root")
            .map_err(|e| Error::Parse(e.to_string()))?;
        let ckey = parse_hex_key(ckey)?;
        let bytes = self.open_ckey(&ckey)?;
        String::from_utf8(bytes).map_err(|e| Error::Parse(format!("root file is not utf-8: {e}")))
    }

    /// Declared decoded size for a content key, if any encoding manifest has
    /// it.
    ///
    /// This is the ckey record's `u32`, the same value `FUN_142498700` writes
    /// to the resolve struct's `+0x10`. It is **not** the ekey record's tail;
    /// that is the smaller stored size (`EncodingFile::stored_size`).
    pub fn decoded_size(&self, ckey: &[u8; 16]) -> Result<Option<u64>> {
        self.ensure_encoding()?;
        let borrowed = self.encoding.borrow();
        let blobs = borrowed.as_ref().expect("just loaded");
        for blob in blobs {
            if let Some(size) = EncodingFile::parse(blob)?.decoded_size(ckey) {
                return Ok(Some(size));
            }
        }
        Ok(None)
    }

    /// Declared stored size for an encoding key, if any encoding manifest has
    /// it. This is the ekey record's 5-byte tail read as the client's 32-bit
    /// value.
    pub fn stored_size(&self, ekey: &[u8; 16]) -> Result<Option<u64>> {
        self.ensure_encoding()?;
        let borrowed = self.encoding.borrow();
        let blobs = borrowed.as_ref().expect("just loaded");
        for blob in blobs {
            if let Some(size) = EncodingFile::parse(blob)?.stored_size(ekey) {
                return Ok(Some(size));
            }
        }
        Ok(None)
    }
}

/// Which route the CDN would use for a key the local store cannot resolve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CdnRoute {
    /// An archive slice: Range GET on `data/…/{archive_hash}`.
    Group {
        archive_hash: String,
        offset: u64,
        size: u32,
        patch: bool,
    },
    /// A whole loose file under its own key.
    Loose { size: u32 },
}

impl std::fmt::Display for CdnRoute {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CdnRoute::Group {
                archive_hash,
                offset,
                size,
                patch,
            } => write!(
                f,
                "{}archive {} at {:#x}, {size} B — Range GET on data/…",
                if *patch { "patch " } else { "" },
                &archive_hash[..archive_hash.len().min(12)],
                offset
            ),
            CdnRoute::Loose { size } => write!(f, "loose file, {size} B — whole GET"),
        }
    }
}

/// The 20-byte CDN index footer: a 12-byte header and the header's own
/// 8-byte MD5.
pub const INDEX_FOOTER_SIZE: usize = 20;

/// The client's derived footer fields (the 0x30-byte output of
/// `0x1424d9af0`). All offsets are bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexFooter {
    pub version: u8,
    pub page_size: u32,
    pub offset_bytes: u8,
    pub size_bytes: u8,
    pub key_bytes: u8,
    pub checksum_size: u8,
    pub element_count: u32,
    pub entries_per_page: u32,
    pub page_count: u32,
    /// `page_count * key_bytes`: the per-page last-key array.
    pub last_key_bytes: u32,
    /// `page_count * checksum_size`: the per-page checksum array.
    pub checksum_bytes: u32,
}

impl IndexFooter {
    /// The client's footer parse, `0x1424d9af0`, operation for operation.
    ///
    /// `footer` is the file's last 20 bytes and `total_len` is the whole file's
    /// size (the `param_3` the client compares against the computed length).
    /// The leaf's output is a 0x30-byte buffer: the 20 footer bytes at
    /// `0..0x14`, then `pageSize` at `0x14`, `entrySize` at `0x18`,
    /// `entriesPerPage` at `0x1c`, `pageCount` at `0x20`, `lastKeyBytes` at
    /// `0x24` and `checksumBytes` at `0x28`. The leaf returns 0 in `AL` for an
    /// accepted footer and 8 for a rejection; this wrapper maps 8 to an error.
    ///
    /// Order and side effects copied from the bytes:
    /// 1. copy the last 20 bytes to `out`, remember `out[0xc..0x14]`;
    /// 2. zero `out[0xc..0x14]`, MD5 the 20 bytes, compare the first 8 with the
    ///    remembered hash; a mismatch returns 8 and leaves the hash zeroed;
    /// 3. restore the hash, accept version 0/1 only, checksum size 0..=16;
    /// 4. `pageSize = out[3] << 10`,
    ///    `entrySize = out[4] + out[5] + out[6]`; either zero returns 8;
    /// 5. `entriesPerPage = pageSize / entrySize`,
    ///    `pageCount = (elementCount - 1 + entriesPerPage) / entriesPerPage`;
    /// 6. `lastKeyBytes = pageCount * keyBytes`,
    ///    `checksumBytes = pageCount * checksumSize`;
    /// 7. the file must be exactly
    ///    `checksumSize + 20 + (checksumSize + keyBytes + pageSize) * pageCount`
    ///    bytes, in 32-bit arithmetic; anything else returns 8.
    ///
    /// The client has no divide-by-zero guard after step 4: a page size smaller
    /// than the entry size would fault. This port returns 8 for that input
    /// instead of faulting.
    pub fn parse(footer: &[u8; INDEX_FOOTER_SIZE], total_len: u32) -> Result<IndexFooter> {
        let mut out = [0u8; 0x30];
        let status = parse_index_footer(footer, INDEX_FOOTER_SIZE, total_len, &mut out);
        if status != 0 {
            return Err(Error::Parse(format!(
                "index footer rejected (client status {status})"
            )));
        }
        Ok(IndexFooter::from_out(&out))
    }

    fn from_out(out: &[u8; 0x30]) -> IndexFooter {
        IndexFooter::from_descriptor(out).expect("the client's 0x30-byte block")
    }

    /// Decode the client's derived-footer block at its own offsets.
    ///
    /// `out` is the 0x30 bytes `CdnIndexFooterParse` (`0x1424d9af0`) writes and
    /// `CdnIndexPageChecksums` (`0x1424d9f20`) reads: the footer copy at
    /// `0..0x14`, then `pageSize` at `0x14`, `entrySize` at `0x18`,
    /// `entriesPerPage` at `0x1c`, `pageCount` at `0x20`, `lastKeyBytes` at
    /// `0x24` and `checksumBytes` at `0x28`. Only `0..0x2c` is read; a shorter
    /// slice is refused rather than padded.
    pub fn from_descriptor(out: &[u8]) -> Option<IndexFooter> {
        if out.len() < 0x2c {
            return None;
        }
        let dword = |at: usize| u32::from_le_bytes(out[at..at + 4].try_into().unwrap());
        Some(IndexFooter {
            version: out[0],
            offset_bytes: out[4],
            size_bytes: out[5],
            key_bytes: out[6],
            checksum_size: out[7],
            element_count: dword(8),
            page_size: dword(0x14),
            entries_per_page: dword(0x1c),
            page_count: dword(0x20),
            last_key_bytes: dword(0x24),
            checksum_bytes: dword(0x28),
        })
    }

    /// `out[4] + out[5] + out[6]`, the client's per-entry stride.
    pub fn entry_size(&self) -> u32 {
        u32::from(self.offset_bytes) + u32::from(self.size_bytes) + u32::from(self.key_bytes)
    }

    /// The bytes after the page bodies: last keys, checksums and the terminal
    /// checksum. `checksumSize + (keyBytes + checksumSize) * pageCount`.
    pub fn tail_len(&self) -> u32 {
        u32::from(self.checksum_size)
            .wrapping_add(self.last_key_bytes)
            .wrapping_add(self.checksum_bytes)
    }

    /// The whole-file length the client's footer check demands, in its 32-bit
    /// arithmetic.
    pub fn expected_len(&self) -> u32 {
        u32::from(self.checksum_size)
            .wrapping_add(INDEX_FOOTER_SIZE as u32)
            .wrapping_add(
                u32::from(self.checksum_size)
                    .wrapping_add(u32::from(self.key_bytes))
                    .wrapping_add(self.page_size)
                    .wrapping_mul(self.page_count),
            )
    }

    /// Page `page` starts here; a page body is always `page_size` bytes.
    pub fn page_offset(&self, page: u32) -> u64 {
        u64::from(page) * u64::from(self.page_size)
    }

    /// Offset of page `page`'s stored checksum, inside the checksum array.
    pub fn checksum_offset(&self, page: u32) -> u64 {
        u64::from(self.page_count) * u64::from(self.page_size)
            + u64::from(self.last_key_bytes)
            + u64::from(page) * u64::from(self.checksum_size)
    }

    /// Entries on `page`: a full page except the last, capped by the footer's
    /// element count. Mirrors the cap the client applies while scanning.
    pub fn page_entries(&self, page: u32) -> u32 {
        if page >= self.page_count {
            return 0;
        }
        let before = u64::from(self.entries_per_page) * u64::from(page);
        (u64::from(self.element_count)
            .saturating_sub(before)
            .min(u64::from(self.entries_per_page))) as u32
    }

    /// Decode the entries of one page body. The page body is the client's
    /// `pageSize` bytes; the first `page_entries(page)` records are used.
    ///
    /// Record order is key, size, archive ordinal, offset. The ordinal width
    /// is `offset_bytes - 4` (2 for the main group, 1 for the patch group, 0
    /// for a loose index); every value is big-endian. A loose record ends after
    /// its size.
    pub fn entries_in_page(&self, page: u32, body: &[u8]) -> Result<Vec<IndexEntry>> {
        if page >= self.page_count {
            return Err(Error::Parse(format!(
                "index page {page} out of range 0..{}",
                self.page_count
            )));
        }
        if self.key_bytes != 16
            || self.size_bytes != 4
            || !matches!(self.offset_bytes, 0 | 4 | 5 | 6)
        {
            return Err(Error::Parse(format!(
                "unsupported index entry geometry (key {}, size {}, offset {})",
                self.key_bytes, self.size_bytes, self.offset_bytes
            )));
        }
        let stride = self.entry_size() as usize;
        if stride == 0 || body.len() < self.page_size as usize {
            return Err(Error::Parse("short index page body".into()));
        }
        let ordinal_width = usize::from(self.offset_bytes.saturating_sub(4));
        let mut out = Vec::with_capacity(self.page_entries(page) as usize);
        for i in 0..self.page_entries(page) {
            let at = i as usize * stride;
            let entry = &body[at..at + stride];
            let key: [u8; 16] = entry[..16].try_into().unwrap();
            let size = u32::from_be_bytes(entry[16..20].try_into().unwrap());
            let mut archive = 0u16;
            for b in &entry[20..20 + ordinal_width] {
                archive = (archive << 8) | u16::from(*b);
            }
            let o = 20 + ordinal_width;
            let offset = if self.offset_bytes == 0 {
                0
            } else {
                u32::from_be_bytes(entry[o..o + 4].try_into().unwrap())
            };
            out.push(IndexEntry {
                key,
                size,
                archive,
                offset,
            });
        }
        Ok(out)
    }
}

/// One CDN index record decoded from a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexEntry {
    pub key: [u8; 16],
    /// Big-endian size, the record's payload length.
    pub size: u32,
    /// Group ordinal (main group u16, patch group u8); 0 for a loose index.
    pub archive: u16,
    /// Big-endian offset inside the archive.
    pub offset: u32,
}

/// `0x1424d9af0`, the pure footer leaf. See [`IndexFooter::parse`].
pub fn parse_index_footer(data: &[u8], len: usize, total_len: u32, out: &mut [u8; 0x30]) -> u8 {
    if len < INDEX_FOOTER_SIZE || data.len() < len {
        return 8;
    }
    let footer = &data[len - INDEX_FOOTER_SIZE..len];
    out[..INDEX_FOOTER_SIZE].copy_from_slice(footer);
    let stored_hash: [u8; 8] = footer[12..20].try_into().unwrap();
    out[12..20].fill(0);
    if blte::md5_digest(&out[..INDEX_FOOTER_SIZE])[..8] != stored_hash {
        return 8;
    }
    out[12..20].copy_from_slice(&stored_hash);
    if out[0] > 1 || out[7] > 16 {
        return 8;
    }
    let page_size = u32::from(out[3]) << 10;
    let entry_size = u32::from(out[4]) + u32::from(out[5]) + u32::from(out[6]);
    out[0x14..0x18].copy_from_slice(&page_size.to_le_bytes());
    out[0x18..0x1c].copy_from_slice(&entry_size.to_le_bytes());
    if page_size == 0 || entry_size == 0 {
        return 8;
    }
    let entries_per_page = page_size / entry_size;
    if entries_per_page == 0 {
        // The client divides by this value and would fault; no real file has
        // an entry larger than a page.
        return 8;
    }
    out[0x1c..0x20].copy_from_slice(&entries_per_page.to_le_bytes());
    let elements = u32::from_le_bytes(out[8..12].try_into().unwrap());
    let page_count = elements.wrapping_sub(1).wrapping_add(entries_per_page) / entries_per_page;
    out[0x20..0x24].copy_from_slice(&page_count.to_le_bytes());
    let last_key_bytes = page_count.wrapping_mul(u32::from(out[6]));
    out[0x24..0x28].copy_from_slice(&last_key_bytes.to_le_bytes());
    let checksum_bytes = page_count.wrapping_mul(u32::from(out[7]));
    out[0x28..0x2c].copy_from_slice(&checksum_bytes.to_le_bytes());
    let expected = u32::from(out[7])
        .wrapping_add(INDEX_FOOTER_SIZE as u32)
        .wrapping_add(
            u32::from(out[7])
                .wrapping_add(u32::from(out[6]))
                .wrapping_add(page_size)
                .wrapping_mul(page_count),
        );
    if expected != total_len {
        return 8;
    }
    0
}

/// The page index a miss reports: the client's `CdnIndexPageSelect` leaves no
/// page and its callers test `page == pageCount` (and `page < 0`).
pub const PAGE_MISS: u32 = u32::MAX;

/// The client's page selection: `CdnIndexPageSelect` (`0x1424da730`) up to the
/// resident call, read operation for operation.
///
/// `last_keys` is the file's per-page last-key array (`pageCount * keyBytes`
/// bytes), `key` the query and `key_len` its length, compared over
/// `min(keyBytes, keyLen)` bytes — the clamp the client computes once, before
/// the loop, and not the record scans' own clamp. The walk is a lower bound
/// with an early equal exit:
///
/// ```text
/// lo = 0; hi = pageCount; page = 0
/// while (lo != hi) {
///   mid = (hi + lo) / 2                    // 32-bit, truncating
///   cmp = memcmp(lastKeys + mid*keyBytes, key, cmpLen)
///   if (cmp < 0) lo = mid + 1
///   else { hi = mid; if (cmp == 0) { page = mid; break; } }
///   page = lo
/// }
/// ```
///
/// The client then returns status 6 when `page == pageCount` (which also covers
/// `pageCount == 0`, where the loop does not run and `page` stays 0) or when
/// `page < 0`; this function reports those as [`PAGE_MISS`]. The equal exit is
/// an early exit, not a pure lower bound: with equal adjacent last keys it
/// answers a later page than `lo` alone would. A miss is `pageCount`, so the
/// early-exit page is never confused with it.
///
/// The client's arithmetic is 32-bit signed (`CDQ`/`SAR` for the midpoint,
/// `IMUL` for the stride, `MOVSXD` for the record address). For every index an
/// arena or a file can hold, `pageCount * keyBytes` is far below `0x7fff_ffff`,
/// so the midpoint and the stride are non-negative and the unsigned arithmetic
/// here is the same value — the sign-extension the client performs is the
/// identity on them.
pub fn select_page(
    last_keys: &[u8],
    page_count: u32,
    key_bytes: u8,
    key: &[u8],
    key_len: u8,
) -> u32 {
    let cmp_len = usize::from(key_bytes.min(key_len));
    let stride = u32::from(key_bytes);
    let (mut lo, mut hi) = (0u32, page_count);
    let mut page = 0u32;
    while lo != hi {
        let mid = lo + (hi - lo) / 2;
        let at = mid.wrapping_mul(stride) as usize;
        let cmp = last_keys[at..at + cmp_len].cmp(&key[..cmp_len]);
        if cmp == std::cmp::Ordering::Less {
            lo = mid + 1;
        } else {
            hi = mid;
            if cmp == std::cmp::Ordering::Equal {
                page = mid;
                break;
            }
        }
        page = lo;
    }
    if page < page_count {
        page
    } else {
        PAGE_MISS
    }
}

fn u32_le(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().expect("4 bytes"))
}

fn u64_le(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().expect("8 bytes"))
}

fn put_u64_le(b: &mut [u8], at: usize, value: u64) {
    b[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

/// One page resident, as `CdnIndexPageResident` (`0x1424da590`) does it when
/// the index is a whole file in memory (`index[0x38] == 0`): the page pointer
/// at `+0x98` becomes `base + (page * pageSize)` (32-bit multiply, adding the
/// zero-extended product) and the current page at `+0xa0` becomes `page`.
///
/// The streamed branch (`index[0x38] != 0`) needs the CDN reader object and its
/// lock - `0x142522870` / `0x142522380` behind an import thunk - so it is the
/// named open stage here and returns 1 without touching the object.
fn resident_page(index: &mut [u8], arena: u64, page: u32) -> u8 {
    if index[0x38] != 0 {
        return 1;
    }
    let page_size = u32_le(index, 0x6c);
    let base = u64_le(index, 0x50).wrapping_sub(arena);
    let at = base.wrapping_add(u64::from(page.wrapping_mul(page_size)));
    put_u64_le(index, 0x98, arena + at);
    index[0xa0..0xa4].copy_from_slice(&page.to_le_bytes());
    0
}

/// Compare `n` bytes at two offsets of the index blob with the client's
/// `memcmp` (`0x142efecf0`): negative below, positive above, zero equal.
fn cmp_at(index: &[u8], a: usize, b: usize, n: usize) -> i32 {
    bytes::client_memcmp(&index[a..a + n], &index[b..b + n], n) as i32
}

/// Zero the outputs of keys `from..count`, which is what the client's tail
/// does after the page cursor runs out: `out_locations[from..count]` (8 bytes
/// each) and `out_found[from..count]`. The client splits this into two
/// `memset`s with an overlap test and a scalar fallback; the final bytes are
/// the same.
fn zero_batch_tail(out_locations: &mut [u8], out_found: &mut [u8], from: u32, count: u32) {
    let start = from as usize;
    let end = count as usize;
    if start >= end {
        return;
    }
    out_locations[start * 8..end * 8].fill(0);
    out_found[start..end].fill(0);
}

/// `CdnIndexPageSelectBatch` (`0x1424da910`): the batch form of the CDN index
/// page selection, read operation for operation.
///
/// `index` is the client's CDN-index object as a byte view; `keys[i]` is the
/// offset of the `i`-th query key **inside** that blob (the oracle's pointer
/// array holds absolute arena addresses; the probe turns them into offsets),
/// and `arena` converts the object's absolute pointers (`+0x50`, `+0x88`, and
/// the `+0x98` the resident step writes) back to offsets. `out_locations`
/// carries `u64` values at `i * 8`, `out_found` one byte per key.
///
/// The object fields the client reads: `+0x38` the whole-file flag, `+0x50`
/// the whole-file base, `+0x5c` the location width in bytes, `+0x5d` the
/// location offset from the key's end, `+0x5e` the key width, `+0x60` the total
/// entry count, `+0x6c` the page size, `+0x70` the entry stride, `+0x74` the
/// entries per page, `+0x78` the page count and `+0x88` the last-key array.
///
/// The page cursor is monotonic across keys: a key is compared with the
/// current page's last key while it orders above it, then the page is made
/// resident and its records are scanned from the record cursor, which is *not*
/// reset on a hit (so a repeated key resumes one record later). `memcmp(entry,
/// key)` below zero continues the scan, above zero is a miss, equal is the hit
/// and its location is decoded from `[0x5c]` big-endian bytes at `entry +
/// keyBytes + [0x5d]`, storing the partial value on every digit. A miss zeroes
/// that key's outputs; when the cursor runs past the last page, the remaining
/// keys are zeroed in one sweep.
///
/// Returns 0, or the page-resident error: 1 is the whole-file read failure and
/// the **open stage** (the streamed branch is not modelled).
pub fn select_page_batch(
    index: &mut [u8],
    keys: &[usize],
    key_len: u8,
    count: u32,
    out_locations: &mut [u8],
    out_found: &mut [u8],
    arena: u64,
) -> u8 {
    if count == 0 {
        return 0;
    }
    let key_bytes = index[0x5e];
    let loc_bytes = index[0x5c];
    let loc_off = index[0x5d];
    let cmp_len = usize::from(key_bytes.min(key_len));
    let total = u32_le(index, 0x60);
    let entry_stride = u32_le(index, 0x70) as usize;
    let per_page = u32_le(index, 0x74);
    let page_count = u32_le(index, 0x78);
    let last_keys = u64_le(index, 0x88).wrapping_sub(arena) as usize;
    let last_key_at = |page: u32| last_keys + usize::from(key_bytes) * page as usize;

    let mut key_index: u32 = 0;
    let mut page: u32 = 0;
    let mut record: u32;
    let mut in_page: u32;
    if page_count == 0 {
        zero_batch_tail(out_locations, out_found, key_index, count);
        return 0;
    }
    'keys: loop {
        // The page cursor: advance while the key orders above the page's last
        // key (the client's `memcmp(key, lastKey, cmpLen); jg`).
        if cmp_at(index, keys[key_index as usize], last_key_at(page), cmp_len) > 0 {
            page = page.wrapping_add(1);
            if page >= page_count {
                zero_batch_tail(out_locations, out_found, key_index, count);
                return 0;
            }
            continue 'keys;
        }
        let error = resident_page(index, arena, page);
        if error != 0 {
            return error;
        }
        let start = per_page.wrapping_mul(page);
        let remaining = total.wrapping_sub(start);
        in_page = if per_page < remaining {
            per_page
        } else {
            remaining
        };
        if in_page == 0 {
            zero_batch_tail(out_locations, out_found, key_index, count);
            return 0;
        }
        record = 0;
        loop {
            if record >= in_page {
                zero_batch_tail(out_locations, out_found, key_index, count);
                return 0;
            }
            let resident = u64_le(index, 0x98).wrapping_sub(arena) as usize;
            let entry = resident + record as usize * entry_stride;
            let cmp = cmp_at(index, entry, keys[key_index as usize], cmp_len);
            if cmp < 0 {
                record = record.wrapping_add(1);
                continue;
            }
            if cmp > 0 {
                put_u64_le(out_locations, key_index as usize * 8, 0);
                out_found[key_index as usize] = 0;
            } else {
                out_found[key_index as usize] = 1;
                put_u64_le(out_locations, key_index as usize * 8, 0);
                let mut at = entry + usize::from(key_bytes) + usize::from(loc_off);
                let mut left = loc_bytes.wrapping_sub(1);
                if (left as i8) >= 0 {
                    let mut acc: u64 = 0;
                    loop {
                        let byte = index[at];
                        at += 1;
                        let value = (u64::from(byte) << ((left as u32) << 3)) | acc;
                        put_u64_le(out_locations, key_index as usize * 8, value);
                        acc = value;
                        left = left.wrapping_sub(1);
                        if (left as i8) < 0 {
                            break;
                        }
                    }
                }
                // The hit advances the record cursor (0x1424daa6e); a miss
                // above the key does not (0x1424daa70 is reached directly).
                record = record.wrapping_add(1);
            }
            key_index = key_index.wrapping_add(1);
            if key_index == count {
                return 0;
            }
            // The next key's pre-check against the same page: above its last
            // key means the page cursor moves; otherwise the record scan
            // continues from where the previous key left it.
            if cmp_at(index, keys[key_index as usize], last_key_at(page), cmp_len) > 0 {
                page = page.wrapping_add(1);
                if page >= page_count {
                    zero_batch_tail(out_locations, out_found, key_index, count);
                    return 0;
                }
                continue 'keys;
            }
        }
    }
}

/// `MD5(tail[..n - checksumSize])[..checksumSize] == tail[n - checksumSize..]`.
///
/// The client's terminal check (`0x1424d9e71`): it hashes the last-key array
/// plus the per-page checksums and compares the digest's first `checksumSize`
/// bytes with the terminal bytes that follow them. `tail` must be that region
/// **including** the terminal; a tail shorter than the checksum is refused
/// (the client would read before it).
fn terminal_checksum_matches(tail: &[u8], checksum_size: usize) -> bool {
    let Some(at) = tail.len().checked_sub(checksum_size) else {
        return false;
    };
    blte::md5_digest(&tail[..at])[..checksum_size] == tail[at..]
}

/// `MD5(terminal + footer)[..16] == key`.
///
/// The client's own-key check (`0x1424d9ed1`): the 16-byte key the index is
/// named by must be the MD5 of the terminal checksum followed by the 20 footer
/// bytes, compared as two 64-bit values.
fn key_checksum_matches(terminal: &[u8], footer: &[u8], key: &[u8; 16]) -> bool {
    let mut hashed = Vec::with_capacity(terminal.len() + footer.len());
    hashed.extend_from_slice(terminal);
    hashed.extend_from_slice(footer);
    blte::md5_digest(&hashed)[..16] == *key
}

/// The client's whole-file CDN index validation: `CdnIndexFileParse`
/// (`0x1424d9c90`), operation for operation.
///
/// `key` is the index's own 16-byte key (its file name in `indices/`), `data`
/// the file, `len` the declared length and `out` the 0x30-byte derived-footer
/// block. The status is the client's `AL`: 0 accepted, 8 rejected, and whatever
/// [`verify_page_checksums`] returns for the last stage (0, 3 or 8).
///
/// The order is the client's:
///
/// 1. [`parse_index_footer`] - copy the last 20 bytes into `out`, check the
///    header MD5 against `MD5(header[..12] + 8 zeros)[..8]`, restore the hash,
///    accept version 0/1 and `checksumSize <= 16`, derive the geometry
///    (`pageSize = footer[3] << 10`, `entrySize = footer[4]+[5]+[6]`,
///    `entriesPerPage`, `pageCount`, `lastKeyBytes`, `checksumBytes`) and
///    require `len == checksumSize + 20 + (checksumSize+keyBytes+pageSize) *
///    pageCount` in 32-bit arithmetic. A header-MD5 rejection leaves the
///    hash bytes in `out` zeroed, exactly as the client does.
/// 2. The **terminal** check: `MD5(last keys + checksums)[..checksumSize]` must
///    equal the terminal bytes that follow them.
/// 3. The **key** check: `MD5(terminal + footer)[..16]` must equal `key`.
/// 4. The **page** pass: [`verify_page_checksums`]'s status is returned
///    unchanged, so a page mismatch is the file's status, not a re-wrap.
///
/// The `bodies`, `tail` and terminal offsets are the client's: `pageCount *
/// pageSize` and `lastKeyBytes + checksumBytes` are 32-bit products (they
/// wrap), then added in 64 bits. A descriptor whose products wrap describes a
/// file the client would read outside its buffer; this port needs `data` to
/// cover `len` and will panic rather than read past it (a fixture never does).
pub fn parse_index_file(key: &[u8; 16], data: &[u8], len: u32, out: &mut [u8; 0x30]) -> u8 {
    if data.len() < len as usize {
        // The client reads `len` bytes from the pointer it was given; a
        // shorter buffer is a caller bug, not an input the client handles.
        return 8;
    }
    let status = parse_index_footer(data, len as usize, len, out);
    if status != 0 {
        return status;
    }
    let footer = IndexFooter::from_descriptor(out).expect("parse_index_footer fills 0x2c bytes");
    let bodies = footer.page_count.wrapping_mul(footer.page_size);
    let tail = footer.last_key_bytes.wrapping_add(footer.checksum_bytes);
    let tail_start = bodies as usize;
    let terminal_at = tail_start + tail as usize;
    let checksum_size = usize::from(footer.checksum_size);
    if !terminal_checksum_matches(
        &data[tail_start..terminal_at + checksum_size],
        checksum_size,
    ) {
        return 8;
    }
    let terminal = &data[terminal_at..terminal_at + checksum_size];
    if !key_checksum_matches(terminal, &out[..INDEX_FOOTER_SIZE], key) {
        return 8;
    }
    verify_page_checksums(data, len, &footer)
}

/// The page-wise MD5 verification of a CDN index: `CdnIndexPageChecksums`
/// (`0x1424d9f20`), read operation for operation.
///
/// `file` is the whole index file (or at least everything this call reads),
/// `len` the length the caller declares, and `footer` the derived-footer block
/// [`IndexFooter::from_descriptor`] decodes. The status byte is the client's
/// `AL`: 0 every page matched, 3 `len` shorter than the page bodies, 8 a
/// stored checksum disagreeing with its page.
///
/// The client's shape, from the disassembly:
///
/// 1. `bodies = pageCount * pageSize` in **32-bit** arithmetic
///    (`imul r8d, dword ptr [r8 + 0x14]`, `0x1424d9f5e`); `len < bodies`
///    returns 3 (`0x1424d9f66 jae` falls through to `mov al, 3`).
/// 2. The checksum array starts at `file + lastKeyBytes + bodies`: both fields
///    are u32, zero-extended and added in 64 bits (`0x1424d9f6c..0x1424d9f77`).
/// 3. `pageCount == 0` returns 0 (`0x1424d9f7d je`).
/// 4. Page `p` hashes `file + (p * pageSize) as u32` over `pageSize` bytes and
///    compares the digest with
///    `file + lastKeyBytes + bodies + (p * checksumSize) as u32` over
///    `checksumSize` bytes; a mismatch returns 8.
/// 5. All pages matching returns 0.
///
/// Both strides wrap in 32 bits, as the client's `imul`s do, which is why the
/// offsets are computed here rather than through
/// [`IndexFooter::checksum_offset`] (that one is the 64-bit form, for a file
/// that fits). `checksumSize` is used unbounded: the client's caller
/// `CdnIndexFileParse` (`0x1424d9c90`) rejects a value above 16 before this
/// function can be reached with a real descriptor.
///
/// The streaming diagnostic reader in [`inspect_index`] keeps its per-page
/// comparison; this whole-file form is the client's own check, and
/// `fn/CdnIndexPageChecksums` proves it.
pub fn verify_page_checksums(file: &[u8], len: u32, footer: &IndexFooter) -> u8 {
    let bodies = footer.page_count.wrapping_mul(footer.page_size);
    if len < bodies {
        return 3;
    }
    if footer.page_count == 0 {
        return 0;
    }
    let checksum_size = usize::from(footer.checksum_size);
    let checksums = footer.last_key_bytes as usize + bodies as usize;
    let page_size = footer.page_size as usize;
    let mut ctx = [0u8; blte::MD5_CTX_SIZE];
    let mut digest = [0u8; 16];
    for page in 0..footer.page_count {
        let at = page.wrapping_mul(footer.page_size) as usize;
        blte::md5_init(&mut ctx);
        blte::md5_update(&mut ctx, &file[at..at + page_size]);
        blte::md5_final(&mut ctx, &mut digest);
        let stored = checksums + page.wrapping_mul(u32::from(footer.checksum_size)) as usize;
        // `memcmp` with a zero length compares nothing and never dereferences
        // either pointer, so the client returns 0 for a `checksumSize` of 0
        // even where the array's address is past the buffer. The guard keeps
        // that case from needing the bytes.
        if checksum_size != 0 && file[stored..stored + checksum_size] != digest[..checksum_size] {
            return 8;
        }
    }
    0
}

/// One CDN index record as `CdnIndexPageSelect` decodes it: the client's
/// `*out_size`, `*out_offset` and 16-byte key copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageRecord {
    /// Big-endian size over `size_bytes` bytes, accumulated in 32 bits.
    pub size: u32,
    /// Big-endian offset over `offset_bytes` bytes, accumulated in 64 bits. For
    /// a group index the field is the archive ordinal followed by the
    /// in-archive offset, so the ordinal is `offset >> 32` and the offset its
    /// low 32 bits.
    pub offset: u64,
    /// `min(keyBytes, 16)` bytes of the *record's* key, zero-filled to 16.
    pub key16: [u8; 16],
}

/// The client's page scan and record decode: the tail of `CdnIndexPageSelect`
/// (`0x1424da730`) after the resident call, read operation for operation.
///
/// `page_base` is the page the resident call made current — in whole-file mode
/// `pageBodies + page*pageSize`, which the caller has already resolved into a
/// slice. The scan is capped by the footer's element count so a partly-filled
/// last page does not read padding:
///
/// ```text
/// count = min(entriesPerPage, elementCount - entriesPerPage*page)   // 32-bit
/// for i in 0..count:
///   rec = pageBase + (u32)(entrySize*i)
///   if memcmp(rec, key, min(keyBytes,keyLen)) == 0 -> found
/// ```
///
/// The first record whose prefix matches wins; there is no equal-range
/// expansion. On a hit the client copies `min(keyBytes,16)` bytes of the
/// **record** (not of the query) into `out_key16` and zero-fills the rest, then
/// writes the size and the offset. Returns `None` on a miss, and also when the
/// supplied page body is shorter than the records the footer describes — the
/// client reads past such a body; this port refuses instead of panicking.
pub fn decode_page_record(
    footer: &IndexFooter,
    page: u32,
    page_base: &[u8],
    key: &[u8],
    key_len: u8,
) -> Option<PageRecord> {
    let key_bytes = footer.key_bytes;
    let cmp_len = usize::from(key_bytes.min(key_len));
    let copy_len = usize::from(key_bytes.min(16));
    let need = cmp_len.max(copy_len);
    // The client computes `elementCount - entriesPerPage * page` in 32-bit
    // wrapping arithmetic and takes the unsigned minimum; a page past the
    // element count therefore scans the full page, and one with no elements
    // left scans none.
    let remaining = footer
        .element_count
        .wrapping_sub(footer.entries_per_page.wrapping_mul(page));
    let count = footer.entries_per_page.min(remaining);
    let entry_size = footer.entry_size();
    for i in 0..count {
        let at = entry_size.wrapping_mul(i) as usize;
        let rec = page_base.get(at..)?;
        if rec.len() < need {
            return None;
        }
        if rec[..cmp_len] != key[..cmp_len] {
            continue;
        }
        let mut key16 = [0u8; 16];
        key16[..copy_len].copy_from_slice(&rec[..copy_len]);
        let field = &rec[copy_len..];
        let mut size = 0u32;
        for k in 0..footer.size_bytes {
            let shift = (u32::from(footer.size_bytes - 1 - k) * 8) & 63;
            let byte = *field.get(usize::from(k))?;
            size |= (u64::from(byte) << shift) as u32;
        }
        let after_size = field.get(usize::from(footer.size_bytes)..).unwrap_or(&[]);
        let mut offset = 0u64;
        for k in 0..footer.offset_bytes {
            let shift = (u32::from(footer.offset_bytes - 1 - k) * 8) & 63;
            let byte = *after_size.get(usize::from(k))?;
            offset |= u64::from(byte) << shift;
        }
        return Some(PageRecord {
            size,
            offset,
            key16,
        });
    }
    None
}

/// One `indices/<hash>.index`, opened by the client's own layout.
///
/// The client's handler reads the last 20 bytes first, parses the footer with
/// `0x1424d9af0`, then reads the tail that begins after the page bodies:
///
/// ```text
/// page bodies     page p at p * pageSize, pageSize bytes each
/// last keys       pageCount * keyBytes, the last key of each page
/// checksums       pageCount * checksumSize, MD5(page body)[..checksumSize]
/// terminal        checksumSize bytes, MD5(last keys + checksums)
/// footer          20 bytes: header 12 + MD5(header 12 + 8 zero bytes)[..8]
/// ```
///
/// The full-file validator `0x1424d9c90` checks the terminal checksum and then
/// MD5(terminal + footer) against the index's own 16-byte key (the file name).
/// This type performs those checks; `open` skips only the key comparison, for
/// synthetic files whose names are not keys.
#[derive(Debug)]
struct PageIndex {
    file: fs::File,
    footer: IndexFooter,
    /// `page_count * key_bytes` bytes of per-page last keys.
    last_keys: Vec<u8>,
    /// `page_count * checksum_size` bytes of per-page MD5s.
    checksums: Vec<u8>,
}

impl PageIndex {
    #[cfg(test)]
    fn open(path: &Path) -> Result<PageIndex> {
        PageIndex::open_with_key(path, None)
    }

    fn open_with_key(path: &Path, key: Option<&[u8; 16]>) -> Result<PageIndex> {
        let mut file = fs::File::open(path)?;
        let len = file.metadata()?.len();
        if len < INDEX_FOOTER_SIZE as u64 {
            return Err(Error::Parse(format!(
                "{}: index too small ({len} B)",
                path.display()
            )));
        }
        let Ok(total_len) = u32::try_from(len) else {
            return Err(Error::Parse(format!(
                "{}: index is larger than the client's 32-bit length",
                path.display()
            )));
        };
        let mut footer_bytes = [0u8; INDEX_FOOTER_SIZE];
        file.seek(SeekFrom::End(-(INDEX_FOOTER_SIZE as i64)))?;
        file.read_exact(&mut footer_bytes)?;
        let footer = IndexFooter::parse(&footer_bytes, total_len)
            .map_err(|e| Error::Parse(format!("{}: {e}", path.display())))?;

        // The entry geometry the page decoder relies on. The client's footer
        // check (step 4 above) only requires a non-zero entry size, then the
        // decoder reads a fixed 16-byte key, a fixed 4-byte size and an
        // offset whose ordinal width is `offset - 4`. `offset_bytes` 1..=3
        // would make the decoder read four bytes past a 21..23-byte stride.
        // The client's selector (`CdnIndexPageSelect`) does support any
        // `key_bytes` (it strides the last-key array by it), but
        // `entries_in_page` decodes a 16-byte key, so this port refuses a
        // width other than 16 rather than mix the two. The client faults on
        // such a file; this port refuses it.
        if footer.key_bytes != 16
            || footer.size_bytes != 4
            || !matches!(footer.offset_bytes, 0 | 4 | 5 | 6)
        {
            return Err(Error::Parse(format!(
                "{}: unsupported index entry geometry (key {}, size {}, offset {})",
                path.display(),
                footer.key_bytes,
                footer.size_bytes,
                footer.offset_bytes
            )));
        }

        // The footer's length check runs in the client's 32-bit arithmetic, so
        // a wrapping `tail_len` can describe a tail the file cannot hold. The
        // client allocates that length and reads; this port refuses it before
        // the allocation.
        let exact_tail = u64::from(footer.checksum_size)
            + u64::from(footer.last_key_bytes)
            + u64::from(footer.checksum_bytes);
        let tail_len = u64::from(footer.tail_len());
        if exact_tail != tail_len {
            return Err(Error::Parse(format!(
                "{}: tail length wrapped in 32 bits ({} B exact, {tail_len} B declared)",
                path.display(),
                exact_tail
            )));
        }
        let tail_start = footer.page_offset(footer.page_count);
        if tail_start.saturating_add(tail_len) > u64::from(total_len) {
            return Err(Error::Parse(format!(
                "{}: {tail_len}-byte tail at {tail_start:#x} does not fit the {total_len}-byte file",
                path.display()
            )));
        }
        if tail_len < u64::from(footer.checksum_size) {
            return Err(Error::Parse(format!(
                "{}: tail shorter than its checksum",
                path.display()
            )));
        }
        let mut tail = vec![0u8; tail_len as usize];
        file.seek(SeekFrom::Start(tail_start))?;
        file.read_exact(&mut tail)?;
        let ck = footer.checksum_size as usize;
        if !terminal_checksum_matches(&tail, ck) {
            return Err(Error::Parse(format!(
                "{}: terminal checksum does not match the last keys and checksums",
                path.display()
            )));
        }
        if let Some(expected) = key {
            let terminal = &tail[tail_len as usize - ck..];
            if !key_checksum_matches(terminal, &footer_bytes, expected) {
                return Err(Error::Parse(format!(
                    "{}: index key does not match MD5(terminal + footer)",
                    path.display()
                )));
            }
        }
        let key_array = &tail[..footer.last_key_bytes as usize];
        let checksums = &tail[footer.last_key_bytes as usize
            ..footer.last_key_bytes as usize + footer.checksum_bytes as usize];
        Ok(PageIndex {
            file,
            footer,
            last_keys: key_array.to_vec(),
            checksums: checksums.to_vec(),
        })
    }

    /// First page whose last key is `>= key`: the client's page selection.
    ///
    /// `CdnIndexPageSelect` (`0x1424da730`) is [`select_page`], which carries
    /// the lower bound, its early equal exit, and the miss. `key_bytes` and
    /// the key length are both 16 in the supported geometry (`open_with_key`
    /// refuses anything else), so the compare clamp is a no-op today; it is
    /// kept so the comparison is the client's expression.
    fn find_page(&self, key: &[u8; 16]) -> Option<u32> {
        let page = select_page(
            &self.last_keys,
            self.footer.page_count,
            self.footer.key_bytes,
            key,
            16,
        );
        (page != PAGE_MISS).then_some(page)
    }

    /// Page `page`, always `pageSize` bytes, at the client's `page * pageSize`.
    fn read_page(&mut self, page: u32) -> Result<Vec<u8>> {
        if page >= self.footer.page_count {
            return Err(Error::Parse(format!(
                "index page {page} out of range 0..{}",
                self.footer.page_count
            )));
        }
        let mut data = vec![0u8; self.footer.page_size as usize];
        self.file
            .seek(SeekFrom::Start(self.footer.page_offset(page)))?;
        self.file.read_exact(&mut data)?;
        Ok(data)
    }

    /// Whether a page body's MD5 matches the stored per-page checksum. The
    /// client's full-file check `0x1424d9f20` compares every page and ignores
    /// a mismatch; this exposes the same comparison for a test.
    fn page_checksum_matches(&self, page: u32, body: &[u8]) -> bool {
        if page >= self.footer.page_count {
            return false;
        }
        let ck = self.footer.checksum_size as usize;
        let at = page as usize * ck;
        if at + ck > self.checksums.len() || body.len() < self.footer.page_size as usize {
            return false;
        }
        blte::md5_digest(&body[..self.footer.page_size as usize])[..ck]
            == self.checksums[at..at + ck]
    }

    /// Entries on a page, capped by the footer's element count so a
    /// partly-filled last page does not read zeros.
    #[cfg(test)]
    fn page_entry_count(&self, page: u32) -> u32 {
        self.footer.page_entries(page)
    }

    /// Find a key in a group index: `(archive ordinal, offset, size)`.
    ///
    /// The ordinal's width is what distinguishes the two group flavours:
    /// `offsetBytes = 6` is the main archive group (u16, up to 65 535
    /// archives), `offsetBytes = 5` is the patch group (u8, up to 255 — its
    /// measured geometry, not a reference constant).
    fn group_lookup(&mut self, key: &[u8; 16]) -> Result<Option<(u16, u64, u32)>> {
        if self.footer.offset_bytes != 6 && self.footer.offset_bytes != 5 {
            return Err(Error::Parse(format!(
                "not a group index (offsetBytes {})",
                self.footer.offset_bytes
            )));
        }
        let Some(page) = self.find_page(key) else {
            return Ok(None);
        };
        let data = self.read_page(page)?;
        // The 6-byte field is the u16 archive ordinal followed by the 32-bit
        // in-archive offset; the 5-byte one is the patch group's u8 ordinal.
        let Some(record) = decode_page_record(&self.footer, page, &data, key, 16) else {
            return Ok(None);
        };
        Ok(Some((
            (record.offset >> 32) as u16,
            record.offset & 0xffff_ffff,
            record.size,
        )))
    }

    /// Find a key in the loose-file index: its size.
    fn loose_lookup(&mut self, key: &[u8; 16]) -> Result<Option<u32>> {
        if self.footer.offset_bytes != 0 {
            return Err(Error::Parse(
                "not a loose-file index (offsetBytes != 0)".into(),
            ));
        }
        let Some(page) = self.find_page(key) else {
            return Ok(None);
        };
        let data = self.read_page(page)?;
        Ok(decode_page_record(&self.footer, page, &data, key, 16).map(|record| record.size))
    }
}

/// What one `indices/<hash>.index` holds, for the install test. Counts and
/// geometry only, never content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexStats {
    pub file_len: u64,
    pub offset_bytes: u8,
    pub size_bytes: u8,
    pub key_bytes: u8,
    pub checksum_size: u8,
    pub element_count: u32,
    pub page_size: u32,
    pub entry_size: u32,
    pub entries_per_page: u32,
    pub page_count: u32,
    /// Entries on page 0 and on the last page.
    pub first_page_entries: u32,
    pub last_page_entries: u32,
    /// First and last 16-byte last-key records.
    pub first_last_key: [u8; 16],
    pub final_last_key: [u8; 16],
    /// Every page body's MD5 matched its stored checksum.
    pub page_checksums_ok: bool,
}

/// Open one index with the client's footer and tail checks, read every page,
/// and report its geometry. `expected` is the index's 16-byte key, the value
/// the client compares against MD5(terminal + footer); pass `None` for a
/// synthetic file.
pub fn inspect_index(path: &Path, expected: Option<&[u8; 16]>) -> Result<IndexStats> {
    let mut index = PageIndex::open_with_key(path, expected)?;
    let footer = index.footer;
    let mut checksums_ok = true;
    for page in 0..footer.page_count {
        let body = index.read_page(page)?;
        if !index.page_checksum_matches(page, &body) {
            checksums_ok = false;
        }
    }
    let first_last_key = if index.last_keys.len() >= 16 {
        index.last_keys[..16].try_into().unwrap()
    } else {
        [0u8; 16]
    };
    let final_last_key = if index.last_keys.len() >= 16 {
        let at = index.last_keys.len() - 16;
        index.last_keys[at..].try_into().unwrap()
    } else {
        [0u8; 16]
    };
    Ok(IndexStats {
        file_len: index.file.metadata()?.len(),
        offset_bytes: footer.offset_bytes,
        size_bytes: footer.size_bytes,
        key_bytes: footer.key_bytes,
        checksum_size: footer.checksum_size,
        element_count: footer.element_count,
        page_size: footer.page_size,
        entry_size: footer.entry_size(),
        entries_per_page: footer.entries_per_page,
        page_count: footer.page_count,
        first_page_entries: footer.page_entries(0),
        last_page_entries: footer
            .page_count
            .checked_sub(1)
            .map_or(0, |page| footer.page_entries(page)),
        first_last_key,
        final_last_key,
        page_checksums_ok: checksums_ok,
    })
}

/// What the local store holds, for the `--index-stats` diagnostic. Counts,
/// never content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreStats {
    /// Encoding keys in the `data/` bucket tables.
    pub keys: usize,
    /// Bucket `.idx` files read.
    pub bucket_files: usize,
    /// `data/data.NNN` archives on disk.
    pub data_packs: usize,
    /// `indices/*.index` files on disk. These are **CDN routes**, not local
    /// locations: their archive ordinals and offsets do not name a local pack.
    pub cdn_indexes: usize,
}

/// Open (or keep open) one `data.NNN` pack.
///
/// The client's `DynamicContainerOpenArchive` (`0x14250c8f0`) keys its handle
/// cache by the archive number and never opens the same pack twice. `len` and
/// `offset` are archive-local.
fn open_archive<'a>(
    archives: &'a mut HashMap<u32, fs::File>,
    install: &Install,
    archive: u32,
) -> Result<&'a mut fs::File> {
    if let std::collections::hash_map::Entry::Vacant(entry) = archives.entry(archive) {
        let path = install.data_dir().join(index::archive_name(archive));
        entry.insert(fs::File::open(path)?);
    }
    Ok(archives.get_mut(&archive).expect("just inserted"))
}

/// `data.NNN` files in the store.
fn pack_count(dir: &Path) -> usize {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_str()
                .map(|n| n.starts_with("data.") && !n.ends_with(".idx"))
                .unwrap_or(false)
        })
        .count()
}

/// `*.index` files in `indices/`.
fn index_file_count(dir: &Path) -> usize {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("index"))
        .count()
}

fn parse_hex_key(hex: &str) -> Result<[u8; 16]> {
    // `is_ascii` before the two-byte slicing below: a key with a multi-byte
    // character is an error, not a panic.
    if hex.len() != 32 || !hex.is_ascii() {
        return Err(Error::Parse(format!("key {hex:?} is not 16 bytes")));
    }
    let mut out = [0u8; 16];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|_| Error::Parse(format!("key {hex:?} is not hex")))?;
    }
    Ok(out)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    const TEST_PAGE: usize = 1024;

    /// A synthetic index with the client's layout, built by the same formula:
    /// page bodies, the per-page last-key array, the per-page checksums, the
    /// terminal checksum, then the 20-byte footer. Entries are `[key 16]
    /// [size u32 BE]`, plus the ordinal and offset when `offset_bytes` is 5 or
    /// 6.
    fn build(entries: &[([u8; 16], u32, u16, u32)], offset_bytes: usize) -> Vec<u8> {
        build_with(entries, offset_bytes, 1)
    }

    fn build_with(
        entries: &[([u8; 16], u32, u16, u32)],
        offset_bytes: usize,
        version: u8,
    ) -> Vec<u8> {
        const CHECKSUM: usize = 8;
        let per_entry = 16 + 4 + offset_bytes;
        let epp = TEST_PAGE / per_entry;
        let pages = ((entries.len() + epp - 1).max(1) / epp).max(1);
        let mut out = Vec::new();
        for p in 0..pages {
            let mut page = vec![0u8; TEST_PAGE];
            for (i, (key, size, ord, off)) in entries.iter().enumerate().skip(p * epp).take(epp) {
                let at = (i % epp) * per_entry;
                page[at..at + 16].copy_from_slice(key);
                page[at + 16..at + 20].copy_from_slice(&size.to_be_bytes());
                match offset_bytes {
                    6 => {
                        page[at + 20..at + 22].copy_from_slice(&ord.to_be_bytes());
                        page[at + 22..at + 26].copy_from_slice(&off.to_be_bytes());
                    }
                    5 => {
                        page[at + 20] = *ord as u8;
                        page[at + 21..at + 25].copy_from_slice(&off.to_be_bytes());
                    }
                    _ => {}
                }
            }
            out.extend_from_slice(&page);
        }
        let page_end = out.len();
        for p in 0..pages {
            let last = entries
                .iter()
                .skip(p * epp)
                .take(epp)
                .next_back()
                .map(|e| e.0)
                .unwrap_or([0u8; 16]);
            out.extend_from_slice(&last);
        }
        let key_array = out[page_end..].to_vec();
        let mut checksums = Vec::with_capacity(pages * CHECKSUM);
        for page in out[..page_end].chunks_exact(TEST_PAGE) {
            checksums.extend_from_slice(&blte::md5_digest(page)[..CHECKSUM]);
        }
        out.extend_from_slice(&checksums);
        let mut tail = key_array;
        tail.extend_from_slice(&checksums);
        out.extend_from_slice(&blte::md5_digest(&tail)[..CHECKSUM]);
        let mut footer = [0u8; INDEX_FOOTER_SIZE];
        footer[0] = version;
        footer[3] = (TEST_PAGE / 1024) as u8;
        footer[4] = offset_bytes as u8;
        footer[5] = 4;
        footer[6] = 16;
        footer[7] = CHECKSUM as u8;
        footer[8..12].copy_from_slice(&(entries.len() as u32).to_le_bytes());
        let mut header = footer;
        header[12..20].fill(0);
        let hash = blte::md5_digest(&header);
        footer[12..20].copy_from_slice(&hash[..8]);
        out.extend_from_slice(&footer);
        out
    }

    fn key(n: u8) -> [u8; 16] {
        let mut k = [0u8; 16];
        k[0] = n;
        k
    }

    fn write_temp(name: &str, bytes: &[u8]) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("ow-casc-{}-{name}.index", std::process::id()));
        fs::write(&path, bytes).expect("write synthetic index");
        path
    }

    /// Recompute the footer's header MD5 after a test edits its header.
    fn resign(footer: &mut [u8; 20]) {
        let mut header = *footer;
        header[12..20].fill(0);
        let hash = blte::md5_digest(&header);
        footer[12..20].copy_from_slice(&hash[..8]);
    }

    /// More entries than fit one page: the binary search over the last-key
    /// array must find a key on the *second* page, and the page scan must stop
    /// at the footer's element count instead of reading zero padding.
    #[test]
    fn group_lookup_finds_across_page_boundaries() {
        let mut entries: Vec<([u8; 16], u32, u16, u32)> = Vec::new();
        for i in 0u8..80 {
            entries.push((
                key(i),
                100 + u32::from(i),
                700 + u16::from(i),
                0x1000 * u32::from(i),
            ));
        }
        let path = write_temp("group", &build(&entries, 6));
        let mut index = PageIndex::open(&path).expect("opens");
        assert!(
            index.footer.page_count >= 2,
            "synthetic file should span pages"
        );

        for i in [0u8, 1, 40, 78, 79] {
            let found = index.group_lookup(&key(i)).expect("lookup");
            let (ord, off, size) = found.unwrap_or_else(|| panic!("key {i} not found"));
            assert_eq!(ord, 700 + u16::from(i));
            assert_eq!(off, u64::from(0x1000 * u32::from(i)));
            assert_eq!(size, 100 + u32::from(i));
        }
        let absent = index.group_lookup(&key(200)).expect("lookup");
        assert!(absent.is_none(), "a key past the last entry must miss");
        fs::remove_file(&path).ok();
    }

    /// Duplicate last keys: the client's search returns the `mid` whose last
    /// key compares equal (the early exit), not the lowest equal page a pure
    /// lower bound would return. Two full pages of the same key pin it: the
    /// first probe is page 1, so page 1's first record is the answer.
    #[test]
    fn duplicate_last_keys_take_the_early_equal_page() {
        let entries: Vec<([u8; 16], u32, u16, u32)> =
            (0..78u32).map(|i| (key(7), 100 + i, 0u16, i)).collect();
        let path = write_temp("duplicate-last-key", &build(&entries, 6));
        let mut index = PageIndex::open(&path).expect("opens");
        assert_eq!(index.footer.page_count, 2, "two full pages of 39 records");
        assert_eq!(index.find_page(&key(7)), Some(1));
        let (ordinal, offset, size) = index.group_lookup(&key(7)).unwrap().unwrap();
        assert_eq!(ordinal, 0);
        assert_eq!(offset, 39, "page 1's first record, not page 0's");
        assert_eq!(size, 139);
        fs::remove_file(&path).ok();
    }

    /// The patch group's one-byte ordinal (`offsetBytes = 5`) parses too.
    #[test]
    fn patch_group_lookup_reads_the_narrow_ordinal() {
        let entries = vec![(key(1), 95, 81u16, 0xf5d9a), (key(2), 102, 70, 0x6f991e3)];
        let path = write_temp("patch", &build(&entries, 5));
        let mut index = PageIndex::open(&path).expect("opens");
        assert_eq!(
            index.group_lookup(&key(1)).expect("lookup"),
            Some((81, 0xf5d9a, 95))
        );
        assert_eq!(
            index.group_lookup(&key(2)).expect("lookup"),
            Some((70, 0x6f991e3, 102))
        );
        fs::remove_file(&path).ok();
    }

    /// A loose index (`offsetBytes = 0`) reports sizes, and the one-page
    /// count is the case the reference formula gets wrong: with the client's
    /// element-count formula a single-page file has `page_count == 1`, not 0.
    #[test]
    fn loose_lookup_and_single_page_count() {
        let entries = vec![(key(9), 109_878_111, 0u16, 0u32)];
        let path = write_temp("loose", &build(&entries, 0));
        let mut index = PageIndex::open(&path).expect("opens");
        assert_eq!(index.footer.page_count, 1);
        assert_eq!(index.footer.element_count, 1);
        assert_eq!(
            index.loose_lookup(&key(9)).expect("lookup"),
            Some(109_878_111)
        );
        assert_eq!(index.loose_lookup(&key(8)).expect("lookup"), None);
        fs::remove_file(&path).ok();
    }

    /// The wrong flavour is refused rather than misread.
    #[test]
    fn a_group_index_refuses_a_loose_lookup_and_vice_versa() {
        let path = write_temp("flavour", &build(&[(key(1), 5, 1, 7)], 6));
        let mut index = PageIndex::open(&path).expect("opens");
        let e = index.loose_lookup(&key(1)).expect_err("must refuse");
        assert!(e.to_string().contains("not a loose-file index"), "{e}");
        fs::remove_file(&path).ok();
    }

    /// The pure footer leaf on a well-formed synthetic file: the exact
    /// geometry, page count and output-byte layout of `0x1424d9af0`.
    #[test]
    fn footer_parse_matches_the_client_geometry() {
        let entries: Vec<_> = (0u8..80)
            .map(|i| (key(i), 100 + u32::from(i), 0, 0))
            .collect();
        let bytes = build(&entries, 0);
        let footer_bytes: [u8; 20] = bytes[bytes.len() - 20..].try_into().unwrap();
        let footer = IndexFooter::parse(&footer_bytes, bytes.len() as u32).expect("footer");
        assert_eq!(footer.version, 1);
        assert_eq!(footer.page_size, 1024);
        assert_eq!(footer.offset_bytes, 0);
        assert_eq!(footer.size_bytes, 4);
        assert_eq!(footer.key_bytes, 16);
        assert_eq!(footer.checksum_size, 8);
        assert_eq!(footer.element_count, 80);
        assert_eq!(footer.entry_size(), 20);
        assert_eq!(footer.entries_per_page, 51);
        assert_eq!(footer.page_count, 2);
        assert_eq!(footer.last_key_bytes, 32);
        assert_eq!(footer.checksum_bytes, 16);
        assert_eq!(footer.expected_len(), bytes.len() as u32);
        assert_eq!(footer.tail_len(), 32 + 16 + 8);
        assert_eq!(footer.page_entries(0), 51);
        assert_eq!(footer.page_entries(1), 29);
        assert_eq!(footer.page_entries(2), 0);
        assert_eq!(footer.page_offset(1), 1024);
    }

    /// Every footer rejection the client's leaf has, plus the two safety
    /// branches (short buffer, page smaller than an entry) the client would
    /// fault on.
    #[test]
    fn footer_rejections_follow_the_client_branches() {
        let entries = vec![(key(1), 5, 0u16, 0u32)];
        let bytes = build(&entries, 0);
        let len = bytes.len() as u32;
        let good: [u8; 20] = bytes[bytes.len() - 20..].try_into().unwrap();

        // Version 2 is the first rejected version; version 1 is the last
        // accepted one.
        let mut footer = good;
        footer[0] = 2;
        resign(&mut footer);
        assert!(IndexFooter::parse(&footer, len).is_err());

        // Checksum size 17 is the first rejected width; 16 is accepted.
        let mut footer = good;
        footer[7] = 17;
        resign(&mut footer);
        assert!(IndexFooter::parse(&footer, len).is_err());

        // Zero page size and zero entry size.
        let mut footer = good;
        footer[3] = 0;
        resign(&mut footer);
        assert!(IndexFooter::parse(&footer, len).is_err());

        let mut footer = good;
        footer[4] = 0;
        footer[5] = 0;
        footer[6] = 0;
        resign(&mut footer);
        assert!(IndexFooter::parse(&footer, len).is_err());

        // A bad header MD5 is the first branch; it leaves the hash zeroed in
        // the output buffer, as the client does.
        let mut footer = good;
        footer[12] ^= 1;
        let mut out = [0xA5u8; 0x30];
        assert_eq!(parse_index_footer(&footer, 20, len, &mut out), 8);
        assert_eq!(&out[12..20], &[0u8; 8]);

        // A file length one byte off fails the client's total-length gate.
        assert!(IndexFooter::parse(&good, len + 1).is_err());
        assert!(IndexFooter::parse(&good, len - 1).is_err());

        // A page size whose KiB byte is 0xff is legal upstream but this
        // file's stored page count then no longer matches the length.
        let mut footer = good;
        footer[3] = 0xff;
        resign(&mut footer);
        assert!(IndexFooter::parse(&footer, len).is_err());

        // Short input is a reject in this port (the client would read before
        // the buffer).
        let mut out = [0u8; 0x30];
        assert_eq!(parse_index_footer(&good, 19, len, &mut out), 8);
        assert_eq!(parse_index_footer(&[], 0, 0, &mut [0u8; 0x30]), 8);
    }

    /// A short file, a bad terminal checksum and a wrong index key are the
    /// three whole-file rejects not covered by the footer leaf alone.
    #[test]
    fn whole_file_rejects_short_terminal_and_wrong_key() {
        let entries = vec![(key(1), 5, 0u16, 0u32), (key(2), 6, 0, 0)];
        let good = build(&entries, 0);

        let short = write_temp("short", &good[..12]);
        let e = PageIndex::open(&short).expect_err("short file must reject");
        assert!(e.to_string().contains("too small"), "{e}");
        fs::remove_file(&short).ok();

        let mut bad_terminal = good.clone();
        let at = bad_terminal.len() - 20 - 8 - 1;
        bad_terminal[at] ^= 0x80;
        let path = write_temp("terminal", &bad_terminal);
        let e = PageIndex::open(&path).expect_err("terminal checksum must reject");
        assert!(e.to_string().contains("terminal checksum"), "{e}");
        fs::remove_file(&path).ok();

        // The real key relation: MD5(terminal + footer) is the file's key.
        let footer_bytes: [u8; 20] = good[good.len() - 20..].try_into().unwrap();
        let terminal = &good[good.len() - 28..good.len() - 20];
        let mut hashed = terminal.to_vec();
        hashed.extend_from_slice(&footer_bytes);
        let expected: [u8; 16] = blte::md5_digest(&hashed);
        let path = write_temp("key", &good);
        assert!(PageIndex::open_with_key(&path, Some(&expected)).is_ok());
        let wrong = [0u8; 16];
        let e = PageIndex::open_with_key(&path, Some(&wrong)).expect_err("wrong key must reject");
        assert!(e.to_string().contains("index key"), "{e}");
        fs::remove_file(&path).ok();
    }

    /// The page reader uses the client's `page * pageSize` and rejects an
    /// out-of-range page; a short body is an I/O short read, not zeros.
    #[test]
    fn page_reader_range_and_short_body() {
        let entries: Vec<_> = (0u8..80)
            .map(|i| (key(i), 100 + u32::from(i), 800 + u16::from(i), 7))
            .collect();
        let bytes = build(&entries, 6);
        let path = write_temp("page-range", &bytes);
        let mut index = PageIndex::open(&path).expect("opens");
        assert_eq!(index.footer.page_count, 3);
        let page1 = index.read_page(1).expect("page 1");
        assert_eq!(page1.len(), 1024);
        let decoded = index.footer.entries_in_page(1, &page1).expect("entries");
        assert_eq!(decoded.len(), 39);
        assert_eq!(decoded[0].key, key(39));
        assert_eq!(decoded[38].key, key(77));
        let e = index.read_page(3).expect_err("out of range");
        assert!(e.to_string().contains("out of range"), "{e}");

        // Truncate after opening: page 1's read is short.
        let file = fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("open");
        file.set_len(1024 + 8).expect("truncate");
        drop(file);
        let e = index.read_page(1).expect_err("short page");
        assert!(matches!(e, Error::Io(_)), "{e:?}");
        fs::remove_file(&path).ok();
    }

    /// The client's per-page MD5 check and the element-count cap.
    #[test]
    fn page_checksums_and_element_cap_match_the_client() {
        let entries: Vec<_> = (0u8..80).map(|i| (key(i), 1, 0, 0)).collect();
        let bytes = build(&entries, 0);
        let path = write_temp("checksums", &bytes);
        let mut index = PageIndex::open(&path).expect("opens");
        for page in 0..index.footer.page_count {
            let body = index.read_page(page).expect("page");
            assert!(index.page_checksum_matches(page, &body), "page {page}");
        }
        let mut body = index.read_page(0).expect("page");
        body[0] ^= 1;
        assert!(!index.page_checksum_matches(0, &body));
        assert_eq!(index.page_entry_count(1), 29);
        assert_eq!(index.page_entry_count(2), 0);
        fs::remove_file(&path).ok();
    }

    /// The whole-file validator `CdnIndexFileParse` (`0x1424d9c90`) on a
    /// complete synthetic index: accepted with its own key, rejected on a
    /// wrong key, a broken terminal, a broken page and a short length.
    #[test]
    fn parse_index_file_walks_the_whole_file_checks() {
        let entries: Vec<_> = (0u8..80).map(|i| (key(i), 1, 0, 0)).collect();
        let bytes = build(&entries, 0);
        let ck = 8usize;
        let footer = &bytes[bytes.len() - 20..];
        let terminal = &bytes[bytes.len() - 20 - ck..bytes.len() - 20];
        let mut hashed = terminal.to_vec();
        hashed.extend_from_slice(footer);
        let index_key: [u8; 16] = blte::md5_digest(&hashed)[..16].try_into().unwrap();
        let len = bytes.len() as u32;

        let mut out = [0xA5u8; 0x30];
        assert_eq!(parse_index_file(&index_key, &bytes, len, &mut out), 0);
        // The derived block is the client's 0x2c bytes; the tail beyond it is
        // untouched.
        assert_eq!(&out[..20], footer);
        assert_eq!(u32::from_le_bytes(out[0x20..0x24].try_into().unwrap()), 2);
        assert!(out[0x2c..].iter().all(|b| *b == 0xA5));

        // A key that is not MD5(terminal + footer).
        let mut wrong = index_key;
        wrong[0] ^= 1;
        assert_eq!(parse_index_file(&wrong, &bytes, len, &mut [0u8; 0x30]), 8);

        // A broken terminal checksum.
        let mut bad = bytes.clone();
        bad[bytes.len() - 20 - ck] ^= 1;
        assert_eq!(parse_index_file(&index_key, &bad, len, &mut [0u8; 0x30]), 8);

        // A broken page body: the last stage is `verify_page_checksums`.
        let mut bad = bytes.clone();
        bad[0] ^= 1;
        assert_eq!(parse_index_file(&index_key, &bad, len, &mut [0u8; 0x30]), 8);

        // Shorter than the footer: the client's `cmp r8d, 0x14; jb`.
        assert_eq!(
            parse_index_file(&index_key, &bytes, 10, &mut [0u8; 0x30]),
            8
        );

        // A header byte flipped after signing: the header MD5 rejects and the
        // hash bytes in `out` stay zeroed, as the client leaves them.
        let mut bad = bytes.clone();
        let at = bad.len() - 20;
        bad[at] ^= 1;
        let mut out = [0xA5u8; 0x30];
        assert_eq!(parse_index_file(&index_key, &bad, len, &mut out), 8);
        assert!(out[12..20].iter().all(|b| *b == 0));
    }

    /// The whole-file status machine of `CdnIndexPageChecksums`
    /// (`0x1424d9f20`): 0 every page matched, 3 a short `len`, 8 a mismatch.
    #[test]
    fn verify_page_checksums_returns_the_client_statuses() {
        // The descriptor as `CdnIndexFooterParse` writes it: two 64-byte
        // pages, 16 bytes of last keys, 8-byte checksums.
        let mut d = [0u8; 0x30];
        d[0] = 1;
        d[6] = 16;
        d[7] = 8;
        d[8..12].copy_from_slice(&2u32.to_le_bytes());
        d[0x14..0x18].copy_from_slice(&64u32.to_le_bytes());
        d[0x20..0x24].copy_from_slice(&2u32.to_le_bytes());
        d[0x24..0x28].copy_from_slice(&32u32.to_le_bytes());
        let footer = IndexFooter::from_descriptor(&d).expect("0x30-byte block");
        assert_eq!(footer.page_count, 2);
        assert_eq!(footer.last_key_bytes, 32);

        // bodies (2 x 64) | last keys (32) | checksums (2 x 8) | terminal (8)
        let mut file = vec![0u8; 128 + 32 + 16 + 8];
        for page in 0..2usize {
            let body: Vec<u8> = (0..64u8).map(|i| i.wrapping_add(page as u8)).collect();
            file[page * 64..page * 64 + 64].copy_from_slice(&body);
            let digest = blte::md5_digest(&body);
            let at = 128 + 32 + page * 8;
            file[at..at + 8].copy_from_slice(&digest[..8]);
        }
        let len = file.len() as u32;
        assert_eq!(verify_page_checksums(&file, len, &footer), 0);
        // One byte short of the page bodies: status 3, before any hashing.
        assert_eq!(verify_page_checksums(&file, 127, &footer), 3);
        // Exactly the bodies is enough: the client's gate is `len < bodies`.
        assert_eq!(verify_page_checksums(&file, 128, &footer), 0);

        // A corrupted page body breaks its own checksum (first page).
        let mut bad = file.clone();
        bad[0] ^= 1;
        assert_eq!(verify_page_checksums(&bad, len, &footer), 8);
        // A corrupted stored checksum on the *second* page: the loop reaches
        // page 2 and stops there.
        let mut bad = file.clone();
        bad[128 + 32 + 8] ^= 1;
        assert_eq!(verify_page_checksums(&bad, len, &footer), 8);
        // A byte past `checksumSize` inside the stored 16-byte MD5 is not
        // compared: the client's `memcmp` length is `checksumSize`.
        let mut extra = file.clone();
        let spare = 128 + 32 + 16;
        extra[spare] ^= 1; // in the terminal, past every compared checksum
        assert_eq!(verify_page_checksums(&extra, len, &footer), 0);

        // Zero pages: status 0 even with `len` 0 (the client's `test r9d`).
        let mut d0 = d;
        d0[0x20..0x24].copy_from_slice(&0u32.to_le_bytes());
        d0[0x24..0x28].copy_from_slice(&0u32.to_le_bytes());
        let empty = IndexFooter::from_descriptor(&d0).expect("0x30-byte block");
        assert_eq!(verify_page_checksums(&[], 0, &empty), 0);

        // `checksumSize` 0 compares nothing, so every page matches.
        let mut d0ck = d;
        d0ck[7] = 0;
        let no_ck = IndexFooter::from_descriptor(&d0ck).expect("0x30-byte block");
        assert_eq!(verify_page_checksums(&file[..128], 128, &no_ck), 0);
    }

    fn bucket_file(bucket: u8, entries: &[([u8; 9], u64, u32)]) -> Vec<u8> {
        let mut out = vec![0u8; index::ENTRIES_OFFSET];
        out[0..4].copy_from_slice(&16u32.to_le_bytes());
        out[8..10].copy_from_slice(&7u16.to_le_bytes());
        out[10] = bucket;
        out[12] = 4;
        out[13] = 5;
        out[14] = 9;
        out[15] = 30;
        out[32..36].copy_from_slice(&((entries.len() * index::ENTRY_SIZE) as i32).to_le_bytes());
        for (key, packed, size) in entries {
            out.extend_from_slice(key);
            out.extend_from_slice(&packed.to_be_bytes()[3..8]);
            out.extend_from_slice(&size.to_le_bytes());
        }
        let mut state = [0u32; 2];
        for entry in out[index::ENTRIES_OFFSET..].chunks_exact(index::ENTRY_SIZE) {
            index::hash_index(entry, &mut state);
        }
        out[36..40].copy_from_slice(&state[0].to_le_bytes());
        let mut state = [0u32; 2];
        index::hash_index(&out[8..24], &mut state);
        out[4..8].copy_from_slice(&state[0].to_le_bytes());
        let page_table_offset = ((entries.len() * index::ENTRY_SIZE) + 32 + 0x1007) & !0xfff;
        out.resize(page_table_offset + index::PAGE_TABLE_MIN_BYTES, 0);
        out
    }

    /// A key whose first byte is the bucket: XOR = b, fold = b ^ 0 = b.
    fn bucket_key(bucket: u8) -> [u8; 9] {
        let mut key = [0u8; 9];
        key[0] = bucket;
        key
    }

    fn temp_dir(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("ow-casc-buckets-{}-{name}", std::process::id()));
        fs::create_dir_all(&path).expect("create synthetic bucket dir");
        path
    }

    fn write_bucket(dir: &Path, bucket: u8, segment: u32, entries: &[([u8; 9], u64, u32)]) {
        let name = index::segment_name(bucket, segment);
        fs::write(dir.join(name), bucket_file(bucket, entries)).expect("write synthetic bucket");
    }

    /// The client's scan keeps the highest segment number per bucket, ignores
    /// `._idx` temporaries, and never picks two files for one bucket.
    #[test]
    fn discover_segments_keeps_the_highest_and_ignores_temporaries() {
        let dir = temp_dir("highest");
        let key = bucket_key(3);
        write_bucket(&dir, 3, 0x10, &[(key, (1u64 << 30) | 0x10, 30)]);
        write_bucket(&dir, 3, 0x20, &[(key, (1u64 << 30) | 0x20, 30)]);
        write_bucket(&dir, 3, 0x05, &[(key, (1u64 << 30) | 0x05, 30)]);
        write_bucket(&dir, 4, 0x30, &[(bucket_key(4), (2u64 << 30) | 0x30, 30)]);
        fs::write(
            dir.join("0300000021._idx"),
            bucket_file(3, &[(key, (9u64 << 30) | 0x21, 30)]),
        )
        .expect("write temporary");
        // A directory with a segment-shaped name must be skipped.
        fs::create_dir(dir.join("0300000030.idx")).expect("create segment-shaped dir");

        let found = discover_segments(&dir).expect("scan");
        let bucket3 = found[3].as_ref().expect("bucket 3");
        assert_eq!(bucket3.segment, 0x20);
        assert!(bucket3.path.ends_with("0300000020.idx"));
        assert_eq!(found[4].as_ref().expect("bucket 4").segment, 0x30);
        assert!(found[5].is_none());
        fs::remove_dir_all(&dir).ok();
    }

    /// The selected segment is loaded alone; a key is looked up only in the
    /// bucket its hash selects. A copy placed in another bucket must not be
    /// merged in, and a lower segment's entry must not survive.
    #[test]
    fn bucket_index_selects_one_segment_and_does_not_merge_buckets() {
        let dir = temp_dir("no-merge");
        for bucket in 0u8..16 {
            write_bucket(
                &dir,
                bucket,
                0x100,
                &[(bucket_key(bucket), (u64::from(bucket) + 1) << 30, 30)],
            );
        }
        // Bucket 3 has a newer segment; its old entry must be gone.
        write_bucket(
            &dir,
            3,
            0x200,
            &[(bucket_key(3), (33u64 << 30) | 0x200, 30)],
        );
        // Bucket 4 also contains bucket 3's key; a merge would see this copy.
        // The entries are in the file's non-decreasing key order, as the
        // client's binary search requires.
        write_bucket(
            &dir,
            4,
            0x100,
            &[
                (bucket_key(3), (99u64 << 30) | 0x400, 30),
                (bucket_key(4), (5u64 << 30) | 0x300, 30),
            ],
        );

        let index = BucketIndex::open_dir(&dir).expect("open synthetic buckets");
        assert_eq!(index.segment_numbers()[3], Some(0x200));
        assert_eq!(index.segment_numbers()[4], Some(0x100));
        let key3 = bucket_key(3);
        let location = index.location_of(&key3).expect("bucket 3 key");
        assert_eq!(location.archive, 33);
        assert_eq!(location.offset, 0x200);
        let key4 = bucket_key(4);
        let location = index.location_of(&key4).expect("bucket 4 key");
        assert_eq!(location.archive, 5);
        assert_eq!(location.offset, 0x300);
        assert_eq!(index.key_count(), 17);
        fs::remove_dir_all(&dir).ok();
    }

    /// A read-only open needs all sixteen buckets, as `DynamicIndexOpen` does.
    #[test]
    fn a_missing_bucket_is_an_error() {
        let dir = temp_dir("missing");
        for bucket in 0u8..16 {
            if bucket == 7 {
                continue;
            }
            write_bucket(
                &dir,
                bucket,
                0x100,
                &[(bucket_key(bucket), u64::from(bucket) << 30, 30)],
            );
        }
        let err = BucketIndex::open_dir(&dir).expect_err("bucket 7 is missing");
        assert!(err.to_string().contains("bucket 07"), "{err}");
        fs::remove_dir_all(&dir).ok();
    }

    /// The file name selected the bucket; the header must agree.
    #[test]
    fn a_header_bucket_disagreement_is_refused() {
        let dir = temp_dir("header");
        for bucket in 0u8..16 {
            let header = if bucket == 2 { 9 } else { bucket };
            write_bucket(
                &dir,
                bucket,
                0x100,
                &[(bucket_key(bucket), u64::from(bucket) << 30, 30)],
            );
            if bucket == 2 {
                let name = index::segment_name(bucket, 0x100);
                fs::write(dir.join(name), bucket_file(header, &[])).expect("rewrite header");
            }
        }
        assert!(BucketIndex::open_dir(&dir).is_err());
        fs::remove_dir_all(&dir).ok();
    }

    /// An offset width of 1..=3 puts the 4-byte offset read at `20..24` on a
    /// 21..23-byte stride. The decoder used to slice past the entry and panic;
    /// the guard refuses the geometry instead.
    #[test]
    fn a_short_offset_width_is_refused_instead_of_read_past_the_stride() {
        let footer = IndexFooter {
            version: 0,
            page_size: 4096,
            offset_bytes: 2,
            size_bytes: 4,
            key_bytes: 16,
            checksum_size: 0,
            element_count: 1,
            entries_per_page: 1,
            page_count: 1,
            last_key_bytes: 16,
            checksum_bytes: 0,
        };
        // Stride 22, but the offset read needs bytes 20..24.
        match footer.entries_in_page(0, &[0u8; 4096]) {
            Err(Error::Parse(message)) => {
                assert!(message.contains("unsupported index entry geometry"));
            }
            other => panic!("expected the short width to be refused, got {other:?}"),
        }

        // Offset 4 is decodable: ordinal width 0, the offset at 20..24 on a
        // 24-byte stride, so the same page decodes to one entry.
        let footer = IndexFooter {
            offset_bytes: 4,
            ..footer
        };
        let entries = footer.entries_in_page(0, &[0u8; 4096]).expect("decodes");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].offset, 0);
    }
}

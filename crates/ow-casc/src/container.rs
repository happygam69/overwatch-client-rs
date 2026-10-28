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
use crate::encoding::EncodingFile;
use crate::index::{self, Location};
use crate::install::{Error as InstallError, Install};

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
    /// Open pack files, keyed by archive ordinal (`data/data.NNN`).
    archives: RefCell<HashMap<u32, fs::File>>,
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

    /// Read the stored (still BLTE-encoded) record for an encoding key.
    ///
    /// The record starts at the entry's packed location and is `size` bytes
    /// long, 30-byte data header included. The client reads the payload from
    /// `(packed + 0x1e) >> 30` for `size - 0x1e` bytes; a record whose header
    /// would cross a 2^30 boundary therefore moves into the next archive.
    /// The loop below reads the same `size` bytes as one contiguous span,
    /// following that boundary, so `blte::decode` still sees the header it
    /// strips. No entry in this install crosses.
    pub fn read_stored(&self, ekey: &[u8]) -> Result<Vec<u8>> {
        let location = self
            .location_of(ekey)
            .ok_or_else(|| Error::KeyNotFound(hex(ekey)))?;

        let mut at = location.packed();
        let mut remaining = u64::from(location.size);
        let mut out = Vec::new();
        while remaining != 0 {
            let archive = (at >> index::ARCHIVE_SHIFT) as u32;
            let offset = at & index::ARCHIVE_MASK;
            // The client's page reader (`DynamicContainerReadPage`,
            // `0x1424fcd20`) rejects an archive number above 0x3fe before
            // opening the pack. The 10-bit field can hold 0..1023, so a
            // corrupt entry can name 1023.
            if archive > index::MAX_ARCHIVE {
                return Err(Error::Parse(format!(
                    "data archive {archive} is beyond the client's 0..=1022 range"
                )));
            }
            let take = remaining.min(index::ARCHIVE_MASK + 1 - offset);
            out.extend_from_slice(&self.read_archive_span(archive, offset, take as usize)?);
            at += take;
            remaining -= take;
        }
        Ok(out)
    }

    /// Read one archive span, keeping the pack open in the `archives` cache.
    ///
    /// The ordinal names a pack in `data/`. `ecache/` is deliberately not a
    /// content store here: its records are chunk manifests (see
    /// `BucketIndex::open_dir`).
    fn read_archive_span(&self, archive: u32, offset: u64, len: usize) -> Result<Vec<u8>> {
        let path = self.install.data_dir().join(index::archive_name(archive));
        let mut archives = self.archives.borrow_mut();
        if let std::collections::hash_map::Entry::Vacant(e) = archives.entry(archive) {
            e.insert(fs::File::open(&path)?);
        }
        let file = archives.get_mut(&archive).expect("just inserted");
        file.seek(SeekFrom::Start(offset))?;
        // The entry claims this many bytes; a truncated archive must not turn
        // the claim into a multi-GB zeroed allocation before `read_exact` can
        // reject it.
        let available = file.metadata()?.len().saturating_sub(offset);
        if len as u64 > available {
            return Err(Error::Parse(format!(
                "data.{archive:03}: record at {offset:#x} claims {len} B but the pack has {available} B left"
            )));
        }
        let mut buf = vec![0u8; len];
        file.read_exact(&mut buf)?;
        Ok(buf)
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
        let stored = match self.read_stored(ekey) {
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
        let mut index = PageIndex::open(
            &self
                .install
                .indices_dir()
                .join(format!("{loose_hash}.index")),
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
        let mut index = PageIndex::open(
            &self
                .install
                .indices_dir()
                .join(format!("{group_hash}.index")),
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

    /// Declared decoded size for an encoding key, if any encoding manifest has
    /// it.
    pub fn decoded_size(&self, ekey: &[u8; 16]) -> Result<Option<u64>> {
        self.ensure_encoding()?;
        let borrowed = self.encoding.borrow();
        let blobs = borrowed.as_ref().expect("just loaded");
        for blob in blobs {
            if let Some(size) = EncodingFile::parse(blob)?.decoded_size(ekey) {
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

/// One `indices/<hash>.index` read on demand: footer parameters plus the
/// per-page last-key array, so a single key can be found without parsing the
/// whole file — the archive group is 260 MB with 9.9 M entries.
///
/// Page geometry, from the reference's `CDNIndexHandler`:
///
/// ```text
/// page p at p * pageSize     entries: [key 16][size u32 BE] then, for a
///                            group index, the archive ordinal (u16 BE in the
///                            main group, offsetBytes = 6; u8 in the patch
///                            group, offsetBytes = 5) before the u32 BE
///                            offset; a loose entry stops after the size
/// n * 16                     the last key of each page, at n * pageSize
/// n * checksumSize           the per-page checksums, then 8 pad bytes
/// footer at len - checksumSize - 12
/// ```
///
/// `pageCount` follows the reference's `GetTableParameters`, `(length -
/// DynamicSize) / (pageSize + keyBytes + checksumSize)` with `DynamicSize =
/// 12 + 2 * checksumSize`; in terms of the footer's position that is
/// `(footerAt - checksumSize) / perPage`. The full-file parser above uses the
/// same expression (it spent a while applying `DynamicSize` to the footer's
/// position, which is one page short on every file — measured 2026-10-17).
struct PageIndex {
    file: fs::File,
    page_size: u64,
    entries_per_page: u64,
    per_entry: u64,
    entry_count: u64,
    page_count: u64,
    /// 0 for the loose file index, 6 for a group index.
    offset_bytes: usize,
    /// `page_count * 16` bytes of per-page last keys.
    last_keys: Vec<u8>,
}

impl PageIndex {
    fn open(path: &Path) -> Result<PageIndex> {
        const FOOTER_SIZE: u64 = 12;
        let mut file = fs::File::open(path)?;
        let len = file.metadata()?.len();
        if len < FOOTER_SIZE + 16 {
            return Err(Error::Parse(format!("{}: index too small", path.display())));
        }
        let tail_len = (FOOTER_SIZE + 16 * 2).min(len);
        let mut tail = vec![0u8; tail_len as usize];
        file.seek(SeekFrom::End(-(tail_len as i64)))?;
        file.read_exact(&mut tail)?;
        let tail_start = len - tail_len;

        let mut footer = None;
        for checksum_size in (8u64..=16).rev() {
            let Some(at) = len.checked_sub(checksum_size + FOOTER_SIZE) else {
                continue;
            };
            if at < tail_start {
                continue;
            }
            let at = (at - tail_start) as usize;
            let f = &tail[at..at + FOOTER_SIZE as usize];
            if f[0] == 1 && f[1] == 0 && f[2] == 0 && f[7] as u64 == checksum_size {
                footer = Some((at as u64 + tail_start, f, checksum_size));
                break;
            }
        }
        let Some((footer_at, f, checksum_size)) = footer else {
            return Err(Error::Parse(format!("{}: no index footer", path.display())));
        };
        let page_size = u64::from(f[3]) * 1024;
        let offset_bytes = f[4] as usize;
        let size_bytes = f[5] as usize;
        let key_bytes = f[6] as usize;
        if page_size == 0 || key_bytes != 16 || size_bytes != 4 {
            return Err(Error::Parse(format!(
                "{}: unsupported index geometry (page {page_size}, key {key_bytes}, size {size_bytes})",
                path.display()
            )));
        }
        let per_entry = (key_bytes + size_bytes + offset_bytes) as u64;
        let per_page = page_size + key_bytes as u64 + checksum_size;
        let page_count = (footer_at - checksum_size) / per_page;
        let entry_count = u64::from(u32::from_le_bytes([f[8], f[9], f[10], f[11]]));
        let mut last_keys = vec![0u8; (page_count * key_bytes as u64) as usize];
        file.seek(SeekFrom::Start(page_count * page_size))?;
        file.read_exact(&mut last_keys)?;
        Ok(PageIndex {
            file,
            page_size,
            entries_per_page: page_size / per_entry,
            per_entry,
            entry_count,
            page_count,
            offset_bytes,
            last_keys,
        })
    }

    /// First page whose last key is `>= key`, over the last-key array — the
    /// reference's `SearchLastKeys`.
    fn find_page(&self, key: &[u8; 16]) -> Option<u64> {
        let (mut lo, mut hi) = (0u64, self.page_count);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let at = (mid * 16) as usize;
            if self.last_keys[at..at + 16] < key[..] {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        (lo < self.page_count).then_some(lo)
    }

    fn read_page(&mut self, page: u64) -> Result<Vec<u8>> {
        let mut data = vec![0u8; self.page_size as usize];
        self.file.seek(SeekFrom::Start(page * self.page_size))?;
        self.file.read_exact(&mut data)?;
        Ok(data)
    }

    /// Entries on a page, capped by the footer's element count so a
    /// partly-filled last page does not read zeros.
    fn page_entry_count(&self, page: u64) -> u64 {
        let remaining = self
            .entry_count
            .saturating_sub(self.entries_per_page * page);
        remaining.min(self.entries_per_page)
    }

    /// Find a key in a group index: `(archive ordinal, offset, size)`.
    ///
    /// The ordinal's width is what distinguishes the two group flavours:
    /// `offsetBytes = 6` is the main archive group (u16, up to 65 535
    /// archives), `offsetBytes = 5` is the patch group (u8, up to 255 — its
    /// measured geometry, not a reference constant).
    fn group_lookup(&mut self, key: &[u8; 16]) -> Result<Option<(u16, u64, u32)>> {
        if self.offset_bytes != 6 && self.offset_bytes != 5 {
            return Err(Error::Parse(format!(
                "not a group index (offsetBytes {})",
                self.offset_bytes
            )));
        }
        let ordinal_width = self.offset_bytes - 4;
        let Some(page) = self.find_page(key) else {
            return Ok(None);
        };
        let data = self.read_page(page)?;
        for i in 0..self.page_entry_count(page) {
            let at = (i * self.per_entry) as usize;
            let entry = &data[at..at + self.per_entry as usize];
            if &entry[..16] != key {
                continue;
            }
            let size = u32::from_be_bytes([entry[16], entry[17], entry[18], entry[19]]);
            let archive = if ordinal_width == 2 {
                u16::from_be_bytes([entry[20], entry[21]])
            } else {
                u16::from(entry[20])
            };
            let o = 20 + ordinal_width;
            let offset = u32::from_be_bytes([entry[o], entry[o + 1], entry[o + 2], entry[o + 3]]);
            return Ok(Some((archive, u64::from(offset), size)));
        }
        Ok(None)
    }

    /// Find a key in the loose-file index: its size.
    fn loose_lookup(&mut self, key: &[u8; 16]) -> Result<Option<u32>> {
        if self.offset_bytes != 0 {
            return Err(Error::Parse(
                "not a loose-file index (offsetBytes != 0)".into(),
            ));
        }
        let Some(page) = self.find_page(key) else {
            return Ok(None);
        };
        let data = self.read_page(page)?;
        for i in 0..self.page_entry_count(page) {
            let at = (i * self.per_entry) as usize;
            let entry = &data[at..at + self.per_entry as usize];
            if &entry[..16] != key {
                continue;
            }
            return Ok(Some(u32::from_be_bytes([
                entry[16], entry[17], entry[18], entry[19],
            ])));
        }
        Ok(None)
    }
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

    /// A synthetic index with the reference's layout: page bodies, the
    /// per-page last-key array, per-page checksums, 8 pad bytes, the 12-byte
    /// footer, 8 trailing bytes. Entries are `[key 16][size u32 BE]`, plus the
    /// ordinal and offset when `offset_bytes` is 5 or 6.
    fn build(entries: &[([u8; 16], u32, u16, u32)], offset_bytes: usize) -> Vec<u8> {
        let per_entry = 16 + 4 + offset_bytes;
        let epp = TEST_PAGE / per_entry;
        let pages = (entries.len() + epp - 1).max(1) / epp;
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
        out.extend(std::iter::repeat_n(0u8, pages * 8));
        out.extend([0u8; 8]);
        let mut footer = [0u8; 12];
        footer[0] = 1;
        footer[3] = (TEST_PAGE / 1024) as u8;
        footer[4] = offset_bytes as u8;
        footer[5] = 4;
        footer[6] = 16;
        footer[7] = 8;
        footer[8..12].copy_from_slice(&(entries.len() as u32).to_le_bytes());
        out.extend_from_slice(&footer);
        out.extend([0u8; 8]);
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
        assert!(index.page_count >= 2, "synthetic file should span pages");

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
    /// count is the case the full-file parser gets wrong: with the corrected
    /// formula a single-page file has `page_count == 1`, not 0.
    #[test]
    fn loose_lookup_and_single_page_count() {
        let entries = vec![(key(9), 109_878_111, 0u16, 0u32)];
        let path = write_temp("loose", &build(&entries, 0));
        let mut index = PageIndex::open(&path).expect("opens");
        assert_eq!(index.page_count, 1);
        assert_eq!(index.entry_count, 1);
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
}

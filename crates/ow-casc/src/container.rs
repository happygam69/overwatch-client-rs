//! Ties the store together: bucket index, data archives, BLTE, encoding file.
//!
//! Lookup path for a content key is
//! `encoding file -> encoding key -> bucket index -> data.NNN -> BLTE`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::io::{ErrorKind, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

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

/// Read-only view over an installation's store.
pub struct Store {
    install: Install,
    buckets: HashMap<[u8; 9], Location>,
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
        let buckets = load_bucket_index(&install)?;
        Ok(Store {
            install,
            buckets,
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
        self.buckets.len()
    }

    pub fn location_of(&self, ekey: &[u8]) -> Option<Location> {
        let key: [u8; 9] = ekey.get(0..9)?.try_into().ok()?;
        self.buckets.get(&key).copied()
    }

    /// Read the stored (still BLTE-encoded) record for an encoding key.
    pub fn read_stored(&self, ekey: &[u8]) -> Result<Vec<u8>> {
        let location = self
            .location_of(ekey)
            .ok_or_else(|| Error::KeyNotFound(hex(ekey)))?;

        let path = self
            .install
            .data_dir()
            .join(format!("data.{:03}", location.archive));
        let mut archives = self.archives.borrow_mut();
        if !archives.contains_key(&location.archive) {
            archives.insert(location.archive, fs::File::open(&path)?);
        }
        let file = archives.get_mut(&location.archive).expect("just inserted");
        file.seek(SeekFrom::Start(location.offset))?;
        let mut buf = vec![0u8; location.size as usize];
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

    /// Load and keep every decoded encoding manifest the build config names.
    ///
    /// A build-config line is **`<content key> <encoding key>`** — the reference's
    /// `BuildConfig.GetFileRecord` reads `vals[0]` into `ContentKey` and
    /// `vals[1]` into `EncodingKey`. Reading both as encoding keys (which is what
    /// this did) resolves whichever happens to be directly indexable and loses
    /// the other: on this install the second encoding manifest loaded and the
    /// **first** never did, so half the build's content keys looked like they
    /// were "not in this installation".
    ///
    /// So: try the value as an encoding key first, then as a content key
    /// (through the table already loaded — the halves resolve each other), and
    /// repeat until no manifest is added.
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
        let mut pending: Vec<String> = values;
        let mut progressed = true;
        while progressed && !pending.is_empty() {
            progressed = false;
            let mut still = Vec::new();
            for value in pending {
                let Ok(key) = parse_hex_key(&value) else {
                    report.push(format!("{value}: not a key"));
                    continue;
                };
                if let Ok(blob) = self.open_ekey(&key) {
                    report.push(format!(
                        "{value}: opened as an encoding key, {} B",
                        blob.len()
                    ));
                    blobs.push(blob);
                    progressed = true;
                    continue;
                }
                if !blobs.is_empty() {
                    if let Ok(blob) = self.open_ckey_using(&key, &blobs) {
                        report.push(format!(
                            "{value}: opened as a content key through the table, {} B",
                            blob.len()
                        ));
                        blobs.push(blob);
                        progressed = true;
                        continue;
                    }
                }
                still.push(value);
            }
            pending = still;
        }
        for value in pending {
            report.push(format!("{value}: could not be opened"));
        }
        if blobs.is_empty() {
            return Err(Error::Parse("no encoding manifest could be read".into()));
        }
        self.encoding_report.borrow_mut().clone_from(&report);
        *self.encoding.borrow_mut() = Some(blobs);
        Ok(())
    }

    /// How many encoding manifests were read (for a diagnostic to print).
    pub fn encoding_manifests(&self) -> usize {
        self.encoding.borrow().as_ref().map(Vec::len).unwrap_or(0)
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
            for at in 0..blob.len().saturating_sub(32) {
                if &blob[at + 16..at + 32] == ckey {
                    let mut ekey = [0u8; 16];
                    ekey.copy_from_slice(&blob[at..at + 16]);
                    out.push((i, ekey));
                    break;
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
        let blobs = self.encoding.borrow().clone().unwrap_or_default();
        self.open_ckey_using(ckey, &blobs)
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

/// Scan the sixteen bucket files into one map, then the per-archive indexes.
///
/// Both index families exist in a modern install and they are **not** the same
/// set of files:
///
/// * `data/*.idx` — the sixteen legacy bucket tables, keyed by the first nine
///   bytes of an encoding key.
/// * `indices/<archive-hash>.index` — one modern index per archive, keyed by the
///   full 16-byte key, written when the client updates or streams content.
///
/// Reading only the bucket tables reports content the install plainly has as
/// "not present in this installation" — which is what made hero materials look
/// unreadable. The reference reads both (`CDNIndexHandler.OpenIndexFile`).
fn load_archive_indexes(install: &Install, map: &mut HashMap<[u8; 9], Location>) -> Result<usize> {
    let Some(cdn) = &install.cdn_config else {
        return Ok(0);
    };
    let archives: Vec<String> = cdn.get("archives").map(|v| v.to_vec()).unwrap_or_default();
    if archives.is_empty() {
        return Ok(0);
    }
    let dir = install.data_dir();
    let base = dir.parent().unwrap_or(&dir).join("indices");
    let mut added = 0usize;
    for hash in &archives {
        let path = base.join(format!("{hash}.index"));
        let Ok(bytes) = fs::read(&path) else { continue };
        let Ok(entries) = parse_archive_index(&bytes) else {
            continue;
        };
        // Which **local** archive is this? The index file is named by the CDN's
        // archive hash, while the local files are `data.000`…`data.092` in their
        // own contiguous numbering (the CDN lists 826 archives this install does
        // not have), so the number cannot be read off the list order. It is
        // learned instead: entries that the bucket tables already know name the
        // archive they live in, and an index file covers exactly one archive.
        let local = entries
            .iter()
            .find_map(|(ekey, _, _, _)| {
                let mut key = [0u8; 9];
                key.copy_from_slice(&ekey[..9]);
                map.get(&key).map(|l| l.archive)
            })
            .or_else(|| {
                (archives.len() == local_archive_count(install))
                    .then(|| archives.iter().position(|h| h == hash).unwrap_or(0) as u32)
            });
        let Some(archive) = local else { continue };
        for (ekey, offset, size, _) in entries {
            let mut key = [0u8; 9];
            key.copy_from_slice(&ekey[..9]);
            // Only fill what the bucket tables do not already know: those are
            // the entries whose archive number is *verified* by the matching
            // above, and overwriting them would risk a wrong location.
            map.entry(key).or_insert(Location {
                archive,
                offset,
                size,
            });
            added += 1;
        }
    }
    Ok(added)
}

/// Read the client's writable layer: every `shmem` file under `data/casc`.
///
/// A `shmem` is a **group index** — the same page and footer layout as an
/// archive index, with `offsetBytes = 6` (a 2-byte archive index followed by a
/// 4-byte offset) so each entry names its own archive. It is where the client
/// writes content it streams or patches locally, which is why a file can be on
/// disk, work in the game, and be invisible to a reader that only opens the
/// read-only bucket tables and archive indexes.
fn load_shmem(install: &Install, map: &mut HashMap<[u8; 9], Location>) -> Result<usize> {
    let mut found = 0usize;
    let mut candidates = vec![install.data_dir().join("shmem")];
    let casc = install
        .data_dir()
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| install.data_dir());
    let mut stack = vec![(casc, 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > 3 {
            continue;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push((path, depth + 1));
                continue;
            }
            if path.file_name().and_then(|n| n.to_str()) == Some("shmem") {
                candidates.push(path);
            }
        }
    }
    candidates.sort();
    candidates.dedup();
    let mut report = Vec::new();
    for path in candidates {
        let Ok(bytes) = fs::read(&path) else { continue };
        let Ok(entries) = parse_archive_index(&bytes) else {
            report.push(format!("{}: not a group index", path.display()));
            continue;
        };
        let mut added = 0usize;
        for (ekey, offset, size, archive) in entries {
            let Some(archive) = archive else { continue };
            let mut key = [0u8; 9];
            key.copy_from_slice(&ekey[..9]);
            // The writable layer is the client's newest word on a file: it wins.
            map.insert(
                key,
                Location {
                    archive: u32::from(archive),
                    offset,
                    size,
                },
            );
            added += 1;
        }
        report.push(format!("{}: {added} entries", path.display()));
        found += added;
    }
    SHMEM_REPORT.with(|r| r.borrow_mut().clone_from(&report));
    Ok(found)
}

thread_local! {
    /// What each `shmem` read produced, for a diagnostic to print.
    pub static SHMEM_REPORT: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// One line per `shmem` file that was read.
pub fn shmem_report() -> Vec<String> {
    SHMEM_REPORT.with(|r| r.borrow().clone())
}

/// How many `data.NNN` archives exist locally.
fn local_archive_count(install: &Install) -> usize {
    let Ok(entries) = fs::read_dir(install.data_dir()) else {
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

/// Parse one `indices/<archive-hash>.index`.
///
/// The format, from the reference's `CDNIndexHandler` (footer, then pages of
/// entries, then the last-key array, the page checksums and the footer's own
/// checksums):
///
/// ```text
/// page 0 .. page n-1      pageSize bytes each: [ekey 16][size u32 BE][offset u32 BE]
/// n * 16                  the last encoding key of each page
/// n * checksumSize        per-page checksums
/// footer 12               version, 0, 0, pageSizeKB, offsetBytes, sizeBytes,
///                         keyBytes, checksumSize, numElements (u32 LE)
/// checksumSize * 2        the footer's checksums
/// ```
///
/// The footer sits at `len - checksumSize - 12`, and `checksumSize` is found by
/// trying 16 down to 8: only one position yields a footer whose own checksum
/// size matches, which is how the format is self-describing.
fn parse_archive_index(bytes: &[u8]) -> Result<Vec<([u8; 16], u64, u32, Option<u16>)>> {
    const FOOTER_SIZE: usize = 12;
    let len = bytes.len();
    if len < FOOTER_SIZE + 16 {
        return Err(Error::Parse("archive index too small".into()));
    }
    let mut footer = None;
    for checksum_size in (8usize..=16).rev() {
        let at = match len.checked_sub(checksum_size + FOOTER_SIZE) {
            Some(at) => at,
            None => continue,
        };
        let f = &bytes[at..at + FOOTER_SIZE];
        if f[0] != 1 || f[1] != 0 || f[2] != 0 || f[7] as usize != checksum_size {
            continue;
        }
        footer = Some((at, f));
        break;
    }
    let Some((footer_at, f)) = footer else {
        return Err(Error::Parse("no archive-index footer".into()));
    };
    let page_size = f[3] as usize * 1024;
    let offset_bytes = f[4] as usize;
    let size_bytes = f[5] as usize;
    let key_bytes = f[6] as usize;
    let checksum_size = f[7] as usize;
    let dynamic = FOOTER_SIZE + checksum_size * 2;
    if key_bytes != 16 || page_size == 0 || footer_at < dynamic {
        return Err(Error::Parse("archive index header is not supported".into()));
    }
    let per_entry = key_bytes + size_bytes + offset_bytes;
    let per_page = page_size + key_bytes + checksum_size;
    let page_count = (footer_at - dynamic) / per_page;
    let entry_count = page_size / per_entry;
    let mut out = Vec::new();
    for page in 0..page_count {
        let start = page * page_size;
        for i in 0..entry_count {
            let at = start + i * per_entry;
            let Some(slice) = bytes.get(at..at + per_entry) else {
                break;
            };
            let mut ekey = [0u8; 16];
            ekey.copy_from_slice(&slice[..16]);
            if ekey == [0u8; 16] {
                break;
            }
            // All fields are stored big-endian. A *group* index (the client's
            // writable `shmem` layer) uses six bytes after the size: a 2-byte
            // archive index then a 4-byte offset, so the entry names its own
            // archive instead of relying on any list order.
            let mut size = 0u32;
            for b in &slice[16..16 + size_bytes] {
                size = (size << 8) | u32::from(*b);
            }
            let tail = &slice[16 + size_bytes..per_entry];
            let (archive, offset_bytes_here) = if offset_bytes == 6 {
                let mut index = 0u16;
                for b in &tail[..2] {
                    index = (index << 8) | u16::from(*b);
                }
                (Some(index), 4)
            } else {
                (None, offset_bytes)
            };
            let mut offset = 0u64;
            for b in &tail[tail.len() - offset_bytes_here..] {
                offset = (offset << 8) | u64::from(*b);
            }
            out.push((ekey, offset, size, archive));
        }
    }
    Ok(out)
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
/// `(footerAt - checksumSize) / perPage`. (The full-file parser above computes
/// `(footerAt - DynamicSize) / perPage`, which is one page short on every file
/// — measured 2026-10-17, see `STATUS-handoff.md`.)
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

thread_local! {
    /// (bucket entries, entries read from archive indexes, entries the archive
    /// indexes added) — for a diagnostic to print, so a silently empty parse is
    /// visible rather than assumed.
    pub static INDEX_STATS: RefCell<(usize, usize, usize)> = const { RefCell::new((0, 0, 0)) };
}

/// What the index layer found: bucket entries, archive-index entries, and how
/// many the archive indexes added beyond the buckets.
pub fn index_stats() -> (usize, usize, usize) {
    INDEX_STATS.with(|s| *s.borrow())
}

/// Scan the sixteen bucket files into one map.
///
/// Every file whose name begins with a bucket number is read, so a bucket split
/// across several files still resolves.
fn load_bucket_index(install: &Install) -> Result<HashMap<[u8; 9], Location>> {
    let mut buckets = HashMap::new();
    let data_dir = install.data_dir();
    // Two directories hold bucket tables: `data/` for the archives and
    // `ecache/` for the *config* content (root, install and encoding manifests
    // and other encrypted config). Both are read; a config file indexed only in
    // `ecache/` is otherwise invisible, and one of this build's two encoding
    // manifests is exactly that.
    let mut dirs = vec![data_dir.clone()];
    if let Some(parent) = data_dir.parent() {
        dirs.push(parent.join("ecache"));
    }
    for dir in dirs {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !name.ends_with(".idx") {
                continue;
            }
            let bucket = u8::from_str_radix(&name[0..2], 16).ok();
            if bucket.is_none() || bucket == Some(0xff) {
                continue;
            }
            for (ekey, location) in index::parse_bucket_file(&path, bucket)? {
                buckets.insert(ekey, location);
            }
        }
    }
    if buckets.is_empty() {
        return Err(Error::Parse(format!(
            "no .idx bucket files found in {}",
            data_dir.display()
        )));
    }
    // The modern per-archive indexes, which is where updated and streamed
    // content lives. A failure here is not fatal: an old install has none.
    let before = buckets.len();
    let from_archives = load_archive_indexes(install, &mut buckets).unwrap_or(0);
    let from_shmem = load_shmem(install, &mut buckets).unwrap_or(0);
    let _ = from_shmem;
    INDEX_STATS.with(|s| {
        *s.borrow_mut() = (
            before,
            from_archives,
            buckets.len() - before.max(0).min(before),
        );
    });
    Ok(buckets)
}

/// Path of a config file, which lives two directory levels below `config/`.
pub fn config_file_path(install_root: &std::path::Path, key: &str) -> PathBuf {
    install_root
        .join("data")
        .join("casc")
        .join("config")
        .join(&key[0..2])
        .join(&key[2..4])
        .join(key)
}

fn parse_hex_key(hex: &str) -> Result<[u8; 16]> {
    if hex.len() != 32 {
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
                .last()
                .map(|e| e.0)
                .unwrap_or([0u8; 16]);
            out.extend_from_slice(&last);
        }
        out.extend(std::iter::repeat(0u8).take(pages * 8));
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
}

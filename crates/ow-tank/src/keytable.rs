//! Per-build manifest key material.
//!
//! A `.cmf` (content manifest) or `.trg` (resource graph) is AES-CBC encrypted
//! with a key and IV derived from a 512-byte table plus a few numbers from the
//! manifest's own header. The table differs for every game build — a retail
//! install's build selects exactly one table, and overtools/TACTLib carries one
//! source file per build holding it.
//!
//! This project deliberately does **not** vendor that key material. Instead it
//! reads the table for the build in front of it from a checkout of that library
//! which the user already has, or from a plain file of 512 bytes they supply.
//! The derivation algorithm lives in [`crate::manifest`]; only the constants
//! are borrowed, from their own MIT-licensed source, at runtime.
//!
//! See `docs/provenance.md` for why.

use std::fmt;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub const TABLE_SIZE: usize = 512;

/// The build whose 512-byte table is embedded in the current `Overwatch.exe`.
///
/// A build's table is not in a config file; it is a plain 512-byte global in
/// the client's `.rdata`. The table address is fixed to this image, so this
/// constant and the layout guard below are deliberately build-specific.
pub const EMBEDDED_TABLE_BUILD: u32 = 154_504;

/// The table's address in the complete current image, `0x140000000 + RVA`.
///
/// `CmfKey154504` (`0x142e48a20`) loads this address into the register it
/// indexes, and `CmfIv154504` (`0x142e48700`) loads the same address for the
/// IV walk. The 512 bytes end at `0x1436102c0`, where a separate data item
/// begins.
const EMBEDDED_TABLE_RVA: u64 = 0x0361_00c0;
const EMBEDDED_TABLE_VA: u64 = 0x1400_0000 + EMBEDDED_TABLE_RVA;

/// The `.rdata` shape of the image the address above was read from.
///
/// Reading a fixed RVA out of a newer executable would silently return some
/// other global. This shape check refuses an image whose `.rdata` is not the
/// one the address belongs to; the manifest's GUID-sorted check is the second
/// gate.
const IMAGE_RDATA_VA: u32 = 0x02f2_d000;
const IMAGE_RDATA_VSIZE: u32 = 0x0091_8f58;
const IMAGE_RDATA_RAW_OFFSET: u32 = 0x02f2_b800;
const IMAGE_RDATA_RAW_SIZE: u32 = 0x0091_9000;

/// The `.data` shape of the same image.
const IMAGE_DATA_VA: u32 = 0x0384_6000;
const IMAGE_DATA_VSIZE: u32 = 0x00cc_43bc;

/// The map loader's chunk-kind table, read by `FUN_141b32e20` (`0x141b32e20`).
///
/// 0x24 entries of 0x20 bytes; the entry's type index is at `+0` and the chunk
/// kind at `+8`. The loader's GUID formula takes that kind, so the table has to
/// come from the image, not from a list.
const CHUNK_KIND_TABLE_RVA: u64 = 0x039a_a320;
const CHUNK_KIND_TABLE_ENTRIES: usize = 0x24;
const CHUNK_KIND_TABLE_STRIDE: usize = 0x20;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    /// No source was configured, or none of the configured ones had this build.
    NotFound {
        build: u32,
        tried: Vec<PathBuf>,
    },
    /// A source was found but did not yield 512 bytes.
    BadTable {
        source: PathBuf,
        found: usize,
    },
    /// The executable is not the 154504 image whose layout carries the table.
    Layout {
        source: PathBuf,
        detail: String,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "io error: {e}"),
            Error::NotFound { build, tried } => {
                write!(
                    f,
                    "no manifest key table for build {build}. Point OW_TACTLIB at a checkout of \
                     overtools/TACTLib, or OW_KEYTABLE at a 512-byte table file. Tried:"
                )?;
                for path in tried {
                    write!(f, "\n  {}", path.display())?;
                }
                Ok(())
            }
            Error::BadTable { source, found } => write!(
                f,
                "{} holds {found} bytes, expected {TABLE_SIZE}",
                source.display()
            ),
            Error::Layout { source, detail } => write!(
                f,
                "{} is not the build-{EMBEDDED_TABLE_BUILD} image the table address {EMBEDDED_TABLE_VA:#x} belongs to: {detail}",
                source.display()
            ),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

/// Where per-build key material, and supplementary names, may be found.
#[derive(Debug, Clone, Default)]
pub struct Sources {
    /// A checkout of overtools/TACTLib, whose `Core/Product/Tank/` holds one
    /// manifest key generator source file per build.
    pub tactlib_checkout: Option<PathBuf>,
    /// A checkout of overtools/OWLib, whose `TankLib/STU/Types/` holds the
    /// generated STU class definitions, used for diagnostic names.
    pub owlib_checkout: Option<PathBuf>,
    /// A plain file holding exactly 512 bytes.
    pub table_file: Option<PathBuf>,
    /// The client executable. Its `.rdata` holds the current build's table,
    /// which is the only source for builds that no reference checkout ships.
    pub executable: Option<PathBuf>,
    /// A supplementary keyring, found in the asset-library checkout.
    pub support_keyring: Option<PathBuf>,
}

impl Sources {
    /// Read the environment for configured sources.
    ///
    /// `OW_TACTLIB` names the container library checkout, `OW_OWLIB` the asset
    /// library checkout, and `OW_KEYTABLE` a table file. Either checkout's root
    /// works for either variable: the candidate paths are probed, so pointing
    /// both at the same repository checkout is fine. `OW_EXE` names
    /// `Overwatch.exe` when its install folder has an unusual shape.
    pub fn from_env() -> Sources {
        let env_path = |name: &str| {
            std::env::var_os(name)
                .map(PathBuf::from)
                .filter(|p| !p.as_os_str().is_empty())
        };
        let executable = env_path("OW_EXE")
            .or_else(|| env_path("OW_INSTALL").and_then(|root| executable_under(&root)));
        Sources {
            tactlib_checkout: env_path("OW_TACTLIB"),
            owlib_checkout: env_path("OW_OWLIB"),
            table_file: env_path("OW_KEYTABLE"),
            executable,
            support_keyring: env_path("OW_SUPPORT_KEYRING"),
        }
    }

    /// Environment first, then a search for the checkouts on disk.
    ///
    /// Requiring two environment variables before the tool will start is a poor
    /// first impression, and it made the tool look broken on a machine that had
    /// both checkouts sitting one directory away. The search is structural
    /// rather than a list of guessed paths: it looks for the *signature* of each
    /// checkout, so it finds one wherever it happens to live.
    pub fn discover(extra_roots: &[PathBuf]) -> Sources {
        let mut sources = Sources::from_env();
        if sources.executable.is_none() {
            sources.executable = extra_roots.iter().find_map(|root| executable_under(root));
        }
        if sources.tactlib_checkout.is_some() && sources.owlib_checkout.is_some() {
            return sources;
        }

        let mut roots: Vec<PathBuf> = Vec::new();
        if let Ok(cwd) = std::env::current_dir() {
            roots.push(cwd.clone());
            // A checkout is often a sibling of the repository, so walk up.
            let mut up = cwd;
            for _ in 0..3 {
                if !up.pop() {
                    break;
                }
                roots.push(up.clone());
            }
        }
        if let Ok(exe) = std::env::current_exe() {
            let mut dir = exe;
            for _ in 0..3 {
                if !dir.pop() {
                    break;
                }
                roots.push(dir.clone());
            }
        }
        roots.extend(extra_roots.iter().cloned());

        let (tactlib, owlib) = find_checkouts(&roots);
        if sources.tactlib_checkout.is_none() {
            sources.tactlib_checkout = tactlib;
        }
        if sources.owlib_checkout.is_none() {
            sources.owlib_checkout = owlib;
        }
        if sources.support_keyring.is_none() {
            // The asset-library checkout ships a supplementary keyring beside
            // its generated types. Read that copy rather than one of our own.
            sources.support_keyring = sources
                .owlib_checkout
                .iter()
                .map(|root| root.join("TankLib").join("Overwatch.keyring"))
                .find(|p| p.is_file());
        }
        sources
    }

    /// Every place discovery looked, for an error message worth reading.
    pub fn searched(&self) -> Vec<PathBuf> {
        self.tactlib_checkout
            .iter()
            .chain(self.owlib_checkout.iter())
            .chain(self.executable.iter())
            .cloned()
            .collect()
    }

    /// Every directory that might hold the generated STU class sources.
    ///
    /// A checkout is often nested (`OWLib/TACTLib/TACTLib`), and the two
    /// libraries sit side by side, so several shapes are worth trying rather
    /// than requiring the caller to know which one is right.
    pub fn class_source_dirs(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for root in [&self.owlib_checkout, &self.tactlib_checkout]
            .into_iter()
            .flatten()
        {
            for suffix in [
                &["TankLib", "STU", "Types"][..],
                &["STU", "Types"][..],
                &["TankLib", "STU"][..],
            ] {
                let candidate = suffix
                    .iter()
                    .fold(root.clone(), |path, part| path.join(part));
                if candidate.is_dir() && !out.contains(&candidate) {
                    out.push(candidate);
                }
            }
            // also look one level up, for nested or sibling checkouts
            if let Some(parent) = root.parent() {
                for suffix in [&["STU", "Types"][..], &["TankLib", "STU", "Types"][..]] {
                    let candidate = suffix
                        .iter()
                        .fold(parent.to_path_buf(), |path, part| path.join(part));
                    if candidate.is_dir() && !out.contains(&candidate) {
                        out.push(candidate);
                    }
                }
            }
        }
        out
    }

    /// The 512-byte table for a build.
    ///
    /// Order: an explicit `OW_KEYTABLE` file, then the installed client's own
    /// table for [`EMBEDDED_TABLE_BUILD`], then a reference checkout's generated
    /// source for older builds. The client's table is the authority for the
    /// build the client is; the checkout is a convenience for earlier builds.
    pub fn table_for(&self, build: u32) -> Result<[u8; TABLE_SIZE], Error> {
        let mut tried = Vec::new();

        if let Some(path) = &self.table_file {
            tried.push(path.clone());
            if path.is_file() {
                let bytes = std::fs::read(path)?;
                return to_table(bytes, path);
            }
        }

        if build == EMBEDDED_TABLE_BUILD {
            if let Some(path) = &self.executable {
                tried.push(path.clone());
                if path.is_file() {
                    return read_table_from_executable(path);
                }
            }
        }

        if let Some(root) = &self.tactlib_checkout {
            for kind in ["CMF", "TRG"] {
                for prefix in ["Core/Product/Tank", "Product/Tank"] {
                    let path = root
                        .join(prefix)
                        .join(kind)
                        .join(format!("Pro{kind}_{build}.cs"));
                    tried.push(path.clone());
                    if path.is_file() {
                        let bytes = std::fs::read_to_string(&path)?;
                        return to_table(parse_keytable_literal(&bytes), &path);
                    }
                }
            }
        }

        Err(Error::NotFound { build, tried })
    }

    /// The chunk-kind table from the installed client's own image.
    ///
    /// This is the table `FUN_141b32e20` indexes when it turns a map asset and
    /// a type index into a chunk GUID. It is read from the executable's
    /// `.data`, which is plaintext on disk, and each slot's type index is
    /// checked against its position so a shifted read cannot pass.
    pub fn chunk_kinds(&self) -> Result<Vec<u64>, Error> {
        let Some(path) = &self.executable else {
            return Err(Error::NotFound {
                build: EMBEDDED_TABLE_BUILD,
                tried: self.searched(),
            });
        };
        let len = (CHUNK_KIND_TABLE_ENTRIES * CHUNK_KIND_TABLE_STRIDE) as u64;
        let (bytes, section) = read_pe_section(path, CHUNK_KIND_TABLE_RVA, len)?;
        if section.name() != ".data"
            || section.virtual_address != IMAGE_DATA_VA
            || section.virtual_size != IMAGE_DATA_VSIZE
        {
            return Err(layout(
                path,
                "the chunk-kind table is not in this image's .data",
            ));
        }
        let mut kinds = Vec::with_capacity(CHUNK_KIND_TABLE_ENTRIES);
        for slot in 0..CHUNK_KIND_TABLE_ENTRIES {
            let at = slot * CHUNK_KIND_TABLE_STRIDE;
            let index =
                u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
            if index as usize != slot {
                return Err(layout(
                    path,
                    "the chunk-kind table's slot index does not match its position",
                ));
            }
            kinds.push(u64::from_le_bytes([
                bytes[at + 8],
                bytes[at + 9],
                bytes[at + 10],
                bytes[at + 11],
                bytes[at + 12],
                bytes[at + 13],
                bytes[at + 14],
                bytes[at + 15],
            ]));
        }
        Ok(kinds)
    }
}

/// Find `Overwatch.exe` in an install root or a game folder.
///
/// A retail install keeps it under `_retail_`; a game-folder path already ends
/// at that folder. Both shapes are accepted, and neither is guessed beyond
/// these two names.
fn executable_under(root: &Path) -> Option<PathBuf> {
    [
        root.join("_retail_").join("Overwatch.exe"),
        root.join("Overwatch.exe"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

fn to_table(bytes: Vec<u8>, source: &Path) -> Result<[u8; TABLE_SIZE], Error> {
    if bytes.len() != TABLE_SIZE {
        return Err(Error::BadTable {
            source: source.to_path_buf(),
            found: bytes.len(),
        });
    }
    let mut out = [0u8; TABLE_SIZE];
    out.copy_from_slice(&bytes);
    Ok(out)
}

/// One PE section header, reduced to what an RVA read needs.
#[derive(Debug, Clone, Copy)]
struct PeSection {
    name: [u8; 8],
    virtual_size: u32,
    virtual_address: u32,
    raw_size: u32,
    raw_offset: u32,
}

impl PeSection {
    fn name(&self) -> &str {
        let end = self.name.iter().position(|b| *b == 0).unwrap_or(8);
        std::str::from_utf8(&self.name[..end]).unwrap_or("")
    }

    fn matches_current_rdata(&self) -> bool {
        self.name() == ".rdata"
            && self.virtual_address == IMAGE_RDATA_VA
            && self.virtual_size == IMAGE_RDATA_VSIZE
            && self.raw_offset == IMAGE_RDATA_RAW_OFFSET
            && self.raw_size == IMAGE_RDATA_RAW_SIZE
    }
}

/// Read `len` bytes at an RVA out of a PE file, using the section table.
///
/// `Overwatch.exe`'s `.rdata` is plaintext on disk; its `.text` is encrypted,
/// so this is only used for the table, which lives in `.rdata`.
fn read_pe_section(path: &Path, rva: u64, len: u64) -> Result<(Vec<u8>, PeSection), Error> {
    let mut file = File::open(path)?;
    let file_len = file.metadata()?.len();

    let mut dos = [0u8; 0x40];
    file.read_exact(&mut dos)?;
    if &dos[0..2] != b"MZ" {
        return Err(layout(path, "no MZ header"));
    }
    let pe = u32::from_le_bytes([dos[0x3c], dos[0x3d], dos[0x3e], dos[0x3f]]) as u64;
    if pe < 0x40 || pe + 24 > file_len {
        return Err(layout(path, "the PE header pointer is outside the file"));
    }

    let mut coff = [0u8; 24];
    read_at(&mut file, pe, &mut coff)?;
    if &coff[0..4] != b"PE\0\0" {
        return Err(layout(path, "no PE signature"));
    }
    let machine = u16::from_le_bytes([coff[4], coff[5]]);
    if machine != 0x8664 {
        return Err(layout(path, "not an x86-64 image"));
    }
    let section_count = u16::from_le_bytes([coff[6], coff[7]]) as usize;
    let optional_size = u16::from_le_bytes([coff[20], coff[21]]) as u64;
    if section_count == 0 || section_count > 96 {
        return Err(layout(path, "implausible section count"));
    }
    if !(2..=0x1000).contains(&optional_size) {
        return Err(layout(path, "implausible optional-header size"));
    }

    let mut magic = [0u8; 2];
    read_at(&mut file, pe + 24, &mut magic)?;
    if u16::from_le_bytes(magic) != 0x20b {
        return Err(layout(path, "not a PE32+ image"));
    }

    let section_table = pe + 24 + optional_size;
    if section_table + section_count as u64 * 40 > file_len {
        return Err(layout(path, "the section table runs past the file"));
    }
    for i in 0..section_count {
        let at = section_table + i as u64 * 40;
        let mut header = [0u8; 40];
        read_at(&mut file, at, &mut header)?;
        let mut name = [0u8; 8];
        name.copy_from_slice(&header[0..8]);
        let section = PeSection {
            name,
            virtual_size: u32::from_le_bytes([header[8], header[9], header[10], header[11]]),
            virtual_address: u32::from_le_bytes([header[12], header[13], header[14], header[15]]),
            raw_size: u32::from_le_bytes([header[16], header[17], header[18], header[19]]),
            raw_offset: u32::from_le_bytes([header[20], header[21], header[22], header[23]]),
        };
        let start = section.virtual_address as u64;
        let end = start + section.virtual_size as u64;
        let raw_end = start + section.raw_size as u64;
        if rva < start || rva + len > end || rva + len > raw_end {
            continue;
        }
        let offset = section.raw_offset as u64 + (rva - start);
        if offset + len > file_len {
            return Err(layout(path, "the table runs past the end of the file"));
        }
        let mut bytes = vec![0u8; len as usize];
        read_at(&mut file, offset, &mut bytes)?;
        return Ok((bytes, section));
    }

    Err(layout(path, "no section contains the table address"))
}

fn read_at(file: &mut File, offset: u64, buf: &mut [u8]) -> Result<(), Error> {
    file.seek(SeekFrom::Start(offset))?;
    file.read_exact(buf)?;
    Ok(())
}

fn layout(path: &Path, detail: &str) -> Error {
    Error::Layout {
        source: path.to_path_buf(),
        detail: detail.to_string(),
    }
}

/// Read the build's 512-byte table from the installed client.
///
/// The address is a fact about the current image, so the section that contains
/// it must also have the current image's `.rdata` shape. A newer executable
/// therefore fails here instead of yielding bytes from an unrelated global; if
/// it somehow passed, the manifest's GUID-sorted record check would still
/// reject the wrong key.
fn read_table_from_executable(path: &Path) -> Result<[u8; TABLE_SIZE], Error> {
    let (bytes, section) = read_pe_section(path, EMBEDDED_TABLE_RVA, TABLE_SIZE as u64)?;
    if !section.matches_current_rdata() {
        return Err(layout(
            path,
            &format!(
                ".rdata is VA {:#x} size {:#x} raw {:#x}/{:#x}, expected VA {IMAGE_RDATA_VA:#x} \
                 size {IMAGE_RDATA_VSIZE:#x} raw {IMAGE_RDATA_RAW_OFFSET:#x}/{IMAGE_RDATA_RAW_SIZE:#x}",
                section.virtual_address,
                section.virtual_size,
                section.raw_offset,
                section.raw_size
            ),
        ));
    }
    if bytes.iter().all(|b| *b == 0) {
        return Err(layout(path, "the table bytes are all zero"));
    }
    let mut out = [0u8; TABLE_SIZE];
    out.copy_from_slice(&bytes);
    Ok(out)
}

/// Pull the 512 hex bytes out of the `Keytable = { 0x.., ... }` literal.
///
/// The identifier also appears in the accessor code, so the declaration is the
/// last occurrence.
pub fn parse_keytable_literal(source: &str) -> Vec<u8> {
    let Some(decl) = source.rfind("Keytable") else {
        return Vec::new();
    };
    let rest = &source[decl..];
    let Some(open) = rest.find('{') else {
        return Vec::new();
    };
    let Some(close) = rest[open..].find('}') else {
        return Vec::new();
    };
    let body = &rest[open + 1..open + close];

    let mut out = Vec::new();
    let bytes = body.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'0' && (bytes[i + 1] == b'x' || bytes[i + 1] == b'X') {
            let start = i + 2;
            let mut end = start;
            while end < bytes.len() && bytes[end].is_ascii_hexdigit() && end - start < 2 {
                end += 1;
            }
            if end - start == 2 {
                if let Ok(v) = u8::from_str_radix(&body[start..end], 16) {
                    out.push(v);
                }
                i = end;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Directory names never worth descending into during a search.
const SKIP: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    "bin",
    "obj",
    ".vs",
    ".idea",
    "packages",
    "data",
    "cache",
    "Cache",
    "System Volume Information",
    "$RECYCLE.BIN",
];

/// Find the two checkouts by looking for what makes each one recognisable.
///
/// * an asset-library checkout contains `TankLib/STU/Types`
/// * a container-library checkout contains `Core/Product/Tank`
///
/// Breadth-first, so a shallow match wins over a deep one, and bounded in both
/// depth and breadth so a search from a drive root cannot run away. The
/// signatures are checked at every directory visited, including each root, which
/// is what lets a checkout be found wherever it lives rather than at a guessed
/// path.
fn find_checkouts(roots: &[PathBuf]) -> (Option<PathBuf>, Option<PathBuf>) {
    // A checkout can be nested: `OWLib/TACTLib/TACTLib` is four levels below a
    // drive root, and the reference checkout on this machine is exactly that
    // shape. Three levels found the outer library and missed the inner one.
    const MAX_DEPTH: usize = 5;
    const MAX_DIRS: usize = 1_500;
    const MAX_CHILDREN: usize = 200;

    /// Names that suggest a directory could hold a source checkout. Past the
    /// first two levels the search only descends into these, which is what keeps
    /// a walk from a drive root to a couple of hundred milliseconds instead of
    /// half a minute. A checkout with an unrelated name is still found through
    /// `OW_TACTLIB` or `OW_OWLIB`.
    fn looks_relevant(name: &str) -> bool {
        let name = name.to_ascii_lowercase();
        [
            "owlib",
            "tactlib",
            "tanklib",
            "casc",
            "overwatch",
            "repos",
            "src",
            "projects",
            "code",
            "dev",
            "tools",
            "git",
            "work",
            "games",
        ]
        .iter()
        .any(|needle| name.contains(needle))
    }

    let is_owlib = |dir: &std::path::Path| dir.join("TankLib").join("STU").join("Types").is_dir();
    let is_tactlib = |dir: &std::path::Path| dir.join("Core").join("Product").join("Tank").is_dir();

    let mut tactlib = None;
    let mut owlib = None;
    let mut queue: std::collections::VecDeque<(PathBuf, usize)> = std::collections::VecDeque::new();
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();

    for root in roots {
        if root.is_dir() && seen.insert(root.clone()) {
            queue.push_back((root.clone(), 0));
        }
    }

    let mut visited = 0usize;
    while let Some((dir, depth)) = queue.pop_front() {
        visited += 1;
        if visited > MAX_DIRS {
            break;
        }

        if owlib.is_none() && is_owlib(&dir) {
            owlib = Some(dir.clone());
        }
        if tactlib.is_none() && is_tactlib(&dir) {
            tactlib = Some(dir.clone());
        }
        if tactlib.is_some() && owlib.is_some() {
            break;
        }
        if depth >= MAX_DEPTH {
            continue;
        }

        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        // Relevant names first, so a shallow match is reached before the
        // breadth-first walk spends its budget on unrelated trees.
        let mut children: Vec<(bool, PathBuf)> = Vec::new();
        for (count, entry) in entries.flatten().enumerate() {
            if count >= MAX_CHILDREN {
                break;
            }
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !path.is_dir() || name.starts_with('.') || SKIP.contains(&name) {
                continue;
            }
            if depth >= 1 && !looks_relevant(name) {
                continue;
            }
            children.push((!looks_relevant(name), path));
        }
        children.sort_by_key(|(unrelated, _)| *unrelated);
        for (_, path) in children {
            if seen.insert(path.clone()) {
                queue.push_back((path, depth + 1));
            }
        }
    }

    (tactlib, owlib)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_keytable_literal() {
        let source = r#"
            public byte[] Key(Header h, int length) { return Keytable[h.x]; }
            private static readonly byte[] Keytable =
            {
                0xBC, 0x63, 0x89, 0x14, 0x11, 0x19, 0x93, 0x89,
                0x51, 0xFB, 0x72, 0xCB, 0x04, 0x5C, 0x49, 0xA3,
            };
        "#;
        let table = parse_keytable_literal(source);
        assert_eq!(table.len(), 16);
        assert_eq!(table[0], 0xBC);
        assert_eq!(table[15], 0xA3);
    }

    /// A tiny PE32+ file with one `.rdata` section: enough to prove the RVA to
    /// file-offset mapping, without any game bytes.
    fn synthetic_pe(table: &[u8; TABLE_SIZE]) -> Vec<u8> {
        let mut file = vec![0u8; 0x1400];
        file[0..2].copy_from_slice(b"MZ");
        file[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        file[0x80..0x84].copy_from_slice(b"PE\0\0");
        file[0x84..0x86].copy_from_slice(&0x8664u16.to_le_bytes());
        file[0x86..0x88].copy_from_slice(&1u16.to_le_bytes());
        file[0x94..0x96].copy_from_slice(&0xf0u16.to_le_bytes());
        file[0x98..0x9a].copy_from_slice(&0x20bu16.to_le_bytes());
        let section = 0x80 + 24 + 0xf0;
        file[section..section + 7].copy_from_slice(b".rdata\0");
        file[section + 8..section + 12].copy_from_slice(&0x2000u32.to_le_bytes());
        file[section + 12..section + 16].copy_from_slice(&0x1000u32.to_le_bytes());
        file[section + 16..section + 20].copy_from_slice(&0x1000u32.to_le_bytes());
        file[section + 20..section + 24].copy_from_slice(&0x400u32.to_le_bytes());
        // RVA 0x1800 = raw 0x400 + (0x1800 - 0x1000) = 0xc00.
        file[0xc00..0xc00 + TABLE_SIZE].copy_from_slice(table);
        file
    }

    #[test]
    fn pe_reader_maps_an_rva_through_the_section_table() {
        let table = std::array::from_fn(|i| (i * 7 + 3) as u8);
        let path = std::env::temp_dir().join(format!(
            "ow-keytable-pe-{}-{}.bin",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, synthetic_pe(&table)).unwrap();
        let (bytes, section) = read_pe_section(&path, 0x1800, TABLE_SIZE as u64).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(section.name(), ".rdata");
        assert_eq!(bytes, table);
    }

    #[test]
    fn pe_reader_refuses_an_rva_outside_every_section() {
        let table = std::array::from_fn(|i| (i * 7 + 3) as u8);
        let path = std::env::temp_dir().join(format!(
            "ow-keytable-pe-miss-{}-{}.bin",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, synthetic_pe(&table)).unwrap();
        let err = read_pe_section(&path, 0x4000, TABLE_SIZE as u64).unwrap_err();
        let _ = std::fs::remove_file(&path);
        assert!(matches!(err, Error::Layout { .. }));
    }
}

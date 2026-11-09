//! Locating and describing an Overwatch installation.
//!
//! An OW install exposes three things this crate needs:
//!
//! * `.build.info` — names the build, CDN and keyring configs for each branch.
//! * `data/casc/` — the store: `config/`, `data/`, `indices/`.
//! * the config files those keys resolve to, including the keyring that holds
//!   the Salsa20 keys for encrypted BLTE chunks.
//!
//! Everything here is read-only. The installation is the user's own copy and is
//! never modified.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::cdn::{self, CdnClient};

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    /// No usable install was found at the given path.
    NotAnInstall(PathBuf),
    /// A file the install's own metadata points at is missing.
    Missing(PathBuf),
    Parse(String),
    /// `OW_CDN` asked for CDN fetching and the client could not be built.
    Cdn(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "io error: {e}"),
            Error::NotAnInstall(p) => write!(
                f,
                "{} does not look like an Overwatch installation \
                 (no .build.info or data/casc)",
                p.display()
            ),
            Error::Missing(p) => {
                write!(f, "missing file referenced by the install: {}", p.display())
            }
            Error::Parse(m) => write!(f, "malformed install metadata: {m}"),
            Error::Cdn(m) => write!(f, "CDN fetch is enabled but unusable: {m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<cdn::Error> for Error {
    fn from(e: cdn::Error) -> Self {
        Error::Cdn(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// A `key = value value ...` CASC config file.
///
/// Values are lists because several keys carry more than one, e.g.
/// `encoding = <ckey> <ekey>` and `archives = <hash> <hash> ...`.
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub values: BTreeMap<String, Vec<String>>,
}

impl Config {
    pub fn parse(text: &str) -> Config {
        let mut values: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for line in text.lines() {
            // `#` starts a comment; content after it is ignored.
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let Some((key, rest)) = line.split_once('=') else {
                continue;
            };
            values.insert(
                key.trim().to_string(),
                rest.split_whitespace().map(str::to_string).collect(),
            );
        }
        Config { values }
    }

    pub fn get(&self, key: &str) -> Option<&[String]> {
        self.values.get(key).map(Vec::as_slice)
    }

    pub fn get_str(&self, key: &str) -> Result<&str> {
        self.get(key)
            .and_then(|v| v.first())
            .map(String::as_str)
            .ok_or_else(|| Error::Parse(format!("config key {key:?} is absent or empty")))
    }
}

/// One row of `.build.info`.
#[derive(Debug, Clone)]
pub struct BuildInfo {
    pub branch: String,
    pub active: bool,
    pub build_key: String,
    pub cdn_key: String,
    pub keyring_key: Option<String>,
    pub version: String,
    pub product: String,
    /// `CDN Path`: the URL segment below a host, e.g. `tpr/ovw`.
    pub cdn_path: String,
    /// `CDN Hosts`: space-separated, usually bare (`level3.blizzard.com`).
    pub cdn_hosts: Vec<String>,
}

impl BuildInfo {
    /// Parse `.build.info`: a header row naming columns, then one row per branch.
    ///
    /// Column names carry a `!TYPE:WIDTH` suffix which we strip.
    pub fn parse(text: &str) -> Result<Vec<BuildInfo>> {
        let mut lines = text.lines();
        let header = lines
            .next()
            .ok_or_else(|| Error::Parse(".build.info is empty".into()))?;
        let names: Vec<&str> = header
            .split('|')
            .map(|h| h.split('!').next().unwrap_or(h).trim())
            .collect();

        let mut out = Vec::new();
        for line in lines {
            if line.trim().is_empty() {
                continue;
            }
            let fields: Vec<&str> = line.split('|').collect();
            let mut row = BTreeMap::new();
            for (name, value) in names.iter().zip(fields.iter()) {
                row.insert(*name, value.trim());
            }
            out.push(BuildInfo {
                branch: row.get("Branch").copied().unwrap_or("").to_string(),
                active: row.get("Active").copied().map(parse_bool).unwrap_or(false),
                build_key: row.get("Build Key").copied().unwrap_or("").to_string(),
                cdn_key: row.get("CDN Key").copied().unwrap_or("").to_string(),
                keyring_key: row
                    .get("KeyRing")
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty()),
                version: row.get("Version").copied().unwrap_or("").to_string(),
                product: row.get("Product").copied().unwrap_or("").to_string(),
                cdn_path: row.get("CDN Path").copied().unwrap_or("").to_string(),
                cdn_hosts: row
                    .get("CDN Hosts")
                    .map(|s| s.split_whitespace().map(str::to_string).collect())
                    .unwrap_or_default(),
            });
        }
        if out.is_empty() {
            return Err(Error::Parse(".build.info has no branch rows".into()));
        }
        Ok(out)
    }
}

/// Resolve the directory that holds `data/casc`, and the product flavor.
///
/// Two layouts exist in the wild:
///
/// * The install root itself holds `.build.info` and `data/casc` (the path
///   the project usually opens).
/// * A game subfolder (for example `_retail_`) holds `.flavor.info` and the
///   executable; `.build.info` and `data/casc` are one level up. The client's
///   `BuildInfoLoadFromInstall` (`0x14248f2a0`) builds
///   `<install>/../.build.info` whenever `.flavor.info` is present, so that is
///   the layout this function resolves.
fn resolve_layout(given: &Path) -> Result<(PathBuf, Option<String>)> {
    let flavor = read_flavor(&given.join(".flavor.info"));
    if flavor.is_some() {
        // The client always goes one level up when `.flavor.info` exists,
        // even if a local `.build.info` is also present.
        if let Some(parent) = given.parent() {
            if parent.join(".build.info").is_file() {
                return Ok((parent.to_path_buf(), flavor));
            }
        }
        return Err(Error::NotAnInstall(given.to_path_buf()));
    }
    if given.join(".build.info").is_file() {
        return Ok((given.to_path_buf(), None));
    }
    Err(Error::NotAnInstall(given.to_path_buf()))
}

/// The first field of the first row of a `.flavor.info` table.
///
/// The file is the same `!TYPE:WIDTH` table shape as `.build.info`; the
/// reference install's `_retail_/.flavor.info` is `Product Flavor!STRING:0`
/// followed by `pro`.
/// The client's boolean parser (`ParseBoolString`, `0x1424d8800`).
///
/// A case-insensitive **prefix** match, first against eight true spellings and
/// then against eight false ones, in the client's table order. A match writes
/// `out` - 1 from the true table, 0 from the false one - and returns 1; when no
/// spelling matches it returns 0 and leaves `out` untouched, so the caller sees
/// "no answer" instead of "false".
///
/// The comparison walks both sides per byte, folding `A..=Z` only, so the
/// caller's text must be a **prefix** of a spelling: `""` matches (the length
/// check is before the first byte), `"o"` matches `on` before `off` is ever
/// tried, `"enab"` matches `enable`, and `"enablex"` matches nothing because
/// its seventh byte is compared against the spelling's NUL. A leading space
/// matches nothing either: nothing in the client trims here.
pub fn parse_bool_string(text: &[u8], len: usize, out: &mut u8) -> u8 {
    /// The client's fold: `LEA EAX,[RCX-0x41] / CMP EAX,0x19 / LEA EDX,[RCX+0x20]
    /// / CMOVA EDX,ECX` - a byte in `A..=Z` becomes lowercase.
    fn fold(c: u8) -> u32 {
        if u32::from(c).wrapping_sub(0x41) <= 0x19 {
            u32::from(c) + 0x20
        } else {
            u32::from(c)
        }
    }
    /// One spelling: walk both sides until a difference, either NUL, or the
    /// end of the caller's `len`.
    fn heads_match(text: &[u8], len: usize, spelling: &[u8]) -> bool {
        let mut i = 0;
        let mut left = len;
        let mut j = 0;
        loop {
            if left == 0 {
                return true;
            }
            let a = fold(text[i]);
            i += 1;
            left -= 1;
            let b = fold(spelling[j]);
            j += 1;
            if a != b {
                return false;
            }
            if b == 0 {
                return true;
            }
        }
    }
    // The client's two tables, in order (`0x143472f10` and `0x143472f50`).
    const TRUE_SPELLINGS: [&[u8]; 8] = [
        b"1\0",
        b"on\0",
        b"enable\0",
        b"enabled\0",
        b"yes\0",
        b"y\0",
        b"true\0",
        b"t\0",
    ];
    const FALSE_SPELLINGS: [&[u8]; 8] = [
        b"0\0",
        b"off\0",
        b"disable\0",
        b"disabled\0",
        b"no\0",
        b"n\0",
        b"false\0",
        b"f\0",
    ];
    for (flag, table) in [(1u8, &TRUE_SPELLINGS), (0u8, &FALSE_SPELLINGS)] {
        for spelling in table {
            if heads_match(text, len, spelling) {
                *out = flag;
                return 1;
            }
        }
    }
    0
}

/// The client's boolean parser applied to one config field.
///
/// The field is already trimmed by the table reader, and the client has no
/// third answer here: a spelling that matches nothing leaves `flag` at its
/// default, which is `false` - the same result the client's callers get from
/// their zeroed flag byte.
fn parse_bool(value: &str) -> bool {
    let mut flag = 0u8;
    parse_bool_string(value.as_bytes(), value.len(), &mut flag) != 0 && flag != 0
}

fn read_flavor(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let mut lines = text.lines();
    lines.next()?; // header
    let row = lines.find(|line| !line.trim().is_empty())?;
    let value = row.split('|').next()?.trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// Row stride of a `.build.info` row in the client's table: 0xb0 bytes per
/// row (`imul rbx, rax, 0xb0` at `0x1424aec7d` and `imul r11, rax, 0xb0` at
/// `0x1424aed1a`).
pub const BUILD_INFO_ROW_STRIDE: usize = 0xb0;

/// The client's row choice over already-extracted products, by row index.
///
/// `BuildInfoSelectRow` (`0x1424aec30`, measured extent
/// `0x1424aec30..0x1424aed8b`, 0x15b bytes through the `ret` at
/// `0x1424aed8a`; the `.pdata` root is only `0x1424aec30..0x1424aec6a`)
/// takes the table in RCX (rows pointer at `+0x20`, count `u32` at `+0x28`)
/// and the flavor pointer in RDX (null allowed), and returns a row pointer
/// or null. The count is `esi = [rcx+0x28]`; a zero count returns null at
/// once (`test esi, esi` / `je 0x1424aed7b`). A null flavor pointer or a
/// flavor whose first byte is zero (`test rdx, rdx` / `je`, then
/// `test dil, dil` / `je` to `0x1424aecf1`) takes the no-flavor path; any
/// other flavor is compared byte for byte (first-char pre-check at
/// `0x1424aeca2`, then the walk at `0x1424aecb0`), case-sensitively, with
/// no trimming or folding. Product is the qword at row `+0` (not Branch
/// at `+8`); a null Product pointer selects the default at `0x142f7aa00`,
/// whose first byte is zero, so it compares as the empty string
/// (`cmovne` at `0x1424aec8d` and `0x1424aed2e`). The active byte is at
/// row `+0x10`.
///
/// With a flavor it returns the first active row whose Product equals it;
/// the match stores the row pointer and returns it. Without a flavor it
/// takes the first row's Product as the baseline, requires every row
/// (active or not) to equal it - the compare runs before the active test,
/// so a mixed-product table returns null even when the differing row is
/// inactive - and returns the last active row. An empty flavor is the
/// no-flavor path, so `Some("")` behaves as `None`.
pub fn select_build_info_index(
    products: &[&[u8]],
    active: &[bool],
    flavor: Option<&[u8]>,
) -> Option<usize> {
    if products.is_empty() {
        return None;
    }
    let flavor = flavor.filter(|f| !f.is_empty() && f[0] != 0);
    match flavor {
        Some(flavor) => (0..products.len()).find(|&i| active[i] && products[i] == flavor),
        None => {
            let first = products[0];
            if products.iter().any(|product| *product != first) {
                return None;
            }
            (0..products.len()).rev().find(|&i| active[i])
        }
    }
}

/// The byte view of `BuildInfoSelectRow` over one table blob.
///
/// `table` is the RCX object: the rows pointer (u64) at `+0x20` and the
/// row count (u32) at `+0x28`. Every pointer inside the blob is absolute
/// against `table_base` (the arena base the probe models the blob at);
/// a null Product pointer is the client's empty-string default. Returns
/// the selected row's byte offset from the rows base (`index * 0xb0`), or
/// `None` for the client's null return.
pub fn select_build_info_row(
    table: &[u8],
    table_base: u64,
    flavor: Option<&[u8]>,
) -> Option<usize> {
    let count = u32::from_le_bytes(table[0x28..0x2c].try_into().unwrap());
    let rows_base = u64::from_le_bytes(table[0x20..0x28].try_into().unwrap());
    let rows_off = rows_base.wrapping_sub(table_base) as usize;
    let mut products: Vec<&[u8]> = Vec::with_capacity(count as usize);
    let mut active: Vec<bool> = Vec::with_capacity(count as usize);
    for i in 0..count {
        let row_off = rows_off + (i as usize) * BUILD_INFO_ROW_STRIDE;
        let product_ptr = u64::from_le_bytes(table[row_off..row_off + 8].try_into().unwrap());
        if product_ptr == 0 {
            products.push(&[]);
        } else {
            let off = product_ptr.wrapping_sub(table_base) as usize;
            let rest = &table[off..];
            let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
            products.push(&rest[..end]);
        }
        active.push(table[row_off + 0x10] != 0);
    }
    select_build_info_index(&products, &active, flavor).map(|i| i * BUILD_INFO_ROW_STRIDE)
}

/// The client's row choice (`BuildInfoSelectRow`, `0x1424aec30`).
///
/// The first field of a row is **Product** (descriptor offset 0; Branch is
/// at +8, Active at +0x10). With a flavor, the client returns the first
/// active row whose Product equals it. Without a flavor it requires every
/// row to carry the same Product and returns the **last active** row;
/// a mixed-product table has no answer and the client returns null.
fn select_build_info(rows: &[BuildInfo], flavor: Option<&str>) -> Result<BuildInfo> {
    let products: Vec<&[u8]> = rows.iter().map(|row| row.product.as_bytes()).collect();
    let active: Vec<bool> = rows.iter().map(|row| row.active).collect();
    let index = select_build_info_index(&products, &active, flavor.map(str::as_bytes));
    index
        .map(|i| rows[i].clone())
        .ok_or_else(|| Error::Parse("no usable branch row".into()))
}

/// An opened installation.
pub struct Install {
    root: PathBuf,
    pub build_info: BuildInfo,
    pub build_config: Config,
    /// The CDN configuration, when the file is present.
    ///
    /// Nothing in a local read needs it — it describes remote downloads — so its
    /// absence must not be fatal. It is not: during a patch the agent deletes the
    /// outgoing build's config before rewriting `.build.info`, which leaves the
    /// install briefly pointing at a file that no longer exists.
    pub cdn_config: Option<Config>,
    /// Keyring entries: key id -> 16-byte key, for encrypted BLTE chunks.
    pub keyring: BTreeMap<u64, [u8; 16]>,
    /// Remote fetching, present only when `OW_CDN` enabled it. See
    /// [`crate::cdn`] for the environment and the routes.
    pub cdn: Option<CdnClient>,
}

impl Install {
    /// Open an install, verifying that the expected directories exist.
    pub fn open(root: impl AsRef<Path>) -> Result<Install> {
        let root = root.as_ref();
        let discovered = discover_support_keyring(root);
        Install::open_with_support_keyring(root, discovered.as_deref())
    }

    /// Open an install, with an extra keyring supplied by the caller.
    ///
    /// The build's own keyring omits a few keys; a supplementary one fills the
    /// gap. `OW_SUPPORT_KEYRING` still wins when set, so an explicit choice is
    /// never overridden by discovery.
    pub fn open_with_support_keyring(
        root: impl AsRef<Path>,
        discovered: Option<&Path>,
    ) -> Result<Install> {
        let given = root.as_ref().to_path_buf();
        let (root, flavor) = resolve_layout(&given)?;
        let build_info_path = root.join(".build.info");
        let casc = root.join("data").join("casc");
        if !build_info_path.is_file() || !casc.is_dir() {
            return Err(Error::NotAnInstall(root));
        }

        let rows = BuildInfo::parse(&fs::read_to_string(&build_info_path)?)?;
        let build_info = select_build_info(&rows, flavor.as_deref())?;

        // Remote fetching is opt-in and decided before any config is read, so
        // a config that is absent locally can come from the CDN too. With
        // `OW_CDN` unset this stays `None` and every path below is unchanged.
        let cdn = if cdn::enabled() {
            let client = CdnClient::from_env(&build_info.cdn_hosts, &build_info.cdn_path)?;
            client.announce();
            Some(client)
        } else {
            None
        };

        let build_config = read_config_or_fetch(&root, &build_info.build_key, &cdn)?;
        let cdn_config = match read_config_or_fetch(&root, &build_info.cdn_key, &cdn) {
            Ok(config) => Some(config),
            Err(e) => {
                // Worth saying out loud, because it is the signature of an
                // install mid-patch rather than of a broken install.
                eprintln!(
                    "note: the CDN config for build {} is unreadable ({e});                      a local read does not need it",
                    build_info.version
                );
                None
            }
        };
        let mut keyring = match &build_info.keyring_key {
            Some(key) => read_keyring_or_fetch(&root, key, &cdn)?,
            None => BTreeMap::new(),
        };

        // A few assets are encrypted with keys the build's own keyring does not
        // carry. Those live in the community keyring file the reference
        // library ships; rather than vendor it, read the copy on the user's own
        // machine. Keys already present win, so the install stays authoritative.
        let support = std::env::var("OW_SUPPORT_KEYRING")
            .ok()
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .or_else(|| discovered.map(Path::to_path_buf));
        if let Some(path) = support {
            if path.is_file() {
                for (id, key) in read_keyring(&path)? {
                    keyring.entry(id).or_insert(key);
                }
            }
        }

        Ok(Install {
            root,
            build_info,
            build_config,
            cdn_config,
            keyring,
            cdn,
        })
    }

    /// Default install location, overridable with `OW_INSTALL`.
    ///
    /// Auto-detection is deliberately conservative: an explicit path is far
    /// more reliable than guessing at drive letters, so the candidate list is
    /// short and each one is verified by the presence of `.build.info`.
    pub fn detect() -> Result<Install> {
        match detect_root() {
            Some(root) => Install::open(root),
            None => Err(Error::Parse(
                "no Overwatch install found; set OW_INSTALL to the folder containing .build.info"
                    .into(),
            )),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn casc_dir(&self) -> PathBuf {
        self.root.join("data").join("casc")
    }

    pub fn data_dir(&self) -> PathBuf {
        self.casc_dir().join("data")
    }

    pub fn indices_dir(&self) -> PathBuf {
        self.casc_dir().join("indices")
    }
}

/// The install root to use: `OW_INSTALL`, else the first candidate that exists.
///
/// Shared so every binary agrees on which install it is talking about, rather
/// than each carrying its own idea of the default.
pub fn detect_root() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("OW_INSTALL") {
        if !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }
    DEFAULT_PATHS
        .iter()
        .map(PathBuf::from)
        .find(|p| p.join(".build.info").is_file())
}

/// Tried only when `OW_INSTALL` is unset.
const DEFAULT_PATHS: &[&str] = &[
    r"C:\Program Files (x86)\Overwatch",
    r"C:\Program Files\Overwatch",
    r"D:\Overwatch",
    r"E:\Overwatch",
];

fn discover_support_keyring(root: &Path) -> Option<PathBuf> {
    if let Ok(p) = std::env::var("OW_SUPPORT_KEYRING") {
        if !p.is_empty() {
            let path = PathBuf::from(p);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    let mut candidates: Vec<PathBuf> = Vec::new();
    candidates.push(root.join("Overwatch.keyring"));
    if let Some(parent) = root.parent() {
        candidates.push(parent.join("Overwatch.keyring"));
    }
    candidates.push(PathBuf::from(
        r"E:\CASCReader\OWLib\TankLib\Overwatch.keyring",
    ));
    candidates.push(PathBuf::from(r"D:\ow-dumps\Overwatch.keyring"));
    candidates.into_iter().find(|c| c.is_file())
}

fn config_path(root: &Path, key: &str) -> PathBuf {
    // Config files live two levels deep, named by their own first four hex
    // digits. ASCII-checked before slicing: a config key with a multi-byte
    // character takes the flat path instead of panicking.
    if key.len() < 4 || !key.is_ascii() {
        return root.join("data").join("casc").join("config").join(key);
    }
    root.join("data")
        .join("casc")
        .join("config")
        .join(&key[0..2])
        .join(&key[2..4])
        .join(key)
}

pub fn read_config(path: &Path) -> Result<Config> {
    let text = fs::read_to_string(path).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => Error::Missing(path.to_path_buf()),
        _ => Error::Io(e),
    })?;
    Ok(Config::parse(&text))
}

/// Read a config from the install; when fetching is enabled and the file is
/// absent locally, fetch it from the CDN's config route and parse that.
///
/// The fetched copy is used in memory only — the installation is never
/// written to.
fn read_config_or_fetch(root: &Path, key: &str, cdn: &Option<CdnClient>) -> Result<Config> {
    match read_config(&config_path(root, key)) {
        Ok(config) => Ok(config),
        Err(local) => match cdn {
            Some(cdn) => {
                let bytes = cdn.fetch_config(key)?;
                let text = String::from_utf8(bytes)
                    .map_err(|e| Error::Parse(format!("CDN config {key} is not utf-8: {e}")))?;
                eprintln!("note: config {key} came from the CDN (locally: {local})");
                Ok(Config::parse(&text))
            }
            None => Err(local),
        },
    }
}

/// The keyring counterpart of [`read_config_or_fetch`].
fn read_keyring_or_fetch(
    root: &Path,
    key: &str,
    cdn: &Option<CdnClient>,
) -> Result<BTreeMap<u64, [u8; 16]>> {
    match read_keyring(&config_path(root, key)) {
        Ok(keys) => Ok(keys),
        Err(local) => match cdn {
            Some(cdn) => {
                let bytes = cdn.fetch_config(key)?;
                let text = String::from_utf8(bytes)
                    .map_err(|e| Error::Parse(format!("CDN keyring {key} is not utf-8: {e}")))?;
                eprintln!("note: keyring {key} came from the CDN (locally: {local})");
                Ok(parse_keyring(&text))
            }
            None => Err(local),
        },
    }
}

/// Parse a keyring config.
///
/// Two spellings exist and they disagree about byte order, so both are handled:
///
/// * The build's own keyring, which the installer writes, uses
///   `key-<16 hex> = <32 hex>`. The id's hex is stored big-endian and addressed
///   little-endian, so it is byte-swapped on the way in.
/// * The community "support" keyring uses plain
///   `<16 hex id> <32 hex key> [1|0] [# comment]`, separated by spaces, and the
///   id is used as written.
///
/// A support keyring supplies keys a build's own keyring omits. It is read from
/// a path the user provides; none of it is carried in this repository.
fn read_keyring(path: &Path) -> Result<BTreeMap<u64, [u8; 16]>> {
    let text = fs::read_to_string(path).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => Error::Missing(path.to_path_buf()),
        _ => Error::Io(e),
    })?;
    Ok(parse_keyring(&text))
}

/// Parse keyring text. The two spellings are described on [`read_keyring`].
fn parse_keyring(text: &str) -> BTreeMap<u64, [u8; 16]> {
    let config_style = text.lines().any(|l| {
        l.split('#')
            .next()
            .unwrap_or("")
            .trim_start()
            .starts_with("key-")
    });

    let mut keys = BTreeMap::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }

        if config_style {
            let Some((name, value)) = line.split_once('=') else {
                continue;
            };
            let name = name.trim();
            let Some(hex_id) = name.strip_prefix("key-") else {
                continue;
            };
            let hex_id = if hex_id.len() > 16 {
                // `.get`, not slicing: a non-ASCII id must fall through to
                // the parse failure below, not panic on a char boundary.
                match hex_id.get(hex_id.len() - 16..) {
                    Some(tail) => tail,
                    None => continue,
                }
            } else {
                hex_id
            };
            let Ok(id) = u64::from_str_radix(hex_id, 16) else {
                continue;
            };
            if let Some(key) = parse_key(value.trim()) {
                // stored big-endian, addressed little-endian
                keys.insert(id.swap_bytes(), key);
            }
        } else {
            let mut parts = line.split_whitespace();
            let (Some(id_hex), Some(value)) = (parts.next(), parts.next()) else {
                continue;
            };
            let enabled = parts.next().map(|flag| flag != "0").unwrap_or(true);
            if !enabled {
                continue;
            }
            let Ok(id) = u64::from_str_radix(id_hex, 16) else {
                continue;
            };
            if let Some(key) = parse_key(value) {
                keys.insert(id, key);
            }
        }
    }
    keys
}

/// The client's hex-pair decoder: `0x14269ab80`, read operation for operation.
///
/// Decodes up to `count` bytes from `text` into `out`, two characters per byte,
/// and returns the bytes written. A pair needs two characters (the client's
/// `index >= len - 1` guard), a zero byte ends the run, a character outside
/// `0-9a-fA-F` ends it, and the loop leaves as soon as `count` bytes are done.
/// A short result is the caller's only signal that the text ran out or held a
/// non-hex character. The client shifts the high character with `shl cl, 4` on
/// a byte, so a digit's ASCII value keeps only its low nibble - which is the
/// digit's value - so values are used directly here.
///
/// Signature: the probe supplies `text` as exactly its `len` characters
/// (`&text[..len]`), so the slice length is the client's `len` (r9). The
/// client returns 0 itself for a null or zero-length text and for a zero
/// count; an empty slice and a zero count give the same 0 here. A null text
/// pointer cannot be represented by the arena probe and is a declared gap
/// in `fn/HexPairDecode`.
pub fn hex_decode(out: &mut [u8], count: usize, text: &[u8]) -> usize {
    let mut written = 0;
    let mut at = 0;
    while written < count {
        if at + 1 >= text.len() {
            break;
        }
        let Some(high) = hex_value(text[at]) else {
            break;
        };
        let Some(low) = hex_value(text[at + 1]) else {
            break;
        };
        out[written] = (high << 4) + low;
        written += 1;
        at += 2;
    }
    written
}

/// One hex digit's value, or `None` for anything else: the three range tests of
/// `0x14269ab80` (`0x30..=0x39`, then `0x61..=0x66`, then `0x41..=0x46`).
fn hex_value(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// The leading whitespace of a key string: space, tab, LF, VT, FF or CR, the
/// client's `cmp al, 0x20` then `sub al, 9; cmp al, 4` test at `0x14248eff0`.
fn leading_whitespace(text: &[u8]) -> usize {
    text.iter()
        .take_while(|c| **c == b' ' || c.wrapping_sub(b'\t') <= 4)
        .count()
}

/// The client's 16-byte key parser: `ParseHexKey` (`0x14248efd0`), read
/// operation for operation.
///
/// Zeroes `out`, zeroes `flag`, skips leading whitespace and decodes exactly 16
/// bytes from what follows. Returns the characters consumed - `whitespace + 32`
/// - or 0 when the decode produced fewer than 16 bytes.
///
/// The client reads `len` characters from `text`; a `len` past the buffer is a
/// caller bug here as well.
pub fn parse_hex_key(text: &[u8], len: usize, out: &mut [u8; 16], flag: &mut u8) -> usize {
    out.fill(0);
    *flag = 0;
    let skip = leading_whitespace(&text[..len]);
    if hex_decode(out, 16, &text[skip..len]) == 16 {
        skip + 32
    } else {
        0
    }
}

/// The client's 8-byte id parser: `ParseHexKeyId` (`0x14248f040`), read operation
/// for operation. The same body as [`parse_hex_key`] with an 8-byte target: the
/// success count is `whitespace + 16`.
pub fn parse_hex_key_id(text: &[u8], len: usize, out: &mut [u8; 8], flag: &mut u8) -> usize {
    out.fill(0);
    *flag = 0;
    let skip = leading_whitespace(&text[..len]);
    if hex_decode(out, 8, &text[skip..len]) == 8 {
        skip + 16
    } else {
        0
    }
}

/// The whitespace set every scan in the config line parser uses: `0x20`, or a
/// byte `c` with `c.wrapping_sub(9) <= 4` - the client's `cmp al, 0x20`
/// follows-by-`sub al, 9` / `cmp al, 4` pair (TAB, LF, VT, FF, CR).
fn config_space(c: u8) -> bool {
    c == b' ' || c.wrapping_sub(9) <= 4
}

/// One appended config key entry: the client's 24-byte record.
pub const CONFIG_KEY_ENTRY_SIZE: usize = 24;

/// `ConfigKeyEntryParse` (`0x1424cebd0`): one `key-<id> = <key>` config line.
///
/// The client's argument is a context object whose `+8` field points at a
/// vector `{allocator, base, count: u32 @+0x10, capacity: u32 @+0x14}`; the
/// vector holds `{u64 id, [u8; 16] key}` records of 24 bytes. The four span
/// arguments are raw pointers, so this byte view takes one arena blob (the
/// context at offset 0) and the four absolute addresses the client was handed,
/// exactly as it holds them: the span length it tests is a **32-bit**
/// subtraction of two pointers (`sub eax, edx`), which an inverted span makes
/// large, while the scans that follow compare the full 64-bit pointers.
///
/// The disposition is the client's `AL`: `0` handled, `1` not handled. The
/// entry is appended **before** either field is parsed, so a line that fails
/// the id parse still grows the vector by one zeroed record.
///
/// Stage by stage, with the instruction that decides each one:
///
/// 1. `cmp byte ptr [rdx], 0x6b` / `jne` (`0x1424cebdd`, `0x1424cebe3`): the
///    key span must start with `k`.
/// 2. `sub eax, edx` / `cmp eax, 4` / `jbe` (`0x1424cebf4`, `0x1424cebfe`):
///    the span length is the 32-bit pointer difference; four or fewer is not
///    a key line.
/// 3. `mov eax, dword ptr [rdx]` / `cmp eax, 0x2d79656b` (`0x1424cec04`,
///    `0x1424cec0a`): the first four bytes are `key-`, compared as a
///    little-endian dword.
/// 4. The token scan (`0x1424cec19..0x1424cec34`) starts at `key_begin + 4`
///    and stops at `key_end`, at `0x20`, or at any other [`config_space`]
///    byte; that end is `token_end`.
/// 5. The vector grow (`cmp eax, ecx` / `jne` at `0x1424cec42`): when the
///    count equals the capacity the client reallocates through the allocator
///    vtable (`[[vec]] + 0x10` alloc, `+0x30` free) and recomputes the
///    capacity by multiplying the eight bytes at `[rsp+0x48]` - the `key-`
///    tag literal, whose high dword is never written - by
///    `0xaaaaaaaaaaaaaaab`. That block is outside the system: this function
///    panics there and the unit declares the edge dead.
/// 6. The append (`0x1424cecdd..0x1424ced13`): three zeroed qwords at
///    `base + count * 24`, then `count + 1` stored back (32-bit).
/// 7. `ParseHexKeyId(p, token_end - p, entry, &flag)` (`0x1424ced38`) over the
///    first non-whitespace byte of the token; a non-zero `flag` returns `1`
///    (`0x1424ced42`) and a zero consumed count returns `1` (`0x1424ced47`).
///    The client's parser only ever zeroes `flag`, so that edge is dead.
/// 8. `val_begin >= val_end` returns `0` (`0x1424ced51`); otherwise the leading
///    whitespace is skipped (`0x1424ced53..0x1424ced67`, running off the end
///    returns `0`).
/// 9. `ParseHexKey(q, val_end - q, entry + 8, &flag)` (`0x1424ced8c`): a
///    non-zero `flag` returns `0` (`0x1424ced96`), a non-zero consumed count
///    returns `1` (`0x1424ced9b`), and a zero consumed count returns `0`
///    (`0x1424ceda1`).
///
/// So a line whose id parses and whose value is 32 hex digits returns `1`, and
/// a line whose value is too short returns `0` - the disposition is not the
/// parse result. `ow-casc` has no caller yet: the client's config-line
/// dispatcher (which splits a line into these two spans) is not read, and the
/// crate's own keyring reader stays as it is until it is.
pub fn config_key_entry_parse(
    blob: &mut [u8],
    blob_base: u64,
    key_begin: u64,
    key_end: u64,
    val_begin: u64,
    val_end: u64,
) -> u8 {
    // A pointer into the blob, as an index. Every span the client was handed
    // is inside the arena the probe copies, so a pointer outside it is a
    // fixture error, not an edge.
    let at = |p: u64| (p.wrapping_sub(blob_base)) as usize;
    let kb = at(key_begin);
    // 1-3. The key span: `k`, longer than four bytes in 32-bit pointer
    // arithmetic, and `key-`.
    if blob[kb] != b'k' {
        return 1;
    }
    if (key_end as u32).wrapping_sub(key_begin as u32) <= 4 {
        return 1;
    }
    if u32::from_le_bytes(blob[kb..kb + 4].try_into().unwrap()) != 0x2d79_656b {
        return 1;
    }
    // 4. The token end: the first whitespace at or after `key_begin + 4`.
    let mut token_end = key_begin.wrapping_add(4);
    while token_end < key_end && !config_space(blob[at(token_end)]) {
        token_end = token_end.wrapping_add(1);
    }
    // 5. The vector object.
    let vec_ptr = u64::from_le_bytes(blob[8..16].try_into().unwrap());
    let vec = at(vec_ptr);
    let count = u32::from_le_bytes(blob[vec + 0x10..vec + 0x14].try_into().unwrap());
    let capacity = u32::from_le_bytes(blob[vec + 0x14..vec + 0x18].try_into().unwrap());
    if count == capacity {
        // The two compares the client makes: `cmp eax, ecx` / `jne` at
        // 0x1424cec44 skips this block unless the count equals the capacity,
        // and `cmp r8d, ecx` / `jbe` at 0x1424cec51 skips the grow unless
        // count + 1 exceeds it (32-bit, so the wrap at 0xffffffff skips too).
        if count.wrapping_add(1) > capacity {
            panic!(
                "config_key_entry_parse: vector grow at 0x1424cec53 is outside the system \
                 (allocator vtable and a capacity computed from uninitialised stack bytes)"
            );
        }
    }
    // 6. The append.
    let base = u64::from_le_bytes(blob[vec + 8..vec + 0x10].try_into().unwrap());
    let entry = at(base) + count as usize * CONFIG_KEY_ENTRY_SIZE;
    blob[entry..entry + CONFIG_KEY_ENTRY_SIZE].fill(0);
    blob[vec + 0x10..vec + 0x14].copy_from_slice(&count.wrapping_add(1).to_le_bytes());
    // 7. The id: the first non-whitespace byte of the token onward.
    let mut p = key_begin.wrapping_add(4);
    while p < token_end && config_space(blob[at(p)]) {
        p = p.wrapping_add(1);
    }
    if p >= token_end {
        return 1;
    }
    let from = at(p);
    let upto = at(token_end);
    let mut id = [0u8; 8];
    let mut flag = 0u8;
    let consumed = parse_hex_key_id(&blob[from..upto], upto - from, &mut id, &mut flag);
    blob[entry..entry + 8].copy_from_slice(&id);
    if flag != 0 {
        return 1;
    }
    if consumed == 0 {
        return 1;
    }
    // 8. The value span.
    if val_begin >= val_end {
        return 0;
    }
    let mut q = val_begin;
    while config_space(blob[at(q)]) {
        q = q.wrapping_add(1);
        if q >= val_end {
            return 0;
        }
    }
    // 9. The key.
    let from = at(q);
    let upto = at(val_end);
    let mut key16 = [0u8; 16];
    let mut flag = 0u8;
    let consumed = parse_hex_key(&blob[from..upto], upto - from, &mut key16, &mut flag);
    blob[entry + 8..entry + CONFIG_KEY_ENTRY_SIZE].copy_from_slice(&key16);
    if flag != 0 {
        return 0;
    }
    if consumed != 0 {
        return 1;
    }
    0
}

fn parse_key(hex: &str) -> Option<[u8; 16]> {
    // The client's parser, with one extra demand: a keyring field must be
    // consumed entirely, so trailing text is a parse failure rather than a
    // silently ignored tail (the client's `ParseHexKey` callers walk a cursor
    // and consume the count, which this one-shot wrapper does not).
    let text = hex.as_bytes();
    let mut out = [0u8; 16];
    let mut flag = 0u8;
    let consumed = parse_hex_key(text, text.len(), &mut out, &mut flag);
    (consumed == text.len()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(product: &str, branch: &str, active: bool) -> BuildInfo {
        BuildInfo {
            branch: branch.to_string(),
            active,
            build_key: String::new(),
            cdn_key: String::new(),
            keyring_key: None,
            version: String::new(),
            product: product.to_string(),
            cdn_path: String::new(),
            cdn_hosts: Vec::new(),
        }
    }

    #[test]
    fn build_info_boolean_spellings_match_the_client() {
        let text = "Branch!STRING:0|Active!DEC:1|Product!STRING:0\n\
                    eu|true|pro\n\
                    us|0|pro\n\
                    kr|enabled|pro\n";
        let rows = BuildInfo::parse(text).expect("parses");
        assert_eq!(
            rows.iter().map(|r| r.active).collect::<Vec<_>>(),
            vec![true, false, true]
        );
    }

    #[test]
    fn boolean_parser_is_a_prefix_match_with_no_third_answer_to_callers() {
        // The client's walk: eight true spellings then eight false ones, a
        // match at either side's end, and the flag written only on a match.
        let mut flag = 0xEE;
        assert_eq!(parse_bool_string(b"t", 1, &mut flag), 1);
        assert_eq!(flag, 1);
        assert_eq!(parse_bool_string(b"enab", 4, &mut flag), 1);
        assert_eq!(flag, 1);
        // The text must be a prefix of a spelling, not the other way round.
        flag = 0xEE;
        assert_eq!(parse_bool_string(b"enablex", 7, &mut flag), 0);
        assert_eq!(flag, 0xEE);
        assert_eq!(parse_bool_string(b"OFF", 3, &mut flag), 1);
        assert_eq!(flag, 0);
        // `on` is tried before `off`; nothing trims.
        assert_eq!(parse_bool_string(b"o", 1, &mut flag), 1);
        assert_eq!(flag, 1);
        assert_eq!(parse_bool_string(b"of", 2, &mut flag), 1);
        assert_eq!(flag, 0);
        // No match: status 0 and the flag untouched.
        flag = 0xEE;
        assert_eq!(parse_bool_string(b" true", 5, &mut flag), 0);
        assert_eq!(flag, 0xEE);
    }

    #[test]
    fn flavor_selects_the_first_active_row_for_that_product() {
        let rows = [
            row("prodev", "eu", true),
            row("pro", "eu", false),
            row("pro", "us", true),
            row("pro", "eu", true),
        ];
        // The inactive `eu` row at index 1 is skipped; the first active `pro`
        // row is the `us` one.
        let picked = select_build_info(&rows, Some("pro")).expect("pro row");
        assert_eq!(picked.branch, "us");
        assert!(picked.active);
        assert_eq!(picked.product, "pro");
    }

    #[test]
    fn without_a_flavor_the_last_active_row_wins_but_products_must_agree() {
        let rows = [
            row("pro", "eu", true),
            row("pro", "us", true),
            row("pro", "kr", false),
        ];
        let picked = select_build_info(&rows, None).expect("same product");
        assert_eq!(picked.branch, "us");

        let mixed = [row("pro", "eu", true), row("prodev", "us", true)];
        assert!(select_build_info(&mixed, None).is_err());
    }

    /// The client's `key-` config handler (`0x1424cebd0`) parses the name's
    /// last 8 bytes with `0x14248f040` and the value's 16 bytes with
    /// `0x14248efd0`. The id is addressed as a little-endian u64 by the BLTE
    /// E-block lookup, so the written big-endian hex appears byte-swapped.
    /// The client's key text leaves: whitespace skip, then 16 (or 8) decoded
    /// bytes, with the consumed character count as the success signal.
    #[test]
    fn hex_key_parsers_skip_whitespace_and_report_the_count() {
        let mut out = [0u8; 16];
        let mut flag = 0u8;
        assert_eq!(
            parse_hex_key(b"0123456789abcdef0123456789abcdef", 32, &mut out, &mut flag),
            32
        );
        assert_eq!(out[0], 0x01);
        assert_eq!(out[15], 0xef);

        // Leading whitespace is skipped and counted; the case of the digits and
        // a tail past the 32 consumed characters are both accepted.
        assert_eq!(
            parse_hex_key(
                b" \t\r\nABCDEF0123456789abcdef0123456789 tail",
                41,
                &mut out,
                &mut flag
            ),
            36
        );
        assert_eq!(out[0], 0xab);

        // A non-hex character or a NUL stops the run: the count is 0, the
        // output is zeroed first, and bytes decoded before the stop stay
        // (the client decodes in place too).
        out = [0xEEu8; 16];
        assert_eq!(
            parse_hex_key(b"0123456789abcdef0123456789abcdeg", 32, &mut out, &mut flag),
            0
        );
        assert_eq!(out[15], 0x00);
        out = [0xEEu8; 16];
        let mut planted = b"0123456789abcdef0123456789abcdef".to_vec();
        planted[16] = 0x01;
        assert_eq!(parse_hex_key(&planted, 32, &mut out, &mut flag), 0);
        assert_eq!(out[8], 0x00);
        out = [0xEEu8; 16];
        assert_eq!(parse_hex_key(b"   ", 3, &mut out, &mut flag), 0);
        assert_eq!(out, [0u8; 16]);
        out = [0xEEu8; 16];
        assert_eq!(
            parse_hex_key(b"0123456789abcdef0123456789abcde", 31, &mut out, &mut flag),
            0
        );
        assert_eq!(&out[..8], &[0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef]);
        assert_eq!(&out[8..15], &[0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd]);
        assert_eq!(out[15], 0x00);

        let mut id = [0u8; 8];
        assert_eq!(
            parse_hex_key_id(b" 0011223344556677", 17, &mut id, &mut flag),
            17
        );
        assert_eq!(id, [0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77]);
        assert_eq!(
            parse_hex_key_id(b"001122334455667", 15, &mut id, &mut flag),
            0
        );
        assert_eq!(&id[..7], &[0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
        assert_eq!(id[7], 0x00);
    }

    #[test]
    fn keyring_spellings_have_opposite_byte_order() {
        let key = parse_key("000102030405060708090a0b0c0d0e0f").expect("16-byte key");
        let config = "key-1b3e4e1ecfb25877 = 000102030405060708090a0b0c0d0e0f\n";
        let keys = parse_keyring(config);
        assert_eq!(keys.get(&0x7758_b2cf_1e4e_3e1b), Some(&key));

        let support = "A5F6830F5BE13A9E 000102030405060708090a0b0c0d0e0f 1\n";
        let keys = parse_keyring(support);
        assert_eq!(keys.get(&0xA5F6_830F_5BE1_3A9E), Some(&key));
    }

    #[test]
    fn flavor_file_and_parent_build_info_are_resolved() {
        let root = std::env::temp_dir().join(format!(
            "ow-casc-layout-{}-{}",
            std::process::id(),
            "flavor"
        ));
        let sub = root.join("_retail_");
        fs::create_dir_all(&sub).expect("create temp tree");
        fs::write(root.join(".build.info"), "Branch!STRING:0\n").expect("build info");
        fs::write(sub.join(".flavor.info"), "Product Flavor!STRING:0\npro\n").expect("flavor info");

        let (resolved, flavor) = resolve_layout(&sub).expect("resolves");
        assert_eq!(resolved, root);
        assert_eq!(flavor.as_deref(), Some("pro"));

        let (resolved, flavor) = resolve_layout(&root).expect("resolves local");
        assert_eq!(resolved, root);
        assert_eq!(flavor, None);

        fs::remove_dir_all(&root).ok();
    }
}

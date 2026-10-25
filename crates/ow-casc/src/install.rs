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
/// The client's boolean parser (`0x1424d8800`) accepts these true spellings;
/// everything else writes zero. The decimal `1` is what `.build.info` uses,
/// but a build is free to write any of them.
fn parse_bool(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on" | "enable" | "enabled"
    )
}

fn read_flavor(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let mut lines = text.lines();
    lines.next()?; // header
    let row = lines.find(|line| !line.trim().is_empty())?;
    let value = row.split('|').next()?.trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// The client's row choice (`BuildInfoSelectRow`, `0x1424aec30`).
///
/// The first field of a row is **Product** (descriptor offset 0; Branch is
/// at +8, Active at +0x10). With a flavor, the client returns the first
/// active row whose Product equals it. Without a flavor it requires every
/// row to carry the same Product and returns the **last active** row;
/// a mixed-product table has no answer and the client returns null.
fn select_build_info(rows: &[BuildInfo], flavor: Option<&str>) -> Result<BuildInfo> {
    let selected = match flavor {
        Some(flavor) => rows.iter().find(|row| row.active && row.product == flavor),
        None => {
            let product = &rows
                .first()
                .ok_or_else(|| Error::Parse("empty .build.info".into()))?
                .product;
            if rows.iter().all(|row| &row.product == product) {
                rows.iter().rev().find(|row| row.active)
            } else {
                None
            }
        }
    };
    selected
        .cloned()
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

fn parse_key(hex: &str) -> Option<[u8; 16]> {
    // ASCII before the two-byte slicing below: a multi-byte character is a
    // parse failure, not a panic.
    if hex.len() != 32 || !hex.is_ascii() {
        return None;
    }
    let mut out = [0u8; 16];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
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

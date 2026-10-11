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

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    /// No usable install was found at the given path.
    NotAnInstall(PathBuf),
    /// A file the install's own metadata points at is missing.
    Missing(PathBuf),
    Parse(String),
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
            Error::Missing(p) => write!(f, "missing file referenced by the install: {}", p.display()),
            Error::Parse(m) => write!(f, "malformed install metadata: {m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
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
}

impl BuildInfo {
    /// Parse `.build.info`: a header row naming columns, then one row per branch.
    ///
    /// Column names carry a `!TYPE:WIDTH` suffix which we strip.
    pub fn parse(text: &str) -> Result<Vec<BuildInfo>> {
        let mut lines = text.lines();
        let header = lines.next().ok_or_else(|| Error::Parse(".build.info is empty".into()))?;
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
                active: row.get("Active").copied() == Some("1"),
                build_key: row.get("Build Key").copied().unwrap_or("").to_string(),
                cdn_key: row.get("CDN Key").copied().unwrap_or("").to_string(),
                keyring_key: row
                    .get("KeyRing")
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty()),
                version: row.get("Version").copied().unwrap_or("").to_string(),
                product: row.get("Product").copied().unwrap_or("").to_string(),
            });
        }
        if out.is_empty() {
            return Err(Error::Parse(".build.info has no branch rows".into()));
        }
        Ok(out)
    }
}

/// An opened installation.
pub struct Install {
    root: PathBuf,
    pub build_info: BuildInfo,
    pub build_config: Config,
    pub cdn_config: Config,
    /// Keyring entries: key id -> 16-byte key, for encrypted BLTE chunks.
    pub keyring: BTreeMap<u64, [u8; 16]>,
}

impl Install {
    /// Open an install, verifying that the expected directories exist.
    pub fn open(root: impl AsRef<Path>) -> Result<Install> {
        Install::open_with_support_keyring(root, None)
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
        let root = root.as_ref().to_path_buf();
        let build_info_path = root.join(".build.info");
        let casc = root.join("data").join("casc");
        if !build_info_path.is_file() || !casc.is_dir() {
            return Err(Error::NotAnInstall(root));
        }

        let rows = BuildInfo::parse(&fs::read_to_string(&build_info_path)?)?;
        // Prefer the active retail branch; fall back to the newest version.
        let build_info = rows
            .iter()
            .find(|r| r.active && r.product == "pro")
            .or_else(|| rows.iter().find(|r| r.active))
            .or_else(|| rows.first())
            .cloned()
            .ok_or_else(|| Error::Parse("no usable branch row".into()))?;

        let build_config = read_config(&config_path(&root, &build_info.build_key))?;
        let cdn_config = read_config(&config_path(&root, &build_info.cdn_key))?;
        let mut keyring = match &build_info.keyring_key {
            Some(key) => read_keyring(&config_path(&root, key))?,
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

fn config_path(root: &Path, key: &str) -> PathBuf {
    // config files live two levels deep, named by their own first four hex digits
    if key.len() < 4 {
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

    let config_style = text
        .lines()
        .any(|l| l.split('#').next().unwrap_or("").trim_start().starts_with("key-"));

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
                &hex_id[hex_id.len() - 16..]
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
    Ok(keys)
}

fn parse_key(hex: &str) -> Option<[u8; 16]> {
    if hex.len() != 32 {
        return None;
    }
    let mut out = [0u8; 16];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

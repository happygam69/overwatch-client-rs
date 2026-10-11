//! Ties the store together: bucket index, data archives, BLTE, encoding file.
//!
//! Lookup path for a content key is
//! `encoding file -> encoding key -> bucket index -> data.NNN -> BLTE`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

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
    encoding: RefCell<Option<Vec<u8>>>,
}

impl Store {
    pub fn open(install: Install) -> Result<Store> {
        let buckets = load_bucket_index(&install)?;
        Ok(Store {
            install,
            buckets,
            archives: RefCell::new(HashMap::new()),
            encoding: RefCell::new(None),
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
    pub fn open_ekey(&self, ekey: &[u8]) -> Result<Vec<u8>> {
        let stored = self.read_stored(ekey)?;
        Ok(blte::decode(&stored, &self.install.keyring)?)
    }

    /// Load and keep the decoded encoding file.
    fn ensure_encoding(&self) -> Result<()> {
        if self.encoding.borrow().is_some() {
            return Ok(());
        }
        let ekey = self
            .install
            .build_config
            .get("encoding")
            .and_then(|v| v.get(1))
            .ok_or_else(|| Error::Parse("build config has no encoding key".into()))?;
        let ekey = parse_hex_key(ekey)?;
        let blob = self.open_ekey(&ekey)?;
        *self.encoding.borrow_mut() = Some(blob);
        Ok(())
    }

    /// Decode a content key: encoding file to encoding key to record.
    pub fn open_ckey(&self, ckey: &[u8; 16]) -> Result<Vec<u8>> {
        self.ensure_encoding()?;
        let borrowed = self.encoding.borrow();
        let blob = borrowed.as_ref().expect("just loaded");
        let encoding = EncodingFile::parse(blob)?;

        let ekeys = encoding.ekeys_for_ckey(ckey);
        if ekeys.is_empty() {
            return Err(Error::KeyNotFound(hex(ckey)));
        }
        let mut last = None;
        for ekey in &ekeys {
            match self.open_ekey(ekey) {
                Ok(data) => return Ok(data),
                Err(e) => last = Some(e),
            }
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

    /// Declared decoded size for an encoding key, if the encoding file has it.
    pub fn decoded_size(&self, ekey: &[u8; 16]) -> Result<Option<u64>> {
        self.ensure_encoding()?;
        let borrowed = self.encoding.borrow();
        let blob = borrowed.as_ref().expect("just loaded");
        Ok(EncodingFile::parse(blob)?.decoded_size(ekey))
    }
}

/// Scan the sixteen bucket files into one map.
///
/// Every file whose name begins with a bucket number is read, so a bucket split
/// across several files still resolves.
fn load_bucket_index(install: &Install) -> Result<HashMap<[u8; 9], Location>> {
    let mut buckets = HashMap::new();
    let data_dir = install.data_dir();
    for entry in fs::read_dir(&data_dir)? {
        let entry = entry?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.ends_with(".idx") {
            continue;
        }
        let Ok(bucket) = u8::from_str_radix(&name[0..2], 16) else {
            continue;
        };
        if bucket >= 16 {
            continue;
        }
        for (ekey, location) in index::parse_bucket_file(&path, Some(bucket))? {
            buckets.insert(ekey, location);
        }
    }
    if buckets.is_empty() {
        return Err(Error::Parse(format!(
            "no .idx bucket files found in {}",
            data_dir.display()
        )));
    }
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

//! The asset package manifest: which assets ship together.
//!
//! The content manifest maps a GUID to bytes and says nothing about grouping.
//! The package manifest is the grouping: every asset belongs to a package, and
//! a package is a thing that is loaded as a unit -- a map, a hero, a game mode.
//! That is what makes it useful here: one chunk of the practice range was
//! recovered from a running game, and its package names the rest of the map.
//!
//! ```text
//! header          u64 build version, four u32, i32 package count,
//!                 u32, i32 entry count, u32 checksum
//! entries         entry count x 20 bytes
//! package entries package count x 24 bytes: GUID, key id, two u32
//! ```
//!
//! Each package is then an asset of its own, holding a header and a run of
//! records -- `u64 GUID, u32 flags` each -- **gzip-compressed** at an offset the
//! header gives. Reading those without decompressing yields plausible-looking
//! pairs of numbers, which is why the compression is called out.

use std::fmt;

use crate::guid;
use crate::{GameData, Result};

const HEADER_SIZE: usize = 40;
const ENTRY_SIZE: usize = 20;
const PACKAGE_ENTRY_SIZE: usize = 24;
const PACKAGE_HEADER_SIZE: usize = 72;
const RECORD_SIZE: usize = 12;

#[derive(Debug)]
pub enum Error {
    NoPackageManifest,
    Malformed(&'static str),
    Decompress(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NoPackageManifest => write!(f, "the root file lists no .apm package manifest"),
            Error::Malformed(m) => write!(f, "malformed package manifest: {m}"),
            Error::Decompress(m) => write!(f, "could not decompress package records: {m}"),
        }
    }
}

impl std::error::Error for Error {}

type EResult<T> = std::result::Result<T, Error>;

fn u32_at(d: &[u8], at: usize) -> Option<u32> {
    d.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}
fn i32_at(d: &[u8], at: usize) -> Option<i32> {
    u32_at(d, at).map(|v| v as i32)
}
fn i64_at(d: &[u8], at: usize) -> Option<i64> {
    d.get(at..at + 8)
        .map(|b| i64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
}
fn u64_at(d: &[u8], at: usize) -> Option<u64> {
    i64_at(d, at).map(|v| v as u64)
}

/// One package: its asset and the GUIDs it holds.
pub struct Package {
    pub guid: u64,
    pub records: Vec<u64>,
}

/// The packages, loaded lazily enough to stay usable on a real install.
pub struct Packages {
    /// GUID of each package asset, in manifest order.
    package_guids: Vec<u64>,
}

impl Packages {
    pub fn open(data: &GameData) -> EResult<Packages> {
        let root = data.root_file().map_err(|_| Error::NoPackageManifest)?;
        let line = root
            .lines()
            .find(|l| l.contains(".apm"))
            .ok_or(Error::NoPackageManifest)?;
        let fields: Vec<&str> = line.split('|').collect();
        let key = fields.get(1).ok_or(Error::Malformed("no key on the apm line"))?;
        let mut ckey = [0u8; 16];
        if key.len() != 32 {
            return Err(Error::Malformed("apm key is not 16 bytes"));
        }
        for (i, byte) in ckey.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&key[i * 2..i * 2 + 2], 16)
                .map_err(|_| Error::Malformed("apm key is not hex"))?;
        }
        let blob = data.open_ckey(&ckey).map_err(|_| Error::Malformed("apm unreadable"))?;

        let package_count = i32_at(&blob, 24).ok_or(Error::Malformed("truncated header"))?;
        let entry_count = i32_at(&blob, 32).ok_or(Error::Malformed("truncated header"))?;
        if package_count < 0 || entry_count < 0 {
            return Err(Error::Malformed("negative counts"));
        }

        let package_entries_at = HEADER_SIZE + entry_count as usize * ENTRY_SIZE;
        let mut package_guids = Vec::with_capacity(package_count as usize);
        for i in 0..package_count as usize {
            let at = package_entries_at + i * PACKAGE_ENTRY_SIZE;
            match u64_at(&blob, at) {
                Some(g) => package_guids.push(g),
                None => break,
            }
        }
        Ok(Packages { package_guids })
    }

    pub fn len(&self) -> usize {
        self.package_guids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.package_guids.is_empty()
    }

    /// The records of one package. Its bytes are opened through the registry,
    /// and the record run is gzip-compressed inside.
    pub fn load(&self, data: &GameData, index: usize) -> EResult<Package> {
        let guid = *self
            .package_guids
            .get(index)
            .ok_or(Error::Malformed("package index out of range"))?;
        let bytes = data
            .bytes_of(guid)
            .map_err(|_| Error::Malformed("package asset unreadable"))?;
        if bytes.len() < PACKAGE_HEADER_SIZE {
            return Err(Error::Malformed("package shorter than its header"));
        }
        let records_at = i64_at(&bytes, 0).ok_or(Error::Malformed("truncated package header"))?;
        let record_count =
            u32_at(&bytes, 48).ok_or(Error::Malformed("truncated package header"))? as usize;
        if records_at <= 0 || record_count == 0 {
            return Ok(Package { guid, records: Vec::new() });
        }
        let at = records_at as usize;
        if at >= bytes.len() {
            return Err(Error::Malformed("record run starts past the end"));
        }

        let mut decoder = flate2::read::GzDecoder::new(&bytes[at..]);
        let mut plain = Vec::new();
        use std::io::Read;
        decoder
            .read_to_end(&mut plain)
            .map_err(|e| Error::Decompress(e.to_string()))?;

        let mut records = Vec::with_capacity(record_count);
        for i in 0..record_count {
            match u64_at(&plain, i * RECORD_SIZE) {
                Some(g) => records.push(g),
                None => break,
            }
        }
        Ok(Package { guid, records })
    }

    /// Every package holding the given GUID, as (index, package).
    pub fn find(&self, data: &GameData, wanted: u64) -> Vec<(usize, Package)> {
        let mut out = Vec::new();
        for index in 0..self.len() {
            if let Ok(package) = self.load(data, index) {
                if package.records.contains(&wanted) {
                    out.push((index, package));
                }
            }
        }
        out
    }
}

/// A one-line description of what a package holds, by asset type.
pub fn describe(package: &Package) -> String {
    let mut counts: std::collections::BTreeMap<u16, usize> = std::collections::BTreeMap::new();
    for guid in &package.records {
        *counts.entry(guid::type_of(*guid)).or_default() += 1;
    }
    let mut parts: Vec<String> = counts
        .iter()
        .map(|(kind, count)| format!("0x{kind:03X}x{count}"))
        .collect();
    parts.truncate(12);
    parts.join(" ")
}

/// Public entry point used by the probe.
pub fn open(data: &GameData) -> Result<Packages> {
    Packages::open(data).map_err(|e| crate::Error::NotFound(e.to_string()))
}

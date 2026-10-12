//! The resource graph: which assets ship as one package.
//!
//! The content manifest maps a GUID to bytes and says nothing about grouping.
//! The resource graph is the grouping. It lists, for every asset, the package
//! it belongs to, and a package is a thing loaded as a unit -- a map, a hero, a
//! game mode. That is what makes it the answer here: one chunk of the practice
//! range was recovered from a running game, and the graph names the rest of the
//! map by looking up everything that shares its package.
//!
//! ```text
//! header          version-dependent, 52 to 88 bytes, ending in a `trg` magic
//! package block   package count x 30 bytes: asset GUID, package key, three u32,
//!                 two bytes
//! skin block      skin count x 40-byte headers, with asset runs at pointers
//! graph block     the dependency edges
//! ```
//!
//! Newer builds encrypt this the way they once encrypted the manifest, and the
//! derivation for them is not public. The `WinPrism` graphs are in the clear,
//! which is the same split the content manifests have.

use std::collections::HashMap;
use std::fmt;

use crate::guid;
use crate::{GameData, Result};

const MAGIC: u32 = 0x00_74_72_67; // "trg"
const PACKAGE_SIZE: usize = 30;

#[derive(Debug)]
pub enum Error {
    NoResourceGraph,
    Malformed(&'static str),
    Encrypted,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NoResourceGraph => write!(f, "the root file lists no .trg resource graph"),
            Error::Malformed(m) => write!(f, "malformed resource graph: {m}"),
            Error::Encrypted => write!(
                f,
                "this resource graph is encrypted and its key derivation is not public; \
                 a WinPrism graph is the one to read"
            ),
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
fn u64_at(d: &[u8], at: usize) -> Option<u64> {
    d.get(at..at + 8).map(|b| {
        u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
    })
}

/// One asset's package membership.
#[derive(Debug, Clone, Copy)]
pub struct Membership {
    pub asset: u64,
    /// Everything with the same key is one package.
    pub package_key: u64,
    pub extra: [u32; 3],
}

pub struct ResourceGraph {
    pub build_version: u32,
    pub version: u8,
    pub memberships: Vec<Membership>,
    /// Asset GUID to its membership, for lookup.
    by_asset: HashMap<u64, usize>,
}

impl ResourceGraph {
    pub fn open(data: &GameData) -> EResult<ResourceGraph> {
        let root = data.root_file().map_err(|_| Error::NoResourceGraph)?;
        // Prefer a graph that is not encrypted, for the reason the content
        // manifest selector does: a name does not say which will open.
        let mut candidates: Vec<(String, [u8; 16])> = Vec::new();
        for line in root.lines() {
            if !line.contains(".trg") {
                continue;
            }
            let fields: Vec<&str> = line.split('|').collect();
            let Some(path) = fields.first() else { continue };
            let name = path.rsplit('/').next().unwrap_or(path).to_string();
            let Some(key) = fields.get(1) else { continue };
            if key.len() != 32 {
                continue;
            }
            let mut ckey = [0u8; 16];
            let mut ok = true;
            for (i, byte) in ckey.iter_mut().enumerate() {
                match u8::from_str_radix(&key[i * 2..i * 2 + 2], 16) {
                    Ok(v) => *byte = v,
                    Err(_) => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok {
                candidates.push((name, ckey));
            }
        }
        // WinPrism first: its graphs are the ones in the clear.
        candidates.sort_by_key(|(name, _)| !name.starts_with("WinPrism"));

        let mut last = Error::NoResourceGraph;
        for (_name, ckey) in candidates {
            let Ok(blob) = data.open_ckey(&ckey) else {
                continue;
            };
            match ResourceGraph::parse(&blob) {
                Ok(graph) => return Ok(graph),
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    pub fn parse(blob: &[u8]) -> EResult<ResourceGraph> {
        // The header's length depends on the version, and the version is only
        // readable once the header is found. So each known length is tried and
        // the one whose magic lands is the right one.
        for (header_size, version) in [(88usize, 13u8), (80, 12), (72, 11), (64, 7), (56, 6)] {
            let Some(magic) = u32_at(blob, header_size - 8) else {
                continue;
            };
            if (magic & 0x00FF_FFFF) != MAGIC {
                continue;
            }
            let build_version = u32_at(blob, 4).ok_or(Error::Malformed("no build version"))?;
            let package_count = i32_at(blob, 24).ok_or(Error::Malformed("truncated header"))?;
            let package_block_size =
                i32_at(blob, 28).ok_or(Error::Malformed("truncated header"))?;
            if package_count < 0 || package_block_size < 0 {
                return Err(Error::Malformed("negative package counts"));
            }

            let start = header_size;
            let end = start + package_count as usize * PACKAGE_SIZE;
            if end > blob.len() || end > start + package_block_size as usize {
                return Err(Error::Malformed("package block runs past the end"));
            }

            let mut memberships = Vec::with_capacity(package_count as usize);
            let mut by_asset = HashMap::with_capacity(package_count as usize);
            for i in 0..package_count as usize {
                let at = start + i * PACKAGE_SIZE;
                let Some(asset) = u64_at(blob, at) else { break };
                let Some(package_key) = u64_at(blob, at + 8) else {
                    break;
                };
                let extra = [
                    u32_at(blob, at + 16).unwrap_or(0),
                    u32_at(blob, at + 20).unwrap_or(0),
                    u32_at(blob, at + 24).unwrap_or(0),
                ];
                by_asset.insert(asset, memberships.len());
                memberships.push(Membership {
                    asset,
                    package_key,
                    extra,
                });
            }

            return Ok(ResourceGraph {
                build_version,
                version,
                memberships,
                by_asset,
            });
        }

        // No magic landed, which on these builds means the body is encrypted.
        Err(Error::Encrypted)
    }

    pub fn membership(&self, asset: u64) -> Option<&Membership> {
        self.by_asset.get(&asset).map(|i| &self.memberships[*i])
    }

    /// Everything in the same package as the given asset.
    pub fn package_of(&self, asset: u64) -> Vec<u64> {
        let Some(mine) = self.membership(asset) else {
            return Vec::new();
        };
        let key = mine.package_key;
        self.memberships
            .iter()
            .filter(|m| m.package_key == key)
            .map(|m| m.asset)
            .collect()
    }

    /// A one-line description of a set of assets, by type.
    pub fn describe(assets: &[u64]) -> String {
        let mut counts: std::collections::BTreeMap<u16, usize> = std::collections::BTreeMap::new();
        for asset in assets {
            *counts.entry(guid::type_of(*asset)).or_default() += 1;
        }
        let mut parts: Vec<String> = counts
            .iter()
            .map(|(kind, count)| format!("0x{kind:03X}x{count}"))
            .collect();
        parts.truncate(14);
        parts.join(" ")
    }
}

/// Public entry point used by the probe.
pub fn open(data: &GameData) -> Result<ResourceGraph> {
    ResourceGraph::open(data).map_err(|e| crate::Error::NotFound(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_package_block() {
        // A synthetic v13 graph: header, then two assets sharing a package key.
        let mut blob = vec![0u8; 88 + 2 * PACKAGE_SIZE];
        blob[4..8].copy_from_slice(&154_088u32.to_le_bytes());
        blob[24..28].copy_from_slice(&2i32.to_le_bytes());
        blob[28..32].copy_from_slice(&(2 * PACKAGE_SIZE as i32).to_le_bytes());
        blob[80..84].copy_from_slice(&MAGIC.to_le_bytes());
        for (i, (asset, key)) in [
            (0x0dd0_0001_0001_6dc1u64, 0x1234u64),
            (0x0dd0_0001_0001_6dc2u64, 0x1234u64),
        ]
        .iter()
        .enumerate()
        {
            let at = 88 + i * PACKAGE_SIZE;
            blob[at..at + 8].copy_from_slice(&asset.to_le_bytes());
            blob[at + 8..at + 16].copy_from_slice(&key.to_le_bytes());
        }

        let graph = ResourceGraph::parse(&blob).expect("parses");
        assert_eq!(graph.version, 13);
        assert_eq!(graph.build_version, 154_088);
        assert_eq!(graph.memberships.len(), 2);
        let package = graph.package_of(0x0dd0_0001_0001_6dc1);
        assert_eq!(package.len(), 2);
    }

    #[test]
    fn rejects_something_without_the_magic() {
        assert!(matches!(
            ResourceGraph::parse(&[0u8; 128]),
            Err(Error::Encrypted)
        ));
    }
}

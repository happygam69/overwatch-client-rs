//! The content manifest: the asset registry, and the encryption around it.
//!
//! A `.cmf` begins with a header that is stored in the clear; everything after
//! it is AES-CBC encrypted. The header's build number selects which key
//! derivation the file used, which is why a build the tool does not know cannot
//! be opened even though the key material is present.
//!
//! Only the derivations that were verified against a real install are
//! implemented. Adding a build means adding one small function, exactly as the
//! reference library carries one source file per build.
//!
//! Decrypted layout:
//!
//! ```text
//! header (48 bytes) as below
//! entries        entry_count * 20 bytes
//! records        data_count  * 29 bytes
//!   u64 GUID, u32 size, u8 unknown, ckey[16]
//! ```

use crate::keytable::{self, Sources};
use ow_casc::{blte, Store};

use aes::cipher::{block_padding::NoPadding, BlockDecryptMut, KeyIvInit};
use std::fmt;

/// The build whose manifest derivation is implemented.
pub const SUPPORTED_CMF_BUILD: u32 = 150_851;
/// The resource graph uses the same table with different constants.
pub const SUPPORTED_TRG_BUILD: u32 = 150_851;

const CMF_HEADER_SIZE: usize = 48;
const CMF_ENTRY_SIZE: usize = 20;
const CMF_RECORD_SIZE: usize = 29;

/// Marker stored in the magic field once the header is encrypted.
const ENCRYPTED_MAGIC: u32 = 0x0063_6D66; // "cmf" in the low 24 bits

#[derive(Debug)]
pub enum Error {
    Store(ow_casc::container::Error),
    Keytable(keytable::Error),
    Malformed(&'static str),
    /// The manifest was produced by a build whose key derivation is not here.
    UnsupportedBuild { build: u32, manifest: &'static str },
    Crypto(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Store(e) => write!(f, "{e}"),
            Error::Keytable(e) => write!(f, "{e}"),
            Error::Malformed(m) => write!(f, "malformed manifest: {m}"),
            Error::UnsupportedBuild { build, manifest } => write!(
                f,
                "{manifest} build {build} is not supported; its key derivation has not been \
                 verified. Supported: {SUPPORTED_CMF_BUILD}"
            ),
            Error::Crypto(m) => write!(f, "manifest decryption failed: {m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<ow_casc::container::Error> for Error {
    fn from(e: ow_casc::container::Error) -> Self {
        Error::Store(e)
    }
}
impl From<keytable::Error> for Error {
    fn from(e: keytable::Error) -> Self {
        Error::Keytable(e)
    }
}

type Result<T> = std::result::Result<T, Error>;

/// A content manifest's clear-text header.
#[derive(Debug, Clone, Copy)]
pub struct Header {
    pub build_version: u32,
    /// Counted in the patch-fixup sections; zero in a clean build.
    pub data_patch_count: i32,
    pub entry_patch_count: i32,
    pub data_count: i32,
    pub entry_count: i32,
    pub magic: u32,
}

impl Header {
    pub fn parse(data: &[u8]) -> Result<Header> {
        if data.len() < CMF_HEADER_SIZE {
            return Err(Error::Malformed("shorter than its header"));
        }
        let rd = |at: usize| -> u32 {
            u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
        };
        Ok(Header {
            build_version: rd(0),
            data_patch_count: rd(28) as i32,
            data_count: rd(32) as i32,
            entry_patch_count: rd(36) as i32,
            entry_count: rd(40) as i32,
            magic: rd(44),
        })
    }

    /// Whether the body is encrypted. The magic carries a `cmf` marker.
    pub fn is_encrypted(&self) -> bool {
        (self.magic >> 8) == ENCRYPTED_MAGIC
    }

    /// The format version, from the magic's top byte.
    pub fn version(&self) -> u8 {
        let raw = if self.is_encrypted() {
            self.magic.swap_bytes()
        } else {
            self.magic
        };
        (raw >> 24) as u8
    }
}

/// Derive the AES key and IV for a manifest of the given build.
///
/// These are the verified derivations. Each walks the 512-byte table with a
/// stride taken from the header, which is what binds the ciphertext to the
/// build: the numbers in the header are the same ones that produced it.
///
/// The key is 32 bytes (AES-256) even though the framework the algorithm comes
/// from nominally asks for 128 bits; the shorter key does not decrypt, which is
/// how this was settled.
pub fn derive_key_iv(
    header: &Header,
    digest: &[u8; 20],
    table: &[u8; keytable::TABLE_SIZE],
    manifest: ManifestKind,
) -> Result<([u8; 32], [u8; 16])> {
    if header.build_version != SUPPORTED_CMF_BUILD {
        return Err(Error::UnsupportedBuild {
            build: header.build_version,
            manifest: manifest.name(),
        });
    }

    let index = |i: u32| table[(i % keytable::TABLE_SIZE as u32) as usize];

    // Key: seed the walk with the data-record count, then step by it.
    let mut key = [0u8; 32];
    let step = header.data_count as u32;
    let mut cursor = u32::from(index(step & 511));
    for slot in key.iter_mut() {
        *slot = index(cursor);
        cursor = cursor.wrapping_add(step);
    }

    // IV: seed from the file name's SHA-1, then step and mix the digest in.
    let mut iv = [0u8; 16];
    let seed = u32::from(index((2 * u32::from(digest[13])).wrapping_sub(16) & 511));
    let mut cursor = seed;
    for (i, slot) in iv.iter_mut().enumerate() {
        *slot = index(cursor);
        cursor = cursor.wrapping_add(seed % 29);
        let mix_at = (cursor.wrapping_add(header.data_count as u32) % 20) as usize;
        *slot ^= digest[mix_at].wrapping_add(1);
        let _ = i;
    }

    Ok((key, iv))
}

/// Which manifest is wanted. All three kinds are `.cmf` files distinguished only
/// by a marker in the name, so the kind has to be matched explicitly rather than
/// inferred from the extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestKind {
    /// The base manifest, untagged and holding most assets.
    Content,
    /// Localised text for one locale.
    Text,
    /// Localised audio for one locale. Nothing here reads it.
    Speech,
    /// The asset dependency graph, a `.trg`.
    ResourceGraph,
}

impl ManifestKind {
    fn name(self) -> &'static str {
        match self {
            ManifestKind::Content => "content manifest (cmf)",
            ManifestKind::Text => "text manifest (cmf)",
            ManifestKind::Speech => "speech manifest (cmf)",
            ManifestKind::ResourceGraph => "resource graph (trg)",
        }
    }

    fn extension(self) -> &'static str {
        match self {
            ManifestKind::ResourceGraph => ".trg",
            _ => ".cmf",
        }
    }

    /// Marker that must appear in the file name, if any.
    ///
    /// Matched on underscore-delimited segments so a hero or map whose name
    /// contains "text" cannot be mistaken for a text manifest.
    fn marker(self) -> Option<&'static str> {
        match self {
            ManifestKind::Text => Some("text"),
            ManifestKind::Speech => Some("speech"),
            _ => None,
        }
    }

    /// Whether this kind is per-locale and therefore needs a locale tag.
    fn is_localised(self) -> bool {
        matches!(self, ManifestKind::Text | ManifestKind::Speech)
    }
}

/// One decrypted record: an asset GUID and where its bytes live.
#[derive(Debug, Clone, Copy)]
pub struct Record {
    pub guid: u64,
    /// Stored length.
    pub size: u32,
    pub unknown: u8,
    pub content_key: [u8; 16],
}

/// The asset registry read from a content manifest.
pub struct Registry {
    pub header: Header,
    pub records: Vec<Record>,
    /// Manifest file name, needed to derive the key.
    pub name: String,
}

impl Registry {
    /// Find a manifest in a root file listing.
    ///
    /// The root file lists every manifest for every platform, region and locale.
    /// `locale` selects which locale tag is wanted; it must be `None` for the
    /// untagged kinds and `Some` for the localised ones.
    ///
    /// Region is tried exactly first, then relaxed. It has to be: on the
    /// reference install the base manifest is an `RDEV` file while the enUS text
    /// manifest is an `RCN` one, so requiring one region for both finds nothing.
    pub fn find_manifest_name(
        root_file: &str,
        kind: ManifestKind,
        platform: &str,
        region: &str,
        locale: Option<&str>,
    ) -> Option<(String, [u8; 16])> {
        let extension = kind.extension();
        let marker = kind.marker();
        let mut fallback: Option<(String, [u8; 16])> = None;

        for line in root_file.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fields: Vec<&str> = line.split('|').collect();
            if fields.len() < 2 {
                continue;
            }
            let path = fields[0];
            if !path.ends_with(extension) {
                continue;
            }
            let name = path.rsplit('/').next().unwrap_or(path);
            if !name.starts_with(&format!("{platform}_")) {
                continue;
            }

            let mut segments = name.split('_');
            // platform tag, then the shape tag (`SPWin`, `SPWinPrism`, ...)
            let _platform = segments.next();
            let _shape = segments.next();
            // The rest are markers, a locale tag, and the trailing `EExt.cmf`,
            // so collect them and test membership rather than positions.
            let rest: Vec<&str> = segments.collect();

            // Exactly one of the three kinds' markers must be present.
            let has_marker = |want: &str| rest.iter().any(|p| p.eq_ignore_ascii_case(want));
            let is_text = has_marker("text");
            let is_speech = has_marker("speech");
            let marked = match marker {
                Some(want) => has_marker(want),
                None => !is_text && !is_speech,
            };
            if !marked {
                continue;
            }

            // A locale tag is `LxxYY` as its own segment.
            let tagged = rest.iter().find(|p| {
                p.len() == 5
                    && p.starts_with('L')
                    && p[1..].chars().all(|c| c.is_ascii_alphanumeric())
            });
            let matches_locale = match (kind.is_localised(), locale, tagged) {
                (true, Some(want), Some(have)) => have[1..].eq_ignore_ascii_case(want),
                (false, _, None) => true,
                // an untagged request must not accept a tagged file, and a
                // localised request must not accept an untagged one
                _ => false,
            };
            if !matches_locale {
                continue;
            }

            let Some(ckey) = parse_hex_key(fields[1]) else {
                continue;
            };
            let found = (name.to_string(), ckey);
            if name.contains(region) {
                return Some(found);
            }
            fallback.get_or_insert(found);
        }
        fallback
    }

    /// Open a content manifest from the store.
    ///
    /// `name` is the manifest's file name as listed in the root file; it is
    /// hashed into the key derivation, so it must be exact.
    pub fn open(
        store: &Store,
        name: &str,
        content_key: &[u8; 16],
        sources: &Sources,
    ) -> Result<Registry> {
        let blob = store.open_ckey(content_key)?;
        if blob.len() < 4 || blob[0..4] != blte::BLTE_MAGIC {
            // already decoded
        }
        let header = Header::parse(&blob)?;
        if header.data_count < 0 || header.entry_count < 0 {
            return Err(Error::Malformed("negative record count"));
        }

        let table = sources.table_for(header.build_version)?;

        // The derivation hashes the file name as ASCII.
        let digest: [u8; 20] = {
            use sha1::{Digest, Sha1};
            let mut hasher = Sha1::new();
            hasher.update(name.as_bytes());
            hasher.finalize().into()
        };

        let (key, iv) = derive_key_iv(&header, &digest, &table, ManifestKind::Content)?;

        let body = &blob[CMF_HEADER_SIZE..];
        let plain = decrypt(body, &key, &iv)?;

        let records_at = header.entry_count as usize * CMF_ENTRY_SIZE;
        let needed = records_at + header.data_count as usize * CMF_RECORD_SIZE;
        if plain.len() < needed {
            return Err(Error::Malformed("records run past the decrypted body"));
        }

        let mut records = Vec::with_capacity(header.data_count as usize);
        for i in 0..header.data_count as usize {
            let at = records_at + i * CMF_RECORD_SIZE;
            let r = &plain[at..at + CMF_RECORD_SIZE];
            let guid = u64::from_le_bytes([
                r[0], r[1], r[2], r[3], r[4], r[5], r[6], r[7],
            ]);
            let size = u32::from_le_bytes([r[8], r[9], r[10], r[11]]);
            let mut content_key = [0u8; 16];
            content_key.copy_from_slice(&r[13..29]);
            records.push(Record {
                guid,
                size,
                unknown: r[12],
                content_key,
            });
        }

        Ok(Registry {
            header,
            records,
            name: name.to_string(),
        })
    }

    /// The record for an asset GUID.
    pub fn get(&self, guid: u64) -> Option<&Record> {
        // Records are GUID-sorted, which is also how decryption was verified.
        self.records
            .binary_search_by_key(&guid, |r| r.guid)
            .ok()
            .map(|i| &self.records[i])
    }

    /// Add records from another manifest, keeping the ones already here.
    ///
    /// The base manifest is authoritative for anything it covers; a locale
    /// manifest mostly adds assets it alone carries. Records arrive sorted, so
    /// the result is re-sorted rather than merged in place.
    pub fn merge_missing(&mut self, other: &Registry) {
        let known: std::collections::HashSet<u64> =
            self.records.iter().map(|r| r.guid).collect();
        self.records
            .extend(other.records.iter().filter(|r| !known.contains(&r.guid)).copied());
        self.records.sort_unstable_by_key(|r| r.guid);
    }

    pub fn guids_of_type(&self, type_id: u16) -> Vec<u64> {
        self.records
            .iter()
            .map(|r| r.guid)
            .filter(|g| crate::guid::type_of(*g) == type_id)
            .collect()
    }
}

fn parse_hex_key(hex: &str) -> Option<[u8; 16]> {
    if hex.len() != 32 {
        return None;
    }
    let mut out = [0u8; 16];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

fn decrypt(body: &[u8], key: &[u8; 32], iv: &[u8; 16]) -> Result<Vec<u8>> {
    type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;
    let mut buf = body.to_vec();
    // Release builds may append a partial block; trim to a whole number.
    let usable = buf.len() / 16 * 16;
    buf.truncate(usable);
    Aes256CbcDec::new(key.into(), iv.into())
        .decrypt_padded_mut::<NoPadding>(&mut buf)
        .map_err(|e| Error::Crypto(e.to_string()))?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_reports_encryption_and_version() {
        // A real retail header: magic 0x636d661b -> encrypted, version 27.
        let mut data = vec![0u8; CMF_HEADER_SIZE];
        data[0..4].copy_from_slice(&150_851u32.to_le_bytes());
        data[32..36].copy_from_slice(&1_208_956i32.to_le_bytes());
        data[40..44].copy_from_slice(&357i32.to_le_bytes());
        data[44..48].copy_from_slice(&0x636d661bu32.to_le_bytes());

        let header = Header::parse(&data).unwrap();
        assert_eq!(header.build_version, 150_851);
        assert_eq!(header.data_count, 1_208_956);
        assert_eq!(header.entry_count, 357);
        assert!(header.is_encrypted());
        assert_eq!(header.version(), 27);
    }

    #[test]
    fn picks_the_right_manifest_kind_and_locale() {
        let root = "\
#FILEID|MD5|CHUNK_ID|PRIORITY|MPRIORITY|FILENAME|INSTALLPATH
Manifest/Win_SPWin_RDEV_LesES_text_EExt.cmf|FE5EA8AB10EBDB4210D3175CDD564004|0|0|255|x|
Manifest/Win_SPWin_RDEV_EExt.cmf|B16ED5CF5EF79F00D8BAB691BF18EB10|0|0|255|x|
Manifest/Win_SPWin_RDEV_LenUS_speech_EExt.cmf|11111111111111111111111111111111|0|0|255|x|
Manifest/Win_SPWin_RDEV_LenUS_text_EExt.cmf|C306CF53339CDEB475EC97B3D55289E9|0|0|255|x|
Manifest/WinPrism_SPWin_RDEV_LenUS_text_EExt.cmf|D724550C0834FE0EE77C34471A73EEBB|0|0|255|x|
Manifest/Win_SPWin_RCN_LenUS_text_EExt.cmf|D3EBF113E2DD2AC5AD7738605D8C66DB|0|0|255|x|
Manifest/Win_SPWin_RDEV_EExt.trg|22222222222222222222222222222222|0|0|255|x|
";
        // The untagged base manifest, not a localised one and not the prism build.
        let (name, key) =
            Registry::find_manifest_name(root, ManifestKind::Content, "Win", "RDEV", None).unwrap();
        assert_eq!(name, "Win_SPWin_RDEV_EExt.cmf");
        assert_eq!(key[0], 0xB1);

        // Text and speech are both `.cmf` and both locale-tagged, so only the
        // name marker tells them apart. Getting this wrong is silent.
        let (name, key) =
            Registry::find_manifest_name(root, ManifestKind::Text, "Win", "RDEV", Some("enUS"))
                .unwrap();
        assert_eq!(name, "Win_SPWin_RDEV_LenUS_text_EExt.cmf");
        assert_eq!(key[0], 0xC3);

        let (name, _) =
            Registry::find_manifest_name(root, ManifestKind::Speech, "Win", "RDEV", Some("enUS"))
                .unwrap();
        assert_eq!(name, "Win_SPWin_RDEV_LenUS_speech_EExt.cmf");

        // A different locale picks that locale's text manifest.
        let (name, _) =
            Registry::find_manifest_name(root, ManifestKind::Text, "Win", "RDEV", Some("esES"))
                .unwrap();
        assert_eq!(name, "Win_SPWin_RDEV_LesES_text_EExt.cmf");

        // Region relaxes when the exact one has no such manifest.
        let (name, _) =
            Registry::find_manifest_name(root, ManifestKind::Text, "Win", "RCN", Some("enUS"))
                .unwrap();
        assert_eq!(name, "Win_SPWin_RCN_LenUS_text_EExt.cmf");

        // An unavailable locale finds nothing rather than falling back.
        assert!(
            Registry::find_manifest_name(root, ManifestKind::Text, "Win", "RDEV", Some("deDE"))
                .is_none()
        );
        // And a base request never returns a localised manifest.
        assert!(Registry::find_manifest_name(
            "Manifest/Win_SPWin_RDEV_LenUS_text_EExt.cmf|C306CF53339CDEB475EC97B3D55289E9|0|0|255|x|\n",
            ManifestKind::Content,
            "Win",
            "RDEV",
            None
        )
        .is_none());
        // The graph kind resolves through its own extension.
        let (name, _) = Registry::find_manifest_name(
            root,
            ManifestKind::ResourceGraph,
            "Win",
            "RDEV",
            None,
        )
        .unwrap();
        assert_eq!(name, "Win_SPWin_RDEV_EExt.trg");
    }

    #[test]
    fn merge_keeps_existing_records_and_stays_sorted() {
        let make = |records: Vec<(u64, u8)>| Registry {
            header: Header {
                build_version: 1,
                data_patch_count: 0,
                entry_patch_count: 0,
                data_count: records.len() as i32,
                entry_count: 0,
                magic: 0,
            },
            records: records
                .into_iter()
                .map(|(guid, byte)| Record {
                    guid,
                    size: 1,
                    unknown: 0,
                    content_key: [byte; 16],
                })
                .collect(),
            name: "test".to_string(),
        };

        let mut base = make(vec![(10, 0xAA), (30, 0xAA)]);
        let extra = make(vec![(10, 0xBB), (20, 0xBB)]);
        base.merge_missing(&extra);

        let guids: Vec<u64> = base.records.iter().map(|r| r.guid).collect();
        assert_eq!(guids, vec![10, 20, 30], "merged records must stay sorted");
        // The overlapping GUID keeps the base manifest's content key.
        assert_eq!(base.get(10).unwrap().content_key[0], 0xAA);
        assert_eq!(base.get(20).unwrap().content_key[0], 0xBB);
    }
}

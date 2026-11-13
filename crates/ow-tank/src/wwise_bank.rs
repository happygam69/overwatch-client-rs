//! The Wwise sound bank container: the `BKHD` section, the section table, and
//! the `DIDX` media index.
//!
//! A type-`0x043` asset is not an STU and not a RIFF media container. It is a
//! Wwise soundbank, and this module reads the container the client reads:
//! `CAkBankReader`'s header parse (`ReadBankHeader` at `0x1428fc300`, 380 bytes)
//! and its section loop (`CAkBankMgr::LoadBank`'s section dispatcher at
//! `0x1428fa3a0`).
//!
//! ## What the client's parse does, and what this follows
//!
//! The file is a flat sequence of sections: an 8-byte header (`id`, `size`,
//! both little-endian) and `size` payload bytes. The first section must be
//! `BKHD` (`0x44484b42`); the loop then dispatches on the id and skips the
//! payload of any id it does not handle.
//!
//! The `BKHD` payload is:
//!
//! | offset | width | field | client evidence |
//! |---|---|---|---|
//! | `0x00` | `u32` | version | compared against the supported window |
//! | `0x04` | `u32` | bank id | passed to the `HIRC` reader |
//! | `0x08` | `u32` | language id | passed to the `HIRC`/`STID` readers |
//! | `0x0c` | `u16` | unknown | copied and masked only |
//! | `0x0e` | `u16` | unknown | copied and masked only |
//! | `0x10` | `u32` | unknown | copied and masked only |
//! | `0x14` | `u32` | bank type (present from version `0x8f`) | must be `0` or `0x1e..=0x1f`, else status 100 |
//! | `0x18` | `[u8; 16]` | unknown (present from version `0x8f`) | copied |
//!
//! **The first 16 payload bytes are XORed with a 16-byte key table at
//! `0x144293db8` before any field is read.** That table is in the image's
//! zero-fill tail (`docs/ghidra-image-audit.md` § 2.1) and has no writer in the
//! image, so on this install the key is zero and the XOR is the identity - which
//! the data confirms: every readable bank's version lands inside the client's
//! window. The XOR is modelled here with a zero key so the expression is
//! visible, not because a key was observed.
//!
//! Version gate: `0x76..=0x91` is supported. `0x76..=0x90` sets the client's
//! "media-only" flag, after which a section other than `DATA`/`DIDX` is
//! rejected with status `0x40`; `<= 0x75` and `>= 0x92` are status `0x40`.
//!
//! `DIDX` is a flat array of 12-byte records - `u32` id, `u32` offset, `u32`
//! size - and the client takes `count = size / 12` (floor) and reads
//! `count * 12` bytes, leaving any trailing partial record in the stream. That
//! floor is mirrored here.
//!
//! ## What this module does not do
//!
//! It does not read `HIRC` (the object graph), `STID`/`STMG` (state), `ENVS`
//! (environment curves) or `INIT`/`PLAT` beyond their section headers. Those are
//! the client's own readers at `0x1428fcc40`, `0x1428fd1a0`, `0x1428fc530`,
//! `0x1428fd020` and `0x1428fc480`; their bodies are not read yet, so the data
//! stays raw and the container is the deliverable.
//!
//! `docs/formats/sound.md` § "The sound bank container" carries the same
//! findings.

use std::fmt;

/// `BKHD` as the little-endian dword the client compares (`0x1428fc355`).
pub const MAGIC: [u8; 4] = *b"BKHD";

/// The lowest version the client accepts (`0x75 < version`).
pub const VERSION_MIN_EXCLUSIVE: u32 = 0x75;
/// The highest version the client accepts (`version < 0x92`).
pub const VERSION_MAX_EXCLUSIVE: u32 = 0x92;
/// The first version whose header carries the bank type and the 16-byte tail.
pub const VERSION_HEADER_EXTENDED: u32 = 0x8f;
/// Above this version the `DATA` alignment comes from the header instead of
/// being the literal 16 (`0x1428fa4c9`).
pub const VERSION_ALIGNMENT_IN_HEADER: u32 = 0x86;

/// The `DATA` alignment the client uses when the header does not carry one.
pub const DEFAULT_DATA_ALIGNMENT: u16 = 0x10;

/// Section ids the client dispatches on, as the little-endian dwords it
/// compares (`0x1428fa52a..0x1428fa7e6`).
pub const SECTION_DATA: [u8; 4] = *b"DATA";
pub const SECTION_DIDX: [u8; 4] = *b"DIDX";
pub const SECTION_HIRC: [u8; 4] = *b"HIRC";
pub const SECTION_STID: [u8; 4] = *b"STID";
pub const SECTION_STMG: [u8; 4] = *b"STMG";
pub const SECTION_ENVS: [u8; 4] = *b"ENVS";
pub const SECTION_PLAT: [u8; 4] = *b"PLAT";
pub const SECTION_INIT: [u8; 4] = *b"INIT";

/// Size of a `DIDX` record: `{u32 id, u32 offset, u32 size}`.
pub const MEDIA_RECORD_SIZE: usize = 12;

/// The 16-byte XOR key the client applies to the header's first 16 bytes.
///
/// Read from `0x144293db8`, which `dev/pe_layout.py` classifies as `zero-fill`
/// `.data`: no raw bytes, no writer in the image. The value below was therefore
/// not read from the file - it is the *modelled* key, and the install shows the
/// header decrypts with it (every bank's version lands in the client's window).
pub const HEADER_KEY: [u8; 16] = [0; 16];

/// One section of the bank: its id, where its payload starts, and its size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Section {
    pub id: [u8; 4],
    /// Byte offset of the payload, i.e. just past the 8-byte section header.
    pub payload: usize,
    pub size: u32,
}

impl Section {
    /// The id as text, for printing: every observed id is four ASCII bytes.
    pub fn name(&self) -> String {
        self.id
            .iter()
            .map(|b| {
                if b.is_ascii_graphic() {
                    *b as char
                } else {
                    '.'
                }
            })
            .collect()
    }
}

/// One `DIDX` record: a media id, its offset inside `DATA` and its byte size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Media {
    pub id: u32,
    pub offset: u32,
    pub size: u32,
}

/// The `BKHD` header, as the client reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub version: u32,
    pub bank_id: u32,
    pub language_id: u32,
    /// The `u16` at payload `0x0c`. The client copies and masks it; no reader
    /// body uses it yet, so it has no name here.
    pub unknown_0c: u16,
    /// The `u16` at payload `0x0e`.
    pub unknown_0e: u16,
    /// The `u32` at payload `0x10`.
    pub unknown_10: u32,
    /// The `u32` at payload `0x14`, present from version `0x8f`. The client
    /// rejects anything but `0` and `0x1e..=0x1f` (status 100).
    pub bank_type: Option<u32>,
    /// The 16 bytes at payload `0x18`, present from version `0x8f`.
    pub tail: Option<[u8; 16]>,
    /// The declared `BKHD` payload size, which the client skips to reach the
    /// next section.
    pub section_size: u32,
}

impl Header {
    /// The `DATA` alignment the client would use for this bank: the header's
    /// value above version `0x86`, the literal `0x10` at or below it.
    ///
    /// Which of the two header `u16`s carries the alignment cannot be settled
    /// by reading the header parse alone; `tests/bank_real.rs` measures it by
    /// checking every `DIDX` offset against both candidates.
    pub fn data_alignment(&self) -> u16 {
        if self.version > VERSION_ALIGNMENT_IN_HEADER {
            self.unknown_0c
        } else {
            DEFAULT_DATA_ALIGNMENT
        }
    }
}

/// A parsed bank.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bank {
    pub header: Header,
    /// Every section in file order, `BKHD` included.
    pub sections: Vec<Section>,
    /// The `DIDX` records, empty when the bank has no `DIDX`.
    pub media: Vec<Media>,
}

impl Bank {
    /// The first section with this id.
    pub fn section(&self, id: [u8; 4]) -> Option<&Section> {
        self.sections.iter().find(|s| s.id == id)
    }

    /// The `DATA` payload, when the bank carries one and the caller has the
    /// bytes to slice.
    pub fn data<'a>(&self, bytes: &'a [u8]) -> Option<&'a [u8]> {
        let s = self.section(SECTION_DATA)?;
        bytes.get(s.payload..s.payload + s.size as usize)
    }
}

/// Why a bank did not parse. The variants mirror the client's own status
/// returns where one exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The file is shorter than the 8-byte section header the client reads
    /// first. The client reports status 7.
    Short,
    /// The first section id is not `BKHD`. The client reports status 7.
    NotBankMagic { found: [u8; 4] },
    /// The `BKHD` payload is shorter than the version's header layout needs.
    ShortHeader { need: usize, have: usize },
    /// The bank type field is not `0` and not `0x1e..=0x1f`; the client
    /// reports status 100.
    UnsupportedBankType(u32),
    /// The version is outside `0x76..=0x91`; the client reports status `0x40`.
    UnsupportedVersion(u32),
    /// A section other than `DATA`/`DIDX` in a `0x76..=0x90` bank; the client
    /// reports status `0x40`.
    OldBankNonMediaSection([u8; 4]),
    /// A section header or payload runs past the end of the file. The client's
    /// short read reports status 7.
    SectionPastEnd {
        id: [u8; 4],
        at: usize,
        size: u32,
        len: usize,
    },
    /// Fewer than 8 bytes remain after the last section's payload, so the file
    /// does not tile. The client's header read comes back short (status 7).
    TrailingBytes { at: usize, len: usize },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Short => write!(f, "not a sound bank: shorter than 8 bytes"),
            Error::NotBankMagic { found } => write!(
                f,
                "not a sound bank: the first four bytes are {:?}, not \"BKHD\"",
                String::from_utf8_lossy(found)
            ),
            Error::ShortHeader { need, have } => {
                write!(f, "bank header truncated: needs {need} bytes, has {have}")
            }
            Error::UnsupportedBankType(t) => {
                write!(f, "bank type 0x{t:X} is not 0 or 0x1e..=0x1f")
            }
            Error::UnsupportedVersion(v) => write!(
                f,
                "bank version 0x{v:X} is outside the client's window 0x76..=0x91"
            ),
            Error::OldBankNonMediaSection(id) => write!(
                f,
                "media-only bank carries a non-media section {:?}",
                String::from_utf8_lossy(id)
            ),
            Error::SectionPastEnd { id, at, size, len } => write!(
                f,
                "section {:?} at {at:#x} declares {size} bytes, file is {len}",
                String::from_utf8_lossy(id)
            ),
            Error::TrailingBytes { at, len } => write!(
                f,
                "{len} bytes at {at:#x} do not fit an 8-byte section header"
            ),
        }
    }
}

impl std::error::Error for Error {}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    bytes
        .get(at..at + 4)
        .and_then(|b| b.try_into().ok())
        .map(u32::from_le_bytes)
}

/// Parse one bank.
///
/// Follows `ReadBankHeader` (`0x1428fc300`) for the header and the section
/// dispatcher (`0x1428fa3a0`) for the rest of the file.
pub fn parse(bytes: &[u8]) -> Result<Bank, Error> {
    if bytes.len() < 8 {
        return Err(Error::Short);
    }
    let magic: [u8; 4] = bytes[0..4].try_into().expect("four bytes");
    if magic != MAGIC {
        return Err(Error::NotBankMagic { found: magic });
    }
    let section_size = u32_at(bytes, 4).expect("checked");
    let payload = 8usize;

    let need = if u32_at(bytes, payload).unwrap_or(0) >= VERSION_HEADER_EXTENDED {
        // 0x14 + 4 + 0x10, the client's `iVar3 = 0x28`.
        0x14 + 4 + 0x10
    } else {
        0x14
    };
    let have = bytes.len().saturating_sub(payload);
    if have < need {
        return Err(Error::ShortHeader { need, have });
    }
    // The client XORs the first 16 header bytes in place with the key table
    // before reading any field (`0x1428fc365..0x1428fc3a7`).
    let mut plain = [0u8; 16];
    for (i, slot) in plain.iter_mut().enumerate() {
        *slot = bytes[payload + i] ^ HEADER_KEY[i];
    }
    let version = u32::from_le_bytes(plain[0..4].try_into().expect("four bytes"));
    let bank_id = u32::from_le_bytes(plain[4..8].try_into().expect("four bytes"));
    let language_id = u32::from_le_bytes(plain[8..12].try_into().expect("four bytes"));
    let unknown_0c = u16::from_le_bytes(plain[12..14].try_into().expect("two bytes"));
    let unknown_0e = u16::from_le_bytes(plain[14..16].try_into().expect("two bytes"));
    let unknown_10 = u32_at(bytes, payload + 0x10).expect("checked");
    let (bank_type, tail) = if version >= VERSION_HEADER_EXTENDED {
        let t = u32_at(bytes, payload + 0x14).expect("checked");
        if t != 0 && !(0x1e..=0x1f).contains(&t) {
            return Err(Error::UnsupportedBankType(t));
        }
        let mut tail = [0u8; 16];
        tail.copy_from_slice(&bytes[payload + 0x18..payload + 0x18 + 16]);
        (Some(t), Some(tail))
    } else {
        (None, None)
    };
    let header = Header {
        version,
        bank_id,
        language_id,
        unknown_0c,
        unknown_0e,
        unknown_10,
        bank_type,
        tail,
        section_size,
    };
    if version <= VERSION_MIN_EXCLUSIVE || version >= VERSION_MAX_EXCLUSIVE {
        return Err(Error::UnsupportedVersion(version));
    }
    let media_only = version <= 0x90;

    // The client skips to the end of the `BKHD` payload: `section_size - 0x14`
    // (or `- 0x28`), which lands on the next section header.
    let mut at = payload + section_size as usize;
    let mut sections = vec![Section {
        id: MAGIC,
        payload,
        size: section_size,
    }];
    let mut media = Vec::new();
    loop {
        let rest = bytes.len() - at.min(bytes.len());
        if rest == 0 {
            break;
        }
        if rest < 8 {
            return Err(Error::TrailingBytes {
                at,
                len: bytes.len() - at,
            });
        }
        let id: [u8; 4] = bytes[at..at + 4].try_into().expect("checked");
        let size = u32_at(bytes, at + 4).expect("checked");
        let body = at + 8;
        if body as u64 + size as u64 > bytes.len() as u64 {
            return Err(Error::SectionPastEnd {
                id,
                at,
                size,
                len: bytes.len(),
            });
        }
        if media_only && id != SECTION_DATA && id != SECTION_DIDX {
            return Err(Error::OldBankNonMediaSection(id));
        }
        sections.push(Section {
            id,
            payload: body,
            size,
        });
        if id == SECTION_DIDX {
            // `count = size / 12` (floor), then exactly `count * 12` bytes are
            // read; a trailing partial record stays in the stream, as it does
            // for the client.
            let count = size as usize / MEDIA_RECORD_SIZE;
            for i in 0..count {
                let band = body + i * MEDIA_RECORD_SIZE;
                media.push(Media {
                    id: u32_at(bytes, band).expect("in range"),
                    offset: u32_at(bytes, band + 4).expect("in range"),
                    size: u32_at(bytes, band + 8).expect("in range"),
                });
            }
            at = body + count * MEDIA_RECORD_SIZE;
            continue;
        }
        at = body + size as usize;
    }
    Ok(Bank {
        header,
        sections,
        media,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal bank: `BKHD` with a version-`0x91` header and one extra
    /// section of `payload` bytes.
    fn bank(version: u32, extra: Option<([u8; 4], &[u8])>) -> Vec<u8> {
        let extra_len = extra.map(|(_, p)| 8 + p.len()).unwrap_or(0);
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"BKHD");
        bytes.extend_from_slice(&(0x28u32).to_le_bytes());
        bytes.extend_from_slice(&version.to_le_bytes());
        bytes.extend_from_slice(&0x0011_8594u32.to_le_bytes());
        bytes.extend_from_slice(&0x1770_5d3eu32.to_le_bytes());
        bytes.extend_from_slice(&0x10u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0x1e5du32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&[0u8; 16]);
        let _ = extra_len;
        if let Some((id, payload)) = extra {
            bytes.extend_from_slice(&id);
            bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            bytes.extend_from_slice(payload);
        }
        bytes
    }

    #[test]
    fn a_bank_with_one_hirc_section_parses() {
        let parsed = parse(&bank(0x91, Some((*b"HIRC", &[1, 2, 3])))).expect("parses");
        assert_eq!(parsed.header.version, 0x91);
        assert_eq!(parsed.header.bank_id, 0x0011_8594);
        assert_eq!(parsed.header.language_id, 0x1770_5d3e);
        assert_eq!(parsed.header.unknown_0c, 0x10);
        assert_eq!(parsed.header.bank_type, Some(0));
        assert_eq!(parsed.sections.len(), 2);
        assert_eq!(parsed.sections[1].id, *b"HIRC");
        assert_eq!(parsed.sections[1].payload, 8 + 0x28 + 8);
        assert_eq!(parsed.sections[1].size, 3);
        assert!(parsed.media.is_empty());
        assert_eq!(parsed.header.data_alignment(), 0x10);
    }

    #[test]
    fn the_short_header_form_is_20_bytes() {
        let mut bytes = bank(0x76, None);
        // The extended header is only read from version 0x8f: shrink the
        // payload to its 20-byte form and drop the tail.
        bytes[4..8].copy_from_slice(&0x14u32.to_le_bytes());
        bytes.truncate(8 + 0x14);
        let parsed = parse(&bytes).expect("parses");
        assert_eq!(parsed.header.bank_type, None);
        assert_eq!(parsed.header.tail, None);
        assert_eq!(parsed.header.data_alignment(), DEFAULT_DATA_ALIGNMENT);
    }

    #[test]
    fn a_version_outside_the_window_is_rejected() {
        for v in [0x75u32, 0x92] {
            let e = parse(&bank(v, None)).unwrap_err();
            assert_eq!(e, Error::UnsupportedVersion(v));
        }
        assert!(parse(&bank(0x76, None)).is_ok());
        assert!(parse(&bank(0x91, None)).is_ok());
    }

    #[test]
    fn a_media_only_bank_may_not_carry_other_sections() {
        let e = parse(&bank(0x90, Some((*b"HIRC", &[0; 4])))).unwrap_err();
        assert_eq!(e, Error::OldBankNonMediaSection(*b"HIRC"));
        // ...but DATA and DIDX are fine, and so is any section from 0x91.
        assert!(parse(&bank(0x90, Some((*b"DATA", &[0; 4])))).is_ok());
        assert!(parse(&bank(0x91, Some((*b"HIRC", &[0; 4])))).is_ok());
    }

    #[test]
    fn the_bank_type_field_is_validated() {
        let mut bytes = bank(0x91, None);
        let at = 8 + 0x14;
        bytes[at..at + 4].copy_from_slice(&0x1eu32.to_le_bytes());
        assert!(parse(&bytes).is_ok());
        bytes[at..at + 4].copy_from_slice(&0x1fu32.to_le_bytes());
        assert!(parse(&bytes).is_ok());
        for bad in [1u32, 0x1d, 0x20] {
            bytes[at..at + 4].copy_from_slice(&bad.to_le_bytes());
            assert_eq!(parse(&bytes).unwrap_err(), Error::UnsupportedBankType(bad));
        }
    }

    #[test]
    fn a_didx_section_yields_twelve_byte_records() {
        let mut didx = Vec::new();
        for (id, off, size) in [(1u32, 0u32, 16u32), (2, 16, 32)] {
            didx.extend_from_slice(&id.to_le_bytes());
            didx.extend_from_slice(&off.to_le_bytes());
            didx.extend_from_slice(&size.to_le_bytes());
        }
        let parsed = parse(&bank(0x91, Some((*b"DIDX", &didx)))).expect("parses");
        assert_eq!(
            parsed.media,
            vec![
                Media {
                    id: 1,
                    offset: 0,
                    size: 16
                },
                Media {
                    id: 2,
                    offset: 16,
                    size: 32
                },
            ]
        );
    }

    #[test]
    fn a_partial_didx_record_desynchronises_like_the_client() {
        // 4 bytes of a record: count is 0, so the 4 bytes are left in the
        // stream and read as a section header, which then runs past the end.
        let mut didx = vec![0u8; 4];
        didx.extend_from_slice(&0x20u32.to_le_bytes());
        let e = parse(&bank(0x91, Some((*b"DIDX", &didx)))).unwrap_err();
        assert!(matches!(e, Error::SectionPastEnd { .. }), "{e:?}");
    }

    #[test]
    fn a_section_past_the_end_is_rejected() {
        let mut bytes = bank(0x91, Some((*b"HIRC", &[0; 4])));
        // The size field sits in the 8-byte section header, just before the
        // four payload bytes.
        let at = bytes.len() - 8;
        bytes[at..at + 4].copy_from_slice(&0xffu32.to_le_bytes());
        let e = parse(&bytes).unwrap_err();
        assert!(
            matches!(e, Error::SectionPastEnd { ref id, .. } if *id == *b"HIRC"),
            "{e:?}"
        );
    }

    #[test]
    fn a_trailing_fragment_is_rejected() {
        let mut bytes = bank(0x91, None);
        bytes.extend_from_slice(&[0u8; 4]);
        assert_eq!(
            parse(&bytes).unwrap_err(),
            Error::TrailingBytes {
                at: bytes.len() - 4,
                len: 4
            }
        );
    }

    #[test]
    fn a_non_bank_is_rejected_with_its_magic() {
        let e = parse(b"RIFFxxxx").unwrap_err();
        assert_eq!(e, Error::NotBankMagic { found: *b"RIFF" });
        assert_eq!(parse(b"BKHD").unwrap_err(), Error::Short);
    }
}

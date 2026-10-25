//! The bucket index: maps a 9-byte encoding key to a location in a data file.
//!
//! `data/<bucket:02X>*.idx` holds 16 buckets. Each file starts with a
//! `BlockSizeAndHash` section header (`i32` byte count, `i32` hash), a 16-byte
//! file header, padding to the 16-byte-aligned end of that section, then the
//! entry block's own `BlockSizeAndHash` and that many bytes of fixed 18-byte
//! entries:
//!
//! ```text
//! EKey[9]  | location[5, big-endian] | size u32 LE
//! ```
//!
//! The two fields disagree on endianness, which is easy to get wrong: the
//! 40-bit location splits into `archive = value >> 30` and
//! `offset = value & (2^30 - 1)` read big-endian, while `size` is a plain
//! little-endian `u32`. `size` counts the *whole* stored record, including the
//! 30-byte data header when the payload is wrapped in one.
//!
//! Files are preallocated, so a zero-padded tail follows the entries; the entry
//! count comes from the block header, never from the file size.
//!
//! Bucket selection hashes the 9 key bytes: XOR them, then fold the nibbles.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The leading `BlockSizeAndHash` section header.
pub const SECTION_HEADER: usize = 8;
/// The 16-byte file header that follows it (version, bucket, widths, ...).
pub const FILE_HEADER_SIZE: usize = 16;
pub const ENTRY_SIZE: usize = 18;
/// Where entries begin in the usual file: a 16-byte section, padding to 32,
/// then the entry block's 8-byte header. Real files can declare another
/// section size; `parse_bucket_file` computes the offset from it.
pub const ENTRIES_OFFSET: usize = 40;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Malformed(PathBuf, &'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "io error: {e}"),
            Error::Malformed(p, m) => write!(f, "malformed index {}: {m}", p.display()),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

type Result<T> = std::result::Result<T, Error>;

/// Where a stored record lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Location {
    /// Ordinal of `data/data.NNN` holding the record.
    pub archive: u32,
    /// Byte offset of the record within that archive.
    pub offset: u64,
    /// Stored length, including any 30-byte data header.
    pub size: u32,
}

/// Parse one bucket file. `expect_bucket` is verified against the header.
pub fn parse_bucket_file(
    path: &Path,
    expect_bucket: Option<u8>,
) -> Result<Vec<([u8; 9], Location)>> {
    let data = fs::read(path)?;
    if data.len() < SECTION_HEADER + FILE_HEADER_SIZE {
        return Err(Error::Malformed(
            path.to_path_buf(),
            "file shorter than its header",
        ));
    }

    // The section header's byte count positions the entry block. The client
    // (`0x14250fcf0`) requires it to be at least 0x10 and rounds
    // `sectionSize + 0x17` down to a 16-byte boundary; the entry block's own
    // 8-byte BlockSizeAndHash follows, then the entries. The usual file has
    // sectionSize 16 -> entry header at 32 -> entries at 40.
    let section_size = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    if section_size < 0x10 {
        return Err(Error::Malformed(
            path.to_path_buf(),
            "section header is smaller than 16 bytes",
        ));
    }
    let entry_header = (section_size as usize + 0x17) & !0xf;
    if data.len() < entry_header + SECTION_HEADER {
        return Err(Error::Malformed(
            path.to_path_buf(),
            "entry block header runs past the end of the file",
        ));
    }
    let entries_offset = entry_header + SECTION_HEADER;

    let version = u16::from_le_bytes([data[8], data[9]]);
    let bucket = data[10];
    let span_size = data[12];
    let span_offs = data[13];
    let ekey_bytes = data[14];

    // The client's parser (`FUN_14250fcf0`) accepts any version >= 7 and
    // checks only the three field widths it hardcodes: 4-byte size, 5-byte
    // location, 9-byte key. It never looks at the extra-bytes or offset-bits
    // bytes, so neither do we; a v8 file with the same widths reads the same.
    if version < 7 || span_size != 4 || span_offs != 5 || ekey_bytes != 9 {
        return Err(Error::Malformed(
            path.to_path_buf(),
            "unsupported index header",
        ));
    }
    if let Some(expected) = expect_bucket {
        if bucket != expected {
            return Err(Error::Malformed(
                path.to_path_buf(),
                "bucket number disagrees with filename",
            ));
        }
    }

    let block_size = i32::from_le_bytes([
        data[entry_header],
        data[entry_header + 1],
        data[entry_header + 2],
        data[entry_header + 3],
    ]);
    if block_size < 0 || block_size as usize % ENTRY_SIZE != 0 {
        return Err(Error::Malformed(
            path.to_path_buf(),
            "entry block size is not a multiple of 18",
        ));
    }
    let count = block_size as usize / ENTRY_SIZE;

    let end = entries_offset + count * ENTRY_SIZE;
    if data.len() < end {
        return Err(Error::Malformed(
            path.to_path_buf(),
            "entries run past the end of the file",
        ));
    }

    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let e = &data[entries_offset + i * ENTRY_SIZE..entries_offset + (i + 1) * ENTRY_SIZE];
        let mut ekey = [0u8; 9];
        ekey.copy_from_slice(&e[0..9]);

        let mut packed: u64 = 0;
        for b in &e[9..14] {
            packed = (packed << 8) | u64::from(*b);
        }
        let size = u32::from_le_bytes([e[14], e[15], e[16], e[17]]);

        out.push((
            ekey,
            Location {
                archive: (packed >> 30) as u32,
                offset: packed & ((1 << 30) - 1),
                size,
            },
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bucket_file(version: u16, bucket: u8, entries: &[([u8; 9], u64, u32)]) -> Vec<u8> {
        let mut out = vec![0u8; ENTRIES_OFFSET];
        out[0..4].copy_from_slice(&16u32.to_le_bytes());
        out[8..10].copy_from_slice(&version.to_le_bytes());
        out[10] = bucket;
        out[12] = 4;
        out[13] = 5;
        out[14] = 9;
        out[15] = 30;
        out[32..36].copy_from_slice(&((entries.len() * ENTRY_SIZE) as i32).to_le_bytes());
        for (key, packed, size) in entries {
            out.extend_from_slice(key);
            out.extend_from_slice(&packed.to_be_bytes()[3..8]);
            out.extend_from_slice(&size.to_le_bytes());
        }
        out
    }

    fn write_temp(name: &str, bytes: &[u8]) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("ow-casc-idx-{}-{name}.idx", std::process::id()));
        fs::write(&path, bytes).expect("write synthetic bucket");
        path
    }

    #[test]
    fn reads_entries_and_both_endiannesses() {
        let key = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99];
        let archive = 5u64;
        let offset = 0x1234_5678u64;
        let packed = (archive << 30) | offset;
        let path = write_temp("entries", &bucket_file(7, 3, &[(key, packed, 0x1122_3344)]));
        let entries = parse_bucket_file(&path, Some(3)).expect("parses");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, key);
        assert_eq!(
            entries[0].1,
            Location {
                archive: 5,
                offset: 0x1234_5678,
                size: 0x1122_3344,
            }
        );
        fs::remove_file(&path).ok();
    }

    /// The client accepts version >= 7; it only hardcodes the 4/5/9 widths.
    #[test]
    fn accepts_a_newer_version_with_the_same_widths() {
        let key = [7u8; 9];
        let path = write_temp("v8", &bucket_file(8, 0, &[(key, 1 << 30, 7)]));
        let entries = parse_bucket_file(&path, Some(0)).expect("v8 parses");
        assert_eq!(entries[0].1.archive, 1);
        assert_eq!(entries[0].1.offset, 0);
        fs::remove_file(&path).ok();
    }

    /// The entry block is positioned by the section header's byte count, not
    /// by a fixed 0x28. The client aligns `sectionSize + 0x17` to 16 bytes;
    /// this file declares a 32-byte section and must still read.
    #[test]
    fn finds_the_entry_block_from_the_section_size() {
        let key = [0x42u8; 9];
        let section = 32usize;
        let entry_header = (section + 0x17) & !0xf;
        let mut data = vec![0u8; entry_header + 8 + ENTRY_SIZE];
        data[0..4].copy_from_slice(&(section as u32).to_le_bytes());
        data[8..10].copy_from_slice(&7u16.to_le_bytes());
        data[10] = 2;
        data[12] = 4;
        data[13] = 5;
        data[14] = 9;
        data[entry_header..entry_header + 4].copy_from_slice(&(ENTRY_SIZE as i32).to_le_bytes());
        let at = entry_header + 8;
        data[at..at + 9].copy_from_slice(&key);
        data[at + 9..at + 14].copy_from_slice(&(1u64 << 30).to_be_bytes()[3..8]);
        data[at + 14..at + 18].copy_from_slice(&99u32.to_le_bytes());

        let path = write_temp("section", &data);
        let entries = parse_bucket_file(&path, Some(2)).expect("parses");
        assert_eq!(entries[0].0, key);
        assert_eq!(entries[0].1.archive, 1);
        assert_eq!(entries[0].1.size, 99);
        fs::remove_file(&path).ok();
    }

    #[test]
    fn rejects_an_older_version_and_a_foreign_bucket() {
        let key = [7u8; 9];
        let path = write_temp("v6", &bucket_file(6, 0, &[(key, 0, 7)]));
        assert!(parse_bucket_file(&path, Some(0)).is_err());
        fs::remove_file(&path).ok();

        let path = write_temp("bucket", &bucket_file(7, 1, &[(key, 0, 7)]));
        assert!(parse_bucket_file(&path, Some(2)).is_err());
        fs::remove_file(&path).ok();
    }
}

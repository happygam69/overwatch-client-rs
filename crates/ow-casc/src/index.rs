//! The bucket index: maps a 9-byte encoding key to a location in a data file.
//!
//! `data/<bucket:02X>*.idx` holds 16 buckets. Each file starts with a 32-byte
//! header, then a `BlockSizeAndHash` (`i32` byte count, `i32` hash), then that
//! many bytes of fixed 18-byte entries:
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

pub const HEADER_SIZE: usize = 32;
pub const ENTRY_SIZE: usize = 18;
/// Entries begin after the header and the entry-table block header.
pub const ENTRIES_OFFSET: usize = HEADER_SIZE + 8;

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

/// Which of the sixteen bucket files a key belongs to.
pub fn bucket_of(ekey: &[u8; 9]) -> u8 {
    let mut x: u8 = 0;
    for b in ekey {
        x ^= b;
    }
    (x & 0x0F) ^ (x >> 4)
}

/// Parse one bucket file. `expect_bucket` is verified against the header.
pub fn parse_bucket_file(path: &Path, expect_bucket: Option<u8>) -> Result<Vec<([u8; 9], Location)>> {
    let data = fs::read(path)?;
    if data.len() < ENTRIES_OFFSET {
        return Err(Error::Malformed(path.to_path_buf(), "file shorter than its header"));
    }

    let version = u16::from_le_bytes([data[8], data[9]]);
    let bucket = data[10];
    let extra_bytes = data[11];
    let span_size = data[12];
    let span_offs = data[13];
    let ekey_bytes = data[14];
    let offset_bits = data[15];

    if version != 7
        || extra_bytes != 0
        || span_size != 4
        || span_offs != 5
        || ekey_bytes != 9
        || offset_bits != 30
    {
        return Err(Error::Malformed(path.to_path_buf(), "unsupported index header"));
    }
    if let Some(expected) = expect_bucket {
        if bucket != expected {
            return Err(Error::Malformed(path.to_path_buf(), "bucket number disagrees with filename"));
        }
    }

    let block_size = i32::from_le_bytes([data[32], data[33], data[34], data[35]]);
    if block_size < 0 || block_size as usize % ENTRY_SIZE != 0 {
        return Err(Error::Malformed(path.to_path_buf(), "entry block size is not a multiple of 18"));
    }
    let count = block_size as usize / ENTRY_SIZE;

    let end = ENTRIES_OFFSET + count * ENTRY_SIZE;
    if data.len() < end {
        return Err(Error::Malformed(path.to_path_buf(), "entries run past the end of the file"));
    }

    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let e = &data[ENTRIES_OFFSET + i * ENTRY_SIZE..ENTRIES_OFFSET + (i + 1) * ENTRY_SIZE];
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

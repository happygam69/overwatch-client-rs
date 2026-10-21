//! The lighting manifest (type `0x0BD`): a small asset that names the GUIDs of
//! the lighting chunks that belong to it.
//!
//! The reference reader (`teLightingManifest`) describes a 32-byte header —
//! magic, four unknowns, an `i32` count, two more unknowns — followed by
//! 8-byte `{u32, i16, i16}` records. No manifest on the install inspected has
//! that shape. Every one carries the magic `EEDC`, a 40-byte header, and then a
//! plain array of 8-byte GUIDs, all of type `0x0BE` (lighting chunk). The
//! layout below is read off those assets; fields the install did not explain
//! keep their offset and are named unknowns.
//!
//! ```text
//! 0x00  u32 magic "EEDC"
//! 0x04  u32 data_size    asset length - 8 on every manifest inspected
//! 0x08  u32 unknown1     constant 45 on every manifest inspected
//! 0x0C  u32 unknown2     0 on 26 of the 27 manifests inspected, 1 on the other
//! 0x10  u32 unknown3     0 on every manifest inspected
//! 0x14  u32 unknown4     equal to unknown5, or exactly 32 greater, on all 27
//! 0x18  u32 unknown5     a multiple of 32 on every manifest inspected
//! 0x1C  u32 chunk_count  40 + 8 * count == asset length on every one inspected
//! 0x20  u32 unknown6     0 on every manifest inspected
//! 0x24  u32 unknown7     0 on every manifest inspected
//! 0x28  chunk_count × u64 chunk GUID (type 0x0BE)
//! ```
//!
//! Evidence class: *install* (bytes read off the manifests in `E:\Overwatch`
//! via `bin/light.rs` and `tests/lighting_real.rs`); the reference file it
//! diverges from is *decompiled*. See `docs/formats/lighting.md`.

use std::fmt;

/// Asset type number of a lighting manifest.
pub const TYPE: u16 = 0x0BD;

/// Asset type number of the lighting chunks a manifest names.
pub const CHUNK_TYPE: u16 = 0x0BE;

/// Magic carried by every lighting manifest on the install inspected: the
/// bytes `45 45 44 43`, which read as a little-endian `u32` are this value.
pub const MAGIC: u32 = 0x4344_4545;

/// Header size: the chunk array starts here.
pub const HEADER_SIZE: usize = 40;
/// One `u64` GUID per chunk record.
pub const CHUNK_SIZE: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub magic: u32,
    /// Asset length minus eight, on every manifest inspected.
    pub data_size: u32,
    /// Constant 45 on every manifest inspected; meaning unknown.
    pub unknown1: u32,
    /// `0` on 26 of the 27 manifests inspected, `1` on the other; meaning
    /// unknown.
    pub unknown2: u32,
    /// `0` on every manifest inspected.
    pub unknown3: u32,
    /// Equal to `unknown5`, or exactly 32 greater, on all 27 manifests
    /// inspected; meaning unknown (both values are multiples of 32).
    pub unknown4: u32,
    /// Equal to `unknown4`, or exactly 32 smaller, on all 27 manifests
    /// inspected; meaning unknown.
    pub unknown5: u32,
    /// Number of chunk GUID records that follow the header.
    pub chunk_count: u32,
    /// `0` on every manifest inspected.
    pub unknown6: u32,
    /// `0` on every manifest inspected.
    pub unknown7: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chunk {
    /// GUID of a lighting chunk (type `0x0BE`). `crate::guid::type_of` on this
    /// value always yields [`CHUNK_TYPE`] on the manifests inspected.
    pub guid: u64,
}

#[derive(Debug, Clone)]
pub struct LightingManifest {
    pub header: Header,
    pub chunks: Vec<Chunk>,
}

#[derive(Debug)]
pub enum Error {
    Malformed(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Malformed(m) => write!(f, "malformed lighting manifest: {m}"),
        }
    }
}

impl std::error::Error for Error {}

type Result<T> = std::result::Result<T, Error>;

fn rd_u32(data: &[u8], at: usize) -> Result<u32> {
    data.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or(Error::Malformed("read past the end"))
}

fn rd_u64(data: &[u8], at: usize) -> Result<u64> {
    data.get(at..at + 8)
        .map(|s| u64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))
        .ok_or(Error::Malformed("read past the end"))
}

/// Parse a lighting manifest asset.
pub fn parse(data: &[u8]) -> Result<LightingManifest> {
    if data.len() < HEADER_SIZE {
        return Err(Error::Malformed("shorter than its 40-byte header"));
    }
    let magic = rd_u32(data, 0)?;
    if magic != MAGIC {
        return Err(Error::Malformed("bad magic"));
    }

    let header = Header {
        magic,
        data_size: rd_u32(data, 4)?,
        unknown1: rd_u32(data, 8)?,
        unknown2: rd_u32(data, 12)?,
        unknown3: rd_u32(data, 16)?,
        unknown4: rd_u32(data, 20)?,
        unknown5: rd_u32(data, 24)?,
        chunk_count: rd_u32(data, 28)?,
        unknown6: rd_u32(data, 32)?,
        unknown7: rd_u32(data, 36)?,
    };

    let count = header.chunk_count as usize;
    let needed = HEADER_SIZE
        .checked_add(
            count
                .checked_mul(CHUNK_SIZE)
                .ok_or(Error::Malformed("chunk count overflows"))?,
        )
        .ok_or(Error::Malformed("chunk count overflows"))?;
    if needed > data.len() {
        return Err(Error::Malformed("chunk records run past the end"));
    }

    let mut chunks = Vec::with_capacity(count);
    for i in 0..count {
        chunks.push(Chunk {
            guid: rd_u64(data, HEADER_SIZE + i * CHUNK_SIZE)?,
        });
    }

    Ok(LightingManifest { header, chunks })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A manifest laid out the way the install writes one: the 40-byte header
    /// then one `u64` GUID per chunk.
    fn synthetic(chunk_count: u32, chunks: &[u64]) -> Vec<u8> {
        let mut out = Vec::new();
        let len = HEADER_SIZE + chunks.len() * CHUNK_SIZE;
        out.extend_from_slice(&MAGIC.to_le_bytes());
        out.extend_from_slice(&(len as u32 - 8).to_le_bytes());
        out.extend_from_slice(&45u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0x1234_5678u32.to_le_bytes());
        out.extend_from_slice(&0x1234_5678u32.to_le_bytes());
        out.extend_from_slice(&chunk_count.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        for guid in chunks {
            out.extend_from_slice(&guid.to_le_bytes());
        }
        out
    }

    #[test]
    fn parses_a_synthetic_manifest() {
        let guids = [0x0BD0_0055_5035_4CD4u64, 0x0BD0_502A_6BBA_3E67u64];
        let data = synthetic(2, &guids);
        assert_eq!(data.len(), HEADER_SIZE + 2 * CHUNK_SIZE);

        let m = parse(&data).expect("parses");
        assert_eq!(m.header.magic, MAGIC);
        assert_eq!(m.header.data_size, data.len() as u32 - 8);
        assert_eq!(m.header.unknown1, 45);
        assert_eq!(m.header.unknown2, 0);
        assert_eq!(m.header.unknown3, 0);
        assert_eq!(m.header.unknown4, 0x1234_5678);
        assert_eq!(m.header.unknown5, 0x1234_5678);
        assert_eq!(m.header.chunk_count, 2);
        assert_eq!(m.header.unknown6, 0);
        assert_eq!(m.header.unknown7, 0);
        assert_eq!(m.chunks.len(), 2);
        assert_eq!(m.chunks[0], Chunk { guid: guids[0] });
        assert_eq!(m.chunks[1], Chunk { guid: guids[1] });
        assert_eq!(crate::guid::type_of(m.chunks[0].guid), CHUNK_TYPE);
    }

    /// The offsets are the whole contract: a field at the wrong place still
    /// parses, so the test pins each one against hand-written bytes.
    #[test]
    fn every_header_field_lands_where_the_install_writes_it() {
        let mut data = synthetic(1, &[0xDEAD_BEEF_DEAD_BEEFu64]);
        // File offsets, not field names: magic 0, size 4, unknowns 8/12/16/20/24,
        // count 28, unknowns 32/36, records at 40.
        data[4..8].copy_from_slice(&0x1111_1111u32.to_le_bytes());
        data[8..12].copy_from_slice(&0x2222_2222u32.to_le_bytes());
        data[12..16].copy_from_slice(&0x3333_3333u32.to_le_bytes());
        data[16..20].copy_from_slice(&0x4444_4444u32.to_le_bytes());
        data[20..24].copy_from_slice(&0x5555_5555u32.to_le_bytes());
        data[24..28].copy_from_slice(&0x6666_6666u32.to_le_bytes());
        data[28..32].copy_from_slice(&1u32.to_le_bytes());
        data[32..36].copy_from_slice(&0x7777_7777u32.to_le_bytes());
        data[36..40].copy_from_slice(&0x8888_8888u32.to_le_bytes());

        let m = parse(&data).expect("parses");
        assert_eq!(m.header.data_size, 0x1111_1111);
        assert_eq!(m.header.unknown1, 0x2222_2222);
        assert_eq!(m.header.unknown2, 0x3333_3333);
        assert_eq!(m.header.unknown3, 0x4444_4444);
        assert_eq!(m.header.unknown4, 0x5555_5555);
        assert_eq!(m.header.unknown5, 0x6666_6666);
        assert_eq!(m.header.chunk_count, 1);
        assert_eq!(m.header.unknown6, 0x7777_7777);
        assert_eq!(m.header.unknown7, 0x8888_8888);
        assert_eq!(m.chunks[0].guid, 0xDEAD_BEEF_DEAD_BEEF);
    }

    #[test]
    fn rejects_a_wrong_magic() {
        let mut data = synthetic(0, &[]);
        data[0..4].copy_from_slice(&0u32.to_le_bytes());
        assert!(matches!(parse(&data), Err(Error::Malformed("bad magic"))));
    }

    #[test]
    fn rejects_a_count_that_runs_past_the_end() {
        let mut data = synthetic(0, &[]);
        data[28..32].copy_from_slice(&3u32.to_le_bytes());
        assert!(matches!(
            parse(&data),
            Err(Error::Malformed("chunk records run past the end"))
        ));
    }

    #[test]
    fn rejects_a_short_header() {
        assert!(matches!(
            parse(&[0u8; HEADER_SIZE - 1]),
            Err(Error::Malformed("shorter than its 40-byte header"))
        ));
    }
}

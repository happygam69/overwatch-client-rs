//! The chunked-data container, which is how Overwatch stores everything with
//! geometry in it: models, skeletons, map chunks, effects.
//!
//! ```text
//! header   u32 magic (0xF123456F), u32 id, i32 size, i32 unknown
//! entries  per chunk: u32 id, i32 serialized size, i32 chunk size,
//!                     u16 serialization tag, u16 chunk version
//! data     each chunk's payload, in order
//! ```
//!
//! A chunk's `id` is its four-character tag with the bytes reversed and XORed,
//! so `StringIdentifier` is how the format names them: `MODL`, `MESH`, `SKEL`.
//! Reading the tags first is the cheapest way to see what an asset holds before
//! writing a parser for any of it.
//!
//! Note the header is 16 bytes and the entries follow it immediately; the
//! payloads are not interleaved with the table.

use std::fmt;

pub const MAGIC: u32 = 0xF123_456F;
pub const HEADER_SIZE: usize = 16;
pub const ENTRY_SIZE: usize = 16;

#[derive(Debug)]
pub enum Error {
    NotChunked,
    Malformed(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotChunked => write!(f, "not a chunked-data asset (bad magic)"),
            Error::Malformed(m) => write!(f, "malformed chunked data: {m}"),
        }
    }
}

impl std::error::Error for Error {}

type Result<T> = std::result::Result<T, Error>;

/// Turn a chunk id into its four-character tag.
///
/// The reference tooling calls this `ReverseXor`, which is a misleading name: it
/// is an XOR-swap of the outer characters inward, which is to say a plain
/// byte-order reversal. There is no XOR constant, and inventing one produces
/// plausible-looking four-character tags that are all wrong.
pub fn decode_tag(id: u32) -> [u8; 4] {
    let mut out = id.to_le_bytes();
    out.reverse();
    out
}

/// The inverse: encode a tag into the id the file stores.
pub fn encode_tag(tag: &[u8; 4]) -> u32 {
    let mut bytes = *tag;
    bytes.reverse();
    u32::from_le_bytes(bytes)
}

/// One chunk's table entry, with its payload located.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkEntry {
    /// Four-character tag, e.g. `MODL`.
    pub tag: [u8; 4],
    /// Offset of the payload within the asset.
    pub offset: usize,
    /// Stored length.
    pub size: usize,
    pub serialized_size: i32,
    pub serialization_tag: u16,
    pub version: u16,
}

impl ChunkEntry {
    /// The tag as text, with non-printable bytes shown as `.`.
    pub fn tag_str(&self) -> String {
        self.tag
            .iter()
            .map(|b| if b.is_ascii_graphic() { *b as char } else { '.' })
            .collect()
    }
}

/// The chunk table of an asset.
#[derive(Debug, Clone)]
pub struct Chunks {
    pub entries: Vec<ChunkEntry>,
}

impl Chunks {
    /// Parse the container and locate every chunk.
    pub fn parse(data: &[u8]) -> Result<Chunks> {
        if data.len() < HEADER_SIZE {
            return Err(Error::NotChunked);
        }
        let magic = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        if magic != MAGIC {
            return Err(Error::NotChunked);
        }
        let header_size = i32::from_le_bytes([data[8], data[9], data[10], data[11]]);
        if header_size < 0 {
            return Err(Error::Malformed("negative header size"));
        }

        // The header's size field is the table's own length, and the payloads
        // start after it.
        let table_start = HEADER_SIZE;
        let table_bytes = (header_size as usize).saturating_sub(HEADER_SIZE);
        if table_bytes % ENTRY_SIZE != 0 {
            return Err(Error::Malformed("chunk table is not a whole number of entries"));
        }
        let count = table_bytes / ENTRY_SIZE;
        if table_start + count * ENTRY_SIZE > data.len() {
            return Err(Error::Malformed("chunk table runs past the end"));
        }

        let mut entries = Vec::with_capacity(count);
        let mut payload = table_start + count * ENTRY_SIZE;
        for i in 0..count {
            let at = table_start + i * ENTRY_SIZE;
            let id = u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]);
            let serialized_size =
                i32::from_le_bytes([data[at + 4], data[at + 5], data[at + 6], data[at + 7]]);
            let chunk_size =
                i32::from_le_bytes([data[at + 8], data[at + 9], data[at + 10], data[at + 11]]);
            let serialization_tag = u16::from_le_bytes([data[at + 12], data[at + 13]]);
            let version = u16::from_le_bytes([data[at + 14], data[at + 15]]);

            if chunk_size < 0 || payload + chunk_size as usize > data.len() {
                return Err(Error::Malformed("chunk payload runs past the end"));
            }
            entries.push(ChunkEntry {
                tag: decode_tag(id),
                offset: payload,
                size: chunk_size as usize,
                serialized_size,
                serialization_tag,
                version,
            });
            payload += chunk_size as usize;
        }

        Ok(Chunks { entries })
    }

    /// The payload of a chunk, by tag.
    pub fn payload<'a>(&self, data: &'a [u8], tag: &[u8; 4]) -> Option<&'a [u8]> {
        let entry = self.entries.iter().find(|e| &e.tag == tag)?;
        data.get(entry.offset..entry.offset + entry.size)
    }

    /// A one-line summary of what the asset holds.
    pub fn summary(&self) -> String {
        self.entries
            .iter()
            .map(|e| format!("{}v{}", e.tag_str(), e.version))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a container with the given chunks, so the parser can be exercised
    /// without a game asset.
    fn synthetic(chunks: &[(&[u8; 4], u16, &[u8])]) -> Vec<u8> {
        let table_size = HEADER_SIZE + chunks.len() * ENTRY_SIZE;
        let mut out = Vec::new();
        out.extend_from_slice(&MAGIC.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&(table_size as i32).to_le_bytes());
        out.extend_from_slice(&0i32.to_le_bytes());
        for (tag, version, payload) in chunks {
            // Encode the tag the way the file does, then let the parser undo it.
            out.extend_from_slice(&encode_tag(tag).to_le_bytes());
            out.extend_from_slice(&(payload.len() as i32).to_le_bytes());
            out.extend_from_slice(&(payload.len() as i32).to_le_bytes());
            out.extend_from_slice(&version.to_le_bytes());
            out.extend_from_slice(&1u16.to_le_bytes());
        }
        for (_, _, payload) in chunks {
            out.extend_from_slice(payload);
        }
        out
    }

    #[test]
    fn tags_round_trip() {
        for tag in [b"MODL", b"MESH", b"SKEL", b"MAPC"] {
            assert_eq!(&decode_tag(encode_tag(tag)), tag);
            assert_eq!(encode_tag(&decode_tag(encode_tag(tag))), encode_tag(tag));
        }
    }

    #[test]
    fn parses_a_synthetic_container() {
        let data = synthetic(&[
            (b"MODL", 4, b"model payload"),
            (b"SKEL", 2, b"skeleton"),
        ]);
        let chunks = Chunks::parse(&data).expect("parses");
        assert_eq!(chunks.entries.len(), 2);
        assert_eq!(chunks.entries[0].tag_str(), "MODL");
        assert_eq!(chunks.entries[0].version, 4);
        assert_eq!(chunks.entries[1].tag_str(), "SKEL");
        assert_eq!(chunks.payload(&data, b"MODL"), Some(&b"model payload"[..]));
        assert_eq!(chunks.payload(&data, b"SKEL"), Some(&b"skeleton"[..]));
        assert_eq!(chunks.payload(&data, b"NOPE"), None);
    }

    #[test]
    fn rejects_something_that_is_not_a_container() {
        assert!(matches!(
            Chunks::parse(b"not a chunked asset at all"),
            Err(Error::NotChunked)
        ));
        assert!(matches!(Chunks::parse(&[]), Err(Error::NotChunked)));
    }

    #[test]
    fn rejects_a_table_that_runs_past_the_end() {
        let mut data = synthetic(&[(b"MODL", 1, b"x")]);
        // Claim a table far larger than the file.
        data[8..12].copy_from_slice(&(HEADER_SIZE as i32 + ENTRY_SIZE as i32 * 100).to_le_bytes());
        assert!(Chunks::parse(&data).is_err());
    }
}

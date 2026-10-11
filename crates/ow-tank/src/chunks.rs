//! The chunked-data container, which is how Overwatch stores everything with
//! geometry in it: models, skeletons, map chunks, effects.
//!
//! ```text
//! header   u32 magic (0xF123456F), u32 id, i32 size, i32 unknown
//! then, from the end of the header, a run of records:
//!   record u32 id, i32 serialized size, i32 chunk size,
//!          u16 serialization tag, u16 chunk version
//!          followed immediately by `chunk size` bytes of payload
//! ```
//!
//! **The table is interleaved with the payloads, not gathered before them.**
//! A record's payload starts right after the record, so the next record follows
//! the previous payload. Reading it as a flat table followed by a blob of
//! payloads produces a parse that fails on every real asset, which is how this
//! was found.
//!
//! The `serialization tag` doubles as a marker: only records whose tag is
//! [`SENTINEL`] are chunks. When `serialized size` exceeds `chunk size` the
//! chunk owns children, and those children follow it before the next sibling.
//! `serialized size` is therefore the whole subtree's length, which is what
//! advancing by it skips.
//!
//! A record's `id` is its four-character tag with the bytes reversed, so
//! `MODL`, `MESH`, `SKEL` read straight out of the file.

use std::fmt;

pub const MAGIC: u32 = 0xF123_456F;
pub const HEADER_SIZE: usize = 16;
pub const ENTRY_SIZE: usize = 16;

/// A record is a chunk only when its serialization tag has this value.
pub const SENTINEL: u16 = 0xC65C;

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
/// is a plain byte-order reversal. There is no XOR constant, and inventing one
/// produces plausible-looking four-character tags that are all wrong.
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

#[derive(Debug, Clone, Copy)]
pub struct Header {
    pub id: [u8; 4],
    /// Length of everything after the header.
    pub size: i32,
    pub unknown: i32,
}

/// One chunk, with its payload located and its nesting recorded.
#[derive(Debug, Clone, Copy)]
pub struct ChunkEntry {
    /// Four-character tag, e.g. `MODL`.
    pub tag: [u8; 4],
    /// Offset of the payload within the asset.
    pub offset: usize,
    /// The chunk's own length.
    pub size: usize,
    /// Length of the chunk and everything nested under it.
    pub serialized_size: i32,
    pub serialization_tag: u16,
    pub version: u16,
    /// Nesting depth: 0 at the top level.
    pub depth: usize,
}

impl ChunkEntry {
    /// The tag as text, with non-printable bytes shown as `.`.
    pub fn tag_str(&self) -> String {
        self.tag
            .iter()
            .map(|b| if b.is_ascii_graphic() { *b as char } else { '.' })
            .collect()
    }

    /// Whether this chunk owns nested chunks.
    pub fn has_children(&self) -> bool {
        self.serialized_size > self.size as i32
    }
}

/// The chunk tree of an asset, flattened in document order.
#[derive(Debug, Clone)]
pub struct Chunks {
    pub header: Header,
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
        let header = Header {
            id: decode_tag(u32::from_le_bytes([data[4], data[5], data[6], data[7]])),
            size: i32::from_le_bytes([data[8], data[9], data[10], data[11]]),
            unknown: i32::from_le_bytes([data[12], data[13], data[14], data[15]]),
        };
        if header.size < 0 {
            return Err(Error::Malformed("negative size"));
        }

        // The size counts everything after the header.
        let end = (HEADER_SIZE + header.size as usize).min(data.len());
        let mut entries = Vec::new();
        let mut at = HEADER_SIZE;
        while at + ENTRY_SIZE <= end {
            let next = walk(data, at, end, 0, &mut entries)?;
            if next <= at {
                break;
            }
            at = next;
        }

        Ok(Chunks { header, entries })
    }

    /// The payload of a chunk, by tag. Searches nested chunks too.
    pub fn payload<'a>(&self, data: &'a [u8], tag: &[u8; 4]) -> Option<&'a [u8]> {
        let entry = self.entries.iter().find(|e| &e.tag == tag)?;
        data.get(entry.offset..entry.offset + entry.size)
    }

    /// Every chunk carrying the given tag, in document order.
    pub fn all_with_tag<'a>(&'a self, tag: &[u8; 4]) -> Vec<&'a ChunkEntry> {
        self.entries.iter().filter(|e| &e.tag == tag).collect()
    }

    /// A one-line summary of what the asset holds.
    pub fn summary(&self) -> String {
        self.entries
            .iter()
            .map(|e| format!("{}{}v{}", "  ".repeat(e.depth), e.tag_str(), e.version))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn read_entry(data: &[u8], at: usize) -> Result<ChunkEntry> {
    if at + ENTRY_SIZE > data.len() {
        return Err(Error::Malformed("chunk record runs past the end"));
    }
    let rd32 = |o: usize| -> i32 {
        i32::from_le_bytes([data[at + o], data[at + o + 1], data[at + o + 2], data[at + o + 3]])
    };
    Ok(ChunkEntry {
        tag: decode_tag(rd32(0) as u32),
        offset: at + ENTRY_SIZE,
        size: 0,
        serialized_size: rd32(4),
        serialization_tag: u16::from_le_bytes([data[at + 12], data[at + 13]]),
        version: u16::from_le_bytes([data[at + 14], data[at + 15]]),
        depth: 0,
    })
}

/// Walk one run of chunks, appending to `out`, and return where it ended.
///
/// A run is a record followed by its payload, repeated while the records carry
/// the sentinel tag; the first record without it terminates the run, and its
/// `serialized size` says how far past it to resume.
fn walk(
    data: &[u8],
    entry_at: usize,
    end: usize,
    depth: usize,
    out: &mut Vec<ChunkEntry>,
) -> Result<usize> {
    let mut entry_at = entry_at;
    let mut entry = read_entry(data, entry_at)?;
    let root_size = entry.serialized_size;
    let mut payload_at = entry_at + ENTRY_SIZE;
    let run_start = payload_at;

    loop {
        if entry.serialization_tag != SENTINEL {
            // Terminator: skip its payload and hand back the position.
            return Ok(payload_at + entry.serialized_size.max(0) as usize);
        }

        let chunk_size = i32::from_le_bytes([
            data[entry_at + 8],
            data[entry_at + 9],
            data[entry_at + 10],
            data[entry_at + 11],
        ]);
        if chunk_size < 0 || payload_at + chunk_size as usize > data.len() {
            return Err(Error::Malformed("chunk payload runs past the end"));
        }

        let mut recorded = entry;
        recorded.offset = payload_at;
        recorded.size = chunk_size as usize;
        recorded.depth = depth;
        out.push(recorded);

        let mut cursor = payload_at + chunk_size as usize;
        if entry.serialized_size > chunk_size {
            // Children follow this chunk's own payload.
            while cursor < end {
                let before = cursor;
                cursor = walk(data, cursor, end, depth + 1, out)?;
                if cursor <= before {
                    break;
                }
                if cursor - run_start >= root_size as usize {
                    break;
                }
            }
        }

        if cursor - run_start >= root_size.max(0) as usize {
            return Ok(cursor);
        }

        entry_at = cursor;
        if entry_at + ENTRY_SIZE > end {
            return Ok(cursor);
        }
        entry = read_entry(data, entry_at)?;
        payload_at = entry_at + ENTRY_SIZE;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a container the way the file lays one out: record then payload.
    fn synthetic(chunks: &[(&[u8; 4], u16, &[u8])]) -> Vec<u8> {
        let body: usize = chunks.iter().map(|(_, _, p)| ENTRY_SIZE + p.len()).sum();
        let mut out = Vec::new();
        out.extend_from_slice(&MAGIC.to_le_bytes());
        out.extend_from_slice(&encode_tag(b"MAPC").to_le_bytes());
        out.extend_from_slice(&(body as i32).to_le_bytes());
        out.extend_from_slice(&0i32.to_le_bytes());
        for (tag, version, payload) in chunks {
            out.extend_from_slice(&encode_tag(tag).to_le_bytes());
            out.extend_from_slice(&(payload.len() as i32).to_le_bytes());
            out.extend_from_slice(&(payload.len() as i32).to_le_bytes());
            out.extend_from_slice(&SENTINEL.to_le_bytes());
            out.extend_from_slice(&version.to_le_bytes());
            out.extend_from_slice(payload);
        }
        // Terminating record: not a chunk, zero payload.
        out.extend_from_slice(&encode_tag(b"NONE").to_le_bytes());
        out.extend_from_slice(&0i32.to_le_bytes());
        out.extend_from_slice(&0i32.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
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
        assert_eq!(chunks.header.id, *b"MAPC");
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
    }
}

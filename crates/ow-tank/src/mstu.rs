//! The model's own structured data (`MSTU`).
//!
//! The chunk is a two-field header -- offset and size -- pointing at an STU
//! blob inside the chunk's own payload. The reference reader
//! (`teModelChunk_STU`) reads the header, then parses the slice as an STU
//! `STUModel`; its chunk reader hands the chunk a stream sliced to the chunk's
//! own bytes, so the header's offset is relative to the payload start.
//!
//! The install agrees: over 60 models carrying an MSTU payload, the STU blob
//! parsed at `payload[offset..offset+size]` on all 60 and at
//! `asset[offset..offset+size]` on none; `offset` was 16 (its own header) on
//! all 60, while `size` left trailing bytes in 55 of them (5 had
//! `offset + size == payload length`).
//!
//! Evidence class: *install* (via `cargo test --test mstu_real -- --nocapture`)
//! cross-checked against the *decompiled* reader. See `docs/formats/stu.md`.

use std::fmt;

pub const TAG: &[u8; 4] = b"MSTU";

/// Bytes of `{i64 offset, i64 size}` before the STU blob.
const HEADER_SIZE: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MstuHeader {
    /// Relative to the MSTU payload start; 16 on every model inspected.
    pub offset: i64,
    /// Length of the STU blob from `offset`; may leave trailing bytes.
    pub size: i64,
}

#[derive(Debug)]
pub enum Error {
    NotAChunkedAsset,
    MissingChunk,
    Malformed(&'static str),
    Stu(ow_stu::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotAChunkedAsset => write!(f, "not a chunked asset"),
            Error::MissingChunk => write!(f, "model has no MSTU chunk"),
            Error::Malformed(m) => write!(f, "malformed MSTU: {m}"),
            Error::Stu(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<ow_stu::Error> for Error {
    fn from(e: ow_stu::Error) -> Self {
        Error::Stu(e)
    }
}

fn payload_of(asset: &[u8]) -> Result<&[u8], Error> {
    let chunks = crate::chunks::Chunks::parse(asset).map_err(|_| Error::NotAChunkedAsset)?;
    chunks.payload(asset, TAG).ok_or(Error::MissingChunk)
}

fn read_header(payload: &[u8]) -> Result<MstuHeader, Error> {
    if payload.len() < HEADER_SIZE {
        return Err(Error::Malformed("chunk shorter than its 16-byte header"));
    }
    Ok(MstuHeader {
        offset: i64::from_le_bytes(payload[0..8].try_into().unwrap()),
        size: i64::from_le_bytes(payload[8..16].try_into().unwrap()),
    })
}

/// The MSTU header of a model asset, if it has one. `asset` is the whole
/// model file.
pub fn parse_header(asset: &[u8]) -> Result<MstuHeader, Error> {
    read_header(payload_of(asset)?)
}

/// The model's structured data, parsed as STU. `asset` is the whole model
/// file; the header's offset is applied to the MSTU payload, not the asset.
pub fn parse(asset: &[u8]) -> Result<ow_stu::Stu, Error> {
    let payload = payload_of(asset)?;
    let header = read_header(payload)?;
    let at = usize::try_from(header.offset).map_err(|_| Error::Malformed("negative STU offset"))?;
    let size = usize::try_from(header.size).map_err(|_| Error::Malformed("negative STU size"))?;
    let end = at
        .checked_add(size)
        .ok_or(Error::Malformed("STU slice runs past the chunk"))?;
    let blob = payload
        .get(at..end)
        .ok_or(Error::Malformed("STU slice runs past the chunk"))?;
    Ok(ow_stu::Stu::parse(blob.to_vec())?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunks::{encode_tag, ENTRY_SIZE, HEADER_SIZE as CONTAINER_HEADER, MAGIC, SENTINEL};

    /// A one-chunk container asset holding `payload` under `tag`, laid out the
    /// way models on the install are.
    fn synthetic_asset(tag: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let total = CONTAINER_HEADER + ENTRY_SIZE + payload.len();
        out.extend_from_slice(&MAGIC.to_le_bytes());
        out.extend_from_slice(&encode_tag(b"MODL").to_le_bytes());
        out.extend_from_slice(&((total - CONTAINER_HEADER) as i32).to_le_bytes());
        out.extend_from_slice(&0i32.to_le_bytes());
        out.extend_from_slice(&encode_tag(tag).to_le_bytes());
        out.extend_from_slice(&(payload.len() as i32).to_le_bytes());
        out.extend_from_slice(&(payload.len() as i32).to_le_bytes());
        out.extend_from_slice(&SENTINEL.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(payload);
        out
    }

    fn mstu_payload(offset: i64, size: i64, blob: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(blob);
        out
    }

    /// The smallest STU blob `ow-stu` accepts: a version-1 header with no
    /// instances.
    fn minimal_stu() -> Vec<u8> {
        let mut out = Vec::from(*b"STUD");
        out.extend_from_slice(&0u32.to_le_bytes());
        out
    }

    #[test]
    fn parses_a_synthetic_model() {
        let blob = minimal_stu();
        let asset = synthetic_asset(TAG, &mstu_payload(16, blob.len() as i64, &blob));

        let header = parse_header(&asset).expect("header parses");
        assert_eq!(
            header,
            MstuHeader {
                offset: 16,
                size: 8
            }
        );

        let stu = parse(&asset).expect("parses");
        assert!(stu.v1);
        assert!(stu.instances.is_empty());
    }

    #[test]
    fn rejects_a_non_chunked_asset() {
        assert!(matches!(
            parse_header(&[0u8; 64]),
            Err(Error::NotAChunkedAsset)
        ));
        assert!(matches!(parse(&[0u8; 64]), Err(Error::NotAChunkedAsset)));
    }

    #[test]
    fn rejects_a_chunked_asset_without_mstu() {
        let asset = synthetic_asset(b"MESH", &[0u8; 32]);
        assert!(matches!(parse_header(&asset), Err(Error::MissingChunk)));
        assert!(matches!(parse(&asset), Err(Error::MissingChunk)));
    }

    #[test]
    fn rejects_a_short_chunk_and_a_slice_past_the_end() {
        let asset = synthetic_asset(TAG, &[0u8; 15]);
        assert!(matches!(
            parse_header(&asset),
            Err(Error::Malformed("chunk shorter than its 16-byte header"))
        ));

        let asset = synthetic_asset(TAG, &mstu_payload(16, 100, &[0u8; 8]));
        assert!(matches!(
            parse_header(&asset),
            Ok(MstuHeader {
                offset: 16,
                size: 100
            })
        ));
        assert!(matches!(
            parse(&asset),
            Err(Error::Malformed("STU slice runs past the chunk"))
        ));

        let asset = synthetic_asset(TAG, &mstu_payload(-1, 8, &[0u8; 8]));
        assert!(matches!(
            parse(&asset),
            Err(Error::Malformed("negative STU offset"))
        ));
    }

    /// A slice that is in bounds but not a STU blob surfaces the STU error,
    /// not an MSTU one.
    #[test]
    fn surfaces_stu_errors_from_the_blob() {
        let blob = [0xAAu8; 8];
        let asset = synthetic_asset(TAG, &mstu_payload(16, blob.len() as i64, &blob));
        assert!(matches!(parse(&asset), Err(Error::Stu(_))));
    }
}

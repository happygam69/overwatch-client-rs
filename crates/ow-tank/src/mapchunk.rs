//! The map chunk: how a map's contents are laid out in the file.
//!
//! Not a chunked-data container. It is a header and a run of *placeables*, each
//! of which says what to put where:
//!
//! ```text
//! header        u32 placeable count, u32 instance-data offset,
//!               u32 placeable offset, u32 unknown
//! placeable     UUID (16), three flag bytes, u32 size, then the body
//! ```
//!
//! `size` covers the whole placeable including the header, so a placeable whose
//! body is not understood can still be skipped exactly -- which is what makes it
//! safe to read a map whose full vocabulary of placeable kinds is not known.
//!
//! The *kind* of body is not in the placeable. A map chunk asset carries one
//! kind throughout, chosen by the game from the asset's role, so the caller says
//! which kind it is reading. Where that is not known, [`Kind::detect`] tries
//! them and keeps the one whose asset references actually resolve.

use std::fmt;

use crate::guid;

pub const HEADER_SIZE: usize = 16;
pub const COMMON_SIZE: usize = 24;

/// What a map chunk's placeables describe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Static geometry: a model and its placement.
    Model,
    /// Gameplay things: entities with component data attached.
    Entity,
    /// A group of models placed together.
    ModelGroup,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Model => "model",
            Kind::Entity => "entity",
            Kind::ModelGroup => "model group",
        }
    }
}

#[derive(Debug)]
pub enum Error {
    Malformed(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Malformed(m) => write!(f, "malformed map chunk: {m}"),
        }
    }
}

impl std::error::Error for Error {}

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy)]
pub struct Header {
    pub placeable_count: u32,
    /// Offset of the component instance data, 16 bytes before it is usable.
    pub instance_data_offset: u32,
    pub placeable_offset: u32,
    pub unknown: u32,
}

/// A placement: what to put, and where.
#[derive(Debug, Clone, Copy)]
pub struct Placeable {
    pub uuid: [u8; 16],
    /// Index of this placeable within the chunk.
    pub index: usize,
    /// Byte offset of the placeable within the asset.
    pub offset: usize,
    /// Total length including its own header.
    pub size: usize,
    /// Model asset, for the kinds that name one.
    pub model: Option<u64>,
    /// Second asset reference: the model's look/variant.
    pub look: Option<u64>,
    /// Entity definition asset, for entity placeables.
    pub entity: Option<u64>,
    /// Identifier token asset, for entity placeables.
    pub identifier: Option<u64>,
    pub translation: [f32; 3],
    pub scale: [f32; 3],
    /// Rotation as a quaternion, x/y/z/w.
    pub rotation: [f32; 4],
    /// Number of component data blobs attached to an entity.
    pub instance_data_count: u32,
}

#[derive(Debug, Clone)]
pub struct MapChunk {
    pub header: Header,
    pub placeables: Vec<Placeable>,
}

fn rd_u32(data: &[u8], at: usize) -> Option<u32> {
    data.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn rd_f32(data: &[u8], at: usize) -> Option<f32> {
    rd_u32(data, at).map(f32::from_bits)
}

fn rd_vec3(data: &[u8], at: usize) -> Option<[f32; 3]> {
    Some([rd_f32(data, at)?, rd_f32(data, at + 4)?, rd_f32(data, at + 8)?])
}

fn rd_quat(data: &[u8], at: usize) -> Option<[f32; 4]> {
    Some([
        rd_f32(data, at)?,
        rd_f32(data, at + 4)?,
        rd_f32(data, at + 8)?,
        rd_f32(data, at + 12)?,
    ])
}

/// Whether a value looks like an asset reference this install can resolve.
fn plausible(guid: u64) -> bool {
    guid != 0 && guid != u64::MAX
}

impl MapChunk {
    pub fn parse(data: &[u8], kind: Kind) -> Result<MapChunk> {
        if data.len() < HEADER_SIZE {
            return Err(Error::Malformed("shorter than its header"));
        }
        let header = Header {
            placeable_count: rd_u32(data, 0).ok_or(Error::Malformed("truncated header"))?,
            instance_data_offset: rd_u32(data, 4).ok_or(Error::Malformed("truncated header"))?,
            placeable_offset: rd_u32(data, 8).ok_or(Error::Malformed("truncated header"))?,
            unknown: rd_u32(data, 12).ok_or(Error::Malformed("truncated header"))?,
        };

        let mut placeables = Vec::new();
        let mut at = header.placeable_offset as usize;
        for index in 0..header.placeable_count as usize {
            if at + COMMON_SIZE > data.len() {
                break;
            }
            let size = rd_u32(data, at + 20).ok_or(Error::Malformed("truncated placeable"))? as usize;
            if size < COMMON_SIZE || at + size > data.len() {
                break;
            }
            let mut uuid = [0u8; 16];
            uuid.copy_from_slice(&data[at..at + 16]);
            let body = at + COMMON_SIZE;

            let mut placeable = Placeable {
                uuid,
                index,
                offset: at,
                size,
                model: None,
                look: None,
                entity: None,
                identifier: None,
                translation: [0.0; 3],
                scale: [1.0; 3],
                rotation: [0.0, 0.0, 0.0, 1.0],
                instance_data_count: 0,
            };

            match kind {
                // The model reference comes first, then a run of counters, then
                // the placement. The generated type for this structure puts the
                // transform straight after the second reference, which reads
                // zeroes and huge numbers -- the file was followed instead.
                Kind::Model | Kind::ModelGroup => {
                    placeable.model = rd_u32(data, body)
                        .zip(rd_u32(data, body + 4))
                        .map(|(lo, hi)| (lo as u64) | ((hi as u64) << 32));
                    placeable.translation = rd_vec3(data, body + 0x30).unwrap_or([0.0; 3]);
                    placeable.scale = rd_vec3(data, body + 0x3C).unwrap_or([1.0; 3]);
                    placeable.rotation = rd_quat(data, body + 0x48).unwrap_or([0.0, 0.0, 0.0, 1.0]);
                }
                // entity definition, identifier, then translation, scale, rotation
                Kind::Entity => {
                    placeable.entity = rd_u32(data, body)
                        .zip(rd_u32(data, body + 4))
                        .map(|(lo, hi)| (lo as u64) | ((hi as u64) << 32));
                    placeable.identifier = rd_u32(data, body + 8)
                        .zip(rd_u32(data, body + 12))
                        .map(|(lo, hi)| (lo as u64) | ((hi as u64) << 32));
                    placeable.translation = rd_vec3(data, body + 16).unwrap_or([0.0; 3]);
                    placeable.scale = rd_vec3(data, body + 28).unwrap_or([1.0; 3]);
                    placeable.rotation = rd_quat(data, body + 40).unwrap_or([0.0, 0.0, 0.0, 1.0]);
                    placeable.instance_data_count = rd_u32(data, body + 56).unwrap_or(0);
                }
            }

            placeables.push(placeable);
            at += size;
        }

        Ok(MapChunk { header, placeables })
    }

    /// Pick the kind whose references look like real assets.
    ///
    /// `resolves` is asked about each candidate reference. The wrong kind reads
    /// a transform as a GUID, which almost never lands on an asset that exists,
    /// so the right kind is the one that keeps its references.
    pub fn detect(data: &[u8], resolves: &dyn Fn(u64) -> bool) -> Result<(Kind, MapChunk)> {
        let mut best: Option<(Kind, MapChunk, usize)> = None;
        for kind in [Kind::Model, Kind::Entity] {
            let Ok(chunk) = MapChunk::parse(data, kind) else {
                continue;
            };
            let hits = chunk
                .placeables
                .iter()
                .filter(|p| {
                    p.model.or(p.entity).map(|g| resolves(g)).unwrap_or(false)
                })
                .count();
            if best.as_ref().map(|(_, _, b)| hits > *b).unwrap_or(true) {
                best = Some((kind, chunk, hits));
            }
        }
        match best {
            Some((kind, chunk, _)) => Ok((kind, chunk)),
            None => Err(Error::Malformed("no placeable kind fits")),
        }
    }

    /// How many placeables name a model that resolves.
    pub fn resolvable_models(&self, resolves: &dyn Fn(u64) -> bool) -> usize {
        self.placeables
            .iter()
            .filter(|p| p.model.map(|g| resolves(g)).unwrap_or(false))
            .count()
    }

    pub fn summary(&self) -> String {
        let models = self
            .placeables
            .iter()
            .filter(|p| p.model.is_some())
            .count();
        let entities = self
            .placeables
            .iter()
            .filter(|p| p.entity.is_some())
            .count();
        format!(
            "{} placeables ({models} with a model, {entities} with an entity)",
            self.placeables.len()
        )
    }
}

/// The type of a placeable's model reference, for a quick sanity check.
pub fn model_type(guid: u64) -> u16 {
    guid::type_of(guid)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lay out a chunk with one model placeable, the way the file does.
    ///
    /// The body is padded to the offsets the real files use, so this test fails
    /// if those offsets drift rather than quietly agreeing with the parser.
    fn synthetic() -> Vec<u8> {
        const BODY: usize = 0x68;
        let mut out = Vec::new();
        out.extend_from_slice(&1u32.to_le_bytes()); // placeable count
        out.extend_from_slice(&0u32.to_le_bytes()); // instance data
        out.extend_from_slice(&(HEADER_SIZE as u32).to_le_bytes()); // placeable offset
        out.extend_from_slice(&0u32.to_le_bytes());
        // one placeable: uuid, flags, size, body
        out.extend_from_slice(&[0xAB; 16]);
        out.extend_from_slice(&[0, 0, 0]);
        out.extend_from_slice(&[0]);
        out.extend_from_slice(&((COMMON_SIZE + BODY) as u32).to_le_bytes());

        let mut body = vec![0u8; BODY];
        body[0..8].copy_from_slice(&0x0000_00C0_0000_0123u64.to_le_bytes()); // model
        let put = |body: &mut Vec<u8>, at: usize, values: &[f32]| {
            for (i, v) in values.iter().enumerate() {
                body[at + i * 4..at + i * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
        };
        put(&mut body, 0x30, &[1.0, 2.0, 3.0]); // translation
        put(&mut body, 0x3C, &[1.0, 1.0, 1.0]); // scale
        put(&mut body, 0x48, &[0.0, 0.0, 0.0, 1.0]); // rotation
        out.extend_from_slice(&body);
        out
    }

    #[test]
    fn reads_a_model_placeable() {
        let data = synthetic();
        let chunk = MapChunk::parse(&data, Kind::Model).unwrap();
        assert_eq!(chunk.header.placeable_count, 1);
        assert_eq!(chunk.placeables.len(), 1);
        let p = &chunk.placeables[0];
        assert_eq!(p.model, Some(0x0000_00C0_0000_0123));
        assert_eq!(p.translation, [1.0, 2.0, 3.0]);
        assert_eq!(p.scale, [1.0, 1.0, 1.0]);
    }

    #[test]
    fn skips_a_placeable_it_cannot_read() {
        // A second placeable whose body is not understood must still be skipped
        // by its declared size, leaving the run intact.
        let mut data = synthetic();
        data[0..4].copy_from_slice(&2u32.to_le_bytes());
        data.extend_from_slice(&[0xCD; 16]);
        data.extend_from_slice(&[0, 0, 0, 0]);
        data.extend_from_slice(&(COMMON_SIZE as u32 + 4).to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        let chunk = MapChunk::parse(&data, Kind::Model).unwrap();
        assert_eq!(chunk.placeables.len(), 2);
        assert_eq!(chunk.placeables[1].size, COMMON_SIZE + 4);
    }
}

//! Models: the render mesh a map placeable or a character points at.
//!
//! A model asset is chunked data. Two chunks matter here:
//!
//! * `MDLC` -- the material list and the bounding box.
//! * `MRNM` -- the geometry, as vertex buffers, index buffers and submeshes.
//!
//! `MRNM` is not a flat array of vertices. It is a small header of three
//! *offsets* into its own chunk (vertex buffers, index buffers, submeshes),
//! each pointing at an array of descriptors, and the vertex data itself lives
//! at offsets those descriptors give. Reading it as one interleaved stream
//! produces nothing usable, so the offsets are followed.
//!
//! Two shapes of vertex buffer exist. Usually the vertex bytes are inside the
//! chunk, at an offset the descriptor gives. But a descriptor can instead name
//! a *separate asset* holding them, and then the descriptor's stream pointers
//! are asset GUIDs rather than offsets. That is why parsing takes a loader: a
//! model cannot always be read from its own bytes.
//!
//! Submeshes index into the vertex buffer through a remap: the index buffer
//! holds original vertex numbers, and the mesh renumbers the ones it uses. The
//! remap is applied here, so what comes out has its own compact vertex array.

use std::fmt;

use crate::chunks::Chunks;

#[derive(Debug)]
pub enum Error {
    NotAChunkedAsset,
    Malformed(&'static str),
    MissingChunk(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotAChunkedAsset => write!(f, "not a chunked asset"),
            Error::Malformed(m) => write!(f, "malformed model: {m}"),
            Error::MissingChunk(t) => write!(f, "model has no {t} chunk"),
        }
    }
}

impl std::error::Error for Error {}

type Result<T> = std::result::Result<T, Error>;

/// A submesh, already resolved to its own vertex array.
#[derive(Debug, Clone, Default)]
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    /// Index into [`Model::materials`].
    pub material: usize,
    /// Level of detail; `-1` means the submesh is used at every level.
    pub lod: i8,
}

impl Mesh {
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Largest distance from the origin, for a quick size sanity check.
    pub fn radius(&self) -> f32 {
        self.positions
            .iter()
            .map(|p| (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt())
            .fold(0.0f32, f32::max)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Model {
    pub materials: Vec<u64>,
    /// The `MDLC` bounding box, as stored.
    pub bounding_box: [[f32; 4]; 4],
    pub meshes: Vec<Mesh>,
}

impl Model {
    pub fn vertex_count(&self) -> usize {
        self.meshes.iter().map(Mesh::vertex_count).sum()
    }

    pub fn triangle_count(&self) -> usize {
        self.meshes.iter().map(Mesh::triangle_count).sum()
    }
}

// ---------------------------------------------------------------- reading

fn u8_at(d: &[u8], at: usize) -> Option<u8> {
    d.get(at).copied()
}
fn u16_at(d: &[u8], at: usize) -> Option<u16> {
    d.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
}
fn u32_at(d: &[u8], at: usize) -> Option<u32> {
    d.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}
fn i64_at(d: &[u8], at: usize) -> Option<i64> {
    d.get(at..at + 8)
        .map(|b| i64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
}
fn u64_at(d: &[u8], at: usize) -> Option<u64> {
    d.get(at..at + 8)
        .map(|b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
}
fn f32_at(d: &[u8], at: usize) -> Option<f32> {
    u32_at(d, at).map(f32::from_bits)
}
fn half_at(d: &[u8], at: usize) -> Option<f32> {
    let bits = u16_at(d, at)?;
    Some(half_to_f32(bits))
}

/// IEEE 754 binary16 to binary32. Written out rather than pulled in: it is ten
/// lines and it keeps the dependency list honest.
fn half_to_f32(h: u16) -> f32 {
    let sign = (h >> 15) & 1;
    let exponent = (h >> 10) & 0x1F;
    let mantissa = h & 0x3FF;
    let value = match exponent {
        0 => (mantissa as f32) * 2f32.powi(-24),
        0x1F => {
            if mantissa == 0 {
                f32::INFINITY
            } else {
                f32::NAN
            }
        }
        _ => (1.0 + mantissa as f32 / 1024.0) * 2f32.powi(exponent as i32 - 15),
    };
    if sign == 1 {
        -value
    } else {
        value
    }
}

/// Vertex element formats, in the order the files use.
///
/// The names in the reference tooling only cover some of these; the numbering
/// follows the DXGI formats it comments, which is why 0 is one float and 2 is
/// three. Handling only the named ones drops positions silently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    F32,
    F32x2,
    F32x3,
    F32x4,
    F16x2,
    F16x4,
    U8x4,
    U16x4,
    U8Unorm4,
    U8Snorm4,
    U32,
    Unknown(u8),
}

impl Format {
    fn from_byte(b: u8) -> Format {
        match b {
            0x0 => Format::F32,
            0x1 => Format::F32x2,
            0x2 => Format::F32x3,
            0x3 => Format::F32x4,
            0x4 => Format::F16x2,
            0x5 => Format::F16x4,
            0x6 => Format::U8x4,
            0x7 => Format::U16x4,
            0x8 => Format::U8Unorm4,
            0x9 => Format::U8Snorm4,
            0xC => Format::U32,
            other => Format::Unknown(other),
        }
    }

    fn components(self) -> usize {
        match self {
            Format::F32 | Format::U32 => 1,
            Format::F32x2 | Format::F16x2 => 2,
            Format::F32x3 => 3,
            Format::F32x4 | Format::F16x4 | Format::U8x4 | Format::U16x4 => 4,
            Format::U8Unorm4 | Format::U8Snorm4 => 4,
            Format::Unknown(_) => 0,
        }
    }

    /// Read the element as up to four floats, which is all the geometry needs.
    fn read(self, d: &[u8], at: usize) -> Option<[f32; 4]> {
        let mut out = [0.0f32; 4];
        match self {
            Format::F32 => out[0] = f32_at(d, at)?,
            Format::F32x2 => {
                out[0] = f32_at(d, at)?;
                out[1] = f32_at(d, at + 4)?;
            }
            Format::F32x3 => {
                for i in 0..3 {
                    out[i] = f32_at(d, at + i * 4)?;
                }
            }
            Format::F32x4 => {
                for i in 0..4 {
                    out[i] = f32_at(d, at + i * 4)?;
                }
            }
            Format::F16x2 => {
                for i in 0..2 {
                    out[i] = half_at(d, at + i * 2)?;
                }
            }
            Format::F16x4 => {
                for i in 0..4 {
                    out[i] = half_at(d, at + i * 2)?;
                }
            }
            Format::U8x4 => {
                for i in 0..4 {
                    out[i] = u8_at(d, at + i)? as f32;
                }
            }
            Format::U16x4 => {
                for i in 0..4 {
                    out[i] = u16_at(d, at + i * 2)? as f32;
                }
            }
            Format::U8Unorm4 => {
                for i in 0..4 {
                    out[i] = u8_at(d, at + i)? as f32 / 255.0;
                }
            }
            Format::U8Snorm4 => {
                for i in 0..4 {
                    out[i] = (u8_at(d, at + i)? as i8) as f32 / 127.0;
                }
            }
            Format::U32 => out[0] = u32_at(d, at)? as f32,
            Format::Unknown(_) => return None,
        }
        Some(out)
    }
}

/// What a vertex element is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Semantic {
    Position,
    Normal,
    TexCoord,
    Colour,
    Other(u8),
}

impl Semantic {
    fn from_byte(b: u8) -> Semantic {
        match b {
            0 => Semantic::Position,
            1 => Semantic::Normal,
            8 => Semantic::Colour,
            9 => Semantic::TexCoord,
            other => Semantic::Other(other),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Element {
    semantic: Semantic,
    index: u8,
    format: Format,
    stream: u8,
    offset: usize,
}

fn read_elements(d: &[u8], at: usize, count: usize) -> Vec<Element> {
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let base = at + i * 8;
        let Some(semantic) = u8_at(d, base) else { break };
        let Some(index) = u8_at(d, base + 1) else { break };
        let Some(format) = u8_at(d, base + 2) else { break };
        let Some(stream) = u8_at(d, base + 3) else { break };
        // The packing is not documented: the low nibble is the slot class, the
        // next four bits the step rate, and the offset sits above those.
        let Some(packed) = u32_at(d, base + 4) else { break };
        out.push(Element {
            semantic: Semantic::from_byte(semantic),
            index,
            format: Format::from_byte(format),
            stream,
            offset: ((packed >> 12) & 1023) as usize,
        });
    }
    out
}

#[derive(Debug, Clone, Copy, Default)]
struct VertexBuffer {
    vertex_count: u32,
    stride1: u8,
    stride2: u8,
    element_count: u8,
    flags: u8,
    element_offset: usize,
    stream1: i64,
    stream2: i64,
}

impl VertexBuffer {
    /// Whether the vertex bytes live in a separate asset rather than the chunk.
    fn separate_asset(&self) -> bool {
        self.flags & 0x10 != 0
    }

    fn stream_asset(&self, which: usize) -> u64 {
        let raw = if which == 0 { self.stream1 } else { self.stream2 };
        raw as u64
    }

    fn stride(&self, which: usize) -> usize {
        if which == 0 {
            self.stride1 as usize
        } else {
            self.stride2 as usize
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Submesh {
    vertex_start: u32,
    index_start: u32,
    indices_to_draw: u32,
    vertices_to_draw: u32,
    index_buffer: u8,
    vertex_buffer: u8,
    material: u8,
    lod: i8,
}

/// Parse a model asset. `load` resolves a GUID to an asset's bytes, and is only
/// consulted for vertex buffers that keep their data elsewhere.
pub fn parse(asset: &[u8], load: &dyn Fn(u64) -> Option<Vec<u8>>) -> Result<Model> {
    let chunks = Chunks::parse(asset).map_err(|_| Error::NotAChunkedAsset)?;

    let mut model = Model::default();
    if let Some(mdlc) = chunks.payload(asset, b"MDLC") {
        // MaterialOffset at 8, the box at 16, MaterialCount at 104.
        if let Some(offset) = i64_at(mdlc, 8) {
            let count = u16_at(mdlc, 104).unwrap_or(0) as usize;
            if offset > 0 {
                for i in 0..count {
                    if let Some(m) = u64_at(mdlc, offset as usize + i * 8) {
                        model.materials.push(m);
                    }
                }
            }
        }
        for row in 0..4 {
            for col in 0..4 {
                if let Some(v) = f32_at(mdlc, 16 + (row * 4 + col) * 4) {
                    model.bounding_box[row][col] = v;
                }
            }
        }
    }

    let mrnm = chunks
        .payload(asset, b"MRNM")
        .ok_or(Error::MissingChunk("MRNM"))?;
    model.meshes = read_render_mesh(mrnm, load)?;
    Ok(model)
}

fn read_render_mesh(d: &[u8], load: &dyn Fn(u64) -> Option<Vec<u8>>) -> Result<Vec<Mesh>> {
    let vbo_ptr = i64_at(d, 0).ok_or(Error::Malformed("truncated MRNM header"))?;
    let ibo_ptr = i64_at(d, 8).ok_or(Error::Malformed("truncated MRNM header"))?;
    let submesh_ptr = i64_at(d, 16).ok_or(Error::Malformed("truncated MRNM header"))?;
    let vbo_count = u8_at(d, 34).ok_or(Error::Malformed("truncated MRNM header"))? as usize;
    let ibo_count = u8_at(d, 35).ok_or(Error::Malformed("truncated MRNM header"))? as usize;
    let submesh_count = u32_at(d, 28).ok_or(Error::Malformed("truncated MRNM header"))? as usize;

    let mut buffers = Vec::with_capacity(vbo_count);
    for i in 0..vbo_count {
        let at = vbo_ptr as usize + i * 96;
        buffers.push(VertexBuffer {
            vertex_count: u32_at(d, at).unwrap_or(0),
            stride1: u8_at(d, at + 8).unwrap_or(0),
            stride2: u8_at(d, at + 9).unwrap_or(0),
            element_count: u8_at(d, at + 10).unwrap_or(0),
            flags: u8_at(d, at + 11).unwrap_or(0),
            element_offset: i64_at(d, at + 16).unwrap_or(0) as usize,
            stream1: i64_at(d, at + 80).unwrap_or(0),
            stream2: i64_at(d, at + 88).unwrap_or(0),
        });
    }

    let mut index_buffers = Vec::with_capacity(ibo_count);
    for i in 0..ibo_count {
        let at = ibo_ptr as usize + i * 16;
        index_buffers.push((
            u32_at(d, at).unwrap_or(0),
            u32_at(d, at + 4).unwrap_or(0),
            i64_at(d, at + 8).unwrap_or(0),
        ));
    }

    let mut submeshes = Vec::with_capacity(submesh_count);
    for i in 0..submesh_count {
        let at = submesh_ptr as usize + i * 128;
        submeshes.push(Submesh {
            vertex_start: u32_at(d, at + 60).unwrap_or(0),
            index_start: u32_at(d, at + 64).unwrap_or(0),
            indices_to_draw: u32_at(d, at + 100).unwrap_or(0),
            vertices_to_draw: u16_at(d, at + 104).unwrap_or(0) as u32,
            index_buffer: u8_at(d, at + 110).unwrap_or(0),
            vertex_buffer: u8_at(d, at + 118).unwrap_or(0),
            material: u8_at(d, at + 119).unwrap_or(0),
            lod: u8_at(d, at + 120).unwrap_or(0) as i8,
        });
    }

    // Vertex data, per buffer and per stream. The stream's bytes come either
    // from this chunk or from an asset the descriptor names.
    let mut streams: Vec<Vec<Option<Vec<u8>>>> = Vec::with_capacity(buffers.len());
    let mut elements_per_buffer: Vec<Vec<Element>> = Vec::with_capacity(buffers.len());
    for (i, buffer) in buffers.iter().enumerate() {
        let elements = read_elements(d, buffer.element_offset, buffer.element_count as usize);
        let mut owned: Vec<Option<Vec<u8>>> = Vec::new();
        if buffer.separate_asset() {
            for which in 0..2 {
                owned.push(load(buffer.stream_asset(which)));
            }
        } else {
            // Offsets are into the chunk itself.
            for which in 0..2 {
                let offset = if which == 0 { buffer.stream1 } else { buffer.stream2 };
                if offset > 0 {
                    owned.push(Some(d.to_vec()));
                } else {
                    owned.push(None);
                }
            }
        }
        let _ = i;
        streams.push(owned);
        elements_per_buffer.push(elements);
    }

    let mut meshes = Vec::with_capacity(submeshes.len());
    for submesh in &submeshes {
        let buffer_index = submesh.vertex_buffer as usize;
        if buffer_index >= buffers.len() || submesh.indices_to_draw == 0 {
            continue;
        }
        let buffer = &buffers[buffer_index];
        let elements = &elements_per_buffer[buffer_index];

        // Indices, then the remap the mesh applies to the vertices it uses.
        let ibo_index = submesh.index_buffer as usize;
        if ibo_index >= index_buffers.len() {
            continue;
        }
        let (_, _, index_at) = index_buffers[ibo_index];
        let mut original: Vec<u16> = Vec::with_capacity(submesh.indices_to_draw as usize);
        let start = index_at as usize + submesh.index_start as usize * 2;
        for j in 0..submesh.indices_to_draw as usize {
            match u16_at(d, start + j * 2) {
                Some(v) => original.push(v),
                None => break,
            }
        }

        let mut remap: std::collections::HashMap<u16, u16> = std::collections::HashMap::new();
        let mut source_of: Vec<u16> = Vec::new();
        let mut indices: Vec<u32> = Vec::with_capacity(original.len());
        for index in original {
            let next = remap.entry(index).or_insert_with(|| {
                source_of.push(index);
                (source_of.len() - 1) as u16
            });
            indices.push(*next as u32);
        }

        let mut mesh = Mesh {
            material: submesh.material as usize,
            lod: submesh.lod,
            indices,
            ..Mesh::default()
        };

        // Read one vertex at a time: each element is at the element's own
        // offset within the vertex, and the vertex advances by the stride.
        let wanted = submesh.vertices_to_draw as usize;
        let vertex_base = submesh.vertex_start as usize;
        for k in 0..wanted {
            let source = match source_of.get(k) {
                Some(s) => vertex_base + *s as usize,
                None => vertex_base + k,
            };
            let mut position = [0.0f32; 3];
            let mut normal = [0.0f32; 3];
            let mut uv = [0.0f32; 2];
            for element in elements {
                let Some(stream) = streams[buffer_index].get(element.stream as usize) else {
                    continue;
                };
                let Some(bytes) = stream else { continue };
                let stride = buffer.stride(element.stream as usize);
                let offset = if buffer.separate_asset() {
                    // A separate asset starts at its own zero.
                    source * stride + element.offset
                } else {
                    let base = if element.stream == 0 {
                        buffer.stream1
                    } else {
                        buffer.stream2
                    } as usize;
                    base + source * stride + element.offset
                };
                let Some(values) = element.format.read(bytes, offset) else {
                    continue;
                };
                match element.semantic {
                    Semantic::Position if element.index == 0 => {
                        position = [values[0], values[1], values[2]];
                    }
                    Semantic::Normal if element.index == 0 => {
                        normal = [values[0], values[1], values[2]];
                    }
                    Semantic::TexCoord if element.index == 0 => {
                        uv = [values[0], values[1]];
                    }
                    _ => {}
                }
            }
            mesh.positions.push(position);
            mesh.normals.push(normal);
            mesh.uvs.push(uv);
        }

        if !mesh.indices.is_empty() && !mesh.positions.is_empty() {
            meshes.push(mesh);
        }
    }

    Ok(meshes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halves_convert() {
        assert_eq!(half_to_f32(0x0000), 0.0);
        assert_eq!(half_to_f32(0x3C00), 1.0);
        assert_eq!(half_to_f32(0xBC00), -1.0);
        assert_eq!(half_to_f32(0x4000), 2.0);
        assert_eq!(half_to_f32(0x3555), 0.333_251_95);
    }

    #[test]
    fn formats_know_their_size() {
        // Getting this wrong shifts every vertex after the first, so the
        // component counts are worth pinning.
        assert_eq!(Format::from_byte(0x0).components(), 1);
        assert_eq!(Format::from_byte(0x2).components(), 3);
        assert_eq!(Format::from_byte(0x3).components(), 4);
        assert_eq!(Format::from_byte(0x4).components(), 2);
        assert_eq!(Format::from_byte(0x9).components(), 4);
    }

    #[test]
    fn unorm_bytes_scale_to_unit() {
        let data = [0u8, 128, 255, 64];
        let value = Format::U8Unorm4.read(&data, 0).unwrap();
        assert_eq!(value[0], 0.0);
        assert!((value[1] - 128.0 / 255.0).abs() < 1e-6);
        assert_eq!(value[2], 1.0);
    }
}

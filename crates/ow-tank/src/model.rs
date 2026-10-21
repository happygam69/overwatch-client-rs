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
    /// Every texcoord set the submesh's vertex buffer declares, in semantic
    /// index order: `uv_sets[set][vertex]`. The reference keeps the same shape
    /// (`Submesh.UV[vertex][set]`, sized by the largest texcoord index the
    /// buffer carries). Empty when the buffer declares no texcoord at all, so
    /// an old single-set reader can ask [`Mesh::uv_sets`]`.first()`.
    pub uv_sets: Vec<Vec<[f32; 2]>>,
    /// Vertex colours: semantic 8, index 0 (`Color1` in the reference) and
    /// index 1 (`Color2`). Values are normalised to 0..=1 by the element
    /// format (UNORM8/UNORM16 are divided, FLOAT is stored as-is), so a reader
    /// can use them directly. Empty when the buffer declares no colour
    /// element, and one inner vector per semantic index otherwise, each as
    /// long as the vertex array.
    pub colors: Vec<Vec<[f32; 4]>>,
    /// Semantic 3: a packed tangent (`U8Snorm4`), as the bytes store it.
    pub tangents: Vec<[f32; 4]>,
    /// Semantic 4: **four bone indices**, as u16s. Which enum is which was
    /// *measured*, not assumed: the weights are the semantics-5 bytes below,
    /// because those sum to exactly 255 on every vertex of Tracer's character
    /// mesh (`ow-tank-model --hero 3 --skin`), while these repeat in runs across
    /// a limb — which is what a per-region bone set looks like.
    pub bone_indices: Vec<[u16; 4]>,
    /// Semantic 5: four blend weights as bytes, summing to 255.
    pub blend_weights: Vec<[u8; 4]>,
    /// Whether this submesh declares semantics 4 and 5 at all. **Most do not**:
    /// measured on Tracer's character model, 3 of its 9 submeshes carry them,
    /// and the other 6 (the head, the hair, the body shell) have no skinning
    /// data — so a reader that takes "the weights array" as authoritative zeroes
    /// them and the shader collapses those submeshes onto the origin. That is
    /// exactly what happened; this flag is what makes it visible.
    pub skinned: bool,
    pub indices: Vec<u32>,
    /// Index into [`Model::materials`].
    pub material: usize,
    /// Level of detail; `-1` means the submesh is used at every level.
    pub lod: i8,
    /// The submesh flags (descriptor offset 108). The reference's
    /// `SubmeshFlags` is a `ushort`, but every bit it names lives in the low
    /// byte, which is what is kept here:
    ///
    /// * `0x01` — unknown ("Unk1": the reference comments "?? nothing").
    /// * `0x02` — `NonStatic`: "moveable or animated".
    /// * `0x04` — `Opaque`.
    /// * `0x08` — unknown (the reference asks whether it means "everything
    ///   that isn't MinDetail?").
    /// * `0x10` — `MinDetail`: "mesh with really low detail".
    /// * `0x20` — unknown (the reference comments "same as NonStatic (-cloth
    ///   physics?)").
    /// * `0x40` — `Vegetation`.
    /// * `0x80` — unknown.
    ///
    /// The names are the reference's, not confirmed against client behaviour.
    pub flags: u8,
    /// The descriptor's seven *extra* index starts (offsets 68..96), as
    /// stored. `index_start` is the first; a submesh may list further index
    /// runs a streamed level uses. Nothing reads them yet.
    pub other_index_starts: [u32; 7],
    /// The seven extra index-buffer indices (offsets 111..118), paired with
    /// [`Mesh::other_index_starts`]. Nothing reads them yet.
    pub other_index_buffers: [u8; 7],
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
    /// The `MSTU` chunk's header -- where the model's own structured data
    /// sits inside this asset and how long it is. `None` when the model has
    /// no structured-data chunk. The blob itself is parsed by
    /// [`crate::mstu::parse`] (the header's offset is relative to the chunk
    /// payload, so pass the asset).
    pub mstu: Option<crate::mstu::MstuHeader>,
}

impl Model {
    pub fn vertex_count(&self) -> usize {
        self.meshes.iter().map(Mesh::vertex_count).sum()
    }

    pub fn triangle_count(&self) -> usize {
        self.meshes.iter().map(Mesh::triangle_count).sum()
    }

    /// The submeshes one viewer should draw: the finest level of detail present,
    /// plus the `-1` submeshes that belong to every level.
    ///
    /// The levels are not guaranteed to include a zero — the hero's character
    /// model carries `1, 2, 4, 8` and no `0`, so a `lod <= 0` filter draws
    /// *every* level at once (measured: 34821 vertices and 50145 triangles for a
    /// 200-bone character, four copies of her on top of each other).
    pub fn finest_level(&self) -> Vec<&Mesh> {
        let finest = self
            .meshes
            .iter()
            .filter(|m| m.lod >= 0)
            .map(|m| m.lod)
            .min();
        match finest {
            Some(level) => self
                .meshes
                .iter()
                .filter(|m| m.lod < 0 || m.lod == level)
                .collect(),
            None => self.meshes.iter().collect(),
        }
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
pub(crate) fn half_to_f32(h: u16) -> f32 {
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
    /// `0xA`: four u16, each ÷ 65535. **Inferred** from the DXGI ordering the
    /// other bytes follow (`0x8`/`0x9` are the UNORM/SNORM byte formats, so
    /// the next pair is the 16-bit one); `docs/formats/*` has no confirmed
    /// sample. Added because vertex colours can be stored this way.
    U16Unorm4,
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
            0xA => Format::U16Unorm4,
            0xC => Format::U32,
            other => Format::Unknown(other),
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn components(self) -> usize {
        match self {
            Format::F32 | Format::U32 => 1,
            Format::F32x2 | Format::F16x2 => 2,
            Format::F32x3 => 3,
            Format::F32x4 | Format::F16x4 | Format::U8x4 | Format::U16x4 => 4,
            Format::U8Unorm4 | Format::U8Snorm4 | Format::U16Unorm4 => 4,
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
            Format::U16Unorm4 => {
                for i in 0..4 {
                    out[i] = u16_at(d, at + i * 2)? as f32 / 65535.0;
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
    /// Named by number where the reference tooling has a name for it: 3 is the
    /// tangent, 4 the blend weights, 5 the blend indices, 16 a per-vertex u32.
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
        let Some(semantic) = u8_at(d, base) else {
            break;
        };
        let Some(index) = u8_at(d, base + 1) else {
            break;
        };
        let Some(format) = u8_at(d, base + 2) else {
            break;
        };
        let Some(stream) = u8_at(d, base + 3) else {
            break;
        };
        // The packing is not documented: the low nibble is the slot class, the
        // next four bits the step rate, and the offset sits above those.
        let Some(packed) = u32_at(d, base + 4) else {
            break;
        };
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
    /// Kept because it is what a reader needs to size a stream; the stride is
    /// what this program walks with.
    #[allow(dead_code)]
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
        let raw = if which == 0 {
            self.stream1
        } else {
            self.stream2
        };
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
    flags: u8,
    other_index_starts: [u32; 7],
    other_index_buffers: [u8; 7],
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

    // The model's own structured data. A model without it is not an error;
    // the caller that needs the STU asks for it explicitly.
    if chunks.payload(asset, crate::mstu::TAG).is_some() {
        if let Ok(header) = crate::mstu::parse_header(asset) {
            model.mstu = Some(header);
        }
    }

    let mrnm = chunks
        .payload(asset, b"MRNM")
        .ok_or(Error::MissingChunk("MRNM"))?;
    model.meshes = read_render_mesh(mrnm, load)?;
    Ok(model)
}

/// The largest semantic index + 1 the elements carry for `semantic`, which is
/// how many arrays the reference allocates for it; 0 when none is declared.
fn max_semantic_index(elements: &[Element], semantic: Semantic) -> usize {
    elements
        .iter()
        .filter(|e| e.semantic == semantic)
        .map(|e| e.index as usize + 1)
        .max()
        .unwrap_or(0)
}

fn read_render_mesh(d: &[u8], load: &dyn Fn(u64) -> Option<Vec<u8>>) -> Result<Vec<Mesh>> {
    let vbo_ptr = i64_at(d, 0).ok_or(Error::Malformed("truncated MRNM header"))?;
    let ibo_ptr = i64_at(d, 8).ok_or(Error::Malformed("truncated MRNM header"))?;
    let submesh_ptr = i64_at(d, 16).ok_or(Error::Malformed("truncated MRNM header"))?;
    let vbo_count = u8_at(d, 34).ok_or(Error::Malformed("truncated MRNM header"))? as usize;
    let ibo_count = u8_at(d, 35).ok_or(Error::Malformed("truncated MRNM header"))? as usize;
    let submesh_count = u32_at(d, 28).ok_or(Error::Malformed("truncated MRNM header"))? as usize;
    // Bound the allocation by the bytes available: each descriptor is 128 bytes.
    let submesh_cap = d.len() / 128 + 1;
    if submesh_count > submesh_cap {
        return Err(Error::Malformed("submesh count exceeds chunk"));
    }

    let mut buffers = Vec::with_capacity(vbo_count);
    for i in 0..vbo_count {
        let base =
            usize::try_from(vbo_ptr).map_err(|_| Error::Malformed("negative MRNM offset"))?;
        let at = base
            .checked_add(
                i.checked_mul(96)
                    .ok_or(Error::Malformed("vertex buffer runs past the end"))?,
            )
            .ok_or(Error::Malformed("vertex buffer runs past the end"))?;
        let element_offset_raw =
            i64_at(d, at + 16).ok_or(Error::Malformed("truncated vertex buffer"))?;
        let element_offset = usize::try_from(element_offset_raw)
            .map_err(|_| Error::Malformed("negative MRNM offset"))?;
        buffers.push(VertexBuffer {
            vertex_count: u32_at(d, at).ok_or(Error::Malformed("truncated vertex buffer"))?,
            stride1: u8_at(d, at + 8).ok_or(Error::Malformed("truncated vertex buffer"))?,
            stride2: u8_at(d, at + 9).ok_or(Error::Malformed("truncated vertex buffer"))?,
            element_count: u8_at(d, at + 10).ok_or(Error::Malformed("truncated vertex buffer"))?,
            flags: u8_at(d, at + 11).ok_or(Error::Malformed("truncated vertex buffer"))?,
            element_offset,
            stream1: i64_at(d, at + 80).ok_or(Error::Malformed("truncated vertex buffer"))?,
            stream2: i64_at(d, at + 88).ok_or(Error::Malformed("truncated vertex buffer"))?,
        });
    }

    let mut index_buffers = Vec::with_capacity(ibo_count);
    for i in 0..ibo_count {
        let base =
            usize::try_from(ibo_ptr).map_err(|_| Error::Malformed("negative MRNM offset"))?;
        let at = base
            .checked_add(
                i.checked_mul(16)
                    .ok_or(Error::Malformed("index buffer runs past the end"))?,
            )
            .ok_or(Error::Malformed("index buffer runs past the end"))?;
        index_buffers.push((
            u32_at(d, at).ok_or(Error::Malformed("truncated index buffer"))?,
            u32_at(d, at + 4).ok_or(Error::Malformed("truncated index buffer"))?,
            i64_at(d, at + 8).ok_or(Error::Malformed("truncated index buffer"))?,
        ));
    }

    let mut submeshes = Vec::with_capacity(submesh_count);
    for i in 0..submesh_count {
        let base =
            usize::try_from(submesh_ptr).map_err(|_| Error::Malformed("negative MRNM offset"))?;
        let at = base
            .checked_add(
                i.checked_mul(128)
                    .ok_or(Error::Malformed("submesh runs past the end"))?,
            )
            .ok_or(Error::Malformed("submesh runs past the end"))?;
        let mut other_index_starts = [0u32; 7];
        for k in 0..7 {
            other_index_starts[k] =
                u32_at(d, at + 68 + k * 4).ok_or(Error::Malformed("truncated submesh"))?;
        }
        let mut other_index_buffers = [0u8; 7];
        for k in 0..7 {
            other_index_buffers[k] =
                u8_at(d, at + 111 + k).ok_or(Error::Malformed("truncated submesh"))?;
        }
        submeshes.push(Submesh {
            vertex_start: u32_at(d, at + 60).ok_or(Error::Malformed("truncated submesh"))?,
            index_start: u32_at(d, at + 64).ok_or(Error::Malformed("truncated submesh"))?,
            // The reference's struct places Flags (a ushort) at 108, between
            // BoneIdOffset (106) and IndexBuffer (110); every field around it
            // matches the offsets this parser already reads, so that is the
            // offset used. Only the low byte is kept (see `Mesh::flags`).
            flags: u16_at(d, at + 108).ok_or(Error::Malformed("truncated submesh"))? as u8,
            other_index_starts,
            indices_to_draw: u32_at(d, at + 100).ok_or(Error::Malformed("truncated submesh"))?,
            vertices_to_draw: u16_at(d, at + 104).ok_or(Error::Malformed("truncated submesh"))?
                as u32,
            index_buffer: u8_at(d, at + 110).ok_or(Error::Malformed("truncated submesh"))?,
            other_index_buffers,
            vertex_buffer: u8_at(d, at + 118).ok_or(Error::Malformed("truncated submesh"))?,
            material: u8_at(d, at + 119).ok_or(Error::Malformed("truncated submesh"))?,
            lod: u8_at(d, at + 120).ok_or(Error::Malformed("truncated submesh"))? as i8,
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
                let offset = if which == 0 {
                    buffer.stream1
                } else {
                    buffer.stream2
                };
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
        let index_base =
            usize::try_from(index_at).map_err(|_| Error::Malformed("negative MRNM offset"))?;
        let start = index_base
            .checked_add(
                (submesh.index_start as usize)
                    .checked_mul(2)
                    .ok_or(Error::Malformed("index run runs past the end"))?,
            )
            .ok_or(Error::Malformed("index run runs past the end"))?;
        for j in 0..submesh.indices_to_draw as usize {
            let at = start
                .checked_add(
                    j.checked_mul(2)
                        .ok_or(Error::Malformed("index run runs past the end"))?,
                )
                .ok_or(Error::Malformed("index run runs past the end"))?;
            match u16_at(d, at) {
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
            flags: submesh.flags,
            other_index_starts: submesh.other_index_starts,
            other_index_buffers: submesh.other_index_buffers,
            indices,
            skinned: elements.iter().any(|e| e.semantic == Semantic::Other(4))
                && elements.iter().any(|e| e.semantic == Semantic::Other(5)),
            ..Mesh::default()
        };
        // One set per declared semantic index, sized like the reference sizes
        // it (the largest index the vertex buffer's elements carry, plus one);
        // zero sets when the buffer declares none.
        let uv_set_count = max_semantic_index(elements, Semantic::TexCoord);
        let color_set_count = max_semantic_index(elements, Semantic::Colour);
        let wanted = submesh.vertices_to_draw as usize;
        mesh.uv_sets = vec![Vec::with_capacity(wanted); uv_set_count];
        mesh.colors = vec![Vec::with_capacity(wanted); color_set_count];

        // Read one vertex at a time: each element is at the element's own
        // offset within the vertex, and the vertex advances by the stride.
        let vertex_base = submesh.vertex_start as usize;
        for k in 0..wanted {
            let source = match source_of.get(k) {
                Some(s) => {
                    let Some(v) = vertex_base.checked_add(*s as usize) else {
                        continue;
                    };
                    v
                }
                None => {
                    let Some(v) = vertex_base.checked_add(k) else {
                        continue;
                    };
                    v
                }
            };
            let mut position = [0.0f32; 3];
            let mut normal = [0.0f32; 3];
            let mut uvs = vec![[0.0f32; 2]; uv_set_count];
            let mut colors = vec![[0.0f32; 4]; color_set_count];
            let mut tangent = [0.0f32; 4];
            let mut joints = [0u16; 4];
            let mut weights = [0u8; 4];
            for element in elements {
                let Some(stream) = streams[buffer_index].get(element.stream as usize) else {
                    continue;
                };
                let Some(bytes) = stream else { continue };
                let stride = buffer.stride(element.stream as usize);
                let vertex_off = source
                    .checked_mul(stride)
                    .and_then(|v| v.checked_add(element.offset));
                let offset = if buffer.separate_asset() {
                    // A separate asset starts at its own zero.
                    let Some(o) = vertex_off else { continue };
                    o
                } else {
                    let raw = if element.stream == 0 {
                        buffer.stream1
                    } else {
                        buffer.stream2
                    };
                    let Ok(base) = usize::try_from(raw) else {
                        continue;
                    };
                    let Some(vo) = vertex_off else { continue };
                    let Some(o) = base.checked_add(vo) else {
                        continue;
                    };
                    o
                };
                // The skin attributes are read raw: their descriptors say
                // "unorm" and "u8x4", but the values are indices and weights,
                // and scaling them by 255 would be wrong.
                match element.semantic {
                    Semantic::Other(4) if element.index == 0 => {
                        for (i, j) in joints.iter_mut().enumerate() {
                            *j = u16_at(bytes, offset + i * 2).unwrap_or(0);
                        }
                        continue;
                    }
                    Semantic::Other(5) if element.index == 0 => {
                        for (i, w) in weights.iter_mut().enumerate() {
                            *w = u8_at(bytes, offset + i).unwrap_or(0);
                        }
                        continue;
                    }
                    _ => {}
                }
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
                    Semantic::TexCoord => {
                        if let Some(slot) = uvs.get_mut(element.index as usize) {
                            *slot = [values[0], values[1]];
                        }
                    }
                    Semantic::Colour => {
                        if let Some(slot) = colors.get_mut(element.index as usize) {
                            *slot = values;
                        }
                    }
                    Semantic::Other(3) if element.index == 0 => {
                        tangent = values;
                    }
                    _ => {}
                }
            }
            mesh.positions.push(position);
            mesh.normals.push(normal);
            for (set, uv) in uvs.iter().enumerate() {
                mesh.uv_sets[set].push(*uv);
            }
            for (set, color) in colors.iter().enumerate() {
                mesh.colors[set].push(*color);
            }
            mesh.tangents.push(tangent);
            mesh.bone_indices.push(joints);
            mesh.blend_weights.push(weights);
        }

        if !mesh.indices.is_empty() && !mesh.positions.is_empty() {
            meshes.push(mesh);
        }
    }

    Ok(meshes)
}

/// Every vertex attribute a model's submeshes declare, as
/// `(semantic byte, index, format byte)` with a count of how many submesh
/// element descriptors carry it.
///
/// The parser decodes position (0), normal (1), colour (8) and texcoord (9);
/// a skinned mesh must also carry bone indices and weights under *some*
/// semantic byte, and this is how to find out which without guessing.
pub fn vertex_semantics(
    asset: &[u8],
    _load: &dyn Fn(u64) -> Option<Vec<u8>>,
) -> Result<Vec<(u8, u8, u8, usize)>> {
    let chunks = Chunks::parse(asset).map_err(|_| Error::NotAChunkedAsset)?;
    let d = chunks
        .payload(asset, b"MRNM")
        .ok_or(Error::MissingChunk("MRNM"))?;
    // The elements belong to the *vertex buffer* descriptors (record stride
    // 96, element count at +10, element array at +16), not to the submeshes.
    let vbo_ptr = i64_at(d, 0).ok_or(Error::Malformed("truncated MRNM header"))? as usize;
    let vbo_count = u8_at(d, 34).ok_or(Error::Malformed("truncated MRNM header"))? as usize;
    let mut seen: Vec<(u8, u8, u8, usize)> = Vec::new();
    for i in 0..vbo_count {
        let at = vbo_ptr + i * 96;
        let element_count = u8_at(d, at + 10).unwrap_or(0) as usize;
        let element_offset = i64_at(d, at + 16).unwrap_or(0).max(0) as usize;
        if element_count == 0 || element_offset == 0 {
            continue;
        }
        for e in read_elements(d, element_offset, element_count) {
            let semantic = match e.semantic {
                Semantic::Position => 0u8,
                Semantic::Normal => 1,
                Semantic::TexCoord => 9,
                Semantic::Colour => 8,
                Semantic::Other(b) => b,
            };
            let format = match e.format {
                Format::F32 => 0x0,
                Format::F32x2 => 0x1,
                Format::F32x3 => 0x2,
                Format::F32x4 => 0x3,
                Format::F16x2 => 0x4,
                Format::F16x4 => 0x5,
                Format::U8x4 => 0x6,
                Format::U16x4 => 0x7,
                Format::U8Unorm4 => 0x8,
                Format::U8Snorm4 => 0x9,
                Format::U16Unorm4 => 0xa,
                Format::U32 => 0xc,
                Format::Unknown(b) => b | 0x80,
            };
            match seen
                .iter_mut()
                .find(|s| s.0 == semantic && s.1 == e.index && s.2 == format)
            {
                Some(s) => s.3 += 1,
                None => seen.push((semantic, e.index, format, 1)),
            }
        }
    }
    seen.sort_by_key(|s| (s.0, s.1, s.2));
    Ok(seen)
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
        assert_eq!(Format::from_byte(0xA).components(), 4);
    }

    #[test]
    fn unorm_bytes_scale_to_unit() {
        let data = [0u8, 128, 255, 64];
        let value = Format::U8Unorm4.read(&data, 0).unwrap();
        assert_eq!(value[0], 0.0);
        assert!((value[1] - 128.0 / 255.0).abs() < 1e-6);
        assert_eq!(value[2], 1.0);
    }

    /// A synthetic `MRNM` chunk holding one submesh with **two texcoord sets,
    /// one colour element and non-zero submesh flags**, laid out at explicit
    /// offsets so the test fails if any of the three is read from the wrong
    /// place. No game bytes: every value is written here.
    fn synth_mrnm(with_uvs_and_colors: bool) -> Vec<u8> {
        // Layout, byte-exact:
        //   0..36   header
        //   64..160 vertex buffer descriptor
        //   160..192 element descriptors (4)
        //   192..208 index buffer descriptor
        //   208..336 submesh descriptor
        //   336..342 indices
        //   344..416 vertex bytes (3 × 24)
        const VBO: usize = 64;
        const ELEMENTS: usize = 160;
        const IBO: usize = 192;
        const SUBMESH: usize = 208;
        const INDICES: usize = 336;
        const VERTICES: usize = 344;
        let mut d = vec![0u8; 416];
        let wide =
            |d: &mut Vec<u8>, at: usize, v: u32| d[at..at + 4].copy_from_slice(&v.to_le_bytes());
        let half =
            |d: &mut Vec<u8>, at: usize, v: u16| d[at..at + 2].copy_from_slice(&v.to_le_bytes());
        let float =
            |d: &mut Vec<u8>, at: usize, v: f32| d[at..at + 4].copy_from_slice(&v.to_le_bytes());
        // header
        d[0..8].copy_from_slice(&(VBO as i64).to_le_bytes());
        d[8..16].copy_from_slice(&(IBO as i64).to_le_bytes());
        d[16..24].copy_from_slice(&(SUBMESH as i64).to_le_bytes());
        wide(&mut d, 24, 3); // total vertex count (unused)
        wide(&mut d, 28, 1); // submesh count
        d[34] = 1; // vertex buffer count
        d[35] = 1; // index buffer count
                   // vertex buffer descriptor
        wide(&mut d, VBO, 3); // vertex count
        d[VBO + 8] = 24; // stride, stream 1
        d[VBO + 9] = 0;
        d[VBO + 10] = if with_uvs_and_colors { 4 } else { 2 }; // element count
        d[VBO + 11] = 0; // not a separate asset
        d[VBO + 16..VBO + 24].copy_from_slice(&(ELEMENTS as i64).to_le_bytes());
        d[VBO + 80..VBO + 88].copy_from_slice(&(VERTICES as i64).to_le_bytes());
        // elements: (semantic, index, format, offset-in-vertex)
        let mut elements: Vec<(u8, u8, u8, u32)> = vec![(0, 0, 0x2, 0)]; // position F32x3
        if with_uvs_and_colors {
            elements.push((9, 0, 0x4, 12)); // texcoord 0, F16x2
            elements.push((9, 1, 0x4, 16)); // texcoord 1, F16x2
            elements.push((8, 0, 0x8, 20)); // colour 0, U8Unorm4
        } else {
            elements.push((1, 0, 0x2, 12)); // normal F32x3 instead
        }
        for (i, (semantic, index, format, offset)) in elements.iter().enumerate() {
            let at = ELEMENTS + i * 8;
            d[at] = *semantic;
            d[at + 1] = *index;
            d[at + 2] = *format;
            d[at + 3] = 0; // stream
            wide(&mut d, at + 4, offset << 12);
        }
        // index buffer descriptor: count, format, data pointer
        wide(&mut d, IBO, 3);
        wide(&mut d, IBO + 4, 0);
        d[IBO + 8..IBO + 16].copy_from_slice(&(INDICES as i64).to_le_bytes());
        // submesh descriptor
        wide(&mut d, SUBMESH + 60, 0); // vertex start
        wide(&mut d, SUBMESH + 64, 0); // index start
        for k in 0..7 {
            wide(&mut d, SUBMESH + 68 + k * 4, 100 + k as u32); // other index starts
            d[SUBMESH + 111 + k] = 200 + k as u8; // other index buffers
        }
        wide(&mut d, SUBMESH + 100, 3); // indices to draw
        half(&mut d, SUBMESH + 104, 3); // vertices to draw
        half(&mut d, SUBMESH + 108, 0x0044); // flags (ushort; low byte kept)
        d[SUBMESH + 110] = 0; // index buffer
        d[SUBMESH + 118] = 0; // vertex buffer
        d[SUBMESH + 119] = 0; // material
        d[SUBMESH + 120] = 0xFF; // lod -1
                                 // indices
        for (i, v) in [0u16, 1, 2].iter().enumerate() {
            half(&mut d, INDICES + i * 2, *v);
        }
        // vertices
        for (v, (x, y, z)) in [(1.0f32, 2.0f32, 3.0f32), (4.0, 5.0, 6.0), (7.0, 8.0, 9.0)]
            .iter()
            .enumerate()
        {
            let at = VERTICES + v * 24;
            float(&mut d, at, *x);
            float(&mut d, at + 4, *y);
            float(&mut d, at + 8, *z);
            if with_uvs_and_colors {
                // uv0, uv1 as f16; colour as u8x4.
                let uv0 = [[0x3400u16, 0x3800], [0x3C00, 0x0000], [0x0000, 0x3C00]][v];
                let uv1 = [[0x3A00u16, 0x3C00], [0x3800, 0x3400], [0x3400, 0x3A00]][v];
                half(&mut d, at + 12, uv0[0]);
                half(&mut d, at + 14, uv0[1]);
                half(&mut d, at + 16, uv1[0]);
                half(&mut d, at + 18, uv1[1]);
                let color = [[255u8, 128, 0, 64], [0, 64, 128, 255], [10, 20, 30, 40]][v];
                d[at + 20..at + 24].copy_from_slice(&color);
            } else {
                float(&mut d, at + 12, 0.0);
                float(&mut d, at + 16, 1.0);
                float(&mut d, at + 20, 0.0); // normal = (0, 1, 0)
            }
        }
        d
    }

    #[test]
    fn two_uv_sets_colors_and_flags_land_at_their_offsets() {
        let d = synth_mrnm(true);
        let meshes = read_render_mesh(&d, &|_| None).expect("synthetic MRNM parses");
        assert_eq!(meshes.len(), 1, "the one submesh should survive");
        let m = &meshes[0];
        assert_eq!(m.vertex_count(), 3);
        assert_eq!(m.indices, vec![0, 1, 2]);
        assert_eq!(m.lod, -1);

        // Both texcoord sets, in semantic-index order, at their own offsets.
        assert_eq!(m.uv_sets.len(), 2, "two declared texcoord sets");
        assert_eq!(m.uv_sets[0][0], [0.25, 0.5]);
        assert_eq!(m.uv_sets[1][0], [0.75, 1.0]);
        assert_eq!(m.uv_sets[0][1], [1.0, 0.0]);
        assert_eq!(m.uv_sets[1][1], [0.5, 0.25]);
        assert_eq!(m.uv_sets[0][2], [0.0, 1.0]);
        assert_eq!(m.uv_sets[1][2], [0.25, 0.75]);

        // One colour set, UNORM8 normalised to 0..=1.
        assert_eq!(m.colors.len(), 1);
        assert_eq!(m.colors[0][0][0], 1.0);
        assert!((m.colors[0][0][1] - 128.0 / 255.0).abs() < 1e-6);
        assert_eq!(m.colors[0][0][2], 0.0);
        assert!((m.colors[0][0][3] - 64.0 / 255.0).abs() < 1e-6);
        assert_eq!(m.colors[0][1], [0.0, 64.0 / 255.0, 128.0 / 255.0, 1.0]);
        assert_eq!(
            m.colors[0][2],
            [10.0 / 255.0, 20.0 / 255.0, 30.0 / 255.0, 40.0 / 255.0]
        );

        // Flags: the ushort at 108, low byte kept.
        assert_eq!(m.flags, 0x44);
        // The extra index starts/buffers the old parser dropped.
        assert_eq!(m.other_index_starts, [100, 101, 102, 103, 104, 105, 106]);
        assert_eq!(m.other_index_buffers, [200, 201, 202, 203, 204, 205, 206]);

        // Internal consistency a caller relies on.
        for set in &m.uv_sets {
            assert_eq!(set.len(), m.vertex_count());
        }
        for set in &m.colors {
            assert_eq!(set.len(), m.vertex_count());
        }
        assert_eq!(m.positions[1], [4.0, 5.0, 6.0]);
    }

    #[test]
    fn a_mesh_without_uvs_or_colors_reports_empty_sets() {
        let d = synth_mrnm(false);
        let meshes = read_render_mesh(&d, &|_| None).expect("synthetic MRNM parses");
        let m = &meshes[0];
        assert!(m.uv_sets.is_empty(), "no texcoord element => no sets");
        assert!(m.colors.is_empty(), "no colour element => no colours");
        // The old single-set behaviour survives through `uv_sets.first()`.
        assert!(m.uv_sets.is_empty());
        assert_eq!(m.normals[0], [0.0, 1.0, 0.0]);
    }
}

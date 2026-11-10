//! Models: the render mesh a map placeable or a character points at.
//!
//! A model asset is chunked data. Three chunks matter here:
//!
//! * `MDLC` -- the material list and the model's bound (`MdlcMaterialRead`,
//!   `0x141b88cb0`).
//! * `MRNM` -- the geometry, as vertex buffers, index buffers and submeshes.
//! * `MSTU` -- a header pointing at the model's own structured data.
//!
//! What is **read and verified** is the container and descriptor layer:
//! `MRNM`'s three pointer slots, its counts, the `0x60`-byte vertex-buffer,
//! `0x10`-byte index-buffer and `0x80`-byte submesh descriptors, and the
//! runtime block the client builds from them ([`crate::mrnm`], three
//! differential units against `0x141b893e0`, `0x141b89390` and `0x141b88f10`).
//! The chunk versions are the client's own gate (`0x141b5e1e0`,
//! `0x141b5df70`): `MDLC` `0x165`, `MRNM` `0x1f`, `MSTU` `2`.
//!
//! The **element record is read from the client**: its own render-mesh writer
//! (`0x14210c450`) emits `{semantic, index, format, stream}` plus a packed dword
//! built as `stride << 22 | offset << 12`, and the D3D11 input-layout consumer
//! (`0x142667be0`) reads that form. The record layout, the offset bits and the
//! stride bits are therefore read, not inferred; see [`vertex_elements`] and
//! `docs/formats/model.md`.
//!
//! What is **not read** is the value decode and the mesh assembly. The client
//! never converts vertex bytes on the CPU -- there is no half-float conversion
//! instruction anywhere in the image -- it hands an *engine* format byte to
//! D3D11 after the table at `0x1434965d0`. The floats [`Mesh`] carries are
//! therefore this viewer's reading of the file, and the compacted vertex array
//! is built here rather than by the client. What joins a model's file-side
//! format byte (`0x00..0x13`) to that engine enum is **open**.
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
    ///
    /// The indices are **mesh slots, not bones**: they go through the
    /// skeleton's Lookup (`Skin::bone_of`). **Only the first four-wide set is
    /// read.** A vertex buffer may declare a second `semantic 4 / index 1`
    /// element (454 model assets in the install do, always format `0x07`), and
    /// on those models the eight weights still sum to exactly 255 and the
    /// second set's indices are inside the Lookup (both measured on all 454) —
    /// so a vertex drawing on the second set loses that share here (`43/255`
    /// was the largest second-set weight seen). That
    /// is an open gap, recorded in `docs/ow-tank-1to1.md`, not a reading of the
    /// file: the element list itself declares both sets.
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
    /// The submesh's `BoneIdOffset`: payload descriptor `+0x6a`, copied by
    /// `MrnmHandler` to runtime submesh `+0x82` (the 0xa0-stride record).
    ///
    /// The reference adds it to a vertex's raw blend index before the
    /// skeleton's Lookup (`teModelChunk_RenderMesh`: `thisIndices[m] +
    /// submeshDescriptor.BoneIdOffset`). **That addition is the reference's,
    /// not the client's**: the client's copy of the field is read, but no
    /// client code that adds it to a mesh index has been found yet, so this
    /// module records the value and does not apply it
    /// (`docs/formats/animation.md`). It is also **unobservable on this
    /// install**: every submesh of every one of the 7,328 skinned models
    /// carries `0`, so adding it and not adding it produce the same indices.
    pub bone_id_offset: u16,
    pub indices: Vec<u32>,
    /// Index into [`Model::materials`].
    pub material: usize,
    /// The LOD byte (payload `+0x78`, copied to runtime `+0x92` by
    /// `MrnmHandler`): **a set of levels, one bit per level**, not an ordinal.
    ///
    /// `0` and `0x7f` are not levels: the client's union (`0x141b40be0` at
    /// `0x141b40c62`) skips exactly those two values. A byte of `0xFF` (`-1`)
    /// sets every bit, which is why such a submesh is drawn at every level —
    /// the client does not special-case it.
    ///
    /// Measured on the install: every value seen is `1`, `2`, `4` or `8`, one
    /// bit each (a hero's body carries 9 submeshes at `1`, 8 at `2`, 6 at `4`
    /// and 6 at `8`).
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

/// The model's own bound, as `MDLC` stores it.
///
/// `MdlcMaterialRead` (`0x141b88cb0`) copies four `vec4` rows from the chunk:
/// `+0x10` the box minimum, `+0x20` the box maximum, `+0x30` a sphere centre
/// and `+0x40` that sphere's radius. The twelve bytes at `+0x44..+0x50` (the
/// sphere row's remaining lanes) are **never read** by the client, so the block
/// is thirteen floats, not the 4x4 matrix an earlier reader took it for.
///
/// Measured against the decoded vertices (`tests/model_attrs_real.rs`): `min`
/// and `max` reproduce the mesh extents (worst gap 0.0143 m across 68 models /
/// 390 submeshes), and the sphere contains every vertex of every level while
/// being smaller than the box's half-diagonal — so `radius` bounds the mesh,
/// not the box corners.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
    /// The sphere's centre (`+0x30`); not the box centre.
    pub centre: [f32; 3],
    /// The sphere's radius (`+0x40`).
    pub radius: f32,
}

impl Bounds {
    /// The four rows, or `None` when the chunk is too short to hold them.
    fn read(d: &[u8]) -> Option<Bounds> {
        let f = |at: usize| f32_at(d, at);
        Some(Bounds {
            min: [f(0x10)?, f(0x14)?, f(0x18)?],
            max: [f(0x20)?, f(0x24)?, f(0x28)?],
            centre: [f(0x30)?, f(0x34)?, f(0x38)?],
            radius: f(0x40)?,
        })
    }

    pub fn size(&self) -> [f32; 3] {
        [
            self.max[0] - self.min[0],
            self.max[1] - self.min[1],
            self.max[2] - self.min[2],
        ]
    }

    /// Whether the box holds `p`.
    pub fn contains(&self, p: [f32; 3]) -> bool {
        (0..3).all(|i| p[i] >= self.min[i] && p[i] <= self.max[i])
    }

    /// The distance from the sphere's centre to `p`.
    pub fn sphere_distance(&self, p: [f32; 3]) -> f32 {
        let d = [
            p[0] - self.centre[0],
            p[1] - self.centre[1],
            p[2] - self.centre[2],
        ];
        (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
    }
}

#[derive(Debug, Clone, Default)]
pub struct Model {
    pub materials: Vec<u64>,
    pub meshes: Vec<Mesh>,
    /// The `MDLC` bound (box plus sphere). `None` when the model has no
    /// `MDLC` chunk or the chunk is shorter than the four rows.
    pub bounds: Option<Bounds>,
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

    /// The client's level mask: every submesh LOD byte except `0` and `0x7f`,
    /// OR'd together (`0x141b40be0`, the read at `0x141b40c62`).
    ///
    /// This is the client's own expression for "which levels does this model
    /// have", not a viewer convenience. `0` is the empty mask, which the
    /// client then treats as level `0` (its `+0x20e` store is `0`, not `0xff`).
    pub fn lod_mask(&self) -> u8 {
        self.meshes
            .iter()
            .map(|m| m.lod as u8)
            .filter(|b| *b != 0 && *b != 0x7f)
            .fold(0, |a, b| a | b)
    }

    /// The submeshes the client draws at the finest level of detail.
    ///
    /// The client's expression, read in `0x141b40be0`:
    ///
    /// 1. union every submesh LOD byte except `0` and `0x7f` — [`Model::lod_mask`];
    /// 2. the finest level is the **lowest set bit** of that union (`+0x20e`,
    ///    the `tzcnt` loop at `0x141b40ce8`); with an empty union it is level `0`;
    /// 3. a submesh belongs to a level when that bit is set in its own byte.
    ///
    /// So `lod` is a set, not an ordinal: a submesh carrying `3` is drawn at
    /// both of the two finest levels, and a submesh carrying `0` is never drawn.
    /// The previous rule here compared for equality (`lod == min`), which keeps
    /// neither case right.
    ///
    /// This is the client's *finest* pick only. `0x141b3ce00` chooses the level
    /// per instance from a distance test and a quality term (the reads at
    /// `0x141b2c0a0`/`0x141b2c0b0` and the mode byte `+0x212`), between the
    /// union's lowest and highest bits, scaled by the per-level float at runtime
    /// `+0x50`. A viewer has no per-instance distance, so the finest bit is the
    /// honest choice; it is also what the reference exporter picks.
    pub fn finest_level(&self) -> Vec<&Mesh> {
        let mask = self.lod_mask();
        // Isolate the lowest set bit; an empty union is level 0, as the client
        // stores `+0x20e = 0` when the mask is zero.
        let finest = if mask == 0 {
            1
        } else {
            mask & mask.wrapping_neg()
        };
        self.meshes
            .iter()
            .filter(|m| (m.lod as u8) & finest != 0)
            .collect()
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
///
/// Two things are read rather than inferred. The client's own render-mesh
/// writer (`0x14210c450`) uses `0x02` for a three-float position and `0x03` for
/// a four-float one, and writes the stream stride beside the element; the
/// byte sizes below are pinned against that stride by
/// `the_element_record_is_the_client_layout`. The mapping from this byte to the
/// engine format the input layout actually needs is **open**.
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

    /// Bytes one element occupies inside a vertex. The client's writer stores
    /// the stream stride next to each element (`packed >> 22`); these sizes
    /// tile those strides on the install.
    #[cfg_attr(not(test), allow(dead_code))]
    fn byte_size(self) -> usize {
        match self {
            Format::F32 | Format::U32 => 4,
            Format::F32x2 => 8,
            Format::F32x3 => 12,
            Format::F32x4 => 16,
            Format::F16x2 | Format::U8x4 | Format::U8Unorm4 | Format::U8Snorm4 => 4,
            Format::F16x4 | Format::U16x4 | Format::U16Unorm4 => 8,
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
                // The client hands these bytes to D3D11, so the conversion is
                // the GPU's: `max(x / 127, -1)`, which makes byte 0x80 exactly
                // -1. OWLib divides by 128, which is not that rule; nothing in
                // the image converts these bytes on the CPU.
                for i in 0..4 {
                    out[i] = ((u8_at(d, at + i)? as i8) as f32 / 127.0).max(-1.0);
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
    /// `packed >> 22`: the stride of the element's stream, stored beside the
    /// offset. The client's own render-mesh writer (`0x14210c450`) builds the
    /// packed dword as `stride << 22 | offset << 12`, so the stride is part of
    /// the record, not an inference. Zero in descriptors that writer did not
    /// produce; the reader takes the stride from the descriptor either way
    /// ([`VertexBuffer::stride`]) because that is the copy the verified
    /// `MrnmHandler` publishes at runtime `+0x18`/`+0x19`.
    stride: u32,
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
        // Read from the client's writer (`0x14210c450`): bits 0..3 are the
        // input slot class, 4..11 the instance step rate (both left zero by
        // that writer, so no meaning is claimed for them here), 12..21 the
        // byte offset within the vertex, and 22..31 the stream stride.
        let Some(packed) = u32_at(d, base + 4) else {
            break;
        };
        out.push(Element {
            semantic: Semantic::from_byte(semantic),
            index,
            format: Format::from_byte(format),
            stream,
            offset: ((packed >> 12) & 1023) as usize,
            stride: (packed >> 22) & 1023,
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
    bone_id_offset: u16,
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
    if let Some(mdlc) = versioned(&chunks, asset, b"MDLC", MDLC_VERSION)? {
        // `MdlcMaterialRead`: the GUID array is behind the pointer at 0x08 and
        // its count is the u16 at 0x68; one u64 per material.
        if let Some(offset) = i64_at(mdlc, 0x08) {
            let count = u16_at(mdlc, 0x68).unwrap_or(0) as usize;
            if offset > 0 {
                for i in 0..count {
                    if let Some(m) = u64_at(mdlc, offset as usize + i * 8) {
                        model.materials.push(m);
                    }
                }
            }
        }
        // The same copy: four `vec4` rows at 0x10/0x20/0x30/0x40.
        model.bounds = Bounds::read(mdlc);
    }

    // The model's own structured data. A model without it is not an error;
    // a present chunk the client would reject is. The client's load dispatcher
    // (`0x141b5df70`) skips a chunk whose version is not the one it wants, and
    // its size pass (`0x141b5e1e0`) fails the whole load with 7.
    if !chunks.all_with_tag(crate::mstu::TAG).is_empty() {
        match crate::mstu::parse_header(asset) {
            Ok(header) => model.mstu = Some(header),
            Err(_) => {
                return Err(Error::Malformed(
                    "MSTU header is short or its chunk version is not 2",
                ));
            }
        }
    }

    let mrnm = versioned(&chunks, asset, b"MRNM", crate::mrnm::VERSION)?
        .ok_or(Error::MissingChunk("MRNM"))?;
    model.meshes = read_render_mesh(mrnm, load)?;
    Ok(model)
}

/// The `MDLC` chunk version both client dispatchers require
/// (`0x141b5e1e0` and `0x141b5df70`: `*(u16*)(record + 0xe) == 0x165`).
const MDLC_VERSION: u16 = 0x165;

/// The first chunk carrying `tag`, after the client's own version gate. A
/// wrong version is an error, not a silent skip: the client's size pass
/// returns 7 and the model load fails.
fn versioned<'a>(
    chunks: &Chunks,
    asset: &'a [u8],
    tag: &[u8; 4],
    version: u16,
) -> Result<Option<&'a [u8]>> {
    match chunks.all_with_tag(tag).first() {
        Some(entry) if entry.version == version => {
            Ok(asset.get(entry.offset..entry.offset + entry.size))
        }
        Some(_) => Err(Error::Malformed(
            "chunk version is not the one the client accepts",
        )),
        None => Ok(None),
    }
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
    use crate::mrnm::{header, index_buffer as ib, submesh as sm, vertex_buffer as vb};
    let layout =
        crate::mrnm::Layout::read(d).map_err(|_| Error::Malformed("truncated MRNM header"))?;
    let vbo_ptr =
        i64_at(d, header::VERTEX_BUFFERS).ok_or(Error::Malformed("truncated MRNM header"))?;
    let ibo_ptr =
        i64_at(d, header::INDEX_BUFFERS).ok_or(Error::Malformed("truncated MRNM header"))?;
    let submesh_ptr =
        i64_at(d, header::SUBMESHES).ok_or(Error::Malformed("truncated MRNM header"))?;
    let vbo_count = layout.vertex_buffer_count as usize;
    let ibo_count = layout.index_buffer_count as usize;
    let submesh_count = layout.submesh_count as usize;
    // Bound the allocation by the bytes available: each descriptor is 128 bytes.
    let submesh_cap = d.len() / sm::STRIDE + 1;
    if submesh_count > submesh_cap {
        return Err(Error::Malformed("submesh count exceeds chunk"));
    }

    let mut buffers = Vec::with_capacity(vbo_count);
    for i in 0..vbo_count {
        let base =
            usize::try_from(vbo_ptr).map_err(|_| Error::Malformed("negative MRNM offset"))?;
        let at = base
            .checked_add(
                i.checked_mul(vb::STRIDE)
                    .ok_or(Error::Malformed("vertex buffer runs past the end"))?,
            )
            .ok_or(Error::Malformed("vertex buffer runs past the end"))?;
        let element_offset_raw =
            i64_at(d, at + vb::ELEMENTS).ok_or(Error::Malformed("truncated vertex buffer"))?;
        let element_offset = usize::try_from(element_offset_raw)
            .map_err(|_| Error::Malformed("negative MRNM offset"))?;
        buffers.push(VertexBuffer {
            vertex_count: u32_at(d, at + vb::VERTEX_COUNT)
                .ok_or(Error::Malformed("truncated vertex buffer"))?,
            stride1: u8_at(d, at + vb::STRIDE_STREAM0)
                .ok_or(Error::Malformed("truncated vertex buffer"))?,
            stride2: u8_at(d, at + vb::STRIDE_STREAM1)
                .ok_or(Error::Malformed("truncated vertex buffer"))?,
            element_count: u8_at(d, at + vb::ELEMENT_COUNT)
                .ok_or(Error::Malformed("truncated vertex buffer"))?,
            flags: u8_at(d, at + vb::FLAGS).ok_or(Error::Malformed("truncated vertex buffer"))?,
            element_offset,
            stream1: i64_at(d, at + vb::STREAM0)
                .ok_or(Error::Malformed("truncated vertex buffer"))?,
            stream2: i64_at(d, at + vb::STREAM1)
                .ok_or(Error::Malformed("truncated vertex buffer"))?,
        });
    }

    let mut index_buffers = Vec::with_capacity(ibo_count);
    for i in 0..ibo_count {
        let base =
            usize::try_from(ibo_ptr).map_err(|_| Error::Malformed("negative MRNM offset"))?;
        let at = base
            .checked_add(
                i.checked_mul(ib::STRIDE)
                    .ok_or(Error::Malformed("index buffer runs past the end"))?,
            )
            .ok_or(Error::Malformed("index buffer runs past the end"))?;
        index_buffers.push((
            u32_at(d, at + ib::INDEX_COUNT).ok_or(Error::Malformed("truncated index buffer"))?,
            u32_at(d, at + ib::FORMAT).ok_or(Error::Malformed("truncated index buffer"))?,
            i64_at(d, at + ib::DATA).ok_or(Error::Malformed("truncated index buffer"))?,
        ));
    }

    let mut submeshes = Vec::with_capacity(submesh_count);
    for i in 0..submesh_count {
        let base =
            usize::try_from(submesh_ptr).map_err(|_| Error::Malformed("negative MRNM offset"))?;
        let at = base
            .checked_add(
                i.checked_mul(sm::STRIDE)
                    .ok_or(Error::Malformed("submesh runs past the end"))?,
            )
            .ok_or(Error::Malformed("submesh runs past the end"))?;
        let mut other_index_starts = [0u32; 7];
        for k in 0..7 {
            other_index_starts[k] = u32_at(d, at + sm::OTHER_INDEX_STARTS + k * 4)
                .ok_or(Error::Malformed("truncated submesh"))?;
        }
        let mut other_index_buffers = [0u8; 7];
        for k in 0..7 {
            other_index_buffers[k] = u8_at(d, at + sm::INDEX_BUFFER + 1 + k)
                .ok_or(Error::Malformed("truncated submesh"))?;
        }
        submeshes.push(Submesh {
            vertex_start: u32_at(d, at + sm::VERTEX_START)
                .ok_or(Error::Malformed("truncated submesh"))?,
            index_start: u32_at(d, at + sm::INDEX_START)
                .ok_or(Error::Malformed("truncated submesh"))?,
            bone_id_offset: u16_at(d, at + sm::BONE_ID_OFFSET)
                .ok_or(Error::Malformed("truncated submesh"))?,
            // The reference's struct places Flags (a ushort) at 108, between
            // BoneIdOffset (106) and IndexBuffer (110); the handler copies it
            // to runtime +0x86. Only the low byte is kept (see `Mesh::flags`).
            flags: u16_at(d, at + sm::FLAGS).ok_or(Error::Malformed("truncated submesh"))? as u8,
            other_index_starts,
            indices_to_draw: u32_at(d, at + sm::INDICES_TO_DRAW)
                .ok_or(Error::Malformed("truncated submesh"))?,
            vertices_to_draw: u16_at(d, at + sm::VERTICES_TO_DRAW)
                .ok_or(Error::Malformed("truncated submesh"))? as u32,
            index_buffer: u8_at(d, at + sm::INDEX_BUFFER)
                .ok_or(Error::Malformed("truncated submesh"))?,
            other_index_buffers,
            vertex_buffer: u8_at(d, at + sm::VERTEX_BUFFER)
                .ok_or(Error::Malformed("truncated submesh"))?,
            material: u8_at(d, at + sm::MATERIAL).ok_or(Error::Malformed("truncated submesh"))?,
            lod: u8_at(d, at + sm::LOD).ok_or(Error::Malformed("truncated submesh"))? as i8,
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
        let (index_count, _index_format, index_at) = index_buffers[ibo_index];
        // `indices_to_draw` is a file word. The index buffer's own count is
        // the bound: allocating the file value first is what lets a corrupt
        // count abort on a multi-gigabyte reservation before anything checks
        // it. The format word stays unused: the client's runtime record does
        // not carry it, so its values are a file reading, not client code
        // (`mrnm::index_buffer::FORMAT`).
        let draw = submesh.indices_to_draw as usize;
        if draw > index_count as usize {
            return Err(Error::Malformed("indices to draw exceed the index buffer"));
        }
        let mut original: Vec<u16> = Vec::with_capacity(draw);
        let index_base =
            usize::try_from(index_at).map_err(|_| Error::Malformed("negative MRNM offset"))?;
        let start = index_base
            .checked_add(
                (submesh.index_start as usize)
                    .checked_mul(2)
                    .ok_or(Error::Malformed("index run runs past the end"))?,
            )
            .ok_or(Error::Malformed("index run runs past the end"))?;
        for j in 0..draw {
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
            bone_id_offset: submesh.bone_id_offset,
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

/// One vertex attribute a model's vertex-buffer descriptors declare, exactly
/// as the file stores it: `{u8 semantic, u8 index, u8 format, u8 stream,
/// u32 packed}` with `offset = (packed >> 12) & 1023` and
/// `stride = packed >> 22`.
///
/// The field order and both packed fields are **read from the client**: its
/// own render-mesh writer (`0x14210c450`) emits records in this form and
/// builds the packed dword as `stride << 22 | offset << 12`. The consumer that
/// would turn them into a D3D11 input layout is not present as a table and is
/// **open** (`docs/formats/model.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VertexElement {
    /// The file's semantic byte (0 position, 1 normal, 3 tangent, 4 blend
    /// indices, 5 blend weights, 8 colour, 9 texcoord, 16 vertex index).
    pub semantic: u8,
    /// The semantic's own index: texcoord set 0/1/2, colour set 0/1.
    pub index: u8,
    /// The file's format byte, as stored. Unknown bytes are returned rather
    /// than hidden, so a census can see a format the reader does not decode.
    pub format: u8,
    /// 0 or 1: which of the descriptor's two streams the element reads.
    pub stream: u8,
    /// Byte offset within the vertex.
    pub offset: u16,
    /// Stream stride stored beside the offset, or 0 when the writer left it
    /// empty. The reader takes the stride from the descriptor instead
    /// (`VertexBuffer::stride`), because that is the copy the verified
    /// `MrnmHandler` publishes at runtime `+0x18`/`+0x19`.
    pub stride: u16,
    /// The vertex-buffer descriptor's own stride for that stream
    /// (`+0x08`/`+0x09`), which is the copy the verified `MrnmHandler`
    /// publishes at runtime `+0x18`/`+0x19`. On this install it equals
    /// [`VertexElement::stride`] whenever the writer filled that in.
    pub descriptor_stride: u8,
    /// How many vertex-buffer descriptors declare this exact element.
    pub buffers: usize,
}

/// Every vertex attribute a model's vertex-buffer descriptors declare.
///
/// This is the census the model and skeleton probes use: it reads the element
/// arrays without decoding a vertex, so an unknown format byte or a surplus
/// semantic shows up instead of being silently skipped.
pub fn vertex_elements(asset: &[u8]) -> Result<Vec<VertexElement>> {
    let chunks = Chunks::parse(asset).map_err(|_| Error::NotAChunkedAsset)?;
    let d = chunks
        .payload(asset, b"MRNM")
        .ok_or(Error::MissingChunk("MRNM"))?;
    // The elements belong to the *vertex buffer* descriptors (record stride
    // 96, element count at +10, element array at +16), not to the submeshes.
    let vbo_ptr = i64_at(d, crate::mrnm::header::VERTEX_BUFFERS)
        .ok_or(Error::Malformed("truncated MRNM header"))? as usize;
    let vbo_count = u8_at(d, crate::mrnm::header::VERTEX_BUFFER_COUNT)
        .ok_or(Error::Malformed("truncated MRNM header"))? as usize;
    let mut seen: Vec<VertexElement> = Vec::new();
    for i in 0..vbo_count {
        let at = vbo_ptr + i * crate::mrnm::vertex_buffer::STRIDE;
        let element_count =
            u8_at(d, at + crate::mrnm::vertex_buffer::ELEMENT_COUNT).unwrap_or(0) as usize;
        let element_offset = i64_at(d, at + crate::mrnm::vertex_buffer::ELEMENTS)
            .unwrap_or(0)
            .max(0) as usize;
        if element_count == 0 || element_offset == 0 {
            continue;
        }
        let stride0 = u8_at(d, at + crate::mrnm::vertex_buffer::STRIDE_STREAM0).unwrap_or(0);
        let stride1 = u8_at(d, at + crate::mrnm::vertex_buffer::STRIDE_STREAM1).unwrap_or(0);
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
                Format::Unknown(b) => b,
            };
            let element = VertexElement {
                semantic,
                index: e.index,
                format,
                stream: e.stream,
                offset: e.offset as u16,
                stride: e.stride as u16,
                descriptor_stride: if e.stream == 0 { stride0 } else { stride1 },
                buffers: 1,
            };
            match seen.iter_mut().find(|s| {
                s.semantic == element.semantic
                    && s.index == element.index
                    && s.format == element.format
                    && s.stream == element.stream
                    && s.offset == element.offset
                    && s.stride == element.stride
                    && s.descriptor_stride == element.descriptor_stride
            }) {
                Some(s) => s.buffers += 1,
                None => seen.push(element),
            }
        }
    }
    seen.sort_by_key(|s| {
        (
            s.semantic,
            s.index,
            s.format,
            s.stream,
            s.offset,
            s.descriptor_stride,
        )
    });
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

    /// The records the client's own render-mesh writer (`0x14210c450`) emits,
    /// in the client's field order and packed-dword form. Nothing here is a
    /// guess: the writer stores `{semantic, index, format, stream}` and
    /// `stride << 22 | offset << 12`, and this is the element set the
    /// practice range's character mesh carries.
    #[test]
    fn the_element_record_is_the_client_layout() {
        // (semantic, index, format, stream, offset, stream stride)
        let records = [
            (0u8, 0u8, 0x02u8, 0u8, 0u32, 24u32), // position, three floats
            (4, 0, 0x07, 0, 12, 24),              // blend indices, four u16
            (5, 0, 0x08, 0, 20, 24),              // blend weights, four u8
            (1, 0, 0x09, 1, 0, 20),               // normal, four snorm bytes
            (3, 0, 0x09, 1, 4, 20),               // tangent, four snorm bytes
            (9, 0, 0x04, 1, 8, 20),               // texcoord 0, two halves
            (9, 1, 0x04, 1, 12, 20),              // texcoord 1, two halves
            (16, 0, 0x0c, 1, 16, 20),             // vertex index, one u32
        ];
        let mut d = vec![0u8; records.len() * 8];
        for (i, (semantic, index, format, stream, offset, stride)) in records.iter().enumerate() {
            let at = i * 8;
            d[at] = *semantic;
            d[at + 1] = *index;
            d[at + 2] = *format;
            d[at + 3] = *stream;
            let packed = (stride << 22) | (offset << 12);
            d[at + 4..at + 8].copy_from_slice(&packed.to_le_bytes());
        }

        let e = read_elements(&d, 0, records.len());
        assert_eq!(e.len(), 8, "every record is read");
        for (i, (semantic, index, format, stream, offset, stride)) in records.iter().enumerate() {
            assert_eq!(
                e[i].semantic,
                Semantic::from_byte(*semantic),
                "semantic {i}"
            );
            assert_eq!(e[i].index, *index, "index {i}");
            assert_eq!(e[i].format, Format::from_byte(*format), "format {i}");
            assert_eq!(e[i].stream, *stream, "stream {i}");
            assert_eq!(e[i].offset, *offset as usize, "offset {i}");
            assert_eq!(e[i].stride, *stride, "stride {i}");
        }

        // The sizes tile each stream's stride: every element begins where the
        // previous one ended, and the last one ends exactly on the stride.
        for stream in 0..2u8 {
            let mut of = e.iter().filter(|x| x.stream == stream).collect::<Vec<_>>();
            of.sort_by_key(|x| x.offset);
            let mut expect = 0usize;
            for element in &of {
                assert_eq!(element.offset, expect, "stream {stream} has a gap");
                expect += element.format.byte_size();
            }
            assert_eq!(
                expect, of[0].stride as usize,
                "stream {stream} does not fill its stride"
            );
        }

        // The writer's four-float position is format 0x03, and the one format
        // it uses that no model file carries (0x13) stays undecoded rather
        // than being given an invented reading.
        assert_eq!(Format::from_byte(0x03).byte_size(), 16);
        assert_eq!(Format::from_byte(0x13), Format::Unknown(0x13));
    }

    #[test]
    fn unorm_bytes_scale_to_unit() {
        let data = [0u8, 128, 255, 64];
        let value = Format::U8Unorm4.read(&data, 0).unwrap();
        assert_eq!(value[0], 0.0);
        assert!((value[1] - 128.0 / 255.0).abs() < 1e-6);
        assert_eq!(value[2], 1.0);
    }

    #[test]
    fn snorm_bytes_clamp_like_the_gpu() {
        // The client sends these bytes to D3D11, so the GPU's `max(x/127, -1)`
        // is the conversion the pipeline performs. 0x80 is the edge the two
        // candidate rules disagree on (OWLib gives -1.0 only by dividing by
        // 128; a bare /127 gives -1.0079).
        let data = [0x80u8, 0x7f, 0x82, 0x00];
        let value = Format::U8Snorm4.read(&data, 0).unwrap();
        assert_eq!(value[0], -1.0);
        assert_eq!(value[1], 1.0);
        assert!((value[2] - (-126.0 / 127.0)).abs() < 1e-6);
        assert_eq!(value[3], 0.0);
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

    #[test]
    fn a_draw_count_past_the_index_buffer_is_rejected_before_allocation() {
        let mut d = synth_mrnm(false);
        // The index buffer holds 3 indices; a submesh that claims to draw
        // u32::MAX of them is malformed. The old code reserved that many u16s
        // (8 GiB) before any bound check, aborting the process.
        let at = 208 + crate::mrnm::submesh::INDICES_TO_DRAW;
        d[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        let meshes = read_render_mesh(&d, &|_| None);
        assert!(
            meshes.is_err(),
            "a draw count past the index buffer must be a refusal"
        );
    }

    /// A container laid out as the file does: one record followed by its
    /// payload, per chunk, with the record's version field set.
    fn container(chunks: &[(&[u8; 4], u16, &[u8])]) -> Vec<u8> {
        use crate::chunks::{encode_tag, ENTRY_SIZE, MAGIC, SENTINEL};
        let body: usize = chunks.iter().map(|(_, _, p)| ENTRY_SIZE + p.len()).sum();
        let mut out = Vec::new();
        out.extend_from_slice(&MAGIC.to_le_bytes());
        out.extend_from_slice(&encode_tag(b"MODL").to_le_bytes());
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
        out
    }

    /// `MdlcMaterialRead` (`0x141b88cb0`): the GUID array is behind the pointer
    /// at 0x08 and its count is the u16 at 0x68. The floats at 0x10 are the
    /// bound, not a count -- reading one of them as the count shifts the list.
    #[test]
    fn the_material_list_is_the_pointer_at_8_with_the_count_at_0x68() {
        let mut mdlc = vec![0u8; 0x90];
        let guids = [0x1122_3344_5566_7788u64, 0x99AA_BBCC_DDEE_FF00];
        mdlc[0x08..0x10].copy_from_slice(&0x80i64.to_le_bytes());
        mdlc[0x68..0x6a].copy_from_slice(&(guids.len() as u16).to_le_bytes());
        // A float box that is *not* a material count: if the parser reads it as
        // one, the material list is wrong.
        mdlc[0x10..0x14].copy_from_slice(&1.0f32.to_le_bytes());
        mdlc[0x80..0x88].copy_from_slice(&guids[0].to_le_bytes());
        mdlc[0x88..0x90].copy_from_slice(&guids[1].to_le_bytes());
        let mrnm = synth_mrnm(false);
        let asset = container(&[
            (b"MDLC", MDLC_VERSION, &mdlc),
            (b"MRNM", crate::mrnm::VERSION, &mrnm),
        ]);
        let model = parse(&asset, &|_| None).expect("synthetic model parses");
        assert_eq!(model.materials, guids);
        assert_eq!(model.meshes.len(), 1);
    }

    /// The bound is four `vec4` rows at 0x10/0x20/0x30/0x40 (min, max, sphere
    /// centre, radius), and the twelve bytes at 0x44..0x50 play no part: the
    /// client never reads them. Values here are the ones measured on the
    /// character model -- if the reader takes a 4x4 matrix, `centre` becomes
    /// the box's fourth row and `radius` a corner, and the assertions fail.
    #[test]
    fn the_bound_is_min_max_a_sphere_centre_and_a_radius() {
        let mut mdlc = vec![0u8; 0x90];
        let floats: [(usize, f32); 13] = [
            (0x10, -0.6616),
            (0x14, -0.0029),
            (0x18, -0.4976),
            (0x1c, 0.0),
            (0x20, 0.6700),
            (0x24, 1.8404),
            (0x28, 0.2020),
            (0x2c, 0.0010),
            (0x30, 0.0),
            (0x34, 0.8924),
            (0x38, 0.0620),
            (0x3c, 0.0),
            (0x40, 0.9801),
        ];
        for (at, v) in floats {
            mdlc[at..at + 4].copy_from_slice(&v.to_le_bytes());
        }
        // The dead lanes are not zero in this fixture, so a reader that walks
        // into them picks up a wrong value rather than a harmless zero.
        for at in [0x44usize, 0x48, 0x4c] {
            mdlc[at..at + 4].copy_from_slice(&f32::MAX.to_le_bytes());
        }
        let mrnm = synth_mrnm(false);
        let asset = container(&[
            (b"MDLC", MDLC_VERSION, &mdlc),
            (b"MRNM", crate::mrnm::VERSION, &mrnm),
        ]);
        let model = parse(&asset, &|_| None).expect("synthetic model parses");
        let b = model.bounds.expect("MDLC carries the bound");
        assert_eq!(b.min, [-0.6616, -0.0029, -0.4976]);
        assert_eq!(b.max, [0.6700, 1.8404, 0.2020]);
        assert_eq!(b.centre, [0.0, 0.8924, 0.0620]);
        assert_eq!(b.radius, 0.9801);
        let size = b.size();
        assert!((size[0] - 1.3316).abs() < 1e-4);
        assert!((size[1] - 1.8433).abs() < 1e-4);
        assert!((size[2] - 0.6996).abs() < 1e-4);
        assert!(b.contains([0.0, 0.9, 0.0]));
        assert!(!b.contains([0.0, 2.0, 0.0]));
        // The sphere is smaller than the box's half-diagonal, so these differ.
        let half_diagonal =
            (size[0] * size[0] + size[1] * size[1] + size[2] * size[2]).sqrt() / 2.0;
        assert!(b.radius < half_diagonal);
        assert!(b.sphere_distance([0.0, 1.8, 0.0]) > 0.8);
    }

    /// A model with no `MDLC` has no bound, rather than a zero one.
    #[test]
    fn a_model_without_mdlc_has_no_bound() {
        let mrnm = synth_mrnm(false);
        let asset = container(&[(b"MRNM", crate::mrnm::VERSION, &mrnm)]);
        let model = parse(&asset, &|_| None).expect("synthetic model parses");
        assert!(model.bounds.is_none());
    }

    /// Both client dispatchers compare the chunk's version and refuse the load
    /// on a mismatch; the parser must not silently read a future layout.
    #[test]
    fn a_wrong_chunk_version_is_rejected() {
        let mrnm = synth_mrnm(false);
        let asset = container(&[(b"MRNM", crate::mrnm::VERSION + 1, &mrnm)]);
        assert!(matches!(
            parse(&asset, &|_| None),
            Err(Error::Malformed(
                "chunk version is not the one the client accepts"
            ))
        ));
    }

    // The LOD rule below is the client's, not a viewer choice: the byte at
    // submesh `+0x78`/`+0x92` is a **set** of levels, the model's finest level
    // is its union's lowest bit, and a submesh belongs to a level when that bit
    // is set (`0x141b40be0`, the union read at `0x141b40c62`).

    fn mesh_at(lod: i8) -> Mesh {
        Mesh {
            lod,
            ..Mesh::default()
        }
    }

    fn lods(model: &Model) -> Vec<i8> {
        model.finest_level().iter().map(|m| m.lod).collect()
    }

    #[test]
    fn the_finest_level_is_the_lowest_bit_of_the_lod_union() {
        let model = Model {
            meshes: vec![mesh_at(8), mesh_at(4), mesh_at(2), mesh_at(1)],
            ..Default::default()
        };
        assert_eq!(model.lod_mask(), 0x0f);
        assert_eq!(lods(&model), vec![1]);
    }

    /// A submesh carrying two levels is drawn at both of them. The old
    /// equality rule (`lod == min`) kept only the exact minimum.
    #[test]
    fn a_submesh_that_spans_two_levels_is_drawn_at_the_finest() {
        let model = Model {
            meshes: vec![mesh_at(3), mesh_at(1)],
            ..Default::default()
        };
        assert_eq!(model.lod_mask(), 3);
        assert_eq!(lods(&model), vec![3, 1]);
    }

    /// `0` is not a level: the client skips it in the union, and its own bit is
    /// never set, so such a submesh is never drawn. The old rule made `0` the
    /// finest level and dropped every real one.
    #[test]
    fn a_lod_of_zero_is_not_a_level() {
        let model = Model {
            meshes: vec![mesh_at(0), mesh_at(2)],
            ..Default::default()
        };
        assert_eq!(model.lod_mask(), 2);
        assert_eq!(lods(&model), vec![2]);

        let only_zero = Model {
            meshes: vec![mesh_at(0)],
            ..Default::default()
        };
        assert_eq!(only_zero.lod_mask(), 0);
        assert!(only_zero.finest_level().is_empty());
    }

    /// `0x7f` is kept out of the union, but as a byte it still carries bits; an
    /// empty union is level 0, so bit 0 of `0x7f` is what keeps it visible.
    #[test]
    fn a_127_lod_is_kept_out_of_the_mask_but_still_carries_its_bits() {
        let model = Model {
            meshes: vec![mesh_at(0x7f)],
            ..Default::default()
        };
        assert_eq!(model.lod_mask(), 0);
        assert_eq!(model.finest_level().len(), 1);
    }

    /// `0xFF` (`-1`) is not special-cased by the client: it enters the union,
    /// which makes it level 0 and sets every bit, so it is drawn at every level.
    #[test]
    fn a_negative_lod_sets_every_bit() {
        let model = Model {
            meshes: vec![mesh_at(-1), mesh_at(2)],
            ..Default::default()
        };
        assert_eq!(model.lod_mask(), 0xff);
        // Level 0: the `-1` submesh has it, the `2` one does not.
        assert_eq!(lods(&model), vec![-1]);
    }
}

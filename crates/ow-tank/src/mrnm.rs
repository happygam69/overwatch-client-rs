//! `MRNM`: the client's render-mesh load.
//!
//! A model asset carries the render mesh in the `MRNM` chunk. The client does
//! not read the file-side descriptors directly: its chunk dispatcher
//! (`ModelChunkAttach`, `0x141b5e1e0`) rewrites the payload's offsets into
//! pointers in place, sizes the runtime block with `MrnmRuntimeSize`
//! (`0x141b89390`), and `MrnmHandler` (`0x141b88f10`) copies the header, the
//! descriptors and every vertex-element array into that block. Everything
//! below is those three steps, operation for operation.
//!
//! **The payload is a relocation target.** Three qwords at `+0x00`, `+0x08`
//! and `+0x10` are payload-relative offsets to the vertex-buffer, index-buffer
//! and submesh descriptor arrays; the vertex-buffer descriptors carry one more
//! offset (`+0x10`, the element array) and two data-stream offsets (`+0x50`,
//! `+0x58`, only when `flags & 0x10` is clear); each index-buffer descriptor
//! carries one (`+0x08`). [`fixup`] is the client's `MrnmFixup`
//! (`0x141b893e0`): it adds the payload base to each of those slots, leaving
//! a stored zero as null.
//!
//! **Address-space substitution.** The client writes absolute pointers. For a
//! Rust `Vec<u8>` there is no stable absolute address, so every function here
//! takes the base as a parameter: pass `0` and the pointers stay offsets into
//! the payload / the runtime block, which is what the parser reads. The
//! differential harness passes the real arena base on both sides, which is how
//! the pointer arithmetic is proven byte for byte (`dev/verify_harness/units/
//! fn-MrnmFixup.json`, `fn-MrnmRuntimeSize.json`, `fn-MrnmHandler.json`).
//!
//! **What is *not* proven here.** The 8-byte vertex-element records are copied
//! as bytes; no client code read by this crate decodes their semantic, format
//! or packed-offset fields, so those stay a reading of the file
//! (`crate::model`). The index-buffer `format` word at descriptor `+0x04` is
//! likewise copied by nobody: the client's runtime record does not carry it.

use std::fmt;
use std::num::Wrapping;

pub const TAG: &[u8; 4] = b"MRNM";
/// The chunk version the dispatcher requires (`ModelChunkAttach` `0x141b5e1e0`).
pub const VERSION: u16 = 0x1f;

/// Payload offsets of the header fields the load reads.
pub mod header {
    /// Payload-relative offset of the vertex-buffer descriptor array
    /// (stride `0x60`, count at [`VERTEX_BUFFER_COUNT`]).
    pub const VERTEX_BUFFERS: usize = 0x00;
    /// Payload-relative offset of the index-buffer descriptor array
    /// (stride `0x10`, count at [`INDEX_BUFFER_COUNT`]).
    pub const INDEX_BUFFERS: usize = 0x08;
    /// Payload-relative offset of the submesh descriptor array
    /// (stride `0x80`, count at [`SUBMESH_COUNT`]).
    pub const SUBMESHES: usize = 0x10;
    /// `u32`: copied to runtime `+0x08`; the client does not read it again.
    pub const VERTEX_COUNT: usize = 0x18;
    /// `u32`: submesh count, and the first runtime array's length.
    pub const SUBMESH_COUNT: usize = 0x1c;
    /// `u16`: copied to runtime `+0x10`.
    pub const MATERIAL_COUNT: usize = 0x20;
    /// `u8`: vertex-buffer count.
    pub const VERTEX_BUFFER_COUNT: usize = 0x22;
    /// `u8`: index-buffer count.
    pub const INDEX_BUFFER_COUNT: usize = 0x23;
    /// `u32`: copied to runtime `+0x14`.
    pub const UNKNOWN_24: usize = 0x24;
    /// Five `0x1c`-byte blocks copied verbatim to runtime `+0x18..+0xa4`:
    /// `+0x28`, `+0x44`, `+0x60`, `+0x7c`, `+0x98`.
    pub const BLOCKS: [usize; 5] = [0x28, 0x44, 0x60, 0x7c, 0x98];
    /// The smallest payload that still holds every header field read above.
    pub const MIN: usize = 0xb4;
}

/// Source offsets inside a vertex-buffer descriptor (`0x60` bytes).
pub mod vertex_buffer {
    pub const VERTEX_COUNT: usize = 0x00;
    pub const UNKNOWN_04: usize = 0x04;
    pub const STRIDE_STREAM0: usize = 0x08;
    pub const STRIDE_STREAM1: usize = 0x09;
    pub const ELEMENT_COUNT: usize = 0x0a;
    /// Bit `0x10`: the stream qwords are asset GUIDs, not payload offsets.
    pub const FLAGS: usize = 0x0b;
    pub const UNKNOWN_0C: usize = 0x0c;
    /// Payload-relative offset of the element array (`count * 8` bytes).
    pub const ELEMENTS: usize = 0x10;
    // +0x18 .. +0x48: seven qwords, copied to runtime +0xb0 .. +0xe0.
    pub const STREAM0: usize = 0x50;
    pub const STREAM1: usize = 0x58;
    pub const STRIDE: usize = 0x60;
}

/// Source offsets inside an index-buffer descriptor (`0x10` bytes).
pub mod index_buffer {
    pub const INDEX_COUNT: usize = 0x00;
    /// The index width selector. The client's runtime record does **not**
    /// carry it, so its values are a reading of the file, not of client code.
    pub const FORMAT: usize = 0x04;
    /// Payload-relative offset of the index bytes.
    pub const DATA: usize = 0x08;
    pub const STRIDE: usize = 0x10;
}

/// Source offsets inside a submesh descriptor (`0x80` bytes), as the client
/// copies them into the runtime record.
pub mod submesh {
    pub const VERTEX_START: usize = 0x3c;
    /// `+0x40..+0x60` is copied as one 32-byte block: the index start followed
    /// by the seven extra index starts.
    pub const INDEX_START: usize = 0x40;
    pub const OTHER_INDEX_STARTS: usize = 0x44;
    pub const INDEX_COUNT: usize = 0x60;
    pub const INDICES_TO_DRAW: usize = 0x64;
    pub const VERTICES_TO_DRAW: usize = 0x68;
    pub const BONE_ID_OFFSET: usize = 0x6a;
    pub const FLAGS: usize = 0x6c;
    /// `+0x6e..+0x76`: the index-buffer index and the seven extra ones.
    pub const INDEX_BUFFER: usize = 0x6e;
    /// `+0x76`, `+0x77`, `+0x78`: vertex-buffer index, material, LOD.
    pub const VERTEX_BUFFER: usize = 0x76;
    pub const MATERIAL: usize = 0x77;
    pub const LOD: usize = 0x78;
    /// `+0x79..+0x7c`: unknown bytes; `+0x7b` falls back to the model default
    /// (the MDLC byte at `+0x78`) when it is zero.
    pub const UNKNOWN_79: usize = 0x79;
    pub const DEFAULTED: usize = 0x7b;
    pub const UNKNOWN_7C: usize = 0x7c;
    pub const STRIDE: usize = 0x80;
}

/// Runtime offsets inside the loaded block (`0xc0` header, then the three
/// arrays). These are the destinations `MrnmHandler` writes.
pub mod runtime {
    /// Submesh-array pointer (`MrnmHandler` `param_3[0x15]`).
    pub const SUBMESH_ARRAY: usize = 0xa8;
    /// Vertex-buffer-array pointer (`param_3[0x16]`).
    pub const VERTEX_ARRAY: usize = 0xb0;
    /// Index-buffer-array pointer (`param_3[0x17]`).
    pub const INDEX_ARRAY: usize = 0xb8;
    /// The arrays start here when the destination is 16-byte aligned: the
    /// client uses `(dst + 0xcf) & !0xf`.
    pub const ARRAYS: usize = 0xc0;
    pub const SUBMESH_STRIDE: usize = 0xa0;
    pub const VERTEX_STRIDE: usize = 0xf8;
    pub const INDEX_STRIDE: usize = 0x18;

    /// Submesh record fields, at the offsets the handler writes.
    pub mod submesh {
        pub const VERTEX_START: usize = 0x54;
        /// `+0x58..+0x78`: index start and the seven extra index starts.
        pub const INDEX_START: usize = 0x58;
        pub const INDICES_TO_DRAW: usize = 0x7c;
        pub const VERTICES_TO_DRAW: usize = 0x80;
        pub const BONE_ID_OFFSET: usize = 0x82;
        pub const FLAGS: usize = 0x86;
        /// `+0x88..+0x90`: index buffer and the seven extra ones.
        pub const INDEX_BUFFER: usize = 0x88;
        pub const VERTEX_BUFFER: usize = 0x90;
        pub const MATERIAL: usize = 0x91;
        pub const LOD: usize = 0x92;
    }

    /// Vertex-buffer record fields, at the offsets the handler writes.
    pub mod vertex_buffer {
        pub const VERTEX_COUNT: usize = 0x10;
        pub const STRIDE_STREAM0: usize = 0x18;
        pub const STRIDE_STREAM1: usize = 0x19;
        pub const ELEMENT_COUNT: usize = 0x1a;
        pub const FLAGS: usize = 0x1b;
        /// The element array, already copied into the block (`count * 8`).
        pub const ELEMENTS: usize = 0x1c;
        pub const STREAM0: usize = 0xa0;
        pub const STREAM1: usize = 0xa8;
    }

    /// Index-buffer record fields, at the offsets the handler writes.
    pub mod index_buffer {
        pub const INDEX_COUNT: usize = 0x08;
        pub const DATA: usize = 0x10;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Malformed(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Malformed(m) => write!(f, "malformed MRNM: {m}"),
        }
    }
}

impl std::error::Error for Error {}

type Result<T> = std::result::Result<T, Error>;

fn u8_at(d: &[u8], at: usize) -> Result<u8> {
    d.get(at)
        .copied()
        .ok_or(Error::Malformed("read past the end"))
}

fn u16_at(d: &[u8], at: usize) -> Result<u16> {
    d.get(at..at + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or(Error::Malformed("read past the end"))
}

fn u32_at(d: &[u8], at: usize) -> Result<u32> {
    d.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or(Error::Malformed("read past the end"))
}

fn u64_at(d: &[u8], at: usize) -> Result<u64> {
    d.get(at..at + 8)
        .map(|b| u64::from_le_bytes(b.try_into().unwrap()))
        .ok_or(Error::Malformed("read past the end"))
}

/// Turn a pointer slot into a payload offset. A stored zero is null; the
/// client's rebase checks the stored value, not the sum, so `0` stays `0`.
fn offset_of(value: u64, base: u64) -> Result<usize> {
    if value == 0 {
        return Err(Error::Malformed("null pointer"));
    }
    let rel = value
        .checked_sub(base)
        .ok_or(Error::Malformed("pointer below the payload base"))?;
    usize::try_from(rel).map_err(|_| Error::Malformed("pointer is not an offset"))
}

/// Rebase one qword slot: `slot = slot == 0 ? 0 : slot + base`.
fn rebase_slot(payload: &mut [u8], at: usize, base: u64) -> Result<()> {
    let value = u64_at(payload, at)?;
    let out = if value == 0 {
        0
    } else {
        value.wrapping_add(base)
    };
    payload
        .get_mut(at..at + 8)
        .ok_or(Error::Malformed("read past the end"))?
        .copy_from_slice(&out.to_le_bytes());
    Ok(())
}

/// The header fields `MrnmHandler` copies to the runtime block.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Layout {
    /// `+0x18`, a whole-model vertex count the client stores but never reads.
    pub vertex_count: u32,
    /// `+0x1c`, the submesh count.
    pub submesh_count: u32,
    /// `+0x20`, copied as a `u16`.
    pub material_count: u16,
    /// `+0x22`, the vertex-buffer count.
    pub vertex_buffer_count: u8,
    /// `+0x23`, the index-buffer count.
    pub index_buffer_count: u8,
    /// `+0x24`, copied as a `u32`.
    pub unknown_24: u32,
    /// The five `0x1c`-byte blocks at `+0x28`, `+0x44`, `+0x60`, `+0x7c`,
    /// `+0x98`, as stored. Neither the client nor this crate interprets them.
    pub blocks: [[u8; 0x1c]; 5],
}

impl Layout {
    pub fn read(payload: &[u8]) -> Result<Layout> {
        let mut blocks = [[0u8; 0x1c]; 5];
        for (slot, source) in blocks.iter_mut().zip(header::BLOCKS) {
            let src = payload
                .get(source..source + 0x1c)
                .ok_or(Error::Malformed("read past the end"))?;
            slot.copy_from_slice(src);
        }
        Ok(Layout {
            vertex_count: u32_at(payload, header::VERTEX_COUNT)?,
            submesh_count: u32_at(payload, header::SUBMESH_COUNT)?,
            material_count: u16_at(payload, header::MATERIAL_COUNT)?,
            vertex_buffer_count: u8_at(payload, header::VERTEX_BUFFER_COUNT)?,
            index_buffer_count: u8_at(payload, header::INDEX_BUFFER_COUNT)?,
            unknown_24: u32_at(payload, header::UNKNOWN_24)?,
            blocks,
        })
    }

    /// Whether the descriptor arrays can physically fit in a payload of
    /// `len` bytes. The client has no such check; it is what keeps `load`
    /// from allocating a wrapped size for a corrupt count.
    fn fits(&self, len: usize) -> bool {
        (self.submesh_count as usize) <= len / submesh::STRIDE
            && (self.vertex_buffer_count as usize) <= len / vertex_buffer::STRIDE
            && (self.index_buffer_count as usize) <= len / index_buffer::STRIDE
    }
}

/// The runtime block size the client allocates before the load
/// (`MrnmRuntimeSize`, `0x141b89390`).
///
/// The return expression is the client's, operation for operation:
/// `submeshes*0xa0 + 0xc0 + vertex_buffers*0xf8 + (index_buffers +
/// index_buffers*2)*8`. The client's expression is 32-bit and unchecked;
/// `Wrapping` keeps the low 32 bits exactly and never panics.
pub fn runtime_size(payload: &[u8]) -> Result<u32> {
    let submeshes = Wrapping(u32_at(payload, header::SUBMESH_COUNT)?);
    let vertex_buffers = Wrapping(u8_at(payload, header::VERTEX_BUFFER_COUNT)? as u32);
    let index_buffers = Wrapping(u8_at(payload, header::INDEX_BUFFER_COUNT)? as u32);
    let mut size = submeshes * Wrapping(0xa0);
    size += Wrapping(0xc0);
    size += vertex_buffers * Wrapping(0xf8);
    size += (index_buffers + index_buffers * Wrapping(2)) * Wrapping(8);
    Ok(size.0)
}

/// The client's in-place offset→pointer rebase (`MrnmFixup`, `0x141b893e0`).
///
/// Three header slots, one element-array slot and up to two stream slots per
/// vertex buffer, one data slot per index buffer. A stored zero is left as
/// zero. Nothing else in the payload is touched, in particular no submesh
/// field: the submesh's 32-byte index-start block is data, not pointers.
pub fn fixup(payload: &mut [u8], base: u64) -> Result<()> {
    rebase_slot(payload, header::VERTEX_BUFFERS, base)?;
    rebase_slot(payload, header::INDEX_BUFFERS, base)?;
    rebase_slot(payload, header::SUBMESHES, base)?;

    let vertex_buffers = offset_of(u64_at(payload, header::VERTEX_BUFFERS)?, base)?;
    let vertex_buffer_count = u8_at(payload, header::VERTEX_BUFFER_COUNT)? as usize;
    for i in 0..vertex_buffer_count {
        let at = vertex_buffers
            .checked_add(i.wrapping_mul(vertex_buffer::STRIDE))
            .ok_or(Error::Malformed("vertex buffer runs past the end"))?;
        rebase_slot(payload, at + vertex_buffer::ELEMENTS, base)?;
        if u8_at(payload, at + vertex_buffer::FLAGS)? & 0x10 == 0 {
            rebase_slot(payload, at + vertex_buffer::STREAM0, base)?;
            rebase_slot(payload, at + vertex_buffer::STREAM1, base)?;
        }
    }

    let index_buffers = offset_of(u64_at(payload, header::INDEX_BUFFERS)?, base)?;
    let index_buffer_count = u8_at(payload, header::INDEX_BUFFER_COUNT)? as usize;
    for i in 0..index_buffer_count {
        let at = index_buffers
            .checked_add(i.wrapping_mul(index_buffer::STRIDE))
            .ok_or(Error::Malformed("index buffer runs past the end"))?;
        rebase_slot(payload, at + index_buffer::DATA, base)?;
    }
    Ok(())
}

/// A loaded runtime block and the header it was built from.
#[derive(Debug, Clone)]
pub struct Loaded {
    block: Vec<u8>,
    pub layout: Layout,
    /// Runtime `+0x00`: the OR of every submesh's `+0x28` qword, skipping the
    /// `-1` values. The client builds it in the submesh loop; it is the only
    /// thing the load computes rather than copies.
    pub used_mask: u64,
    /// The runtime offset of the submesh array (`block[+0xa8]`, or
    /// `(dst_base + 0xcf) & !0xf` relative to `dst_base`).
    pub submesh_array: usize,
    /// The runtime offset of the vertex-buffer array (`block[+0xb0]`).
    pub vertex_array: usize,
    /// The runtime offset of the index-buffer array (`block[+0xb8]`).
    pub index_array: usize,
}

impl Loaded {
    /// The whole runtime block. With base `0` the three array slots at
    /// `+0xa8/+0xb0/+0xb8` are offsets into this block. The vertex-buffer
    /// record's `+0xa0/+0xa8` are **not** stream pointers: the client zeroes
    /// them for in-chunk streams and copies the two GUIDs only for a separate
    /// vertex-data asset.
    pub fn block(&self) -> &[u8] {
        &self.block
    }

    pub fn submesh(&self, i: usize) -> Option<&[u8]> {
        let at = self
            .submesh_array
            .checked_add(i.checked_mul(runtime::SUBMESH_STRIDE)?)?;
        self.block.get(at..at + runtime::SUBMESH_STRIDE)
    }

    pub fn vertex_buffer(&self, i: usize) -> Option<&[u8]> {
        let at = self
            .vertex_array
            .checked_add(i.checked_mul(runtime::VERTEX_STRIDE)?)?;
        self.block.get(at..at + runtime::VERTEX_STRIDE)
    }

    pub fn index_buffer(&self, i: usize) -> Option<&[u8]> {
        let at = self
            .index_array
            .checked_add(i.checked_mul(runtime::INDEX_STRIDE)?)?;
        self.block.get(at..at + runtime::INDEX_STRIDE)
    }
}

/// The client's `MrnmHandler` (`0x141b88f10`): build the runtime block.
///
/// `payload` must already be fixed up ([`fixup`]); pass the same `base` here.
/// `payload_base` and `dst_base` are the addresses the client's pointers would
/// have. With both `0` the block's pointer slots are offsets and the caller
/// can read it directly. `default_byte` is the model object's `+0xaa`, which
/// MDLC fills with its `+0x78`; it replaces a zero submesh byte at `+0x7b`.
pub fn load(payload: &[u8], default_byte: u8, payload_base: u64, dst_base: u64) -> Result<Loaded> {
    let layout = Layout::read(payload)?;
    if !layout.fits(payload.len()) {
        return Err(Error::Malformed("descriptor count exceeds the payload"));
    }
    let size = runtime_size(payload)? as usize;
    let mut block = vec![0u8; size];

    // The client aligns `dst + 0xcf` down to 16; with a 16-byte-aligned dst
    // that is `dst + 0xc0`, and the size formula budgets exactly that.
    let arrays = (dst_base.wrapping_add(0xcf) & !0xf).wrapping_sub(dst_base);
    let submesh_array = usize::try_from(arrays)
        .map_err(|_| Error::Malformed("destination base is not an address"))?;
    let vertex_array = submesh_array
        .checked_add((layout.submesh_count as usize).wrapping_mul(runtime::SUBMESH_STRIDE))
        .ok_or(Error::Malformed("runtime block overflows"))?;
    let index_array = vertex_array
        .checked_add((layout.vertex_buffer_count as usize).wrapping_mul(runtime::VERTEX_STRIDE))
        .ok_or(Error::Malformed("runtime block overflows"))?;
    if index_array
        .checked_add((layout.index_buffer_count as usize).wrapping_mul(runtime::INDEX_STRIDE))
        .is_none_or(|end| end > block.len())
    {
        return Err(Error::Malformed("runtime block is shorter than its arrays"));
    }

    // ---- header -------------------------------------------------------
    // `*param_3 = 0` is the block's first qword.
    block[0x08..0x0c].copy_from_slice(&layout.vertex_count.to_le_bytes());
    block[0x0c..0x10].copy_from_slice(&layout.submesh_count.to_le_bytes());
    block[0x10..0x12].copy_from_slice(&layout.material_count.to_le_bytes());
    block[0x12] = layout.vertex_buffer_count;
    block[0x13] = layout.index_buffer_count;
    block[0x14..0x18].copy_from_slice(&layout.unknown_24.to_le_bytes());
    for (i, source) in layout.blocks.iter().enumerate() {
        let at = 0x18 + i * 0x1c;
        block[at..at + 0x1c].copy_from_slice(source);
    }
    block[runtime::SUBMESH_ARRAY..runtime::SUBMESH_ARRAY + 8]
        .copy_from_slice(&(dst_base.wrapping_add(submesh_array as u64)).to_le_bytes());
    block[runtime::VERTEX_ARRAY..runtime::VERTEX_ARRAY + 8]
        .copy_from_slice(&(dst_base.wrapping_add(vertex_array as u64)).to_le_bytes());
    block[runtime::INDEX_ARRAY..runtime::INDEX_ARRAY + 8]
        .copy_from_slice(&(dst_base.wrapping_add(index_array as u64)).to_le_bytes());

    // ---- submeshes ----------------------------------------------------
    let submeshes = offset_of(u64_at(payload, header::SUBMESHES)?, payload_base)?;
    let mut used_mask = 0u64;
    for i in 0..layout.submesh_count as usize {
        let s = submeshes
            .checked_add(i.wrapping_mul(submesh::STRIDE))
            .ok_or(Error::Malformed("submesh runs past the end"))?;
        if payload.get(s..s + submesh::STRIDE).is_none() {
            return Err(Error::Malformed("submesh runs past the end"));
        }
        let d = submesh_array + i * runtime::SUBMESH_STRIDE;
        let out = &mut block[d..d + runtime::SUBMESH_STRIDE];
        let src = &payload[s..s + submesh::STRIDE];
        out[0x00..0x0c].copy_from_slice(&src[0x00..0x0c]);
        out[0x10..0x14].copy_from_slice(&src[0x0c..0x10]);
        out[0x20..0x2c].copy_from_slice(&src[0x10..0x1c]);
        out[0x30..0x3c].copy_from_slice(&src[0x1c..0x28]);
        out[0x40..0x50].copy_from_slice(&src[0x28..0x38]);
        out[0x50..0x54].copy_from_slice(&src[0x38..0x3c]);
        out[0x54..0x58].copy_from_slice(&src[0x3c..0x40]);
        out[0x58..0x78].copy_from_slice(&src[0x40..0x60]);
        out[0x78..0x7c].copy_from_slice(&src[0x60..0x64]);
        out[0x7c..0x80].copy_from_slice(&src[0x64..0x68]);
        out[0x80..0x82].copy_from_slice(&src[0x68..0x6a]);
        out[0x82..0x84].copy_from_slice(&src[0x6a..0x6c]);
        out[0x84..0x86].copy_from_slice(&src[0x7c..0x7e]);
        out[0x86..0x88].copy_from_slice(&src[0x6c..0x6e]);
        out[0x88..0x90].copy_from_slice(&src[0x6e..0x76]);
        out[0x90] = src[0x76];
        out[0x91] = src[0x77];
        out[0x92] = src[0x78];
        out[0x93] = src[0x79];
        out[0x94] = src[0x7a];
        out[0x95] = if src[0x7b] == 0 {
            default_byte
        } else {
            src[0x7b]
        };
        // The client ORs the 64-bit value at submesh `+0x28` into the block's
        // first qword, unless it is -1 (which would poison the mask).
        let mask = u64::from_le_bytes(src[0x28..0x30].try_into().unwrap());
        if mask != u64::MAX {
            used_mask |= mask;
        }
    }
    block[0x00..0x08].copy_from_slice(&used_mask.to_le_bytes());

    // ---- vertex buffers -----------------------------------------------
    let vertex_buffers = offset_of(u64_at(payload, header::VERTEX_BUFFERS)?, payload_base)?;
    for i in 0..layout.vertex_buffer_count as usize {
        let s = vertex_buffers
            .checked_add(i.wrapping_mul(vertex_buffer::STRIDE))
            .ok_or(Error::Malformed("vertex buffer runs past the end"))?;
        let src = payload
            .get(s..s + vertex_buffer::STRIDE)
            .ok_or(Error::Malformed("vertex buffer runs past the end"))?;
        let d = vertex_array + i * runtime::VERTEX_STRIDE;
        let out = &mut block[d..d + runtime::VERTEX_STRIDE];
        out[0x00..0x10].fill(0xff); // both links start at -1
        out[0x10..0x14].copy_from_slice(&src[0x00..0x04]);
        out[0x14..0x18].copy_from_slice(&src[0x04..0x08]);
        out[0x18] = src[vertex_buffer::STRIDE_STREAM0];
        out[0x19] = src[vertex_buffer::STRIDE_STREAM1];
        out[0x1a] = src[vertex_buffer::ELEMENT_COUNT];
        out[0x1b] = src[vertex_buffer::FLAGS];
        out[0xe8..0xf8].fill(0);

        let elements = offset_of(u64_at(src, vertex_buffer::ELEMENTS)?, payload_base)?;
        let count = src[vertex_buffer::ELEMENT_COUNT] as usize;
        let bytes = count
            .checked_mul(8)
            .ok_or(Error::Malformed("element array overflows"))?;
        let source = payload
            .get(elements..elements + bytes)
            .ok_or(Error::Malformed("element array runs past the end"))?;
        out[runtime::vertex_buffer::ELEMENTS..runtime::vertex_buffer::ELEMENTS + bytes]
            .copy_from_slice(source);

        if src[vertex_buffer::FLAGS] & 0x10 == 0 {
            // The streams are payload offsets and stay in the source
            // descriptor: the client zeroes the runtime slot instead of
            // copying them. Only a separate vertex-data asset leaves its two
            // GUIDs here.
            out[0xa0..0xb0].fill(0);
        } else {
            out[0xa0..0xa8].copy_from_slice(&src[0x50..0x58]);
            out[0xa8..0xb0].copy_from_slice(&src[0x58..0x60]);
        }
        out[0xb0..0xe8].copy_from_slice(&src[0x18..0x50]);
    }

    // ---- index buffers ------------------------------------------------
    let index_buffers = offset_of(u64_at(payload, header::INDEX_BUFFERS)?, payload_base)?;
    for i in 0..layout.index_buffer_count as usize {
        let s = index_buffers
            .checked_add(i.wrapping_mul(index_buffer::STRIDE))
            .ok_or(Error::Malformed("index buffer runs past the end"))?;
        let src = payload
            .get(s..s + index_buffer::STRIDE)
            .ok_or(Error::Malformed("index buffer runs past the end"))?;
        let d = index_array + i * runtime::INDEX_STRIDE;
        let out = &mut block[d..d + runtime::INDEX_STRIDE];
        out[0x00..0x08].fill(0xff); // link starts at -1
        out[0x08..0x0c].copy_from_slice(&src[0x00..0x04]);
        out[0x10..0x18].fill(0);
    }

    Ok(Loaded {
        block,
        layout,
        used_mask,
        submesh_array,
        vertex_array,
        index_array,
    })
}

/// Read a whole model payload into a runtime block, fixing it up first, as the
/// client's dispatcher does before it calls the load.
pub fn load_payload(payload: &[u8], default_byte: u8) -> Result<Loaded> {
    let mut fixed = payload.to_vec();
    fixup(&mut fixed, 0)?;
    load(&fixed, default_byte, 0, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One vertex buffer, one index buffer and one submesh, laid out at fixed
    /// offsets with an element array and two stream pointers. No game bytes.
    fn synth() -> Vec<u8> {
        const VBO: usize = 0x100;
        const IBO: usize = 0x200;
        const SUBMESH: usize = 0x300;
        const ELEMENTS: usize = 0x400;
        const STREAM0: usize = 0x500;
        const STREAM1: usize = 0x510;
        const DATA: usize = 0x600;
        let mut d = vec![0u8; 0x700];
        let q =
            |d: &mut Vec<u8>, at: usize, v: u64| d[at..at + 8].copy_from_slice(&v.to_le_bytes());
        q(&mut d, header::VERTEX_BUFFERS, VBO as u64);
        q(&mut d, header::INDEX_BUFFERS, IBO as u64);
        q(&mut d, header::SUBMESHES, SUBMESH as u64);
        d[header::VERTEX_COUNT..header::VERTEX_COUNT + 4].copy_from_slice(&7u32.to_le_bytes());
        d[header::SUBMESH_COUNT..header::SUBMESH_COUNT + 4].copy_from_slice(&1u32.to_le_bytes());
        d[header::MATERIAL_COUNT..header::MATERIAL_COUNT + 2].copy_from_slice(&3u16.to_le_bytes());
        d[header::VERTEX_BUFFER_COUNT] = 1;
        d[header::INDEX_BUFFER_COUNT] = 1;
        d[header::UNKNOWN_24..header::UNKNOWN_24 + 4].copy_from_slice(&0x1234u32.to_le_bytes());
        for (i, value) in d[0x28..0x28 + 0x1c].iter_mut().enumerate() {
            *value = i as u8;
        }

        // Vertex buffer: 2 elements, stream 0 inside the chunk.
        d[VBO..VBO + 4].copy_from_slice(&11u32.to_le_bytes());
        d[VBO + 4..VBO + 8].copy_from_slice(&22u32.to_le_bytes());
        d[VBO + vertex_buffer::STRIDE_STREAM0] = 24;
        d[VBO + vertex_buffer::STRIDE_STREAM1] = 0;
        d[VBO + vertex_buffer::ELEMENT_COUNT] = 2;
        d[VBO + vertex_buffer::FLAGS] = 0;
        q(&mut d, VBO + vertex_buffer::ELEMENTS, ELEMENTS as u64);
        q(&mut d, VBO + vertex_buffer::STREAM0, STREAM0 as u64);
        q(&mut d, VBO + vertex_buffer::STREAM1, STREAM1 as u64);
        for k in 0..7 {
            q(&mut d, VBO + 0x18 + k * 8, 0x80 + k as u64);
        }
        for k in 0..2 {
            d[ELEMENTS + k * 8] = k as u8;
            d[ELEMENTS + k * 8 + 2] = 2;
            d[ELEMENTS + k * 8 + 4..ELEMENTS + k * 8 + 8]
                .copy_from_slice(&((k as u32) << 12).to_le_bytes());
        }

        // Index buffer.
        d[IBO..IBO + 4].copy_from_slice(&33u32.to_le_bytes());
        d[IBO + 4..IBO + 8].copy_from_slice(&1u32.to_le_bytes());
        q(&mut d, IBO + index_buffer::DATA, DATA as u64);

        // Submesh: every field the copy distinguishes.
        d[SUBMESH..SUBMESH + 4].copy_from_slice(&1u32.to_le_bytes());
        d[SUBMESH + 4..SUBMESH + 8].copy_from_slice(&2u32.to_le_bytes());
        d[SUBMESH + 8..SUBMESH + 12].copy_from_slice(&3u32.to_le_bytes());
        d[SUBMESH + 0x0c..SUBMESH + 0x10].copy_from_slice(&4u32.to_le_bytes());
        d[SUBMESH + 0x10..SUBMESH + 0x14].copy_from_slice(&5u32.to_le_bytes());
        d[SUBMESH + 0x28..SUBMESH + 0x30].copy_from_slice(&0x05u64.to_le_bytes());
        d[SUBMESH + 0x38..SUBMESH + 0x3c].copy_from_slice(&0x38u32.to_le_bytes());
        d[SUBMESH + submesh::VERTEX_START..SUBMESH + submesh::VERTEX_START + 4]
            .copy_from_slice(&0x3cu32.to_le_bytes());
        for k in 0..8 {
            d[SUBMESH + submesh::INDEX_START + k * 4..SUBMESH + submesh::INDEX_START + k * 4 + 4]
                .copy_from_slice(&(0x40 + k as u32).to_le_bytes());
        }
        d[SUBMESH + submesh::INDEX_COUNT..SUBMESH + submesh::INDEX_COUNT + 4]
            .copy_from_slice(&0x60u32.to_le_bytes());
        d[SUBMESH + submesh::INDICES_TO_DRAW..SUBMESH + submesh::INDICES_TO_DRAW + 4]
            .copy_from_slice(&0x64u32.to_le_bytes());
        d[SUBMESH + submesh::VERTICES_TO_DRAW..SUBMESH + submesh::VERTICES_TO_DRAW + 2]
            .copy_from_slice(&0x68u16.to_le_bytes());
        d[SUBMESH + submesh::BONE_ID_OFFSET..SUBMESH + submesh::BONE_ID_OFFSET + 2]
            .copy_from_slice(&0x6au16.to_le_bytes());
        d[SUBMESH + submesh::FLAGS..SUBMESH + submesh::FLAGS + 2]
            .copy_from_slice(&0x6cu16.to_le_bytes());
        for k in 0..8 {
            d[SUBMESH + submesh::INDEX_BUFFER + k] = 0x6e + k as u8;
        }
        d[SUBMESH + submesh::VERTEX_BUFFER] = 0x76;
        d[SUBMESH + submesh::MATERIAL] = 0x77;
        d[SUBMESH + submesh::LOD] = 0xff;
        d[SUBMESH + submesh::UNKNOWN_79] = 0x79;
        d[SUBMESH + 0x7a] = 0x7a;
        d[SUBMESH + submesh::DEFAULTED] = 0; // takes the model default
        d[SUBMESH + submesh::UNKNOWN_7C] = 0x7c;
        d[SUBMESH + 0x7d] = 0x7d;
        d
    }

    #[test]
    fn the_runtime_size_is_the_client_formula() {
        let d = synth();
        assert_eq!(runtime_size(&d).unwrap(), 0xa0 + 0xc0 + 0xf8 + 0x18);
    }

    #[test]
    fn the_fixup_rebases_exactly_the_pointer_slots() {
        let mut d = synth();
        let before = d.clone();
        fixup(&mut d, 0x1000).unwrap();
        // The header, the element array and both streams, the index data.
        assert_eq!(u64_at(&d, 0).unwrap(), 0x1100);
        assert_eq!(u64_at(&d, 8).unwrap(), 0x1200);
        assert_eq!(u64_at(&d, 0x10).unwrap(), 0x1300);
        assert_eq!(u64_at(&d, 0x100 + 0x10).unwrap(), 0x1400);
        assert_eq!(u64_at(&d, 0x100 + 0x50).unwrap(), 0x1500);
        assert_eq!(u64_at(&d, 0x100 + 0x58).unwrap(), 0x1510);
        assert_eq!(u64_at(&d, 0x200 + 8).unwrap(), 0x1600);
        // Everything else is untouched, including the submesh 32-byte block.
        for (i, (a, b)) in d.iter().zip(before.iter()).enumerate() {
            if !matches!(i, 0..=0x17 | 0x110..=0x117 | 0x150..=0x15f | 0x208..=0x20f) {
                assert_eq!(a, b, "byte {i:#x} changed");
            }
        }
        // The submesh at 0x300..0x380 was not touched at all.
        assert_eq!(d[0x300..0x380], before[0x300..0x380]);
    }

    #[test]
    fn a_separate_asset_keeps_its_stream_guids() {
        let mut d = synth();
        d[0x100 + vertex_buffer::FLAGS] = 0x10;
        let guid0 = u64_at(&d, 0x150).unwrap();
        let guid1 = u64_at(&d, 0x158).unwrap();
        fixup(&mut d, 0x1000).unwrap();
        assert_eq!(
            u64_at(&d, 0x150).unwrap(),
            guid0,
            "guid must not be rebased"
        );
        assert_eq!(
            u64_at(&d, 0x158).unwrap(),
            guid1,
            "guid must not be rebased"
        );
    }

    #[test]
    fn the_load_copies_the_header_and_every_descriptor() {
        let d = synth();
        let loaded = load_payload(&d, 0xaa).unwrap();
        let b = loaded.block();
        assert_eq!(&b[0x00..0x08], &0x05u64.to_le_bytes(), "used mask");
        assert_eq!(&b[0x08..0x0c], &7u32.to_le_bytes());
        assert_eq!(&b[0x0c..0x10], &1u32.to_le_bytes());
        assert_eq!(&b[0x10..0x12], &3u16.to_le_bytes());
        assert_eq!(b[0x12], 1);
        assert_eq!(b[0x13], 1);
        assert_eq!(&b[0x14..0x18], &0x1234u32.to_le_bytes());
        assert_eq!(&b[0x18..0x34], &d[0x28..0x44]);
        // The three array pointers are block offsets with base 0.
        assert_eq!(loaded.submesh_array, 0xc0);
        assert_eq!(loaded.vertex_array, 0xc0 + 0xa0);
        assert_eq!(loaded.index_array, 0xc0 + 0xa0 + 0xf8);

        let s = loaded.submesh(0).unwrap();
        assert_eq!(&s[0x00..0x0c], &d[0x300..0x30c]);
        assert_eq!(&s[0x50..0x58], &d[0x338..0x340]);
        assert_eq!(&s[0x54..0x58], &0x3cu32.to_le_bytes());
        assert_eq!(&s[0x58..0x78], &d[0x340..0x360]);
        assert_eq!(&s[0x7c..0x80], &0x64u32.to_le_bytes());
        assert_eq!(&s[0x80..0x82], &0x68u16.to_le_bytes());
        assert_eq!(&s[0x86..0x88], &0x6cu16.to_le_bytes());
        assert_eq!(
            &s[0x88..0x90],
            &[0x6e, 0x6f, 0x70, 0x71, 0x72, 0x73, 0x74, 0x75]
        );
        assert_eq!(s[0x90], 0x76);
        assert_eq!(s[0x91], 0x77);
        assert_eq!(s[0x92], 0xff);
        assert_eq!(s[0x95], 0xaa, "zero byte takes the model default");

        let v = loaded.vertex_buffer(0).unwrap();
        assert_eq!(&v[0x00..0x08], &[0xff; 8]);
        assert_eq!(&v[0x08..0x10], &[0xff; 8]);
        assert_eq!(&v[0x10..0x14], &11u32.to_le_bytes());
        assert_eq!(v[0x18], 24);
        assert_eq!(v[0x1a], 2);
        assert_eq!(&v[0x1c..0x2c], &d[0x400..0x410]);
        assert_eq!(&v[0xa0..0xb0], &[0u8; 16], "in-chunk streams are zeroed");
        assert_eq!(&v[0xb0..0xb8], &0x80u64.to_le_bytes());

        let i = loaded.index_buffer(0).unwrap();
        assert_eq!(&i[0x00..0x08], &[0xff; 8]);
        assert_eq!(&i[0x08..0x0c], &33u32.to_le_bytes());
        assert_eq!(&i[0x10..0x18], &[0u8; 8]);
    }

    #[test]
    fn a_separate_asset_lands_its_guids_in_the_record() {
        let mut d = synth();
        d[0x100 + vertex_buffer::FLAGS] = 0x10;
        let guid0 = u64::from_le_bytes(d[0x150..0x158].try_into().unwrap());
        let guid1 = u64::from_le_bytes(d[0x158..0x160].try_into().unwrap());
        let loaded = load_payload(&d, 0).unwrap();
        let v = loaded.vertex_buffer(0).unwrap();
        assert_eq!(&v[0xa0..0xa8], &guid0.to_le_bytes());
        assert_eq!(&v[0xa8..0xb0], &guid1.to_le_bytes());
    }

    #[test]
    fn a_negative_one_mask_is_skipped() {
        let mut d = synth();
        d[0x300 + 0x28..0x300 + 0x30].copy_from_slice(&u64::MAX.to_le_bytes());
        let loaded = load_payload(&d, 0).unwrap();
        assert_eq!(loaded.used_mask, 0);
    }

    #[test]
    fn a_null_array_pointer_is_rejected_not_followed() {
        let mut d = synth();
        d[header::SUBMESHES..header::SUBMESHES + 8].fill(0);
        assert!(load_payload(&d, 0).is_err());
    }
}

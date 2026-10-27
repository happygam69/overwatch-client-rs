//! Textures: the last hop of the model → material → texture chain.
//!
//! A texture asset (type `004`) starts with a 32-byte header and then either an
//! *embedded* payload or the raw data:
//!
//! ```text
//! +0x00  i16 flags        +0x02 u8 mip_count    +0x03 u8 format (DXGI-ish)
//! +0x04  u8  surfaces     +0x05 u8 usage        +0x06 u8 payload_count
//! +0x07  u8  unknown      +0x08 u16 width       +0x0a u16 height
//! +0x0c  u16 data_size    +0x0e u16 unknown     +0x10 u64 unknown (two)
//! ```
//!
//! When `data_size == 0` or `payload_count > 0`, a payload sits at `+0x20` with
//! its own header (`mips`, `surfaces`, `image_size`, `header_size`, four `u32`)
//! and the compressed blocks after `header_size`. Otherwise `data_size` bytes of
//! blocks follow the 32-byte header. Formats are block compressed and named by
//! DXGI id: 71/72 BC1, 74/75 BC2, 77/78 BC3, 80/81 BC4, 83/84 BC5, 98/99 BC7;
//! the client stores one value higher from 26 up, so the byte is decremented
//! first (the "ow2 hack" the TankLib reference records).
//!
//! Decoding is ours: BC1/BC2/BC3/BC4/BC5/BC6H/BC7 and the uncompressed families
//! are implemented here, with synthetic block tests.
//!
//! BC7's eight modes, its 64 2-subset and 64 3-subset partition patterns and
//! the anchor tables are tables, not derivable code: they are the ones the BC7
//! specification defines, taken from the public-domain `bc7decomp` reference
//! (Rich Geldreich) and re-checked against an independent decoder — see
//! `docs/formats/materials.md` § "BC7".
//!
//! BC6H is the high-dynamic-range member of the family (skyboxes, light
//! probes): fourteen modes, half-float output, and endpoint values that are
//! usually stored as low-precision *deltas* against one high-precision base.
//! Its tables come from Microsoft's published "BC6H Format" description and
//! the Khronos `EXT_texture_compression_bptc` extension (both public
//! specifications), re-derived from those sources plus `bcdec` by
//! `dev/bc6h_tables.py`, and the decode is compared **half float for half
//! float** against `bcdec` over synthetic blocks covering all fourteen modes in
//! both variants and over 21 real install assets — 0 differing channels in
//! every sweep. `texture2ddecoder` is the second opinion but a weak one (8-bit
//! clamped, and it saturates on a few real pixels), so it is reported rather
//! than trusted; see `docs/formats/materials.md` § "BC6H". The partition
//! table it uses is the first half of BC7's; the note above `BC6H_MODES`
//! explains why sharing that is safe.

use std::fmt;

#[derive(Debug)]
pub enum Error {
    Truncated(&'static str),
    UnsupportedFormat(u8),
    NoData,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Truncated(w) => write!(f, "truncated texture: {w}"),
            Error::UnsupportedFormat(id) => write!(f, "unsupported block format {id}"),
            Error::NoData => write!(f, "texture carries no payload data"),
        }
    }
}

impl std::error::Error for Error {}

type Result<T> = std::result::Result<T, Error>;

/// The block-compression family, from the header's format byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Bc1,
    Bc2,
    Bc3,
    Bc4,
    Bc5,
    /// BC7, 8-bit RGBA (DXGI 97/98/99).
    Bc7,
    /// BC6H, high-dynamic-range RGB stored as half floats.
    ///
    /// DXGI 94 is `BC6H_TYPELESS`, 95 is `BC6H_UF16` (unsigned endpoints) and
    /// 96 is `BC6H_SF16` (signed endpoints) — that is the standard enum's
    /// numbering, and 94 has no signedness of its own, so this variant carries
    /// the one thing the header cannot say. The two variants decode the same
    /// fourteen modes; the flag says whether an endpoint carries a sign bit,
    /// which changes the arithmetic, not the layout.
    Bc6h {
        signed: bool,
    },
    /// R8G8B8A8, uncompressed (DXGI 27/28/29).
    Rgba8,
    /// R16G16B16A16_FLOAT (DXGI 10): four half floats, 8 B/px.
    Rgba16Float,
    /// R16_FLOAT (DXGI 48/54): one half float, 2 B/px.
    R16Float,
    /// R16_UNORM (DXGI 50): one 16-bit unorm, 2 B/px.
    R16Unorm,
    Unknown(u8),
}

impl Default for Format {
    fn default() -> Self {
        Format::Unknown(0)
    }
}

impl Format {
    /// DXGI id → family, undoing the client's +1 offset from 26 up.
    pub fn from_header(byte: u8) -> (Format, u8) {
        let id = if byte >= 0x1a { byte - 1 } else { byte };
        let family = match id {
            70..=72 => Format::Bc1,
            73..=75 => Format::Bc2,
            76..=78 => Format::Bc3,
            79..=81 => Format::Bc4,
            82..=84 => Format::Bc5,
            97..=99 => Format::Bc7,
            // The two ids BC6H owns in the standard enum are 95 (`BC6H_UF16`,
            // unsigned) and 96 (`BC6H_SF16`, signed); 94 is `BC6H_TYPELESS`,
            // which the header alone cannot resolve to one of the two variants,
            // so it is decoded as the unsigned one and that is recorded as a
            // choice, not a reading. Note the arms are on `id`, the byte after
            // the client's -1, so the *stored* bytes that land here are 95 and
            // 96 (typeless and unsigned) and 97 (signed).
            //
            // Evidence, measured on this install (evidence class *install*):
            // a 600-asset spread census (`ow-tank-tex --census 600`) has 51
            // BC6H assets and every one of them stores byte 96; none stores
            // 97. So under the -1 rule they are all `BC6H_UF16`. The other
            // candidate reading — that the stored 96 means DXGI 96 and the
            // offset does not apply to BC6H — would make them `SF16`, and
            // that is refuted by the pixels: decoded as UF16, 21 of the assets
            // (2,319,360 channels over 36,240 blocks, all fourteen modes
            // present) contain *no* negative value, their means sit at 0.26 to
            // 0.68 and their largest value at 0.79 to 3.19, and each mode's
            // pixels differ from their right-hand neighbour by 0.0002 to 0.08
            // — a smooth image. Decoded as SF16 the same bytes give means of
            // 7 to 6260, values reaching the largest finite half, negative
            // values in up to 49% of a mode's channels, and a neighbour
            // difference 3 to 5 orders of magnitude larger. See
            // `docs/formats/materials.md` § "BC6H".
            94 | 95 => Format::Bc6h { signed: false },
            96 => Format::Bc6h { signed: true },
            27..=29 => Format::Rgba8,
            10 => Format::Rgba16Float,
            // DXGI 48 is the typeless-free `R16_FLOAT`; 54 is `R16_FLOAT` one
            // block later (the ids the standard enum puts it at, and the one
            // the client's own +1 can land on for either).
            48 | 54 => Format::R16Float,
            50 => Format::R16Unorm,
            other => Format::Unknown(other),
        };
        (family, id)
    }

    pub fn block_bytes(self) -> usize {
        match self {
            Format::Bc1 | Format::Bc4 => 8,
            _ => 16,
        }
    }

    /// Whether the format stores one channel, which lands in red with the other
    /// channels zero (the D3D convention, and what the hardware hands a
    /// shader). A viewer that wants a picture promotes it; the decode does not
    /// invent green and blue.
    pub fn single_channel(self) -> bool {
        matches!(self, Format::Bc4 | Format::R16Float | Format::R16Unorm)
    }
}

/// A decoded texture: mip 0 as 8-bit RGBA.
#[derive(Debug, Clone, Default)]
pub struct Texture {
    pub width: u16,
    pub height: u16,
    pub mip_count: u8,
    pub surfaces: u8,
    pub format: Format,
    /// The raw format byte, as stored.
    pub format_byte: u8,
    /// Mip 0 as 8-bit RGBA. For a format with no byte form (BC6H, the float
    /// families) this is the clamped view described on [`half_to_unorm8`]:
    /// enough to draw, not enough to measure. The values themselves are in
    /// [`Texture::rgba16`].
    pub rgba: Vec<u8>,
    /// Mip 0 as half floats, four per pixel (`r, g, b, a`), for the formats
    /// that produce one. Empty for every 8-bit format: a BC7 or BC1 pixel has
    /// no second form, and inventing one would invite a reader to measure a
    /// clamped byte and call it the data.
    pub rgba16: Vec<u16>,
    /// Where the decoded blocks actually came from. A texture can carry them
    /// inside its own bytes *or* in one of its payload assets, and the two are
    /// not interchangeable (measured: hero texture `0x0c00000000038c3d` embeds
    /// a descriptor table at `+0x20` and keeps mip 0 in payload 1) — a reader
    /// that assumes the embedded one reads the wrong bytes.
    pub source: BlockSource,
    /// Offset of the block run inside its source.
    pub block_offset: usize,
    /// The exact level-0 bytes the decode used.
    pub blocks: Vec<u8>,
}

/// Which asset the decoded blocks came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BlockSource {
    #[default]
    Asset,
    /// One of the payload assets (`guid::payload_of`).
    Payload(u32),
}

impl Texture {
    /// Serialise the decoded texture to a DDS byte stream (level 0, one surface).
    pub fn to_dds(&self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        out.extend_from_slice(b"DDS ");
        let mut hdr = [0u8; 124];
        hdr[0..4].copy_from_slice(&124u32.to_le_bytes());
        let mut flags = 0x1u32 | 0x2 | 0x4 | 0x1000;
        let fourcc: u32;
        let dxgi: Option<u32>;
        let pitch: u32;
        let is_bc = matches!(
            self.format,
            Format::Bc1
                | Format::Bc2
                | Format::Bc3
                | Format::Bc4
                | Format::Bc5
                | Format::Bc7
                | Format::Bc6h { .. }
        );
        if matches!(
            self.format,
            Format::Bc1 | Format::Bc2 | Format::Bc3 | Format::Bc4 | Format::Bc5
        ) {
            flags |= 0x80000;
            fourcc = match self.format {
                Format::Bc1 => u32::from_le_bytes(*b"DXT1"),
                Format::Bc2 => u32::from_le_bytes(*b"DXT3"),
                Format::Bc3 => u32::from_le_bytes(*b"DXT5"),
                Format::Bc4 => u32::from_le_bytes(*b"ATI1"),
                Format::Bc5 => u32::from_le_bytes(*b"ATI2"),
                _ => 0,
            };
            dxgi = None;
            let bw = (self.width as usize).div_ceil(4);
            let bh = (self.height as usize).div_ceil(4);
            pitch = (bw * bh * self.format.block_bytes()) as u32;
        } else if matches!(self.format, Format::Bc7 | Format::Bc6h { .. }) {
            // A DX10 header, because the DXGI number is what names BC6H and
            // BC7: fourCC "DX10" plus a 20-byte block that starts with it.
            flags |= 0x80000;
            fourcc = u32::from_le_bytes(*b"DX10");
            // The client's stored byte is one higher from 26 up, so undo it to
            // get the id a DDS reader expects: 98/99 for BC7, 95/96 for BC6H
            // (95 = unsigned float, 96 = signed float).
            dxgi = Some(match self.format {
                Format::Bc7 => {
                    if self.format_byte >= 0x1a {
                        99
                    } else {
                        98
                    }
                }
                // The DXGI id, which is what the DX10 block wants — *not* the
                // client's stored byte, which is one higher from 26 up. Asking
                // `from_header` keeps the two in step: 94 (typeless), 95
                // (unsigned float) or 96 (signed float).
                Format::Bc6h { .. } => Format::from_header(self.format_byte).1 as u32,
                _ => 0,
            });
            let bw = (self.width as usize).div_ceil(4);
            let bh = (self.height as usize).div_ceil(4);
            pitch = (bw * bh * 16) as u32;
        } else {
            flags |= 0x8;
            fourcc = 0;
            dxgi = None;
            pitch = self.width as u32 * 4;
        }
        hdr[4..8].copy_from_slice(&flags.to_le_bytes());
        hdr[8..12].copy_from_slice(&(self.height as u32).to_le_bytes());
        hdr[12..16].copy_from_slice(&(self.width as u32).to_le_bytes());
        hdr[16..20].copy_from_slice(&pitch.to_le_bytes());
        hdr[28..32].copy_from_slice(&1u32.to_le_bytes());
        let pf_off = 76usize;
        hdr[pf_off..pf_off + 4].copy_from_slice(&32u32.to_le_bytes());
        if is_bc || matches!(self.format, Format::Bc7 | Format::Bc6h { .. }) {
            hdr[pf_off + 4..pf_off + 8].copy_from_slice(&0x4u32.to_le_bytes());
            hdr[pf_off + 8..pf_off + 12].copy_from_slice(&fourcc.to_le_bytes());
        } else {
            let flags2 = 0x40u32 | 0x1;
            hdr[pf_off + 4..pf_off + 8].copy_from_slice(&flags2.to_le_bytes());
            hdr[pf_off + 12..pf_off + 16].copy_from_slice(&32u32.to_le_bytes());
            hdr[pf_off + 16..pf_off + 20].copy_from_slice(&0x00FF0000u32.to_le_bytes());
            hdr[pf_off + 20..pf_off + 24].copy_from_slice(&0x0000FF00u32.to_le_bytes());
            hdr[pf_off + 24..pf_off + 28].copy_from_slice(&0x000000FFu32.to_le_bytes());
            hdr[pf_off + 28..pf_off + 32].copy_from_slice(&0xFF000000u32.to_le_bytes());
        }
        hdr[108..112].copy_from_slice(&0x1000u32.to_le_bytes());
        out.extend_from_slice(&hdr);
        if let Some(fmt) = dxgi {
            let mut dx = [0u8; 20];
            dx[0..4].copy_from_slice(&fmt.to_le_bytes());
            dx[4..8].copy_from_slice(&3u32.to_le_bytes());
            dx[12..16].copy_from_slice(&1u32.to_le_bytes());
            out.extend_from_slice(&dx);
        }
        if is_bc {
            if self.blocks.is_empty() {
                return Err(Error::NoData);
            }
            out.extend_from_slice(&self.blocks);
        } else {
            out.extend_from_slice(&self.rgba);
        }
        Ok(out)
    }

    pub fn pixel(&self, x: usize, y: usize) -> [u8; 4] {
        let at = (y * self.width as usize + x) * 4;
        let mut px = [0u8; 4];
        if at + 4 <= self.rgba.len() {
            px.copy_from_slice(&self.rgba[at..at + 4]);
        }
        px
    }
}

fn u16_at(d: &[u8], at: usize) -> Option<u16> {
    d.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
}
fn u32_at(d: &[u8], at: usize) -> Option<u32> {
    d.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Largest width/height accepted for a decode. Real assets top out well below
/// this; anything larger is a corrupt header, and allocating `w*h*4` for it
/// would exhaust memory before the block-length check runs.
const MAX_DIM: usize = 16384;

fn check_dims(w: usize, h: usize) -> Result<()> {
    if w == 0 || h == 0 || w > MAX_DIM || h > MAX_DIM {
        return Err(Error::Truncated("dimensions"));
    }
    Ok(())
}

fn pixel_bytes(w: usize, h: usize, bpp: usize) -> Result<usize> {
    w.checked_mul(h)
        .and_then(|v| v.checked_mul(bpp))
        .ok_or(Error::Truncated("dimensions"))
}

fn block_bytes_needed(w: usize, h: usize, per_block: usize) -> Result<usize> {
    let bw = w.div_ceil(4);
    let bh = h.div_ceil(4);
    bw.checked_mul(bh)
        .and_then(|v| v.checked_mul(per_block))
        .ok_or(Error::Truncated("dimensions"))
}

/// Where the compressed blocks start and how many bytes they cover.
pub fn block_span(d: &[u8]) -> Result<(usize, usize)> {
    if d.len() < 0x20 {
        return Err(Error::Truncated("header"));
    }
    let payload_count = d[6];
    let data_size = u16_at(d, 0x0c).unwrap_or(0) as usize;
    if data_size == 0 || payload_count > 0 {
        if d.len() < 0x30 {
            return Err(Error::Truncated("embedded payload header"));
        }
        let image_size = u32_at(d, 0x28).unwrap_or(0) as usize;
        let header_size = u32_at(d, 0x2c).unwrap_or(0x10) as usize;
        if image_size == 0 {
            return Err(Error::NoData);
        }
        Ok((0x20 + header_size, image_size))
    } else {
        Ok((0x20, data_size))
    }
}

/// Dimensions and family from a texture header alone.
pub fn header_dimensions(asset: &[u8]) -> Option<(u16, u16, Format)> {
    if asset.len() < 0x20 {
        return None;
    }
    let (format, _) = Format::from_header(asset[3]);
    Some((u16_at(asset, 8)?, u16_at(asset, 10)?, format))
}

/// Parse a texture asset, following the *separate* payload asset when the
/// embedded bytes do not hold level 0 (measured: an 8-mip 512x512 BC5 texture
/// embeds only mips 2..7 — 21840 B — and keeps levels 0..1 in payload 1; and a
/// 256x256 BC7 hero texture embeds a 16-byte descriptor table at `+0x20` while
/// its mip 0 lives in payload 1).
pub fn parse_asset(
    asset: &[u8],
    guid: u64,
    load: &dyn Fn(u64) -> Option<Vec<u8>>,
) -> Result<Texture> {
    match parse(asset) {
        Ok(t) => Ok(t),
        Err(Error::Truncated(_)) => {
            // A chain can be split over several payload assets (measured:
            // 1024x1024 BC5 with payloads 3 keeps the big levels in payload 1
            // *or* 2), so try them in order and keep the first whose block run
            // covers level 0.
            let level0 = header_dimensions(asset)
                .map(|(w, h, f)| level0_bytes(f, w as usize, h as usize))
                .unwrap_or(0);
            for idx in 1..=3u32 {
                let Some(bytes) = load(crate::guid::payload_of(guid, idx)) else {
                    continue;
                };
                // The payload asset is a four-u32 header then the blocks.
                let start = u32_at(&bytes, 12).unwrap_or(0x10) as usize;
                let image = u32_at(&bytes, 8).unwrap_or(0) as usize;
                if image == 0 || bytes.len() < start + image {
                    continue;
                }
                let run = &bytes[start..start + image];
                if level0 > 0 && run.len() < level0 {
                    continue;
                }
                if let Ok(t) = decode(asset, run) {
                    let mut t = t;
                    t.source = BlockSource::Payload(idx);
                    t.block_offset = start;
                    return Ok(t);
                }
            }
            Err(Error::NoData)
        }
        Err(e) => Err(e),
    }
}

/// Parse a texture asset and decode mip 0.
pub fn parse(asset: &[u8]) -> Result<Texture> {
    let (start, size) = block_span(asset)?;
    if start >= asset.len() {
        return Err(Error::Truncated("blocks"));
    }
    let end = start.checked_add(size).ok_or(Error::Truncated("blocks"))?;
    if end > asset.len() {
        return Err(Error::Truncated("blocks"));
    }
    let blocks = &asset[start..end];
    let mut t = decode(asset, blocks)?;
    t.source = BlockSource::Asset;
    t.block_offset = start;
    Ok(t)
}

/// Decode a texture from an explicit header and block run: the entry point for
/// a payload that came from a *separate* asset, where the header's own
/// `data_size` (u16) cannot describe the level (measured: a 512x512 BC5 level
/// is 262144 B).
pub fn decode(asset: &[u8], blocks_in: &[u8]) -> Result<Texture> {
    if asset.len() < 0x20 {
        return Err(Error::Truncated("header"));
    }
    let mip_count = asset[2];
    let format_byte = asset[3];
    let surfaces = asset[4];
    let width = u16_at(asset, 8).ok_or(Error::Truncated("width"))?;
    let height = u16_at(asset, 10).ok_or(Error::Truncated("height"))?;
    let (format, _id) = Format::from_header(format_byte);

    if width == 0 || height == 0 {
        return Err(Error::NoData);
    }
    let mut blocks = blocks_in;
    // The payload may hold the whole mip chain and store it *smallest first*,
    // in which case the level-0 blocks are at the end of the payload (measured:
    // 162 of 400 sampled textures read as truncated from the front).  Prefer
    // whichever end actually holds level 0.
    let level0 = level0_bytes(format, width as usize, height as usize);
    if level0 > 0 && blocks.len() >= level0 {
        // A block run larger than one level is a chain; level 0 is the first
        // run (the reference stores the chain largest-first inside a payload).
        blocks = &blocks[..level0];
    }

    let (rgba, rgba16) = match format {
        Format::Bc1 => (
            decode_bc1(blocks, width as usize, height as usize)?,
            Vec::new(),
        ),
        Format::Bc2 => (
            decode_bc2(blocks, width as usize, height as usize)?,
            Vec::new(),
        ),
        Format::Bc3 => (
            decode_bc3(blocks, width as usize, height as usize)?,
            Vec::new(),
        ),
        Format::Bc4 => (
            decode_bc4(blocks, width as usize, height as usize)?,
            Vec::new(),
        ),
        Format::Bc5 => (
            decode_bc5(blocks, width as usize, height as usize)?,
            Vec::new(),
        ),
        Format::Bc7 => (
            decode_bc7(blocks, width as usize, height as usize)?,
            Vec::new(),
        ),
        Format::Bc6h { signed } => decode_bc6h(blocks, width as usize, height as usize, signed)?,
        Format::Rgba8 => (
            decode_rgba8(blocks, width as usize, height as usize)?,
            Vec::new(),
        ),
        Format::Rgba16Float => (
            decode_rgba16f(blocks, width as usize, height as usize)?,
            Vec::new(),
        ),
        Format::R16Float => (
            decode_r16_float(blocks, width as usize, height as usize)?,
            Vec::new(),
        ),
        Format::R16Unorm => (
            decode_r16_unorm(blocks, width as usize, height as usize)?,
            Vec::new(),
        ),
        Format::Unknown(id) => return Err(Error::UnsupportedFormat(id)),
    };

    Ok(Texture {
        width,
        height,
        mip_count,
        surfaces,
        format,
        format_byte,
        rgba,
        rgba16,
        source: BlockSource::default(),
        block_offset: 0,
        blocks: blocks.to_vec(),
    })
}

/// Bytes of one level's block grid, or 0 for an unknown family.
pub fn level0_bytes(format: Format, w: usize, h: usize) -> usize {
    let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
    match format {
        Format::Bc1 | Format::Bc4 => bw * bh * 8,
        Format::Bc2 | Format::Bc3 | Format::Bc5 | Format::Bc7 | Format::Bc6h { .. } => bw * bh * 16,
        Format::Rgba8 => w * h * 4,
        Format::Rgba16Float => w * h * 8,
        Format::R16Float | Format::R16Unorm => w * h * 2,
        Format::Unknown(_) => 0,
    }
}

/// A half float as an 8-bit value: unorm semantics, clamped.
///
/// The colour side of a texture is authored in [0, 1]; HDR data (a BC6H
/// skybox, or an R16F bloom chain) has no byte form and clamps here rather
/// than being misinterpreted as a colour.
pub fn half_to_unorm8(h: u16) -> u8 {
    let v = half_to_f32(h);
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// IEEE 754 binary16 to binary32, shared with the vertex reader.
pub fn half_to_f32(h: u16) -> f32 {
    crate::model::half_to_f32(h)
}

fn decode_rgba8(blocks: &[u8], w: usize, h: usize) -> Result<Vec<u8>> {
    check_dims(w, h)?;
    let need = pixel_bytes(w, h, 4)?;
    if blocks.len() < need {
        return Err(Error::Truncated("rgba8 data"));
    }
    Ok(blocks[..need].to_vec())
}

/// R16G16B16A16_FLOAT: four half floats per pixel, 8 bytes.
fn decode_rgba16f(blocks: &[u8], w: usize, h: usize) -> Result<Vec<u8>> {
    check_dims(w, h)?;
    let need = pixel_bytes(w, h, 8)?;
    if blocks.len() < need {
        return Err(Error::Truncated("rgba16f data"));
    }
    let out_len = pixel_bytes(w, h, 4)?;
    let count = w.checked_mul(h).ok_or(Error::Truncated("dimensions"))?;
    let mut out = vec![0u8; out_len];
    for i in 0..count {
        for c in 0..4 {
            out[i * 4 + c] = half_to_unorm8(u16_at(blocks, i * 8 + c * 2).unwrap_or(0));
        }
    }
    Ok(out)
}

fn decode_r16_float(blocks: &[u8], w: usize, h: usize) -> Result<Vec<u8>> {
    check_dims(w, h)?;
    let need = pixel_bytes(w, h, 2)?;
    if blocks.len() < need {
        return Err(Error::Truncated("r16f data"));
    }
    let out_len = pixel_bytes(w, h, 4)?;
    let count = w.checked_mul(h).ok_or(Error::Truncated("dimensions"))?;
    let mut out = vec![0u8; out_len];
    for i in 0..count {
        out[i * 4] = half_to_unorm8(u16_at(blocks, i * 2).unwrap_or(0));
        out[i * 4 + 3] = 255;
    }
    Ok(out)
}

fn decode_r16_unorm(blocks: &[u8], w: usize, h: usize) -> Result<Vec<u8>> {
    check_dims(w, h)?;
    let need = pixel_bytes(w, h, 2)?;
    if blocks.len() < need {
        return Err(Error::Truncated("r16 data"));
    }
    let out_len = pixel_bytes(w, h, 4)?;
    let count = w.checked_mul(h).ok_or(Error::Truncated("dimensions"))?;
    let mut out = vec![0u8; out_len];
    for i in 0..count {
        let raw = u16_at(blocks, i * 2).unwrap_or(0) as u32;
        out[i * 4] = ((raw * 255 + 32767) / 65535) as u8;
        out[i * 4 + 3] = 255;
    }
    Ok(out)
}

/// A 565 colour as 8 bits per channel.
///
/// The expansion is **bit replication**, not a scale by 255/31: the hardware
/// repeats the high bits into the vacated ones, and both the D3D
/// specification and the independent decoder this was checked against do the
/// same. Rounding instead reads one level high on many values (r = 7 gives 58
/// where replication gives 57), which is exactly the kind of off-by-one that
/// makes a "looks right" decode wrong.
fn rgb565(c: u16) -> [u8; 3] {
    let r = ((c >> 11) & 0x1f) as u8;
    let g = ((c >> 5) & 0x3f) as u8;
    let b = (c & 0x1f) as u8;
    [
        (r << 3) | (r >> 2),
        (g << 2) | (g >> 4),
        (b << 3) | (b >> 2),
    ]
}

/// The four BC1 colours and, for the 3-colour mode, the transparent index.
fn bc1_palette(c0: u16, c1: u16) -> ([[u8; 4]; 4], bool) {
    let a = rgb565(c0);
    let b = rgb565(c1);
    let mut pal = [[0u8; 4]; 4];
    pal[0] = [a[0], a[1], a[2], 255];
    pal[1] = [b[0], b[1], b[2], 255];
    if c0 > c1 {
        for i in 0..3 {
            pal[2][i] = ((2 * a[i] as u32 + b[i] as u32) / 3) as u8;
            pal[3][i] = ((a[i] as u32 + 2 * b[i] as u32) / 3) as u8;
        }
        pal[2][3] = 255;
        pal[3][3] = 255;
        (pal, false)
    } else {
        for i in 0..3 {
            pal[2][i] = ((a[i] as u32 + b[i] as u32) / 2) as u8;
        }
        pal[2][3] = 255;
        pal[3] = [0, 0, 0, 0];
        (pal, true)
    }
}

fn decode_bc_colour(blocks: &[u8], w: usize, h: usize, at: usize) -> Result<Vec<u8>> {
    let c0 = u16_at(blocks, at).ok_or(Error::Truncated("bc colour block"))?;
    let c1 = u16_at(blocks, at + 2).ok_or(Error::Truncated("bc colour block"))?;
    let bits = u32_at(blocks, at + 4).ok_or(Error::Truncated("bc colour block"))?;
    let (pal, _) = bc1_palette(c0, c1);
    let mut out = vec![0u8; w * h * 4];
    for y in 0..4 {
        for x in 0..4 {
            let idx = ((bits >> (2 * (y * 4 + x))) & 0x3) as usize;
            let (px, py) = (x, y);
            if px < w && py < h {
                let o = (py * w + px) * 4;
                out[o..o + 4].copy_from_slice(&pal[idx]);
            }
        }
    }
    Ok(out)
}

fn decode_bc1(blocks: &[u8], w: usize, h: usize) -> Result<Vec<u8>> {
    check_dims(w, h)?;
    let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
    // A shipped asset can carry fewer bytes than its dimensions imply (a mip
    // chain whose level 0 is what we read, or a texture whose payload the
    // manifest truncated); refuse rather than read out of bounds.
    let need = block_bytes_needed(w, h, 8)?;
    if blocks.len() < need {
        return Err(Error::Truncated("block data"));
    }
    let mut out = vec![0u8; pixel_bytes(w, h, 4)?];
    for by in 0..bh {
        for bx in 0..bw {
            let at = (by * bw + bx) * 8;
            let tile = decode_bc_colour(blocks, 4, 4, at)?;
            for y in 0..4 {
                for x in 0..4 {
                    let (px, py) = (bx * 4 + x, by * 4 + y);
                    if px < w && py < h {
                        let src = (y * 4 + x) * 4;
                        let dst = (py * w + px) * 4;
                        out[dst..dst + 4].copy_from_slice(&tile[src..src + 4]);
                    }
                }
            }
        }
    }
    Ok(out)
}

/// BC4's single channel: two 8-bit endpoints and 16 three-bit indices.
fn decode_bc4_channel(blocks: &[u8], at: usize) -> [u8; 16] {
    debug_assert!(at + 8 <= blocks.len());
    let a0 = blocks[at] as u16;
    let a1 = blocks[at + 1] as u16;
    let mut table = [0u8; 8];
    table[0] = a0 as u8;
    table[1] = a1 as u8;
    if a0 > a1 {
        for i in 0..6 {
            table[2 + i] = (((6 - i) as u16 * a0 + (i as u16 + 1) * a1) / 7) as u8;
        }
    } else {
        for i in 0..4 {
            table[2 + i] = (((4 - i) as u16 * a0 + (i as u16 + 1) * a1) / 5) as u8;
        }
        table[6] = 0;
        table[7] = 255;
    }
    let mut bits = 0u64;
    for i in 0..6 {
        bits |= (blocks[at + 2 + i] as u64) << (8 * i);
    }
    let mut out = [0u8; 16];
    for i in 0..16 {
        out[i] = table[((bits >> (3 * i)) & 0x7) as usize];
    }
    out
}

fn decode_bc4(blocks: &[u8], w: usize, h: usize) -> Result<Vec<u8>> {
    check_dims(w, h)?;
    let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
    // A shipped asset can carry fewer bytes than its dimensions imply (a mip
    // chain whose level 0 is what we read, or a texture whose payload the
    // manifest truncated); refuse rather than read out of bounds.
    let need = block_bytes_needed(w, h, 8)?;
    if blocks.len() < need {
        return Err(Error::Truncated("block data"));
    }
    let mut out = vec![0u8; pixel_bytes(w, h, 4)?];
    for by in 0..bh {
        for bx in 0..bw {
            let at = (by * bw + bx) * 8;
            let r = decode_bc4_channel(blocks, at);
            for y in 0..4 {
                for x in 0..4 {
                    let (px, py) = (bx * 4 + x, by * 4 + y);
                    if px < w && py < h {
                        let dst = (py * w + px) * 4;
                        out[dst] = r[y * 4 + x];
                        out[dst + 3] = 255;
                    }
                }
            }
        }
    }
    Ok(out)
}

fn decode_bc5(blocks: &[u8], w: usize, h: usize) -> Result<Vec<u8>> {
    check_dims(w, h)?;
    let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
    // A shipped asset can carry fewer bytes than its dimensions imply (a mip
    // chain whose level 0 is what we read, or a texture whose payload the
    // manifest truncated); refuse rather than read out of bounds.
    let need = block_bytes_needed(w, h, 16)?;
    if blocks.len() < need {
        return Err(Error::Truncated("block data"));
    }
    let mut out = vec![0u8; pixel_bytes(w, h, 4)?];
    for by in 0..bh {
        for bx in 0..bw {
            let at = (by * bw + bx) * 16;
            let r = decode_bc4_channel(blocks, at);
            let g = decode_bc4_channel(blocks, at + 8);
            for y in 0..4 {
                for x in 0..4 {
                    let (px, py) = (bx * 4 + x, by * 4 + y);
                    if px < w && py < h {
                        let dst = (py * w + px) * 4;
                        let i = y * 4 + x;
                        out[dst] = r[i];
                        out[dst + 1] = g[i];
                        out[dst + 2] = 0;
                        out[dst + 3] = 255;
                    }
                }
            }
        }
    }
    Ok(out)
}

fn decode_bc2(blocks: &[u8], w: usize, h: usize) -> Result<Vec<u8>> {
    check_dims(w, h)?;
    let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
    // A shipped asset can carry fewer bytes than its dimensions imply (a mip
    // chain whose level 0 is what we read, or a texture whose payload the
    // manifest truncated); refuse rather than read out of bounds.
    let need = block_bytes_needed(w, h, 16)?;
    if blocks.len() < need {
        return Err(Error::Truncated("block data"));
    }
    let mut out = vec![0u8; pixel_bytes(w, h, 4)?];
    for by in 0..bh {
        for bx in 0..bw {
            let at = (by * bw + bx) * 16;
            let tile = decode_bc_colour(blocks, 4, 4, at + 8)?;
            for y in 0..4 {
                for x in 0..4 {
                    let (px, py) = (bx * 4 + x, by * 4 + y);
                    if px < w && py < h {
                        let src = (y * 4 + x) * 4;
                        let dst = (py * w + px) * 4;
                        out[dst..dst + 4].copy_from_slice(&tile[src..src + 4]);
                        // The alpha half: 4 bits per pixel, expanded to 0..255.
                        let byte = blocks[at + y * 2 + x / 2];
                        let nibble = if x % 2 == 0 { byte & 0xf } else { byte >> 4 };
                        out[dst + 3] = (nibble as u32 * 255 / 15) as u8;
                    }
                }
            }
        }
    }
    Ok(out)
}

fn decode_bc3(blocks: &[u8], w: usize, h: usize) -> Result<Vec<u8>> {
    check_dims(w, h)?;
    let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
    // A shipped asset can carry fewer bytes than its dimensions imply (a mip
    // chain whose level 0 is what we read, or a texture whose payload the
    // manifest truncated); refuse rather than read out of bounds.
    let need = block_bytes_needed(w, h, 16)?;
    if blocks.len() < need {
        return Err(Error::Truncated("block data"));
    }
    let mut out = vec![0u8; pixel_bytes(w, h, 4)?];
    for by in 0..bh {
        for bx in 0..bw {
            let at = (by * bw + bx) * 16;
            let alpha = decode_bc4_channel(blocks, at);
            let tile = decode_bc_colour(blocks, 4, 4, at + 8)?;
            for y in 0..4 {
                for x in 0..4 {
                    let (px, py) = (bx * 4 + x, by * 4 + y);
                    if px < w && py < h {
                        let src = (y * 4 + x) * 4;
                        let dst = (py * w + px) * 4;
                        out[dst..dst + 3].copy_from_slice(&tile[src..src + 3]);
                        out[dst + 3] = alpha[y * 4 + x];
                    }
                }
            }
        }
    }
    Ok(out)
}

fn decode_bc7(blocks: &[u8], w: usize, h: usize) -> Result<Vec<u8>> {
    check_dims(w, h)?;
    let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
    // A shipped asset can carry fewer bytes than its dimensions imply (a mip
    // chain whose level 0 is what we read, or a texture whose payload the
    // manifest truncated); refuse rather than read out of bounds.
    let need = block_bytes_needed(w, h, 16)?;
    if blocks.len() < need {
        return Err(Error::Truncated("block data"));
    }
    let mut out = vec![0u8; pixel_bytes(w, h, 4)?];
    let mut tile = [0u8; 64];
    for by in 0..bh {
        for bx in 0..bw {
            let at = (by * bw + bx) * 16;
            bc7_block(&blocks[at..at + 16], &mut tile);
            for y in 0..4 {
                for x in 0..4 {
                    let (px, py) = (bx * 4 + x, by * 4 + y);
                    if px < w && py < h {
                        let src = (y * 4 + x) * 4;
                        let dst = (py * w + px) * 4;
                        out[dst..dst + 4].copy_from_slice(&tile[src..src + 4]);
                    }
                }
            }
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------ BC7

/// 2-subset partition patterns as 16-bit masks: bit `i` is the subset of pixel
/// `i`. 3-subset patterns are 32-bit masks with two bits per pixel.
///
/// From the BC7 specification's tables (as distributed with the public-domain
/// `bc7decomp`). They cannot be derived — 64 patterns each, in a fixed order —
/// so this is a transcription, and the decoder is verified against an
/// independent implementation over real assets rather than by inspection.
#[rustfmt::skip]
static BC7_PARTITION_2: [u16; 64] = [
    0xcccc, 0x8888, 0xeeee, 0xecc8, 0xc880, 0xfeec, 0xfec8, 0xec80,
    0xc800, 0xffec, 0xfe80, 0xe800, 0xffe8, 0xff00, 0xfff0, 0xf000,
    0xf710, 0x008e, 0x7100, 0x08ce, 0x008c, 0x7310, 0x3100, 0x8cce,
    0x088c, 0x3110, 0x6666, 0x366c, 0x17e8, 0x0ff0, 0x718e, 0x399c,
    0xaaaa, 0xf0f0, 0x5a5a, 0x33cc, 0x3c3c, 0x55aa, 0x9696, 0xa55a,
    0x73ce, 0x13c8, 0x324c, 0x3bdc, 0x6996, 0xc33c, 0x9966, 0x0660,
    0x0272, 0x04e4, 0x4e40, 0x2720, 0xc936, 0x936c, 0x39c6, 0x639c,
    0x9336, 0x9cc6, 0x817e, 0xe718, 0xccf0, 0x0fcc, 0x7744, 0xee22,
];

/// The anchor pixel of the *second* subset of each 2-subset pattern. Subset 0's
/// anchor is always pixel 0, so only the others need a table.
#[rustfmt::skip]
static BC7_ANCHOR_2: [u8; 64] = [
    15, 15, 15, 15, 15, 15, 15, 15,
    15, 15, 15, 15, 15, 15, 15, 15,
    15,  2,  8,  2,  2,  8,  8, 15,
     2,  8,  2,  2,  8,  8,  2,  2,
    15, 15,  6,  8,  2,  8, 15, 15,
     2,  8,  2,  2,  2, 15, 15,  6,
     6,  2,  6,  8, 15, 15,  2,  2,
    15, 15, 15, 15, 15,  2,  2, 15,
];

#[rustfmt::skip]
static BC7_PARTITION_3: [u32; 64] = [
    0xaa685050, 0x6a5a5040, 0x5a5a4200, 0x5450a0a8, 0xa5a50000, 0xa0a05050,
    0x5555a0a0, 0x5a5a5050, 0xaa550000, 0xaa555500, 0xaaaa5500, 0x90909090,
    0x94949494, 0xa4a4a4a4, 0xa9a59450, 0x2a0a4250, 0xa5945040, 0x0a425054,
    0xa5a5a500, 0x55a0a0a0, 0xa8a85454, 0x6a6a4040, 0xa4a45000, 0x1a1a0500,
    0x0050a4a4, 0xaaa59090, 0x14696914, 0x69691400, 0xa08585a0, 0xaa821414,
    0x50a4a450, 0x6a5a0200, 0xa9a58000, 0x5090a0a8, 0xa8a09050, 0x24242424,
    0x00aa5500, 0x24924924, 0x24499224, 0x50a50a50, 0x500aa550, 0xaaaa4444,
    0x66660000, 0xa5a0a5a0, 0x50a050a0, 0x69286928, 0x44aaaa44, 0x66666600,
    0xaa444444, 0x54a854a8, 0x95809580, 0x96969600, 0xa85454a8, 0x80959580,
    0xaa141414, 0x96960000, 0xaaaa1414, 0xa05050a0, 0xa0a5a5a0, 0x96000000,
    0x40804080, 0xa9a8a9a8, 0xaaaaaa44, 0x2a4a5254,
];

/// 3-subset anchors: `[0]` is subset 1's anchor per pattern, `[1]` subset 2's.
#[rustfmt::skip]
static BC7_ANCHOR_3: [[u8; 64]; 2] = [
    [
         3,  3, 15, 15,  8,  3, 15, 15,
         8,  8,  6,  6,  6,  5,  3,  3,
         3,  3,  8, 15,  3,  3,  6, 10,
         5,  8,  8,  6,  8,  5, 15, 15,
         8, 15,  3,  5,  6, 10,  8, 15,
        15,  3, 15,  5, 15, 15, 15, 15,
         3, 15,  5,  5,  5,  8,  5, 10,
         5, 10,  8, 13, 15, 12,  3,  3,
    ],
    [
        15,  8,  8,  3, 15, 15,  3,  8,
        15, 15, 15, 15, 15, 15, 15,  8,
        15,  8, 15,  3, 15,  8, 15,  8,
         3, 15,  6, 10, 15, 15, 10,  8,
        15,  3, 15, 10, 10,  8,  9, 10,
         6, 15,  8, 15,  3,  6,  6,  8,
        15,  3, 15, 15, 15, 15, 15, 15,
        15, 15, 15, 15,  3, 15, 15,  8,
    ],
];

/// Interpolation weights over 64 for 2-, 3- and 4-bit indices.
static BC7_WEIGHTS: [[u8; 16]; 3] = [
    [0, 21, 43, 64, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [0, 9, 18, 27, 37, 46, 55, 64, 0, 0, 0, 0, 0, 0, 0, 0],
    [0, 4, 9, 13, 17, 21, 26, 30, 34, 38, 43, 47, 51, 55, 60, 64],
];

/// One BC7 mode's field widths, in the order the block stores them.
struct Bc7Mode {
    subsets: u8,
    partition_bits: u8,
    rotation_bits: u8,
    index_selection_bits: u8,
    colour_bits: u8,
    alpha_bits: u8,
    /// Bits of p-bit per endpoint, or 0 when the mode has none.
    endpoint_p_bits: u8,
    /// Whether a pair of endpoints shares one p-bit instead of taking two.
    shared_p_bits: bool,
    /// Index width of the first set, and of the second (0 when there is none).
    index_bits: [u8; 2],
}

static BC7_MODES: [Bc7Mode; 8] = [
    // 0: 3 subsets, 4-bit RGB, one p-bit per endpoint, 3-bit indices.
    Bc7Mode {
        subsets: 3,
        partition_bits: 4,
        rotation_bits: 0,
        index_selection_bits: 0,
        colour_bits: 4,
        alpha_bits: 0,
        endpoint_p_bits: 1,
        shared_p_bits: false,
        index_bits: [3, 0],
    },
    // 1: 2 subsets, 6-bit RGB, one *shared* p-bit per endpoint pair.
    Bc7Mode {
        subsets: 2,
        partition_bits: 6,
        rotation_bits: 0,
        index_selection_bits: 0,
        colour_bits: 6,
        alpha_bits: 0,
        endpoint_p_bits: 0,
        shared_p_bits: true,
        index_bits: [3, 0],
    },
    // 2: 3 subsets, 5-bit RGB, no p-bits.
    Bc7Mode {
        subsets: 3,
        partition_bits: 6,
        rotation_bits: 0,
        index_selection_bits: 0,
        colour_bits: 5,
        alpha_bits: 0,
        endpoint_p_bits: 0,
        shared_p_bits: false,
        index_bits: [2, 0],
    },
    // 3: 2 subsets, 7-bit RGB, one p-bit per endpoint.
    Bc7Mode {
        subsets: 2,
        partition_bits: 6,
        rotation_bits: 0,
        index_selection_bits: 0,
        colour_bits: 7,
        alpha_bits: 0,
        endpoint_p_bits: 1,
        shared_p_bits: false,
        index_bits: [2, 0],
    },
    // 4: one subset, two index sets, 5-bit RGB with 6-bit alpha, rotation.
    Bc7Mode {
        subsets: 1,
        partition_bits: 0,
        rotation_bits: 2,
        index_selection_bits: 1,
        colour_bits: 5,
        alpha_bits: 6,
        endpoint_p_bits: 0,
        shared_p_bits: false,
        index_bits: [2, 3],
    },
    // 5: one subset, 7-bit RGB with 8-bit alpha, rotation.
    Bc7Mode {
        subsets: 1,
        partition_bits: 0,
        rotation_bits: 2,
        index_selection_bits: 0,
        colour_bits: 7,
        alpha_bits: 8,
        endpoint_p_bits: 0,
        shared_p_bits: false,
        index_bits: [2, 2],
    },
    // 6: one subset, 7-bit RGBA with one p-bit per endpoint and 4-bit indices.
    Bc7Mode {
        subsets: 1,
        partition_bits: 0,
        rotation_bits: 0,
        index_selection_bits: 0,
        colour_bits: 7,
        alpha_bits: 7,
        endpoint_p_bits: 1,
        shared_p_bits: false,
        index_bits: [4, 0],
    },
    // 7: 2 subsets, 5-bit RGBA with one p-bit per endpoint.
    Bc7Mode {
        subsets: 2,
        partition_bits: 6,
        rotation_bits: 0,
        index_selection_bits: 0,
        colour_bits: 5,
        alpha_bits: 5,
        endpoint_p_bits: 1,
        shared_p_bits: false,
        index_bits: [2, 0],
    },
];

/// A 128-bit block read least-significant bit first, which is the order BC7
/// stores its fields in.
struct Bits {
    value: u128,
    pos: usize,
}

impl Bits {
    fn take(&mut self, n: u8) -> u32 {
        let out = self.peek(0, n);
        self.pos += n as usize;
        out
    }

    fn peek(&self, offset: usize, n: u8) -> u32 {
        if n == 0 {
            return 0;
        }
        let mask = (1u128 << n) - 1;
        ((self.value >> (self.pos + offset)) & mask) as u32
    }

    /// Read `n` bits and reverse their order. Only BC6H needs it: three of its
    /// modes store the top bits of an endpoint most-significant-bit first,
    /// interleaved with the low bits, which is why the specification's own
    /// field tables are not a simple sequence of fields.
    fn take_reversed(&mut self, n: u8) -> u32 {
        let mut v = self.take(n);
        let mut out = 0u32;
        for _ in 0..n {
            out = (out << 1) | (v & 1);
            v >>= 1;
        }
        out
    }
}

/// Replicate a quantised value up to 8 bits: `bits` significant bits in, the
/// top bits repeated into the low ones so that all-ones maps to 255.
fn bc7_expand(v: u8, bits: u8) -> u8 {
    if bits >= 8 {
        return v;
    }
    let shifted = v << (8 - bits);
    shifted | (shifted >> bits)
}

/// Decode one 16-byte BC7 block into 16 RGBA pixels.
fn bc7_block(block: &[u8], out: &mut [u8; 64]) {
    let mut value = 0u128;
    for (i, byte) in block.iter().take(16).enumerate() {
        value |= (*byte as u128) << (8 * i);
    }
    let mut bits = Bits { value, pos: 0 };

    // The mode is the position of the first set bit; eight zero bits is not a
    // valid block and decodes to transparent black.
    let mut mode = 0usize;
    while mode < 8 && bits.take(1) == 0 {
        mode += 1;
    }
    if mode == 8 {
        out.fill(0);
        return;
    }
    let m = &BC7_MODES[mode];
    let p_bits = if m.endpoint_p_bits != 0 {
        m.endpoint_p_bits
    } else {
        m.shared_p_bits as u8
    };
    let partition = bits.take(m.partition_bits) as usize;
    let rotation = bits.take(m.rotation_bits);
    let index_selection = bits.take(m.index_selection_bits) as usize;

    // Endpoints: two per subset, all the reds then all the greens then all the
    // blues then all the alphas. A p-bit extends the value into an extra low
    // bit, so the raw read is shifted up by the p-bit width first.
    let mut ep = [[0u8; 6]; 4]; // R, G, B, A per endpoint
    for channel in 0..3 {
        for s in 0..m.subsets as usize {
            for e in 0..2 {
                ep[channel][s * 2 + e] = (bits.take(m.colour_bits) as u8) << p_bits;
            }
        }
    }
    if m.alpha_bits != 0 {
        for s in 0..m.subsets as usize {
            for e in 0..2 {
                ep[3][s * 2 + e] = (bits.take(m.alpha_bits) as u8) << p_bits;
            }
        }
    } else {
        ep[3] = [0xff; 6];
    }
    if p_bits != 0 {
        for s in 0..m.subsets as usize {
            let a = bits.take(p_bits) as u8;
            let b = if m.shared_p_bits {
                a
            } else {
                bits.take(p_bits) as u8
            };
            for channel in 0..4 {
                ep[channel][s * 2] |= a & 1;
                ep[channel][s * 2 + 1] |= b & 1;
            }
        }
    }

    let colour_bits = m.colour_bits + p_bits;
    for channel in 0..3 {
        for e in 0..6 {
            ep[channel][e] = bc7_expand(ep[channel][e], colour_bits);
        }
    }
    if m.alpha_bits != 0 {
        for e in 0..6 {
            ep[3][e] = bc7_expand(ep[3][e], m.alpha_bits + p_bits);
        }
    }

    // Indices: the first set's bits occupy the run after the endpoints, the
    // second set follows it (only modes 4 and 5 have two). Each subset's anchor
    // pixel stores one bit fewer, its top bit being implied zero.
    let ib = [m.index_bits[0] as usize, m.index_bits[1] as usize];
    let w0 = &BC7_WEIGHTS[ib[0] - 2];
    let w1 = if ib[1] != 0 {
        &BC7_WEIGHTS[ib[1] - 2]
    } else {
        w0
    };
    let set1_base = if ib[1] != 0 {
        m.subsets as usize * (16 * ib[0] - 1)
    } else {
        0
    };
    let mut off = [0usize, set1_base];
    for idx in 0..16usize {
        let (subset, anchor) = match m.subsets {
            2 => {
                let s = ((BC7_PARTITION_2[partition] >> idx) & 1) as usize;
                let a = if s != 0 {
                    BC7_ANCHOR_2[partition] as usize
                } else {
                    0
                };
                (s, a)
            }
            3 => {
                let s = ((BC7_PARTITION_3[partition] >> (2 * idx)) & 3) as usize;
                let a = if s != 0 {
                    BC7_ANCHOR_3[s - 1][partition] as usize
                } else {
                    0
                };
                (s, a)
            }
            _ => (0, 0),
        };
        let anchor = (idx == anchor) as usize;
        let n0 = ib[0] - anchor;
        let n1 = if ib[1] != 0 { ib[1] - anchor } else { 0 };
        let i0 = bits.peek(off[0], n0 as u8) as usize;
        off[0] += n0;
        let i1 = if ib[1] != 0 {
            let v = bits.peek(off[1], n1 as u8) as usize;
            off[1] += n1;
            v
        } else {
            i0
        };
        // The index-selection bit (mode 4) picks which index set is the colour.
        let (ic, ia) = if index_selection == 0 {
            (w0[i0] as u32, w1[i1] as u32)
        } else {
            (w1[i1] as u32, w0[i0] as u32)
        };

        let (e0, e1) = (subset * 2, subset * 2 + 1);
        let mut px = [0u8; 4];
        for channel in 0..4 {
            let weight = if channel == 3 { ia } else { ic };
            let (a, b) = (ep[channel][e0] as u32, ep[channel][e1] as u32);
            px[channel] = (((64 - weight) * a + weight * b + 32) >> 6) as u8;
        }
        // Rotation says which colour channel shares the alpha endpoints; the
        // swap is its own inverse.
        match rotation {
            1 => px.swap(0, 3),
            2 => px.swap(1, 3),
            3 => px.swap(2, 3),
            _ => {}
        }
        out[idx * 4..idx * 4 + 4].copy_from_slice(&px);
    }
}

// ------------------------------------------------------------------ BC6H

/// BC6H: 16 bytes per 4x4 pixels of high-dynamic-range RGB held as half
/// floats. It is the one format in this family with no byte output and no
/// alpha — the specification fixes alpha at 1.0 in every mode, which is why
/// [`Texture::rgba16`] carries a fourth channel that is always the half float
/// for 1.0.
///
/// In plain language, one 16-byte block says:
///
/// * a **mode** — one of fourteen numbers, each of which is a different way of
///   packing the same information;
/// * the **endpoints**: the two ends of the colour line. Ten of the modes
///   store *four* endpoints (two per region, two regions), four store two;
/// * for a two-region mode, a **partition** index choosing one of 32 shapes
///   that says which pixels belong to which of the two endpoint pairs;
/// * one small **index** per pixel saying where on the line between its two
///   endpoints that pixel sits.
///
/// Most modes store the three non-base endpoints as small *deltas* against a
/// single high-precision base endpoint (the specification calls this
/// "transformed"), because neighbouring endpoints in a smooth HDR image are
/// close together and the bits are better spent elsewhere. Everything is
/// un-quantised back to 16-bit integers, interpolated, scaled, and the result
/// is a legal half float.
///
/// Sources, both public specifications: Microsoft's "BC6H Format"
/// (learn.microsoft.com/windows/win32/direct3d11/bc6h-format) for the mode
/// table, the sign-extension, transform-inversion, unquantisation and
/// interpolation steps; the Khronos `EXT_texture_compression_bptc` extension
/// for the same table in a different notation (its `Table.F` is the field
/// layout transcribed below, field for field, including its reversed fields).
/// The partition table is the first 32 rows of BC7's — see
/// [`BC6H_PARTITION_NOTE`].
/// Endpoint slots, named as the specification names them: `RW` is
/// `endpt[0].A` in red, `RX` is `endpt[0].B`, `RY` is `endpt[1].A` and `RZ` is
/// `endpt[1].B` — and then the same four for green and for blue. So `W` is
/// always the high-precision base endpoint of its channel, and the other three
/// are what the deltas add to it.
const RW: u8 = 0;
const RX: u8 = 1;
const RY: u8 = 2;
const RZ: u8 = 3;
const GW: u8 = 4;
const GX: u8 = 5;
const GY: u8 = 6;
const GZ: u8 = 7;
const BW: u8 = 8;
const BX: u8 = 9;
const BY: u8 = 10;
const BZ: u8 = 11;

/// IEEE 754 binary32 to binary16, the inverse of [`half_to_f32`].
///
/// Round-to-nearest, ties to even, overflow to infinity and underflow to a
/// subnormal or zero — the standard conversion, written out because the only
/// half values this module needs to *produce* are the ones a decoder already
/// holds as bits, and a value must survive `half -> f32 -> half` unchanged.
pub fn f32_to_half(v: f32) -> u16 {
    let bits = v.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32;
    let mantissa = bits & 0x007f_ffff;
    if exp == 0xff {
        return sign | 0x7c00 | if mantissa != 0 { 0x0200 } else { 0 };
    }
    let e = exp - 127 + 15;
    if e >= 0x1f {
        return sign | 0x7c00; // overflow: infinity
    }
    if e <= 0 {
        if e < -10 {
            return sign; // underflow: zero
        }
        let m = mantissa | 0x0080_0000;
        let shift = (14 - e) as u32;
        let mut half = (m >> shift) as u16;
        let rest = m & ((1 << shift) - 1);
        let mid = 1 << (shift - 1);
        if rest > mid || (rest == mid && half & 1 == 1) {
            half += 1; // may carry into the smallest normal, which is correct
        }
        return sign | half;
    }
    let mut half = ((e as u16) << 10) | ((mantissa >> 13) as u16);
    let rest = mantissa & 0x1fff;
    if rest > 0x1000 || (rest == 0x1000 && half & 1 == 1) {
        half += 1;
    }
    sign | half
}

/// Where one field of a mode's stored layout goes: `width` bits of the block,
/// landing at bit `offset` of endpoint `slot`, most-significant-bit first when
/// `rev`. Public so the tests can check the table's arithmetic; it is data, not
/// an interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bc6hField {
    pub slot: u8,
    pub offset: u8,
    pub width: u8,
    pub rev: bool,
}

/// The specification writes its fields `x[hi:lo]` (read `hi-lo+1` bits, store
/// at bit `lo`) and `x[b]` (one bit at `b`). This macro lets the mode table
/// below use exactly that notation, so it can be read side by side with the
/// specification's own `Table.F` / "bit fields for bc6h compressed endpoint
/// formats" image.
///
/// The three reversed fields are the exception: the specification prints those
/// the other way round (`r0[10:11]`, `r0[10:15]`, low bit first, because the
/// bits are stored high-first), so the macro normalises them to `hi:lo` like
/// everything else and the table says `11:10` where the specification says
/// `10:11`.
macro_rules! f {
    ($slot:ident $hi:literal : $lo:literal) => {
        Bc6hField {
            slot: $slot,
            offset: $lo as u8,
            width: ($hi - $lo + 1) as u8,
            rev: false,
        }
    };
    ($slot:ident $hi:literal) => {
        Bc6hField {
            slot: $slot,
            offset: $hi as u8,
            width: 1,
            rev: false,
        }
    };
    ($slot:ident $hi:literal : $lo:literal, rev) => {
        Bc6hField {
            slot: $slot,
            offset: $lo as u8,
            width: ($hi - $lo + 1) as u8,
            rev: true,
        }
    };
}

/// One of BC6H's fourteen modes. The first four fields are the columns of the
/// specification's `Table.MF` (mode number, `Tr` = transformed, `EPB` =
/// endpoint bits, and the three delta-bit counts); `index_bits` and
/// `two_regions` follow from the block's shape, and `fields` is the layout.
/// Public so the tests can check that each mode's widths fill 128 bits.
pub struct Bc6hMode {
    /// The mode number as stored in the block.
    pub number: u8,
    /// Whether the other endpoints are deltas against the base endpoint.
    pub transformed: bool,
    /// Precision of the base endpoint, per channel, in bits.
    pub base_bits: u8,
    /// Bits stored for endpoints 1, 2 and 3, per channel.
    pub delta_bits: [u8; 3],
    /// Palette indices per pixel: 3 bits when the block has two regions (16
    /// pixels x 3 bits, less one bit at each region's anchor = 46), 4 when it
    /// has one (64 less the anchor's one bit = 63).
    pub index_bits: u8,
    pub two_regions: bool,
    pub fields: &'static [Bc6hField],
}

/// Every field width in the table below, in bits, mode field and partition
/// index included. The test `bc6h_mode_table_fills_the_block_exactly` checks
/// each mode sums to 128, so a typo in a width cannot pass unnoticed.
pub static BC6H_MODES: [Bc6hMode; 14] = [
    // Mode 0 (specification mode 1), stored as the two bits `00`.
    // 2 mode bits + 75 endpoint bits + 5 partition bits + 46 index bits.
    // 10.555 per region: a 10-bit base endpoint and three 5-bit deltas.
    Bc6hMode {
        number: 0,
        transformed: true,
        base_bits: 10,
        delta_bits: [5, 5, 5],
        index_bits: 3,
        two_regions: true,
        fields: &[
            f!(GY 4),
            f!(BY 4),
            f!(BZ 4),
            f!(RW 9:0),
            f!(GW 9:0),
            f!(BW 9:0),
            f!(RX 4:0),
            f!(GZ 4),
            f!(GY 3:0),
            f!(GX 4:0),
            f!(BZ 0),
            f!(GZ 3:0),
            f!(BX 4:0),
            f!(BZ 1),
            f!(BY 3:0),
            f!(RY 4:0),
            f!(BZ 2),
            f!(RZ 4:0),
            f!(BZ 3),
        ],
    },
    // Mode 1 (specification mode 2), the two bits `01`. 7666: a 7-bit base
    // endpoint and three 6-bit deltas, the cheapest mode in the format.
    Bc6hMode {
        number: 1,
        transformed: true,
        base_bits: 7,
        delta_bits: [6, 6, 6],
        index_bits: 3,
        two_regions: true,
        fields: &[
            f!(GY 5),
            f!(GZ 4),
            f!(GZ 5),
            f!(RW 6:0),
            f!(BZ 0),
            f!(BZ 1),
            f!(BY 4),
            f!(GW 6:0),
            f!(BY 5),
            f!(BZ 2),
            f!(GY 4),
            f!(BW 6:0),
            f!(BZ 3),
            f!(BZ 5),
            f!(BZ 4),
            f!(RX 5:0),
            f!(GY 3:0),
            f!(GX 5:0),
            f!(GZ 3:0),
            f!(BX 5:0),
            f!(BY 3:0),
            f!(RY 5:0),
            f!(RZ 5:0),
        ],
    },
    // Mode 2 (specification mode 3). 11.555: an 11-bit base endpoint, whose
    // top bit `RW[10]` is stored late and separately, and 5/4/4 delta bits —
    // the modes from here to 5 spend the endpoint bits unevenly per channel.
    Bc6hMode {
        number: 2,
        transformed: true,
        base_bits: 11,
        delta_bits: [5, 4, 4],
        index_bits: 3,
        two_regions: true,
        fields: &[
            f!(RW 9:0),
            f!(GW 9:0),
            f!(BW 9:0),
            f!(RX 4:0),
            f!(RW 10),
            f!(GY 3:0),
            f!(GX 3:0),
            f!(GW 10),
            f!(BZ 0),
            f!(GZ 3:0),
            f!(BX 3:0),
            f!(BW 10),
            f!(BZ 1),
            f!(BY 3:0),
            f!(RY 4:0),
            f!(BZ 2),
            f!(RZ 4:0),
            f!(BZ 3),
        ],
    },
    // Mode 6 (specification mode 4): 11.444 — the extra precision is on green.
    Bc6hMode {
        number: 6,
        transformed: true,
        base_bits: 11,
        delta_bits: [4, 5, 4],
        index_bits: 3,
        two_regions: true,
        fields: &[
            f!(RW 9:0),
            f!(GW 9:0),
            f!(BW 9:0),
            f!(RX 3:0),
            f!(RW 10),
            f!(GZ 4),
            f!(GY 3:0),
            f!(GX 4:0),
            f!(GW 10),
            f!(GZ 3:0),
            f!(BX 3:0),
            f!(BW 10),
            f!(BZ 1),
            f!(BY 3:0),
            f!(RY 3:0),
            f!(BZ 0),
            f!(BZ 2),
            f!(RZ 3:0),
            f!(GY 4),
            f!(BZ 3),
        ],
    },
    // Mode 10 (specification mode 5): 11.555 — the extra precision is on blue.
    Bc6hMode {
        number: 10,
        transformed: true,
        base_bits: 11,
        delta_bits: [4, 4, 5],
        index_bits: 3,
        two_regions: true,
        fields: &[
            f!(RW 9:0),
            f!(GW 9:0),
            f!(BW 9:0),
            f!(RX 3:0),
            f!(RW 10),
            f!(BY 4),
            f!(GY 3:0),
            f!(GX 3:0),
            f!(GW 10),
            f!(BZ 0),
            f!(GZ 3:0),
            f!(BX 4:0),
            f!(BW 10),
            f!(BY 3:0),
            f!(RY 3:0),
            f!(BZ 1),
            f!(BZ 2),
            f!(RZ 3:0),
            f!(BZ 4),
            f!(BZ 3),
        ],
    },
    // Mode 14 (specification mode 6): 9555.
    Bc6hMode {
        number: 14,
        transformed: true,
        base_bits: 9,
        delta_bits: [5, 5, 5],
        index_bits: 3,
        two_regions: true,
        fields: &[
            f!(RW 8:0),
            f!(BY 4),
            f!(GW 8:0),
            f!(GY 4),
            f!(BW 8:0),
            f!(BZ 4),
            f!(RX 4:0),
            f!(GZ 4),
            f!(GY 3:0),
            f!(GX 4:0),
            f!(BZ 0),
            f!(GZ 3:0),
            f!(BX 4:0),
            f!(BZ 1),
            f!(BY 3:0),
            f!(RY 4:0),
            f!(BZ 2),
            f!(RZ 4:0),
            f!(BZ 3),
        ],
    },
    // Mode 18 (specification mode 7): 8666.
    Bc6hMode {
        number: 18,
        transformed: true,
        base_bits: 8,
        delta_bits: [6, 5, 5],
        index_bits: 3,
        two_regions: true,
        fields: &[
            f!(RW 7:0),
            f!(GZ 4),
            f!(BY 4),
            f!(GW 7:0),
            f!(BZ 2),
            f!(GY 4),
            f!(BW 7:0),
            f!(BZ 3),
            f!(BZ 4),
            f!(RX 5:0),
            f!(GY 3:0),
            f!(GX 4:0),
            f!(BZ 0),
            f!(GZ 3:0),
            f!(BX 4:0),
            f!(BZ 1),
            f!(BY 3:0),
            f!(RY 5:0),
            f!(RZ 5:0),
        ],
    },
    // Mode 22 (specification mode 8): 8555.
    Bc6hMode {
        number: 22,
        transformed: true,
        base_bits: 8,
        delta_bits: [5, 6, 5],
        index_bits: 3,
        two_regions: true,
        fields: &[
            f!(RW 7:0),
            f!(BZ 0),
            f!(BY 4),
            f!(GW 7:0),
            f!(GY 5),
            f!(GY 4),
            f!(BW 7:0),
            f!(GZ 5),
            f!(BZ 4),
            f!(RX 4:0),
            f!(GZ 4),
            f!(GY 3:0),
            f!(GX 5:0),
            f!(GZ 3:0),
            f!(BX 4:0),
            f!(BZ 1),
            f!(BY 3:0),
            f!(RY 4:0),
            f!(BZ 2),
            f!(RZ 4:0),
            f!(BZ 3),
        ],
    },
    // Mode 26 (specification mode 9): 8555, the sixth blue delta bit stored early.
    //
    // The ninth field is *blue*'s sixth delta bit, `b3[5]`. `Table.MF` settles it
    // on its own: this mode's delta widths are {5, 5, 6}, so blue carries the
    // six-bit delta and green five — a green sixth bit would leave blue's
    // third delta five bits wide and contradict that row. (Checked against the
    // live text on 2026-10-17: `dev/bc6h_tables.py` re-reads Khronos
    // `EXT_texture_compression_bptc` revision 2, whose `Table.F` prints
    // `b3[5]` here, and Microsoft's own field table, and both agree.) The
    // cross-decoder check settles it too: `bcdec` reads it as blue, and our
    // decode is bit-identical to `bcdec` over all fourteen modes in both
    // variants — had this bit been green's, those blocks would differ. See
    // `docs/formats/materials.md` § "BC6H".
    Bc6hMode {
        number: 26,
        transformed: true,
        base_bits: 8,
        delta_bits: [5, 5, 6],
        index_bits: 3,
        two_regions: true,
        fields: &[
            f!(RW 7:0),
            f!(BZ 1),
            f!(BY 4),
            f!(GW 7:0),
            f!(BY 5),
            f!(GY 4),
            f!(BW 7:0),
            f!(BZ 5),
            f!(BZ 4),
            f!(RX 4:0),
            f!(GZ 4),
            f!(GY 3:0),
            f!(GX 4:0),
            f!(BZ 0),
            f!(GZ 3:0),
            f!(BX 5:0),
            f!(BY 3:0),
            f!(RY 4:0),
            f!(BZ 2),
            f!(RZ 4:0),
            f!(BZ 3),
        ],
    },
    // Mode 30 (specification mode 10): 6666, and the first mode that does *not*
    // use delta compression — both endpoints are stored outright.
    Bc6hMode {
        number: 30,
        transformed: false,
        base_bits: 6,
        delta_bits: [6, 6, 6],
        index_bits: 3,
        two_regions: true,
        fields: &[
            f!(RW 5:0),
            f!(GZ 4),
            f!(BZ 0),
            f!(BZ 1),
            f!(BY 4),
            f!(GW 5:0),
            f!(GY 5),
            f!(BY 5),
            f!(BZ 2),
            f!(GY 4),
            f!(BW 5:0),
            f!(GZ 5),
            f!(BZ 3),
            f!(BZ 5),
            f!(BZ 4),
            f!(RX 5:0),
            f!(GY 3:0),
            f!(GX 5:0),
            f!(GZ 3:0),
            f!(BX 5:0),
            f!(BY 3:0),
            f!(RY 5:0),
            f!(RZ 5:0),
        ],
    },
    // Mode 3 (specification mode 11): the first *signed* mode, 10.10, and the
    // first single-region mode: no partition index, 63 index bits.
    Bc6hMode {
        number: 3,
        transformed: false,
        base_bits: 10,
        delta_bits: [10, 10, 10],
        index_bits: 4,
        two_regions: false,
        fields: &[
            f!(RW 9:0),
            f!(GW 9:0),
            f!(BW 9:0),
            f!(RX 9:0),
            f!(GX 9:0),
            f!(BX 9:0),
        ],
    },
    // Mode 7 (specification mode 12): 11.9, transformed.
    Bc6hMode {
        number: 7,
        transformed: true,
        base_bits: 11,
        delta_bits: [9, 9, 9],
        index_bits: 4,
        two_regions: false,
        fields: &[
            f!(RW 9:0),
            f!(GW 9:0),
            f!(BW 9:0),
            f!(RX 8:0),
            f!(RW 10),
            f!(GX 8:0),
            f!(GW 10),
            f!(BX 8:0),
            f!(BW 10),
        ],
    },
    // Mode 11 (specification mode 13): 12.8. The base endpoint's top two bits
    // are stored most-significant-bit first (the only reversed field; the
    // specification writes it `r0[10:11]`).
    Bc6hMode {
        number: 11,
        transformed: true,
        base_bits: 12,
        delta_bits: [8, 8, 8],
        index_bits: 4,
        two_regions: false,
        fields: &[
            f!(RW 9:0),
            f!(GW 9:0),
            f!(BW 9:0),
            f!(RX 7:0),
            f!(RW 11:10, rev),
            f!(GX 7:0),
            f!(GW 11:10, rev),
            f!(BX 7:0),
            f!(BW 11:10, rev),
        ],
    },
    // Mode 15 (specification mode 14): 16.4 — a full 16-bit base endpoint with
    // a 4-bit delta, so a transform cannot overflow at all. The specification
    // writes its reversed field `r0[10:15]`.
    Bc6hMode {
        number: 15,
        transformed: true,
        base_bits: 16,
        delta_bits: [4, 4, 4],
        index_bits: 4,
        two_regions: false,
        fields: &[
            f!(RW 9:0),
            f!(GW 9:0),
            f!(BW 9:0),
            f!(RX 3:0),
            f!(RW 15:10, rev),
            f!(GX 3:0),
            f!(GW 15:10, rev),
            f!(BX 3:0),
            f!(BW 15:10, rev),
        ],
    },
];

/// The fourteen mode numbers, in the order the table lists them. Exposed so a
/// sweep can visit every mode without repeating the list.
pub const BC6H_MODE_NUMBERS: [u8; 14] = [0, 1, 2, 6, 10, 14, 18, 22, 26, 30, 3, 7, 11, 15];

/// A mode by its stored number, or `None` for a reserved one.
///
/// Reserved blocks are not an error and not a guess: the specification says a
/// reserved mode "must contain all zeroes in all channels except for the alpha
/// channel", and alpha is 1.0.
fn bc6h_mode(number: u8) -> Option<&'static Bc6hMode> {
    BC6H_MODES.iter().find(|m| m.number == number)
}

/// BC6H's partition table is BC7's, truncated to half its length: a two-region
/// BC6H block has a 5-bit partition index and so one of 32 shapes, and those
/// 32 shapes are exactly the first 32 entries of BC7's 64-entry table — the
/// same rows in the same order, with the same anchors. The specification says so
/// in as many words ("the partition index references the first half of
/// `Table.P2`"), and the first 32 rows of that table and of its anchor table
/// `Table.A2` are what `dev/bc6h_tables.py` re-reads from Khronos and `bcdec`
/// (exit 0 = every source agrees). The in-file test
/// `bc6h_shares_the_first_half_of_the_bc7_partition_table` pins the shared rows
/// against a transcribed listing, so the sharing cannot drift silently; a note
/// here rather than a second copy of the table.
/// Sign-extend a value of `bits` bits (the specification's `SIGN_EXTEND`).
fn extend_sign(v: i32, bits: u8) -> i32 {
    (v << (32 - bits)) >> (32 - bits)
}

/// Undo the delta encoding: the other endpoints are stored as offsets from the
/// base endpoint, wrapped at the base endpoint's own precision so the sum can
/// never escape the number of bits it is supposed to have.
fn bc6h_transform_inverse(v: i32, base: i32, bits: u8, signed: bool) -> i32 {
    let v = (v + base) & ((1 << bits) - 1);
    if signed {
        extend_sign(v, bits)
    } else {
        v
    }
}

/// Stretch a quantised endpoint back towards the 16-bit range. The two special
/// cases are the specification's: an all-zero endpoint is exactly zero, and an
/// all-ones endpoint is exactly the largest half (so that the extreme of the
/// range is representable rather than approached).
fn bc6h_unquantize(v: i32, bits: u8, signed: bool) -> i32 {
    if !signed {
        if bits >= 15 {
            return v;
        }
        if v == 0 {
            return 0;
        }
        if v == (1 << bits) - 1 {
            return 0xFFFF;
        }
        return ((v << 16) + 0x8000) >> bits;
    }
    if bits >= 16 {
        return v;
    }
    let (negative, magnitude) = if v < 0 { (true, -v) } else { (false, v) };
    let unq = if magnitude == 0 {
        0
    } else if magnitude >= (1 << (bits - 1)) - 1 {
        0x7FFF
    } else {
        ((magnitude << 15) + 0x4000) >> (bits - 1)
    };
    if negative {
        -unq
    } else {
        unq
    }
}

/// The last scaling step: 16-bit integer back to a half float's bits. The
/// factors (31/64 unsigned, 31/32 signed) are the specification's.
fn bc6h_finish_unquantize(v: i32, signed: bool) -> u16 {
    if !signed {
        return ((v * 31) >> 6) as u16;
    }
    let v = if v < 0 {
        -((-v * 31) >> 5)
    } else {
        (v * 31) >> 5
    };
    let (sign, magnitude) = if v < 0 {
        (0x8000u16, (-v) as u16)
    } else {
        (0, v as u16)
    };
    sign | magnitude
}

/// One pixel's worth of interpolation: `weight` is the palette weight out of 64,
/// so 0 gives the near endpoint and 64 the far one.
fn bc6h_interpolate(a: i32, b: i32, weight: u8) -> i32 {
    (a * (64 - weight as i32) + b * weight as i32 + 32) >> 6
}

/// Decode one 16-byte BC6H block into 16 RGBA pixels of half floats.
fn bc6h_block(block: &[u8], signed: bool, out: &mut [u16; 64]) {
    /// Alpha is not stored: the specification fixes it at 1.0, which is the
    /// half float 0x3C00.
    const ALPHA: u16 = 0x3C00;
    // A reserved mode decodes to black at alpha 1.0.
    let mut blank = || {
        for i in 0..16 {
            out[i * 4..i * 4 + 4].copy_from_slice(&[0, 0, 0, ALPHA]);
        }
    };

    let mut value = 0u128;
    for (i, byte) in block.iter().take(16).enumerate() {
        value |= (*byte as u128) << (8 * i);
    }
    let mut bits = Bits { value, pos: 0 };

    // The mode is the low two bits when they are below two, and otherwise the
    // low five — so "mode 0" and "mode 1" are two-bit fields and the other
    // twelve are five-bit fields sharing those two bits' value 2 or 3.
    let low = bits.take(2) as u8;
    let number = if low < 2 {
        low
    } else {
        low | (bits.take(3) as u8) << 2
    };
    let Some(m) = bc6h_mode(number) else {
        blank();
        return;
    };

    // Endpoints, in the mode's stored order.
    let mut ep = [0i32; 12];
    for field in m.fields {
        let v = if field.rev {
            bits.take_reversed(field.width)
        } else {
            bits.take(field.width)
        };
        ep[field.slot as usize] |= (v as i32) << field.offset;
    }
    let partition = if m.two_regions {
        bits.take(5) as usize
    } else {
        0
    };

    // Sign extension, exactly as the specification's `sign_extend_*`: the base
    // endpoint only when the *format* is signed, and the others when the mode
    // stores deltas or the format is signed.
    for channel in 0..3 {
        let w = channel * 4;
        // Four slots per channel when the block has two regions, two when it
        // has one: the base endpoint plus the ones the deltas describe.
        let used = if m.two_regions { 4 } else { 2 };
        if signed {
            ep[w] = extend_sign(ep[w], m.base_bits);
        }
        if m.transformed || signed {
            // Slots w+1, w+2, w+3 are endpoints 1, 2, 3 in that order, and each
            // was stored at *this channel's* delta width — the widths differ
            // per channel, not per endpoint (modes 2, 6, 10, 18, 22 and 26 are
            // the ones that spend their bits unevenly).
            for k in 1..used {
                ep[w + k] = extend_sign(ep[w + k], m.delta_bits[channel]);
            }
        }
    }

    // Undo the deltas, then stretch everything back to 16 bits.
    for channel in 0..3 {
        let w = channel * 4;
        let used = if m.two_regions { 4 } else { 2 };
        if m.transformed {
            for slot in (w + 1)..(w + used) {
                ep[slot] = bc6h_transform_inverse(ep[slot], ep[w], m.base_bits, signed);
            }
        }
        for slot in w..(w + used) {
            ep[slot] = bc6h_unquantize(ep[slot], m.base_bits, signed);
        }
    }

    // One index per pixel, in raster order. Each region's *anchor* pixel —
    // always pixel 0 for region 0, and the shape's fix-up pixel for region 1 —
    // stores one bit fewer, because the two endpoints can be swapped without
    // changing the picture, so its top bit is known to be zero.
    let weights = if m.index_bits == 3 {
        &BC7_WEIGHTS[1]
    } else {
        &BC7_WEIGHTS[2]
    };
    for y in 0..4usize {
        for x in 0..4usize {
            let pixel = y * 4 + x;
            // Which region this pixel belongs to, and how wide its index is.
            let (subset, index_bits) = if m.two_regions {
                let mask = BC7_PARTITION_2[partition];
                let s = ((mask >> pixel) & 1) as usize;
                let anchor = if s != 0 {
                    BC7_ANCHOR_2[partition] as usize
                } else {
                    0
                };
                (
                    s,
                    if pixel == anchor {
                        m.index_bits - 1
                    } else {
                        m.index_bits
                    },
                )
            } else {
                (
                    0,
                    if pixel == 0 {
                        m.index_bits - 1
                    } else {
                        m.index_bits
                    },
                )
            };
            let index = bits.take(index_bits) as usize;
            debug_assert!(bits.pos <= 128, "a mode's fields ran past the block");
            let w = weights[index];
            let near = subset * 2;
            let mut px = [0u16; 4];
            for (c, channel) in [0usize, 4, 8].iter().enumerate() {
                px[c] = bc6h_finish_unquantize(
                    bc6h_interpolate(ep[channel + near], ep[channel + near + 1], w),
                    signed,
                );
            }
            px[3] = ALPHA;
            out[pixel * 4..pixel * 4 + 4].copy_from_slice(&px);
        }
    }
}

/// Decode a BC6H block run into both forms: the half floats themselves, and
/// the clamped 8-bit view every other format in this module puts in
/// [`Texture::rgba`] (see [`half_to_unorm8`] — an HDR value has no byte form,
/// so anything above 1.0 pins at 255 and anything below 0.0 at 0).
fn decode_bc6h(blocks: &[u8], w: usize, h: usize, signed: bool) -> Result<(Vec<u8>, Vec<u16>)> {
    check_dims(w, h)?;
    let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
    // A shipped asset can carry fewer bytes than its dimensions imply (a mip
    // chain whose level 0 is what we read, or a texture whose payload the
    // manifest truncated); refuse rather than read out of bounds.
    let need = block_bytes_needed(w, h, 16)?;
    if blocks.len() < need {
        return Err(Error::Truncated("block data"));
    }
    let out_len = pixel_bytes(w, h, 4)?;
    let mut out = vec![0u8; out_len];
    let mut out16 = vec![0u16; out_len];
    let mut tile = [0u16; 64];
    for by in 0..bh {
        for bx in 0..bw {
            let at = (by * bw + bx) * 16;
            bc6h_block(&blocks[at..at + 16], signed, &mut tile);
            for y in 0..4 {
                for x in 0..4 {
                    let (px, py) = (bx * 4 + x, by * 4 + y);
                    if px < w && py < h {
                        let src = (y * 4 + x) * 4;
                        let dst = (py * w + px) * 4;
                        out16[dst..dst + 4].copy_from_slice(&tile[src..src + 4]);
                        for c in 0..3 {
                            out[dst + c] = half_to_unorm8(tile[src + c]);
                        }
                        out[dst + 3] = 255;
                    }
                }
            }
        }
    }
    Ok((out, out16))
}

/// Decode a single 4x4 BC6H block, as `(clamped 8-bit RGBA, half-float RGBA)`.
/// The block-by-block entry point the synthetic per-mode sweep uses, so the
/// Rust decoder and the independent decoders can be pointed at the same 16
/// bytes without going through a whole texture.
pub fn decode_bc6h_block(block: &[u8], signed: bool) -> (Vec<u8>, Vec<u16>) {
    let mut halves = [0u16; 64];
    bc6h_block(block, signed, &mut halves);
    let bytes = (0..16)
        .flat_map(|i| {
            [
                half_to_unorm8(halves[i * 4]),
                half_to_unorm8(halves[i * 4 + 1]),
                half_to_unorm8(halves[i * 4 + 2]),
                255u8,
            ]
        })
        .collect();
    (bytes, halves.to_vec())
}

/// Deterministic pseudo-random blocks for one mode.
///
/// This exists for coverage, not for production decoding. BC6H decoding is
/// *total* — every one of the 2^128 possible block values decodes to something,
/// there is no "invalid" block the way there is in BC7 — so a block with the
/// right mode number and random everything else is a perfectly valid input, and
/// a useful one: random payloads land on delta wraps, on both unquantisation
/// clamps and on negative (signed) values, which hand-built blocks mostly
/// miss. `tests/bc6h.rs` and `ow-tank-tex --bc6h-selftest` both use it, so the
/// Rust decoder and the independent decoders are pointed at identical bytes.
pub fn bc6h_random_blocks(mode_number: u8, count: usize, seed: u64) -> Vec<[u8; 16]> {
    let mut state = seed ^ ((mode_number as u64) << 56) ^ 0x9E37_79B9_7F4A_7C15;
    let mut next = || {
        // splitmix64: a small, well-known generator whose only job is to be
        // the *same* on every machine and every run.
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    (0..count)
        .map(|_| {
            let mut block = [0u8; 16];
            for chunk in block.chunks_mut(8) {
                let v = next().to_le_bytes();
                chunk.copy_from_slice(&v[..chunk.len()]);
            }
            // Keep the mode number, drop every other bit of the low byte.
            block[0] = (block[0] & !0x1f) | (mode_number & 0x1f);
            block
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // The published BC6H tables, transcribed once and checked against
    // `BC6H_MODES` and BC7's partition table below. Keeping a second copy is
    // the point: it is the same data read from the specification rather than
    // from the table that was itself written from the specification, so the two
    // have to agree.
    //
    // Evidence class *published-check*. Sources, both public:
    //   - Khronos `EXT_texture_compression_bptc`, `Table.F` (the field order of
    //     each mode), `Table.P2` and `Table.A2` (BC7's partition shapes and the
    //     anchors of their second subset, of which BC6H uses the first half);
    //   - Microsoft's "BC6H Format" page, whose mode table gives the same bit
    //     counts and mode numbers.
    // `dev/bc6h_tables.py` re-reads both and prints these same two tables, so
    // they can be re-derived rather than taken on trust.

    /// Each mode's stored fields, as `Table.F` prints them, in the `f!`
    /// notation this file uses. The mode field itself is *not* here: the decoder
    /// reads it separately, before any of these.
    static BC6H_PUBLISHED_FIELDS: [(&[Bc6hField], u8); 14] = [
        // stored 0 (specification mode 1)
        (
            &[
                f!(GY 4),
                f!(BY 4),
                f!(BZ 4),
                f!(RW 9:0),
                f!(GW 9:0),
                f!(BW 9:0),
                f!(RX 4:0),
                f!(GZ 4),
                f!(GY 3:0),
                f!(GX 4:0),
                f!(BZ 0),
                f!(GZ 3:0),
                f!(BX 4:0),
                f!(BZ 1),
                f!(BY 3:0),
                f!(RY 4:0),
                f!(BZ 2),
                f!(RZ 4:0),
                f!(BZ 3),
            ],
            2,
        ),
        // stored 1 (specification mode 2)
        (
            &[
                f!(GY 5),
                f!(GZ 4),
                f!(GZ 5),
                f!(RW 6:0),
                f!(BZ 0),
                f!(BZ 1),
                f!(BY 4),
                f!(GW 6:0),
                f!(BY 5),
                f!(BZ 2),
                f!(GY 4),
                f!(BW 6:0),
                f!(BZ 3),
                f!(BZ 5),
                f!(BZ 4),
                f!(RX 5:0),
                f!(GY 3:0),
                f!(GX 5:0),
                f!(GZ 3:0),
                f!(BX 5:0),
                f!(BY 3:0),
                f!(RY 5:0),
                f!(RZ 5:0),
            ],
            2,
        ),
        // stored 2 (specification mode 3)
        (
            &[
                f!(RW 9:0),
                f!(GW 9:0),
                f!(BW 9:0),
                f!(RX 4:0),
                f!(RW 10),
                f!(GY 3:0),
                f!(GX 3:0),
                f!(GW 10),
                f!(BZ 0),
                f!(GZ 3:0),
                f!(BX 3:0),
                f!(BW 10),
                f!(BZ 1),
                f!(BY 3:0),
                f!(RY 4:0),
                f!(BZ 2),
                f!(RZ 4:0),
                f!(BZ 3),
            ],
            5,
        ),
        // stored 6 (specification mode 4)
        (
            &[
                f!(RW 9:0),
                f!(GW 9:0),
                f!(BW 9:0),
                f!(RX 3:0),
                f!(RW 10),
                f!(GZ 4),
                f!(GY 3:0),
                f!(GX 4:0),
                f!(GW 10),
                f!(GZ 3:0),
                f!(BX 3:0),
                f!(BW 10),
                f!(BZ 1),
                f!(BY 3:0),
                f!(RY 3:0),
                f!(BZ 0),
                f!(BZ 2),
                f!(RZ 3:0),
                f!(GY 4),
                f!(BZ 3),
            ],
            5,
        ),
        // stored 10 (specification mode 5)
        (
            &[
                f!(RW 9:0),
                f!(GW 9:0),
                f!(BW 9:0),
                f!(RX 3:0),
                f!(RW 10),
                f!(BY 4),
                f!(GY 3:0),
                f!(GX 3:0),
                f!(GW 10),
                f!(BZ 0),
                f!(GZ 3:0),
                f!(BX 4:0),
                f!(BW 10),
                f!(BY 3:0),
                f!(RY 3:0),
                f!(BZ 1),
                f!(BZ 2),
                f!(RZ 3:0),
                f!(BZ 4),
                f!(BZ 3),
            ],
            5,
        ),
        // stored 14 (specification mode 6)
        (
            &[
                f!(RW 8:0),
                f!(BY 4),
                f!(GW 8:0),
                f!(GY 4),
                f!(BW 8:0),
                f!(BZ 4),
                f!(RX 4:0),
                f!(GZ 4),
                f!(GY 3:0),
                f!(GX 4:0),
                f!(BZ 0),
                f!(GZ 3:0),
                f!(BX 4:0),
                f!(BZ 1),
                f!(BY 3:0),
                f!(RY 4:0),
                f!(BZ 2),
                f!(RZ 4:0),
                f!(BZ 3),
            ],
            5,
        ),
        // stored 18 (specification mode 7)
        (
            &[
                f!(RW 7:0),
                f!(GZ 4),
                f!(BY 4),
                f!(GW 7:0),
                f!(BZ 2),
                f!(GY 4),
                f!(BW 7:0),
                f!(BZ 3),
                f!(BZ 4),
                f!(RX 5:0),
                f!(GY 3:0),
                f!(GX 4:0),
                f!(BZ 0),
                f!(GZ 3:0),
                f!(BX 4:0),
                f!(BZ 1),
                f!(BY 3:0),
                f!(RY 5:0),
                f!(RZ 5:0),
            ],
            5,
        ),
        // stored 22 (specification mode 8)
        (
            &[
                f!(RW 7:0),
                f!(BZ 0),
                f!(BY 4),
                f!(GW 7:0),
                f!(GY 5),
                f!(GY 4),
                f!(BW 7:0),
                f!(GZ 5),
                f!(BZ 4),
                f!(RX 4:0),
                f!(GZ 4),
                f!(GY 3:0),
                f!(GX 5:0),
                f!(GZ 3:0),
                f!(BX 4:0),
                f!(BZ 1),
                f!(BY 3:0),
                f!(RY 4:0),
                f!(BZ 2),
                f!(RZ 4:0),
                f!(BZ 3),
            ],
            5,
        ),
        // stored 26 (specification mode 9). The ninth field is `b3[5]` — blue.
        (
            &[
                f!(RW 7:0),
                f!(BZ 1),
                f!(BY 4),
                f!(GW 7:0),
                f!(BY 5),
                f!(GY 4),
                f!(BW 7:0),
                f!(BZ 5),
                f!(BZ 4),
                f!(RX 4:0),
                f!(GZ 4),
                f!(GY 3:0),
                f!(GX 4:0),
                f!(BZ 0),
                f!(GZ 3:0),
                f!(BX 5:0),
                f!(BY 3:0),
                f!(RY 4:0),
                f!(BZ 2),
                f!(RZ 4:0),
                f!(BZ 3),
            ],
            5,
        ),
        // stored 30 (specification mode 10)
        (
            &[
                f!(RW 5:0),
                f!(GZ 4),
                f!(BZ 0),
                f!(BZ 1),
                f!(BY 4),
                f!(GW 5:0),
                f!(GY 5),
                f!(BY 5),
                f!(BZ 2),
                f!(GY 4),
                f!(BW 5:0),
                f!(GZ 5),
                f!(BZ 3),
                f!(BZ 5),
                f!(BZ 4),
                f!(RX 5:0),
                f!(GY 3:0),
                f!(GX 5:0),
                f!(GZ 3:0),
                f!(BX 5:0),
                f!(BY 3:0),
                f!(RY 5:0),
                f!(RZ 5:0),
            ],
            5,
        ),
        // stored 3 (specification mode 11)
        (
            &[
                f!(RW 9:0),
                f!(GW 9:0),
                f!(BW 9:0),
                f!(RX 9:0),
                f!(GX 9:0),
                f!(BX 9:0),
            ],
            5,
        ),
        // stored 7 (specification mode 12)
        (
            &[
                f!(RW 9:0),
                f!(GW 9:0),
                f!(BW 9:0),
                f!(RX 8:0),
                f!(RW 10),
                f!(GX 8:0),
                f!(GW 10),
                f!(BX 8:0),
                f!(BW 10),
            ],
            5,
        ),
        // stored 11 (specification mode 13)
        (
            &[
                f!(RW 9:0),
                f!(GW 9:0),
                f!(BW 9:0),
                f!(RX 7:0),
                f!(RW 11:10, rev),
                f!(GX 7:0),
                f!(GW 11:10, rev),
                f!(BX 7:0),
                f!(BW 11:10, rev),
            ],
            5,
        ),
        // stored 15 (specification mode 14)
        (
            &[
                f!(RW 9:0),
                f!(GW 9:0),
                f!(BW 9:0),
                f!(RX 3:0),
                f!(RW 15:10, rev),
                f!(GX 3:0),
                f!(GW 15:10, rev),
                f!(BX 3:0),
                f!(BW 15:10, rev),
            ],
            5,
        ),
    ];

    /// BC6H's 32 shapes: the 16-bit subset mask (bit `i` is the region of pixel
    /// `i`) and the pixel that carries the second region's fix-up index.
    /// `Table.P2`'s first 32 rows with `Table.A2`'s anchors.
    #[rustfmt::skip]
    static BC6H_PUBLISHED_SHAPES: [(u16, u8); 32] = [
        (0xcccc, 15), (0x8888, 15), (0xeeee, 15), (0xecc8, 15),
        (0xc880, 15), (0xfeec, 15), (0xfec8, 15), (0xec80, 15),
        (0xc800, 15), (0xffec, 15), (0xfe80, 15), (0xe800, 15),
        (0xffe8, 15), (0xff00, 15), (0xfff0, 15), (0xf000, 15),
        (0xf710, 15), (0x008e,  2), (0x7100,  8), (0x08ce,  2),
        (0x008c,  2), (0x7310,  8), (0x3100,  8), (0x8cce, 15),
        (0x088c,  2), (0x3110,  8), (0x6666,  2), (0x366c,  2),
        (0x17e8,  8), (0x0ff0,  8), (0x718e,  2), (0x399c,  2),
    ];

    fn texture_prefix(format: u8, w: u16, h: u16, data_size: u16) -> Vec<u8> {
        let mut d = vec![0u8; 0x20];
        d[2] = 1; // one mip
        d[3] = format;
        d[4] = 1;
        d[6] = 0;
        d[8..10].copy_from_slice(&w.to_le_bytes());
        d[10..12].copy_from_slice(&h.to_le_bytes());
        d[12..14].copy_from_slice(&data_size.to_le_bytes());
        d
    }

    #[test]
    fn bc1_solid_red_block_decodes() {
        // c0 = red (0xF800), c1 = 0, every index 0 -> all pixels red, opaque.
        let mut d = texture_prefix(71, 4, 4, 8);
        let mut block = Vec::new();
        block.extend_from_slice(&0xF800u16.to_le_bytes());
        block.extend_from_slice(&0u16.to_le_bytes());
        block.extend_from_slice(&0u32.to_le_bytes());
        d.extend_from_slice(&block);
        let t = parse(&d).expect("parses");
        assert_eq!(t.format, Format::Bc1);
        assert_eq!(t.pixel(0, 0), [255, 0, 0, 255]);
        assert_eq!(t.pixel(3, 3), [255, 0, 0, 255]);
    }

    #[test]
    fn bc1_three_colour_mode_has_a_transparent_slot() {
        // c0 = 0 <= c1 = 0xFFFF: index 3 is transparent black.
        let mut d = texture_prefix(71, 4, 4, 8);
        d.extend_from_slice(&0u16.to_le_bytes());
        d.extend_from_slice(&0xFFFFu16.to_le_bytes());
        d.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); // every index = 3
        let t = parse(&d).expect("parses");
        assert_eq!(
            t.pixel(0, 0)[3],
            0,
            "index 3 is transparent in 3-colour mode"
        );
    }

    #[test]
    fn bc4_endpoints_and_index_zero_read_the_first_endpoint() {
        let mut d = texture_prefix(80, 4, 4, 8);
        d.push(200); // a0
        d.push(10); // a1
        d.extend_from_slice(&[0u8; 6]);
        let t = parse(&d).expect("parses");
        // One channel, in red: what the hardware hands a shader.
        assert_eq!(t.pixel(0, 0), [200, 0, 0, 255]);
        assert_eq!(t.pixel(3, 3), [200, 0, 0, 255]);
    }

    #[test]
    fn bc3_composes_colour_and_alpha_halves() {
        let mut d = texture_prefix(77, 4, 4, 16);
        d.push(255); // alpha a0
        d.push(0); // alpha a1
        d.extend_from_slice(&[0u8; 6]); // all indices 0 -> alpha 255
        d.extend_from_slice(&0x001Fu16.to_le_bytes()); // blue in RGB565
        d.extend_from_slice(&0u16.to_le_bytes());
        d.extend_from_slice(&0u32.to_le_bytes());
        let t = parse(&d).expect("parses");
        let px = t.pixel(1, 1);
        assert_eq!(px[3], 255);
        assert_eq!(&px[..3], &[0, 0, 255]);
    }

    /// Assemble a BC7 block field by field so a test can state what each mode
    /// *should* decode to instead of pinning whatever the decoder happens to
    /// produce.
    struct Block {
        value: u128,
        pos: usize,
    }

    impl Block {
        fn new() -> Block {
            Block { value: 0, pos: 0 }
        }

        fn put(&mut self, v: u32, n: u8) {
            self.value |= (v as u128) << self.pos;
            self.pos += n as usize;
        }

        fn bytes(&self) -> [u8; 16] {
            self.value.to_le_bytes()
        }
    }

    /// Decode one block through the public path: a 4x4 texture whose payload is
    /// a single BC7 block.
    fn bc7_pixels(block: [u8; 16]) -> Vec<[u8; 4]> {
        let mut d = texture_prefix(100, 4, 4, 16);
        d.extend_from_slice(&block);
        let t = parse(&d).expect("parses");
        assert_eq!(t.format, Format::Bc7);
        (0..16).map(|i| t.pixel(i % 4, i / 4)).collect()
    }

    #[test]
    fn bc7_header_byte_maps_to_the_srgb_id() {
        // The client stores one higher from 26 up, so a shipped 100 is DXGI 99.
        assert_eq!(Format::from_header(100), (Format::Bc7, 99));
        assert_eq!(Format::from_header(99), (Format::Bc7, 98));
    }

    #[test]
    fn bc7_mode6_decodes_an_opaque_white_block() {
        // Mode 6: 7-bit RGBA endpoints, one p-bit each, 4-bit indices. Both
        // endpoints at 127 with p=1 and every index 0 -> opaque white.
        let mut b = Block::new();
        b.put(0b100_0000, 7); // mode 6
        for _ in 0..3 {
            for _ in 0..2 {
                b.put(127, 7); // two endpoints per channel
            }
        }
        b.put(127, 7); // alpha endpoint 0
        b.put(127, 7); // alpha endpoint 1
        b.put(1, 1); // p-bit, endpoint 0
        b.put(1, 1); // p-bit, endpoint 1
        b.put(0, 3); // pixel 0 is the anchor: one bit fewer
        for _ in 1..16 {
            b.put(0, 4);
        }
        assert_eq!(b.pos, 128);

        let px = bc7_pixels(b.bytes());
        assert!(px.iter().all(|p| *p == [255, 255, 255, 255]), "{px:?}");
    }

    #[test]
    fn bc7_mode5_rotation_swaps_the_alpha_into_red() {
        // Mode 5: 7-bit RGB, 8-bit alpha, rotation bits. With rotation 1 the
        // alpha value ends up in red and red in alpha.
        let mut b = Block::new();
        b.put(0b10_0000, 6); // mode 5
        b.put(1, 2); // rotation: swap alpha and red
        for _ in 0..3 {
            for _ in 0..2 {
                b.put(0, 7); // black colour endpoints
            }
        }
        b.put(255, 8); // alpha endpoint 0
        b.put(0, 8); // alpha endpoint 1
        b.put(0, 1); // index-set-0 anchor (pixel 0) skips its top bit
        for _ in 1..16 {
            b.put(0, 2);
        }
        b.put(0, 1);
        for _ in 1..16 {
            b.put(0, 2);
        }
        assert_eq!(b.pos, 128);

        let px = bc7_pixels(b.bytes());
        // Alpha drove the colour, so every pixel is red with a transparent
        // alpha: the swap is its own inverse.
        assert!(px.iter().all(|p| *p == [255, 0, 0, 0]), "{px:?}");
    }

    #[test]
    fn bc7_mode0_partitions_three_subsets_and_reads_their_p_bits() {
        // Mode 0: 3 subsets, 4-bit RGB, one p-bit per endpoint, 3-bit indices.
        // Partition 0 = {0,0,1,1,0,0,1,1,0,2,2,1,2,2,2,2}; give each subset a
        // single colour and leave every index at 0.
        let mut b = Block::new();
        b.put(1, 1); // mode 0
        b.put(0, 4); // partition 0
                     // R for the three subsets, then G, then B.
        for subset in 0..3u32 {
            for _ in 0..2 {
                b.put(if subset == 0 { 15 } else { 0 }, 4);
            }
        }
        for subset in 0..3u32 {
            for _ in 0..2 {
                b.put(if subset == 1 { 15 } else { 0 }, 4);
            }
        }
        for subset in 0..3u32 {
            for _ in 0..2 {
                b.put(if subset == 2 { 15 } else { 0 }, 4);
            }
        }
        for _ in 0..6 {
            b.put(1, 1); // every endpoint takes p=1
        }
        // 3 subsets x 16 pixels x 3 bits, less one bit per subset anchor:
        // partition 0's anchors are pixels 0, 3 and 15.
        b.put(0, 2); // pixel 0 anchors subset 0
        for pixel in 1..16usize {
            let bits = if pixel == 3 || pixel == 15 { 2 } else { 3 };
            b.put(0, bits);
        }
        assert_eq!(b.pos, 128);

        let px = bc7_pixels(b.bytes());
        // 15 << 1 | p = 31 quantised into 5 bits expands to 255.
        assert_eq!(px[0], [255, 8, 8, 255], "subset 0");
        assert_eq!(px[2], [8, 255, 8, 255], "subset 1");
        assert_eq!(px[9], [8, 8, 255, 255], "subset 2");
    }

    #[test]
    fn bc7_anchor_pixel_skips_its_top_index_bit() {
        // Mode 4: one subset, two index sets (2-bit colour, 3-bit alpha). Make
        // the alpha endpoints identical so the alpha indices cannot matter, and
        // check that pixel 0's colour index is read one bit short: writing a
        // single "1" bit for it must read as index 1 (a quarter of the way), not
        // as the start of the next pixel's index.
        let mut b = Block::new();
        b.put(0b1_0000, 5); // mode 4
        b.put(0, 2); // no rotation
        b.put(0, 1); // colour uses index set 0
        b.put(0, 5); // colour endpoints: black then white, per channel
        b.put(31, 5);
        b.put(0, 5);
        b.put(31, 5);
        b.put(0, 5);
        b.put(31, 5);
        for _ in 0..2 {
            b.put(63, 6); // opaque alpha at both endpoints
        }
        b.put(1, 1); // pixel 0, index 1 written one bit short
        for _ in 1..16 {
            b.put(3, 2); // every other pixel: index 3 = the far endpoint
        }
        b.put(0, 2); // alpha indices, one bit short at the anchor too
        for _ in 1..16 {
            b.put(0, 3);
        }
        assert_eq!(b.pos, 128);

        let px = bc7_pixels(b.bytes());
        // Index 1 of {0, 21, 43, 64} over 64: 21/64 of white.
        assert_eq!(px[0], [84, 84, 84, 255], "the anchor pixel");
        assert_eq!(px[1], [255, 255, 255, 255]);
        assert_eq!(px[15], [255, 255, 255, 255]);
    }

    #[test]
    fn bc7_an_invalid_block_decodes_to_transparent_black() {
        // Eight zero bits is not a mode; the specification says zero-fill.
        let px = bc7_pixels([0u8; 16]);
        assert!(px.iter().all(|p| *p == [0, 0, 0, 0]), "{px:?}");
    }

    /// BC6H's 32 partition shapes are BC7's first 32, and the decoder shares
    /// BC7's table rather than copying it.
    ///
    /// The Khronos `EXT_texture_compression_bptc` specification says so in as
    /// many words: "The partition index references the first half of Table.P2.
    /// Indices are read in the same way as the fixed-point formats including
    /// obeying the anchor values for index 0 and as needed by Table.A2."
    /// [`BC6H_PUBLISHED_SHAPES`] is that first half, and the test below checks
    /// the shared rows against it row for row — so "shares BC7's first half"
    /// is a claim with a number behind it, not a hope.
    ///
    /// What else is checked here needs no second reference: pixel 0 is region 0
    /// in every shape (which is what makes pixel 0 the region-0 anchor, and
    /// what the specification states), each named fix-up index really is a
    /// region-1 pixel, and both regions are non-empty in every shape. That the
    /// decode *uses* the shape is `a_bc6h_partition_index_changes_the_decode`
    /// and `a_bc6h_shape_moves_pixels_between_the_two_regions` below, and that
    /// it matches other decoders on real encoder output is measured — see
    /// `docs/formats/materials.md` § "BC6H".
    #[test]
    fn bc6h_shares_the_first_half_of_the_bc7_partition_table() {
        for i in 0..32usize {
            let mask = BC7_PARTITION_2[i];
            let anchor = BC7_ANCHOR_2[i] as usize;
            assert_eq!(mask & 1, 0, "shape {i}: pixel 0 is region 0");
            assert_eq!(
                (mask >> anchor) & 1,
                1,
                "shape {i}: the fix-up index {anchor} must be a region-1 pixel"
            );
            assert_ne!(mask, 0, "shape {i}: region 1 has no pixels");
            assert_ne!(mask, 0xffff, "shape {i}: region 0 has no pixels");
            // The published listing, row for row.
            assert_eq!(mask, BC6H_PUBLISHED_SHAPES[i].0, "shape {i}: subset mask");
            assert_eq!(
                anchor, BC6H_PUBLISHED_SHAPES[i].1 as usize,
                "shape {i}: fix-up pixel"
            );
        }
        // A five-bit partition index cannot reach past these 32: the table is
        // read as BC7_PARTITION_2[partition] with `partition < 32`.
        assert!(BC7_PARTITION_2.len() >= 32);
    }

    /// The mode table itself, against the published one. This is the check that
    /// makes `BC6H_MODES` auditable: it compares the whole thing field by field
    /// — every slot, every bit offset, every width, every reversed field — plus
    /// the width of the mode field, against [`BC6H_PUBLISHED_FIELDS`], which is
    /// the specification's own `Table.F` transcribed independently.
    ///
    /// A transposition of two fields, a wrong width or a dropped `rev` all show
    /// up here; nothing in the decoder's own arithmetic can hide one.
    #[test]
    fn bc6h_mode_table_matches_the_published_field_table() {
        assert_eq!(BC6H_MODES.len(), BC6H_PUBLISHED_FIELDS.len());
        for (m, (published, mode_bits)) in BC6H_MODES.iter().zip(&BC6H_PUBLISHED_FIELDS) {
            assert_eq!(m.fields, *published, "mode {}", m.number);
            // Two modes stop after two bits; the other twelve spend five.
            let expected = if m.number < 2 { 2 } else { 5 };
            assert_eq!(*mode_bits, expected, "mode {}: mode field width", m.number);
        }
        // The same pairing by mode *number*, not by table position, so a
        // reordering of either list is caught too.
        for (m, published) in BC6H_MODES.iter().zip(&BC6H_PUBLISHED_FIELDS) {
            let same = BC6H_MODES
                .iter()
                .find(|o| o.number == m.number)
                .expect("the mode is in the table");
            assert_eq!(same.fields, published.0, "mode {} by number", m.number);
        }
    }

    /// Every mode's fields have to add up to the block, and the sum has to be
    /// 128: mode field + endpoint fields + partition index + one index per
    /// pixel (one bit fewer at each region's fix-up pixel). A typo in a width,
    /// a missing field or an extra one all change it.
    #[test]
    fn bc6h_mode_table_fills_the_block_exactly() {
        for m in BC6H_MODES.iter() {
            let mode_bits = if m.number < 2 { 2 } else { 5 };
            let endpoint_bits: u32 = m.fields.iter().map(|f| f.width as u32).sum();
            let partition_bits = if m.two_regions { 5 } else { 0 };
            // Per channel: the base endpoint plus one delta per region.
            let per_channel = m.base_bits as u32
                + m.delta_bits[..if m.two_regions { 3 } else { 1 }]
                    .iter()
                    .map(|b| *b as u32)
                    .sum::<u32>();
            assert_eq!(
                endpoint_bits,
                3 * per_channel,
                "mode {}: the fields must account for every endpoint bit",
                m.number
            );
            let index_bits: u32 = (0..16u32)
                .map(|p| {
                    let fixup = if m.two_regions {
                        p == 0 || p == BC7_ANCHOR_2[0] as u32
                    } else {
                        p == 0
                    };
                    if fixup {
                        (m.index_bits - 1) as u32
                    } else {
                        m.index_bits as u32
                    }
                })
                .sum();
            assert_eq!(
                mode_bits + endpoint_bits + partition_bits + index_bits,
                128,
                "mode {}: the block must be filled exactly",
                m.number
            );
        }
    }

    /// The partition index must matter: two blocks that differ only in those
    /// five bits must not decode the same. (A decoder that read the partition
    /// from the wrong place, or not at all, would fail this.)
    #[test]
    fn a_bc6h_partition_index_changes_the_decode() {
        // Bit 77 is where a two-region mode's five-bit partition index starts.
        // Two independent sources agree: the published "BC6H Format" mode table
        // gives mode 9 (`11010`, stored as 26 here) 5 mode bits + 72 colour
        // endpoint bits + a 5-bit partition + 46 index bits, and the mode table
        // in this file lists 72 bits of endpoint fields for mode 26, so the index
        // starts at 5 + 72 = 77. (`tests/bc6h.rs` pins the same offset.)
        const PARTITION_AT: u32 = 77;
        const PARTITION_MASK: u128 = 0x1f << PARTITION_AT;
        let mut changed = 0;
        for block in bc6h_random_blocks(26, 64, 0x9E3D) {
            let value = u128::from_le_bytes(block);
            let shape0 = value & !PARTITION_MASK;
            let shape31 = shape0 | PARTITION_MASK;
            // The two blocks differ in those five bits and nowhere else, so any
            // difference in the output is the shape's doing and nothing else's.
            assert_eq!(shape0 ^ shape31, PARTITION_MASK);
            // Each decode needs its own buffer and has to be read back out of
            // that buffer. `bc6h_block` returns *nothing* — it fills the array
            // it is handed — so the calls cannot be compared as return values.
            let (mut a, mut b) = ([0u16; 64], [0u16; 64]);
            bc6h_block(&shape0.to_le_bytes(), false, &mut a);
            bc6h_block(&shape31.to_le_bytes(), false, &mut b);
            if a != b {
                changed += 1;
            }
        }
        assert_eq!(changed, 64, "every block's decode must depend on its shape");
    }

    /// The lock the test above needs: a decoder that read the endpoints and the
    /// weights but handed *every* pixel to the same region would leave that test
    /// passing for the wrong reason. Build the clearest block the format allows
    /// — every base endpoint black, every region-1 near endpoint as far from
    /// black as the deltas reach — and check the shape really does move pixels
    /// between the two palettes.
    ///
    /// Which endpoints that is depends on the slot naming, so it is spelled out
    /// rather than assumed: the slots are numbered channel-first (red 0..3,
    /// green 4..7, blue 8..11) and the *letter* in a field's name is which of
    /// the mode's four endpoints it belongs to — `W` is the base endpoint, `X`,
    /// `Y` and `Z` the three others. So region 0's two endpoints are `W` and
    /// `X`, region 1's are `Y` and `Z`, and an index of zero puts a pixel on
    /// its own region's *near* endpoint, which is `W` for region 0 and `Y` for
    /// region 1. That is the endpoint this block sets: a delta of all ones in
    /// its own width is -1 (the deltas are signed), which the inverse transform
    /// wraps from a base of zero to 255 — the all-ones endpoint, and so the top
    /// of the range.
    #[test]
    fn a_bc6h_shape_moves_pixels_between_the_two_regions() {
        // Mode 26 is `8555`: an 8-bit base endpoint per channel and 5/5/6-bit
        // deltas. The field order below is the mode table's, read off it rather
        // than retyped, so the two cannot drift apart silently.
        let mode = &BC6H_MODES[BC6H_MODE_NUMBERS.iter().position(|n| *n == 26).unwrap()];
        let mut b = Block::new();
        b.put(mode.number as u32, 5);
        for f in mode.fields {
            b.put(
                if matches!(f.slot, RY | GY | BY) {
                    (1u32 << f.width) - 1
                } else {
                    0
                },
                f.width,
            );
        }
        b.put(0, 5); // partition index: shape 0, region 1 at pixels 2,3,6,7,10,11,14,15
        for pixel in 0..16usize {
            let anchor = pixel == 0 || pixel == BC7_ANCHOR_2[0] as usize;
            b.put(0, if anchor { 2 } else { 3 }); // every index zero
        }
        assert_eq!(b.pos, 128, "the block must be filled exactly");
        let bytes = b.bytes();

        let mut out = [0u16; 64];
        bc6h_block(&bytes, false, &mut out);
        let alpha = 0x3C00u16; // 1.0: BC6H has no alpha channel
        let at = |p: usize| [out[p * 4], out[p * 4 + 1], out[p * 4 + 2], out[p * 4 + 3]];
        // What region 1's near endpoint decodes to: 255 at eight bits is all
        // ones, which unquantises to 0xFFFF and finishes to the largest finite
        // half. Every channel agrees because every delta was set the same way.
        let white = 0x7BFFu16;
        for y in 0..4usize {
            for x in 0..4usize {
                let p = y * 4 + x;
                let px = at(p);
                let region1 = (BC7_PARTITION_2[0] >> p) & 1 == 1;
                assert_eq!(px[3], alpha, "alpha is 1.0 at ({x},{y})");
                if region1 {
                    assert_eq!(
                        px,
                        [white, white, white, alpha],
                        "({x},{y}) is a region-1 pixel and sits on region 1's palette"
                    );
                } else {
                    assert_eq!(px, [0, 0, 0, alpha], "({x},{y}) is a region-0 pixel");
                }
            }
        }
        // Both regions have to be non-empty for the above to mean anything, and
        // the two shapes the test compares have to be different shapes.
        assert_eq!(
            BC7_PARTITION_2[0].count_ones(),
            8,
            "shape 0 splits the tile"
        );
        assert_ne!(
            BC7_PARTITION_2[1], BC7_PARTITION_2[0],
            "two distinct shapes"
        );

        // Change nothing but the five partition bits and the pixels that change
        // colour must be exactly the pixels the two shapes disagree about. That
        // is the shape doing the work, not the endpoints: the two blocks are
        // identical everywhere else.
        let mut shape1 = Block::new();
        shape1.put(mode.number as u32, 5);
        for f in mode.fields {
            shape1.put(
                if matches!(f.slot, RY | GY | BY) {
                    (1u32 << f.width) - 1
                } else {
                    0
                },
                f.width,
            );
        }
        shape1.put(1, 5); // shape 1: region 1 at pixels 3, 7, 11 and 15 only
        for pixel in 0..16usize {
            let anchor = pixel == 0 || pixel == BC7_ANCHOR_2[1] as usize;
            shape1.put(0, if anchor { 2 } else { 3 });
        }
        assert_eq!(shape1.pos, 128);
        let mut other = [0u16; 64];
        bc6h_block(&shape1.bytes(), false, &mut other);
        let other_at = |p: usize| {
            [
                other[p * 4],
                other[p * 4 + 1],
                other[p * 4 + 2],
                other[p * 4 + 3],
            ]
        };
        for p in 0..16usize {
            let in0 = (BC7_PARTITION_2[0] >> p) & 1 == 1;
            let in1 = (BC7_PARTITION_2[1] >> p) & 1 == 1;
            let white_now = other[p * 4] != 0;
            assert_eq!(
                white_now,
                in1,
                "pixel {p}: shape 1 puts it {}",
                if in1 { "in region 1" } else { "in region 0" }
            );
            if in0 != in1 {
                assert_ne!(
                    at(p),
                    other_at(p),
                    "pixel {p}: the shapes disagree, so its value must move"
                );
            } else {
                assert_eq!(at(p), other_at(p), "pixel {p}: same region, same value");
            }
        }

        // Non-vacuity, in the sense the movement tests use elsewhere: a decoder
        // that ignored the partition and gave every pixel region 0 would produce
        // exactly this tile, and a decoder that ignored which palette a pixel
        // belongs to and gave every pixel region 1 would produce that one —
        // neither is what this block decodes to, so the assertions above are not
        // passing because the palette cannot matter.
        let tile_of = |rgb: [u16; 3]| {
            let mut t = [0u16; 64];
            for px in t.chunks_mut(4) {
                px.copy_from_slice(&[rgb[0], rgb[1], rgb[2], alpha]);
            }
            t
        };
        assert_ne!(
            &out[..],
            &tile_of([0, 0, 0])[..],
            "a decoder that gave every pixel region 0 would paint this all black"
        );
        assert_ne!(
            &out[..],
            &tile_of([white, white, white])[..],
            "a decoder that gave every pixel region 1 would paint this all white"
        );
    }

    /// Where the partition index actually sits, derived from the mode table
    /// rather than asserted: the mode field, then every endpoint field, then the
    /// partition index. The partition test above hard-codes the answer, so this
    /// is the test that keeps that constant honest.
    ///
    /// Every two-region mode lands on the same bit, which is why one constant
    /// serves all ten of them: modes 0 and 1 spend two bits on the mode and 75
    /// on endpoints, modes 2..30 spend five and 72.
    #[test]
    fn a_two_region_mode_puts_its_partition_index_at_bit_77() {
        let mut checked = 0;
        for m in BC6H_MODES.iter() {
            if !m.two_regions {
                continue;
            }
            let mode_bits = if m.number < 2 { 2 } else { 5 };
            let endpoint_bits: u32 = m.fields.iter().map(|f| f.width as u32).sum();
            assert_eq!(
                mode_bits + endpoint_bits,
                77,
                "mode {}: partition index offset",
                m.number
            );
            checked += 1;
        }
        assert_eq!(checked, 10, "ten modes have two regions");
        // And the index run that follows the partition is 46 bits, less one at
        // each region's fix-up pixel, which is what makes the block add up.
        for m in BC6H_MODES.iter().filter(|m| m.two_regions) {
            let index_bits: u32 = (0..16)
                .map(|p| {
                    let anchor = p == 0 || p == BC7_ANCHOR_2[0] as usize;
                    u32::from(if anchor {
                        m.index_bits - 1
                    } else {
                        m.index_bits
                    })
                })
                .sum();
            assert_eq!(index_bits, 46, "mode {}", m.number);
        }
    }

    /// The fourteen mode numbers, the mode field's two widths, and the
    /// reserved ones.
    ///
    /// The mode field is *not* a flat five bits: the low two bits are the whole
    /// mode when they are below two, and otherwise three more bits make five.
    /// Both published sources say so (Microsoft's mode table prints the mode
    /// bits per mode; the Khronos extension says it in prose), and the four
    /// reachable reserved numbers are the ones the rule leaves over.
    #[test]
    fn a_bc6h_mode_field_is_two_bits_or_five() {
        assert_eq!(BC6H_MODE_NUMBERS.len(), 14);
        for n in BC6H_MODE_NUMBERS {
            assert!(bc6h_mode(n).is_some(), "mode {n} must be a real mode");
        }
        // Reachable: 0 and 1 through the two-bit rule, and every number from 2
        // up whose low two bits are 2 or 3 through the five-bit rule.
        let reachable: Vec<u8> = (0u8..32).filter(|n| *n < 2 || n & 3 >= 2).collect();
        let reserved: Vec<u8> = reachable
            .iter()
            .copied()
            .filter(|n| !BC6H_MODE_NUMBERS.contains(n))
            .collect();
        assert_eq!(reserved, [19, 23, 27, 31], "the reserved mode numbers");
        // A reserved mode is not an error: it decodes to black at alpha 1.0.
        for n in reserved {
            let mut block = [0u8; 16];
            block[0] = n;
            let mut out = [0u16; 64];
            bc6h_block(&block, false, &mut out);
            assert!(
                out.chunks(4).all(|p| p == [0, 0, 0, 0x3C00]),
                "reserved mode {n} must decode to black at alpha 1.0"
            );
        }
    }

    /// Each of the fourteen modes, on a payload of all ones, has to decode through
    /// its own arithmetic rather than fall through to the reserved branch, which
    /// paints a whole tile black at alpha 1.0.
    ///
    /// The check is per *tile*, not per pixel, and it has to be: mode 15 read as
    /// signed puts both of its endpoints at -1 (all sixteen bits set
    /// sign-extends to -1, and the four-bit delta of -1 wraps the sum to -2),
    /// and the specification's last step — negate, scale by 31/32, or in the
    /// sign bit on the scaled value — maps -1 to a magnitude of zero. So the
    /// anchor pixel of that one block comes out as the bit pattern 0x0000, which
    /// is the same pattern the reserved branch produces. The two are
    /// indistinguishable *at that pixel*; they are not indistinguishable as
    /// tiles, which is why the tile is what is compared. (`bcdec` does exactly
    /// the same arithmetic here, so this is the format's behaviour and not ours:
    /// see the cross-decoder numbers in `docs/formats/materials.md`.)
    #[test]
    fn all_fourteen_bc6h_modes_decode_through_their_own_arithmetic() {
        let black = [0u16, 0, 0, 0x3C00];
        // What the reserved branch returns for *any* payload: the whole tile
        // black. Built once, from the decoder itself, by handing it a reserved
        // mode number, so this is not a hand-written guess at it.
        let mut reserved_block = [0xffu8; 16];
        reserved_block[0] = 0b10011 | 0xe0; // a reserved mode number, bits 4..0
        let mut reserved_out = [0u16; 64];
        bc6h_block(&reserved_block, false, &mut reserved_out);
        assert!(
            reserved_out.chunks(4).all(|p| *p == black),
            "the reserved branch is a whole tile of black at alpha 1.0"
        );
        for mode in BC6H_MODE_NUMBERS {
            for signed in [false, true] {
                let mut block = [0xffu8; 16];
                block[0] = (mode & 0x1f) | (block[0] & 0xe0);
                let mut out = [0u16; 64];
                bc6h_block(&block, signed, &mut out);
                // The mode field has to have been read: the same payload under a
                // reserved mode number is the reserved tile, so an equal result
                // would mean this block took the reserved branch instead.
                assert_ne!(
                    &out[..],
                    &reserved_out[..],
                    "mode {mode} signed={signed} decoded to the reserved fill"
                );
                assert!(
                    out.chunks(4).all(|p| p[3] == 0x3C00),
                    "mode {mode} signed={signed}: alpha is always 1.0"
                );
                // Nothing comes out as infinity or NaN, which is the property a
                // viewer depends on: the values go straight into a half-float
                // buffer, where an all-ones exponent is a NaN.
                for (i, h) in out.iter().enumerate() {
                    assert!(
                        h & 0x7c00 != 0x7c00,
                        "mode {mode} signed={signed}: channel {i} is 0x{h:04x}"
                    );
                }
            }
        }
        // The fourteen are fourteen *different* decoders, so they cannot all
        // agree on the same payload either: a mode-blind decode would produce
        // one answer for all fourteen.
        let mut distinct: Vec<[u16; 64]> = Vec::new();
        for mode in BC6H_MODE_NUMBERS {
            let mut block = [0xffu8; 16];
            block[0] = (mode & 0x1f) | (block[0] & 0xe0);
            let mut out = [0u16; 64];
            bc6h_block(&block, false, &mut out);
            if !distinct.contains(&out) {
                distinct.push(out);
            }
        }
        assert!(
            distinct.len() > 1,
            "fourteen modes decoded one payload identically, so at least one of \
             them is not being read: {} distinct answers",
            distinct.len()
        );
    }

    /// The two values the unquantisation cannot reach with a shift: zero and
    /// all ones. Both are special-cased, because stretching them would give the
    /// wrong answer — an all-ones endpoint has to land on the largest finite
    /// half exactly (65504) rather than one step below it, so the top of the
    /// range is representable rather than merely approached.
    ///
    /// 6-bit endpoints (mode 30) show it at its clearest: 0 stays 0, and 63 is
    /// all ones at six bits.
    #[test]
    fn a_bc6h_endpoint_at_zero_or_all_ones_hits_the_exact_extreme() {
        // Unsigned: zero is exactly zero, all ones is exactly 0xFFFF.
        assert_eq!(bc6h_unquantize(0, 6, false), 0);
        assert_eq!(bc6h_unquantize(63, 6, false), 0xFFFF);
        assert_eq!(bc6h_unquantize(62, 6, false), ((62 << 16) + 0x8000) >> 6);
        // Non-vacuity: the fixup is not a different spelling of the plain
        // stretch. Without the two special cases `((v << 16) + 0x8000) >> bits`
        // would put zero at 512 — a small non-zero value, 2^-14 in half
        // floats, instead of exact zero — and all ones at 65024, which the
        // 31/64 scaling then turns into 0x7B08: the top of the range is 247
        // counts (0.8% of full scale) short of representable, and the bottom is
        // not reachable at all.
        let plain_stretch = |v: i32, bits: u8| ((v << 16) + 0x8000) >> bits;
        assert_eq!(
            plain_stretch(0, 6),
            512,
            "what the un-fixed stretch says of zero"
        );
        assert_eq!(plain_stretch(63, 6), 65024, "and of all ones");
        assert_eq!(bc6h_finish_unquantize(plain_stretch(63, 6), false), 0x7B08);
        // And the value that comes out must be a real number, not an infinity:
        // a half float with an all-ones exponent is 0x7C00 or above. The last
        // step scales by 31/64, so the fixed-up 0xFFFF is the largest finite
        // half, 0x7BFF = 65504.
        assert_eq!(bc6h_finish_unquantize(0, false), 0x0000);
        assert_eq!(bc6h_finish_unquantize(0xFFFF, false), 0x7BFF);
        assert_ne!(
            bc6h_finish_unquantize(0xFFFF, false) & 0x7C00,
            0x7C00,
            "the fixed-up value must not be an infinity"
        );
        // Signed: the magnitude saturates at 0x7FFF and the sign goes on top.
        assert_eq!(bc6h_unquantize(0, 6, true), 0);
        assert_eq!(bc6h_unquantize(31, 6, true), 0x7FFF);
        assert_eq!(bc6h_unquantize(-31, 6, true), -0x7FFF);
        assert_eq!(bc6h_unquantize(30, 6, true), ((30 << 15) + 0x4000) >> 5);
        assert_eq!(bc6h_finish_unquantize(0x7FFF, true), 0x7BFF);
        assert_eq!(bc6h_finish_unquantize(-0x7FFF, true), 0xFBFF);
        // A 15-bit or wider endpoint is already the whole range, so the fixup
        // is skipped rather than applied wrongly.
        assert_eq!(bc6h_unquantize(65535, 16, false), 65535);
        assert_eq!(bc6h_unquantize(32767, 16, true), 32767);
        // And the fixup really is what the decoder runs, not just a helper: a
        // mode-30 block with a base endpoint of all ones and an all-ones index
        // puts that 0x7BFF in every pixel.
        let mode = &BC6H_MODES[BC6H_MODE_NUMBERS.iter().position(|n| *n == 30).unwrap()];
        let mut b = Block::new();
        b.put(30, 5);
        for f in mode.fields {
            b.put((1u32 << f.width) - 1, f.width);
        }
        b.put(0x1f, 5); // partition 31
        for pixel in 0..16usize {
            let anchor = pixel == 0 || pixel == BC7_ANCHOR_2[31] as usize;
            b.put(3, if anchor { 2 } else { 3 }); // the far endpoint
        }
        assert_eq!(b.pos, 128);
        let mut out = [0u16; 64];
        bc6h_block(&b.bytes(), false, &mut out);
        for p in 0..16usize {
            assert_eq!(
                [out[p * 4], out[p * 4 + 1], out[p * 4 + 2]],
                [0x7BFF, 0x7BFF, 0x7BFF],
                "pixel {p}"
            );
        }
    }

    #[test]
    fn rgba16_float_and_r16_formats_decode_their_channels() {
        // R16G16B16A16_FLOAT: DXGI 10, no offset (below the client's +1 range).
        let mut d = texture_prefix(10, 2, 1, 16);
        for half in [0x3C00u16, 0x0000, 0x3800, 0x0000] {
            d.extend_from_slice(&half.to_le_bytes()); // 1.0, 0, 0.5, 0
        }
        for half in [0x0000u16, 0x3800, 0x3C00, 0x3C00] {
            d.extend_from_slice(&half.to_le_bytes()); // 0, 0.5, 1.0, 1.0
        }
        let t = parse(&d).expect("parses");
        assert_eq!(t.pixel(0, 0), [255, 0, 128, 0]);
        assert_eq!(t.pixel(1, 0), [0, 128, 255, 255]);

        // R16_FLOAT: stored 49 is DXGI 48; 0.25 -> 64 in red.
        let mut d = texture_prefix(49, 1, 1, 2);
        d.extend_from_slice(&0x3400u16.to_le_bytes());
        let t = parse(&d).expect("parses");
        assert_eq!(t.pixel(0, 0), [64, 0, 0, 255]);

        // R16_UNORM: stored 51 is DXGI 50; half of full scale -> 128.
        let mut d = texture_prefix(51, 1, 1, 2);
        d.extend_from_slice(&32768u16.to_le_bytes());
        let t = parse(&d).expect("parses");
        assert_eq!(t.pixel(0, 0), [128, 0, 0, 255]);
    }

    #[test]
    fn to_dds_writes_a_bc1_dds() {
        let mut d = texture_prefix(71, 4, 4, 8);
        d.extend_from_slice(&0xF800u16.to_le_bytes());
        d.extend_from_slice(&0u16.to_le_bytes());
        d.extend_from_slice(&0u32.to_le_bytes());
        let t = parse(&d).expect("parses");
        let dds = t.to_dds().expect("dds");
        assert_eq!(&dds[0..4], b"DDS ");
        assert!(dds.len() > 128);
    }

    #[test]
    fn an_embedded_payload_is_found_behind_its_header() {
        let mut d = vec![0u8; 0x20];
        d[3] = 71; // BC1
        d[6] = 1; // payload_count > 0 -> payload at 0x20
        d[8..10].copy_from_slice(&4u16.to_le_bytes());
        d[10..12].copy_from_slice(&4u16.to_le_bytes());
        // payload header: mips, surfaces, image_size, header_size
        d.extend_from_slice(&1u32.to_le_bytes());
        d.extend_from_slice(&1u32.to_le_bytes());
        d.extend_from_slice(&8u32.to_le_bytes());
        d.extend_from_slice(&16u32.to_le_bytes());
        d.extend_from_slice(&0xF800u16.to_le_bytes());
        d.extend_from_slice(&0u16.to_le_bytes());
        d.extend_from_slice(&0u32.to_le_bytes());
        let t = parse(&d).expect("parses");
        assert_eq!(t.pixel(0, 0), [255, 0, 0, 255]);
    }
}

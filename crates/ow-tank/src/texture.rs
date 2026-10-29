//! Textures: the last hop of the model → material → texture chain.
//!
//! A texture asset (type `004`) starts with the 32-byte header the texture load
//! callback at `0x141dacf00` reads:
//!
//! ```text
//! +0x00  u16 flags        +0x02 u8 mip_count    +0x03 u8 format
//! +0x04  u8  surfaces     +0x05 u8 usage        +0x06 u8 payload_count
//! +0x07  u8  unknown      +0x08 u16 width       +0x0a u16 height
//! +0x0c  u32 total_size   +0x10 u8 base_mips    +0x11 u8 chunk_mask
//! +0x12  u8  reserved     +0x13..+0x1f unread by the loader
//! ```
//!
//! The format byte is the **client's engine enum**, not a DXGI id. The client
//! maps it through two tables: `0x143356f50` (0x72 8-byte records, byte 0) gives
//! an internal renderer format number, and `0x14348f500` (u32) gives the DXGI
//! id for that number. [`dxgi_of`] is that composition. `total_size` is the sum
//! over every mip and surface; it is a u32, not a u16. The stored value is the
//! on-disk block sum ([`cooked_size`]), which equals the runtime size walk
//! (`0x142e37070`, [`size_walk`]) whenever the halved dimensions stay on block
//! boundaries — on 5 of 400 sampled assets (non-power-of-two) the runtime walk
//! is slightly smaller. The loader's validation (`0x141dacf00`) is exactly:
//! format <= 0x71, mip_count <= 0x10, width != 0, height != 0,
//! payload_count < 5, base_mips <= mip_count.
//!
//! With `payload_count == 0` the block run starts at `+0x20` and is
//! `total_size` bytes long; the descriptor's `+0x38`/`+0x40` fields are exactly
//! that pointer and size, and the renderer's no-payload branch (`0x141e29210`)
//! builds the whole chain from them. With `payload_count >= 1` an embedded
//! chunk header sits at `+0x20`:
//!
//! ```text
//! +0x00 u32 first_from_smallest   +0x04 u32 level_count
//! +0x08 u32 image_size            +0x0c u32 header_size
//! ```
//!
//! The four fields are read through `0x142e38740` (first), `0x142255e10`
//! (count), `0x142e38750` (image size) and `0x142e38730` (the data pointer is
//! `chunk + header_size`). Its data starts at `+0x20 + header_size`.
//! `first_from_smallest` is the distance from the *smallest* mip, so the
//! chunk's first level is `mip_count - 1 - first_from_smallest`. A separate
//! payload asset (GUID from [`crate::guid::payload_of`]) has the same 16-byte
//! header at its own `+0x00`, and its chunk header says which levels it holds.
//! That is the selection rule: the decoder reads every payload the header
//! names (`1..=payload_count-1`) and places each run by its own
//! `first_from_smallest`/`level_count`. The loader requests the immediate set
//! from [`TextureHeader::requested_payloads`] and fetches the highest index
//! lazily when `chunk_mask` bit 3 marks it streamed; the bytes are the same.
//! On the six pinned assets the highest index happens to hold level 0 — an
//! install observation, not the rule.
//!
//! The byte order inside one run is read from the client's record builder
//! `0x141e296a0`: the outer loop is the array element (six faces for a cube),
//! the inner loop is the mip from `first_from_smallest`'s level upwards, and
//! the data pointer advances by each level's own size. The level's resource
//! dimensions are `max(w >> mip, 1)` (`0x142e2db70`/`0x142e2db50`,
//! [`level_dimensions`]); its stored size is `0x142e36ec0`'s block-ceil
//! arithmetic ([`level_byte_size`]). The record index is the D3D12 subresource
//! number `(array * faces + face) * mip_count + mip` (`0x142e36e80`; `faces`
//! is 6 for renderer texture types 4/7), and the copy loop in `0x141f0a000`
//! walks those records with its record-array flag bit 1 set, so its index is
//! `mip + surface * mip_count` (`0x141f0a53a`), where `surface` is the flat
//! `array * faces + face`. A run is therefore **surface-major, largest mip
//! first within each surface**. The D3D12 resource descriptor
//! (`0x1426545d0`) gets `mip_count` levels and `surfaces` array layers (six
//! per cube, from the renderer kind; kinds 0/1 are 1D and 8 is 3D). The viewer
//! uploads the same subresources/levels but always creates a 2D texture and
//! binds a 2D view of layer 0: the 1D resource/SRV dimension, the
//! cube-vs-array view dimension and the cube-face direction are open. An asset
//! whose format has a half-float form (BC6H, R16G16B16A16_FLOAT, R16_FLOAT,
//! DXGI 95/96/10/54) is uploaded with its exact halves; every other decoded
//! format is uploaded as the clamped RGBA8 view described on
//! [`half_to_unorm8`]. The upload's `Rgba16Float` resource is a **viewer
//! substitution** for those client DXGI ids, not the client's own resource
//! format.
//!
//! Decoding is ours: BC1/BC2/BC3/BC4/BC5/BC6H/BC7 and the uncompressed families
//! are implemented here, with synthetic block tests. Formats the client maps but
//! this decoder has no path for (the signed BC4/BC5 variants, BGRA, the depth
//! and video families) are reported as unsupported rather than mis-decoded.
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
    /// The client's loader would reject this header. The payload is the client
    /// check that failed, in the order the loader applies them.
    Invalid(&'static str),
    /// A mapped format this decoder does not implement. The value is the DXGI
    /// id when the client's tables resolve one, otherwise the stored engine
    /// byte.
    UnsupportedFormat(u8),
    NoData,
    /// A layout this decoder does not implement yet: the volume depth axis.
    /// The client creates a 3D resource for a `flags & 4` asset; every level
    /// list here is 2D. No such asset appears in the 400-asset sample.
    UnsupportedLayout(&'static str),
    /// The asset's level 0 is in a separate payload asset that the caller did
    /// not supply (index in the payload chain).
    Level0InPayload(u32),
    /// Payload `n` was requested from the loader and was not present.
    PayloadNotLoaded(u32),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Truncated(w) => write!(f, "truncated texture: {w}"),
            Error::Invalid(w) => write!(f, "invalid texture header: {w}"),
            Error::UnsupportedFormat(id) => write!(f, "unsupported texture format {id}"),
            Error::NoData => write!(f, "texture carries no payload data"),
            Error::UnsupportedLayout(w) => write!(f, "unsupported texture layout: {w}"),
            Error::Level0InPayload(i) => {
                write!(f, "level 0 is in payload {i}, which is a separate asset")
            }
            Error::PayloadNotLoaded(i) => write!(f, "payload {i} is not present"),
        }
    }
}

impl std::error::Error for Error {}

type Result<T> = std::result::Result<T, Error>;

/// The client's engine-format → renderer-format table, at `0x143356f50`.
///
/// The table is 0x72 records of 8 bytes; the client reads byte 0 of the record
/// (`(&DAT_143356f50)[format * 8]`), so only that byte is reproduced. The index
/// is the stored engine byte; the value is the renderer's internal format
/// number — the thing the docs call `pm`.
#[rustfmt::skip]
static ENGINE_TO_PM: [u8; 0x72] = [
    0, 54, 56, 54, 55, 51, 53, 51, 52, 48, 50, 46, 48, 47,
    49, 43, 45, 43, 44, 0, 135, 0, 0, 39, 38, 40, 39, 41,
    34, 31, 32, 34, 33, 35, 28, 30, 26, 28, 0, 29, 23, 132,
    25, 23, 24, 134, 134, 133, 0, 15, 12, 15, 14, 16, 9, 11,
    131, 7, 9, 8, 10, 4, 1, 4, 3, 5, 6, 0, 42, 0,
    0, 57, 57, 58, 59, 59, 60, 61, 61, 62, 63, 63, 64, 65,
    65, 66, 20, 21, 36, 36, 38, 36, 37, 36, 37, 67, 68, 67,
    69, 69, 70, 0, 0, 0, 95, 95, 81, 99, 99, 85, 102, 102,
    88, 136,
];

/// The renderer's format → DXGI table, at `0x14348f500`, one u32 per entry.
///
/// The largest value [`ENGINE_TO_PM`] produces is 136, so the first 137
/// entries are the ones the texture path can reach. Zero means the renderer
/// format has no DXGI format (the mobile block-compression families have no D3D
/// equivalent, and several engine ids are unused).
#[rustfmt::skip]
static PM_TO_DXGI: [u8; 137] = [
    0, 61, 0, 63, 62, 64, 65, 56, 58, 57, 59, 54, 49, 0,
    51, 50, 52, 0, 115, 0, 85, 0, 86, 42, 43, 41, 35, 37,
    36, 38, 34, 28, 29, 31, 30, 32, 87, 91, 24, 25, 0, 26,
    67, 17, 18, 16, 11, 13, 12, 14, 10, 7, 8, 6, 3, 4,
    2, 71, 72, 74, 75, 77, 78, 80, 81, 83, 84, 96, 95, 98,
    99, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 55, 40, 45, 45, 20, 103,
];

/// The DXGI id a stored engine format byte resolves to, or 0 when the client's
/// tables give it none. This is the composition the client performs:
/// `0x14348f500[0x143356f50[byte]]`.
pub fn dxgi_of(stored: u8) -> u8 {
    let Some(&pm) = ENGINE_TO_PM.get(stored as usize) else {
        return 0;
    };
    PM_TO_DXGI.get(pm as usize).copied().unwrap_or(0)
}

/// Whether a DXGI id is an sRGB format.
///
/// The five ids the install's colour textures resolve to — `R8G8B8A8_UNORM_SRGB`
/// (29), `BC1_UNORM_SRGB` (72), `BC2_UNORM_SRGB` (75), `BC3_UNORM_SRGB` (78) and
/// `BC7_UNORM_SRGB` (99) — plus `B8G8R8A8_UNORM_SRGB` (91), which the same
/// tables reach through stored bytes `0x5C`/`0x5E`. The format id itself carries
/// the sRGB view; there is no separate client predicate. The material-side set
/// this replaces is `{30, 73, 76, 79, 100}`, exactly the stored bytes of the
/// five the install's material lists use.
pub fn is_srgb_dxgi(dxgi: u8) -> bool {
    matches!(dxgi, 29 | 72 | 75 | 78 | 91 | 99)
}

/// Whether a stored engine format byte is an sRGB (colour) format.
pub fn is_srgb_format(stored: u8) -> bool {
    is_srgb_dxgi(dxgi_of(stored))
}

/// The client's format → (family, bits per pixel) switch at `0x142e372b0`.
///
/// Family 0 is linear; 1..=10 are the block families (BC1, BC2, BC3, BC4, BC5,
/// BC6H, BC7 and the three EAC sizes). The bits-per-pixel value is what the
/// size walk multiplies by.
pub fn format_family_and_bpp(stored: u8) -> (u8, u32) {
    match stored {
        0 => (0, 0),
        1..=4 => (0, 0x80),
        5..=8 => (0, 0x60),
        9..=0x16 => (0, 0x40),
        0x17..=0x30 | 0x44..=0x46 | 0x58 | 0x59 | 0x5b | 0x5c => (0, 0x20),
        0x31..=0x3c | 0x56 | 0x57 => (0, 0x10),
        0x3d..=0x42 => (0, 8),
        0x43 => (0, 1),
        0x47..=0x49 | 0x65..=0x67 => (1, 4),
        0x4a..=0x4c => (2, 8),
        0x4d..=0x4f => (3, 8),
        0x50..=0x52 => (4, 4),
        0x53..=0x55 => (5, 8),
        0x5f..=0x61 => (6, 8),
        0x62..=0x64 => (7, 8),
        0x68..=0x6a => (8, 8),
        0x6b..=0x6d => (9, 4),
        0x6e..=0x70 => (10, 2),
        0x71 => (0, 0xc),
        _ => (0, 0),
    }
}

/// The family number from the switch above.
pub fn format_family(stored: u8) -> u8 {
    format_family_and_bpp(stored).0
}

/// Bits per pixel from the switch above.
pub fn format_bits_per_pixel(stored: u8) -> u32 {
    format_family_and_bpp(stored).1
}

/// Whether the size walk clamps the halved dimensions at 4 rather than 1.
///
/// This is the size walk's *own* switch (`0x142e37070`), not the family switch:
/// the EAC families `0x65..0x71` are block formats to the renderer but are not
/// in this list, so the walk treats them as linear and clamps at 1. That is the
/// client's arithmetic and is reproduced as read.
pub fn is_block_format(stored: u8) -> bool {
    matches!(stored, 0x47..=0x55 | 0x5f..=0x64)
}

/// The client's size walk (`0x142e37070`): the byte size of `mip_count` levels.
///
/// Each level adds `w * h * depth`, then width and height halve (clamped at 4
/// for the formats [`is_block_format`] names, otherwise at 1) and depth halves
/// (clamped at 1). The total is scaled by bits per pixel and surfaces:
/// `bpp * surfaces * total >> 3`. Zero width, height, depth, mip count or
/// surface count gives zero, exactly as the client's guard does. The client
/// uses 64-bit arithmetic, so this does too.
pub fn size_walk(
    stored: u8,
    mut width: u32,
    mut height: u32,
    mut depth: u32,
    mip_count: u32,
    surfaces: u32,
) -> u64 {
    let bpp = format_bits_per_pixel(stored) as u64;
    if width == 0 || height == 0 || depth == 0 || mip_count == 0 || surfaces == 0 {
        return 0;
    }
    let min = if is_block_format(stored) { 4 } else { 1 };
    let mut total: u64 = 0;
    for _ in 0..mip_count {
        total = total.wrapping_add(
            (width as u64)
                .wrapping_mul(height as u64)
                .wrapping_mul(depth as u64),
        );
        width = (width >> 1).max(min);
        height = (height >> 1).max(min);
        depth = (depth >> 1).max(1);
    }
    bpp.wrapping_mul(surfaces as u64)
        .wrapping_mul(total)
        .wrapping_shr(3)
}

/// The descriptor consumer `0x142e2dbb0`: the byte size of the mip chain from
/// `bias` down, as the renderer requests it from [`size_walk`].
///
/// The descriptor's width, height and (for a volume) depth fields are shifted
/// right by `bias & 0x1f` and clamped at 1 before the walk; a non-volume
/// descriptor has depth 1 and a volume descriptor has one surface. The mip
/// count is the 4-bit descriptor field minus `bias` with 32-bit wrap, and the
/// walk's 64-bit answer is truncated to 32 bits exactly as the client's
/// `undefined4` return is.
pub fn descriptor_size_walk(descriptor: u64, object_flags: u32, format: u8, bias: u32) -> u32 {
    let shift = bias & 0x1f;
    let field = (((descriptor >> 28) as u32) & 0x7ff) + 1;
    let (surfaces, depth) = if object_flags & 4 == 0 {
        (field, 1u32)
    } else {
        (1u32, field)
    };
    let width = ((((descriptor as u32) & 0x3fff) + 1) >> shift).max(1);
    let height = (((((descriptor >> 14) as u32) & 0x3fff) + 1) >> shift).max(1);
    let depth = (depth >> shift).max(1);
    let mip_count = (((descriptor >> 39) as u32) & 0xf).wrapping_sub(bias);
    size_walk(format, width, height, depth, mip_count, surfaces) as u32
}

/// The sum of the client's per-level sizes over the chain (`0x142e36ec0`).
///
/// The recorded `total_size` is the on-disk size: a block format's level is
/// `ceil(w/4) * ceil(h/4) * block bytes` (a 4x4 block is 16 pixels times the
/// format's bits per pixel, so BC1/BC4's 4 bpp is 8 bytes and the 8 bpp families
/// are 16), and a linear format's level is `w * h * bpp / 8`; levels halve
/// exactly as [`size_walk`] does. The per-level arithmetic is the read helper
/// `0x142e36ec0` — [`level_dimensions`] and [`level_byte_size`] are its halves —
/// and the recorded field equals this sum on all 400 sampled assets. The
/// client's `0x142e37260` is not a universal cooked-size formula: it returns 8
/// or 16 for a subset of ids and 0 for several accepted ones (`0x47`, `0x4A`,
/// `0x4D`, `0x50`, `0x52`, `0x53`, `0x55`, `0x5F`, `0x65`).
///
/// This sum and the runtime walk (`0x142e37070`) agree for a block texture while
/// every halved dimension stays on a multiple of 4, and the walk is smaller
/// otherwise: on this install 5 of a 400-asset sample are non-power-of-two and
/// their recorded `total_size` carries the block ceil.
///
/// Only the BC 4x4 families are handled here. The EAC/mobile groups are 6x6 or
/// 8x8 blocks (`0x142e371c0`) but are not decoded by this module, and no such
/// asset appeared in the 400-asset sample; their block size is **not tested**.
pub fn cooked_size(
    stored: u8,
    mut width: u32,
    mut height: u32,
    mut depth: u32,
    mip_count: u32,
    surfaces: u32,
) -> u64 {
    let bpp = format_bits_per_pixel(stored) as u64;
    if width == 0 || height == 0 || depth == 0 || mip_count == 0 || surfaces == 0 {
        return 0;
    }
    let block = is_block_format(stored);
    let min = if block { 4 } else { 1 };
    let mut total: u64 = 0;
    for _ in 0..mip_count {
        let level = if block {
            let bw = (width as u64).div_ceil(4);
            let bh = (height as u64).div_ceil(4);
            bw.wrapping_mul(bh)
                .wrapping_mul(depth as u64)
                .wrapping_mul(bpp * 2)
        } else {
            (width as u64)
                .wrapping_mul(height as u64)
                .wrapping_mul(depth as u64)
                .wrapping_mul(bpp)
                .wrapping_shr(3)
        };
        total = total.wrapping_add(level.wrapping_mul(surfaces as u64));
        width = (width >> 1).max(min);
        height = (height >> 1).max(min);
        depth = (depth >> 1).max(1);
    }
    total
}

/// The client's popcount helper `0x142e374d0`: the number of set bits in
/// `value >> ((bias + 2) & 0x1f)`.
///
/// The loader calls it with `bias = 1` (`0x142e376d0(DAT_143a7c390)` where the
/// image's `DAT_143a7c390` is 1), so the shift is 3. The helper is public so a
/// differential unit can drive the client's own function over every bias.
pub fn count_bits_shifted(value: u32, bias: u32) -> u32 {
    (value >> (bias.wrapping_add(2) & 0x1f)).count_ones()
}

/// The client's request-count leaf `0x142e37600`.
///
/// `payload_count`, `mip_count`, `base_mips` and `streamed` are the four
/// arguments in order. It returns 0 when `mip_count - base_mips <= streamed`,
/// otherwise `max(1, payload_count - streamed - 1)`. The loader only calls it
/// when `payload_count > 1`; the gate is in [`TextureHeader::request_count`],
/// not in this leaf.
pub fn request_count(payload_count: u32, mip_count: u32, base_mips: u32, streamed: u32) -> u32 {
    let payload = payload_count as i32;
    let mip = mip_count as i32;
    let base = base_mips as i32;
    let streamed = streamed as i32;
    if mip.wrapping_sub(base) <= streamed {
        0
    } else {
        payload.wrapping_sub(streamed).wrapping_sub(1).max(1) as u32
    }
}

/// The format family a stored engine byte decodes as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// BC1, 8 bytes per 4x4 block (DXGI 71/72).
    Bc1,
    /// BC2, 16 bytes per 4x4 block (DXGI 74/75).
    Bc2,
    /// BC3, 16 bytes per 4x4 block (DXGI 77/78).
    Bc3,
    /// BC4_UNORM, 8 bytes per 4x4 block (DXGI 80).
    Bc4,
    /// BC5_UNORM, 16 bytes per 4x4 block (DXGI 83).
    Bc5,
    /// BC4_SNORM (DXGI 81). The block layout is BC4's, but the endpoints are
    /// signed; this decoder has no signed-endpoint path, so it refuses rather
    /// than decode the bytes as the unsigned variant.
    Bc4Snorm,
    /// BC5_SNORM (DXGI 84), refused for the same reason.
    Bc5Snorm,
    /// BC7, 16 bytes per 4x4 block (DXGI 98/99).
    Bc7,
    /// BC6H, high-dynamic-range RGB stored as half floats.
    ///
    /// DXGI 95 is `BC6H_UF16` (unsigned endpoints) and 96 is `BC6H_SF16`
    /// (signed endpoints). The two variants decode the same fourteen modes; the
    /// flag says whether an endpoint carries a sign bit, which changes the
    /// arithmetic, not the layout.
    Bc6h { signed: bool },
    /// R8G8B8A8 (DXGI 28 UNORM, 29 sRGB, 30 UINT), four bytes per pixel.
    Rgba8,
    /// R16G16B16A16_FLOAT (DXGI 10): four half floats, 8 B/px.
    Rgba16Float,
    /// R16_FLOAT (DXGI 54): one half float, 2 B/px.
    R16Float,
    /// R16_UNORM (DXGI 56): one 16-bit unorm, 2 B/px. The decode keeps only a
    /// rounded 8-bit view ([`decode_r16_unorm`]); the raw `u16` is not
    /// preserved and `rgba16` stays empty (open).
    R16Unorm,
    /// R8G8 UNORM or UINT (DXGI 49/50): two byte channels, 2 B/px. The bytes
    /// are identical; UNORM is the byte/255 view and UINT is the same integer.
    Rg8,
    /// The stored engine byte maps to a format this decoder has no path for.
    /// The value is the **stored byte**, so a message names what the asset says;
    /// [`dxgi_of`] gives the DXGI id when the tables have one.
    Unknown(u8),
}

impl Default for Format {
    fn default() -> Self {
        Format::Unknown(0)
    }
}

impl Format {
    /// Stored engine format byte → (family, DXGI id).
    ///
    /// The second value is the DXGI id from [`dxgi_of`], or 0 when the client's
    /// tables give none. The family is the decoder path, and is `Unknown` for
    /// every DXGI id this module does not decode.
    pub fn from_header(byte: u8) -> (Format, u8) {
        let dxgi = dxgi_of(byte);
        let family = match dxgi {
            71 | 72 => Format::Bc1,
            74 | 75 => Format::Bc2,
            77 | 78 => Format::Bc3,
            80 => Format::Bc4,
            81 => Format::Bc4Snorm,
            83 => Format::Bc5,
            84 => Format::Bc5Snorm,
            95 => Format::Bc6h { signed: false },
            96 => Format::Bc6h { signed: true },
            98 | 99 => Format::Bc7,
            10 => Format::Rgba16Float,
            28..=30 => Format::Rgba8,
            49 | 50 => Format::Rg8,
            54 => Format::R16Float,
            56 => Format::R16Unorm,
            _ => Format::Unknown(byte),
        };
        (family, dxgi)
    }

    /// Bytes in one 4x4 block, or 0 for a family with no block grid.
    pub fn block_bytes(self) -> usize {
        match self {
            Format::Bc1 | Format::Bc4 | Format::Bc4Snorm => 8,
            Format::Bc2
            | Format::Bc3
            | Format::Bc5
            | Format::Bc5Snorm
            | Format::Bc7
            | Format::Bc6h { .. } => 16,
            _ => 0,
        }
    }

    /// Whether the format is block compressed.
    pub fn is_block(self) -> bool {
        matches!(
            self,
            Format::Bc1
                | Format::Bc2
                | Format::Bc3
                | Format::Bc4
                | Format::Bc4Snorm
                | Format::Bc5
                | Format::Bc5Snorm
                | Format::Bc7
                | Format::Bc6h { .. }
        )
    }

    /// Whether the format stores one channel, which lands in red with the other
    /// channels zero (the D3D convention, and what the hardware hands a
    /// shader). A viewer that wants a picture promotes it; the decode does not
    /// invent green and blue.
    pub fn single_channel(self) -> bool {
        matches!(
            self,
            Format::Bc4 | Format::Bc4Snorm | Format::R16Float | Format::R16Unorm
        )
    }
}

/// The 32-byte header of a type-`004` texture asset.
///
/// Field order and validation are the client's texture load callback
/// (`0x141dacf00`, reading through the descriptor builder `0x142e2d900`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextureHeader {
    pub flags: u16,
    pub mip_count: u8,
    /// The client's engine format enum. Use [`dxgi_of`] for its DXGI id.
    pub format: u8,
    pub surfaces: u8,
    pub usage: u8,
    pub payload_count: u8,
    /// Byte `+0x07`. The loader reads it: value 1 adds `0x4000` to the
    /// object's flags at `+0x08`.
    pub unknown: u8,
    pub width: u16,
    pub height: u16,
    /// Sum of every mip and surface, as `+0x0c` stores it (a u32, not a u16).
    pub total_size: u32,
    pub base_mips: u8,
    /// Byte `+0x11`. Only bit 3 is read: it marks the streamed chunk the
    /// request arithmetic excludes. The low bits are **not read**; the install
    /// relation `payload_count == popcount(chunk_mask)` is an observation (all
    /// 400 sampled assets), not a per-bit meaning. See [`TextureHeader::streamed_chunks`].
    pub chunk_mask: u8,
    /// Byte `+0x12`; the descriptor carries it at bits 59..62.
    pub reserved: u8,
}

impl TextureHeader {
    pub const SIZE: usize = 0x20;

    /// Parse and validate a texture header exactly as the client does.
    ///
    /// The loader's checks, in its order: format <= 0x71, mip_count <= 0x10,
    /// width != 0, height != 0, payload_count < 5, base_mips <= mip_count. It
    /// does **not** check surfaces, `total_size` or the reserved bytes.
    pub fn parse(asset: &[u8]) -> Result<TextureHeader> {
        if asset.len() < Self::SIZE {
            return Err(Error::Truncated("header"));
        }
        let h = TextureHeader {
            flags: u16::from_le_bytes([asset[0], asset[1]]),
            mip_count: asset[2],
            format: asset[3],
            surfaces: asset[4],
            usage: asset[5],
            payload_count: asset[6],
            unknown: asset[7],
            width: u16::from_le_bytes([asset[8], asset[9]]),
            height: u16::from_le_bytes([asset[10], asset[11]]),
            total_size: u32::from_le_bytes([asset[12], asset[13], asset[14], asset[15]]),
            base_mips: asset[16],
            chunk_mask: asset[17],
            reserved: asset[18],
        };
        if h.format > 0x71 {
            return Err(Error::Invalid("format > 0x71"));
        }
        if h.mip_count > 0x10 {
            return Err(Error::Invalid("mip_count > 0x10"));
        }
        if h.width == 0 {
            return Err(Error::Invalid("width == 0"));
        }
        if h.height == 0 {
            return Err(Error::Invalid("height == 0"));
        }
        if h.payload_count >= 5 {
            return Err(Error::Invalid("payload_count >= 5"));
        }
        if h.base_mips > h.mip_count {
            return Err(Error::Invalid("base_mips > mip_count"));
        }
        Ok(h)
    }

    /// The u64 the descriptor builder (`0x142e2d900`) assembles from the
    /// header's fields:
    ///
    /// ```text
    /// bits  0..13  width - 1        bits 39..42  mip_count (as stored)
    /// bits 14..27  height - 1       bits 43..47  usage
    /// bits 28..38  surfaces - 1     bits 48..50  payload_count
    /// bits 51..54  base_mips        bits 55..58  chunk_mask
    /// bits 59..62  byte +0x12       bit  63      clear
    /// ```
    pub fn descriptor(&self) -> u64 {
        let mut d = 0u64;
        d |= (self.width as u64).wrapping_sub(1) & 0x3fff;
        d |= ((self.height as u64).wrapping_sub(1) & 0x3fff) << 14;
        d |= ((self.surfaces as u64).wrapping_sub(1) & 0x7ff) << 28;
        d |= (self.mip_count as u64 & 0xf) << 39;
        d |= (self.usage as u64 & 0x1f) << 43;
        d |= (self.payload_count as u64 & 0x7) << 48;
        d |= (self.base_mips as u64 & 0xf) << 51;
        d |= (self.chunk_mask as u64 & 0xf) << 55;
        d |= (self.reserved as u64 & 0xf) << 59;
        d
    }

    /// The descriptor's flags word at `+0x08`: the header's u16 flags plus the
    /// loader's `0x100000`, and `0x4000` when byte `+0x07` is 1.
    pub fn object_flags(&self) -> u32 {
        let mut f = self.flags as u32 | 0x0010_0000;
        if self.unknown == 1 {
            f |= 0x4000;
        }
        f
    }

    /// Bit 2 of the flags marks a volume texture, whose `surfaces` byte is the
    /// depth. The descriptor builder tests it as `(descriptor_flags & 4)` when
    /// it feeds the size walk (`0x142e2dbb0`).
    pub fn is_volume(&self) -> bool {
        self.flags & 4 != 0
    }

    /// `(depth, surfaces)` as the descriptor's consumer sees them.
    ///
    /// The builder stores `(surfaces - 1) & 0x7FF` and the consumer adds one, so
    /// a header `surfaces == 0` becomes **2048** (the client's wrap), not 0. A
    /// volume texture has depth = that value and one surface.
    pub fn depth_and_surfaces(&self) -> (u32, u32) {
        let surfaces = ((self.surfaces as u32).wrapping_sub(1) & 0x7ff) + 1;
        if self.is_volume() {
            (surfaces, 1)
        } else {
            (1, surfaces)
        }
    }

    /// The mip count as the descriptor carries it (bits 39..42).
    ///
    /// The loader's request arithmetic reads this field, not the header byte;
    /// `mip_count == 16` is accepted by the loader and stored as 0.
    pub fn descriptor_mip_count(&self) -> u8 {
        self.mip_count & 0xf
    }

    /// The base mip count as the descriptor carries it (bits 51..54).
    pub fn descriptor_base_mips(&self) -> u8 {
        self.base_mips & 0xf
    }

    /// The chunk mask as the descriptor carries it (bits 55..58).
    pub fn descriptor_chunk_mask(&self) -> u8 {
        self.chunk_mask & 0xf
    }

    /// How many payload chunks the descriptor says are streamed: the client
    /// popcounts `(descriptor chunk_mask) >> 3` (`0x142e374d0` with bias 1).
    ///
    /// The mask is the 4-bit descriptor field, so only bit 3 survives the
    /// shift; bits 4..7 of the header byte are not in the descriptor.
    pub fn streamed_chunks(&self) -> u32 {
        count_bits_shifted(self.descriptor_chunk_mask() as u32, 1)
    }

    /// The descriptor's residency field at `+0x0c` as the load callback sets it
    /// (`0x141dacf00`): zero from the builder, then the streamed chunk count
    /// when `payload_count > 1` (`0x142e2dd70` gates that branch). The image's
    /// `DAT_14414ce69` gate is zero, so the alternative branch that would store
    /// `mip_count - chunk.level_count` is not the code this build runs; see
    /// [`TextureChunk::streamed_mips`] for that formula.
    pub fn resident_mips(&self) -> u32 {
        if self.payload_count > 1 {
            self.streamed_chunks()
        } else {
            0
        }
    }

    /// The number of immediate payload requests the loader issues:
    /// `0x142e37600(descriptor payload_count, descriptor mip_count, descriptor
    /// base_mips, resident_mips)`, and only when `payload_count > 1`. Indices
    /// are `1..=count`.
    pub fn request_count(&self) -> u32 {
        if self.payload_count <= 1 {
            return 0;
        }
        request_count(
            self.payload_count as u32,
            self.descriptor_mip_count() as u32,
            self.descriptor_base_mips() as u32,
            self.resident_mips(),
        )
    }

    /// The payload indices the loader requests immediately, in order.
    pub fn requested_payloads(&self) -> Vec<u32> {
        (1..=self.request_count()).collect()
    }

    /// This header's format family and DXGI id.
    pub fn format(&self) -> (Format, u8) {
        Format::from_header(self.format)
    }
}

/// The 16-byte chunk header at `+0x20` of a texture, and at `+0x00` of a
/// separate payload asset (type `0x04d`).
///
/// All four fields and their offsets are read: the first level is
/// `0x142e38740` (`first_from_smallest`, counted from the smallest mip), the
/// level count is `0x142255e10` (`+0x04`), the image size is `0x142e38750`
/// (`+0x08`), and the data pointer `0x142e38730` is `chunk + header_size`
/// (`+0x0c`). `0x141dacf00`'s dead branch also reads `+0x04`. The values on
/// this install (every sampled chunk has `header_size == 0x10` and an
/// `image_size` equal to the block-ceil sum of the levels it claims) are
/// **install** evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextureChunk {
    /// `mip_count - 1 - first_level`: the chunk's first level counted from the
    /// smallest mip.
    pub first_from_smallest: u32,
    pub level_count: u32,
    /// Block bytes in this chunk: every level it holds, times every surface.
    pub image_size: u32,
    /// Where the data starts, relative to the chunk header.
    pub header_size: u32,
}

impl TextureChunk {
    pub const SIZE: usize = 0x10;

    pub fn parse(bytes: &[u8]) -> Result<TextureChunk> {
        if bytes.len() < Self::SIZE {
            return Err(Error::Truncated("chunk header"));
        }
        Ok(TextureChunk {
            first_from_smallest: u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            level_count: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            image_size: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            header_size: u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
        })
    }

    /// The chunk's first level, counted from the largest mip (level 0), or
    /// `None` when `first_from_smallest` runs past the chain.
    ///
    /// `mip_count` is the header's stored byte (0..=16), not the 4-bit
    /// descriptor field: `first_from_smallest` is an asset-side relation to the
    /// header, not a consumer of the descriptor. (The descriptor field is what
    /// the loader's request arithmetic reads; see
    /// [`TextureHeader::descriptor_mip_count`].)
    pub fn first_level(&self, mip_count: u8) -> Option<u32> {
        (mip_count as u32)
            .checked_sub(1)?
            .checked_sub(self.first_from_smallest)
    }

    /// The dead branch's residency formula (`0x141dacf00`, gated by
    /// `DAT_14414ce69`): the mips *not* in this chunk.
    pub fn streamed_mips(&self, mip_count: u8) -> Option<u32> {
        (mip_count as u32).checked_sub(self.level_count)
    }

    /// Offset of the block data inside the asset that carries this header.
    pub fn data_offset(&self) -> usize {
        self.header_size as usize
    }
}

/// Where level 0's block run is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockPlan {
    /// `payload_count == 0`: `total_size` bytes at `+0x20`.
    Asset,
    /// The embedded chunk's first level is 0.
    Embedded(TextureChunk),
    /// Level 0 is not in the embedded chunk. The value is the highest separate
    /// payload index, which is where the six pinned assets put it; it is **not**
    /// the selection rule. [`parse_asset`] reads each payload's own chunk
    /// header and places its levels there. This value exists for `parse`'s
    /// [`Error::Level0InPayload`] and for `block_span`.
    Payload(u32),
}

/// Decide where level 0 lives, from the header and the embedded chunk.
///
/// `payload_count == 0` meaning a raw run at `+0x20`, and the chunk header's
/// location at `+0x20`, are **read** (`0x142e2d900` stores the chunk pointer at
/// object `+0x38`). Which level the embedded chunk starts at is **read**
/// (`0x142e38740`/`0x142e38730`). The `Payload(index)` value is the install's
/// highest-index convention, not a read selection rule; the levels a payload
/// actually holds come from its own chunk header.
pub fn block_plan(asset: &[u8]) -> Result<BlockPlan> {
    let header = TextureHeader::parse(asset)?;
    if header.payload_count == 0 {
        return Ok(BlockPlan::Asset);
    }
    let chunk = TextureChunk::parse(
        asset
            .get(0x20..)
            .ok_or(Error::Truncated("embedded chunk header"))?,
    )?;
    if chunk.image_size == 0 {
        return Err(Error::NoData);
    }
    match chunk.first_level(header.mip_count) {
        Some(0) => Ok(BlockPlan::Embedded(chunk)),
        Some(_) if header.payload_count >= 2 => {
            Ok(BlockPlan::Payload(header.payload_count as u32 - 1))
        }
        Some(_) => Err(Error::NoData),
        None => Err(Error::Invalid("chunk starts past the last mip")),
    }
}

/// One decoded subresource: one mip level of one array element (or cube face).
///
/// The client builds the D3D12 subresource record array at `0x141e296a0`: the
/// outer loop is the array element, then the cube face, and the inner loop runs
/// the mip levels of the source chunk from the largest to the smallest. The
/// data pointer advances by each level's own byte size in that order, so a
/// chunk's bytes are **surface-major, and within a surface mip 0 first**. The
/// record index is the D3D12 subresource number
/// `(array * faces + face) * mip_count + mip` (`0x142e36e80`; `faces` is 6
/// for renderer types 4/7), and the copy loop in `0x141f0a000` walks the same
/// order with the record-array flag bit 1 set (`0x141f0a53a`): its index is
/// `mip + surface * mip_count`, with `surface = array * faces + face`.
#[derive(Debug, Clone, Default)]
pub struct TextureLevel {
    /// Mip level, 0 = the largest.
    pub mip: u8,
    /// Layer index in the asset's own order (`0..surfaces`). A cube stores six
    /// consecutive layers; the header does not name a face direction, and the
    /// viewer's cube order is open (see the module note).
    pub surface: u16,
    pub width: u16,
    pub height: u16,
    /// This level's own block (or texel) run, exactly as stored.
    pub blocks: Vec<u8>,
    /// The 8-bit RGBA decode of `blocks`; for BC6H and the float families this
    /// is the clamped view described on [`half_to_unorm8`].
    pub rgba: Vec<u8>,
    /// The exact half-float decode, four per pixel in `r, g, b, a` order, for
    /// every format with a half-float form: BC6H (DXGI 95/96),
    /// R16G16B16A16_FLOAT (10) and R16_FLOAT (54). An R16_FLOAT asset stores
    /// one channel, so its entries are `(r, 0, 0, 1.0)` — the default the D3D
    /// view applies to the missing channels, with `r` the stored half word
    /// itself. R16_UNORM (56) is **not** here: it is still the clamped 8-bit
    /// view only (open), and empty for every 8-bit format.
    pub rgba16: Vec<u16>,
    /// Which asset supplied this level.
    pub source: BlockSource,
    /// The level's absolute offset inside that asset.
    pub block_offset: usize,
}

/// A decoded texture.
#[derive(Debug, Clone, Default)]
pub struct Texture {
    /// The header the decode came from.
    pub header: TextureHeader,
    pub width: u16,
    pub height: u16,
    pub mip_count: u8,
    pub surfaces: u8,
    pub format: Format,
    /// The raw format byte, as stored.
    pub format_byte: u8,
    /// Mip 0 of surface 0 as 8-bit RGBA, kept for the viewer and the existing
    /// tests. For a format with no byte form (BC6H and the float families) this
    /// is the clamped view described on [`half_to_unorm8`]: enough to draw, not
    /// enough to measure.
    pub rgba: Vec<u8>,
    /// Mip 0 of surface 0 as half floats, four per pixel (`r, g, b, a`), for
    /// the formats that produce one: BC6H, R16G16B16A16_FLOAT and R16_FLOAT.
    /// The values are the stored half words (R16_FLOAT is padded with
    /// `(0, 0, 1.0)`), so an upload can carry them unchanged. R16_UNORM and
    /// every 8-bit format leave this empty; R16_UNORM is still a clamped 8-bit
    /// view only (open).
    pub rgba16: Vec<u16>,
    /// Where mip 0 of surface 0 came from. A texture can carry its blocks
    /// inside its own bytes *or* in one of its payload assets, and the two are
    /// not interchangeable (measured: hero texture `0x0c00000000038c3d` embeds
    /// a descriptor table at `+0x20` and keeps mip 0 in payload 1) — a reader
    /// that assumes the embedded one reads the wrong bytes.
    pub source: BlockSource,
    /// Offset of that level's block run inside its source.
    pub block_offset: usize,
    /// The exact mip-0/surface-0 bytes the decode used.
    pub blocks: Vec<u8>,
    /// Every subresource the client copies, in D3D12 order: surface outer,
    /// mip inner (surface 0 mips 0.., then surface 1 mips 0.., ...).
    pub levels: Vec<TextureLevel>,
}

impl Texture {
    /// Whether the texture's stored format is an sRGB (colour) format.
    pub fn is_srgb(&self) -> bool {
        is_srgb_format(self.format_byte)
    }

    /// The decoded `(mip, surface)` subresource, if it was decoded.
    pub fn level(&self, mip: u8, surface: u16) -> Option<&TextureLevel> {
        self.levels
            .iter()
            .find(|level| level.mip == mip && level.surface == surface)
    }
}

/// Which asset the decoded blocks came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BlockSource {
    /// The raw run at `+0x20` of a `payload_count == 0` asset.
    #[default]
    Asset,
    /// The 16-byte chunk header at `+0x20` of the texture asset itself.
    Embedded,
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
        let is_bc = self.format.is_block();
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
            // BC7: fourCC "DX10" plus a 20-byte block that starts with it. The
            // number is the one the client's own tables resolve the stored
            // byte to (95/96 for BC6H, 98/99 for BC7).
            flags |= 0x80000;
            fourcc = u32::from_le_bytes(*b"DX10");
            dxgi = Some(dxgi_of(self.format_byte) as u32);
            let bw = (self.width as usize).div_ceil(4);
            let bh = (self.height as usize).div_ceil(4);
            pitch = (bw * bh * 16) as u32;
        } else if is_bc {
            // BC4/BC5 SNORM: no fourCC this writer can name without claiming
            // the unsigned variant.
            return Err(Error::UnsupportedFormat(self.format_byte));
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
        if is_bc {
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

/// Where the embedded block run starts and how many bytes it covers.
///
/// This is the run a caller can read from the parent asset alone: the raw
/// `total_size` bytes at `+0x20` when `payload_count == 0`, or the embedded
/// chunk's `image_size` bytes behind its `header_size` when that chunk holds
/// level 0. When level 0 is a separate payload, this returns
/// [`Error::Level0InPayload`] — the payload's own header is needed.
pub fn block_span(d: &[u8]) -> Result<(usize, usize)> {
    let header = TextureHeader::parse(d)?;
    match block_plan(d)? {
        BlockPlan::Asset => Ok((0x20, header.total_size as usize)),
        BlockPlan::Embedded(chunk) => {
            let start = 0x20usize
                .checked_add(chunk.header_size as usize)
                .ok_or(Error::Truncated("chunk header"))?;
            Ok((start, chunk.image_size as usize))
        }
        BlockPlan::Payload(i) => Err(Error::Level0InPayload(i)),
    }
}

/// Dimensions and family from a texture header alone.
pub fn header_dimensions(asset: &[u8]) -> Option<(u16, u16, Format)> {
    let header = TextureHeader::parse(asset).ok()?;
    Some((header.width, header.height, header.format().0))
}

/// The width and height of one mip level: `max(width >> mip, 1)`, the client's
/// width/height-at-level helpers `0x142e2db70`/`0x142e2db50`.
///
/// These are the resource's mip dimensions, not the block-storage grid: a 2x2
/// BC level still occupies one 4x4 block, which is why [`level_byte_size`]
/// rounds the dimensions up. (The client's block-pitch helper `0x142e36ec0`
/// clamps the dimensions at the block size before rounding; the block count is
/// the same either way.)
pub fn level_dimensions(width: u32, height: u32, mip: u32) -> (u32, u32) {
    let shift = mip & 0x1f;
    ((width >> shift).max(1), (height >> shift).max(1))
}

/// Bytes in one surface's mip level: `ceil(w/4) * ceil(h/4) * block bytes` for
/// a BC family, `w * h * bytes per pixel` for a linear one.
///
/// The same block count the client's pitch helper `0x142e36ec0` computes.
pub fn level_byte_size(format: Format, width: u32, height: u32) -> usize {
    level0_bytes(format, width as usize, height as usize)
}

/// Reject the families this decoder has no path for before their level sizes
/// are needed. The error names the DXGI id when the client's tables have one.
fn check_decodable(format: Format) -> Result<()> {
    match format {
        Format::Bc4Snorm => Err(Error::UnsupportedFormat(81)),
        Format::Bc5Snorm => Err(Error::UnsupportedFormat(84)),
        Format::Unknown(byte) => {
            let id = dxgi_of(byte);
            Err(Error::UnsupportedFormat(if id != 0 { id } else { byte }))
        }
        _ => Ok(()),
    }
}

/// One contiguous source of block data: the raw run, the embedded chunk, or a
/// separate payload's chunk.
#[derive(Debug, Clone, Copy)]
struct BlockRun {
    source: BlockSource,
    /// Offset of the chunk's data inside `bytes`.
    data_start: usize,
    /// The chunk's `image_size`: every level it holds, times every surface.
    image_size: usize,
    first_mip: u32,
    level_count: u32,
}

/// A run and the bytes it indexes (the texture asset or one payload asset).
struct RunData<'a> {
    bytes: &'a [u8],
    run: BlockRun,
}

/// A chunk header at `base` in its asset, turned into a run.
///
/// The client reads these fields directly: the data pointer is
/// `chunk + header_size` (`0x142e38730`), the first level is
/// `mip_count - 1 - first_from_smallest` (`0x142e38740`), `image_size`
/// (`0x142e38750`) and `level_count` (`0x142255e10`).
fn chunk_run(
    base: usize,
    chunk: &TextureChunk,
    mip_count: u8,
    source: BlockSource,
) -> Result<BlockRun> {
    let first_mip = chunk
        .first_level(mip_count)
        .ok_or(Error::Invalid("chunk starts past the last mip"))?;
    let data_start = base
        .checked_add(chunk.header_size as usize)
        .ok_or(Error::Truncated("chunk header"))?;
    Ok(BlockRun {
        source,
        data_start,
        image_size: chunk.image_size as usize,
        first_mip,
        level_count: chunk.level_count,
    })
}

/// The `payload_count == 0` run: `total_size` bytes at `+0x20`, every level from
/// the largest to the smallest (`0x141e29210`'s no-payload branch passes the
/// descriptor's `+0x38` pointer and `+0x40` size with first mip 0 and
/// `mip_count - 1` as the last).
fn raw_run(header: &TextureHeader) -> BlockRun {
    BlockRun {
        source: BlockSource::Asset,
        data_start: TextureHeader::SIZE,
        image_size: header.total_size as usize,
        first_mip: 0,
        level_count: header.mip_count as u32,
    }
}

/// Lay the runs out into subresources and decode every one.
///
/// The order inside one run is the one `0x141e296a0` walks: array element
/// (six faces for a cube) outermost, then the mip levels in ascending order
/// (largest first), with the data pointer advancing by each level's own byte
/// size. The D3D12 record index for a level is
/// `(array * faces + face) * mip_count + mip` (`0x142e36e80`; `faces` is 6
/// for renderer types 4/7).
fn build_texture(
    header: TextureHeader,
    format: Format,
    surfaces: u32,
    runs: &[RunData<'_>],
) -> Result<Texture> {
    let mip_count = header.mip_count as u32;
    if mip_count == 0 || surfaces == 0 {
        return Err(Error::NoData);
    }
    let surfaces_usize = surfaces as usize;
    let mips_usize = mip_count as usize;
    let count = mips_usize
        .checked_mul(surfaces_usize)
        .ok_or(Error::Truncated("dimensions"))?;
    let mut slots: Vec<Option<(usize, usize)>> = vec![None; count];

    for (run_index, data) in runs.iter().enumerate() {
        let run = data.run;
        if run.level_count == 0 {
            continue;
        }
        let end_mip = run
            .first_mip
            .checked_add(run.level_count)
            .filter(|end| *end <= mip_count)
            .ok_or(Error::Invalid("chunk levels past the last mip"))?;
        if run.image_size % surfaces_usize != 0 {
            return Err(Error::Invalid(
                "chunk image size is not over whole surfaces",
            ));
        }
        let per_surface = run.image_size / surfaces_usize;
        let end = run
            .data_start
            .checked_add(run.image_size)
            .ok_or(Error::Truncated("blocks"))?;
        if end > data.bytes.len() {
            return Err(Error::Truncated("blocks"));
        }
        let mut prefix = 0usize;
        for mip in run.first_mip..end_mip {
            let (w, h) = level_dimensions(header.width as u32, header.height as u32, mip);
            let size = level_byte_size(format, w, h);
            for surface in 0..surfaces_usize {
                let at = run.data_start + surface * per_surface + prefix;
                if at + size > end {
                    return Err(Error::Truncated("blocks"));
                }
                slots[surface * mips_usize + mip as usize] = Some((run_index, at));
            }
            prefix = prefix
                .checked_add(size)
                .ok_or(Error::Truncated("dimensions"))?;
        }
        if prefix * surfaces_usize != run.image_size {
            return Err(Error::Invalid(
                "chunk image size is not the sum of its levels",
            ));
        }
    }

    if slots.iter().any(Option::is_none) {
        return Err(Error::Invalid("a level is in no chunk"));
    }

    let mut levels = Vec::with_capacity(count);
    for (index, slot) in slots.iter().enumerate() {
        let (run_index, at) = slot.ok_or(Error::Invalid("a level is in no chunk"))?;
        let mip = (index % mips_usize) as u32;
        let surface = (index / mips_usize) as u32;
        let (w, h) = level_dimensions(header.width as u32, header.height as u32, mip);
        let size = level_byte_size(format, w, h);
        let data = &runs[run_index];
        let blocks = &data.bytes[at..at + size];
        let (rgba, rgba16) = decode_level(format, w as usize, h as usize, blocks)?;
        levels.push(TextureLevel {
            mip: mip as u8,
            surface: surface as u16,
            width: w as u16,
            height: h as u16,
            blocks: blocks.to_vec(),
            rgba,
            rgba16,
            source: data.run.source,
            block_offset: at,
        });
    }

    let level0 = &levels[0];
    Ok(Texture {
        header,
        width: header.width,
        height: header.height,
        mip_count: header.mip_count,
        surfaces: header.surfaces,
        format,
        format_byte: header.format,
        rgba: level0.rgba.clone(),
        rgba16: level0.rgba16.clone(),
        source: level0.source,
        block_offset: level0.block_offset,
        blocks: level0.blocks.clone(),
        levels,
    })
}

/// Parse a texture asset, following its separate payload assets.
///
/// The chunk fields are read (`0x142e38730`/`0x142e38740`/`0x142e38750`/
/// `0x142255e10`), and so is the loader's payload GUID (`0x142e37520`). Every
/// payload the header names (`payload_count > 1`, indices `1..=payload_count-1`)
/// is read; the client requests the immediate set from
/// [`TextureHeader::requested_payloads`] and fetches the highest index lazily
/// when `chunk_mask` bit 3 marks it streamed, but the bytes and the level each
/// payload holds are the same either way. Which levels a payload holds is its
/// own chunk header's `first_from_smallest`/`level_count`, not its index.
pub fn parse_asset(
    asset: &[u8],
    guid: u64,
    load: &dyn Fn(u64) -> Option<Vec<u8>>,
) -> Result<Texture> {
    let header = TextureHeader::parse(asset)?;
    let (format, _id) = header.format();
    check_decodable(format)?;
    if header.is_volume() {
        return Err(Error::UnsupportedLayout("volume"));
    }
    let plan = block_plan(asset)?;
    let (_depth, surfaces) = header.depth_and_surfaces();

    let mut payload_bytes: Vec<Vec<u8>> = Vec::new();
    let mut payload_runs: Vec<BlockRun> = Vec::new();
    let mut missing: Vec<u32> = Vec::new();
    if header.payload_count > 1 {
        for index in 1..header.payload_count as u32 {
            let payload_guid = crate::guid::payload_of(guid, index);
            let Some(bytes) = load(payload_guid) else {
                missing.push(index);
                continue;
            };
            let chunk = TextureChunk::parse(&bytes)?;
            let run = chunk_run(0, &chunk, header.mip_count, BlockSource::Payload(index))?;
            payload_bytes.push(bytes);
            payload_runs.push(run);
        }
    }

    let mut runs: Vec<RunData<'_>> = Vec::new();
    match plan {
        BlockPlan::Asset => runs.push(RunData {
            bytes: asset,
            run: raw_run(&header),
        }),
        BlockPlan::Embedded(chunk) => runs.push(RunData {
            bytes: asset,
            run: chunk_run(0x20, &chunk, header.mip_count, BlockSource::Embedded)?,
        }),
        BlockPlan::Payload(_) => {
            // The embedded chunk holds the tail; read it from `+0x20` and let
            // each payload's own header place its levels.
            let chunk = TextureChunk::parse(
                asset
                    .get(0x20..)
                    .ok_or(Error::Truncated("embedded chunk header"))?,
            )?;
            runs.push(RunData {
                bytes: asset,
                run: chunk_run(0x20, &chunk, header.mip_count, BlockSource::Embedded)?,
            });
        }
    }
    for (bytes, run) in payload_bytes.iter().zip(payload_runs.iter()) {
        runs.push(RunData { bytes, run: *run });
    }

    build_texture(header, format, surfaces, &runs).map_err(|e| match e {
        Error::Invalid("a level is in no chunk") if !missing.is_empty() => {
            Error::PayloadNotLoaded(missing[0])
        }
        other => other,
    })
}

/// Parse a texture asset and decode every level it carries itself.
///
/// When level 0 lives in a separate payload this returns
/// [`Error::Level0InPayload`]; use [`parse_asset`] with the payload loader for
/// that case. The `payload_count == 0` asset is one raw run at `+0x20`; an
/// asset with a chunk decodes the chunk's own levels (surface-major, mip
/// ascending) and nothing from the payload assets it names.
pub fn parse(asset: &[u8]) -> Result<Texture> {
    let header = TextureHeader::parse(asset)?;
    let (format, _id) = header.format();
    check_decodable(format)?;
    if header.is_volume() {
        return Err(Error::UnsupportedLayout("volume"));
    }
    let plan = block_plan(asset)?;
    let (_depth, surfaces) = header.depth_and_surfaces();
    let run = match plan {
        BlockPlan::Asset => raw_run(&header),
        BlockPlan::Embedded(chunk) => {
            chunk_run(0x20, &chunk, header.mip_count, BlockSource::Embedded)?
        }
        BlockPlan::Payload(index) => return Err(Error::Level0InPayload(index)),
    };
    build_texture(header, format, surfaces, &[RunData { bytes: asset, run }])
}

/// Decode one explicit run as a single mip 0 / surface 0 texture.
///
/// Kept for callers that hold a payload's bytes but not its parent asset. The
/// run is trimmed to one level ([`level_byte_size`]); the full asset-level
/// entry points are [`parse`] and [`parse_asset`].
pub fn decode(asset: &[u8], blocks_in: &[u8]) -> Result<Texture> {
    let header = TextureHeader::parse(asset)?;
    let (format, _id) = header.format();
    check_decodable(format)?;
    if header.is_volume() {
        return Err(Error::UnsupportedLayout("volume"));
    }
    let width = header.width as u32;
    let height = header.height as u32;
    let size = level_byte_size(format, width, height);
    let blocks = blocks_in.get(..size).ok_or(Error::Truncated("level 0"))?;
    let (rgba, rgba16) = decode_level(format, width as usize, height as usize, blocks)?;
    let level = TextureLevel {
        mip: 0,
        surface: 0,
        width: header.width,
        height: header.height,
        blocks: blocks.to_vec(),
        rgba: rgba.clone(),
        rgba16: rgba16.clone(),
        source: BlockSource::default(),
        block_offset: 0,
    };
    Ok(Texture {
        header,
        width: header.width,
        height: header.height,
        mip_count: header.mip_count,
        surfaces: header.surfaces,
        format,
        format_byte: header.format,
        rgba,
        rgba16,
        source: BlockSource::default(),
        block_offset: 0,
        blocks: blocks.to_vec(),
        levels: vec![level],
    })
}

/// Decode one subresource's blocks. `blocks` is exactly one level of one
/// surface, so the length checks are against that level's own dimensions.
fn decode_level(
    format: Format,
    width: usize,
    height: usize,
    blocks: &[u8],
) -> Result<(Vec<u8>, Vec<u16>)> {
    Ok(match format {
        Format::Bc1 => (decode_bc1(blocks, width, height)?, Vec::new()),
        Format::Bc2 => (decode_bc2(blocks, width, height)?, Vec::new()),
        Format::Bc3 => (decode_bc3(blocks, width, height)?, Vec::new()),
        Format::Bc4 => (decode_bc4(blocks, width, height)?, Vec::new()),
        Format::Bc5 => (decode_bc5(blocks, width, height)?, Vec::new()),
        Format::Bc7 => (decode_bc7(blocks, width, height)?, Vec::new()),
        Format::Bc6h { signed } => decode_bc6h(blocks, width, height, signed)?,
        Format::Rgba8 => (decode_rgba8(blocks, width, height)?, Vec::new()),
        Format::Rgba16Float => decode_rgba16f(blocks, width, height)?,
        Format::R16Float => decode_r16_float(blocks, width, height)?,
        // R16_UNORM's exact 16-bit value is not preserved: it still decodes to
        // the rounded 8-bit view only, so `rgba16` stays empty. Open.
        Format::R16Unorm => (decode_r16_unorm(blocks, width, height)?, Vec::new()),
        Format::Rg8 => (decode_rg8(blocks, width, height)?, Vec::new()),
        // `check_decodable` rejects these before a level reaches the decoder.
        Format::Bc4Snorm => return Err(Error::UnsupportedFormat(81)),
        Format::Bc5Snorm => return Err(Error::UnsupportedFormat(84)),
        Format::Unknown(byte) => {
            let id = dxgi_of(byte);
            return Err(Error::UnsupportedFormat(if id != 0 { id } else { byte }));
        }
    })
}

/// Bytes of one level's block grid for one surface, or 0 for an unknown family.
pub fn level0_bytes(format: Format, w: usize, h: usize) -> usize {
    let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
    match format {
        Format::Bc1 | Format::Bc4 | Format::Bc4Snorm => bw * bh * 8,
        Format::Bc2
        | Format::Bc3
        | Format::Bc5
        | Format::Bc5Snorm
        | Format::Bc7
        | Format::Bc6h { .. } => bw * bh * 16,
        Format::Rgba8 => w * h * 4,
        Format::Rgba16Float => w * h * 8,
        Format::R16Float | Format::R16Unorm | Format::Rg8 => w * h * 2,
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

/// R8G8, two byte channels: green is the second byte, blue is absent. UNORM
/// and UINT have the same byte layout (a byte/255 view and the raw integer), so
/// one decode serves both.
fn decode_rg8(blocks: &[u8], w: usize, h: usize) -> Result<Vec<u8>> {
    check_dims(w, h)?;
    let need = pixel_bytes(w, h, 2)?;
    if blocks.len() < need {
        return Err(Error::Truncated("rg8 data"));
    }
    let count = w.checked_mul(h).ok_or(Error::Truncated("dimensions"))?;
    let mut out = vec![0u8; pixel_bytes(w, h, 4)?];
    for i in 0..count {
        out[i * 4] = blocks[i * 2];
        out[i * 4 + 1] = blocks[i * 2 + 1];
        out[i * 4 + 3] = 255;
    }
    Ok(out)
}

/// R16G16B16A16_FLOAT: four half floats per pixel, 8 bytes.
///
/// `rgba16` is the four stored half words per pixel, unmodified (`r, g, b, a`);
/// `rgba` is the clamped byte view on [`half_to_unorm8`].
fn decode_rgba16f(blocks: &[u8], w: usize, h: usize) -> Result<(Vec<u8>, Vec<u16>)> {
    check_dims(w, h)?;
    let need = pixel_bytes(w, h, 8)?;
    if blocks.len() < need {
        return Err(Error::Truncated("rgba16f data"));
    }
    let count = w.checked_mul(h).ok_or(Error::Truncated("dimensions"))?;
    let mut out = vec![0u8; pixel_bytes(w, h, 4)?];
    let mut out16 = Vec::with_capacity(count * 4);
    for i in 0..count {
        let mut px = [0u16; 4];
        for (c, half) in px.iter_mut().enumerate() {
            *half = u16_at(blocks, i * 8 + c * 2).unwrap_or(0);
        }
        out16.extend_from_slice(&px);
        for c in 0..4 {
            out[i * 4 + c] = half_to_unorm8(px[c]);
        }
    }
    Ok((out, out16))
}

/// R16_FLOAT: one half float per pixel, 2 bytes.
///
/// `rgba16` is `(r, 0, 0, 1.0)` per pixel. `r` is the stored half word exactly;
/// the other three are the channel defaults a D3D one-channel float view
/// applies, not values the asset stores.
fn decode_r16_float(blocks: &[u8], w: usize, h: usize) -> Result<(Vec<u8>, Vec<u16>)> {
    check_dims(w, h)?;
    let need = pixel_bytes(w, h, 2)?;
    if blocks.len() < need {
        return Err(Error::Truncated("r16f data"));
    }
    let count = w.checked_mul(h).ok_or(Error::Truncated("dimensions"))?;
    let mut out = vec![0u8; pixel_bytes(w, h, 4)?];
    let mut out16 = Vec::with_capacity(count * 4);
    for i in 0..count {
        let half = u16_at(blocks, i * 2).unwrap_or(0);
        out16.extend_from_slice(&[half, 0, 0, 0x3C00]);
        out[i * 4] = half_to_unorm8(half);
        out[i * 4 + 3] = 255;
    }
    Ok((out, out16))
}

fn decode_r16_unorm(blocks: &[u8], w: usize, h: usize) -> Result<Vec<u8>> {
    // The 16-bit value is rounded to the nearest 8-bit level for the viewer.
    // That is a display conversion, not the stored value: `rgba16` stays empty
    // for R16_UNORM because the exact unorm is not preserved here (open).
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
///
/// BC1 picks the mode from the endpoints: `c0 > c1` is the four-colour block and
/// `c0 <= c1` the three-colour one with a transparent slot. BC2 and BC3 carry
/// their alpha in a separate half, so their colour block is **always** the
/// four-colour form; `four_colour` forces that branch for them and leaves BC1's
/// endpoint test untouched.
fn bc1_palette(c0: u16, c1: u16, four_colour: bool) -> ([[u8; 4]; 4], bool) {
    let a = rgb565(c0);
    let b = rgb565(c1);
    let mut pal = [[0u8; 4]; 4];
    pal[0] = [a[0], a[1], a[2], 255];
    pal[1] = [b[0], b[1], b[2], 255];
    if four_colour || c0 > c1 {
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

fn decode_bc_colour(
    blocks: &[u8],
    w: usize,
    h: usize,
    at: usize,
    four_colour: bool,
) -> Result<Vec<u8>> {
    let c0 = u16_at(blocks, at).ok_or(Error::Truncated("bc colour block"))?;
    let c1 = u16_at(blocks, at + 2).ok_or(Error::Truncated("bc colour block"))?;
    let bits = u32_at(blocks, at + 4).ok_or(Error::Truncated("bc colour block"))?;
    let (pal, _) = bc1_palette(c0, c1, four_colour);
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
            let tile = decode_bc_colour(blocks, 4, 4, at, false)?;
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
            let tile = decode_bc_colour(blocks, 4, 4, at + 8, true)?;
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
            let tile = decode_bc_colour(blocks, 4, 4, at + 8, true)?;
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

    /// A one-mip, one-surface, no-payload header with `total_size` set, then
    /// no chunk header: `parse` reads the block run from `+0x20`.
    fn texture_prefix(format: u8, w: u16, h: u16, total_size: u16) -> Vec<u8> {
        let mut d = vec![0u8; 0x20];
        d[2] = 1; // one mip
        d[3] = format;
        d[4] = 1; // one surface
        d[6] = 0;
        d[8..10].copy_from_slice(&w.to_le_bytes());
        d[10..12].copy_from_slice(&h.to_le_bytes());
        d[12..16].copy_from_slice(&(total_size as u32).to_le_bytes());
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

    /// A 4x4 BC colour-index word: the left two columns at `left`, the right
    /// two at `right`, so two palette entries can be read back side by side.
    fn colour_indices(left: u32, right: u32) -> u32 {
        let mut bits = 0u32;
        for y in 0..4 {
            for x in 0..4 {
                let idx = if x < 2 { left } else { right };
                bits |= idx << (2 * (y * 4 + x));
            }
        }
        bits
    }

    #[test]
    fn bc1_three_colour_mode_has_a_transparent_slot() {
        // c0 = 0 <= c1 = 0xFFFF: index 2 is the integer average 127 and index 3
        // is transparent black. BC1 is the only format that takes this branch.
        let mut d = texture_prefix(71, 4, 4, 8);
        d.extend_from_slice(&0u16.to_le_bytes());
        d.extend_from_slice(&0xFFFFu16.to_le_bytes());
        d.extend_from_slice(&colour_indices(2, 3).to_le_bytes());
        let t = parse(&d).expect("parses");
        assert_eq!(
            t.pixel(0, 0),
            [127, 127, 127, 255],
            "index 2 is the average"
        );
        assert_eq!(
            t.pixel(3, 0),
            [0, 0, 0, 0],
            "index 3 is transparent in 3-colour mode"
        );
    }

    #[test]
    fn bc2_always_uses_the_four_colour_block() {
        // c0 = 0 <= c1 = 0xFFFF would select BC1's 3-colour branch, but BC2's
        // colour half is always four-colour: index 2 is (2*0 + 255)/3 = 85 and
        // index 3 is (0 + 2*255)/3 = 170. Alpha comes from the 4-bit half.
        let mut d = texture_prefix(74, 4, 4, 16);
        d.extend_from_slice(&[0xFF; 8]); // every 4-bit alpha nibble is 15
        d.extend_from_slice(&0u16.to_le_bytes());
        d.extend_from_slice(&0xFFFFu16.to_le_bytes());
        d.extend_from_slice(&colour_indices(2, 3).to_le_bytes());
        let t = parse(&d).expect("parses");
        assert_eq!(t.format, Format::Bc2);
        assert_eq!(t.pixel(0, 0), [85, 85, 85, 255], "index 2, alpha opaque");
        assert_eq!(t.pixel(3, 0), [170, 170, 170, 255], "index 3, alpha opaque");
    }

    #[test]
    fn bc3_always_uses_the_four_colour_block() {
        // Same endpoints and indices as BC2. BC3's alpha half is the BC4-style
        // block: a0 = 0xFF with index 0 gives 255 everywhere.
        let mut d = texture_prefix(77, 4, 4, 16);
        d.push(0xFF); // alpha a0
        d.push(0x00); // alpha a1
        d.extend_from_slice(&[0u8; 6]); // all indices 0 -> alpha 255
        d.extend_from_slice(&0u16.to_le_bytes());
        d.extend_from_slice(&0xFFFFu16.to_le_bytes());
        d.extend_from_slice(&colour_indices(2, 3).to_le_bytes());
        let t = parse(&d).expect("parses");
        assert_eq!(t.format, Format::Bc3);
        assert_eq!(t.pixel(0, 0), [85, 85, 85, 255], "index 2, alpha opaque");
        assert_eq!(t.pixel(3, 0), [170, 170, 170, 255], "index 3, alpha opaque");
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
        assert_eq!(
            t.rgba16,
            vec![0x3C00, 0x0000, 0x3800, 0x0000, 0x0000, 0x3800, 0x3C00, 0x3C00],
            "RGBA16F keeps the stored half words, including alpha"
        );

        // R16_FLOAT: stored 55 resolves to DXGI 54; 0.25 -> 64 in red.
        let mut d = texture_prefix(55, 1, 1, 2);
        d.extend_from_slice(&0x3400u16.to_le_bytes());
        let t = parse(&d).expect("parses");
        assert_eq!(t.pixel(0, 0), [64, 0, 0, 255]);
        assert_eq!(
            t.rgba16,
            vec![0x3400, 0, 0, 0x3C00],
            "R16F is padded with the D3D view's (0, 0, 1.0) defaults"
        );

        // R16_UNORM: stored 57 resolves to DXGI 56; half of full scale -> 128.
        // Its exact 16-bit value is *not* preserved: still the 8-bit view only.
        let mut d = texture_prefix(57, 1, 1, 2);
        d.extend_from_slice(&32768u16.to_le_bytes());
        let t = parse(&d).expect("parses");
        assert_eq!(t.pixel(0, 0), [128, 0, 0, 255]);
        assert!(t.rgba16.is_empty(), "R16_UNORM is still clamped (open)");

        // R8G8_UINT: stored 51 resolves to DXGI 50 (the old code called this
        // R16_UNORM; the client's table says two 8-bit channels).
        let mut d = texture_prefix(51, 2, 1, 4);
        d.extend_from_slice(&[10, 20, 30, 40]);
        let t = parse(&d).expect("parses");
        assert_eq!(t.format, Format::Rg8);
        assert_eq!(t.pixel(0, 0), [10, 20, 0, 255]);
        assert_eq!(t.pixel(1, 0), [30, 40, 0, 255]);
    }

    /// The exact halves, not just their count: a value outside [0, 1] and a
    /// negative value both collapse in the byte view, so only `rgba16` can tell
    /// a preserving decode from a clamped one.
    #[test]
    fn rgba16_float_keeps_values_the_byte_view_clamps() {
        let stored = [
            [0xC000u16, 0x7BFF, 0x0001, 0x3C00], // -2.0, 65504, subnormal, 1.0
            [0x0000u16, 0x3800, 0x3C00, 0x0000], // 0, 0.5, 1.0, 0
        ];
        let mut d = texture_prefix(10, 2, 1, 16);
        for px in stored {
            for half in px {
                d.extend_from_slice(&half.to_le_bytes());
            }
        }
        let t = parse(&d).expect("parses");
        let want: Vec<u16> = stored.iter().flatten().copied().collect();
        assert_eq!(t.rgba16, want);
        assert_eq!(t.levels[0].rgba16, want);
        // The byte view is the clamped one: the first three channels pin.
        assert_eq!(t.pixel(0, 0), [0, 255, 0, 255]);
    }

    #[test]
    fn r16_float_keeps_its_half_and_pads_the_other_channels() {
        let stored = [0x3400u16, 0xC000]; // 0.25, -2.0
        let mut d = texture_prefix(55, 2, 1, 4);
        for half in stored {
            d.extend_from_slice(&half.to_le_bytes());
        }
        let t = parse(&d).expect("parses");
        assert_eq!(t.rgba16, vec![0x3400, 0, 0, 0x3C00, 0xC000, 0, 0, 0x3C00]);
        assert_eq!(t.levels[0].rgba16, t.rgba16);
    }

    #[test]
    fn bc6h_still_keeps_its_exact_halves_through_parse() {
        let block = bc6h_random_blocks(11, 1, 7)[0]; // mode 11, one region
        let mut d = texture_prefix(96, 4, 4, 16); // stored 96 -> DXGI 95 UF16
        d.extend_from_slice(&block);
        let t = parse(&d).expect("parses");
        let (_, exact) = decode_bc6h_block(&block, false);
        assert_eq!(t.rgba16, exact);
        assert_eq!(t.level(0, 0).expect("level 0").rgba16, exact);
        assert_eq!(exact.len(), 16 * 4);
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
        d[2] = 1; // one mip
        d[3] = 71; // BC1
        d[4] = 1; // one surface
        d[6] = 1; // payload_count > 0 -> chunk header at 0x20
        d[8..10].copy_from_slice(&4u16.to_le_bytes());
        d[10..12].copy_from_slice(&4u16.to_le_bytes());
        d[12..16].copy_from_slice(&8u32.to_le_bytes()); // total_size
                                                        // chunk header: first_from_smallest, level_count, image_size, header_size
        d.extend_from_slice(&0u32.to_le_bytes()); // level 0 is embedded
        d.extend_from_slice(&1u32.to_le_bytes());
        d.extend_from_slice(&8u32.to_le_bytes());
        d.extend_from_slice(&16u32.to_le_bytes());
        d.extend_from_slice(&0xF800u16.to_le_bytes());
        d.extend_from_slice(&0u16.to_le_bytes());
        d.extend_from_slice(&0u32.to_le_bytes());
        let t = parse(&d).expect("parses");
        assert_eq!(t.pixel(0, 0), [255, 0, 0, 255]);
        assert_eq!(t.block_offset, 0x30);
    }

    /// A texture header for the layout tests: no chunk, every field explicit.
    fn layout_header(
        mip_count: u8,
        format: u8,
        surfaces: u8,
        width: u16,
        height: u16,
        total_size: u32,
        payload_count: u8,
    ) -> Vec<u8> {
        let mut d = vec![0u8; TextureHeader::SIZE];
        d[2] = mip_count;
        d[3] = format;
        d[4] = surfaces;
        d[6] = payload_count;
        d[8..10].copy_from_slice(&width.to_le_bytes());
        d[10..12].copy_from_slice(&height.to_le_bytes());
        d[12..16].copy_from_slice(&total_size.to_le_bytes());
        d
    }

    fn tagged(out: &mut Vec<u8>, tag: u8, len: usize) {
        out.extend(std::iter::repeat_n(tag, len));
    }

    /// `payload_count == 0`: one raw run at `+0x20`, surface-major and mip 0
    /// first within each surface (`0x141e296a0`'s loop order). A mip below 4x4
    /// is clamped to a 4x4 block by the level arithmetic (`0x142e36ec0`).
    #[test]
    fn a_raw_run_is_surface_major_and_mip_ascending() {
        // 8x8 BC1, 3 mips (8, 4, 4): 32 + 8 + 8 B per surface, two surfaces.
        let mut d = layout_header(3, 0x47, 2, 8, 8, 96, 0);
        for (tag, len) in [
            (0xA0, 32),
            (0xA1, 8),
            (0xA2, 8),
            (0xB0, 32),
            (0xB1, 8),
            (0xB2, 8),
        ] {
            tagged(&mut d, tag, len);
        }
        let t = parse(&d).expect("parses");
        assert_eq!(t.levels.len(), 6);
        let want = [
            (0u8, 0u16, 8u16, 8u16, 0xA0u8),
            (1, 0, 4, 4, 0xA1),
            (2, 0, 2, 2, 0xA2),
            (0, 1, 8, 8, 0xB0),
            (1, 1, 4, 4, 0xB1),
            (2, 1, 2, 2, 0xB2),
        ];
        for (level, (mip, surface, w, h, tag)) in t.levels.iter().zip(want) {
            assert_eq!(
                (level.mip, level.surface, level.width, level.height),
                (mip, surface, w, h)
            );
            assert!(level.blocks.iter().all(|b| *b == tag), "tag {tag:#x}");
            assert_eq!(level.source, BlockSource::Asset);
        }
        assert_eq!(t.rgba, t.levels[0].rgba);
        assert_eq!(t.blocks, t.levels[0].blocks);
        assert_eq!(t.block_offset, 0x20);
        assert_eq!(t.level(0, 1).map(|l| l.width), Some(8));
    }

    /// The embedded chunk and the separate payloads are placed by their own
    /// chunk headers, not by their index: `first_from_smallest` names the first
    /// level of each run, and within it the bytes are surface-major/mip
    /// ascending. The client uploads the embedded tail at creation
    /// (`0x141dac400` → `0x141e29210`) and each payload when it arrives
    /// (`0x141dabee0`); the bytes and their level placement are the same.
    #[test]
    fn a_payload_chunk_places_its_levels_by_its_own_header() {
        // 16x16 BC1, 3 mips (128, 32, 8 B per surface), two surfaces.
        // Embedded chunk: first_from_smallest 1 -> mip 1, two levels,
        // 80 B over two surfaces. Payload 1: first_from_smallest 2 -> mip 0,
        // 256 B over two surfaces.
        let mut asset = layout_header(3, 0x47, 2, 16, 16, 336, 2);
        asset.extend_from_slice(&1u32.to_le_bytes());
        asset.extend_from_slice(&2u32.to_le_bytes());
        asset.extend_from_slice(&80u32.to_le_bytes());
        asset.extend_from_slice(&16u32.to_le_bytes());
        // Embedded data: per surface 32 (mip 1) + 8 (mip 2).
        for (tag, len) in [(0xE1, 32), (0xE2, 8), (0xF1, 32), (0xF2, 8)] {
            tagged(&mut asset, tag, len);
        }

        let mut payload = Vec::new();
        payload.extend_from_slice(&2u32.to_le_bytes());
        payload.extend_from_slice(&1u32.to_le_bytes());
        payload.extend_from_slice(&256u32.to_le_bytes());
        payload.extend_from_slice(&16u32.to_le_bytes());
        tagged(&mut payload, 0x10, 128);
        tagged(&mut payload, 0x11, 128);

        let guid = 0x0c00_0000_0000_1234u64;
        let payload_guid = crate::guid::payload_of(guid, 1);
        let loader = |g: u64| (g == payload_guid).then(|| payload.clone());
        let t = parse_asset(&asset, guid, &loader).expect("parses with its payload");
        assert_eq!(t.levels.len(), 6);
        let want = [
            (
                0u8,
                0u16,
                16u16,
                16u16,
                BlockSource::Payload(1),
                0x10usize,
                0x10u8,
            ),
            (1, 0, 8, 8, BlockSource::Embedded, 0x30, 0xE1),
            (2, 0, 4, 4, BlockSource::Embedded, 0x50, 0xE2),
            (0, 1, 16, 16, BlockSource::Payload(1), 0x90, 0x11),
            (1, 1, 8, 8, BlockSource::Embedded, 0x58, 0xF1),
            (2, 1, 4, 4, BlockSource::Embedded, 0x78, 0xF2),
        ];
        for (level, (mip, surface, w, h, source, offset, tag)) in t.levels.iter().zip(want) {
            assert_eq!(
                (level.mip, level.surface, level.width, level.height),
                (mip, surface, w, h)
            );
            assert_eq!(level.source, source);
            assert_eq!(level.block_offset, offset);
            assert!(level.blocks.iter().all(|b| *b == tag), "tag {tag:#x}");
        }
        assert_eq!(t.source, BlockSource::Payload(1));
        assert_eq!(t.block_offset, 0x10);
    }

    /// Six layers and two mips is twelve subresources, in the same surface-major
    /// order; the header's `surfaces` byte is already the layer count.
    #[test]
    fn a_six_layer_texture_decodes_every_layer_and_mip() {
        // 8x8 BC1, 2 mips (32 + 8 B), six layers.
        let mut d = layout_header(2, 0x47, 6, 8, 8, 240, 0);
        for layer in 0..6u8 {
            tagged(&mut d, layer, 32);
            tagged(&mut d, 0x80 + layer, 8);
        }
        let t = parse(&d).expect("parses");
        assert_eq!(t.levels.len(), 12);
        assert_eq!(t.surfaces, 6);
        for level in &t.levels {
            let tag = if level.mip == 0 {
                level.surface as u8
            } else {
                0x80 + level.surface as u8
            };
            assert!(level.blocks.iter().all(|b| *b == tag));
        }
        assert_eq!(t.levels[0].surface, 0);
        assert_eq!(t.levels[2].surface, 1, "surface outer, mip inner");
    }

    /// The per-level arithmetic, independent of any asset: the 128x128 BC6H
    /// cubemap chain is 21,872 B per face and 131,232 B over six.
    #[test]
    fn level_dimensions_and_sizes_follow_the_level_arithmetic() {
        assert_eq!(level_dimensions(8, 8, 0), (8, 8));
        assert_eq!(level_dimensions(8, 8, 1), (4, 4));
        assert_eq!(
            level_dimensions(8, 8, 2),
            (2, 2),
            "the resource dimension, not the 4x4 block grid"
        );
        assert_eq!(level_dimensions(8, 8, 7), (1, 1));
        assert_eq!(level_dimensions(3, 5, 1), (1, 2));
        assert_eq!(level_byte_size(Format::Bc1, 8, 8), 32);
        assert_eq!(level_byte_size(Format::Bc1, 4, 4), 8);
        assert_eq!(
            level_byte_size(Format::Bc1, 2, 2),
            8,
            "a 2x2 BC level is one 4x4 block"
        );
        assert_eq!(
            level_byte_size(Format::Bc6h { signed: false }, 128, 128),
            16384
        );

        let mut face = 0usize;
        for mip in 0..8u32 {
            let (w, h) = level_dimensions(128, 128, mip);
            face += level_byte_size(Format::Bc6h { signed: false }, w, h);
        }
        assert_eq!(face, 21872, "all eight mips of one 128x128 face");
        assert_eq!(face * 6, 131232, "six faces");
    }

    // ------------------------------------------------------------------
    // The client's header, format tables, size walk and payload selection.
    //
    // Evidence class: *read* for the code at the addresses named in the
    // module note, *install* for the six headers, chunk headers and payload
    // chunk headers below. Each byte sequence is copied from the operator's
    // installation; the expected values are recomputed from it, not from the
    // research report.
    // ------------------------------------------------------------------

    /// `TextureHeader` bytes of one real asset, and the chunk header at `+0x20`.
    struct Golden {
        guid: u64,
        header: [u8; 0x20],
        chunk: [u8; 0x10],
        descriptor: u64,
        dxgi: u8,
        format: Format,
        srgb: bool,
        /// The separate payloads, as `(index, its own 16-byte chunk header)`.
        payloads: &'static [(u32, [u8; 0x10])],
        requests: &'static [u32],
        /// The load-time value the descriptor's `+0x0C` field gets.
        resident: u32,
        /// `Some(index)` when level 0 is a separate payload.
        level0_payload: Option<u32>,
    }

    const GOLDEN: [Golden; 6] = [
        // 1024x1024 BC1_SRGB, 11 mips, 3 payloads, embedded mips 2..10.
        Golden {
            guid: 0x0c00_0000_0000_09c1,
            header: [
                0x22, 0x00, 0x0b, 0x49, 0x01, 0x02, 0x03, 0x00, 0x00, 0x04, 0x00, 0x04, 0xb8, 0xaa,
                0x0a, 0x00, 0x09, 0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00,
            ],
            chunk: [
                0x08, 0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0xb8, 0xaa, 0x00, 0x00, 0x10, 0x00,
                0x00, 0x00,
            ],
            descriptor: 0x03cb_1580_00ff_c3ff,
            dxgi: 72,
            format: Format::Bc1,
            srgb: true,
            payloads: &[
                (
                    1,
                    [
                        0x09, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00,
                        0x10, 0x00, 0x00, 0x00,
                    ],
                ),
                (
                    2,
                    [
                        0x0a, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08, 0x00,
                        0x10, 0x00, 0x00, 0x00,
                    ],
                ),
            ],
            requests: &[1, 2],
            resident: 0,
            level0_payload: Some(2),
        },
        // 128x128 BC6H_UF16 cubemap, 8 mips, 6 surfaces, one embedded chunk.
        Golden {
            guid: 0x0c00_0000_0006_a2c6,
            header: [
                0x08, 0x00, 0x08, 0x60, 0x06, 0x10, 0x01, 0x00, 0x80, 0x00, 0x80, 0x00, 0xa0, 0x00,
                0x02, 0x00, 0x08, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00,
            ],
            chunk: [
                0x07, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0xa0, 0x00, 0x02, 0x00, 0x10, 0x00,
                0x00, 0x00,
            ],
            descriptor: 0x00c1_8400_501f_c07f,
            dxgi: 95,
            format: Format::Bc6h { signed: false },
            srgb: false,
            payloads: &[],
            requests: &[],
            resident: 0,
            level0_payload: None,
        },
        // 512x512 BC7_SRGB, 10 mips, 2 payloads; payload 1 is level 0 and the
        // streamed chunk (bit 3), so the immediate request count is zero.
        Golden {
            guid: 0x0c00_0000_0003_a505,
            header: [
                0x22, 0x00, 0x0a, 0x64, 0x01, 0x04, 0x02, 0x00, 0x00, 0x02, 0x00, 0x02, 0x70, 0x55,
                0x05, 0x00, 0x09, 0x09, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00,
            ],
            chunk: [
                0x08, 0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0x70, 0x55, 0x01, 0x00, 0x10, 0x00,
                0x00, 0x00,
            ],
            descriptor: 0x04ca_2500_007f_c1ff,
            dxgi: 99,
            format: Format::Bc7,
            srgb: true,
            payloads: &[(
                1,
                [
                    0x09, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x00, 0x10,
                    0x00, 0x00, 0x00,
                ],
            )],
            requests: &[],
            resident: 1,
            level0_payload: Some(1),
        },
        // 512x512 BC4_UNORM, 8 mips, 2 payloads.
        Golden {
            guid: 0x0c00_0000_0000_64d2,
            header: [
                0x02, 0x00, 0x08, 0x51, 0x01, 0x04, 0x02, 0x00, 0x00, 0x02, 0x00, 0x02, 0xa8, 0xaa,
                0x02, 0x00, 0x07, 0x05, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00,
            ],
            chunk: [
                0x06, 0x00, 0x00, 0x00, 0x07, 0x00, 0x00, 0x00, 0xa8, 0xaa, 0x00, 0x00, 0x10, 0x00,
                0x00, 0x00,
            ],
            descriptor: 0x02ba_2400_007f_c1ff,
            dxgi: 80,
            format: Format::Bc4,
            srgb: false,
            payloads: &[(
                1,
                [
                    0x07, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x10,
                    0x00, 0x00, 0x00,
                ],
            )],
            requests: &[1],
            resident: 0,
            level0_payload: Some(1),
        },
        // 1024x1024 BC4_UNORM, 9 mips, 3 payloads; payload 2 is level 0 and is
        // the streamed chunk, payload 1 (level 1) is requested immediately.
        Golden {
            guid: 0x0c00_0000_0004_70e8,
            header: [
                0x02, 0x00, 0x09, 0x51, 0x01, 0x04, 0x03, 0x00, 0x00, 0x04, 0x00, 0x04, 0xa8, 0xaa,
                0x0a, 0x00, 0x07, 0x0b, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00,
            ],
            chunk: [
                0x06, 0x00, 0x00, 0x00, 0x07, 0x00, 0x00, 0x00, 0xa8, 0xaa, 0x00, 0x00, 0x10, 0x00,
                0x00, 0x00,
            ],
            descriptor: 0x05bb_2480_00ff_c3ff,
            dxgi: 80,
            format: Format::Bc4,
            srgb: false,
            payloads: &[
                (
                    1,
                    [
                        0x07, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00,
                        0x10, 0x00, 0x00, 0x00,
                    ],
                ),
                (
                    2,
                    [
                        0x08, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08, 0x00,
                        0x10, 0x00, 0x00, 0x00,
                    ],
                ),
            ],
            requests: &[1],
            resident: 1,
            level0_payload: Some(2),
        },
        // 512x512 BC4_UNORM, 8 mips, 2 payloads; payload 1 is level 0 and the
        // streamed chunk.
        Golden {
            guid: 0x0c00_0000_0004_70d8,
            header: [
                0x02, 0x00, 0x08, 0x51, 0x01, 0x04, 0x02, 0x00, 0x00, 0x02, 0x00, 0x02, 0xa8, 0xaa,
                0x02, 0x00, 0x07, 0x09, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                0x00, 0x00, 0x00, 0x00,
            ],
            chunk: [
                0x06, 0x00, 0x00, 0x00, 0x07, 0x00, 0x00, 0x00, 0xa8, 0xaa, 0x00, 0x00, 0x10, 0x00,
                0x00, 0x00,
            ],
            descriptor: 0x04ba_2400_007f_c1ff,
            dxgi: 80,
            format: Format::Bc4,
            srgb: false,
            payloads: &[(
                1,
                [
                    0x07, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x10,
                    0x00, 0x00, 0x00,
                ],
            )],
            requests: &[],
            resident: 1,
            level0_payload: Some(1),
        },
    ];

    /// The DXGI id each stored engine byte resolves to, one row per byte the
    /// client accepts (0x00..=0x71), read from `0x143356f50` → `0x14348f500`.
    #[rustfmt::skip]
    const EXPECTED_DXGI: [u8; 0x72] = [
        0, 3, 2, 3, 4, 7, 6, 7, 8, 12, 10, 11, 12, 13, 14, 17,
        16, 17, 18, 0, 20, 0, 0, 25, 24, 0, 25, 26, 30, 28, 29, 30,
        31, 32, 36, 34, 35, 36, 0, 38, 42, 40, 41, 42, 43, 45, 45, 45,
        0, 50, 49, 50, 51, 52, 57, 54, 55, 56, 57, 58, 59, 62, 61, 62,
        63, 64, 65, 0, 67, 0, 0, 71, 71, 72, 74, 74, 75, 77, 77, 78,
        80, 80, 81, 83, 83, 84, 85, 0, 87, 87, 24, 87, 91, 87, 91, 96,
        95, 96, 98, 98, 99, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 103,
    ];

    /// A header for the pure formula tests; the fields the formula reads are
    /// the only ones that matter.
    fn formula_header(mip: u8, base: u8, payload: u8, mask: u8) -> TextureHeader {
        TextureHeader {
            mip_count: mip,
            base_mips: base,
            payload_count: payload,
            chunk_mask: mask,
            width: 4,
            height: 4,
            ..TextureHeader::default()
        }
    }

    #[test]
    fn every_engine_format_byte_maps_through_the_client_tables() {
        for (stored, want) in EXPECTED_DXGI.iter().copied().enumerate() {
            let stored = stored as u8;
            assert_eq!(dxgi_of(stored), want, "stored 0x{stored:02x}");
            assert_eq!(
                Format::from_header(stored).1,
                want,
                "from_header(0x{stored:02x})"
            );
        }
        // Beyond the client's 0x72-entry table there is no format at all.
        for stored in 0x72..=0xffu8 {
            assert_eq!(dxgi_of(stored), 0, "stored 0x{stored:02x}");
            assert_eq!(Format::from_header(stored).0, Format::Unknown(stored));
        }
        // The rows the research named, and the one it had wrong: stored 0x33
        // is R8G8_UINT (50), not R16_UNORM.
        for (stored, want) in [
            (0x0a, 10),
            (0x1e, 29),
            (0x33, 50),
            (0x47, 71),
            (0x49, 72),
            (0x4a, 74),
            (0x4c, 75),
            (0x4d, 77),
            (0x4f, 78),
            (0x50, 80),
            (0x52, 81),
            (0x53, 83),
            (0x55, 84),
            (0x5f, 96),
            (0x60, 95),
            (0x61, 96),
            (0x62, 98),
            (0x64, 99),
        ] {
            assert_eq!(dxgi_of(stored), want, "stored 0x{stored:02x}");
        }
        assert_eq!(Format::from_header(0x33), (Format::Rg8, 50));
        assert_eq!(
            Format::from_header(0x5f),
            (Format::Bc6h { signed: true }, 96)
        );
        assert_eq!(
            Format::from_header(0x60),
            (Format::Bc6h { signed: false }, 95)
        );
        assert_eq!(Format::from_header(0x52), (Format::Bc4Snorm, 81));
        assert_eq!(Format::from_header(0x55), (Format::Bc5Snorm, 84));
    }

    #[test]
    fn srgb_is_the_dxgi_id_and_includes_bgra_srgb() {
        // The stored bytes the material side used to hard-code, plus the two
        // the tables resolve to B8G8R8A8_UNORM_SRGB (91).
        for (stored, dxgi) in [
            (30u8, 29u8),
            (73, 72),
            (76, 75),
            (79, 78),
            (100, 99),
            (0x5c, 91),
            (0x5e, 91),
        ] {
            assert_eq!(dxgi_of(stored), dxgi, "stored {stored}");
            assert!(is_srgb_format(stored), "stored {stored}");
        }
        let srgb: Vec<u8> = (0u8..=0x71).filter(|b| is_srgb_format(*b)).collect();
        assert_eq!(srgb, [0x1e, 0x49, 0x4c, 0x4f, 0x5c, 0x5e, 0x64]);
        // The five the install's material lists actually use are the non-BGRA
        // five; the material-side observation is that set, not the predicate.
        let material: Vec<u8> = srgb
            .iter()
            .copied()
            .filter(|b| !matches!(b, 0x5c | 0x5e))
            .collect();
        assert_eq!(material, [0x1e, 0x49, 0x4c, 0x4f, 0x64]);
        // Every other mapped format is linear.
        for stored in 0u8..=0x71 {
            if !srgb.contains(&stored) {
                assert!(!is_srgb_format(stored), "stored {stored}");
            }
        }
    }

    #[test]
    fn the_six_golden_headers_pin_the_clients_fields() {
        for g in &GOLDEN {
            let header = TextureHeader::parse(&g.header).expect("golden header parses");
            assert_eq!(header.format, g.header[3]);
            assert_eq!((header.format().0, header.format().1), (g.format, g.dxgi));
            assert_eq!(
                is_srgb_format(header.format),
                g.srgb,
                "0x{:016x}: sRGB",
                g.guid
            );
            let (depth, surface_count) = header.depth_and_surfaces();
            assert_eq!(
                header.total_size as u64,
                cooked_size(
                    header.format,
                    header.width as u32,
                    header.height as u32,
                    depth,
                    header.mip_count as u32,
                    surface_count
                )
            );
            assert_eq!(
                header.total_size as u64,
                size_walk(
                    header.format,
                    header.width as u32,
                    header.height as u32,
                    depth,
                    header.mip_count as u32,
                    surface_count
                )
            );
            assert_eq!(header.descriptor(), g.descriptor, "0x{:016x}", g.guid);

            // payload_count is the population count of the chunk mask.
            assert_eq!(
                header.payload_count,
                header.chunk_mask.count_ones() as u8,
                "0x{:016x}",
                g.guid
            );
            assert!(header.base_mips <= header.mip_count);

            let chunk = TextureChunk::parse(&g.chunk).expect("chunk parses");
            let first = chunk.first_level(header.mip_count).expect("first level");
            assert_eq!(
                first + chunk.level_count,
                header.mip_count as u32,
                "0x{:016x}: the chunk's levels end at the smallest mip",
                g.guid
            );
            assert_eq!(chunk.header_size, 0x10);
            // The chunk's image size is the size walk from its first level.
            let min = if is_block_format(header.format) { 4 } else { 1 };
            let w = (header.width as u32 >> first).max(min);
            let h = (header.height as u32 >> first).max(min);
            assert_eq!(
                chunk.image_size as u64,
                cooked_size(header.format, w, h, depth, chunk.level_count, surface_count),
                "0x{:016x}: chunk image size",
                g.guid
            );

            // Level 0 is embedded iff the chunk starts at level 0.
            let mut asset = g.header.to_vec();
            asset.extend_from_slice(&g.chunk);
            let plan = block_plan(&asset).expect("plan");
            match g.level0_payload {
                None => {
                    assert_eq!(first, 0, "0x{:016x}: embedded level 0", g.guid);
                    assert!(matches!(plan, BlockPlan::Embedded(_)));
                }
                Some(index) => {
                    assert_ne!(first, 0, "0x{:016x}: level 0 is separate", g.guid);
                    assert_eq!(plan, BlockPlan::Payload(index));
                }
            }

            // Requests and residency, from the loader's own formulas.
            assert_eq!(header.requested_payloads(), g.requests, "0x{:016x}", g.guid);
            assert_eq!(header.resident_mips(), g.resident, "0x{:016x}", g.guid);

            // The payload GUIDs are the client's formula, and each payload's
            // own chunk header says which level it starts at.
            for (index, bytes) in g.payloads {
                let payload_guid = crate::guid::payload_of(g.guid, *index);
                assert_eq!(
                    payload_guid,
                    0x0320_0000_0000_0000 | ((*index as u64) << 32) | (g.guid & 0xffff_ffff),
                    "0x{:016x} payload {index}",
                    g.guid
                );
                let chunk = TextureChunk::parse(bytes).expect("payload chunk");
                assert_eq!(chunk.header_size, 0x10);
                let first = chunk.first_level(header.mip_count).expect("first level");
                // A separate payload holds a contiguous run of levels and does
                // not have to reach the smallest mip; the embedded chunk does.
                assert!(
                    first + chunk.level_count <= header.mip_count as u32,
                    "0x{:016x} payload {index} levels",
                    g.guid
                );
                assert_eq!(chunk.level_count, 1, "0x{:016x} payload {index}", g.guid);
                let min = if is_block_format(header.format) { 4 } else { 1 };
                let w = (header.width as u32 >> first).max(min);
                let h = (header.height as u32 >> first).max(min);
                assert_eq!(
                    chunk.image_size as u64,
                    cooked_size(header.format, w, h, depth, chunk.level_count, surface_count),
                    "0x{:016x} payload {index} image size",
                    g.guid
                );
            }
            if let Some(index) = g.level0_payload {
                let (_, bytes) = g
                    .payloads
                    .iter()
                    .find(|(i, _)| *i == index)
                    .expect("the level-0 payload is listed");
                let chunk = TextureChunk::parse(bytes).unwrap();
                assert_eq!(
                    chunk.first_level(header.mip_count),
                    Some(0),
                    "0x{:016x}: the chosen payload starts at level 0",
                    g.guid
                );
            }
        }
    }

    #[test]
    fn the_descriptor_bitfield_round_trips_every_field() {
        for g in &GOLDEN {
            let header = TextureHeader::parse(&g.header).unwrap();
            let d = header.descriptor();
            assert_eq!(d >> 63, 0, "bit 63 is clear");
            assert_eq!((d & 0x3fff) as u16 + 1, header.width);
            assert_eq!(((d >> 14) & 0x3fff) as u16 + 1, header.height);
            let (depth, surfaces) = header.depth_and_surfaces();
            assert_eq!(
                ((d >> 28) & 0x7ff) as u32 + 1,
                if header.is_volume() { depth } else { surfaces },
                "the descriptor's surface/depth field"
            );
            assert_eq!(((d >> 39) & 0xf) as u8, header.descriptor_mip_count());
            assert_eq!(((d >> 43) & 0x1f) as u8, header.usage);
            assert_eq!(((d >> 48) & 0x7) as u8, header.payload_count);
            assert_eq!(((d >> 51) & 0xf) as u8, header.descriptor_base_mips());
            assert_eq!(((d >> 55) & 0xf) as u8, header.descriptor_chunk_mask());
            assert_eq!(((d >> 59) & 0xf) as u8, header.reserved);
            // The object flags are the header flags plus the loader's bits.
            assert_eq!(
                header.object_flags(),
                header.flags as u32 | 0x10_0000 | if header.unknown == 1 { 0x4000 } else { 0 }
            );
        }
        // Byte +0x07 == 1 adds 0x4000 (the loader's `0x141dacf00` check).
        let mut bytes = GOLDEN[0].header;
        bytes[7] = 1;
        let header = TextureHeader::parse(&bytes).unwrap();
        assert_eq!(
            header.object_flags(),
            0x22 | 0x10_0000 | 0x4000,
            "byte +0x07 == 1"
        );
    }

    #[test]
    fn the_client_rejects_the_headers_it_rejects() {
        let good = GOLDEN[0].header;
        assert!(TextureHeader::parse(&good).is_ok());
        for (at, value, why) in [
            (3usize, 0x72u8, "format > 0x71"),
            (2, 0x11, "mip_count > 0x10"),
            (8, 0x00, "width == 0"),
            (10, 0x00, "height == 0"),
            (6, 5, "payload_count >= 5"),
            (16, 0xff, "base_mips > mip_count"),
        ] {
            let mut bytes = good;
            bytes[at] = value;
            // Width and height are 16-bit: zero both halves.
            if at == 8 {
                bytes[9] = 0;
            }
            if at == 10 {
                bytes[11] = 0;
            }
            match TextureHeader::parse(&bytes) {
                Err(Error::Invalid(msg)) => assert_eq!(msg, why, "byte {at}"),
                other => panic!("byte {at}: expected Invalid({why}), got {other:?}"),
            }
        }
        // The loader does not validate surfaces or total_size: a zero surface
        // count reaches the descriptor builder (which wraps it), so parse does
        // not invent a check for it.
        let mut bytes = good;
        bytes[4] = 0;
        assert!(TextureHeader::parse(&bytes).is_ok());
    }

    #[test]
    fn the_size_walk_matches_the_install_and_the_minimum_clamp() {
        // Every golden asset's recorded total_size is the walk's answer.
        for g in &GOLDEN {
            let h = TextureHeader::parse(&g.header).unwrap();
            assert_eq!(
                size_walk(
                    h.format,
                    h.width as u32,
                    h.height as u32,
                    1,
                    h.mip_count as u32,
                    h.surfaces as u32
                ),
                h.total_size as u64,
                "0x{:016x}",
                g.guid
            );
        }
        // Block formats clamp at 4: 8x8 BC1 for four levels is
        // 32 + 8 + 8 + 8 = 56 bytes.
        assert_eq!(size_walk(0x47, 8, 8, 1, 4, 1), 56);
        // A linear 1-bpp format clamps at 1: (64 + 16 + 4 + 1) / 8 = 10.
        assert_eq!(size_walk(0x43, 8, 8, 1, 4, 1), 10);
        // Depth halves and stops at 1: 64 + 8 + 1 = 73, / 8 = 9.
        assert_eq!(size_walk(0x43, 4, 4, 4, 3, 1), 9);
        // R8G8B8A8: 2x2 x 4 bytes.
        assert_eq!(size_walk(0x1e, 2, 2, 1, 1, 1), 16);
        // The client's zero guard covers every one of the five counts.
        for (w, h, d, m, s) in [
            (0, 1, 1, 1, 1),
            (1, 0, 1, 1, 1),
            (1, 1, 0, 1, 1),
            (1, 1, 1, 0, 1),
            (1, 1, 1, 1, 0),
        ] {
            assert_eq!(size_walk(0x47, w, h, d, m, s), 0);
        }
    }

    #[test]
    fn payload_requests_follow_the_descriptor_mask_and_the_loader_guard() {
        // Only bit 3 is read as the streamed flag (`0x142e374d0` with bias 1).
        // The descriptor keeps four bits, so bits 4..7 of the header byte are
        // dropped before the popcount. With the 4-bit field only bit 3 survives
        // the shift, so the popcount and a bit-3 test agree here; the
        // differential unit fn/TextureCountBitsShifted is what proves the
        // popcount expression itself. The low bits' meaning is open.
        for (mask, streamed, requests) in [
            (0x01u8, 0u32, vec![1, 2, 3]),
            (0x05, 0, vec![1, 2, 3]),
            (0x07, 0, vec![1, 2, 3]),
            (0x09, 1, vec![1, 2]),
            (0x0b, 1, vec![1, 2]),
            (0x0f, 1, vec![1, 2]),
            (0x10, 0, vec![1, 2, 3]), // bit 4 is not in the descriptor
            (0x18, 1, vec![1, 2]),    // truncates to 0x8
            (0x19, 1, vec![1, 2]),    // raw popcount would be 2; field is 1
            (0x1f, 1, vec![1, 2]),
            (0xf0, 0, vec![1, 2, 3]),
            (0xf8, 1, vec![1, 2]),
        ] {
            let h = formula_header(8, 3, 4, mask);
            assert_eq!(h.streamed_chunks(), streamed, "mask 0x{mask:02x}");
            assert_eq!(h.resident_mips(), streamed, "mask 0x{mask:02x}");
            assert_eq!(h.requested_payloads(), requests, "mask 0x{mask:02x}");
        }
        // Every bit above 3 is outside the descriptor field and does not
        // change the streamed count.
        for mask in 0x10u8..=0xff {
            assert_eq!(
                formula_header(8, 3, 4, mask).streamed_chunks(),
                count_bits_shifted((mask & 0xf) as u32, 1),
                "mask 0x{mask:02x}"
            );
        }
        // The 4-bit descriptor fields: mip 16 and base 16 are stored as 0.
        let h = formula_header(16, 16, 4, 0x09);
        assert_eq!(
            (
                h.descriptor_mip_count(),
                h.descriptor_base_mips(),
                h.descriptor_chunk_mask()
            ),
            (0, 0, 9)
        );
        assert_eq!((h.descriptor() >> 39) & 0xf, 0, "mip_count field");
        assert_eq!((h.descriptor() >> 51) & 0xf, 0, "base_mips field");
        assert_eq!(
            h.requested_payloads(),
            Vec::<u32>::new(),
            "mip 0 - base 0 <= streamed 1"
        );
        // The loader accepts mip_count == 16, so the truncation is reachable.
        let mut bytes = GOLDEN[0].header;
        bytes[2] = 16;
        assert!(TextureHeader::parse(&bytes).is_ok());
        // `mip_count - base_mips <= streamed` means no immediate request.
        assert!(formula_header(4, 3, 4, 0x09)
            .requested_payloads()
            .is_empty());
        // The loader skips the whole request block when payload_count <= 1
        // (`0x142e2dd70` is false), so even the formula's max(1, …) does not
        // turn a single embedded chunk into a request.
        let one = formula_header(8, 3, 1, 0x01);
        assert_eq!(one.requested_payloads(), Vec::<u32>::new());
        assert_eq!(one.resident_mips(), 0);
        // A payload_count of 1 with bit 3 set still has no separate payload.
        assert!(formula_header(8, 3, 1, 0x09)
            .requested_payloads()
            .is_empty());
    }

    /// The pure leaves `request_count` and `count_bits_shifted` on their own,
    /// independently of the header gate.
    #[test]
    fn the_request_leaf_and_the_popcount_leaf_match_the_client() {
        // `0x142e37600` with (payload, mip, base, streamed).
        assert_eq!(request_count(4, 8, 3, 0), 3);
        assert_eq!(request_count(4, 8, 3, 1), 2);
        assert_eq!(request_count(4, 8, 3, 2), 1);
        assert_eq!(request_count(4, 4, 3, 1), 0);
        assert_eq!(request_count(4, 0, 0, 0), 0);
        assert_eq!(request_count(1, 8, 3, 0), 1, "the leaf clamps at 1");
        assert_eq!(request_count(2, 8, 3, 0), 1);
        // `0x142e374d0` shifts by `(bias + 2) & 31` then popcounts.
        assert_eq!(count_bits_shifted(0xf, 1), 1);
        assert_eq!(count_bits_shifted(0x1f, 1), 2);
        assert_eq!(count_bits_shifted(0xff, 1), 5);
        assert_eq!(count_bits_shifted(0xff, 0), 6);
        assert_eq!(count_bits_shifted(0xff, 2), 4);
        assert_eq!(count_bits_shifted(0xffff_ffff, 31), 31);
        assert_eq!(count_bits_shifted(0xffff_ffff, 255), 31);
    }

    #[test]
    fn a_zero_surface_header_has_the_clients_2048_surfaces() {
        // The builder stores `(surfaces - 1) & 0x7ff` and the consumer adds
        // one, so 0 wraps to 2048 rather than staying 0.
        let mut bytes = GOLDEN[0].header;
        bytes[4] = 0;
        let h = TextureHeader::parse(&bytes).expect("surfaces 0 is accepted");
        assert_eq!(h.depth_and_surfaces(), (1, 2048));
        let mut volume = bytes;
        volume[0] |= 4;
        let h = TextureHeader::parse(&volume).unwrap();
        assert_eq!(h.depth_and_surfaces(), (2048, 1));
    }

    #[test]
    fn a_chunk_says_which_level_it_starts_at() {
        // first_from_smallest counts back from the smallest mip.
        let chunk = TextureChunk::parse(&GOLDEN[0].chunk).unwrap();
        assert_eq!(chunk.first_level(11), Some(2));
        assert_eq!(chunk.streamed_mips(11), Some(2));
        assert_eq!(chunk.data_offset(), 0x10);
        let embedded = TextureChunk {
            first_from_smallest: 0,
            ..chunk
        };
        assert_eq!(embedded.first_level(11), Some(10));
        // A chunk claiming more levels than the chain has no first level.
        let past = TextureChunk {
            first_from_smallest: 12,
            ..chunk
        };
        assert_eq!(past.first_level(11), None);
        // The chunk relation uses the header's stored mip count, not the
        // 4-bit descriptor field: mip_count 16 is stored as 0 in the descriptor
        // but the asset's first_from_smallest still counts 16 levels.
        let sixteen = TextureChunk {
            first_from_smallest: 15,
            level_count: 1,
            ..chunk
        };
        assert_eq!(sixteen.first_level(16), Some(0));
        assert_eq!(formula_header(16, 0, 1, 0).descriptor_mip_count(), 0);
    }
}

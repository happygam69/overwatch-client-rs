//! Shader assets: code (`teShaderCode`), groups (`teShaderGroup`) and
//! instances (`teShaderInstance`).
//!
//! The group and code layouts below are read off the install (the code
//! reference agrees with none of what the assets carry, and the group
//! reference describes a different header entirely). The instance layout is a
//! port of the reference reader only: this build's registry holds zero
//! type-`0x086` assets, so no instance below is verified against real data.
//! See `docs/formats/shaders.md` for the findings.
//!
//! Group (`0x085`), header 184 bytes, `N = u32` at `0x64`. Verified on 100
//! openable groups; every offset relation below held on all of them:
//!
//! ```text
//! 0x00  u64 unknown0         a type-0x040 GUID; likely the group's source
//! 0x08  u64 x_offset         always 184 (the header end)
//! 0x10  u64 a_offset         X = 24-byte records × N, all zero observed
//! 0x18  u64 null             -1 on every group inspected
//! 0x20  {i16, i16, i16, i16} repeated three times; small numbers (< N)
//! 0x38  u64 zero
//! 0x40  {i16, i16, i16, i16} repeated three times; small numbers (< N)
//! 0x58  u64 b_offset         B = u32 × N, padded to 8; all zero observed
//! 0x60  u32 unknown60        seen 0x01100001, 0x01000001, 0x00200004
//! 0x64  u32 count            N
//! 0x68  u64 c_offset         C = u64 × N, code GUIDs (type 0x087)
//! 0x70  u64 d_offset         D = u64 × N, all 0xFF observed
//! 0x78  u64 e_offset         E = u64 × N, source GUIDs (type 0x040)
//! 0x80  u64 f_offset         F = u64 × N, all 0xFF observed; ends at EOF
//! 0x88  u64 zero
//! 0x90  u64 zero
//! 0x98  u64 source_guid      type 0x088
//! 0xA0  u64 zero
//! 0xA8  u64 zero
//! 0xB0  u64 0x000000FFFFFFFFFF
//! ```
//!
//! Code (`0x087`), header at least 140 bytes; verified on the 144 code assets
//! the install test could open (of the first 200 sampled; the probe's 30/30
//! sample agrees):
//!
//! ```text
//! 0x00  u64 unknown0         -1 on every code inspected
//! 0x08  u64 unknown1         seen 0x0000000140000401 and 0x0000000140000402
//! 0x70  u64 data_offset      where the gzip stream starts
//! 0x84  u32 compressed_size  equal to asset length - data_offset, always
//! 0x88  u32 uncompressed_size  the inflated length, always
//! ```
//!
//! Evidence classes: *install* for group and code (bytes read via
//! `bin/shader.rs`), *decompiled* for instance (TankLib `teShaderInstance.cs`,
//! validated against nothing).

use std::fmt;

/// Asset type number of a shader group.
pub const TYPE_GROUP: u16 = 0x085;
/// Asset type number of a shader instance.
pub const TYPE_INSTANCE: u16 = 0x086;
/// Asset type number of a shader code blob, and the GUID type groups carry
/// for them.
pub const TYPE_CODE: u16 = 0x087;
/// GUID type carried by a group's source references (the E array, and
/// `unknown0`). In the first 100 openable groups, 2882 of 2933 source refs are
/// this type; the other 51 are type 0x001.
pub const SOURCE_TYPE: u16 = 0x040;

#[derive(Debug)]
pub enum Error {
    Malformed(&'static str),
    Decompress(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Malformed(m) => write!(f, "malformed shader asset: {m}"),
            Error::Decompress(m) => write!(f, "decompressing shader code: {m}"),
        }
    }
}

impl std::error::Error for Error {}

type Result<T> = std::result::Result<T, Error>;

fn rd_u32(data: &[u8], at: usize) -> Result<u32> {
    data.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or(Error::Malformed("read past the end"))
}

fn rd_u64(data: &[u8], at: usize) -> Result<u64> {
    data.get(at..at + 8)
        .map(|s| u64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))
        .ok_or(Error::Malformed("read past the end"))
}

fn rd_i16(data: &[u8], at: usize) -> Result<i16> {
    data.get(at..at + 2)
        .map(|s| i16::from_le_bytes([s[0], s[1]]))
        .ok_or(Error::Malformed("read past the end"))
}

fn rd_u16(data: &[u8], at: usize) -> Result<u16> {
    data.get(at..at + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
        .ok_or(Error::Malformed("read past the end"))
}

// ---------------------------------------------------------------- code

/// Header field offsets of a code asset.
const CODE_UNKNOWN0: usize = 0x00;
const CODE_UNKNOWN1: usize = 0x08;
const CODE_DATA_OFFSET: usize = 0x70;
const CODE_COMPRESSED: usize = 0x84;
const CODE_UNCOMPRESSED: usize = 0x88;
/// Smallest code asset that still holds every field read below.
const CODE_HEADER_MIN: usize = 0x8C;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeHeader {
    /// `-1` on every code asset inspected; meaning unknown.
    pub unknown0: u64,
    /// `0x0000000140000401` or `0x0000000140000402` on the assets inspected;
    /// meaning unknown (likely a bytecode/profile pair).
    pub unknown1: u64,
    /// Where the gzip stream starts.
    pub data_offset: u64,
    /// `asset length - data_offset` on every code asset inspected.
    pub compressed_size: u32,
    /// The inflated length on every code asset inspected.
    pub uncompressed_size: u32,
}

#[derive(Debug, Clone)]
pub struct ShaderCode {
    pub header: CodeHeader,
    /// The inflated bytecode.
    pub bytecode: Vec<u8>,
}

/// Parse a shader code asset. Inflates the gzip payload eagerly and keeps the
/// bytes; the compressed form is not exposed.
pub fn parse_code(data: &[u8]) -> Result<ShaderCode> {
    if data.len() < CODE_HEADER_MIN {
        return Err(Error::Malformed("shorter than its 140-byte header"));
    }
    let header = CodeHeader {
        unknown0: rd_u64(data, CODE_UNKNOWN0)?,
        unknown1: rd_u64(data, CODE_UNKNOWN1)?,
        data_offset: rd_u64(data, CODE_DATA_OFFSET)?,
        compressed_size: rd_u32(data, CODE_COMPRESSED)?,
        uncompressed_size: rd_u32(data, CODE_UNCOMPRESSED)?,
    };

    let at = usize::try_from(header.data_offset)
        .map_err(|_| Error::Malformed("gzip data offset past the end"))?;
    if at >= data.len() {
        return Err(Error::Malformed("gzip data offset past the end"));
    }
    if data.get(at..at + 2) != Some(&[0x1f, 0x8b]) {
        return Err(Error::Malformed(
            "payload does not start with the gzip magic",
        ));
    }
    if header.compressed_size as usize != data.len() - at {
        return Err(Error::Malformed(
            "compressed size does not match the asset length",
        ));
    }

    use std::io::Read;
    let mut decoder = flate2::read::GzDecoder::new(&data[at..]);
    let mut bytecode = Vec::with_capacity(header.uncompressed_size as usize);
    decoder
        .read_to_end(&mut bytecode)
        .map_err(|e| Error::Decompress(e.to_string()))?;
    if bytecode.len() != header.uncompressed_size as usize {
        return Err(Error::Malformed(
            "decompressed length does not match its declared size",
        ));
    }

    Ok(ShaderCode { header, bytecode })
}

// ---------------------------------------------------------------- group

/// Bytes before the first record of a group's arrays.
pub const GROUP_HEADER_SIZE: usize = 0xB8;

const GROUP_UNKNOWN0: usize = 0x00;
const GROUP_X_OFFSET: usize = 0x08;
const GROUP_A_OFFSET: usize = 0x10;
const GROUP_B_OFFSET: usize = 0x58;
const GROUP_UNKNOWN60: usize = 0x60;
const GROUP_COUNT: usize = 0x64;
const GROUP_C_OFFSET: usize = 0x68;
const GROUP_D_OFFSET: usize = 0x70;
const GROUP_E_OFFSET: usize = 0x78;
const GROUP_F_OFFSET: usize = 0x80;
const GROUP_G_OFFSET: usize = 0x88;
const GROUP_H_OFFSET: usize = 0x90;
const GROUP_SOURCE_GUID: usize = 0x98;

/// One shader of a group: its code blob and its source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupEntry {
    /// Code GUID, type [`TYPE_CODE`] on every entry of every group inspected.
    pub code: u64,
    /// Source GUID, type [`SOURCE_TYPE`] on 2882 of the 2933 entries across
    /// the first 100 openable groups (type 0x001 on the rest).
    pub source: u64,
    /// The G array's `u64` for this record (one per record by size). Its
    /// meaning is unknown: in the groups inspected it is a single value for
    /// most records with a block of another value in the middle.
    pub hash: u64,
}

#[derive(Debug, Clone)]
pub struct ShaderGroup {
    /// `0x00`: a type-`0x040` GUID; likely the group's own source. Fitted, not
    /// confirmed.
    pub unknown0: u64,
    /// `0x60`: meaning unknown.
    pub unknown60: u32,
    /// `0x98`: type-`0x088` GUID; the reference names this field SourceGUID.
    pub source_guid: u64,
    /// The C and E arrays zipped: one entry per shader, in order.
    pub entries: Vec<GroupEntry>,
}

/// Parse a shader group asset.
///
/// Every array offset is checked against the record count before anything is
/// read, so a group whose layout differs fails here instead of yielding
/// garbage.
pub fn parse_group(data: &[u8]) -> Result<ShaderGroup> {
    if data.len() < GROUP_HEADER_SIZE {
        return Err(Error::Malformed("shorter than its 184-byte header"));
    }
    if rd_u64(data, GROUP_X_OFFSET)? != GROUP_HEADER_SIZE as u64 {
        return Err(Error::Malformed(
            "first array does not start at the header end",
        ));
    }

    let a = rd_u64(data, GROUP_A_OFFSET)?;
    let b = rd_u64(data, GROUP_B_OFFSET)?;
    let n = rd_u32(data, GROUP_COUNT)? as u64;
    let c = rd_u64(data, GROUP_C_OFFSET)?;
    let d = rd_u64(data, GROUP_D_OFFSET)?;
    let e = rd_u64(data, GROUP_E_OFFSET)?;
    let f = rd_u64(data, GROUP_F_OFFSET)?;
    let g = rd_u64(data, GROUP_G_OFFSET)?;
    let h = rd_u64(data, GROUP_H_OFFSET)?;

    // X | A | B (padded to 8) | C | D | E | F | G | H | end
    let head = GROUP_HEADER_SIZE as u64;
    let want_a = head + 24 * n;
    let want_b = want_a + 8 * n;
    let want_c = want_b + 4 * n;
    if a != want_a || b != want_b || (c != want_c && c != want_c + 4) {
        return Err(Error::Malformed(
            "array offsets do not match the record count",
        ));
    }
    // The later arrays chain off C where it actually sits, padded or not.
    let want_d = c + 8 * n;
    let want_e = want_d + 8 * n;
    let want_f = want_e + 8 * n;
    let want_g = want_f + 8 * n;
    let want_h = want_g + 8 * n;
    let want_end = want_h + 8 * n;
    if d != want_d
        || e != want_e
        || f != want_f
        || g != want_g
        || h != want_h
        || data.len() as u64 != want_end
    {
        return Err(Error::Malformed(
            "array offsets do not match the record count",
        ));
    }

    let n = n as usize;
    let mut entries = Vec::with_capacity(n);
    for i in 0..n {
        entries.push(GroupEntry {
            code: rd_u64(data, c as usize + i * 8)?,
            source: rd_u64(data, e as usize + i * 8)?,
            hash: rd_u64(data, g as usize + i * 8)?,
        });
    }

    Ok(ShaderGroup {
        unknown0: rd_u64(data, GROUP_UNKNOWN0)?,
        unknown60: rd_u32(data, GROUP_UNKNOWN60)?,
        source_guid: rd_u64(data, GROUP_SOURCE_GUID)?,
        entries,
    })
}

// ---------------------------------------------------------------- instance

/// Ported from TankLib `teShaderInstance.cs`. Field offsets below are what
/// that reader's `Pack`-annotated structs produce under `Marshal` sizing (the
/// C# source comments disagree for `BufferHeader`; the struct is what the
/// reader actually executes). No asset in this build's registry is a type
/// `0x086`, so nothing here is verified against real data.
const INSTANCE_HEADER_SIZE: usize = 108;
const SHADER_RESOURCE_SIZE: usize = 12;
const SAMPLER_STATE_SIZE: usize = 12;
const INPUT_ELEMENT_SIZE: usize = 8;
const BUFFER_HEADER_SIZE: usize = 40;
const BUFFER_PART_SIZE: usize = 12;

const IH_BUFFER_OFFSET: usize = 0;
const IH_SHADER_RESOURCE_OFFSET: usize = 8;
const IH_SAMPLER_STATES_OFFSET: usize = 16;
const IH_VERTEX_LAYOUT_OFFSET: usize = 24;
const IH_SHADER_CODE: usize = 48;
const IH_UNK1: usize = 76;
const IH_SHADER_INPUT_CRC: usize = 80;
const IH_UNK4: usize = 94;
const IH_NUM_CONSTANT_BUFFERS: usize = 102;
const IH_NUM_SHADER_RESOURCES: usize = 103;
const IH_SAMPLER_STATE_COUNT: usize = 104;
const IH_VERTEX_INPUT_ELEMENT_COUNT: usize = 105;

const BH_PART_OFFSET: usize = 0;
const BH_SKELETON_OFFSET: usize = 8;
const BH_BUFFER_SIZE: usize = 16;
const BH_HASH: usize = 28;
const BH_PART_COUNT: usize = 36;
const BH_REGISTER: usize = 38;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InstanceHeader {
    /// Offset of the constant-buffer headers; 0 = none.
    pub buffer_offset: u64,
    /// Offset of the shader resource table; 0 = none.
    pub shader_resource_offset: u64,
    /// Offset of the sampler state table; 0 = none.
    pub sampler_states_offset: u64,
    /// Offset of the vertex input layout; 0 = none.
    pub vertex_layout_offset: u64,
    /// GUID of the code asset this instance binds (type [`TYPE_CODE`]).
    pub shader_code: u64,
    pub unk1: u32,
    pub shader_input_crc: u32,
    pub unk4: i16,
    pub num_constant_buffers: u8,
    pub num_shader_resources: u8,
    pub sampler_state_count: u8,
    pub vertex_input_element_count: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShaderResource {
    pub name_hash: u32,
    pub register: u8,
    pub flags: u8,
    pub view_dimension: u8,
    pub global_index: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SamplerState {
    pub name_hash: u32,
    pub register: u8,
    pub global_index: i8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputElement {
    pub input_use: u8,
    pub register: u8,
    pub format: i16,
    pub slot: u8,
    pub unk1: u8,
    pub unk2: u8,
    pub unk3: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BufferPart {
    pub hash: u32,
    pub size: u16,
    pub offset: u16,
    pub element_size: u8,
    pub element_count: u8,
    pub is_dynamic: u8,
    pub unknown: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BufferHeader {
    /// Relative to the start of this buffer header.
    pub part_offset: u64,
    /// Relative to the start of this buffer header; 0 = no skeleton.
    pub skeleton_offset: u64,
    pub buffer_size: u16,
    pub hash: u64,
    pub part_count: i16,
    pub register: u8,
}

#[derive(Debug, Clone)]
pub struct ShaderInstance {
    pub header: InstanceHeader,
    pub shader_resources: Vec<ShaderResource>,
    pub sampler_states: Vec<SamplerState>,
    pub vertex_layout: Vec<InputElement>,
    pub buffer_headers: Vec<BufferHeader>,
    /// One part list per buffer header, in order.
    pub buffer_parts: Vec<Vec<BufferPart>>,
    /// One skeleton per buffer header, in order; `None` where the header's
    /// skeleton offset is zero.
    pub buffer_skeletons: Vec<Option<Vec<u8>>>,
}

fn table_at(data: &[u8], offset: u64, count: usize, stride: usize) -> Result<usize> {
    let at = usize::try_from(offset).map_err(|_| Error::Malformed("offset out of range"))?;
    let end = count
        .checked_mul(stride)
        .and_then(|bytes| at.checked_add(bytes))
        .ok_or(Error::Malformed("offset out of range"))?;
    if end > data.len() {
        return Err(Error::Malformed("read past the end"));
    }
    Ok(at)
}

fn relative(data: &[u8], start: usize, offset: u64, len: usize) -> Result<usize> {
    let at = usize::try_from(offset)
        .ok()
        .and_then(|o| start.checked_add(o))
        .ok_or(Error::Malformed("offset out of range"))?;
    if at.checked_add(len).is_none_or(|end| end > data.len()) {
        return Err(Error::Malformed("read past the end"));
    }
    Ok(at)
}

/// Parse a shader instance asset, following the reference reader's read order
/// and its zero-offset/length gating.
pub fn parse_instance(data: &[u8]) -> Result<ShaderInstance> {
    if data.len() < INSTANCE_HEADER_SIZE {
        return Err(Error::Malformed("shorter than its 108-byte header"));
    }
    let header = InstanceHeader {
        buffer_offset: rd_u64(data, IH_BUFFER_OFFSET)?,
        shader_resource_offset: rd_u64(data, IH_SHADER_RESOURCE_OFFSET)?,
        sampler_states_offset: rd_u64(data, IH_SAMPLER_STATES_OFFSET)?,
        vertex_layout_offset: rd_u64(data, IH_VERTEX_LAYOUT_OFFSET)?,
        shader_code: rd_u64(data, IH_SHADER_CODE)?,
        unk1: rd_u32(data, IH_UNK1)?,
        shader_input_crc: rd_u32(data, IH_SHADER_INPUT_CRC)?,
        unk4: rd_i16(data, IH_UNK4)?,
        num_constant_buffers: *data.get(IH_NUM_CONSTANT_BUFFERS).unwrap(),
        num_shader_resources: *data.get(IH_NUM_SHADER_RESOURCES).unwrap(),
        sampler_state_count: *data.get(IH_SAMPLER_STATE_COUNT).unwrap(),
        vertex_input_element_count: *data.get(IH_VERTEX_INPUT_ELEMENT_COUNT).unwrap(),
    };

    let mut shader_resources = Vec::new();
    if header.shader_resource_offset != 0 {
        let count = header.num_shader_resources as usize;
        let at = table_at(
            data,
            header.shader_resource_offset,
            count,
            SHADER_RESOURCE_SIZE,
        )?;
        for i in 0..count {
            let p = at + i * SHADER_RESOURCE_SIZE;
            shader_resources.push(ShaderResource {
                name_hash: rd_u32(data, p)?,
                register: data[p + 4],
                flags: data[p + 5],
                view_dimension: data[p + 6],
                global_index: data[p + 11],
            });
        }
    }

    let mut sampler_states = Vec::new();
    if header.sampler_states_offset > 0 {
        let count = header.sampler_state_count as usize;
        let at = table_at(
            data,
            header.sampler_states_offset,
            count,
            SAMPLER_STATE_SIZE,
        )?;
        for i in 0..count {
            let p = at + i * SAMPLER_STATE_SIZE;
            sampler_states.push(SamplerState {
                name_hash: rd_u32(data, p)?,
                register: data[p + 4],
                global_index: data[p + 8] as i8,
            });
        }
    }

    let mut vertex_layout = Vec::new();
    if header.vertex_layout_offset > 0 {
        let count = header.vertex_input_element_count as usize;
        let at = table_at(data, header.vertex_layout_offset, count, INPUT_ELEMENT_SIZE)?;
        for i in 0..count {
            let p = at + i * INPUT_ELEMENT_SIZE;
            vertex_layout.push(InputElement {
                input_use: data[p],
                register: data[p + 1],
                format: rd_i16(data, p + 2)?,
                slot: data[p + 4],
                unk1: data[p + 5],
                unk2: data[p + 6],
                unk3: data[p + 7],
            });
        }
    }

    let mut buffer_headers = Vec::new();
    let mut buffer_parts = Vec::new();
    let mut buffer_skeletons = Vec::new();
    if header.buffer_offset > 0 {
        let mut cursor = usize::try_from(header.buffer_offset)
            .map_err(|_| Error::Malformed("offset out of range"))?;
        for _ in 0..header.num_constant_buffers {
            if cursor + BUFFER_HEADER_SIZE > data.len() {
                return Err(Error::Malformed("read past the end"));
            }
            let bh = BufferHeader {
                part_offset: rd_u64(data, cursor + BH_PART_OFFSET)?,
                skeleton_offset: rd_u64(data, cursor + BH_SKELETON_OFFSET)?,
                buffer_size: rd_u16(data, cursor + BH_BUFFER_SIZE)?,
                hash: rd_u64(data, cursor + BH_HASH)?,
                part_count: rd_i16(data, cursor + BH_PART_COUNT)?,
                register: data[cursor + BH_REGISTER],
            };

            let skeleton = if bh.skeleton_offset != 0 {
                let at = relative(data, cursor, bh.skeleton_offset, bh.buffer_size as usize)?;
                Some(data[at..at + bh.buffer_size as usize].to_vec())
            } else {
                None
            };

            if bh.part_count < 0 {
                return Err(Error::Malformed("negative part count"));
            }
            let count = bh.part_count as usize;
            let at = relative(data, cursor, bh.part_offset, count * BUFFER_PART_SIZE)?;
            let mut parts = Vec::with_capacity(count);
            for i in 0..count {
                let p = at + i * BUFFER_PART_SIZE;
                parts.push(BufferPart {
                    hash: rd_u32(data, p)?,
                    size: rd_u16(data, p + 4)?,
                    offset: rd_u16(data, p + 6)?,
                    element_size: data[p + 8],
                    element_count: data[p + 9],
                    is_dynamic: data[p + 10],
                    unknown: data[p + 11],
                });
            }

            buffer_headers.push(bh);
            buffer_parts.push(parts);
            buffer_skeletons.push(skeleton);
            cursor += BUFFER_HEADER_SIZE;
        }
    }

    Ok(ShaderInstance {
        header,
        shader_resources,
        sampler_states,
        vertex_layout,
        buffer_headers,
        buffer_parts,
        buffer_skeletons,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real code GUID (`0x0610847cd3f6ecd7`) and a real source GUID
    /// (`0x0fc0fd2231bd6e04`), both read off the install.
    const REAL_CODE: u64 = 0x0610_847C_D3F6_ECD7;
    const REAL_SOURCE: u64 = 0x0FC0_FD22_31BD_6E04;

    fn gzip(payload: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(payload).expect("writes");
        enc.finish().expect("finishes")
    }

    /// A code asset laid out the way the install writes one: gzip payload at
    /// 0x90, sizes and offsets in the header.
    fn synthetic_code(bytecode: &[u8]) -> Vec<u8> {
        let data_offset = 0x90usize;
        let payload = gzip(bytecode);
        let mut out = vec![0u8; data_offset];
        out[CODE_UNKNOWN0..CODE_UNKNOWN0 + 8].copy_from_slice(&u64::MAX.to_le_bytes());
        out[CODE_UNKNOWN1..CODE_UNKNOWN1 + 8]
            .copy_from_slice(&0x0000_0001_4000_0401u64.to_le_bytes());
        out[CODE_DATA_OFFSET..CODE_DATA_OFFSET + 8]
            .copy_from_slice(&(data_offset as u64).to_le_bytes());
        out[CODE_COMPRESSED..CODE_COMPRESSED + 4]
            .copy_from_slice(&(payload.len() as u32).to_le_bytes());
        out[CODE_UNCOMPRESSED..CODE_UNCOMPRESSED + 4]
            .copy_from_slice(&(bytecode.len() as u32).to_le_bytes());
        out.extend_from_slice(&payload);
        out
    }

    #[test]
    fn code_parses_and_inflates_a_synthetic_asset() {
        let bytecode: Vec<u8> = (0..4096u32).map(|i| (i * 7) as u8).collect();
        let data = synthetic_code(&bytecode);

        let code = parse_code(&data).expect("parses");
        assert_eq!(code.header.unknown0, u64::MAX);
        assert_eq!(code.header.unknown1, 0x0000_0001_4000_0401);
        assert_eq!(code.header.data_offset, 0x90);
        assert_eq!(code.header.compressed_size as usize, data.len() - 0x90);
        assert_eq!(code.header.uncompressed_size as usize, bytecode.len());
        assert_eq!(code.bytecode, bytecode);
    }

    /// Each field offset is pinned by corrupting exactly that field: a
    /// misplaced read turns into a different failure or a wrong value.
    #[test]
    fn code_field_offsets_are_pinned() {
        let data = synthetic_code(b"empty");

        let mut wrong_offset = data.clone();
        wrong_offset[CODE_DATA_OFFSET..CODE_DATA_OFFSET + 8]
            .copy_from_slice(&0x94u64.to_le_bytes());
        assert!(matches!(
            parse_code(&wrong_offset),
            Err(Error::Malformed(
                "payload does not start with the gzip magic"
            ))
        ));

        let mut wrong_compressed = data.clone();
        wrong_compressed[CODE_COMPRESSED..CODE_COMPRESSED + 4].copy_from_slice(&0u32.to_le_bytes());
        assert!(matches!(
            parse_code(&wrong_compressed),
            Err(Error::Malformed(
                "compressed size does not match the asset length"
            ))
        ));

        let mut wrong_uncompressed = data.clone();
        wrong_uncompressed[CODE_UNCOMPRESSED..CODE_UNCOMPRESSED + 4]
            .copy_from_slice(&4u32.to_le_bytes());
        assert!(matches!(
            parse_code(&wrong_uncompressed),
            Err(Error::Malformed(
                "decompressed length does not match its declared size"
            ))
        ));
    }

    #[test]
    fn code_rejects_short_and_misdeclared_assets() {
        assert!(matches!(
            parse_code(&[0u8; CODE_HEADER_MIN - 1]),
            Err(Error::Malformed("shorter than its 140-byte header"))
        ));

        let mut data = synthetic_code(b"trailing");
        data.push(0);
        assert!(matches!(
            parse_code(&data),
            Err(Error::Malformed(
                "compressed size does not match the asset length"
            ))
        ));
    }

    /// A group laid out the way the install writes one: the arrays in order,
    /// B padded to eight bytes.
    fn synthetic_group(codes: &[u64], sources: &[u64]) -> Vec<u8> {
        let n = codes.len();
        assert_eq!(n, sources.len());
        let x = GROUP_HEADER_SIZE;
        let a = x + 24 * n;
        let b = a + 8 * n;
        let c = b + (4 * n).div_ceil(8) * 8;
        let d = c + 8 * n;
        let e = d + 8 * n;
        let f = e + 8 * n;
        let g = f + 8 * n;
        let h = g + 8 * n;
        let len = h + 8 * n;

        let mut out = vec![0u8; len];
        out[GROUP_UNKNOWN0..GROUP_UNKNOWN0 + 8]
            .copy_from_slice(&0x0FC0_FFFE_7BFF_EFBFu64.to_le_bytes());
        out[GROUP_X_OFFSET..GROUP_X_OFFSET + 8].copy_from_slice(&(x as u64).to_le_bytes());
        out[GROUP_A_OFFSET..GROUP_A_OFFSET + 8].copy_from_slice(&(a as u64).to_le_bytes());
        out[0x18..0x20].copy_from_slice(&u64::MAX.to_le_bytes());
        out[GROUP_B_OFFSET..GROUP_B_OFFSET + 8].copy_from_slice(&(b as u64).to_le_bytes());
        out[GROUP_UNKNOWN60..GROUP_UNKNOWN60 + 4].copy_from_slice(&0x0110_0001u32.to_le_bytes());
        out[GROUP_COUNT..GROUP_COUNT + 4].copy_from_slice(&(n as u32).to_le_bytes());
        out[GROUP_C_OFFSET..GROUP_C_OFFSET + 8].copy_from_slice(&(c as u64).to_le_bytes());
        out[GROUP_D_OFFSET..GROUP_D_OFFSET + 8].copy_from_slice(&(d as u64).to_le_bytes());
        out[GROUP_E_OFFSET..GROUP_E_OFFSET + 8].copy_from_slice(&(e as u64).to_le_bytes());
        out[GROUP_F_OFFSET..GROUP_F_OFFSET + 8].copy_from_slice(&(f as u64).to_le_bytes());
        out[GROUP_G_OFFSET..GROUP_G_OFFSET + 8].copy_from_slice(&(g as u64).to_le_bytes());
        out[GROUP_H_OFFSET..GROUP_H_OFFSET + 8].copy_from_slice(&(h as u64).to_le_bytes());
        out[GROUP_SOURCE_GUID..GROUP_SOURCE_GUID + 8]
            .copy_from_slice(&0x0E10_0000_0000_0032u64.to_le_bytes());
        out[0xB0..0xB8].copy_from_slice(&0x0000_00FF_FFFF_FFFFu64.to_le_bytes());
        for i in 0..n {
            out[c + i * 8..c + i * 8 + 8].copy_from_slice(&codes[i].to_le_bytes());
            out[e + i * 8..e + i * 8 + 8].copy_from_slice(&sources[i].to_le_bytes());
            out[g + i * 8..g + i * 8 + 8]
                .copy_from_slice(&(0x0D08_8000_EC22_C503u64).to_le_bytes());
            out[h + i * 8..h + i * 8 + 8].copy_from_slice(&u64::MAX.to_le_bytes());
        }
        out
    }

    #[test]
    fn group_parses_a_synthetic_group() {
        let data = synthetic_group(&[REAL_CODE, REAL_CODE + 1], &[REAL_SOURCE, REAL_SOURCE + 1]);

        let group = parse_group(&data).expect("parses");
        assert_eq!(group.unknown0, 0x0FC0_FFFE_7BFF_EFBF);
        assert_eq!(group.unknown60, 0x0110_0001);
        assert_eq!(group.source_guid, 0x0E10_0000_0000_0032);
        assert_eq!(group.entries.len(), 2);
        assert_eq!(
            group.entries[0],
            GroupEntry {
                code: REAL_CODE,
                source: REAL_SOURCE,
                hash: 0x0D08_8000_EC22_C503,
            }
        );
        assert_eq!(group.entries[1].code, REAL_CODE + 1);
        assert_eq!(crate::guid::type_of(group.entries[0].code), TYPE_CODE);
        assert_eq!(crate::guid::type_of(group.entries[0].source), SOURCE_TYPE);
    }

    /// Three entries exercise the u32-B padding step (4×3 is not 8-aligned).
    #[test]
    fn group_parses_an_odd_count_and_an_empty_group() {
        let data = synthetic_group(&[REAL_CODE; 3], &[REAL_SOURCE; 3]);
        let group = parse_group(&data).expect("parses");
        assert_eq!(group.entries.len(), 3);

        let empty = synthetic_group(&[], &[]);
        assert_eq!(empty.len(), GROUP_HEADER_SIZE);
        let group = parse_group(&empty).expect("parses");
        assert!(group.entries.is_empty());
    }

    /// Every array offset is pinned: moving any one of them by eight breaks
    /// the offset chain.
    #[test]
    fn group_array_offsets_are_pinned() {
        let base = synthetic_group(&[REAL_CODE; 3], &[REAL_SOURCE; 3]);
        for at in [
            GROUP_X_OFFSET,
            GROUP_A_OFFSET,
            GROUP_B_OFFSET,
            GROUP_C_OFFSET,
            GROUP_D_OFFSET,
            GROUP_E_OFFSET,
            GROUP_F_OFFSET,
            GROUP_G_OFFSET,
            GROUP_H_OFFSET,
        ] {
            let mut data = base.clone();
            let mut v = u64::from_le_bytes(data[at..at + 8].try_into().unwrap());
            v += 8;
            data[at..at + 8].copy_from_slice(&v.to_le_bytes());
            assert!(
                matches!(
                    parse_group(&data),
                    Err(Error::Malformed(
                        "first array does not start at the header end"
                            | "array offsets do not match the record count"
                    ))
                ),
                "offset at {at:#x} was not checked"
            );
        }
    }

    #[test]
    fn group_rejects_a_count_that_grows() {
        let mut data = synthetic_group(&[REAL_CODE; 2], &[REAL_SOURCE; 2]);
        data[GROUP_COUNT..GROUP_COUNT + 4].copy_from_slice(&3u32.to_le_bytes());
        assert!(matches!(
            parse_group(&data),
            Err(Error::Malformed(
                "array offsets do not match the record count"
            ))
        ));
    }

    #[test]
    fn group_rejects_a_short_header() {
        assert!(matches!(
            parse_group(&[0u8; GROUP_HEADER_SIZE - 1]),
            Err(Error::Malformed("shorter than its 184-byte header"))
        ));
    }

    fn put_u64(out: &mut [u8], at: usize, v: u64) {
        out[at..at + 8].copy_from_slice(&v.to_le_bytes());
    }
    fn put_u32(out: &mut [u8], at: usize, v: u32) {
        out[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn put_i16(out: &mut [u8], at: usize, v: i16) {
        out[at..at + 2].copy_from_slice(&v.to_le_bytes());
    }

    /// An instance laid out the way the reference reader walks one: header,
    /// then two of each table, then one constant buffer with a skeleton.
    fn synthetic_instance() -> Vec<u8> {
        let resources_at = 108usize;
        let samplers_at = resources_at + 2 * SHADER_RESOURCE_SIZE;
        let vertex_at = samplers_at + 2 * SAMPLER_STATE_SIZE;
        let buffers_at = vertex_at + 2 * INPUT_ELEMENT_SIZE;
        let parts_at = buffers_at + BUFFER_HEADER_SIZE;
        let skeleton_at = parts_at + 2 * BUFFER_PART_SIZE;
        let len = skeleton_at + 6;

        let mut out = vec![0u8; len];
        put_u64(&mut out, IH_BUFFER_OFFSET, buffers_at as u64);
        put_u64(&mut out, IH_SHADER_RESOURCE_OFFSET, resources_at as u64);
        put_u64(&mut out, IH_SAMPLER_STATES_OFFSET, samplers_at as u64);
        put_u64(&mut out, IH_VERTEX_LAYOUT_OFFSET, vertex_at as u64);
        put_u64(&mut out, IH_SHADER_CODE, REAL_CODE);
        put_u32(&mut out, IH_UNK1, 7);
        put_u32(&mut out, IH_SHADER_INPUT_CRC, 0xC0FF_EE12);
        put_i16(&mut out, IH_UNK4, -3);
        out[IH_NUM_CONSTANT_BUFFERS] = 1;
        out[IH_NUM_SHADER_RESOURCES] = 2;
        out[IH_SAMPLER_STATE_COUNT] = 2;
        out[IH_VERTEX_INPUT_ELEMENT_COUNT] = 2;

        for i in 0..2 {
            let p = resources_at + i * SHADER_RESOURCE_SIZE;
            put_u32(&mut out, p, 0xA000 + i as u32);
            out[p + 4] = 1 + i as u8;
            out[p + 5] = 0x10;
            out[p + 6] = 2 + i as u8;
            out[p + 11] = 5;
        }
        for i in 0..2 {
            let p = samplers_at + i * SAMPLER_STATE_SIZE;
            put_u32(&mut out, p, 0xB000 + i as u32);
            out[p + 4] = 2 + i as u8;
            out[p + 8] = (-5i8 as u8).wrapping_add(i as u8);
        }
        for i in 0..2 {
            let p = vertex_at + i * INPUT_ELEMENT_SIZE;
            out[p] = 9 + i as u8;
            out[p + 1] = i as u8;
            put_i16(&mut out, p + 2, 3 + i as i16);
            out[p + 4] = 15;
        }

        put_u64(&mut out, buffers_at + BH_PART_OFFSET, 40);
        put_u64(&mut out, buffers_at + BH_SKELETON_OFFSET, 64);
        put_u16_test(&mut out, buffers_at + BH_BUFFER_SIZE, 6);
        put_u64(&mut out, buffers_at + BH_HASH, 0x1122_3344_5566_7788);
        put_i16(&mut out, buffers_at + BH_PART_COUNT, 2);
        out[buffers_at + BH_REGISTER] = 3;

        for i in 0..2 {
            let p = parts_at + i * BUFFER_PART_SIZE;
            put_u32(&mut out, p, 0xC000 + i as u32);
            put_u16_test(&mut out, p + 4, 0x20 + i as u16);
            put_u16_test(&mut out, p + 6, 0x100 + i as u16);
            out[p + 8] = 4;
            out[p + 9] = 1 + i as u8;
            out[p + 10] = i as u8;
            out[p + 11] = 0xFF;
        }
        out[skeleton_at..skeleton_at + 6].copy_from_slice(&[1, 2, 3, 4, 5, 6]);

        out
    }

    fn put_u16_test(out: &mut [u8], at: usize, v: u16) {
        out[at..at + 2].copy_from_slice(&v.to_le_bytes());
    }

    #[test]
    fn instance_parses_a_synthetic_instance() {
        let data = synthetic_instance();
        let inst = parse_instance(&data).expect("parses");

        assert_eq!(inst.header.buffer_offset, 172);
        assert_eq!(inst.header.shader_resource_offset, 108);
        assert_eq!(inst.header.sampler_states_offset, 132);
        assert_eq!(inst.header.vertex_layout_offset, 156);
        assert_eq!(inst.header.shader_code, REAL_CODE);
        assert_eq!(crate::guid::type_of(inst.header.shader_code), TYPE_CODE);
        assert_eq!(inst.header.unk1, 7);
        assert_eq!(inst.header.shader_input_crc, 0xC0FF_EE12);
        assert_eq!(inst.header.unk4, -3);
        assert_eq!(inst.header.num_constant_buffers, 1);
        assert_eq!(inst.header.num_shader_resources, 2);
        assert_eq!(inst.header.sampler_state_count, 2);
        assert_eq!(inst.header.vertex_input_element_count, 2);

        assert_eq!(inst.shader_resources.len(), 2);
        assert_eq!(
            inst.shader_resources[1],
            ShaderResource {
                name_hash: 0xA001,
                register: 2,
                flags: 0x10,
                view_dimension: 3,
                global_index: 5
            }
        );
        assert_eq!(
            inst.sampler_states[1],
            SamplerState {
                name_hash: 0xB001,
                register: 3,
                global_index: -4
            }
        );
        assert_eq!(inst.vertex_layout[1].input_use, 10);
        assert_eq!(inst.vertex_layout[1].format, 4);
        assert_eq!(inst.vertex_layout[1].slot, 15);

        assert_eq!(inst.buffer_headers.len(), 1);
        assert_eq!(
            inst.buffer_headers[0],
            BufferHeader {
                part_offset: 40,
                skeleton_offset: 64,
                buffer_size: 6,
                hash: 0x1122_3344_5566_7788,
                part_count: 2,
                register: 3,
            }
        );
        assert_eq!(inst.buffer_parts[0].len(), 2);
        assert_eq!(inst.buffer_parts[0][1].size, 0x21);
        assert_eq!(inst.buffer_parts[0][1].offset, 0x101);
        assert_eq!(inst.buffer_parts[0][1].is_dynamic, 1);
        assert_eq!(
            inst.buffer_skeletons[0].as_deref(),
            Some(&[1u8, 2, 3, 4, 5, 6][..])
        );
    }

    #[test]
    fn instance_with_zero_offsets_parses_empty() {
        let data = vec![0u8; INSTANCE_HEADER_SIZE];
        let inst = parse_instance(&data).expect("parses");
        assert_eq!(inst.header, InstanceHeader::default());
        assert!(inst.shader_resources.is_empty());
        assert!(inst.sampler_states.is_empty());
        assert!(inst.vertex_layout.is_empty());
        assert!(inst.buffer_headers.is_empty());
    }

    #[test]
    fn instance_rejects_tables_and_parts_past_the_end() {
        let mut data = synthetic_instance();
        data[IH_SAMPLER_STATE_COUNT] = 200;
        assert!(matches!(
            parse_instance(&data),
            Err(Error::Malformed("read past the end"))
        ));

        let mut data = synthetic_instance();
        put_i16(&mut data, 172 + BH_PART_COUNT, -1);
        assert!(matches!(
            parse_instance(&data),
            Err(Error::Malformed("negative part count"))
        ));

        assert!(matches!(
            parse_instance(&[0u8; INSTANCE_HEADER_SIZE - 1]),
            Err(Error::Malformed("shorter than its 108-byte header"))
        ));
    }
}

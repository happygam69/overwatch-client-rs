//! Reader for STU ("structured data") assets.
//!
//! STU is the format Overwatch stores gameplay data in: hero definitions,
//! loadouts, ability and weapon state graphs, settings. A file is a set of
//! typed instances; each instance's payload sits in a shared data buffer, and
//! variable-length payloads (strings, arrays) live in a separate dynamic block.
//!
//! Version 1 starts with the four bytes `STUD`. Version 2 has no magic, so the
//! absence of that marker is what selects it.
//!
//! Version 2 layout:
//!
//! ```text
//! bag<InstanceInfo>        u32 hash; u32 embedder field; i32 embedder idx; i32 size
//! bag<InlineArrayInfo>     u32 type hash; i32 count
//! bag<bag<FieldInfo>>      u32 hash; i32 size
//! i32 dynamic size, i32 dynamic offset, i32 data offset
//! ```
//!
//! A bag is `i32 count; i32 offset;` followed by `count` items at `offset`. The
//! offset is absolute, not relative to the bag.
//!
//! Each instance payload begins with an `i32` index into the field-bag array
//! (`-1` meaning empty). Field offsets are the running sum of the bag's sizes,
//! except that a field whose bag size is 0 carries its size inline as an `i32`,
//! consumed before the field body.
//!
//! Asset references are obfuscated with the header checksum and the field hash;
//! see [`deobfuscate`].

use std::fmt;

pub mod crc;

pub const MAGIC_V1: [u8; 4] = *b"STUD";
const INSTANCE_INFO_SIZE: usize = 16;

#[derive(Debug)]
pub enum Error {
    Empty,
    Malformed(&'static str),
    /// Version 1 uses a different, offset-based layout that is not handled yet.
    UnsupportedV1,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Empty => write!(f, "asset is empty"),
            Error::Malformed(m) => write!(f, "malformed STU: {m}"),
            Error::UnsupportedV1 => {
                write!(f, "this is a version 1 STU asset, which is not supported yet")
            }
        }
    }
}

impl std::error::Error for Error {}

type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstanceInfo {
    /// Type hash of the instance, matching TankLib's `[STU(hash, size)]`.
    pub hash: u32,
    /// Field hash of the field that embedded this instance, if any.
    pub embedder_field: u32,
    /// Instance index that embedded this one, or -1 when it is the root.
    pub embedder_index: i32,
    /// Length of this instance's payload in the data buffer.
    pub size: i32,
    /// Offset of this instance's payload within the data buffer.
    pub data_offset: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldInfo {
    pub hash: u32,
    /// Byte offset of the field within its instance payload.
    pub offset: usize,
    /// Byte length of the field.
    pub size: usize,
}

/// A parsed STU asset.
pub struct Stu {
    blob: Vec<u8>,
    /// CRC64 over the first 36 bytes; mixed into asset references.
    pub header_checksum: u64,
    pub instances: Vec<InstanceInfo>,
    field_bags: Vec<Vec<(u32, i32)>>,
    /// Where the instance payloads start in `blob`.
    data_start: usize,
    /// The dynamic block, where variable-length payloads live.
    dyn_start: usize,
    dyn_len: usize,
    /// Per-instance resolved field layouts, computed on first use.
    layouts: Vec<Option<Vec<FieldInfo>>>,
}

impl Stu {
    /// Parse an asset. `blob` must be the decoded contents of one STU file.
    pub fn parse(blob: Vec<u8>) -> Result<Stu> {
        if blob.len() < 4 {
            return Err(Error::Empty);
        }
        if blob[0..4] == MAGIC_V1 {
            return Err(Error::UnsupportedV1);
        }
        if blob.len() < 36 {
            return Err(Error::Malformed("shorter than its 36-byte v2 header"));
        }

        let header_checksum = crc::crc64(&blob[0..36]);

        let instance_info = parse_instance_bag(&blob, 0)?;

        // The inline-array bag at offset 8 describes arrays embedded directly in
        // instance payloads. Walking fields does not need it, but a parser that
        // understands those arrays will.
        let field_bags = parse_field_bags(&blob, 16)?;

        let dyn_size = rd_i32(&blob, 24)?;
        let dyn_offset = rd_i32(&blob, 28)?;
        let data_offset = rd_i32(&blob, 32)?;
        if dyn_size < 0 || dyn_offset < 0 || data_offset < 0 {
            return Err(Error::Malformed("negative section offset"));
        }
        let dyn_start = dyn_offset as usize;
        let dyn_len = dyn_size as usize;
        let data_start = data_offset as usize;
        if dyn_start + dyn_len > blob.len() || data_start > blob.len() {
            return Err(Error::Malformed("section runs past the end of the file"));
        }

        let mut instances = Vec::with_capacity(instance_info.len());
        let mut cursor = 0usize;
        for (hash, embedder_field, embedder_index, size) in instance_info {
            instances.push(InstanceInfo {
                hash,
                embedder_field,
                embedder_index,
                size,
                data_offset: cursor,
            });
            cursor += size.max(0) as usize;
        }

        let count = instances.len();
        Ok(Stu {
            blob,
            header_checksum,
            instances,
            field_bags,
            data_start,
            dyn_start,
            dyn_len,
            layouts: vec![None; count],
        })
    }

    /// Index of the first instance of the given type hash.
    pub fn instance(&self, type_hash: u32) -> Option<usize> {
        self.instances.iter().position(|i| i.hash == type_hash)
    }

    /// All instance indices of the given type hash.
    pub fn instances_of(&self, type_hash: u32) -> Vec<usize> {
        self.instances
            .iter()
            .enumerate()
            .filter(|(_, i)| i.hash == type_hash)
            .map(|(idx, _)| idx)
            .collect()
    }

    /// The raw payload of an instance.
    pub fn data(&self, instance: usize) -> &[u8] {
        let info = &self.instances[instance];
        let start = self.data_start + info.data_offset;
        let end = (start + info.size.max(0) as usize).min(self.blob.len());
        &self.blob[start..end]
    }

    /// The dynamic block.
    pub fn dyn_data(&self) -> &[u8] {
        &self.blob[self.dyn_start..self.dyn_start + self.dyn_len]
    }

    /// Resolved field layout for an instance, computed once.
    pub fn fields(&mut self, instance: usize) -> &[FieldInfo] {
        if self.layouts[instance].is_none() {
            self.layouts[instance] = Some(self.compute_layout(instance));
        }
        self.layouts[instance].as_deref().unwrap_or(&[])
    }

    fn compute_layout(&self, instance: usize) -> Vec<FieldInfo> {
        let payload = self.data(instance);
        if payload.len() < 4 {
            return Vec::new();
        }
        let bag_index = i32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
        if bag_index < 0 {
            return Vec::new();
        }
        let Some(bag) = self.field_bags.get(bag_index as usize) else {
            return Vec::new();
        };

        let mut out = Vec::with_capacity(bag.len());
        let mut p = 4usize;
        for (hash, declared) in bag {
            let mut size = *declared;
            if size == 0 {
                // size carried inline, consumed before the body
                if p + 4 > payload.len() {
                    break;
                }
                size = i32::from_le_bytes([payload[p], payload[p + 1], payload[p + 2], payload[p + 3]]);
                p += 4;
            }
            if size < 0 {
                break;
            }
            out.push(FieldInfo {
                hash: *hash,
                offset: p,
                size: size as usize,
            });
            p += size as usize;
        }
        out
    }

    /// Byte slice of a named field within an instance payload.
    pub fn field_bytes(&mut self, instance: usize, field_hash: u32) -> Option<(usize, usize)> {
        let fields = self.fields(instance);
        let f = fields.iter().find(|f| f.hash == field_hash)?;
        Some((f.offset, f.size))
    }

    /// Read a `teString` field.
    ///
    /// The field stores an `i32` offset into the dynamic block; there sits
    /// `i32 length`, `u32 checksum`, `i64 offset`, and the characters begin at
    /// that final offset.
    pub fn read_string(&mut self, instance: usize, field_hash: u32) -> Option<String> {
        let (offset, _size) = self.field_bytes(instance, field_hash)?;
        let payload_start = self.data_start + self.instances[instance].data_offset;
        let at = payload_start + offset;

        let dyn_off = rd_i32(&self.blob, at).ok()?;
        if dyn_off < 0 {
            return None;
        }
        let dyn_at = self.dyn_start + dyn_off as usize;
        let len = rd_i32(&self.blob, dyn_at).ok()?;
        if len < 0 {
            return None;
        }
        if len == 0 {
            return Some(String::new());
        }
        let text_at = rd_i64(&self.blob, dyn_at + 8).ok()?;
        if text_at < 0 {
            return None;
        }
        let start = text_at as usize;
        let end = start.checked_add(len as usize)?;
        let bytes = self.blob.get(start..end)?;
        Some(String::from_utf8_lossy(bytes).into_owned())
    }

    /// Read an `f32` field.
    pub fn read_f32(&mut self, instance: usize, field_hash: u32) -> Option<f32> {
        let (offset, _) = self.field_bytes(instance, field_hash)?;
        let at = self.data_start + self.instances[instance].data_offset + offset;
        let raw = rd_u32(&self.blob, at).ok()?;
        Some(f32::from_bits(raw))
    }

    /// Read a `u32` field.
    pub fn read_u32(&mut self, instance: usize, field_hash: u32) -> Option<u32> {
        let (offset, _) = self.field_bytes(instance, field_hash)?;
        let at = self.data_start + self.instances[instance].data_offset + offset;
        rd_u32(&self.blob, at).ok()
    }

    /// Read an asset reference field and undo its obfuscation.
    pub fn read_asset_ref(&mut self, instance: usize, field_hash: u32) -> Option<u64> {
        let (offset, _) = self.field_bytes(instance, field_hash)?;
        let at = self.data_start + self.instances[instance].data_offset + offset;
        let stored = rd_u64(&self.blob, at).ok()?;
        Some(deobfuscate(self.header_checksum, field_hash, stored))
    }

    /// Resolve a `teColorRGBA` field: four `f32`.
    pub fn read_rgba(&mut self, instance: usize, field_hash: u32) -> Option<[f32; 4]> {
        let (offset, _) = self.field_bytes(instance, field_hash)?;
        let at = self.data_start + self.instances[instance].data_offset + offset;
        let mut out = [0f32; 4];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = f32::from_bits(rd_u32(&self.blob, at + i * 4).ok()?);
        }
        Some(out)
    }
}

/// Undo the obfuscation applied to an asset reference.
///
/// The stored value is XORed with `field_hash | field_hash << 32` and the
/// asset's header checksum, then five byte positions are swapped. Getting this
/// wrong yields GUIDs that do not exist in the asset registry, which is how it
/// was verified.
pub fn deobfuscate(header_checksum: u64, field_hash: u32, stored: u64) -> u64 {
    if stored == 0 {
        return 0;
    }
    let mixed = (field_hash as u64) | ((field_hash as u64) << 32);
    let mut bytes = (stored ^ mixed ^ header_checksum).to_le_bytes();
    bytes.swap(0, 3);
    bytes.swap(7, 1);
    bytes.swap(2, 6);
    bytes.swap(4, 5);
    u64::from_le_bytes(bytes)
}

/// Parse a bag of instance records. Each record is 16 bytes.
fn parse_instance_bag(data: &[u8], at: usize) -> Result<Vec<(u32, u32, i32, i32)>> {
    let count = rd_i32(data, at)?;
    let offset = rd_i32(data, at + 4)?;
    if count < 0 || offset < 0 {
        return Err(Error::Malformed("negative instance bag count or offset"));
    }
    let mut out = Vec::with_capacity(count as usize);
    let mut p = offset as usize;
    for _ in 0..count {
        out.push((
            rd_u32(data, p)?,
            rd_u32(data, p + 4)?,
            rd_i32(data, p + 8)?,
            rd_i32(data, p + 12)?,
        ));
        p += INSTANCE_INFO_SIZE;
    }
    Ok(out)
}

/// Parse the field-bag array: one inner bag per entry, each `i32 count;
/// i32 offset` followed by `count` `(u32 hash, i32 size)` pairs.
fn parse_field_bags(data: &[u8], at: usize) -> Result<Vec<Vec<(u32, i32)>>> {
    let count = rd_i32(data, at)?;
    let offset = rd_i32(data, at + 4)?;
    if count < 0 || offset < 0 {
        return Err(Error::Malformed("negative field-bag count or offset"));
    }
    let mut out = Vec::with_capacity(count as usize);
    let mut bag_at = offset as usize;
    for _ in 0..count {
        let inner_count = rd_i32(data, bag_at)?;
        let inner_offset = rd_i32(data, bag_at + 4)?;
        if inner_count < 0 || inner_offset < 0 {
            return Err(Error::Malformed("negative inner field bag"));
        }
        let mut fields = Vec::with_capacity(inner_count as usize);
        let mut p = inner_offset as usize;
        for _ in 0..inner_count {
            fields.push((rd_u32(data, p)?, rd_i32(data, p + 4)?));
            p += 8;
        }
        out.push(fields);
        bag_at += 8;
    }
    Ok(out)
}

fn rd_u32(data: &[u8], at: usize) -> Result<u32> {
    let s = data.get(at..at + 4).ok_or(Error::Malformed("read past end"))?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn rd_i32(data: &[u8], at: usize) -> Result<i32> {
    Ok(rd_u32(data, at)? as i32)
}

fn rd_u64(data: &[u8], at: usize) -> Result<u64> {
    let s = data.get(at..at + 8).ok_or(Error::Malformed("read past end"))?;
    Ok(u64::from_le_bytes([
        s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7],
    ]))
}

fn rd_i64(data: &[u8], at: usize) -> Result<i64> {
    Ok(rd_u64(data, at)? as i64)
}

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

use std::collections::HashMap;
use std::fmt;

pub mod crc;
pub mod types;

pub const MAGIC_V1: [u8; 4] = *b"STUD";
const INSTANCE_INFO_SIZE: usize = 16;

#[derive(Debug)]
pub enum Error {
    Empty,
    Malformed(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Empty => write!(f, "asset is empty"),
            Error::Malformed(m) => write!(f, "malformed STU: {m}"),
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

/// One version 1 instance record (`STUInstanceRecordV1`).
///
/// A version 1 asset writes no per-instance field bags at all: the record
/// table says where each instance starts, and the class definitions say what
/// its fields are and at which offsets. Embedded instance references resolve
/// through [`Stu::instance_at_v1_offset`], keyed by `offset` as stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct V1Record {
    /// Absolute offset of the instance's `{u32 hash, u32 next}` prefix.
    pub offset: i32,
    /// Raw flags word; carried for fidelity, never read.
    pub flags: u32,
}

/// A parsed STU asset.
pub struct Stu {
    blob: Vec<u8>,
    /// CRC64 over the first 36 bytes; mixed into asset references.
    pub header_checksum: u64,
    pub v1: bool,
    pub instances: Vec<InstanceInfo>,
    field_bags: Vec<Vec<(u32, i32)>>,
    /// Where the instance payloads start in `blob`.
    data_start: usize,
    /// The dynamic block, where variable-length payloads live.
    dyn_start: usize,
    dyn_len: usize,
    /// Version 1's record table, one record per instance, in file order.
    v1_records: Vec<V1Record>,
    /// Version 1 record offset -> instance index.
    v1_by_offset: HashMap<i32, usize>,
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
            return Self::parse_v1(blob);
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
            let Some(next) = cursor.checked_add(size.max(0) as usize) else {
                return Err(Error::Malformed("instance payloads overflow address space"));
            };
            cursor = next;
        }

        let count = instances.len();
        Ok(Stu {
            blob,
            header_checksum,
            v1: false,
            instances,
            field_bags,
            data_start,
            dyn_start,
            dyn_len,
            v1_records: Vec::new(),
            v1_by_offset: HashMap::new(),
            layouts: vec![None; count],
        })
    }

    fn parse_v1(blob: Vec<u8>) -> Result<Stu> {
        if blob.len() < 8 {
            return Err(Error::Malformed("truncated v1 header"));
        }
        let count = u32::from_le_bytes([blob[4], blob[5], blob[6], blob[7]]) as usize;
        let table_end = 8usize
            .checked_add(count * 8)
            .ok_or(Error::Malformed("v1 record count overflows address space"))?;
        if table_end > blob.len() {
            return Err(Error::Malformed(
                "v1 record table runs past the end of the file",
            ));
        }

        // One record per instance, in file order, kept exactly as written:
        // instance indices are positions in this table, and instance
        // references resolve by record offset, not by index.
        let mut v1_records = Vec::with_capacity(count);
        for i in 0..count {
            let at = 8 + i * 8;
            v1_records.push(V1Record {
                offset: rd_i32(&blob, at)?,
                flags: rd_u32(&blob, at + 4)?,
            });
        }

        // Record offsets, sorted and deduplicated, give each instance its
        // extent: the payload runs to whatever record starts next.
        let mut starts: Vec<i32> = v1_records
            .iter()
            .map(|r| r.offset)
            .filter(|offset| *offset > 0)
            .collect();
        starts.sort_unstable();
        starts.dedup();

        // Later records overwrite earlier ones at the same offset, matching
        // the reference's `_instanceOffsets[record.Offset] = instance`.
        let mut v1_by_offset = HashMap::new();
        for (index, record) in v1_records.iter().enumerate() {
            if record.offset > 0 {
                v1_by_offset.insert(record.offset, index);
            }
        }

        let mut instances = Vec::with_capacity(count);
        for record in &v1_records {
            // A non-positive offset is not a place in the file any instance
            // can live; the slot still takes an index so references stay
            // stable, but it carries nothing.
            let (hash, size, data_offset) = if record.offset <= 0 {
                (0, 0, 0)
            } else {
                let start = record.offset as usize;
                let hash = if start + 4 <= blob.len() {
                    rd_u32(&blob, start)?
                } else {
                    0
                };
                let payload = record.offset as i64 + 8;
                let next = starts
                    .iter()
                    .find(|s| **s > record.offset)
                    .map(|s| *s as i64)
                    .unwrap_or(blob.len() as i64);
                let size = (next - payload).max(0) as i32;
                let data_offset = (record.offset as usize).saturating_add(8).min(blob.len());
                (hash, size, data_offset)
            };
            instances.push(InstanceInfo {
                hash,
                embedder_field: record.flags,
                embedder_index: -1,
                size,
                data_offset,
            });
        }

        let n = instances.len();
        let blob_len = blob.len();
        Ok(Stu {
            blob,
            header_checksum: 0,
            v1: true,
            instances,
            field_bags: Vec::new(),
            data_start: 0,
            dyn_start: 0,
            dyn_len: blob_len,
            v1_records,
            v1_by_offset,
            layouts: vec![None; n],
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

    /// Whether this asset uses the version 1 layout.
    pub fn is_v1(&self) -> bool {
        self.v1
    }

    /// Version 1's record for an instance, where the instance starts in the
    /// file (its 8-byte `hash, next` prefix included).
    pub fn v1_record(&self, instance: usize) -> Option<V1Record> {
        self.v1_records.get(instance).copied()
    }

    /// The instance whose version 1 record starts at `offset`, for resolving
    /// embedded references. A later record overwrites an earlier one at the
    /// same offset, matching the reference.
    pub fn instance_at_v1_offset(&self, offset: i32) -> Option<usize> {
        self.v1_by_offset.get(&offset).copied()
    }

    /// The raw payload of an instance.
    pub fn data(&self, instance: usize) -> &[u8] {
        let Some(info) = self.instances.get(instance) else {
            return &[];
        };
        let start = self
            .data_start
            .checked_add(info.data_offset)
            .unwrap_or(self.blob.len())
            .min(self.blob.len());
        let end = start
            .checked_add(info.size.max(0) as usize)
            .unwrap_or(self.blob.len())
            .min(self.blob.len());
        &self.blob[start..end]
    }

    /// The dynamic block.
    pub fn dyn_data(&self) -> &[u8] {
        &self.blob[self.dyn_start..self.dyn_start + self.dyn_len]
    }

    /// Resolved field layout for an instance, computed once.
    pub fn fields(&mut self, instance: usize) -> &[FieldInfo] {
        // Version 1 has no per-instance field bag; its fields live at the
        // class's declared offsets, which is what the typed layer reads.
        if self.v1 {
            return &[];
        }
        let Some(info) = self.instances.get(instance).copied() else {
            return &[];
        };
        // Instances and layouts stay in lockstep; a missing slot means a bad
        // index, not a layout to compute.
        if self.layouts.get(instance).is_none() {
            return &[];
        }
        let needs_layout = matches!(self.layouts.get(instance), Some(None));
        if needs_layout {
            let Some(start) = self.data_start.checked_add(info.data_offset) else {
                return &[];
            };
            let bag_index = self.bag_index_at(start);
            let layout = self.layout_at(start, bag_index);
            if let Some(slot) = self.layouts.get_mut(instance) {
                *slot = Some(layout);
            }
        }
        self.layouts
            .get(instance)
            .and_then(|slot| slot.as_deref())
            .unwrap_or(&[])
    }

    /// The field-bag index at the start of a payload, or -1 for an empty one.
    fn bag_index_at(&self, payload_start: usize) -> i32 {
        rd_i32(&self.blob, payload_start).unwrap_or(-1)
    }

    /// Field layout of a payload that begins at `payload_start` and uses the
    /// given field bag. Offsets are relative to `payload_start`.
    ///
    /// This is the one place the inline-size rule lives, and it serves both
    /// instances and the elements of an inline array, which are laid out
    /// identically.
    pub fn layout_at(&self, payload_start: usize, bag_index: i32) -> Vec<FieldInfo> {
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
                // The size is carried inline and consumed before the body, so
                // reading it advances the cursor.
                let Some(at) = payload_start.checked_add(p) else {
                    break;
                };
                match rd_i32(&self.blob, at) {
                    Ok(inline) => size = inline,
                    Err(_) => break,
                }
                let Some(next) = p.checked_add(4) else {
                    break;
                };
                p = next;
            }
            if size < 0 {
                break;
            }
            out.push(FieldInfo {
                hash: *hash,
                offset: p,
                size: size as usize,
            });
            let Some(next) = p.checked_add(size as usize) else {
                break;
            };
            p = next;
        }
        out
    }

    /// Elements of an inline instance array field.
    ///
    /// An inline array is an `i32` count followed by that many instance
    /// payloads, each starting with its own field-bag index. An element with a
    /// bag index of -1 is empty and occupies only those four bytes, which is a
    /// real case: a hero's weapon list has holes in it.
    ///
    /// `(bag_index, payload_start)` is returned per element, where
    /// `payload_start` points at the field-bag index itself, so
    /// [`Self::layout_at`] can lay the element out directly.
    pub fn inline_elements(&self, instance: usize, field_hash: u32) -> Vec<(i32, usize)> {
        let Some(field) = self
            .fields_of_instance(instance)
            .into_iter()
            .find(|f| f.hash == field_hash)
        else {
            return Vec::new();
        };
        // Inline arrays are read by value, so `size` here is not a length; the
        // count is the leading i32.
        let Some(info) = self.instances.get(instance).copied() else {
            return Vec::new();
        };
        let Some(payload) = self.data_start.checked_add(info.data_offset) else {
            return Vec::new();
        };
        let Some(base) = payload.checked_add(field.offset) else {
            return Vec::new();
        };
        let Ok(count) = rd_i32(&self.blob, base) else {
            return Vec::new();
        };
        if count <= 0 || count > 4096 {
            return Vec::new();
        }

        let mut out = Vec::with_capacity(count as usize);
        let Some(mut at) = base.checked_add(4) else {
            return Vec::new();
        };
        for _ in 0..count {
            // Elements live in the data range; a stride that walks out of the
            // file means a malformed array, not more elements.
            if at >= self.blob.len() {
                break;
            }
            let bag_index = self.bag_index_at(at);
            out.push((bag_index, at));
            // A layout starts counting at 4, past the bag index, and its final
            // offset already accounts for any inline size prefixes, so the
            // element's stride is just where the layout ends.
            let stride = self
                .layout_at(at, bag_index)
                .last()
                .map(|f| f.offset.saturating_add(f.size))
                .unwrap_or(4)
                .max(4);
            let Some(next) = at.checked_add(stride) else {
                break;
            };
            at = next;
        }
        out
    }

    fn fields_of_instance(&self, instance: usize) -> Vec<FieldInfo> {
        let Some(info) = self.instances.get(instance).copied() else {
            return Vec::new();
        };
        let Some(start) = self.data_start.checked_add(info.data_offset) else {
            return Vec::new();
        };
        let bag_index = self.bag_index_at(start);
        self.layout_at(start, bag_index)
    }

    /// Byte slice of a named field within an instance payload.
    pub fn field_bytes(&mut self, instance: usize, field_hash: u32) -> Option<(usize, usize)> {
        let fields = self.fields(instance);
        let f = fields.iter().find(|f| f.hash == field_hash)?;
        Some((f.offset, f.size))
    }

    /// Read a `u32` at an absolute offset in the asset.
    pub fn u32_at(&self, at: usize) -> Option<u32> {
        rd_u32(&self.blob, at).ok()
    }

    /// Read a `u64` at an absolute offset in the asset.
    pub fn u64_at(&self, at: usize) -> Option<u64> {
        rd_u64(&self.blob, at).ok()
    }

    /// Read an `i32` at an absolute offset in the asset.
    pub fn i32_at(&self, at: usize) -> Option<i32> {
        rd_i32(&self.blob, at).ok()
    }

    /// Read an `f32` at an absolute offset in the asset.
    pub fn f32_at(&self, at: usize) -> Option<f32> {
        rd_u32(&self.blob, at).ok().map(f32::from_bits)
    }

    /// Read an obfuscated asset reference at an absolute offset, passing the
    /// field hash that owns it.
    pub fn asset_ref_at(&self, at: usize, field_hash: u32) -> Option<u64> {
        let stored = rd_u64(&self.blob, at).ok()?;
        Some(deobfuscate(self.header_checksum, field_hash, stored))
    }

    /// Absolute offset of an instance's payload.
    pub fn payload_start(&self, instance: usize) -> usize {
        match self.instances.get(instance) {
            Some(info) => self
                .data_start
                .checked_add(info.data_offset)
                .unwrap_or(self.blob.len()),
            None => self.blob.len(),
        }
    }

    /// Read a `teString` field.
    ///
    /// The field stores an `i32` offset into the dynamic block; there sits
    /// `i32 length`, `u32 checksum`, `i64 offset`, and the characters begin at
    /// that final offset -- **within the dynamic block**, not within the file.
    /// Treating it as an absolute file offset reads whatever happens to live at
    /// that address, which is how this was found: it produced plausible-looking
    /// binary rather than nothing.
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
        let start = self.dyn_start + text_at as usize;
        let end = start.checked_add(len as usize)?;
        if end > self.dyn_start + self.dyn_len {
            return None;
        }
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

    /// Read an asset reference from an in-place payload (v2 array element or
    /// inline instance) and undo its obfuscation.
    ///
    /// `start` and `bag` are the pair [`Self::layout_at`] takes; the field's
    /// own declared width is used, as in [`Self::read_asset_ref`].
    pub fn read_asset_ref_at(&self, start: usize, bag: i32, field_hash: u32) -> Option<u64> {
        let stored = self.read_u64_at(start, bag, field_hash)?;
        Some(deobfuscate(self.header_checksum, field_hash, stored))
    }

    /// Read a raw `u64` field from an in-place payload (v2 array element or
    /// inline instance), without de-obfuscation.
    pub fn read_u64_at(&self, start: usize, bag: i32, field_hash: u32) -> Option<u64> {
        let field = self
            .layout_at(start, bag)
            .into_iter()
            .find(|f| f.hash == field_hash)?;
        let at = start.checked_add(field.offset)?;
        rd_u64(&self.blob, at).ok()
    }

    /// The instance's raw record payload, as the file carries it (reference
    /// fields still obfuscated). For probes and namer passes.
    pub fn read_raw_record(&self, instance: usize) -> Vec<u8> {
        let Some(info) = self.instances.get(instance) else {
            return Vec::new();
        };
        let Some(start) = self.data_start.checked_add(info.data_offset) else {
            return Vec::new();
        };
        let end = start.saturating_add(info.size.max(0) as usize);
        self.blob
            .get(start..end.min(self.blob.len()))
            .unwrap_or_default()
            .to_vec()
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
    let count_us = count as usize;
    let offset_us = offset as usize;
    // Validate the whole span with checked math before reserving anything, so
    // a huge count cannot pre-allocate before the bounds check fails.
    let Some(need) = count_us.checked_mul(INSTANCE_INFO_SIZE) else {
        return Err(Error::Malformed("instance bag overflows address space"));
    };
    let Some(end) = offset_us.checked_add(need) else {
        return Err(Error::Malformed("instance bag overflows address space"));
    };
    if end > data.len() {
        return Err(Error::Malformed(
            "instance bag runs past the end of the file",
        ));
    }
    let mut out = Vec::with_capacity(count_us);
    let mut p = offset_us;
    for _ in 0..count {
        out.push((
            rd_u32(data, p)?,
            rd_u32(data, p + 4)?,
            rd_i32(data, p + 8)?,
            rd_i32(data, p + 12)?,
        ));
        // `p` stays inside `end <= data.len()`, so this cannot wrap.
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
    let count_us = count as usize;
    let offset_us = offset as usize;
    // Same rule as the instance bag: prove the outer span fits before
    // reserving for it.
    let Some(outer_need) = count_us.checked_mul(8) else {
        return Err(Error::Malformed("field-bag array overflows address space"));
    };
    let Some(outer_end) = offset_us.checked_add(outer_need) else {
        return Err(Error::Malformed("field-bag array overflows address space"));
    };
    if outer_end > data.len() {
        return Err(Error::Malformed(
            "field-bag array runs past the end of the file",
        ));
    }
    let mut out = Vec::with_capacity(count_us);
    let mut bag_at = offset_us;
    for _ in 0..count {
        let inner_count = rd_i32(data, bag_at)?;
        let inner_offset = rd_i32(data, bag_at + 4)?;
        if inner_count < 0 || inner_offset < 0 {
            return Err(Error::Malformed("negative inner field bag"));
        }
        let inner_count_us = inner_count as usize;
        let inner_offset_us = inner_offset as usize;
        let Some(inner_need) = inner_count_us.checked_mul(8) else {
            return Err(Error::Malformed("inner field bag overflows address space"));
        };
        let Some(inner_end) = inner_offset_us.checked_add(inner_need) else {
            return Err(Error::Malformed("inner field bag overflows address space"));
        };
        if inner_end > data.len() {
            return Err(Error::Malformed(
                "inner field bag runs past the end of the file",
            ));
        }
        let mut fields = Vec::with_capacity(inner_count_us);
        let mut p = inner_offset_us;
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
    let s = data
        .get(at..at + 4)
        .ok_or(Error::Malformed("read past end"))?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn rd_i32(data: &[u8], at: usize) -> Result<i32> {
    Ok(rd_u32(data, at)? as i32)
}

fn rd_u64(data: &[u8], at: usize) -> Result<u64> {
    let s = data
        .get(at..at + 8)
        .ok_or(Error::Malformed("read past end"))?;
    Ok(u64::from_le_bytes([
        s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7],
    ]))
}

fn rd_i64(data: &[u8], at: usize) -> Result<i64> {
    Ok(rd_u64(data, at)? as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal v2 asset laid out by hand, with regions that do not overlap:
    ///
    /// ```text
    ///   0..36   header: three bag headers, then dyn size / off / data off
    ///  40..56   instance record (hash, embedder field, embedder idx, size)
    ///  56..80   three inner field-bag headers
    ///  80..96   the two field definitions the bags point at
    ///  96..     data buffer: bagIdx | count | elem0 | elem1 | elem1 body
    /// ```
    ///
    /// Element 0 is empty (`bagIdx = -1`, four bytes) and element 1 has one
    /// 8-byte field, which is the shape a hero's weapon list has.
    fn synthetic_asset() -> Vec<u8> {
        let mut blob = vec![0u8; 128];
        let put_i32 = |b: &mut Vec<u8>, at: usize, v: i32| {
            b[at..at + 4].copy_from_slice(&v.to_le_bytes());
        };

        // header
        put_i32(&mut blob, 0, 1); // instance bag: 1 record
        put_i32(&mut blob, 4, 40); // at 40
        put_i32(&mut blob, 16, 3); // field-bag array: 3 inner bags
        put_i32(&mut blob, 20, 56); // headers at 56
        put_i32(&mut blob, 24, 0); // dyn size
        put_i32(&mut blob, 28, 0); // dyn offset
        put_i32(&mut blob, 32, 96); // data buffer starts at 96

        // instance record
        blob[40..44].copy_from_slice(&0x1111_1111u32.to_le_bytes());
        blob[44..48].copy_from_slice(&0u32.to_le_bytes());
        put_i32(&mut blob, 48, -1);
        put_i32(&mut blob, 52, 28); // payload length

        // inner bags: bag 0 has one inline-sized field, bag 1 is empty,
        // bag 2 has one 8-byte field
        put_i32(&mut blob, 56, 1);
        put_i32(&mut blob, 60, 80);
        put_i32(&mut blob, 64, 0);
        put_i32(&mut blob, 72, 1);
        put_i32(&mut blob, 76, 88);

        blob[80..84].copy_from_slice(&0xAAAA_AAAAu32.to_le_bytes());
        put_i32(&mut blob, 84, 0); // size 0 -> read inline
        blob[88..92].copy_from_slice(&0xBBBB_BBBBu32.to_le_bytes());
        put_i32(&mut blob, 92, 8);

        // data buffer. The array field's inline size is the array's byte
        // length: 4 for the count, 4 for the empty element, 12 for the other.
        put_i32(&mut blob, 96, 0); // payload uses bag 0, the array field
        put_i32(&mut blob, 100, 20); // inline size of the array
        put_i32(&mut blob, 104, 2); // inline array of 2 elements
        put_i32(&mut blob, 108, -1); // element 0 is empty
        put_i32(&mut blob, 112, 2); // element 1 uses bag 2
        blob[116..124].copy_from_slice(&0x1234_5678_9abc_def0u64.to_le_bytes());

        blob
    }

    #[test]
    fn inline_array_walks_an_empty_element() {
        let blob = synthetic_asset();
        let stu = Stu::parse(blob).expect("parses");

        // The instance's own field is the inline array.
        let layout = stu.layout_at(96, 0);
        assert_eq!(layout.len(), 1, "bag 0 declares one field");
        assert_eq!(layout[0].hash, 0xAAAA_AAAA);
        assert_eq!(
            layout[0].offset, 8,
            "past the bag index and the inline size"
        );
        assert_eq!(layout[0].size, 20, "the array's byte length, read inline");

        let elements = stu.inline_elements(0, 0xAAAA_AAAA);
        assert_eq!(elements.len(), 2, "both elements, including the empty one");
        assert_eq!(elements[0], (-1, 108), "first element is empty");
        assert_eq!(elements[1], (2, 112), "second element uses bag 2");

        // The second element's field resolves at its own payload plus offset.
        let (bag, start) = elements[1];
        let layout = stu.layout_at(start, bag);
        assert_eq!(layout.len(), 1);
        assert_eq!(layout[0].hash, 0xBBBB_BBBB);
        assert_eq!(
            stu.u64_at(start + layout[0].offset),
            Some(0x1234_5678_9abc_def0)
        );
    }
}

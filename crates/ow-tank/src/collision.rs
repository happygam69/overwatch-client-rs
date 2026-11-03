//! The **pathing** chunk's cell container and the client's runtime cell fixup.
//!
//! The name is historical: when this module was written the 220-cell container
//! was believed to be the collision chunk. The kind table and its name decoder
//! (read 2026-11-02, `docs/formats/chunk-kinds.md`) show kind `0x12`
//! `MAP_CHUNK_PATHING`; the collision chunk is kind `0x11` and is read in
//! `ow_tank::chunkkind`.
//!
//! The map's kind-`0x12` chunk carries one `STU_9199BF6B` component instance
//! (class hash `0x9199BF6B`, size `0x78`) whose fourth field at `+0x68` is
//! `m_cellBytes` (the reference registry's name). This module reads that field
//! as the data it is: a count and a block-RVA table whose cells hold 17
//! self-relative array boundaries.
//!
//! All offsets are relative to the first STUD blob (the `STUD` magic), which
//! is the container base the client itself uses: `LoadMapAssetChunks` resolves
//! the chunk header's first RVA and `FinishMapChunkAsset` fixes the component
//! up in place. The placement evidence is in `docs/formats/maps.md`, "The
//! collision chunk payload" and "The cell container, decoded".
//!
//! The second half is behaviour, not layout: [`fixup_cell_bytes`] is the read
//! expression of `MapChunkCellBytesFixup` (`0x141ff51a0`). It turns each
//! record's 17 self-relative qwords into pointers, in place, for the runtime
//! array encoding the loader hands it (mode 1 vs mode 2/3). The client's
//! allocation branches (header mode 0, entry mode 0) are rejected here: those
//! paths allocate and then dereference the zeroed allocation, and the shipping
//! loader converts the file's entries before this pass runs. The differential
//! unit `fn/MapChunkCellBytesFixup` pins the non-allocating paths.
//!
//! The same instance's other two array-bearing fields are read by [`tables`]:
//! the nested array inside the field at `+0x08` and the array field at
//! `+0x58`. The three arrays plus the cell table tile the blob with no gap,
//! and each element stride follows from that tiling by exact division, so no
//! stride is assumed. What the elements *mean* is not read, and the record
//! conversion is not in this module.
//!
//! This is the ingest's container stage. It does **not** turn cells into
//! collision shapes: the cell-to-shape decoder has no reader in the image
//! (`docs/formats/maps.md`, "The cell path's consumer is not in the image"),
//! so it stays open (`docs/open-questions.md` item 6).

use std::fmt;

/// The collision chunk component's class hash (`STU_9199BF6B`).
pub const COMPONENT_CLASS: u32 = 0x9199_BF6B;
/// The component's `m_cellBytes` field offset.
pub const CELL_FIELD_OFFSET: usize = 0x68;
/// A record's 17 self-relative array boundaries.
pub const CELL_ARRAYS: usize = 17;
/// The first array boundary sits after the 17 qwords and the 0x18 header.
pub const CELL_FIRST_ARRAY: u64 = 0x88 + 0x18;

/// Why a collision payload could not be read as cells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The payload is shorter than the header being read.
    Truncated(&'static str),
    /// The first blob does not begin with the v1 `STUD` magic.
    BadMagic,
    /// The blob must hold exactly one instance for this component.
    InstanceCount(u32),
    /// The record's instance offset is not positive or runs past the blob.
    BadInstance(i32),
    /// The instance's class hash is not the collision component.
    WrongClass(u32),
    /// A cell offset, size or boundary leaves the blob.
    OutOfRange {
        cell: usize,
        what: &'static str,
        value: u64,
    },
    /// The 17 boundaries are not non-decreasing.
    BoundariesNotSorted { cell: usize },
    /// The first boundary does not leave room for the record header.
    FirstArrayTooLow { cell: usize, value: u64 },
    /// The cells do not tile the block list (a gap or an overlap).
    NotTiled { cell: usize },
    /// A v1 array's elements do not divide its span exactly, so the tiling
    /// that gives the element stride does not hold.
    ArrayNotTiled { field: usize, count: u32, span: u64 },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Truncated(what) => write!(f, "collision payload is truncated at {what}"),
            Error::BadMagic => write!(f, "the first blob is not a v1 STUD container"),
            Error::InstanceCount(n) => write!(f, "the first blob holds {n} instances, expected 1"),
            Error::BadInstance(off) => write!(f, "the first instance offset {off} is invalid"),
            Error::WrongClass(c) => {
                write!(
                    f,
                    "the first instance's class {c:#010x} is not {COMPONENT_CLASS:#010x}"
                )
            }
            Error::OutOfRange { cell, what, value } => {
                write!(f, "cell {cell}: {what} {value:#x} leaves the blob")
            }
            Error::BoundariesNotSorted { cell } => {
                write!(f, "cell {cell}: the 17 array boundaries are not sorted")
            }
            Error::FirstArrayTooLow { cell, value } => {
                write!(
                    f,
                    "cell {cell}: first array boundary {value:#x} < {CELL_FIRST_ARRAY:#x}"
                )
            }
            Error::NotTiled { cell } => write!(f, "cell {cell} does not tile the block list"),
            Error::ArrayNotTiled { field, count, span } => write!(
                f,
                "the array at field {field:#x} does not tile: {count} elements over {span:#x} bytes"
            ),
        }
    }
}

impl std::error::Error for Error {}

fn rd_u32(b: &[u8], at: usize) -> Result<u32, Error> {
    let bytes = b.get(at..at + 4).ok_or(Error::Truncated("a u32"))?;
    Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
}

fn rd_u64(b: &[u8], at: usize) -> Result<u64, Error> {
    let bytes = b.get(at..at + 8).ok_or(Error::Truncated("a u64"))?;
    Ok(u64::from_le_bytes(bytes.try_into().unwrap()))
}

fn rd_i32(b: &[u8], at: usize) -> Result<i32, Error> {
    let bytes = b.get(at..at + 4).ok_or(Error::Truncated("an i32"))?;
    Ok(i32::from_le_bytes(bytes.try_into().unwrap()))
}

/// One cell: its block size, its 17 boundaries and the 17 array slices.
///
/// The boundaries and the arrays are the file's form. The client converts the
/// boundaries to pointers in [`fixup_cell_bytes`] before the arrays are
/// consumed; a zero boundary means an absent array at runtime, while in this
/// payload the boundaries are non-zero and may be equal for an empty one.
#[derive(Debug)]
pub struct Cell<'a> {
    /// The cell's index in the block list.
    pub index: usize,
    /// The cell header's container-relative offset.
    pub cell_offset: u64,
    /// The record's container-relative offset.
    pub record_offset: u64,
    /// The record's extent, and the end of its 17th array.
    pub block_size: u64,
    /// The 17 self-relative array boundaries, exactly as stored.
    pub boundaries: [u64; CELL_ARRAYS],
    /// The 17 arrays, in boundary order.
    pub arrays: [&'a [u8]; CELL_ARRAYS],
}

/// The collision chunk's cell container, borrowed from the payload.
#[derive(Debug)]
pub struct Cells<'a> {
    /// The first STUD blob's offset in the payload.
    pub blob_offset: usize,
    /// The instance's offset inside the blob.
    pub instance: usize,
    /// The instance's class hash (checked against [`COMPONENT_CLASS`]).
    pub class: u32,
    /// The number of cells.
    pub count: u32,
    /// The block-list table's container-relative offset.
    pub data_offset: u64,
    /// One entry per cell.
    pub cells: Vec<Cell<'a>>,
}

/// Locate the payload's single `STUD` blob and its component instance.
///
/// Returns the blob's offset in the payload, the blob itself and the
/// instance's offset inside the blob. The checks are the client's: the v1
/// magic, exactly one record, a positive record offset, and the component's
/// class hash.
fn locate(payload: &[u8]) -> Result<(usize, &[u8], usize), Error> {
    if payload.len() < 0x20 {
        return Err(Error::Truncated("the chunk's container header"));
    }
    let blob_rva = rd_u64(payload, 0)? as usize;
    if blob_rva >= payload.len() {
        return Err(Error::OutOfRange {
            cell: 0,
            what: "the first blob RVA",
            value: blob_rva as u64,
        });
    }
    let blob = &payload[blob_rva..];
    // The client's loader compares the first dword with 0x53545544
    // (`StuLoadSubAssets`, 0x1421d26d0). On disk that is the byte sequence
    // `DUTS`; read little-endian it is the integer the docs call `STUD`.
    if blob.len() < 8 || blob[0..4] != ow_stu::MAGIC_V1 {
        return Err(Error::BadMagic);
    }
    let instances = rd_u32(blob, 4)?;
    if instances != 1 {
        return Err(Error::InstanceCount(instances));
    }
    let instance_offset = rd_i32(blob, 8)?;
    if instance_offset <= 0 || instance_offset as usize + 0x78 > blob.len() {
        return Err(Error::BadInstance(instance_offset));
    }
    let instance = instance_offset as usize;
    let class = rd_u32(blob, instance)?;
    if class != COMPONENT_CLASS {
        return Err(Error::WrongClass(class));
    }
    Ok((blob_rva, blob, instance))
}

/// Read the collision chunk payload's single component and its cells.
///
/// The payload's own container header supplies the first blob's RVA; the blob's
/// v1 record table supplies the instance offset; the component's `+0x68` field
/// supplies the cell header. Every offset is relative to the blob start.
pub fn parse(payload: &[u8]) -> Result<Cells<'_>, Error> {
    let (blob_rva, blob, instance) = locate(payload)?;

    // `m_cellBytes` is a v1 array field: a container-relative offset to a
    // `{u32 count; u32 pad; u32 data_offset}` header.
    let field = rd_u64(blob, instance + CELL_FIELD_OFFSET)? as usize;
    let count = rd_u32(blob, field)?;
    let data_offset = rd_u32(blob, field + 8)? as u64;
    let data_start = usize::try_from(data_offset).map_err(|_| Error::OutOfRange {
        cell: 0,
        what: "the block-list offset",
        value: data_offset,
    })?;
    let entries_len = (count as usize).checked_mul(16).ok_or(Error::OutOfRange {
        cell: 0,
        what: "the block-list length",
        value: count as u64,
    })?;
    let entries_end = data_start
        .checked_add(entries_len)
        .ok_or(Error::OutOfRange {
            cell: 0,
            what: "the block-list end",
            value: data_offset,
        })?;
    if entries_end > blob.len() {
        return Err(Error::OutOfRange {
            cell: 0,
            what: "the block list",
            value: entries_end as u64,
        });
    }

    let mut cells = Vec::with_capacity(count as usize);
    let mut prev_end: Option<u64> = None;
    for index in 0..count as usize {
        let entry = data_start + index * 16;
        let cell_offset = rd_u64(blob, entry)?;
        let cell_at = usize::try_from(cell_offset).map_err(|_| Error::OutOfRange {
            cell: index,
            what: "the cell offset",
            value: cell_offset,
        })?;
        let block_size = rd_u64(blob, cell_at)?;
        let record_offset = rd_u64(blob, cell_at + 8)?;
        let record_at = usize::try_from(record_offset).map_err(|_| Error::OutOfRange {
            cell: index,
            what: "the record offset",
            value: record_offset,
        })?;
        let record_end = record_offset
            .checked_add(block_size)
            .ok_or(Error::OutOfRange {
                cell: index,
                what: "the record end",
                value: record_offset,
            })?;
        if record_at + CELL_ARRAYS * 8 + 0x18 > blob.len() {
            return Err(Error::OutOfRange {
                cell: index,
                what: "the record",
                value: record_offset,
            });
        }
        if record_end > blob.len() as u64 {
            return Err(Error::OutOfRange {
                cell: index,
                what: "the record extent",
                value: record_end,
            });
        }
        if let Some(end) = prev_end {
            if end != cell_offset {
                return Err(Error::NotTiled { cell: index });
            }
        }
        prev_end = Some(record_end);

        let mut boundaries = [0u64; CELL_ARRAYS];
        for (k, slot) in boundaries.iter_mut().enumerate() {
            *slot = rd_u64(blob, record_at + k * 8)?;
        }
        if boundaries[0] < CELL_FIRST_ARRAY {
            return Err(Error::FirstArrayTooLow {
                cell: index,
                value: boundaries[0],
            });
        }
        for k in 1..CELL_ARRAYS {
            if boundaries[k] < boundaries[k - 1] {
                return Err(Error::BoundariesNotSorted { cell: index });
            }
        }
        if boundaries[CELL_ARRAYS - 1] > block_size {
            return Err(Error::OutOfRange {
                cell: index,
                what: "the last array boundary",
                value: boundaries[CELL_ARRAYS - 1],
            });
        }

        let mut arrays: [&[u8]; CELL_ARRAYS] = [&[]; CELL_ARRAYS];
        for k in 0..CELL_ARRAYS {
            let start = (record_at as u64 + boundaries[k]) as usize;
            let end_offset = if k + 1 < CELL_ARRAYS {
                boundaries[k + 1]
            } else {
                block_size
            };
            let end = (record_at as u64 + end_offset) as usize;
            arrays[k] = &blob[start..end];
        }

        cells.push(Cell {
            index,
            cell_offset,
            record_offset,
            block_size,
            boundaries,
            arrays,
        });
    }

    Ok(Cells {
        blob_offset: blob_rva,
        instance,
        class: COMPONENT_CLASS,
        count,
        data_offset,
        cells,
    })
}

/// The offset of the array field inside the 40-byte field at `+0x08`.
///
/// The reference registry names the field at `+0x08` (hash `0x81DEFC98`, 40
/// bytes inline); the loader converts the pointer it holds at `+0x10` and the
/// component's destructor (`0x140f8f310`, `CollisionChunkComponentDtor`)
/// releases exactly that sub-object, so `+0x10` is where the nested array
/// lives.
pub const NESTED_FIELD_OFFSET: usize = 0x10;
/// The array field at `+0x58` (reference hash `0x9DBA1F4E`).
pub const ARRAY_FIELD_OFFSET: usize = 0x58;

/// One `{u32 count; u32 pad; u32 data_off}` v1 array header reached from a
/// component field, with its element stride.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArrayHeader {
    /// The field's offset inside the component instance.
    pub field: usize,
    /// The header's container-relative offset.
    pub header_offset: u64,
    /// The element count.
    pub count: u32,
    /// The first element's container-relative offset.
    pub data_offset: u64,
    /// The element stride. **Derived**, not read: the array runs from
    /// [`ArrayHeader::data_offset`] to the next structure's header, and the
    /// stride is that span divided by the count. A span that does not divide
    /// exactly is rejected ([`Error::ArrayNotTiled`]), so this value is never
    /// guessed.
    pub stride: u64,
}

impl ArrayHeader {
    /// One past the last element.
    pub fn end(&self) -> u64 {
        self.data_offset + u64::from(self.count) * self.stride
    }
}

/// The collision component's three array-bearing fields.
///
/// The arrays, in payload order, tile the blob with no gap: the nested array,
/// then the field at `+0x58`, then the cell table, then the first cell. The
/// practice range's payload places them at `0x98`, `0x2358` and `0x2428`, with
/// element strides `0x28`, `0x20` and `0x10`.
///
/// What the elements mean is **not** read: no client code reads any of these
/// arrays (`docs/formats/maps.md`), so this is the container only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tables {
    /// The first `STUD` blob's offset in the payload.
    pub blob_offset: usize,
    /// The component instance's offset inside the blob.
    pub instance: usize,
    /// The array nested in the 40-byte field at `+0x08`.
    pub nested: ArrayHeader,
    /// The array field at `+0x58`.
    pub array: ArrayHeader,
    /// The `m_cellBytes` array field at `+0x68`.
    pub cell_bytes: ArrayHeader,
}

fn read_header(blob: &[u8], at: u64) -> Result<(u32, u64), Error> {
    let at = usize::try_from(at).map_err(|_| Error::OutOfRange {
        cell: 0,
        what: "an array header",
        value: at,
    })?;
    let count = rd_u32(blob, at)?;
    let data_offset = rd_u32(blob, at + 8)? as u64;
    if data_offset > blob.len() as u64 {
        return Err(Error::OutOfRange {
            cell: 0,
            what: "an array's data offset",
            value: data_offset,
        });
    }
    Ok((count, data_offset))
}

/// Derive one array's header and stride from the tiling.
fn tiled(
    blob: &[u8],
    field: usize,
    header_offset: u64,
    next_start: u64,
) -> Result<ArrayHeader, Error> {
    let (count, data_offset) = read_header(blob, header_offset)?;
    if data_offset > next_start {
        return Err(Error::OutOfRange {
            cell: 0,
            what: "an array's data offset",
            value: data_offset,
        });
    }
    let span = next_start - data_offset;
    let stride = if count == 0 {
        0
    } else {
        if span % u64::from(count) != 0 {
            return Err(Error::ArrayNotTiled { field, count, span });
        }
        span / u64::from(count)
    };
    Ok(ArrayHeader {
        field,
        header_offset,
        count,
        data_offset,
        stride,
    })
}

/// Read the component's three array fields and derive their element strides
/// from the exact tiling of the blob.
///
/// The stride derivation is the only claim this function makes beyond the
/// header form: each array's data must end exactly where the next structure
/// begins. That is checked, and a span that does not divide by the count is an
/// error rather than a rounded value. Nothing here is a read expression from
/// the client's code: the header form is shared with the verified `m_cellBytes`
/// field and the tiling is observed in the payload.
pub fn tables(payload: &[u8]) -> Result<Tables, Error> {
    let (blob_offset, blob, instance) = locate(payload)?;
    let header = |field: usize| -> Result<u64, Error> { rd_u64(blob, instance + field) };
    let nested_header = header(NESTED_FIELD_OFFSET)?;
    let array_header = header(ARRAY_FIELD_OFFSET)?;
    let cell_header = header(CELL_FIELD_OFFSET)?;

    // The cell entries are `{u64 cell_off; u64 0}`; the first entry is the
    // first cell, which is where the cell array ends.
    let (_, cell_data) = read_header(blob, cell_header)?;
    let first_cell = rd_u64(
        blob,
        usize::try_from(cell_data).map_err(|_| Error::OutOfRange {
            cell: 0,
            what: "the cell data offset",
            value: cell_data,
        })?,
    )?;

    // Each array must end exactly where the next structure's header begins.
    let nested = tiled(blob, NESTED_FIELD_OFFSET, nested_header, array_header)?;
    let array = tiled(blob, ARRAY_FIELD_OFFSET, array_header, cell_header)?;
    let cell_bytes = tiled(blob, CELL_FIELD_OFFSET, cell_header, first_cell)?;
    Ok(Tables {
        blob_offset,
        instance,
        nested,
        array,
        cell_bytes,
    })
}

/// An address-keyed view over the runtime cell block.
///
/// The client's fixup works on pointers; the unit's arena and a caller's own
/// buffer both have a base address, so the arithmetic is the same and the
/// written values are comparable.
pub struct CellMemory<'a> {
    base: u64,
    bytes: &'a mut [u8],
}

impl<'a> CellMemory<'a> {
    /// Wrap `bytes` at `base`; every pointer handed to the fixup must lie in
    /// `[base, base + bytes.len())`.
    pub fn new(base: u64, bytes: &'a mut [u8]) -> Self {
        CellMemory { base, bytes }
    }

    fn offset(&self, addr: u64, len: usize) -> Option<usize> {
        let offset = addr.checked_sub(self.base)?;
        (offset as usize)
            .checked_add(len)
            .filter(|end| *end <= self.bytes.len())
            .map(|_| offset as usize)
    }

    /// Read a little-endian u64 at an absolute address.
    pub fn read_u64(&self, addr: u64) -> Result<u64, FixupError> {
        let at = self.offset(addr, 8).ok_or(FixupError::OutOfBounds(addr))?;
        Ok(u64::from_le_bytes(
            self.bytes[at..at + 8].try_into().unwrap(),
        ))
    }

    /// Read a little-endian u32 at an absolute address.
    pub fn read_u32(&self, addr: u64) -> Result<u32, FixupError> {
        let at = self.offset(addr, 4).ok_or(FixupError::OutOfBounds(addr))?;
        Ok(u32::from_le_bytes(
            self.bytes[at..at + 4].try_into().unwrap(),
        ))
    }

    /// Write a little-endian u64 at an absolute address.
    pub fn write_u64(&mut self, addr: u64, value: u64) -> Result<(), FixupError> {
        let at = self.offset(addr, 8).ok_or(FixupError::OutOfBounds(addr))?;
        self.bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
        Ok(())
    }
}

/// Why the runtime fixup could not run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixupError {
    /// A pointer the client would dereference is outside the supplied block.
    OutOfBounds(u64),
    /// The runtime encoding's mode-0 path allocates a 0x10 object before it
    /// reads it; the client's shipping loader converts the file's entries, so
    /// this path is not exercised and is not modelled.
    AllocationRequired(u64),
}

impl fmt::Display for FixupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FixupError::OutOfBounds(addr) => write!(f, "cell pointer {addr:#x} is out of bounds"),
            FixupError::AllocationRequired(addr) => {
                write!(f, "cell array at {addr:#x} needs the client's allocator")
            }
        }
    }
}

impl std::error::Error for FixupError {}

/// `MapChunkCellBytesFixup` (`0x141ff51a0`): convert each record's 17
/// self-relative qwords to pointers, in place.
///
/// `header` points at a `{*mut u64, u64}` pair. The second qword selects the
/// runtime array encoding:
///
/// * `1`: the first qword is the entry table and the count is at `header + 8`;
/// * any other value: the count is at `header` and the entry table pointer is
///   at `header + 8`.
///
/// Each 0x10-byte entry is a `{u64, u64}` pair. Entry mode `1` stores the
/// record pointer in the first qword; another mode stores it at `+8` of the
/// object the first qword points at. Every record qword `q` becomes
/// `record + q`, or stays `0` when `q` is zero.
///
/// The loop index is a signed 32-bit counter compared with a signed 32-bit
/// count, exactly as the client's `CMP ESI, [RCX]` / `JGE` does.
pub fn fixup_cell_bytes(mem: &mut CellMemory<'_>, header: u64) -> Result<(), FixupError> {
    let mut index: i32 = 0;
    let mut entry_offset: u64 = 0;
    loop {
        let header_ptr = mem.read_u64(header)?;
        let header_mode = mem.read_u64(header + 8)?;
        let count_ptr = if header_mode == 1 {
            header_ptr + 8
        } else {
            header_ptr
        };
        if index >= mem.read_u32(count_ptr)? as i32 {
            return Ok(());
        }
        if header_mode == 0 {
            return Err(FixupError::AllocationRequired(header));
        }
        let entries_at = if header_mode != 1 {
            header_ptr + 8
        } else {
            header_ptr
        };
        let entries = mem.read_u64(entries_at)?;
        let entry = entries.wrapping_add(entry_offset);
        let entry_mode = mem.read_u64(entry + 8)?;
        if entry_mode == 0 {
            return Err(FixupError::AllocationRequired(entry + 8));
        }
        let record_slot = if entry_mode == 1 {
            mem.read_u64(entry)?
        } else {
            mem.read_u64(entry)?.wrapping_add(8)
        };
        let record = mem.read_u64(record_slot)?;
        for k in 0..CELL_ARRAYS {
            let at = record.wrapping_add((k as u64) * 8);
            let value = mem.read_u64(at)?;
            let converted = if value == 0 {
                0
            } else {
                record.wrapping_add(value)
            };
            mem.write_u64(at, converted)?;
        }
        index = index.wrapping_add(1);
        entry_offset = entry_offset.wrapping_add(0x10);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: u64 = 0x1000;

    fn put_u64(b: &mut [u8], at: u64, v: u64) {
        let at = (at - BASE) as usize;
        b[at..at + 8].copy_from_slice(&v.to_le_bytes());
    }

    fn get_u64(b: &[u8], at: u64) -> u64 {
        let at = (at - BASE) as usize;
        u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
    }

    fn put_u32(b: &mut [u8], at: u64, v: u32) {
        let at = (at - BASE) as usize;
        b[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }

    /// The outer pair is `{inner header, mode}`; mode 1 puts the count at
    /// `inner + 8`. Entry mode 1 stores the record pointer in the entry's
    /// first qword.
    fn mode_one_layout(block: &mut [u8]) -> (u64, u64, u64) {
        let header = BASE;
        let inner = BASE + 0x10;
        let entries = BASE + 0x30;
        put_u64(block, header, inner);
        put_u64(block, header + 8, 1);
        put_u64(block, inner, entries);
        (header, inner, entries)
    }

    /// Header mode 1, entry mode 1, two records with zero and non-zero qwords.
    #[test]
    fn converts_mode_one_records_in_place() {
        let mut block = vec![0u8; 0x400];
        let (header, inner, entries) = mode_one_layout(&mut block);
        let slots = BASE + 0x100;
        let records = BASE + 0x200;
        put_u32(&mut block, inner + 8, 2);
        for i in 0..2u64 {
            put_u64(&mut block, entries + i * 0x10, slots + i * 0x20);
            put_u64(&mut block, entries + i * 0x10 + 8, 1);
            put_u64(&mut block, slots + i * 0x20, records + i * 0x100);
        }
        for k in 0..CELL_ARRAYS {
            let v = if k % 3 == 0 { 0 } else { 0x18 + k as u64 };
            put_u64(&mut block, records + k as u64 * 8, v);
            put_u64(&mut block, records + 0x100 + k as u64 * 8, v);
        }

        let mut mem = CellMemory::new(BASE, &mut block);
        fixup_cell_bytes(&mut mem, header).unwrap();

        for i in 0..2u64 {
            let record = records + i * 0x100;
            for k in 0..CELL_ARRAYS {
                let v = if k % 3 == 0 { 0 } else { 0x18 + k as u64 };
                let want = if v == 0 { 0 } else { record + v };
                assert_eq!(get_u64(&block, record + k as u64 * 8), want);
            }
        }
    }

    /// Outer mode 2 (count at `inner + 0`, entries at `inner + 8`) and entry
    /// mode 2 (the record pointer is at `+8` of the object the first qword
    /// names).
    #[test]
    fn converts_header_and_entry_mode_two() {
        let mut block = vec![0u8; 0x300];
        let header = BASE;
        let inner = BASE + 0x10;
        let entries = BASE + 0x30;
        let slot = BASE + 0x100;
        let record = BASE + 0x200;
        put_u64(&mut block, header, inner);
        put_u64(&mut block, header + 8, 2);
        put_u32(&mut block, inner, 1);
        put_u64(&mut block, inner + 8, entries);
        put_u64(&mut block, entries, slot);
        put_u64(&mut block, entries + 8, 2);
        put_u64(&mut block, slot, 0xDEAD_BEEF);
        put_u64(&mut block, slot + 8, record);
        for k in 0..CELL_ARRAYS {
            put_u64(&mut block, record + k as u64 * 8, 8 * (k as u64 + 1));
        }

        let mut mem = CellMemory::new(BASE, &mut block);
        fixup_cell_bytes(&mut mem, header).unwrap();

        for k in 0..CELL_ARRAYS {
            let v = 8 * (k as u64 + 1);
            assert_eq!(get_u64(&block, record + k as u64 * 8), record + v);
        }
    }

    /// A zero count exits before any entry is read.
    #[test]
    fn empty_container_is_a_no_op() {
        let mut block = vec![0u8; 0x40];
        let (header, inner, _) = mode_one_layout(&mut block);
        put_u32(&mut block, inner + 8, 0);
        let before = block.clone();
        let mut mem = CellMemory::new(BASE, &mut block);
        fixup_cell_bytes(&mut mem, header).unwrap();
        assert_eq!(block, before);
    }

    /// The allocation modes are rejected, not guessed at. A positive count is
    /// needed: the loop's count test runs before the mode test.
    #[test]
    fn mode_zero_requires_the_allocator() {
        let mut block = vec![0u8; 0x20];
        let header = BASE;
        let inner = BASE + 0x10;
        put_u64(&mut block, header, inner);
        put_u64(&mut block, header + 8, 0);
        put_u32(&mut block, inner, 1);
        let mut mem = CellMemory::new(BASE, &mut block);
        assert_eq!(
            fixup_cell_bytes(&mut mem, header),
            Err(FixupError::AllocationRequired(header))
        );
    }

    /// A synthetic two-cell payload with the same layout as the install's
    /// (container header, v1 blob, component, cell table, records).
    fn synthetic_payload() -> Vec<u8> {
        const BLOB: usize = 0x20;
        let mut p = vec![0u8; 0x3E0];
        p[0..8].copy_from_slice(&(BLOB as u64).to_le_bytes());
        p[16..24].copy_from_slice(&1u64.to_le_bytes());
        let blob = &mut p[BLOB..];
        blob[0..4].copy_from_slice(&ow_stu::MAGIC_V1);
        blob[4..8].copy_from_slice(&1u32.to_le_bytes());
        blob[8..12].copy_from_slice(&0x20i32.to_le_bytes());
        let inst = 0x20;
        blob[inst..inst + 4].copy_from_slice(&COMPONENT_CLASS.to_le_bytes());
        blob[inst + CELL_FIELD_OFFSET..inst + CELL_FIELD_OFFSET + 8]
            .copy_from_slice(&0x100u64.to_le_bytes());
        // Cell header: {u32 count; u32 pad; u32 data_off}.
        blob[0x100..0x104].copy_from_slice(&2u32.to_le_bytes());
        blob[0x108..0x10C].copy_from_slice(&0x110u32.to_le_bytes());
        // Two entries and two cells; the first record ends at the second cell.
        for (i, cell_off) in [0x130u64, 0x270u64].iter().enumerate() {
            let at = 0x110 + i * 16;
            blob[at..at + 8].copy_from_slice(&cell_off.to_le_bytes());
        }
        for (cell_off, record_off) in [(0x130u64, 0x140u64), (0x270, 0x280)] {
            let at = cell_off as usize;
            blob[at..at + 8].copy_from_slice(&0x130u64.to_le_bytes());
            blob[at + 8..at + 16].copy_from_slice(&record_off.to_le_bytes());
            let rec = record_off as usize;
            for k in 0..CELL_ARRAYS {
                let v = CELL_FIRST_ARRAY + (k as u64) * 8;
                blob[rec + k * 8..rec + k * 8 + 8].copy_from_slice(&v.to_le_bytes());
            }
        }
        p
    }

    #[test]
    fn parses_a_synthetic_two_cell_container() {
        let payload = synthetic_payload();
        let cells = parse(&payload).expect("synthetic container parses");
        assert_eq!(cells.count, 2);
        assert_eq!(cells.class, COMPONENT_CLASS);
        assert_eq!(cells.data_offset, 0x110);
        assert_eq!(cells.cells.len(), 2);
        let first = &cells.cells[0];
        assert_eq!(first.cell_offset, 0x130);
        assert_eq!(first.record_offset, 0x140);
        assert_eq!(first.block_size, 0x130);
        assert_eq!(first.boundaries[0], CELL_FIRST_ARRAY);
        assert_eq!(first.boundaries[16], CELL_FIRST_ARRAY + 16 * 8);
        for k in 0..CELL_ARRAYS {
            let end = if k + 1 < CELL_ARRAYS {
                first.boundaries[k + 1]
            } else {
                first.block_size
            };
            assert_eq!(first.arrays[k].len() as u64, end - first.boundaries[k]);
        }
        assert_eq!(cells.cells[1].cell_offset, 0x270);
    }

    #[test]
    fn rejects_a_wrong_magic_and_broken_tiling() {
        let mut p = synthetic_payload();
        p[0x20] ^= 0xFF;
        assert!(matches!(parse(&p), Err(Error::BadMagic)));

        let mut p = synthetic_payload();
        // Make the first record one byte too long for the second cell.
        p[0x20 + 0x130..0x20 + 0x138].copy_from_slice(&0x131u64.to_le_bytes());
        assert!(matches!(parse(&p), Err(Error::NotTiled { cell: 1 })));
    }

    /// A pointer outside the block is reported instead of read.
    #[test]
    fn out_of_bounds_pointer_is_rejected() {
        let mut block = vec![0u8; 0x40];
        let (header, inner, _) = mode_one_layout(&mut block);
        put_u32(&mut block, inner + 8, 1);
        put_u64(&mut block, inner, BASE + 0x1000);
        let mut mem = CellMemory::new(BASE, &mut block);
        assert_eq!(
            fixup_cell_bytes(&mut mem, header),
            Err(FixupError::OutOfBounds(BASE + 0x1008))
        );
    }

    /// A payload whose three arrays tile the blob: the nested array at `0x98`
    /// with two `0x28` elements, the `+0x58` array at `0x160` with two `0x20`
    /// elements, the cell table at `0x1B0` with three `0x10` elements, and the
    /// first cell at `0x1F0`.
    fn synthetic_tables_payload() -> Vec<u8> {
        const BLOB: usize = 0x20;
        let mut p = vec![0u8; BLOB + 0x200];
        p[0..8].copy_from_slice(&(BLOB as u64).to_le_bytes());
        let blob = &mut p[BLOB..];
        blob[0..4].copy_from_slice(&ow_stu::MAGIC_V1);
        blob[4..8].copy_from_slice(&1u32.to_le_bytes());
        blob[8..12].copy_from_slice(&0x20i32.to_le_bytes());
        let inst = 0x20;
        blob[inst..inst + 4].copy_from_slice(&COMPONENT_CLASS.to_le_bytes());
        // Each field holds the container-relative offset of its header.
        for (field, header) in [
            (NESTED_FIELD_OFFSET, 0x100u64),
            (ARRAY_FIELD_OFFSET, 0x160),
            (CELL_FIELD_OFFSET, 0x1B0),
        ] {
            blob[inst + field..inst + field + 8].copy_from_slice(&header.to_le_bytes());
        }
        for (header, count, data) in [
            (0x100usize, 2u32, 0x110u32),
            (0x160, 2, 0x170),
            (0x1B0, 3, 0x1C0),
        ] {
            blob[header..header + 4].copy_from_slice(&count.to_le_bytes());
            blob[header + 8..header + 12].copy_from_slice(&data.to_le_bytes());
        }
        // The cell table's first entry is the first cell's offset.
        blob[0x1C0..0x1C8].copy_from_slice(&0x1F0u64.to_le_bytes());
        p
    }

    #[test]
    fn tables_derive_their_strides_from_the_tiling() {
        let payload = synthetic_tables_payload();
        let t = tables(&payload).expect("the synthetic tables tile");
        assert_eq!(t.blob_offset, 0x20);
        assert_eq!(t.instance, 0x20);
        assert_eq!(
            t.nested,
            ArrayHeader {
                field: NESTED_FIELD_OFFSET,
                header_offset: 0x100,
                count: 2,
                data_offset: 0x110,
                stride: 0x28,
            }
        );
        assert_eq!(
            t.array,
            ArrayHeader {
                field: ARRAY_FIELD_OFFSET,
                header_offset: 0x160,
                count: 2,
                data_offset: 0x170,
                stride: 0x20,
            }
        );
        assert_eq!(
            t.cell_bytes,
            ArrayHeader {
                field: CELL_FIELD_OFFSET,
                header_offset: 0x1B0,
                count: 3,
                data_offset: 0x1C0,
                stride: 0x10,
            }
        );
        assert_eq!(t.nested.end(), 0x160);
        assert_eq!(t.array.end(), 0x1B0);
        assert_eq!(t.cell_bytes.end(), 0x1F0, "the first cell");
    }

    /// A span that does not divide by the count is refused: the tiling is the
    /// only thing that gives the stride, so there is nothing to fall back on.
    #[test]
    fn tables_refuse_a_span_that_does_not_divide() {
        let mut payload = synthetic_tables_payload();
        // Three elements over the same `0x50` span: 0x50 / 3 is not integral.
        payload[0x20 + 0x100..0x20 + 0x104].copy_from_slice(&3u32.to_le_bytes());
        assert_eq!(
            tables(&payload),
            Err(Error::ArrayNotTiled {
                field: NESTED_FIELD_OFFSET,
                count: 3,
                span: 0x50,
            })
        );
    }
}

//! The client's chunk-kind table, and the collision chunk's payload.
//!
//! Two questions, one file, because the second answer is only trustworthy when
//! the first one is settled.
//!
//! # The kind table
//!
//! The client keeps one 0x24-entry table of map kinds at `0x1439aa320`, stride
//! `0x20`. Each entry is `{u64 slot; u64 id; u32 a; u32 b; char *name}`, with
//! the id at `+8` being the byte the asset GUID carries in bits 32..39
//! (`kind = (guid >> 32) & 0xFF`). Slots `0..=0x1C` are placeable kinds and
//! slots `0x1D..=0x23` are chunk kinds.
//!
//! `MapChunkGuid` (`0x141b32e20`) reads the id, and `ChunkSlotForGuid`
//! (`0x141b32e70`) searches the table for a GUID's id and returns the slot, so
//! a chunk's declared kind resolves to its slot and from there to its name.
//!
//! The names are obfuscated. The routine at `0x1404c19b0` deobfuscates all
//! 0x24 of them once, in table order: for each entry it takes the name pointer
//! from `+0x18`, reads the length from the dword at `pointer - 4` (low 24 bits;
//! the top byte is a "still encoded" flag the routine clears), and XORs name
//! byte `i` with byte `i & 7` of the eight bytes at `pointer - 0xC`. Nothing
//! else is mixed in: no length-dependent key, no rotation. The decoded strings
//! are [`TABLE`]'s names, and eight of them (`MAP_PLACEABLE_MODEL_GROUP`,
//! `_ENTITY`, `_LIGHT`, `_SOUND`, `_EFFECT`, `_TEXT`, `_REFLECTIONPOINT`,
//! `_AREA`) match the placeable names this project had already read from the
//! install by other means, which is the check that the decode is right.
//!
//! **Slot `0x1D` is kind `0x11` = `MAP_CHUNK_COLLISION`, and slot `0x1E` is
//! kind `0x12` = `MAP_CHUNK_PATHING`.** The two additive kinds are slot `0x21`
//! (kind `0x10`, `teMAP_CHUNK_ADDITIVE_COLLISION`) and slot `0x22` (kind
//! `0x23`, `teMAP_CHUNK_ADDITIVE_PATHING`).
//!
//! The same split shows up in the dispatch: `FinishMapChunkAsset`
//! (`0x141ff54e0`) sends slot `0x1D` **and** slot `0x21` to `FUN_142e492a0`
//! and slots `0x1E`/`0x22` to the `StuLoadSubAssets` + `MapChunkCellBytesFixup`
//! cell path. So the two kinds the client names "collision" are exactly the two
//! it fixes up with the same function, and the two it names "pathing" are
//! exactly the two whose payload is a cell container.
//!
//! # The collision payload
//!
//! A collision chunk's payload is not a `STUD` container: the dispatch fixes it
//! up in place and never asks the sub-asset loader for it. `FUN_142e492a0`
//! reads this header:
//!
//! ```text
//! +0x00  u64  not touched by the fixup
//! +0x08  i32  count A      -> array A at +0x20, stride 0x58
//! +0x0C  i32  count B      -> array B at +0x28, stride 0x38
//! +0x10  u64  RVA -> pointer (zero stays zero)
//! +0x18  u64  RVA -> pointer (zero stays zero)
//! +0x20  u64  RVA -> pointer (zero stays zero), array A
//! +0x28  u64  RVA -> pointer (zero stays zero), array B
//! +0x30  u64  not touched by the fixup
//! ```
//!
//! and then adds the payload base to four qwords of every `0x58` element
//! (`+0x00`, `+0x08`, `+0x10`, `+0x18`) and to three qwords of every `0x38`
//! element (`+0x00`, `+0x08`, `+0x10`). Those two element passes are
//! unconditional, unlike the header's: an element field of zero becomes the
//! payload base, it does not stay null. [`fixup_in_place`] is that expression,
//! operation for operation.
//!
//! Nothing in the image was read that says what the sectors and the arrays
//! *mean*, so this module stops where the read stops: it exposes the header,
//! both arrays, and the sector element's four pointers. The extra fact it does
//! assert is a tiling, measured over the practice range's chunk and required of
//! every payload it parses: element A's four pointers, taken together with the
//! three counts at `+0x2C`/`+0x30`/`+0x34`, describe four adjacent runs that
//! exactly fill the gap between that element and the next one, in the fixed
//! order `+0x00`, `+0x18`, `+0x08`, `+0x10`. That is a property of the bytes,
//! checked by [`Collision::sectors`]' callers through [`Sector::runs`]; the
//! *meaning* of the three counts is not read.
//!
//! The consumer that turns a sector into collision shapes is not in this
//! module and is not read (`docs/formats/chunk-kinds.md`).
//!
//! # The pathing chunk's header
//!
//! The pathing kinds take a different relocator: the client's dispatch
//! (`FinishMapChunkAsset`, `0x141ff54e0`) sends slot `0x1E` to
//! `MapChunkHeaderFixup` (`0x142e49360`), which converts three
//! self-relative RVAs of the header record - at `+0x00`, `+0x08` and `+0x18` -
//! to pointers and leaves a stored zero as zero. The record's `+0x00` is then
//! handed to the STUD loader, and the cell fixup runs on the blob it returns.
//! [`fixup_header_in_place`] is that expression, operation for operation.
//!
//! The sibling relocators the same dispatch uses - `0x142e49270` (two
//! pointers, the additive pathing kind), `0x142e493a0` (`+0x2E8`/`+0x2F0`) and
//! `0x142e493e0` (`+0x08`, the sound area) - are **not** implemented here.

use std::fmt;

/// The client's kind table: `0x1439aa320`, 0x24 entries of 0x20 bytes.
pub const KIND_TABLE_ADDRESS: u64 = 0x1_439a_a320;
/// The routine that deobfuscates the table's 0x24 names (`0x1404c19b0`).
pub const KIND_NAMES_DECODER: u64 = 0x1_404c_19b0;
/// The chunk GUID prefix and the map index's mask (`MapChunkGuid`).
pub const CHUNK_GUID_PREFIX: u64 = 0x0DD0_0000_0000_0000;

/// `MAP_CHUNK_COLLISION`, slot `0x1D`.
pub const KIND_COLLISION: u8 = 0x11;
/// `MAP_CHUNK_PATHING`, slot `0x1E`.
pub const KIND_PATHING: u8 = 0x12;
/// `MAP_CHUNK_SOUNDAREA`, slot `0x1F`.
pub const KIND_SOUNDAREA: u8 = 0x13;
/// `MAP_CHUNK_DYNAMIC_SPAWN_GROUPS`, slot `0x20`.
pub const KIND_DYNAMIC_SPAWN_GROUPS: u8 = 0x1B;
/// `teMAP_CHUNK_ADDITIVE_COLLISION`, slot `0x21`.
pub const KIND_ADDITIVE_COLLISION: u8 = 0x10;
/// `teMAP_CHUNK_ADDITIVE_PATHING`, slot `0x22`.
pub const KIND_ADDITIVE_PATHING: u8 = 0x23;
/// `teMAP_CHUNK_STREAMING`, slot `0x23`.
pub const KIND_STREAMING: u8 = 0x27;

/// One entry of the kind table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// The table slot (the entry's first qword, and the index the map loader
    /// uses).
    pub slot: u8,
    /// The kind id (the entry's `+0x08`): the byte at bits 32..39 of a chunk
    /// GUID, and the value `teMAP_PLACEABLE_TYPE`/`teMAP_CHUNK_TYPE` carries.
    pub id: u8,
    /// The dword at `+0x10`. Raw: no reader of it was found in the image.
    pub raw_10: u32,
    /// The dword at `+0x14`. Raw, as above.
    pub raw_14: u32,
    /// The decoded name from the entry's `+0x18` blob.
    pub name: &'static str,
}

/// The 0x24 entries of `0x1439aa320`, in slot order.
///
/// Slots `0..=0x1C` are placeable kinds, `0x1D..=0x23` chunk kinds. The ids are
/// what the asset GUID carries; the names are the deobfuscated strings.
pub const TABLE: [Entry; 0x24] = [
    Entry {
        slot: 0x00,
        id: 0x00,
        raw_10: 0,
        raw_14: 0,
        name: "MAP_PLACEABLE_UNKNOWN",
    },
    Entry {
        slot: 0x01,
        id: 0x01,
        raw_10: 0,
        raw_14: 3,
        name: "MAP_PLACEABLE_MODEL_GROUP",
    },
    Entry {
        slot: 0x02,
        id: 0x03,
        raw_10: 0,
        raw_14: 3,
        name: "MAP_PLACEABLE_OCCLUDER",
    },
    Entry {
        slot: 0x03,
        id: 0x04,
        raw_10: 0,
        raw_14: 3,
        name: "MAP_PLACEABLE_REFLECTIONPOINT",
    },
    Entry {
        slot: 0x04,
        id: 0x06,
        raw_10: 0,
        raw_14: 3,
        name: "MAP_PLACEABLE_LABEL",
    },
    Entry {
        slot: 0x05,
        id: 0x07,
        raw_10: 0,
        raw_14: 3,
        name: "MAP_PLACEABLE_TEXT",
    },
    Entry {
        slot: 0x06,
        id: 0x09,
        raw_10: 1,
        raw_14: 3,
        name: "MAP_PLACEABLE_LIGHT",
    },
    Entry {
        slot: 0x07,
        id: 0x0A,
        raw_10: 1,
        raw_14: 3,
        name: "MAP_PLACEABLE_AREA",
    },
    Entry {
        slot: 0x08,
        id: 0x0B,
        raw_10: 1,
        raw_14: 3,
        name: "MAP_PLACEABLE_ENTITY",
    },
    Entry {
        slot: 0x09,
        id: 0x0C,
        raw_10: 1,
        raw_14: 3,
        name: "MAP_PLACEABLE_SOUND",
    },
    Entry {
        slot: 0x0A,
        id: 0x0D,
        raw_10: 1,
        raw_14: 3,
        name: "MAP_PLACEABLE_EFFECT",
    },
    Entry {
        slot: 0x0B,
        id: 0x0E,
        raw_10: 1,
        raw_14: 3,
        name: "MAP_PLACEABLE_FOG",
    },
    Entry {
        slot: 0x0C,
        id: 0x0F,
        raw_10: 1,
        raw_14: 3,
        name: "MAP_PLACEABLE_INDOORBOX",
    },
    Entry {
        slot: 0x0D,
        id: 0x15,
        raw_10: 0,
        raw_14: 3,
        name: "MAP_PLACEABLE_TYPE_MAP_REGION",
    },
    Entry {
        slot: 0x0E,
        id: 0x16,
        raw_10: 1,
        raw_14: 3,
        name: "MAP_PLACEABLE_TYPE_SPAWN_GROUP",
    },
    Entry {
        slot: 0x0F,
        id: 0x17,
        raw_10: 1,
        raw_14: 3,
        name: "teMAP_PLACEABLE_TYPE_TRIGGER_VOLUME",
    },
    Entry {
        slot: 0x10,
        id: 0x18,
        raw_10: 1,
        raw_14: 3,
        name: "MAP_PLACEABLE_SEQUENCE",
    },
    Entry {
        slot: 0x11,
        id: 0x19,
        raw_10: 1,
        raw_14: 3,
        name: "MAP_PLACEABLE_SPAWN_VOLUME",
    },
    Entry {
        slot: 0x12,
        id: 0x1A,
        raw_10: 0,
        raw_14: 3,
        name: "MAP_PLACEABLE_LENSFLARE",
    },
    Entry {
        slot: 0x13,
        id: 0x1C,
        raw_10: 1,
        raw_14: 3,
        name: "MAP_PLACEABLE_GLOBAL_SHADOW_REGION",
    },
    Entry {
        slot: 0x14,
        id: 0x1D,
        raw_10: 1,
        raw_14: 3,
        name: "MAP_PLACEABLE_TYPE_EFFECTVOLUME",
    },
    Entry {
        slot: 0x15,
        id: 0x1E,
        raw_10: 1,
        raw_14: 3,
        name: "MAP_PLACEABLE_TYPE_DYNAMIC_SPAWN_POINT",
    },
    Entry {
        slot: 0x16,
        id: 0x20,
        raw_10: 1,
        raw_14: 3,
        name: "MAP_PLACEABLE_TYPE_AI_HINT_VOLUME",
    },
    Entry {
        slot: 0x17,
        id: 0x21,
        raw_10: 1,
        raw_14: 3,
        name: "MAP_PLACEABLE_TYPE_CLOWN_CAR",
    },
    Entry {
        slot: 0x18,
        id: 0x22,
        raw_10: 1,
        raw_14: 3,
        name: "MAP_PLACEABLE_TYPE_WATER",
    },
    Entry {
        slot: 0x19,
        id: 0x24,
        raw_10: 1,
        raw_14: 3,
        name: "MAP_PLACEABLE_DEPLOYMENT_ZONE_VOLUME",
    },
    Entry {
        slot: 0x1A,
        id: 0x25,
        raw_10: 1,
        raw_14: 3,
        name: "teMAP_PLACEABLE_TYPE_FRACTURABLE_CHILD_BREAKABLE",
    },
    Entry {
        slot: 0x1B,
        id: 0x26,
        raw_10: 0,
        raw_14: 3,
        name: "teMAP_PLACEABLE_TYPE_SH_PROXY_VOLUME",
    },
    Entry {
        slot: 0x1C,
        id: 0x28,
        raw_10: 0,
        raw_14: 3,
        name: "teMAP_PLACEABLE_TYPE_DIFFUSE_GI_VOLUME",
    },
    Entry {
        slot: 0x1D,
        id: 0x11,
        raw_10: 2,
        raw_14: 1,
        name: "MAP_CHUNK_COLLISION",
    },
    Entry {
        slot: 0x1E,
        id: 0x12,
        raw_10: 2,
        raw_14: 1,
        name: "MAP_CHUNK_PATHING",
    },
    Entry {
        slot: 0x1F,
        id: 0x13,
        raw_10: 2,
        raw_14: 3,
        name: "MAP_CHUNK_SOUNDAREA",
    },
    Entry {
        slot: 0x20,
        id: 0x1B,
        raw_10: 2,
        raw_14: 3,
        name: "MAP_CHUNK_DYNAMIC_SPAWN_GROUPS",
    },
    Entry {
        slot: 0x21,
        id: 0x10,
        raw_10: 2,
        raw_14: 0,
        name: "teMAP_CHUNK_ADDITIVE_COLLISION",
    },
    Entry {
        slot: 0x22,
        id: 0x23,
        raw_10: 2,
        raw_14: 2,
        name: "teMAP_CHUNK_ADDITIVE_PATHING",
    },
    Entry {
        slot: 0x23,
        id: 0x27,
        raw_10: 2,
        raw_14: 3,
        name: "teMAP_CHUNK_STREAMING",
    },
];

/// The table entry whose id is `kind`, if any (what `ChunkSlotForGuid` does).
pub fn entry_for_kind(kind: u8) -> Option<&'static Entry> {
    TABLE.iter().find(|e| e.id == kind)
}

/// The table slot whose id is `kind`, if any.
pub fn slot_for_kind(kind: u8) -> Option<u8> {
    entry_for_kind(kind).map(|e| e.slot)
}

/// The kind a chunk GUID declares: bits 32..39.
pub fn kind_of_guid(guid: u64) -> u8 {
    ((guid >> 32) & 0xFF) as u8
}

/// The client's name for a chunk GUID's kind, if the kind is in the table.
pub fn kind_name(guid: u64) -> Option<&'static str> {
    entry_for_kind(kind_of_guid(guid)).map(|e| e.name)
}

/// The collision chunk's sector element (`0x58` bytes) and tail element
/// (`0x38` bytes).
pub const SECTOR_SIZE: usize = 0x58;
/// Element size of the array at header `+0x28`.
pub const TAIL_SIZE: usize = 0x38;
/// The header `FUN_142e492a0` reads before touching either array.
pub const HEADER_SIZE: usize = 0x38;
/// Qwords of a sector element the fixup converts.
pub const SECTOR_POINTERS: usize = 4;
/// Qwords of a tail element the fixup converts.
pub const TAIL_POINTERS: usize = 3;

/// Why a collision payload could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The payload is shorter than the structure being read.
    Truncated(&'static str),
    /// A count is negative, which the client's signed loop compares against.
    NegativeCount { field: usize, value: i32 },
    /// An RVA leaves the payload.
    OutOfRange { what: &'static str, value: u64 },
    /// A sector's four runs do not tile the span up to the next sector.
    SectorNotTiled { sector: usize },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Truncated(what) => write!(f, "collision payload is truncated at {what}"),
            Error::NegativeCount { field, value } => {
                write!(f, "collision payload's count at {field:#x} is {value}")
            }
            Error::OutOfRange { what, value } => {
                write!(
                    f,
                    "collision payload's {what} {value:#x} leaves the payload"
                )
            }
            Error::SectorNotTiled { sector } => {
                write!(f, "collision sector {sector}'s four runs do not tile")
            }
        }
    }
}

impl std::error::Error for Error {}

type Result<T> = std::result::Result<T, Error>;

fn u32_at(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
}

fn i32_at(data: &[u8], at: usize) -> i32 {
    i32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
}

fn u64_at(data: &[u8], at: usize) -> u64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&data[at..at + 8]);
    u64::from_le_bytes(b)
}

fn set_u64(data: &mut [u8], at: usize, value: u64) {
    data[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

/// The collision payload's header, as `FUN_142e492a0` reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// `+0x00`, not touched by the fixup.
    pub unknown_00: u64,
    /// `+0x08`: element count of the array at `+0x20` (stride `0x58`).
    pub count_a: i32,
    /// `+0x0C`: element count of the array at `+0x28` (stride `0x38`).
    pub count_b: i32,
    /// `+0x10`: an RVA the fixup converts; zero stays zero.
    pub rva_10: u64,
    /// `+0x18`: an RVA the fixup converts; zero stays zero.
    pub rva_18: u64,
    /// `+0x20`: the RVA of array A.
    pub rva_20: u64,
    /// `+0x28`: the RVA of array B.
    pub rva_28: u64,
    /// `+0x30`, not touched by the fixup.
    pub unknown_30: u64,
}

impl Header {
    fn read(data: &[u8]) -> Result<Header> {
        if data.len() < HEADER_SIZE {
            return Err(Error::Truncated("the 0x38-byte header"));
        }
        let header = Header {
            unknown_00: u64_at(data, 0x00),
            count_a: i32_at(data, 0x08),
            count_b: i32_at(data, 0x0C),
            rva_10: u64_at(data, 0x10),
            rva_18: u64_at(data, 0x18),
            rva_20: u64_at(data, 0x20),
            rva_28: u64_at(data, 0x28),
            unknown_30: u64_at(data, 0x30),
        };
        for (field, value) in [(0x08, header.count_a), (0x0C, header.count_b)] {
            if value < 0 {
                return Err(Error::NegativeCount { field, value });
            }
        }
        Ok(header)
    }
}

/// A parsed collision payload: the header and the offsets of its two arrays.
#[derive(Debug, Clone)]
pub struct Collision<'a> {
    /// The payload bytes, as stored.
    pub data: &'a [u8],
    /// The header the fixup reads.
    pub header: Header,
}

impl<'a> Collision<'a> {
    /// Read the header, and require both arrays to lie inside the payload.
    pub fn parse(data: &'a [u8]) -> Result<Collision<'a>> {
        let header = Header::read(data)?;
        let len = data.len() as u64;
        for (what, rva) in [("array A", header.rva_20), ("array B", header.rva_28)] {
            if rva != 0 && rva > len {
                return Err(Error::OutOfRange { what, value: rva });
            }
        }
        let span_a = header.count_a as u64 * SECTOR_SIZE as u64;
        let span_b = header.count_b as u64 * TAIL_SIZE as u64;
        if header.count_a > 0 && header.rva_20.saturating_add(span_a) > len {
            return Err(Error::OutOfRange {
                what: "array A",
                value: header.rva_20 + span_a,
            });
        }
        if header.count_b > 0 && header.rva_28.saturating_add(span_b) > len {
            return Err(Error::OutOfRange {
                what: "array B",
                value: header.rva_28 + span_b,
            });
        }
        Ok(Collision { data, header })
    }

    /// The sector array: `count_a` elements of `0x58` bytes at `+0x20`.
    pub fn sectors(&self) -> Sectors<'a> {
        Sectors {
            data: self.data,
            at: self.header.rva_20 as usize,
            index: 0,
            count: self.header.count_a as usize,
        }
    }

    /// The tail array: `count_b` elements of `0x38` bytes at `+0x28`.
    pub fn tails(&self) -> Tails<'a> {
        Tails {
            data: self.data,
            at: self.header.rva_28 as usize,
            index: 0,
            count: self.header.count_b as usize,
        }
    }

    /// The bytes of one run named by a sector element.
    ///
    /// `None` when the run leaves the payload or the offsets do not fit a
    /// `usize`.
    pub fn run_bytes(&self, run: (u64, u64)) -> Option<&'a [u8]> {
        let (start, len) = run;
        let start = usize::try_from(start).ok()?;
        let len = usize::try_from(len).ok()?;
        self.data.get(start..start.checked_add(len)?)
    }

    /// Check the tiling every sector element is required to satisfy.
    ///
    /// This is the observation the practice range's chunk establishes: sector
    /// `i`'s four runs, sorted by offset, are adjacent and end exactly where
    /// sector `i + 1`'s first run begins, the last sector ends where array B
    /// begins, and `+0x00` is the lowest of the four. A payload whose counts
    /// are not the tiling ones is refused rather than described, so every run
    /// must be non-empty.
    pub fn check_sector_tiling(&self) -> Result<()> {
        let sectors: Vec<Sector<'a>> = self.sectors().collect();
        for (i, sector) in sectors.iter().enumerate() {
            let next = match sectors.get(i + 1) {
                Some(next) => next.pointers[0],
                None => self.header.rva_28,
            };
            if !tiled(&sector.runs(), next) {
                return Err(Error::SectorNotTiled { sector: i });
            }
        }
        Ok(())
    }
}

fn tiled(runs: &[(u64, u64); 4], next: u64) -> bool {
    if runs.iter().any(|(_, len)| *len == 0) {
        return false;
    }
    let lowest = runs.iter().map(|r| r.0).min().unwrap_or(0);
    if runs[0].0 != lowest {
        return false;
    }
    let mut ordered = *runs;
    ordered.sort_by_key(|r| r.0);
    let mut at = ordered[0].0;
    for (start, len) in ordered {
        if start != at {
            return false;
        }
        at += len;
    }
    at == next
}

/// One `0x58`-byte sector element.
#[derive(Debug, Clone, Copy)]
pub struct Sector<'a> {
    /// The element's index in the array.
    pub index: usize,
    /// The element's raw bytes.
    pub raw: &'a [u8],
    /// The four qwords the fixup converts (`+0x00`, `+0x08`, `+0x10`,
    /// `+0x18`), still as RVAs.
    pub pointers: [u64; 4],
}

impl<'a> Sector<'a> {
    /// The dword at `+0x2C`, `+0x30` and `+0x34`.
    ///
    /// Not read from code: the tiling check shows these three values are the
    /// element counts of the four runs (`+0x2C` for the run at `+0x00` at a
    /// `0x0C` stride, `+0x30` for the runs at `+0x18` (one byte each) and
    /// `+0x08` (`0x10` bytes each), `+0x34` for the run at `+0x10` (four bytes
    /// each)). Their meaning is therefore still open, and the names stay
    /// positional.
    pub fn counts(&self) -> [u32; 3] {
        [
            u32_at(self.raw, 0x2C),
            u32_at(self.raw, 0x30),
            u32_at(self.raw, 0x34),
        ]
    }

    /// The four runs the element names, as `(offset, length)` in payload bytes.
    ///
    /// The run order is the array order the element stores, not the memory
    /// order: `+0x00`, `+0x18`, `+0x08`, `+0x10`.
    pub fn runs(&self) -> [(u64, u64); 4] {
        let c = self.counts();
        [
            (self.pointers[0], c[0] as u64 * 0x0C),
            (self.pointers[3], c[1] as u64),
            (self.pointers[1], c[1] as u64 * 0x10),
            (self.pointers[2], c[2] as u64 * 0x04),
        ]
    }

    /// The run at `+0x00` read as 12-byte float triples.
    ///
    /// The stride and the count are the tiling ones; that the 12 bytes are
    /// three `f32`s is an observation (they read as map-scale coordinates), not
    /// a read expression, so this helper is a convenience and not part of the
    /// parse.
    pub fn vertices(&self, payload: &[u8]) -> Vec<[f32; 3]> {
        let (start, len) = self.runs()[0];
        let Ok(start) = usize::try_from(start) else {
            return Vec::new();
        };
        let Ok(len) = usize::try_from(len) else {
            return Vec::new();
        };
        let Some(bytes) = payload.get(start..start + len) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(len / 0x0C);
        for chunk in bytes.chunks_exact(0x0C) {
            let f =
                |o: usize| f32::from_le_bytes([chunk[o], chunk[o + 1], chunk[o + 2], chunk[o + 3]]);
            out.push([f(0), f(4), f(8)]);
        }
        out
    }
}

/// The `0x58`-stride sector array iterator.
pub struct Sectors<'a> {
    data: &'a [u8],
    at: usize,
    index: usize,
    count: usize,
}

impl<'a> Iterator for Sectors<'a> {
    type Item = Sector<'a>;

    fn next(&mut self) -> Option<Sector<'a>> {
        if self.index >= self.count {
            return None;
        }
        let base = self.at + self.index * SECTOR_SIZE;
        let raw = self.data.get(base..base + SECTOR_SIZE)?;
        let pointers = [
            u64_at(raw, 0x00),
            u64_at(raw, 0x08),
            u64_at(raw, 0x10),
            u64_at(raw, 0x18),
        ];
        let sector = Sector {
            index: self.index,
            raw,
            pointers,
        };
        self.index += 1;
        Some(sector)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let left = self.count.saturating_sub(self.index);
        (left, Some(left))
    }
}

/// One `0x38`-byte tail element.
#[derive(Debug, Clone, Copy)]
pub struct Tail<'a> {
    /// The element's index in the array.
    pub index: usize,
    /// The element's raw bytes.
    pub raw: &'a [u8],
    /// The three qwords the fixup converts (`+0x00`, `+0x08`, `+0x10`), still
    /// as RVAs.
    pub pointers: [u64; 3],
}

/// The `0x38`-stride tail array iterator.
pub struct Tails<'a> {
    data: &'a [u8],
    at: usize,
    index: usize,
    count: usize,
}

impl<'a> Iterator for Tails<'a> {
    type Item = Tail<'a>;

    fn next(&mut self) -> Option<Tail<'a>> {
        if self.index >= self.count {
            return None;
        }
        let base = self.at + self.index * TAIL_SIZE;
        let raw = self.data.get(base..base + TAIL_SIZE)?;
        let pointers = [u64_at(raw, 0x00), u64_at(raw, 0x08), u64_at(raw, 0x10)];
        let tail = Tail {
            index: self.index,
            raw,
            pointers,
        };
        self.index += 1;
        Some(tail)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let left = self.count.saturating_sub(self.index);
        (left, Some(left))
    }
}

/// `FUN_142e492a0` (`0x142e492a0`), the collision chunk's fixup, in place.
///
/// `base` is the payload's address in memory, which is what the client adds to
/// every RVA. The header's four RVAs at `+0x10`, `+0x18`, `+0x20` and `+0x28`
/// keep a zero value as zero; the element passes do not — every one of the four
/// (sector) and three (tail) qwords gets `base` added whatever it holds.
///
/// The header is converted before either array is walked, because the walk
/// takes its bases from `+0x20` and `+0x28`. Counts are `i32` and the loops run
/// while the counter is below the count, exactly as the client's do.
pub fn fixup_in_place(data: &mut [u8], base: u64) -> Result<()> {
    if data.len() < HEADER_SIZE {
        return Err(Error::Truncated("the 0x38-byte header"));
    }
    // The counts are read as the client reads them: a signed dword compared
    // against a counter, so a non-positive count runs no pass rather than
    // failing. The RVAs are kept before the conversion because the client
    // indexes the converted pointer and this runs on the stored bytes.
    let count_a = i32_at(data, 0x08);
    let count_b = i32_at(data, 0x0C);
    let array_a = u64_at(data, 0x20);
    let array_b = u64_at(data, 0x28);
    for at in [0x10usize, 0x18, 0x20, 0x28] {
        let rva = u64_at(data, at);
        set_u64(data, at, if rva == 0 { 0 } else { rva.wrapping_add(base) });
    }
    if count_a > 0 {
        let at = array_a as usize;
        let span = count_a as usize * SECTOR_SIZE;
        if at.saturating_add(span) > data.len() {
            return Err(Error::OutOfRange {
                what: "array A",
                value: array_a,
            });
        }
        for i in 0..count_a as usize {
            let element = at + i * SECTOR_SIZE;
            for field in [0x00usize, 0x08, 0x10, 0x18] {
                let value = u64_at(data, element + field);
                set_u64(data, element + field, value.wrapping_add(base));
            }
        }
    }
    if count_b > 0 {
        let at = array_b as usize;
        let span = count_b as usize * TAIL_SIZE;
        if at.saturating_add(span) > data.len() {
            return Err(Error::OutOfRange {
                what: "array B",
                value: array_b,
            });
        }
        for i in 0..count_b as usize {
            let element = at + i * TAIL_SIZE;
            for field in [0x00usize, 0x08, 0x10] {
                let value = u64_at(data, element + field);
                set_u64(data, element + field, value.wrapping_add(base));
            }
        }
    }
    Ok(())
}

/// The pathing chunk's header record, as `MapChunkHeaderFixup`
/// (`0x142e49360`) reads it.
///
/// ```text
/// +0x00  u64  RVA -> pointer (zero stays zero)   the first STUD blob
/// +0x08  u64  RVA -> pointer (zero stays zero)
/// +0x10  u64  not touched by the fixup
/// +0x18  u64  RVA -> pointer (zero stays zero)
/// ```
///
/// The client's dispatch sends the pathing kinds (slot `0x1E`, kind `0x12`)
/// here before it hands `+0x00` to the STUD loader, and the additive pathing
/// kind (slot `0x22`) to a sibling that converts two pointers instead.
pub const PATHING_HEADER_SIZE: usize = 0x20;
/// The three slots `MapChunkHeaderFixup` converts.
pub const PATHING_HEADER_POINTERS: usize = 3;

/// `MapChunkHeaderFixup` (`0x142e49360`), the pathing chunk's header fixup,
/// in place.
///
/// The client converts three self-relative RVAs - at `+0x00`, `+0x08` and
/// `+0x18` - to pointers by adding the record's own address to each, and a
/// stored zero stays zero. Each conversion is the same three instructions: a
/// `lea` of the value plus the base, a `test` of the value, and a `cmove`
/// back to zero when the value was zero. The client's `+0x10` is not touched.
///
/// The three conversions are written out rather than looped, because the
/// machine does them one at a time and each one carries its own zero test.
///
/// `base` is the record's address in memory, which is what the client adds;
/// the client takes it from the record's own pointer, so a caller reading the
/// file's bytes has to supply it.
pub fn fixup_header_in_place(data: &mut [u8], base: u64) -> Result<()> {
    if data.len() < PATHING_HEADER_SIZE {
        return Err(Error::Truncated("the chunk's header record"));
    }
    let blob = u64_at(data, 0x00);
    set_u64(
        data,
        0x00,
        if blob == 0 {
            0
        } else {
            blob.wrapping_add(base)
        },
    );
    let second = u64_at(data, 0x08);
    set_u64(
        data,
        0x08,
        if second == 0 {
            0
        } else {
            second.wrapping_add(base)
        },
    );
    let third = u64_at(data, 0x18);
    set_u64(
        data,
        0x18,
        if third == 0 {
            0
        } else {
            third.wrapping_add(base)
        },
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(data: &mut [u8], at: usize, value: u64) {
        set_u64(data, at, value);
    }

    #[test]
    fn the_header_fixup_rebases_three_slots_and_keeps_a_zero_zero() {
        let mut d = vec![0u8; PATHING_HEADER_SIZE];
        put(&mut d, 0x00, 0x20);
        put(&mut d, 0x08, 0x1000);
        put(&mut d, 0x10, 0xDEAD_BEEF); // not touched
        put(&mut d, 0x18, 0x40);
        fixup_header_in_place(&mut d, 0x1_0000).unwrap();
        assert_eq!(u64_at(&d, 0x00), 0x1_0020);
        assert_eq!(u64_at(&d, 0x08), 0x1_1000);
        assert_eq!(u64_at(&d, 0x18), 0x1_0040);
        assert_eq!(
            u64_at(&d, 0x10),
            0xDEAD_BEEF,
            "the client does not touch +0x10"
        );
    }

    #[test]
    fn a_stored_zero_stays_zero() {
        let mut d = vec![0u8; PATHING_HEADER_SIZE];
        fixup_header_in_place(&mut d, 0x1_0000).unwrap();
        assert_eq!(u64_at(&d, 0x00), 0);
        assert_eq!(u64_at(&d, 0x08), 0);
        assert_eq!(u64_at(&d, 0x18), 0);
    }

    #[test]
    fn the_fixup_is_the_clients_order() {
        // The client reads each slot before it writes that slot, so a slot's
        // own value is never the base of the next conversion.
        let mut d = vec![0u8; PATHING_HEADER_SIZE];
        put(&mut d, 0x00, 0x30);
        put(&mut d, 0x08, 0x08);
        put(&mut d, 0x18, 0x10);
        fixup_header_in_place(&mut d, 0x100).unwrap();
        assert_eq!(u64_at(&d, 0x00), 0x130);
        assert_eq!(u64_at(&d, 0x08), 0x108);
        assert_eq!(u64_at(&d, 0x18), 0x110);
    }

    #[test]
    fn a_short_record_is_refused() {
        let mut d = vec![0u8; PATHING_HEADER_SIZE - 1];
        assert!(matches!(
            fixup_header_in_place(&mut d, 0),
            Err(Error::Truncated(_))
        ));
    }
}

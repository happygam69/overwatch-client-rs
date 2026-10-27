//! Asset bundles: the container for assets that are not in the encoded store.
//!
//! A type-`0x09C` asset (`0x0D90...`) is a bundle. Its payload is a table:
//!
//! ```text
//! +0x00  i32  entry count
//! +0x04  u8   offset width (1, 2 or 4)
//! +0x05  entry * count
//!          u64 GUID
//!          offset, `width` bytes, relative to the start of this payload
//! ```
//!
//! The client's entry lookup is `FUN_141fd5820` (`0x141fd5820`): it reads the
//! count and width, binary-searches the entries by GUID (the table is
//! GUID-sorted), and returns `payload + offset`, with offset `0` meaning "not
//! here". It carries **no length**: an asset's length is its CMF record's
//! declared size, which is why the caller passes that size separately.
//!
//! Which assets are in a bundle is the CMF record byte at `+12`, bit 7. The
//! client copies that bit to bit 16 of its in-memory record (`FUN_141feff80`,
//! `0x141feff80`) and exposes it as bit 9 of the query result (`FUN_141fef610`,
//! `0x141fef610`); the loader diverts those assets to its bundle directory
//! (`FUN_141fd4a50`, `0x141fd4a50`). See `docs/formats/maps.md` for the
//! directory layout and `docs/formats/casc.md` for the record byte.

use std::collections::HashMap;
use std::fmt;

/// The asset type of a bundle. `guid::type_of(0x0d90_0000_0000_0000)` is this.
pub const BUNDLE_TYPE: u16 = 0x09C;

/// The CMF record byte bit that says the payload is in a bundle.
pub const RECORD_IN_BUNDLE: u8 = 0x80;

/// Bytes before the first entry: `i32 count`, `u8 width`.
pub const HEADER_SIZE: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// Shorter than the header or than the entry table the header declares.
    Truncated {
        needed: usize,
        have: usize,
    },
    NegativeCount(i32),
    /// A width other than 1, 2 or 4 has no entry stride.
    UnsupportedWidth(u8),
    /// The entry's offset plus the record's declared size runs past the payload.
    OutOfRange {
        offset: u32,
        size: u32,
        len: usize,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Truncated { needed, have } => {
                write!(f, "bundle is truncated: needs {needed} B, has {have} B")
            }
            Error::NegativeCount(count) => write!(f, "bundle entry count is negative: {count}"),
            Error::UnsupportedWidth(width) => {
                write!(f, "bundle offset width {width} is not 1, 2 or 4")
            }
            Error::OutOfRange { offset, size, len } => write!(
                f,
                "bundle entry at {offset} + {size} B runs past the {len}-byte payload"
            ),
        }
    }
}

impl std::error::Error for Error {}

type Result<T> = std::result::Result<T, Error>;

/// One bundle payload, read as the client reads it.
#[derive(Clone, Copy)]
pub struct Bundle<'a> {
    payload: &'a [u8],
    count: usize,
    width: u8,
}

impl<'a> Bundle<'a> {
    /// Read the header and check that the declared table fits.
    ///
    /// The client does not bound-check the table (it trusts the file); a table
    /// that runs past the payload is reported here rather than read.
    pub fn parse(payload: &'a [u8]) -> Result<Bundle<'a>> {
        if payload.len() < HEADER_SIZE {
            return Err(Error::Truncated {
                needed: HEADER_SIZE,
                have: payload.len(),
            });
        }
        let count = i32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
        if count < 0 {
            return Err(Error::NegativeCount(count));
        }
        let width = payload[4];
        if !matches!(width, 1 | 2 | 4) {
            return Err(Error::UnsupportedWidth(width));
        }
        let count = count as usize;
        let needed = HEADER_SIZE + count.saturating_mul(8 + width as usize);
        if payload.len() < needed {
            return Err(Error::Truncated {
                needed,
                have: payload.len(),
            });
        }
        Ok(Bundle {
            payload,
            count,
            width,
        })
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Bytes per entry: `u64` GUID plus the `width`-byte offset.
    pub fn stride(&self) -> usize {
        8 + self.width as usize
    }

    pub fn width(&self) -> u8 {
        self.width
    }

    /// The entry at `index`, as `(guid, offset)`.
    ///
    /// The offset is the raw value, so `0` is the client's "not here" marker.
    pub fn entry(&self, index: usize) -> Option<(u64, u32)> {
        if index >= self.count {
            return None;
        }
        let at = HEADER_SIZE + index * self.stride();
        let e = &self.payload[at..at + self.stride()];
        let guid = u64::from_le_bytes([e[0], e[1], e[2], e[3], e[4], e[5], e[6], e[7]]);
        let offset = match self.width {
            1 => u32::from(e[8]),
            2 => u32::from(u16::from_le_bytes([e[8], e[9]])),
            _ => u32::from_le_bytes([e[8], e[9], e[10], e[11]]),
        };
        Some((guid, offset))
    }

    /// Every entry, in file order.
    pub fn entries(&self) -> impl Iterator<Item = (u64, u32)> + '_ {
        (0..self.count).filter_map(|i| self.entry(i))
    }

    /// The entry's offset, or `None` when the table has no such GUID.
    ///
    /// This is the client's search (`BundleEntryLookup`, `0x141fd5820`)
    /// operation for operation: the bounds are the first and the **last**
    /// entry index (`0` and `count - 1`), not the usual half-open pair, and
    /// the midpoint is the arithmetic mean of those two indices. A count of
    /// `0` returns before any entry is read. On a GUID-sorted table this is a
    /// lower bound; on an unsorted table it is the same walk the client takes,
    /// which is why the table has to be GUID-sorted. Offset `0` is the
    /// client's "absent" value and is reported as `None` here too.
    pub fn offset_of(&self, guid: u64) -> Option<u32> {
        if self.count == 0 {
            return None;
        }
        let last = self.count - 1;
        let mut lo = 0usize;
        let mut hi = last;
        while lo < hi {
            let mid = (lo + hi) >> 1;
            let (at, _) = self.entry(mid)?;
            if at < guid {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        let (found, offset) = self.entry(lo)?;
        if found != guid || offset == 0 {
            return None;
        }
        Some(offset)
    }

    /// The asset's bytes: `payload[offset .. offset + size]`.
    ///
    /// `size` is the CMF record's declared size — the bundle stores no length.
    /// The client adds the offset to the payload pointer and trusts the size;
    /// this returns an error instead when the span runs past the payload.
    pub fn span(&self, offset: u32, size: u32) -> Result<&'a [u8]> {
        let start = offset as usize;
        let end = start.checked_add(size as usize).ok_or(Error::OutOfRange {
            offset,
            size,
            len: self.payload.len(),
        })?;
        if end > self.payload.len() {
            return Err(Error::OutOfRange {
                offset,
                size,
                len: self.payload.len(),
            });
        }
        Ok(&self.payload[start..end])
    }

    /// The whole payload, for diagnostics.
    pub fn payload(&self) -> &'a [u8] {
        self.payload
    }
}

/// The client's `BundleEntryLookup` (`0x141fd5820`) as a pure function.
///
/// The client's public argument is a tagged bundle request handle: its low six
/// bits are a tag and the rest masks to a 64-byte-aligned object whose `+0`
/// qword is the bundle payload and whose `+8` dword carries the tag. A handle
/// of `u64::MAX`, a handle that masks to zero, a tag mismatch or a null
/// payload pointer each return `0` before the search.
///
/// `object_tag` is the object's `+8` dword; `payload` is the bytes the `+0`
/// pointer names, or `None` when that pointer is null. `payload_base` is the
/// guest address the payload occupies. The client returns `payload + offset`,
/// a pointer; this returns that guest address, and `0` for the client's
/// "absent". The client's first argument (the bundle directory pointer) is not
/// read by the function and is not part of this contract.
pub fn entry_lookup(
    handle: u64,
    object_tag: u32,
    payload: Option<&[u8]>,
    guid: u64,
    payload_base: u64,
) -> u64 {
    if handle == u64::MAX {
        return 0;
    }
    if handle & !0x3f == 0 {
        return 0;
    }
    if ((handle as u32) ^ object_tag) & 0x3f != 0 {
        return 0;
    }
    let Some(payload) = payload else {
        return 0;
    };
    match Bundle::parse(payload) {
        Ok(bundle) => match bundle.offset_of(guid) {
            Some(offset) => payload_base.wrapping_add(u64::from(offset)),
            None => 0,
        },
        // The client's width gate and its "the table runs past the payload"
        // case both land here: a width other than 1, 2 or 4 makes the client
        // return 0 before it reads an entry, and an out-of-bounds table is
        // outside the client's contract (it does not bound-check).
        Err(_) => 0,
    }
}

/// The client's per-type bundle directory, read as `BundleDirectoryLookup`
/// (`0x141fd5160`) walks it.
///
/// The directory is the content database's blob at `DAT_1441b3b88`. Its type
/// count is an `i32` at `+0x20` and its type-entry array pointer an absolute
/// qword at `+0x18`; every pointer in the blob is absolute in the client. The
/// type-entry array has stride `0x20`:
///
/// ```text
/// type entry (0x20)   +0x00 i32  type key (the GUID hash below)
///                     +0x08 u64  type block
///                     +0x10 u64  bundle handle array
///
/// type block          +0x00 u64  group array
///                     +0x08 u32  group stride (0x28; not read by the lookup)
///                     +0x0c u16  group count
///                     +0x0e u16  type id (not read by the lookup)
///
/// group (0x28)        +0x00 u64  listed GUID array (not read by the lookup)
///                     +0x08 u64  sorted GUID array
///                     +0x10 u64  index array, one u16 per sorted GUID
///                     +0x18 u64  the bundle's own GUID
///                     +0x20 i32  sorted GUID count
///                     +0x24 i16  listed GUID count
/// ```
///
/// A hit's index array entry is the sorted row's position in the bundle's
/// *listed* GUID array; adding the listed counts of the preceding groups gives
/// the row in the type's bundle handle array. That is how one handle array can
/// span the groups of a type.
///
/// `blob` is the memory the pointers name and `base` the guest address of
/// `blob[0]`; `directory` is the guest address of the directory object (the
/// client's fixed `DAT_1441b3b88`). The client's second read of the type-entry
/// pointer is the same qword as `directory + 0x18` — its global address is the
/// field's address — so the model reads it once.
#[derive(Clone, Copy)]
pub struct Directory<'a> {
    blob: &'a [u8],
    base: u64,
    directory: u64,
}

impl<'a> Directory<'a> {
    /// Read the directory at `directory` from `blob`, whose first byte is the
    /// guest address `base`.
    pub fn new(blob: &'a [u8], base: u64, directory: u64) -> Directory<'a> {
        Directory {
            blob,
            base,
            directory,
        }
    }

    fn bytes(&self, addr: u64, len: usize) -> Option<&'a [u8]> {
        let off = addr.checked_sub(self.base)? as usize;
        self.blob.get(off..off.checked_add(len)?)
    }

    fn u8(&self, addr: u64) -> Option<u8> {
        self.bytes(addr, 1).map(|b| b[0])
    }

    fn u16(&self, addr: u64) -> Option<u16> {
        Some(u16::from_le_bytes(self.bytes(addr, 2)?.try_into().ok()?))
    }

    fn i16(&self, addr: u64) -> Option<i16> {
        Some(i16::from_le_bytes(self.bytes(addr, 2)?.try_into().ok()?))
    }

    fn u32(&self, addr: u64) -> Option<u32> {
        Some(u32::from_le_bytes(self.bytes(addr, 4)?.try_into().ok()?))
    }

    fn i32(&self, addr: u64) -> Option<i32> {
        Some(i32::from_le_bytes(self.bytes(addr, 4)?.try_into().ok()?))
    }

    fn u64(&self, addr: u64) -> Option<u64> {
        Some(u64::from_le_bytes(self.bytes(addr, 8)?.try_into().ok()?))
    }

    /// The client's `FUN_141fd3970` handle state.
    ///
    /// `4` is "loaded" (the lookup returns it at once), `2` and `3` are
    /// "known but not ready" (the lookup remembers the first one), and `0`
    /// is an unusable handle (no object, a tag mismatch, or `-1`). The client
    /// also spins on the object's `+0x3c` lock word, which nets to zero and so
    /// has no observable effect here.
    ///
    /// The two readiness bytes are `obj+0xa` bit 0 (`FUN_141fd3f40`) and
    /// `obj+8` bit 15 (`FUN_141fd4040`, which the decompiler printed as the
    /// wider mask `0xffffff01`; the disassembly is `SHR EAX,0xf` / `AND AL,1`).
    fn handle_status(&self, handle: u64) -> i32 {
        if handle == u64::MAX {
            return 0;
        }
        let object = handle & !0x3f;
        if object == 0 {
            return 0;
        }
        let Some(tag) = self.u32(object.wrapping_add(8)) else {
            return 0;
        };
        if ((handle as u32) ^ tag) & 0x3f != 0 {
            return 0;
        }
        if self.u8(object.wrapping_add(0x0a)).unwrap_or(0) & 1 == 0 {
            return 2;
        }
        if (tag >> 15) & 1 != 0 {
            3
        } else {
            4
        }
    }
}

/// The client's GUID-to-type key, as `BundleDirectoryLookup` computes it.
///
/// The key is the asset type id: the bit-reversed 12-bit field at GUID bits
/// 48..59, plus one, sign-extended from 12 bits. It is the same value as
/// `guid::type_of` for every field except `0xFFF`, where the client's 12-bit
/// sign extension wraps to `0` while `type_of` returns `0x1000`. A
/// type-`0x09C` bundle GUID keys to `0x09C`, which is how the type-entry array
/// is indexed.
pub fn guid_type_key(guid: u64) -> i32 {
    let a = (guid & 0xFFFF_0000_0000_0000) << 4;
    let s = a >> 1;
    let mut x = ((a << 1) ^ s) & 0xAAAA_AAAA_AAAA_AAAA;
    x ^= s;
    let s = x >> 2;
    let mut y = ((x << 2) ^ s) & 0xCCCC_CCCC_CCCC_CCCC;
    y ^= s;
    let s = y >> 4;
    let mut z = ((y << 4) ^ s) & 0xF0F0_F0F0_F0F0_F0F0;
    z ^= s;
    let w = (z.swap_bytes() as u32).wrapping_add(1);
    ((w << 20) as i32) >> 20
}

/// `BundleDirectoryLookup` (`0x141fd5160`) as a pure function.
///
/// Returns the request handle of the bundle that holds `guid`, or `u64::MAX`
/// (the client's `-1`) when no loaded group holds it. The first group that
/// holds the GUID wins unless its handle is not fully loaded; the client then
/// remembers it and keeps looking for a fully loaded one.
///
/// `is_loaded` is the client's `FUN_141fcbf40`: the group's bundle GUID is
/// looked up in the content database's loaded-bundle set. GUID `0` is the
/// client's unconditional "loaded" answer. The set itself is live loader
/// state, so the caller supplies it.
///
/// The search is the client's: bounds `0` and `count - 1` (not half-open),
/// midpoint `(lo + hi) >> 1` in 32-bit arithmetic, an unsigned
/// `slice[mid] >= guid` pivot and a lower bound, then one equality test. A
/// non-positive sorted count skips the group; zero type entries, a missing
/// type and a zero group count all return `u64::MAX`. Negative counts are
/// outside the producer's contract (the client's count arithmetic wraps), so
/// they are treated as empty.
pub fn directory_lookup(
    directory: &Directory<'_>,
    guid: u64,
    is_loaded: impl Fn(u64) -> bool,
) -> u64 {
    let mut out = u64::MAX;
    let Some(count) = directory.i32(directory.directory.wrapping_add(0x20)) else {
        return out;
    };
    if count <= 0 {
        return out;
    }
    let Some(types) = directory.u64(directory.directory.wrapping_add(0x18)) else {
        return out;
    };
    let key = guid_type_key(guid);
    let mut found = None;
    let mut scan: i64 = 0;
    while scan < i64::from(count) {
        let entry = types.wrapping_add((scan as u64) << 5);
        let Some(entry_key) = directory.i32(entry) else {
            return out;
        };
        if entry_key == key {
            found = Some(entry);
            break;
        }
        scan += 1;
    }
    let Some(entry) = found else {
        return out;
    };
    let Some(block) = directory.u64(entry.wrapping_add(8)) else {
        return out;
    };
    let Some(handles) = directory.u64(entry.wrapping_add(0x10)) else {
        return out;
    };
    let Some(group_count) = directory.u16(block.wrapping_add(0x0c)) else {
        return out;
    };
    if group_count == 0 {
        return out;
    }
    let Some(groups) = directory.u64(block) else {
        return out;
    };
    let mut running: u32 = 0;
    let mut group_offset: u64 = 0;
    let mut group_index: u16 = 0;
    while group_index < group_count {
        let group = groups.wrapping_add(group_offset);
        let Some(bundle) = directory.u64(group.wrapping_add(0x18)) else {
            return out;
        };
        if is_loaded(bundle) {
            let Some(count) = directory.i32(group.wrapping_add(0x20)) else {
                return out;
            };
            if count > 0 {
                let Some(sorted) = directory.u64(group.wrapping_add(8)) else {
                    return out;
                };
                let last = count - 1;
                let mut lo: i32 = 0;
                let mut hi: i32 = last;
                while lo < hi {
                    let mid = lo.wrapping_add(hi) >> 1;
                    let Some(pivot) = directory.u64(sorted.wrapping_add((mid as u64) << 3)) else {
                        return out;
                    };
                    if pivot >= guid {
                        hi = mid;
                    } else {
                        lo = mid.wrapping_add(1);
                    }
                }
                let Some(found_guid) = directory.u64(sorted.wrapping_add((lo as u64) << 3)) else {
                    return out;
                };
                if found_guid == guid {
                    let Some(index_array) = directory.u64(group.wrapping_add(0x10)) else {
                        return out;
                    };
                    let Some(index) = directory.u16(index_array.wrapping_add((lo as u64) << 1))
                    else {
                        return out;
                    };
                    let slot = u32::from(index).wrapping_add(running) as i32;
                    let Some(handle) = directory.u64(handles.wrapping_add((slot as u64) << 3))
                    else {
                        return out;
                    };
                    let status = directory.handle_status(handle);
                    if status == 4 {
                        return handle;
                    }
                    if out == u64::MAX {
                        out = handle;
                    }
                }
            }
        }
        running = running
            .wrapping_add(directory.i16(group.wrapping_add(0x24)).unwrap_or(0) as i32 as u32);
        group_offset = group_offset.wrapping_add(0x28);
        group_index += 1;
    }
    out
}

/// Where an asset lives inside a bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Location {
    /// The bundle asset's GUID.
    pub bundle: u64,
    /// The entry's offset inside the bundle payload.
    pub offset: u32,
}

/// GUID -> bundle location, built from the manifest's type-`0x09C` assets.
///
/// The client keeps the same relation as a per-type directory handed to its
/// bundle job (`FUN_141fd53a0`, `0x141fd53a0`): each group is a bundle, with the
/// bundle's own GUID and the GUIDs it holds. This index is the flat form of that
/// directory — the asset GUID to `(bundle, offset)`. The client's *producer* of
/// the directory blob is not read yet; see `docs/open-questions.md`.
#[derive(Default)]
pub struct BundleIndex {
    entries: HashMap<u64, Location>,
    /// Bundles whose payload parsed.
    pub bundles: usize,
    /// Bundles whose payload could not be opened at all.
    pub unreadable: Vec<u64>,
    /// Bundles whose payload opened but is not a bundle table.
    pub malformed: Vec<u64>,
    /// Entry records seen, including offset-0 rows.
    pub entries_seen: usize,
    /// Entry rows whose GUID was already indexed from another bundle.
    ///
    /// The client's directory search keeps looking when the first hit is not
    /// loaded, so a duplicate can resolve to a later bundle; this index keeps
    /// the first. On this install the count is zero, which is why that
    /// difference cannot show.
    pub duplicates: usize,
}

impl BundleIndex {
    /// Index `bundle_guids`, opening each payload through `payload`.
    ///
    /// `payload` returns `None` when the asset cannot be opened; such a bundle
    /// is recorded in [`BundleIndex::unreadable`]. A payload that opens but is
    /// not a bundle table is recorded in [`BundleIndex::malformed`]. Neither is
    /// fatal: an install is allowed to be partial. Entries whose offset is `0`
    /// are the client's "absent" rows and are not indexed. The first bundle
    /// that holds a GUID wins, in manifest order.
    pub fn build(
        bundle_guids: impl Iterator<Item = u64>,
        mut payload: impl FnMut(u64) -> Option<Vec<u8>>,
    ) -> BundleIndex {
        let mut index = BundleIndex::default();
        for bundle_guid in bundle_guids {
            let Some(bytes) = payload(bundle_guid) else {
                index.unreadable.push(bundle_guid);
                continue;
            };
            let Ok(bundle) = Bundle::parse(&bytes) else {
                index.malformed.push(bundle_guid);
                continue;
            };
            index.bundles += 1;
            for (guid, offset) in bundle.entries() {
                index.entries_seen += 1;
                if offset == 0 {
                    continue;
                }
                let location = Location {
                    bundle: bundle_guid,
                    offset,
                };
                match index.entries.entry(guid) {
                    std::collections::hash_map::Entry::Occupied(slot) => {
                        if *slot.get() != location {
                            index.duplicates += 1;
                        }
                    }
                    std::collections::hash_map::Entry::Vacant(slot) => {
                        slot.insert(location);
                    }
                }
            }
        }
        index
    }

    pub fn get(&self, guid: u64) -> Option<Location> {
        self.entries.get(&guid).copied()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle(width: u8, entries: &[(u64, u32)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(entries.len() as i32).to_le_bytes());
        out.push(width);
        for (guid, offset) in entries {
            out.extend_from_slice(&guid.to_le_bytes());
            match width {
                1 => out.push(*offset as u8),
                2 => out.extend_from_slice(&(*offset as u16).to_le_bytes()),
                _ => out.extend_from_slice(&offset.to_le_bytes()),
            }
        }
        out
    }

    #[test]
    fn reads_all_three_widths() {
        // The header read and the stride are the client's: 5 bytes, then
        // 8 + width per entry.
        for width in [1u8, 2, 4] {
            let rows = [(0x10u64, 5u32), (0x20, 6), (0x30, 7)];
            let payload = bundle(width, &rows);
            let parsed = Bundle::parse(&payload).unwrap();
            assert_eq!(parsed.len(), 3);
            assert_eq!(parsed.width(), width);
            assert_eq!(parsed.stride(), 8 + width as usize);
            assert_eq!(parsed.entries().collect::<Vec<_>>(), rows.to_vec());
            for (guid, offset) in rows {
                assert_eq!(parsed.offset_of(guid), Some(offset));
                assert_eq!(
                    parsed.span(offset, 2).unwrap(),
                    &payload[offset as usize..][..2]
                );
            }
            assert_eq!(parsed.offset_of(0x11), None);
        }
    }

    #[test]
    fn offset_zero_is_absent() {
        // FUN_141fd5820 returns 0 for an offset of 0, which the caller reads as
        // "not in this bundle".
        let payload = bundle(2, &[(0x10, 0), (0x20, 0x40)]);
        let parsed = Bundle::parse(&payload).unwrap();
        assert_eq!(parsed.entry(0), Some((0x10, 0)));
        assert_eq!(parsed.offset_of(0x10), None);
        assert_eq!(parsed.offset_of(0x20), Some(0x40));
    }

    #[test]
    fn rejects_a_table_that_runs_past_the_payload() {
        let mut payload = bundle(4, &[(0x10, 0x20)]);
        payload.truncate(payload.len() - 1);
        assert!(matches!(
            Bundle::parse(&payload),
            Err(Error::Truncated { .. })
        ));
        assert!(matches!(
            Bundle::parse(&[0, 0, 0, 0]),
            Err(Error::Truncated { .. })
        ));
    }

    #[test]
    fn rejects_bad_width_and_negative_count() {
        let mut payload = bundle(1, &[(0x10, 0x20)]);
        payload[4] = 3;
        assert_eq!(
            Bundle::parse(&payload).err(),
            Some(Error::UnsupportedWidth(3))
        );
        let mut payload = bundle(1, &[]);
        payload[0..4].copy_from_slice(&(-1i32).to_le_bytes());
        assert_eq!(
            Bundle::parse(&payload).err(),
            Some(Error::NegativeCount(-1))
        );
    }

    #[test]
    fn search_finds_the_lower_bound_the_client_finds() {
        // The client binary-searches; an unsorted table must not be trusted,
        // and a duplicate GUID yields the first match.
        let payload = bundle(2, &[(0x10, 3), (0x20, 4), (0x20, 5), (0x30, 6)]);
        let parsed = Bundle::parse(&payload).unwrap();
        assert_eq!(parsed.offset_of(0x20), Some(4));
        assert_eq!(parsed.offset_of(0x30), Some(6));
        assert_eq!(parsed.offset_of(0x31), None);
        assert_eq!(parsed.offset_of(0x0F), None);
    }

    #[test]
    fn unsorted_table_takes_the_clients_walk() {
        // The client's bounds are 0 and count - 1. On `[2, 1]` looking for 2
        // it examines no index at all and reads entry 0: a hit. The previous
        // half-open [0, count) search examined index 1, moved past the end
        // and returned absent. A bundle is GUID-sorted by contract; the walk
        // still has to be the client's.
        let payload = bundle(4, &[(2, 0x11), (1, 0x22)]);
        let parsed = Bundle::parse(&payload).unwrap();
        assert_eq!(parsed.offset_of(2), Some(0x11));
        assert_eq!(parsed.offset_of(1), None);
    }

    #[test]
    fn the_first_duplicate_wins_and_its_zero_offset_is_absent() {
        // A lower bound returns the first of equal GUIDs, so a later
        // duplicate's offset cannot rescue an offset-0 first row.
        let payload = bundle(4, &[(0x10, 0x20), (0x20, 0), (0x20, 0x30), (0x30, 0x40)]);
        let parsed = Bundle::parse(&payload).unwrap();
        assert_eq!(parsed.offset_of(0x20), None);
        assert_eq!(parsed.offset_of(0x30), Some(0x40));
    }

    #[test]
    fn one_entry_and_empty_tables_are_the_clients_edges() {
        let empty = bundle(2, &[]);
        assert_eq!(Bundle::parse(&empty).unwrap().offset_of(0), None);
        let one = bundle(1, &[(u64::MAX, 0x7f)]);
        let parsed = Bundle::parse(&one).unwrap();
        assert_eq!(parsed.offset_of(u64::MAX), Some(0x7f));
        assert_eq!(parsed.offset_of(0), None);
        assert_eq!(parsed.offset_of(u64::MAX - 1), None);
    }

    #[test]
    fn entry_lookup_models_the_tagged_handle_guard() {
        let payload = bundle(2, &[(0x10, 0x40), (0x20, 0x50)]);
        let tag = 0x21u32;
        let handle = 0x0000_0002_0000_0000u64 | u64::from(tag);
        let object_tag = tag | 0xffff_ffc0;
        assert_eq!(
            entry_lookup(handle, object_tag, Some(&payload), 0x20, 0x1000),
            0x1050
        );
        assert_eq!(
            entry_lookup(handle, object_tag, Some(&payload), 0x30, 0x1000),
            0
        );
        assert_eq!(
            entry_lookup(u64::MAX, object_tag, Some(&payload), 0x20, 0x1000),
            0
        );
        assert_eq!(
            entry_lookup(u64::from(tag), object_tag, Some(&payload), 0x20, 0x1000),
            0
        );
        assert_eq!(
            entry_lookup(handle, tag ^ 1, Some(&payload), 0x20, 0x1000),
            0
        );
        assert_eq!(entry_lookup(handle, object_tag, None, 0x20, 0x1000), 0);
    }

    /// A synthetic in-memory directory: `base == 0`, so every pointer is an
    /// offset into `blob`. Fixed regions keep the builder readable.
    struct Dir {
        blob: Vec<u8>,
    }

    impl Dir {
        const DIR: usize = 0x00;
        const TYPE: usize = 0x40;
        const BLOCK: usize = 0x100;
        const GROUP: usize = 0x200;
        const SORTED: usize = 0x400;
        const INDEX: usize = 0x600;
        const LISTED: usize = 0x700;
        const HANDLES: usize = 0x800;
        const OBJECT: usize = 0x900;

        fn new() -> Dir {
            Dir {
                blob: vec![0; 0x1000],
            }
        }

        fn u64(&mut self, at: usize, v: u64) {
            self.blob[at..at + 8].copy_from_slice(&v.to_le_bytes());
        }

        fn u32(&mut self, at: usize, v: u32) {
            self.blob[at..at + 4].copy_from_slice(&v.to_le_bytes());
        }

        fn u16(&mut self, at: usize, v: u16) {
            self.blob[at..at + 2].copy_from_slice(&v.to_le_bytes());
        }

        fn i32(&mut self, at: usize, v: i32) {
            self.u32(at, v as u32);
        }

        fn i16(&mut self, at: usize, v: i16) {
            self.u16(at, v as u16);
        }

        fn u8(&mut self, at: usize, v: u8) {
            self.blob[at] = v;
        }

        /// One type entry keyed by `key`, pointing at one group at `GROUP`
        /// with `sorted` rows, `indices` and `listed`, a bundle GUID and the
        /// type's handle array at `HANDLES`.
        fn one_type(key: i32, sorted: &[u64], indices: &[u16], listed: i16) -> Dir {
            let mut d = Dir::new();
            d.i32(Dir::DIR + 0x20, 1);
            d.u64(Dir::DIR + 0x18, Dir::TYPE as u64);
            d.i32(Dir::TYPE, key);
            d.u64(Dir::TYPE + 8, Dir::BLOCK as u64);
            d.u64(Dir::TYPE + 0x10, Dir::HANDLES as u64);
            d.u64(Dir::BLOCK, Dir::GROUP as u64);
            d.u32(Dir::BLOCK + 8, 0x28);
            d.u16(Dir::BLOCK + 0x0c, 1);
            d.u16(Dir::BLOCK + 0x0e, 0x9c);
            d.u64(Dir::GROUP, Dir::LISTED as u64);
            d.u64(Dir::GROUP + 8, Dir::SORTED as u64);
            d.u64(Dir::GROUP + 0x10, Dir::INDEX as u64);
            d.u64(Dir::GROUP + 0x18, 0);
            d.i32(Dir::GROUP + 0x20, sorted.len() as i32);
            d.i16(Dir::GROUP + 0x24, listed);
            for (i, &guid) in sorted.iter().enumerate() {
                d.u64(Dir::SORTED + i * 8, guid);
            }
            for (i, &index) in indices.iter().enumerate() {
                d.u16(Dir::INDEX + i * 2, index);
            }
            d
        }

        /// A handle-array row pointing at `OBJECT + n * 0x40`, with `tag` in
        /// the low six bits and the object's own tag dword.
        fn handle(&mut self, row: usize, n: usize, tag: u8, loaded: bool, status_bits: u32) {
            let object = Dir::OBJECT + n * 0x40;
            self.u64(
                Dir::HANDLES + row * 8,
                (object as u64) | u64::from(tag & 0x3f),
            );
            self.u32(object + 8, u32::from(tag & 0x3f) | status_bits);
            self.u8(object + 0x0a, u8::from(loaded));
        }

        /// The same row with the object's `+8` dword spelled out.
        fn handle_word(&mut self, row: usize, n: usize, tag: u8, dword: u32) {
            let object = Dir::OBJECT + n * 0x40;
            self.u64(
                Dir::HANDLES + row * 8,
                (object as u64) | u64::from(tag & 0x3f),
            );
            self.u32(object + 8, dword);
        }

        /// Two groups that both hold `G`, sharing one sorted/index array.
        fn two_group_dir(handles: usize) -> Dir {
            let mut d = Dir::one_type(guid_type_key(G), &[G], &[0], 1);
            d.u16(Dir::BLOCK + 0x0c, 2);
            let g1 = Dir::GROUP + 0x28;
            d.u64(g1, Dir::LISTED as u64);
            d.u64(g1 + 8, Dir::SORTED as u64);
            d.u64(g1 + 0x10, Dir::INDEX as u64);
            d.u64(g1 + 0x18, 0);
            d.i32(g1 + 0x20, 1);
            d.i16(g1 + 0x24, 1);
            let _ = handles;
            d
        }

        fn lookup(&self, guid: u64) -> u64 {
            directory_lookup(
                &Directory::new(&self.blob, 0, Dir::DIR as u64),
                guid,
                |_| true,
            )
        }
    }

    const G: u64 = 0x0D90_847B_FC92_C67C;

    #[test]
    fn the_type_key_is_the_asset_type_id() {
        // A type-0x09C bundle GUID keys to 0x09C. The top nibble of the type
        // word is part of the input: `0xA72C...` keys to 847, which a mask
        // that dropped GUID bits 60..63 would miss.
        assert_eq!(guid_type_key(G), 0x09c);
        assert_eq!(guid_type_key(0xA72C_BBE1_3BBD_4F5C), 847);
        assert_eq!(guid_type_key(0), 1);
        for guid in [G, 0xA72C_BBE1_3BBD_4F5C, 0, 0x1234_5678_9ABC_DEF0] {
            assert_eq!(guid_type_key(guid), i32::from(crate::guid::type_of(guid)));
        }
        // Only GUID bits 48..59 reach the (post-shift) input.
        assert_eq!(guid_type_key(G + 1), guid_type_key(G));
        assert_eq!(guid_type_key(G ^ (1 << 20)), guid_type_key(G));
        // Field 0xFFF is the one value where the client's 12-bit sign
        // extension and `type_of`'s untruncated `+ 1` differ.
        let all_ones = 0x0FFF_u64 << 48;
        assert_eq!(guid_type_key(all_ones), 0);
        assert_eq!(crate::guid::type_of(all_ones), 0x1000);
    }

    #[test]
    fn directory_finds_the_handle_of_a_loaded_group() {
        let mut d = Dir::one_type(guid_type_key(G), &[G], &[0], 1);
        d.handle(0, 0, 0x21, true, 0);
        assert_eq!(d.lookup(G), (Dir::OBJECT as u64) | 0x21);
        assert_eq!(d.lookup(G + 1), u64::MAX);
        assert_eq!(d.lookup(G - 1), u64::MAX);
    }

    #[test]
    fn directory_searches_first_middle_and_last() {
        let rows: Vec<u64> = (0..8).map(|i| G + i * 0x10).collect();
        let mut d = Dir::one_type(guid_type_key(G), &rows, &[0, 1, 2, 3, 4, 5, 6, 7], 8);
        for row in 0..8 {
            d.handle(row, row, 0x21, true, 0);
        }
        let base = Dir::OBJECT as u64;
        assert_eq!(d.lookup(rows[0]), base | 0x21);
        assert_eq!(d.lookup(rows[3]), (base + 3 * 0x40) | 0x21);
        assert_eq!(d.lookup(rows[7]), (base + 7 * 0x40) | 0x21);
        assert_eq!(d.lookup(rows[0] - 1), u64::MAX);
        assert_eq!(d.lookup(rows[3] + 1), u64::MAX);
        assert_eq!(d.lookup(rows[7] + 1), u64::MAX);
    }

    #[test]
    fn directory_lower_bound_uses_the_first_duplicate() {
        // Rows 0 and 1 are both G; the lower bound is row 0, whose index
        // array entry names handle row 2.
        let mut d = Dir::one_type(guid_type_key(G), &[G, G, G + 0x10], &[2, 1, 0], 3);
        d.handle(0, 0, 0x21, true, 0);
        d.handle(1, 1, 0x22, true, 0);
        d.handle(2, 2, 0x23, true, 0);
        assert_eq!(d.lookup(G), ((Dir::OBJECT as u64) + 2 * 0x40) | 0x23);
        assert_eq!(d.lookup(G + 0x10), (Dir::OBJECT as u64) | 0x21);
    }

    #[test]
    fn directory_index_runs_across_the_groups_of_a_type() {
        // Group 0 lists two GUIDs and misses; group 1's hit names listed row
        // 0, which is handle row 2 of the type's array.
        let mut d = Dir::one_type(guid_type_key(G), &[G], &[0], 1);
        d.u64(Dir::BLOCK, Dir::GROUP as u64);
        d.u16(Dir::BLOCK + 0x0c, 2);
        // Group 1 sits at GROUP + 0x28 and reuses the same sorted/index arrays.
        let g1 = Dir::GROUP + 0x28;
        d.u64(g1, Dir::LISTED as u64);
        d.u64(g1 + 8, Dir::SORTED as u64);
        d.u64(g1 + 0x10, Dir::INDEX as u64);
        d.u64(g1 + 0x18, 0);
        d.i32(g1 + 0x20, 1);
        d.i16(g1 + 0x24, 1);
        d.i16(Dir::GROUP + 0x24, 2);
        d.u64(Dir::SORTED, G + 0x10);
        d.u16(Dir::INDEX, 0);
        d.handle(2, 2, 0x21, true, 0);
        assert_eq!(d.lookup(G + 0x10), ((Dir::OBJECT as u64) + 2 * 0x40) | 0x21);
    }

    #[test]
    fn directory_keeps_the_first_unready_handle_and_takes_a_loaded_one() {
        // Group 0's handle is known but not ready (status 2); group 1's is
        // loaded (status 4) and wins.
        let mut d = Dir::one_type(guid_type_key(G), &[G], &[0], 1);
        d.u16(Dir::BLOCK + 0x0c, 2);
        let g1 = Dir::GROUP + 0x28;
        d.u64(g1, Dir::LISTED as u64);
        d.u64(g1 + 8, Dir::SORTED as u64);
        d.u64(g1 + 0x10, Dir::INDEX as u64);
        d.u64(g1 + 0x18, 0);
        d.i32(g1 + 0x20, 1);
        d.i16(g1 + 0x24, 1);
        d.i16(Dir::GROUP + 0x24, 1);
        d.handle(0, 0, 0x21, false, 0);
        d.handle(1, 1, 0x22, true, 0);
        assert_eq!(d.lookup(G), ((Dir::OBJECT as u64) + 0x40) | 0x22);

        // With no loaded group the first unready handle is returned.
        let mut d = Dir::one_type(guid_type_key(G), &[G], &[0], 1);
        d.u16(Dir::BLOCK + 0x0c, 2);
        d.u64(g1, Dir::LISTED as u64);
        d.u64(g1 + 8, Dir::SORTED as u64);
        d.u64(g1 + 0x10, Dir::INDEX as u64);
        d.u64(g1 + 0x18, 0);
        d.i32(g1 + 0x20, 1);
        d.i16(g1 + 0x24, 1);
        d.i16(Dir::GROUP + 0x24, 1);
        d.handle(0, 0, 0x21, false, 0);
        d.handle(1, 1, 0x22, false, 0);
        assert_eq!(d.lookup(G), (Dir::OBJECT as u64) | 0x21);
    }

    #[test]
    fn directory_returns_a_status_three_handle_only_when_nothing_loads() {
        let mut d = Dir::one_type(guid_type_key(G), &[G], &[0], 1);
        d.handle(0, 0, 0x21, true, 1 << 15);
        assert_eq!(d.lookup(G), (Dir::OBJECT as u64) | 0x21);
    }

    #[test]
    fn directory_readiness_is_obj_plus_a_bit_0_and_obj_plus_8_bit_15() {
        // FUN_141fd4040 disassembles as `SHR EAX,0xf` then `AND AL,1`, so
        // only bit 15 of obj+8 is read; the decompiler's `& 0xffffff01`
        // would also have made bits 23 and 31 status bits, and the unit's
        // first run caught that on a bit-31 object (the client said 4).
        for bits in [1 << 23, 1 << 31] {
            let mut d = Dir::one_type(guid_type_key(G), &[G], &[0], 1);
            d.handle(0, 0, 0x21, true, bits);
            assert_eq!(d.lookup(G), (Dir::OBJECT as u64) | 0x21);
        }
        // The first group's handle is status 3 (bit 15 set); a later loaded
        // group wins. If bit 15 had been missed the first would have won.
        let mut d = Dir::two_group_dir(2);
        d.handle(0, 0, 0x21, true, 1 << 15);
        d.handle(1, 1, 0x22, true, 0);
        assert_eq!(d.lookup(G), ((Dir::OBJECT as u64) + 0x40) | 0x22);
        // obj+0xa bit 0 clear is status 2 even when the byte's other bits are
        // set: a wider mask would call it loaded and return it at once.
        let mut d = Dir::two_group_dir(2);
        d.handle(0, 0, 0x21, false, 0);
        d.u8(Dir::OBJECT + 0x0a, 2);
        d.handle(1, 1, 0x22, true, 0);
        assert_eq!(d.lookup(G), ((Dir::OBJECT as u64) + 0x40) | 0x22);
        // obj+8 is tested for its low six bits: bit 6 is not a tag bit, so
        // this handle is loaded (status 4) and returns before group 1.
        let mut d = Dir::two_group_dir(2);
        d.handle_word(0, 0, 0x21, 0x21 | 0x40 | 0x1_0000);
        d.handle(1, 1, 0x22, true, 0);
        assert_eq!(d.lookup(G), (Dir::OBJECT as u64) | 0x21);
    }

    #[test]
    fn directory_stores_an_unusable_handle_the_client_would() {
        // A tag mismatch fails FUN_141fd3970 but the client still records the
        // handle in the output when it is the first one.
        let mut d = Dir::one_type(guid_type_key(G), &[G], &[0], 1);
        d.u64(Dir::HANDLES, (Dir::OBJECT as u64) | 0x21);
        d.u32(Dir::OBJECT + 8, 0xffff_ffe2);
        d.u8(Dir::OBJECT + 0x0a, 1);
        assert_eq!(d.lookup(G), (Dir::OBJECT as u64) | 0x21);

        // A handle of -1 is the client's absent value and stays absent.
        let mut d = Dir::one_type(guid_type_key(G), &[G], &[0], 1);
        d.u64(Dir::HANDLES, u64::MAX);
        assert_eq!(d.lookup(G), u64::MAX);
    }

    #[test]
    fn directory_takes_the_clients_walk_on_unsorted_rows() {
        // [G+0x10, G] looking for G+0x10 examines no row and reads row 0: a
        // hit, as in Bundle::offset_of. Looking for G reads row 0 and misses.
        // Both GUIDs hash to the type's key.
        let mut d = Dir::one_type(guid_type_key(G), &[G + 0x10, G], &[0, 1], 2);
        d.handle(0, 0, 0x21, true, 0);
        d.handle(1, 1, 0x22, true, 0);
        assert_eq!(d.lookup(G + 0x10), (Dir::OBJECT as u64) | 0x21);
        assert_eq!(d.lookup(G), u64::MAX);
    }

    #[test]
    fn directory_edges_are_the_clients() {
        // No types.
        let d = Dir::new();
        assert_eq!(d.lookup(G), u64::MAX);
        // A key that is not listed.
        let d = Dir::one_type(guid_type_key(G) + 1, &[G], &[0], 1);
        assert_eq!(d.lookup(G), u64::MAX);
        // A type with no groups.
        let mut d = Dir::one_type(guid_type_key(G), &[G], &[0], 1);
        d.u16(Dir::BLOCK + 0x0c, 0);
        assert_eq!(d.lookup(G), u64::MAX);
        // A loaded group with an empty sorted array, then a hit in group 1.
        let mut d = Dir::one_type(guid_type_key(G), &[], &[], 0);
        d.u16(Dir::BLOCK + 0x0c, 2);
        let g1 = Dir::GROUP + 0x28;
        d.u64(g1, Dir::LISTED as u64);
        d.u64(g1 + 8, Dir::SORTED as u64);
        d.u64(g1 + 0x10, Dir::INDEX as u64);
        d.u64(g1 + 0x18, 0);
        d.i32(g1 + 0x20, 1);
        d.i16(g1 + 0x24, 1);
        d.u64(Dir::SORTED, G);
        d.u16(Dir::INDEX, 0);
        d.handle(0, 0, 0x21, true, 0);
        assert_eq!(d.lookup(G), (Dir::OBJECT as u64) | 0x21);
        // One row (no search loop) hit and miss.
        let mut d = Dir::one_type(guid_type_key(G), &[G], &[0], 1);
        d.handle(0, 0, 0x21, true, 0);
        assert_eq!(d.lookup(G), (Dir::OBJECT as u64) | 0x21);
        assert_eq!(d.lookup(0), u64::MAX);
        // A pointer outside the blob is reported, not read.
        let d = Dir::one_type(guid_type_key(G), &[G], &[0], 1);
        let mut d = d;
        d.u64(Dir::TYPE + 8, 0x8000);
        assert_eq!(d.lookup(G), u64::MAX);
    }

    #[test]
    fn directory_honours_the_loaded_query() {
        let mut d = Dir::one_type(guid_type_key(G), &[G], &[0], 1);
        d.u64(Dir::GROUP + 0x18, 0x1234);
        d.handle(0, 0, 0x21, true, 0);
        let dir = Directory::new(&d.blob, 0, Dir::DIR as u64);
        assert_eq!(
            directory_lookup(&dir, G, |_| true),
            (Dir::OBJECT as u64) | 0x21
        );
        assert_eq!(directory_lookup(&dir, G, |_| false), u64::MAX);
        assert_eq!(
            directory_lookup(&dir, G, |bundle| bundle == 0x1234),
            (Dir::OBJECT as u64) | 0x21
        );
    }

    #[test]
    fn index_keeps_the_first_bundle_and_skips_absent_rows() {
        let a = bundle(2, &[(0x10, 0x20), (0x20, 0)]);
        let b = bundle(1, &[(0x10, 0x30), (0x40, 0x50)]);
        let index = BundleIndex::build([1u64, 2].into_iter(), |guid| {
            Some(if guid == 1 { a.clone() } else { b.clone() })
        });
        assert_eq!(index.bundles, 2);
        assert_eq!(index.entries_seen, 4);
        assert_eq!(index.len(), 2);
        assert_eq!(
            index.get(0x10),
            Some(Location {
                bundle: 1,
                offset: 0x20
            })
        );
        assert_eq!(index.get(0x20), None);
        assert_eq!(
            index.get(0x40),
            Some(Location {
                bundle: 2,
                offset: 0x50
            })
        );
    }
}

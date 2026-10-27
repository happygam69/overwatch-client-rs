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
    /// Same search as the client's: a lower-bound binary search over the
    /// GUID-sorted table, then an equality check. Offset `0` is the client's
    /// "absent" value and is reported as `None` here too.
    pub fn offset_of(&self, guid: u64) -> Option<u32> {
        let mut lo = 0usize;
        let mut hi = self.count;
        while lo < hi {
            let mid = (lo + hi) / 2;
            let (at, _) = self.entry(mid)?;
            if at < guid {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        let (found, offset) = self.entry(lo)?;
        if found == guid && offset != 0 {
            Some(offset)
        } else {
            None
        }
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

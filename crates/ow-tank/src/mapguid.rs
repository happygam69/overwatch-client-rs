//! Map chunk GUIDs.
//!
//! A map asset does not list its chunk assets. The client synthesises each
//! chunk GUID in `FUN_141b32e20` (`0x141b32e20`):
//!
//! ```text
//! guid = 0x0DD0000000000000
//!      | (chunkKind[typeIndex] << 32)
//!      | (mapGuid & 0xFFFFFFFF)
//! ```
//!
//! `chunkKind[]` is the 0x24-entry table at `0x1439aa320` (see
//! [`crate::keytable::Sources::chunk_kinds`]), stride 0x20, the kind at `+8`.
//! The map loader `FUN_141ff5c80` (`0x141ff5c80`) calls the formula for type
//! indices `1..=0x1C`, then `0x1F`, `0x20` and `0x23` — 31 chunk GUIDs.
//!
//! Those 31 slots are **not every chunk a map ships**. On this install the
//! chunk kinds `0x10`, `0x11`, `0x12` and `0x23` sit at table slots `0x21`,
//! `0x1D`, `0x1E` and `0x22`, outside the loader's set, and the map asset
//! stores their GUIDs in four fields the same loader reads
//! (`+0x1B8`, the `+0x1B0` list, `+0x1C0`, `+0x1C8`). See
//! [`crate::mapresolve`]; the practice range's two collision chunks are stored
//! at `+0x1B8`/`+0x1C0`.
//!
//! A map ships only the chunks it uses; the rest of the 31 are simply not in
//! the manifest. That is why a chunk list is the client's set for one map GUID
//! and never a proximity list: the GUID is the map's index under a kind.
//!
//! `docs/formats/maps.md` holds the proof against the install.

/// The type bits' fixed prefix in every synthesised chunk GUID.
pub const CHUNK_GUID_BASE: u64 = 0x0DD0_0000_0000_0000;

/// The type indices the map loader asks for, in the loader's order.
///
/// `FUN_141ff5c80` issues `0x1F`, `0x20` and `0x23` first, then the loop
/// `1..0x1D`; the set is what matters, not the order.
pub fn loader_type_indices() -> impl Iterator<Item = usize> {
    (1..=0x1c).chain([0x1f, 0x20, 0x23])
}

/// The table slot whose kind is `kind` — the inverse the client uses in
/// `FUN_141b32e70` (`0x141b32e70`): a chunk GUID's `(guid >> 32) & 0xFF` byte
/// names a kind, and the slot is the entry whose `+8` holds it.
pub fn slot_of_kind(kinds: &[u64], kind: u64) -> Option<usize> {
    kinds.iter().position(|k| *k == kind)
}

/// One chunk GUID: the map's low 32 bits under `kind`.
pub fn chunk_guid(map: u64, kind: u64) -> u64 {
    CHUNK_GUID_BASE | ((kind & 0xFFFF) << 32) | (map & 0xFFFF_FFFF)
}

/// The chunk GUIDs the map loader asks for, in type-index order.
///
/// A type index the table does not cover is skipped: the loader only calls the
/// formula after checking its index against the table's entry count.
pub fn chunk_guids(map: u64, kinds: &[u64]) -> Vec<u64> {
    loader_type_indices()
        .filter_map(|index| kinds.get(index).map(|kind| chunk_guid(map, *kind)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_formula_keeps_the_map_index() {
        // The practice range's map asset is 0x0800000000000688; its registered
        // chunks carry kinds 0x11 and 0x12 (see docs/formats/maps.md).
        let map = 0x0800_0000_0000_0688u64;
        assert_eq!(chunk_guid(map, 0x11), 0x0DD0_0011_0000_0688);
        assert_eq!(chunk_guid(map, 0x12), 0x0DD0_0012_0000_0688);
        // The reference map the client's table was proven on.
        assert_eq!(
            chunk_guid(0x0800_0000_00F4_7508, 0x01),
            0x0DD0_0001_00F4_7508
        );
    }

    #[test]
    fn the_loader_asks_for_thirty_one_indices() {
        let indices: Vec<usize> = loader_type_indices().collect();
        assert_eq!(indices.len(), 31);
        assert_eq!(indices.first(), Some(&1));
        assert_eq!(indices.last(), Some(&0x23));
        assert!(indices.contains(&0x1c));
        assert!(!indices.contains(&0x1d));
        assert_eq!(
            indices
                .iter()
                .filter(|i| **i >= 0x1f)
                .copied()
                .collect::<Vec<_>>(),
            vec![0x1f, 0x20, 0x23]
        );
    }

    #[test]
    fn a_short_kind_table_produces_only_covered_indices() {
        let kinds = [0u64, 1, 2, 3];
        let guids = chunk_guids(0x0800_0000_0000_0688, &kinds);
        assert_eq!(guids.len(), 3, "indices 1, 2 and 3 are covered");
        assert_eq!(
            guids,
            vec![
                0x0DD0_0001_0000_0688,
                0x0DD0_0002_0000_0688,
                0x0DD0_0003_0000_0688
            ]
        );
    }
}

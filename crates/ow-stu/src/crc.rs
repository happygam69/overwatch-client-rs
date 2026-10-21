//! CRC64 as Overwatch's asset layer uses it.
//!
//! Reflected, initial and final values inverted, over the polynomial that
//! reproduces overtools/TACTLib's published table exactly. Rebuilding the table
//! from the polynomial rather than copying 256 constants keeps the provenance
//! of this file to the algorithm alone.
//!
//! STU asset references are mixed with a CRC64 of the asset's first 36 bytes,
//! so a wrong implementation produces GUIDs that resolve to nothing.

/// Reflected form of the polynomial, derived from the published table.
const POLY_REFLECTED: u64 = 0x95AC_9329_AC4B_C9B5;

/// The 256-entry table, computed once.
fn table() -> &'static [u64; 256] {
    use std::sync::OnceLock;
    static TABLE: OnceLock<[u64; 256]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = [0u64; 256];
        for (i, slot) in table.iter_mut().enumerate() {
            let mut c = i as u64;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    (c >> 1) ^ POLY_REFLECTED
                } else {
                    c >> 1
                };
            }
            *slot = c;
        }
        table
    })
}

/// CRC64 with an initial value of zero, inverted on the way out.
///
/// Zero here matches the callers in the asset layer, which pass an all-ones
/// seed that the routine complements before use.
pub fn crc64(data: &[u8]) -> u64 {
    let table = table();
    let mut crc: u64 = 0;
    for byte in data {
        crc = (crc >> 8) ^ table[((crc ^ u64::from(*byte)) & 0xFF) as usize];
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_internally_consistent() {
        // The first entry is always zero for a reflected table.
        assert_eq!(table()[0], 0);
        // And the table must not be the identity.
        assert_ne!(table()[1], 1);
    }

    #[test]
    fn matches_known_published_entries() {
        // Spot-check against two constants published in TACTLib's CRC64 table,
        // which the polynomial is derived from.
        let t = table();
        assert_eq!(t[1], 0x7AD8_70C8_3035_8979);
        assert_eq!(t[2], 0xF5B0_E190_606B_12F2);
    }
}

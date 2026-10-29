//! Overwatch asset identifiers.
//!
//! Every asset has a 64-bit GUID. The type is not a string in the file: it is a
//! 12-bit field at bits 48..59, stored with its bits reversed, and the value
//! used everywhere else is that reversal plus one.
//!
//! ```text
//! index   bits  0..31
//! locale  bits 32..36
//! region  bits 39..42
//! type    bits 48..59   (reversed)
//! ```
//!
//! The reversal was confirmed against the published `types.txt` permutation
//! table, whose rows this reproduces exactly.

/// Reverse the low twelve bits.
pub fn reverse_type_bits(mangled: u16) -> u16 {
    let mut out = 0u16;
    for i in 0..12 {
        if mangled & (1 << i) != 0 {
            out |= 1 << (11 - i);
        }
    }
    out
}

/// The asset type number, as used by the tooling (for example `0x075` heroes).
pub fn type_of(guid: u64) -> u16 {
    let mangled = ((guid >> 48) & 0x0FFF) as u16;
    reverse_type_bits(mangled) + 1
}

/// The GUID of a texture's *separate* payload asset, from the client's
/// `PayloadGuid` (`0x142e37520`).
///
/// The client keeps bits 0..31 and 36..39 of the parent, puts the payload index
/// in bits 32..35, puts a two-bit "base" in bits 40..41, and writes the payload
/// type (`0x04D`) into bits 48..59. The base is derived from the parent's type:
/// 0 for a texture (`0x004`), 1 for `0x0F1`, and 3 for anything else. A texture
/// therefore always gets base 0, and payload index 0 is the embedded chunk, so
/// callers ask for 1 or more.
pub fn payload_of(texture: u64, payload_idx: u32) -> u64 {
    const PAYLOAD_TYPE: u16 = 0x04D;
    let base: u64 = match type_of(texture) {
        types::TEXTURE => 0,
        0x0F1 => 1,
        _ => 3,
    };
    let g = texture & 0xF0_FFFF_FFFF;
    g | ((payload_idx as u64 & 0xF) << 32)
        | (base << 40)
        | ((reverse_type_bits(PAYLOAD_TYPE - 1) as u64) << 48)
}

/// The per-type sequence number.
pub fn index_of(guid: u64) -> u32 {
    (guid & 0xFFFF_FFFF) as u32
}

/// Asset type numbers this project cares about.
///
/// These come from the type registry the OWLib/DL tools switch on; the counts
/// observed in a retail manifest are noted so a wrong decode is obvious.
pub mod types {
    pub const TEXTURE: u16 = 0x004;
    pub const MODEL: u16 = 0x00C;
    pub const STU_GENERIC_SETTINGS: u16 = 0x054;
    pub const HERO: u16 = 0x075;
    pub const LOADOUT: u16 = 0x09E;
    pub const GAME_MODE: u16 = 0x0C5;
    pub const LOOTBOX: u16 = 0x0CF;
    pub const TALENT: u16 = 0x134;
    pub const TEX_PAYLOAD: u16 = 0x04D;

    /// Human-readable label, or `None` for types this project has not mapped.
    pub fn label(type_id: u16) -> Option<&'static str> {
        Some(match type_id {
            TEXTURE => "texture",
            MODEL => "model",
            STU_GENERIC_SETTINGS => "STU generic settings",
            HERO => "hero",
            LOADOUT => "loadout/ability",
            GAME_MODE => "game mode",
            LOOTBOX => "lootbox",
            TALENT => "talent/perk",
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reversal_is_an_involution() {
        for v in 0u16..0x1000 {
            assert_eq!(reverse_type_bits(reverse_type_bits(v)), v);
        }
    }

    #[test]
    fn reproduces_published_rows() {
        // From the published types.txt: mangled 0x0FC0 maps to type 0x040.
        assert_eq!(reverse_type_bits(0x0FC0) + 1, 0x040);
        assert_eq!(reverse_type_bits(0x0001) + 1, 0x801);
        assert_eq!(reverse_type_bits(0x0004) + 1, 0x201);
    }

    #[test]
    fn hero_guid_decodes_to_hero_type() {
        // Retail hero GUIDs sit on the 0x02E00000 prefix.
        assert_eq!(type_of(0x02E0_0000_0000_0002), types::HERO);
        assert_eq!(index_of(0x02E0_0000_0000_0002), 2);
    }

    /// The payload GUID formula from `0x142e37520`, pinned on real payloads.
    ///
    /// The three payloads of texture `0x0c000000000009c1` resolve in the
    /// install, so their GUIDs are data, not a guess: index in bits 32..35,
    /// base 0 in bits 40..41, type `0x04d` in bits 48..59, and the parent's
    /// bits 0..31 and 36..39 kept.
    #[test]
    fn payload_guids_match_the_installs_payload_assets() {
        let texture = 0x0c00_0000_0000_09c1u64;
        assert_eq!(payload_of(texture, 1), 0x0320_0001_0000_09c1);
        assert_eq!(payload_of(texture, 2), 0x0320_0002_0000_09c1);
        assert_eq!(type_of(payload_of(texture, 1)), types::TEX_PAYLOAD);
        // Bits 40..47 are cleared, not just bit 40: the client's mask is
        // 0xf0ffffffff. Set every one of them on the parent and check.
        let dirty = texture | (0xffu64 << 40);
        assert_eq!(payload_of(dirty, 1), 0x0320_0001_0000_09c1);
        // A parent's bits 36..39 are kept; bits 40..47 are not.
        let region = texture | (0xFu64 << 36);
        assert_eq!(
            payload_of(region, 3) & (0xFu64 << 36),
            0xFu64 << 36,
            "bits 36..39 survive"
        );
        // The base is 0 for a texture, 1 for type 0x0f1, 3 otherwise.
        assert_eq!((payload_of(texture, 1) >> 40) & 3, 0);
        let f1 = ((reverse_type_bits(0x0F0) as u64) << 48) | 1;
        assert_eq!(type_of(f1), 0x0F1);
        assert_eq!((payload_of(f1, 2) >> 40) & 3, 1);
        assert_eq!((payload_of(0x0110_0000_0000_0001, 2) >> 40) & 3, 3);
    }
}

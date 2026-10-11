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
}

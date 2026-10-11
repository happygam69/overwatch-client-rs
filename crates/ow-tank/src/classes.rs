//! Instance type hashes for the STU classes this project reads.
//!
//! An STU instance is identified by a 32-bit hash of its class, not by a name.
//! A name table for the whole format is community-maintained and not something
//! this project ships; the handful of classes below were verified against a
//! retail install by opening the assets and reading fields whose meaning is
//! independently checked (see `docs/formats/stu.md`).
//!
//! When a user points `OW_TACTLIB` at a checkout of the reference library, the
//! full generated class list is read from it and those names are used for
//! diagnostics. The hashes below stay authoritative for the classes the code
//! depends on, so behaviour does not change with what happens to be installed.

use crate::keytable::Sources;
use std::collections::HashMap;
use std::path::PathBuf;

/// `STUHero`, the hero definition.
pub const HERO: u32 = 0x7C34_57DC;
/// `STULoadout`, an ability or weapon entry shown to players.
pub const LOADOUT: u32 = 0x3229_0C2E;
/// `STUWeaponDefinition`, whose behaviour is a state-script graph.
pub const WEAPON_DEFINITION: u32 = 0x6138_6B75;
/// `STUHealthComponent`.
pub const HEALTH_COMPONENT: u32 = 0xA658_CA1C;

/// Field hashes this project reads.
pub mod hero {
    /// `m_gameplayEntity`: asset reference to the hero's gameplay entity.
    pub const GAMEPLAY_ENTITY: u32 = 0x950B_BA06;
    /// `m_heroColor`: theme colour.
    pub const HERO_COLOR: u32 = 0xE25D_DDA1;
    /// `m_heroSize`: `STUHeroSize` enum.
    pub const HERO_SIZE: u32 = 0xAF4E_C410;
    /// `m_gender`.
    pub const GENDER: u32 = 0x7D88_A63A;
    /// `m_heroLoadout`: array of loadout references.
    pub const HERO_LOADOUT: u32 = 0x77FE_D604;
}

/// Type-hash to class-name map.
#[derive(Debug, Default)]
pub struct ClassMap {
    names: HashMap<u32, String>,
}

impl ClassMap {
    /// The built-in, verified subset.
    pub fn builtin() -> ClassMap {
        let mut names = HashMap::new();
        names.insert(HERO, "STUHero".to_string());
        names.insert(LOADOUT, "STULoadout".to_string());
        names.insert(WEAPON_DEFINITION, "STUWeaponDefinition".to_string());
        names.insert(HEALTH_COMPONENT, "STUHealthComponent".to_string());
        ClassMap { names }
    }

    /// Add class names from a reference-library checkout.
    ///
    /// The generated sources carry a `[STU(0xHASH, size)]` attribute per class,
    /// which is enough to recover the mapping without any game data. Names are
    /// diagnostics only: nothing in this crate branches on them, and a class the
    /// table does not reach still parses.
    pub fn load_from_checkouts(&mut self, sources: &Sources) {
        for dir in sources.class_source_dirs() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path: PathBuf = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("cs") {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let Some(hash) = parse_stu_attribute(&text) else {
                    continue;
                };
                // The built-in, verified entries win over anything discovered.
                if self.names.contains_key(&hash) {
                    continue;
                }
                if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                    self.names.insert(hash, name.to_string());
                }
            }
        }
    }

    pub fn name(&self, hash: u32) -> Option<&str> {
        self.names.get(&hash).map(String::as_str)
    }

    pub fn describe(&self, hash: u32) -> String {
        match self.names.get(&hash) {
            Some(name) => format!("{name} (0x{hash:08X})"),
            None => format!("STU_0x{hash:08X}"),
        }
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

/// Read the hash out of a `[STU(0xHASH, size)]` attribute.
pub fn parse_stu_attribute(source: &str) -> Option<u32> {
    let at = source.find("[STU(0x")?;
    let rest = &source[at + "[STU(0x".len()..];
    let end = rest.find(|c: char| !c.is_ascii_hexdigit())?;
    u32::from_str_radix(&rest[..end], 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_stu_attribute() {
        let source = "
            namespace TankLib.STU.Types {
                [STU(0x61386B75, 56)]
                public class STUWeaponDefinition : STUInstance { }
            }";
        assert_eq!(parse_stu_attribute(source), Some(0x6138_6B75));
    }

    #[test]
    fn builtin_map_covers_what_the_code_reads() {
        let map = ClassMap::builtin();
        assert_eq!(map.name(HERO), Some("STUHero"));
        assert_eq!(map.describe(0xDEAD_BEEF), "STU_0xDEADBEEF");
    }
}

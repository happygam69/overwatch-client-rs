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
/// `STUGenericSettings_HeroSettings`, the root of the per-hero tuning set.
pub const HERO_SETTINGS: u32 = 0x6021_C7EC;
/// `STUHeroSpecificSettings`, one hero's slice of that set.
pub const HERO_SPECIFIC_SETTINGS: u32 = 0x24DF_4688;
/// `STUHeroSettingFloat`, a bounded float a player can tune.
pub const HERO_SETTING_FLOAT: u32 = 0x76B1_DB1F;
/// `STUHeroSettingIdentifier`, a choice from a fixed set.
pub const HERO_SETTING_IDENTIFIER: u32 = 0x90A5_0AC1;
/// `STUHeroSettingCategory`, a group heading in the tuning UI.
pub const HERO_SETTING_CATEGORY: u32 = 0xB87D_57AA;
/// `STUStatescriptSchemaEntry`, one `(token, value)` pair of an entity's schema.
pub const STATESCRIPT_SCHEMA_ENTRY: u32 = 0x5866_C45E;
/// `STUConfigVarFloat`, a literal float.
pub const CONFIG_VAR_FLOAT: u32 = 0x7B45_B325;
/// `STUConfigVarInt`, a literal integer.
pub const CONFIG_VAR_INT: u32 = 0x12E4_34A1;
/// `STUConfigVarBool`, a literal boolean.
pub const CONFIG_VAR_BOOL: u32 = 0xB567_AADA;
/// `STUWeaponComponent`, which lists an entity's weapons.
pub const WEAPON_COMPONENT: u32 = 0x15CB_6532;
/// `STUStatescriptGraph`, the node graph a weapon's behaviour lives in.
pub const STATESCRIPT_GRAPH: u32 = 0x7236_F6E3;

/// `STUCharacterMoverComponent`, the per-hero movement component. The runtime
/// slots of its scalar fields sit at their STU offsets plus 4, as read back from
/// the client's own registration (see `docs/formats/character-controller.md`).
pub const CHARACTER_MOVER_COMPONENT: u32 = 0x6EFD_BD73;

/// Fields of the character mover component.
pub mod character_mover {
    /// The per-hero travel-speed figure (`m_031FE7EE`), a float at STU offset
    /// 220 / runtime slot 0xe0. The slot pairing is confirmed from the client's
    /// own registration; the *role* reads back from hero values, not from a name
    /// table anywhere in the capture (see `docs/formats/character-controller.md`).
    pub const SPEED: u32 = 0x031F_E7EE;
}

/// Asset type of a statescript graph. Graphs are shipped and addressable, unlike
/// the token type, so a graph reference can be followed to a real asset.
pub const STATESCRIPT_GRAPH_ASSET_TYPE: u16 = 0x01B;

/// Fields of a weapon component.
pub mod weapon_component {
    /// `m_weapons`, an inline array of weapon definitions.
    pub const WEAPONS: u32 = 0x3BD1_6B9E;
    /// `m_managerScript`.
    pub const MANAGER_SCRIPT: u32 = 0xC63E_11DD;
}

/// Fields of a weapon definition.
pub mod weapon_definition {
    /// `m_graph`: the overrides wrapper, holding the graph reference.
    pub const GRAPH: u32 = 0xC71E_A6BC;
    /// `m_script`.
    pub const SCRIPT: u32 = 0x4D55_97A8;
}

/// Fields of a statescript schema entry.
pub mod schema_entry {
    /// The token naming which property the entry sets.
    pub const TOKEN: u32 = 0x0D09_D2D9;
}

/// `STUStatescriptStateWeaponVolley`, the node that fires a weapon. The hash is
/// confirmed against the client's own class registration in the captures; see
/// `docs/formats/weapons.md` § "The volley class, read from the captures".
pub const VOLLEY_STATE: u32 = 0xEED6_D952;
/// `STUStatescriptWeaponProjectileMotion`, one projectile-motion element of a
/// volley's `m_projectileMotions` array.
pub const PROJECTILE_MOTION: u32 = 0x487A_E227;
/// `STUStatescriptModifyHealth`, the data node that carries a health
/// modification (its `m_amount` is the figure found in the graphs).
pub const MODIFY_HEALTH: u32 = 0xA8BC_2948;
/// `STUConfigVarDynamic`, a config var whose value is evaluated at runtime
/// from an identifier rather than stored. Tracer's volley points both
/// `numProjectilesPerShot` and `numShotsPerSecond` at these.
pub const CONFIG_VAR_DYNAMIC: u32 = 0x4B09_3107;
/// A concrete `STUConfigVarExpression` (base `0xA7E04FB2`): the value is a
/// computed expression over operand vars (`m_expression` plus `m_configVars`),
/// so it carries no literal. Reaper's pellets and several heroes' rates point
/// here; see `docs/formats/weapons.md`.
pub const CONFIG_VAR_EXPRESSION: u32 = 0x919F_D47C;
/// A concrete subclass of `STUConfigVarFloatBase` (`0xF9C57875`) that adds two
/// config-var refs (a value source at `m_DBF61E9A`, an entity at `m_entity`)
/// and declares no own `m_value`, so it, too, is computed rather than stored.
/// Genji's fan volleys point their pellet count here.
pub const CONFIG_VAR_FLOAT_SOURCE: u32 = 0x082F_A44E;
/// The volley's `m_D2D11CE9` child (class `0x4AA6CA44`): an impact/effect
/// descriptor holding an explosion-ring array, about twenty config vars and
/// **two embedded `STUStatescriptModifyHealth` nodes** (`m_modifyHealth`,
/// `m_4DB7B249`). This is the volley-to-damage wiring; see
/// `docs/formats/weapons.md`.
pub const VOLLEY_IMPACT: u32 = 0x4AA6_CA44;

/// Well-known statescript field hashes the sim reads by value.
pub mod statescript {
    /// `numShotsPerSecond`, the fire-rate literal on volley states; named in
    /// the reference types, so its readings are name-backed.
    pub const NUM_SHOTS_PER_SECOND: u32 = 0x02F3_F5D4;
}

/// Fields of the volley state (`0xEED6D952`).
///
/// The trailing comment on each is the **runtime slot** the client's own
/// registrar assigns it, re-parsed from
/// `D:/ow-dumps/dumps_range/7ff64b73e000_00006000_00040000.bin` (registrar
/// entry at range `0x7ff64b73f190`, offsets stored as qwords at `[rsp+0x20]`).
/// For this class the runtime slot equals the STU offset; the run shifts +8
/// past `0x208` because the live class carries one field the reference
/// checkout predates. Slots are what a live-memory read of a volley object
/// needs; the field hashes are what the file-side join uses.
pub mod volley {
    /// `m_projectileVisual` — 0xf8.
    pub const PROJECTILE_VISUAL: u32 = 0x81F1_D90B;
    /// `m_aimID` — 0x100.
    pub const AIM_ID: u32 = 0xF980_8409;
    /// `m_projectileLifetime` — 0x108.
    pub const PROJECTILE_LIFETIME: u32 = 0xBA49_0EB9;
    /// `m_FEC435C6`, the cast-length candidate — 0x110. Unnamed in the
    /// reference types; the reading is evidence-based (`docs/formats/weapons.md`).
    pub const CAST_LENGTH: u32 = 0xFEC4_35C6;
    /// `m_projectileSpeed` — 0x120.
    pub const PROJECTILE_SPEED: u32 = 0x26E6_8021;
    /// `m_projectileMotions`, the inline array of motion elements. Registered
    /// before the visible window start, so no slot is claimed here.
    pub const PROJECTILE_MOTIONS: u32 = 0x351B_D93F;
    /// `m_projectileEntity`. Registered before the visible window start.
    pub const PROJECTILE_ENTITY: u32 = 0xBAA3_90E1;
    /// `m_numProjectilesPerShot` — 0x1b8.
    pub const NUM_PROJECTILES_PER_SHOT: u32 = 0xF2AE_0897;
    /// `m_numShotsPerSecond` — 0x1c0.
    pub const NUM_SHOTS_PER_SECOND: u32 = 0x02F3_F5D4;
    /// `m_A55BC904`, unnamed — 0x1c8.
    pub const A55BC904: u32 = 0xA55B_C904;
    /// `m_A17BE89B`, unnamed — 0x1d0.
    pub const A17BE89B: u32 = 0xA17B_E89B;
    /// `m_5EC1EB38`, the volley launch block (muzzles etc.) — 0x1f8.
    pub const LAUNCH: u32 = 0x5EC1_EB38;
    /// `m_D2D11CE9`, the impact/effect descriptor child (class
    /// [`super::VOLLEY_IMPACT`]). Present on only some volleys.
    pub const IMPACT: u32 = 0xD2D1_1CE9;
}

/// Fields of a config var whose value binds at runtime (`0x4B093107`).
pub mod config_var_dynamic {
    /// `m_identifier`: which runtime value the var evaluates to. The same
    /// name-hash as the hero-setting identifier field; the identifier domains
    /// are not established to be the same.
    pub const IDENTIFIER: u32 = 0x4D2D_B658;
}

/// Fields of a projectile-motion element (`0x487AE227`). Both are
/// `STUConfigVar`s; the reference types do not name them.
pub mod projectile_motion {
    /// `m_30A41C77` — candidate for speed (see `docs/formats/weapons.md`).
    pub const A_30A41C77: u32 = 0x30A4_1C77;
    /// `m_468EC3BE` — candidate for gravity/acceleration.
    pub const A_468EC3BE: u32 = 0x468E_C3BE;
}

/// Fields of the modify-health data node (`0xA8BC2948`).
pub mod modify_health {
    /// `m_amount`, a `STUConfigVar` holding the modification figure.
    pub const AMOUNT: u32 = 0xAF87_2E86;
}

/// Fields shared by the literal-valued config vars.
pub mod config_var {
    /// `m_value`, shared by `STUConfigVarFloat`, `STUConfigVarInt`,
    /// `STUConfigVarBool` and others, so the value is read the same way and the
    /// *class* decides how to interpret it.
    pub const VALUE: u32 = 0x07DD_813E;
}

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

/// Fields shared by every hero setting.
///
/// A setting only stores the fields that differ from its class default, so a
/// field missing from an instance is not zero — it is the default declared on
/// the class. [`HERO_SETTING_FLOAT_DEFAULTS`] carries the float ones.
pub mod hero_setting {
    /// `m_identifier`: a typed token, not an asset. No asset of the identifier
    /// type is shipped, so this names a setting rather than pointing at one.
    pub const IDENTIFIER: u32 = 0x4D2D_B658;
    /// `m_name`: display text.
    pub const NAME: u32 = 0xB48F_1D22;
    /// `m_category`.
    pub const CATEGORY: u32 = 0x2C54_AEAF;
}

/// Fields specific to a float-valued hero setting.
pub mod hero_setting_float {
    /// `m_default`, the value the tuning starts at.
    pub const DEFAULT: u32 = 0x45A7_A69C;
    /// `m_min`.
    pub const MIN: u32 = 0x7594_F80E;
    /// `m_max`.
    pub const MAX: u32 = 0x7CEA_65C7;
    /// The step the UI uses.
    pub const STEP: u32 = 0x9321_E2B9;
}

/// Class defaults for float-valued hero settings, which apply when the field is
/// absent from an instance.
///
/// `min` has no declared default, so an absent minimum is zero.
pub const HERO_SETTING_FLOAT_DEFAULTS: [(u32, f32); 4] = [
    (hero_setting_float::DEFAULT, 0.0),
    (hero_setting_float::MIN, 0.0),
    (hero_setting_float::MAX, 1.0),
    (hero_setting_float::STEP, 0.01),
];

/// Type-hash to class-name map.
#[derive(Debug, Default)]
pub struct ClassMap {
    names: HashMap<u32, String>,
}

impl ClassMap {
    /// The built-in, verified subset.
    pub fn builtin() -> ClassMap {
        let mut names = HashMap::new();
        for (hash, name) in [
            (HERO, "STUHero"),
            (LOADOUT, "STULoadout"),
            (WEAPON_DEFINITION, "STUWeaponDefinition"),
            (HEALTH_COMPONENT, "STUHealthComponent"),
            (HERO_SETTINGS, "STUGenericSettings_HeroSettings"),
            (HERO_SPECIFIC_SETTINGS, "STUHeroSpecificSettings"),
            (HERO_SETTING_FLOAT, "STUHeroSettingFloat"),
            (HERO_SETTING_IDENTIFIER, "STUHeroSettingIdentifier"),
            (HERO_SETTING_CATEGORY, "STUHeroSettingCategory"),
            (STATESCRIPT_SCHEMA_ENTRY, "STUStatescriptSchemaEntry"),
            (CONFIG_VAR_FLOAT, "STUConfigVarFloat"),
            (CONFIG_VAR_INT, "STUConfigVarInt"),
            (CONFIG_VAR_BOOL, "STUConfigVarBool"),
            (WEAPON_COMPONENT, "STUWeaponComponent"),
            (STATESCRIPT_GRAPH, "STUStatescriptGraph"),
            (WEAPON_COMPONENT, "STUWeaponComponent"),
            (0x6EFD_BD73, "STUCharacterMoverComponent"),
        ] {
            names.insert(hash, name.to_string());
        }
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

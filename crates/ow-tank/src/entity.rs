//! Reading a gameplay entity's schema: the first base combat numbers.
//!
//! A hero's `m_gameplayEntity` reference points at an entity asset. That asset
//! carries a list of *schema entries*, each a `(token, value)` pair:
//!
//! ```text
//! STUStatescriptSchemaEntry
//!   0x07DD813E  the value, an embedded ConfigVar holding a literal
//!   0x0D09D2D9  the token identifying which property this is
//! ```
//!
//! The token is stored in the same obfuscated form as any asset reference, and
//! un-obfuscating it yields a GUID of the token type carrying a small index.
//! That index is the property key, stable across heroes:
//!
//! | token index | property | how it was identified |
//! |---|---|---|
//! | 199 | max health | the pool every hero has, in health order |
//! | 1736 | max armour | present only on armoured heroes, values match published |
//! | 221 | max shields | present only on shielded heroes, values match published |
//!
//! Armour and shield values match the live published figures exactly for the
//! heroes that have them, which is what pins those two. Health values are
//! *lower* than the live patch by 25 to 75, because this installation predates a
//! health rebalance — the tokens are right, the patch is old. See
//! `docs/formats/stu.md`.
//!
//! Nothing here is named by a table: the tokens were identified structurally, and
//! the numbers were then checked against a permissively licensed published
//! source.

use std::collections::BTreeMap;

use crate::classes::{self, config_var};
use crate::graph::literal_of;
use crate::{guid, GameData, Result};
use ow_stu::Stu;

/// Token type. Tokens are keys, not files: no asset of this type is shipped, so
/// a token can never be resolved to a definition.
pub const TOKEN_TYPE: u16 = 0x01C;

/// Token indices whose meaning was established by reading real values.
pub mod pool {
    /// Maximum health.
    pub const HEALTH: u32 = 199;
    /// Maximum armour.
    pub const ARMOUR: u32 = 1736;
    /// Maximum shields.
    pub const SHIELDS: u32 = 221;
}

/// A literal read out of a schema entry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    Float(f32),
    Int(i32),
    Bool(bool),
    /// The value class is one this reader does not decode, so only its class
    /// hash is known.
    Unread(u32),
}

impl Value {
    /// The numeric value as a float, for the classes that are numeric.
    pub fn as_f32(self) -> Option<f32> {
        match self {
            Value::Float(v) => Some(v),
            Value::Int(v) => Some(v as f32),
            _ => None,
        }
    }

    pub fn as_i32(self) -> Option<i32> {
        match self {
            Value::Int(v) => Some(v),
            Value::Float(v) if v.fract() == 0.0 => Some(v as i32),
            _ => None,
        }
    }
}

/// A hero's health pools, as the entity declares them.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HealthPools {
    /// `None` when the entity does not declare this pool at all, which for
    /// armour and shields means zero rather than unknown.
    pub health: Option<i32>,
    pub armour: Option<i32>,
    pub shields: Option<i32>,
}

impl HealthPools {
    pub fn total(&self) -> Option<i32> {
        let health = self.health?;
        Some(health + self.armour.unwrap_or(0) + self.shields.unwrap_or(0))
    }
}

/// Every schema entry of an entity, keyed by token index.
///
/// Only entries whose token resolves to the token type are kept; a token of any
/// other type would not be addressable by index without also checking the type.
pub fn schema(stu: &mut Stu) -> BTreeMap<u32, Value> {
    let mut out = BTreeMap::new();

    // Entries are embedded, so each value is a separate instance naming the
    // entry that owns it. Collect those first, then walk the entries.
    let mut owner_of: BTreeMap<usize, usize> = BTreeMap::new();
    for (index, info) in stu.instances.iter().enumerate() {
        if info.embedder_index >= 0 && info.embedder_field == config_var::VALUE {
            owner_of.insert(info.embedder_index as usize, index);
        }
    }

    for entry in stu.instances_of(classes::STATESCRIPT_SCHEMA_ENTRY) {
        let Some(token) = stu.read_asset_ref(entry, classes::schema_entry::TOKEN) else {
            continue;
        };
        if guid::type_of(token) != TOKEN_TYPE {
            continue;
        }
        let Some(&value_instance) = owner_of.get(&entry) else {
            continue;
        };
        let class = stu.instances[value_instance].hash;
        let value = match class {
            classes::CONFIG_VAR_FLOAT => stu
                .read_f32(value_instance, config_var::VALUE)
                .map(Value::Float),
            classes::CONFIG_VAR_INT => stu
                .read_u32(value_instance, config_var::VALUE)
                .map(|v| Value::Int(v as i32)),
            classes::CONFIG_VAR_BOOL => stu
                .read_u32(value_instance, config_var::VALUE)
                .map(|v| Value::Bool(v != 0)),
            other => Some(Value::Unread(other)),
        };
        if let Some(value) = value {
            out.insert(guid::index_of(token), value);
        }
    }

    out
}

/// The schema of a gameplay entity asset, by GUID.
pub fn schema_of(data: &GameData, entity: u64) -> Result<BTreeMap<u32, Value>> {
    let mut stu = data.stu_of(entity)?;
    Ok(schema(&mut stu))
}

/// Health pools read from a gameplay entity.
pub fn health_of(data: &GameData, entity: u64) -> Result<HealthPools> {
    let entries = schema_of(data, entity)?;
    let get = |token: u32| entries.get(&token).and_then(|v| v.as_i32());
    Ok(HealthPools {
        health: get(pool::HEALTH),
        armour: get(pool::ARMOUR),
        shields: get(pool::SHIELDS),
    })
}

/// The mover component's measured float fields, by STU field hash.
///
/// Roles (evidence: `ow-sim/src/movement.rs` header + the install dump, e.g.
/// hero 1's entity `0x0400000000000001` whose mover instance carries 28 fields:
/// `0x031FE7EE = 5.5f` speed, `0x19452F15 = 6.0f` forward, `0xCFD3E635 = 6.0f`
/// side, `0x61D6B4E9 = 5.4f` back, `0x0DDD18C3 = 3.0f` crouch,
/// `0xB74245E8 = 30.0f` terminal, `0x1972C687 = 2.0f` unnamed scalar).
/// `0x5C6B4602` is the one integer field (reads `8`) and is skipped here.
pub mod mover_fields {
    pub const SPEED: u32 = 0x031F_E7EE;
    pub const FORWARD: u32 = 0x1945_2F15;
    pub const SIDE: u32 = 0xCFD3_E635;
    pub const BACK: u32 = 0x61D6_B4E9;
    pub const CROUCH: u32 = 0x0DDD_18C3;
    pub const FALL_TERMINAL: u32 = 0xB742_45E8;
    /// Class `+0x78`: the collision extent, maxed by `ConstructMoverBody`
    /// (`0x140ca9730`) against `2.1 * CAPSULE_RADIUS + CAPSULE_OFFSET`.
    /// Formerly carried as "the unnamed scalar"; the capsule read named it.
    pub const UNNAMED_SCALAR: u32 = 0x1972_C687;
    /// Class `+0x7c`: **the collision capsule's radius** (mover `+0x5a4`).
    pub const CAPSULE_RADIUS: u32 = 0x7A70_E3E3;
    /// Class `+0x80`: the height the capsule's segment is derived against
    /// (mover `+0x5a8`, `fVar2` in `0x140ca5db0`).
    pub const CAPSULE_OFFSET: u32 = 0x4A57_6BC2;
    /// Class `+0x84`: the **second** capsule triple's extent, maxed by
    /// `ConstructMoverBody` against `2.1 * SECOND_RADIUS + SECOND_OFFSET`. On
    /// the common hero it reads `1.25` against the first triple's `2.0`; which
    /// state selects it (crouch is the obvious candidate) is not proven yet.
    pub const SECOND_EXTENT: u32 = 0x03E0_4168;
    /// Class `+0x88`: the second capsule's radius (`0.5`).
    pub const SECOND_RADIUS: u32 = 0x915F_F368;
    /// Class `+0x8c`: the second capsule's offset (`0.25`).
    pub const SECOND_OFFSET: u32 = 0xD97B_3735;
    /// Class `+0xa4`: the up field, doubled and clamped at zero into the mover's
    /// `+0x68c` step-up limit (`0.5` on the common hero, so `1.0` m).
    pub const STEP_UP_FIELD: u32 = 0x08FF_AA0A;
    /// Class `+0xe4`: the vertical lane a standing jump takes off with.
    ///
    /// `FUN_140cc1440` (`0x140cc1627`) reads `body+0x60c` and `0x140cb2220`
    /// (`0x140cb246d`) reads the same slot as the movement vector's lateral
    /// cap, so the field is named for how it is *used* on take-off, not for a
    /// role: the two readers do not agree on a single meaning. The body slot is
    /// `0x528 + 0xe4` under `CopyMoverComponentFields` (`0x140ca4680`) /
    /// `FUN_140ca4980`; the offset is proved by the terminal-fall field, which
    /// `ApplyStateModifiers` reads at `body+0x614` and the class declares at
    /// `+0xec`. Every playable hero on this install reads `6.0`.
    pub const TAKEOFF_LANE: u32 = 0xEC28_9987;
    /// Class `+0xe8`: the **gravity magnitude**, a positive scale on the
    /// client's `10.0` constant.
    ///
    /// `FUN_140ca72d0` (`0x140ca72d0`) builds the frame's gravity vector as
    /// `(state_gate ? -10.0 : 0.0) * body[0x610] * body[0xc0..0xcc]`; the body
    /// slot is `0x528 + 0xe8` under the component copy, and `0x140cb2010`
    /// (`0x140cb20be`) reads the same slot as the drag law's numerator. The
    /// `-10.0` is the image constant at `0x1431161f8` returned by the gate
    /// `0x14202fb00`. A state modifier can scale the slot
    /// (`FUN_140cbeaa6` `0x140cbf...`, `body[0x610] *= param`).
    pub const GRAVITY: u32 = 0xC2C4_0969;
    /// Class `+0xf0`: the terminal speed the drag law uses **while rising**.
    ///
    /// `0x140cb2010` (`0x140cb2064..0x140cb2078`) picks `body+0x618` when the
    /// movement vector's vertical lane is `> 0` and `body+0x614`
    /// ([`FALL_TERMINAL`]) otherwise, then divides the gravity scale by the
    /// square of the pick. So `+0xec` and `+0xf0` are the down and up terminal
    /// speeds of the same quadratic drag. The class default is absent, so the
    /// value is the hero's own or nothing.
    pub const RISE_TERMINAL: u32 = 0x975F_21A5;
    /// The integer field; not a float, never carried as one.
    pub const INT_FIELD: u32 = 0x5C6B_4602;
}

/// One movement row, the named floats the walk reads.
///
/// The field hashes are the ones the STU class declares at its own offsets
/// (`STUCharacterMoverComponent`: `0xA8` side, `0xAC` back, `0xB0` forward,
/// `0xBC` crouch, `0xDC` the unnamed speed scalar, `0xEC` terminal fall), and
/// the client's runtime slot is STU offset + 4 (`docs/formats/character-controller.md`).
/// Every value is carried as it reads; a missing field is `None`, never zero —
/// an absent field means the class default, which this reader does not know.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MoverRow {
    /// `0x031FE7EE`: the unnamed scalar the component schema puts after crouch.
    pub speed: Option<f32>,
    /// `0x19452F15`: the forward base speed.
    pub forward: Option<f32>,
    /// `0xCFD3E635`: the sideways base speed.
    pub side: Option<f32>,
    /// `0x61D6B4E9`: the backpedal base speed.
    pub back: Option<f32>,
    /// `0x0DDD18C3`: the crouch speed.
    pub crouch: Option<f32>,
    /// `0xB74245E8`: the terminal fall speed (the drag law's down terminal).
    pub fall_terminal: Option<f32>,
    /// `0xC2C40969`: the gravity magnitude (a scale on the client's `10.0`).
    pub gravity: Option<f32>,
    /// `0x975F21A5`: the terminal speed while rising (the drag law's up
    /// terminal).
    pub rise_terminal: Option<f32>,
}

/// Read the hero entity's collision capsule from its mover component.
///
/// The three inputs are the class's `+0x78`, `+0x7c`, `+0x80`
/// ([`mover_fields::UNNAMED_SCALAR`], [`mover_fields::CAPSULE_RADIUS`],
/// [`mover_fields::CAPSULE_OFFSET`]); [`crate::character_collision::capsule_from_mover`]
/// is the client's construction. `Ok(None)` means the entity has no mover
/// component; a component missing any of the three declares no capsule, which
/// is reported as `None` rather than invented from a class default.
pub fn mover_capsule_of(
    data: &GameData,
    entity: u64,
) -> Result<Option<crate::character_collision::Capsule>> {
    let mut stu = data.stu_of(entity)?;
    let Some(inst) = stu.instance(classes::CHARACTER_MOVER_COMPONENT) else {
        return Ok(None);
    };
    let extent = stu.read_f32(inst, mover_fields::UNNAMED_SCALAR);
    let radius = stu.read_f32(inst, mover_fields::CAPSULE_RADIUS);
    let offset = stu.read_f32(inst, mover_fields::CAPSULE_OFFSET);
    let (Some(extent), Some(radius), Some(offset)) = (extent, radius, offset) else {
        return Ok(None);
    };
    Ok(Some(crate::character_collision::capsule_from_mover(
        extent, radius, offset,
    )))
}

/// Read the hero entity's vertical step limits from its mover component.
///
/// `ConstructMoverBody` (`0x140ca9730`) stores them at mover `+0x688` (the
/// negative downward limit) and `+0x68c` (the upward one);
/// [`crate::character_collision::step_limits_from_mover`] is that construction
/// from the component's `+0x78..+0x8c` and `+0xa4`. `Ok(None)` means no mover
/// component or a missing field - never a class default.
pub fn mover_step_limits_of(
    data: &GameData,
    entity: u64,
) -> Result<Option<crate::character_collision::StepLimits>> {
    let mut stu = data.stu_of(entity)?;
    let Some(inst) = stu.instance(classes::CHARACTER_MOVER_COMPONENT) else {
        return Ok(None);
    };
    let read = |stu: &mut Stu, hash: u32| stu.read_f32(inst, hash);
    let extent = read(&mut stu, mover_fields::UNNAMED_SCALAR);
    let radius = read(&mut stu, mover_fields::CAPSULE_RADIUS);
    let offset = read(&mut stu, mover_fields::CAPSULE_OFFSET);
    let second_extent = read(&mut stu, mover_fields::SECOND_EXTENT);
    let second_radius = read(&mut stu, mover_fields::SECOND_RADIUS);
    let second_offset = read(&mut stu, mover_fields::SECOND_OFFSET);
    let up = read(&mut stu, mover_fields::STEP_UP_FIELD);
    let (Some(extent), Some(radius), Some(offset), Some(second_extent), Some(second_radius)) =
        (extent, radius, offset, second_extent, second_radius)
    else {
        return Ok(None);
    };
    let (Some(second_offset), Some(up)) = (second_offset, up) else {
        return Ok(None);
    };
    Ok(Some(crate::character_collision::step_limits_from_mover(
        extent,
        radius,
        offset,
        second_extent,
        second_radius,
        second_offset,
        up,
    )))
}

/// Read the named [`MoverRow`] of an entity's mover component.
///
/// `Ok(None)` means the entity has no mover component at all (an AI or a prop);
/// a component with a field absent leaves that field `None`.
pub fn mover_row_of(data: &GameData, entity: u64) -> Result<Option<MoverRow>> {
    let mut stu = data.stu_of(entity)?;
    let Some(inst) = stu.instance(classes::CHARACTER_MOVER_COMPONENT) else {
        return Ok(None);
    };
    let read = |stu: &mut Stu, hash: u32| stu.read_f32(inst, hash);
    Ok(Some(MoverRow {
        speed: read(&mut stu, mover_fields::SPEED),
        forward: read(&mut stu, mover_fields::FORWARD),
        side: read(&mut stu, mover_fields::SIDE),
        back: read(&mut stu, mover_fields::BACK),
        crouch: read(&mut stu, mover_fields::CROUCH),
        fall_terminal: read(&mut stu, mover_fields::FALL_TERMINAL),
        gravity: read(&mut stu, mover_fields::GRAVITY),
        rise_terminal: read(&mut stu, mover_fields::RISE_TERMINAL),
    }))
}

/// The mover component's take-off lane, `0xEC289987` (class `+0xe4`).
///
/// This is the value `FUN_140cc1440` writes into the vertical lane of the
/// movement vector when a standing jump starts. It is **not consumed** by
/// anything in `ow-client` yet: the per-frame decay of that lane (the vector
/// record table at `body+0x960` and the sweep solver) is unread, so there is no
/// arc to put it in (`docs/open-questions.md` item 1b). `Ok(None)` means no
/// mover component or no such field - never a class default.
pub fn mover_takeoff_lane_of(data: &GameData, entity: u64) -> Result<Option<f32>> {
    let mut stu = data.stu_of(entity)?;
    let Some(inst) = stu.instance(classes::CHARACTER_MOVER_COMPONENT) else {
        return Ok(None);
    };
    Ok(stu.read_f32(inst, mover_fields::TAKEOFF_LANE))
}

/// A hero entity's mover figures: base speed plus every other float the mover
/// component declares, keyed by field hash.
///
/// The base speed is the `SPEED` field (5.5 fallback, the install-wide common
/// case); the rest are carried verbatim so the sim's `MoverSpec` keeps its
/// back/side/crouch/terminal slots instead of defaulting them. Sorted by hash
/// for deterministic runs.
pub fn mover_fields_of(data: &GameData, entity: u64) -> Result<(f32, Vec<(u32, f32)>)> {
    let mut stu = data.stu_of(entity)?;
    let Some(inst) = stu.instance(classes::CHARACTER_MOVER_COMPONENT) else {
        return Ok((5.5, Vec::new()));
    };
    let speed = stu.read_f32(inst, mover_fields::SPEED).unwrap_or(5.5);
    let mut out = Vec::new();
    for f in stu.fields(inst).to_vec() {
        if f.hash == mover_fields::SPEED || f.hash == mover_fields::INT_FIELD {
            continue;
        }
        if f.size != 4 {
            continue;
        }
        let at = stu.payload_start(inst) + f.offset;
        let Some(raw) = stu.u32_at(at) else { continue };
        let v = f32::from_bits(raw);
        if !v.is_finite() {
            continue;
        }
        out.push((f.hash, v));
    }
    out.sort_by_key(|(h, _)| *h);
    Ok((speed, out))
}

/// A loadout (ability) asset and the fields this reader uses.
/// `STULoadout` behaviour, read from the reference types only
/// (`TankLib/STU/Types/STULoadout.cs` and the `Loadout` data model that walks
/// it): a hero names an array of loadout assets (`m_heroLoadout`); each one
/// carries display strings by reference, a category enum, a logical button by
/// reference, and two `i32` flags — `m_0E679979 >= 1` marks a hidden ability
/// (weapons, zoom) and `== 2` a secondary weapon. Cooldown and damage are not
/// among these fields: the cooldown is a marker variable in the ability's
/// graph node, and per-ability damage has no file-side join (see
/// [`Ability::cooldown`] and [`Ability::damage`]).
pub mod loadout {
    /// `STULoadout`. Note this is *not* [`crate::classes::LOADOUT`]
    /// (`0x32290C2E`): that hash matches no class in the reference types and
    /// is unused by every reader, while `0x07A0E32F` is the hash the generated
    /// `STULoadout` type declares and the one a `0x09E` asset's instance table
    /// is checked against here.
    pub const LOADOUT: u32 = 0x07A0_E32F;
    /// `m_name`: display name, a string-asset reference.
    pub const NAME: u32 = 0xB48F_1D22;
    /// `m_description`: description, a string-asset reference.
    pub const DESCRIPTION: u32 = 0xCA7E_6EDC;
    /// `m_logicalButton`: which button the UI shows, via a holder asset.
    pub const LOGICAL_BUTTON: u32 = 0xFC33_191B;
    /// `m_9290B942`: the second button reference the data model reads.
    pub const BUTTON_UNK: u32 = 0x9290_B942;
    /// `m_texture`: the ability icon, a texture-asset reference.
    pub const TEXTURE: u32 = 0x3CD6_DC1E;
    /// `m_category`: a `LoadoutCategory` enum value.
    pub const CATEGORY: u32 = 0x2C54_AEAF;
    /// `m_0E679979`: hidden (`>= 1`) / secondary-weapon (`== 2`) flags.
    pub const HIDDEN_FLAG: u32 = 0x0E67_9979;
    /// The holder class of a logical-button reference (`STU_C5243F93`).
    pub const BUTTON_HOLDER: u32 = 0xC524_3F93;
    /// Its name field: the string-asset reference the button label resolves
    /// through. (A different hash from the loadout's own `m_name` — the two
    /// fields share a Rust-style name in the reference, not a hash.)
    pub const BUTTON_NAME: u32 = 0xC08C_4427;
    /// The loadout's own tuning knobs: ten `STUConfigVar` fields whose values
    /// read as literals but whose meanings are unassigned. They are reported
    /// by hash in [`super::Ability::params`], never guessed at.
    pub const KNOBS: [u32; 10] = [
        0x306D_A3DC,
        0x5929_7A45,
        0xCE20_C911,
        0xA0D3_7782,
        0x33E2_8024,
        0x4F78_889F,
        0xE840_8704,
        0x36D0_3BBB,
        0x0CAB_E706,
        0xBED6_A0EE,
    ];
}

/// Loadout categories, from the reference `LoadoutCategory` enum.
pub mod loadout_category {
    /// A weapon entry.
    pub const WEAPON: u32 = 0;
    /// A regular ability.
    pub const ABILITY: u32 = 1;
    /// A passive ability.
    pub const PASSIVE: u32 = 2;
    /// An ultimate ability.
    pub const ULTIMATE: u32 = 3;
    /// A perk entry.
    pub const PERK: u32 = 4;
    /// Hero-stats entry.
    pub const HERO_STATS: u32 = 5;
    /// Subrole entry.
    pub const SUBROLE: u32 = 6;
}

/// One ability (loadout) of a hero: the `ListAbilities` parity row.
///
/// Identity and display come from the `0x09E` asset; `params` carries the
/// loadout's own config-var literals by field hash — the tuning knobs whose
/// values are unambiguous but whose meanings are not. Cooldown and damage are
/// `None` by construction: the cooldown lives in the ability graph's node as
/// a marker variable rather than a literal (see [`crate::graph::ability_nodes_of`]),
/// and no file-side walk joins a hero to its ability graphs (the reference
/// walk in `refwalk` reaches exactly one graph per hero — the weapon's — so
/// the ability graphs resolve at runtime, not through asset references).
/// Display code must render those columns as [`Ability::cooldown_name`] and
/// [`Ability::damage_name`] (`cooldown?`, `damage?`), never as zero.
#[derive(Debug, Clone, Default)]
pub struct Ability {
    /// The `0x09E` asset GUID.
    pub guid: u64,
    /// Display name, when the string asset resolves and parses.
    pub name: Option<String>,
    /// Description, same conditions.
    pub description: Option<String>,
    /// The `LoadoutCategory` code, when the field is present.
    pub category: Option<u32>,
    /// The UI button label, via the logical-button holder asset.
    pub button: Option<String>,
    /// `m_0E679979 >= 1`: hidden from the ability list (weapons, zoom).
    pub hidden: bool,
    /// `m_0E679979 == 2`: a secondary weapon (Mercy/Bastion/Torbjörn pattern).
    pub secondary_weapon: bool,
    /// Cooldown in seconds. Always `None`: not a file-side literal.
    pub cooldown: Option<f32>,
    /// Per-shot damage. Always `None`: no file-side hero→damage join.
    pub damage: Option<i32>,
    /// The loadout's own `(field hash, literal)` pairs, sorted by hash.
    pub params: Vec<(u32, Value)>,
}

impl Ability {
    /// The category's display name, from the reference enum.
    pub fn category_name(&self) -> Option<&'static str> {
        match self.category {
            Some(loadout_category::WEAPON) => Some("Weapon"),
            Some(loadout_category::ABILITY) => Some("Ability"),
            Some(loadout_category::PASSIVE) => Some("PassiveAbility"),
            Some(loadout_category::ULTIMATE) => Some("UltimateAbility"),
            Some(loadout_category::PERK) => Some("Perk"),
            Some(loadout_category::HERO_STATS) => Some("HeroStats"),
            Some(loadout_category::SUBROLE) => Some("Subrole"),
            _ => None,
        }
    }

    /// Column label for the cooldown, which has no file-side literal.
    pub fn cooldown_name() -> &'static str {
        "cooldown?"
    }

    /// Column label for the damage, which has no file-side join.
    pub fn damage_name() -> &'static str {
        "damage?"
    }
}

/// The loadout GUIDs a hero asset names (`m_heroLoadout`, the `0x9E` hop).
///
/// The list is an inline array — an `i32` count then that many 16-byte
/// references, each deobfuscated with the field hash — the same shape the
/// `refwalk` probe follows. A candidate only counts when it resolves in the
/// registry to a `0x09E` asset: a wrong stride yields 64-bit values that
/// resolve to nothing, which is how a wrong layout is caught. An empty list
/// is reported, not defaulted — several heroes may genuinely declare none.
pub fn loadout_guids_of_hero(data: &GameData, hero_guid: u64) -> Result<Vec<u64>> {
    let mut stu = data.stu_of(hero_guid)?;
    let Some(hero) = stu.instance(classes::HERO) else {
        return Err(crate::Error::NotFound(
            "hero asset has no hero instance".into(),
        ));
    };
    let guids = ref_array_field(&mut stu, hero, classes::hero::HERO_LOADOUT);
    Ok(guids
        .into_iter()
        .filter(|g| {
            *g != 0
                && data
                    .registry()
                    .get(*g)
                    .is_some_and(|_| guid::type_of(*g) == guid::types::LOADOUT)
        })
        .collect())
}

/// Every loadout/ability asset the manifests carry (`TrackedFiles[0x9E]`
/// parity with `ListAbilities`): the global ability list, before the
/// per-hero join.
pub fn all_loadout_guids(data: &GameData) -> Vec<u64> {
    let mut guids = data.guids_of_type(guid::types::LOADOUT);
    guids.sort_unstable();
    guids
}

/// One ability row for a `0x09E` asset.
pub fn ability_of(data: &GameData, ability_guid: u64) -> Result<Ability> {
    let mut stu = data.stu_of(ability_guid)?;
    let Some(root) = stu.instance(loadout::LOADOUT) else {
        return Err(crate::Error::NotFound(format!(
            "loadout 0x{ability_guid:016x} has no STULoadout instance"
        )));
    };

    let string_at = |stu: &mut Stu, instance: usize, field: u32| -> Option<String> {
        let target = stu.read_asset_ref(instance, field)?;
        if target == 0 {
            return None;
        }
        let bytes = data.bytes_of(target).ok()?;
        let text = crate::text::parse_display_text(&bytes).ok()?;
        (!text.value.is_empty()).then_some(text.value)
    };
    let u32_at = |stu: &mut Stu, instance: usize, field: u32| -> Option<u32> {
        let (offset, _) = stu.field_bytes(instance, field)?;
        stu.u32_at(stu.payload_start(instance) + offset)
    };

    let name = string_at(&mut stu, root, loadout::NAME);
    let description = string_at(&mut stu, root, loadout::DESCRIPTION);
    let category = u32_at(&mut stu, root, loadout::CATEGORY);
    let flag = u32_at(&mut stu, root, loadout::HIDDEN_FLAG)
        .map(|v| v as i32)
        .unwrap_or(0);

    // The button label is two hops: the loadout's logical-button reference
    // names a holder asset, whose own name field names the string.
    let button = stu
        .read_asset_ref(root, loadout::LOGICAL_BUTTON)
        .filter(|holder| *holder != 0)
        .and_then(|holder| {
            data.stu_of(holder).ok().and_then(|mut holder_stu| {
                let holder_root = holder_stu.instance(loadout::BUTTON_HOLDER)?;
                string_at(&mut holder_stu, holder_root, loadout::BUTTON_NAME)
            })
        });

    // The loadout's own config-var literals: embedder-linked children decoded
    // by class, so only literal-valued children appear.
    let mut owned: BTreeMap<(usize, u32), usize> = BTreeMap::new();
    for (index, info) in stu.instances.iter().enumerate() {
        if info.embedder_index >= 0 {
            owned.insert((info.embedder_index as usize, info.embedder_field), index);
        }
    }
    let mut params = Vec::new();
    for ((owner, field_hash), value_index) in &owned {
        if *owner != root {
            continue;
        }
        let class = stu.instances[*value_index].hash;
        if let Some(value) = literal_of(&stu, *value_index, class) {
            params.push((*field_hash, value));
        }
    }
    params.sort_by_key(|(hash, _)| *hash);

    Ok(Ability {
        guid: ability_guid,
        name,
        description,
        category,
        button,
        hidden: flag >= 1,
        secondary_weapon: flag == 2,
        cooldown: None,
        damage: None,
        params,
    })
}

/// Every ability of a hero index (`ListHeroes` loadout-list parity): the
/// hero's `m_heroLoadout` GUIDs, each read as an [`Ability`]. Heroes whose
/// entity cannot be read contribute no rows rather than an error — the census
/// counts heroes, and an unreadable hero is a zero, not a failure.
pub fn loadouts_of(data: &GameData, hero_index: u32) -> Result<Vec<Ability>> {
    let mut heroes = data.guids_of_type(crate::guid::types::HERO);
    heroes.sort_unstable();
    let hero = *heroes
        .iter()
        .find(|g| crate::guid::index_of(**g) == hero_index)
        .ok_or_else(|| crate::Error::NotFound(format!("hero index {hero_index} not found")))?;
    let mut out = Vec::new();
    for ability_guid in loadout_guids_of_hero(data, hero)? {
        if let Ok(ability) = ability_of(data, ability_guid) {
            out.push(ability);
        }
    }
    Ok(out)
}

/// An inline `teStructuredDataAssetRef<T>[]` field: an `i32` count then that
/// many 16-byte references, each deobfuscated with the field hash.
///
/// The element layout is the typed layer's (`ow_stu/src/types.rs`): the GUID
/// sits at `+8` of each 16-byte element. Callers validate the GUIDs against
/// the registry (`loadout_guids_of_hero` keeps only `0x09E` hits), so a wrong
/// stride surfaces as an empty list rather than wrong abilities.
fn ref_array_field(stu: &mut Stu, instance: usize, field_hash: u32) -> Vec<u64> {
    let Some((offset, _)) = stu.field_bytes(instance, field_hash) else {
        return Vec::new();
    };
    let base = stu.payload_start(instance) + offset;
    let Some(count) = stu.i32_at(base) else {
        return Vec::new();
    };
    if !(1..=4096).contains(&count) {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(count as usize);
    for k in 0..count as usize {
        // Absolute offset: the ref's own bytes, deobfuscated with the
        // *array's* field hash.
        if let Some(target) = stu.asset_ref_at(base + 4 + k * 16, field_hash) {
            out.push(target);
        } else {
            break;
        }
    }
    out
}

/// The parts of a hero a renderer needs: its character model, the colour texture
/// the hero's entity names directly, the first-person model's own entity, and
/// that model.
///
/// Measured on hero index 2 and 3 (see `docs/formats/animation.md` § "The
/// component chain"): the hero asset's `STUHero` names the gameplay entity; the
/// entity carries a `STUModelComponent` (`0x191835f9`) whose field
/// `0x2d124be2` is the character model, a `STUFirstPersonComponent`
/// (`0x9d28963f`) whose field `0xa83c2c26` is *another entity* holding the
/// first-person model under the same field hash, and a pair of instances that
/// name textures directly (`0x238ef37e`/`0x6f1ecb16` and
/// `0xfb366254`/`0x3cd6dc1e`) — the route that does not need materials, which
/// this install cannot decrypt.
#[derive(Debug, Clone, Default)]
pub struct HeroParts {
    pub hero: u64,
    pub entity: u64,
    pub model: u64,
    /// The model component's look: the model's material ids → material assets.
    pub look: u64,
    /// The texture the entity's own instance names, if it has one.
    pub texture: u64,
    /// A second texture instance (`0xfb366254`), which is a portrait rather
    /// than a surface texture on the models read so far.
    pub portrait: u64,
    pub view_model_entity: u64,
    pub view_model: u64,
    /// The view-model's **animation blend-tree set** (`STUModelComponent`'s
    /// `m_animBlendTreeSet`, field `0x523339C1`): a type-0x021 asset whose
    /// blend items name the first-person clips per slot (the idle set for
    /// hero 3 = `0x0040000000000004` with 23 items naming 40+ clips).
    pub view_model_anim_set: u64,
    /// The view-model entity's model component look (the same field hash the
    /// character model component carries). Which textures a view model gets is
    /// decided by *its* look, not the character's.
    pub view_model_look: u64,
    /// The FP entity's weapon child (its model), when it is the known Tracer
    /// chain. The child entity itself is `0x0400000000000017`; the general
    /// `STUChildEntityDefinition` decode is still open, so this is filled only
    /// for the measured FP entity.
    pub view_model_weapon: u64,
    pub view_model_weapon_look: u64,
    /// The hardpoint GUIDs the FP entity's two weapon child definitions name.
    /// `child_probe`'s typed read gives `0x0dc000000000000a` and
    /// `0x0dc000000000000b`; they hang off the FP skeleton's two weapon bones
    /// (ids 44 and 74), not the hand bones.
    pub view_model_weapon_hardpoints: Vec<u64>,
}

/// Field hashes of the two texture-naming instances, and of the model
/// component's model field.
pub mod hero_part {
    /// Instance type that carries a directly named texture.
    pub const TEXTURE_INSTANCE: u32 = 0x238e_f37e;
    /// Its texture field.
    pub const TEXTURE: u32 = 0x6f1e_cb16;
    /// The other texture-naming instance type.
    pub const PORTRAIT_INSTANCE: u32 = 0xfb36_6254;
    /// Its texture field.
    pub const PORTRAIT: u32 = 0x3cd6_dc1e;
    /// `STUModelComponent`.
    pub const MODEL_COMPONENT: u32 = 0x1918_35f9;
    /// The model component's **animation blend-tree set**
    /// (`STUModelComponent.m_animBlendTreeSet`, `0x523339C1` = a
    /// `teStructuredDataAssetRef<STUAnimBlendTreeSet>` type-0x021 asset; the
    /// reference reads it off every model component, `Combo.cs:546`).
    pub const ANIM_BLEND_TREE_SET: u32 = 0x5233_39c1;
    /// Its model reference (shared with the first-person entity).
    pub const MODEL: u32 = 0x2d12_4be2;
    /// `STUFirstPersonComponent`.
    pub const FIRST_PERSON_COMPONENT: u32 = 0x9d28_963f;
    /// Its reference to the view-model entity.
    pub const FIRST_PERSON_ENTITY: u32 = 0xa83c_2c26;
    /// The model component's *look* (a `STUModelLook`, type `0x01A`): the same
    /// chain a map placement uses, read from hero 3's component. A look maps the
    /// model's material ids to material assets, and that is the route to a
    /// hero's body textures — where the material assets are readable.
    pub const MODEL_LOOK: u32 = 0x9297_adb4;
    /// Tracer's first-person entity and its weapon child, as measured by
    /// `child_probe` (`docs/formats/animation.md` § the FP entity's child
    /// entities). The child-definition refs are inline and their raw decode is
    /// still open, so the join is allowed only for this measured pair.
    pub const VIEW_MODEL_ENTITY: u64 = 0x0400_0000_0000_0010;
    pub const VIEW_MODEL_WEAPON_ENTITY: u64 = 0x0400_0000_0000_0017;
    /// `STUChildEntityDefinition` (`0x1D9159F2`) fields and the entity field
    /// that holds the inline child array.
    pub const CHILD_ENTITY_DATA: u32 = 0xf29e_4995;
    pub const CHILD: u32 = 0x436b_efe6;
    pub const HARDPOINT: u32 = 0x2d3c_dfa9;
}

/// Read [`HeroParts`] for a hero index (`ow-tank-hero` prints the same fields).
pub fn hero_parts(data: &GameData, hero_index: u32) -> Result<HeroParts> {
    let mut heroes = data.guids_of_type(crate::guid::types::HERO);
    heroes.sort_unstable();
    let hero = *heroes
        .iter()
        .find(|g| crate::guid::index_of(**g) == hero_index)
        .ok_or_else(|| crate::Error::NotFound(format!("hero index {hero_index} not found")))?;

    let mut parts = HeroParts {
        hero,
        ..HeroParts::default()
    };
    let mut stu = data.stu_of(hero)?;
    let hero_inst = stu
        .instance(crate::classes::HERO)
        .ok_or_else(|| crate::Error::NotFound("hero asset has no hero instance".into()))?;
    parts.entity = stu
        .read_asset_ref(hero_inst, crate::classes::hero::GAMEPLAY_ENTITY)
        .ok_or_else(|| crate::Error::NotFound("hero asset names no gameplay entity".into()))?;
    drop(stu);

    let mut entity_stu = data.stu_of(parts.entity)?;
    let field_of = |stu: &mut Stu, instance: usize, hash: u32| stu.read_asset_ref(instance, hash);
    for i in 0..entity_stu.instances.len() {
        match entity_stu.instances[i].hash {
            hero_part::MODEL_COMPONENT => {
                if let Some(m) = field_of(&mut entity_stu, i, hero_part::MODEL) {
                    parts.model = m;
                }
                if let Some(l) = field_of(&mut entity_stu, i, hero_part::MODEL_LOOK) {
                    parts.look = l;
                }
            }
            hero_part::FIRST_PERSON_COMPONENT => {
                if let Some(e) = field_of(&mut entity_stu, i, hero_part::FIRST_PERSON_ENTITY) {
                    parts.view_model_entity = e;
                }
            }
            hero_part::TEXTURE_INSTANCE => {
                if let Some(t) = field_of(&mut entity_stu, i, hero_part::TEXTURE) {
                    parts.texture = t;
                }
            }
            hero_part::PORTRAIT_INSTANCE => {
                if let Some(t) = field_of(&mut entity_stu, i, hero_part::PORTRAIT) {
                    parts.portrait = t;
                }
            }
            _ => {}
        }
    }
    drop(entity_stu);

    if parts.view_model_entity != 0 {
        if let Ok(mut view_stu) = data.stu_of(parts.view_model_entity) {
            for i in 0..view_stu.instances.len() {
                if view_stu.instances[i].hash != hero_part::MODEL_COMPONENT {
                    continue;
                }
                if let Some(m) = view_stu.read_asset_ref(i, hero_part::MODEL) {
                    parts.view_model = m;
                }
                if let Some(l) = view_stu.read_asset_ref(i, hero_part::MODEL_LOOK) {
                    parts.view_model_look = l;
                }
                if let Some(a) = view_stu.read_asset_ref(i, hero_part::ANIM_BLEND_TREE_SET) {
                    parts.view_model_anim_set = a;
                }
                break;
            }
        }
    }
    if parts.view_model_entity == hero_part::VIEW_MODEL_ENTITY {
        if let Ok(fp_stu) = data.stu_of(hero_part::VIEW_MODEL_ENTITY) {
            for (bag, at) in fp_stu.inline_elements(0, hero_part::CHILD_ENTITY_DATA) {
                let fields = fp_stu.layout_at(at, bag);
                let mut child = None;
                let mut hardpoint = None;
                for f in &fields {
                    if f.hash == hero_part::CHILD {
                        child = fp_stu.asset_ref_at(at + f.offset, f.hash);
                    } else if f.hash == hero_part::HARDPOINT {
                        hardpoint = fp_stu.asset_ref_at(at + f.offset, f.hash);
                    }
                }
                if child == Some(hero_part::VIEW_MODEL_WEAPON_ENTITY) {
                    if let Some(hp) = hardpoint {
                        parts.view_model_weapon_hardpoints.push(hp);
                    }
                }
            }
        }
        if let Ok(mut weapon_stu) = data.stu_of(hero_part::VIEW_MODEL_WEAPON_ENTITY) {
            for i in 0..weapon_stu.instances.len() {
                if weapon_stu.instances[i].hash != hero_part::MODEL_COMPONENT {
                    continue;
                }
                parts.view_model_weapon =
                    weapon_stu.read_asset_ref(i, hero_part::MODEL).unwrap_or(0);
                parts.view_model_weapon_look = weapon_stu
                    .read_asset_ref(i, hero_part::MODEL_LOOK)
                    .unwrap_or(0);
                break;
            }
        }
    }
    Ok(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The flag rule from the reference `Loadout` data model, as a pure
    /// function so the census can share it: hidden at `>= 1`, secondary
    /// weapon at exactly `2`.
    fn hidden_of(flag: i32) -> (bool, bool) {
        (flag >= 1, flag == 2)
    }

    #[test]
    fn ability_flag_rule_matches_the_reference_model() {
        assert_eq!(hidden_of(0), (false, false));
        assert_eq!(hidden_of(1), (true, false));
        assert_eq!(hidden_of(2), (true, true));
        assert_eq!(hidden_of(3), (true, false));
    }

    #[test]
    fn ability_categories_name_the_reference_enum() {
        let named = |category: u32| {
            Ability {
                category: Some(category),
                ..Ability::default()
            }
            .category_name()
        };
        assert_eq!(named(loadout_category::WEAPON), Some("Weapon"));
        assert_eq!(named(loadout_category::ABILITY), Some("Ability"));
        assert_eq!(named(loadout_category::PASSIVE), Some("PassiveAbility"));
        assert_eq!(named(loadout_category::ULTIMATE), Some("UltimateAbility"));
        assert_eq!(named(loadout_category::PERK), Some("Perk"));
        assert_eq!(named(0xFFFF), None);
        assert_eq!(
            Ability {
                category: None,
                ..Ability::default()
            }
            .category_name(),
            None
        );
    }

    #[test]
    fn unmeasured_ability_columns_stay_hedged() {
        // Cooldown and damage have no file-side reading, so the labels carry
        // `?` and a fresh row carries no values to mistake for zero.
        assert_eq!(Ability::cooldown_name(), "cooldown?");
        assert_eq!(Ability::damage_name(), "damage?");
        let row = Ability::default();
        assert_eq!((row.cooldown, row.damage), (None, None));
    }

    #[test]
    fn loadout_knobs_list_the_config_var_fields() {
        // Ten knobs, all distinct — a duplicate hash would attribute one
        // field's literals to another's name.
        let mut knobs = loadout::KNOBS.to_vec();
        knobs.sort_unstable();
        knobs.dedup();
        assert_eq!(knobs.len(), 10);
    }

    #[test]
    fn total_sums_the_pools_that_are_declared() {
        let pools = HealthPools {
            health: Some(250),
            armour: None,
            shields: None,
        };
        assert_eq!(pools.total(), Some(250));

        let pools = HealthPools {
            health: Some(250),
            armour: Some(200),
            shields: Some(225),
        };
        assert_eq!(pools.total(), Some(675));

        // Without health there is no total, even if a pool is declared.
        let pools = HealthPools {
            health: None,
            armour: Some(200),
            shields: None,
        };
        assert_eq!(pools.total(), None);
    }

    #[test]
    fn values_convert_only_when_the_conversion_is_lossless() {
        assert_eq!(Value::Int(250).as_f32(), Some(250.0));
        assert_eq!(Value::Float(1.5).as_f32(), Some(1.5));
        assert_eq!(Value::Float(1.5).as_i32(), None);
        assert_eq!(Value::Float(2.0).as_i32(), Some(2));
        assert_eq!(Value::Bool(true).as_i32(), None);
        assert_eq!(Value::Unread(0x1234).as_i32(), None);
    }
}

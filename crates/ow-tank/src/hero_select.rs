//! The hero selector's roster source: a game mode's per-team hero pool.
//!
//! When a mode is entered, the set of heroes a player may pick is declared by
//! the mode's own data, not by an engine screen: each team of a
//! `STUGameMode` (`0x494C33C4`) carries an `m_availableHeroes` object, and that
//! object says either "no restriction" or "exactly these heroes".
//!
//! Read from the install and the class registry (names are the reference
//! checkout's; the values and the shape are the client's):
//!
//! * `STUGameMode.m_teams` (`0xD440A0F7`): an inline array of
//!   `STUGameModeTeam` (`0xD797394C`).
//! * `STUGameModeTeam.m_availableHeroes` (`0x59C86C8D`, `+64`):
//!   an embedded instance of `STU_5427ADC1`.
//!   `STUGameRulesetTeam.m_availableHeroes` (`0x59C86C8D`, `+56`) is the same
//!   object in the ruleset hierarchy.
//! * `STU_5427ADC1` (`0x5427ADC1`, 88 bytes) is the general restriction object:
//!   skin themes (`0x62E537BD`), `0x6183A698`, and three per-hero inline arrays
//!   whose entries all carry a hero (`0x71DBD5E9`) plus an unlock or identifier.
//! * `STU_D1DC7C55` (`0xD1DC7C55`, 88 bytes) is the **empty marker**: it
//!   declares nothing, so every hero may be picked.
//! * `STU_3C14F504` (`0x3C14F504`) and `STU_C45DE560` (`0xC45DE560`), both 104
//!   bytes, add `m_heroes` (`0x056D3E39`, `+88`): the hero whitelist.
//!
//! All four classes are also **runtime component types**, registered as one
//! consecutive block of stubs: `0x1402dc690` (base, size `0x58`),
//! `0x1402dc790` (empty marker, `0x58`), `0x1402dc890` (whitelist `0x3C14F504`,
//! `0x68`) and `0x1402dc990` (whitelist `0xC45DE560`, `0x68`), each calling
//! `RegisterComponentTypeDescriptor` with the class hash and that size, 8-byte
//! aligned. The whitelist's `m_heroes` therefore sits at `+0x58` of the runtime
//! component.
//!
//! Measured on this install (every mode team answers with one of the three
//! cases; a pool class this reader does not know is an error, never a guess):
//!
//! | mode | asset | teams |
//! |---|---|---|
//! | Practice Range | `0x0230000000000018` | both the empty marker: every hero |
//! | Escort | `0x023000000000011e` | both whitelist the same 29 heroes |
//! | Stadium | `0x0230000000000143` | team 0 whitelists 33, team 1 the marker |
//! | Mei's Snowball Offensive | `0x0230000000000008` | both whitelist Mei only |
//!
//! Every whitelist a mode team carries in this install is `STU_C45DE560`;
//! `STU_3C14F504` is declared and registered but no mode team uses it.
//!
//! The pool says *which* heroes; the hero asset says how to name one and what
//! the walk applies (added 2026-11-06):
//!
//! * `STUHero.m_0EDCE350` names a display string (type `0x07C`): the hero's own
//!   name, decoded by [`hero_name`].
//! * `STUHero.m_64DC571F` is the playable-hero marker ([`hero_is_playable`]).
//!   An open roster contains NPC heroes too (the practice range's 135 hero
//!   assets are 54 playable and 81 NPC, and the range's `Training Bot` is a
//!   *named* NPC), so a player picker filters on the marker.
//! * `STUHero.m_gameplayEntity` leads to the hero's `STUCharacterMoverComponent`;
//!   `crate::entity::mover_row_of` reads its forward/side/back/crouch/terminal
//!   rows.
//! * [`selectable_heroes`] is the mode's roster, [`resolve_hero`] turns a name
//!   or asset index into one entry, and an unreadable pool class is an error,
//!   never a widened roster.
//!
//! Open: which component consumer draws the picker, what binds runtime
//! variable 940 (the store that IS the pick), and what the spawn does with the
//! hero id. `ow-client`'s select stage is a declared viewer default over the
//! read roster and rows. See `docs/formats/frontend-pipeline.md` sections
//! 4a-4c.

use ow_stu::types::{ClassRegistry, TypedField, Value};

use crate::guid::{self, types};
use crate::{classes, Error, GameData};

/// `STUGameMode`.
pub const STU_GAME_MODE: u32 = 0x494C33C4;
/// `STUGameModeTeam`.
pub const STU_GAME_MODE_TEAM: u32 = 0xD797394C;
/// `STUGameModeTeam.m_teams` / `STUGameRulesetGameMode.m_teams`.
pub const F_TEAMS: u32 = 0xD440A0F7;
/// `m_team` on `STUGameModeTeam` (a `TeamIndex`).
pub const F_TEAM: u32 = 0x33B0B2B6;
/// `m_availableHeroes`, on both team objects.
pub const F_AVAILABLE_HEROES: u32 = 0x59C86C8D;
/// `STU_3C14F504.m_heroes` / `STU_C45DE560.m_heroes`.
pub const F_HEROES: u32 = 0x056D3E39;
/// The restriction object's base class.
pub const STU_POOL: u32 = 0x5427ADC1;
/// The empty marker: no restriction, every hero selectable.
pub const STU_POOL_ALL: u32 = 0xD1DC7C55;
/// The classes that add a hero whitelist.
pub const STU_POOL_WHITELIST: [u32; 2] = [0x3C14F504, 0xC45DE560];

/// What a team's `m_availableHeroes` says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeroPool {
    /// The team carries no `m_availableHeroes` at all.
    Absent,
    /// `STU_D1DC7C55`: every hero may be picked.
    All,
    /// `STU_3C14F504` / `STU_C45DE560`: exactly these heroes.
    Whitelist(Vec<u64>),
    /// A restriction class this reader does not know. Carries its class hash so
    /// the caller can report it rather than assume.
    Unrecognized(u32),
}

/// One team of a mode, as far as the selector is concerned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamPool {
    /// The team's `TeamIndex` bits, when the team carries one. Team 0 of the
    /// practice mode omits it (the class default).
    pub team: Option<i32>,
    pub pool: HeroPool,
}

/// `STUGameModeTeam.m_controllerScript` / `m_bodyScript`: the statescript
/// graphs the mode runs for that team.
pub const F_CONTROLLER_SCRIPT: u32 = 0xA278_1AA4;
pub const F_BODY_SCRIPT: u32 = 0x76E8_C82A;

/// The scripts one team of a mode runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TeamScripts {
    pub team: Option<i32>,
    /// `m_controllerScript`, an asset ref to a `STUStatescriptGraph`.
    pub controller: Option<u64>,
    /// `m_bodyScript`.
    pub body: Option<u64>,
}

/// The per-team scripts of one `STUGameMode` asset.
///
/// The scripts are where the mode actually uses the hero: the practice range's
/// controller graphs carry a `STUStatescriptStateHero` node whose `hero` input
/// is `STUConfigVarDynamic` identifier `940` (see
/// [`crate::graph::hero_states_of`]). This reader only reports the refs; the
/// graph walk is the other module's.
pub fn mode_team_scripts(
    data: &GameData,
    mode: u64,
    reg: &ClassRegistry,
) -> Result<Vec<TeamScripts>, Error> {
    let mut stu = data.stu_of(mode)?;
    let mut out = Vec::new();
    let count = stu.instances.len();
    for w in 0..count {
        if stu.instances[w].hash != STU_GAME_MODE {
            continue;
        }
        let fields = stu.typed_fields(w, reg);
        let teams = match field(&fields, F_TEAMS).map(undefault) {
            Some(Value::Array(teams)) => teams.clone(),
            _ => continue,
        };
        for team in teams {
            let Value::Inline { start, bag } = undefault(&team) else {
                continue;
            };
            let (start, bag) = (*start, *bag);
            let tf = stu.typed_fields_at(STU_GAME_MODE_TEAM, start, bag, reg);
            let team_index = field(&tf, F_TEAM).and_then(|v| match undefault(v) {
                Value::Enum { raw, .. } => Some(*raw as i32),
                Value::Prim(_, bits) => Some(*bits as i32),
                _ => None,
            });
            let asset = |hash| match field(&tf, hash).map(undefault) {
                Some(Value::AssetRef(guid)) if *guid != 0 => Some(*guid),
                _ => None,
            };
            out.push(TeamScripts {
                team: team_index,
                controller: asset(F_CONTROLLER_SCRIPT),
                body: asset(F_BODY_SCRIPT),
            });
        }
    }
    Ok(out)
}

/// The per-team hero pools of one `STUGameMode` asset.
pub fn mode_team_pools(
    data: &GameData,
    mode: u64,
    reg: &ClassRegistry,
) -> Result<Vec<TeamPool>, Error> {
    let mut stu = data.stu_of(mode)?;
    let mut out = Vec::new();
    let count = stu.instances.len();
    for w in 0..count {
        if stu.instances[w].hash != STU_GAME_MODE {
            continue;
        }
        let fields = stu.typed_fields(w, reg);
        let teams = match field(&fields, F_TEAMS).map(undefault) {
            Some(Value::Array(teams)) => teams.clone(),
            _ => continue,
        };
        for team in teams {
            let Value::Inline { start, bag } = undefault(&team) else {
                out.push(TeamPool {
                    team: None,
                    pool: HeroPool::Unrecognized(0),
                });
                continue;
            };
            let (start, bag) = (*start, *bag);
            let tf = stu.typed_fields_at(STU_GAME_MODE_TEAM, start, bag, reg);
            let team_index = field(&tf, F_TEAM).and_then(|v| match undefault(v) {
                Value::Enum { raw, .. } => Some(*raw as i32),
                Value::Prim(_, bits) => Some(*bits as i32),
                _ => None,
            });
            out.push(TeamPool {
                team: team_index,
                pool: pool_of(&mut stu, reg, field(&tf, F_AVAILABLE_HEROES)),
            });
        }
    }
    Ok(out)
}

fn pool_of(stu: &mut ow_stu::Stu, reg: &ClassRegistry, value: Option<&Value>) -> HeroPool {
    let Some(value) = value else {
        return HeroPool::Absent;
    };
    let Value::Instance(idx) = undefault(value) else {
        return HeroPool::Unrecognized(0);
    };
    let idx = *idx;
    let hash = stu.instances[idx].hash;
    if hash == STU_POOL_ALL {
        return HeroPool::All;
    }
    if !STU_POOL_WHITELIST.contains(&hash) {
        return HeroPool::Unrecognized(hash);
    }
    let fields = stu.typed_fields(idx, reg);
    let heroes = match field(&fields, F_HEROES).map(undefault) {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| match undefault(item) {
                Value::AssetRef(guid) => Some(*guid),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    HeroPool::Whitelist(heroes)
}

/// `STUHero.m_0EDCE350`: the hero's display-string asset (type `0x07C`).
///
/// The field sits at class offset `48` in the generated `STUHero` the reference
/// checkout declares, and every playable hero on this install carries it
/// (measured by `tests/hero_select_real.rs`).
pub const F_HERO_NAME: u32 = 0x0EDC_E350;
/// `STUHero.m_64DC571F` (class offset `904`, one byte): the hero-is-playable
/// marker. The reference tooling reads it as `IsHero` and separates player
/// heroes from NPC heroes with it (`DataTool/DataModels/Hero/Hero.cs:53`); on
/// this install it is set on exactly the heroes that name themselves.
pub const F_HERO_PLAYABLE: u32 = 0x64DC_571F;
/// The asset type a display string decodes from (`teString`).
pub const TYPE_DISPLAY_STRING: u16 = 0x07C;

/// One hero a mode offers, with the round-trip data the selector needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeroChoice {
    /// The hero asset GUID (`0x02E0...`, type `0x075`).
    pub guid: u64,
    /// The hero's asset index ([`crate::guid::index_of`]), which is what
    /// `ow-scene --hero` and `ow-client --hero` take.
    pub index: u32,
    /// The hero's own display name, when its string asset reads.
    pub name: Option<String>,
    /// `STUHero.m_64DC571F`: the playable-hero marker. A mode's open roster
    /// contains NPC heroes too; a character selector offers the playable ones.
    pub playable: bool,
}

impl HeroChoice {
    /// The label a picker shows: the name, or the index when no name reads.
    pub fn label(&self) -> String {
        match &self.name {
            Some(name) => name.clone(),
            None => format!("hero {}", self.index),
        }
    }
}

/// A display string asset, read as text.
///
/// `STUHero.m_0EDCE350` names a `teString` payload (type `0x07C`); the text
/// starts after the two-byte header and ends before the final NUL. The trailing
/// space and the non-breaking space are the two characters the reference's
/// clean-name helper strips (`DataTool/Helper/IO.cs`, `GetCleanString`), so the
/// same two are stripped here and no other transformation is applied.
fn read_display_name(data: &GameData, string: u64) -> Option<String> {
    let bytes = data.open_asset(string).ok()?;
    let text = crate::text::parse_display_text(&bytes).ok()?;
    let name = text
        .value
        .replace('\u{a0}', " ")
        .trim_end_matches(' ')
        .to_string();
    (!name.is_empty()).then_some(name)
}

/// A hero asset's selector fields, read from one parse of the asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeroInfo {
    pub guid: u64,
    pub index: u32,
    /// `STUHero.m_0EDCE350`'s string asset, decoded.
    pub name: Option<String>,
    /// `STUHero.m_64DC571F` present and non-zero.
    pub playable: bool,
    /// `STUHero.m_gameplayEntity`: the asset the mover component lives in.
    pub entity: Option<u64>,
}

/// Read a hero asset's selector fields (`0x075`).
///
/// The hero asset must parse and carry one `STUHero` instance; a value that
/// cannot be read is an error rather than a partially-filled hero.
pub fn hero_info(data: &GameData, hero: u64) -> Result<HeroInfo, Error> {
    let mut stu = data.stu_of(hero)?;
    let inst = stu.instance(classes::HERO).ok_or_else(|| {
        Error::NotFound(format!("hero asset 0x{hero:016x} has no STUHero instance"))
    })?;
    let entity = stu
        .read_asset_ref(inst, classes::hero::GAMEPLAY_ENTITY)
        .filter(|entity| *entity != 0);
    let string = stu
        .read_asset_ref(inst, F_HERO_NAME)
        .filter(|string| *string != 0 && guid::type_of(*string) == TYPE_DISPLAY_STRING);
    // The marker is one byte; `u32_at` is the narrowest read the STU layer
    // offers, so only its low byte is the value.
    let playable = stu
        .field_bytes(inst, F_HERO_PLAYABLE)
        .and_then(|(offset, _)| {
            let at = stu.payload_start(inst) + offset;
            stu.u32_at(at)
        })
        .is_some_and(|raw| raw & 0xFF != 0);
    drop(stu);
    Ok(HeroInfo {
        guid: hero,
        index: guid::index_of(hero),
        name: string.and_then(|string| read_display_name(data, string)),
        playable,
        entity,
    })
}

/// A hero's display name, read from its own string asset.
pub fn hero_name(data: &GameData, hero: u64) -> Option<String> {
    hero_info(data, hero).ok()?.name
}

/// Whether the hero asset carries the playable marker (`STUHero.m_64DC571F`).
pub fn hero_is_playable(data: &GameData, hero: u64) -> Option<bool> {
    hero_info(data, hero).ok().map(|info| info.playable)
}

/// The hero's gameplay entity, the asset its mover component lives in.
pub fn hero_entity(data: &GameData, hero: u64) -> Option<u64> {
    hero_info(data, hero).ok()?.entity
}

/// Every hero a mode offers, in asset-index order.
///
/// The set is the mode's own rules: a team that carries the empty marker makes
/// the mode open (every hero asset), and a whitelist names exactly its heroes.
/// A team with no pool at all, or one whose pool class this reader does not
/// know, is an error rather than a silently widened roster.
///
/// The names come from each hero asset's own string, so this opens every hero
/// asset of an open mode once. It does not read any model or texture.
pub fn selectable_heroes(
    data: &GameData,
    mode: u64,
    reg: &ClassRegistry,
) -> Result<Vec<HeroChoice>, Error> {
    let teams = mode_team_pools(data, mode, reg)?;
    let mut guids: Vec<u64> = Vec::new();
    let mut open = false;
    for team in &teams {
        match &team.pool {
            HeroPool::All => open = true,
            HeroPool::Whitelist(heroes) => guids.extend_from_slice(heroes),
            HeroPool::Absent => {
                return Err(Error::NotFound(format!(
                    "mode 0x{mode:016x} team {:?} declares no m_availableHeroes; the class default is unread, so the roster is not widened",
                    team.team
                )))
            }
            HeroPool::Unrecognized(hash) => {
                return Err(Error::NotFound(format!(
                    "mode 0x{mode:016x} team {:?} names hero-pool class 0x{hash:08X}, which this reader does not know",
                    team.team
                )))
            }
        }
    }
    if open {
        guids = data.guids_of_type(types::HERO);
    }
    guids.sort_unstable();
    guids.dedup();

    let mut out = Vec::with_capacity(guids.len());
    for guid in guids {
        if guid::type_of(guid) != types::HERO {
            return Err(Error::NotFound(format!(
                "mode 0x{mode:016x} names 0x{guid:016x} as a hero, but its type is 0x{:03X}",
                guid::type_of(guid)
            )));
        }
        let info = hero_info(data, guid)?;
        out.push(HeroChoice {
            guid: info.guid,
            index: info.index,
            name: info.name,
            playable: info.playable,
        });
    }
    Ok(out)
}

/// Reduce a hero name to the form a pick is matched in.
///
/// Case, spaces and punctuation are ignored, so `D.Va`, `d.va`, `DVA` and
/// `D Va` are one name. No other folding is applied.
pub fn normalize_name(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Resolve a picker's text to one hero of the roster.
///
/// `spec` is a hero asset index in decimal (what the picker prints) or a hero
/// name in any spelling [`normalize_name`] folds. An index outside the roster,
/// a name no hero carries, and two heroes on one name are each an error with
/// the roster in the message: a pick is never guessed.
pub fn resolve_hero<'a>(heroes: &'a [HeroChoice], spec: &str) -> Result<&'a HeroChoice, Error> {
    let trimmed = spec.trim();
    if !trimmed.is_empty() && trimmed.chars().all(|c| c.is_ascii_digit()) {
        let index: u32 = trimmed.parse().map_err(|_| {
            Error::NotFound(format!(
                "hero index {trimmed} is not a number this roster has"
            ))
        })?;
        return heroes.iter().find(|h| h.index == index).ok_or_else(|| {
            Error::NotFound(format!(
                "no hero with index {index} is selectable here; the roster is {}",
                roster_indexes(heroes)
            ))
        });
    }
    let wanted = normalize_name(trimmed);
    if wanted.is_empty() {
        return Err(Error::NotFound("empty hero name".into()));
    }
    let hits: Vec<&HeroChoice> = heroes
        .iter()
        .filter(|h| h.name.as_deref().map(normalize_name).as_deref() == Some(wanted.as_str()))
        .collect();
    match hits.as_slice() {
        [one] => Ok(one),
        [] => Err(Error::NotFound(format!(
            "no selectable hero is named {trimmed:?}; the roster is {}",
            roster_names(heroes)
        ))),
        many => Err(Error::NotFound(format!(
            "{trimmed:?} names {} selectable heroes ({}); use an index instead",
            many.len(),
            many.iter()
                .map(|h| format!("{} {}", h.index, h.label()))
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

fn roster_indexes(heroes: &[HeroChoice]) -> String {
    heroes
        .iter()
        .map(|h| h.index.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn roster_names(heroes: &[HeroChoice]) -> String {
    const SHOWN: usize = 12;
    let mut names: Vec<String> = heroes.iter().take(SHOWN).map(|h| h.label()).collect();
    if heroes.len() > SHOWN {
        names.push(format!(
            "... ({} total; list them with --list-heroes)",
            heroes.len()
        ));
    }
    names.join(", ")
}

fn field(fields: &[TypedField], hash: u32) -> Option<&Value> {
    fields.iter().find(|f| f.hash == hash).map(|f| &f.value)
}

fn undefault(value: &Value) -> &Value {
    match value {
        Value::Defaulted(inner) => undefault(inner),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roster() -> Vec<HeroChoice> {
        vec![
            HeroChoice {
                guid: 0x02E0_0000_0000_0002,
                index: 2,
                name: Some("Reaper".into()),
                playable: true,
            },
            HeroChoice {
                guid: 0x02E0_0000_0000_0003,
                index: 3,
                name: Some("Tracer".into()),
                playable: true,
            },
            HeroChoice {
                guid: 0x02E0_0000_0000_007A,
                index: 0x7A,
                name: Some("D.Va".into()),
                playable: true,
            },
            HeroChoice {
                guid: 0x02E0_0000_0000_00DD,
                index: 0xDD,
                name: None,
                playable: false,
            },
        ]
    }

    #[test]
    fn a_name_matches_in_any_spelling() {
        let roster = roster();
        for spec in [
            "Tracer", "tracer", "TRACER", " Tracer ", "d.va", "DVA", "D Va",
        ] {
            let wanted = if spec.to_ascii_lowercase().contains("va") {
                0x7A
            } else {
                3
            };
            assert_eq!(resolve_hero(&roster, spec).unwrap().index, wanted, "{spec}");
        }
    }

    #[test]
    fn an_index_selects_exactly_that_hero() {
        let roster = roster();
        assert_eq!(resolve_hero(&roster, "2").unwrap().label(), "Reaper");
        assert_eq!(resolve_hero(&roster, "221").unwrap().index, 0xDD);
    }

    #[test]
    fn an_unknown_pick_is_an_error_not_a_default() {
        let roster = roster();
        let unknown = resolve_hero(&roster, "Winston").unwrap_err().to_string();
        assert!(unknown.contains("no selectable hero is named"), "{unknown}");
        let index = resolve_hero(&roster, "99").unwrap_err().to_string();
        assert!(index.contains("no hero with index 99"), "{index}");
        assert!(resolve_hero(&roster, "   ").is_err());
    }

    #[test]
    fn an_unnamed_hero_still_has_a_label_and_an_index_pick() {
        let roster = roster();
        assert_eq!(resolve_hero(&roster, "221").unwrap().label(), "hero 221");
    }
}

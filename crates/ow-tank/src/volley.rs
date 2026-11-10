//! Reading a weapon graph's volley: the node that fires, and what it declares.
//!
//! The weapon chain ends in a statescript graph (see [`crate::weapons`]). The
//! node that actually fires is `STUStatescriptStateWeaponVolley`
//! ([`crate::classes::VOLLEY_STATE`]); its config vars are the per-weapon
//! numbers a simulation needs. The reference types name five of them
//! (`numShotsPerSecond`, `numProjectilesPerShot`, `projectileSpeed`,
//! `projectileLifetime`, `aimID`); four more carry a `?`-suffixed reading
//! assigned by value-shape analysis across the install's heroes (`castLength?`,
//! `minShots?`, `maxShots?`, `enabled?` — the distributions are in
//! `docs/formats/weapons.md`, and the `?` stays until a second source agrees).
//!
//! This module groups a graph's literals by the node that owns them and walks
//! one level of the volley's children:
//!
//! ```text
//! volley state
//!   literal config vars        (field hash -> value, named or ?-named)
//!   m_projectileMotions        each element's own two config vars
//!   m_projectileEntity         entity-definition reference and companions
//!   m_5EC1EB38                 muzzle block, inline
//! ```
//!
//! The raw `m_projectileMotions` walk only sees elements the instance table
//! links back to the volley (embedder fields) and matches the base
//! `STUStatescriptWeaponProjectileMotion` hash exactly. Motions stored as bare
//! instance indices of a subclass — every one sampled on the install — are
//! invisible to it; [`motions_of_typed`] reads the same array through the
//! typed layer instead (`ow_stu::types`, which follows the stored indices
//! rather than the embedder back-links), and [`is_motion_shaped`] recognises
//! a subclass element by the motion fields it declares. The agreement between
//! the two walks is cross-checked by `tests/stu_typed_real.rs`.
//!
//! Nothing here decides what a number *means*; it reports what the file
//! declares, grouped the way the runtime groups it, so the meaning stays a
//! documented reading rather than a code assumption.

use std::collections::BTreeMap;

use crate::classes::{
    self, config_var_dynamic, projectile_motion, volley, CONFIG_VAR_DYNAMIC, PROJECTILE_MOTION,
    VOLLEY_STATE,
};
use crate::entity::Value;
use crate::graph::literal_of;
use ow_stu::Stu;

/// `m_ECDAB602`: a boolean flag the reference types do not name, carried on the
/// volley state beside the named fields. Its declaration site is the volley
/// state's own field table (it arrives through the instance table like every
/// other ConfigVar child); the reading `enabled?` is value-shape only — true
/// on 128 of the 153 declaring volleys in the install census — so the `?`
/// stays. Declared here rather than in [`crate::classes`] because that table
/// is frozen to what the code already depends on.
pub const VOLLEY_ENABLED: u32 = 0xECDA_B602;

/// `STU_E1A5B579`: the projectile-motion subclass every typed-only motion
/// sampled on the install carries. It keeps the base's `0x30A41C77` (always a
/// `STUConfigVarBool` child with no stored `m_value` in the samples) and
/// replaces `0x468EC3BE` — never carried by any sampled motion — with its own
/// fields, of which `0x599AB8D7` is the measured one.
pub const PROJECTILE_MOTION_SUBCLASS: u32 = 0xE1A5_B579;

/// Fields of the motion subclass (`0xE1A5B579`). Only the gravity reading has
/// a value shape behind it (`f 9.8` on three heroes); the rest are reported by
/// hash in the install census (`docs/formats/weapons.md`) with no names.
pub mod motion_subclass {
    /// `m_599AB8D7` — `gravity?` (assumed): `f 9.8` on every motion that
    /// stores it (5 motions on heroes 5, 6 and 810), the Earth-gravity
    /// signature a projectile-motion gravity term would carry.
    pub const GRAVITY: u32 = 0x599A_B8D7;
    /// `m_D0C4CF49` — boolean flag, true where stored; meaning unassigned.
    pub const FLAG: u32 = 0xD0C4_CF49;
}

/// One volley state and everything it declares.
#[derive(Debug, Clone)]
pub struct Volley {
    /// Instance index of the volley state in the graph's instance table.
    pub state: usize,
    /// Direct `(field hash, literal)` pairs of the volley state, sorted by
    /// hash. Includes `Unread` values, whose class hash is all that is known.
    pub literals: Vec<(u32, Value)>,
    /// `(field hash, child class hash, child instance)` for children that are
    /// not literals — arrays and embedded instances, kept so the shape is
    /// visible.
    pub children: Vec<(u32, u32, usize)>,
    /// The projectile-motion elements: each is one projectile's motion record,
    /// with its own `(field hash, literal)` pairs.
    pub motions: Vec<Motion>,
}

/// One projectile-motion element of a volley.
#[derive(Debug, Clone)]
pub struct Motion {
    /// Instance index of the motion element.
    pub instance: usize,
    /// Its `(field hash, literal)` pairs, sorted by hash.
    pub literals: Vec<(u32, Value)>,
}

impl Motion {
    pub fn get(&self, field: u32) -> Option<Value> {
        self.literals
            .iter()
            .find(|(h, _)| *h == field)
            .map(|(_, v)| *v)
    }
}

impl Volley {
    /// The literal under a field hash, if declared.
    pub fn get(&self, field: u32) -> Option<Value> {
        self.literals
            .iter()
            .find(|(h, _)| *h == field)
            .map(|(_, v)| *v)
    }

    /// The literal under a field hash as a number, for firing figures.
    pub fn number(&self, field: u32) -> Option<f32> {
        self.get(field).and_then(Value::as_f32)
    }
}

/// Children of an instance, in instance order: `(field hash, child index)`.
fn children_of(stu: &Stu) -> BTreeMap<usize, Vec<(u32, usize)>> {
    let mut children: BTreeMap<usize, Vec<(u32, usize)>> = BTreeMap::new();
    for (index, info) in stu.instances.iter().enumerate() {
        if info.embedder_index >= 0 {
            children
                .entry(info.embedder_index as usize)
                .or_default()
                .push((info.embedder_field, index));
        }
    }
    children
}

/// Literal pairs of one instance's direct children, sorted by field hash.
fn literal_children(stu: &Stu, child_list: &[(u32, usize)]) -> Vec<(u32, Value)> {
    let mut out = Vec::new();
    for (field, index) in child_list {
        if let Some(value) = literal_of(stu, *index, stu.instances[*index].hash) {
            out.push((*field, value));
        }
    }
    out.sort_by_key(|(hash, _)| *hash);
    out
}

/// The identifier of a dynamic config var (`0x4B093107`) instance, when the
/// instance is one. This is the runtime value the var binds to — its meaning
/// is not in the files, but its *identity* is, and identity is what lets two
/// heroes' rates be compared.
pub fn dynamic_identifier_of(stu: &Stu, instance: usize) -> Option<u32> {
    if stu.instances[instance].hash != CONFIG_VAR_DYNAMIC {
        return None;
    }
    let start = stu.payload_start(instance);
    let bag_index = stu.i32_at(start)?;
    let layout = stu.layout_at(start, bag_index);
    let field = layout
        .iter()
        .find(|f| f.hash == config_var_dynamic::IDENTIFIER)?;
    stu.u32_at(start + field.offset)
}

/// A short annotation for child classes whose values are computed rather than
/// stored, so a dump names the known cases instead of printing a bare hash.
/// Both readings come from the reference types (`docs/formats/weapons.md`).
pub fn child_note(class_hash: u32) -> Option<&'static str> {
    match class_hash {
        classes::CONFIG_VAR_EXPRESSION => Some("expression"),
        classes::CONFIG_VAR_FLOAT_SOURCE => Some("computed"),
        _ => None,
    }
}

/// Direct children of any instance: `(field hash, class hash, child index)`.
pub fn children_with_class(stu: &Stu, instance: usize) -> Vec<(u32, u32, usize)> {
    children_of(stu)
        .get(&instance)
        .map(|list| {
            list.iter()
                .map(|(field, index)| (*field, stu.instances[*index].hash, *index))
                .collect()
        })
        .unwrap_or_default()
}

/// Literal pairs among an instance's direct children, sorted by field hash.
pub fn literals_of_children(stu: &Stu, instance: usize) -> Vec<(u32, Value)> {
    let children = children_of(stu);
    literal_children(stu, &children.get(&instance).cloned().unwrap_or_default())
}

/// Whether a class is motion-shaped: it declares the two config-var fields of
/// `STUStatescriptWeaponProjectileMotion` (`0x30A41C77`, `0x468EC3BE`), either
/// as the base class or as a subclass. The typed motion array holds bare
/// instance indices whose classes are not the base hash — every typed-only
/// element sampled on the install is `0xE1A5B579`, a subclass with two extra
/// fields — so an exact-hash match would reject exactly the elements the raw
/// walk cannot see. A declaration check accepts them.
pub fn is_motion_shaped(reg: &ow_stu::types::ClassRegistry, class_hash: u32) -> bool {
    reg.field(class_hash, projectile_motion::A_30A41C77)
        .is_some()
        && reg
            .field(class_hash, projectile_motion::A_468EC3BE)
            .is_some()
}

/// The projectile-motion elements of one volley state, read through the typed
/// layer: the `m_projectileMotions` array's stored instance indices, each with
/// its own `(field hash, literal)` pairs.
///
/// This is the walk that sees what [`volleys_of`] cannot — elements with no
/// embedder back-link (stored top-level rather than embedded) and elements of
/// a motion subclass. Every raw-visible motion must appear here; an element
/// found only here is motion-shaped by [`is_motion_shaped`] or it is a data
/// surprise worth reporting, not silently accepting.
pub fn motions_of_typed(
    stu: &mut Stu,
    state: usize,
    reg: &ow_stu::types::ClassRegistry,
) -> Vec<Motion> {
    use ow_stu::types::Value as Typed;

    let Some(field) = stu.typed_field(state, volley::PROJECTILE_MOTIONS, reg) else {
        return Vec::new();
    };
    let indices: Vec<usize> = match field.unwrap_defaulted() {
        Typed::Array(items) => items
            .iter()
            .filter_map(|item| match item.unwrap_defaulted() {
                // A null element is an empty slot, reported by absence rather
                // than as a motion with no literals.
                Typed::Instance(index) => Some(*index),
                _ => None,
            })
            .collect(),
        // Null or missing: the volley declares no motions at all.
        _ => return Vec::new(),
    };
    indices
        .into_iter()
        .map(|instance| Motion {
            instance,
            literals: literals_of_children(stu, instance),
        })
        .collect()
}

/// Every volley state in a graph, with its literals, children and motions.
pub fn volleys_of(stu: &Stu) -> Vec<Volley> {
    let children = children_of(stu);
    let mut out = Vec::new();
    for state in stu.instances_of(VOLLEY_STATE) {
        let child_list = children.get(&state).cloned().unwrap_or_default();
        let literals = literal_children(stu, &child_list);

        let mut non_literals = Vec::new();
        let mut motions = Vec::new();
        for (field, index) in &child_list {
            let child_hash = stu.instances[*index].hash;
            if literal_of(stu, *index, child_hash).is_some() {
                continue;
            }
            non_literals.push((*field, child_hash, *index));
            // A motion element: read its own literal children now, so the
            // motion numbers stay attributed to this volley.
            if child_hash == PROJECTILE_MOTION {
                let inner = children.get(index).cloned().unwrap_or_default();
                motions.push(Motion {
                    instance: *index,
                    literals: literal_children(stu, &inner),
                });
            }
        }
        out.push(Volley {
            state,
            literals,
            children: non_literals,
            motions,
        });
    }
    out
}

/// Every modify-health data node in a graph, with its literals (the `amount`
/// among them when the graph declares it).
pub fn modify_health_of(stu: &Stu) -> Vec<(usize, Vec<(u32, Value)>)> {
    let children = children_of(stu);
    let mut out = Vec::new();
    for node in stu.instances_of(classes::MODIFY_HEALTH) {
        let child_list = children.get(&node).cloned().unwrap_or_default();
        out.push((node, literal_children(stu, &child_list)));
    }
    out
}

// ---- the weapon model: one volley's figures with their provenance --------
//
// Everything below is a *reading*, not an assumption in code: each figure
// carries where it came from, and a field the files do not resolve stays
// `None` with its reason. `docs/formats/weapons.md` carries the layout that
// motivated each hash.

/// Where a figure came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// A literal stored on the volley state.
    Literal,
    /// A `STUConfigVarDynamic` whose identifier was measured against the
    /// hero-setting domain (see `weapon_model_of`'s comment for the rule).
    Dynamic,
    /// A class this reader declines to evaluate — `ConfigVarExpression` /
    /// `ConfigVarFloatSource` — reported, not guessed at.
    Unresolved(u32),
}

/// One volley's firing figures, each with its provenance.
#[derive(Debug, Clone, Default)]
pub struct VolleySpec {
    /// Shots per second, when the volley declares a rate the files resolve.
    pub shots_per_second: Option<(f32, Source)>,
    /// Projectiles per trigger pull (pellets).
    pub projectiles_per_shot: Option<(u32, Source)>,
    /// The cast-length candidate `0xFEC435C6` in metres (`?`-hedge stays in
    /// the field name, the figure itself is the literal).
    pub cast_length: Option<f32>,
    /// Projectile speed, from the named `projectileSpeed` literal.
    pub projectile_speed: Option<f32>,
    /// Projectile lifetime, from the named `projectileLifetime` literal.
    pub projectile_lifetime: Option<f32>,
    /// Whether the volley declares the `enabled?` flag true.
    pub enabled: Option<bool>,
    /// Instance index of the volley state.
    pub state: usize,
}

/// The file-resolvable firing figures of the volley best qualified to be a
/// hero's primary fire.
///
/// "Best qualified" is a stated preference order, applied to the volleys the
/// graph holds — the graph can declare several and nothing names a primary:
/// 1. a volley that carries a *literal rate* other than 999 (999 = "as fast
///    as input allows", not a rate;
/// 2. otherwise a volley that carries a literal pellet count;
/// 3. otherwise the first volley of the graph.
///
/// Every figure carries its provenance — the literal classes are the only
/// evaluation this performs. A class the files do not resolve (an expression
/// or a dynamic binding) reports [`Source::Unresolved`] and its value stays
/// `None`: a default dressed as data is the failure this project steers
/// around (the same rule `numShotsPerSecond = 999` follows).
pub fn weapon_model_of(stu: &Stu) -> Option<VolleySpec> {
    let volleys = volleys_of(stu);
    if volleys.is_empty() {
        return None;
    }

    // Qualification: (has a non-999 literal rate, has a literal pellet count).
    // Ties keep graph order, so the pick is stable across runs.
    let key = |v: &Volley| {
        let rate = v
            .number(volley::NUM_SHOTS_PER_SECOND)
            .filter(|r| *r > 0.0 && *r < 999.0);
        let pellets = v.number(volley::NUM_PROJECTILES_PER_SHOT);
        (rate.is_some(), pellets.is_some())
    };
    let pick = volleys
        .iter()
        .enumerate()
        .max_by_key(|(i, v)| (key(v), std::cmp::Reverse(*i)))
        .map(|(i, _)| i)
        .unwrap_or(0);
    let volley = &volleys[pick];

    let shots_per_second = volley
        .number(volley::NUM_SHOTS_PER_SECOND)
        .filter(|r| *r > 0.0 && *r < 999.0)
        .map(|r| (r, Source::Literal));
    let projectiles_per_shot = volley
        .number(volley::NUM_PROJECTILES_PER_SHOT)
        .filter(|p| *p >= 1.0)
        .map(|p| (p as u32, Source::Literal));

    Some(VolleySpec {
        state: volley.state,
        shots_per_second,
        projectiles_per_shot,
        cast_length: volley.number(volley::CAST_LENGTH),
        projectile_speed: volley.number(volley::PROJECTILE_SPEED),
        projectile_lifetime: volley.number(volley::PROJECTILE_LIFETIME),
        enabled: volley.get(VOLLEY_ENABLED).and_then(|v| match v {
            Value::Bool(b) => Some(b),
            _ => None,
        }),
    })
}

/// One hero's file-resolvable weapon figures, merged across its graphs.
///
/// This is the sandbox-facing join: the primary volley of each weapon graph
/// (via [`weapon_model_of`]) ranked by the same qualification order, so the
/// pick is stable; plus the projectile-motion gravity and the graph's
/// modify-health literals as damage *candidates* (reported, never labelled —
/// the weapon→node join is still open, see `docs/formats/weapons.md`).
/// Fields the files do not resolve stay `None` with the caller's default in
/// place, never a dressed-up guess.
#[derive(Debug, Clone, Default)]
pub struct WeaponFigs {
    /// The graph asset the pick came from.
    pub graph: Option<u64>,
    /// The picked volley's figures.
    pub volley: VolleySpec,
    /// Whether any graph contributed a volley at all.
    pub has_volley: bool,
    /// Projectile-motion gravity (`m_599AB8D7`, `gravity?`) in m/s², when a
    /// typed motion of the picked volley stores it. Needs the class registry
    /// (`OW_OWLIB`); `None` without one.
    pub gravity: Option<f32>,
    /// Modify-health literals across the hero's weapon graphs as
    /// `(graph, node, field, value)` rows. Candidates only: ability/heal
    /// figures and trigger/scale sentinels are in the same pool.
    pub damage_candidates: Vec<(u64, usize, u32, f32)>,
}

/// The ranked pick of [`weapon_figs_of`]: a later graph replaces the best
/// only when its qualification key is strictly higher, so ties keep graph
/// order. Extracted as a pure function so the ranking is testable without an
/// install.
fn pick_best_volley(
    candidates: impl IntoIterator<Item = (u64, VolleySpec)>,
) -> Option<(u64, VolleySpec)> {
    let mut best_key = (false, false);
    let mut best: Option<(u64, VolleySpec)> = None;
    for (graph, spec) in candidates {
        let rate = spec.shots_per_second.is_some();
        let pellets = spec.projectiles_per_shot.is_some();
        if best.is_none() || (rate, pellets) > best_key {
            best_key = (rate, pellets);
            best = Some((graph, spec));
        }
    }
    best
}

/// A hero entity's weapon figures, merged across its weapon graphs.
///
/// Rank: a graph whose primary volley carries a literal non-999 rate first,
/// then one with a literal pellet count, then graph order. Damage candidates
/// are collected across *all* weapon graphs (not just the pick), sorted by
/// value, deduplicated, capped at 16 rows so a HUD line stays readable.
pub fn weapon_figs_of(data: &crate::GameData, entity: u64) -> WeaponFigs {
    let mut out = WeaponFigs::default();
    let weapons = crate::weapons::weapons_of(data, entity).unwrap_or_default();
    let mut candidates: Vec<(u64, VolleySpec)> = Vec::new();
    for w in &weapons {
        let Some(graph) = w.graph else { continue };
        let Ok(stu) = data.stu_of(graph) else {
            continue;
        };
        if let Some(spec) = weapon_model_of(&stu) {
            candidates.push((graph, spec));
        }
        for (node, literals) in modify_health_of(&stu) {
            for (field, value) in literals {
                let v = match value {
                    Value::Float(v) if v.is_finite() => v,
                    Value::Int(v) => v as f32,
                    _ => continue,
                };
                out.damage_candidates.push((graph, node, field, v));
            }
        }
    }
    if let Some((graph, spec)) = pick_best_volley(candidates) {
        out.volley = spec;
        out.graph = Some(graph);
        out.has_volley = true;
    }
    out.damage_candidates
        .sort_by(|a, b| a.3.partial_cmp(&b.3).unwrap_or(std::cmp::Ordering::Equal));
    out.damage_candidates
        .dedup_by(|a, b| a.0 == b.0 && a.1 == b.1 && a.2 == b.2 && (a.3 - b.3).abs() < 1e-6);
    out.damage_candidates.truncate(16);

    // Gravity via the typed motion array of the picked volley's graph. The
    // registry loads from the local OWLib checkout at runtime (nothing
    // vendored); without it there is no typed read and gravity stays None.
    if let Some(graph) = out.graph {
        let sources = crate::keytable::Sources::discover(
            &ow_casc::install::detect_root()
                .map(|r| vec![r])
                .unwrap_or_default(),
        );
        let registry = crate::classreg::ClassRegistry::load_from_sources(&sources);
        if !registry.is_empty() {
            let reg = registry.inner();
            if let Ok(mut stu) = data.stu_of(graph) {
                let state = out.volley.state;
                for m in motions_of_typed(&mut stu, state, reg) {
                    if let Some(Value::Float(g)) = m.get(motion_subclass::GRAVITY) {
                        if g.is_finite() && g > 0.0 && g < 100.0 {
                            out.gravity = Some(g);
                            break;
                        }
                    }
                }
            }
        }
    }
    out
}

/// Readable name for a volley field, from the named set this project reads.
///
/// Names without a `?` come from the reference types. Names with one are
/// value-shape readings across the install's heroes, documented in
/// `docs/formats/weapons.md`: `castLength?` (range-scale values, matching the
/// named `castLength` the raycast state carries), `minShots?`/`maxShots?`
/// (near-constant 1 with sentinel-scale 999/9999/99999 on the max side, and
/// paired 2/3 bursts on one hero), `enabled?` (boolean, true on 128 of the 153
/// volleys that declare it). Fields with no evidence for a meaning return
/// `None` rather than a guess.
pub fn volley_field_name(field: u32) -> Option<&'static str> {
    match field {
        volley::PROJECTILE_VISUAL => Some("projectileVisual"),
        volley::AIM_ID => Some("aimID"),
        volley::PROJECTILE_LIFETIME => Some("projectileLifetime"),
        volley::PROJECTILE_SPEED => Some("projectileSpeed"),
        volley::PROJECTILE_MOTIONS => Some("projectileMotions"),
        volley::PROJECTILE_ENTITY => Some("projectileEntity"),
        volley::NUM_PROJECTILES_PER_SHOT => Some("numProjectilesPerShot"),
        volley::NUM_SHOTS_PER_SECOND => Some("numShotsPerSecond"),
        volley::LAUNCH => Some("launch"),
        volley::IMPACT => Some("impact"),
        volley::CAST_LENGTH => Some("castLength?"),
        volley::A55BC904 => Some("minShots?"),
        volley::A17BE89B => Some("maxShots?"),
        VOLLEY_ENABLED => Some("enabled?"),
        _ => None,
    }
}

/// Readable name for a projectile-motion field.
///
/// The base-class readings stay hedged: on the install census the `0x30A41C77`
/// children are always `STUConfigVarBool` with no stored value (zero literals
/// in 48 motions) and `0x468EC3BE` is never carried at all, so neither has a
/// measured value shape. The subclass gravity term is the one measured field.
pub fn motion_field_name(field: u32) -> &'static str {
    match field {
        projectile_motion::A_30A41C77 => "0x30A41C77 = speed?",
        projectile_motion::A_468EC3BE => "0x468EC3BE = gravity?",
        motion_subclass::GRAVITY => "0x599AB8D7 = gravity?",
        _ => "unnamed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ow_stu::types::{ClassDef, ClassRegistry, DefaultValue, FieldDef, FieldType, ReaderKind};

    /// A minimal registry declaring the base motion class with its two fields
    /// plus a subclass mirroring the measured `0xE1A5B579` shape (a base link
    /// and two extra fields of its own), so the subclass check runs without
    /// an install.
    fn motion_registry() -> ClassRegistry {
        let mut def = ClassDef::new(
            PROJECTILE_MOTION,
            "STUStatescriptWeaponProjectileMotion".into(),
            24,
            None,
        );
        for hash in [projectile_motion::A_30A41C77, projectile_motion::A_468EC3BE] {
            def.add_field(FieldDef {
                hash,
                name: format!("m_{hash:08X}"),
                v1_offset: 0,
                size_hint: None,
                reader: ReaderKind::Embedded,
                ty: FieldType::Instance(crate::classes::CONFIG_VAR_FLOAT),
                default: DefaultValue::None,
            });
        }
        let mut sub = ClassDef::new(
            0xE1A5_B579,
            "STU_E1A5B579".into(),
            40,
            Some(PROJECTILE_MOTION),
        );
        for hash in [0x599A_B8D7, 0xD0C4_CF49] {
            sub.add_field(FieldDef {
                hash,
                name: format!("m_{hash:08X}"),
                v1_offset: 0,
                size_hint: None,
                reader: ReaderKind::Embedded,
                ty: FieldType::Instance(crate::classes::CONFIG_VAR_FLOAT),
                default: DefaultValue::None,
            });
        }
        let mut reg = ClassRegistry::new();
        reg.add_class(def);
        reg.add_class(sub);
        reg
    }

    #[test]
    fn volley_field_names_cover_the_rates() {
        assert_eq!(
            volley_field_name(volley::NUM_SHOTS_PER_SECOND),
            Some("numShotsPerSecond")
        );
        assert_eq!(
            volley_field_name(volley::NUM_PROJECTILES_PER_SHOT),
            Some("numProjectilesPerShot")
        );
        assert_eq!(
            volley_field_name(volley::PROJECTILE_SPEED),
            Some("projectileSpeed")
        );
        assert_eq!(
            volley_field_name(volley::PROJECTILE_LIFETIME),
            Some("projectileLifetime")
        );
        assert_eq!(volley_field_name(volley::AIM_ID), Some("aimID"));
        assert_eq!(volley_field_name(0xDEAD_BEEF), None);
    }

    #[test]
    fn hedged_volley_fields_keep_their_question_marks() {
        // The shape readings are hypotheses, so every one of them carries `?`.
        for field in [
            volley::CAST_LENGTH,
            volley::A55BC904,
            volley::A17BE89B,
            VOLLEY_ENABLED,
        ] {
            let name = volley_field_name(field).expect("hedged field has a name");
            assert!(name.contains('?'), "{name} lost its hedge");
        }
        assert_eq!(volley_field_name(volley::CAST_LENGTH), Some("castLength?"));
        assert_eq!(volley_field_name(volley::A55BC904), Some("minShots?"));
        assert_eq!(volley_field_name(volley::A17BE89B), Some("maxShots?"));
        assert_eq!(volley_field_name(VOLLEY_ENABLED), Some("enabled?"));
        // The impact descriptor is reference-typed, not hedged.
        assert_eq!(volley_field_name(volley::IMPACT), Some("impact"));
    }

    #[test]
    fn motion_names_stay_hedged() {
        assert!(motion_field_name(projectile_motion::A_30A41C77).contains('?'));
        assert!(motion_field_name(projectile_motion::A_468EC3BE).contains('?'));
        assert!(motion_field_name(motion_subclass::GRAVITY).contains('?'));
        assert_eq!(motion_field_name(0xDEAD_BEEF), "unnamed");
    }

    #[test]
    fn motion_shape_accepts_a_subclass_and_rejects_other_classes() {
        let reg = motion_registry();
        assert!(is_motion_shaped(&reg, PROJECTILE_MOTION));
        // The subclass declares only its own two extra fields; the motion
        // fields resolve through its base link, which is exactly the shape
        // the install carries. The check is by declared fields, never by
        // exact hash, so it accepts the subclass the raw walk rejects.
        assert!(is_motion_shaped(&reg, 0xE1A5_B579));
        // A class declaring nothing motion-like is not motion-shaped.
        assert!(!is_motion_shaped(&reg, 0xDEAD_BEEF));
    }

    #[test]
    fn computed_child_classes_are_named_not_numbered() {
        assert_eq!(
            child_note(classes::CONFIG_VAR_EXPRESSION),
            Some("expression")
        );
        assert_eq!(
            child_note(classes::CONFIG_VAR_FLOAT_SOURCE),
            Some("computed")
        );
        assert_eq!(child_note(0xDEAD_BEEF), None);
    }

    #[test]
    fn the_volley_pick_is_the_ranked_graph_not_the_last() {
        let spec = |rate: bool, pellets: bool| VolleySpec {
            shots_per_second: rate.then_some((1.0, Source::Literal)),
            projectiles_per_shot: pellets.then_some((1, Source::Literal)),
            ..VolleySpec::default()
        };

        // Equal keys keep graph order: the first graph wins, not the last.
        let pick = pick_best_volley([(0x1, spec(false, false)), (0x2, spec(false, false))]);
        assert_eq!(pick.unwrap().0, 0x1);

        // A later graph with a rate beats an earlier one without.
        let pick = pick_best_volley([(0x1, spec(false, true)), (0x2, spec(true, false))]);
        assert_eq!(pick.unwrap().0, 0x2);

        // A later graph with a weaker key does not replace the best.
        let pick = pick_best_volley([(0x1, spec(true, false)), (0x2, spec(false, true))]);
        assert_eq!(pick.unwrap().0, 0x1);

        // Pellets break a rate tie.
        let pick = pick_best_volley([(0x1, spec(true, false)), (0x2, spec(true, true))]);
        assert_eq!(pick.unwrap().0, 0x2);

        // No candidate at all.
        assert!(pick_best_volley(Vec::<(u64, VolleySpec)>::new()).is_none());
    }
}

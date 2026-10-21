//! A hero's weapons, and the state-script graphs their behaviour lives in.
//!
//! The chain so far:
//!
//! ```text
//! hero            m_gameplayEntity
//!   entity        STUWeaponComponent
//!     component   m_weapons: an inline array of STUWeaponDefinition
//!       weapon    m_graph: STUStatescriptGraphWithOverrides
//!         graph   a statescript graph asset (type 0x01B)
//! ```
//!
//! Reaching the graph is as far as this goes. The graph is a *program*: nodes,
//! links, input and output plugs, with numeric literals held in config vars. The
//! weapon's per-hero numbers — damage, fire interval, projectile speed — are in
//! there, but nothing so far identifies which node means what. Getting from
//! "there is a float whose value is 0.5" to "that is the fire interval" is the
//! remaining problem, and it is a bigger one than the pool reading was.
//!
//! What *is* checkable, and is checked here, is the chain itself: every weapon
//! definition must resolve to a graph asset that exists in the registry and is
//! of the graph type.
//!
//! ## Two shapes worth noting
//!
//! The weapon array is **inline**, and its first element is empty in the
//! reference install — a weapon slot with nothing in it. An implementation that
//! assumes elements are dense reads the following element's bytes as its own.
//!
//! The overrides wrapper is 12 bytes here, a leading `i32` then the graph
//! reference, where the generated type describes a 40-byte shape with the
//! reference first. Builds differ in this struct; the bytes were followed rather
//! than the type.

use crate::classes::{self, weapon_component, weapon_definition};
use crate::{guid, GameData, Result};
use ow_stu::Stu;

/// One weapon definition, as much of it as is currently understood.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Weapon {
    /// Index of the weapon within the component's array.
    pub slot: usize,
    /// The statescript graph asset holding this weapon's behaviour.
    pub graph: Option<u64>,
    /// The script asset, when the weapon declares one directly.
    pub script: Option<u64>,
}

impl Weapon {
    /// Whether the slot is empty, which happens in practice.
    pub fn is_empty(&self) -> bool {
        self.graph.is_none() && self.script.is_none()
    }
}

/// Weapons declared by a gameplay entity.
pub fn weapons_of(data: &GameData, entity: u64) -> Result<Vec<Weapon>> {
    let mut stu = data.stu_of(entity)?;
    Ok(weapons_in(&mut stu))
}

/// Weapons declared by an already-parsed entity asset.
pub fn weapons_in(stu: &mut Stu) -> Vec<Weapon> {
    let Some(component) = stu.instance(classes::WEAPON_COMPONENT) else {
        return Vec::new();
    };
    let elements = stu.inline_elements(component, weapon_component::WEAPONS);

    let mut out = Vec::with_capacity(elements.len());
    for (slot, (bag_index, start)) in elements.into_iter().enumerate() {
        if bag_index < 0 {
            // An empty slot. Reported rather than skipped, because the slot
            // numbering is what a caller indexes a hero's weapons by.
            out.push(Weapon {
                slot,
                graph: None,
                script: None,
            });
            continue;
        }

        let layout = stu.layout_at(start, bag_index);
        let mut graph = None;
        let mut script = None;
        for field in &layout {
            match field.hash {
                weapon_definition::GRAPH => {
                    // The overrides wrapper: a leading i32, then the reference.
                    graph = stu.asset_ref_at(start + field.offset + 4, field.hash);
                }
                weapon_definition::SCRIPT => {
                    script = stu.asset_ref_at(start + field.offset, field.hash);
                }
                _ => {}
            }
        }
        out.push(Weapon {
            slot,
            graph,
            script,
        });
    }
    out
}

/// Whether a graph reference points at an asset that exists and is a graph.
pub fn graph_is_present(data: &GameData, graph: u64) -> bool {
    data.registry().get(graph).is_some()
        && guid::type_of(graph) == classes::STATESCRIPT_GRAPH_ASSET_TYPE
}

/// The health-modification amounts a weapon graph declares, as `(node, field,
/// value)` rows.
///
/// This is the file-side end of the per-shot damage hunt: a graph's
/// `STUStatescriptModifyHealth` nodes carry their `m_amount` as a literal, and
/// those literals are unambiguous *as literals*. Whether one is weapon damage,
/// a heal, a trigger tick (`1e-05`), or a percentage-scale value (`10000`)
/// is decided by the operation enum and the graph wiring, which this does not
/// interpret — so the rows are reported, not labelled, and the per-ability
/// damage column stays hedged (see [`crate::entity::Ability`]).
/// Returns an empty list when the graph holds no modify-health node, which is
/// the common case for pure weapon graphs (their damage lands in ability
/// graphs the hero only reaches at runtime).
pub fn modify_health_amounts(
    data: &GameData,
    graph: u64,
) -> Result<Vec<(usize, u32, crate::entity::Value)>> {
    let stu = data.stu_of(graph)?;
    Ok(crate::volley::modify_health_of(&stu)
        .into_iter()
        .flat_map(|(node, literals)| {
            literals
                .into_iter()
                .map(move |(field, value)| (node, field, value))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_slot_is_reported_not_skipped() {
        let weapon = Weapon {
            slot: 0,
            graph: None,
            script: None,
        };
        assert!(weapon.is_empty());

        let weapon = Weapon {
            slot: 1,
            graph: Some(0x0580_0000_0000_0019),
            script: None,
        };
        assert!(!weapon.is_empty());
    }
}

/// A per-hero movement speed, in metres per second.
///
/// **Provisional.** The field is on `STUCharacterMoverComponent` and reads 5.5
/// for 117 of 119 heroes, 11.0 for one and 5.0 for another; the hero that
/// differs is the one with a speed ability, which is consistent with this being
/// movement speed. What is *not* established is whether it is a base speed, a
/// maximum, or something else, so treat it as a hint rather than a fact.
pub const MOVER_COMPONENT: u32 = 0x6EFD_BD73;
/// The field that reads as a speed.
pub const MOVER_SPEED_FIELD: u32 = 0x031F_E7EE;

/// Read a hero entity's movement speed, if the component is present.
pub fn move_speed(data: &GameData, entity: u64) -> Result<Option<f32>> {
    let mut stu = data.stu_of(entity)?;
    let Some(instance) = stu.instance(MOVER_COMPONENT) else {
        return Ok(None);
    };
    Ok(stu.read_f32(instance, MOVER_SPEED_FIELD))
}

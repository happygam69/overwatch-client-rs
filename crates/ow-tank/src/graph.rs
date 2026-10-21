//! Reading named parameters out of a statescript graph.
//!
//! A graph's states own literals, and each literal's owning *field* names it.
//! Joining the field hash to a name ([`crate::fieldnames`]) is what turns a bare
//! float into a parameter.
//!
//! The join is done through the instance table rather than by decoding each
//! field: an embedded instance records the field hash it arrived through, so for
//! every instance that is a literal, the owning field hash is already known and
//! only the name has to be looked up.

use std::collections::HashMap;

use crate::classes::{self, config_var};
use crate::entity::Value;
use crate::fieldnames::FieldNames;

use ow_stu::Stu;

/// One named parameter of a graph state.
#[derive(Debug, Clone)]
pub struct Param {
    /// Index of the state instance that owns the literal.
    pub state: usize,
    /// Class hash of that state.
    pub state_hash: u32,
    /// Whether the owning class's name is known.
    pub state_name: Option<String>,
    /// The field hash the literal arrived through.
    pub field_hash: u32,
    /// The field's name, when the reference types name it.
    pub field_name: Option<String>,
    /// The field's declared type, which says whether the field held a value or
    /// pointed at another instance.
    pub field_type: Option<String>,
    pub value: Value,
    /// Class of the literal.
    pub value_hash: u32,
}

impl Param {
    /// `class.field` when both are named, for display.
    pub fn describe(&self) -> String {
        let class = self
            .state_name
            .clone()
            .unwrap_or_else(|| format!("STU_0x{:08X}", self.state_hash));
        match &self.field_name {
            Some(field) => format!("{class}.{field}"),
            None => format!("{class}.0x{:08X}", self.field_hash),
        }
    }
}

/// Named parameters of every state in a graph.
///
/// `names` supplies field names; without them this returns nothing useful, so a
/// caller that has no naming source should skip the call rather than print
/// hashes.
pub fn params_of(
    stu: &Stu,
    class_names: &crate::classes::ClassMap,
    names: &FieldNames,
) -> Vec<Param> {
    // (owner index, field hash) -> embedded instance index
    let mut owned: HashMap<(usize, u32), usize> = HashMap::new();
    for (index, info) in stu.instances.iter().enumerate() {
        if info.embedder_index >= 0 {
            owned.insert((info.embedder_index as usize, info.embedder_field), index);
        }
    }

    let mut out = Vec::new();
    for ((owner, field_hash), value_index) in &owned {
        let owner_hash = stu.instances[*owner].hash;
        // Only report fields the reference types actually name; a hash-shaped
        // placeholder is not an answer.
        let Some(field_name) = names.get(owner_hash, *field_hash) else {
            continue;
        };
        let value_hash = stu.instances[*value_index].hash;
        let value = literal_of(stu, *value_index, value_hash);
        let Some(value) = value else {
            continue;
        };
        out.push(Param {
            state: *owner,
            state_hash: owner_hash,
            state_name: class_names.name(owner_hash).map(str::to_string),
            field_hash: *field_hash,
            field_name: Some(field_name.short_name().to_string()),
            field_type: Some(field_name.type_name.clone()),
            value,
            value_hash,
        });
    }
    // Stable order, so runs are comparable across builds.
    out.sort_by_key(|p| (p.state, p.field_hash));
    out
}

/// `STUStatescriptStateAbility`, the node that gates an ability behind its
/// cooldown and targeting. Its hash is from the reference types
/// (`TankLib/STU/Types/STUStatescriptStateAbility.cs`); the cooldown itself is
/// a marker variable (`m_out_CooldownVar`, `0xBBC9EFFF`, class `0x076E0DBA`),
/// not a literal, so no field read here yields seconds — the literals are the
/// ability's other knobs, reported with hashes, and the cooldown column stays
/// `None` (see [`crate::entity::Ability`]).
pub const ABILITY_STATE: u32 = 0xC38B_92B4;

/// Every ability state in a graph, with its literals.
///
/// This is [`all_params_of`] narrowed to the ability-state class, so the same
/// owner-link join applies and the unnamed fields stay in: the cooldown is
/// among the children only as a non-literal marker variable
/// (`m_out_CooldownVar`), never as a number, which is why no literal here is
/// seconds and the per-ability cooldown column stays hedged (see
/// [`crate::entity::Ability`]).
pub fn ability_nodes_of(stu: &Stu) -> Vec<(usize, Vec<(u32, Value)>)> {
    let names = crate::classes::ClassMap::default();
    let mut by_node: std::collections::BTreeMap<usize, Vec<(u32, Value)>> =
        std::collections::BTreeMap::new();
    for param in all_params_of(stu, &names) {
        if param.state_hash == ABILITY_STATE {
            by_node
                .entry(param.state)
                .or_default()
                .push((param.field_hash, param.value));
        }
    }
    by_node.into_iter().collect()
}

/// A literal read out of one instance, by its class.
///
/// `instance` is the instance's index; `class_hash` is its class, deciding how
/// the value is decoded. Returns `None` when the instance has no `m_value`
/// field at all; classes this reader does not decode come back as
/// [`Value::Unread`] with their class hash.
pub fn literal_of(stu: &Stu, instance: usize, class_hash: u32) -> Option<Value> {
    let start = stu.payload_start(instance);
    let bag_index = stu.i32_at(start)?;
    let layout = stu.layout_at(start, bag_index);
    let field = layout.iter().find(|f| f.hash == config_var::VALUE)?;
    let at = start + field.offset;

    match class_hash {
        classes::CONFIG_VAR_FLOAT => stu.f32_at(at).map(Value::Float),
        classes::CONFIG_VAR_INT => stu.i32_at(at).map(Value::Int),
        classes::CONFIG_VAR_BOOL => stu.u32_at(at).map(|v| Value::Bool(v != 0)),
        other => Some(Value::Unread(other)),
    }
}

/// Literals of every state in a graph, named or not.
///
/// [`params_of`] reports only fields the reference types name, which is right
/// for a display of known parameters and wrong for a hunt: the unnamed fields
/// carry the same literals, and *which* hash agrees with a value across all
/// heroes is itself evidence. Field hash joins happen at the caller.
pub fn all_params_of(stu: &Stu, class_names: &crate::classes::ClassMap) -> Vec<Param> {
    let mut owned: HashMap<(usize, u32), usize> = HashMap::new();
    for (index, info) in stu.instances.iter().enumerate() {
        if info.embedder_index >= 0 {
            owned.insert((info.embedder_index as usize, info.embedder_field), index);
        }
    }

    let mut out = Vec::new();
    for ((owner, field_hash), value_index) in &owned {
        let owner_hash = stu.instances[*owner].hash;
        let value_hash = stu.instances[*value_index].hash;
        let value = literal_of(stu, *value_index, value_hash);
        let Some(value) = value else {
            continue;
        };
        out.push(Param {
            state: *owner,
            state_hash: owner_hash,
            state_name: class_names.name(owner_hash).map(str::to_string),
            field_hash: *field_hash,
            field_name: None,
            field_type: None,
            value,
            value_hash,
        });
    }
    out.sort_by_key(|p| (p.state, p.field_hash));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ability_state_hash_is_the_reference_one() {
        // Pinned so an accidental edit fails loudly: the ability-node walk
        // keys every row off this hash.
        assert_eq!(ABILITY_STATE, 0xC38B_92B4);
    }

    #[test]
    fn names_are_shown_without_the_m_prefix() {
        let param = Param {
            state: 0,
            state_hash: 0xEED6_D952,
            state_name: Some("STUStatescriptStateWeaponVolley".into()),
            field_hash: 0x02F3_F5D4,
            field_name: Some("numShotsPerSecond".into()),
            field_type: Some("STUConfigVar".into()),
            value: Value::Int(20),
            value_hash: 0x12E4_34A1,
        };
        assert_eq!(
            param.describe(),
            "STUStatescriptStateWeaponVolley.numShotsPerSecond"
        );
    }

    #[test]
    fn an_unnamed_field_falls_back_to_its_hash() {
        let param = Param {
            state: 0,
            state_hash: 0xEED6_D952,
            state_name: None,
            field_hash: 0x02F3_F5D4,
            field_name: None,
            field_type: None,
            value: Value::Int(1),
            value_hash: 0x12E4_34A1,
        };
        assert_eq!(param.describe(), "STU_0xEED6D952.0x02F3F5D4");
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

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

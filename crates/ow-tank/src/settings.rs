//! Reading the per-hero tuning values.
//!
//! Hero settings are the numbers a player can move in a custom game. They are
//! valuable here for two reasons beyond their own content: they exercise the
//! whole reading path end to end, and they reveal the *vocabulary* of tunable
//! quantities per hero.
//!
//! They are **not** the base combat statistics. A setting's default is a
//! multiplier, 1 most of the time, not a damage figure. Base damage, health and
//! cooldowns live in the state-script graphs and components, which are a
//! separate problem.
//!
//! ## A setting only stores what differs from its class default
//!
//! This is the trap. Most settings omit `min`, `max` and `step` entirely, so a
//! reader that treats a missing field as zero reports nonsense bounds like
//! "0 to 0". A field absent from the instance means the value declared on the
//! class applies; [`crate::classes::HERO_SETTING_FLOAT_DEFAULTS`] carries those.
//!
//! ## Settings nest, but not in an array
//!
//! A settings asset holds hundreds of instances: the root set, one record per
//! hero, and the settings themselves. They are all listed in the instance table
//! and tagged with what embedded them, so they can be read directly. It is not
//! necessary to walk the embedded arrays that link them.

use crate::classes::{
    hero_setting, HERO_SETTING_FLOAT, HERO_SETTING_FLOAT_DEFAULTS, HERO_SETTINGS,
};
use crate::{GameData, Result};
use ow_stu::Stu;

/// One tunable float, with its bounds already resolved.
#[derive(Debug, Clone, Copy)]
pub struct HeroSettingFloat {
    /// GUID of the settings asset this came from.
    pub asset: u64,
    /// Index of the instance within that asset.
    pub instance: usize,
    /// `m_default`.
    pub default: f32,
    pub min: f32,
    pub max: f32,
    pub step: f32,
    /// Whether each of default/min/max/step was present rather than defaulted.
    pub explicit: [bool; 4],
    /// Identifier token. Note this is a *token*, not an asset: no asset of the
    /// identifier type is shipped, so it cannot be resolved to anything.
    pub identifier: Option<u64>,
    /// Display-text asset for the setting's name.
    pub name: Option<u64>,
    /// Category asset reference.
    pub category: Option<u64>,
}

impl HeroSettingFloat {
    /// Whether the bounds are the class defaults rather than authored.
    pub fn bounds_are_defaulted(&self) -> bool {
        !self.explicit[1] && !self.explicit[2]
    }
}

/// Read every float-valued setting out of one settings asset.
pub fn read_settings_asset(data: &GameData, asset: u64) -> Result<Vec<HeroSettingFloat>> {
    let mut stu = data.stu_of(asset)?;
    Ok(read_from_stu(&mut stu, asset))
}

/// Read every float-valued setting out of an already-parsed asset.
pub fn read_from_stu(stu: &mut Stu, asset: u64) -> Vec<HeroSettingFloat> {
    let indices = stu.instances_of(HERO_SETTING_FLOAT);
    let mut out = Vec::with_capacity(indices.len());

    for instance in indices {
        let fields = stu.fields(instance).to_vec();
        let has = |hash: u32| fields.iter().any(|f| f.hash == hash);

        // Start from the class defaults, then let anything present override.
        let mut values = [0f32; 4];
        let mut explicit = [false; 4];
        for (slot, (hash, fallback)) in HERO_SETTING_FLOAT_DEFAULTS.iter().enumerate() {
            match stu.read_f32(instance, *hash) {
                Some(v) => {
                    values[slot] = v;
                    explicit[slot] = true;
                }
                None => values[slot] = *fallback,
            }
        }

        out.push(HeroSettingFloat {
            asset,
            instance,
            default: values[0],
            min: values[1],
            max: values[2],
            step: values[3],
            explicit,
            identifier: stu.read_asset_ref(instance, hero_setting::IDENTIFIER),
            name: stu.read_asset_ref(instance, hero_setting::NAME),
            category: if has(hero_setting::CATEGORY) {
                stu.read_asset_ref(instance, hero_setting::CATEGORY)
            } else {
                None
            },
        });
    }
    out
}

/// Every settings asset in the installation, and the floats it contains.
///
/// Assets whose root instance is not the hero-settings class are skipped: the
/// type also covers other settings files.
pub fn read_all(data: &GameData) -> Result<Vec<(u64, Vec<HeroSettingFloat>)>> {
    let mut out = Vec::new();
    for asset in data.guids_of_type(crate::guid::types::STU_GENERIC_SETTINGS) {
        let mut stu = match data.stu_of(asset) {
            Ok(stu) => stu,
            // Not every asset of this type is present in the loaded manifests.
            Err(_) => continue,
        };
        if stu.instance(HERO_SETTINGS).is_none() {
            continue;
        }
        let floats = read_from_stu(&mut stu, asset);
        if !floats.is_empty() {
            out.push((asset, floats));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    // The slices below are only needed to check the defaults line up.
    use crate::classes::hero_setting_float;

    /// The defaults are parallel to the slots read from them, so the order is
    /// load-bearing: slot 0 is the default, 2 the maximum, 3 the step. Swapping
    /// two entries would silently report a step of 1.0 and a maximum of 0.01.
    #[test]
    fn defaults_line_up_with_the_read_order() {
        assert_eq!(HERO_SETTING_FLOAT_DEFAULTS[0].0, hero_setting_float::DEFAULT);
        assert_eq!(HERO_SETTING_FLOAT_DEFAULTS[1].0, hero_setting_float::MIN);
        assert_eq!(HERO_SETTING_FLOAT_DEFAULTS[2].0, hero_setting_float::MAX);
        assert_eq!(HERO_SETTING_FLOAT_DEFAULTS[3].0, hero_setting_float::STEP);

        assert_eq!(HERO_SETTING_FLOAT_DEFAULTS[2].1, 1.0, "max defaults to 1");
        assert_eq!(HERO_SETTING_FLOAT_DEFAULTS[3].1, 0.01, "step defaults to 0.01");
        assert_eq!(HERO_SETTING_FLOAT_DEFAULTS[1].1, 0.0, "min has no authored default");
    }

    #[test]
    fn bounds_are_only_defaulted_when_none_of_them_are_authored() {
        let mut setting = HeroSettingFloat {
            asset: 0,
            instance: 0,
            default: 1.0,
            min: 0.0,
            max: 1.0,
            step: 0.01,
            // default present, min and max absent, step present
            explicit: [true, false, false, true],
            identifier: None,
            name: None,
            category: None,
        };
        assert!(setting.bounds_are_defaulted());

        // One authored bound is enough to stop calling them defaulted.
        setting.explicit = [true, true, false, true];
        assert!(!setting.bounds_are_defaulted());
    }
}

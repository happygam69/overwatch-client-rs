//! The rules the simulation applies, separated because they are **assumptions**.
//!
//! Everything else in this crate is arithmetic. These are game *rules*, and none
//! of them could be extracted from the installation — the install carries values,
//! not behaviours. They are written down here, in one place, with the reasoning
//! and the confidence, so that:
//!
//! * nothing is quietly wrong: a reader can see exactly what is assumed, and
//! * correcting one is a single edit rather than a hunt.
//!
//! Two rules are modelled, and both should be treated as provisional until
//! checked against the real game:
//!
//! **Damage order.** Which pool absorbs a hit first. Health, armour and shields
//! are three pools, and the game removes them in some order. This is set from
//! [`Rules::damage_order`].
//!
//! **Armour mitigation.** Armour is the pool that reduces incoming damage rather
//! than merely absorbing it. [`ArmourMitigation`] offers the two behaviours seen
//! in the game's history — a flat reduction per hit, and a percentage reduction
//! for small hits — and neither has been verified here.
//!
//! Shields are treated as a plain pool: they absorb damage and do not regenerate
//! during a fight, because regeneration timing has not been established either.

use crate::{PoolSet, Vec3};

/// Which pool absorbs damage first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
// FIT-COM-01: damage order: assumed
pub enum DamageOrder {
    /// Shields, then armour, then health.
    #[default]
    ShieldsArmourHealth,
    /// Health, then armour, then shields. Shields recover, so a game that wants
    /// them to matter holds them back; a game that wants them spent first does
    /// the opposite. Both have been observed in this genre.
    HealthArmourShields,
}

/// How armour reduces a hit.
#[derive(Debug, Clone, Copy, PartialEq)]
// FIT-COM-02: armour mitigation: assumed
pub enum ArmourMitigation {
    /// No reduction: armour only absorbs.
    None,
    /// Subtract a flat amount from each hit, never below one.
    Flat(f32),
    /// Reduce by a fraction, but only for hits below `threshold`; larger hits
    /// take the flat reduction instead.
    FractionBelow {
        threshold: f32,
        fraction: f32,
        flat: f32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
// FIT-COM-03: combat defaults: assumed (shield regen, spread, falloff)
pub struct Rules {
    pub damage_order: DamageOrder,
    pub armour_mitigation: ArmourMitigation,
    /// Whether a hit that has nothing left to absorb still reports as dealt.
    pub report_overkill: bool,
}

impl Default for Rules {
    fn default() -> Self {
        Rules {
            damage_order: DamageOrder::default(),
            // The commonly cited behaviour: a small hit is reduced by a
            // fraction, a large one by a flat amount. Provisional.
            armour_mitigation: ArmourMitigation::FractionBelow {
                threshold: 10.0,
                fraction: 0.3,
                flat: 5.0,
            },
            report_overkill: false,
        }
    }
}

/// Reduce an incoming hit by armour, and take it from the pools.
///
/// Returns `(dealt, mitigated)`. `mitigated` is only the part attributable to
/// armour's reduction, not the part armour absorbed.
pub fn resolve(raw: f32, pools: &mut PoolSet, rules: &Rules) -> (f32, f32) {
    if raw <= 0.0 {
        return (0.0, 0.0);
    }

    // The reduction applies to whatever portion lands on armour, so compute it
    // per pool rather than once for the whole hit.
    let order: [usize; 3] = match rules.damage_order {
        DamageOrder::ShieldsArmourHealth => [2, 1, 0],
        DamageOrder::HealthArmourShields => [0, 1, 2],
    };

    let mut left = raw;
    let mut dealt = 0.0;
    let mut mitigated = 0.0;

    for which in order {
        if left <= 0.0 {
            break;
        }
        let available = match which {
            0 => pools.health,
            1 => pools.armour,
            _ => pools.shields,
        };
        if available <= 0.0 {
            continue;
        }

        // Armour reduces before absorbing.
        let effective = if which == 1 {
            match rules.armour_mitigation {
                ArmourMitigation::None => left,
                ArmourMitigation::Flat(flat) => (left - flat).max(1.0).min(left),
                ArmourMitigation::FractionBelow {
                    threshold,
                    fraction,
                    flat,
                } => {
                    if left < threshold {
                        left * (1.0 - fraction)
                    } else {
                        (left - flat).max(1.0).min(left)
                    }
                }
            }
        } else {
            left
        };

        mitigated += left - effective;
        let absorbed = effective.min(available);
        dealt += absorbed;
        left -= absorbed;

        match which {
            0 => pools.health -= absorbed,
            1 => pools.armour -= absorbed,
            _ => pools.shields -= absorbed,
        }
    }

    if !rules.report_overkill {
        dealt = dealt.min(raw);
    }
    // Pools never go negative, so a caller can compare against a maximum.
    pools.health = pools.health.max(0.0);
    pools.armour = pools.armour.max(0.0);
    pools.shields = pools.shields.max(0.0);

    (dealt, mitigated)
}

/// Where a shooter is aiming from and at, used to build a shot.
#[derive(Debug, Clone, Copy)]
pub struct Aim {
    pub origin: Vec3,
    pub forward: Vec3,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pools(health: f32, armour: f32, shields: f32) -> PoolSet {
        PoolSet {
            health,
            armour,
            shields,
        }
    }

    #[test]
    fn damage_comes_off_in_the_configured_order() {
        let rules = Rules {
            damage_order: DamageOrder::ShieldsArmourHealth,
            armour_mitigation: ArmourMitigation::None,
            report_overkill: false,
        };
        let mut p = pools(100.0, 50.0, 30.0);
        resolve(40.0, &mut p, &rules);
        // Shields absorb first, and 30 of them are gone.
        assert_eq!(p.shields, 0.0);
        assert_eq!(p.armour, 40.0, "the remaining 10 comes off armour");
        assert_eq!(p.health, 100.0);
    }

    #[test]
    fn a_health_first_order_spends_health_before_shields() {
        let rules = Rules {
            damage_order: DamageOrder::HealthArmourShields,
            armour_mitigation: ArmourMitigation::None,
            report_overkill: false,
        };
        let mut p = pools(100.0, 50.0, 30.0);
        resolve(40.0, &mut p, &rules);
        assert_eq!(p.health, 60.0);
        assert_eq!(p.armour, 50.0);
        assert_eq!(p.shields, 30.0);
    }

    #[test]
    fn flat_armour_reduction_never_drops_a_hit_below_one() {
        let rules = Rules {
            damage_order: DamageOrder::HealthArmourShields,
            armour_mitigation: ArmourMitigation::Flat(5.0),
            report_overkill: false,
        };
        let mut p = pools(0.0, 100.0, 0.0);
        let (dealt, mitigated) = resolve(6.0, &mut p, &rules);
        assert_eq!(dealt, 1.0, "6 minus 5");
        assert_eq!(mitigated, 5.0);
        assert_eq!(p.armour, 99.0);
    }

    #[test]
    fn fraction_mitigation_applies_below_the_threshold_only() {
        let rules = Rules {
            damage_order: DamageOrder::HealthArmourShields,
            armour_mitigation: ArmourMitigation::FractionBelow {
                threshold: 10.0,
                fraction: 0.3,
                flat: 5.0,
            },
            report_overkill: false,
        };
        // A small hit loses 30 percent.
        let mut p = pools(0.0, 100.0, 0.0);
        let (dealt, _) = resolve(8.0, &mut p, &rules);
        assert!((dealt - 5.6).abs() < 1e-5);

        // A large one loses a flat 5 instead.
        let mut p = pools(0.0, 100.0, 0.0);
        let (dealt, _) = resolve(30.0, &mut p, &rules);
        assert_eq!(dealt, 25.0);
    }

    #[test]
    fn pools_never_go_negative_and_dealt_never_exceeds_the_hit() {
        let rules = Rules::default();
        let mut p = pools(10.0, 0.0, 0.0);
        let (dealt, _) = resolve(500.0, &mut p, &rules);
        assert_eq!(p.health, 0.0);
        assert!(p.armour >= 0.0 && p.shields >= 0.0);
        assert_eq!(dealt, 10.0, "only what was there is reported as dealt");
    }

    #[test]
    fn a_zero_hit_changes_nothing() {
        let rules = Rules::default();
        let mut p = pools(100.0, 20.0, 20.0);
        assert_eq!(resolve(0.0, &mut p, &rules), (0.0, 0.0));
        assert_eq!(p.total(), 140.0);
    }
}

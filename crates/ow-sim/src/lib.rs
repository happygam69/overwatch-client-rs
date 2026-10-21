//! A simulation of the client's movement and combat, driven by data read from
//! an install.
//!
//! This crate holds no IO and no Overwatch-specific knowledge: it takes hero
//! specs ([`movement::MoverSpec`] and [`PoolSet`] values) assembled elsewhere
//! and simulates them. `ow-view` is what supplies those values from the
//! install.
//!
//! # What is extracted and what is assumed
//!
//! Keeping these apart matters more than any single number here.
//!
//! **Extracted from the installation** (see `docs/formats/`): health pools,
//! armour and shields per hero; and per weapon, the fire interval, projectiles
//! per shot, projectile speed and lifetime.
//!
//! **Measured from the client's own traces**: the character mover's arithmetic
//! ([`movement`]) — per-frame integration with the trace's own frame dt, the
//! ramp, friction, gravity, jump impulse, airborne and slope rules — verified
//! by the gate differ in [`trace`] (`trace_diff keys --by-state`), which also
//! holds the recorded trace format. The mover control as *code* is closed as
//! tier 3 (obfuscated; `docs/formats/character-controller.md`).
//!
//! **Assumed, and marked as such in the code**: the order in which damage is
//! taken from the three pools, and how armour mitigates it. Both are game rules
//! rather than values, and neither has been verified from the install or from a
//! primary source. They live in [`rules`] as named constants with the reasoning,
//! so correcting them is a one-line change and no number is quietly wrong.
//!
//! **Supplied by the caller**: per-shot damage. It could not be extracted —
//! `docs/formats/weapons.md` records the seven places it was looked for — so it
//! is an explicit input rather than a guess dressed up as data.

pub mod body;
pub mod movement;
pub mod rules;
pub mod trace;

use std::f32::consts::PI;

/// A position and direction in metres.
pub type Vec3 = [f32; 3];

pub fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub fn scale(a: Vec3, s: f32) -> Vec3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

pub fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn length(a: Vec3) -> f32 {
    dot(a, a).sqrt()
}

pub fn normalize(a: Vec3) -> Vec3 {
    let len = length(a);
    if len <= f32::EPSILON {
        [0.0, 0.0, 1.0]
    } else {
        scale(a, 1.0 / len)
    }
}

/// The three pools a hero's health bar is made of.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PoolSet {
    pub health: f32,
    pub armour: f32,
    pub shields: f32,
}

impl PoolSet {
    pub fn total(&self) -> f32 {
        self.health + self.armour + self.shields
    }

    /// Whether any pool still holds something.
    pub fn is_alive(&self) -> bool {
        self.total() > 0.0
    }
}

/// Outcome of applying one damage instance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DamageOutcome {
    /// What was asked for.
    pub raw: f32,
    /// What actually came off the pools, after mitigation.
    pub dealt: f32,
    /// How much armour reduced it. Zero when no armour was in the way.
    pub mitigated: f32,
    /// Whether this instance was the one that emptied the pools.
    pub lethal: bool,
    pub remaining: PoolSet,
}

/// A combatant: fixed pools and a position, with no hero semantics attached.
#[derive(Debug, Clone)]
pub struct Combatant {
    pub pos: Vec3,
    /// Collision radius, in metres. Hit resolution treats a combatant as a
    /// vertical capsule; this is its radius.
    pub radius: f32,
    pub height: f32,
    pub pools: PoolSet,
    /// The most this combatant ever had, for reporting.
    pub max_pools: PoolSet,
    pub alive: bool,
}

impl Combatant {
    pub fn new(pos: Vec3, pools: PoolSet) -> Combatant {
        Combatant {
            pos,
            radius: 0.4,
            height: 1.8,
            pools,
            max_pools: pools,
            alive: pools.is_alive(),
        }
    }

    /// Apply one damage instance using the configured rules.
    pub fn take_damage(&mut self, raw: f32, rules: &rules::Rules) -> DamageOutcome {
        let lethal_before = !self.pools.is_alive();

        // Armour mitigates only the portion of the hit that lands on it, which
        // is what makes a small hit against a large armour pool behave
        // differently from a large one.
        let (dealt, mitigated) = rules::resolve(raw, &mut self.pools, rules);

        if !self.pools.is_alive() {
            self.alive = false;
        }
        DamageOutcome {
            raw,
            dealt,
            mitigated,
            lethal: !lethal_before && !self.alive,
            remaining: self.pools,
        }
    }
}

/// How a weapon delivers its damage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FireMode {
    /// Instant hit along the aim direction, up to `range` metres.
    Hitscan { range: f32 },
    /// A travelling projectile.
    Projectile { speed: f32, gravity: f32 },
}

/// Damage falloff over distance. Absent means none.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Falloff {
    /// Distance at which falloff begins, in metres.
    pub start: f32,
    /// Distance at which it stops decreasing.
    pub end: f32,
    /// Fraction of full damage at and beyond `end`.
    pub min_factor: f32,
}

impl Falloff {
    /// The multiplier at a distance, clamped to the range.
    pub fn factor(&self, distance: f32) -> f32 {
        if distance <= self.start {
            return 1.0;
        }
        if distance >= self.end {
            return self.min_factor;
        }
        let t = (distance - self.start) / (self.end - self.start).max(f32::EPSILON);
        1.0 + t * (self.min_factor - 1.0)
    }
}

/// A weapon as the simulation needs it.
#[derive(Debug, Clone)]
pub struct Weapon {
    pub name: String,
    /// Seconds between shots. The reciprocal of the extracted shots per second.
    pub interval: f32,
    /// Projectiles per shot, i.e. pellets for a shotgun.
    pub pellets: u32,
    /// Damage per projectile. Supplied, not extracted.
    pub damage_per_projectile: f32,
    pub mode: FireMode,
    /// Projectile lifetime in seconds, where the weapon declares one.
    pub lifetime: Option<f32>,
    /// Cone half-angle in degrees. Zero means perfectly accurate.
    pub spread_deg: f32,
    pub falloff: Option<Falloff>,
    /// Magazine size and reload time, where known.
    pub ammo: Option<u32>,
    pub reload: Option<f32>,
}

impl Default for Weapon {
    fn default() -> Self {
        Weapon {
            name: "unnamed".to_string(),
            interval: 0.5,
            pellets: 1,
            damage_per_projectile: 0.0,
            mode: FireMode::Hitscan { range: 40.0 },
            lifetime: None,
            spread_deg: 0.0,
            falloff: None,
            ammo: None,
            reload: None,
        }
    }
}

impl Weapon {
    /// Shots per second, as the game reports it.
    pub fn shots_per_second(&self) -> f32 {
        if self.interval <= 0.0 {
            0.0
        } else {
            1.0 / self.interval
        }
    }

    /// Damage of one full shot before falloff and mitigation.
    pub fn shot_damage(&self) -> f32 {
        self.damage_per_projectile * self.pellets as f32
    }

    /// Damage per second at a distance, ignoring reloads and mitigation.
    pub fn dps(&self, distance: f32) -> f32 {
        let factor = self.falloff.map(|f| f.factor(distance)).unwrap_or(1.0);
        self.shot_damage() * factor * self.shots_per_second()
    }
}

/// Where a shot went.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    pub target: usize,
    pub distance: f32,
    /// Falloff multiplier that applied.
    pub factor: f32,
}

/// Result of firing once.
#[derive(Debug, Clone, PartialEq)]
pub struct ShotResult {
    /// One entry per pellet; `None` for a miss.
    pub hits: Vec<Option<Hit>>,
    pub damage_total: f32,
    pub kills: Vec<usize>,
}

/// A deterministic spread generator.
///
/// Reproducibility is the point: a shot's pellets must land in the same places
/// every run, or nothing here can be tested.
#[derive(Debug, Clone)]
pub struct Spread {
    state: u64,
}

impl Spread {
    pub fn new(seed: u64) -> Spread {
        Spread { state: seed | 1 }
    }

    /// A uniform value in `[0, 1)`.
    pub fn next_unit(&mut self) -> f32 {
        // xorshift64*, chosen for being short and having no dependencies.
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        let v = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
        ((v >> 40) as f32) / ((1u32 << 24) as f32)
    }

    /// A direction within `half_angle_deg` of `forward`, biased toward the
    /// centre the way a cone of pellets is.
    pub fn perturb(&mut self, forward: Vec3, half_angle_deg: f32) -> Vec3 {
        if half_angle_deg <= 0.0 {
            return forward;
        }
        let angle = half_angle_deg.to_radians();
        // sqrt on the radius gives uniform area coverage in a disc; using it
        // without the sqrt would clump pellets at the centre.
        let r = self.next_unit().sqrt() * angle.tan();
        let theta = self.next_unit() * 2.0 * PI;

        // An orthonormal basis around forward.
        let up = if forward[1].abs() > 0.99 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
        let right = normalize(cross(forward, up));
        let real_up = cross(right, forward);

        normalize(add(
            forward,
            add(
                scale(right, r * theta.cos()),
                scale(real_up, r * theta.sin()),
            ),
        ))
    }
}

pub fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Distance along `dir` at which a ray first enters a target's capsule, if it
/// does within `max_range`.
///
/// The capsule is treated as a vertical cylinder with hemispherical caps ignored,
/// which is accurate enough for a body and much simpler to reason about.
pub fn ray_capsule(origin: Vec3, dir: Vec3, target: &Combatant, max_range: f32) -> Option<f32> {
    let rel = sub(target.pos, origin);
    // Horizontal projection.
    let rel_xz = [rel[0], 0.0, rel[2]];
    let dir_xz = normalize([dir[0], 0.0, dir[2]]);
    let along = dot(rel_xz, dir_xz);
    let closest = sub(rel_xz, scale(dir_xz, along));
    let lateral = length(closest);
    if lateral > target.radius {
        return None;
    }
    // Distance to the entry point along the ray.
    let back = (target.radius * target.radius - lateral * lateral).sqrt();
    let enter_xz = along - back;
    if enter_xz < 0.0 || enter_xz > max_range {
        return None;
    }
    // Vertical extent: the target spans [pos.y, pos.y + height].
    if dir[1].abs() > 1e-6 {
        let t_low = (target.pos[1] - origin[1]) / dir[1];
        let t_high = (target.pos[1] + target.height - origin[1]) / dir[1];
        let (lo, hi) = if t_low <= t_high {
            (t_low, t_high)
        } else {
            (t_high, t_low)
        };
        if enter_xz < lo || enter_xz > hi {
            return None;
        }
    } else if origin[1] < target.pos[1] || origin[1] >= target.pos[1] + target.height {
        return None;
    }
    Some(enter_xz)
}

/// A projectile in flight.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projectile {
    pub pos: Vec3,
    pub vel: Vec3,
    /// Seconds of flight left.
    pub life: f32,
    pub damage: f32,
    pub falloff: Option<Falloff>,
    /// Downward acceleration in m/s², from the volley's projectile-motion
    /// gravity term where the files declare one (`m_599AB8D7 = gravity?`,
    /// measured `9.8` on heroes 5/6/810 — `docs/formats/weapons.md`).
    /// Zero means straight flight.
    pub gravity: f32,
    /// The combatant that fired it, which it cannot hit.
    pub owner: Option<usize>,
}

/// Something that happened as a consequence of damage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DamageEvent {
    pub target: usize,
    pub dealt: f32,
    pub lethal: bool,
    pub remaining: PoolSet,
}

/// The world: combatants, projectiles in flight, and the tick counter.
#[derive(Debug, Default)]
pub struct World {
    pub combatants: Vec<Combatant>,
    pub projectiles: Vec<Projectile>,
    /// Seconds simulated so far.
    pub time: f32,
    pub tick: u64,
    /// Seconds a projectile takes to reach a target. Reported rather than
    /// simulated when a caller asks for an instant answer.
    pub last_flight_time: f32,
}

impl World {
    pub fn new() -> World {
        World::default()
    }

    pub fn spawn(&mut self, combatant: Combatant) -> usize {
        self.combatants.push(combatant);
        self.combatants.len() - 1
    }

    /// The nearest live combatant along a ray.
    pub fn raycast(&self, origin: Vec3, dir: Vec3, max_range: f32) -> Option<Hit> {
        let mut best: Option<Hit> = None;
        for (index, target) in self.combatants.iter().enumerate() {
            if !target.alive {
                continue;
            }
            if let Some(distance) = ray_capsule(origin, dir, target, max_range) {
                if best.map(|b| distance < b.distance).unwrap_or(true) {
                    best = Some(Hit {
                        target: index,
                        distance,
                        factor: 1.0,
                    });
                }
            }
        }
        best
    }

    /// Advance the world. Projectiles move, and land their damage on arrival.
    ///
    /// A step is resolved as a segment test from the projectile's old position
    /// to its new one, so a fast projectile cannot tunnel through a target
    /// between ticks.
    pub fn step(&mut self, dt: f32, rules: &rules::Rules) -> Vec<DamageEvent> {
        self.time += dt;
        self.tick += 1;
        let mut events = Vec::new();

        let mut resolved: Vec<usize> = Vec::new();
        for (index, projectile) in self.projectiles.iter_mut().enumerate() {
            if projectile.life <= 0.0 {
                resolved.push(index);
                continue;
            }
            let mut remaining = dt;
            let mut landed = false;

            // Sub-step so a single tick cannot skip past a target, even at high
            // speed: each sub-step covers at most one body radius.
            while remaining > 1e-6 && !landed {
                let speed = length(projectile.vel);
                let max_step = if speed > 1e-6 {
                    (0.4 / speed).clamp(1e-4, 0.05)
                } else {
                    remaining
                };
                let sub = remaining.min(max_step);
                remaining -= sub;

                let delta = scale(projectile.vel, sub);
                let distance = length(delta);
                if distance > 1e-6 {
                    let dir = normalize(delta);
                    let mut best: Option<(usize, f32)> = None;
                    for (other, target) in self.combatants.iter().enumerate() {
                        if !target.alive || Some(other) == projectile.owner {
                            continue;
                        }
                        if let Some(d) = ray_capsule(projectile.pos, dir, target, distance) {
                            if best.map(|b| d < b.1).unwrap_or(true) {
                                best = Some((other, d));
                            }
                        }
                    }
                    if let Some((hit_index, at)) = best {
                        let travelled = at;
                        let factor = projectile
                            .falloff
                            .map(|f| f.factor(travelled))
                            .unwrap_or(1.0);
                        let outcome = self.combatants[hit_index]
                            .take_damage(projectile.damage * factor, rules);
                        events.push(DamageEvent {
                            target: hit_index,
                            dealt: outcome.dealt,
                            lethal: outcome.lethal,
                            remaining: outcome.remaining,
                        });
                        landed = true;
                        break;
                    }
                    projectile.pos = add(projectile.pos, delta);
                }
                // Gravity, when the volley declares it, is applied per
                // sub-step so a fast arc cannot tunnel past its apex.
                projectile.vel[1] -= projectile.gravity * sub;
            }

            projectile.life -= dt;
            if landed || projectile.life <= 0.0 {
                resolved.push(index);
            }
        }

        // Remove in reverse so indices stay valid.
        for index in resolved.into_iter().rev() {
            self.projectiles.remove(index);
        }
        events
    }

    /// Resolve one shot from a weapon.
    pub fn fire(
        &mut self,
        weapon: &Weapon,
        origin: Vec3,
        forward: Vec3,
        spread: &mut Spread,
        rules: &rules::Rules,
    ) -> ShotResult {
        let forward = normalize(forward);
        let mut hits = Vec::with_capacity(weapon.pellets as usize);
        let mut damage_total = 0.0;
        let mut kills = Vec::new();

        for _ in 0..weapon.pellets.max(1) {
            let dir = spread.perturb(forward, weapon.spread_deg * 0.5);
            let range = match weapon.mode {
                FireMode::Hitscan { range } => range,
                // A travelling projectile is resolved as hitting the same way,
                // at the same instant, until projectile flight is simulated.
                FireMode::Projectile { .. } => weapon
                    .lifetime
                    .map(|t| {
                        t * match weapon.mode {
                            FireMode::Projectile { speed, .. } => speed,
                            _ => 0.0,
                        }
                    })
                    .unwrap_or(100.0),
            };

            match weapon.mode {
                FireMode::Projectile { speed, gravity } => {
                    // The shot leaves now and lands later, which is the whole
                    // difference from hitscan. Damage is applied by `step`.
                    let life = weapon.lifetime.unwrap_or(3.0);
                    self.projectiles.push(Projectile {
                        pos: origin,
                        vel: scale(dir, speed),
                        life,
                        damage: weapon.damage_per_projectile,
                        falloff: weapon.falloff,
                        gravity,
                        owner: None,
                    });
                    hits.push(None);
                }
                FireMode::Hitscan { .. } => {
                    let hit = self.raycast(origin, dir, range).map(|mut h| {
                        h.factor = weapon.falloff.map(|f| f.factor(h.distance)).unwrap_or(1.0);
                        h
                    });
                    if let Some(h) = hit {
                        let raw = weapon.damage_per_projectile * h.factor;
                        let outcome = self.combatants[h.target].take_damage(raw, rules);
                        damage_total += outcome.dealt;
                        if outcome.lethal {
                            kills.push(h.target);
                        }
                    }
                    hits.push(hit);
                }
            }
        }

        ShotResult {
            hits,
            damage_total,
            kills,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::Rules;

    fn pools(health: f32, armour: f32, shields: f32) -> PoolSet {
        PoolSet {
            health,
            armour,
            shields,
        }
    }

    #[test]
    fn falloff_is_flat_inside_the_range_and_clamped_beyond() {
        let f = Falloff {
            start: 10.0,
            end: 20.0,
            min_factor: 0.5,
        };
        assert_eq!(f.factor(0.0), 1.0);
        assert_eq!(f.factor(10.0), 1.0);
        assert!((f.factor(15.0) - 0.75).abs() < 1e-6);
        assert_eq!(f.factor(20.0), 0.5);
        assert_eq!(f.factor(100.0), 0.5);
    }

    #[test]
    fn spread_is_reproducible_for_a_seed() {
        let mut a = Spread::new(7);
        let mut b = Spread::new(7);
        for _ in 0..10 {
            assert_eq!(a.next_unit(), b.next_unit());
        }
        let mut c = Spread::new(7);
        let mut d = Spread::new(8);
        assert_ne!(c.next_unit(), d.next_unit());
    }

    #[test]
    fn a_centred_shot_hits_a_target_in_front() {
        let mut world = World::new();
        let target = world.spawn(Combatant::new([0.0, 0.0, 10.0], pools(200.0, 0.0, 0.0)));
        let hit = world
            .raycast([0.0, 0.9, 0.0], [0.0, 0.0, 1.0], 40.0)
            .expect("should hit");
        assert_eq!(hit.target, target);
        assert!((hit.distance - 9.6).abs() < 0.1, "got {}", hit.distance);
    }

    #[test]
    fn a_shot_beside_the_target_misses() {
        let mut world = World::new();
        world.spawn(Combatant::new([0.0, 0.0, 10.0], pools(200.0, 0.0, 0.0)));
        // 2 m to the side, well outside the 0.4 m radius.
        assert!(world
            .raycast([2.0, 0.9, 0.0], [0.0, 0.0, 1.0], 40.0)
            .is_none());
    }

    #[test]
    fn the_nearest_target_is_the_one_hit() {
        let mut world = World::new();
        let far = world.spawn(Combatant::new([0.0, 0.0, 20.0], pools(200.0, 0.0, 0.0)));
        let near = world.spawn(Combatant::new([0.0, 0.0, 8.0], pools(200.0, 0.0, 0.0)));
        let hit = world
            .raycast([0.0, 0.9, 0.0], [0.0, 0.0, 1.0], 40.0)
            .unwrap();
        assert_eq!(hit.target, near);
        assert_ne!(hit.target, far);
    }

    #[test]
    fn a_dead_target_is_not_hit_again() {
        let mut world = World::new();
        let index = world.spawn(Combatant::new([0.0, 0.0, 10.0], pools(10.0, 0.0, 0.0)));
        let rules = Rules::default();
        world.combatants[index].take_damage(50.0, &rules);
        assert!(!world.combatants[index].alive);
        assert!(world
            .raycast([0.0, 0.9, 0.0], [0.0, 0.0, 1.0], 40.0)
            .is_none());
    }

    #[test]
    fn firing_a_shotgun_applies_every_pellet_and_can_kill() {
        let mut world = World::new();
        let index = world.spawn(Combatant::new([0.0, 0.0, 5.0], pools(60.0, 0.0, 0.0)));
        let weapon = Weapon {
            pellets: 8,
            damage_per_projectile: 10.0,
            ..Weapon::default()
        };
        let mut spread = Spread::new(1);
        let result = world.fire(
            &weapon,
            [0.0, 0.9, 0.0],
            [0.0, 0.0, 1.0],
            &mut spread,
            &Rules::default(),
        );
        assert_eq!(result.hits.len(), 8, "one entry per pellet");
        // 8 pellets of 10 against 60 health: lethal, and it stops at zero rather
        // than going negative.
        assert!(result.damage_total >= 60.0);
        assert_eq!(result.kills, vec![index]);
        assert_eq!(world.combatants[index].pools.total(), 0.0);
    }

    #[test]
    fn shots_per_second_and_dps_follow_the_interval() {
        let weapon = Weapon {
            interval: 0.05,
            pellets: 1,
            damage_per_projectile: 6.0,
            ..Weapon::default()
        };
        assert!((weapon.shots_per_second() - 20.0).abs() < 1e-4);
        assert!((weapon.dps(0.0) - 120.0).abs() < 1e-3);
    }
}

#[cfg(test)]
mod projectile_tests {
    use super::*;
    use crate::rules::Rules;

    fn target_world() -> (World, usize) {
        let mut world = World::new();
        let index = world.spawn(Combatant::new(
            [0.0, 0.0, 20.0],
            PoolSet {
                health: 200.0,
                armour: 0.0,
                shields: 0.0,
            },
        ));
        (world, index)
    }

    #[test]
    fn a_projectile_takes_time_to_arrive() {
        let (mut world, target) = target_world();
        let weapon = Weapon {
            interval: 1.0,
            pellets: 1,
            damage_per_projectile: 20.0,
            mode: FireMode::Projectile {
                speed: 50.0,
                gravity: 0.0,
            },
            lifetime: Some(5.0),
            ..Weapon::default()
        };
        let mut spread = Spread::new(1);
        let rules = Rules::default();

        let shot = world.fire(
            &weapon,
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            &mut spread,
            &rules,
        );
        assert_eq!(shot.damage_total, 0.0, "nothing lands on the shot itself");
        assert_eq!(world.projectiles.len(), 1);

        // 20 m at 50 m/s takes 0.4 s, so 0.2 s of flight is not enough.
        let early = world.step(0.2, &rules);
        assert!(early.is_empty());
        assert_eq!(world.combatants[target].pools.health, 200.0);

        // The rest of the flight lands it.
        let mut events = Vec::new();
        for _ in 0..8 {
            events.extend(world.step(0.05, &rules));
        }
        assert_eq!(events.len(), 1, "exactly one impact");
        assert_eq!(events[0].target, target);
        assert!((events[0].dealt - 20.0).abs() < 1e-3);
        assert!(world.projectiles.is_empty(), "the projectile is consumed");
    }

    #[test]
    fn a_fast_projectile_does_not_tunnel_through_a_target() {
        let (mut world, target) = target_world();
        // 400 m/s over a 0.1 s tick covers 40 m in one step, passing the target.
        let weapon = Weapon {
            pellets: 1,
            damage_per_projectile: 10.0,
            mode: FireMode::Projectile {
                speed: 400.0,
                gravity: 0.0,
            },
            lifetime: Some(1.0),
            ..Weapon::default()
        };
        let mut spread = Spread::new(1);
        let rules = Rules::default();
        world.fire(
            &weapon,
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            &mut spread,
            &rules,
        );

        let events = world.step(0.1, &rules);
        assert_eq!(events.len(), 1, "the target is in the way and must be hit");
        assert_eq!(events[0].target, target);
    }

    #[test]
    fn gravity_pulls_a_projectile_down_and_it_expires() {
        let (mut world, _) = target_world();
        let projectile = Projectile {
            pos: [0.0, 10.0, 0.0],
            vel: [0.0, 0.0, 10.0],
            life: 0.05,
            damage: 1.0,
            falloff: None,
            gravity: 9.8,
            owner: None,
        };
        world.projectiles.push(projectile);
        let rules = Rules::default();
        world.step(0.02, &rules);
        let vy = world.projectiles.first().map(|p| p.vel[1]).unwrap_or(0.0);
        assert!(
            (vy + 9.8 * 0.02).abs() < 1e-4,
            "one step sheds g*dt of vertical velocity, got {vy}"
        );
        for _ in 0..3 {
            world.step(0.02, &rules);
        }
        assert!(world.projectiles.is_empty(), "lifetime expired");
    }
}

//! The sandbox simulation: `ow-sim` plus the bookkeeping a shell needs.
//!
//! The sim owns the clock. `advance` accumulates frame seconds and runs whole
//! fixed ticks; the input the caller passes is the snapshot for every tick in
//! that frame, which is what makes a replay ("same inputs, same bits")
//! well-defined. No Bevy type crosses into this module — every number here is
//! the same f32 the core computes.

use ow_sim::movement::{Controller, Input, MoverSpec, TICK_RATE_HZ};
use ow_sim::rules::Rules;
use ow_sim::{Combatant, Spread, Vec3, Weapon, World};

/// One fixed tick, from the core's stride.
pub const DT: f32 = 1.0 / TICK_RATE_HZ;
/// Eye height above the standing surface, metres.
pub const EYE_HEIGHT: f32 = 1.75;

/// The sandbox state.
pub struct Sim {
    pub controller: Controller,
    pub world: World,
    pub weapon: Weapon,
    pub spread: Spread,
    pub rules: Rules,
    /// Look pitch, radians, +up.
    pub pitch: f32,
    /// Whether the trigger is held.
    pub firing: bool,
    fire_timer: f32,
    acc: f32,
    pub ticks: u64,
    pub shots: u64,
    pub pellets_landed: u64,
    pub kills: u64,
    pub last_damage: f32,
}

impl Sim {
    pub fn new(
        spec: MoverSpec,
        start: Vec3,
        yaw: f32,
        weapon: Weapon,
        bots: Vec<Combatant>,
    ) -> Sim {
        let mut world = World::new();
        for bot in bots {
            world.spawn(bot);
        }
        Sim {
            controller: Controller::new(spec, start, yaw),
            world,
            weapon,
            spread: Spread::new(0x5EED),
            rules: Rules::default(),
            pitch: 0.0,
            firing: false,
            fire_timer: 0.0,
            acc: 0.0,
            ticks: 0,
            shots: 0,
            pellets_landed: 0,
            kills: 0,
            last_damage: 0.0,
        }
    }

    /// Look direction from yaw/pitch, in the mover's own axis convention
    /// (`fwd = (cos yaw, 0, -sin yaw)`, `up = +y`).
    pub fn eye_dir(&self) -> Vec3 {
        let (y, p) = (self.controller.yaw, self.pitch);
        [y.cos() * p.cos(), p.sin(), -y.sin() * p.cos()]
    }

    pub fn eye_pos(&self) -> Vec3 {
        self.controller.pos
    }

    /// Snapshot `input`/look and run whole ticks until `dt` seconds are spent.
    /// Returns the number of ticks run.
    pub fn advance<F: Fn(f32, f32, f32) -> f32>(
        &mut self,
        dt: f32,
        input: Input,
        ground: F,
    ) -> u32 {
        self.acc += dt;
        let mut ran = 0;
        while self.acc >= DT {
            self.acc -= DT;
            self.tick(input, &ground);
            ran += 1;
        }
        ran
    }

    /// One tick, in the order the player perceives it: move, settle on the
    /// surface, fire, then advance projectiles.
    fn tick<F: Fn(f32, f32, f32) -> f32>(&mut self, input: Input, ground: &F) {
        self.controller.advance(DT, input);
        let eye = self.controller.pos;
        let floor = ground(eye[0], eye[2], eye[1] - 0.5);
        self.controller.settle_ground(floor + EYE_HEIGHT);
        self.ticks += 1;

        self.fire_timer -= DT;
        if self.firing && self.fire_timer <= 0.0 {
            self.fire_timer = self.weapon.interval.max(DT);
            self.fire();
        }

        let events = self.world.step(DT, &self.rules);
        for event in &events {
            self.last_damage = event.dealt;
            if event.lethal {
                self.kills += 1;
            }
        }
    }

    /// One trigger pull: the weapon as the install describes it, aimed along
    /// the view ray.
    fn fire(&mut self) {
        let origin = self.eye_pos();
        let dir = self.eye_dir();
        let result = self
            .world
            .fire(&self.weapon, origin, dir, &mut self.spread, &self.rules);
        self.shots += 1;
        self.pellets_landed += result.hits.iter().filter(|h| h.is_some()).count() as u64;
        self.kills += result.kills.len() as u64;
        if result.damage_total > 0.0 {
            self.last_damage = result.damage_total;
        }
    }

    pub fn alive_bots(&self) -> usize {
        self.world.combatants.iter().filter(|c| c.alive).count()
    }

    /// The HUD text, one line per fact, with no claim it cannot back.
    pub fn hud(&self) -> String {
        let p = self.eye_pos();
        let weapon = &self.weapon;
        let mode = match weapon.mode {
            ow_sim::FireMode::Hitscan { range } => format!("hitscan r{range:.0}"),
            ow_sim::FireMode::Projectile { speed, gravity } => {
                format!("proj s{speed:.0} g{gravity:.1}")
            }
        };
        format!(
            "tick {ticks}   t {t:.2}s\n\
             pos ({x:8.3}, {y:8.3}, {z:8.3})   pitch {pitch:+.2}\n\
             vertical {v:8.3} m/s   grounded {g}\n\
             hero speed {speed:.1} m/s   back {back:.1}   crouch {crouch:.1}   fall max {fall:.0}\n\
             weapon {mode} interval {interval:.3}s ({rate:.1}/s)  pellets {pellets}  damage/shot {damage}\n\
             shots {shots}   pellets landed {landed}   bots alive {alive}/{total}   kills {kills}   last hit {last:.1}",
            ticks = self.ticks,
            t = self.ticks as f32 * DT,
            x = p[0],
            y = p[1],
            z = p[2],
            pitch = self.pitch,
            v = self.controller.vertical,
            g = self.controller.grounded,
            speed = self.controller.spec.speed,
            back = self.controller.spec.back_speed(),
            crouch = self.controller.spec.crouch_speed(),
            fall = self.controller.spec.fall_terminal(),
            interval = weapon.interval,
            rate = weapon.shots_per_second(),
            pellets = weapon.pellets,
            damage = weapon.damage_per_projectile,
            shots = self.shots,
            landed = self.pellets_landed,
            alive = self.alive_bots(),
            total = self.world.combatants.len(),
            kills = self.kills,
            last = self.last_damage,
        )
    }
}

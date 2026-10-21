//! The playable harness: first-person movement and combat on real geometry.
//!
//! Movement runs on [`ow_sim::movement::Controller`] with the per-hero figures
//! the install carries (the mover dump, mapped by the field hashes in
//! `ow-sim::movement`). Combat is hitscan fire against the bots the harness
//! stands on the map; their pools come from their own entity schema
//! (`ow_tank::entity::health_of`), unmodified.
//!
//! **Per-shot damage is an explicit input.** `docs/formats/weapons.md` records
//! the search that failed to find it in the files, so `--damage` supplies it
//! and nothing here invents it.
//!
//! Modes:
//! * `--play` opens the window: WASD + Space to move, mouse-locked look,
//!   left click fires one volley per weapon interval, Esc exits. A headless
//!   variant `--play-th <seconds>` advances the state and prints the HUD after
//!   each tick batch, so a scripted run (`--shot`) can validate it without a
//!   human.

use glam::Vec2;
use ow_sim::movement::{Controller, Input, MoverSpec};
use ow_sim::{Combatant, FireMode, PoolSet, Projectile, Vec3, Weapon};
use ow_tank::guid;
use ow_tank::GameData;

use ow_scene as scene;

/// Parameters the caller supplies because the install does not hold them.
// FIT-COM-04: per-shot damage: supplied by the caller, not a fit
pub struct Supplied {
    /// Per-shot damage.
    pub damage: f32,
    /// Which hero the player is, by install index.
    pub hero: u32,
    /// Bots in the ring.
    pub bots: usize,
}

/// Stand-in collision for movement testing: a flat ground plane plus a few
/// axis-aligned boxes around the spawn.
///
/// The map mesh is **not** a collision mesh — sampling it with a ray is not
/// what the client collides against, and the client's real character step
/// (ledge and slope handling) is not read yet. So the harness stops pretending
/// the geometry is solid and uses this instead: one plane at the height the
/// spawn floor reports, four boxes to walk into, step onto and jump onto.
// FIT-MOV-04: player collision: stand-in (ground plane plus boxes)
pub struct Sandbox {
    /// Ground plane height.
    pub ground: f32,
    /// Axis-aligned boxes as `(min, max)`.
    pub boxes: Vec<([f32; 3], [f32; 3])>,
    /// Player capsule radius — the assumed 0.4 m hero capsule.
    pub radius: f32,
    /// How high a lip the feet still climb while walking.
    pub step: f32,
}

impl Sandbox {
    /// An empty plane, for tests.
    pub fn flat(ground: f32) -> Sandbox {
        Sandbox {
            ground,
            boxes: Vec::new(),
            radius: 0.4,
            step: 0.55,
        }
    }

    /// The default test course around `spawn`.
    ///
    /// The layout lives in `ow-scene` (`sandbox_boxes`) because `--no-map`
    /// *draws* it: one definition, so the player cannot walk through a wall that
    /// is not on screen.
    pub fn around(spawn: [f32; 3], ground: f32) -> Sandbox {
        Sandbox {
            ground,
            radius: 0.4,
            step: 0.55,
            boxes: scene::sandbox_boxes(spawn, ground),
        }
    }

    /// Floor height under `(x, z)` for feet at `feet_y`. Box tops count when
    /// the feet are above them, and when the lip is within [`Sandbox::step`]
    /// **and the body is grounded** — so walking climbs a low lip but a jump
    /// only lands once the feet reach the top.
    pub fn ground_at(&self, x: f32, z: f32, feet_y: f32, grounded: bool) -> f32 {
        let mut g = self.ground;
        for (mn, mx) in &self.boxes {
            let over = x >= mn[0] - self.radius
                && x <= mx[0] + self.radius
                && z >= mn[2] - self.radius
                && z <= mx[2] + self.radius;
            if !over {
                continue;
            }
            if feet_y >= mx[1] || (grounded && mx[1] - feet_y <= self.step) {
                g = g.max(mx[1]);
            }
        }
        g
    }

    /// Push a body out of any box it overlaps horizontally, along the axis of
    /// least penetration. `pos` is the eye; the body spans
    /// `[eye − 1.75, eye + 0.05]`. A grounded body walking into a lip no
    /// higher than [`Sandbox::step`] is not pushed out — that lip is climbed
    /// by [`Sandbox::ground_at`] instead.
    pub fn resolve(&self, pos: &mut [f32; 3], grounded: bool) {
        let (feet, head) = (pos[1] - 1.75, pos[1] + 0.05);
        for (mn, mx) in &self.boxes {
            if head <= mn[1] || feet >= mx[1] {
                continue; // above or below it
            }
            if grounded && mx[1] - feet <= self.step {
                continue; // a lip the feet climb
            }
            let (x0, x1) = (mn[0] - self.radius, mx[0] + self.radius);
            let (z0, z1) = (mn[2] - self.radius, mx[2] + self.radius);
            if pos[0] <= x0 || pos[0] >= x1 || pos[2] <= z0 || pos[2] >= z1 {
                continue;
            }
            let dx = (pos[0] - x0).min(x1 - pos[0]);
            let dz = (pos[2] - z0).min(z1 - pos[2]);
            if dx < dz {
                pos[0] = if pos[0] - x0 < x1 - pos[0] { x0 } else { x1 };
            } else {
                pos[2] = if pos[2] - z0 < z1 - pos[2] { z0 } else { z1 };
            }
        }
    }
}

/// The whole play state: mover, look, bots, fire bookkeeping.
pub struct PlayState {
    pub controller: Controller,
    pub pitch: f32,
    /// The stand-in collision (plane + boxes), not the map mesh.
    pub sandbox: Sandbox,
    // FIT-PRE-03: bots and projectiles: drawn as boxes and markers
    pub bots: Vec<Combatant>,
    pub labels: Vec<String>,
    pub log: Vec<String>,
    pub damage: f32,
    /// Seconds between trigger pulls; from the hero's volley literals.
    pub weapon_interval: f32,
    /// The hero's sandbox weapon: pellets, range/mode, lifetime and motion
    /// gravity from the install's volley figures (`hero_weapon`).
    pub weapon: Weapon,
    /// Projectiles in flight, stepped in `frame` with the volley's gravity.
    pub projectiles: Vec<Projectile>,
    /// Modify-health literals across the hero's weapon graphs: damage
    /// *candidates*, reported never labelled.
    pub damage_candidates: Vec<(u64, usize, u32, f32)>,
    /// One line on the weapon pick + ability join state, for the HUD/title.
    pub weapon_note: String,
    fire_timer: f32,
    /// Whether fire is held (single pull per interval).
    pub firing: bool,
}

impl PlayState {
    /// Build the state: hero movers and pools from the entities, bots in a
    /// ring around the scene centroid, standing on the collider's ground.
    pub fn build(
        data: &GameData,
        scene: &scene::Scene,
        collider: &scene::Collider,
        supplied: &Supplied,
    ) -> Result<PlayState, String> {
        let (spec, _pools, _interval) = hero_spec_and_pools(data, supplied.hero)?;
        // A sandbox scene states its spawn; a map scene's is derived from where
        // its geometry is, since a placement centroid carries tall pieces.
        let (centre, start, base) = match scene.sandbox_spawn {
            Some([x, ground, z]) => ([x, ground, z], [x, ground + 1.75, z], ground),
            None => {
                let centre = centroid(scene);
                let base = collider.ground_at(centre[0], centre[2], centre[1] + 4.0);
                (centre, [centre[0], base + 1.75, centre[2]], base)
            }
        };
        let sandbox = Sandbox::around(start, base);
        let controller = Controller::new(spec, start, 0.0);
        let radius = 9.0;
        let bots_stock = stock_bots(data, supplied.bots)?;
        let mut bots = Vec::new();
        let mut labels = Vec::new();
        for (i, (pools, label)) in bots_stock.iter().enumerate() {
            let angle = i as f32 / supplied.bots.max(1) as f32 * std::f32::consts::TAU;
            let bx = centre[0] + radius * angle.cos();
            let bz = centre[2] + radius * angle.sin();
            let y = collider.ground_at(bx, bz, base + 1.2);
            let bot = Combatant::new([bx, y, bz], *pools);
            bots.push(bot);
            labels.push(label.clone());
        }
        let (weapon, damage_candidates, weapon_note) =
            hero_weapon(supplied.damage, supplied.hero, data);
        let abilities_note = hero_gameplay_note(data, supplied.hero);
        let weapon_note = format!("{weapon_note}; {abilities_note}");
        Ok(PlayState {
            controller,
            pitch: -0.05,
            sandbox,
            bots,
            labels,
            log: Vec::new(),
            damage: supplied.damage,
            // The weapon's own interval is the fire cadence; the field is a
            // mirror so the HUD timer and the weapon can never disagree.
            weapon_interval: weapon.interval,
            weapon,
            projectiles: Vec::new(),
            damage_candidates,
            weapon_note,
            fire_timer: 0.0,
            firing: false,
        })
    }

    /// Advance one frame: look, movement ticks, ground settle, projectiles,
    /// and fire.
    pub fn frame(&mut self, dt: f32, input: Input, look: Vec2) {
        self.controller.yaw -= look.x;
        self.pitch = (self.pitch - look.y).clamp(-1.45, 1.45);
        self.controller.advance(dt, input);
        let eye = self.controller.pos;
        let feet = eye[1] - 1.75;
        let ground = self
            .sandbox
            .ground_at(eye[0], eye[2], feet, self.controller.grounded);
        self.controller.settle_ground(ground + 1.75);
        // Horizontal push-out after the floor snap, so standing on a box top
        // is not treated as being inside it.
        self.sandbox
            .resolve(&mut self.controller.pos, self.controller.grounded);
        self.step_projectiles(dt);
        self.fire_timer -= dt;
        if self.firing && self.fire_timer <= 0.0 {
            self.fire_timer = self.weapon_interval;
            self.fire();
        }
    }

    /// One trigger pull: pellets along the view ray (hitscan), or pellets
    /// away as projectiles when the volley declares a projectile speed.
    fn fire(&mut self) {
        let eye = self.controller.pos;
        let (yaw, pitch) = (self.controller.yaw, self.pitch);
        // The mover's yaw convention (see `crate::Camera::forward`): the view,
        // the movement axes and this ray all have to agree, or shots land 90°
        // off what the crosshair covers.
        let dir: Vec3 = [
            yaw.sin() * pitch.cos(),
            pitch.sin(),
            yaw.cos() * pitch.cos(),
        ];
        let pellets = self.weapon.pellets.max(1);
        match self.weapon.mode {
            FireMode::Projectile { speed, gravity } => {
                let life = self.weapon.lifetime.unwrap_or(3.0);
                for _ in 0..pellets {
                    self.projectiles.push(Projectile {
                        pos: eye,
                        vel: ow_sim::scale(dir, speed),
                        life,
                        damage: self.damage,
                        falloff: None,
                        gravity,
                        owner: None,
                    });
                }
            }
            FireMode::Hitscan { range } => {
                let rules = ow_sim::rules::Rules::default();
                let mut landed = 0u32;
                let mut dealt = 0.0f32;
                let mut label = String::new();
                let mut pool = 0.0f32;
                for _ in 0..pellets {
                    let mut nearest: Option<(usize, f32)> = None;
                    for (i, bot) in self.bots.iter().enumerate() {
                        if !bot.alive {
                            continue;
                        }
                        if let Some(t) = ow_sim::ray_capsule(eye, dir, bot, range) {
                            if nearest.is_none() || t < nearest.unwrap().1 {
                                nearest = Some((i, t));
                            }
                        }
                    }
                    if let Some((i, _t)) = nearest {
                        let outcome = self.bots[i].take_damage(self.damage, &rules);
                        landed += 1;
                        dealt += outcome.dealt;
                        label = self.labels[i].clone();
                        pool = outcome.remaining.total();
                    }
                }
                if landed > 0 {
                    self.log.push(format!(
                        "{label}: dealt {:.0} (pool {:.0}, {landed}/{pellets} pellets)",
                        dealt, pool,
                    ));
                } else {
                    self.log.push("miss".into());
                }
            }
        }
    }

    /// Step projectiles: segment-tested flight with the volley's gravity,
    /// landing damage on bots. Mirrors `ow_sim::World::step` over this
    /// harness's bot list (which is not a `World`).
    fn step_projectiles(&mut self, dt: f32) {
        let rules = ow_sim::rules::Rules::default();
        let mut resolved: Vec<usize> = Vec::new();
        for (index, projectile) in self.projectiles.iter_mut().enumerate() {
            if projectile.life <= 0.0 {
                resolved.push(index);
                continue;
            }
            let mut remaining = dt;
            let mut landed = false;
            while remaining > 1e-6 && !landed {
                let speed = ow_sim::length(projectile.vel);
                let max_step = if speed > 1e-6 {
                    (0.4 / speed).clamp(1e-4, 0.05)
                } else {
                    remaining
                };
                let sub = remaining.min(max_step);
                remaining -= sub;
                let delta = ow_sim::scale(projectile.vel, sub);
                let distance = ow_sim::length(delta);
                if distance > 1e-6 {
                    let dir = ow_sim::normalize(delta);
                    let mut best: Option<(usize, f32)> = None;
                    for (other, target) in self.bots.iter().enumerate() {
                        if !target.alive {
                            continue;
                        }
                        if let Some(d) = ow_sim::ray_capsule(projectile.pos, dir, target, distance)
                        {
                            if best.map(|b| d < b.1).unwrap_or(true) {
                                best = Some((other, d));
                            }
                        }
                    }
                    if let Some((hit_index, _at)) = best {
                        let outcome = self.bots[hit_index].take_damage(projectile.damage, &rules);
                        let label = self.labels[hit_index].clone();
                        self.log.push(format!(
                            "{label}: dealt {:.0} (mitigated {:.0}), pool {:.0}",
                            outcome.dealt,
                            outcome.mitigated,
                            outcome.remaining.total()
                        ));
                        landed = true;
                        break;
                    }
                    projectile.pos = ow_sim::add(projectile.pos, delta);
                }
                projectile.vel[1] -= projectile.gravity * sub;
            }
            projectile.life -= dt;
            if landed || projectile.life <= 0.0 {
                resolved.push(index);
            }
        }
        for index in resolved.into_iter().rev() {
            self.projectiles.swap_remove(index);
        }
    }

    /// The HUD line the window title carries.
    ///
    /// Kept inside the overlay's 64 columns: `HS`/`PJ` carry the weapon mode,
    /// pellets, and the range or speed+gravity. The bot pool minimum moved to
    /// the fire log, which reports each hit's pool anyway.
    pub fn hud(&self) -> String {
        let p = self.controller.pos;
        let alive = self.bots.iter().filter(|b| b.alive).count();
        let shown: Vec<String> = self.log.iter().rev().take(3).rev().cloned().collect();
        let weapon_desc = match self.weapon.mode {
            FireMode::Hitscan { range } => {
                format!(
                    "HS x{} r{:.0} {:.2}s",
                    self.weapon.pellets, range, self.weapon_interval
                )
            }
            FireMode::Projectile { speed, gravity } => {
                format!(
                    "PJ x{} s{:.0} g{:.1} {:.2}s",
                    self.weapon.pellets, speed, gravity, self.weapon_interval
                )
            }
        };
        format!(
            "pos ({:.1},{:.1},{:.1})  bots {alive}/{}  v {:.2}  {weapon_desc}  |  {}",
            p[0],
            p[1],
            p[2],
            self.bots.len(),
            self.controller.vertical,
            shown.join(" / ")
        )
    }
}

/// Per-hero mover figures, health pools, and fire interval from the install
/// for one hero index.
pub fn hero_spec_and_pools(
    data: &GameData,
    hero_index: u32,
) -> Result<(MoverSpec, PoolSet, f32), String> {
    let mut guids = data.guids_of_type(guid::types::HERO);
    guids.sort_unstable();
    let guid = *guids
        .iter()
        .find(|g| guid::index_of(**g) == hero_index)
        .ok_or("hero index not found")?;
    let mut stu = data.stu_of(guid).map_err(|e| e.to_string())?;
    let Some(hero) = stu.instance(ow_tank::classes::HERO) else {
        return Err("hero asset has no hero instance".into());
    };
    let Some(entity) = stu.read_asset_ref(hero, ow_tank::classes::hero::GAMEPLAY_ENTITY) else {
        return Err("hero asset has no gameplay entity".into());
    };
    drop(stu);

    let pools = health_poolset_of(data, entity)?;

    // The full mover: base speed plus every other float the component
    // declares (back/side/crouch/terminal slots), so the sim keeps the
    // install's slots instead of defaulting them (`ow_tank::entity::
    // mover_fields_of`, measured in `ow-sim/src/movement.rs`).
    let (speed, unlabelled) =
        ow_tank::entity::mover_fields_of(data, entity).map_err(|e| e.to_string())?;
    let mut spec = MoverSpec::from_base_speed(speed);
    for (hash, value) in unlabelled {
        spec = spec.with_unlabelled(hash, value);
    }

    // The fire interval from the volley join, with the legacy all-params
    // scan as fallback (it sees non-volley rate literals the volley pick
    // skips) and the half-second default last.
    let figs = ow_tank::volley::weapon_figs_of(data, entity);
    let interval = figs
        .volley
        .shots_per_second
        .map(|(rate, _)| 1.0 / rate)
        .or_else(|| weapon_interval_of(data, entity))
        .unwrap_or(0.5);
    Ok((spec, pools, interval))
}

/// PoolSet via the existing health reader.
fn health_poolset_of(data: &GameData, entity: u64) -> Result<PoolSet, String> {
    let pools = ow_tank::entity::health_of(data, entity).map_err(|e| e.to_string())?;
    Ok(PoolSet {
        health: pools.health.unwrap_or(0) as f32,
        armour: pools.armour.unwrap_or(0) as f32,
        shields: pools.shields.unwrap_or(0) as f32,
    })
}

/// The shortest fire interval named across the hero's volley graphs.
///
/// `numShotsPerSecond` is a *named* field, so this reading is name-backed; a
/// hero without one gets the half-second default and the HUD makes that
/// visible in the interval it fires at.
fn weapon_interval_of(data: &GameData, entity: u64) -> Option<f32> {
    const HASH: u32 = ow_tank::classes::statescript::NUM_SHOTS_PER_SECOND;
    let weapons = ow_tank::weapons::weapons_of(data, entity).ok()?;
    let mut best_rate: Option<f32> = None;
    for weapon in &weapons {
        let Some(graph) = weapon.graph else { continue };
        let Ok(stu) = data.stu_of(graph) else {
            continue;
        };
        for param in ow_tank::graph::all_params_of(&stu, &data.classes) {
            if param.field_hash != HASH {
                continue;
            }
            let rate = match param.value {
                ow_tank::entity::Value::Float(v) if v > 0.0 => v,
                ow_tank::entity::Value::Int(v) if v > 0 => v as f32,
                _ => continue,
            };
            // 999 is "as fast as input allows", not a rate
            // (docs/formats/weapons.md) — it must not become 999 shots/s.
            if rate >= 999.0 {
                continue;
            }
            best_rate = Some(best_rate.map_or(rate, |b| b.max(rate)));
        }
    }
    best_rate.map(|rate| 1.0 / rate)
}

/// The mode a picked volley's figures imply.
///
/// A *named* `projectileSpeed` literal outranks the `?`-assumed `castLength?`:
/// once a single volley is picked (which is what [`hero_weapon`] does), its
/// own speed is the stronger evidence, and the cast length then reads as the
/// projectile's range cap — measured on the install, hero 5 carries speed 85,
/// gravity 9.8 and pellets 5 on a volley that also carries cast 1000, and
/// hero 810 speed 70 with gravity 9.8 (`D:/ow-dumps/wfigs.txt`). Without a
/// speed, a cast length is hitscan with that range; without either, the
/// harness's 200 m hitscan default stands.
// FIT-COM-06: cast-length interpretation: assumed
fn volley_mode(cast: Option<f32>, speed: Option<f32>, gravity: f32) -> FireMode {
    if let Some(speed) = speed {
        return FireMode::Projectile { speed, gravity };
    }
    if let Some(range) = cast {
        return FireMode::Hitscan { range };
    }
    FireMode::Hitscan { range: 200.0 }
}

/// A hero's sandbox weapon from the install's volley figures.
///
/// The mode comes from [`volley_mode`] (speed first, then cast length, then
/// the 200 m default). Pellets, lifetime and motion gravity ride along where
/// the files declare them; spread/falloff/ammo stay at defaults (unread).
/// Damage stays supplied (`--damage`): the modify-health literals are
/// candidates, reported in the note, never labelled.
// FIT-COM-05: spread, falloff and ammo: left unread
pub fn hero_weapon(
    damage: f32,
    hero_index: u32,
    data: &GameData,
) -> (Weapon, Vec<(u64, usize, u32, f32)>, String) {
    let entity = {
        let mut guids = data.guids_of_type(guid::types::HERO);
        guids.sort_unstable();
        guids
            .iter()
            .find(|g| guid::index_of(**g) == hero_index)
            .and_then(|hero_guid| {
                data.stu_of(*hero_guid).ok().and_then(|mut stu| {
                    stu.instance(ow_tank::classes::HERO).and_then(|hero| {
                        stu.read_asset_ref(hero, ow_tank::classes::hero::GAMEPLAY_ENTITY)
                    })
                })
            })
    };
    let figs = entity
        .map(|e| ow_tank::volley::weapon_figs_of(data, e))
        .unwrap_or_default();
    // One source of truth for the cadence: the picked volley's rate, the
    // legacy graph-wide scan as fallback, the half-second default last. The
    // play state's fire timer reads this same `Weapon.interval`.
    let interval = figs
        .volley
        .shots_per_second
        .map(|(rate, _)| 1.0 / rate)
        .or_else(|| entity.and_then(|e| weapon_interval_of(data, e)))
        .unwrap_or(0.5);
    let pellets = figs
        .volley
        .projectiles_per_shot
        .map(|(p, _)| p)
        .unwrap_or(1);
    let mode = volley_mode(
        figs.volley.cast_length,
        figs.volley.projectile_speed,
        figs.gravity.unwrap_or(0.0),
    );
    let lifetime = match mode {
        FireMode::Projectile { .. } => figs.volley.projectile_lifetime,
        FireMode::Hitscan { .. } => None,
    };
    let name = match figs.graph {
        Some(g) => format!("graph {g:#x} volley"),
        None => "sandbox default".into(),
    };
    let weapon = Weapon {
        name: name.clone(),
        interval,
        pellets,
        damage_per_projectile: damage,
        mode,
        lifetime,
        spread_deg: 0.0,
        falloff: None,
        ammo: None,
        reload: None,
    };
    let mut note = match figs.graph {
        Some(g) => format!("weapon {g:#x}: {pellets} pellets interval {interval:.3}s"),
        None => "weapon: no volley in this build's graphs; sandbox defaults".into(),
    };
    if !figs.damage_candidates.is_empty() {
        let shown: Vec<String> = figs
            .damage_candidates
            .iter()
            .take(4)
            .map(|(_, _, _, v)| format!("{v}"))
            .collect();
        note.push_str(&format!(
            "; damage candidates [{}] (unlabelled)",
            shown.join(", ")
        ));
    }
    (weapon, figs.damage_candidates, note)
}

/// One line on what the sandbox knows about a hero's gameplay beyond pools.
///
/// Volley provenance + damage candidates (file-side) plus the ability join
/// state: the per-hero `m_heroLoadout` walk is empty in this build (measured
/// 0/135 heroes), so abilities are reported as the global `0x09E` count with
/// the pointer to `docs/formats/weapons.md`, never as per-hero cooldowns.
pub fn hero_gameplay_note(data: &GameData, hero_index: u32) -> String {
    let per_hero = ow_tank::entity::loadouts_of(data, hero_index)
        .map(|v| v.len())
        .unwrap_or(0);
    let abilities_global = ow_tank::entity::all_loadout_guids(data).len();
    format!(
        "abilities: per-hero {per_hero} in this build (global {abilities_global} loadouts); cooldown?/damage? stay ?"
    )
}

/// Append the marker batch to a scene: one unit box drawn once per instance,
/// covering `bots` bot boxes and `projectiles` projectile markers. Returns
/// `(bot_base, projectile_base, projectile_capacity)` instance indices.
///
/// The sandbox sim has no geometry of its own in the viewer (`ow-game` draws
/// its own boxes; `--play` did not draw bots at all), so hitscan kills and
/// projectile flight were invisible. The mesh is unskinned (joint 0 is the
/// identity) and points at the white placeholder; per-instance matrices are
/// rewritten every frame through [`crate::Renderer::set_instance`].
fn install_markers(
    scene: &mut scene::Scene,
    bots: usize,
    projectiles: usize,
) -> (usize, usize, usize) {
    // A 24-vertex unit box (per-face normals) spanning [-0.5, 0.5]^3.
    let base_vertex = scene.vertices.len() as i32;
    let first_index = scene.indices.len() as u32;
    let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
        (
            [0.0, 0.0, -1.0],
            [
                [-0.5, -0.5, -0.5],
                [0.5, -0.5, -0.5],
                [0.5, 0.5, -0.5],
                [-0.5, 0.5, -0.5],
            ],
        ),
        (
            [0.0, 0.0, 1.0],
            [
                [-0.5, -0.5, 0.5],
                [0.5, -0.5, 0.5],
                [0.5, 0.5, 0.5],
                [-0.5, 0.5, 0.5],
            ],
        ),
        (
            [-1.0, 0.0, 0.0],
            [
                [-0.5, -0.5, -0.5],
                [-0.5, -0.5, 0.5],
                [-0.5, 0.5, 0.5],
                [-0.5, 0.5, -0.5],
            ],
        ),
        (
            [1.0, 0.0, 0.0],
            [
                [0.5, -0.5, -0.5],
                [0.5, -0.5, 0.5],
                [0.5, 0.5, 0.5],
                [0.5, 0.5, -0.5],
            ],
        ),
        (
            [0.0, 1.0, 0.0],
            [
                [-0.5, 0.5, -0.5],
                [0.5, 0.5, -0.5],
                [0.5, 0.5, 0.5],
                [-0.5, 0.5, 0.5],
            ],
        ),
        (
            [0.0, -1.0, 0.0],
            [
                [-0.5, -0.5, -0.5],
                [0.5, -0.5, -0.5],
                [0.5, -0.5, 0.5],
                [-0.5, -0.5, 0.5],
            ],
        ),
    ];
    for (normal, corners) in faces {
        let at = scene.vertices.len() as u32;
        for (k, corner) in corners.iter().enumerate() {
            scene.vertices.push(scene::Vertex {
                position: *corner,
                normal,
                uv: [(k & 1) as f32, ((k >> 1) & 1) as f32],
                // Joint 0 is the identity slot in every palette this renderer
                // uploads (unlike the `[1,0,0,0]` map-prop default, which
                // relies on the identity pool at base 1 and would take the
                // hero's bone 0 in an animated scene).
                joints: [0, 0, 0, 0],
                weights: [255, 0, 0, 0],
            });
        }
        scene
            .indices
            .extend_from_slice(&[at, at + 1, at + 2, at, at + 2, at + 3]);
    }
    let instance_start = scene.instances.len() as u32;
    let count = (bots + projectiles) as u32;
    let parked = park_matrix().to_cols_array_2d();
    for _ in 0..count {
        scene.instances.push(scene::Instance { model: parked });
    }
    scene.draws.push(scene::Draw {
        first_index,
        index_count: scene.indices.len() as u32 - first_index,
        base_vertex,
        instance_start,
        instance_count: count,
        texture: 0,
        model: 0,
    });
    (
        instance_start as usize,
        instance_start as usize + bots,
        projectiles,
    )
}

/// A matrix that moves a marker far below the ground, for unused slots.
fn park_matrix() -> glam::Mat4 {
    glam::Mat4::from_translation(glam::Vec3::new(0.0, -1000.0, 0.0))
}

/// The centroid of placement origins, for a sensible spawn.
fn centroid(scene: &scene::Scene) -> Vec3 {
    let view = scene.tour_points(64);
    let n = view.len();
    if n == 0 {
        return [0.0, 4.0, 0.0];
    }
    let scale = 1.0 / n as f32;
    view.iter().fold([0.0f32; 3], |acc, p| {
        [
            acc[0] + p[0] * scale,
            acc[1] + p[1] * scale,
            acc[2] + p[2] * scale,
        ]
    })
}

/// Bots stand with the pools of real heroes the install carries, labelled by
/// install index since names are not in the readable data.
fn stock_bots(data: &GameData, count: usize) -> Result<Vec<(PoolSet, String)>, String> {
    const PICK: [u32; 3] = [2, 3, 7]; // by install index: the 250/150/450-ish pool spans
    let mut out = Vec::new();
    for i in 0..count {
        let hero = PICK[i % PICK.len()];
        let (_spec, pools, _) = hero_spec_and_pools(data, hero)?;
        out.push((pools, format!("hero{hero}")));
    }
    Ok(out)
}

// ---- headless run -------------------------------------------------------

/// Open the install exactly the way `scene::build` does, so both read the same
/// bytestream and the collider matches what was drawn.
pub fn open_data() -> Result<GameData, String> {
    let root = ow_casc::install::detect_root()
        .ok_or("no Overwatch install found; set OW_INSTALL to the folder containing .build.info")?;
    let sources = ow_tank::keytable::Sources::discover(std::slice::from_ref(&root));
    let install_root = root.display().to_string();
    GameData::open(
        &install_root,
        &ow_tank::ManifestSelector::default(),
        &sources,
    )
    .map_err(|e| e.to_string())
}

/// The **blend-tree set's clip list**, read the way the client nests it: for
/// each item instance of the FP set asset decode its own fields
/// (`0x5AD927D3` = the per-item `STUAnimation[]` refs) through the class
/// registry, and beside it each gamedata instance's `{slot enum
/// `0xB2E34614 (STUPlayAnimSlot), weight f32 `0x9EFA9957`}`. The gamedata
/// instance follows its item in the set's table (the measured pattern),
/// except the set's head — items 1..6 carry no marked gamedata before the
/// first one, which is why the head group prints head-of-table (the idle
/// candidates by shape).
///
/// Returns the items verbatim, in table order: `(clips, Option<(slot,
/// weight)>)`, with the `head` count named (how many items precede the first
/// gamedata). No slot is named; the enum's member names are hashes wherever
/// they have been read.
// FIT-ANI-06: blend-item order: read mapping, fitted driver math
fn read_blend_set(
    data: &GameData,
    set: u64,
) -> Result<(Vec<(Vec<u64>, Option<(u32, Option<f32>)>)>, usize), String> {
    let registry = {
        let root = ow_casc::install::detect_root()
            .ok_or("no Overwatch install found while reading the blend set")?;
        let sources = ow_tank::keytable::Sources::discover(std::slice::from_ref(&root));
        ow_tank::classreg::ClassRegistry::load_from_sources(&sources)
    };
    if registry.is_empty() {
        return Err("no STU class checkout found; set OW_OWLIB to read blend items".into());
    }
    let reg = registry.inner();
    let mut stu = data.stu_of(set).map_err(|e| e.to_string())?;
    let mut items: Vec<(Vec<u64>, Option<(u32, Option<f32>)>)> = Vec::new();
    for i in 0..stu.instances.len() {
        match stu.instances[i].hash {
            0x3b150012 => {
                let mut clips = Vec::new();
                if let Some(ow_stu::types::Value::Array(items)) =
                    stu.typed_field(i, 0x5ad927d3, reg)
                {
                    for it in items {
                        if let ow_stu::types::Value::AssetRef(g) = it {
                            clips.push(g);
                        }
                    }
                }
                items.push((clips, None));
            }
            0x89f8dbb3 => {
                let slot = stu.typed_field(i, 0xb2e34614, reg).and_then(|v| match v {
                    ow_stu::types::Value::Enum { raw, .. } => Some(raw as u32),
                    _ => None,
                });
                let weight = stu.typed_field(i, 0x9efa9957, reg).and_then(|v| match v {
                    ow_stu::types::Value::Prim(ow_stu::types::Prim::F32, raw) => {
                        Some(f32::from_bits(raw as u32))
                    }
                    _ => None,
                });
                // the gamedata follows its item in the measured table order
                if let Some(last) = items.last_mut() {
                    last.1 = Some((slot.unwrap_or(u32::MAX), weight));
                }
            }
            _ => {}
        }
    }
    let head = items
        .iter()
        .position(|(_, gd)| gd.is_some())
        .unwrap_or(items.len());
    Ok((items, head))
}

/// Print the client's per-hero first-person component values.
///
/// `STUFirstPersonComponent` (0x9D28963F) carries the FP data read by
/// `fpcomp_probe`: an offset struct (`m_position`), the view-model entity
/// and four floats (hero 3: 50 / 1.25 / 0.875 / 0.8). Which float is the FP
/// camera's FOV, eye height or scale is not yet pinned by a consumer read;
/// the values are printed here so a run records them next to the flags.
fn print_first_person_component(
    data: &GameData,
    reg: &ow_stu::types::ClassRegistry,
    parts: &ow_tank::entity::HeroParts,
) {
    let Some(p) = ow_tank::firstperson::read(data, reg, parts.entity) else {
        return;
    };
    eprintln!(
        "first-person component (client data): fov {:.3} eye {:.3} scale {:.3}/{:.3} offset ({:.5}, {:.5}, {:.5}) spring ({:.1}, {:.1}, {:.1}) view-model entity {:#x}",
        p.fov,
        p.eye_height,
        p.scale_a,
        p.scale_b,
        p.offset[0],
        p.offset[1],
        p.offset[2],
        p.spring[0],
        p.spring[1],
        p.spring[2],
        p.view_model_entity
    );
}

/// Build the first-person blend tree for `--anim auto` if the hero's set
/// carries one. Read through `ow_tank::blendtree`; the tree is content, so
/// a hero without one falls back to the single-clip resolver.
fn build_tree_anim(
    data: &GameData,
    scene: &scene::Scene,
    args: &[String],
    hero: u32,
) -> Option<crate::anim_tree::TreeAnim> {
    let parts = ow_tank::entity::hero_parts(data, hero).ok()?;
    let set = parts.view_model_anim_set;
    if set == 0 {
        return None;
    }
    let root = ow_casc::install::detect_root()?;
    let sources = ow_tank::keytable::Sources::discover(&[root]);
    let registry = ow_tank::classreg::ClassRegistry::load_from_sources(&sources);
    let reg = registry.inner();
    print_first_person_component(data, reg, &parts);
    let item_index = flag_f32(args, "--anim-item").unwrap_or(3.0).max(0.0) as usize;
    let item = match ow_tank::blendtree::read_set_item(data, reg, set, item_index) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("tree: {e}; falling back to the single clip");
            return None;
        }
    };
    if item.tree == 0 {
        return None;
    }
    let tree = match ow_tank::blendtree::read_tree(data, reg, item.tree) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("tree: {e}; falling back to the single clip");
            return None;
        }
    };
    let _skel_ids: Vec<u32> = scene
        .hero_model_guid
        .and_then(|m| data.bytes_of(m).ok())
        .and_then(|mb| ow_tank::skeleton::parse_model(&mb).ok())
        .map(|s| s.ids)
        .unwrap_or_default();
    let fp_skel = data
        .bytes_of(parts.view_model)
        .ok()
        .and_then(|b| ow_tank::skeleton::parse_model(&b).ok());
    let char_skel = data
        .bytes_of(parts.model)
        .ok()
        .and_then(|b| ow_tank::skeleton::parse_model(&b).ok());
    let char_local = char_skel
        .as_ref()
        .map(|s| ow_tank::skin::Skin::from_skeleton(s).local)
        .unwrap_or_default();
    let bones = scene.skin.as_ref().map(|s| s.parents.len()).unwrap_or(0);
    let mut clips = Vec::with_capacity(item.clips.len());
    for guid in &item.clips {
        let layer = data.bytes_of(*guid).ok().and_then(|bytes| {
            ow_tank::clip::BlendPose::decode(&bytes).map(|source| {
                let bone_chain = match (&fp_skel, &char_skel) {
                    (Some(fp), Some(ch)) => crate::anim_tree::build_chains(
                        &source,
                        &fp.ids,
                        &fp.parents,
                        &ch.ids,
                        &ch.parents,
                        &char_local,
                    ),
                    _ => vec![Vec::new(); bones],
                };
                crate::anim_tree::ClipLayer {
                    guid: *guid,
                    source,
                    bone_chain,
                }
            })
        });
        clips.push(layer);
    }
    // Category asset: 0x028xxxxxxxNNNN. A bare number selects by low id.
    let cat = flag_f32(args, "--anim-category").unwrap_or(5.0);
    let category = if cat >= 0x0280_0000u64 as f32 && cat <= 0xFFFF_FFFFu64 as f32 {
        cat as u64
    } else {
        0x0280_0000_0000_0000 | (cat as u64 & 0xFFFF)
    };
    Some(crate::anim_tree::TreeAnim {
        tree,
        slots: item.slots,
        clips,
        category,
        x: flag_f32(args, "--tree-x").unwrap_or(0.0),
        y: flag_f32(args, "--tree-y").unwrap_or(0.0),
        layer_gain: flag_f32(args, "--tree-gain").unwrap_or(1.0),
        layer_weight: flag_f32(args, "--tree-layer"),
    })
}

/// The idle clip through the hero's own data, the `--anim auto` resolver.
///
/// **Measured correction (2026-10-19):** the first slot-marked item is *not*
/// the idle. On hero 3 it is item 6 (`STUPlayAnimSlot` member `0x43619957`),
/// whose clip `0x0a00000000000065` the operator recognized on screen as
/// Tracer's **reload** ("recognizable but deformed"). The idle is the
/// **unmarked head chain** (the items before the first gamedata): item 4
/// holds the operator-verified pair, c71 (the 16-frame sway) and c50 (the
/// 5-frame rest). The shape rule here picks the **longest card-family clip
/// among the unmarked items** (a sway loop is longer than a rest pose; c71 on
/// hero 3), so the resolver is not tied to one item index. Fallbacks: the
/// last unmarked item's first clip, then the old first-marked item.
// FIT-ANI-01: playback fallback: single clip when there is no tree
fn resolve_idle_clip(data: &GameData, hero: u32) -> Option<u64> {
    let parts = ow_tank::entity::hero_parts(data, hero).ok()?;
    let set = parts.view_model_anim_set;
    if set == 0 {
        return None;
    }
    let (items, head) = read_blend_set(data, set).ok()?;
    let mut best: Option<(u16, u64)> = None;
    for (clips, gd) in items.iter().take(head) {
        if gd.is_some() {
            continue;
        }
        for g in clips.iter().copied().filter(|g| *g != 0) {
            let Ok(bytes) = data.bytes_of(g) else {
                continue;
            };
            let Ok(clip) = ow_tank::clip::Clip::parse(&bytes) else {
                continue;
            };
            if !ow_tank::clip::is_blend_card_family(clip.header.family) {
                continue;
            }
            if best.map(|(f, _)| clip.header.frames > f).unwrap_or(true) {
                best = Some((clip.header.frames, g));
            }
        }
    }
    if let Some((_, g)) = best {
        return Some(g);
    }
    items
        .get(head.saturating_sub(1))
        .or_else(|| items.get(head))
        .and_then(|(clips, _)| clips.first().copied())
}

/// Whether a record's sampled translation reads over 1.5× the bone's own
/// bind-local segment length — the two upper-arm rows (13/54) of the FP card
/// family (`docs/formats/animation.md` § "The two arm rows ..."). This is a
/// **diagnostic**, not a hold: the pose applies the file's P lane (the client's
/// slot has a position lane); the over-length rows are the source rig's grip
/// width.
fn holds_unphysical(
    scene: &crate::scene::Scene,
    bone: usize,
    sample: Option<(Option<glam::Quat>, Option<glam::Vec3>)>,
) -> bool {
    let Some(skin) = scene.skin.as_ref() else {
        return false;
    };
    let Some(sample) = sample else {
        return false;
    };
    let Some(t) = sample.1 else {
        return false;
    };
    let own = skin.local[bone].w_axis;
    let own_len = glam::Vec3::new(own.x, own.y, own.z).length();
    t.length() > own_len * 1.5 && own_len > 0.0
}

/// `--play-th <seconds>`: advance the state headless with a scripted schedule,
/// printing the HUD line every half second. The point is that a run is
/// repeatable: the same schedule produces the same HUD trace to diff across
/// builds.
pub fn headless(args: &[String]) -> Result<Vec<String>, String> {
    let seconds = args
        .iter()
        .position(|a| a == "--play-th")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse::<f32>().ok())
        .ok_or("--play-th needs a seconds value")?;
    let supplied = Supplied {
        damage: flag_f32(args, "--damage").unwrap_or(50.0),
        hero: flag_f32(args, "--hero").unwrap_or(3.0) as u32,
        bots: flag_f32(args, "--bots").unwrap_or(3.0) as usize,
    };
    let data = open_data()?;
    // Same first-person placement as the windowed harness (see `windowed`).
    let mut scene_args = args.to_vec();
    if !scene_args.iter().any(|a| a == "--view-model") {
        scene_args.push("--view-model".into());
    }
    // And at the player's own transform — the client's offsets from it are all
    // zero, so that is where the first-person model belongs (`Scene::
    // view_model_matrix`). Without it the static placement would be the
    // inspection framing, which is not what `--play` draws.
    if !scene_args.iter().any(|a| a == "--first-person") {
        scene_args.push("--first-person".into());
    }
    if !scene_args.iter().any(|a| a == "--hero") {
        scene_args.push("--hero".into());
        scene_args.push(supplied.hero.to_string());
    }
    let scene = scene::build(&scene_args)?;
    let collider = scene::Collider::build(&scene);
    let mut state = PlayState::build(&data, &scene, &collider, &supplied)?;
    eprintln!("play: {}", state.weapon_note);

    // The blend tree the windowed harness would pose with, decoded here too
    // so a headless run reports the same content and the same fits.
    if matches!(flag_str(args, "--anim"), Some("auto") | Some("AUTO")) {
        if let Some(tree) = build_tree_anim(&data, &scene, args, supplied.hero) {
            eprintln!("{}", tree.summary());
        }
    }

    // `--anim <guid|auto>` on the headless trace: evaluate one blend pose
    // mid-clip and report how many skin bones moved off bind. This is the
    // window-less proof that the feed pairs and poses; the full run is the
    // interactive `--play` above. `auto` = the set's head clip through the
    // hero's own `m_animBlendTreeSet`.
    if let Some(hex) = flag_str(args, "--anim") {
        let guid = match hex {
            "auto" | "AUTO" => resolve_idle_clip(&data, supplied.hero),
            hex => u64::from_str_radix(hex.trim_start_matches("0x"), 16).ok(),
        };
        if let Some(guid) = guid {
            if let Ok(bytes) = data.bytes_of(guid) {
                if let Ok(clip) = ow_tank::clip::Clip::parse(&bytes) {
                    println!(
                        "anim auto: {guid:#018x}, family {:#010x}, {} frames @ {} fps",
                        clip.header.family, clip.header.frames, clip.header.fps
                    );
                    // Any kind-006 family the pose readers decode: cards via
                    // the verified card path, the rest via the general row
                    // reader (`decode_general_records`, the published
                    // reference's row-relative walk).
                    if let Some(source) = ow_tank::clip::BlendPose::decode(&bytes) {
                        let skel_ids: Vec<u32> = scene
                            .hero_model_guid
                            .and_then(|m| data.bytes_of(m).ok())
                            .and_then(|mb| ow_tank::skeleton::parse_model(&mb).ok())
                            .map(|s| s.ids)
                            .unwrap_or_default();
                        let mut moved = 0usize;
                        let mut paired = 0usize;
                        let mut held: Vec<u32> = Vec::new();
                        if let Some(skin) = scene.skin.as_ref() {
                            let mid = source.frames() as f32 * 0.5 / (source.fps());
                            for record in 0..source.records() {
                                let Some(id) = source.bone_id(record) else {
                                    continue;
                                };
                                let Some(bone) = skel_ids.iter().position(|x| *x == id) else {
                                    continue;
                                };
                                paired += 1;
                                if bone >= skin.local.len() {
                                    continue;
                                }
                                // Diagnostic only: the file rows whose P lane
                                // reads over 1.5x the bone's bind length. The
                                // pose holds every translation at bind (the
                                // runtime pose slot is quaternion-only), so
                                // these do not drive.
                                if holds_unphysical(&scene, bone, source.sample_record(record, mid))
                                {
                                    held.push(id);
                                }
                                let Some((_r, t)) = source.sample_record(record, mid) else {
                                    continue;
                                };
                                let (_, _, bind_t) =
                                    skin.local[bone].to_scale_rotation_translation();
                                let pt = t.unwrap_or(bind_t);
                                // the file's own translation differs from bind?
                                let dt2 = (pt - bind_t).length();
                                if dt2 > 1e-3 {
                                    moved += 1;
                                }
                            }
                        }
                        println!(
                                "anim eval: {paired} paired records; rotations and the file translations drive (the slot has a position lane); {moved} file translations differ from bind at mid-clip; over-1.5x rows {held:?} (applied)",
                            );
                    }
                }
            }
        }
    }

    let mut out = Vec::new();
    let dt = 1.0 / 60.0;
    let mut sim = 0.0f32;
    let mut framed = 0usize;
    let frames = (seconds * 60.0) as usize;
    while framed < frames {
        sim += dt;
        // Schedule: walk forward for the first second, jump once at 1.2s,
        // fire from 1.5s onward.
        let jump = (sim - 1.2).abs() < dt * 0.5;
        let input = Input {
            forward: if sim < 1.0 { 1.0 } else { 0.0 },
            side: 0.0,
            crouch: false,
            jump,
        };
        state.firing = sim >= 1.5;
        state.frame(dt, input, Vec2::ZERO);
        if (sim * 2.0).round() as i32 != ((sim - dt) * 2.0).round() as i32 {
            out.push(format!("t={sim:.2} {}", state.hud()));
        }
        framed += 1;
    }
    for line in &out {
        println!("{line}");
    }
    Ok(out)
}

fn flag_str<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(|v| v.as_str())
}

fn flag_f32(args: &[String], name: &str) -> Option<f32> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
}

// ---- a scriptable fire for tests ---------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use ow_sim::movement::MoverSpec;

    #[test]
    fn camera_and_movement_share_one_yaw_convention() {
        // The regression this pins: the renderer's camera used (cos, sin) for
        // its forward while the trace-fitted mover uses (sin, cos), so the view
        // and W disagreed by 90° and turning the mouse looked like it did not
        // steer. For every yaw the two must point the same way.
        for yaw in [0.0f32, 0.7, 1.570_796_4, 3.0, -2.2] {
            let spec = MoverSpec::from_base_speed(5.5);
            let mut c = Controller::new(spec, [0.0, 0.0, 0.0], yaw);
            c.advance(
                1.0 / 30.0,
                Input {
                    forward: 1.0,
                    ..Default::default()
                },
            );
            let v = [c.vel[0], c.vel[1]];
            let len = (v[0] * v[0] + v[1] * v[1]).sqrt();
            assert!(len > 1.0, "did not move at yaw {yaw}: {v:?}");
            let f = crate::Camera {
                position: glam::Vec3::ZERO,
                yaw,
                pitch: 0.0,
                speed: 0.0,
            }
            .forward();
            let fl = (f.x * f.x + f.z * f.z).sqrt();
            let dot = (v[0] / len) * (f.x / fl) + (v[1] / len) * (f.z / fl);
            assert!(
                dot > 0.999,
                "yaw {yaw}: movement {v:?} vs view forward ({}, {})",
                f.x,
                f.z
            );
        }
    }

    #[test]
    fn sandbox_wall_stops_a_walk() {
        let sb = Sandbox::around([0.0, 1.75, 0.0], 0.0);
        let block = sb.boxes[1]; // the 0.9 m block at +12z
        let wall = sb.boxes[2]; // the 2.5 m wall at +18z
                                // A grounded walk from the spawn meets the block first (0.9 > step).
        let mut pos = [0.0f32, 1.75, 0.0];
        for _ in 0..600 {
            pos[2] += 0.2;
            sb.resolve(&mut pos, true);
        }
        assert!(
            (pos[2] - (block.0[2] - 0.4)).abs() < 1e-3,
            "walk stopped at {pos:?}, expected the block face {:#?}",
            block.0[2] - 0.4
        );
        // Starting past the block, the same walk stops at the wall.
        let mut pos = [0.0f32, 1.75, 14.0];
        for _ in 0..600 {
            pos[2] += 0.2;
            sb.resolve(&mut pos, true);
        }
        assert!(
            (pos[2] - (wall.0[2] - 0.4)).abs() < 1e-3,
            "walk stopped at {pos:?}, expected the wall face {:#?}",
            wall.0[2] - 0.4
        );
    }

    #[test]
    fn sandbox_steps_up_a_lip_and_not_a_block() {
        let sb = Sandbox::around([0.0, 1.75, 0.0], 0.0);
        // 0.5 m platform at +6z: grounded feet climb it, and it is not a wall.
        assert_eq!(sb.ground_at(0.0, 6.0, 0.0, true), 0.5);
        let mut pos = [0.0f32, 1.75, 5.9];
        sb.resolve(&mut pos, true);
        assert_eq!(pos[2], 5.9, "the low lip pushed the body back out");
        // 0.9 m block at +12z: grounded feet do not climb it...
        assert_eq!(sb.ground_at(0.0, 12.0, 0.0, true), 0.0);
        // ...but feet above the top land on it, and airborne feet are pushed out.
        assert_eq!(sb.ground_at(0.0, 12.0, 0.95, false), 0.9);
        let mut air = [0.0f32, 1.2, 12.0];
        sb.resolve(&mut air, false);
        assert!(air[2] != 12.0, "airborne body stayed inside the block");
        // off to the side, the plane.
        assert_eq!(sb.ground_at(20.0, 0.0, 0.0, true), 0.0);
    }

    #[test]
    fn fire_hits_only_bot_in_front() {
        let _ = 0; // no deps: emulate with local state
        let spec = MoverSpec::from_base_speed(5.5);
        let mut state = PlayState {
            controller: Controller::new(spec, [0.0, 1.75, 0.0], 0.0),
            pitch: 0.0,
            bots: vec![Combatant::new(
                [0.0, 0.0, 10.0],
                PoolSet {
                    health: 250.0,
                    armour: 0.0,
                    shields: 0.0,
                },
            )],
            labels: vec!["bot".into()],
            log: Vec::new(),
            damage: 50.0,
            weapon_interval: 0.5,
            weapon: Weapon {
                interval: 0.5,
                ..Weapon::default()
            },
            projectiles: Vec::new(),
            damage_candidates: Vec::new(),
            weapon_note: String::new(),
            fire_timer: 0.0,
            firing: false,
            sandbox: Sandbox::flat(0.0),
        };
        state.controller.yaw = 0.0;
        state.pitch = 0.0;
        state.fire_timer = 0.0;
        state.firing = true;
        // dir (0,0,1) at yaw 0 with pitch 0 in the mover's convention — the
        // same axes the movement uses; the bot stands on that axis.
        let looking = Vec2::ZERO;
        state.frame(0.016, Input::default(), looking);
        assert!(
            state.log.iter().any(|l| l.contains("bot")),
            "{:?}",
            state.log
        );
    }

    #[test]
    fn markers_extend_the_scene_with_one_draw() {
        let mut scene = scene::Scene::empty();
        let (bot_base, proj_base, cap) = install_markers(&mut scene, 3, 8);
        assert_eq!(bot_base, 0, "bots come first in the batch");
        assert_eq!(proj_base, 3);
        assert_eq!(cap, 8);
        assert_eq!(scene.instances.len(), 11, "one instance per marker");
        assert_eq!(scene.draws.len(), 1, "the batch is one draw");
        assert_eq!(scene.draws[0].instance_count, 11);
        assert_eq!(scene.vertices.len(), 24, "a 24-vertex unit box");
        assert_eq!(scene.indices.len(), 36, "12 triangles");
        // Joint 0 is the identity slot in every palette the renderer uploads.
        assert!(scene.vertices.iter().all(|v| v.joints == [0, 0, 0, 0]));
    }

    #[test]
    fn a_literal_speed_outranks_the_hedged_cast_length() {
        // The census (`D:/ow-dumps/wfigs.txt`): hero 5 carries speed 85 with
        // cast 1000 — a projectile weapon, not a 1 km hitscan. Hero 3 carries
        // cast 100 alone — hitscan; neither carries anything — the 200 m
        // default stands.
        match volley_mode(Some(1000.0), Some(85.0), 9.8) {
            FireMode::Projectile { speed, gravity } => {
                assert_eq!(speed, 85.0);
                assert_eq!(gravity, 9.8);
            }
            other => panic!("speed must win: {other:?}"),
        }
        match volley_mode(Some(100.0), None, 0.0) {
            FireMode::Hitscan { range } => assert_eq!(range, 100.0),
            other => panic!("cast length is the range: {other:?}"),
        }
        match volley_mode(None, None, 0.0) {
            FireMode::Hitscan { range } => assert_eq!(range, 200.0),
            other => panic!("the default stands: {other:?}"),
        }
    }

    #[test]
    fn hitscan_pellets_land_per_pellet() {
        let spec = MoverSpec::from_base_speed(5.5);
        let mut state = PlayState {
            controller: Controller::new(spec, [0.0, 1.75, 0.0], 0.0),
            pitch: 0.0,
            bots: vec![Combatant::new(
                [0.0, 0.0, 10.0],
                PoolSet {
                    health: 250.0,
                    armour: 0.0,
                    shields: 0.0,
                },
            )],
            labels: vec!["bot".into()],
            log: Vec::new(),
            damage: 10.0,
            weapon_interval: 0.5,
            weapon: Weapon {
                pellets: 3,
                mode: ow_sim::FireMode::Hitscan { range: 200.0 },
                ..Weapon::default()
            },
            projectiles: Vec::new(),
            damage_candidates: Vec::new(),
            weapon_note: String::new(),
            fire_timer: 0.0,
            firing: false,
            sandbox: Sandbox::flat(0.0),
        };
        state.controller.yaw = 0.0;
        state.pitch = 0.0;
        state.fire_timer = 0.0;
        state.firing = true;
        state.frame(0.016, Input::default(), Vec2::ZERO);
        assert!(
            state.log.iter().any(|l| l.contains("3/3 pellets")),
            "{:?}",
            state.log
        );
        assert_eq!(state.bots[0].pools.total(), 250.0 - 30.0);
    }

    #[test]
    fn projectile_fire_lands_on_step() {
        let spec = MoverSpec::from_base_speed(5.5);
        let mut state = PlayState {
            controller: Controller::new(spec, [0.0, 1.75, 0.0], 0.0),
            pitch: 0.0,
            bots: vec![Combatant::new(
                [0.0, 0.0, 10.0],
                PoolSet {
                    health: 250.0,
                    armour: 0.0,
                    shields: 0.0,
                },
            )],
            labels: vec!["bot".into()],
            log: Vec::new(),
            damage: 10.0,
            weapon_interval: 0.5,
            weapon: Weapon {
                pellets: 1,
                mode: ow_sim::FireMode::Projectile {
                    speed: 50.0,
                    gravity: 0.0,
                },
                lifetime: Some(3.0),
                ..Weapon::default()
            },
            projectiles: Vec::new(),
            damage_candidates: Vec::new(),
            weapon_note: String::new(),
            fire_timer: 0.0,
            firing: false,
            sandbox: Sandbox::flat(0.0),
        };
        state.controller.yaw = 0.0;
        state.pitch = 0.0;
        // A direct trigger pull leaves the shot in flight; nothing lands yet.
        state.fire();
        assert_eq!(state.projectiles.len(), 1);
        assert!(state.log.is_empty());
        // 10 m at 50 m/s lands inside one 0.2 s step.
        state.step_projectiles(0.2);
        assert!(state.projectiles.is_empty());
        assert!(
            state.log.iter().any(|l| l.contains("bot")),
            "{:?}",
            state.log
        );
    }
}

// ---- the interactive window --------------------------------------------

/// Try to hold the pointer in the window: `Locked`, then `Confined`.
/// Returns whether either was accepted — on Windows only `Confined` is.
fn grab_pointer(window: &winit::window::Window) -> bool {
    use winit::window::CursorGrabMode;
    for mode in [CursorGrabMode::Locked, CursorGrabMode::Confined] {
        if window.set_cursor_grab(mode).is_ok() {
            return true;
        }
    }
    false
}

/// Wrap an angle to `(-π, π]`. Used for the camera's frame-to-frame delta so
/// crossing the ±π seam does not read as a full turn.
fn wrap_pi(a: f32) -> f32 {
    (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

/// One semi-implicit Euler step of the view model's drag spring.
///
/// `offset` is the spring's displacement from the camera angle, `vel` its
/// rate, and `drive` the camera's angular velocity (rad/s). The client's block
/// is `STUFirstPersonComponent.m_F486DCF2` (`STU_6AE5A305`, 64 / 16 / -1): the
/// first two are stiffness and damping, the third the drive scale. The rest
/// position is zero, so a constant turn settles at `gain·drive/stiffness`
/// behind the camera and stopping returns the offset to zero.
// FIT-CAM-01: view-model drag spring: form fitted, constants read
fn vm_spring_step(
    offset: &mut f32,
    vel: &mut f32,
    drive: f32,
    stiffness: f32,
    damping: f32,
    gain: f32,
    h: f32,
) {
    let accel = -stiffness * *offset - damping * *vel + gain * drive;
    *vel += accel * h;
    *offset += *vel * h;
}

/// `--play`: open the window, run the loop, HUD in the title bar.
pub fn windowed(args: &[String]) -> Result<(), String> {
    use std::sync::Arc;
    use winit::application::ApplicationHandler;
    use winit::dpi::LogicalSize;
    use winit::event::{ElementState, MouseButton, WindowEvent};
    use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
    use winit::keyboard::{KeyCode, PhysicalKey};
    use winit::window::Window;

    struct PlayApp {
        state: PlayState,
        renderer: Option<crate::Renderer>,
        /// The built geometry, handed to the renderer on `resumed`.
        scene: scene::Scene,
        /// Whether the window holds the pointer. Best effort only: Windows
        /// rejects `Locked`, and a `Confined` grab alone is **not** enough —
        /// with the cursor hidden and pinned at the clip edge it stops
        /// producing deltas, which is why look must not depend on this.
        grabbed: bool,
        /// Whether the window has focus. Look and the pointer re-centre run
        /// only while it does.
        focused: bool,
        /// Radians of look per raw mouse count (`--sens`).
        sensitivity: f32,
        keys: std::collections::HashMap<KeyCode, bool>,
        last_frame: Option<std::time::Instant>,
        look: Vec2,
        firing: bool,
        frame_no: u32,
        /// The animation clip driving the hero's pose, when `--anim` named one.
        anim: Option<Anim>,
        /// Seconds since the animation started, advanced by each frame's dt.
        anim_time: f32,
        /// Frames per second as an exponential average over the recent frames.
        fps: f32,
        /// The view model's yaw/pitch **offset from the camera**, integrated
        /// by the client's spring: `STUFirstPersonComponent.m_F486DCF2`
        /// (`STU_6AE5A305`) defaults to 64 / 16 / -1. The first two are the
        /// stiffness and damping of a critically damped spring (2·√64 = 16);
        /// the -1 is the drive scale. The offset is driven by the camera's
        /// angular velocity and rests at zero, because an absolute-angle
        /// follower with these constants would trail by c/k·ω — 72° at
        /// 5 rad/s — which is not what the client does. Which transform the
        /// block drives is still inferred (the consumer page is cold).
        vm_yaw: f32,
        vm_pitch: f32,
        vm_yaw_vel: f32,
        vm_pitch_vel: f32,
        vm_stiffness: f32,
        vm_damping: f32,
        /// Drive scale on the camera's angular velocity (`--vm-g`, default -1).
        vm_gain: f32,
        // FIT-CAM-02: camera look filter: time constant read, follower form fitted
        /// The camera's own yaw/pitch. With `look_inverse` 0 these are the
        /// mover's exact output; with the client's 0.1 they are a fitted
        /// exponential follow of it.
        cam_yaw: f32,
        cam_pitch: f32,
        /// `camera.lookInterpolationSpeedInverse` (client default 0.1 s).
        /// Opt-in: the consumer page is cold and the earlier whole-view lag
        /// was reverted in session 19g.
        look_inverse: f32,
        /// Fitted first-person camera framing relative to the world eye
        /// (`--fp-cam-drop` / `--fp-cam-back`): the client's FP camera
        /// transform is in the absent renderer pages, and the measured idle
        /// hands sit below a level eye camera.
        fp_cam_drop: f32,
        fp_cam_back: f32,
        /// First-person pass vertical FOV (`--fp-fov`), None = world FOV.
        // FIT-ANI-03: FP camera/view-model parameters: values read, the mapping is fitted
        fp_fov: Option<f32>,
        /// Camera yaw/pitch at the previous frame, for the angular velocity.
        prev_cam_yaw: f32,
        prev_cam_pitch: f32,
        /// When the window opened: the T the HUD reports, so a recorded run or
        /// a probe can be aligned with what was on screen.
        opened: std::time::Instant,
        /// The marker batch's first bot instance index.
        bot_base: usize,
        /// The marker batch's first projectile instance index.
        proj_base: usize,
        /// How many projectile markers the batch holds.
        proj_cap: usize,
        /// Drive the weapon hooks from the character rig's weapon bones
        /// (the chain fold). Off by default: the composition is exact for
        /// the character rig but the FP hooks are a different attachment,
        /// so this is an A/B knob, not a claim.
        weapon_fold: bool,
    }

    /// The two pose sources `--anim` carries, decoded once.
    enum Anim {
        /// A bone-list clip (`PoseClip`): per-bone rotations, positionally
        /// paired.
        Bones(ow_tank::clip::PoseClip),
        /// An FP blend-card clip (`BlendPose`): per-record `{rotation,
        /// translation}` blocks, paired by record id → skin bone through the
        /// model's own skeleton. `pairs` is `pairs[record] = Option<bone>`;
        /// `None` = the record's id is one of the clip's extras the skin
        /// never names. The runtime pose slot is **quaternion-only**
        /// (`FUN_7ff614edc560` / `FUN_7ff614ed98d0`): every record's
        /// translation is held at the bone's bind value and only rotations
        /// drive. The card P lane is not a bone-local translation (the two
        /// shoulder rows read 2.395× bind — see
        /// `docs/formats/animation.md`).
        Blend {
            source: ow_tank::clip::BlendPose,
            /// Per-bone record chains over the character rig (the retarget:
            /// dropped intermediates are composed into the child).
            chains: Vec<Vec<crate::anim_tree::ChainLink>>,
        },
        /// The hero's first-person blend tree (`--anim auto`): the decoded
        /// STUAnimBlendTree plus the item's slots. See
        /// [`crate::anim_tree`] and `docs/formats/animation.md`
        /// 2026-10-20 (5)-(7).
        Tree(crate::anim_tree::TreeAnim),
    }

    impl ApplicationHandler for PlayApp {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            if self.renderer.is_some() {
                return;
            }
            let attributes = Window::default_attributes()
                .with_title("playing")
                .with_inner_size(LogicalSize::new(1280.0, 720.0));
            let window = Arc::new(event_loop.create_window(attributes).expect("a window"));
            // Hold the pointer: `Locked` is not supported on Windows, so try
            // it, then `Confined`, and fall back to a centre warp per frame
            // (`grab_pointer`) — with no grab the cursor reaches the border and
            // look runs out of room there.
            self.grabbed = grab_pointer(&window);
            window.set_cursor_visible(false);
            // Start from the centre so the first delta is measured from there.
            let size = window.inner_size();
            let _ = window.set_cursor_position(winit::dpi::PhysicalPosition::new(
                size.width as f64 / 2.0,
                size.height as f64 / 2.0,
            ));
            eprintln!(
                "play: {} draws, {} instances, {} vertices",
                self.scene.draws.len(),
                self.scene.instances.len(),
                self.scene.vertices.len()
            );
            let mut renderer = crate::Renderer::new_posed(window, &self.scene, self.pose());
            renderer.fp_fov = self.fp_fov;
            self.renderer = Some(renderer);
        }

        /// Raw mouse motion, the only correct source for camera look.
        ///
        /// `WindowEvent::CursorMoved` carries an OS-processed position (winit's
        /// own docs say not to use it for 3D camera control): it stops at the
        /// window edge, it is affected by pointer acceleration, and any warp
        /// used to keep it centred feeds back into the next delta. Raw motion
        /// has none of those problems — and winit's Windows backend registers
        /// raw input with `DeviceEvents::WhenFocused` by default, so these
        /// events arrive while the window is focused without any grab.
        fn device_event(
            &mut self,
            _event_loop: &ActiveEventLoop,
            _device_id: winit::event::DeviceId,
            event: winit::event::DeviceEvent,
        ) {
            if let winit::event::DeviceEvent::MouseMotion { delta } = event {
                if self.focused {
                    self.look.x += delta.0 as f32 * self.sensitivity;
                    self.look.y += delta.1 as f32 * self.sensitivity;
                }
            }
        }

        fn window_event(
            &mut self,
            event_loop: &ActiveEventLoop,
            _id: winit::window::WindowId,
            event: WindowEvent,
        ) {
            let Some(renderer) = self.renderer.as_mut() else {
                return;
            };
            match event {
                WindowEvent::Focused(false) => {
                    self.keys.clear();
                    self.firing = false;
                    self.grabbed = false;
                    self.focused = false;
                }
                WindowEvent::Focused(true) => {
                    self.focused = true;
                    self.grabbed = grab_pointer(&renderer.window);
                    // Re-centre once so the first move after refocusing is a
                    // real delta rather than a jump from wherever the pointer
                    // was left.
                    let size = renderer.window.inner_size();
                    let _ = renderer
                        .window
                        .set_cursor_position(winit::dpi::PhysicalPosition::new(
                            size.width as f64 / 2.0,
                            size.height as f64 / 2.0,
                        ));
                }
                WindowEvent::CloseRequested => event_loop.exit(),
                WindowEvent::Resized(size) => renderer.resize(size.width, size.height),
                WindowEvent::KeyboardInput { event, .. } => {
                    let held = event.state == ElementState::Pressed;
                    if let PhysicalKey::Code(code) = event.physical_key {
                        if held && code == KeyCode::Escape {
                            event_loop.exit();
                        }
                        self.keys.insert(code, held);
                    }
                }
                WindowEvent::MouseInput { state, button, .. } => {
                    if button == MouseButton::Left {
                        self.firing = state == ElementState::Pressed;
                    }
                }
                WindowEvent::CursorMoved { position, .. } => {
                    // Steering does **not** read this position — it comes from
                    // raw mouse motion (see `device_event`). This handler only
                    // nudges the hidden pointer back when it has wandered far
                    // from the centre, so clicks stay in the window; a failed or
                    // delayed warp cannot break the look, because no look delta
                    // is derived from positions.
                    if !self.focused {
                        return;
                    }
                    let size = renderer.window.inner_size();
                    let (cx, cy) = (size.width as f64 / 2.0, size.height as f64 / 2.0);
                    let far = (position.x - cx).abs() > size.width as f64 * 0.25
                        || (position.y - cy).abs() > size.height as f64 * 0.25;
                    if far {
                        let _ = renderer
                            .window
                            .set_cursor_position(winit::dpi::PhysicalPosition::new(cx, cy));
                    }
                }
                WindowEvent::RedrawRequested => {
                    let now = std::time::Instant::now();
                    self.frame_no += 1;
                    let dt = now
                        .duration_since(
                            self.last_frame
                                .take()
                                .unwrap_or(now - std::time::Duration::from_secs_f64(1.0 / 60.0)),
                        )
                        .as_secs_f32()
                        .min(0.1);
                    self.last_frame = Some(now);
                    let held = |c: KeyCode| self.keys.get(&c).copied().unwrap_or(false);
                    let input = Input {
                        forward: (held(KeyCode::KeyW) as i32 - held(KeyCode::KeyS) as i32) as f32,
                        side: (held(KeyCode::KeyD) as i32 - held(KeyCode::KeyA) as i32) as f32,
                        crouch: held(KeyCode::ControlLeft),
                        jump: held(KeyCode::Space),
                    };
                    let look = self.look;
                    self.look = Vec2::ZERO;
                    // The trigger: mouse state → the sim (the wiring was
                    // missing, so clicks fired nothing in the window).
                    self.state.firing = self.firing;
                    self.state.frame(dt, input, look);
                    let p = &self.state.controller;
                    // The camera is exactly the mover's output; the view
                    // model's **offset** from it is the client's drag. The
                    // first-person component's 64 / 16 / -1 block is a
                    // critically damped spring (2·√64 = 16) driven by the
                    // camera's angular velocity: the offset rests at zero and
                    // trails by gain·ω/k (4.5° at 5 rad/s with gain -1), not
                    // the 72° an absolute-angle follower at these constants
                    // would produce. Integrate in small steps so a frame spike
                    // cannot blow the spring up.
                    // Camera look: exact unless the fitted client filter is
                    // enabled. `camera.lookInterpolationSpeedInverse` is 0.1 s
                    // in the install (`0x7ff7c86aeec0`), but its consumer is
                    // cold; the earlier whole-view lag was reverted in 19g, so
                    // this defaults off. The mover
                    // and fire direction keep the exact controller angles;
                    // only the rendered camera is smoothed.
                    let target_yaw = p.yaw;
                    let target_pitch = self.state.pitch;
                    if self.look_inverse > 0.0 {
                        let alpha = 1.0 - (-dt / self.look_inverse).exp();
                        self.cam_yaw += wrap_pi(target_yaw - self.cam_yaw) * alpha;
                        self.cam_pitch += (target_pitch - self.cam_pitch) * alpha;
                    } else {
                        self.cam_yaw = target_yaw;
                        self.cam_pitch = target_pitch;
                    }
                    let cam_yaw = self.cam_yaw;
                    let cam_pitch = self.cam_pitch;
                    let vel_dt = dt.max(1e-6);
                    let cam_yaw_vel = wrap_pi(cam_yaw - self.prev_cam_yaw) / vel_dt;
                    let cam_pitch_vel = (cam_pitch - self.prev_cam_pitch) / vel_dt;
                    self.prev_cam_yaw = cam_yaw;
                    self.prev_cam_pitch = cam_pitch;
                    let steps = ((dt / (1.0 / 120.0)).ceil() as usize).clamp(1, 8);
                    let h = dt / steps as f32;
                    for _ in 0..steps {
                        vm_spring_step(
                            &mut self.vm_yaw,
                            &mut self.vm_yaw_vel,
                            cam_yaw_vel,
                            self.vm_stiffness,
                            self.vm_damping,
                            self.vm_gain,
                            h,
                        );
                        vm_spring_step(
                            &mut self.vm_pitch,
                            &mut self.vm_pitch_vel,
                            cam_pitch_vel,
                            self.vm_stiffness,
                            self.vm_damping,
                            self.vm_gain,
                            h,
                        );
                    }
                    let vm_yaw = cam_yaw + self.vm_yaw;
                    let vm_pitch = cam_pitch + self.vm_pitch;
                    renderer.camera.position = glam::Vec3::from_array(p.pos);
                    renderer.camera.yaw = cam_yaw;
                    renderer.camera.pitch = cam_pitch;
                    renderer.fp_camera_drop = self.fp_cam_drop;
                    renderer.fp_camera_back = self.fp_cam_back;
                    // Markers: a box per bot (parked underground when dead)
                    // and one stretched box per projectile, so hitscan kills
                    // and projectile flight are visible.
                    let parked = park_matrix();
                    for (i, bot) in self.state.bots.iter().enumerate() {
                        let m = if bot.alive {
                            glam::Mat4::from_scale_rotation_translation(
                                glam::Vec3::new(0.8, 1.8, 0.8),
                                glam::Quat::IDENTITY,
                                glam::Vec3::new(bot.pos[0], bot.pos[1] + 0.9, bot.pos[2]),
                            )
                        } else {
                            parked
                        };
                        renderer.set_instance(self.bot_base + i, m);
                    }
                    for i in 0..self.proj_cap {
                        let m = match self.state.projectiles.get(i) {
                            Some(projectile) => {
                                let dir = glam::Vec3::from_array(ow_sim::normalize(projectile.vel));
                                let rot = glam::Quat::from_rotation_arc(glam::Vec3::Z, dir);
                                glam::Mat4::from_scale_rotation_translation(
                                    glam::Vec3::new(0.12, 0.12, 0.6),
                                    rot,
                                    glam::Vec3::from_array(projectile.pos),
                                )
                            }
                            None => parked,
                        };
                        renderer.set_instance(self.proj_base + i, m);
                    }
                    // The first-person model is the player's own body: the same
                    // transform the character would get, with the camera's yaw
                    // and nothing added, which is what the client's own
                    // (all-zero) firstPerson offsets say. Rebuilt from the live
                    // controller every frame.
                    if let Some(slot) = renderer.view_model_instance {
                        let eye = glam::Vec3::from_array(p.pos);
                        let matrix = scene::Scene::view_model_matrix(eye, vm_yaw, vm_pitch);
                        renderer.set_instance(slot, matrix);
                    }
                    // The animation: advance the clock and re-pose the rig. The
                    // palette is `[identity, bone 0, …]`, matching what the
                    // renderer uploaded at build time. Two pose sources ride
                    // `--anim`: the bone-list `PoseClip` (rotation-only) and
                    // the FP blend-card `BlendPose` (rotation + translation,
                    // paired by bone id through the model's skeleton).
                    if let (Some(anim), Some(skin)) = (&self.anim, &self.scene.skin) {
                        self.anim_time += dt;
                        // The weapon bones are **attachment hooks**, not
                        // animated limbs: the clip is authored on the
                        // character rig, and its records for these ids rotate
                        // the pistols inward (measured fwd44 −0.48 x, fwd74
                        // +0.31 x — the guns cross). Leaving their local at
                        // bind keeps the model's authored gun orientation and
                        // lets the posed hand parents carry them.
                        let weapon_bones: Vec<usize> = self
                            .scene
                            .weapon_attachments
                            .iter()
                            .map(|(_, bone, _)| *bone)
                            .collect();
                        let palette = match anim {
                            Anim::Bones(a) => {
                                let frame = a.frame_at(self.anim_time);
                                let mut posed = skin.palette(&|b| {
                                    if !self.weapon_fold && weapon_bones.contains(&b) {
                                        return None;
                                    }
                                    a.quat_at(b, frame)
                                        .map(|q| glam::Quat::from_xyzw(q[0], q[1], q[2], q[3]))
                                });
                                let mut palette = Vec::with_capacity(posed.len() + 1);
                                palette.push(glam::Mat4::IDENTITY);
                                palette.append(&mut posed);
                                palette
                            }
                            Anim::Blend { source, chains } => {
                                let mut bones: Vec<Option<(glam::Quat, glam::Vec3)>> =
                                    vec![None; skin.parents.len()];
                                for (bone, links) in chains.iter().enumerate() {
                                    if bone >= bones.len() {
                                        break;
                                    }
                                    if !self.weapon_fold && weapon_bones.contains(&bone) {
                                        continue;
                                    }
                                    if let Some(s) = crate::anim_tree::sample_chain(
                                        source,
                                        links,
                                        self.anim_time,
                                    ) {
                                        bones[bone] = Some(s);
                                    }
                                }
                                let mut palette = Vec::with_capacity(skin.parents.len() + 1);
                                palette.push(glam::Mat4::IDENTITY);
                                palette.extend(
                                    skin.palette_full(&|b| bones.get(b).copied().flatten()),
                                );
                                palette
                            }
                            Anim::Tree(tree) => {
                                let mut bones: Vec<Option<(glam::Quat, glam::Vec3)>> =
                                    tree.evaluate(self.anim_time, skin.parents.len());
                                if !self.weapon_fold {
                                    for bone in &weapon_bones {
                                        if *bone < bones.len() {
                                            bones[*bone] = None;
                                        }
                                    }
                                }
                                let mut palette = Vec::with_capacity(skin.parents.len() + 1);
                                palette.push(glam::Mat4::IDENTITY);
                                palette.extend(
                                    skin.palette_full(&|b| bones.get(b).copied().flatten()),
                                );
                                palette
                            }
                        };
                        renderer.set_joints(&palette);
                        // The guns are authored on the character rig. Their
                        // FP bones (44/74) have a different parent chain
                        // (char 284/285 vs vm 28/58), so the bind fold matches
                        // but the animated FP local crosses them. Pose the two
                        // weapon roots from the character c71 rig, then apply
                        // the hardpoint local and the camera placement.
                        let mut char_weapon_world: std::collections::HashMap<u32, glam::Mat4> =
                            std::collections::HashMap::new();
                        if let (Some(cskin), Some(cpose)) = (
                            &self.scene.character_skin,
                            &self.scene.character_weapon_pose,
                        ) {
                            let ids = &self.scene.character_ids;
                            if ids.len() == cskin.parents.len() {
                                let mut rec = std::collections::HashMap::new();
                                for r in 0..cpose.records() {
                                    if let Some(id) = cpose.bone_id(r) {
                                        rec.insert(id, r);
                                    }
                                }
                                let dur = cpose.frames() as f32 / cpose.fps().max(1.0);
                                let t = if dur > 0.0 { self.anim_time % dur } else { 0.0 };
                                let char_palette = cskin.palette_full(&|b| {
                                    let id = *ids.get(b)?;
                                    let r = *rec.get(&id)?;
                                    let (q, p) = cpose.sample_record(r, t).unwrap_or((None, None));
                                    let (_, bq, bt) =
                                        cskin.local[b].to_scale_rotation_translation();
                                    Some((q.unwrap_or(bq), p.unwrap_or(bt)))
                                });
                                let bind = cskin.bind_world();
                                for &(_, bone, local) in &self.scene.weapon_attachments {
                                    let Some(fp_id) = self.scene.view_model_ids.get(bone).copied()
                                    else {
                                        continue;
                                    };
                                    let Some(ci) = ids.iter().position(|id| *id == fp_id) else {
                                        continue;
                                    };
                                    if let Some(m) = char_palette.get(1 + ci) {
                                        char_weapon_world.insert(fp_id, *m * bind[ci] * local);
                                    }
                                }
                            }
                        }
                        // Tracer's dual pistols are rigid children of the two
                        // hand bones (ids 14 and 55). Each instance is the
                        // camera placement times the hand's posed world times
                        // its hardpoint frame; the palette entry is
                        // world * inverse-bind, so multiply the bind world
                        // back in.
                        if !self.scene.weapon_attachments.is_empty() {
                            let eye = glam::Vec3::from_array(p.pos);
                            let camera = scene::Scene::view_model_matrix(eye, vm_yaw, vm_pitch);
                            for &(weapon, bone, local) in &self.scene.weapon_attachments {
                                let fp_id = self.scene.view_model_ids.get(bone).copied();
                                let world = fp_id
                                    .and_then(|id| char_weapon_world.get(&id).copied())
                                    .or_else(|| {
                                        palette
                                            .get(1 + bone)
                                            .map(|m| *m * skin.bind_world()[bone] * local)
                                    });
                                if let Some(world) = world {
                                    renderer.set_instance(weapon, camera * world);
                                }
                            }
                        }
                    }
                    renderer.render();
                    if self.frame_no.is_multiple_of(10) {
                        // Exponential average: one frame's dt is one jittery
                        // sample, the average is the number worth comparing
                        // across settings changes.
                        self.fps = if self.fps > 0.0 {
                            self.fps * 0.9 + (1.0 / dt) * 0.1
                        } else {
                            1.0 / dt
                        };
                        // The same report in the title and in the corner: the
                        // overlay for the eye, the title for captures. The
                        // fire log rides on after `|` in both, on its own line.
                        let hud = self.state.hud();
                        let (state_line, log_line) = match hud.split_once('|') {
                            Some((s, l)) => (s.trim_end().to_string(), l.trim().to_string()),
                            None => (hud.clone(), String::new()),
                        };
                        let tris: u64 = renderer
                            .draws
                            .iter()
                            .map(|d| d.index_count as u64)
                            .sum::<u64>()
                            / 3;
                        let lines = vec![
                            format!(
                                "FPS {:.0}  DT {:.1}MS  DRAWS {}  TRIS {}  T {:.1}S",
                                self.fps,
                                dt * 1000.0,
                                renderer.draws.len(),
                                tris,
                                self.opened.elapsed().as_secs_f32()
                            ),
                            state_line,
                            log_line,
                        ];
                        for line in &lines {
                            if line.len() > crate::hud::COLS {
                                eprintln!("hud line clipped ({} chars): {line}", line.len());
                            }
                        }
                        renderer.set_hud(&lines);
                        renderer
                            .window
                            .set_title(&format!("play | {hud} | {}", self.state.weapon_note));
                    }
                    renderer.window.request_redraw();
                }
                _ => {}
            }
        }
    }

    impl PlayApp {
        fn pose(&self) -> crate::Camera {
            crate::Camera {
                position: glam::Vec3::from_array(self.state.controller.pos),
                yaw: self.state.controller.yaw,
                pitch: self.state.pitch,
                speed: 0.0,
            }
        }
    }

    let supplied = Supplied {
        damage: flag_f32(args, "--damage").unwrap_or(50.0),
        hero: flag_f32(args, "--hero").unwrap_or(3.0) as u32,
        bots: flag_f32(args, "--bots").unwrap_or(3.0) as usize,
    };
    let data = open_data()?;
    // This harness is first-person, so it asks for the hero's **view model**:
    // its own entity and its own look (see `docs/formats/animation.md`). A hero
    // whose first-person model is missing or unreadable simply draws nothing
    // there, and the build says so.
    let mut scene_args = args.to_vec();
    if !scene_args.iter().any(|a| a == "--view-model") {
        scene_args.push("--view-model".into());
    }
    // And at the player's own transform — the client's offsets from it are all
    // zero, so that is where the first-person model belongs (`Scene::
    // view_model_matrix`). Without it the static placement would be the
    // inspection framing, which is not what `--play` draws.
    if !scene_args.iter().any(|a| a == "--first-person") {
        scene_args.push("--first-person".into());
    }
    // The harness already uses a hero for its mover, pools and fire interval;
    // the scene needs the same index to place that hero's view model.
    if !scene_args.iter().any(|a| a == "--hero") {
        scene_args.push("--hero".into());
        scene_args.push(supplied.hero.to_string());
    }
    let mut scene = scene::build(&scene_args)?;
    let collider = scene::Collider::build(&scene);
    let state = PlayState::build(&data, &scene, &collider, &supplied)?;
    eprintln!("play: {}", state.weapon_note);
    // The marker batch: one box per bot, a stretched box per projectile, so
    // the sim's combat has something on screen (see `install_markers`).
    let (bot_base, proj_base, proj_cap) = install_markers(&mut scene, state.bots.len(), 64);

    // `--anim <hex guid|auto>`: decode a clip from the install and pose the
    // rig with it. Two layouts route by family: the bone-list families decode
    // a `PoseClip` (positional bone pairing — the count is reported so that a
    // wrong rig is visible rather than silent), and the FP blend-card family
    // (`0x0053XXXX`) decodes a `BlendPose` whose records pair by *bone id*
    // through the placed model's own skeleton (`Skeleton::ids`). `auto` =
    // `--play`'s own idle: the hero's blend-tree set's head clip, read from
    // the client's own data and printed with the route.
    //
    // A structural resolution failure (`auto` finds no blend set on this
    // hero's view model) warns and runs unposed rather than killing the
    // window; an explicit bad guid is still a hard error.
    let anim: Option<Anim> = (|| -> Result<Option<Anim>, String> {
        let Some(hex) = flag_str(args, "--anim") else {
            return Ok(None);
        };
        {
            if matches!(hex, "auto" | "AUTO") {
                if let Some(tree) = build_tree_anim(&data, &scene, args, supplied.hero) {
                    eprintln!("{}", tree.summary());
                    return Ok(Some(Anim::Tree(tree)));
                }
            }
            let guid = match hex.trim_start_matches("0x") {
                "auto" | "AUTO" => match resolve_idle_clip(&data, supplied.hero) {
                    Some(guid) => guid,
                    None => {
                        eprintln!(
                            "--anim: hero {}'s view model names no blend set head clip; running unposed",
                            supplied.hero
                        );
                        return Ok(None);
                    }
                },
                trimmed => u64::from_str_radix(trimmed, 16)
                    .map_err(|_| format!("--anim wants a hex guid, got {hex:?}"))?,
            };
            let bytes = data.bytes_of(guid).map_err(|e| format!("--anim: {e}"))?;
            let clip = ow_tank::clip::Clip::parse(&bytes)
                .map_err(|e| format!("--anim: not a clip: {e}"))?;
            let rig = scene.skin.as_ref().map(|s| s.parents.len()).unwrap_or(0);
            if let Some(source) = ow_tank::clip::BlendPose::decode(&bytes) {
                // the model's skeleton ids: the placed model's own bone list
                let skel_ids: Vec<u32> = scene
                    .hero_model_guid
                    .and_then(|m| data.bytes_of(m).ok())
                    .and_then(|mb| ow_tank::skeleton::parse_model(&mb).ok())
                    .map(|s| s.ids)
                    .unwrap_or_default();
                let pairs: Vec<Option<usize>> = (0..source.records())
                    .map(|r| {
                        source
                            .bone_id(r)
                            .and_then(|id| skel_ids.iter().position(|x| *x == id))
                    })
                    .collect();
                // Retarget: compose the character rig's dropped ancestors into
                // each FP bone (the bind identity is proven; see
                // docs/formats/animation.md 2026-10-20 (10)).
                let parts = ow_tank::entity::hero_parts(&data, supplied.hero).ok();
                let char_skel = parts
                    .as_ref()
                    .and_then(|p| data.bytes_of(p.model).ok())
                    .and_then(|b| ow_tank::skeleton::parse_model(&b).ok());
                let char_local = char_skel
                    .as_ref()
                    .map(|s| ow_tank::skin::Skin::from_skeleton(s).local)
                    .unwrap_or_default();
                let fp_skel = scene
                    .hero_model_guid
                    .and_then(|m| data.bytes_of(m).ok())
                    .and_then(|b| ow_tank::skeleton::parse_model(&b).ok());
                let chains = match (&fp_skel, &char_skel) {
                    (Some(fp), Some(ch)) => crate::anim_tree::build_chains(
                        &source,
                        &fp.ids,
                        &fp.parents,
                        &ch.ids,
                        &ch.parents,
                        &char_local,
                    ),
                    _ => Vec::new(),
                };
                // Diagnostic: the file rows whose P lane reads well over the
                // bone's parent length (the FP arms' two shoulder records,
                // 2.395x — `animation.md`). The pose now holds **every**
                // translation at bind, so this list only describes the file,
                // it does not drive. Checked at both the wrap start and
                // mid-clip so the count is stable.
                let held: Vec<bool> = (0..source.records())
                    .map(|r| {
                        let Some(bone) = pairs[r] else {
                            return false;
                        };
                        holds_unphysical(&scene, bone, source.sample_record(r, 0.0))
                            && holds_unphysical(
                                &scene,
                                bone,
                                source
                                    .sample_record(r, source.frames() as f32 * 0.5 / source.fps()),
                            )
                    })
                    .collect();
                let named = pairs.iter().filter(|b| b.is_some()).count();
                let held_n = held.iter().filter(|h| **h).count();
                if skel_ids.is_empty() {
                    eprintln!(
                        "--anim: no skeleton ids for the placed model; the blend pose cannot pair"
                    );
                }
                println!(
                    "anim {guid:#018x}: family {:#010x}, {} frames @ {} fps, {} records, {} named a skin bone of {rig}, {held_n} file rows over 1.5x bind (applied as the pose translation)",
                    clip.header.family,
                    source.frames(),
                    source.fps(),
                    source.records(),
                    named,
                );
                Ok(Some(Anim::Blend { source, chains }))
            } else if ow_tank::clip::is_blend_card_family(clip.header.family) {
                Err("--anim: the blend clip decoded no records".into())
            } else {
                let pose = ow_tank::clip::PoseClip::decode(
                    &clip.body,
                    clip.header.frames,
                    clip.header.fps as f32,
                );
                println!(
                    "anim {guid:#018x}: family {:#010x}, {} frames @ {} fps, {} tracks                  for a {rig}-bone rig{}",
                    clip.header.family,
                    clip.header.frames,
                    clip.header.fps,
                    pose.animated_bones(),
                    if pose.animated_bones() > rig {
                        "  <-- MORE TRACKS THAN BONES: wrong rig"
                    } else {
                        ""
                    }
                );
                Ok(Some(Anim::Bones(pose)))
            }
        }
    })()?;

    let event_loop = EventLoop::new().expect("an event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    let prev_cam_yaw = state.controller.yaw;
    let prev_cam_pitch = state.pitch;
    let mut app = PlayApp {
        state,
        scene,
        anim,
        anim_time: 0.0,
        grabbed: false,
        focused: true,
        sensitivity: flag_f32(args, "--sens").unwrap_or(0.0022),
        renderer: None,
        keys: std::collections::HashMap::new(),
        last_frame: None,
        fps: 0.0,
        vm_yaw: 0.0,
        vm_pitch: 0.0,
        vm_yaw_vel: 0.0,
        vm_pitch_vel: 0.0,
        vm_stiffness: flag_f32(args, "--vm-k").unwrap_or(64.0),
        vm_damping: flag_f32(args, "--vm-c").unwrap_or(16.0),
        vm_gain: flag_f32(args, "--vm-g").unwrap_or(-1.0),
        cam_yaw: prev_cam_yaw,
        cam_pitch: prev_cam_pitch,
        look_inverse: flag_f32(args, "--cam-look-inverse").unwrap_or(0.0),
        fp_cam_drop: flag_f32(args, "--fp-cam-drop").unwrap_or_else(|| {
            flag_f32(args, "--fp-eye")
                .map(|eye| scene::EYE_HEIGHT - eye)
                .unwrap_or(0.35)
        }),
        fp_cam_back: flag_f32(args, "--fp-cam-back").unwrap_or(0.30),
        fp_fov: flag_f32(args, "--fp-fov"),
        prev_cam_yaw,
        prev_cam_pitch,
        opened: std::time::Instant::now(),
        look: Vec2::ZERO,
        firing: false,
        frame_no: 0,
        bot_base,
        proj_base,
        proj_cap,
        weapon_fold: flag_f32(args, "--weapon-fold").unwrap_or(0.0) > 0.0,
    };
    event_loop.run_app(&mut app).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod drag_tests {
    use super::{vm_spring_step, wrap_pi};

    const K: f32 = 64.0;
    const C: f32 = 16.0;
    const GAIN: f32 = -1.0;
    const H: f32 = 1.0 / 240.0;

    #[test]
    fn wrap_pi_crosses_the_seam_without_a_full_turn() {
        let a = wrap_pi(3.0 - (-3.0));
        assert!((a - (6.0 - std::f32::consts::TAU)).abs() < 1e-5, "a={a}");
        assert!(a.abs() < std::f32::consts::PI);
    }

    /// A constant camera turn must settle at `gain·ω/k` and no further.
    /// The old absolute-angle follower settled at `c·ω/k` (72° at 5 rad/s).
    #[test]
    fn drive_settles_at_gain_times_rate_over_stiffness() {
        let (mut off, mut vel) = (0.0, 0.0);
        for _ in 0..(240 * 4) {
            vm_spring_step(&mut off, &mut vel, 5.0, K, C, GAIN, H);
        }
        let expected = GAIN * 5.0 / K;
        assert!(
            (off - expected).abs() < 1e-3,
            "off={off} expected={expected}"
        );
        assert!(vel.abs() < 1e-3, "vel={vel}");
    }

    /// With no drive the offset returns to zero and, at critical damping,
    /// does not swing past it.
    #[test]
    fn offset_returns_to_zero_without_overshoot() {
        let (mut off, mut vel) = (0.1, 0.0);
        let mut prev = off;
        for i in 0..(240 * 2) {
            vm_spring_step(&mut off, &mut vel, 0.0, K, C, 0.0, H);
            assert!(off <= prev + 1e-6, "overshoot at step {i}: {prev} -> {off}");
            prev = off;
        }
        assert!(off.abs() < 1e-3, "off={off}");
    }

    /// The substep cap is h = 0.1/8 = 12.5 ms; a 20 rad/s turn must stay
    /// bounded and finite there (a frame spike cannot blow the spring up).
    #[test]
    fn spring_is_stable_at_the_substep_cap() {
        let h = 0.1 / 8.0;
        let (mut off, mut vel) = (0.0, 0.0);
        for _ in 0..(8 * 60) {
            vm_spring_step(&mut off, &mut vel, 20.0, K, C, GAIN, h);
        }
        assert!(off.is_finite() && vel.is_finite(), "off={off} vel={vel}");
        assert!(off.abs() < 0.5, "off={off}");
    }
}

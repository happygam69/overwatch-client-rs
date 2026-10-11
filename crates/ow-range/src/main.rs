//! Practice range: test extracted heroes and weapons against targets.
//!
//! This joins the two halves of the project. `ow-tank` reads what the
//! installation actually says — health pools, fire rates, projectile speeds,
//! lifetimes, ranges — and `ow-sim` turns that into a fight.
//!
//! # Two honest limits, surfaced rather than hidden
//!
//! **Per-shot damage is not extracted.** It could not be found in any of the
//! seven places searched (`docs/formats/weapons.md` records them), so the range
//! asks for it: `damage <n>` sets it for the selected weapon's shot. Nothing here
//! invents a number and presents it as data.
//!
//! **A graph holds several parameter sets.** A weapon graph often declares more
//! than one firing mode — Widowmaker's declares two shot rates, one scoped and
//! one not — and nothing in the file says which is the primary fire. The range
//! therefore shows every distinct value it found and defaults to the fastest or
//! largest, which is a heuristic and labelled as one. `params` prints them all.

use std::collections::BTreeMap;
use std::io::{BufRead, Write};

use ow_sim::rules::{ArmourMitigation, DamageOrder, Rules};
use ow_sim::{Combatant, FireMode, PoolSet, Spread, Weapon, World};
use ow_tank::classes;
use ow_tank::keytable::Sources;
use ow_tank::{guid, GameData, ManifestSelector};

/// One weapon of a hero, with everything the install had to say about it.
#[derive(Debug, Clone, Default)]
struct WeaponFacts {
    slot: usize,
    graph: Option<u64>,
    /// Every distinct value found for each named parameter, because a graph can
    /// declare several firing modes and nothing says which is the primary one.
    params: BTreeMap<String, Vec<String>>,
}

impl WeaponFacts {
    /// The largest numeric value seen for a parameter.
    /// Shots per second, or `None` when only the sentinel was declared.
    fn declared_rate(&self) -> Option<f32> {
        let max = self.max_of("numShotsPerSecond")?;
        if max >= 999.0 {
            None
        } else {
            Some(max)
        }
    }

    fn max_of(&self, name: &str) -> Option<f32> {
        self.params
            .get(name)?
            .iter()
            .filter_map(|v| v.parse::<f32>().ok())
            .fold(None, |acc: Option<f32>, v| Some(acc.map_or(v, |a| a.max(v))))
    }

    /// Build a simulatable weapon. `damage` is supplied by the user, not read.
    fn to_weapon(&self, damage: f32, rate: Option<f32>) -> Weapon {
        let shots = self.declared_rate().or(rate).unwrap_or(1.0).clamp(0.05, 60.0);
        let pellets = self.max_of("numProjectilesPerShot").unwrap_or(1.0).max(1.0) as u32;
        let speed = self.max_of("projectileSpeed");
        let lifetime = self.max_of("projectileLifetime");
        let range = self.max_of("castLength").unwrap_or(40.0).max(5.0);

        Weapon {
            name: format!("slot {}", self.slot),
            // A shots-per-second of 999 is a sentinel meaning "as fast as input
            // allows", not a rate; clamp it to something simulatable.
            interval: 1.0 / shots,
            pellets,
            damage_per_projectile: damage,
            // A raycast in the graph is direct evidence of hitscan
            // behaviour and outranks a projectile speed, because one graph
            // may declare both: a hitscan rifle whose secondary is thrown.
            // Preferring the speed would make the rifle a projectile.
            mode: if self.params.contains_key("castLength") {
                FireMode::Hitscan { range }
            } else {
                match speed {
                    Some(speed) if speed > 0.0 => FireMode::Projectile { speed, gravity: 0.0 },
                    _ => FireMode::Hitscan { range },
                }
            },
            lifetime: lifetime.map(|l| l.clamp(0.05, 10.0)),
            spread_deg: 0.0,
            falloff: None,
            ammo: None,
            reload: None,
        }
    }

    /// One line describing what the graph declared.
    fn summary(&self) -> String {
        let mut parts = Vec::new();
        for (name, values) in &self.params {
            parts.push(format!("{name}={}", values.join("/")));
        }
        if parts.is_empty() {
            "no named parameters".to_string()
        } else {
            parts.join(" ")
        }
    }
}

/// A hero assembled from the installation.
#[derive(Debug, Clone, Default)]
struct HeroFacts {
    index: u32,
    guid: u64,
    pools: PoolSet,
    /// Provisional: see `ow_tank::weapons::MOVDER_SPEED_FIELD` documentation.
    move_speed: Option<f32>,
    weapons: Vec<WeaponFacts>,
}

fn assemble(data: &GameData, guid: u64) -> Option<HeroFacts> {
    let mut stu = data.stu_of(guid).ok()?;
    let hero = stu.instance(classes::HERO)?;
    let entity = stu.read_asset_ref(hero, classes::hero::GAMEPLAY_ENTITY)?;

    let entries = ow_tank::entity::schema_of(data, entity).ok()?;
    let get = |token: u32| entries.get(&token).and_then(|v| v.as_i32());
    let pools = PoolSet {
        health: get(ow_tank::entity::pool::HEALTH).unwrap_or(0) as f32,
        armour: get(ow_tank::entity::pool::ARMOUR).unwrap_or(0) as f32,
        shields: get(ow_tank::entity::pool::SHIELDS).unwrap_or(0) as f32,
    };

    let mut weapons = Vec::new();
    for weapon in ow_tank::weapons::weapons_of(data, entity).ok()? {
        if weapon.is_empty() {
            continue;
        }
        let mut facts = WeaponFacts {
            slot: weapon.slot,
            graph: weapon.graph,
            params: BTreeMap::new(),
        };
        if let Some(graph) = weapon.graph {
            if let Ok(params) = ow_tank::graph::params_for_graph(data, graph) {
                for param in params {
                    let Some(name) = param.field_name.clone() else {
                        continue;
                    };
                    let rendered = match param.value {
                        ow_tank::entity::Value::Int(v) => v.to_string(),
                        ow_tank::entity::Value::Float(v) => format!("{v}"),
                        ow_tank::entity::Value::Bool(v) => v.to_string(),
                        ow_tank::entity::Value::Unread(_) => continue,
                    };
                    let slot = facts.params.entry(name).or_default();
                    if !slot.contains(&rendered) {
                        slot.push(rendered);
                    }
                }
            }
        }
        weapons.push(facts);
    }

    let move_speed = ow_tank::weapons::move_speed(data, entity).ok().flatten();

    Some(HeroFacts {
        index: guid::index_of(guid),
        guid,
        pools,
        move_speed,
        weapons,
    })
}

struct Range {
    data: GameData,
    heroes: Vec<u64>,
    selected: Option<HeroFacts>,
    world: World,
    rules: Rules,
    spread: Spread,
    /// Damage supplied by the user for the selected weapon's shot.
    damage: f32,
    /// Shots per second, when the graph declares only the sentinel.
    ///
    /// A shots-per-second of 999 means "as fast as input allows", not a
    /// rate. Rather than invent one from it, the range asks.
    rate: Option<f32>,
    /// Which weapon slot to fire. A hero may carry more than one.
    slot: usize,
    shots_fired: u32,
    log: Vec<String>,
}

impl Range {
    fn new(data: GameData) -> Range {
        let mut heroes = data.guids_of_type(guid::types::HERO);
        heroes.sort_unstable();
        Range {
            data,
            heroes,
            selected: None,
            world: World::new(),
            rules: Rules::default(),
            spread: Spread::new(0x5EED),
            damage: 0.0,
            rate: None,
            slot: 0,
            shots_fired: 0,
            log: Vec::new(),
        }
    }

    fn select(&mut self, index: u32) -> Result<(), String> {
        let guid = *self
            .heroes
            .iter()
            .find(|g| guid::index_of(**g) == index)
            .ok_or_else(|| format!("no hero with index {index}"))?;
        let facts = assemble(&self.data, guid).ok_or_else(|| {
            format!("hero {index} has no readable gameplay entity (not downloaded?)")
        })?;
        println!(
            "hero {} (0x{:016x}): {:.0} health, {:.0} armour, {:.0} shields, {} weapon(s)",
            facts.index,
            facts.guid,
            facts.pools.health,
            facts.pools.armour,
            facts.pools.shields,
            facts.weapons.len()
        );
        match facts.move_speed {
            Some(speed) => println!(
                "  move speed {speed} m/s (provisional: see docs/formats/entity-schema.md)"
            ),
            None => println!("  move speed not declared"),
        }
        for weapon in &facts.weapons {
            println!("  slot {}: {}", weapon.slot, weapon.summary());
            if weapon.declared_rate().is_none() {
                println!(
                    "    note: no fixed rate declared (999 means as fast as input allows); \
                     defaulting to 1/s, set it with `rate <n>`"
                );
            }
        }
        self.selected = Some(facts);
        Ok(())
    }

    fn spawn_target(&mut self, health: f32, distance: f32) -> usize {
        let pools = PoolSet {
            health,
            armour: 0.0,
            shields: 0.0,
        };
        self.world
            .spawn(Combatant::new([0.0, 0.0, distance], pools))
    }

    /// The weapon the range is currently firing: the chosen slot if it holds a
    /// weapon, otherwise the first one that does.
    fn weapon(&self) -> Option<(Weapon, usize)> {
        let hero = self.selected.as_ref()?;
        let facts = hero
            .weapons
            .iter()
            .find(|w| w.slot == self.slot)
            .or_else(|| hero.weapons.first())?;
        Some((facts.to_weapon(self.damage, self.rate), facts.slot))
    }

    fn weapon_slots(&self) -> Vec<usize> {
        self.selected
            .as_ref()
            .map(|h| h.weapons.iter().map(|w| w.slot).collect())
            .unwrap_or_default()
    }

    /// Fire `count` shots at the nearest target and report.
    fn fire(&mut self, count: u32, spread_rng: &mut Spread) -> Result<(), String> {
        let (weapon, _slot) = self
            .weapon()
            .ok_or("select a hero with a weapon first")?;
        if self.damage <= 0.0 {
            return Err("per-shot damage is not extracted from the installation \
                        and no value is set; use `damage <n>`"
                .into());
        }
        if self.world.combatants.is_empty() {
            return Err("no targets; use `target add <health> <distance>`".into());
        }

        let origin = [0.0, 1.2, 0.0];
        let forward = [0.0, 0.0, 1.0];
        let mut total = 0.0;
        let mut hits = 0;
        let mut pellets = 0;
        let projectile = matches!(weapon.mode, FireMode::Projectile { .. });

        for _ in 0..count {
            let result = self.world.fire(&weapon, origin, forward, spread_rng, &self.rules);
            pellets += result.hits.len();
            hits += result.hits.iter().filter(|h| h.is_some()).count();
            total += result.damage_total;
            self.shots_fired += 1;
            if projectile {
                // Let the shots travel, one shot interval at a time.
                for _ in 0..40 {
                    let events = self.world.step(weapon.interval.min(0.05), &self.rules);
                    for event in &events {
                        total += event.dealt;
                        hits += 1;
                    }
                    if self.world.projectiles.is_empty() {
                        break;
                    }
                }
            }
        }

        // Anything still in the air is reported rather than silently dropped.
        if !self.world.projectiles.is_empty() {
            println!(
                "  note: {} projectile(s) still in flight",
                self.world.projectiles.len()
            );
        }

        let elapsed = count as f32 * weapon.interval;
        let line = format!(
            "{count} shot(s) in {elapsed:.2}s: {hits}/{pellets} pellets hit, \
             {total:.1} damage applied, {:.1} dps",
            if elapsed > 0.0 { total / elapsed } else { 0.0 }
        );
        println!("{line}");
        for (index, target) in self.world.combatants.iter().enumerate() {
            println!(
                "  target {index}: {:.0}/{:.0} hp{}",
                target.pools.total(),
                target.max_pools.total(),
                if target.alive { "" } else { "  [down]" }
            );
        }
        self.log.push(line);
        Ok(())
    }

    /// Time to kill the current target with the current weapon and damage.
    fn ttk(&mut self) -> Result<(), String> {
        let (weapon, _) = self
            .weapon()
            .ok_or("select a hero with a weapon first")?;
        if self.damage <= 0.0 {
            return Err("set `damage <n>` first".into());
        }
        let Some(target) = self.world.combatants.iter().find(|c| c.alive) else {
            return Err("no live target".into());
        };
        let per_shot = weapon.shot_damage() * weapon.falloff.map(|f| f.factor(0.0)).unwrap_or(1.0);
        if per_shot <= 0.0 {
            return Err("weapon deals no damage with the current values".into());
        }
        let shots = (target.max_pools.total() / per_shot).ceil();
        println!(
            "{:.0} hp at {:.1} damage per shot ({:.0} pellets x {:.1}) = {shots:.0} shots, \
             {:.2}s at {:.2} shots/s",
            target.max_pools.total(),
            per_shot,
            weapon.pellets,
            weapon.damage_per_projectile,
            shots * weapon.interval,
            weapon.shots_per_second()
        );
        Ok(())
    }

    /// Run one command. `Ok(false)` means quit.
    fn handle(&mut self, line: &str) -> Result<bool, String> {
        let mut parts = line.split_whitespace();
        let Some(command) = parts.next() else {
            return Ok(true);
        };
        let args: Vec<&str> = parts.collect();

        match command {
            "quit" | "exit" => return Ok(false),
            "help" => {
                print_help();
                Ok(true)
            }
            "heroes" => {
                let filter = args.first().map(|s| s.to_lowercase());
                let mut shown = 0;
                for g in &self.heroes {
                    let index = guid::index_of(*g);
                    if let Some(f) = &filter {
                        if !index.to_string().contains(f.as_str()) {
                            continue;
                        }
                    }
                    if shown < 40 {
                        println!("  hero {index}");
                    }
                    shown += 1;
                }
                println!("{shown} heroes");
                Ok(true)
            }
            "hero" => {
                let index: u32 = args
                    .first()
                    .ok_or("usage: hero <index>")?
                    .parse()
                    .map_err(|_| "hero index must be a number".to_string())?;
                self.select(index)?;
                Ok(true)
            }
            "params" => {
                let hero = self.selected.as_ref().ok_or("select a hero first")?;
                for weapon in &hero.weapons {
                    println!("slot {} graph {:?}", weapon.slot, weapon.graph);
                    for (name, values) in &weapon.params {
                        println!("   {name} = {}", values.join(" / "));
                    }
                }
                Ok(true)
            }
            "damage" => {
                let value: f32 = args
                    .first()
                    .ok_or("usage: damage <per-projectile>")?
                    .parse()
                    .map_err(|_| "damage must be a number".to_string())?;
                self.damage = value;
                println!(
                    "per-projectile damage set to {value}, supplied by you and not read from the install"
                );
                Ok(true)
            }
            "weapon" => {
                let slot: usize = args
                    .first()
                    .ok_or("usage: weapon <slot>")?
                    .parse()
                    .map_err(|_| "slot must be a number".to_string())?;
                let slots = self.weapon_slots();
                if !slots.contains(&slot) {
                    return Err(format!("hero has weapon slots {slots:?}"));
                }
                self.slot = slot;
                println!("firing weapon slot {slot}");
                Ok(true)
            }
            "rate" => {
                let value: f32 = args
                    .first()
                    .ok_or("usage: rate <shots per second>")?
                    .parse()
                    .map_err(|_| "rate must be a number".to_string())?;
                self.rate = Some(value);
                println!("rate set to {value} shots/s (supplied by you)");
                Ok(true)
            }
            "target" => {
                match args.first().copied() {
                    Some("add") => {
                        let health: f32 = args.get(1).unwrap_or(&"200").parse().unwrap_or(200.0);
                        let distance: f32 = args.get(2).unwrap_or(&"10").parse().unwrap_or(10.0);
                        let index = self.spawn_target(health, distance);
                        println!("target {index}: {health:.0} hp at {distance:.1} m");
                    }
                    Some("clear") => {
                        self.world.combatants.clear();
                        println!("targets cleared");
                    }
                    _ => {
                        for (index, c) in self.world.combatants.iter().enumerate() {
                            println!(
                                "  target {index}: {:.0}/{:.0} hp at {:.1} m{}",
                                c.pools.total(),
                                c.max_pools.total(),
                                c.pos[2],
                                if c.alive { "" } else { "  [down]" }
                            );
                        }
                    }
                }
                Ok(true)
            }
            "fire" => {
                let count: u32 = args.first().and_then(|s| s.parse().ok()).unwrap_or(1);
                let mut rng = self.spread.clone();
                let result = self.fire(count, &mut rng);
                self.spread = rng;
                result?;
                Ok(true)
            }
            "ttk" => {
                self.ttk()?;
                Ok(true)
            }
            "rules" => {
                println!(
                    "damage order: {:?}
armour mitigation: {:?}
\
                     These are assumptions, not extracted rules; see ow-sim/src/rules.rs.",
                    self.rules.damage_order, self.rules.armour_mitigation
                );
                Ok(true)
            }
            "order" => {
                self.rules.damage_order = match args.first().copied() {
                    Some("health") => DamageOrder::HealthArmourShields,
                    Some("shields") => DamageOrder::ShieldsArmourHealth,
                    _ => {
                        return Err("usage: order <health|shields>".to_string());
                    }
                };
                println!("damage order: {:?}", self.rules.damage_order);
                Ok(true)
            }
            "armour" => {
                self.rules.armour_mitigation = match args.first().copied() {
                    Some("none") => ArmourMitigation::None,
                    Some("flat") => ArmourMitigation::Flat(5.0),
                    Some("ow2") => Rules::default().armour_mitigation,
                    _ => {
                        return Err("usage: armour <none|flat|ow2>".to_string());
                    }
                };
                println!("armour mitigation: {:?}", self.rules.armour_mitigation);
                Ok(true)
            }
            other => Err(format!("unknown command {other:?}; try `help`")),
        }
    }
}

fn print_help() {
    println!(
        "commands:\n  \
         heroes [filter]        list hero indices\n  \
         hero <index>           select a hero and print what was extracted\n  \
         params                 every distinct named parameter the graph declared\n  \
         damage <n>             per-projectile damage (not extracted; you supply it)\n  \
         rate <n>               shots per second, when the graph declares only 999\n  \
         target add <hp> <m>    spawn a target\n  \
         target [clear]         show or clear targets\n  \
         fire [n]               fire n shots\n  \
         ttk                    shots and time to kill the nearest target\n  \
         rules | order <health|shields> | armour <none|flat|ow2>\n  \
         quit"
    );
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let sources = Sources::from_env();
    let install_root =
        std::env::var("OW_INSTALL").unwrap_or_else(|_| String::from("E:\\Overwatch"));
    println!("install: {install_root}");

    let data = match GameData::open(&install_root, &ManifestSelector::default(), &sources) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("error: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };

    let mut range = Range::new(data);
    println!(
        "{} heroes available. Type `help` for commands.",
        range.heroes.len()
    );

    // A script file runs the same commands without a terminal, which is how the
    // range is tested.
    if let Some(path) = args.first() {
        let Ok(text) = std::fs::read_to_string(path) else {
            eprintln!("error: cannot read script {path}");
            return std::process::ExitCode::FAILURE;
        };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            println!("> {line}");
            match range.handle(line) {
                Ok(true) => {}
                Ok(false) => break,
                Err(message) => println!("error: {message}"),
            }
        }
        return std::process::ExitCode::SUCCESS;
    }

    let stdin = std::io::stdin();
    loop {
        print!("> ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => match range.handle(line.trim()) {
                Ok(true) => {}
                Ok(false) => break,
                Err(message) => println!("error: {message}"),
            },
            Err(_) => break,
        }
    }
    std::process::ExitCode::SUCCESS
}

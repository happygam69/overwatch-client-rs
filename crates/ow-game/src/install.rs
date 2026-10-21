//! The install reads the sandbox needs: one hero's mover spec, health pools
//! and fire interval, and a stock of bots.
//!
//! These mirror `ow-view`'s play harness reads field for field (same STU
//! classes, same hash-named fields); they live here rather than in a shared
//! crate because the viewer's copy is a binary-internal module and the reads
//! are short. Per-shot damage stays an explicit input: `docs/formats/weapons.md`
//! records the search that failed to find it in the files.

use ow_sim::movement::MoverSpec;
use ow_sim::PoolSet;
use ow_tank::{guid, GameData};

/// Open the install the same way `ow_scene::build` does, so the sim and the
/// geometry read one bytestream.
pub fn open_data() -> Result<GameData, String> {
    let root = ow_casc::install::detect_root()
        .ok_or("no Overwatch install found; set OW_INSTALL to the folder containing .build.info")?;
    let sources = ow_tank::keytable::Sources::discover(&[root.clone()]);
    let install_root = root.display().to_string();
    GameData::open(
        &install_root,
        &ow_tank::ManifestSelector::default(),
        &sources,
    )
    .map_err(|e| e.to_string())
}

/// `(mover spec, pools, fire interval)` for one hero, by install index.
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

    // The full mover, same join as `ow-view --play`: base speed plus every
    // other float the component declares (back/side/crouch/terminal).
    let (speed, unlabelled) =
        ow_tank::entity::mover_fields_of(data, entity).map_err(|e| e.to_string())?;
    let mut spec = MoverSpec::from_base_speed(speed);
    for (hash, value) in unlabelled {
        spec = spec.with_unlabelled(hash, value);
    }

    let figs = ow_tank::volley::weapon_figs_of(data, entity);
    let interval = figs
        .volley
        .shots_per_second
        .map(|(rate, _)| 1.0 / rate)
        .or_else(|| weapon_interval_of(data, entity))
        .unwrap_or(0.5);
    Ok((spec, pools, interval))
}

/// A hero's sandbox weapon plus the one-line provenance note.
///
/// Same join as `ow-view --play` (`hero_weapon` there): pellets, cast-length
/// range vs projectile speed/lifetime/gravity from the volley figures; damage
/// stays supplied and the modify-health literals ride in the note as
/// candidates. Falls back to the sandbox default when the hero or entity is
/// unreadable.
pub fn hero_weapon_and_note(
    data: &GameData,
    hero_index: u32,
    damage: f32,
) -> (ow_sim::Weapon, String) {
    use ow_sim::{FireMode, Weapon};
    let entity = {
        let mut guids = data.guids_of_type(ow_tank::guid::types::HERO);
        guids.sort_unstable();
        guids
            .iter()
            .find(|g| ow_tank::guid::index_of(**g) == hero_index)
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
    // Mode by evidence order, same rule as `ow-view --play`'s `volley_mode`:
    // a named `projectileSpeed` literal outranks the `?`-assumed `castLength?`
    // (heroes 5/810 carry speeds 85/70 with gravity 9.8), then the cast
    // length as a hitscan range, then the 200 m default.
    let mode = if let Some(speed) = figs.volley.projectile_speed {
        FireMode::Projectile {
            speed,
            gravity: figs.gravity.unwrap_or(0.0),
        }
    } else if let Some(range) = figs.volley.cast_length {
        FireMode::Hitscan { range }
    } else {
        FireMode::Hitscan { range: 200.0 }
    };
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
        None => "weapon: sandbox defaults".into(),
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
    let per_hero = ow_tank::entity::loadouts_of(data, hero_index)
        .map(|v| v.len())
        .unwrap_or(0);
    let global = ow_tank::entity::all_loadout_guids(data).len();
    note.push_str(&format!(
        "; abilities: per-hero {per_hero} (global {global} loadouts); cooldown?/damage? stay ?"
    ));
    (weapon, note)
}

fn health_poolset_of(data: &GameData, entity: u64) -> Result<PoolSet, String> {
    let pools = ow_tank::entity::health_of(data, entity).map_err(|e| e.to_string())?;
    Ok(PoolSet {
        health: pools.health.unwrap_or(0) as f32,
        armour: pools.armour.unwrap_or(0) as f32,
        shields: pools.shields.unwrap_or(0) as f32,
    })
}

/// The shortest fire interval named across the hero's volley graphs
/// (`numShotsPerSecond` is a name-backed field; 0.5 s when absent).
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
            // 999 is not a rate: it means "as fast as input allows"
            // (docs/formats/weapons.md). Treating it as one fired 999
            // shots a second; the honest reading is that no rate was
            // declared and the caller's default stands.
            if rate >= 999.0 {
                continue;
            }
            best_rate = Some(best_rate.map_or(rate, |b| b.max(rate)));
        }
    }
    best_rate.map(|rate| 1.0 / rate)
}

/// Bots with real hero pools, by install index (names are not in the readable
/// data, so install indices label them).
pub fn stock_bots(data: &GameData, count: usize) -> Result<Vec<(PoolSet, String)>, String> {
    const PICK: [u32; 3] = [2, 3, 7];
    let mut out = Vec::new();
    for i in 0..count {
        let hero = PICK[i % PICK.len()];
        let (_spec, pools, _) = hero_spec_and_pools(data, hero)?;
        out.push((pools, format!("hero{hero}")));
    }
    Ok(out)
}

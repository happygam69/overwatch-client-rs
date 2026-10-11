//! Probe that reads gameplay data out of a local Overwatch installation.
//!
//! The point of this binary is to prove the data path works end to end against
//! a real install: container, manifest, registry, structured data.
//!
//! ```text
//! ow-probe info             what install was found, and its build
//! ow-probe types            asset counts per type
//! ow-probe heroes           every hero, with a resolved asset reference
//! ow-probe hero <index>     one hero in detail
//! ow-probe classmap         how many STU class names were recovered
//! ```

use std::process::ExitCode;

use ow_tank::classes;
use ow_tank::guid::{self, types};
use ow_tank::keytable::Sources;
use ow_tank::{GameData, ManifestSelector};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("info");

    match run(command, &args[1..]) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(command: &str, rest: &[String]) -> Result<(), String> {
    let sources = Sources::from_env();
    let selector = ManifestSelector::default();

    let install_root = std::env::var("OW_INSTALL").unwrap_or_else(|_| default_install());
    println!("install: {}", install_root);

    // `buckets` diagnoses the container alone, so it must not depend on the
    // manifest layer succeeding.
    if command == "buckets" {
        return buckets(&install_root);
    }

    let data = GameData::open(&install_root, &selector, &sources).map_err(|e| e.to_string())?;

    match command {
        "info" => info(&data),
        "types" => type_histogram(&data),
        "heroes" => heroes(&data),
        "hero" => {
            let index: u32 = rest
                .first()
                .ok_or("usage: ow-probe hero <index>")?
                .parse()
                .map_err(|_| "hero index must be a number".to_string())?;
            hero_detail(&data, index)
        }
        "classmap" => {
            println!("STU class names known: {}", data.classes.len());
            println!(
                "  {} -> {}",
                guid::types::label(guid::types::HERO).unwrap_or("?"),
                data.classes.describe(classes::HERO)
            );
            Ok(())
        }
        "settings" => settings(&data, rest),
        "health" => health(&data),
        "weapons" => weapons(&data),
        other => Err(format!(
            "unknown command {other:?}; try info, types, heroes, hero <index>, \
             health, weapons, settings [n], classmap, buckets"
        )),
    }
}

/// The per-hero tuning values.
///
/// These are the multipliers a player can move in a custom game, not base combat
/// statistics. They are read here because they exercise the whole path and reveal
/// the vocabulary of tunable quantities per hero.
fn settings(data: &GameData, rest: &[String]) -> Result<(), String> {
    let limit: usize = rest
        .first()
        .map(|s| s.parse().unwrap_or(25))
        .unwrap_or(25);

    let assets = ow_tank::settings::read_all(data).map_err(|e| e.to_string())?;
    if assets.is_empty() {
        return Err("no hero-settings asset was readable".into());
    }

    for (asset, floats) in &assets {
        println!(
            "settings asset 0x{asset:016x} (index {}) -- {} float settings",
            guid::index_of(*asset),
            floats.len()
        );

        let defaulted = floats.iter().filter(|f| f.bounds_are_defaulted()).count();
        println!(
            "  {defaulted} of {} leave min/max at the class default",
            floats.len()
        );
        let tuned = floats
            .iter()
            .filter(|f| (f.default - 1.0).abs() > 1e-6)
            .count();
        println!("  {tuned} have a default other than 1.0");
        println!();

        println!(
            "{:>5}  {:>9}  {:>9}  {:>9}  {:>8}  {:<18}  bounds",
            "inst", "default", "min", "max", "step", "identifier"
        );
        for f in floats.iter().take(limit) {
            let bounds = if f.bounds_are_defaulted() {
                "defaulted"
            } else if f.explicit[1] && f.explicit[2] {
                "authored"
            } else {
                "partial"
            };
            println!(
                "{:>5}  {:>9.4}  {:>9.4}  {:>9.4}  {:>8.4}  0x{:016x}  {bounds}",
                f.instance,
                f.default,
                f.min,
                f.max,
                f.step,
                f.identifier.unwrap_or(0)
            );
        }
        if floats.len() > limit {
            println!("... {} more", floats.len() - limit);
        }
    }
    Ok(())
}

/// Diagnostic: what the bucket index claims, before anything else can fail.
fn buckets(install_root: &str) -> Result<(), String> {
    let install = ow_casc::Install::open(install_root).map_err(|e| e.to_string())?;
    let store = ow_casc::Store::open(install).map_err(|e| e.to_string())?;
    let install = store.install();
    println!("index keys   {}", store.key_count());

    for key_name in ["encoding", "root"] {
        let Some(values) = install.build_config.get(key_name) else {
            println!("{key_name}: absent from build config");
            continue;
        };
        for (slot, value) in values.iter().enumerate() {
            if value.len() != 32 {
                continue;
            }
            let mut bytes = [0u8; 16];
            for (i, b) in bytes.iter_mut().enumerate() {
                *b = u8::from_str_radix(&value[i * 2..i * 2 + 2], 16).map_err(|e| e.to_string())?;
            }
            match store.location_of(&bytes) {
                Some(location) => {
                    let path = install
                        .data_dir()
                        .join(format!("data.{:03}", location.archive));
                    let file_len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    let fits = location.offset + u64::from(location.size) <= file_len;
                    println!(
                        "{key_name}[{slot}] {} -> archive {} offset 0x{:x} size {} \
                         (file {} bytes, fits={fits})",
                        &value[..16],
                        location.archive,
                        location.offset,
                        location.size,
                        file_len
                    );
                }
                None => println!("{key_name}[{slot}] {} -> not in the bucket index", &value[..16]),
            }
        }
    }
    Ok(())
}

fn default_install() -> String {
    for candidate in [
        r"E:\Overwatch",
        r"C:\Program Files (x86)\Overwatch",
        r"C:\Program Files\Overwatch",
        r"D:\Overwatch",
    ] {
        if std::path::Path::new(candidate).join(".build.info").is_file() {
            return candidate.to_string();
        }
    }
    String::new()
}

fn info(data: &GameData) -> Result<(), String> {
    let install = data.store().install();
    println!("version      {}", install.build_info.version);
    println!(
        "branch       {} (product {})",
        install.build_info.branch, install.build_info.product
    );
    let build_name = install
        .build_config
        .get("build-name")
        .and_then(|v| v.first())
        .cloned()
        .unwrap_or_default();
    println!("build name   {build_name}");
    let _ = data.registry().header.build_version;
    println!("index keys   {}", data.store().key_count());
    println!("keyring keys {}", install.keyring.len());
    println!(
        "registry     {} records from {} manifest(s)",
        data.registry().records.len(),
        data.manifests.len()
    );
    for name in &data.manifests {
        println!("  {name}");
    }
    Ok(())
}

fn type_histogram(data: &GameData) -> Result<(), String> {
    let histogram = data.type_histogram();
    println!("{} distinct asset types", histogram.len());
    let mut rows: Vec<(u16, usize)> = histogram.iter().map(|(k, v)| (*k, *v)).collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1));

    println!("\ntop 12:");
    for (type_id, count) in rows.iter().take(12) {
        println!("  type 0x{type_id:03X}  {count:>8}");
    }

    println!("\ncombat-relevant:");
    for type_id in [
        types::TEXTURE,
        types::MODEL,
        types::STU_GENERIC_SETTINGS,
        types::HERO,
        types::LOADOUT,
        types::GAME_MODE,
        types::TALENT,
        types::LOOTBOX,
    ] {
        let count = histogram.get(&type_id).copied().unwrap_or(0);
        let label = types::label(type_id).unwrap_or("unknown");
        println!("  type 0x{type_id:03X}  {count:>8}  {label}");
    }
    Ok(())
}

/// Hero GUIDs sit on the fixed `0x02E00000` prefix; index 2 is the first.
fn hero_guids(data: &GameData) -> Vec<u64> {
    let mut guids = data.guids_of_type(types::HERO);
    guids.sort_unstable();
    guids
}

fn heroes(data: &GameData) -> Result<(), String> {
    let guids = hero_guids(data);
    println!("{} heroes", guids.len());
    println!(
        "{:>6}  {:<18}  {:<18}  {:<8}  colour",
        "index", "guid", "gameplay entity", "entity#"
    );

    let mut resolved = 0usize;
    let mut unreadable: Vec<(u32, String)> = Vec::new();
    for guid in &guids {
        // One asset that cannot be opened must not hide the rest: a few are
        // encrypted with keys the installation's own keyring does not carry.
        let mut stu = match data.stu_of(*guid) {
            Ok(stu) => stu,
            Err(e) => {
                unreadable.push((guid::index_of(*guid), e.to_string()));
                continue;
            }
        };
        let Some(hero) = stu.instance(classes::HERO) else {
            continue;
        };
        let entity = stu.read_asset_ref(hero, classes::hero::GAMEPLAY_ENTITY);
        let colour = stu.read_rgba(hero, classes::hero::HERO_COLOR);
        let in_registry = entity
            .map(|e| data.registry().get(e).is_some())
            .unwrap_or(false);
        if in_registry {
            resolved += 1;
        }
        println!(
            "{:>6}  0x{:016x}  0x{:016x}  {:<8}  {}",
            guid::index_of(*guid),
            guid,
            entity.unwrap_or(0),
            entity.map(guid::index_of).unwrap_or(0),
            colour
                .map(|c| format!("({:.3}, {:.3}, {:.3}, {:.3})", c[0], c[1], c[2], c[3]))
                .unwrap_or_else(|| "-".into())
        );
    }

    println!(
        "\n{resolved} of {} hero references resolve to a registered asset",
        guids.len()
    );
    if !unreadable.is_empty() {
        println!(
            "{} hero asset(s) could not be opened (set OW_SUPPORT_KEYRING to a \
             community keyring file to read them):",
            unreadable.len()
        );
        for (index, why) in &unreadable {
            println!("  index {index}: {why}");
        }
    }
    Ok(())
}

fn hero_detail(data: &GameData, index: u32) -> Result<(), String> {
    let guid = hero_guids(data)
        .into_iter()
        .find(|g| guid::index_of(*g) == index)
        .ok_or_else(|| format!("no hero with index {index}"))?;

    let bytes = data.bytes_of(guid).map_err(|e| e.to_string())?;
    let mut stu = data.stu_of(guid).map_err(|e| e.to_string())?;

    println!("hero         0x{guid:016x} (index {index})");
    println!("stored size  {} bytes", bytes.len());
    println!("instances    {}", stu.instances.len());
    for (i, info) in stu.instances.iter().enumerate() {
        println!(
            "  [{i}] {} size={} embedded_by={}",
            data.classes.describe(info.hash),
            info.size,
            info.embedder_index
        );
    }

    let Some(hero) = stu.instance(classes::HERO) else {
        return Err("no STUHero instance in this asset".into());
    };

    let fields = stu.fields(hero).to_vec();
    println!("\nSTUHero fields ({}):", fields.len());
    for field in fields.iter().take(60) {
        println!(
            "  0x{:08X} offset={:>5} size={:>5}",
            field.hash, field.offset, field.size
        );
    }

    println!();
    if let Some(entity) = stu.read_asset_ref(hero, classes::hero::GAMEPLAY_ENTITY) {
        let kind = guid::type_of(entity);
        let label = types::label(kind).unwrap_or("unmapped type");
        let present = data.registry().get(entity).is_some();
        println!("gameplay entity 0x{entity:016x} (type 0x{kind:03X} {label}, registry={present})");
    }
    if let Some(colour) = stu.read_rgba(hero, classes::hero::HERO_COLOR) {
        println!(
            "colour          ({:.4}, {:.4}, {:.4}, {:.4})",
            colour[0], colour[1], colour[2], colour[3]
        );
    }
    if let Some(size) = stu.read_u32(hero, classes::hero::HERO_SIZE) {
        println!("hero size enum  {size}");
    }
    if let Some(gender) = stu.read_u32(hero, classes::hero::GENDER) {
        println!("gender enum     {gender}");
    }
    Ok(())
}

/// Per-hero health pools, read from each hero's gameplay entity schema.
///
/// This is the first base combat data: not a tuning multiplier but the pool the
/// hero actually spawns with. Armour and shields are absent for heroes that have
/// none, which is a real zero rather than a failed reading.
fn health(data: &GameData) -> Result<(), String> {
    use ow_tank::entity::{pool, HealthPools};

    println!(
        "{:>6}  {:>7}  {:>7}  {:>8}  {:>7}",
        "index", "health", "armour", "shields", "total"
    );

    let mut read = 0usize;
    let mut missing = 0usize;
    let mut totals: Vec<i32> = Vec::new();

    for guid in hero_guids(data) {
        let entity = {
            let mut stu = match data.stu_of(guid) {
                Ok(stu) => stu,
                Err(_) => {
                    missing += 1;
                    continue;
                }
            };
            let Some(hero) = stu.instance(classes::HERO) else {
                missing += 1;
                continue;
            };
            match stu.read_asset_ref(hero, classes::hero::GAMEPLAY_ENTITY) {
                Some(entity) => entity,
                None => {
                    missing += 1;
                    continue;
                }
            }
        };

        let entries = match ow_tank::entity::schema_of(data, entity) {
            Ok(entries) => entries,
            Err(_) => {
                missing += 1;
                continue;
            }
        };
        let show = |token: u32| {
            entries
                .get(&token)
                .and_then(|v| v.as_i32())
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".to_string())
        };
        let pools = HealthPools {
            health: entries.get(&pool::HEALTH).and_then(|v| v.as_i32()),
            armour: entries.get(&pool::ARMOUR).and_then(|v| v.as_i32()),
            shields: entries.get(&pool::SHIELDS).and_then(|v| v.as_i32()),
        };
        if let Some(total) = pools.total() {
            totals.push(total);
        }
        read += 1;

        println!(
            "{:>6}  {:>7}  {:>7}  {:>8}  {:>7}",
            guid::index_of(guid),
            show(pool::HEALTH),
            show(pool::ARMOUR),
            show(pool::SHIELDS),
            pools
                .total()
                .map(|t| t.to_string())
                .unwrap_or_else(|| "-".into())
        );
    }

    println!("\n{read} heroes read, {missing} not readable");
    if !totals.is_empty() {
        totals.sort_unstable();
        println!(
            "totals min {} max {} median {}",
            totals.first().unwrap(),
            totals.last().unwrap(),
            totals[totals.len() / 2]
        );
    }
    Ok(())
}

/// Each hero's weapons, and the state-script graph each one uses.
///
/// The graph is where a weapon's numbers live, but a graph is a program rather
/// than a record, so this prints the graph reference and checks it resolves.
/// Reading damage and fire rate out of the graph is a separate problem.
fn weapons(data: &GameData) -> Result<(), String> {
    println!(
        "{:>6}  {:>5}  {:<18}  {:>7}  {}",
        "index", "slot", "graph", "type", "present"
    );

    let mut heroes_with_weapons = 0usize;
    let mut graphs = 0usize;
    let mut present = 0usize;
    let mut empty_slots = 0usize;
    let mut unreadable = 0usize;

    for guid in hero_guids(data) {
        let entity = {
            let mut stu = match data.stu_of(guid) {
                Ok(stu) => stu,
                Err(_) => {
                    unreadable += 1;
                    continue;
                }
            };
            let Some(hero) = stu.instance(classes::HERO) else {
                unreadable += 1;
                continue;
            };
            match stu.read_asset_ref(hero, classes::hero::GAMEPLAY_ENTITY) {
                Some(entity) => entity,
                None => {
                    unreadable += 1;
                    continue;
                }
            }
        };

        let weapons = match ow_tank::weapons::weapons_of(data, entity) {
            Ok(weapons) => weapons,
            Err(_) => {
                unreadable += 1;
                continue;
            }
        };
        if weapons.is_empty() {
            continue;
        }
        heroes_with_weapons += 1;

        for weapon in &weapons {
            if weapon.is_empty() {
                empty_slots += 1;
                continue;
            }
            if let Some(graph) = weapon.graph {
                graphs += 1;
                let ok = ow_tank::weapons::graph_is_present(data, graph);
                if ok {
                    present += 1;
                }
                println!(
                    "{:>6}  {:>5}  0x{graph:016x}  {:>7}  {ok}",
                    guid::index_of(guid),
                    weapon.slot,
                    format!("0x{:03X}", guid::type_of(graph))
                );
            }
        }
    }

    println!(
        "\n{heroes_with_weapons} heroes with weapons, {graphs} graph references \
         ({present} present), {empty_slots} empty slots, {unreadable} heroes unreadable"
    );
    Ok(())
}

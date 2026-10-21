//! Dump the parameters of statescript graphs, by hero or by asset.
//!
//! A graph's embedded instances carry literals whose owning field names them
//! (see [`ow_tank::graph`]); this tool is the CLI for that join. It exists for
//! hunts like the damage search: `ModifyHealth.m_amount` is the field hash
//! `0xAF872E86`, so
//!
//! ```text
//! ow-tank-params --hero 16 --field 0xAF872E86 --all-graphs
//! ```
//!
//! walks every graph asset of the graph type and prints every `m_amount`
//! literal it finds, with its owning state. Requires the same environment as
//! the rest of the tree (`OW_INSTALL`, optional `OW_OWLIB` for names).
//!
//! Scope notes: `--all-graphs` reads *every* graph asset of type 0x01B, which
//! is thousands of assets and takes a minute or two; it never writes anything.

use std::collections::BTreeMap;

use ow_tank::graph::all_params_of;
use ow_tank::{guid, GameData, ManifestSelector};

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    if args.iter().any(|a| a == "--help" || a == "-h") || args.is_empty() {
        eprintln!(
            "usage: ow-tank-params [--hero N] [--graph 0xGUID] [--field 0xHASH] [--all-graphs]\n\
             \n\
             Without --field, every named parameter of the selected graphs is printed.\n\
             With --field, only that field hash (named or not) is printed."
        );
        return std::process::ExitCode::from(2);
    }

    let root = match ow_casc::install::detect_root() {
        Some(r) => r,
        None => {
            eprintln!("no Overwatch install found; set OW_INSTALL");
            return std::process::ExitCode::from(2);
        }
    };
    let sources = ow_tank::keytable::Sources::discover(&[root.clone()]);
    let data = match GameData::open(
        &root.display().to_string(),
        &ManifestSelector::default(),
        &sources,
    ) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("open failed: {e}");
            return std::process::ExitCode::from(2);
        }
    };

    let field: Option<u32> =
        flag("--field").and_then(|v| u32::from_str_radix(v.trim_start_matches("0x"), 16).ok());

    let mut graphs: Vec<u64> = Vec::new();
    if let Some(g) = flag("--graph") {
        match u64::from_str_radix(g.trim_start_matches("0x"), 16) {
            Ok(guid) => graphs.push(guid),
            Err(_) => {
                eprintln!("--graph wants a hex GUID");
                return std::process::ExitCode::from(2);
            }
        }
    } else if let Some(hero) = flag("--hero").and_then(|v| v.parse::<u32>().ok()) {
        let mut heroes = data.guids_of_type(guid::types::HERO);
        heroes.sort_unstable();
        let Some(hg) = heroes.into_iter().find(|g| guid::index_of(*g) == hero) else {
            eprintln!("hero index {hero} not found");
            return std::process::ExitCode::from(1);
        };
        let mut stu = match data.stu_of(hg) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("hero {hero}: {e}");
                return std::process::ExitCode::from(1);
            }
        };
        let entity = stu
            .instance(ow_tank::classes::HERO)
            .and_then(|h| stu.read_asset_ref(h, ow_tank::classes::hero::GAMEPLAY_ENTITY));
        drop(stu);
        let Some(entity) = entity else {
            eprintln!("hero {hero} has no gameplay entity");
            return std::process::ExitCode::from(1);
        };
        match ow_tank::weapons::weapons_of(&data, entity) {
            Ok(weapons) => {
                for w in &weapons {
                    if let Some(g) = w.graph {
                        graphs.push(g);
                    }
                }
                eprintln!("hero {hero}: {} weapon graph(s)", graphs.len());
            }
            Err(e) => eprintln!("hero {hero}: weapons: {e}"),
        }
        // The weapon component's manager script is the graph that runs the
        // weapon behaviour; the per-weapon `m_script` fields point at more.
        if args.iter().any(|a| a == "--manager") {
            let mut entity_stu = match data.stu_of(entity) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("hero {hero}: entity: {e}");
                    return std::process::ExitCode::from(1);
                }
            };
            let mut found = 0;
            if let Some(&wc) = entity_stu
                .instances_of(ow_tank::classes::WEAPON_COMPONENT)
                .first()
            {
                if let Some(g) = entity_stu
                    .read_asset_ref(wc, ow_tank::classes::weapon_component::MANAGER_SCRIPT)
                {
                    eprintln!("hero {hero}: weapon manager script 0x{g:016x}");
                    graphs.push(g);
                    found += 1;
                }
            }
            if let Ok(weapons) = ow_tank::weapons::weapons_of(&data, entity) {
                for w in &weapons {
                    if let Some(script) = w.script {
                        graphs.push(script);
                        found += 1;
                    }
                }
            }
            eprintln!("hero {hero}: {found} manager/script graph(s) added");
        }
        if args.iter().any(|a| a == "--all-graphs") {
            let all = data.guids_of_type(0x01B);
            eprintln!("--all-graphs: {} graph asset(s) of type 0x01B", all.len());
            graphs.extend(all);
        }
    } else {
        eprintln!("select --hero N, --graph 0xGUID, or --all-graphs");
        return std::process::ExitCode::from(2);
    }

    // Deduplicate, keep order stable for comparable runs.
    graphs.sort_unstable();
    graphs.dedup();

    let mut hits = 0usize;
    let mut by_field: BTreeMap<u32, usize> = BTreeMap::new();
    for (n, graph) in graphs.iter().enumerate() {
        let Ok(stu) = data.stu_of(*graph) else {
            continue;
        };
        for param in all_params_of(&stu, &data.classes) {
            *by_field.entry(param.field_hash).or_default() += 1;
            if let Some(want) = field {
                if param.field_hash != want {
                    continue;
                }
            }
            let name = data
                .fields
                .get(param.state_hash, param.field_hash)
                .map(|f| f.short_name().to_string())
                .unwrap_or_else(|| format!("0x{:08X}", param.field_hash));
            println!(
                "graph 0x{graph:016x}  state 0x{:08X} {}  .{} = {:?}",
                param.state_hash,
                param.state_name.as_deref().unwrap_or("(unnamed)"),
                name,
                param.value
            );
            hits += 1;
        }
        if n % 500 == 0 && n > 0 {
            eprintln!("  {n}/{} graphs, {hits} hit(s)", graphs.len());
        }
    }
    eprintln!(
        "{hits} parameter row(s) printed across {} graph(s)",
        graphs.len()
    );
    if field.is_none() {
        // Without a field filter, summarise where the literals live.
        eprintln!("field hashes seen (top 20 by count):");
        let mut rows: Vec<(u32, usize)> = by_field.into_iter().collect();
        rows.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
        for (hash, count) in rows.into_iter().take(20) {
            eprintln!("  0x{hash:08X} {count:6}");
        }
    }
    std::process::ExitCode::SUCCESS
}

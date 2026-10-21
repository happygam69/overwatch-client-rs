//! Walk asset references transitively and report the statescript graphs a
//! hero (or any asset) reaches, with the damage nodes inside them.
//!
//! The join this tool exists for: a hero's asset references its gameplay
//! entity, its loadout/ability assets, and so on; the *graphs* those abilities
//! run are several hops out, and the ability graph's fields are unnamed in the
//! reference types. Following the references is the only name-free way to
//! attribute a graph to a hero.
//!
//! ```text
//! ow-tank-refwalk --hero 16 --depth 5
//! ow-tank-refwalk --from 0x0580000000002020 --depth 2 --field 0xAF872E86
//! ```
//!
//! For every reachable graph asset (type 0x01B) it prints the graph and, when
//! `--field` is given (default: `STUStatescriptModifyHealth.m_amount`,
//! `0xAF872E86`), every parameter with that field hash inside it.
//!
//! Read-only: opens the install the same way the rest of the tree does and
//! never writes.

use std::collections::{HashSet, VecDeque};

use ow_tank::{guid, GameData, ManifestSelector};

/// `STUStatescriptModifyHealth.m_amount`.
const M_AMOUNT: u32 = 0xAF87_2E86;

struct Edge {
    from: u64,
    field: u32,
    to: u64,
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    if args.is_empty() {
        eprintln!("usage: ow-tank-refwalk (--hero N | --from 0xGUID) [--depth D] [--field 0xHASH]");
        return std::process::ExitCode::from(2);
    }
    let depth: usize = flag("--depth").and_then(|v| v.parse().ok()).unwrap_or(4);
    let field: u32 = flag("--field")
        .and_then(|v| u32::from_str_radix(v.trim_start_matches("0x"), 16).ok())
        .unwrap_or(M_AMOUNT);

    let root = match ow_casc::install::detect_root() {
        Some(r) => r,
        None => {
            eprintln!("no Overwatch install found; set OW_INSTALL");
            return std::process::ExitCode::from(2);
        }
    };
    let sources = ow_tank::keytable::Sources::discover(std::slice::from_ref(&root));
    let data = match GameData::open(
        root.display().to_string(),
        &ManifestSelector::default(),
        &sources,
    ) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("open failed: {e}");
            return std::process::ExitCode::from(2);
        }
    };

    // Starting asset(s): --from, or the hero asset *and* its gameplay entity
    // via --hero (the loadout list lives on the hero asset; the components on
    // the entity).
    let mut roots: Vec<u64> = Vec::new();
    let start: u64 = if let Some(g) = flag("--from") {
        match u64::from_str_radix(g.trim_start_matches("0x"), 16) {
            Ok(v) => v,
            Err(_) => {
                eprintln!("--from wants a hex GUID");
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
        eprintln!("hero {hero}: hero asset 0x{hg:016x}, gameplay entity 0x{entity:016x}");
        roots.push(hg);
        entity
    } else {
        eprintln!("select --hero N or --from 0xGUID");
        return std::process::ExitCode::from(2);
    };
    roots.push(start);

    // BFS over asset refs. Every ref field of every instance of every reached
    // asset is followed, bounded by depth and a visited set.
    let mut visited: HashSet<u64> = HashSet::new();
    let mut queue: VecDeque<(u64, usize)> = VecDeque::new();
    let mut edges: Vec<Edge> = Vec::new();
    let mut graphs: Vec<u64> = Vec::new();
    for r in &roots {
        if visited.insert(*r) {
            queue.push_back((*r, 0));
        }
    }

    while let Some((asset, d)) = queue.pop_front() {
        if d >= depth {
            continue;
        }
        let Ok(mut stu) = data.stu_of(asset) else {
            continue;
        };
        if guid::type_of(asset) == ow_tank::classes::STATESCRIPT_GRAPH_ASSET_TYPE {
            graphs.push(asset);
            continue;
        }
        let instance_count = stu.instances.len();
        for inst in 0..instance_count {
            // Collect the field hashes for this instance first; `fields`
            // borrows the reader mutably.
            let hashes: Vec<u32> = stu.fields(inst).iter().map(|f| f.hash).collect();
            for hash in hashes {
                // Single reference.
                if let Some(target) = stu.read_asset_ref(inst, hash) {
                    if target != 0 && visited.insert(target) {
                        edges.push(Edge {
                            from: asset,
                            field: hash,
                            to: target,
                        });
                        queue.push_back((target, d + 1));
                    }
                }
                // Inline array of references: an i32 count, then that many
                // 16-byte refs, each deobfuscated with the owning field hash.
                // `read_asset_ref` does not see these, and the hero's loadout
                // list -- the hop to the ability graphs -- is one of them.
                if let Some((offset, _size)) = stu.field_bytes(inst, hash) {
                    let base = stu.payload_start(inst) + offset;
                    if let Some(count) = stu.i32_at(base) {
                        if (1..=256).contains(&count) {
                            for k in 0..count as usize {
                                let at = base + 4 + k * 16;
                                if let Some(target) = stu.asset_ref_at(at, hash) {
                                    if target != 0 && visited.insert(target) {
                                        edges.push(Edge {
                                            from: asset,
                                            field: hash,
                                            to: target,
                                        });
                                        queue.push_back((target, d + 1));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    println!(
        "{} asset(s) reached, {} graph(s)",
        visited.len(),
        graphs.len()
    );
    for g in &graphs {
        println!("graph 0x{g:016x}");
    }
    if graphs.is_empty() {
        return std::process::ExitCode::SUCCESS;
    }

    // Damage nodes inside the graphs reached.
    let mut hits = 0usize;
    for g in &graphs {
        let Ok(stu) = data.stu_of(*g) else { continue };
        for param in ow_tank::graph::all_params_of(&stu, &data.classes) {
            if param.field_hash != field {
                continue;
            }
            println!(
                "  graph 0x{g:016x}  state 0x{:08X} {}  .0x{field:08X} = {:?}",
                param.state_hash,
                param.state_name.as_deref().unwrap_or("(unnamed)"),
                param.value
            );
            hits += 1;
        }
    }
    println!("{hits} hit(s) for field 0x{field:08X}");
    if args.iter().any(|a| a == "--edges") {
        for e in &edges {
            println!(
                "  edge 0x{:016x} -0x{:08X}-> 0x{:016x}",
                e.from, e.field, e.to
            );
        }
    }
    std::process::ExitCode::SUCCESS
}

//! Associate assets the way the reference does, but at **component** granularity.
//!
//! **Why component level.** The reference (`DataTool/FindLogic/Combo.cs`) files an
//! animation under whatever model/entity the walk is *inside*, and its own code
//! logs `"Animation with no model or entity. will be lost"` when there is no such
//! context. On the installed build that is exactly what happens: Tracer's clips
//! are referenced by a **component** (`0x7c3457dc`), not nested under a model, so
//! a traversal-context walk attributes nothing. Measured with the previous
//! version of this tool: 5 animations reached, **0** attributed to a model.
//!
//! So the association here is the one the data actually carries: for every
//! **instance** (a component) of every reached asset, which assets does it
//! reference, at which **field hash**, and of which type. A component is the
//! unit that owns things in this format, so that is the unit we group by.
//!
//! Usage: `ow-tank-combo --hero 3 | --from 0xGUID [--depth N]`.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use ow_tank::{guid, GameData, ManifestSelector};

#[allow(dead_code)]
const T_ENTITY: u16 = 0x003;
const T_ANIMATION: u16 = 0x006;
const T_MODEL: u16 = 0x00c;
const T_HERO: u16 = 0x075;

fn type_name(t: u16) -> &'static str {
    match t {
        0x003 => "entity",
        0x004 => "texture",
        0x006 => "animation",
        0x008 => "material",
        0x00c => "model",
        0x01a => "look",
        0x075 => "hero",
        _ => "?",
    }
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let depth: usize = flag("--depth").and_then(|v| v.parse().ok()).unwrap_or(4);

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

    let mut roots: Vec<u64> = Vec::new();
    if let Some(hex) = flag("--from") {
        match u64::from_str_radix(hex.trim_start_matches("0x"), 16) {
            Ok(g) => roots.push(g),
            Err(_) => {
                eprintln!("--from wants a hex guid");
                return std::process::ExitCode::from(2);
            }
        }
    } else if let Some(h) = flag("--hero").and_then(|v| v.parse::<usize>().ok()) {
        let heroes = data.guids_of_type(T_HERO);
        let Some(&hg) = heroes.get(h) else {
            eprintln!("no hero {h}");
            return std::process::ExitCode::from(1);
        };
        roots.push(hg);
        if let Ok(mut stu) = data.stu_of(hg) {
            // The hero asset names its gameplay entity; walk from both, because
            // the loadout list and the components live in different places.
            if let Some(e) = stu.read_asset_ref(0, ow_tank::classes::hero::GAMEPLAY_ENTITY) {
                roots.push(e);
            }
        }
        println!("hero {h}: asset {hg:#018x}");
    } else {
        eprintln!("select --hero N or --from 0xGUID");
        return std::process::ExitCode::from(2);
    }

    // BFS, recording every component-level edge.
    struct Edge {
        from: u64,
        component: u32,
        field: u32,
        to: u64,
    }
    let mut seen: HashSet<u64> = HashSet::new();
    let mut queue: VecDeque<(u64, usize)> = VecDeque::new();
    let mut edges: Vec<Edge> = Vec::new();
    let mut reached: BTreeSet<u64> = BTreeSet::new();

    for r in &roots {
        if seen.insert(*r) {
            queue.push_back((*r, 0));
        }
    }
    while let Some((asset, d)) = queue.pop_front() {
        reached.insert(asset);
        if d >= depth {
            continue;
        }
        let Ok(mut stu) = data.stu_of(asset) else {
            continue;
        };
        let instances = stu.instances.len();
        for inst in 0..instances {
            let component = stu.instances[inst].hash;
            let hashes: Vec<u32> = stu.fields(inst).iter().map(|f| f.hash).collect();
            for field in hashes {
                if let Some(to) = stu.read_asset_ref(inst, field) {
                    if to == 0 {
                        continue;
                    }
                    edges.push(Edge {
                        from: asset,
                        component,
                        field,
                        to,
                    });
                    if seen.insert(to) {
                        queue.push_back((to, d + 1));
                    }
                }
            }
        }
        let _ = instances;
    }

    // Group by (owning asset, component type): what does each component hold?
    let mut by_component: BTreeMap<(u64, u32), BTreeMap<u16, BTreeMap<u64, Vec<u32>>>> =
        BTreeMap::new();
    for e in &edges {
        let t = guid::type_of(e.to);
        by_component
            .entry((e.from, e.component))
            .or_default()
            .entry(t)
            .or_default()
            .entry(e.to)
            .or_default()
            .push(e.field);
    }

    println!(
        "{} assets reached, {} component references",
        reached.len(),
        edges.len()
    );
    for ((owner, component), types) in &by_component {
        let ot = guid::type_of(*owner);
        // Only report components that own something interesting.
        if !types.contains_key(&T_MODEL) && !types.contains_key(&T_ANIMATION) {
            continue;
        }
        println!(
            "\ncomponent {component:#010x} on {owner:#018x} ({}):",
            type_name(ot)
        );
        for (t, targets) in types {
            let mut names: Vec<String> = targets
                .iter()
                .map(|(g, fields)| {
                    let extra = if *t == T_MODEL {
                        data.bytes_of(*g)
                            .ok()
                            .and_then(|b| ow_tank::skeleton::parse_model(&b).ok())
                            .map(|s| format!(" [{} bones]", s.bones_abs))
                            .unwrap_or_default()
                    } else {
                        String::new()
                    };
                    format!("{g:#018x}{extra} via {fields:?}")
                })
                .collect();
            names.sort();
            println!("   {} ({}):", type_name(*t), names.len());
            for n in names.iter().take(8) {
                println!("      {n}");
            }
            if names.len() > 8 {
                println!("      … {} more", names.len() - 8);
            }
        }
    }

    // The join the harness wants: which animations sit on the same component as
    // which model, with the rig's bone count so a mismatch is visible.
    let mut rig_anims: HashMap<u64, HashSet<u64>> = HashMap::new();
    for ((_owner, _component), types) in &by_component {
        let Some(models) = types.get(&T_MODEL) else {
            continue;
        };
        let Some(anims) = types.get(&T_ANIMATION) else {
            continue;
        };
        for m in models.keys() {
            for a in anims.keys() {
                rig_anims.entry(*m).or_default().insert(*a);
            }
        }
    }
    println!("\n--- model -> animations on the same component ---");
    for (model, anims) in &rig_anims {
        let bones = data
            .bytes_of(*model)
            .ok()
            .and_then(|b| ow_tank::skeleton::parse_model(&b).ok())
            .map(|s| s.bones_abs)
            .unwrap_or(0);
        println!(
            "model {model:#018x} ({bones} bones): {} animation(s)",
            anims.len()
        );
        for a in anims {
            println!("    {a:#018x}");
        }
    }
    std::process::ExitCode::SUCCESS
}

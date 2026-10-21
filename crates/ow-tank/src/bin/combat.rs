//! Dump a per-hero combat table: pools, weapon graphs, every volley literal.
//!
//! One row per fact, seven TSV columns, stdout only — redirect to a file to
//! keep it (`D:/ow-dumps` is where extracted data lives, never the repo):
//!
//! ```text
//! cargo run -p ow-tank --bin combat -- > D:/ow-dumps/combat/hero_weapons.tsv
//! cargo run -p ow-tank --bin combat -- --hero 3
//! ```
//!
//! Columns: `hero  slot  graph  node  field  name  value`.
//!
//! * `node=pools` — the hero's declared health/armour/shields, one row each.
//! * `node=volley#<i>` — one literal of the volley state at instance `i`;
//!   `name` is the reference type's name where it has one, else the hash.
//! * `field` a volley field, `name=-> 0x…(inst i)` a non-literal child
//!   (arrays, embedded instances), reported so the wiring is visible. A child
//!   that is a `STUConfigVarDynamic` also carries `dynamic@<id>`; the two known
//!   computed config-var classes are annotated `expression` / `computed`.
//! * `node=volley#<i>/impact#<j>` — the children of a volley's `m_D2D11CE9`
//!   impact descriptor (`0x4AA6CA44`), so each volley joins to the
//!   `STUStatescriptModifyHealth` instances it fires (`health#<k>` rows).
//! * `node=motion#<i>` — one literal of a projectile-motion element.
//! * `node=health#<i>` — one literal of a modify-health data node in the
//!   graph, `amount` among them when declared.
//!
//! What it is for: the per-hero firing figures (rates, pellet counts, cast
//! lengths, projectile speeds) and the raw material to decide what the
//! unnamed volley fields mean (`docs/formats/weapons.md`). `--hero N` limits
//! to one hero; the default walks every hero that has a weapon graph.

use ow_tank::entity::Value;
use ow_tank::graph::literal_of;
use ow_tank::{classes, guid, volley, GameData, ManifestSelector};

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    if args.iter().any(|a| a == "--help" || a == "-h") {
        eprintln!(
            "usage: ow-tank-combat [--hero N]\n\
             \n\
             Prints a TSV table of every hero's pools, weapon graphs and the\n\
             literals of their volley states. Reads the install; writes nothing."
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

    let only: Option<u32> = flag("--hero").and_then(|v| v.parse().ok());

    let mut heroes = data.guids_of_type(guid::types::HERO);
    heroes.sort_unstable();

    println!("# hero\tslot\tgraph\tnode\tfield\tname\tvalue");

    let mut rows = 0usize;
    for hero_guid in heroes {
        let index = guid::index_of(hero_guid);
        if let Some(only) = only {
            if index != only {
                continue;
            }
        }

        let mut stu = match data.stu_of(hero_guid) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let entity = stu
            .instance(ow_tank::classes::HERO)
            .and_then(|h| stu.read_asset_ref(h, ow_tank::classes::hero::GAMEPLAY_ENTITY));
        drop(stu);
        let Some(entity) = entity else { continue };

        // Pools: rows of their own, so a hero with no readable weapon still
        // appears in the table.
        if let Ok(pools) = ow_tank::entity::health_of(&data, entity) {
            for (pool, value) in [
                ("hp", pools.health),
                ("armour", pools.armour),
                ("shields", pools.shields),
            ] {
                let value = value.map_or_else(|| "none".to_string(), |v| v.to_string());
                row(index, "-", "-", "pools", "", pool, &value);
                rows += 1;
            }
        }

        let Ok(weapons) = ow_tank::weapons::weapons_of(&data, entity) else {
            continue;
        };
        for weapon in &weapons {
            let Some(graph) = weapon.graph else { continue };
            let Ok(graph_stu) = data.stu_of(graph) else {
                continue;
            };
            let slot = weapon.slot.to_string();
            let graph_s = format!("0x{graph:016x}");

            for v in volley::volleys_of(&graph_stu) {
                let node = format!("volley#{}", v.state);
                for (field, value) in &v.literals {
                    let name = data
                        .fields
                        .get(ow_tank::classes::VOLLEY_STATE, *field)
                        .map(|f| f.short_name().to_string())
                        .or_else(|| volley::volley_field_name(*field).map(str::to_string))
                        .unwrap_or_else(|| format!("0x{field:08X}"));
                    row(
                        index,
                        &slot,
                        &graph_s,
                        &node,
                        &format!("{field:08X}"),
                        &name,
                        &fmt_value(*value),
                    );
                    rows += 1;
                }
                for (field, child_hash, child) in &v.children {
                    let dynamic = volley::dynamic_identifier_of(&graph_stu, *child)
                        .map(|id| format!(" dynamic@{id}"))
                        .unwrap_or_default();
                    let note = volley::child_note(*child_hash)
                        .map(|n| format!(" {n}"))
                        .unwrap_or_default();
                    row(
                        index,
                        &slot,
                        &graph_s,
                        &node,
                        &format!("{field:08X}"),
                        &format!("-> 0x{child_hash:08X} (inst {child}){dynamic}{note}"),
                        "",
                    );
                    rows += 1;
                    // The impact descriptor is where a firing volley meets its
                    // damage nodes; emit its children so the join to the
                    // graph's health# rows is possible.
                    if *field == classes::volley::IMPACT {
                        let inode = format!("volley#{}/impact#{child}", v.state);
                        for (inner_field, inner_value) in
                            volley::literals_of_children(&graph_stu, *child)
                        {
                            let name = impact_field_name(&data, inner_field);
                            row(
                                index,
                                &slot,
                                &graph_s,
                                &inode,
                                &format!("{inner_field:08X}"),
                                &name,
                                &fmt_value(inner_value),
                            );
                            rows += 1;
                        }
                        for (inner_field, inner_hash, inner) in
                            volley::children_with_class(&graph_stu, *child)
                        {
                            if literal_of(&graph_stu, inner, inner_hash).is_some() {
                                continue; // already reported as a literal above
                            }
                            let dynamic = volley::dynamic_identifier_of(&graph_stu, inner)
                                .map(|id| format!(" dynamic@{id}"))
                                .unwrap_or_default();
                            let note = volley::child_note(inner_hash)
                                .map(|n| format!(" {n}"))
                                .unwrap_or_default();
                            let name = impact_field_name(&data, inner_field);
                            row(
                                index,
                                &slot,
                                &graph_s,
                                &inode,
                                &format!("{inner_field:08X}"),
                                &format!(
                                    "{name} -> 0x{inner_hash:08X} (inst {inner}){dynamic}{note}"
                                ),
                                "",
                            );
                            rows += 1;
                        }
                    }
                }
                for m in &v.motions {
                    for (field, value) in &m.literals {
                        row(
                            index,
                            &slot,
                            &graph_s,
                            &format!("motion#{}", m.instance),
                            &format!("{field:08X}"),
                            volley::motion_field_name(*field),
                            &fmt_value(*value),
                        );
                        rows += 1;
                    }
                }
            }
            for (node, literals) in volley::modify_health_of(&graph_stu) {
                for (field, value) in &literals {
                    let name = data
                        .fields
                        .get(ow_tank::classes::MODIFY_HEALTH, *field)
                        .map(|f| f.short_name().to_string())
                        .unwrap_or_else(|| format!("0x{field:08X}"));
                    row(
                        index,
                        &slot,
                        &graph_s,
                        &format!("health#{node}"),
                        &format!("{field:08X}"),
                        &name,
                        &fmt_value(*value),
                    );
                    rows += 1;
                }
            }
        }
    }
    eprintln!("{rows} row(s)");
    std::process::ExitCode::SUCCESS
}

#[allow(clippy::too_many_arguments)]
fn row(hero: u32, slot: &str, graph: &str, node: &str, field: &str, name: &str, value: &str) {
    println!("{hero}\t{slot}\t{graph}\t{node}\t{field}\t{name}\t{value}");
}

fn fmt_value(value: Value) -> String {
    match value {
        Value::Float(v) => format!("f {v}"),
        Value::Int(v) => format!("i {v}"),
        Value::Bool(v) => format!("b {v}"),
        Value::Unread(h) => format!("? 0x{h:08X}"),
    }
}

/// Name for an impact-descriptor field, from the reference types where they
/// name it (`m_modifyHealth` among them), else the bare hash.
fn impact_field_name(data: &GameData, field: u32) -> String {
    data.fields
        .get(classes::VOLLEY_IMPACT, field)
        .map(|f| f.short_name().to_string())
        .unwrap_or_else(|| format!("0x{field:08X}"))
}

//! The practice range's hero application, read from the mode's own scripts.
//!
//! The mode does not hardcode a hero and does not draw the hero from its own
//! `m_availableHeroes`: each team's controller graph carries one
//! `STUStatescriptStateHero` node whose `hero` input is a
//! `STUConfigVarDynamic` with identifier **940**. Both teams use the same
//! identifier, and the body graph carries no such node.
//!
//! Every hero-setting node in the whole install takes its hero from that one
//! runtime variable; exactly one mode writes a hero into its graph instead
//! ("Assault", hero `0x02E0000000000009`). What binds variable 940 to a value
//! (the client's runtime variable table) is **open**; that is why these tests
//! pin the identifier rather than a hero.
//!
//! Skipped when no install is present. Read-only.
//!
//! ```text
//! cargo test --release -p ow-tank --test hero_pick_real -- --nocapture
//! ```

use std::collections::BTreeSet;

use ow_tank::classreg::ClassRegistry as TankRegistry;
use ow_tank::graph::{hero_states_of, HeroInput};
use ow_tank::hero_select::mode_team_scripts;
use ow_tank::keytable::Sources;
use ow_tank::{GameData, ManifestSelector};

const PRACTICE_MODE: u64 = 0x0230_0000_0000_0018;
/// The two controller graphs and the shared body graph of the practice mode.
const CONTROLLER_TEAM_0: u64 = 0x0580_0000_0000_13C1;
const CONTROLLER_TEAM_1: u64 = 0x0580_0000_0000_0C90;
const BODY: u64 = 0x0580_0000_0000_0C8E;
/// `STUConfigVarEntityID`, the class of the `controller` input.
const CONFIG_VAR_ENTITY_ID: u32 = 0xB5A0_CAF0;
/// The runtime variable the hero comes from, in both teams' controllers.
const HERO_VAR_ID: u32 = 940;

fn open() -> Option<(GameData, TankRegistry)> {
    let root = ow_casc::install::detect_root()?;
    let sources = Sources::discover(std::slice::from_ref(&root));
    let data = match GameData::open(&root, &ManifestSelector::default(), &sources) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("note: install not readable here ({e}); nothing to assert");
            return None;
        }
    };
    let registry = TankRegistry::load_from_sources(&sources);
    if registry.is_empty() {
        eprintln!("note: no STU class checkout here; nothing to assert");
        return None;
    }
    Some((data, registry))
}

#[test]
fn the_practice_mode_teams_run_the_two_controller_graphs_and_one_body_graph() {
    let Some((data, registry)) = open() else {
        return;
    };

    let scripts = mode_team_scripts(&data, PRACTICE_MODE, registry.inner())
        .expect("the mode's team scripts read");
    assert_eq!(scripts.len(), 2);
    assert_eq!(scripts[0].controller, Some(CONTROLLER_TEAM_0));
    assert_eq!(scripts[1].controller, Some(CONTROLLER_TEAM_1));
    assert_eq!(
        scripts[0].body,
        Some(BODY),
        "both teams share one body graph"
    );
    assert_eq!(scripts[1].body, Some(BODY));
}

#[test]
fn each_controller_takes_the_hero_from_runtime_variable_940() {
    let Some((data, _registry)) = open() else {
        return;
    };

    for graph in [CONTROLLER_TEAM_0, CONTROLLER_TEAM_1] {
        let stu = data.stu_of(graph).expect("the controller graph opens");
        let states = hero_states_of(&stu);
        assert_eq!(states.len(), 1, "0x{graph:016x}: one hero node");
        let state = states[0];
        assert_eq!(
            state.hero,
            HeroInput::Dynamic(HERO_VAR_ID),
            "0x{graph:016x}: the hero is runtime variable {HERO_VAR_ID}"
        );
        assert_eq!(
            state.controller,
            Some(CONFIG_VAR_ENTITY_ID),
            "0x{graph:016x}: the controller input is an entity id"
        );
    }
}

#[test]
fn the_body_graph_sets_no_hero() {
    let Some((data, _registry)) = open() else {
        return;
    };

    let stu = data.stu_of(BODY).expect("the body graph opens");
    assert!(
        hero_states_of(&stu).is_empty(),
        "the body graph carries no hero-setting node"
    );
}

#[test]
fn every_mode_hero_node_reads_the_same_runtime_variable() {
    let Some((data, registry)) = open() else {
        return;
    };

    // Survey the whole install: each mode's team controller graphs are walked
    // once, and every hero-setting node's input is classified. The numbers are
    // pins, not thresholds - a build that changes them must change this test.
    let mut modes = 0usize;
    let mut graphs: BTreeSet<u64> = BTreeSet::new();
    let mut modes_with_a_hero = 0usize;
    let mut dynamic = 0usize;
    let mut literal: Vec<(u64, u64)> = Vec::new();
    let mut other: Vec<(u64, u32)> = Vec::new();

    for guid in data.guids_of_type(0x0C5) {
        let Ok(scripts) = mode_team_scripts(&data, guid, registry.inner()) else {
            continue;
        };
        modes += 1;
        let mut any = false;
        for team in scripts {
            let Some(controller) = team.controller else {
                continue;
            };
            if !graphs.insert(controller) {
                continue;
            }
            let Ok(stu) = data.stu_of(controller) else {
                continue;
            };
            for state in hero_states_of(&stu) {
                any = true;
                match state.hero {
                    HeroInput::Dynamic(940) => dynamic += 1,
                    HeroInput::Dynamic(id) => other.push((guid, id)),
                    HeroInput::Literal(hero) => literal.push((guid, hero)),
                    HeroInput::Other(hash) => other.push((guid, hash)),
                    HeroInput::Missing => other.push((guid, 0)),
                }
            }
        }
        if any {
            modes_with_a_hero += 1;
        }
    }

    assert_eq!(modes, 137, "type-0x0C5 mode assets");
    assert_eq!(graphs.len(), 35, "distinct team controller graphs");
    assert_eq!(modes_with_a_hero, 13, "modes whose script sets a hero");
    assert_eq!(dynamic, 12, "nodes reading runtime variable 940");
    assert!(other.is_empty(), "unclassified hero inputs: {other:?}");
    assert_eq!(
        literal,
        vec![(0x0230_0000_0000_016B, 0x02E0_0000_0000_0009)],
        "the one mode that writes its hero into the graph (Assault)"
    );
}

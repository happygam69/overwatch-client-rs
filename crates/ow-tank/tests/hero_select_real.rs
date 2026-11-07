//! The hero selector's roster, read from the install's own mode data.
//!
//! A mode team's `m_availableHeroes` is one of three answers: absent, the empty
//! marker (`STU_D1DC7C55`) meaning every hero, or a whitelist
//! (`STU_3C14F504` / `STU_C45DE560`) naming the heroes. The fixtures below are
//! the three answers measured on this install.
//!
//! Skipped when no install is present. Read-only.
//!
//! ```text
//! cargo test --release -p ow-tank --test hero_select_real -- --nocapture
//! ```

use ow_tank::classreg::ClassRegistry as TankRegistry;
use ow_tank::entity::mover_row_of;
use ow_tank::guid::{type_of, types};
use ow_tank::hero_select::{
    hero_entity, hero_name, mode_team_pools, resolve_hero, selectable_heroes, HeroPool,
};
use ow_tank::keytable::Sources;
use ow_tank::{GameData, ManifestSelector};

/// "Practice Range" (`0x0230000000000018`), the mode the range's header names.
const PRACTICE_MODE: u64 = 0x0230_0000_0000_0018;
/// "Escort" (`0x023000000000011e`): both teams restrict heroes to the same 29.
const WHITELIST_MODE: u64 = 0x0230_0000_0000_011E;
/// "Stadium" (`0x0230000000000143`): one restricted team, one unrestricted.
const MIXED_MODE: u64 = 0x0230_0000_0000_0143;
/// "Mei's Snowball Offensive" (`0x0230000000000008`): exactly one hero, Mei.
const ONE_HERO_MODE: u64 = 0x0230_0000_0000_0008;

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

fn pools(
    data: &GameData,
    registry: &TankRegistry,
    mode: u64,
) -> Vec<ow_tank::hero_select::TeamPool> {
    mode_team_pools(data, mode, registry.inner()).expect("the mode's teams read")
}

#[test]
fn the_practice_range_offers_every_hero() {
    let Some((data, registry)) = open() else {
        return;
    };

    let teams = pools(&data, &registry, PRACTICE_MODE);
    assert_eq!(teams.len(), 2, "the practice mode declares two teams");
    // Team 0 carries no `m_team` field (the class default); team 1 is TeamRed.
    // Neither restricts heroes: both carry the empty marker.
    assert_eq!(teams[0].team, None);
    assert_eq!(teams[1].team, Some(1));
    assert_eq!(teams[0].pool, HeroPool::All, "team 0: no restriction");
    assert_eq!(teams[1].pool, HeroPool::All, "team 1: no restriction");
}

#[test]
fn a_restricted_mode_names_its_heroes() {
    let Some((data, registry)) = open() else {
        return;
    };

    let teams = pools(&data, &registry, WHITELIST_MODE);
    assert_eq!(teams.len(), 2);
    let HeroPool::Whitelist(left) = &teams[0].pool else {
        panic!("team 0 is restricted, got {:?}", teams[0].pool);
    };
    let HeroPool::Whitelist(right) = &teams[1].pool else {
        panic!("team 1 is restricted, got {:?}", teams[1].pool);
    };
    assert_eq!(left.len(), 29, "the whitelist's length");
    assert_eq!(left, right, "both teams name the same heroes");
    assert_eq!(
        left[0], 0x02E0_0000_0000_0003,
        "the first hero, in file order"
    );
    for hero in left {
        assert_eq!(type_of(*hero), types::HERO, "every entry is a hero asset");
    }
}

#[test]
fn an_unrestricted_team_sits_next_to_a_restricted_one() {
    let Some((data, registry)) = open() else {
        return;
    };

    let teams = pools(&data, &registry, MIXED_MODE);
    assert_eq!(teams.len(), 2);
    let HeroPool::Whitelist(heroes) = &teams[0].pool else {
        panic!("team 0 is restricted, got {:?}", teams[0].pool);
    };
    assert_eq!(heroes.len(), 33);
    assert_eq!(teams[1].pool, HeroPool::All, "team 1 keeps every hero");
}

#[test]
fn a_one_hero_mode_allows_exactly_that_hero() {
    let Some((data, registry)) = open() else {
        return;
    };

    let teams = pools(&data, &registry, ONE_HERO_MODE);
    assert_eq!(teams.len(), 2);
    for team in &teams {
        assert_eq!(
            team.pool,
            HeroPool::Whitelist(vec![0x02E0_0000_0000_00DD]),
            "one hero, twice (the mode carries two pool objects with the same list)"
        );
    }
}

/// The practice mode is open, so its roster is every hero asset. The playable
/// marker is set on exactly the heroes that name themselves, and every playable
/// hero carries the mover row a walk reads. An open roster also contains NPC
/// heroes (no marker, no name), which a picker must not offer.
#[test]
fn the_practice_range_names_every_playable_hero() {
    let Some((data, registry)) = open() else {
        return;
    };

    let roster = selectable_heroes(&data, PRACTICE_MODE, registry.inner())
        .expect("the practice roster reads");
    let every_hero = data.guids_of_type(types::HERO);
    assert_eq!(
        roster.len(),
        every_hero.len(),
        "an open mode offers every type-0x075 asset"
    );
    let playable: Vec<_> = roster.iter().filter(|h| h.playable).cloned().collect();
    assert!(
        playable.len() >= 40,
        "the playable roster is the whole hero list ({} of {} assets)",
        playable.len(),
        roster.len()
    );
    assert!(
        playable.len() < roster.len(),
        "the open roster carries NPC heroes too; this install must show both cases"
    );

    let mut indexes = std::collections::BTreeSet::new();
    let mut names = std::collections::BTreeSet::new();
    let mut fast = Vec::new();
    for hero in &roster {
        assert_eq!(type_of(hero.guid), types::HERO, "0x{:016x}", hero.guid);
        assert!(
            indexes.insert(hero.index),
            "hero {} appears twice",
            hero.index
        );
        // Every playable hero names itself. The converse is false: an NPC hero
        // can carry a name too (the range's own `Training Bot`), which is why
        // the marker, not the name, decides what a picker offers.
        if !hero.playable {
            continue;
        }
        let name = hero
            .name
            .as_deref()
            .unwrap_or_else(|| panic!("playable hero {} carries no name", hero.index));
        assert!(
            !name.trim().is_empty(),
            "hero {} has an empty name",
            hero.index
        );
        assert!(
            names.insert(name.to_ascii_lowercase()),
            "{name:?} appears twice"
        );

        // The mover row is what the walk will apply. Every playable hero must
        // have its forward speed; the other rows are carried when present.
        let entity = hero_entity(&data, hero.guid)
            .unwrap_or_else(|| panic!("hero {} names no gameplay entity", hero.index));
        let row = mover_row_of(&data, entity)
            .expect("the hero entity reads")
            .unwrap_or_else(|| panic!("hero {} has no mover component", hero.index));
        let forward = row
            .forward
            .unwrap_or_else(|| panic!("hero {} ({name}) declares no forward speed", hero.index));
        assert!(
            forward.is_finite() && forward > 0.0,
            "{name}: forward {forward}"
        );
        if forward > 5.5 {
            fast.push(hero.index);
        }
        if let Some(terminal) = row.fall_terminal {
            assert!(terminal > 0.0, "{name}: terminal fall {terminal}");
        }
    }

    // The install's three 6.0 m/s heroes, in asset-index order. Every other
    // playable hero reads the 5.5 common forward speed.
    assert_eq!(fast, vec![3, 41, 1245], "the 6.0 m/s heroes");

    // Two anchors read in `docs/formats/animation.md` and the mode tests above:
    // hero 3 is Tracer, and Mei is the one-hero mode's 0xDD.
    let tracer = roster.iter().find(|h| h.index == 3).expect("hero 3");
    assert!(tracer.playable);
    assert_eq!(tracer.name.as_deref(), Some("Tracer"));
    assert_eq!(
        mover_row_of(&data, hero_entity(&data, tracer.guid).unwrap())
            .unwrap()
            .unwrap()
            .forward,
        Some(6.0),
        "Tracer is one of the 6.0 m/s heroes"
    );
    let mei = roster.iter().find(|h| h.index == 0xDD).expect("hero 0xDD");
    assert!(mei.playable);
    assert_eq!(mei.name.as_deref(), Some("Mei"));
    // A named NPC hero: the practice range's own bot is in the open roster and
    // must not be offered by a player picker.
    let bot = roster.iter().find(|h| h.index == 363).expect("hero 363");
    assert!(!bot.playable, "the training bot is not a player hero");
    assert_eq!(bot.name.as_deref(), Some("Training Bot"));
    println!(
        "practice roster: {} hero assets, {} playable, {} NPC",
        roster.len(),
        playable.len(),
        roster.len() - playable.len()
    );

    // The picker's text resolves against the playable roster only.
    assert_eq!(resolve_hero(&playable, "Tracer").unwrap().index, 3);
    assert_eq!(resolve_hero(&playable, "3").unwrap().guid, tracer.guid);
    assert_eq!(resolve_hero(&playable, " mei ").unwrap().guid, mei.guid);
    assert!(
        resolve_hero(&roster, "hero 357").is_err(),
        "an unnamed NPC hero has no name to match"
    );
    assert_eq!(hero_name(&data, tracer.guid).as_deref(), Some("Tracer"));
}

/// A whitelist mode's roster is exactly the names its teams list, and a
/// one-hero mode offers that one hero.
#[test]
fn a_restricted_mode_offers_exactly_its_heroes() {
    let Some((data, registry)) = open() else {
        return;
    };

    let teams = pools(&data, &registry, WHITELIST_MODE);
    let mut listed: Vec<u64> = Vec::new();
    for team in &teams {
        let HeroPool::Whitelist(heroes) = &team.pool else {
            panic!("escort restricts both teams, got {:?}", team.pool);
        };
        listed.extend_from_slice(heroes);
    }
    listed.sort_unstable();
    listed.dedup();

    let roster = selectable_heroes(&data, WHITELIST_MODE, registry.inner()).expect("the roster");
    assert_eq!(roster.len(), listed.len());
    assert_eq!(
        roster.iter().map(|h| h.guid).collect::<Vec<_>>(),
        listed,
        "the roster is exactly the union the teams name"
    );
    for hero in &roster {
        assert!(hero.playable, "a whitelist names player heroes only");
        assert!(hero.name.is_some(), "hero {} carries no name", hero.index);
    }

    let one =
        selectable_heroes(&data, ONE_HERO_MODE, registry.inner()).expect("the one-hero roster");
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].guid, 0x02E0_0000_0000_00DD);
    assert_eq!(one[0].name.as_deref(), Some("Mei"));
    assert_eq!(resolve_hero(&one, "mei").unwrap().guid, one[0].guid);
    assert!(
        resolve_hero(&one, "Tracer").is_err(),
        "a hero the mode does not offer is not in its roster"
    );
}

#[test]
fn every_mode_team_pool_is_a_known_class() {
    let Some((data, registry)) = open() else {
        return;
    };

    // Every type-0x0C5 asset is a game mode. Reading all of them proves the
    // reader knows every class this install actually uses - a pool class it
    // does not know must be an `Unrecognized` failure here, not a guess.
    let mut teams_seen = 0;
    let mut whitelisted = 0;
    for guid in data.guids_of_type(0x0C5) {
        let Ok(teams) = mode_team_pools(&data, guid, registry.inner()) else {
            continue;
        };
        for team in &teams {
            teams_seen += 1;
            match &team.pool {
                HeroPool::Whitelist(heroes) => {
                    whitelisted += 1;
                    assert!(!heroes.is_empty(), "0x{guid:016x}: an empty whitelist");
                }
                HeroPool::All => {}
                HeroPool::Absent => {
                    panic!("0x{guid:016x}: team {:?} declares no hero pool; its class default is unread", team.team)
                }
                HeroPool::Unrecognized(hash) => {
                    panic!("0x{guid:016x}: unrecognised hero-pool class 0x{hash:08X}")
                }
            }
        }
    }
    assert!(teams_seen > 200, "most modes declare teams ({teams_seen})");
    assert!(whitelisted > 0, "some mode restricts its heroes");
}

/// The character's collision capsule, read per hero from
/// `STUCharacterMoverComponent` (`0x6EFDBD73`, id `0x26`): the class's `+0x78`
/// (`0x1972C687`), `+0x7c` (`0x7A70E3E3`) and `+0x80` (`0x4A576BC2`) through
/// `ow_tank::character_collision::capsule_from_mover`.
///
/// Measured on this install: the roster is dominated by `0.5 / 1.5` (radius,
/// height), the large heroes carry `0.7 / 1.8` and `0.7 / 2.0`, and two heroes
/// are neither - 6 at `1.2` and 657 at `0.54 / 1.67`. Hero 458 lands exactly on
/// the client's `0.1 * r` segment floor (`0.7 / 0.07`), which makes it the
/// floor-branch anchor.
#[test]
fn every_hero_declares_a_collision_capsule() {
    use ow_tank::character_collision::SEGMENT_FRACTION;
    use ow_tank::entity::mover_capsule_of;

    let Some((data, registry)) = open() else {
        return;
    };
    let roster = selectable_heroes(&data, PRACTICE_MODE, registry.inner())
        .expect("the practice roster reads");
    let mut measured = 0usize;
    let mut common = 0usize;
    let mut tall = 0usize;
    for hero in roster.iter().filter(|h| h.playable) {
        let Some(entity) = hero_entity(&data, hero.guid) else {
            continue;
        };
        let Some(capsule) = mover_capsule_of(&data, entity).expect("the entity reads") else {
            continue;
        };
        measured += 1;
        assert!(
            capsule.radius >= 0.02,
            "hero {}: radius {}",
            hero.index,
            capsule.radius
        );
        assert!(
            capsule.point_b[1] > capsule.point_a[1],
            "hero {}: the capsule is not degenerate ({} -> {})",
            hero.index,
            capsule.point_a[1],
            capsule.point_b[1]
        );
        assert_eq!(
            (capsule.point_a[0], capsule.point_a[2]),
            (0.0, 0.0),
            "hero {}: the capsule is vertical",
            hero.index
        );
        if (capsule.radius - 0.5).abs() < 1e-6 && (capsule.height() - 1.5).abs() < 1e-6 {
            common += 1;
        }
        if (capsule.radius - 0.7).abs() < 1e-6 {
            tall += 1;
        }
    }
    assert!(
        measured > 10,
        "only {measured} playable heroes carry a mover"
    );
    assert!(
        common * 2 > measured,
        "the 0.5 / 1.5 capsule should dominate: {common} of {measured}"
    );
    assert!(
        tall >= 5,
        "the roster should carry the large 0.7 m capsule: {tall} of {measured}"
    );

    let capsule_of = |index: u32| {
        let hero = roster
            .iter()
            .find(|h| h.index == index)
            .unwrap_or_else(|| panic!("hero {index}"));
        mover_capsule_of(&data, hero_entity(&data, hero.guid).unwrap())
            .expect("reads")
            .unwrap_or_else(|| panic!("hero {index} has no mover"))
    };

    // The large-hero anchor.
    let large = capsule_of(7);
    assert!((large.radius - 0.7).abs() < 1e-6, "{}", large.radius);
    assert!((large.segment() - 0.4).abs() < 1e-6, "{}", large.segment());
    assert!((large.height() - 1.8).abs() < 1e-6, "{}", large.height());

    // The segment floor, in real data.
    let floored = capsule_of(458);
    assert!(
        (floored.segment() - SEGMENT_FRACTION * floored.radius).abs() < 1e-6,
        "hero 458 should hit the floor: {} vs {}",
        floored.segment(),
        SEGMENT_FRACTION * floored.radius
    );

    // Two heroes that are neither of the two common shapes.
    let short = capsule_of(6);
    assert!((short.height() - 1.2).abs() < 1e-6, "{}", short.height());
    let custom = capsule_of(657);
    assert!((custom.radius - 0.54).abs() < 1e-6, "{}", custom.radius);
    assert!((custom.height() - 1.67).abs() < 1e-6, "{}", custom.height());

    println!("capsules: {measured} playable heroes, {common} at 0.5/1.5, {tall} at r0.7");
}

/// The mover's vertical step limits, `+0x688`/`+0x68c`, read from
/// `STUCharacterMoverComponent` through
/// `ow_tank::character_collision::step_limits_from_mover`.
///
/// `ConstructMoverBody` (`0x140ca9730`) derives them from both capsule triples
/// and the `+0xa4` up field; this checks the roster's shape and the two
/// measured anchors.
#[test]
fn every_hero_declares_step_limits() {
    use ow_tank::entity::mover_step_limits_of;

    let Some((data, registry)) = open() else {
        return;
    };
    let roster = selectable_heroes(&data, PRACTICE_MODE, registry.inner())
        .expect("the practice roster reads");
    let mut measured = 0usize;
    let mut common = 0usize;
    for hero in roster.iter().filter(|h| h.playable) {
        let Some(entity) = hero_entity(&data, hero.guid) else {
            continue;
        };
        let Some(limits) = mover_step_limits_of(&data, entity).expect("the entity reads") else {
            continue;
        };
        measured += 1;
        assert!(limits.min <= 0.0, "hero {}: min {}", hero.index, limits.min);
        assert!(limits.max >= 0.0, "hero {}: max {}", hero.index, limits.max);
        assert!(
            limits.reach() > 0.0 && limits.reach() < 10.0,
            "hero {}: reach {}",
            hero.index,
            limits.reach()
        );
        if (limits.min + 2.5).abs() < 1e-6 && (limits.max - 1.0).abs() < 1e-6 {
            common += 1;
        }
        println!(
            "hero {:>4}: step down {:.3} up {:.3}",
            hero.index,
            limits.reach(),
            limits.max
        );
    }
    assert!(
        measured > 10,
        "only {measured} playable heroes carry a mover"
    );
    assert!(
        common * 2 > measured,
        "the 2.5 / 1.0 step range should dominate: {common} of {measured}"
    );

    let limits_of = |index: u32| {
        let hero = roster
            .iter()
            .find(|h| h.index == index)
            .unwrap_or_else(|| panic!("hero {index}"));
        mover_step_limits_of(&data, hero_entity(&data, hero.guid).unwrap())
            .expect("reads")
            .unwrap_or_else(|| panic!("hero {index} has no mover"))
    };
    // Hero 2: max(2.0 + 0.5, 1.3 + 0.25) = 2.5 down, 2 * 0.5 = 1.0 up.
    let common_limits = limits_of(2);
    assert!(
        (common_limits.reach() - 2.5).abs() < 1e-6,
        "{common_limits:?}"
    );
    assert!((common_limits.max - 1.0).abs() < 1e-6, "{common_limits:?}");
    // Hero 7: max(2.3 + 0.5, 1.72 + 0.25) = 2.8 down.
    let large = limits_of(7);
    assert!((large.reach() - 2.8).abs() < 1e-6, "{large:?}");
    println!("steps: {measured} playable heroes, {common} at 2.5/1.0");
}

/// The mover's take-off lane, class `+0xe4` = hash `0xEC289987`.
///
/// `FUN_140cc1440` (`0x140cc1627`) writes this value into the vertical lane of
/// the movement vector when a standing jump starts, and `0x140cb2220`
/// (`0x140cb246d`) reads the same body slot (`+0x60c`) as the lateral cap. The
/// body slot is the class field carried by the component copy at `+0x528`; the
/// offset mapping is proved separately by the terminal fall
/// (`+0xec` -> `body+0x614`, which `ApplyStateModifiers` reads).
///
/// The value is **not consumed** by `ow-client`: the per-frame resolve of that
/// vector is unread, so there is no arc to put it in. This test only pins that
/// the install supplies the field for every playable hero.
#[test]
fn every_hero_declares_a_takeoff_lane() {
    use ow_tank::entity::mover_takeoff_lane_of;

    let Some((data, registry)) = open() else {
        return;
    };
    let roster = selectable_heroes(&data, PRACTICE_MODE, registry.inner())
        .expect("the practice roster reads");
    let mut measured = 0usize;
    let mut lanes: Vec<(u32, f32)> = Vec::new();
    for hero in roster.iter().filter(|h| h.playable) {
        let Some(entity) = hero_entity(&data, hero.guid) else {
            continue;
        };
        let Some(lane) = mover_takeoff_lane_of(&data, entity).expect("the entity reads") else {
            continue;
        };
        measured += 1;
        assert!(
            lane > 0.0 && lane < 40.0,
            "hero {}: take-off lane {lane}",
            hero.index
        );
        lanes.push((hero.index, lane));
    }
    lanes.sort_by_key(|(index, _)| *index);
    println!("take-off lanes: {lanes:?}");
    assert!(
        measured > 10,
        "only {measured} playable heroes carry a mover"
    );
    // Measured: 6.0 for every one of the 55 playable heroes, so the value is a
    // single roster-wide constant rather than a per-hero speed. It is *not*
    // the forward base speed (`0x19452F15` reads 5.5 for most of the roster).
    let common = lanes.iter().filter(|(_, v)| (v - 6.0).abs() < 1e-6).count();
    assert_eq!(
        common, measured,
        "every playable hero should read 6.0: {common} of {measured}"
    );
}

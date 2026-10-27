//! The practice-range launch is selected from the install's own map headers.
//!
//! No GUID list is an input. The header is found by its `m_mapName`, and the
//! variation by its paired mode's `STUGameMode.displayName`. The constants are
//! expected values, not seeds.
//!
//! Skipped when no install is present. Read-only.
//!
//! ```text
//! cargo test --release -p ow-tank --test practice_range_real -- --nocapture
//! ```

use ow_tank::keytable::Sources;
use ow_tank::mapresolve::{
    find_map_by_name, find_practice_range, list_maps, resolve_practice_range, Error as MapError,
    PRACTICE_MAP_NAME, PRACTICE_MODE_NAME,
};
use ow_tank::{GameData, ManifestSelector};

const PRACTICE_HEADER: u64 = 0x0790_0000_0000_0688;
const PRACTICE_BASE_MAP: u64 = 0x0800_0000_0000_0688;
const PRACTICE_SCENE_MAP: u64 = 0x0800_0000_0F69_002E;
const PRACTICE_MODE: u64 = 0x0230_0000_0000_0018;

fn open() -> Option<GameData> {
    let root = ow_casc::install::detect_root()?;
    let sources = Sources::discover(std::slice::from_ref(&root));
    match GameData::open(&root, &ManifestSelector::default(), &sources) {
        Ok(data) => Some(data),
        Err(e) => {
            eprintln!("note: install not readable here ({e}); nothing to assert");
            None
        }
    }
}

fn kinds() -> Vec<u64> {
    let root = ow_casc::install::detect_root().expect("the install root");
    let sources = Sources::discover(std::slice::from_ref(&root));
    let kinds = sources
        .chunk_kinds()
        .expect("the client's chunk-kind table is readable");
    assert_eq!(kinds.len(), 0x24);
    kinds
}

#[test]
fn practice_range_is_found_by_map_name_and_mode_display_name() {
    let Some(data) = open() else {
        return;
    };

    let range = find_practice_range(&data).expect("the practice range is found");
    assert_eq!(range.header, PRACTICE_HEADER, "the found header");
    assert_eq!(range.map_name, PRACTICE_MAP_NAME);
    assert_eq!(range.base_map, Some(PRACTICE_BASE_MAP));
    assert_eq!(range.variation.index, 0, "the first matching variation");
    assert_eq!(range.variation.game_mode, PRACTICE_MODE);
    assert_eq!(range.variation_count, 3);
    assert_eq!(range.mode_name, PRACTICE_MODE_NAME);
    assert_eq!(range.scene_map, PRACTICE_SCENE_MAP);

    // Two entries name "Practice Range" on this header. The launch takes the
    // first in header order, and reports the other rather than hiding it; the
    // client's own selector is still open.
    assert_eq!(range.matching_variations, vec![0, 2]);

    let kinds = kinds();
    let resolved = resolve_practice_range(&data, &kinds).expect("the practice scene resolves");
    assert_eq!(resolved.range.header, PRACTICE_HEADER);
    assert_eq!(resolved.scene.requested.len(), 34);
    assert_eq!(resolved.scene.chunks.len(), 34, "all 34 chunks open");
    assert!(resolved.scene.unregistered.is_empty());
    assert!(resolved.scene.unopened.is_empty());
    assert_eq!(resolved.scene.placeables, 1358);
    assert_eq!(resolved.scene.models, 229);
    assert_eq!(resolved.scene.entities, 550);
    let (min, max) = resolved.scene.bounds.expect("bounds");
    assert_eq!(
        min.map(f32::to_bits),
        [3265552818, 3254810865, 3272246313],
        "bounds minimum, bit-exact"
    );
    assert_eq!(
        max.map(f32::to_bits),
        [1128103400, 1115134915, 1120629371],
        "bounds maximum, bit-exact"
    );
}

#[test]
fn every_map_header_lists_its_pairing() {
    let Some(data) = open() else {
        return;
    };
    let maps = list_maps(&data);
    assert_eq!(maps.len(), 305, "all 305 type-0x09F headers are readable");
    let practice = maps
        .iter()
        .find(|m| m.map_name == PRACTICE_MAP_NAME)
        .expect("the practice header is in the list");
    assert_eq!(practice.header, PRACTICE_HEADER);
    assert_eq!(practice.variations.len(), 3);
    assert_eq!(
        practice.variations[0].mode_name.as_deref(),
        Some("Practice Range")
    );
    assert_eq!(practice.variations[1].mode_name.as_deref(), Some("Stadium"));
    assert_eq!(
        practice.variations[2].mode_name.as_deref(),
        Some("Practice Range")
    );
    for v in &practice.variations {
        assert_eq!(
            ow_tank::guid::type_of(v.scene_map),
            0x002,
            "variation {} names a type-0x002 map asset",
            v.index
        );
    }

    let unresolved_modes: usize = maps
        .iter()
        .flat_map(|m| &m.variations)
        .filter(|v| v.mode_name.is_none())
        .count();
    eprintln!(
        "{} map headers, {} variations, {} with no readable mode display name",
        maps.len(),
        maps.iter().map(|m| m.variations.len()).sum::<usize>(),
        unresolved_modes
    );
}

#[test]
fn a_missing_map_name_is_an_error_not_a_default() {
    let Some(data) = open() else {
        return;
    };
    let missing = "Maps\\TrainingRoom\\Not A Map";
    match find_map_by_name(&data, missing) {
        Err(MapError::NoMapNamed { name, scanned, .. }) => {
            assert_eq!(name, missing);
            assert_eq!(scanned, 305, "every type-0x09F header was scanned");
        }
        other => panic!("a missing map name must not resolve: {other:?}"),
    }
}

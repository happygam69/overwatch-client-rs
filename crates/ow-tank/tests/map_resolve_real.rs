//! The practice-range map, resolved from the header through the client's own
//! variation pairing and chunk loader.
//!
//! proof-covers: 0x141ff5c80 0x141b32e20
//!
//! No GUID list is an input. The header names its variations; the variation
//! names its map asset; the map asset stores some chunk GUIDs and the rest are
//! synthesised by the client's table. The constants below are *expected*
//! values, not seeds: the manifest's own type-`0x0BC` records under the map's
//! index must equal the synthesised set.
//!
//! Skipped when no install is present. Read-only.
//!
//! ```text
//! cargo test --release -p ow-tank --test map_resolve_real -- --nocapture
//! ```

use ow_tank::keytable::Sources;
use ow_tank::mapresolve::{self, Header, LOADER_SLOT_ORDER, MAP_HEADER_CLASS};
use ow_tank::{bundle, guid, mapguid, GameData, ManifestSelector};

const PRACTICE_HEADER: u64 = 0x0790_0000_0000_0688;
const PRACTICE_MAP_NAME: &str = "Maps\\TrainingRoom\\Training Room - Practice";
/// `m_map`: the base map, which carries the collision/pathing chunks.
const PRACTICE_BASE_MAP: u64 = 0x0800_0000_0000_0688;

/// The three variations' `m_BF231F12` map assets, in header order.
const VARIATION_MAPS: [u64; 3] = [
    0x0800_0000_0F69_002E,
    0x0800_0000_3879_2434,
    0x0800_0000_8784_09C8,
];
/// The three variations' `m_364ADD04` layer keys, in header order.
const VARIATION_LAYERS: [u64; 3] = [
    0x0800_0100_0F69_002E,
    0x0800_0100_3879_2434,
    0x0800_0100_8784_09C8,
];
/// The modes the paired `m_D97BC44F` entries name, in header order.
const VARIATION_MODES: [u64; 3] = [
    0x0230_0000_0000_0018,
    0x0230_0000_0000_0143,
    0x0230_0000_0000_005A,
];
/// The mode entries' `m_A9253C68` map-variant identifier. It is a
/// `STUIdentifier` (`0xD800000000000000 | u32`), the same for all three
/// practice modes, and it is not a registered asset.
const PRACTICE_VARIANT: u64 = 0x0D80_0000_0000_8F9D;

/// The two base-map chunks every practice variation stores at `+0x1B8`/`+0x1C0`.
const BASE_COLLISION_CHUNKS: [u64; 2] = [0x0DD0_0011_0000_0688, 0x0DD0_0012_0000_0688];
/// The stored kind-`0x23` chunk at `+0x1C8`.
const STORED_KIND_23: u64 = 0x0DD0_0023_1AAE_B587;

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
fn practice_range_resolves_through_the_header_and_the_map_asset() {
    let Some(data) = open() else {
        return;
    };
    let kinds = kinds();

    // -----------------------------------------------------------------------
    // The header: class, name, base map.
    // -----------------------------------------------------------------------
    let mut header = Header::open(&data, PRACTICE_HEADER).expect("the practice header opens");
    assert_eq!(
        header.map_name().as_deref(),
        Some(PRACTICE_MAP_NAME),
        "m_mapName"
    );
    assert_eq!(header.map(), Some(PRACTICE_BASE_MAP), "m_map");
    assert_eq!(
        header.layer(),
        Some(0x0800_0100_0000_0688),
        "the header's own m_364ADD04 layer key"
    );

    // -----------------------------------------------------------------------
    // The variation pairing: one mode entry per variation, same index.
    // -----------------------------------------------------------------------
    let variations = header.variations().expect("the arrays pair");
    assert_eq!(variations.len(), 3, "m_78715D57 has three variations");
    for (i, v) in variations.iter().enumerate() {
        assert_eq!(v.index, i);
        assert_eq!(v.game_mode, VARIATION_MODES[i], "variation {i} mode");
        assert_eq!(v.variant, PRACTICE_VARIANT, "variation {i} variant id");
        assert_eq!(v.layer, VARIATION_LAYERS[i], "variation {i} layer key");
        assert_eq!(v.map, VARIATION_MAPS[i], "variation {i} map asset");
        assert_eq!(
            guid::type_of(v.map),
            0x002,
            "variation {i} maps to a type-0x002 asset"
        );
    }
    assert!(
        data.registry().get(PRACTICE_VARIANT).is_none(),
        "the variant identifier is a STUIdentifier, not a registered asset"
    );

    // The practice modes are unique per variation, so asking for the standard
    // practice mode has one answer. This is a data query, not a default.
    let practice = header
        .variations_for_mode(VARIATION_MODES[0])
        .expect("the arrays pair");
    assert_eq!(practice.len(), 1, "mode 0x18 names exactly one variation");
    assert_eq!(practice[0].index, 0);
    assert_eq!(practice[0].map, VARIATION_MAPS[0]);
    // The other two practice/stadium modes are distinct entries.
    assert_eq!(
        header
            .variations_for_mode(VARIATION_MODES[1])
            .unwrap()
            .iter()
            .map(|v| v.index)
            .collect::<Vec<_>>(),
        vec![1]
    );
    assert_eq!(
        header
            .variations_for_mode(VARIATION_MODES[2])
            .unwrap()
            .iter()
            .map(|v| v.index)
            .collect::<Vec<_>>(),
        vec![2]
    );

    // -----------------------------------------------------------------------
    // The selected map asset: the stored chunks and the synthesised set.
    // -----------------------------------------------------------------------
    let scene_map = practice[0].map;
    let asset = mapresolve::MapAsset::open(&data, scene_map).expect("the variation map opens");
    let stored = asset.stored_chunks().expect("the stored chunk fields");
    assert_eq!(
        stored,
        vec![
            BASE_COLLISION_CHUNKS[0],
            BASE_COLLISION_CHUNKS[1],
            STORED_KIND_23
        ],
        "the variation stores the base collision/pathing and the kind-0x23 chunk"
    );
    assert_eq!(
        stored[0] & 0xFFFF_FFFF,
        0x0688,
        "the collision chunk is under the base map's index, not the variation's"
    );

    let requested = asset.requested_chunks(&kinds).expect("the full chunk set");
    assert_eq!(requested.len(), 34, "31 synthesised + 3 stored");
    for g in &stored {
        assert!(requested.contains(g));
    }
    assert_eq!(&requested[..3], &stored[..], "the stored chunks come first");
    for (i, slot) in LOADER_SLOT_ORDER.iter().enumerate() {
        assert_eq!(
            requested[3 + i],
            mapguid::chunk_guid(scene_map, kinds[*slot]),
            "loader slot {slot:#x} is chunk {} of the request",
            3 + i
        );
    }

    // The 31 synthesised GUIDs are the manifest's own type-0x0BC records under
    // the variation map's index. This is the install's answer, not a list.
    let index = (scene_map & 0xFFFF_FFFF) as u32;
    let mut local: Vec<u64> = data
        .registry()
        .records
        .iter()
        .map(|r| r.guid)
        .filter(|g| guid::type_of(*g) == 0x0BC && (*g & 0xFFFF_FFFF) as u32 == index)
        .collect();
    local.sort_unstable();
    assert_eq!(local.len(), 31, "the variation map ships every loader slot");
    let mut synth: Vec<u64> = requested
        .iter()
        .copied()
        .filter(|g| (*g & 0xFFFF_FFFF) as u32 == index)
        .collect();
    synth.sort_unstable();
    assert_eq!(local, synth, "manifest records == the client's formula");

    // Every slot's kind is the kind table's entry, and its GUID is the
    // formula's output.
    for slot in LOADER_SLOT_ORDER {
        let g = mapguid::chunk_guid(scene_map, kinds[slot]);
        assert_eq!((g >> 32) & 0xFF, kinds[slot], "slot {slot:#x} kind");
        assert!(requested.contains(&g), "slot {slot:#x} is requested");
    }

    // -----------------------------------------------------------------------
    // Open and walk the scene.
    // -----------------------------------------------------------------------
    let scene = mapresolve::resolve_scene(&data, scene_map, &kinds).expect("the scene resolves");
    assert_eq!(scene.requested, requested);
    assert_eq!(scene.chunks.len(), 34, "all 34 requested chunks opened");
    assert!(
        scene.unregistered.is_empty(),
        "no requested chunk is unregistered: {:x?}",
        scene.unregistered
    );
    assert!(
        scene.unopened.is_empty(),
        "no requested chunk failed to open"
    );
    assert!(scene.placeables > 0, "the scene has placeables");
    assert!(scene.models > 0, "the model-group chunk names models");
    assert!(scene.entities > 0, "the entity chunk names entities");
    let (min, max) = scene.bounds.expect("the scene has placed transforms");
    assert!(min.iter().chain(max.iter()).all(|v| v.is_finite()));
    assert_eq!(scene.placeables, 1358, "the scene's placeable count");
    assert_eq!(scene.models, 229, "placeables naming a model");
    assert_eq!(scene.entities, 550, "placeables naming an entity");
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
    for c in &scene.chunks {
        assert!(
            c.parse_error.is_none(),
            "chunk 0x{:016x} (kind {:#04x}) parse: {:?}",
            c.guid,
            c.kind,
            c.parse_error
        );
        assert!(
            c.placeables()
                <= c.chunk
                    .as_ref()
                    .map(|c| c.header.placeable_count as usize)
                    .unwrap_or(0),
            "chunk 0x{:016x}: the walk cannot invent placeables",
            c.guid
        );
    }

    // The base map asset alone requests nothing that exists: its 31 synthesised
    // chunks are absent and its stored list is empty. The variation map is the
    // scene; the base map only supplies the collision/pathing chunks the
    // variation stores explicitly.
    let base = mapresolve::resolve_scene(&data, PRACTICE_BASE_MAP, &kinds)
        .expect("the base map asset opens");
    assert_eq!(base.requested.len(), 31);
    assert!(base.chunks.is_empty(), "the base map opens no chunk");
    assert_eq!(base.unregistered.len(), 31);
    assert_eq!(
        mapresolve::MapAsset::open(&data, PRACTICE_BASE_MAP)
            .unwrap()
            .stored_chunks()
            .unwrap(),
        Vec::<u64>::new(),
        "the base map stores no chunk GUID"
    );

    // -----------------------------------------------------------------------
    // Variants 1 and 2 share geometry with variant 0 for kind 0x01 but differ
    // in the entity chunk (kind 0x0B): the variations are real, not copies.
    // -----------------------------------------------------------------------
    let key_of = |map: u64, kind: u64| -> Option<[u8; 16]> {
        let g = mapguid::chunk_guid(map, kind);
        data.registry().get(g).map(|r| r.content_key)
    };
    assert_eq!(
        key_of(VARIATION_MAPS[0], 0x01),
        key_of(VARIATION_MAPS[1], 0x01),
        "the model-group chunk is shared by variations 0 and 1"
    );
    assert_ne!(
        key_of(VARIATION_MAPS[0], 0x0B),
        key_of(VARIATION_MAPS[1], 0x0B),
        "the entity chunk differs between variations 0 and 1"
    );

    // -----------------------------------------------------------------------
    // Report.
    // -----------------------------------------------------------------------
    let mut by_kind: Vec<(u8, usize, usize)> = scene
        .chunks
        .iter()
        .map(|c| (c.kind, c.size, c.placeables()))
        .collect();
    by_kind.sort_unstable();
    eprintln!(
        "practice header 0x{PRACTICE_HEADER:016x} class 0x{MAP_HEADER_CLASS:08X}: {:?}; \
         base map 0x{PRACTICE_BASE_MAP:016x} ({} requested, 0 open); variations {}; \
         mode 0x{:016x} -> variation {} map 0x{scene_map:016x}; requested {} chunks, \
         opened {} ({} placeables, {} model refs, {} entity refs), bounds {:?}..{:?}",
        header.map_name(),
        base.requested.len(),
        variations.len(),
        VARIATION_MODES[0],
        practice[0].index,
        scene.requested.len(),
        scene.chunks.len(),
        scene.placeables,
        scene.models,
        scene.entities,
        min,
        max,
    );
    for (kind, size, placeables) in by_kind {
        eprintln!("  kind 0x{kind:02x}: {size:>9} B, {placeables} placeables");
    }

    // The header census is reported by map_header_bundle_real; this test only
    // needs the practice header to be bundle-hosted, which it is.
    let record = *data.registry().get(PRACTICE_HEADER).unwrap();
    assert_eq!(
        record.unknown & bundle::RECORD_IN_BUNDLE,
        bundle::RECORD_IN_BUNDLE
    );
}

/// Cairo has 10 variations and its variations share one layer key. This proves
/// the resolver follows the variation's `m_BF231F12` scene asset, not the layer
/// key: the two have different low 32 bits.
#[test]
fn cairo_variations_use_the_scene_map_not_the_layer_key() {
    let Some(data) = open() else {
        return;
    };
    let kinds = kinds();
    const CAIRO_HEADER: u64 = 0x0790_0000_0000_005B;
    const CAIRO_BASE_MAP: u64 = 0x0800_0000_0000_005B;
    const CAIRO_LAYER: u64 = 0x0800_0100_0000_005B;
    const CAIRO_VARIATION_0_MAP: u64 = 0x0800_0000_EB17_A0F1;
    const CAIRO_VARIATION_0_MODE: u64 = 0x0230_0000_0000_0014;

    let mut header = Header::open(&data, CAIRO_HEADER).expect("the Cairo header opens");
    assert_eq!(header.map(), Some(CAIRO_BASE_MAP));
    let variations = header.variations().expect("the arrays pair");
    assert_eq!(variations.len(), 10, "Cairo has 10 variation entries");
    assert_eq!(variations[0].game_mode, CAIRO_VARIATION_0_MODE);
    assert_eq!(variations[0].layer, CAIRO_LAYER);
    assert_eq!(variations[0].map, CAIRO_VARIATION_0_MAP);
    assert_ne!(
        variations[0].map & 0xFFFF_FFFF,
        variations[0].layer & 0xFFFF_FFFF,
        "the scene map and the layer key are different indices on Cairo"
    );

    let scene = mapresolve::resolve_scene(&data, variations[0].map, &kinds)
        .expect("the Cairo variation scene resolves");
    assert_eq!(scene.requested.len(), 34);
    assert_eq!(scene.chunks.len(), 34, "all 34 chunks open");
    assert!(scene.unregistered.is_empty());
    assert!(scene.unopened.is_empty());
    assert!(scene.placeables > 0);

    let base =
        mapresolve::resolve_scene(&data, CAIRO_BASE_MAP, &kinds).expect("the Cairo base map opens");
    assert_eq!(base.requested.len(), 31);
    assert!(base.chunks.is_empty(), "the base map opens no chunk");

    eprintln!(
        "Cairo: 10 variations; variation 0 mode 0x{CAIRO_VARIATION_0_MODE:016x} \
         layer 0x{CAIRO_LAYER:016x} scene 0x{CAIRO_VARIATION_0_MAP:016x}; \
         scene {} chunks, {} placeables; base keeps 0 open",
        scene.chunks.len(),
        scene.placeables
    );
}

/// A mode can name several variations. Header `0x07900000000000d4` has 63
/// variations for 21 modes, so a requested mode does not by itself identify a
/// scene map; that choice is the open selector.
#[test]
fn a_mode_can_name_several_variations() {
    let Some(data) = open() else {
        return;
    };
    const LAYERED_HEADER: u64 = 0x0790_0000_0000_00D4;
    let header = Header::open(&data, LAYERED_HEADER).expect("the 63-variation header opens");
    let variations = header.variations().expect("the arrays pair");
    assert_eq!(variations.len(), 63);
    let mut counts = std::collections::BTreeMap::new();
    for v in &variations {
        *counts.entry(v.game_mode).or_insert(0usize) += 1;
    }
    assert_eq!(counts.len(), 20, "63 variations over 20 distinct modes");
    assert!(counts.values().all(|n| *n == 3 || *n == 6));
    let (mode, count) = counts
        .iter()
        .max_by_key(|(_, n)| **n)
        .expect("at least one mode");
    assert_eq!(*count, 6, "one mode names six variations");
    assert_eq!(header.variations_for_mode(*mode).unwrap().len(), *count);
    eprintln!(
        "header 0x{LAYERED_HEADER:016x}: 63 variations over 20 modes; \
         mode 0x{mode:016x} names {count} variations"
    );
}

/// A mode plus its map-variant identifier still does not identify one scene
/// map. Header `0x07900000000000d4` has 63 variations over 20 modes and three
/// variant identifiers; mode `0x0230000000000016` names each identifier
/// twice, with a different scene map and resource key each time. This is why
/// `find_practice_range` cannot be replaced by a `(mode, variant)` lookup: the
/// client is supplied a type-`0x002` map GUID, it does not derive one here.
#[test]
fn a_mode_and_variant_pair_can_still_name_two_scene_maps() {
    let Some(data) = open() else {
        return;
    };
    const LAYERED_HEADER: u64 = 0x0790_0000_0000_00D4;
    const SIX_ENTRY_MODE: u64 = 0x0230_0000_0000_0016;
    const VARIANTS: [u64; 3] = [
        0x0D80_0000_0000_A039,
        0x0D80_0000_0000_A062,
        0x0D80_0000_0000_A0C4,
    ];

    let header = Header::open(&data, LAYERED_HEADER).expect("the 63-variation header opens");
    let variations = header.variations().expect("the arrays pair");
    assert_eq!(variations.len(), 63);

    let mut by_variant = std::collections::BTreeMap::new();
    let mut by_mode = std::collections::BTreeMap::new();
    let mut pairs = std::collections::BTreeMap::new();
    for v in &variations {
        *by_variant.entry(v.variant).or_insert(0usize) += 1;
        *by_mode.entry(v.game_mode).or_insert(0usize) += 1;
        *pairs.entry((v.game_mode, v.variant)).or_insert(0usize) += 1;
        assert!(
            data.registry().get(v.variant).is_none(),
            "variant 0x{:016x} is an identifier, not an asset",
            v.variant
        );
    }
    assert_eq!(
        by_variant,
        VARIANTS.iter().map(|v| (*v, 21)).collect(),
        "three variant identifiers, 21 entries each"
    );
    assert_eq!(by_mode.len(), 20, "63 variations over 20 distinct modes");
    assert_eq!(by_mode.get(&SIX_ENTRY_MODE), Some(&6));
    // The six entries are two scene maps for each of the three identifiers.
    let mut six: Vec<(u64, u64)> = variations
        .iter()
        .filter(|v| v.game_mode == SIX_ENTRY_MODE)
        .map(|v| (v.variant, v.map))
        .collect();
    six.sort_unstable();
    assert_eq!(six.len(), 6);
    for chunk in six.chunks(2) {
        assert_eq!(chunk[0].0, chunk[1].0, "two entries share the variant id");
        assert_ne!(chunk[0].1, chunk[1].1, "but name different scene maps");
    }
    assert_eq!(pairs.get(&(SIX_ENTRY_MODE, VARIANTS[0])), Some(&2));
    eprintln!(
        "header 0x{LAYERED_HEADER:016x}: {} variations, {} modes, 3 variant ids \
         (21 each); mode 0x{SIX_ENTRY_MODE:016x} names six, two per variant",
        variations.len(),
        by_mode.len()
    );
}

/// The replay header's map GUID is a header's `m_map` (base map), not a
/// variation's `m_BF231F12` scene map. This is the install half of the proof
/// that the game's map identity is the base map (see `docs/formats/maps.md`).
#[test]
fn a_replay_map_guid_is_the_base_map_not_a_scene_map() {
    let Some(data) = open() else {
        return;
    };
    const REPLAY_HEADER: u64 = 0x0790_0000_0000_069E;
    const REPLAY_MAP: u64 = 0x0800_0000_0000_069E;

    let maps = mapresolve::list_maps(&data);
    let replay = maps
        .iter()
        .find(|m| m.header == REPLAY_HEADER)
        .expect("the replay's header is in the catalog");
    assert_eq!(
        replay.base_map,
        Some(REPLAY_MAP),
        "the replay's map GUID is the header's m_map"
    );
    assert!(
        maps.iter()
            .flat_map(|m| &m.variations)
            .all(|v| v.scene_map != REPLAY_MAP),
        "the replay's map GUID is not any variation's scene map"
    );
    eprintln!(
        "replay map 0x{REPLAY_MAP:016x} = header 0x{REPLAY_HEADER:016x} m_map; \
         scene of none of the {} headers",
        maps.len()
    );
}

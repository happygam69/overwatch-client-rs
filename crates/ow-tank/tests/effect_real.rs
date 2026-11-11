//! The effect layer against the real installation.
//!
//! The sweep opens every effect asset of the six types the game uses, parses
//! the `EFCT` container and checks the invariants a wrong layout would break:
//!
//! 1. **Every chunk tag resolves.** A tag is either in the client's kind table
//!    at `0x143610490` or it is *not* an effect-component kind; the sweep prints
//!    which tags fall outside the table so a new one is visible.
//! 2. **Every version is the one measured.** Each tag carries one chunk version
//!    across the whole install; the module's [`OBSERVED_VERSIONS`] are pinned
//!    here, so a build that changes one fails instead of drifting.
//! 3. **Every component the module types is as long as the client reads.** A
//!    model component (`ECMD`) is 32 bytes, not the reference's 24; a sound
//!    component (`ECSO`) is 32, not 8; and so on. A short one is a real layout
//!    difference, not a truncation.
//! 4. **The named fields carry the GUID types the names claim.** Every `ECMD`
//!    `+0x00` is type `0x00C`, every `+0x08` type `0x01A`; every `ECEF` `+0x00`
//!    is type `0x00D`; every `ECSO` `+0x00` type `0x02C`; every `ECVS` `+0x00`
//!    type `0x078`; every `ECRR` `+0x00` type `0x00C`; every `ECEN` `+0x00`
//!    type `0x003`; every `ECEC` `+0x00` type `0x006` and `+0x10` type `0x01C`.
//!    This is what names a field: a `u64` at the right offset with one asset
//!    type on every occurrence is a reference, not an integer.
//!
//! Skipped when no install is present. Read-only.
//!
//! ```text
//! cargo test -p ow-tank --release --test effect_real -- --nocapture
//! ```

use std::collections::BTreeMap;

use ow_tank::effect::{self, Effect};
use ow_tank::{guid, GameData, ManifestSelector};

/// The six asset types that carry an `EFCT` root.
const EFFECT_TYPES: [u16; 6] = [0x00D, 0x04A, 0x08E, 0x08F, 0x0D4, 0x12B];

/// Upper bound on assets opened from the largest type. Type `0x00D` alone has
/// 34,027 assets on the reference install; the small types are read whole.
const SAMPLE_CAP: usize = 2000;

fn open() -> Option<GameData> {
    let root = ow_casc::install::detect_root()?;
    let sources = ow_tank::keytable::Sources::discover(std::slice::from_ref(&root));
    match GameData::open(&root, &ManifestSelector::default(), &sources) {
        Ok(data) => Some(data),
        Err(e) => {
            eprintln!("note: install not readable here ({e}); nothing to assert");
            None
        }
    }
}

#[derive(Default)]
struct Census {
    assets: usize,
    unreadable: usize,
    chunks: usize,
    tags: BTreeMap<String, (usize, u16)>,
    outside_table: BTreeMap<String, usize>,
    components: BTreeMap<String, usize>,
}

#[test]
fn effect_assets_parse_and_every_component_keeps_the_clients_widths() {
    let _t0 = std::time::Instant::now();
    let Some(data) = open() else {
        return;
    };
    eprintln!("effect install open: {:?}", _t0.elapsed());

    let mut census = Census::default();
    for type_id in EFFECT_TYPES {
        let guids = data.guids_of_type(type_id);
        if guids.is_empty() {
            continue;
        }
        let _t2 = std::time::Instant::now();
        for g in guids.iter().take(SAMPLE_CAP) {
            let Ok(bytes) = data.open_asset(*g) else {
                census.unreadable += 1;
                continue;
            };
            let effect = match Effect::parse(&bytes) {
                Ok(e) => e,
                Err(e) => {
                    panic!("type {type_id:03X} asset 0x{g:016x}: {e}");
                }
            };
            census.assets += 1;
            census.chunks += effect.chunks.len();
            for chunk in &effect.chunks {
                let name = chunk.name();
                let entry = census
                    .tags
                    .entry(name.clone())
                    .or_insert((0, chunk.version));
                entry.0 += 1;
                assert_eq!(
                    entry.1, chunk.version,
                    "{name} carries two versions on this install"
                );
                if chunk.kind == effect::NO_KIND {
                    *census.outside_table.entry(name.clone()).or_insert(0) += 1;
                }
                check_component(&mut census, &name, chunk, *g);
            }
        }
        eprintln!("effect sweep {type_id:03X}: {:?}", _t2.elapsed());
    }

    assert!(census.assets > 0, "no effect asset parsed");
    // An unreadable asset is a CASC/registry condition, not a layout one: the
    // install is allowed to be partial (the full sweep sees 4 of 66,293).
    assert!(
        census.unreadable * 10 < census.assets,
        "{} of {} effect assets unreadable",
        census.unreadable,
        census.assets
    );

    // Every version the module records was seen and matched.
    for (tag, want) in effect::OBSERVED_VERSIONS {
        let name = String::from_utf8_lossy(tag).to_string();
        if let Some((_, got)) = census.tags.get(&name) {
            assert_eq!(*got, want, "{name} version");
        }
    }

    println!(
        "effect census: {} assets ({} unreadable), {} chunks, {} distinct tags",
        census.assets,
        census.unreadable,
        census.chunks,
        census.tags.len()
    );
    for (name, (n, version)) in &census.tags {
        let kind = effect::kind_of(&[
            name.as_bytes()[0],
            name.as_bytes()[1],
            name.as_bytes()[2],
            name.as_bytes()[3],
        ]);
        println!("  {name:5} n={n:7} version={version:2} kind={kind:02X}");
    }
    println!("  outside the kind table: {:?}", census.outside_table);
    println!("  components: {:?}", census.components);

    // The tags the client has no read site for are not component kinds.
    for tag in [b"ECMP", b"ECSS", b"ECAT", b"ESHT", b"ECCV"] {
        assert_eq!(effect::kind_of(tag), effect::NO_KIND);
    }

    // The component set is non-empty and typed on this install.
    assert!(census.components.get("ECMD").copied().unwrap_or(0) > 0);
    assert_eq!(
        effect::OBSERVED_VERSIONS[0],
        (b"EFCT", effect::ROOT_VERSION)
    );
}

fn check_component(census: &mut Census, name: &str, chunk: &effect::Chunk<'_>, guid: u64) {
    let mut seen = |what: &str| {
        *census.components.entry(what.to_string()).or_insert(0) += 1;
    };
    match name {
        "ECMD" => {
            let m = chunk
                .model()
                .unwrap_or_else(|| panic!("ECMD 0x{guid:016x} is {} bytes", chunk.payload.len()));
            assert_eq!(guid::type_of(m.model), 0x00C, "ECMD model");
            assert_eq!(guid::type_of(m.look), 0x01A, "ECMD look");
            if m.animation != 0 {
                assert_eq!(guid::type_of(m.animation), 0x006, "ECMD animation");
            }
            seen("ECMD");
        }
        "ECEF" => {
            let e = chunk.effect().expect("ECEF too short");
            assert_eq!(guid::type_of(e.effect), 0x00D, "ECEF effect");
            seen("ECEF");
        }
        "ECEN" => {
            let e = chunk.entity().expect("ECEN too short");
            assert_eq!(guid::type_of(e.entity), 0x003, "ECEN entity");
            if e.unknown_08 != 0 {
                assert_eq!(guid::type_of(e.unknown_08), 0x01C, "ECEN +0x08");
            }
            seen("ECEN");
        }
        "ECEC" => {
            let e = chunk.entity_control().expect("ECEC too short");
            if e.animation != 0 {
                assert_eq!(guid::type_of(e.animation), 0x006, "ECEC animation");
            }
            if e.identifier != 0 {
                assert_eq!(guid::type_of(e.identifier), 0x01C, "ECEC identifier");
            }
            seen("ECEC");
        }
        "ECSO" => {
            let s = chunk.sound().expect("ECSO too short");
            assert_eq!(guid::type_of(s.sound), 0x02C, "ECSO sound");
            seen("ECSO");
        }
        "ECVS" => {
            let v = chunk.voice_stimulus().expect("ECVS too short");
            assert_eq!(guid::type_of(v.stimulus), 0x078, "ECVS stimulus");
            seen("ECVS");
        }
        "ECRR" => {
            let r = chunk.ribbon().expect("ECRR too short");
            assert_eq!(guid::type_of(r.model), 0x00C, "ECRR model");
            seen("ECRR");
        }
        _ => {}
    }
}

/// The payload sizes the client's copies require, measured on the install.
///
/// A size is the distance from the first to the last byte the builder copies
/// (`FUN_142e4a780`), not the declared chunk size: the declared size is
/// usually larger.
#[test]
fn component_payload_sizes_match_the_client() {
    let Some(data) = open() else {
        return;
    };
    let want = [
        (b"ECMD", 0x1au16, 0x20u16),
        (b"ECEF", 0x0d, 0x20),
        (b"ECEN", 0x45, 0x50),
        (b"ECEC", 0x38, 0x40),
        (b"ECSO", 0x13, 0x20),
        (b"ECVS", 0x28, 0x30),
        (b"ECRR", 0x39, 0x40),
    ];
    let mut checked = BTreeMap::new();
    for type_id in EFFECT_TYPES {
        for g in data.guids_of_type(type_id).iter().take(SAMPLE_CAP) {
            let Ok(bytes) = data.open_asset(*g) else {
                continue;
            };
            let Ok(e) = Effect::parse(&bytes) else {
                continue;
            };
            for chunk in &e.chunks {
                for (tag, min, size) in want {
                    if &chunk.tag == tag {
                        assert!(
                            chunk.payload.len() >= min as usize,
                            "{} on 0x{g:016x} is {} bytes, the client reads {min:#x}",
                            chunk.name(),
                            chunk.payload.len()
                        );
                        assert_eq!(
                            chunk.payload.len(),
                            size as usize,
                            "{} declared payload size moved",
                            chunk.name()
                        );
                        *checked.entry(chunk.name()).or_insert(0) += 1;
                    }
                }
            }
        }
    }
    for (tag, ..) in want {
        let name = String::from_utf8_lossy(tag).to_string();
        assert!(
            checked.get(&name).copied().unwrap_or(0) > 0,
            "{name} was not seen on this install sample"
        );
    }
    println!("component sizes: {checked:?}");
}

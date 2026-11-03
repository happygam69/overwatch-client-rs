//! Where the brush would have to come from, measured against the install.
//!
//! The brush itself is not stored as a chunk of any asset this install's
//! content manifests list, and the one route the image does name — a source
//! record whose asset reference must resolve to GUID type `0x00A` — has no
//! candidates at all. This test **pins the measurement** rather than a brush,
//! because no brush-bearing asset was found to pin. What it proves is narrow
//! and worth having: a reader that goes looking for a brush asset in this
//! install's manifests will find none, and the model chunk sets carry no
//! collision container.
//!
//! The evidence for each statement, and what remains open, is in
//! `docs/formats/brush-collision.md`.
//!
//! ```text
//! cargo test --release -p ow-tank --test brush_real -- --nocapture
//! ```

use ow_tank::brush;
use ow_tank::chunks::Chunks;
use ow_tank::guid;
use ow_tank::keytable::Sources;
use ow_tank::{GameData, ManifestSelector};

/// The GUID type the brush-source driver requires of a source's asset: it
/// calls the type extractor `0x141fd3b20` at `0x14213bc8a` and compares the
/// answer against `0x0a` at `0x14213bc8f`.
const BRUSH_SOURCE_TYPE: u16 = 0x00A;
/// GUID type `0x00C` is the model, which is where a brush tree would sit if it
/// were a chunk of the model.
const MODEL_TYPE: u16 = 0x00C;
/// How many models the chunk-tag census covers.
const MODEL_SAMPLE: usize = 300;

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

#[test]
fn no_asset_of_the_brush_source_type_exists_in_this_install() {
    let Some(data) = open() else {
        return;
    };
    let histogram = data.type_histogram();
    let brush_type = data.guids_of_type(BRUSH_SOURCE_TYPE);
    eprintln!(
        "manifests {:?}: {} records, {} distinct types, type 0x{BRUSH_SOURCE_TYPE:03X}: {}",
        data.manifests,
        data.registry().records.len(),
        histogram.len(),
        brush_type.len()
    );
    assert_eq!(
        brush_type.len(),
        0,
        "the brush-source asset type 0x{BRUSH_SOURCE_TYPE:03X} must have no records for the \
         asset route to be blocked here"
    );
    // The neighbouring types are populated, so the zero above is a fact about
    // this type and not an empty registry.
    assert!(histogram.get(&0x00C).is_some_and(|n| *n > 0));
    assert!(histogram.get(&0x00D).is_some_and(|n| *n > 0));
    // The type numbering is the read one: a type-0x00A GUID's mangled field is
    // 0x900, since the field is the 12-bit reversal of (type - 1).
    assert_eq!(guid::type_of(0x0900_0000_0000_0001), BRUSH_SOURCE_TYPE);
}

#[test]
fn a_model_carries_no_brush_container_chunk() {
    let Some(data) = open() else {
        return;
    };
    let models = data.guids_of_type(MODEL_TYPE);
    assert!(!models.is_empty(), "type 0x00C is the model type");
    let mut tags: std::collections::BTreeMap<String, usize> = Default::default();
    let mut parsed = 0usize;
    for guid in models.iter().take(MODEL_SAMPLE) {
        let Ok(bytes) = data.open_asset(*guid) else {
            continue;
        };
        let Ok(chunks) = Chunks::parse(&bytes) else {
            continue;
        };
        parsed += 1;
        for entry in &chunks.entries {
            *tags.entry(entry.tag_str()).or_default() += 1;
        }
    }
    eprintln!("{parsed} models parsed; chunk tags: {tags:?}");
    assert_eq!(parsed, MODEL_SAMPLE, "every sampled model opened");
    let known = ["MDLC", "MRNM", "MSTU", "MESD", "mskl", "CLTH"];
    for tag in tags.keys() {
        assert!(
            known.contains(&tag.as_str()),
            "unexpected model chunk tag {tag}: a brush container would show up here"
        );
    }
}

/// The record sizes the reader assumes are the ones the ingest copies: a
/// vertex block of `count << 5` bytes and a node block of `count * 0x28`.
///
/// This is a statement about the image, not the install, and it is here so the
/// constants the reader uses are tied to the bytes rather than to a claim in a
/// comment. `ow_tank::brush` builds and re-reads its own records to prove the
/// layout is self-consistent; the sizes are what `0x14213a510` copies.
#[test]
fn the_record_sizes_are_the_ones_the_ingest_copies() {
    let points = [
        [0.0, 0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0, 0.0],
        [1.0, 1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
    ];
    let brush = brush::build_from_instances(&points, [1.0, 1.0]).expect("the square is accepted");
    assert_eq!(brush.vertices.len() * brush::VERTEX_SIZE, 4 << 5);
    assert_eq!(brush.nodes.len() * brush::NODE_SIZE, 4 * 0x28);
    assert_eq!(brush.vertices.len(), brush.nodes.len());
}

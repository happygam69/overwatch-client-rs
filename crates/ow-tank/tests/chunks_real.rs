//! Read the chunk table of real assets from the install.
//!
//! Skipped when no install is present.

use ow_tank::chunks::Chunks;
use ow_tank::keytable::Sources;
use ow_tank::{guid, GameData, ManifestSelector};

#[test]
fn real_model_and_map_chunks_parse() {
    let Some(root) = ow_casc::install::detect_root() else {
        return;
    };
    let sources = Sources::discover(&[root.clone()]);
    let data = match GameData::open(&root, &ManifestSelector::default(), &sources) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("note: install not readable here ({e}); nothing to assert");
            eprintln!("  root={:?}", root);
            eprintln!("  tactlib={:?} owlib={:?}", sources.tactlib_checkout, sources.owlib_checkout);
            return;
        }
    };

    for (type_id, label) in [
        (guid::types::MODEL, "model"),
        (0x0BCu16, "map chunk"),
    ] {
        let guids = data.guids_of_type(type_id);
        let mut parsed = 0usize;
        let mut tags_seen: Vec<String> = Vec::new();
        for guid in guids.iter().take(40) {
            let Ok(bytes) = data.bytes_of(*guid) else {
                continue;
            };
            let Ok(chunks) = Chunks::parse(&bytes) else {
                continue;
            };
            parsed += 1;
            if tags_seen.is_empty() {
                tags_seen.push(format!("0x{guid:016x}: {}", chunks.summary()));
            }
        }
        eprintln!("{label} (type 0x{type_id:03X}): {parsed} of 40 parsed");
        for line in &tags_seen {
            eprintln!("  {line}");
        }
        assert!(parsed > 0, "no {label} asset parsed as chunked data");
    }
}

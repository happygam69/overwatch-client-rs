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
    let sources = Sources::discover(std::slice::from_ref(&root));
    let data = match GameData::open(&root, &ManifestSelector::default(), &sources) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("note: install not readable here ({e}); nothing to assert");
            eprintln!("  root={:?}", root);
            eprintln!(
                "  tactlib={:?} owlib={:?}",
                sources.tactlib_checkout, sources.owlib_checkout
            );
            return;
        }
    };

    for (type_id, label, expect_chunked) in [
        (guid::types::MODEL, "model", true),
        (0x0BCu16, "map chunk", false),
    ] {
        let guids = data.guids_of_type(type_id);
        let mut parsed = 0usize;
        let mut open_failed = 0usize;
        let mut parse_failed = 0usize;
        let mut first_open_error: Option<String> = None;
        let mut first_bytes: Option<String> = None;
        let mut tags_seen: Vec<String> = Vec::new();
        for guid in guids.iter().take(40) {
            let bytes = match data.bytes_of(*guid) {
                Ok(bytes) => bytes,
                Err(e) => {
                    open_failed += 1;
                    first_open_error.get_or_insert_with(|| format!("0x{guid:016x}: {e}"));
                    continue;
                }
            };
            if first_bytes.is_none() {
                let head: Vec<String> = bytes.iter().take(16).map(|b| format!("{b:02x}")).collect();
                first_bytes = Some(format!(
                    "0x{guid:016x}: {} bytes, head {}",
                    bytes.len(),
                    head.join(" ")
                ));
            }
            let Ok(chunks) = Chunks::parse(&bytes) else {
                parse_failed += 1;
                continue;
            };
            parsed += 1;
            if tags_seen.is_empty() {
                tags_seen.push(format!("0x{guid:016x}: {}", chunks.summary()));
            }
        }
        eprintln!(
            "{label} (type 0x{type_id:03X}): {parsed} of {} parsed \
             ({open_failed} would not open, {parse_failed} not chunked)",
            guids.len().min(40)
        );
        if let Some(line) = &first_open_error {
            eprintln!("  first open error: {line}");
        }
        if let Some(line) = &first_bytes {
            eprintln!("  first bytes: {line}");
        }
        for line in &tags_seen {
            eprintln!("  {line}");
        }
        if expect_chunked {
            assert!(parsed > 0, "no {label} asset parsed as chunked data");
        } else {
            // A map chunk is a placeable list, not a chunked container, so the
            // assertion here is the inverse: it must *not* look chunked.
            assert_eq!(parsed, 0, "{label} unexpectedly parsed as chunked data");
        }
    }
}

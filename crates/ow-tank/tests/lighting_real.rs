//! Parse real lighting manifests (type `0x0BD`) from the install and check the
//! header shape the parser was written against.
//!
//! Skipped when no install is present.

use ow_tank::keytable::Sources;
use ow_tank::{guid, lighting, GameData, ManifestSelector};

#[test]
fn real_lighting_manifests_parse_and_match_their_length() {
    let Some(root) = ow_casc::install::detect_root() else {
        return;
    };
    let sources = Sources::discover(&[root.clone()]);
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

    let guids = data.guids_of_type(lighting::TYPE);
    let tried = guids.len().min(400);
    let mut parsed = 0usize;
    let mut open_failed = 0usize;
    let mut parse_failed = 0usize;
    let mut first_open_error: Option<String> = None;
    let mut first_parse_error: Option<String> = None;
    let mut first_line: Option<String> = None;
    let mut chunks_seen = 0usize;
    let mut exact_length = 0usize;
    let mut size_matches = 0usize;
    let mut chunk_type_ok = 0usize;
    let mut bad_chunk_types: Vec<String> = Vec::new();
    for guid_v in guids.iter().take(tried) {
        let bytes = match data.bytes_of(*guid_v) {
            Ok(bytes) => bytes,
            Err(e) => {
                open_failed += 1;
                first_open_error.get_or_insert_with(|| format!("0x{guid_v:016x}: {e}"));
                continue;
            }
        };
        let manifest = match lighting::parse(&bytes) {
            Ok(m) => m,
            Err(e) => {
                parse_failed += 1;
                first_parse_error.get_or_insert_with(|| {
                    let head: Vec<String> =
                        bytes.iter().take(16).map(|b| format!("{b:02x}")).collect();
                    format!("0x{guid_v:016x}: {e}; head {}", head.join(" "))
                });
                continue;
            }
        };
        parsed += 1;
        assert_eq!(manifest.header.magic, lighting::MAGIC);
        assert_eq!(
            manifest.chunks.len(),
            manifest.header.chunk_count as usize,
            "0x{guid_v:016x}: record count does not follow the header count"
        );
        if lighting::HEADER_SIZE + manifest.chunks.len() * lighting::CHUNK_SIZE == bytes.len() {
            exact_length += 1;
        }
        if manifest.header.data_size as usize == bytes.len() - 8 {
            size_matches += 1;
        }
        for c in &manifest.chunks {
            if guid::type_of(c.guid) == lighting::CHUNK_TYPE {
                chunk_type_ok += 1;
            } else if bad_chunk_types.len() < 5 {
                bad_chunk_types.push(format!(
                    "0x{guid_v:016x}: record type 0x{:03x} (0x{:016x})",
                    guid::type_of(c.guid),
                    c.guid
                ));
            }
        }
        chunks_seen += manifest.chunks.len();
        if first_line.is_none() {
            let h = &manifest.header;
            first_line = Some(format!(
                "0x{guid_v:016x}: {} bytes, chunk_count {}, unknown1 {}, unknown2 {}, \
                 unknown4/5 {}/{}, first chunk type 0x{:03x}",
                bytes.len(),
                h.chunk_count,
                h.unknown1,
                h.unknown2,
                h.unknown4,
                h.unknown5,
                guid::type_of(manifest.chunks[0].guid)
            ));
        }
    }

    eprintln!(
        "lighting manifests (type 0x{:03X}): {parsed} of {tried} parsed \
         ({open_failed} would not open, {parse_failed} did not parse; {} in the registry)",
        lighting::TYPE,
        guids.len()
    );
    if let Some(line) = &first_open_error {
        eprintln!("  first open error: {line}");
    }
    if let Some(line) = &first_parse_error {
        eprintln!("  first parse error: {line}");
    }
    if let Some(line) = &first_line {
        eprintln!("  {line}");
    }
    eprintln!(
        "  chunks named: {chunks_seen}; chunk type 0x{:03X}: {chunk_type_ok}/{chunks_seen}",
        lighting::CHUNK_TYPE
    );
    eprintln!(
        "  invariants: header+8*count == asset length: {exact_length}/{parsed}; \
         data_size == length-8: {size_matches}/{parsed}"
    );

    if parsed == 0 && open_failed == tried {
        eprintln!("  every lighting manifest was streamed, not packed; nothing to assert");
        return;
    }
    assert!(parsed > 0, "no lighting manifest parsed");
    assert_eq!(
        exact_length, parsed,
        "some manifests' chunk arrays do not fill the asset"
    );
    assert!(
        bad_chunk_types.is_empty(),
        "chunk records that are not lighting chunks: {bad_chunk_types:?}"
    );
}

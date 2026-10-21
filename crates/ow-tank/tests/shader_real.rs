//! Parse real shader assets from the install and check the invariants the
//! parser was written against: groups (type `0x085`) and code blobs (`0x087`).
//! Instances (`0x086`) do not exist in this build's registry — the test says
//! so instead of failing; the instance layout is a port of the reference and
//! carries no real-data validation.
//!
//! Skipped when no install is present.

use ow_tank::keytable::Sources;
use ow_tank::{guid, shader, GameData, ManifestSelector};

#[test]
fn real_shader_assets_parse() {
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

    // ---- groups ----
    let guids = data.guids_of_type(shader::TYPE_GROUP);
    let tried = guids.len().min(400);
    let mut parsed = 0usize;
    let mut open_failed = 0usize;
    let mut parse_failed = 0usize;
    let mut first_open_error: Option<String> = None;
    let mut first_parse_error: Option<String> = None;
    let mut first_line: Option<String> = None;
    let mut entries_seen = 0usize;
    let mut code_type_ok = 0usize;
    let mut bad_code_types: Vec<String> = Vec::new();
    let mut source_types: std::collections::BTreeMap<u16, usize> = Default::default();
    for guid_v in guids.iter().take(tried) {
        let bytes = match data.bytes_of(*guid_v) {
            Ok(bytes) => bytes,
            Err(e) => {
                open_failed += 1;
                first_open_error.get_or_insert_with(|| format!("0x{guid_v:016x}: {e}"));
                continue;
            }
        };
        let group = match shader::parse_group(&bytes) {
            Ok(g) => g,
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
        for entry in &group.entries {
            entries_seen += 1;
            if guid::type_of(entry.code) == shader::TYPE_CODE {
                code_type_ok += 1;
            } else if bad_code_types.len() < 5 {
                bad_code_types.push(format!(
                    "0x{guid_v:016x}: code ref type 0x{:03x} (0x{:016x})",
                    guid::type_of(entry.code),
                    entry.code
                ));
            }
            *source_types.entry(guid::type_of(entry.source)).or_insert(0) += 1;
        }
        if first_line.is_none() {
            first_line = Some(format!(
                "0x{guid_v:016x}: {} bytes, {} entries, source_guid 0x{:016x} (type 0x{:03x})",
                bytes.len(),
                group.entries.len(),
                group.source_guid,
                guid::type_of(group.source_guid)
            ));
        }
    }

    eprintln!(
        "shader groups (type 0x{:03X}): {parsed} of {tried} parsed \
         ({open_failed} would not open, {parse_failed} did not parse; {} in the registry)",
        shader::TYPE_GROUP,
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
        "  entries: {entries_seen}; code refs of type 0x{:03X}: {code_type_ok}/{entries_seen}",
        shader::TYPE_CODE
    );
    eprintln!("  source ref types: {source_types:03x?}");

    // ---- code ----
    let code_guids = data.guids_of_type(shader::TYPE_CODE);
    let code_tried = code_guids.len().min(200);
    let mut code_parsed = 0usize;
    let mut code_open_failed = 0usize;
    let mut code_parse_failed = 0usize;
    let mut first_code_open_error: Option<String> = None;
    let mut first_code_parse_error: Option<String> = None;
    let mut inflated_total = 0usize;
    let mut first_code_line: Option<String> = None;
    for guid_v in code_guids.iter().take(code_tried) {
        let bytes = match data.bytes_of(*guid_v) {
            Ok(bytes) => bytes,
            Err(e) => {
                code_open_failed += 1;
                first_code_open_error.get_or_insert_with(|| format!("0x{guid_v:016x}: {e}"));
                continue;
            }
        };
        let code = match shader::parse_code(&bytes) {
            Ok(c) => c,
            Err(e) => {
                code_parse_failed += 1;
                first_code_parse_error.get_or_insert_with(|| {
                    let head: Vec<String> =
                        bytes.iter().take(16).map(|b| format!("{b:02x}")).collect();
                    format!("0x{guid_v:016x}: {e}; head {}", head.join(" "))
                });
                continue;
            }
        };
        code_parsed += 1;
        inflated_total += code.bytecode.len();
        if first_code_line.is_none() {
            first_code_line = Some(format!(
                "0x{guid_v:016x}: {} bytes, data at 0x{:x}, compressed {}, inflated {}",
                bytes.len(),
                code.header.data_offset,
                code.header.compressed_size,
                code.bytecode.len()
            ));
        }
    }

    eprintln!(
        "shader code (type 0x{:03X}): {code_parsed} of {code_tried} parsed \
         ({code_open_failed} would not open, {code_parse_failed} did not parse; {} in the registry)",
        shader::TYPE_CODE,
        code_guids.len()
    );
    if let Some(line) = &first_code_open_error {
        eprintln!("  first open error: {line}");
    }
    if let Some(line) = &first_code_parse_error {
        eprintln!("  first parse error: {line}");
    }
    if let Some(line) = &first_code_line {
        eprintln!("  {line}");
    }
    eprintln!("  inflated bytes: {inflated_total}");

    // ---- instances ----
    let instance_guids = data.guids_of_type(shader::TYPE_INSTANCE);
    if instance_guids.is_empty() {
        eprintln!(
            "shader instances (type 0x{:03X}): NOT FOUND — 0 in the registry; the reference \
             layout stays unvalidated against this install",
            shader::TYPE_INSTANCE
        );
    } else {
        let inst_tried = instance_guids.len().min(40);
        let mut inst_parsed = 0usize;
        let mut inst_open_failed = 0usize;
        let mut first_inst_error: Option<String> = None;
        for guid_v in instance_guids.iter().take(inst_tried) {
            let bytes = match data.bytes_of(*guid_v) {
                Ok(bytes) => bytes,
                Err(e) => {
                    inst_open_failed += 1;
                    first_inst_error.get_or_insert_with(|| format!("0x{guid_v:016x} open: {e}"));
                    continue;
                }
            };
            match shader::parse_instance(&bytes) {
                Ok(_) => inst_parsed += 1,
                Err(e) => {
                    first_inst_error.get_or_insert_with(|| format!("0x{guid_v:016x}: {e}"));
                }
            }
        }
        eprintln!(
            "shader instances (type 0x{:03X}): {inst_parsed} of {inst_tried} parsed \
             ({inst_open_failed} would not open)",
            shader::TYPE_INSTANCE
        );
        if let Some(line) = &first_inst_error {
            eprintln!("  first error: {line}");
        }
        if !(inst_parsed == 0 && inst_open_failed == inst_tried) {
            assert!(inst_parsed > 0, "no shader instance parsed");
        }
    }

    // ---- verdicts ----
    if !(parsed == 0 && open_failed == tried) {
        assert!(parsed > 0, "no shader group parsed");
        assert!(
            bad_code_types.is_empty(),
            "group code refs that are not code assets: {bad_code_types:?}"
        );
    } else {
        eprintln!("  every shader group was streamed, not packed; nothing to assert");
    }
    if !(code_parsed == 0 && code_open_failed == code_tried) {
        assert!(code_parsed > 0, "no shader code parsed");
    } else {
        eprintln!("  every shader code was streamed, not packed; nothing to assert");
    }
}

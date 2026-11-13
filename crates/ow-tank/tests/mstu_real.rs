//! Parse real models' `MSTU` chunks (the model's structured data) and check
//! the semantics the parser was written against: the header's offset is
//! relative to the MSTU payload, not the asset.
//!
//! proof-covers: 0x141b5df70 0x141b88df0
//!
//! Skipped when no install is present.

use ow_tank::chunks::Chunks;
use ow_tank::keytable::Sources;
use ow_tank::{mstu, GameData, ManifestSelector};

/// Asset type number of a model.
const TYPE_MODEL: u16 = 0x00C;

fn u64_at(data: &[u8], at: usize) -> Option<u64> {
    data.get(at..at + 8)
        .map(|s| u64::from_le_bytes(s.try_into().unwrap()))
}

#[test]
fn real_mstu_chunks_parse() {
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

    let guids = data.guids_of_type(TYPE_MODEL);
    let mut with_mstu = 0usize;
    let mut header_parsed = 0usize;
    let mut parsed = 0usize;
    let mut offset_16 = 0usize;
    let mut in_bounds = 0usize;
    let mut instances_total = 0usize;
    let mut as_is_ok_count = 0usize;
    let mut integration_tried = 0usize;
    let mut integration_ok = 0usize;
    let mut first_line: Option<String> = None;
    let mut first_error: Option<String> = None;
    let mut scanned = 0usize;
    let scan_cap = 300;
    for guid_v in guids.iter().take(scan_cap) {
        if with_mstu >= 60 {
            break;
        }
        scanned += 1;
        let Ok(bytes) = data.bytes_of(*guid_v) else {
            continue;
        };
        let Ok(chunks) = Chunks::parse(&bytes) else {
            continue;
        };
        if chunks.all_with_tag(b"MSTU").is_empty() {
            continue;
        }
        let Some(payload) = chunks.payload(&bytes, b"MSTU") else {
            continue;
        };
        with_mstu += 1;

        let header = match mstu::parse_header(&bytes) {
            Ok(h) => h,
            Err(e) => {
                first_error.get_or_insert_with(|| format!("0x{guid_v:016x}: header: {e}"));
                continue;
            }
        };
        header_parsed += 1;
        if header.offset == 16 {
            offset_16 += 1;
        }
        if header.offset >= 0 && (header.offset + header.size as i64) as u64 <= payload.len() as u64
        {
            in_bounds += 1;
        }

        match mstu::parse(&bytes) {
            Ok(stu) => {
                parsed += 1;
                instances_total += stu.instances.len();
            }
            Err(e) => {
                first_error.get_or_insert_with(|| format!("0x{guid_v:016x}: parse: {e}"));
            }
        }

        // The parser is wired into the model reader: a model that parses from
        // its own bytes must carry the MSTU header. (Models whose vertices live
        // in a separate asset cannot be parsed here and are not counted.)
        if integration_tried < 5 {
            integration_tried += 1;
            if let Ok(model) = ow_tank::model::parse(&bytes, &|_| None) {
                assert!(
                    model.mstu.is_some(),
                    "0x{guid_v:016x}: model::parse dropped the MSTU header"
                );
                integration_ok += 1;
            }
        }

        // The reading the header would support if its offset were measured
        // from the asset start instead: `asset[offset..offset+size]`.
        let offset = u64_at(payload, 0).unwrap_or(u64::MAX) as usize;
        let size = u64_at(payload, 8).unwrap_or(u64::MAX) as usize;
        if offset
            .checked_add(size)
            .is_some_and(|end| end <= bytes.len())
            && ow_stu::Stu::parse(bytes[offset..offset + size].to_vec()).is_ok()
        {
            as_is_ok_count += 1;
        }

        if first_line.is_none() {
            first_line = Some(format!(
                "0x{guid_v:016x}: asset {} B, MSTU at {}, payload {} B, offset {}, size {}",
                bytes.len(),
                chunks.all_with_tag(b"MSTU")[0].offset,
                payload.len(),
                header.offset,
                header.size
            ));
        }
    }

    eprintln!(
        "models scanned: {scanned} of {} (capped at {scan_cap}); with an MSTU payload: {with_mstu}",
        guids.len()
    );
    if let Some(line) = &first_line {
        eprintln!("  {line}");
    }
    if let Some(line) = &first_error {
        eprintln!("  first error: {line}");
    }
    eprintln!(
        "  headers parsed: {header_parsed}/{with_mstu} (offset == 16: {offset_16}); \
         slice in bounds of the payload: {in_bounds}/{with_mstu}"
    );
    eprintln!(
        "  STU parsed: {parsed}/{with_mstu}; instances total: {instances_total}; \
         asset-relative reading that would also parse: {as_is_ok_count}/{with_mstu}"
    );

    assert!(with_mstu > 0, "no model with an MSTU payload found");
    assert_eq!(
        header_parsed, with_mstu,
        "an MSTU header did not parse; {first_error:?}"
    );
    assert_eq!(
        parsed, with_mstu,
        "an MSTU chunk did not parse as STU; {first_error:?}"
    );
    assert_eq!(
        in_bounds, with_mstu,
        "an MSTU slice ran outside its payload"
    );
    assert!(instances_total > 0, "no STU instances in any model");
    assert!(
        integration_ok > 0,
        "no model parsed from its own bytes; MSTU integration unverified"
    );
}

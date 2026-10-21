//! The asset-association layer: synthetic pins first, then a real-install pass.
//!
//! The synthetic tests build a structured-data file byte by byte, so they run
//! with no install present and they fail if an offset, a filter or the shape of
//! the result drifts rather than quietly agreeing with it. The install pass is
//! skipped when there is no install, like `tests/mapchunk_kinds.rs`.
//!
//! Two properties are pinned here that were both wrong once, and both were
//! wrong *quietly*:
//!
//! * a candidate that is not a GUID in this installation's registry must not be
//!   reported as a reference (and a candidate that is must be);
//! * "I read this and it references nothing" must not look like "I could not
//!   read this".
//!
//! Run the census and the timing pass with:
//!
//! ```text
//! OW_INSTALL=E:\Overwatch cargo test -p ow-tank --release --test assoc -- --nocapture
//! OW_ASSOC_CENSUS=100000 ...   # read every asset of the type, not a sample
//! ```

use std::collections::{BTreeSet, HashSet};

use ow_tank::assoc::{self, AssetStatus};
use ow_tank::{guid, GameData, ManifestSelector};

// ===========================================================================
// A minimal structured-data file, built by hand
// ===========================================================================

/// Type hash of the synthetic instance. Arbitrary -- the reference collection
/// walks fields by offset, not by class.
const SYN_TYPE_HASH: u32 = 0x1111_1111;
/// Field hash of the synthetic instance's first field; later fields add one.
const SYN_FIELD_HASH: u32 = 0xBBBB_BBBB;

/// A GUID the synthetic tests put *into* the fake registry, so it must survive.
const GUID_IN: u64 = 0x3800_0000_0000_0001;
/// A GUID the synthetic tests leave *out* of the fake registry, so it must go.
///
/// Its type number is 8, which is inside the plausible range the raw-field path
/// tests for, so it is genuinely a candidate -- a value outside that range would
/// be dropped by the type test alone and the membership check would not be what
/// removed it.
const GUID_OUT: u64 = 0x3800_0000_0000_0002;

/// Byte offsets that do not move with the field count.
const SYN_INSTANCE_RECORD: usize = 40;
const SYN_BAG_HEADER: usize = 56;
/// Where the data buffer starts once the field definitions are written.
fn syn_data_start(fields: usize) -> usize {
    SYN_BAG_HEADER + 8 + fields * 8
}

fn put_i32(blob: &mut [u8], at: usize, v: i32) {
    blob[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

/// A minimal version-2 structured-data file with one instance whose payload is
/// `stored.len()` eight-byte fields, all with non-zero declared sizes.
///
/// ```text
///  0..36   header: instance bag, field-bag array, dyn size/off, data offset
/// 40..56   instance record: hash, embedder field, embedder index, payload size
/// 56..64   inner field-bag header: field count, offset of the definitions
/// 64..     one field definition per field: field hash, byte size (always 8)
/// ..       data buffer: bag index (4 bytes), then the stored values
/// ```
///
/// The values are written in as given; [`synthetic_file`] rewrites them once
/// the header checksum is known, because the deobfuscation mixes that checksum
/// in and it is computed over the first 36 bytes.
fn synthetic_stu(stored: &[u64]) -> Vec<u8> {
    let data = syn_data_start(stored.len());
    let payload = 4 + stored.len() * 8;
    let mut blob = vec![0u8; data + payload];

    put_i32(&mut blob, 0, 1); // instance bag holds one record
    put_i32(&mut blob, 4, SYN_INSTANCE_RECORD as i32);
    put_i32(&mut blob, 16, 1); // field-bag array holds one inner bag
    put_i32(&mut blob, 20, SYN_BAG_HEADER as i32);
    put_i32(&mut blob, 24, 0); // dynamic block: size
    put_i32(&mut blob, 28, 0); // dynamic block: offset
    put_i32(&mut blob, 32, data as i32); // data buffer starts here

    blob[SYN_INSTANCE_RECORD..SYN_INSTANCE_RECORD + 4]
        .copy_from_slice(&SYN_TYPE_HASH.to_le_bytes());
    put_i32(&mut blob, SYN_INSTANCE_RECORD + 8, -1); // not embedded
    put_i32(&mut blob, SYN_INSTANCE_RECORD + 12, payload as i32);

    put_i32(&mut blob, SYN_BAG_HEADER, stored.len() as i32);
    put_i32(&mut blob, SYN_BAG_HEADER + 4, (SYN_BAG_HEADER + 8) as i32);
    for (i, _) in stored.iter().enumerate() {
        let def = SYN_BAG_HEADER + 8 + i * 8;
        blob[def..def + 4].copy_from_slice(&(SYN_FIELD_HASH + i as u32).to_le_bytes());
        put_i32(&mut blob, def + 4, 8); // an 8-byte field
    }

    put_i32(&mut blob, data, 0); // payload uses bag 0
    write_stored(&mut blob, data, stored);
    blob
}

/// The data buffer of a file with `fields` fields starts here.
fn write_stored(blob: &mut [u8], data: usize, stored: &[u64]) {
    for (i, v) in stored.iter().enumerate() {
        let at = data + 4 + i * 8;
        blob[at..at + 8].copy_from_slice(&v.to_le_bytes());
    }
}

/// The synthetic file for `stored`, together with the header checksum the
/// deobfuscation will use.
fn synthetic_file(stored: &[u64]) -> (Vec<u8>, u64) {
    let mut blob = synthetic_stu(stored);
    let checksum = ow_stu::Stu::parse(blob.clone())
        .expect("synthetic file parses")
        .header_checksum;
    write_stored(&mut blob, syn_data_start(stored.len()), stored);
    (blob, checksum)
}

/// The bytes to store so that the reference path decodes them as `wanted`.
///
/// **This is not `ow_stu::deobfuscate` again.** That function XORs first and
/// shuffles the bytes second, and the two steps do not commute -- shuffle and
/// XOR by a *different* byte pattern per position are not inverses of each
/// other, so calling it twice does not give the original value back. The
/// inverse therefore has to run in the opposite order: shuffle, then XOR. The
/// shuffle is its own inverse, so the same four swaps serve both directions.
fn stored_for(checksum: u64, field_hash: u32, wanted: u64) -> u64 {
    let mut bytes = wanted.to_le_bytes();
    bytes.swap(0, 3);
    bytes.swap(7, 1);
    bytes.swap(2, 6);
    bytes.swap(4, 5);
    let mixed = (field_hash as u64) | ((field_hash as u64) << 32);
    let mask = (mixed ^ checksum).to_le_bytes();
    for (b, m) in bytes.iter_mut().zip(mask) {
        *b ^= m;
    }
    u64::from_le_bytes(bytes)
}

/// The two values one eight-byte field produces: what the reference path
/// decodes out of it, and what the raw path sees.
fn derived_values(checksum: u64, field: usize, stored: u64) -> (u64, u64) {
    (
        ow_stu::deobfuscate(checksum, SYN_FIELD_HASH + field as u32, stored),
        stored,
    )
}

/// The condition the raw-field path applies before it even looks at the
/// registry: non-zero, and a type number inside the plausible range.
fn looks_like_guid(v: u64) -> bool {
    let ty = guid::type_of(v);
    v != 0 && ty > 1 && ty < 0xFF
}

/// The fake registry the synthetic tests run against.
fn fake_known() -> HashSet<u64> {
    HashSet::from([GUID_IN])
}

/// The lowest-numbered GUID of a type. The type number lives in bits 48..59
/// stored reversed, so it has to be put there the way the registry stores it
/// rather than shifted in directly.
fn guid_of_type(t: u16) -> u64 {
    assert!(t >= 1, "the type number is the reversed field plus one");
    ((guid::reverse_type_bits(t - 1) as u64) << 48) | 1
}

/// The candidate values a structured-data walk produces with **no** membership
/// check -- that is, what this layer emitted before the filter existed. Kept
/// here so a test can compare the two answers directly.
fn unfiltered_candidates(stu: &mut ow_stu::Stu) -> BTreeSet<u64> {
    let mut out = BTreeSet::new();
    for i in 0..stu.instances.len() {
        let fields: Vec<_> = stu.fields(i).to_vec();
        let base = stu.payload_start(i);
        for f in fields {
            if f.size != 8 {
                continue;
            }
            let at = base + f.offset;
            if let Some(v) = stu.asset_ref_at(at, f.hash) {
                if v != 0 {
                    out.insert(v);
                }
            }
            if let Some(v) = stu.u64_at(at) {
                let ty = guid::type_of(v);
                if v != 0 && ty > 1 && ty < 0xFF {
                    out.insert(v);
                }
            }
        }
    }
    out
}

// ===========================================================================
// 1 + 2 + 4: membership filtering, and that it is not vacuous
// ===========================================================================

/// The pair that keeps the filter test honest: a candidate that is not in the
/// registry must be dropped, and a candidate that is must survive.
#[test]
fn membership_filter_keeps_a_registry_guid_and_drops_everything_else() {
    let known = fake_known();

    // Two fields. The header's data offset depends on the field count, so the
    // checksum used to build the fixture must come from a file with the same
    // number of fields -- a file with a different shape is a different file.
    let (probe, checksum) = synthetic_file(&[0, 0]);
    let store_real = stored_for(checksum, SYN_FIELD_HASH, GUID_IN);
    let store_junk = GUID_OUT;
    assert_eq!(
        ow_stu::Stu::parse(probe)
            .expect("probe parses")
            .header_checksum,
        checksum,
        "the checksum must not depend on the payload, only on the first 36 bytes"
    );

    // Before asserting anything, check the fixture is actually two-sided: the
    // two fields must not have swapped roles, and the noise must really be
    // noise. Otherwise the test could pass with a filter that drops everything.
    let (ref_real, raw_real) = derived_values(checksum, 0, store_real);
    assert_eq!(
        ref_real, GUID_IN,
        "field 0 must deobfuscate to the registered GUID"
    );
    assert!(
        !known.contains(&raw_real),
        "field 0's raw value {raw_real:#018x} must not itself be a registered GUID, \
         or the two paths would not be distinguishable"
    );
    let (ref_junk, raw_junk) = derived_values(checksum, 1, store_junk);
    assert_eq!(
        raw_junk, GUID_OUT,
        "field 1 stores the unregistered GUID raw"
    );
    assert!(
        looks_like_guid(raw_junk),
        "field 1's raw value must survive the plausible-type test, or the \
         membership check would not be what removed it"
    );
    assert!(
        !known.contains(&ref_junk),
        "field 1's deobfuscated value {ref_junk:#018x} must not be registered either"
    );

    // Which candidates the two paths produced that are not in the registry.
    // Field 0's deobfuscated value is the registered GUID, so it is not one.
    // Field 1's two values always are: the reference path has no type test, and
    // field 1's raw value was checked above to survive one.
    let mut rejected = vec![ref_junk, raw_junk];
    if looks_like_guid(raw_real) {
        rejected.push(raw_real);
    }
    assert!(
        rejected.iter().all(|v| !known.contains(v)),
        "the rejected candidates must not be in the registry, or this would not be \
         a membership test"
    );
    assert!(
        rejected.iter().collect::<BTreeSet<_>>().len() == rejected.len(),
        "the rejected candidates must be distinct for the counts below to line up: \
         {rejected:?}"
    );

    let (blob, _checksum) = synthetic_file(&[store_real, store_junk]);
    let mut stu = ow_stu::Stu::parse(blob).expect("synthetic file parses");
    let unfiltered = unfiltered_candidates(&mut stu);
    let got = assoc::stu_references_with(&mut stu, &known);

    // 1: the junk is gone.
    assert!(
        !got.references.contains(&GUID_OUT),
        "a GUID that is not in the registry must not be reported as a reference"
    );
    assert!(
        !got.references.contains(&ref_junk),
        "the deobfuscated value that is not in the registry must not be reported either"
    );

    // 2: the real one survived. Without this, a filter that drops everything
    // would satisfy the assertions above.
    assert!(
        got.references.contains(&GUID_IN),
        "a GUID that is in the registry must be reported; got {:?}",
        got.references
    );
    assert_eq!(
        got.references.len(),
        1,
        "exactly the one registered GUID survives"
    );

    // 4: non-vacuity. The unfiltered answer and the filtered answer must
    // differ, or the filter is not doing anything and this test proves nothing.
    assert!(
        unfiltered.contains(&GUID_OUT) && unfiltered.contains(&ref_junk),
        "the unfiltered walk must produce the values the filter removes, \
         otherwise there is nothing to filter; it produced {unfiltered:?}"
    );
    assert!(
        unfiltered.len() > got.references.len(),
        "the unfiltered walk produced {} value(s) and the filtered walk {}, so the \
         filter changed nothing",
        unfiltered.len(),
        got.references.len()
    );
    assert!(
        got.references.is_subset(&unfiltered),
        "filtering must only ever remove values, never invent them"
    );
    assert_eq!(
        got.dropped,
        rejected.len(),
        "every candidate that was not a GUID in the registry must be counted as \
         dropped, so the cost of the check is visible"
    );
    assert_eq!(
        got.dropped,
        unfiltered.len() - got.references.len(),
        "the number dropped and the difference between the two answers are the \
         same fact, counted two ways"
    );
}

// ===========================================================================
// 3: a failed read is not an empty list
// ===========================================================================

/// Three results that must not be confused: an asset read cleanly that happens
/// to reference nothing, an asset whose structured data would not parse, and an
/// asset whose bytes are not here at all.
///
/// The first two need no install. The third does, because "the bytes are not
/// on this machine" is a fact about the machine.
#[test]
fn empty_parse_failed_and_unreadable_are_three_different_results() {
    let known = fake_known();
    // A hero, so the structured-data path is the one taken.
    let stu_type_guid = guid_of_type(0x075);

    // (a) Read cleanly, references nothing. Every field is zero, which both
    //     paths treat as "no value".
    let (blob, _) = synthetic_file(&[0]);
    let empty = assoc::scan_asset_bytes(stu_type_guid, &blob, &known);
    assert_eq!(empty.status, AssetStatus::Stu, "the file parsed");
    assert!(empty.references.is_empty(), "and it references nothing");

    // (b) Structured data would not parse: too short, and not a v1 magic either.
    let broken = assoc::scan_asset_bytes(stu_type_guid, &[0u8; 8], &known);
    assert_ne!(
        broken.status,
        AssetStatus::Stu,
        "an eight-byte file cannot be structured data"
    );
    assert!(
        matches!(broken.status, AssetStatus::StuParseFailed(_)),
        "a structured-data type whose file does not parse must say so; got {}",
        broken.status
    );
    assert!(
        broken.status.detail().is_some_and(|d| !d.is_empty()),
        "the parse failure must carry its reason"
    );
    assert!(
        broken.structured_data_info.is_none(),
        "the reference tool emits no stu_info for a fallback record, and neither \
         do we -- the status is what distinguishes it, not stu_info"
    );

    // The three must be distinguishable from one another. (a) and (b) both have
    // an empty reference set, so the status is the only thing telling them
    // apart, and that is the whole point.
    assert_eq!(empty.references.len(), broken.references.len());
    assert_ne!(empty.status, broken.status);

    // (c) Unreadable: the GUID is not in the registry, so there are no bytes.
    let Some(data) = open() else {
        eprintln!("note: no install; the unreadable case is not exercised here");
        return;
    };
    let absent = 0x0BCD_0000_00FF_FFFFu64;
    if data.registry().get(absent).is_some() {
        eprintln!("note: asset {absent:#018x} is unexpectedly in the registry; skipped");
        return;
    }
    let unreadable = assoc::process_asset(&data, absent, &known);
    assert!(
        matches!(unreadable.status, AssetStatus::Unreadable(_)),
        "an asset with no bytes must say it was unreadable, not that it is empty; \
         got {}",
        unreadable.status
    );
    assert!(
        !unreadable.status.is_complete(),
        "an unreadable read is never a complete one"
    );
    assert!(
        unreadable.status.detail().is_some_and(|d| !d.is_empty()),
        "the unreadable case must carry its reason"
    );
    assert_ne!(unreadable.status, empty.status);
    assert_ne!(unreadable.status, broken.status);

    // And a payload type is a fourth thing again: deliberately not read.
    let skipped = assoc::scan_asset_bytes(guid_of_type(0x004), &blob, &known);
    assert_eq!(skipped.status, AssetStatus::PayloadSkipped);
    assert!(!skipped.status.is_complete());
    assert_ne!(skipped.status, empty.status);
}

// ===========================================================================
// 5: the exported JSON
// ===========================================================================

/// The four fields the reference tool emits must stay byte for byte what it
/// emits, so a file written here can be diffed against one written by it. The
/// two fields after them are ours and are documented as a divergence.
#[test]
fn exported_json_keeps_the_reference_fields_byte_identical() {
    let mut record = assoc::AssetRecord {
        guid: GUID_IN,
        references: BTreeSet::from([GUID_OUT]),
        structured_data_info: Some(assoc::StructuredDataInfo { instances: None }),
        chunked_data_info: None,
        status: AssetStatus::Stu,
        dropped_by_membership: 7,
    };
    let json = assoc::asset_record_json(&record);

    let expected_prefix = format!(
        "{{\n  \"guid\": \"{}\",\n  \"references\": [\n    \"{}\"\n  ],\n  \
         \"stu_info\": {{\n    \"instances\": null\n  }},\n  \"chunked_info\": null,\n",
        assoc::guid_string(GUID_IN),
        assoc::guid_string(GUID_OUT),
    );
    assert!(
        json.starts_with(&expected_prefix),
        "the reference's four fields changed. Expected the file to start with:\n\
         {expected_prefix}\ngot:\n{json}"
    );
    assert!(
        json.contains("\"status\": { \"kind\": \"stu\" }"),
        "the status must be reported, and reported after the reference's fields:\n{json}"
    );
    assert!(
        json.contains("\"dropped_by_membership\": 7"),
        "what the membership check cost must be reported:\n{json}"
    );

    // The empty-reference-list case, which is the one a consumer is most likely
    // to mistake for a failed read, must also keep the reference's formatting.
    record.references.clear();
    let json = assoc::asset_record_json(&record);
    let expected_prefix = format!(
        "{{\n  \"guid\": \"{}\",\n  \"references\": [],\n  \"stu_info\": {{\n    \
         \"instances\": null\n  }},\n  \"chunked_info\": null,\n",
        assoc::guid_string(GUID_IN),
    );
    assert!(
        json.starts_with(&expected_prefix),
        "the empty reference list changed shape:\n{json}"
    );

    // An unreadable record still carries all four fields -- null ones -- so the
    // file is well formed and the failure is visible rather than inferred from
    // a missing key.
    record.status = AssetStatus::Unreadable("key not present in this installation".into());
    let json = assoc::asset_record_json(&record);
    assert!(
        json.contains("\"kind\": \"unreadable\""),
        "an unreadable record must say so:\n{json}"
    );
    assert!(
        json.contains("\"detail\": \"key not present in this installation\""),
        "the reason must be carried:\n{json}"
    );
    assert!(
        json.starts_with(&expected_prefix),
        "the reference's fields must not change because the read failed:\n{json}"
    );
}

// ===========================================================================
// Type classification
// ===========================================================================

/// Type `0x051` is in both of the reference's lists. The reference resolves
/// that in favour of structured data; this pins that decision so a caller
/// cannot quietly get it back the other way round.
#[test]
fn type_0x051_is_structured_data_not_an_opaque_payload() {
    assert!(
        assoc::KNOWN_PAYLOAD_TYPES.contains(&0x051),
        "the reference's payload list still lists it; this test is about the \
         overlap, so the list itself must not drift"
    );
    assert!(assoc::is_stu_v2_type(0x051));
    assert_eq!(assoc::classify(0x051), assoc::AssetKind::StuV2);
    assert!(
        !assoc::is_known_payload_type(0x051),
        "with the payload list consulted first this returned true, and the type \
         emitted no record at all"
    );
    // A type that is only in the payload list is unaffected.
    assert!(assoc::is_known_payload_type(0x004));
    assert_eq!(assoc::classify(0x004), assoc::AssetKind::Payload);
    // The ordinary shapes.
    assert_eq!(assoc::classify(0x075), assoc::AssetKind::StuV2);
    assert_eq!(assoc::classify(0x05A), assoc::AssetKind::StuV1);
    assert_eq!(assoc::classify(0x008), assoc::AssetKind::Raw);
}

// ===========================================================================
// Real install
// ===========================================================================

fn open() -> Option<GameData> {
    let root = ow_casc::install::detect_root()?;
    let sources = ow_tank::keytable::Sources::discover(&[root.clone()]);
    match GameData::open(&root, &ManifestSelector::default(), &sources) {
        Ok(data) => Some(data),
        Err(e) => {
            eprintln!("note: install not readable here ({e}); nothing to assert");
            None
        }
    }
}

/// How many assets per type the census reads. The full type 0x01B is several
/// thousand assets, which is minutes of disk time; the census strides over the
/// rest and says so. Set `OW_ASSOC_CENSUS` to override the cap.
const CENSUS_CAP: usize = 400;

fn census_cap() -> usize {
    std::env::var("OW_ASSOC_CENSUS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(CENSUS_CAP)
}

/// Count what the two answers are on real assets: what the layer emits now, and
/// what it would have emitted with no membership check at all. The second
/// number is the defect this file is here to catch.
#[test]
fn census_of_real_references() {
    let Some(data) = open() else {
        return;
    };
    let known = assoc::known_guids(&data);
    let occupied: HashSet<u16> = data
        .registry()
        .records
        .iter()
        .map(|r| guid::type_of(r.guid))
        .collect();

    for t in [0x075u16, 0x01B, 0x051] {
        let all = data.guids_of_type(t);
        if all.is_empty() {
            eprintln!("type 0x{t:03x}: no assets in the registry");
            continue;
        }
        let cap = census_cap();
        let stride = (all.len() / cap).max(1);
        let mut sampled = 0usize;
        let mut unreadable = 0usize;
        let mut parse_failed = 0usize;
        let mut unfiltered_total = 0usize;
        let mut unfiltered_distinct = BTreeSet::new();
        let mut unfiltered_impossible = 0usize;
        let mut impossible_distinct = BTreeSet::new();
        let mut filtered_total = 0usize;
        let mut filtered_distinct = BTreeSet::new();
        let mut dropped = 0usize;
        let mut bytes_total = 0usize;

        for g in all.iter().step_by(stride).take(cap) {
            sampled += 1;
            let Ok(bytes) = data.bytes_of(*g) else {
                unreadable += 1;
                continue;
            };
            bytes_total += bytes.len();
            let mut stu = match ow_stu::Stu::parse(bytes) {
                Ok(s) => s,
                Err(_) => {
                    parse_failed += 1;
                    continue;
                }
            };
            let raw = unfiltered_candidates(&mut stu);
            unfiltered_total += raw.len();
            for v in &raw {
                unfiltered_distinct.insert(*v);
                if !occupied.contains(&guid::type_of(*v)) {
                    unfiltered_impossible += 1;
                    impossible_distinct.insert(*v);
                }
            }
            let got = assoc::stu_references_with(&mut stu, &known);
            filtered_total += got.references.len();
            filtered_distinct.extend(got.references.iter().copied());
            dropped += got.dropped;
        }

        eprintln!(
            "type 0x{t:03x}: sampled {sampled} of {} asset(s), {bytes_total} bytes, \
             {unreadable} unreadable, {parse_failed} parse-failed",
            all.len()
        );
        eprintln!(
            "  without the membership filter: {unfiltered_total} reference(s) emitted, \
             {} distinct, {unfiltered_impossible} occurrence(s) of {} distinct value(s) \
             whose type no asset on this install occupies",
            unfiltered_distinct.len(),
            impossible_distinct.len()
        );
        eprintln!(
            "  with the membership filter:    {filtered_total} reference(s) emitted, \
             {} distinct, {dropped} candidate(s) dropped",
            filtered_distinct.len()
        );
        assert!(
            filtered_distinct.iter().all(|v| known.contains(v)),
            "every reported reference must be a GUID in this registry"
        );
    }
}

/// How many assets the timing run reads, spread across size bands.
const TIMING_CAP: usize = 120;
/// How many size bands the timing run splits the sample into.
const TIMING_BANDS: usize = 4;
/// The types the timing run measures. `0x075` holds small assets, `0x01B`
/// medium ones and `0x0BC` map chunks, which are megabytes each -- without a
/// type with big files in the sample there is no way to see whether the cost
/// tracks the file.
const TIMING_TYPES: &[u16] = &[0x075, 0x01B, 0x0BC];
/// How many assets are opened just to learn their sizes. Reading a size means
/// reading the whole file, and type `0x0BC` is tens of gigabytes on this
/// install, so the scan strides rather than walking every one.
const TIMING_SIZE_SCAN: usize = 240;

/// Time the per-asset entry point and split the result by asset size, because
/// the question is whether the per-asset cost now tracks the file or is a flat
/// charge paid on every asset whatever its size.
///
/// Before the membership set was hoisted out of the asset loop this reported a
/// flat 64-73 ms per asset from 366 bytes to 288 KB.
#[test]
fn timing_of_per_asset_read() {
    let Some(data) = open() else {
        return;
    };
    let known = assoc::known_guids(&data);
    // Measured once, so the report can say what the hoist removed rather
    // than infer it from two totals.
    let started = std::time::Instant::now();
    let _rebuilt = assoc::known_guids(&data);
    eprintln!(
        "building the membership set once: {:.1} ms",
        started.elapsed().as_secs_f64() * 1000.0
    );
    for t in TIMING_TYPES.iter().copied() {
        let all = data.guids_of_type(t);
        if all.is_empty() {
            continue;
        }
        let mut sizes: Vec<(u64, usize)> = Vec::new();
        // Stride over the type rather than reading all of it: a size costs a
        // whole file, and one of these types is tens of gigabytes.
        let scan_stride = (all.len() / TIMING_SIZE_SCAN).max(1);
        for g in all.iter().step_by(scan_stride).take(TIMING_SIZE_SCAN) {
            if let Ok(b) = data.bytes_of(*g) {
                sizes.push((*g, b.len()));
            }
        }
        if sizes.is_empty() {
            eprintln!("type 0x{t:03x}: no asset could be read; timing skipped");
            continue;
        }
        eprintln!(
            "type 0x{t:03x}: {} asset(s), sized from a {} asset strided sample",
            all.len(),
            sizes.len()
        );
        sizes.sort_by_key(|(_, n)| *n);

        // Split into bands by rank, so each band holds a similar number of
        // assets and together the bands span the size range.
        let per_band = (sizes.len() / TIMING_BANDS).max(1);
        for b in 0..TIMING_BANDS {
            let start = (b * per_band).min(sizes.len());
            let end = (((b + 1) * per_band).min(sizes.len())).max(start);
            let slice = &sizes[start..end];
            if slice.is_empty() {
                continue;
            }
            let stride = (slice.len() / (TIMING_CAP / TIMING_BANDS).max(1)).max(1);
            let sample: Vec<(u64, usize)> = slice.iter().step_by(stride).copied().collect();
            let total_bytes: usize = sample.iter().map(|(_, n)| *n).sum();

            let started = std::time::Instant::now();
            for (g, _) in &sample {
                let _ = assoc::process_asset(&data, *g, &known);
            }
            let elapsed = started.elapsed().as_secs_f64();

            // How much of that is just getting the bytes out of the container?
            // Whatever is left is the work this layer actually does.
            let started = std::time::Instant::now();
            for (g, _) in &sample {
                let _ = data.bytes_of(*g);
            }
            let read_only = started.elapsed().as_secs_f64();

            eprintln!(
                "type 0x{t:03x} band {b}: {} asset(s), mean {:.0} bytes \
                 (min {} max {}), {:.1} ms per asset total, {:.1} ms of it the \
                 file read, {:.1} ms the work",
                sample.len(),
                total_bytes as f64 / sample.len() as f64,
                sample.iter().map(|(_, n)| *n).min().unwrap_or(0),
                sample.iter().map(|(_, n)| *n).max().unwrap_or(0),
                elapsed * 1000.0 / sample.len() as f64,
                read_only * 1000.0 / sample.len() as f64,
                // This difference comes out slightly negative on the small
                // bands, and that is the measurement, not a negative amount of
                // work: the two loops are timed separately and the noise in the
                // container read is larger than the work itself.
                (elapsed - read_only) * 1000.0 / sample.len() as f64,
            );
        }
    }
}

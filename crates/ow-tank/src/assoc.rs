//! Asset association — OWLib TankTonka-style per-asset reference collection.
//!
//! This mirrors the reference tool's two entry points:
//!
//! * blob assets are byte-scanned for verbatim asset GUIDs;
//! * STU v2 assets have every instance walked for obfuscated asset refs and for
//!   raw integer fields whose values decode to valid asset types;
//!
//! and the reference's file layout is emitted by `ow-tank-assoc --out`.
//!
//! Two things this layer is careful about, because both were wrong once:
//!
//! **Every candidate is checked against the registry.** An eight-byte field in
//! a structured-data asset yields two candidate values (the deobfuscated one and
//! the raw one), and either can be any 64-bit number at all. A value is only
//! reported as a reference if it is a GUID that exists in this installation's
//! registry. The trade this makes is real: a reference to an asset that is not
//! downloaded here is dropped, because nothing here can tell it apart from
//! noise. The dropped count is kept (see [`AssetRecord::dropped_by_membership`])
//! so the loss is visible rather than silent.
//!
//! **A failed read is never an empty list.** Every asset comes back as an
//! [`AssetRecord`] whose [`AssetStatus`] says whether the reference list was
//! read, whether it was guessed from a byte scan, or whether the bytes were
//! never available at all. Before this, an unreadable asset and an asset that
//! references nothing were the same value to every caller.

use std::collections::{BTreeSet, HashSet};
use std::fmt;

use crate::{guid, GameData};

/// OWLib's hardcoded type buckets (`TankTonka/TypeClassifications.cs`).
pub const STU_V1_TYPES: &[u16] = &[0x05A, 0x05E];
pub const STU_V2_TYPES: &[u16] = &[
    0x003, 0x013, 0x014, 0x015, 0x018, 0x01A, 0x01B, 0x01F, 0x020, 0x021, 0x024, 0x025, 0x02C,
    0x02D, 0x02E, 0x02F, 0x030, 0x031, 0x032, 0x036, 0x039, 0x03A, 0x03B, 0x045, 0x049, 0x04C,
    0x051, 0x053, 0x054, 0x055, 0x058, 0x05B, 0x05F, 0x062, 0x063, 0x065, 0x068, 0x070, 0x071,
    0x072, 0x075, 0x078, 0x079, 0x07A, 0x07F, 0x081, 0x090, 0x095, 0x096, 0x097, 0x098, 0x09D,
    0x09E, 0x09F, 0x0A2, 0x0A3, 0x0A5, 0x0A6, 0x0A8, 0x0AA, 0x0AB, 0x0AC, 0x0AD, 0x0AE, 0x0AF,
    0x0B0, 0x0B5, 0x0B7, 0x0BF, 0x0C0, 0x0C2, 0x0C5, 0x0C6, 0x0C7, 0x0C8, 0x0C9, 0x0CA, 0x0CC,
    0x0CE, 0x0CF, 0x0D0, 0x0D5, 0x0D6, 0x0D7, 0x0D9, 0x0DF, 0x0EB, 0x0EC, 0x0EE,
];
pub const CHUNKED_TYPES: &[u16] = &[0x007, 0x00C, 0x00D, 0x04A, 0x08E, 0x08F, 0x0CB];
pub const MAP_CHUNK_TYPES: &[u16] = &[0x0BC];
/// The reference's opaque-payload list, verbatim.
///
/// **This list is not the whole answer.** Type `0x051` also appears in
/// [`STU_V2_TYPES`]; the reference resolves that overlap in favour of
/// structured data. Use [`classify`], not this list, to decide what a type is.
pub const KNOWN_PAYLOAD_TYPES: &[u16] = &[
    0x004, 0x04D, 0x0B2, 0x0BB, 0x03F, 0x0B6, 0x0F1, 0x043, 0x04B, 0x050, 0x051,
];

pub fn is_stu_v1_type(t: u16) -> bool {
    STU_V1_TYPES.contains(&t)
}
pub fn is_stu_v2_type(t: u16) -> bool {
    STU_V2_TYPES.contains(&t)
}
pub fn is_chunked_type(t: u16) -> bool {
    CHUNKED_TYPES.contains(&t) || MAP_CHUNK_TYPES.contains(&t)
}

/// What kind of file a type holds.
///
/// This is the single place the reference's overlapping type lists are
/// reconciled, so no call site can get the answer wrong by checking in the
/// wrong order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKind {
    /// Structured data, version 2: every instance is walked for asset refs.
    StuV2,
    /// Structured data, version 1. The version-1 field bags are not walked;
    /// these assets are byte-scanned like any other blob.
    StuV1,
    /// An opaque payload: deliberately not read.
    Payload,
    /// Raw bytes: byte-scanned for verbatim asset GUIDs.
    ///
    /// This also covers the chunked-data types. Parsing chunked containers is
    /// `chunks.rs`'s job and this layer does not do it — a declared boundary,
    /// not an oversight. Chunked assets still get the verbatim byte scan.
    Raw,
}

/// Decide what a type is, resolving the overlap between the reference's lists.
///
/// **Why structured data wins.** The reference's dispatch is reported to test
/// the structured-data lists first and the opaque-payload list second
/// (`TankTonka/Program.cs` lines 71-73 against line 79). **That ordering was
/// given to this project, not read out of the reference here** — the checkout
/// is not in the tree, so treat the line numbers as a pointer to re-derive
/// rather than as a citation. What *is* measured, on this install, is the
/// effect of getting it backwards: type `0x051` is in both lists, and when the
/// payload test was consulted first the function returned early and the type
/// emitted no record at all. Its assets were invisible rather than reported as
/// empty.
///
/// Putting the ordering here, rather than in each caller, is what stops it
/// drifting again.
pub fn classify(t: u16) -> AssetKind {
    if is_stu_v2_type(t) {
        return AssetKind::StuV2;
    }
    if is_stu_v1_type(t) {
        return AssetKind::StuV1;
    }
    if KNOWN_PAYLOAD_TYPES.contains(&t) {
        return AssetKind::Payload;
    }
    AssetKind::Raw
}

/// Whether a type is an opaque payload *after* the overlap is resolved.
///
/// This used to be a bare lookup in [`KNOWN_PAYLOAD_TYPES`], which is why
/// `0x051` was skipped everywhere it was consulted. It now answers the
/// question the caller actually means.
pub fn is_known_payload_type(t: u16) -> bool {
    matches!(classify(t), AssetKind::Payload)
}

pub type RefSet = BTreeSet<u64>;

#[derive(Debug, Clone, Default)]
pub struct StructuredDataInfo {
    pub instances: Option<Vec<u32>>,
}

#[derive(Debug, Clone, Default)]
pub struct ChunkedDataInfo {
    pub chunks: Option<Vec<u32>>,
}

/// How one asset's reference list came to be.
///
/// Only [`AssetStatus::Stu`] and [`AssetStatus::BlobScan`] mean "this is the
/// asset's reference list". The other variants mean we do not know it, and a
/// caller that treats an empty list as a fact will be wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetStatus {
    /// Read as structured data. An empty reference list here is a fact about
    /// the asset, not a failure to read it.
    Stu,
    /// Not structured data; the raw bytes were scanned for asset GUIDs.
    /// An empty list means the scan found nothing, which is a weaker statement
    /// than the structured-data one but still a real result.
    BlobScan,
    /// Structured data did not parse, so the raw bytes were scanned instead.
    /// The list is the scan's guess at what the asset points at, not the
    /// asset's own answer. Kept distinct from [`AssetStatus::BlobScan`] because
    /// this one is a fallback that may have happened for no good reason.
    StuParseFailed(String),
    /// The bytes are not on this machine, or would not decode. There is no
    /// reference list at all: anything empty here means nothing was read.
    Unreadable(String),
    /// An opaque-payload type. Deliberately not read. This is a fact about the
    /// type rather than a failure of this machine.
    PayloadSkipped,
}

impl AssetStatus {
    /// Whether the reference list is the asset's own answer rather than a
    /// fallback or a non-answer.
    pub fn is_complete(&self) -> bool {
        matches!(self, AssetStatus::Stu | AssetStatus::BlobScan)
    }

    /// A short machine-readable name, used in the exported JSON.
    pub fn kind_str(&self) -> &'static str {
        match self {
            AssetStatus::Stu => "stu",
            AssetStatus::BlobScan => "blob-scan",
            AssetStatus::StuParseFailed(_) => "stu-parse-failed",
            AssetStatus::Unreadable(_) => "unreadable",
            AssetStatus::PayloadSkipped => "payload-skipped",
        }
    }

    /// Why, when there is a why.
    pub fn detail(&self) -> Option<&str> {
        match self {
            AssetStatus::StuParseFailed(m) | AssetStatus::Unreadable(m) => Some(m),
            _ => None,
        }
    }
}

impl fmt::Display for AssetStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.detail() {
            Some(detail) => write!(f, "{}: {detail}", self.kind_str()),
            None => write!(f, "{}", self.kind_str()),
        }
    }
}

impl std::error::Error for AssetStatus {}

/// One asset and everything we could learn about it.
///
/// `references` is only the asset's own answer when
/// [`AssetRecord::status`] says so. Check [`AssetStatus::is_complete`] before
/// drawing a conclusion from an empty set.
#[derive(Debug, Clone)]
pub struct AssetRecord {
    pub guid: u64,
    pub references: RefSet,
    pub structured_data_info: Option<StructuredDataInfo>,
    pub chunked_data_info: Option<ChunkedDataInfo>,
    pub status: AssetStatus,
    /// Candidate values the structured-data walk produced that are **not** in
    /// this installation's registry, so were not reported as references.
    ///
    /// **This is the cost of the membership check, counted.** It is the number
    /// of values we could not prove were references and could not prove were
    /// not. It is *not* the number of legitimate references we lost, which
    /// nobody can count from here — that is what the comment on the module says.
    pub dropped_by_membership: usize,
}

/// One type's worth of results: the records, plus the assets that could not be
/// read and the ones that only produced a fallback answer.
#[derive(Debug, Clone, Default)]
pub struct TypeRecords {
    pub type_id: u16,
    pub records: Vec<AssetRecord>,
    /// Assets whose bytes are not on this machine, or would not decode. These
    /// are genuinely missing, not empty.
    pub unreadable: Vec<u64>,
    /// Assets that did not parse as structured data and were byte-scanned
    /// instead. Their reference lists are the scan's guess.
    pub stu_fallback: Vec<u64>,
}

impl TypeRecords {
    /// Records that were really read, with their reference lists intact.
    pub fn complete_records(&self) -> impl Iterator<Item = &AssetRecord> {
        self.records.iter().filter(|r| r.status.is_complete())
    }
}

#[derive(Debug, Clone, Default)]
pub struct TypeManifest {
    pub type_id: u16,
    pub structured_data_info: Option<StructuredDataInfo>,
    pub chunked_data_info: Option<ChunkedDataInfo>,
    pub guid_reference_types: HashSet<u16>,
}

pub fn guid_string(guid: u64) -> String {
    format!(
        "{:012x}.{:03x}",
        guid & 0x0000_FFFF_FFFF_FFFF,
        guid::type_of(guid)
    )
}

/// Every GUID in the registry — the membership set a candidate must be in to
/// count as a reference.
///
/// **Build this once per [`GameData`] and pass it down.** It holds a little
/// over a million entries and rebuilding it inside a per-asset loop charged
/// every asset the same flat toll whatever its size: measured at 64-73 ms per
/// asset across files from 366 bytes to 288 KB, with no trend at all. The
/// functions below all take it as a parameter for that reason.
pub fn known_guids(data: &GameData) -> HashSet<u64> {
    data.registry().records.iter().map(|r| r.guid).collect()
}

pub fn blob_references_with(bytes: &[u8], known: &HashSet<u64>) -> RefSet {
    let mut out = RefSet::new();
    if bytes.len() < 8 {
        return out;
    }
    for i in 0..=bytes.len() - 8 {
        let v = u64::from_le_bytes(bytes[i..i + 8].try_into().unwrap());
        if known.contains(&v) {
            out.insert(v);
        }
    }
    out
}

/// What a structured-data walk produced.
#[derive(Debug, Clone, Default)]
pub struct StuRefs {
    /// Values that are GUIDs in this installation's registry.
    pub references: RefSet,
    /// Values the walk produced that are not in the registry, so were dropped.
    pub dropped: usize,
}

/// Walk a parsed structured-data file for asset references.
///
/// Two values come out of every eight-byte field: the deobfuscated one (which
/// is what a typed asset-reference field holds) and the raw one (an ordinary
/// integer field that happens to look like a GUID). Both are checked against
/// `known` before being reported.
///
/// Before this check existed the parameter was named `_known` and ignored, so
/// both paths accumulated whatever 64-bit number the file happened to contain.
/// That is measurable: on a hero asset, 333 of 1,972 reported references were
/// values whose type no asset on the installation occupies; on a statescript
/// graph, 165,397 of 252,031.
pub fn stu_references_with(stu: &mut ow_stu::Stu, known: &HashSet<u64>) -> StuRefs {
    let mut out = StuRefs::default();
    let n = stu.instances.len();
    for i in 0..n {
        let fields: Vec<_> = stu.fields(i).to_vec();
        let base = stu.payload_start(i);
        for f in fields {
            if f.size != 8 {
                continue;
            }
            let at = base + f.offset;
            // Obfuscated asset reference fields are u64. Mirror OWLib's typed
            // AssetRef by only considering 8-byte fields.
            if let Some(v) = stu.asset_ref_at(at, f.hash) {
                if v != 0 {
                    if known.contains(&v) {
                        out.references.insert(v);
                    } else {
                        out.dropped += 1;
                    }
                }
            }
            // Raw integer field that itself looks like a GUID. The type range
            // test is kept as a cheap first pass, but it is not what decides:
            // membership in the registry is, because a plausible type number is
            // not evidence that the value is a real asset.
            if let Some(v) = stu.u64_at(at) {
                let ty = guid::type_of(v);
                if v != 0 && ty > 1 && ty < 0xFF {
                    if known.contains(&v) {
                        out.references.insert(v);
                    } else {
                        out.dropped += 1;
                    }
                }
            }
        }
    }
    out
}

/// Read one asset's references from bytes already in hand.
///
/// The type comes from the GUID, the same way every other layer reads it, so
/// this cannot be pointed at a type its GUID does not carry.
pub fn scan_asset_bytes(guid: u64, bytes: &[u8], known: &HashSet<u64>) -> AssetRecord {
    let kind = classify(guid::type_of(guid));
    let mut record = AssetRecord {
        guid,
        references: RefSet::new(),
        structured_data_info: None,
        chunked_data_info: None,
        // Set properly below; a placeholder so the struct is built in one place.
        status: AssetStatus::PayloadSkipped,
        dropped_by_membership: 0,
    };

    match kind {
        AssetKind::Payload => {
            record.status = AssetStatus::PayloadSkipped;
        }
        AssetKind::StuV2 => match ow_stu::Stu::parse(bytes.to_vec()) {
            Ok(mut stu) => {
                let got = stu_references_with(&mut stu, known);
                record.references = got.references;
                record.dropped_by_membership = got.dropped;
                record.status = AssetStatus::Stu;
                record.structured_data_info = Some(StructuredDataInfo { instances: None });
            }
            Err(e) => {
                // Fall back to the byte scan, and say so. The old code took
                // this same fallback silently and left `stu_info` null, which
                // made a parse failure look exactly like "not a structured
                // data asset".
                record.references = blob_references_with(bytes, known);
                record.status = AssetStatus::StuParseFailed(e.to_string());
            }
        },
        AssetKind::StuV1 | AssetKind::Raw => {
            record.references = blob_references_with(bytes, known);
            record.status = AssetStatus::BlobScan;
        }
    }

    // OWLib leaves ChunkedDataInfo null for these records.
    record.chunked_data_info = None;

    record
}

/// Read one asset's references from the installation.
///
/// An asset whose bytes are not available comes back with
/// [`AssetStatus::Unreadable`] and an empty reference set — which is a
/// statement that nothing was read, not that nothing was there. There is no
/// `Result` here on purpose: every way this can fail is a status, so no caller
/// can quietly turn a failure into an empty list.
pub fn process_asset(data: &GameData, guid: u64, known: &HashSet<u64>) -> AssetRecord {
    match data.bytes_of(guid) {
        Ok(bytes) => scan_asset_bytes(guid, &bytes, known),
        Err(e) => AssetRecord {
            guid,
            references: RefSet::new(),
            structured_data_info: None,
            chunked_data_info: None,
            status: AssetStatus::Unreadable(e.to_string()),
            dropped_by_membership: 0,
        },
    }
}

/// Read every asset of one type.
pub fn build_asset_records(data: &GameData, type_id: u16, known: &HashSet<u64>) -> TypeRecords {
    let mut out = TypeRecords {
        type_id,
        ..Default::default()
    };
    if !is_known_payload_type(type_id) {
        for guid in data.guids_of_type(type_id) {
            let record = process_asset(data, guid, known);
            match record.status {
                AssetStatus::Unreadable(_) => out.unreadable.push(guid),
                AssetStatus::StuParseFailed(_) => out.stu_fallback.push(guid),
                _ => {}
            }
            out.records.push(record);
        }
    }
    out
}

/// Render one record as JSON.
///
/// **The first four fields are the reference's output, byte for byte:**
/// `guid`, `references`, `stu_info`, `chunked_info`, in that order, with that
/// spacing. They are kept identical on purpose so a file here can be diffed
/// against one from the reference tool.
///
/// Everything after them is ours. `status` and `dropped_by_membership` do not
/// exist in the reference's output; they are here so that a consumer can tell
/// a genuinely empty reference list from an unreadable asset, and can see what
/// the membership check cost.
pub fn asset_record_json(record: &AssetRecord) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str(&format!("  \"guid\": \"{}\",\n", guid_string(record.guid)));
    out.push_str("  \"references\": [");
    let refs: Vec<String> = record
        .references
        .iter()
        .map(|g| format!("\"{}\"", guid_string(*g)))
        .collect();
    if refs.is_empty() {
        out.push_str("],\n");
    } else {
        out.push_str("\n    ");
        out.push_str(&refs.join(", "));
        out.push_str("\n  ],\n");
    }
    // OWLib emits stu_info for STU assets with instances null.
    match &record.structured_data_info {
        Some(info) => {
            out.push_str("  \"stu_info\": {\n    \"instances\": ");
            match &info.instances {
                Some(vals) => {
                    let vals: Vec<String> = vals.iter().map(|v| v.to_string()).collect();
                    out.push_str(&format!("[{}]", vals.join(", ")));
                }
                None => out.push_str("null"),
            }
            out.push_str("\n  },\n");
        }
        None => out.push_str("  \"stu_info\": null,\n"),
    }
    match &record.chunked_data_info {
        Some(info) => {
            out.push_str("  \"chunked_info\": {\n    \"chunks\": ");
            match &info.chunks {
                Some(vals) => {
                    let vals: Vec<String> = vals.iter().map(|v| v.to_string()).collect();
                    out.push_str(&format!("[{}]", vals.join(", ")));
                }
                None => out.push_str("null"),
            }
            // Comma, because our own fields follow.
            out.push_str("\n  },\n");
        }
        None => out.push_str("  \"chunked_info\": null,\n"),
    }
    // Ours, not the reference's.
    match record.status.detail() {
        Some(detail) => out.push_str(&format!(
            "  \"status\": {{ \"kind\": \"{}\", \"detail\": {} }},\n",
            record.status.kind_str(),
            json_string(detail)
        )),
        None => out.push_str(&format!(
            "  \"status\": {{ \"kind\": \"{}\" }},\n",
            record.status.kind_str()
        )),
    }
    out.push_str(&format!(
        "  \"dropped_by_membership\": {}\n",
        record.dropped_by_membership
    ));
    out.push_str("}\n");
    out
}

/// Quote a string as a JSON string literal. The detail text is an error
/// message from another layer, so it can contain anything, including quotes
/// and backslashes.
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn write_named_missing_json(missing: &[u64]) -> String {
    let mut out = String::from("[\n");
    let entries: Vec<String> = missing
        .iter()
        .map(|g| format!("  \"{}\"", guid_string(*g)))
        .collect();
    out.push_str(&entries.join(",\n"));
    out.push_str("\n]\n");
    out
}

pub fn all_type_ids(data: &GameData) -> Vec<u16> {
    let mut types: Vec<u16> = data.type_histogram().keys().copied().collect();
    types.sort_unstable();
    types
}

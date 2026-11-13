//! The practice-range map header, opened through the client's own asset path.
//!
//! proof-covers: 0x141fd5820
//!
//! The header is not in the encoded store: its CMF record byte is `132`
//! (`0x84`), so the payload lives inside a type-`0x09C` bundle. This test opens
//! it with [`GameData::open_asset`] — the record byte decides, the bundle entry
//! table is read as `FUN_141fd5820` reads it — and then checks the parsed
//! `STUMapHeader` and the map → chunk chain.
//!
//! No GUID list is used as input. The header names the map asset; the map's
//! chunks are the type-`0x0BC` records whose low 32 bits are that asset's
//! index, and each one's kind is checked against the kind table read out of the
//! installed executable and against the client's GUID formula. The constants
//! below are *expected* values, not seeds.
//!
//! Skipped when no install is present. Read-only.
//!
//! ```text
//! cargo test --release -p ow-tank --test map_header_bundle_real -- --nocapture
//! ```

use ow_stu::Stu;
use ow_tank::keytable::Sources;
use ow_tank::manifest::Record;
use ow_tank::{bundle, guid, mapguid, GameData, ManifestSelector};

/// `STUMapHeader`: the class registered at `0x14031c580`, size `0x240`.
const STU_MAP_HEADER: u32 = 0x0251_4B6A;
/// `mapName` on `STUMapHeader`.
const FIELD_MAP_NAME: u32 = 0x506F_A8D8;
/// The selected map (a type-`0x002` asset) on `STUMapHeader`.
const FIELD_MAP: u32 = 0x4E87_690F;

/// The practice-range header: type `0x09F`, 1132 bytes, record byte `132`.
const PRACTICE_HEADER: u64 = 0x0790_0000_0000_0688;
/// The map asset that header names (1056 bytes).
const PRACTICE_MAP: u64 = 0x0800_0000_0000_0688;
const PRACTICE_MAP_NAME: &str = "Maps\\TrainingRoom\\Training Room - Practice";

/// The two chunk assets the manifest registers under the practice map's index.
const PRACTICE_CHUNKS: [u64; 2] = [0x0DD0_0011_0000_0688, 0x0DD0_0012_0000_0688];

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

fn record(data: &GameData, guid: u64) -> Record {
    *data
        .registry()
        .get(guid)
        .unwrap_or_else(|| panic!("0x{guid:016x} is not in the manifest"))
}

#[test]
fn practice_header_opens_through_the_client_bundle_path() {
    let Some(data) = open() else {
        return;
    };

    // -----------------------------------------------------------------------
    // The record says where the payload is, and it is a bundle.
    // -----------------------------------------------------------------------
    let header_record = record(&data, PRACTICE_HEADER);
    assert_eq!(
        guid::type_of(PRACTICE_HEADER),
        0x09F,
        "tool type of the header"
    );
    assert_eq!(header_record.size, 1132, "the CMF record's declared size");
    assert_eq!(
        header_record.unknown & 0x7F,
        4,
        "the record's low class bits"
    );
    assert_ne!(
        header_record.unknown & bundle::RECORD_IN_BUNDLE,
        0,
        "the header's payload is in a bundle (record byte {})",
        header_record.unknown
    );

    // -----------------------------------------------------------------------
    // The client's open path: entry table, offset, declared size.
    // -----------------------------------------------------------------------
    let bytes = data
        .open_asset(PRACTICE_HEADER)
        .expect("the practice header opens through its bundle");
    assert_eq!(bytes.len(), header_record.size as usize);

    let index = data.bundle_index().expect("a bundle index");
    assert!(
        index.malformed.is_empty(),
        "every opened bundle payload is a bundle table; {} are not",
        index.malformed.len()
    );
    let location = index
        .get(PRACTICE_HEADER)
        .expect("the header is an entry of a type-0x09C bundle");
    let payload = data.bytes_of(location.bundle).expect("the bundle payload");
    let parsed = bundle::Bundle::parse(&payload).expect("the bundle table parses");
    assert_eq!(
        parsed.offset_of(PRACTICE_HEADER),
        Some(location.offset),
        "the index and the table must agree"
    );
    assert_eq!(
        parsed.span(location.offset, header_record.size).unwrap(),
        &bytes[..],
        "the payload slice is the asset's bytes"
    );

    // -----------------------------------------------------------------------
    // It parses as STUMapHeader with the expected name and map.
    // -----------------------------------------------------------------------
    let mut stu = Stu::parse(bytes.clone()).expect("the slice is an STU asset");
    let instance = stu
        .instance(STU_MAP_HEADER)
        .expect("class 0x02514B6A (STUMapHeader) is present");
    assert_eq!(
        stu.read_string(instance, FIELD_MAP_NAME).as_deref(),
        Some(PRACTICE_MAP_NAME)
    );
    assert_eq!(
        stu.read_asset_ref(instance, FIELD_MAP),
        Some(PRACTICE_MAP),
        "the header names the map asset"
    );

    // -----------------------------------------------------------------------
    // The map asset is readable; on this install it is bundle-hosted too.
    // -----------------------------------------------------------------------
    let map_record = record(&data, PRACTICE_MAP);
    assert_eq!(guid::type_of(PRACTICE_MAP), 0x002);
    assert_eq!(map_record.size, 1056);
    let map_bytes = data.open_asset(PRACTICE_MAP).expect("the map asset opens");
    assert_eq!(map_bytes.len(), map_record.size as usize);

    // -----------------------------------------------------------------------
    // The chunk set: the map's index under the image's own kind table.
    // -----------------------------------------------------------------------
    let sources = Sources::discover(std::slice::from_ref(
        &ow_casc::install::detect_root().expect("the install root"),
    ));
    let kinds = sources
        .chunk_kinds()
        .expect("the client's chunk-kind table is readable");
    assert_eq!(kinds.len(), 0x24);
    // Spot-check the table against the image dump: slots 29 and 30 are the
    // kinds of the two chunks this map ships.
    assert_eq!(kinds[29], 0x11);
    assert_eq!(kinds[30], 0x12);

    // A map's chunks are the assets whose low 32 bits are the map asset's index.
    // Enumerating them by that index is derived from the header's `m_map`, not a
    // list: the map does not name its chunks anywhere.
    let map_index = (PRACTICE_MAP & 0xFFFF_FFFF) as u32;
    let mut local: Vec<u64> = data
        .registry()
        .records
        .iter()
        .map(|r| r.guid)
        .filter(|g| guid::type_of(*g) == 0x0BC && (*g & 0xFFFF_FFFF) as u32 == map_index)
        .collect();
    local.sort_unstable();
    assert_eq!(
        local, PRACTICE_CHUNKS,
        "the manifest registers exactly two chunk assets under the map's index"
    );

    // Each chunk's kind byte is a slot in the client's table, and the formula
    // reproduces its GUID from that slot.
    for chunk in &local {
        let kind = (chunk >> 32) & 0xFF;
        let slot = mapguid::slot_of_kind(&kinds, kind).unwrap_or_else(|| {
            panic!("chunk 0x{chunk:016x}: kind 0x{kind:02x} is not in the table")
        });
        assert_eq!(mapguid::chunk_guid(PRACTICE_MAP, kinds[slot]), *chunk);
        assert!(
            !mapguid::loader_type_indices().any(|i| i == slot),
            "slot {slot} (kind 0x{kind:02x}) is outside the map loader's fixed 31"
        );
    }

    // The loader's own 31 indices produce no local asset for this map; report it
    // rather than pretend they are the whole set.
    let loader_set = mapguid::chunk_guids(PRACTICE_MAP, &kinds);
    assert_eq!(loader_set.len(), 31, "the loader asks for 31 chunk indices");
    assert!(
        loader_set.iter().all(|g| data.registry().get(*g).is_none()),
        "this map's chunks are not among the loader's fixed set"
    );

    // How many of the local chunks open, and how their placeable walk ends.
    let mut opened = 0usize;
    let mut placeables = 0usize;
    let mut kind_counts = std::collections::BTreeMap::new();
    for chunk in &local {
        let chunk_record = record(&data, *chunk);
        assert_eq!(
            chunk_record.unknown & bundle::RECORD_IN_BUNDLE,
            0,
            "chunk 0x{chunk:016x} is in the encoded store (record byte {})",
            chunk_record.unknown
        );
        let bytes = data
            .open_asset(*chunk)
            .unwrap_or_else(|e| panic!("chunk 0x{chunk:016x} did not open: {e}"));
        assert_eq!(bytes.len(), chunk_record.size as usize);
        let kind_id = ((chunk >> 32) & 0xFF) as u16;
        // The synthesised kind selects the load policy, not the placeable body
        // kind; the size walk is kind-independent, so it is read as Unknown.
        let parsed = ow_tank::mapchunk::MapChunk::parse(&bytes, ow_tank::mapchunk::Kind::Unknown)
            .unwrap_or_else(|e| panic!("chunk 0x{chunk:016x} did not parse: {e}"));
        assert!(
            parsed.placeables.len() <= parsed.header.placeable_count as usize,
            "chunk 0x{chunk:016x}: the walk cannot invent placeables"
        );
        placeables += parsed.placeables.len();
        *kind_counts.entry(kind_id).or_insert(0usize) += 1;
        opened += 1;
    }
    assert_eq!(opened, PRACTICE_CHUNKS.len());

    eprintln!(
        "practice header 0x{PRACTICE_HEADER:016x}: {} B, bundle 0x{:016x} at offset {} \
         (record byte {}); map 0x{PRACTICE_MAP:016x} {} B (record byte {}); bundle index: \
         {} bundles, {} unreadable, {} malformed, {} entries; {} of {} chunks under the map \
         index local and open, {} placeables, kinds {:?}; loader's 31 slots: 0 local; \
         {} duplicate entry rows",
        bytes.len(),
        location.bundle,
        location.offset,
        header_record.unknown,
        map_bytes.len(),
        map_record.unknown,
        index.bundles,
        index.unreadable.len(),
        index.malformed.len(),
        index.entries_seen,
        opened,
        local.len(),
        placeables,
        kind_counts,
        index.duplicates,
    );

    // -----------------------------------------------------------------------
    // The whole header set, through the same path: 42 store + 263 bundle.
    // -----------------------------------------------------------------------
    let headers: Vec<u64> = data.guids_of_type(0x09F);
    assert_eq!(headers.len(), 305, "the install registers 305 map headers");
    let mut from_store = 0usize;
    let mut from_bundle = 0usize;
    let mut parsed = 0usize;
    let mut named = 0usize;
    for header in &headers {
        let r = record(&data, *header);
        if r.unknown & bundle::RECORD_IN_BUNDLE != 0 {
            from_bundle += 1;
        } else {
            from_store += 1;
        }
        let bytes = data
            .open_asset(*header)
            .unwrap_or_else(|e| panic!("header 0x{header:016x} did not open: {e}"));
        assert_eq!(
            bytes.len(),
            r.size as usize,
            "header 0x{header:016x} length"
        );
        let mut stu = Stu::parse(bytes).unwrap_or_else(|e| panic!("0x{header:016x}: {e}"));
        let Some(instance) = stu.instance(STU_MAP_HEADER) else {
            panic!("header 0x{header:016x} does not carry class 0x02514B6A");
        };
        parsed += 1;
        if stu.read_string(instance, FIELD_MAP_NAME).is_some() {
            named += 1;
        }
    }
    assert_eq!(
        from_store, 42,
        "headers whose payload is in the encoded store"
    );
    assert_eq!(from_bundle, 263, "headers whose payload is in a bundle");
    assert_eq!(parsed, 305, "every header parses as STUMapHeader");
    assert_eq!(named, 305, "every header names its map");
    eprintln!(
        "map headers: {parsed}/305 parse as class 0x02514B6A ({from_store} store, \
         {from_bundle} bundle), {named} with a mapName"
    );
}

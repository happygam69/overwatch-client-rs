//! The kind table decides which chunk kind is collision, and the collision
//! chunk parses as the client's sector container.
//!
//! The first half needs no install: the 0x24 names, the slot ids and the
//! fixup's arithmetic are all read from the image and pinned here. The second
//! half opens the practice range's `0x0DD0001100000688` and pins the header,
//! the sector pointers, the tail pointers and the tiling, and is skipped when
//! the install is not readable.
//!
//! ```text
//! cargo test --release -p ow-tank --test chunkkind_real -- --nocapture
//! ```

use ow_tank::chunkkind::{
    self, kind_name, kind_of_guid, slot_for_kind, Collision, Sector, HEADER_SIZE, SECTOR_SIZE,
    TABLE, TAIL_SIZE,
};
use ow_tank::keytable::Sources;
use ow_tank::{GameData, ManifestSelector};

/// The practice range's `MAP_CHUNK_COLLISION` chunk (the base map's `+0x1B8`).
const PRACTICE_COLLISION: u64 = 0x0DD0_0011_0000_0688;
/// The practice range's `MAP_CHUNK_PATHING` chunk (the base map's `+0x1C0`).
const PRACTICE_PATHING: u64 = 0x0DD0_0012_0000_0688;

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

// ---------------------------------------------------------------- kind table

#[test]
fn the_two_collision_kinds_are_0x11_and_0x10() {
    // The client's own names, decoded from the table's name blobs.
    assert_eq!(kind_name(PRACTICE_COLLISION), Some("MAP_CHUNK_COLLISION"));
    assert_eq!(kind_name(PRACTICE_PATHING), Some("MAP_CHUNK_PATHING"));
    assert_eq!(
        kind_name(0x0DD0_0010_0000_0688),
        Some("teMAP_CHUNK_ADDITIVE_COLLISION")
    );
    assert_eq!(
        kind_name(0x0DD0_0023_0000_0688),
        Some("teMAP_CHUNK_ADDITIVE_PATHING")
    );
    assert_eq!(
        kind_name(0x0DD0_0013_0000_0688),
        Some("MAP_CHUNK_SOUNDAREA")
    );
    assert_eq!(
        kind_name(0x0DD0_001B_0000_0688),
        Some("MAP_CHUNK_DYNAMIC_SPAWN_GROUPS")
    );
    assert_eq!(
        kind_name(0x0DD0_0027_0000_0688),
        Some("teMAP_CHUNK_STREAMING")
    );

    // The slot is what the map dispatch switches on.
    assert_eq!(slot_for_kind(chunkkind::KIND_COLLISION), Some(0x1D));
    assert_eq!(slot_for_kind(chunkkind::KIND_PATHING), Some(0x1E));
    assert_eq!(slot_for_kind(chunkkind::KIND_SOUNDAREA), Some(0x1F));
    assert_eq!(
        slot_for_kind(chunkkind::KIND_DYNAMIC_SPAWN_GROUPS),
        Some(0x20)
    );
    assert_eq!(
        slot_for_kind(chunkkind::KIND_ADDITIVE_COLLISION),
        Some(0x21)
    );
    assert_eq!(slot_for_kind(chunkkind::KIND_ADDITIVE_PATHING), Some(0x22));
    assert_eq!(slot_for_kind(chunkkind::KIND_STREAMING), Some(0x23));

    assert_eq!(kind_of_guid(PRACTICE_COLLISION), 0x11);
    assert_eq!(kind_of_guid(PRACTICE_PATHING), 0x12);
}

#[test]
fn the_table_holds_the_placeable_names_this_project_already_knew() {
    // The decode's own check: eight placeable names were established from the
    // install before the table was decoded, and the decoded slots must carry
    // them. A wrong key or a wrong length would break these first.
    for (slot, id, name) in [
        (0x01u8, 0x01u8, "MAP_PLACEABLE_MODEL_GROUP"),
        (0x03, 0x04, "MAP_PLACEABLE_REFLECTIONPOINT"),
        (0x05, 0x07, "MAP_PLACEABLE_TEXT"),
        (0x06, 0x09, "MAP_PLACEABLE_LIGHT"),
        (0x07, 0x0A, "MAP_PLACEABLE_AREA"),
        (0x08, 0x0B, "MAP_PLACEABLE_ENTITY"),
        (0x09, 0x0C, "MAP_PLACEABLE_SOUND"),
        (0x0A, 0x0D, "MAP_PLACEABLE_EFFECT"),
    ] {
        let entry = TABLE.iter().find(|e| e.slot == slot).expect("slot");
        assert_eq!(entry.id, id, "slot {slot:#04x} id");
        assert_eq!(entry.name, name, "slot {slot:#04x} name");
    }
    // Every slot is present once and every id is distinct apart from slot 0.
    for (i, entry) in TABLE.iter().enumerate() {
        assert_eq!(entry.slot as usize, i, "slot order");
        assert!(!entry.name.is_empty(), "slot {i:#04x} has a name");
    }
    let mut ids: Vec<u8> = TABLE.iter().map(|e| e.id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), TABLE.len(), "the kind ids are distinct");
}

// -------------------------------------------------------------------- fixup

/// A synthetic payload with one sector and one tail element, laid out so that
/// every converted field points somewhere recognisable.
fn synthetic(count_a: i32, count_b: i32) -> Vec<u8> {
    let mut buf = vec![0u8; 0xC8];
    buf[0x08..0x0C].copy_from_slice(&count_a.to_le_bytes());
    buf[0x0C..0x10].copy_from_slice(&count_b.to_le_bytes());
    // +0x10 stays zero -- the client's null test.
    buf[0x18..0x20].copy_from_slice(&0x0040u64.to_le_bytes());
    buf[0x20..0x28].copy_from_slice(&(HEADER_SIZE as u64).to_le_bytes());
    buf[0x28..0x30].copy_from_slice(&((HEADER_SIZE + SECTOR_SIZE) as u64).to_le_bytes());
    let sector = HEADER_SIZE;
    for (field, rva) in [(0x00usize, 0u64), (0x08, 0x0100), (0x10, 0x0200), (0x18, 0)] {
        buf[sector + field..sector + field + 8].copy_from_slice(&rva.to_le_bytes());
    }
    let tail = HEADER_SIZE + SECTOR_SIZE;
    for (field, rva) in [(0x00usize, 0u64), (0x08, 0x0040), (0x10, 0)] {
        buf[tail + field..tail + field + 8].copy_from_slice(&rva.to_le_bytes());
    }
    buf
}

fn qword(buf: &[u8], at: usize) -> u64 {
    let mut b = [0u8; 8];
    b.copy_from_slice(&buf[at..at + 8]);
    u64::from_le_bytes(b)
}

#[test]
fn the_fixup_keeps_a_zero_header_rva_null_and_adds_to_every_element_qword() {
    let mut buf = synthetic(1, 1);
    chunkkind::fixup_in_place(&mut buf, 0x1000).expect("the synthetic payload fixes up");
    // Header: zero stays zero, the rest gets the base.
    assert_eq!(qword(&buf, 0x10), 0, "+0x10 was zero");
    assert_eq!(qword(&buf, 0x18), 0x1040, "+0x18");
    assert_eq!(qword(&buf, 0x20), 0x1038, "+0x20");
    assert_eq!(qword(&buf, 0x28), 0x1090, "+0x28");
    // Sector element: unconditional, so a zero becomes the base.
    let sector = HEADER_SIZE;
    assert_eq!(qword(&buf, sector), 0x1000, "sector +0x00");
    assert_eq!(qword(&buf, sector + 0x08), 0x1100, "sector +0x08");
    assert_eq!(qword(&buf, sector + 0x10), 0x1200, "sector +0x10");
    assert_eq!(qword(&buf, sector + 0x18), 0x1000, "sector +0x18");
    // Tail element: three qwords, unconditional as well.
    let tail = HEADER_SIZE + SECTOR_SIZE;
    assert_eq!(qword(&buf, tail), 0x1000, "tail +0x00");
    assert_eq!(qword(&buf, tail + 0x08), 0x1040, "tail +0x08");
    assert_eq!(qword(&buf, tail + 0x10), 0x1000, "tail +0x10");
    assert_eq!(qword(&buf, tail + 0x18), 0, "tail +0x18 is not converted");
}

#[test]
fn a_non_positive_count_runs_no_pass() {
    // The client's loops compare a signed counter against the count, so zero
    // and negative counts touch no element.
    let mut buf = synthetic(0, 0);
    chunkkind::fixup_in_place(&mut buf, 0x1000).expect("fixup");
    let sector = HEADER_SIZE;
    assert_eq!(qword(&buf, sector + 0x08), 0x0100, "sector untouched");
    let tail = HEADER_SIZE + SECTOR_SIZE;
    assert_eq!(qword(&buf, tail + 0x08), 0x0040, "tail untouched");
    // A negative count is refused at parse time, and the fixup agrees.
    let mut negative = synthetic(-1, 0);
    assert!(Collision::parse(&negative).is_err());
    chunkkind::fixup_in_place(&mut negative, 0x1000).expect("fixup refuses to loop");
    assert_eq!(qword(&negative, sector + 0x08), 0x0100);
}

#[test]
fn a_truncated_payload_is_refused() {
    assert!(Collision::parse(&[0u8; 8]).is_err());
    assert!(Collision::parse(&[0u8; HEADER_SIZE]).is_ok());
    // A count that runs past the payload is refused.
    let mut buf = synthetic(2, 1);
    assert!(Collision::parse(&buf).is_err());
    buf[0x08..0x0C].copy_from_slice(&1i32.to_le_bytes());
    assert!(Collision::parse(&buf).is_ok());
}

// ------------------------------------------------------------------ install

#[test]
fn the_practice_range_collision_chunk_parses_as_36_sectors() {
    let Some(data) = open() else {
        return;
    };
    let bytes = data
        .open_asset(PRACTICE_COLLISION)
        .expect("the practice range's collision chunk opens");
    assert_eq!(bytes.len(), 21_858_688, "the chunk's stored length");
    assert_eq!(
        kind_name(PRACTICE_COLLISION),
        Some("MAP_CHUNK_COLLISION"),
        "the chunk's declared kind"
    );

    let collision = Collision::parse(&bytes).expect("the collision payload parses");
    let h = collision.header;
    assert_eq!(h.unknown_00, 0, "+0x00");
    assert_eq!(h.count_a, 36, "+0x08: the sector count");
    assert_eq!(h.count_b, 1, "+0x0C: the tail count");
    assert_eq!(h.rva_10, 0, "+0x10 is an unset RVA");
    assert_eq!(h.rva_18, 0, "+0x18 is an unset RVA");
    assert_eq!(h.rva_20, 0x14D_3810, "+0x20: the sector array");
    assert_eq!(h.rva_28, 0x14D_8948, "+0x28: the tail array");
    assert_eq!(h.unknown_30, u64::MAX, "+0x30");
    assert_eq!(
        h.rva_28 + TAIL_SIZE as u64,
        bytes.len() as u64,
        "array B ends the payload"
    );

    let sectors: Vec<Sector<'_>> = collision.sectors().collect();
    assert_eq!(sectors.len(), 36);
    eprintln!(
        "collision chunk: {} B, {} sectors at {:#x}, tail at {:#x}",
        bytes.len(),
        sectors.len(),
        h.rva_20,
        h.rva_28
    );

    // Sector 0: four pointers, three counts, four runs.
    let first = &sectors[0];
    assert_eq!(
        first.pointers,
        [0x14D_4470, 0x14D_460C, 0x14D_47CC, 0x14D_45F0],
        "sector 0's four RVAs"
    );
    assert_eq!(first.counts(), [32, 28, 116], "sector 0's three counts");
    assert_eq!(
        first.runs(),
        [
            (0x14D_4470, 0x180),
            (0x14D_45F0, 0x1C),
            (0x14D_460C, 0x1C0),
            (0x14D_47CC, 0x1D0)
        ],
        "sector 0's four runs in array order"
    );

    // A small sector, and the last sector, so both ends of the range are held.
    let fourth = &sectors[4];
    assert_eq!(
        fourth.pointers,
        [0x14D_5920, 0x14D_5986, 0x14D_59E6, 0x14D_5980],
        "sector 4's four RVAs"
    );
    assert_eq!(fourth.counts(), [8, 6, 24], "sector 4's three counts");
    let last = &sectors[35];
    assert_eq!(last.pointers[0], 0x14D_87A0, "sector 35 starts");
    let end: u64 = last.runs().iter().map(|r| r.1).sum::<u64>() + last.runs()[0].0;
    assert_eq!(end, 0x14D_8948, "sector 35 ends where array B begins");

    // The tiling, over all 36 sectors.
    collision
        .check_sector_tiling()
        .expect("every sector's four runs tile up to the next sector");

    // The first vertex, bit-exact.
    let vertices = first.vertices(&bytes);
    assert_eq!(vertices.len(), 32, "sector 0's vertex count");
    assert_eq!(
        vertices[0].map(f32::to_bits),
        [0x4181_CCAE, 0x410B_47D4, 0xC074_8A94],
        "sector 0's first vertex"
    );

    // The tail element names three runs, ascending.
    let tails: Vec<chunkkind::Tail<'_>> = collision.tails().collect();
    assert_eq!(tails.len(), 1);
    assert_eq!(
        tails[0].pointers,
        [0x31_CE30, 0x40, 0xA5_81D0],
        "the tail element's three RVAs"
    );
    let p = tails[0].pointers;
    assert!(
        p[1] < p[0] && p[0] < p[2],
        "the three runs ascend and start after the header"
    );
    assert_eq!(
        p[1] as usize,
        HEADER_SIZE + 8,
        "the first run starts at 0x40"
    );
}

#[test]
fn the_practice_range_pathing_chunk_is_the_cell_container() {
    // The counterpart: the same slot family the cell fixup rewrites is the
    // pathing kind, not the collision one. Its payload is a STUD container, so
    // it does not parse as a collision header by accident.
    let Some(data) = open() else {
        return;
    };
    let bytes = data
        .open_asset(PRACTICE_PATHING)
        .expect("the practice range's pathing chunk opens");
    assert_eq!(bytes.len(), 8_792_256, "the chunk's stored length");
    assert_eq!(kind_name(PRACTICE_PATHING), Some("MAP_CHUNK_PATHING"));
    // Its header is the STUD container's: the qword at +0x20 the collision
    // reader would take for array A is inside the sub-asset blob, so the count
    // and the offsets do not resolve and the reader refuses the payload. The
    // two payloads are not interchangeable.
    assert!(
        Collision::parse(&bytes).is_err(),
        "the pathing payload is not a collision container"
    );
}

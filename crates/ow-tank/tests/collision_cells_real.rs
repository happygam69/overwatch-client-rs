//! The practice range's collision chunk parses as the client's cell container.
//!
//! The chunk is the base map's kind-`0x12` stored reference. The reader is
//! `ow_tank::collision::parse`; every boundary is checked against the payload,
//! and the first cell's layout is pinned because the fixture derived it from
//! the bytes. The test is skipped when the install is not readable.
//!
//! ```text
//! cargo test --release -p ow-tank --test collision_cells_real -- --nocapture
//! ```

use ow_tank::collision::{self, CELL_ARRAYS};
use ow_tank::keytable::Sources;
use ow_tank::{GameData, ManifestSelector};

const PRACTICE_COLLISION: u64 = 0x0DD0_0012_0000_0688;

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
fn the_practice_range_collision_chunk_parses_as_220_cells() {
    let Some(data) = open() else {
        return;
    };
    let bytes = data
        .open_asset(PRACTICE_COLLISION)
        .expect("the practice range's collision chunk opens");
    assert_eq!(bytes.len(), 8_792_256, "the chunk's stored length");

    let cells = collision::parse(&bytes).expect("the collision payload parses");
    assert_eq!(cells.class, collision::COMPONENT_CLASS);
    assert_eq!(cells.blob_offset, 0x20, "the first STUD blob");
    assert_eq!(cells.instance, 0x20, "the one v1 instance");
    assert_eq!(cells.count, 220, "the m_cellBytes count");
    assert_eq!(cells.data_offset, 0x2438, "the block-list offset");
    assert_eq!(cells.cells.len(), 220);

    // Offsets below are container-relative (the blob starts at file 0x20).
    eprintln!(
        "collision chunk: {} B, {} cells, blob at {:#x}, table at {:#x}",
        bytes.len(),
        cells.count,
        cells.blob_offset,
        cells.data_offset
    );
    let first = &cells.cells[0];
    assert_eq!(first.index, 0);
    assert_eq!(first.cell_offset, 0x31F8);
    assert_eq!(first.record_offset, 0x3208);
    assert_eq!(first.block_size, 0x67F8);
    assert_eq!(
        first.boundaries,
        [
            0xA0, 0xC8, 0x130, 0x1E0, 0x228, 0x280, 0x388, 0x648, 0x888, 0x8D0, 0x998, 0xD78,
            0x1188, 0x1548, 0x2A58, 0x5618, 0x65F8,
        ],
        "the first record's 17 boundaries"
    );
    assert_eq!(first.arrays[0].len(), 0x28, "array 0");
    let last = &cells.cells[219];
    assert_eq!(last.cell_offset, 0x84EE10);
    assert_eq!(last.record_offset, 0x84EE20);
    assert_eq!(last.block_size, 0x138E0);
    assert_eq!(last.record_offset + last.block_size, 0x862700);
    eprintln!(
        "first record {:#x} block {:#x}, last cell {:#x} ends at {:#x}",
        first.record_offset,
        first.block_size,
        last.cell_offset,
        last.record_offset + last.block_size
    );

    // Every cell's 17 arrays tile its record from the first boundary to the
    // block size, and the block list tiles the payload up to the second blob.
    let mut total_arrays = 0usize;
    for cell in &cells.cells {
        assert_eq!(cell.arrays.len(), CELL_ARRAYS);
        let mut covered = 0u64;
        for (k, array) in cell.arrays.iter().enumerate() {
            let end = if k + 1 < CELL_ARRAYS {
                cell.boundaries[k + 1]
            } else {
                cell.block_size
            };
            assert_eq!(
                array.len() as u64,
                end - cell.boundaries[k],
                "cell {}",
                cell.index
            );
            covered += array.len() as u64;
            total_arrays += 1;
        }
        assert_eq!(covered + cell.boundaries[0], cell.block_size);
    }
    assert_eq!(total_arrays, 220 * CELL_ARRAYS);
    assert_eq!(
        cells.cells[219].record_offset + cells.cells[219].block_size,
        0x862700,
        "the cells stop before the payload's second STUD blob"
    );
    assert_eq!(
        u64::from_le_bytes(bytes[0x08..0x10].try_into().unwrap()),
        0x862720,
        "the second blob's stored RVA"
    );
}

/// The client's header fixup (`MapChunkHeaderFixup`, `0x142e49360`) converts
/// the pathing chunk's header record before the STUD loader runs, so the
/// record's first slot must become the blob's pointer - which is the same
/// `0x20` the reader above uses as the blob's offset.
#[test]
fn the_header_fixup_turns_the_chunk_s_first_slot_into_the_blob_pointer() {
    let Some(data) = open() else {
        return;
    };
    let bytes = data
        .open_asset(PRACTICE_COLLISION)
        .expect("the practice range's collision chunk opens");

    const BASE: u64 = 0x1_0000;
    let mut header = bytes[..0x20].to_vec();
    ow_tank::chunkkind::fixup_header_in_place(&mut header, BASE).unwrap();
    let slot = |at: usize| u64::from_le_bytes(header[at..at + 8].try_into().unwrap());

    // The three slots the client converts, and the one it leaves alone.
    assert_eq!(slot(0x00), BASE + 0x20, "the blob's pointer");
    assert_eq!(slot(0x08), BASE + 0x862720, "the second blob's pointer");
    assert_eq!(slot(0x18), BASE + 0x862878, "the third slot's pointer");
    assert_eq!(
        slot(0x10),
        u64::from_le_bytes(bytes[0x10..0x18].try_into().unwrap()),
        "the client does not touch +0x10"
    );
    // The second and third slots name the two blobs at the very end of the
    // chunk (0x862720 and 0x862878, inside the 0x8628C0-byte payload, just
    // past the cells), and the fixup converts them like the first.
    eprintln!(
        "header slots: blob {:#x}, second {:#x}, third {:#x}, chunk {:#x}",
        slot(0x00),
        slot(0x08),
        slot(0x18),
        bytes.len()
    );
    assert!(
        0x862720 < bytes.len(),
        "the second blob is inside the chunk"
    );
    assert!(0x862878 < bytes.len(), "the third blob is inside the chunk");
    assert!(
        slot(0x08) - BASE > 0x862700,
        "the second blob is past the cells, which stop at 0x862700"
    );
}

/// The component's three array fields tile the blob exactly, so each element
/// stride follows from the tiling. Values are the practice payload's; the cell
/// table is the field `MapChunkCellBytesFixup` rewrites.
///
/// The elements themselves are pinned as raw bytes only: no client code reads
/// them (`docs/formats/maps.md`), so no field of a record is named here.
#[test]
fn the_collision_component_holds_three_tiled_arrays() {
    let Some(data) = open() else {
        return;
    };
    let bytes = data
        .open_asset(PRACTICE_COLLISION)
        .expect("the practice range's collision chunk opens");
    let tables = collision::tables(&bytes).expect("the component's arrays tile");
    assert_eq!(tables.blob_offset, 0x20, "the first STUD blob");
    assert_eq!(tables.instance, 0x20, "the one v1 instance");

    eprintln!(
        "nested {:#x}: {} x {:#x} at {:#x}; +0x58 {:#x}: {} x {:#x} at {:#x};          cells {:#x}: {} x {:#x} at {:#x}",
        tables.nested.header_offset,
        tables.nested.count,
        tables.nested.stride,
        tables.nested.data_offset,
        tables.array.header_offset,
        tables.array.count,
        tables.array.stride,
        tables.array.data_offset,
        tables.cell_bytes.header_offset,
        tables.cell_bytes.count,
        tables.cell_bytes.stride,
        tables.cell_bytes.data_offset
    );

    // The nested array inside the field at +0x08.
    assert_eq!(tables.nested.field, collision::NESTED_FIELD_OFFSET);
    assert_eq!(tables.nested.header_offset, 0x98);
    assert_eq!(tables.nested.count, 222);
    assert_eq!(tables.nested.data_offset, 0xA8);
    assert_eq!(tables.nested.stride, 0x28);

    // The array field at +0x58.
    assert_eq!(tables.array.field, collision::ARRAY_FIELD_OFFSET);
    assert_eq!(tables.array.header_offset, 0x2358);
    assert_eq!(tables.array.count, 6);
    assert_eq!(tables.array.data_offset, 0x2368);
    assert_eq!(tables.array.stride, 0x20);

    // `m_cellBytes` at +0x68 and its 220 block-list entries.
    assert_eq!(tables.cell_bytes.field, collision::CELL_FIELD_OFFSET);
    assert_eq!(tables.cell_bytes.header_offset, 0x2428);
    assert_eq!(tables.cell_bytes.count, 220);
    assert_eq!(tables.cell_bytes.data_offset, 0x2438);
    assert_eq!(tables.cell_bytes.stride, 0x10);

    // The tiling: each array ends exactly where the next structure starts, and
    // the cell table ends exactly where the first cell begins.
    assert_eq!(tables.nested.end(), tables.array.header_offset);
    assert_eq!(tables.array.end(), tables.cell_bytes.header_offset);
    assert_eq!(tables.cell_bytes.end(), 0x31F8, "the first cell");

    let cells = collision::parse(&bytes).expect("the collision payload parses");
    assert_eq!(tables.cell_bytes.end(), cells.cells[0].cell_offset);

    // Raw bytes only: the first element of each array, and the second word of
    // every +0x58 element - pinned so a format change cannot pass unnoticed.
    let at = |offset: u64| (tables.blob_offset as u64 + offset) as usize;
    let nested = &bytes[at(tables.nested.data_offset)..at(tables.nested.data_offset) + 0x28];
    assert_eq!(
        nested
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
        "18116095c99e5e9003d87f7e0000000052a133a8891dd8a855fd509a0dbf0802f867000000000000"
    );
    // The last qword of that element repeats the first cell's block size, which
    // is a measured coincidence of this payload and nothing more.
    assert_eq!(
        u64::from_le_bytes(nested[0x20..0x28].try_into().unwrap()),
        cells.cells[0].block_size
    );

    for k in 0..6u64 {
        let rec = &bytes[at(tables.array.data_offset + k * tables.array.stride)
            ..at(tables.array.data_offset + k * tables.array.stride) + 0x20];
        assert_eq!(
            u64::from_le_bytes(rec[0..8].try_into().unwrap()),
            0,
            "element {k} first qword"
        );
        assert_eq!(
            u32::from_le_bytes(rec[8..12].try_into().unwrap()),
            7,
            "element {k} second dword"
        );
        assert_eq!(
            u32::from_le_bytes(rec[12..16].try_into().unwrap()),
            0,
            "element {k} third dword"
        );
    }
    let first = &bytes[at(tables.array.data_offset)..at(tables.array.data_offset) + 0x20];
    assert_eq!(
        first.iter().map(|b| format!("{b:02x}")).collect::<String>(),
        "000000000000000007000000000000008ae88d42b4ce60c0c004554200000000"
    );
}

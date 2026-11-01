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

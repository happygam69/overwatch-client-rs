//! The practice range's spawn-room volume, read from the map's own
//! `MAP_PLACEABLE_TYPE_TRIGGER_VOLUME` chunk (kind `0x17`) and the area boxes
//! it sits inside.
//!
//! Expected values, not seeds: the test resolves the practice header through
//! the client's variation pairing, opens the variation's chunk set, and reads
//! the volume record. The spawn *selector* is still unread; this is the map
//! volume `ow-client` uses as its declared viewer default.
//!
//! Skipped when no install is present. Read-only.
//!
//! ```text
//! cargo test --release -p ow-tank --test spawn_room_real -- --nocapture
//! ```

use ow_tank::keytable::Sources;
use ow_tank::{mapresolve, GameData, ManifestSelector};

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
fn the_practice_range_has_a_spawn_room_volume() {
    let Some(data) = open() else { return };
    let root = ow_casc::install::detect_root().expect("the install root");
    let sources = Sources::discover(std::slice::from_ref(&root));
    let kinds = sources.chunk_kinds().expect("the chunk-kind table");
    let scene = mapresolve::resolve_practice_range(&data, &kinds).expect("the practice scene");

    let room = mapresolve::spawn_room(&scene.scene, &data).expect("the spawn-room volume");
    let close = |a: f32, b: f32| (a - b).abs() < 0.01;
    assert!(
        close(room.centre[0], 71.169),
        "centre x {:.3}",
        room.centre[0]
    );
    assert!(
        close(room.centre[1], 0.948),
        "centre y {:.3}",
        room.centre[1]
    );
    assert!(
        close(room.centre[2], 53.110),
        "centre z {:.3}",
        room.centre[2]
    );
    assert!(
        close(room.extents[0], 11.861),
        "extent x {:.3}",
        room.extents[0]
    );
    assert!(
        close(room.extents[1], 5.179),
        "extent y {:.3}",
        room.extents[1]
    );
    assert!(
        close(room.extents[2], 7.608),
        "extent z {:.3}",
        room.extents[2]
    );
    assert!(close(room.axis[1], 0.0), "axis must be yaw-only");
    assert!(room.floor() < 0.0, "the volume reaches below the floor");
    println!(
        "spawn room: centre ({:.3}, {:.3}, {:.3}), half extents ({:.3}, {:.3}, {:.3}), floor {:.3}",
        room.centre[0],
        room.centre[1],
        room.centre[2],
        room.extents[0],
        room.extents[1],
        room.extents[2],
        room.floor()
    );
}

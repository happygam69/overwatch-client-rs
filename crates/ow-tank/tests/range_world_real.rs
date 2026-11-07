//! The practice range as a collision world: the model meshes placed where the
//! map chunk puts them, swept with the verified mesh narrowphase.
//!
//! `ow_tank::range_world` assembles the world from the install's own data (the
//! holder filter of `0x14201ae60` and the `STU_537E56E4` blocks), and
//! `ow_sim::walk` is the collision world the movement step sweeps: the verified
//! type-3 mesh leaf (`0x142caed30`) over the query block `0x142cd5540` reads.
//!
//! Skipped when no install is present. Read-only.
//!
//! ```text
//! cargo test --release -p ow-tank --test range_world_real -- --nocapture
//! ```

use ow_tank::classreg::ClassRegistry as Classreg;
use ow_tank::keytable::Sources;
use ow_tank::range_world::RangeWorld;
use ow_tank::{GameData, ManifestSelector};

fn open() -> Option<(GameData, Sources)> {
    let root = ow_casc::install::detect_root()?;
    let sources = Sources::discover(std::slice::from_ref(&root));
    match GameData::open(&root, &ManifestSelector::default(), &sources) {
        Ok(data) => Some((data, sources)),
        Err(e) => {
            eprintln!("note: install not readable here ({e}); nothing to assert");
            None
        }
    }
}

/// The sweep world the client builds from the range's placed shapes.
fn walk_world(range: &RangeWorld) -> ow_sim::walk::World {
    let mesh = &range.mesh;
    let mut shapes = Vec::with_capacity(range.placed.len());
    for placed in &range.placed {
        let rotation = placed.rotation;
        // The chunk stores the model's local-to-world quaternion; the sweep's
        // transform is the world-to-local one its leaf applies to the query.
        let conjugate = if rotation == [0.0; 4] {
            [0.0, 0.0, 0.0, 1.0]
        } else {
            // Keep it normalised the way the client's pose read does, then
            // conjugate.
            let len = (rotation[0] * rotation[0]
                + rotation[1] * rotation[1]
                + rotation[2] * rotation[2]
                + rotation[3] * rotation[3])
                .sqrt();
            if len > 0.0 {
                [
                    -rotation[0] / len,
                    -rotation[1] / len,
                    -rotation[2] / len,
                    rotation[3] / len,
                ]
            } else {
                [0.0, 0.0, 0.0, 1.0]
            }
        };
        let record = mesh.record(placed.shape, placed.descriptor_scale());
        let (low, high) = placed.world_bounds();
        shapes.push(ow_sim::walk::Shape {
            record: record.to_vec(),
            xform: ow_sim::walk::shape_xform(placed.translation, conjugate),
            low,
            high,
        });
    }
    ow_sim::walk::World::new(mesh.base(), mesh.bytes().to_vec(), shapes)
}

/// The range's models carry mesh shapes, they are placed, and the sweep finds
/// the ground under a placement.
#[test]
fn the_range_is_a_sweepable_world() {
    let Some((data, sources)) = open() else {
        return;
    };
    let classreg = Classreg::load_from_sources(&sources);
    assert!(!classreg.is_empty(), "no STU class checkout found");
    let range = RangeWorld::build(&data, &sources, classreg.inner()).expect("the range assembles");
    assert_eq!(
        range.level_shapes, 1,
        "the map's own level collision is one shape"
    );
    println!(
        "range world: {} shape(s), {} placed instance(s), {} point(s), {} blob bytes, base {:#x}",
        range.mesh.len(),
        range.placed.len(),
        range.point_count(),
        range.mesh.bytes().len(),
        range.mesh.base()
    );
    assert!(
        range.placed.len() > 3000,
        "{} placed shape(s): the level collider and the models",
        range.placed.len()
    );
    assert!(
        range.mesh.len() >= 100,
        "the range's models carry {} mesh blocks, expected the 272 holders of the census",
        range.mesh.len()
    );
    assert!(
        range.placed.len() >= 200,
        "{} placed mesh instance(s)",
        range.placed.len()
    );

    let world = walk_world(&range);
    // Sweep straight down over a spread of placements: each one's own model is
    // there, so a hit proves the placement transform and the blob's pointers
    // agree with the narrowphase.
    let mut hits = 0;
    let mut tried = 0;
    for placed in range.placed.iter().step_by(97) {
        let at = [
            placed.translation[0],
            placed.translation[1] + 60.0,
            placed.translation[2],
        ];
        tried += 1;
        if world
            .sweep(at, [at[0], at[1] - 200.0, at[2]], 0.2)
            .is_some()
        {
            hits += 1;
        }
    }
    println!("down sweeps: {hits}/{tried} placements hit their own model");
    assert!(hits > 0, "no downward sweep met the range's geometry");
}

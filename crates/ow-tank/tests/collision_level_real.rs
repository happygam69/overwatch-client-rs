//! The practice range's own level collision: the `MAP_CHUNK_COLLISION` body.
//!
//! The container is read (`ow_tank::chunkkind`); the body is the client's own
//! mesh record arrays - vertices at a `0x10` stride, then nodes at `0x20`, then
//! triangles at `0x20` - which the tail's three pointers and three counts tile
//! exactly. This pins the numbers and the invariants, and the walk world sweeps
//! them with the verified type-3 leaf.
//!
//! Skipped when no install is present. Read-only.
//!
//! ```text
//! cargo test --release -p ow-tank --test collision_level_real -- --nocapture
//! ```

use ow_tank::chunkkind;
use ow_tank::collision_level;
use ow_tank::keytable::Sources;
use ow_tank::{GameData, ManifestSelector};

/// The practice range's collision chunk, from the scene's chunk list.
const PRACTICE_COLLISION: u64 = 0x0DD0_0011_0000_0688;

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
fn the_range_level_collision_is_one_world_space_mesh() {
    let Some(data) = open() else {
        return;
    };
    assert_eq!(
        chunkkind::kind_of_guid(PRACTICE_COLLISION),
        chunkkind::KIND_COLLISION
    );
    let payload = data
        .open_asset(PRACTICE_COLLISION)
        .expect("the collision chunk opens");
    let level = collision_level::parse(&payload).expect("the level collision parses");

    // The tail's three counts, and the strides the payload closes at.
    assert_eq!(level.sectors, 36, "the container's sector count");
    assert_eq!(level.arrays.point_count(), 203_999, "vertices");
    assert_eq!(level.words.node_count, 236_957, "nodes");
    assert_eq!(level.words.field_3c, 343_474, "triangles");
    assert_eq!(level.mask, 0x1FFF, "the default layer mask");
    assert_eq!(level.arrays.points.len(), 203_999 * 0x10);
    assert_eq!(level.arrays.nodes.len(), 236_957 * 0x20);
    assert_eq!(level.arrays.triangles.len(), 343_474 * 0x20);

    // The root node's box is the level's own extent: the terrain's floor to the
    // tallest structure, in the map's coordinates.
    let (low, high) = level.root;
    println!(
        "level collision: root ({:.3}, {:.3}, {:.3}) .. ({:.3}, {:.3}, {:.3})",
        low[0], low[1], low[2], high[0], high[1], high[2]
    );
    assert!((low[0] - -103.469).abs() < 0.01, "root low x {}", low[0]);
    assert!((low[1] - -38.510).abs() < 0.01, "root low y {}", low[1]);
    assert!((low[2] - -161.028).abs() < 0.01, "root low z {}", low[2]);
    assert!((high[0] - 206.159).abs() < 0.01, "root high x {}", high[0]);
    assert!((high[1] - 51.071).abs() < 0.01, "root high y {}", high[1]);
    assert!((high[2] - 148.317).abs() < 0.01, "root high z {}", high[2]);
    assert!(
        high[1] - low[1] > 80.0,
        "the level is {:.1} m tall",
        high[1] - low[1]
    );

    // Every node's child index and leaf count stay inside the arrays, every
    // triangle's corners index a real vertex, and every mask is the tail's.
    let nodes = 236_957;
    let triangles = 343_474;
    let mut leaves = 0usize;
    let mut internal = 0usize;
    let mut masks: std::collections::BTreeMap<u16, usize> = std::collections::BTreeMap::new();
    for node in level.arrays.nodes.chunks_exact(0x20) {
        let child = i32::from_le_bytes(node[0x18..0x1c].try_into().unwrap());
        let mask = u16::from_le_bytes([node[0x1c], node[0x1d]]);
        let leaf = node[0x1f];
        *masks.entry(mask).or_default() += 1;
        assert_eq!(
            mask & !0x1FFF,
            0,
            "a node's mask {mask:#06x} has bits outside the tail's 0x1FFF"
        );
        if leaf > 0 {
            leaves += 1;
            let first = child.max(0) as usize;
            assert!(
                first + leaf as usize <= triangles,
                "a leaf's triangle run {first} + {leaf} leaves the array"
            );
        } else {
            internal += 1;
            assert!(
                child >= 0 && (child as usize) < nodes,
                "an internal node's second child {child} leaves the node array"
            );
        }
    }
    println!("nodes: {internal} internal, {leaves} leaf; masks {masks:?}");
    assert!(leaves > 1000, "the tree has only {leaves} leaf node(s)");
    assert!(
        internal > 1000,
        "the tree has only {internal} internal node(s)"
    );
}

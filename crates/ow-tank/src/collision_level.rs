//! The map's static collision: `MAP_CHUNK_COLLISION` (kind `0x11`) as the client's
//! own mesh record arrays.
//!
//! The container is read (`ow_tank::chunkkind`): a `0x38` header names a sector
//! array and one tail element. The sector array carries a per-sector index
//! (the 36 sectors this project already parses), and the **tail element is the
//! level's collider**: three pointers and three counts over the payload's body.
//!
//! Reading the tail's element against the payload, the numbers close exactly:
//!
//! ```text
//! tail +0x00  u64  vertices  = 0x40        count +0x1c = 203,999
//!                                  (0x31CE30 - 0x40) / 203,999 = 16.000
//! tail +0x08  u64  nodes     = 0x31CE30    count +0x18 = 236,957
//!                                  (0xA581D0 - 0x31CE30) / 236,957 = 32.000
//! tail +0x10  u64  triangles = 0xA581D0    count +0x20 = 343,474
//!                                  (0x14D3810 - 0xA581D0) / 343,474 = 32.000
//! tail +0x18..   the three counts, then 30, a float, and 0x1FFF
//! ```
//!
//! The three strides are the client's own mesh record strides
//! (`mesh_world::VERTEX_LEN` `0x10`, `NODE_LEN` `0x20`, `TRIANGLE_LEN` `0x20`),
//! and the element contents decode as those records: the first node's box is
//! `(-103.469, -38.510, -161.028) .. (206.159, 51.071, 148.317)`, the level's
//! own extent, with child `62684`, mask `0x1FFF`, axis `0`, leaf count `0`; the
//! first triangle's corners `120319, 120323, 120317` are all below the vertex
//! count, followed by three adjacency words, two ids and the mask `0x1FFF` (the
//! tail's own mask word). The mask agrees with every node and triangle, and the
//! type-3 leaf `0x142caed30` reads exactly these offsets (`mesh_world`).
//!
//! **The arrays are used as stored.** No conversion, no rebuild: the client
//! copies pointers, not data (`docs/formats/static-collision.md`, check 4), so
//! the collision world hands these three runs to a shape record unchanged and
//! the verified leaf walks them in world coordinates.
//!
//! Read-only over the install.

use crate::chunkkind::{self, Collision};
use crate::mesh_shape::MeshSource;
use crate::mesh_world::MeshArrays;

/// The tail element's field offsets.
mod tail {
    /// `+0x00`: the node array's RVA.
    pub const NODES: usize = 0x00;
    /// `+0x08`: the vertex array's RVA.
    pub const VERTICES: usize = 0x08;
    /// `+0x10`: the triangle array's RVA.
    pub const TRIANGLES: usize = 0x10;
    /// `+0x18`: the node count.
    pub const NODE_COUNT: usize = 0x18;
    /// `+0x1c`: the vertex count.
    pub const VERTEX_COUNT: usize = 0x1c;
    /// `+0x20`: the triangle count.
    pub const TRIANGLE_COUNT: usize = 0x20;
    /// `+0x30`: the default layer mask, `0x1FFF`, which every node and
    /// triangle in the body carries too.
    pub const MASK: usize = 0x30;
}

/// The level's collision mesh: the three arrays, as stored.
pub struct LevelMesh {
    pub arrays: MeshArrays,
    pub words: MeshSource,
    /// The sector count the container declares (`header.count_a`).
    pub sectors: u32,
    /// The root node's box, the level's own extent.
    pub root: ([f32; 3], [f32; 3]),
    /// The tail's default layer mask.
    pub mask: u16,
}

fn u64_at(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn f32_at(bytes: &[u8], at: usize) -> Option<f32> {
    u32_at(bytes, at).map(f32::from_bits)
}

/// Parse a `MAP_CHUNK_COLLISION` payload into the client's mesh arrays.
///
/// Fails rather than guessing: the tail's three runs must tile the body exactly
/// at the record strides, the first node's box must be a well-formed extent, and
/// every triangle's corners must index a real vertex.
pub fn parse(payload: &[u8]) -> Result<LevelMesh, String> {
    let collision =
        Collision::parse(payload).map_err(|e| format!("the collision container: {e}"))?;
    let header = collision.header;
    if header.count_b != 1 {
        return Err(format!(
            "the collision container declares {} tail element(s), expected one",
            header.count_b
        ));
    }
    let tail_at = usize::try_from(header.rva_28).map_err(|_| "the tail RVA".to_string())?;
    let tail = payload
        .get(tail_at..tail_at + chunkkind::TAIL_SIZE)
        .ok_or_else(|| "the tail element is outside the payload".to_string())?;

    let vertices_at = usize::try_from(u64_at(tail, tail::VERTICES).unwrap_or(0))
        .map_err(|_| "the vertex RVA".to_string())?;
    let nodes_at = usize::try_from(u64_at(tail, tail::NODES).unwrap_or(0))
        .map_err(|_| "the node RVA".to_string())?;
    let triangles_at = usize::try_from(u64_at(tail, tail::TRIANGLES).unwrap_or(0))
        .map_err(|_| "the triangle RVA".to_string())?;
    let vertex_count = u32_at(tail, tail::VERTEX_COUNT).unwrap_or(0) as usize;
    let node_count = u32_at(tail, tail::NODE_COUNT).unwrap_or(0) as usize;
    let triangle_count = u32_at(tail, tail::TRIANGLE_COUNT).unwrap_or(0) as usize;
    let mask = u32_at(tail, tail::MASK).unwrap_or(0) as u16;
    if vertex_count == 0 || node_count == 0 || triangle_count == 0 {
        return Err(format!(
            "the level mesh is empty: {vertex_count} vertices, {node_count} nodes, {triangle_count} triangles"
        ));
    }

    let vertex_len = vertex_count * crate::mesh_world::VERTEX_LEN;
    let node_len = node_count * crate::mesh_world::NODE_LEN;
    let triangle_len = triangle_count * crate::mesh_world::TRIANGLE_LEN;
    let vertices = payload
        .get(vertices_at..vertices_at + vertex_len)
        .ok_or_else(|| "the vertex array is outside the payload".to_string())?;
    let nodes = payload
        .get(nodes_at..nodes_at + node_len)
        .ok_or_else(|| "the node array is outside the payload".to_string())?;
    let triangles = payload
        .get(triangles_at..triangles_at + triangle_len)
        .ok_or_else(|| "the triangle array is outside the payload".to_string())?;

    // The three runs must tile the body at exactly those strides: a mismatch
    // means the tail is not what the strides say it is. The tail's field order
    // is not the memory order (vertices come first in the body, nodes second),
    // so the runs are sorted by where they start before the tiling is checked.
    let mut runs = [
        (
            "vertices",
            vertices_at,
            vertex_count,
            crate::mesh_world::VERTEX_LEN,
        ),
        ("nodes", nodes_at, node_count, crate::mesh_world::NODE_LEN),
        (
            "triangles",
            triangles_at,
            triangle_count,
            crate::mesh_world::TRIANGLE_LEN,
        ),
    ];
    runs.sort_by_key(|(_, start, _, _)| *start);
    for pair in runs.windows(2) {
        let (name, start, count, stride) = pair[0];
        let (next_name, next_start, _, _) = pair[1];
        let end = start + count * stride;
        if end != next_start {
            return Err(format!(
                "the {name} run ends at {end:#x}, the {next_name} run begins at {next_start:#x}: the {count} elements of stride {stride:#x} do not tile"
            ));
        }
    }
    // The body ends where the sector array begins, and the sector array, its
    // runs and the tail fill the rest of the payload.
    let sectors_at = usize::try_from(header.rva_20).map_err(|_| "the sector RVA".to_string())?;
    let (last, last_start, last_count, last_stride) = runs[2];
    let last_end = last_start + last_count * last_stride;
    if last_end != sectors_at {
        return Err(format!(
            "the {last} run ends at {last_end:#x}, the sector array begins at {sectors_at:#x}"
        ));
    }

    // The root node's box, and every triangle's corners.
    let root_low = [
        f32_at(nodes, crate::mesh_world::NODE_MIN).unwrap_or(0.0),
        f32_at(nodes, crate::mesh_world::NODE_MIN + 4).unwrap_or(0.0),
        f32_at(nodes, crate::mesh_world::NODE_MIN + 8).unwrap_or(0.0),
    ];
    let root_high = [
        f32_at(nodes, crate::mesh_world::NODE_MAX).unwrap_or(0.0),
        f32_at(nodes, crate::mesh_world::NODE_MAX + 4).unwrap_or(0.0),
        f32_at(nodes, crate::mesh_world::NODE_MAX + 8).unwrap_or(0.0),
    ];
    for lane in 0..3 {
        if !(root_low[lane].is_finite() && root_high[lane].is_finite()) {
            return Err("the root node's box is not finite".into());
        }
        if root_low[lane] > root_high[lane] {
            return Err("the root node's box is inverted".into());
        }
    }
    let mut worst = 0i32;
    for triangle in triangles.chunks_exact(crate::mesh_world::TRIANGLE_LEN) {
        for lane in 0..3 {
            let corner = i32::from_le_bytes(
                triangle[lane * 4..lane * 4 + 4]
                    .try_into()
                    .expect("a triangle record holds three corners"),
            );
            if corner > worst {
                worst = corner;
            }
        }
    }
    if worst < 0 || worst as usize >= vertex_count {
        return Err(format!(
            "a triangle corner indexes {worst}, the vertex array holds {vertex_count}"
        ));
    }

    Ok(LevelMesh {
        arrays: MeshArrays {
            nodes: nodes.to_vec(),
            points: vertices.to_vec(),
            triangles: triangles.to_vec(),
        },
        words: MeshSource {
            nodes: 0,
            points: 0,
            triangles: 0,
            node_count: node_count as u32,
            point_count: vertex_count as u32,
            field_3c: triangle_count as u32,
            height: 0,
            field_44: 0.0,
        },
        sectors: header.count_a as u32,
        root: (root_low, root_high),
        mask,
    })
}

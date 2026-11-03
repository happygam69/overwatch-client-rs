//! The model mesh's **world image**: its three arrays laid out in one byte
//! buffer behind guest addresses, and the per-shape source a shape record
//! points at.
//!
//! Everything here is the layout [`crate::mesh_shape`] and the collision query
//! read, applied to an array set taken as stored. The client never converts or
//! copies these arrays (`docs/formats/static-collision.md`, check 4): the block
//! `STU_537E56E4` holds them, `0x141b53af0` hands their pointers to a
//! `0x30`-byte source, `MeshShapeFill` (`0x142cae540`) copies the three
//! pointers into the `0x4c`-byte record at `+0x20`/`+0x28`/`+0x30`, and the
//! type-3 narrowphase `0x142caed30` walks them.
//!
//! ## The byte layout the walk reads
//!
//! `MeshSweep` (`0x142caed30`), current image:
//!
//! | constant | value | what the function does |
//! |---|---|---|
//! | [`NODE_LEN`] | `0x20` | the second child is `node + *(i32*)(node + 0x18) * 0x20`, the adjacent one is `node + 0x20` |
//! | [`NODE_MIN`] | `0x00` | the box's low corner, four lanes read as one quad |
//! | [`NODE_MAX`] | `0x0c` | the box's high corner, four lanes; lane 3 is the same dword as [`NODE_CHILD`] |
//! | [`NODE_CHILD`] | `0x18` | `i32`: the second child's index, or a leaf's first triangle index |
//! | [`NODE_MASK`] | `0x1c` | `u16` layer mask, tested against the query's |
//! | [`NODE_AXIS`] | `0x1e` | `u8`: the split axis the near child is chosen by |
//! | [`NODE_LEAF`] | `0x1f` | `u8`: leaf triangle count, `0` for an internal node |
//! | [`TRIANGLE_LEN`] | `0x20` | `triangles + index * 0x20` |
//! | [`TRIANGLE_CORNERS`] | `0x00` | three `i32` vertex indices |
//! | [`TRIANGLE_IDS`] | `0x18` | two `u16` ids, published in the hit record |
//! | [`TRIANGLE_MASK`] | `0x1c` | `u16` layer mask |
//! | [`VERTEX_LEN`] | `0x10` | `vertices + index * 0x10`, one `Vec3A` |
//!
//! The strides are confirmed twice: by that code, and by the range's own block
//! data -- model `0x0d0000000000078c` carries `288` node bytes with
//! `m_nodeCount` `9` (`288 / 9 = 0x20`) and `384` triangle bytes against the
//! block's `0x38B71436` word `12` (`384 / 12 = 0x20`).
//!
//! ## The query block
//!
//! `0x142caed30` reads the query block at [`QUERY_START`] (a quad),
//! [`QUERY_END`] (a quad), [`QUERY_MOTION`], [`QUERY_SUPPORT_POINTS`] (a
//! pointer), [`QUERY_SUPPORT_COUNT`], [`QUERY_ROTATION`], [`QUERY_EXPANSION`],
//! [`QUERY_MASK`] and [`QUERY_FLAG_72`]. The handle dispatcher `0x142cc45a0`
//! copies the descriptor into a stack block that runs from the start quad
//! through the byte at `+0x72` (`0x73` bytes); [`QUERY_BLOCK_LEN`] rounds that up
//! so every quad lane read stays inside the block.
//!
//! **Only the no-support-point query is representable here.** `MeshTriangleTest`
//! (`0x142cce430`) takes the read triangle path only when the support-point
//! count is exactly `0` *and* every lane of the scaled expansion is at most
//! `0.005`; any other state hands the triangle to `0x142cc9460`, which is not
//! read. [`query_block`] therefore writes a count of `0` and no support-point
//! pointer, and the capsule / support-point narrowphase stays **open**.

use crate::mesh_shape::{self, MeshSource};

/// `0x20`: the BVH node stride (`0x142caed30`: `node + 0x20` and
/// `node + child * 0x20`).
pub const NODE_LEN: usize = 0x20;
/// The node's low box corner.
pub const NODE_MIN: usize = 0x00;
/// The node's high box corner.
pub const NODE_MAX: usize = 0x0c;
/// The node's `i32` second-child index / leaf triangle base.
pub const NODE_CHILD: usize = 0x18;
/// The node's `u16` layer mask.
pub const NODE_MASK: usize = 0x1c;
/// The node's `u8` split axis.
pub const NODE_AXIS: usize = 0x1e;
/// The node's `u8` leaf triangle count.
pub const NODE_LEAF: usize = 0x1f;

/// `0x20`: the triangle record stride (`0x142caed30`:
/// `triangles + *(i32*)(node + 0x18) * 0x20`).
pub const TRIANGLE_LEN: usize = 0x20;
/// The triangle's three `i32` vertex indices.
pub const TRIANGLE_CORNERS: usize = 0x00;
/// The triangle's two `u16` ids.
pub const TRIANGLE_IDS: usize = 0x18;
/// The triangle's `u16` layer mask.
pub const TRIANGLE_MASK: usize = 0x1c;

/// `0x10`: the vertex stride, one `Vec3A` per vertex.
pub const VERTEX_LEN: usize = 0x10;

/// The mesh element stride the record is written into: `MeshShapeFill` writes
/// `0x4c` bytes of a `0x50`-byte element.
pub const RECORD_LEN: usize = 0x50;
/// The bytes `MeshShapeFill` writes: `RECORD_DWORDS * 4`.
pub const RECORD_WRITTEN: usize = mesh_shape::RECORD_DWORDS * 4;

/// The query block's length: `0x73` bytes are read (the start quad through the
/// byte at `+0x72`), rounded up so every four-lane read stays inside.
pub const QUERY_BLOCK_LEN: usize = 0x78;
/// The query block's `P0` quad.
pub const QUERY_START: usize = 0x00;
/// The query block's `P1` quad.
pub const QUERY_END: usize = 0x10;
/// The query block's motion budget quad.
pub const QUERY_MOTION: usize = 0x20;
/// The query block's pointer to the local support points.
pub const QUERY_SUPPORT_POINTS: usize = 0x30;
/// The query block's support-point count (`i32`).
pub const QUERY_SUPPORT_COUNT: usize = 0x38;
/// The query block's local rotation quaternion, `(x, y, z, w)`.
pub const QUERY_ROTATION: usize = 0x40;
/// The query block's expansion base quad.
pub const QUERY_EXPANSION: usize = 0x50;
/// The query block's `u16` layer mask.
pub const QUERY_MASK: usize = 0x70;
/// The query block's byte at `+0x72` (read by the unread slow path only).
pub const QUERY_FLAG_72: usize = 0x72;

/// Every field the walk loads has to lie inside the record it loads it from, and
/// the record it fills has to fit the element it is written into. Checked at
/// compile time so a wrong offset cannot become a runtime read.
const _: () = {
    assert!(NODE_LEAF < NODE_LEN);
    assert!(TRIANGLE_MASK + 2 <= TRIANGLE_LEN);
    assert!(QUERY_FLAG_72 < QUERY_BLOCK_LEN);
    assert!(RECORD_WRITTEN < RECORD_LEN);
};

/// A shape's world transform: the position quad at `+0x00` and the world-to-local
/// quaternion at `+0x10` (`0x142caed30` reads `param_4[0..3]` and
/// `param_4[4..7]`).
pub const XFORM_LEN: usize = 0x20;
/// The xform's position quad.
pub const XFORM_POSITION: usize = 0x00;
/// The xform's world-to-local quaternion.
pub const XFORM_ROTATION: usize = 0x10;

/// `0.005`: the largest scaled expansion lane `MeshTriangleTest`
/// (`0x142cce430`) runs the read triangle path for.
pub const FAST_EXPANSION_LIMIT: f32 = 0.005;

/// One mesh block's three arrays, byte for byte as the block stores them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MeshArrays {
    /// The node array: `node_count * NODE_LEN` bytes.
    pub nodes: Vec<u8>,
    /// The point array: `point_count * VERTEX_LEN` bytes.
    pub points: Vec<u8>,
    /// The triangle array: the walk's records, `TRIANGLE_LEN` bytes each.
    pub triangles: Vec<u8>,
}

impl MeshArrays {
    /// The nodes the array holds, by stride.
    pub fn node_count(&self) -> usize {
        self.nodes.len() / NODE_LEN
    }

    /// The points the array holds, by stride.
    pub fn point_count(&self) -> usize {
        self.points.len() / VERTEX_LEN
    }

    /// The triangle records the array holds, by stride.
    pub fn triangle_count(&self) -> usize {
        self.triangles.len() / TRIANGLE_LEN
    }

    /// Whether each array is a whole number of its records. A block whose arrays
    /// are not cannot be walked by `0x142caed30`'s strides, so the assembler
    /// refuses it rather than padding it.
    pub fn is_strided(&self) -> bool {
        self.nodes.len() % NODE_LEN == 0
            && self.points.len() % VERTEX_LEN == 0
            && self.triangles.len() % TRIANGLE_LEN == 0
    }
}

/// One shape's place in a [`MeshWorld`]: the guest address of each of its arrays
/// and the block's own scalar words.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapeSlot {
    /// The guest address of this shape's node array.
    pub nodes: u64,
    /// The guest address of this shape's point array.
    pub points: u64,
    /// The guest address of this shape's triangle array.
    pub triangles: u64,
    /// `m_nodeCount`, the block's own word.
    pub node_count: u32,
    /// The point array's header count.
    pub point_count: u32,
    /// `m_38B71436`, copied into the record at `+0x40`.
    pub field_3c: u32,
    /// `m_height`, copied into the record at `+0x44`.
    pub height: u32,
    /// `m_5BD33E19`, scaled into the record at `+0x48`.
    pub field_44: f32,
}

impl ShapeSlot {
    /// The `0x30`-byte source `0x141b53af0` would hand on: three guest
    /// addresses and the block's words.
    pub fn to_source(&self) -> MeshSource {
        MeshSource {
            nodes: self.nodes,
            points: self.points,
            triangles: self.triangles,
            node_count: self.node_count,
            point_count: self.point_count,
            field_3c: self.field_3c,
            height: self.height,
            field_44: self.field_44,
        }
    }
}

/// A set of mesh shapes in one guest address space.
///
/// The arrays are appended to `bytes` in push order -- nodes, then points, then
/// triangles, with no padding, because the walk reads every record through its
/// stride and never assumes an alignment. Each shape's slots are the addresses
/// of its own arrays, so a record built from them addresses its own bytes.
#[derive(Debug, Clone)]
pub struct MeshWorld {
    base: u64,
    bytes: Vec<u8>,
    shapes: Vec<ShapeSlot>,
}

impl MeshWorld {
    /// A world whose first byte is at guest address `base`.
    ///
    /// The client's addresses are heap allocations and are not recoverable from
    /// an asset; any base works, because every pointer in a record is derived
    /// from it and the walk only subtracts it.
    pub fn new(base: u64) -> Self {
        MeshWorld {
            base,
            bytes: Vec::new(),
            shapes: Vec::new(),
        }
    }

    /// The guest address of `bytes()[0]`.
    pub fn base(&self) -> u64 {
        self.base
    }

    /// Every array byte, in push order.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The shapes pushed so far, in push order.
    pub fn shapes(&self) -> &[ShapeSlot] {
        &self.shapes
    }

    /// The number of shapes.
    pub fn len(&self) -> usize {
        self.shapes.len()
    }

    /// Whether no shape has been pushed.
    pub fn is_empty(&self) -> bool {
        self.shapes.is_empty()
    }

    /// The byte offset a guest address has in [`Self::bytes`].
    pub fn offset_of(&self, address: u64) -> Option<usize> {
        let offset = address.checked_sub(self.base)?;
        usize::try_from(offset)
            .ok()
            .filter(|o| *o <= self.bytes.len())
    }

    /// Append one shape's arrays and return its index.
    ///
    /// `words` supplies the block's scalar words; its three pointer fields are
    /// ignored, because those are this world's addresses, not the block's.
    ///
    /// # Panics
    ///
    /// When an array is not a whole number of its records, or when the node
    /// array does not hold exactly `words.node_count` nodes, or the point array
    /// not exactly `words.point_count` points -- each is a shape the walk would
    /// run off the end of.
    pub fn push(&mut self, arrays: &MeshArrays, words: &MeshSource) -> usize {
        assert!(arrays.is_strided(), "an array is not a whole record count");
        assert_eq!(
            arrays.node_count(),
            words.node_count as usize,
            "the node array holds {} nodes but the block records {}",
            arrays.node_count(),
            words.node_count
        );
        assert_eq!(
            arrays.point_count(),
            words.point_count as usize,
            "the point array holds {} points but the header records {}",
            arrays.point_count(),
            words.point_count
        );

        let nodes = self.append(&arrays.nodes);
        let points = self.append(&arrays.points);
        let triangles = self.append(&arrays.triangles);
        self.shapes.push(ShapeSlot {
            nodes,
            points,
            triangles,
            node_count: words.node_count,
            point_count: words.point_count,
            field_3c: words.field_3c,
            height: words.height,
            field_44: words.field_44,
        });
        self.shapes.len() - 1
    }

    fn append(&mut self, bytes: &[u8]) -> u64 {
        let address = self.base + self.bytes.len() as u64;
        self.bytes.extend_from_slice(bytes);
        address
    }

    /// One shape's arrays, read back out of [`Self::bytes`]. The arrays were
    /// appended nodes, points, triangles with no padding, so each one is the
    /// span from its own address to the next.
    ///
    /// # Panics
    ///
    /// When the shape's addresses do not lie inside the buffer.
    pub fn arrays(&self, index: usize) -> MeshArrays {
        let slot = self.shapes[index];
        let nodes = self.address_offset(slot.nodes);
        let points = self.address_offset(slot.points);
        let triangles = self.address_offset(slot.triangles);
        let triangles_end = match self.shapes.get(index + 1) {
            Some(next) => self.address_offset(next.nodes),
            None => self.bytes.len(),
        };
        MeshArrays {
            nodes: self.bytes[nodes..points].to_vec(),
            points: self.bytes[points..triangles].to_vec(),
            triangles: self.bytes[triangles..triangles_end].to_vec(),
        }
    }

    fn address_offset(&self, address: u64) -> usize {
        self.offset_of(address)
            .filter(|offset| *offset < self.bytes.len())
            .unwrap_or_else(|| panic!("address {address:#x} is not inside the world"))
    }

    /// One shape's slot.
    pub fn shape(&self, index: usize) -> &ShapeSlot {
        &self.shapes[index]
    }

    /// One shape's source, with this world's addresses in the pointer lanes.
    pub fn source(&self, index: usize) -> MeshSource {
        self.shapes[index].to_source()
    }

    /// One shape's record, `MeshShapeFill`'s `0x4c` bytes zero-extended to the
    /// `0x50`-byte element they are written into.
    pub fn record(&self, index: usize, scale: f32) -> [u8; RECORD_LEN] {
        let dwords = mesh_shape::shape_record(&self.source(index), scale);
        let mut out = [0u8; RECORD_LEN];
        for (slot, dword) in dwords.iter().enumerate() {
            out[slot * 4..slot * 4 + 4].copy_from_slice(&dword.to_le_bytes());
        }
        out
    }
}

fn put_quad(bytes: &mut [u8], offset: usize, quad: [f32; 4]) {
    for (lane, value) in quad.iter().enumerate() {
        bytes[offset + lane * 4..offset + lane * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
}

/// The query block `0x142cd5540` and `0x142caed30` read, for the
/// **no-support-point** query: `P0` at [`QUERY_START`], `P1` at [`QUERY_END`],
/// `motion` at [`QUERY_MOTION`], `expansion` at [`QUERY_EXPANSION`] (all four
/// lanes) and the layer `mask` at [`QUERY_MASK`]. The support-point count is
/// written as `0` and its pointer as null, because [`FAST_EXPANSION_LIMIT`] and
/// a zero count are the only state the read triangle path accepts.
///
/// `sweep_bounds` (`0x142cd5540`) leaves `P0`, `P1` and the expansion untouched
/// on this path, so its three outputs are exactly these three inputs.
pub fn query_block(
    start: [f32; 3],
    end: [f32; 3],
    motion: f32,
    expansion: f32,
    mask: u16,
) -> [u8; QUERY_BLOCK_LEN] {
    let mut bytes = [0u8; QUERY_BLOCK_LEN];
    put_quad(&mut bytes, QUERY_START, [start[0], start[1], start[2], 0.0]);
    put_quad(&mut bytes, QUERY_END, [end[0], end[1], end[2], 0.0]);
    put_quad(&mut bytes, QUERY_MOTION, [motion; 4]);
    put_quad(&mut bytes, QUERY_EXPANSION, [expansion; 4]);
    bytes[QUERY_SUPPORT_COUNT..QUERY_SUPPORT_COUNT + 4].copy_from_slice(&0i32.to_le_bytes());
    bytes[QUERY_MASK..QUERY_MASK + 2].copy_from_slice(&mask.to_le_bytes());
    bytes
}

/// The shape world transform `0x142caed30` reads: a position quad and a
/// world-to-local quaternion.
pub fn shape_xform(position: [f32; 3], rotation: [f32; 4]) -> [u8; XFORM_LEN] {
    let mut bytes = [0u8; XFORM_LEN];
    put_quad(
        &mut bytes,
        XFORM_POSITION,
        [position[0], position[1], position[2], 0.0],
    );
    put_quad(&mut bytes, XFORM_ROTATION, rotation);
    bytes
}

/// The identity rotation, so a query can run in the shape's own frame.
pub const IDENTITY_ROTATION: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

fn f32_at(bytes: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

/// One node's box, low and high corner, as `0x142caed30` reads it.
pub fn node_bounds(node: &[u8]) -> ([f32; 3], [f32; 3]) {
    let low = [
        f32_at(node, NODE_MIN),
        f32_at(node, NODE_MIN + 4),
        f32_at(node, NODE_MIN + 8),
    ];
    let high = [
        f32_at(node, NODE_MAX),
        f32_at(node, NODE_MAX + 4),
        f32_at(node, NODE_MAX + 8),
    ];
    (low, high)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A shape whose arrays are one node, one point and one triangle, so every
    /// layout assertion is a single record.
    fn one_of_each() -> (MeshArrays, MeshSource) {
        let mut nodes = vec![0u8; NODE_LEN];
        let mut points = vec![0u8; VERTEX_LEN];
        let mut triangles = vec![0u8; TRIANGLE_LEN];
        nodes[NODE_CHILD..NODE_CHILD + 4].copy_from_slice(&(-1i32).to_le_bytes());
        nodes[NODE_MASK..NODE_MASK + 2].copy_from_slice(&1u16.to_le_bytes());
        nodes[NODE_AXIS] = 2;
        nodes[NODE_LEAF] = 1;
        points[0..4].copy_from_slice(&1.5f32.to_le_bytes());
        triangles[TRIANGLE_CORNERS..TRIANGLE_CORNERS + 4].copy_from_slice(&7i32.to_le_bytes());
        triangles[TRIANGLE_IDS..TRIANGLE_IDS + 2].copy_from_slice(&0x21u16.to_le_bytes());
        triangles[TRIANGLE_IDS + 2..TRIANGLE_IDS + 4].copy_from_slice(&0x22u16.to_le_bytes());
        triangles[TRIANGLE_MASK..TRIANGLE_MASK + 2].copy_from_slice(&3u16.to_le_bytes());
        let words = MeshSource {
            nodes: 0,
            points: 0,
            triangles: 0,
            node_count: 1,
            point_count: 1,
            field_3c: 12,
            height: 5,
            field_44: 96.0,
        };
        (
            MeshArrays {
                nodes,
                points,
                triangles,
            },
            words,
        )
    }

    /// The three strides are the walk's own, and they are the ones `ow-sim`'s
    /// read narrowphase uses, so the two cannot drift apart.
    #[test]
    fn the_strides_are_the_walks_and_the_narrowphases() {
        assert_eq!(NODE_LEN, 0x20);
        assert_eq!(TRIANGLE_LEN, 0x20);
        assert_eq!(VERTEX_LEN, 0x10);
        assert_eq!(NODE_LEN, ow_sim::collide::MESH_NODE_LEN);
        assert_eq!(TRIANGLE_LEN, ow_sim::collide::MESH_TRIANGLE_LEN);
        assert_eq!(VERTEX_LEN, ow_sim::collide::MESH_VERTEX_LEN);
        // The record is written into an element one dword larger than the write.
        assert_eq!(RECORD_WRITTEN, 0x4c);
        assert_eq!(RECORD_LEN, ow_sim::collide::MESH_RECORD_LEN);
    }

    /// The node and triangle field offsets are the ones `0x142caed30` loads, and
    /// the child field shares its dword with the box's fourth high lane.
    #[test]
    fn the_record_offsets_are_the_walks() {
        assert_eq!(NODE_MIN, 0x00);
        assert_eq!(NODE_MAX, 0x0c);
        assert_eq!(NODE_CHILD, 0x18);
        assert_eq!(NODE_MASK, 0x1c);
        assert_eq!(NODE_AXIS, 0x1e);
        assert_eq!(NODE_LEAF, 0x1f);
        assert_eq!(TRIANGLE_CORNERS, 0x00);
        assert_eq!(TRIANGLE_IDS, 0x18);
        assert_eq!(TRIANGLE_MASK, 0x1c);
        // Every field lies inside its record; the module's `const` block proves
        // it at compile time, and these are the same numbers written out.
        assert_eq!(NODE_LEAF + 1, NODE_LEN);
    }

    /// The query block's fields are the ones the sweep and the triangle test
    /// read, and `query_block` writes only the no-support-point state.
    #[test]
    fn the_query_block_is_the_no_support_point_query() {
        let block = query_block([1.0, 2.0, 3.0], [4.0, 5.0, 6.0], 0.5, 0.004, 0x0001);
        assert_eq!(block.len(), QUERY_BLOCK_LEN);
        assert_eq!(
            &block[QUERY_START..QUERY_START + 12],
            &[1.0f32, 2.0, 3.0].map(f32::to_le_bytes).concat()
        );
        assert_eq!(
            &block[QUERY_START + 12..QUERY_START + 16],
            &0.0f32.to_le_bytes()
        );
        assert_eq!(
            &block[QUERY_END..QUERY_END + 12],
            &[4.0f32, 5.0, 6.0].map(f32::to_le_bytes).concat()
        );
        assert_eq!(
            &block[QUERY_MOTION..QUERY_MOTION + 16],
            &[0.5f32.to_le_bytes(); 4].concat()
        );
        assert_eq!(
            &block[QUERY_EXPANSION..QUERY_EXPANSION + 16],
            &[0.004f32.to_le_bytes(); 4].concat()
        );
        assert_eq!(
            &block[QUERY_SUPPORT_COUNT..QUERY_SUPPORT_COUNT + 4],
            &0i32.to_le_bytes()
        );
        assert_eq!(
            &block[QUERY_SUPPORT_POINTS..QUERY_SUPPORT_POINTS + 8],
            &[0u8; 8]
        );
        assert_eq!(&block[QUERY_MASK..QUERY_MASK + 2], &1u16.to_le_bytes());
        assert_eq!(block[QUERY_FLAG_72], 0);
        // The block is long enough for every field the sweep and the triangle
        // test read, the byte at `+0x72` included.
        assert_eq!(QUERY_FLAG_72 + 1, 0x73);
        // `0.005` is the fast path's own limit.
        assert_eq!(FAST_EXPANSION_LIMIT.to_bits(), 0.005f32.to_bits());
    }

    /// Pushing appends nodes, points and triangles with no padding, and the
    /// second shape's addresses start where the first's bytes end.
    #[test]
    fn the_arrays_are_appended_and_each_shape_keeps_its_own_addresses() {
        let (arrays, words) = one_of_each();
        let mut world = MeshWorld::new(0x1000);
        let first = world.push(&arrays, &words);
        let second = world.push(&arrays, &words);
        assert_eq!((first, second), (0, 1));
        assert_eq!(world.len(), 2);
        assert_eq!(world.base(), 0x1000);
        assert_eq!(
            world.bytes().len(),
            2 * (NODE_LEN + VERTEX_LEN + TRIANGLE_LEN)
        );
        let a = world.shape(0);
        let b = world.shape(1);
        assert_eq!(a.nodes, 0x1000);
        assert_eq!(a.points, 0x1000 + NODE_LEN as u64);
        assert_eq!(a.triangles, 0x1000 + (NODE_LEN + VERTEX_LEN) as u64);
        assert_eq!(b.nodes, a.triangles + TRIANGLE_LEN as u64);
        // Each shape's bytes are its own, and the offsets resolve back to them.
        for slot in [a, b] {
            let nodes = world.offset_of(slot.nodes).unwrap();
            let points = world.offset_of(slot.points).unwrap();
            let triangles = world.offset_of(slot.triangles).unwrap();
            assert_eq!(
                &world.bytes()[nodes..nodes + NODE_LEN],
                &arrays.nodes[..],
                "the node array round-trips"
            );
            assert_eq!(
                &world.bytes()[points..points + VERTEX_LEN],
                &arrays.points[..]
            );
            assert_eq!(
                &world.bytes()[triangles..triangles + TRIANGLE_LEN],
                &arrays.triangles[..]
            );
        }
        // The record's bytes are the shape's own, read back by address.
        assert_eq!(world.arrays(0), arrays, "the first shape round-trips");
        assert_eq!(world.arrays(1), arrays, "the second shape round-trips");
        // The second shape's addresses address the second copy, not the first.
        assert_ne!(a.nodes, b.nodes);
        assert_eq!(
            world.offset_of(b.nodes).unwrap(),
            world.offset_of(a.nodes).unwrap() + NODE_LEN + VERTEX_LEN + TRIANGLE_LEN
        );
        assert_eq!(world.offset_of(0x0fff), None, "below the base");
        assert_eq!(
            world.offset_of(0x1000 + world.bytes().len() as u64 + 1),
            None
        );
    }

    /// The record's three pointer lanes address the shape's own arrays and its
    /// scalar lanes are the block's words, scaled where `MeshShapeFill` scales.
    #[test]
    fn the_record_addresses_the_shapes_own_arrays() {
        let (arrays, words) = one_of_each();
        let mut world = MeshWorld::new(0x2000_0000);
        world.push(&arrays, &words);
        world.push(&arrays, &words);
        for index in 0..2 {
            let slot = *world.shape(index);
            let record = world.record(index, 1.0);
            let dwords: Vec<u32> = record[..RECORD_WRITTEN]
                .chunks(4)
                .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
                .collect();
            assert_eq!(&dwords[0..4], &[1.0f32.to_bits(); 4], "+0x00 scale");
            assert_eq!(&dwords[4..8], &[1.0f32.to_bits(); 4], "+0x10 inverse");
            assert_eq!(dwords[8], slot.nodes as u32, "+0x20 nodes low");
            assert_eq!(dwords[9], (slot.nodes >> 32) as u32, "+0x24 nodes high");
            assert_eq!(dwords[10], slot.points as u32, "+0x28 points low");
            assert_eq!(dwords[11], (slot.points >> 32) as u32, "+0x2c points high");
            assert_eq!(dwords[12], slot.triangles as u32, "+0x30 triangles low");
            assert_eq!(dwords[13], (slot.triangles >> 32) as u32, "+0x34 high");
            assert_eq!(dwords[14], 1, "+0x38 nodeCount");
            assert_eq!(dwords[15], 1, "+0x3c point count");
            assert_eq!(dwords[16], 12, "+0x40 the 0x3c word");
            assert_eq!(dwords[17], 5, "+0x44 height");
            assert_eq!(dwords[18], 96.0f32.to_bits(), "+0x48 scale * 0x5BD33E19");
            // The three lanes after the write are the element's, untouched.
            assert_eq!(
                &record[RECORD_WRITTEN..],
                &[0u8; RECORD_LEN - RECORD_WRITTEN]
            );
            // The record's own pointers resolve inside this world, and each
            // shape's address its own copy of the arrays.
            let shape_len = NODE_LEN + VERTEX_LEN + TRIANGLE_LEN;
            assert_eq!(world.offset_of(slot.nodes), Some(index * shape_len));
            assert_eq!(
                world.offset_of(slot.triangles),
                Some(index * shape_len + NODE_LEN + VERTEX_LEN)
            );
        }
        // The addresses differ between the two shapes.
        assert_ne!(world.shape(0).nodes, world.shape(1).nodes);
        assert_eq!(
            u32::from_le_bytes(world.record(1, 1.0)[8 * 4..8 * 4 + 4].try_into().unwrap()) as u64,
            world.shape(1).nodes
        );
    }

    /// `node_bounds` reads the two corners `0x142caed30` reads.
    #[test]
    fn the_node_bounds_are_the_two_corners() {
        let mut node = vec![0u8; NODE_LEN];
        put_quad(&mut node, NODE_MIN, [1.0, 2.0, 3.0, 4.0]);
        put_quad(&mut node, NODE_MAX, [5.0, 6.0, 7.0, 8.0]);
        let (low, high) = node_bounds(&node);
        assert_eq!(low, [1.0, 2.0, 3.0]);
        assert_eq!(high, [5.0, 6.0, 7.0]);
    }

    /// A block whose arrays are not whole records is refused rather than padded,
    /// and a node or point count that disagrees with the block's word is refused
    /// too: the walk would read past the arrays either way.
    #[test]
    #[should_panic(expected = "not a whole record count")]
    fn a_short_array_is_refused() {
        let (mut arrays, words) = one_of_each();
        arrays.nodes.pop();
        MeshWorld::new(0).push(&arrays, &words);
    }

    #[test]
    #[should_panic(expected = "but the block records")]
    fn a_node_count_that_disagrees_is_refused() {
        let (arrays, mut words) = one_of_each();
        words.node_count = 2;
        MeshWorld::new(0).push(&arrays, &words);
    }

    #[test]
    #[should_panic(expected = "but the header records")]
    fn a_point_count_that_disagrees_is_refused() {
        let (arrays, mut words) = one_of_each();
        words.point_count = 2;
        MeshWorld::new(0).push(&arrays, &words);
    }

    /// The identity xform is the shape's own frame: position zero, the identity
    /// quaternion at `+0x10`.
    #[test]
    fn the_identity_xform_is_the_shapes_own_frame() {
        let xform = shape_xform([0.0; 3], IDENTITY_ROTATION);
        assert_eq!(xform.len(), XFORM_LEN);
        assert_eq!(&xform[XFORM_POSITION..XFORM_POSITION + 16], &[0u8; 16]);
        assert_eq!(
            &xform[XFORM_ROTATION..XFORM_ROTATION + 16],
            &[0.0f32, 0.0, 0.0, 1.0].map(f32::to_le_bytes).concat()
        );
    }
}

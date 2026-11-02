//! The collider mesh build, read from `Overwatch.exe`.
//!
//! The chain is: a brush tree becomes a mesh source (`BrushTreeToMesh`
//! `0x142cf23c0`), the source is thickened into a closed double-sided shell
//! (`MeshToColliderFaces` `0x142cf2010`), the shell's triangles become 0x60-byte
//! face records (`ColliderFaceAppend` `0x142cca460`), those are turned into a
//! BVH (`0x142ccb1b0`) and the BVH is flattened into the 0x30-byte mesh source
//! the type-3 sweep walks (`0x142ccad80`, reader
//! [`crate::collide::mesh_sweep`]).
//!
//! Every stage below follows the client operation for operation. Where a stage
//! is not fully read, the function says so and refuses to guess.

use crate::collide::Quad;
use crate::movement::minss;

/// A 0x60-byte face record (`ColliderFaceAppend`).
pub const FACE_LEN: usize = 0x60;
/// A 0x50-byte BVH build node (`0x142ccb1b0`).
pub const BUILD_NODE_LEN: usize = 0x50;
/// The runtime node the finalizer emits, and the type-3 sweep walks.
pub const RUNTIME_NODE_LEN: usize = 0x20;
/// A runtime triangle record.
pub const TRIANGLE_LEN: usize = 0x20;
/// A vertex, one `vec4`.
pub const VERTEX_LEN: usize = 0x10;
/// The mesh source the sweep reads.
pub const SOURCE_LEN: usize = 0x30;

/// `0x142f897c4`: the shell-wall threshold and the BVH flat-cluster threshold.
const SHELL_THRESHOLD: f32 = 0.005;
/// `0x142f80ff8`: one half.
const HALF: f32 = 0.5;
/// `0x14311e0c0`: the leaf box padding floor.
const BOX_PADDING: f32 = 0.01;

/// A mesh source as `BrushTreeToMesh` hands it over: `{+0x00 vertex array,
/// +0x08 vertex count, +0x10 face records, +0x18 face count}`. A face record is
/// three 64-bit entries at `+0x00`, `+0x08`, `+0x10` whose low dword is a vertex
/// index and whose high dword is `-1` when the sub-face exists.
#[derive(Clone, Copy, Debug, Default)]
pub struct SourceFace {
    pub index: [i32; 3],
    pub present: [bool; 3],
}

/// One index record the shell emits (`0x14` bytes in the client): three vertex
/// indices plus the surface tag and the two id halves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShellIndex {
    pub index: [i32; 3],
    pub mode: u16,
    pub a: u16,
    pub b: u16,
}

/// The shell's two arrays: the vertex block (expanded then shrunk) and the
/// index records.
#[derive(Clone, Debug, Default)]
pub struct Shell {
    pub vertices: Vec<[f32; 4]>,
    pub indices: Vec<ShellIndex>,
}

/// A 0x60-byte face record, in the client's field order.
#[derive(Clone, Copy, Debug)]
pub struct Face {
    pub min: [f32; 4],
    pub max: [f32; 4],
    pub centroid: [f32; 4],
    pub marker: i32,
    pub area: f32,
    pub index: [i32; 3],
    pub mode: u16,
    pub a: u16,
    pub b: u16,
    /// The slot this face came from in the caller's array. The builder
    /// permutes the array in place; this keeps the identity of each record.
    pub source: usize,
    /// The bin this face falls into on the split axis (`+0x30`, written by the
    /// builder and meaningless before a split).
    pub bucket: i32,
    /// The record's `+0x54` word — the third `u16` of `+0x50`, i.e. `b`. The
    /// BVH ORs it into each node's `+0x44`, which the finalizer copies to the
    /// runtime node's `+0x1c` layer mask. The neighbour links at
    /// `+0x3c..+0x4c` have no known reader and are not modelled.
    pub id_bits: u16,
}

impl Face {
    pub fn width(&self) -> [f32; 3] {
        [
            self.max[0] - self.min[0],
            self.max[1] - self.min[1],
            self.max[2] - self.min[2],
        ]
    }
}

/// A 0x50-byte BVH build node.
#[derive(Clone, Copy, Debug)]
pub struct BuildNode {
    pub min: [f32; 4],
    pub max: [f32; 4],
    pub first_face: u32,
    pub axis: i32,
    pub first_child: u32,
    pub second_child: u32,
    pub face_count: u32,
    pub height: u32,
    pub id_bits: u16,
}

impl BuildNode {
    fn empty() -> Self {
        Self {
            min: [0.0; 4],
            max: [0.0; 4],
            first_face: 0,
            axis: -1,
            first_child: u32::MAX,
            second_child: u32::MAX,
            face_count: 0,
            height: 0,
            id_bits: 0,
        }
    }

    pub fn is_leaf(&self) -> bool {
        self.face_count != 0
    }
}

/// The flattened mesh source the type-3 sweep reads.
#[derive(Clone, Debug, Default)]
pub struct ColliderMesh {
    /// `0x20`-byte runtime nodes.
    pub nodes: Vec<u8>,
    /// `0x10`-stride vertices.
    pub vertices: Vec<u8>,
    /// `0x20`-byte triangle records.
    pub triangles: Vec<u8>,
    pub build_node_count: u32,
    pub vertex_count: u32,
    pub face_count: u32,
    pub area: f32,
}

// ---------------------------------------------------------------------------
// `0x142cd56e0`: the per-triangle skip test MeshToColliderFaces applies.
// ---------------------------------------------------------------------------

/// The same test as [`crate::collide::triangle_degenerate`], working on the
/// three corners directly. Kept here so the build reads as one chain.
fn skip_triangle(corners: [Quad; 3]) -> bool {
    crate::collide::triangle_degenerate(corners[0], corners[1], corners[2]) != 0
}

/// Twice the triangle area, or `None` when the triangle is skipped.
fn triangle_double_area(corners: [Quad; 3]) -> Option<f32> {
    if skip_triangle(corners) {
        return None;
    }
    let e0 = sub_quad(corners[1], corners[0]);
    let e1 = sub_quad(corners[2], corners[0]);
    let cross = rot3(sub_quad(mul_quad(rot3(e1), e0), mul_quad(rot3(e0), e1)));
    let length = fold_add(mul_quad(cross, cross)).sqrt();
    Some(length)
}

// ---------------------------------------------------------------------------
// `0x142cf2010`: the double-sided shell.
// ---------------------------------------------------------------------------

/// `0x142cf2010` `MeshToColliderFaces`: thicken a mesh source into the closed
/// shell `ColliderFaceAppend` consumes.
///
/// The vertex block holds every source vertex twice: the first half is the
/// vertex with **z** raised by `epsilon`, the second half with z lowered. x, y
/// and w are passed through `+ 0.0` / `- 0.0` respectively, which is a no-op
/// except that it turns `-0.0` into `+0.0` in the first half and keeps it in the
/// second. The index array is three blocks in this order: every source face
/// outward (indices as written), every source face inward (reversed, second
/// vertex block), and — only when `epsilon >= 0.005` — one stitching pair per
/// marked edge. Each record's `+0x04` mode is 0 and its id word is the surface
/// id the builder passes in.
pub fn shell(
    source_vertices: &[[f32; 4]],
    source_faces: &[SourceFace],
    epsilon: f32,
    id: u16,
) -> Shell {
    let count = source_vertices.len();
    let n = count as i32;
    let mut vertices = Vec::with_capacity(count * 2);
    for v in source_vertices {
        vertices.push([v[0] + 0.0, v[1] + 0.0, v[2] + epsilon, v[3] + 0.0]);
    }
    for v in source_vertices {
        vertices.push([v[0] - 0.0, v[1] - 0.0, v[2] - epsilon, v[3] - 0.0]);
    }

    let walls = epsilon >= SHELL_THRESHOLD;
    let record = |index: [i32; 3]| ShellIndex {
        index,
        mode: 0,
        a: id,
        b: 0,
    };
    let mut indices = Vec::with_capacity(source_faces.len() * 2);
    for face in source_faces {
        indices.push(record(face.index));
    }
    for face in source_faces {
        let [i0, i1, i2] = face.index;
        indices.push(record([i2 + n, i1 + n, i0 + n]));
    }
    if walls {
        for face in source_faces {
            for slot in 0..3 {
                if !face.present[slot] {
                    continue;
                }
                let a = face.index[slot];
                let b = face.index[(slot + 1) % 3];
                indices.push(record([a, a + n, b + n]));
                indices.push(record([b + n, b, a]));
            }
        }
    }
    Shell { vertices, indices }
}

// ---------------------------------------------------------------------------
// `0x142cca460`: face records.
// ---------------------------------------------------------------------------

/// The face records of one index list, skipping degenerate triangles.
///
/// `0x142cca460` walks the `0x14`-byte index records, reads the three vertices,
/// skips the record when `ColliderTriangleDegenerate` says so, and appends the
/// `0x60`-byte record below. It also accumulates the triangle area.
pub fn faces(vertices: &[[f32; 4]], indices: &[ShellIndex]) -> (Vec<Face>, f32) {
    let mut out = Vec::with_capacity(indices.len());
    let mut area = 0.0f32;
    for item in indices {
        let corners = [
            vertices[item.index[0] as usize],
            vertices[item.index[1] as usize],
            vertices[item.index[2] as usize],
        ];
        let Some(double) = triangle_double_area(corners) else {
            continue;
        };
        let min = quad_min(quad_min(corners[0], corners[1]), corners[2]);
        let max = quad_max(quad_max(corners[0], corners[1]), corners[2]);
        let centroid = mul(add_quad(min, max), HALF);
        let triangle_area = double * HALF;
        area += triangle_area;
        // The record's `+0x54` word is `b`; the BVH ORs it into `+0x44`.
        let id_bits = item.b;
        out.push(Face {
            min,
            max,
            centroid,
            marker: -1,
            area: triangle_area,
            index: item.index,
            mode: item.mode,
            a: item.a,
            b: item.b,
            source: 0,
            bucket: 0,
            id_bits,
        });
    }
    (out, area)
}

// ---------------------------------------------------------------------------
// `0x142ccb1b0`: the BVH builder.
// ---------------------------------------------------------------------------

/// How many faces a leaf holds.
pub const LEAF_SIZE: usize = 4;
/// The client's bin count (`ECX = 4` passes of 16 buckets).
const BIN_COUNT: usize = 64;
/// `0x14305af0c`: the bin scale.
const BIN_SCALE: f32 = 64.0;

/// Surface area of a box, as the client computes it: `x*(z+y) + z*y`, doubled.
fn box_area(min: Quad, max: Quad) -> f32 {
    let width = sub_quad(max, min);
    let (x, y, z) = (width[0], width[1], width[2]);
    let half = x * (z + y) + z * y;
    half + half
}

/// A build stage whose client code has not been read. The build stops rather
/// than inventing the missing rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unread {
    /// The surface-area split (`0x142ccb462`-`0x142ccbd37`) found no boundary
    /// with faces on both sides. The client's recovery from that is unread.
    EmptyBinSplit,
}

/// `0x142ccb1b0`: build the BVH over `faces` and return the nodes, the root
/// being index 0.
///
/// The client's builder takes the source array in place: a split reorders the
/// face list so each child sees a contiguous run. Rust mirrors that with slice
/// splitting, and the node's `first_face` records the slice offset in the
/// original array.
///
/// Two paths, decided by the *centroid* spread (`0x142ccb2d0`):
/// - every centroid extent below `0.005`: the faces are clustered, so the node
///   splits at the array half and its axis is the largest face-box extent
///   (`0x142ccb314`). This path is implemented.
/// - otherwise the face-bin split runs (`0x142ccb462`). Its binning is read but
///   its cost expression is not, so this path returns [`Unread::BinCost`].
pub fn build(faces: &[Face]) -> Result<(Vec<BuildNode>, Vec<Face>), Unread> {
    let mut faces = faces.to_vec();
    for (slot, face) in faces.iter_mut().enumerate() {
        face.source = slot;
    }
    let mut nodes = Vec::with_capacity(2 * faces.len().max(1));
    build_node(&mut faces, 0, &mut nodes)?;
    Ok((nodes, faces))
}

fn build_node(faces: &mut [Face], base: usize, nodes: &mut Vec<BuildNode>) -> Result<u32, Unread> {
    let index = nodes.len() as u32;
    nodes.push(BuildNode::empty());
    if faces.len() <= LEAF_SIZE {
        let mut min = faces[0].min;
        let mut max = faces[0].max;
        let mut bits = 0u16;
        for face in faces.iter() {
            min = quad_min(min, face.min);
            max = quad_max(max, face.max);
            bits |= face.id_bits;
        }
        nodes[index as usize] = BuildNode {
            min,
            max,
            first_face: base as u32,
            axis: -1,
            first_child: u32::MAX,
            second_child: u32::MAX,
            face_count: faces.len() as u32,
            height: 1,
            id_bits: bits,
        };
        return Ok(index);
    }

    // `0x142ccb2a2`: the centroid union decides which path runs.
    let mut centroid_min = faces[0].centroid;
    let mut centroid_max = faces[0].centroid;
    for face in faces.iter().skip(1) {
        centroid_min = quad_min(centroid_min, face.centroid);
        centroid_max = quad_max(centroid_max, face.centroid);
    }
    let spread = sub_quad(centroid_max, centroid_min);
    if spread[0] >= SHELL_THRESHOLD || spread[1] >= SHELL_THRESHOLD || spread[2] >= SHELL_THRESHOLD
    {
        // `0x142ccb462`: the 64-bin surface-area split.
        let axis = widest_axis(spread);
        let (bucket, split) = bin_split(faces, axis, centroid_min[axis], spread[axis])
            .ok_or(Unread::EmptyBinSplit)?;
        // `0x142ccbc17`: the client partitions by swapping each qualifying face
        // into the next free slot, so the taken faces keep their order and the
        // displaced ones move to the source's old slot (not a stable pass).
        let mut dst = 0usize;
        for src in 0..faces.len() {
            if faces[src].bucket <= bucket {
                faces.swap(dst, src);
                dst += 1;
            }
        }
        let left_count = dst;
        debug_assert_eq!(left_count, split);
        let (left, right) = faces.split_at_mut(left_count);
        let first = build_node(left, base, nodes)?;
        let second = build_node(right, base + left_count, nodes)?;
        let height = nodes[first as usize]
            .height
            .max(nodes[second as usize].height)
            + 1;
        let bits = nodes[first as usize].id_bits | nodes[second as usize].id_bits;
        nodes[index as usize] = BuildNode {
            min: quad_min(nodes[first as usize].min, nodes[second as usize].min),
            max: quad_max(nodes[first as usize].max, nodes[second as usize].max),
            first_face: 0,
            axis: axis as i32,
            first_child: first,
            second_child: second,
            face_count: 0,
            height,
            id_bits: bits,
        };
        return Ok(index);
    }

    // `0x142ccb314`: the clustered path, box union then array half.
    let mut min = faces[0].min;
    let mut max = faces[0].max;
    for face in faces.iter().skip(1) {
        min = quad_min(min, face.min);
        max = quad_max(max, face.max);
    }
    let width = sub_quad(max, min);
    let axis = if width[0] > width[1] {
        if width[0] > width[2] {
            0
        } else {
            2
        }
    } else if width[1] > width[2] {
        1
    } else {
        2
    };

    let split = (faces.len() / 2).clamp(1, faces.len() - 1);
    let (left, right) = faces.split_at_mut(split);
    let first = build_node(left, base, nodes)?;
    let second = build_node(right, base + split, nodes)?;
    let height = nodes[first as usize]
        .height
        .max(nodes[second as usize].height)
        + 1;
    let bits = nodes[first as usize].id_bits | nodes[second as usize].id_bits;
    nodes[index as usize] = BuildNode {
        min,
        max,
        first_face: 0,
        axis,
        first_child: first,
        second_child: second,
        face_count: 0,
        height,
        id_bits: bits,
    };
    Ok(index)
}

/// The axis order the client uses (`0x142ccb4b4`, `0x142ccb356`): `x` beats
/// `y`, `y` beats `z`, `z` is the default.
fn widest_axis(width: Quad) -> usize {
    if width[0] > width[1] {
        if width[0] > width[2] {
            0
        } else {
            2
        }
    } else if width[1] > width[2] {
        1
    } else {
        2
    }
}

/// One of the builder's 64 bins: a box and a face count.
#[derive(Clone, Copy)]
struct Bin {
    min: Quad,
    max: Quad,
    count: i32,
}

impl Bin {
    fn empty() -> Self {
        Self {
            min: [f32::INFINITY; 4],
            max: [f32::NEG_INFINITY; 4],
            count: 0,
        }
    }

    fn add(&mut self, face: &Face) {
        self.min = quad_min(self.min, face.min);
        self.max = quad_max(self.max, face.max);
        self.count += 1;
    }

    fn add_bin(&mut self, other: &Bin) {
        self.min = quad_min(self.min, other.min);
        self.max = quad_max(self.max, other.max);
        self.count += other.count;
    }

    fn cost(&self) -> f32 {
        if self.count == 0 {
            0.0
        } else {
            box_area(self.min, self.max) * self.count as f32
        }
    }
}

/// `0x142ccb670`: the bin a centroid falls in on one axis.
fn bin_index(centroid: f32, low: f32, extent: f32) -> usize {
    let scale = 1.0f32 / extent;
    let value = ((centroid - low) * BIN_SCALE) * scale;
    let index = value as i32;
    index.clamp(0, (BIN_COUNT - 1) as i32) as usize
}

/// `0x142ccb891`-`0x142ccbb96`: assign every face a bin, accumulate the prefix
/// and suffix boxes, and pick the cheapest boundary.
///
/// Returns `(bucket, left_count)`: every face whose bucket is `<= bucket` goes
/// left. `None` when no boundary leaves faces on both sides.
fn bin_split(faces: &mut [Face], axis: usize, low: f32, extent: f32) -> Option<(i32, usize)> {
    const BINS: usize = 64;
    let mut bins = [Bin::empty(); BINS];
    for face in faces.iter_mut() {
        let index = bin_index(face.centroid[axis], low, extent);
        face.bucket = index as i32;
        bins[index].add(face);
    }

    let mut prefix = [Bin::empty(); BINS];
    let mut running = Bin::empty();
    for index in 0..BINS {
        running.add_bin(&bins[index]);
        prefix[index] = running;
    }
    let mut suffix = [Bin::empty(); BINS];
    let mut running = Bin::empty();
    for index in (0..BINS).rev() {
        running.add_bin(&bins[index]);
        suffix[index] = running;
    }

    let mut best_cost = f32::INFINITY;
    let mut best: Option<(i32, usize)> = None;
    for index in 0..BINS - 1 {
        let left = prefix[index];
        let right = suffix[index + 1];
        if left.count == 0 || right.count == 0 {
            continue;
        }
        let cost = left.cost() + right.cost();
        if cost < best_cost {
            best_cost = cost;
            best = Some((index as i32, left.count as usize));
        }
    }
    best
}

/// Write the `0x50`-byte build nodes in the client's field order.
///
/// The client keeps an absolute face pointer at `+0x28`; `face_base` is the
/// address the face array starts at, so a harness fixture can reproduce the
/// same numbers.
pub fn write_build_nodes(nodes: &[BuildNode], face_base: u64, out: &mut [u8]) {
    for (slot, node) in nodes.iter().enumerate() {
        let at = slot * BUILD_NODE_LEN;
        for lane in 0..4 {
            put_f32(out, at + lane * 4, node.min[lane]);
            put_f32(out, at + 0x10 + lane * 4, node.max[lane]);
        }
        out[at + 0x20..at + 0x30].fill(0);
        if node.is_leaf() {
            let pointer = face_base + u64::from(node.first_face) * FACE_LEN as u64;
            out[at + 0x28..at + 0x30].copy_from_slice(&pointer.to_le_bytes());
        }
        out[at + 0x30..at + 0x34].copy_from_slice(&node.axis.to_le_bytes());
        out[at + 0x34..at + 0x38].copy_from_slice(&node.first_child.to_le_bytes());
        out[at + 0x38..at + 0x3c].copy_from_slice(&node.second_child.to_le_bytes());
        out[at + 0x3c..at + 0x40].copy_from_slice(&node.face_count.to_le_bytes());
        out[at + 0x40..at + 0x44].copy_from_slice(&node.height.to_le_bytes());
        out[at + 0x44..at + 0x46].copy_from_slice(&node.id_bits.to_le_bytes());
    }
}

// ---------------------------------------------------------------------------
// `0x142ccad80`: the flattening to the runtime source.
// ---------------------------------------------------------------------------

/// `0x142ccad80`: turn the build nodes into the `0x30`-byte source.
///
/// Each build node becomes one runtime `0x20`-byte node: `+0x00` min, `+0x0c`
/// max, `+0x18` the leaf's triangle base or the internal node's second child,
/// `+0x1c` the id mask, `+0x1e` the axis and `+0x1f` the leaf's triangle count.
/// Every box is padded by at least `0.01` per side.
pub fn finalize(
    nodes: &[BuildNode],
    faces: &[Face],
    vertices: &[[f32; 4]],
    area: f32,
) -> ColliderMesh {
    // The walk expects an internal node's first child to be physically next to
    // it. The client flattens the build tree in its own order; this mirrors
    // "children in visit order, parent before its subtree", which the
    // differential unit confirms.
    let mut ordered = Vec::with_capacity(2 * faces.len() + 1);
    let mut remap = vec![0u32; nodes.len()];
    flatten(nodes, 0, &mut ordered, &mut remap);

    let mut mesh = ColliderMesh {
        nodes: vec![0u8; ordered.len() * RUNTIME_NODE_LEN],
        vertices: vec![0u8; vertices.len() * VERTEX_LEN],
        triangles: vec![0u8; faces.len() * TRIANGLE_LEN],
        build_node_count: nodes.len() as u32,
        vertex_count: vertices.len() as u32,
        face_count: faces.len() as u32,
        area,
    };

    // Vertices: `w` is forced to zero.
    for (slot, vertex) in vertices.iter().enumerate() {
        let at = slot * VERTEX_LEN;
        for lane in 0..3 {
            put_f32(&mut mesh.vertices, at + lane * 4, vertex[lane]);
        }
    }

    // Triangles: the leaf's faces in face-array order.
    for (slot, face) in faces.iter().enumerate() {
        let at = slot * TRIANGLE_LEN;
        for lane in 0..3 {
            mesh.triangles[at + lane * 4..at + lane * 4 + 4]
                .copy_from_slice(&face.index[lane].to_le_bytes());
        }
        mesh.triangles[at + 0x18..at + 0x1a].copy_from_slice(&face.a.to_le_bytes());
        mesh.triangles[at + 0x1a..at + 0x1c].copy_from_slice(&face.b.to_le_bytes());
    }

    for (slot, node) in ordered.iter().enumerate() {
        let build = &nodes[*node as usize];
        let at = slot * RUNTIME_NODE_LEN;
        let padding = BOX_PADDING.max(SHELL_THRESHOLD);
        for lane in 0..3 {
            let low = build.min[lane] - padding;
            let high = build.max[lane] + padding;
            put_f32(&mut mesh.nodes, at + lane * 4, low);
            put_f32(&mut mesh.nodes, at + 0x0c + lane * 4, high);
        }
        let slot_value = if build.is_leaf() {
            build.first_face
        } else {
            remap[build.second_child as usize]
        };
        mesh.nodes[at + 0x18..at + 0x1c].copy_from_slice(&slot_value.to_le_bytes());
        mesh.nodes[at + 0x1c..at + 0x1e].copy_from_slice(&build.id_bits.to_le_bytes());
        mesh.nodes[at + 0x1e] = if build.is_leaf() { 3 } else { build.axis as u8 };
        mesh.nodes[at + 0x1f] = if build.is_leaf() {
            build.face_count as u8
        } else {
            0
        };
    }
    mesh
}

/// Visit-order flattening: a node then its first child's subtree then its
/// second child's subtree, so the first child always sits at `node + 0x20`.
fn flatten(nodes: &[BuildNode], node: u32, out: &mut Vec<u32>, remap: &mut [u32]) {
    let slot = out.len() as u32;
    out.push(node);
    remap[node as usize] = slot;
    if !nodes[node as usize].is_leaf() {
        let first = nodes[node as usize].first_child;
        let second = nodes[node as usize].second_child;
        flatten(nodes, first, out, remap);
        flatten(nodes, second, out, remap);
    }
}

fn put_f32(bytes: &mut [u8], at: usize, value: f32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

// ---------------------------------------------------------------------------
// The whole chain.
// ---------------------------------------------------------------------------

/// Build a collider source from a mesh source, the way `GeometryBuild`
/// (`0x14213c440`) does for a brush: shell, face records, BVH, flatten.
///
/// `epsilon` is the caller's thickening radius. Returns `None` when the source
/// produces no faces.
pub fn build_source(
    source_vertices: &[[f32; 4]],
    source_faces: &[SourceFace],
    epsilon: f32,
) -> Option<ColliderMesh> {
    build_source_with_ids(source_vertices, source_faces, epsilon, |index| ShellIndex {
        index,
        mode: 0,
        a: 0,
        b: 0,
    })
}

/// [`build_source`] with the shell's per-record `mode`/`a`/`b` supplied by the
/// caller, which the brush path fills from the brush plane.
pub fn build_source_with_ids(
    source_vertices: &[[f32; 4]],
    source_faces: &[SourceFace],
    epsilon: f32,
    tag: impl Fn([i32; 3]) -> ShellIndex,
) -> Option<ColliderMesh> {
    let mut shell = shell(source_vertices, source_faces, epsilon, 0);
    for item in shell.indices.iter_mut() {
        let index = item.index;
        *item = tag(index);
    }
    let (records, area) = faces(&shell.vertices, &shell.indices);
    if records.is_empty() {
        return None;
    }
    let (nodes, ordering) = build(&records).ok()?;
    Some(finalize(&nodes, &ordering, &shell.vertices, area))
}

// ---------------------------------------------------------------------------
// vec4 helpers with the client's operation order.
// ---------------------------------------------------------------------------

fn add_quad(a: Quad, b: Quad) -> Quad {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3]]
}

fn sub_quad(a: Quad, b: Quad) -> Quad {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2], a[3] - b[3]]
}

fn mul(v: Quad, scale: f32) -> Quad {
    [v[0] * scale, v[1] * scale, v[2] * scale, v[3] * scale]
}

fn mul_quad(a: Quad, b: Quad) -> Quad {
    [a[0] * b[0], a[1] * b[1], a[2] * b[2], a[3] * b[3]]
}

/// `SHUFPS x, x, 0xc9`: `[y, z, x, w]`.
fn rot3(v: Quad) -> Quad {
    [v[1], v[2], v[0], v[3]]
}

fn quad_min(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        minss(a[0], b[0]),
        minss(a[1], b[1]),
        minss(a[2], b[2]),
        minss(a[3], b[3]),
    ]
}

fn quad_max(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        maxss_bits(a[0], b[0]),
        maxss_bits(a[1], b[1]),
        maxss_bits(a[2], b[2]),
        maxss_bits(a[3], b[3]),
    ]
}

/// `MAXPS`'s operand rule: `a > b ? a : b` written from `minss`.
fn maxss_bits(a: f32, b: f32) -> f32 {
    if a > b {
        a
    } else {
        b
    }
}

fn fold_add(v: [f32; 4]) -> f32 {
    (v[0] + v[1]) + v[2]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad(z: f32) -> (Vec<[f32; 4]>, Vec<SourceFace>) {
        let vertices = vec![
            [0.0, 0.0, z, 1.0],
            [1.0, 0.0, z, 1.0],
            [1.0, 1.0, z, 1.0],
            [0.0, 1.0, z, 1.0],
        ];
        let faces = vec![
            SourceFace {
                index: [0, 1, 2],
                present: [false; 3],
            },
            SourceFace {
                index: [0, 2, 3],
                present: [false; 3],
            },
        ];
        (vertices, faces)
    }

    #[test]
    fn the_shell_holds_both_surfaces() {
        let (vertices, faces) = quad(0.0);
        let shell = shell(&vertices, &faces, 0.1, 7);
        assert_eq!(shell.vertices.len(), 8);
        // Only z moves: x, y and w go through `+ 0.0` / `- 0.0` unchanged.
        assert_eq!(shell.vertices[0], [0.0, 0.0, 0.1, 1.0]);
        assert_eq!(shell.vertices[4], [0.0, 0.0, -0.1, 1.0]);
        assert_eq!(shell.indices.len(), 4);
        // The outward block first, then the inward block.
        assert_eq!(shell.indices[0].index, [0, 1, 2]);
        assert_eq!(shell.indices[1].index, [0, 2, 3]);
        assert_eq!(shell.indices[2].index, [6, 5, 4]);
        assert_eq!(shell.indices[3].index, [7, 6, 4]);
        for item in &shell.indices {
            assert_eq!(item.mode, 0);
            assert_eq!(item.a, 7);
            assert_eq!(item.b, 0);
        }
    }

    #[test]
    fn a_thick_shell_closes_its_edges() {
        let (vertices, faces) = quad(0.0);
        let mut present = faces.clone();
        for face in present.iter_mut() {
            face.present = [true; 3];
        }
        let shell = shell(&vertices, &present, 0.01, 1);
        // two surfaces plus two wall records per present entry
        assert_eq!(shell.indices.len(), 4 + 12);
        let wall = shell.indices[4];
        assert_eq!(wall.index, [0, 4, 5]);
    }

    #[test]
    fn a_quad_becomes_four_faces() {
        let (vertices, source) = quad(0.0);
        let shell = shell(&vertices, &source, 0.01, 1);
        let (records, area) = faces(&shell.vertices, &shell.indices);
        assert_eq!(records.len(), 4);
        // two unit right triangles per surface, and the same for the shrunk
        // surface: the areas cancel out even though the boxes differ.
        assert_eq!(area, 2.0);
        let e = 0.01f32;
        assert_eq!(records[0].min, [0.0, 0.0, e, 1.0]);
        assert_eq!(records[0].max, [1.0, 1.0, e, 1.0]);
        assert_eq!(records[0].centroid, [0.5, 0.5, e, 1.0]);
        assert_eq!(records[0].area, 0.5);
        assert_eq!(records[0].index, [0, 1, 2]);
    }

    #[test]
    fn the_flat_path_splits_at_the_half() {
        // Ten faces whose centroids all sit in one place: the clustered path.
        let mut faces = Vec::new();
        for slot in 0..10 {
            let width = 0.001 * slot as f32;
            faces.push(Face {
                min: [0.5 - width, 0.5, 0.0, 1.0],
                max: [0.5 + width, 0.5, 0.0, 1.0],
                centroid: [0.5, 0.5, 0.0, 1.0],
                marker: -1,
                area: 0.5,
                index: [0, 1, 2],
                mode: 0,
                a: 0,
                b: 0,
                source: 0,
                bucket: 0,
                id_bits: 0,
            });
        }
        let (nodes, _) = build(&faces).unwrap();
        assert!(!nodes[0].is_leaf());
        assert_eq!(nodes[0].axis, 0);
        assert_eq!(nodes[0].first_child, 1);
        // Ten faces halve to 5+5, then 2+3 and 2+3, and the leaves keep the
        // source order: (base, count) = (0,2), (2,3), (5,2), (7,3).
        let leaves: Vec<(u32, u32)> = nodes
            .iter()
            .filter(|node| node.is_leaf())
            .map(|node| (node.first_face, node.face_count))
            .collect();
        assert_eq!(leaves, vec![(0, 2), (2, 3), (5, 2), (7, 3)]);
        assert_eq!(nodes[0].height, 3);
    }

    #[test]
    fn the_bin_path_splits_at_the_cheapest_bucket() {
        // Two clusters with a gap at x = 4: the cheap split is the gap.
        let xs = [0.0, 1.0, 2.0, 3.0, 7.0, 8.0, 9.0, 10.0];
        let mut faces = Vec::new();
        for (slot, x) in xs.iter().enumerate() {
            faces.push(Face {
                min: [*x - 0.25, 0.0, 0.0, 1.0],
                max: [*x + 0.25, 1.0, 1.0, 1.0],
                centroid: [*x, 0.5, 0.5, 1.0],
                marker: -1,
                area: 0.5,
                index: [0, 1, 2],
                mode: 0,
                a: 0,
                b: 0,
                source: slot,
                bucket: 0,
                id_bits: 0,
            });
        }
        let (nodes, ordering) = build(&faces).unwrap();
        assert!(!nodes[0].is_leaf());
        assert_eq!(nodes[0].axis, 0);
        let first = nodes[nodes[0].first_child as usize];
        let second = nodes[nodes[0].second_child as usize];
        assert_eq!(first.face_count + second.face_count, 8);
        assert_eq!(first.face_count, 4);
        assert_eq!(second.face_count, 4);
        // The first cluster keeps its order; the gap is what moved.
        assert_eq!(
            ordering.iter().map(|face| face.source).collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 4, 5, 6, 7]
        );
    }

    #[test]
    fn a_source_flattens_into_the_runtime_nodes() {
        let (vertices, faces) = quad(0.0);
        let mesh = build_source(&vertices, &faces, 0.01).unwrap();
        assert_eq!(mesh.face_count, 4);
        assert_eq!(mesh.vertex_count, 8);
        assert_eq!(mesh.build_node_count, 1);
        assert_eq!(mesh.nodes.len(), RUNTIME_NODE_LEN);
        // a single leaf holding all four triangles
        assert_eq!(mesh.nodes[0x1f], 4);
        assert_eq!(mesh.nodes[0x18..0x1c], 0u32.to_le_bytes());
        let low = f32::from_le_bytes(mesh.nodes[0x00..0x04].try_into().unwrap());
        let high = f32::from_le_bytes(mesh.nodes[0x0c..0x10].try_into().unwrap());
        // x does not move: only the 0.01 padding widens it.
        assert_eq!(low, -0.01);
        assert_eq!(high, 1.01);
        // z carries the thickening: the shrunk surface at -0.01, plus padding.
        let low_z = f32::from_le_bytes(mesh.nodes[0x08..0x0c].try_into().unwrap());
        let high_z = f32::from_le_bytes(mesh.nodes[0x14..0x18].try_into().unwrap());
        assert_eq!(low_z, -0.02);
        assert_eq!(high_z, 0.02);
        assert_eq!(
            &mesh.triangles[0..12],
            &[0, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0]
        );
    }
}

#[cfg(test)]
mod swap_partition {
    use super::*;

    fn face(centroid_x: f32, half: f32, ident: u16, slot: usize) -> Face {
        Face {
            min: [centroid_x - half, 0.5 - half, 0.5 - half, 1.0],
            max: [centroid_x + half, 0.5 + half, 0.5 + half, 1.0],
            centroid: [centroid_x, 0.5, 0.5, 1.0],
            marker: -1,
            area: 0.5,
            index: [0, 1, 2],
            mode: 0,
            a: 0,
            b: 0,
            source: slot,
            bucket: 0,
            id_bits: ident,
        }
    }

    /// The case that told the stable pass apart from the client's swap pass:
    /// seven small faces in a row plus one huge face at x = 3. The client's
    /// own output puts the huge face fourth and the displaced small face last.
    #[test]
    fn the_client_swaps_rather_than_shifting() {
        let mut faces: Vec<Face> = (0..7).map(|i| face(i as f32, 0.1, i as u16, i)).collect();
        faces.push(face(3.0, 5.0, 99, 7));
        let (nodes, ordering) = build(&faces).unwrap();
        assert_eq!(
            ordering.iter().map(|f| f.source).collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 7, 5, 6, 4]
        );
        assert_eq!(
            ordering.iter().map(|f| f.bucket).collect::<Vec<_>>(),
            vec![0, 10, 21, 0, 0, 42, 63, 21]
        );
        assert_eq!(nodes.len(), 5);
        assert_eq!(nodes[0].axis, 0);
        assert_eq!(nodes[1].face_count, 3);
        assert_eq!(nodes[3].face_count, 2);
        assert_eq!(nodes[4].face_count, 3);
    }
}

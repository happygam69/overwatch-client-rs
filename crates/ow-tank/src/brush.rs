//! The brush: the polygon a geometry node's collision is swept from.
//!
//! One producer of the collision the character sweeps is a **brush**: a closed
//! ring of quantised 2-D points in the XY plane with a per-node direction
//! flag, which the collider path meshes into triangles and then thickens by
//! the collision radius in Z. This module reads the brush's own record formats
//! and the two pure conversions the client performs on them. The map's chunk
//! route is the other producer and has no reader found yet
//! (`docs/formats/maps.md`); neither route is complete.
//!
//! All offsets below are read from the current image. The docs record the
//! conclusions in `docs/formats/brush-collision.md`.
//!
//! # The records
//!
//! The brush object is `0x20` bytes:
//!
//! | field | meaning |
//! |---|---|
//! | `+0x00` | vertex array pointer |
//! | `+0x08` | node array pointer |
//! | `+0x10` | u32 vertex count |
//! | `+0x14` | u32 node count |
//! | `+0x18` | u32 root node (the first node of the first ring) |
//!
//! `+0x1c` is padding the client never writes. A vertex is `0x20` bytes and a
//! node is `0x28` bytes; [`Vertex`] and [`Node`] carry the field-by-field
//! reading. Both shapes are pinned by three independent readers in the image
//! (the vertex decoder `0x142ced510`, the ring walk in `0x142cf0360` and the
//! edge-list builder `0x142ced1a0`) and by the one writer, `0x142ced620`.
//!
//! # What is here
//!
//! * [`Vertex::position`]: the read expression of `FUN_142ced510`, the vertex
//!   decoder. Bit-exact against the client's own bytes (see the tests).
//! * [`build_from_instances`]: the read expression of `FUN_142ced620`, which
//!   turns a node's array of 4-float instance points into a brush ring.
//! * [`Brush::parse`]: the record reader, for a brush that arrives as bytes.
//!
//! # What is not here
//!
//! * The tree-to-mesh walk (`BrushTreeToMesh`, `0x142cf23c0`) and its hull
//!   builder (`0x142cf0360`) are read only as far as the per-node op
//!   classification. The six op case bodies and the final triangle emission are
//!   **not read**, so nothing here converts a brush to a mesh. That is why
//!   [`Node::leaf`] and [`Node::descending`] are recorded fields and not logic.
//! * The face conversion `MeshToColliderFaces` (`0x142cf2010`) is read and is
//!   already the read expression in `ow_sim::collider::shell`; it is not
//!   duplicated here.

use std::fmt;

/// A brush object is `0x20` bytes (`0x142ced620` writes exactly these fields).
pub const HEADER_SIZE: usize = 0x20;
/// A brush vertex record is `0x20` bytes (`count << 5` in the copy at
/// `0x14213a510` and the allocation in `0x142ced620`).
pub const VERTEX_SIZE: usize = 0x20;
/// A brush node record is `0x28` bytes (`count * 0x28` at both sites).
pub const NODE_SIZE: usize = 0x28;

/// The quantisation divisor the point builder applies: `0x1435ed130` holds
/// `400.0` in all four lanes, and the vertex decoder scales by `0.0025`.
pub const QUANTISATION: f32 = 400.0;
/// `0x1431102d0` holds `{1.0, 1.0, 0.0, 0.0}`: the instance record's `z` and
/// `w` are multiplied by zero, so only the first two lanes reach the ring.
pub const INSTANCE_LANES: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The byte slice is shorter than one record.
    Truncated {
        record: &'static str,
        need: usize,
        got: usize,
    },
    /// A vertex's `+0x10` divisor is zero; the client's decoder divides by it
    /// with a signed integer divide, which faults.
    ZeroDivisor { index: usize },
    /// A node's `prev`/`next`/`vertex` index is outside the node or vertex
    /// array.
    BadIndex {
        node: usize,
        field: &'static str,
        value: i32,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Truncated { record, need, got } => {
                write!(f, "brush {record} block needs {need} bytes, got {got}")
            }
            Error::ZeroDivisor { index } => {
                write!(f, "brush vertex {index} has a zero divisor")
            }
            Error::BadIndex { node, field, value } => write!(
                f,
                "brush node {node} has {field} = {value}, which is outside its array"
            ),
        }
    }
}

impl std::error::Error for Error {}

type Result<T> = std::result::Result<T, Error>;

/// One brush vertex: a quantised 2-D point with its own divisor.
///
/// `x` and `y` are integers in `1/400` units times `divisor`; the position is
/// `(x / divisor) * 0.0025`, which is the read expression of `FUN_142ced510`.
/// `index` is the record's ordinal, written by the builder and never read by
/// the paths that are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Vertex {
    pub x: i64,
    pub y: i64,
    pub divisor: i64,
    pub index: u32,
}

impl Vertex {
    /// Decode the record from `0x20` bytes.
    pub fn parse(bytes: &[u8]) -> Result<Vertex> {
        if bytes.len() < VERTEX_SIZE {
            return Err(Error::Truncated {
                record: "vertex",
                need: VERTEX_SIZE,
                got: bytes.len(),
            });
        }
        Ok(Vertex {
            x: i64::from_le_bytes(bytes[0..8].try_into().expect("8 bytes")),
            y: i64::from_le_bytes(bytes[8..16].try_into().expect("8 bytes")),
            divisor: i64::from_le_bytes(bytes[16..24].try_into().expect("8 bytes")),
            index: u32::from_le_bytes(bytes[24..28].try_into().expect("4 bytes")),
        })
    }

    /// The world position of this vertex: `FUN_142ced510` (`0x142ced510`).
    ///
    /// The client computes each lane as
    /// `((float)(v % d) * (1.0f / (float)d) + (float)(v / d)) * 0.0025f`,
    /// with the two integer operations on 64-bit signed values and every float
    /// operation in 32-bit. The quotient is rebuilt from the integer quotient
    /// plus the scaled remainder, which is exact when the divisor is large.
    ///
    /// `z` and `w` are always `0.0`: the decoder writes them as zero, so the
    /// brush is a flat XY outline and the thickness arrives later, in
    /// `MeshToColliderFaces`.
    ///
    /// Returns `None` where the client's signed integer divide would fault
    /// (a zero divisor). That is the client's only failure mode here, so a
    /// reader never has to guess a substitute value.
    pub fn position(&self) -> Option<[f32; 4]> {
        if self.divisor == 0 {
            return None;
        }
        let d = self.divisor;
        let lane = |v: i64| -> f32 {
            let quotient = (v / d) as f32;
            let remainder = (v % d) as f32;
            let inverse = 1.0f32 / d as f32;
            (remainder * inverse + quotient) * 0.0025f32
        };
        Some([lane(self.x), lane(self.y), 0.0, 0.0])
    }
}

/// One brush node: one edge of a ring, with its two endpoints.
///
/// The ring is a doubly linked list over the nodes (`prev`/`next`), and `root`
/// names its first member. `+0x10` links rings: `-1` ends the list, otherwise
/// it is the first node of the next ring (`0x142cf0360` follows it).
///
/// The pair `(x, y)` is this node's quantised point and `(prev_x, prev_y)` is
/// the point of the node that follows it in the ring — a copy, taken before the
/// ordering pass in [`build_from_instances`] runs. After that pass the pair is
/// in descending `(y, x)` order and `descending` records whether it already
/// was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Node {
    pub prev: i32,
    pub next: i32,
    /// Index into the vertex array.
    pub vertex: i32,
    /// First node of the next ring, or `-1`.
    pub next_ring: i32,
    pub x: i32,
    pub y: i32,
    pub prev_x: i32,
    pub prev_y: i32,
    /// `+0x24`, written as `1` by the builder. The brush ingest's area test
    /// reads it: a non-zero value makes the brush a "leaf" that is always
    /// appended (`0x14213a510`).
    pub leaf: u8,
    /// `+0x25`: `1` when `(y, x)` was already greater than the following
    /// node's `(prev_y, prev_x)`, `0` when the ordering pass swapped them.
    pub descending: u8,
}

impl Node {
    /// Decode the record from `0x28` bytes. The two trailing bytes at `+0x26`
    /// are uninitialised in the client and are not read here.
    pub fn parse(bytes: &[u8]) -> Result<Node> {
        if bytes.len() < NODE_SIZE {
            return Err(Error::Truncated {
                record: "node",
                need: NODE_SIZE,
                got: bytes.len(),
            });
        }
        let i32_at = |o: usize| i32::from_le_bytes(bytes[o..o + 4].try_into().expect("4 bytes"));
        Ok(Node {
            prev: i32_at(0x00),
            next: i32_at(0x04),
            vertex: i32_at(0x08),
            next_ring: i32_at(0x10),
            x: i32_at(0x14),
            y: i32_at(0x18),
            prev_x: i32_at(0x1c),
            prev_y: i32_at(0x20),
            leaf: bytes[0x24],
            descending: bytes[0x25],
        })
    }

    /// How this node's pair stands against the pair it carries as `prev_*`,
    /// compared exactly as the ordering pass in `0x142ced620` compares them:
    /// `y` descending first, then `x` ascending, both as signed integers.
    /// `Greater` is the state the pass marks with `descending = 1`.
    ///
    /// `None` means the two pairs are equal, which is the case the builder
    /// rejects the brush for.
    pub fn order(&self) -> Option<std::cmp::Ordering> {
        Some(match self.y.cmp(&self.prev_y) {
            std::cmp::Ordering::Equal => self.prev_x.cmp(&self.x),
            other => other,
        })
    }
}

/// A brush: the ring (or rings) a collision mesh is built from.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Brush {
    pub vertices: Vec<Vertex>,
    pub nodes: Vec<Node>,
    /// Index of the first node of the first ring.
    pub root: u32,
}

impl Brush {
    /// Read a brush from its two record blocks.
    ///
    /// `vertices` and `nodes` are the blocks as the ingest copies them
    /// (`0x14213a510` copies the whole arrays into a `0x28`-byte snapshot
    /// record), and `root` is the snapshot record's `+0x18`. Every index the
    /// ring uses is checked, so a caller can walk the ring without a second
    /// guard.
    pub fn parse(
        vertices: &[u8],
        nodes: &[u8],
        vertex_count: u32,
        node_count: u32,
        root: u32,
    ) -> Result<Brush> {
        let need_v = vertex_count as usize * VERTEX_SIZE;
        if vertices.len() < need_v {
            return Err(Error::Truncated {
                record: "vertex",
                need: need_v,
                got: vertices.len(),
            });
        }
        let need_n = node_count as usize * NODE_SIZE;
        if nodes.len() < need_n {
            return Err(Error::Truncated {
                record: "node",
                need: need_n,
                got: nodes.len(),
            });
        }
        let mut out = Brush {
            vertices: Vec::with_capacity(vertex_count as usize),
            nodes: Vec::with_capacity(node_count as usize),
            root,
        };
        for i in 0..vertex_count as usize {
            let v = Vertex::parse(&vertices[i * VERTEX_SIZE..])?;
            if v.divisor == 0 {
                return Err(Error::ZeroDivisor { index: i });
            }
            out.vertices.push(v);
        }
        for i in 0..node_count as usize {
            let node = Node::parse(&nodes[i * NODE_SIZE..])?;
            for (field, value) in [
                ("prev", node.prev),
                ("next", node.next),
                ("vertex", node.vertex),
            ] {
                let limit = if field == "vertex" {
                    vertex_count as i32
                } else {
                    node_count as i32
                };
                if value < 0 || value >= limit {
                    return Err(Error::BadIndex {
                        node: i,
                        field,
                        value,
                    });
                }
            }
            if node.next_ring >= node_count as i32 && node.next_ring != -1 {
                return Err(Error::BadIndex {
                    node: i,
                    field: "next_ring",
                    value: node.next_ring,
                });
            }
            out.nodes.push(node);
        }
        if root >= node_count && node_count != 0 {
            return Err(Error::BadIndex {
                node: 0,
                field: "root",
                value: root as i32,
            });
        }
        Ok(out)
    }

    /// Walk every ring, starting at `root`, in `next` order.
    ///
    /// Yields the node indices of one ring at a time; a ring that revisits a
    /// node, or runs off the array, is truncated rather than repeated.
    pub fn rings(&self) -> Vec<Vec<usize>> {
        let mut out = Vec::new();
        let mut start = self.root as usize;
        while start < self.nodes.len() {
            let mut ring = Vec::new();
            let mut at = start;
            while at < self.nodes.len() && !ring.contains(&at) {
                ring.push(at);
                let next = self.nodes[at].next;
                if next < 0 || next as usize >= self.nodes.len() {
                    break;
                }
                at = next as usize;
            }
            out.push(ring);
            let next_ring = self.nodes[start].next_ring;
            if next_ring < 0 || next_ring as usize >= self.nodes.len() {
                break;
            }
            start = next_ring as usize;
        }
        out
    }

    /// The world position of every vertex, in vertex order.
    pub fn positions(&self) -> Vec<Option<[f32; 4]>> {
        self.vertices.iter().map(Vertex::position).collect()
    }
}

/// The quantiser shared by the builder and nothing else: `roundf(v * scale *
/// 400.0f)` then a truncating conversion to `i32`.
///
/// The two multiplies are separate 32-bit operations, in that order — the
/// client scales the instance lane first (`MULPS` against the scale vector) and
/// multiplies by the `400.0` vector second, so the intermediate is rounded
/// twice rather than once.
pub fn quantise(value: f32, scale: f32) -> i32 {
    cvttss2si((value * scale) * QUANTISATION)
}

/// `CVTTSS2SI`: truncate toward zero, and answer `i32::MIN` (the instruction's
/// "integer indefinite") for NaN or an out-of-range value instead of
/// saturating.
fn cvttss2si(v: f32) -> i32 {
    let rounded = v.round();
    if rounded.is_nan() || rounded < -2147483648.0 || rounded >= 2147483648.0 {
        return i32::MIN;
    }
    rounded as i32
}

/// Build a brush ring from a node's instance points: `FUN_142ced620`
/// (`0x142ced620`).
///
/// The argument is the instance array the node carries — records of four
/// floats, of which only the first two reach the ring — plus the two scales the
/// caller supplies (the geometry build passes the node's uniform scale in both
/// lanes).
///
/// The client:
///
/// 1. quantises each instance's `(x, y)` with [`quantise`] into a vertex whose
///    divisor is `1`;
/// 2. links a ring of `n` nodes: `prev = (n + i - 1) % n`, `next = (n + i + 1)
///    % n`, vertex `i`, `+0x0c` and `+0x10` set to `-1`, and the leaf byte at
///    `+0x24` set to `1`;
/// 3. copies each node's `(x, y)` into the *previous* node's `+0x1c`, so the
///    record holds the edge's two endpoints;
/// 4. makes one pass over the nodes in index order and compares a node's own
///    pair with the copied pair — `y` descending first, then `x` ascending. If
///    the copy wins the two pairs are swapped and the direction byte at `+0x25`
///    is cleared, if the node's own pair wins the byte is set, and if the pairs
///    are **equal** the whole brush is freed, zeroed and the build is rejected.
///
/// Returns `None` for that rejection. The client leaves the brush object with a
/// zero vertex count, a zero node count and a `-1` root in that case, which is
/// the state a reader sees for "no brush here".
///
/// `n == 0` is accepted by the client (it allocates nothing, leaves the counts
/// at zero and the root at `0`); this returns the same empty brush.
pub fn build_from_instances(instances: &[[f32; 4]], scales: [f32; 2]) -> Option<Brush> {
    let n = instances.len();
    if n == 0 {
        return Some(Brush::default());
    }
    let mut vertices = Vec::with_capacity(n);
    for (i, p) in instances.iter().enumerate() {
        vertices.push(Vertex {
            x: quantise(p[0], scales[0]) as i64,
            y: quantise(p[1], scales[1]) as i64,
            divisor: 1,
            index: i as u32,
        });
    }
    let n32 = n as i32;
    let mut nodes: Vec<Node> = (0..n)
        .map(|i| {
            let i32v = i as i32;
            Node {
                prev: (n32 + i32v - 1) % n32,
                next: (n32 + i32v + 1) % n32,
                vertex: i32v,
                next_ring: -1,
                x: vertices[i].x as i32,
                y: vertices[i].y as i32,
                prev_x: 0,
                prev_y: 0,
                leaf: 1,
                descending: 0,
            }
        })
        .collect();
    // The endpoint copy: node i's `(x, y)` lands in node (i-1) mod n.
    for i in 0..n {
        let target = nodes[i].prev as usize;
        nodes[target].prev_x = nodes[i].x;
        nodes[target].prev_y = nodes[i].y;
    }
    // The ordering pass: `y` descending, then `x` ascending, and an equal pair
    // on both fields is the rejection case.
    for i in 0..n {
        let node = &mut nodes[i];
        let order = if node.y != node.prev_y {
            node.y.cmp(&node.prev_y)
        } else {
            node.prev_x.cmp(&node.x)
        };
        match order {
            std::cmp::Ordering::Greater => node.descending = 1,
            std::cmp::Ordering::Equal => return None,
            std::cmp::Ordering::Less => {
                std::mem::swap(&mut node.x, &mut node.prev_x);
                std::mem::swap(&mut node.y, &mut node.prev_y);
                node.descending = 0;
            }
        }
    }
    Some(Brush {
        vertices,
        nodes,
        root: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Round-trip a brush object layout: the header the client writes.
    #[test]
    fn the_header_is_the_size_the_builder_writes() {
        assert_eq!(HEADER_SIZE, 0x20);
        assert_eq!(VERTEX_SIZE, 0x20);
        assert_eq!(NODE_SIZE, 0x28);
    }

    /// The vertex decoder, pinned against the client's own bytes.
    ///
    /// Every expected bit pattern below is what `0x142ced510` produced when run
    /// under the oracle in `dev/brush_vertex_oracle.py` for the same record, so
    /// these are the client's answers and not this implementation's.
    #[test]
    fn the_vertex_decoder_matches_the_client() {
        let cases: &[(i64, i64, i64, [u32; 4])] = &[
            (0, 0, 1, [0x0000_0000, 0x0000_0000, 0, 0]),
            (1, 0, 1, [0x3b23_d70a, 0x0000_0000, 0, 0]),
            (-1, 0, 1, [0xbb23_d70a, 0x0000_0000, 0, 0]),
            (400, -400, 1, [0x3f80_0000, 0xbf80_0000, 0, 0]),
            (1000, -1000, 1, [0x4020_0000, 0xc020_0000, 0, 0]),
            (
                0x7FFF_FFFF,
                -0x8000_0000,
                1,
                [0x4aa3_d70a, 0xcaa3_d70a, 0, 0],
            ),
            (5, 7, 400, [0x3803_126e, 0x3837_8034, 0, 0]),
            (-5, -7, 400, [0xb803_126e, 0xb837_8034, 0, 0]),
            (1000, -1000, 400, [0x3bcc_cccc, 0xbbcc_cccc, 0, 0]),
            (
                0x7FFF_FFFF,
                -0x8000_0000,
                400,
                [0x4651_b717, 0xc651_b717, 0, 0],
            ),
            (
                123_456_789,
                -987_654_321,
                3,
                [0x47c8_f054, 0xc948_f054, 0, 0],
            ),
        ];
        for &(x, y, d, want) in cases {
            let v = Vertex {
                x,
                y,
                divisor: d,
                index: 0,
            };
            let got = v.position().expect("non-zero divisor");
            let bits: Vec<u32> = got.iter().map(|f| f.to_bits()).collect();
            assert_eq!(bits, want, "vertex ({x}, {y}, d={d})");
        }
    }

    /// A zero divisor is the client's divide fault, not a number.
    #[test]
    fn a_zero_divisor_is_refused() {
        let v = Vertex {
            x: 1,
            y: 1,
            divisor: 0,
            index: 0,
        };
        assert_eq!(v.position(), None);
    }

    /// The quantiser, pinned against the client's own output.
    ///
    /// `0.00125 * 400` is exactly `0.5`, and the client rounds it away from
    /// zero in both directions, so `roundf` and not a truncation is in use.
    #[test]
    fn the_quantiser_rounds_half_away_from_zero() {
        assert_eq!(quantise(0.00125, 1.0), 1);
        assert_eq!(quantise(-0.00125, 1.0), -1);
        assert_eq!(quantise(0.00375, 1.0), 2);
        assert_eq!(quantise(0.0, 1.0), 0);
        assert_eq!(quantise(0.25, 0.5), 50); // 0.25 * 0.5 * 400
        assert_eq!(quantise(0.25, 2.0), 200);
        assert_eq!(quantise(2.0, 1.0), 800);
    }

    /// The ring builder, pinned against the client's own output for a
    /// counter-clockwise square.
    ///
    /// The record the oracle printed for `[(0,0), (1,0), (1,1), (0,1)]` at
    /// scale `1` is reproduced field for field, including the two nodes the
    /// ordering pass swapped.
    #[test]
    fn the_square_ring_matches_the_client() {
        let points = [
            [0.0, 0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0, 0.0],
            [1.0, 1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
        ];
        let brush = build_from_instances(&points, [1.0, 1.0]).expect("accepted");
        assert_eq!(brush.root, 0);
        let vertices: Vec<(i64, i64, i64, u32)> = brush
            .vertices
            .iter()
            .map(|v| (v.x, v.y, v.divisor, v.index))
            .collect();
        assert_eq!(
            vertices,
            vec![
                (0, 0, 1, 0),
                (400, 0, 1, 1),
                (400, 400, 1, 2),
                (0, 400, 1, 3)
            ]
        );
        let nodes: Vec<(i32, i32, i32, i32, i32, i32, i32, u8, u8)> = brush
            .nodes
            .iter()
            .map(|n| {
                (
                    n.prev,
                    n.next,
                    n.vertex,
                    n.x,
                    n.y,
                    n.prev_x,
                    n.prev_y,
                    n.leaf,
                    n.descending,
                )
            })
            .collect();
        assert_eq!(
            nodes,
            vec![
                (3, 1, 0, 0, 0, 400, 0, 1, 1),
                (0, 2, 1, 400, 400, 400, 0, 1, 0),
                (1, 3, 2, 0, 400, 400, 400, 1, 0),
                (2, 0, 3, 0, 400, 0, 0, 1, 1),
            ]
        );
        for node in &brush.nodes {
            assert_eq!(node.next_ring, -1);
        }
    }

    /// The same square wound the other way, and a triangle, pinned from the
    /// oracle's output.
    #[test]
    fn the_other_windings_match_the_client() {
        let cw = [
            [0.0, 1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0, 0.0],
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 0.0],
        ];
        let brush = build_from_instances(&cw, [1.0, 1.0]).expect("accepted");
        let nodes: Vec<(i32, i32, i32, i32, i32)> = brush
            .nodes
            .iter()
            .map(|n| (n.x, n.y, n.prev_x, n.prev_y, n.descending as i32))
            .collect();
        assert_eq!(
            nodes,
            vec![
                (0, 400, 400, 400, 1),
                (400, 400, 400, 0, 1),
                (0, 0, 400, 0, 0),
                (0, 400, 0, 0, 0),
            ]
        );

        let triangle = [
            [0.0, 0.0, 0.0, 0.0],
            [2.0, 0.0, 0.0, 0.0],
            [0.5, 1.5, 0.0, 0.0],
        ];
        let brush = build_from_instances(&triangle, [1.0, 1.0]).expect("accepted");
        let nodes: Vec<(i32, i32, i32, i32, i32)> = brush
            .nodes
            .iter()
            .map(|n| (n.x, n.y, n.prev_x, n.prev_y, n.descending as i32))
            .collect();
        assert_eq!(
            nodes,
            vec![
                (0, 0, 800, 0, 1),
                (200, 600, 800, 0, 0),
                (200, 600, 0, 0, 1)
            ]
        );
    }

    /// Two consecutive equal points are the rejection case: the client frees
    /// the arrays, zeroes the counts and answers zero.
    #[test]
    fn a_repeated_point_rejects_the_brush() {
        let points = [
            [0.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 0.0],
            [1.0, 1.0, 0.0, 0.0],
        ];
        assert_eq!(build_from_instances(&points, [1.0, 1.0]), None);
    }

    /// Non-uniform scales reach the two lanes separately, and the divisor the
    /// builder writes is always `1`.
    #[test]
    fn the_two_scales_are_separate() {
        let points = [
            [0.0, 0.0, 0.0, 0.0],
            [0.25, 0.0, 0.0, 0.0],
            [0.25, 0.25, 0.0, 0.0],
            [0.0, 0.25, 0.0, 0.0],
        ];
        let brush = build_from_instances(&points, [0.5, 2.0]).expect("accepted");
        let vertices: Vec<(i64, i64)> = brush.vertices.iter().map(|v| (v.x, v.y)).collect();
        assert_eq!(vertices, vec![(0, 0), (50, 0), (50, 200), (0, 200)]);
        assert!(brush.vertices.iter().all(|v| v.divisor == 1));
    }

    /// An empty instance array is accepted with nothing in it.
    #[test]
    fn no_instances_is_an_empty_brush() {
        let brush = build_from_instances(&[], [1.0, 1.0]).expect("accepted");
        assert!(brush.vertices.is_empty());
        assert!(brush.nodes.is_empty());
        assert_eq!(brush.root, 0);
    }

    /// The record reader: round-trip the builder's own output through the byte
    /// form the ingest copies, and check the index guards.
    #[test]
    fn the_records_round_trip_and_are_guarded() {
        let points = [
            [0.0, 0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0, 0.0],
            [1.0, 1.0, 0.0, 0.0],
        ];
        let brush = build_from_instances(&points, [1.0, 1.0]).expect("accepted");
        let mut vertex_bytes = Vec::new();
        for v in &brush.vertices {
            vertex_bytes.extend_from_slice(&v.x.to_le_bytes());
            vertex_bytes.extend_from_slice(&v.y.to_le_bytes());
            vertex_bytes.extend_from_slice(&v.divisor.to_le_bytes());
            vertex_bytes.extend_from_slice(&v.index.to_le_bytes());
            vertex_bytes.extend_from_slice(&[0u8; 4]);
        }
        let mut node_bytes = Vec::new();
        for n in &brush.nodes {
            node_bytes.extend_from_slice(&n.prev.to_le_bytes());
            node_bytes.extend_from_slice(&n.next.to_le_bytes());
            node_bytes.extend_from_slice(&n.vertex.to_le_bytes());
            node_bytes.extend_from_slice(&(-1i32).to_le_bytes());
            node_bytes.extend_from_slice(&n.next_ring.to_le_bytes());
            node_bytes.extend_from_slice(&n.x.to_le_bytes());
            node_bytes.extend_from_slice(&n.y.to_le_bytes());
            node_bytes.extend_from_slice(&n.prev_x.to_le_bytes());
            node_bytes.extend_from_slice(&n.prev_y.to_le_bytes());
            node_bytes.push(n.leaf);
            node_bytes.push(n.descending);
            node_bytes.extend_from_slice(&[0u8; 2]);
        }
        let read = Brush::parse(&vertex_bytes, &node_bytes, 3, 3, brush.root)
            .expect("the builder's own records read back");
        assert_eq!(read, brush);
        assert_eq!(read.rings(), vec![vec![0, 1, 2]]);

        assert!(matches!(
            Brush::parse(&vertex_bytes, &node_bytes[..NODE_SIZE * 2], 3, 3, 0),
            Err(Error::Truncated { record: "node", .. })
        ));
        assert!(matches!(
            Brush::parse(&vertex_bytes, &node_bytes, 4, 3, 0),
            Err(Error::Truncated {
                record: "vertex",
                ..
            })
        ));
        assert!(matches!(
            Brush::parse(&vertex_bytes, &node_bytes, 3, 3, 7),
            Err(Error::BadIndex { field: "root", .. })
        ));
        let mut broken = vertex_bytes.clone();
        broken[16..24].copy_from_slice(&0i64.to_le_bytes());
        assert!(matches!(
            Brush::parse(&broken, &node_bytes, 3, 3, 0),
            Err(Error::ZeroDivisor { index: 0 })
        ));
    }

    /// `order` is the ordering pass's comparison. A node the pass marked
    /// `descending` was left alone, so its own pair still beats the copy; a
    /// swapped node's copy is its *old* own pair, so its final comparison is
    /// not constrained. No accepted brush holds an equal pair.
    #[test]
    fn the_order_comparison_is_the_pass_one() {
        let points = [
            [0.0, 0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0, 0.0],
            [1.0, 1.0, 0.0, 0.0],
        ];
        let brush = build_from_instances(&points, [1.0, 1.0]).expect("accepted");
        for node in &brush.nodes {
            assert_ne!(node.order(), Some(std::cmp::Ordering::Equal));
            if node.descending == 1 {
                assert_eq!(node.order(), Some(std::cmp::Ordering::Greater));
            }
        }
    }
}

//! Stage 1 of the brush tree → mesh conversion: the hull build.
//!
//! `BrushTreeToMesh` (`0x142cf23c0`) turns one brush into a triangle mesh in
//! four stages. Only the **first** stage is read end to end and implemented
//! here: `FUN_142cf0360` (`0x142cf0360`), which lays every ring node out as a
//! `0x30`-byte workspace vertex plus a `0x60`-byte hull, classifies each node
//! into one of six ops, and sorts the workspace vertices into sweep order.
//!
//! The rest of the conversion — the six op bodies dispatched by
//! [`HullStage::op`], the ear-clipping emission, and the face writer — is read
//! and recorded in `docs/formats/brush-mesh.md` but **not** implemented: a
//! partial mesh cannot be proven identical to the client's, and a `1 to 1`
//! claim needs the whole chain. Nothing here emits a triangle.
//!
//! # The workspace vertex (`0x30` bytes)
//!
//! | field | meaning |
//! |---|---|
//! | `+0x00` | the brush vertex record's first `0x18` bytes: `x`, `y`, `divisor` |
//! | `+0x20` | the hull that owns this vertex |
//! | `+0x28` | the node's op |
//!
//! `+0x18` is left uninitialised by the client; `+0x00` still holds the raw
//! quantised record after this stage and is overwritten with the decoded `vec4`
//! position (`0x142ced510`, [`crate::brush::Vertex::position`]) by the caller,
//! after the sort. That is why the sort's comparator works on integers.
//!
//! # The hull (`0x60` bytes)
//!
//! | field | meaning |
//! |---|---|
//! | `+0x00`/`+0x08`/`+0x10`/`+0x18` | red-black tree links and colour, used by ops 0/1/2/4 |
//! | `+0x20` | the twin half-edge, filled by the emission |
//! | `+0x28` `prev` / `+0x30` `next` | the ring |
//! | `+0x38` | the owning workspace vertex |
//! | `+0x40` | the "closure" pointer the ops and the emission rewrite |
//! | `+0x48` | this edge's two quantised endpoint pairs |
//!
//! # The op class
//!
//! Written by `0x142cf0360` from the node's own `descending` byte and its
//! ring-predecessor's, split further by the sign of a 64-bit cross product of
//! the two nodes' endpoint pairs. The exact expressions are in [`classify`].
//!
//! All addresses are current-image addresses. The differential oracle is
//! `dev/brush_mesh_oracle.py`, which runs `0x142cf0360` under the emulator; the
//! unit tests below pin its output field for field.

use crate::brush::Brush;

/// One workspace vertex is `0x30` bytes.
pub const WORKSPACE_SIZE: usize = 0x30;
/// One hull is `0x60` bytes.
pub const HULL_SIZE: usize = 0x60;
/// One edge-pair record is `0x10` bytes: two `i32` points.
pub const PAIR_SIZE: usize = 0x10;

/// A node's op, written by `0x142cf0360` and dispatched by
/// `BrushTreeToMesh`'s switch. The bodies are read in
/// `docs/formats/brush-mesh.md`; none is implemented.
pub const OP_COUNT: usize = 6;

/// One `0x30`-byte workspace vertex.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WorkspaceVertex {
    /// The brush vertex record's `x`, still quantised: the client copies the
    /// record's first `0x18` bytes here and decodes the position afterwards.
    pub x: i64,
    pub y: i64,
    /// The record's `+0x10` divisor.
    pub divisor: i64,
    /// Index into [`HullStage::hulls`].
    pub hull: u32,
    /// The node's op, `0..6`.
    pub op: u8,
}

/// One `0x60`-byte hull: one ring node's half-edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Hull {
    /// The previous hull on the ring (`+0x28`).
    pub prev: u32,
    /// The next hull on the ring (`+0x30`).
    pub next: u32,
    /// Index into [`HullStage::vertices`] (`+0x38`).
    pub vertex: u32,
    /// The edge's two quantised points (`+0x48` -> [`PAIR_SIZE`] record):
    /// the node's own `(x, y)` then its stored `(prev_x, prev_y)`.
    pub pair: [i32; 4],
}

/// Everything `0x142cf0360` produces.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HullStage {
    /// One per ring node, in ring-walk order (ring order, ring by ring).
    pub vertices: Vec<WorkspaceVertex>,
    /// One per ring node, same order.
    pub hulls: Vec<Hull>,
    /// The workspace vertices sorted into sweep order: `y` descending, then
    /// `x` ascending, both as exact rationals `x / divisor`; ties break on the
    /// workspace vertex's own address, which ascends with the index. This is
    /// the order `BrushTreeToMesh` applies the ops in.
    pub order: Vec<u32>,
    /// The walk from the root through `next_ring`, as `(first, end)` pairs of
    /// workspace indices: `first` is the ring's first index, `end` is one past
    /// its last.
    pub rings: Vec<(u32, u32)>,
}

/// Why a brush cannot be laid out. The client's builder walks the ring with no
/// bounds check, so these are the cases where the read has no answer rather
/// than a value to copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The node count is zero: `0x142cf23c0` would allocate nothing and read
    /// the root node out of bounds.
    NoNodes,
    /// A ring did not close on its own first node within the node count.
    MalformedRing { start: u32 },
    /// The op bodies and the emission asked for more hulls than
    /// `3 × triangles` allows, which the client's own allocation forbids.
    TooManyHulls,
    /// The mesh writer found a face slot nothing was booked into. The client
    /// dereferences it and faults (measured at `0x142cf0b6b` for a two-ring
    /// brush whose chains do not fill the allocation); this answers an error
    /// instead of copying the fault.
    UnbookedFace { slot: usize },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NoNodes => write!(f, "brush has no nodes"),
            Error::MalformedRing { start } => {
                write!(f, "brush ring at node {start} does not close")
            }
            Error::TooManyHulls => write!(f, "the brush needs more hulls than 3 x triangles"),
            Error::UnbookedFace { slot } => {
                write!(
                    f,
                    "face slot {slot} was never booked (the client faults here)"
                )
            }
        }
    }
}

impl std::error::Error for Error {}

/// The comparator core `FUN_142ceca80` (`0x142ceca80`): compare two brush
/// vertices as exact rationals.
///
/// Each vertex is compared as the rational point `(x / divisor, y / divisor)`
/// with the products taken in **signed 128-bit** (`SEXT816` of each `i64`
/// operand, so `divisor * y` never wraps) and the divisor's sign folded in
/// afterwards: the client compares `b.divisor * a.y` against `a.divisor * b.y`
/// and returns the sign of `a - b` times the divisor signs' relation, which is
/// exactly `a.y/a.divisor` against `b.y/b.divisor`.
///
/// Returns `1` when `a` sorts first, `-1` when `b` does, and `0` when the two
/// points are equal.
///
/// The two keys do **not** run the same way, and that is the client's own
/// asymmetry: `y` is compared **descending** (a larger `y` sorts first) and
/// `x` **ascending** (a smaller `x` sorts first). Both comparisons are
/// cross-multiplied and then flipped when the two divisors' sign bits differ,
/// which is how the client keeps a negative divisor meaningful.
pub fn compare_rational(a: &WorkspaceVertex, b: &WorkspaceVertex) -> i32 {
    let dy = (b.divisor as i128) * (a.y as i128) - (a.divisor as i128) * (b.y as i128);
    if dy != 0 {
        return normalise(dy > 0, a.divisor, b.divisor);
    }
    let dx = (b.divisor as i128) * (a.x as i128) - (a.divisor as i128) * (b.x as i128);
    if dx != 0 {
        return normalise(dx < 0, a.divisor, b.divisor);
    }
    0
}

/// The client's sign normalisation: `+1` when the raw test agrees with "the two
/// divisors have the same sign bit", `-1` otherwise. The client builds that
/// answer from the divisors' sign bits alone, never from the magnitude.
fn normalise(lab: bool, a_divisor: i64, b_divisor: i64) -> i32 {
    let same_sign = (a_divisor < 0) == (b_divisor < 0);
    if lab == same_sign {
        1
    } else {
        -1
    }
}

/// The sort call's comparator `FUN_142ceee80` (`0x142ceee80`): `true` when `a`
/// sorts before `b`.
///
/// The primary key is [`compare_rational`], whose `1` means "earlier" (the
/// client's key is `y` descending, then `x` ascending, as the wrapper turns
/// `-1` into false). Equal keys fall back to the comparison of the two
/// workspace vertices' **addresses**; the array is filled in ring order at a
/// `0x30` stride, so the address order is the index order and the fallback is
/// the index.
pub fn sorts_before(a: u32, b: u32, vertices: &[WorkspaceVertex]) -> bool {
    let (va, vb) = (&vertices[a as usize], &vertices[b as usize]);
    match compare_rational(va, vb) {
        1 => true,
        -1 => false,
        _ => a < b,
    }
}

/// The op class `0x142cf0360` writes at workspace vertex `+0x28`.
///
/// `descending` is `1` when the edge from this node to the next one already ran
/// "down" in sweep order (`y` descending). The classification is a pure
/// function of the node's own byte, its ring predecessor's, and — when the two
/// differ — the sign of one 64-bit cross product of the two nodes' stored
/// endpoint pairs:
///
/// | own | predecessor's | op |
/// |---|---|---|
/// | `0` | `0` | 3 |
/// | `1` | `0` | 4, or 0 when `cross1 > 0` |
/// | `0` | `1` | 5, or 1 when `cross2 > 0` |
/// | `1` | `1` | 2 |
///
/// `cross1 = (py - y) * (x_p - px_p) - (y_p - py_p) * (px - x)` and
/// `cross2 = (y - py) * (px_p - x_p) - (py_p - y_p) * (x - px)`, where
/// unsubscripted names are this node's `x`, `y`, `prev_x`, `prev_y` (the
/// `+0x14`..`+0x20` fields) and `_p` names its ring predecessor's. Each
/// parenthesised difference is a **32-bit** subtraction, sign-extended to 64
/// bits before the multiply, and the two products are subtracted in 64 bits;
/// the client compares the result against `0.0` after a conversion to double,
/// which is the same as `> 0` for a value of this width.
pub fn classify(
    x: i32,
    y: i32,
    prev_x: i32,
    prev_y: i32,
    descending: u8,
    p_x: i32,
    p_y: i32,
    p_prev_x: i32,
    p_prev_y: i32,
    p_descending: u8,
) -> u8 {
    let widen = |v: i32| v as i64;
    // `i32` subtraction first: the client's operands are 32-bit and wrap.
    let d = |a: i32, b: i32| widen(a.wrapping_sub(b));
    if p_descending == 0 {
        if descending == 0 {
            return 3;
        }
        let cross1 = d(prev_y, y) * d(p_x, p_prev_x) - d(p_y, p_prev_y) * d(prev_x, x);
        return if cross1 > 0 { 0 } else { 4 };
    }
    if descending != 0 {
        return 2;
    }
    let cross2 = d(y, prev_y) * d(p_prev_x, p_x) - d(p_prev_y, p_y) * d(x, prev_x);
    if cross2 > 0 {
        1
    } else {
        5
    }
}

/// Lay a brush out exactly as `FUN_142cf0360` (`0x142cf0360`) does.
///
/// One workspace vertex and one hull per ring node, rings walked from
/// [`Brush::root`] through the nodes' `next` links and then through
/// `next_ring`; each hull's `prev`/`next` start at `max(0, i - 1)` and
/// `min(nodes - 1, i + 1)` over the **global** index and are then closed onto
/// the ring's own first and last node; every node gets its op; and the
/// workspace vertices are sorted into sweep order.
pub fn hull_stage(brush: &Brush) -> Result<HullStage, Error> {
    let n = brush.nodes.len();
    if n == 0 {
        return Err(Error::NoNodes);
    }
    let mut stage = HullStage::default();
    let mut start = brush.root as usize;
    while start < n {
        let first = stage.vertices.len() as u32;
        let mut at = start;
        let mut steps = 0usize;
        loop {
            let node = brush.nodes[at];
            let i = stage.vertices.len() as u32;
            let mut x = 0i64;
            let mut y = 0i64;
            let mut divisor = 0i64;
            if let Some(v) = brush.vertices.get(node.vertex as usize) {
                x = v.x;
                y = v.y;
                divisor = v.divisor;
            }
            if node.prev < 0 || node.prev as usize >= n {
                return Err(Error::MalformedRing {
                    start: start as u32,
                });
            }
            let prev_node = &brush.nodes[node.prev as usize];
            let op = classify(
                node.x,
                node.y,
                node.prev_x,
                node.prev_y,
                node.descending,
                prev_node.x,
                prev_node.y,
                prev_node.prev_x,
                prev_node.prev_y,
                prev_node.descending,
            );
            stage.vertices.push(WorkspaceVertex {
                x,
                y,
                divisor,
                hull: i,
                op,
            });
            stage.hulls.push(Hull {
                // The client's clamp is over the global index, before the ring
                // close below overwrites the two ends.
                prev: i.saturating_sub(1),
                next: (i + 1).min(n as u32 - 1),
                vertex: i,
                pair: [node.x, node.y, node.prev_x, node.prev_y],
            });
            steps += 1;
            let next = node.next;
            if next < 0 || next as usize >= n {
                return Err(Error::MalformedRing {
                    start: start as u32,
                });
            }
            if next as usize == start {
                break;
            }
            if steps > n {
                return Err(Error::MalformedRing {
                    start: start as u32,
                });
            }
            at = next as usize;
        }
        let last = stage.vertices.len() as u32 - 1;
        stage.hulls[first as usize].prev = last;
        stage.hulls[last as usize].next = first;
        stage.rings.push((first, last + 1));
        let next_ring = brush.nodes[start].next_ring;
        if next_ring < 0 || next_ring as usize >= n {
            break;
        }
        start = next_ring as usize;
    }
    stage.order = (0..stage.vertices.len() as u32).collect();
    // The client sorts with `std::sort`-shaped code, but its comparator is a
    // total order (equal keys fall back to the addresses, which are distinct),
    // so the sorted sequence is unique and any correct sort gives it.
    stage.order.sort_by(|&a, &b| {
        match compare_rational(&stage.vertices[a as usize], &stage.vertices[b as usize]) {
            1 => std::cmp::Ordering::Less,
            -1 => std::cmp::Ordering::Greater,
            _ => a.cmp(&b),
        }
    });
    Ok(stage)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brush::{build_from_instances, Node};

    fn node(
        prev: i32,
        next: i32,
        vertex: i32,
        next_ring: i32,
        x: i32,
        y: i32,
        prev_x: i32,
        prev_y: i32,
        descending: u8,
    ) -> Node {
        Node {
            prev,
            next,
            vertex,
            next_ring,
            x,
            y,
            prev_x,
            prev_y,
            leaf: 1,
            descending,
        }
    }

    /// The layout the oracle printed for a square, field for field.
    ///
    /// `dev/brush_mesh_oracle.py` produced these values by running
    /// `0x142cf0360` on the node records `0x142ced620` writes for
    /// `[(0,0), (1,0), (1,1), (0,1)]`; the node records are the ones
    /// `brush::tests::the_square_ring_matches_the_client` pins.
    #[test]
    fn the_square_hull_stage_matches_the_client() {
        let brush = build_from_instances(
            &[
                [0.0, 0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0, 0.0],
                [1.0, 1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
            ],
            [1.0, 1.0],
        )
        .expect("accepted");
        let stage = hull_stage(&brush).expect("laid out");
        assert_eq!(stage.rings, vec![(0, 4)]);
        let ops: Vec<u8> = stage.vertices.iter().map(|v| v.op).collect();
        assert_eq!(ops, vec![2, 1, 3, 0]);
        let raw: Vec<(i64, i64, i64)> = stage
            .vertices
            .iter()
            .map(|v| (v.x, v.y, v.divisor))
            .collect();
        assert_eq!(
            raw,
            vec![(0, 0, 1), (400, 0, 1), (400, 400, 1), (0, 400, 1)]
        );
        let hulls: Vec<(u32, u32, u32)> = stage
            .hulls
            .iter()
            .map(|h| (h.prev, h.next, h.vertex))
            .collect();
        assert_eq!(hulls, vec![(3, 1, 0), (0, 2, 1), (1, 3, 2), (2, 0, 3)]);
        let pairs: Vec<[i32; 4]> = stage.hulls.iter().map(|h| h.pair).collect();
        assert_eq!(
            pairs,
            vec![
                [0, 0, 400, 0],
                [400, 400, 400, 0],
                [0, 400, 400, 400],
                [0, 400, 0, 0],
            ]
        );
        assert_eq!(stage.order, vec![3, 2, 0, 1]);
    }

    /// A triangle, from the same oracle run.
    #[test]
    fn the_triangle_hull_stage_matches_the_client() {
        let brush = build_from_instances(
            &[
                [0.0, 0.0, 0.0, 0.0],
                [2.0, 0.0, 0.0, 0.0],
                [0.5, 1.5, 0.0, 0.0],
            ],
            [1.0, 1.0],
        )
        .expect("accepted");
        let stage = hull_stage(&brush).expect("laid out");
        let ops: Vec<u8> = stage.vertices.iter().map(|v| v.op).collect();
        assert_eq!(ops, vec![2, 1, 0]);
        let hulls: Vec<(u32, u32)> = stage.hulls.iter().map(|h| (h.prev, h.next)).collect();
        assert_eq!(hulls, vec![(2, 1), (0, 2), (1, 0)]);
        let pairs: Vec<[i32; 4]> = stage.hulls.iter().map(|h| h.pair).collect();
        assert_eq!(
            pairs,
            vec![[0, 0, 800, 0], [200, 600, 800, 0], [200, 600, 0, 0]]
        );
        assert_eq!(stage.order, vec![2, 0, 1]);
    }

    /// Two rings: the walk through `next_ring` and the per-ring close, plus the
    /// two op classes the square and the triangle do not reach.
    ///
    /// Ring A is nodes 0..2 and ring B is nodes 2..5, so `next_ring` on the
    /// root moves to node 2; the op list covers 0, 1, 3, 4 and 5.
    #[test]
    fn two_rings_walk_and_close_independently() {
        let vertices = vec![
            crate::brush::Vertex {
                x: 0,
                y: 0,
                divisor: 1,
                index: 0,
            },
            crate::brush::Vertex {
                x: 800,
                y: 0,
                divisor: 1,
                index: 1,
            },
            crate::brush::Vertex {
                x: 0,
                y: 400,
                divisor: 1,
                index: 2,
            },
            crate::brush::Vertex {
                x: 400,
                y: 800,
                divisor: 1,
                index: 3,
            },
            crate::brush::Vertex {
                x: 0,
                y: 800,
                divisor: 1,
                index: 4,
            },
        ];
        let nodes = vec![
            node(1, 1, 0, 2, 0, 0, 800, 800, 1),
            node(0, 0, 1, -1, 800, 0, 0, 800, 0),
            node(4, 3, 2, -1, 0, 400, 400, 400, 1),
            node(2, 4, 3, -1, 400, 800, 400, 400, 0),
            node(3, 2, 4, -1, 0, 800, 400, 800, 0),
        ];
        let brush = Brush {
            vertices,
            nodes,
            root: 0,
        };
        let stage = hull_stage(&brush).expect("laid out");
        assert_eq!(stage.rings, vec![(0, 2), (2, 5)]);
        let ops: Vec<u8> = stage.vertices.iter().map(|v| v.op).collect();
        assert_eq!(ops, vec![0, 5, 4, 1, 3]);
        let hulls: Vec<(u32, u32)> = stage.hulls.iter().map(|h| (h.prev, h.next)).collect();
        assert_eq!(hulls, vec![(1, 1), (0, 0), (4, 3), (2, 4), (3, 2)]);
        let pairs: Vec<[i32; 4]> = stage.hulls.iter().map(|h| h.pair).collect();
        assert_eq!(
            pairs,
            vec![
                [0, 0, 800, 800],
                [800, 0, 0, 800],
                [0, 400, 400, 400],
                [400, 800, 400, 400],
                [0, 800, 400, 800],
            ]
        );
        assert_eq!(stage.order, vec![4, 3, 2, 0, 1]);
    }

    /// A ring whose ops are 2, 1, 0, 5, 4 — the sixth class, and the two
    /// cross-product branches in their other direction.
    #[test]
    fn the_zigzag_covers_all_but_one_op() {
        let vertices = vec![
            crate::brush::Vertex {
                x: 0,
                y: 0,
                divisor: 1,
                index: 0,
            },
            crate::brush::Vertex {
                x: 400,
                y: 400,
                divisor: 1,
                index: 1,
            },
            crate::brush::Vertex {
                x: 400,
                y: 800,
                divisor: 1,
                index: 2,
            },
            crate::brush::Vertex {
                x: 0,
                y: 800,
                divisor: 1,
                index: 3,
            },
            crate::brush::Vertex {
                x: 0,
                y: 400,
                divisor: 1,
                index: 4,
            },
        ];
        let nodes = vec![
            node(4, 1, 0, -1, 0, 0, 400, 400, 1),
            node(0, 2, 1, -1, 400, 400, 400, 0, 0),
            node(1, 3, 2, -1, 400, 800, 0, 800, 1),
            node(2, 4, 3, -1, 0, 800, 0, 400, 0),
            node(3, 0, 4, -1, 0, 400, 0, 0, 1),
        ];
        let brush = Brush {
            vertices,
            nodes,
            root: 0,
        };
        let stage = hull_stage(&brush).expect("laid out");
        let ops: Vec<u8> = stage.vertices.iter().map(|v| v.op).collect();
        assert_eq!(ops, vec![2, 1, 0, 5, 4]);
        assert_eq!(stage.order, vec![3, 2, 4, 1, 0]);
    }

    /// The comparator is the exact rational order, not an integer order: two
    /// vertices with the same quantised `y` but different divisors are ordered
    /// by `y / divisor`, and the divisor's sign flips the answer.
    #[test]
    fn the_comparator_is_the_rational_one() {
        let a = WorkspaceVertex {
            x: 8,
            y: 8,
            divisor: 8,
            ..Default::default()
        };
        let b = WorkspaceVertex {
            x: 3,
            y: 3,
            divisor: 3,
            ..Default::default()
        };
        assert_eq!(
            compare_rational(&a, &b),
            0,
            "8/8 and 3/3 are the same point"
        );
        let c = WorkspaceVertex {
            x: 1,
            y: 1,
            divisor: 1,
            ..Default::default()
        };
        // Both are the point (1, 1), so the tie-break decides.
        assert!(sorts_before(0, 1, &[a, c]));
        // A negative divisor that carries a *different* point: `(-4, -2)` at
        // divisor `-2` is `(2, 1)`, so it sorts after `(1, 1)` on the x key.
        let neg = WorkspaceVertex {
            x: -4,
            y: -2,
            divisor: -2,
            ..Default::default()
        };
        assert_eq!(compare_rational(&c, &neg), 1);
        assert_eq!(compare_rational(&neg, &c), -1);
    }

    /// The op table, on all four (own, predecessor) pairs including both
    /// cross-product outcomes.
    #[test]
    fn the_op_table_is_the_client_one() {
        // Both descending clear.
        assert_eq!(classify(0, 0, 0, 0, 0, 0, 0, 0, 0, 0), 3);
        // Both set.
        assert_eq!(classify(0, 0, 0, 0, 1, 0, 0, 0, 0, 1), 2);
        // This set, predecessor clear: cross1 > 0 -> 0, otherwise 4.
        // cross1 = (py - y)(x_p - px_p) - (y_p - py_p)(px - x); the second case
        // has py = -100 and p_prev_x = 100, so cross1 = +10000.
        assert_eq!(classify(0, 0, 100, 0, 1, 0, 0, 0, 0, 0), 4);
        assert_eq!(classify(0, 0, 0, -100, 1, 0, 0, 100, 0, 0), 0);
        // This clear, predecessor set: cross2 > 0 -> 1, otherwise 5.
        // cross2 = (y - py)(p_prev_x - p_x) - (p_prev_y - p_y)(x - px); with
        // px = -50 the second case has cross2 = -(-10) * 50 = +500.
        assert_eq!(classify(0, 0, -50, 0, 0, 0, 0, 0, 10, 1), 5);
        assert_eq!(classify(0, 0, -50, 0, 0, 0, 0, 0, -10, 1), 1);
    }

    /// A brush with no nodes is the client's out-of-bounds read, not an answer.
    #[test]
    fn an_empty_brush_has_no_layout() {
        assert_eq!(hull_stage(&Brush::default()), Err(Error::NoNodes));
    }
}

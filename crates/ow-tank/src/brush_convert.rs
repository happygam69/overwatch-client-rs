//! The brush tree → mesh conversion: the six ops and the emission.
//!
//! `BrushTreeToMesh` (`0x142cf23c0`) turns one brush into a triangle mesh. Its
//! first stage — the hull build, the op class and the sweep order — is
//! [`crate::brush_mesh::hull_stage`]. This module is the rest, read from the
//! image and implemented operation for operation:
//!
//! * the **status tree** (an intrusive red-black tree over hulls) the ops use:
//!   insert `FUN_142cf0900` + fixup `FUN_142cf07d0`, lookup `FUN_142cefd70`,
//!   erase `FUN_142cf0cc0` + rebalance `FUN_142cefe30`, rotations
//!   `0x142cf1060`/`0x142cf10a0`;
//! * the **six ops** (the `switch` in `0x142cf23c0`, op 5 = `FUN_142cf01b0`);
//! * the **emission** `FUN_142ceeee0` (ear tags, ear list, clipping); and
//! * the **face writer** `FUN_142cf0ae0`.
//!
//! # Records
//!
//! The hull is `0x60` bytes; [`Hull`] carries the field-by-field reading. Two
//! fields do double duty, and the phases never overlap, which is why one field
//! each is enough here:
//!
//! * `+0x40` (the *closure*) is a **hull** index while the ops run and a **face
//!   slot** index while the emission runs. The emission clears it first, marks a
//!   visited chain with the face array's base pointer — slot `0` — and then
//!   writes real slots. So `-1` is null, `0` is "the sentinel or slot 0" and
//!   `k > 0` is slot `k`, exactly the client's pointer arithmetic.
//! * `+0x50`/`+0x58` are the emission's scratch ear list, unused by the ops;
//! * the workspace vertex's `+0x28` holds the **op** while the ops run and the
//!   **ear tag** while the emission runs — the emission reuses it, and the op
//!   dispatch has finished by then.
//!
//! # Inputs the client cannot survive
//!
//! Some brushes make the client dereference a null pointer. Each is measured and
//! answered as an [`Error`], never as a mesh the client would not emit: a
//! clockwise ring and a two-node ring both die at `0x142cf265f` (the op-3 lookup
//! answers the null tree root), and a two-ring brush whose chains do not fill the
//! `nodes − 2 + 2·rings` allocation dies at `0x142cf0b6b` in the face writer.
//! `docs/formats/brush-mesh.md` records the measurements.
//!
//! All addresses are current-image addresses.

use crate::brush::Brush;
use crate::brush_mesh::{hull_stage, Error};

/// A hull is `0x60` bytes.
pub const HULL_SIZE: usize = 0x60;

/// A produced mesh: the triangle records `MeshToColliderFaces` (`0x142cf2010`)
/// consumes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Mesh {
    /// One `vec4` per workspace vertex, as bit patterns — `z` and `w` are zero
    /// because the brush is flat and the thickness arrives in the collider.
    pub vertices: Vec<[u32; 4]>,
    /// Six `i32` per triangle: `(i0, h0, i1, h1, i2, h2)` where `i` is a vertex
    /// index and `h` an adjacent half-edge index (`3 · face + edge`) or `-1`
    /// when the edge is open.
    pub triangles: Vec<[i32; 6]>,
}

/// One hull (`0x60`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Hull {
    /// `+0x00` red-black parent, `-1` null.
    parent: i32,
    /// `+0x08` left child.
    left: i32,
    /// `+0x10` right child.
    right: i32,
    /// `+0x18` colour: `0` detached, `1` black, `2` red.
    colour: i32,
    /// `+0x20` the twin half-edge.
    twin: i32,
    /// `+0x28` ring previous.
    prev: i32,
    /// `+0x30` ring next.
    next: i32,
    /// `+0x38` the owning workspace vertex.
    vertex: i32,
    /// `+0x40` closure (a hull during the ops, a face slot during the emission).
    closure: i32,
    /// `+0x48` the edge's two quantised points, `-1` for a spliced hull.
    pair: i32,
    /// `+0x50` the emission's ear list, previous.
    ear_prev: i32,
    /// `+0x58` the emission's ear list, next.
    ear_next: i32,
}

impl Default for Hull {
    fn default() -> Self {
        Hull {
            parent: -1,
            left: -1,
            right: -1,
            colour: 0,
            twin: -1,
            prev: -1,
            next: -1,
            vertex: -1,
            closure: -1,
            pair: -1,
            ear_prev: -1,
            ear_next: -1,
        }
    }
}

/// A workspace vertex (`0x30`) as the conversion sees it after the position
/// decode.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct Ws {
    pos: [f32; 4],
    /// The brush vertex's `x`, `y` and `divisor`, still quantised.
    x: i64,
    y: i64,
    divisor: i64,
    /// `+0x28`: the op, then the ear tag.
    op: i32,
}

struct Ctx {
    ws: Vec<Ws>,
    hull: Vec<Hull>,
    pairs: Vec<[i32; 4]>,
    faces: Vec<i32>,
    groups: Vec<i32>,
    root: i32,
    next_hull: usize,
    next_face: usize,
}

// ---------------------------------------------------------------------------
// Predicates
// ---------------------------------------------------------------------------

/// `FUN_142ced380` (`0x142ced380`): the sweep order — `true` when `a` sorts
/// before `b`.
///
/// `y` descending then `x` ascending, each as the exact rationals `v / divisor`
/// cross-multiplied in signed 128 bits, the answer flipped when the two
/// divisors' sign bits differ. Callers reach it through a 13-byte
/// argument-reorder thunk at `0x142ced450`, so every call in the insert passes
/// its two vertices in the opposite order to its argument list —
/// [`Ctx::insert_goes_right`] calls this with `(later, earlier)` for that reason.
fn sorts_before(a: &Ws, b: &Ws) -> bool {
    let same_sign = (a.divisor < 0) == (b.divisor < 0);
    let dy = (b.divisor as i128) * (a.y as i128) - (a.divisor as i128) * (b.y as i128);
    if dy > 0 {
        return same_sign;
    }
    if dy < 0 {
        return !same_sign;
    }
    let dx = (b.divisor as i128) * (a.x as i128) - (a.divisor as i128) * (b.x as i128);
    if dx < 0 {
        return same_sign;
    }
    if dx > 0 {
        return !same_sign;
    }
    false
}

/// `FUN_142ceca20` (`0x142ceca20`): the two vertices are the same rational
/// point.
fn same_point(a: &Ws, b: &Ws) -> bool {
    (b.divisor as i128) * (a.y as i128) == (a.divisor as i128) * (b.y as i128)
        && (b.divisor as i128) * (a.x as i128) == (a.divisor as i128) * (b.x as i128)
}

/// `FUN_142cecb60` (`0x142cecb60`): which side of the edge `a → b` the
/// homogeneous point `v` is on.
///
/// The client forms `(v.y − a.y·D)·(b.x − a.x) − (v.x − a.x·D)·(b.y − a.y)` in
/// signed 128 bits and answers `+1`/`−1` from that sign divided by the divisor's
/// sign; `0` means collinear.
fn side_edge(a: (i32, i32), b: (i32, i32), v: &Ws) -> i32 {
    let d = v.divisor as i128;
    let t1 = (v.y as i128 - a.1 as i128 * d) * (b.0 as i128 - a.0 as i128);
    let t2 = (v.x as i128 - a.0 as i128 * d) * (b.1 as i128 - a.1 as i128);
    let diff = t1 - t2;
    if diff < 0 {
        if v.divisor < 0 {
            -1
        } else {
            1
        }
    } else if diff == 0 {
        0
    } else if v.divisor < 0 {
        1
    } else {
        -1
    }
}

/// `0x142cecb10` (`0x142cecb10`): the negative of `cross(b − a, c − a)`'s sign,
/// all three points plain `i32` pairs and 64-bit products of sign-extended
/// 32-bit differences.
fn side_tri(a: (i32, i32), b: (i32, i32), c: (i32, i32)) -> i32 {
    let t = (c.1 as i64 - a.1 as i64) * (b.0 as i64 - a.0 as i64)
        - (b.1 as i64 - a.1 as i64) * (c.0 as i64 - a.0 as i64);
    if t > 0 {
        -1
    } else if t < 0 {
        1
    } else {
        0
    }
}

fn pair_point(p: [i32; 4], i: usize) -> (i32, i32) {
    if i == 0 {
        (p[0], p[1])
    } else {
        (p[2], p[3])
    }
}

fn cross2(u: [f32; 4], v: [f32; 4]) -> f32 {
    u[0] * v[1] - u[1] * v[0]
}

/// `perp(x, y) = (y, −x, 0, 0)` — the client's `SHUFPS 0xc9` reached after
/// masking with `(0, 0, 1, 0)`, which is the same 90° rotation.
fn perp(v: [f32; 4]) -> [f32; 4] {
    [v[1], -v[0], 0.0, 0.0]
}

/// The client's three-lane dot: multiply, fold `x + y`, then `+ z`.
fn dot3(a: [f32; 4], b: [f32; 4]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sub(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2], a[3] - b[3]]
}

// ---------------------------------------------------------------------------
// The status tree
// ---------------------------------------------------------------------------

impl Ctx {
    fn pair_of(&self, h: i32) -> [i32; 4] {
        self.pairs[self.hull[h as usize].pair as usize]
    }

    /// `FUN_142cf1060` (`0x142cf1060`): left rotate about `x`. The root slot is
    /// untouched — the caller refreshes it by walking parents, as the client
    /// does.
    fn rotate_left(&mut self, x: i32) {
        let r = self.hull[x as usize].right;
        let rl = self.hull[r as usize].left;
        self.hull[r as usize].left = x;
        let p = self.hull[x as usize].parent;
        self.hull[r as usize].parent = p;
        self.hull[x as usize].right = rl;
        self.hull[x as usize].parent = r;
        if rl >= 0 {
            self.hull[rl as usize].parent = x;
        }
        if p >= 0 {
            if self.hull[p as usize].left == x {
                self.hull[p as usize].left = r;
            } else {
                self.hull[p as usize].right = r;
            }
        }
    }

    /// `0x142cf10a0` (`0x142cf10a0`): right rotate about `x`.
    fn rotate_right(&mut self, x: i32) {
        let l = self.hull[x as usize].left;
        let lr = self.hull[l as usize].right;
        self.hull[l as usize].right = x;
        let p = self.hull[x as usize].parent;
        self.hull[l as usize].parent = p;
        self.hull[x as usize].left = lr;
        self.hull[x as usize].parent = l;
        if lr >= 0 {
            self.hull[lr as usize].parent = x;
        }
        if p >= 0 {
            if self.hull[p as usize].left == x {
                self.hull[p as usize].left = l;
            } else {
                self.hull[p as usize].right = l;
            }
        }
    }

    /// The client's root refresh: follow parents from wherever the root slot
    /// points.
    fn refresh_root(&mut self) {
        let mut top = self.root;
        while top >= 0 {
            let up = self.hull[top as usize].parent;
            if up < 0 {
                break;
            }
            top = up;
        }
        self.root = top;
    }

    /// The insert's key (`FUN_142cf0900`, `0x142cf0900`): does `new` descend to
    /// the right of `node`?
    ///
    /// A hull's *representative* is the lesser of its own workspace vertex and
    /// its `+0x30` neighbour's under the sweep order. Same point → the
    /// orientation test `0x142cecb10`; the node's representative first → the
    /// side-of-edge test on the node's pair; otherwise the same test on the new
    /// hull's pair with the opposite sign.
    fn insert_goes_right(&self, node: i32, new: i32) -> bool {
        let own = self.hull[node as usize].vertex;
        let own_next = self.hull[self.hull[node as usize].next as usize].vertex;
        let newv = self.hull[new as usize].vertex;
        let new_next = self.hull[self.hull[new as usize].next as usize].vertex;
        let a = &self.ws[own as usize];
        let a2 = &self.ws[own_next as usize];
        let b = &self.ws[newv as usize];
        let b2 = &self.ws[new_next as usize];
        let rep_node = if sorts_before(a2, a) { own_next } else { own };
        let rep_new = if sorts_before(b2, b) { new_next } else { newv };
        let rn = &self.ws[rep_node as usize];
        let rx = &self.ws[rep_new as usize];
        if same_point(rn, rx) {
            let np = self.pair_of(node);
            let xp = self.pair_of(new);
            return side_tri(pair_point(np, 1), pair_point(np, 0), pair_point(xp, 1)) == 1;
        }
        if sorts_before(rn, rx) {
            let np = self.pair_of(node);
            return side_edge(pair_point(np, 1), pair_point(np, 0), rx) == 1;
        }
        let xp = self.pair_of(new);
        side_edge(pair_point(xp, 1), pair_point(xp, 0), rn) == -1
    }

    /// `FUN_142cf0900` + `FUN_142cf07d0` (`0x142cf0900`, `0x142cf07d0`): insert
    /// `z` and fix the tree up. Colours are `1` = black, `2` = red; the fixup
    /// stops on a black parent and its rotation branch writes the new local root
    /// black and the old grandparent red.
    fn tree_insert(&mut self, z: i32) {
        let placed = self.root < 0;
        let mut cur = self.root;
        if !placed {
            self.hull[z as usize].parent = -1;
        } else {
            self.root = z;
        }
        while !placed && cur >= 0 {
            let right = self.insert_goes_right(cur, z);
            let child = if right {
                self.hull[cur as usize].right
            } else {
                self.hull[cur as usize].left
            };
            if child < 0 {
                if right {
                    self.hull[cur as usize].right = z;
                } else {
                    self.hull[cur as usize].left = z;
                }
                self.hull[z as usize].parent = cur;
                break;
            }
            cur = child;
        }
        self.hull[z as usize].left = -1;
        self.hull[z as usize].right = -1;
        self.hull[z as usize].colour = 2;

        let mut x = z;
        loop {
            let p = self.hull[x as usize].parent;
            if p < 0 {
                self.hull[x as usize].colour = 1;
                break;
            }
            if self.hull[p as usize].colour == 1 {
                break;
            }
            let gp = self.hull[p as usize].parent;
            if gp < 0 {
                self.hull[x as usize].colour = 1;
                break;
            }
            let uncle = if self.hull[gp as usize].left == p {
                self.hull[gp as usize].right
            } else {
                self.hull[gp as usize].left
            };
            if uncle >= 0 && self.hull[uncle as usize].colour == 2 {
                self.hull[p as usize].colour = 1;
                self.hull[uncle as usize].colour = 1;
                self.hull[gp as usize].colour = 2;
                x = gp;
                continue;
            }
            if self.hull[gp as usize].left == p {
                if self.hull[p as usize].left != x {
                    self.rotate_left(p);
                    x = p;
                }
                let px = self.hull[x as usize].parent;
                let g = self.hull[px as usize].parent;
                self.rotate_right(g);
                self.hull[px as usize].colour = 1;
                self.hull[g as usize].colour = 2;
            } else {
                if self.hull[p as usize].right != x {
                    self.rotate_right(p);
                    x = p;
                }
                let px = self.hull[x as usize].parent;
                let g = self.hull[px as usize].parent;
                self.rotate_left(g);
                self.hull[px as usize].colour = 1;
                self.hull[g as usize].colour = 2;
            }
            break;
        }
        self.refresh_root();
        if self.root >= 0 {
            self.hull[self.root as usize].colour = 1;
        }
    }

    /// `FUN_142cefd70` (`0x142cefd70`): descend by the side-of-edge test and
    /// answer the **last hull visited**. An empty tree answers `-1`, which the
    /// client dereferences (measured at `0x142cf265f`).
    fn tree_lookup(&self, v: i32) -> i32 {
        let mut last = -1;
        let mut cur = self.root;
        while cur >= 0 {
            last = cur;
            let p = self.pair_of(cur);
            let s = side_edge(pair_point(p, 1), pair_point(p, 0), &self.ws[v as usize]);
            cur = if s == 1 {
                self.hull[cur as usize].right
            } else {
                self.hull[cur as usize].left
            };
        }
        last
    }

    /// The client's erase clears only the four tree fields of the removed hull
    /// (`+0x00`, `+0x08`, `+0x10`, `+0x18`); its ring links and vertex stay.
    fn detach(&mut self, h: i32) {
        self.hull[h as usize].parent = -1;
        self.hull[h as usize].left = -1;
        self.hull[h as usize].right = -1;
        self.hull[h as usize].colour = 0;
    }

    fn colour_of(&self, h: i32) -> i32 {
        if h < 0 {
            1
        } else {
            self.hull[h as usize].colour
        }
    }

    /// `FUN_142cf0cc0` (`0x142cf0cc0`): erase `z`.
    ///
    /// Read from the instruction stream. The case split is the root pre-fix; a
    /// leaf (detach; red → done, black → rebalance from a null `x`); one child
    /// (spliced up and blackened); two children (the **successor** — the left
    /// spine of the right subtree — takes `z`'s place, then: successor red →
    /// done, its right child non-null → blackened and done, otherwise rebalance
    /// from that child). The root slot is refreshed afterwards by following
    /// parents, which is the client's own root update.
    fn tree_erase(&mut self, z: i32) {
        if z == self.root {
            let l = self.hull[z as usize].left;
            if l >= 0 {
                self.root = l;
            } else {
                let r = self.hull[z as usize].right;
                self.root = r;
                if r < 0 {
                    self.detach(z);
                    return;
                }
            }
        }
        let left = self.hull[z as usize].left;
        let right = self.hull[z as usize].right;
        if left < 0 && right < 0 {
            let p = self.hull[z as usize].parent;
            if p >= 0 {
                if self.hull[p as usize].left == z {
                    self.hull[p as usize].left = -1;
                } else {
                    self.hull[p as usize].right = -1;
                }
            }
            let was_red = self.hull[z as usize].colour == 2;
            self.detach(z);
            if was_red {
                self.refresh_root();
                return;
            }
            let sibling = if p < 0 {
                -1
            } else if self.hull[p as usize].left >= 0 {
                self.hull[p as usize].left
            } else {
                self.hull[p as usize].right
            };
            if p < 0 {
                return;
            }
            self.rebalance(-1, sibling, p);
            self.refresh_root();
            return;
        }
        if left < 0 || right < 0 {
            let child = if left >= 0 { left } else { right };
            let p = self.hull[z as usize].parent;
            if p >= 0 {
                if self.hull[p as usize].left == z {
                    self.hull[p as usize].left = child;
                } else {
                    self.hull[p as usize].right = child;
                }
            }
            self.hull[child as usize].parent = p;
            self.hull[child as usize].colour = 1;
            self.detach(z);
            self.refresh_root();
            return;
        }
        // Two children: y = the successor.
        let mut y = right;
        while self.hull[y as usize].left >= 0 {
            y = self.hull[y as usize].left;
        }
        let yp = self.hull[y as usize].parent;
        let x = self.hull[y as usize].right;
        let y_was_red = self.hull[y as usize].colour == 2;
        let z_colour = self.hull[z as usize].colour;
        if yp != z {
            if x >= 0 {
                self.hull[x as usize].parent = yp;
            }
            self.hull[yp as usize].left = x;
        }
        let zp = self.hull[z as usize].parent;
        if zp >= 0 {
            if self.hull[zp as usize].left == z {
                self.hull[zp as usize].left = y;
            } else {
                self.hull[zp as usize].right = y;
            }
        }
        self.hull[y as usize].parent = zp;
        self.hull[y as usize].left = left;
        self.hull[left as usize].parent = y;
        if yp != z {
            self.hull[y as usize].right = right;
            self.hull[right as usize].parent = y;
        }
        self.detach(z);
        self.hull[y as usize].colour = z_colour;
        if y_was_red {
            self.refresh_root();
            return;
        }
        if x >= 0 {
            self.hull[x as usize].colour = 1;
            self.refresh_root();
            return;
        }
        let (sibling, parent) = if yp == z {
            (self.hull[y as usize].left, y)
        } else {
            (self.hull[yp as usize].right, yp)
        };
        self.rebalance(x, sibling, parent);
        self.refresh_root();
    }

    /// `FUN_142cefe30` (`0x142cefe30`): the erase's rebalance, called as
    /// `(x, sibling, parent)`.
    ///
    /// A red sibling recolours and rotates the parent, then the sibling is
    /// re-read. Then a **black** parent with a black sibling and two black
    /// nephews recolours the sibling red and moves up, stopping when there is no
    /// grandparent; a **red** parent with that same shape recolours the sibling
    /// red and the parent black and stops; anything else runs the two rotations
    /// that finish the case.
    fn rebalance(&mut self, mut x: i32, mut sibling: i32, mut parent: i32) {
        loop {
            if self.colour_of(sibling) == 2 {
                self.hull[parent as usize].colour = 2;
                self.hull[sibling as usize].colour = 1;
                if self.hull[parent as usize].left == x {
                    self.rotate_left(parent);
                    sibling = self.hull[parent as usize].right;
                } else {
                    self.rotate_right(parent);
                    sibling = self.hull[parent as usize].left;
                }
            }
            if sibling < 0 {
                return;
            }
            let ln = self.hull[sibling as usize].left;
            let rn = self.hull[sibling as usize].right;
            let lc = self.colour_of(ln);
            let rc = self.colour_of(rn);
            let pc = self.hull[parent as usize].colour;
            let sc = self.hull[sibling as usize].colour;
            if pc == 1 {
                if sc != 1 || lc != 1 || rc != 1 {
                    // fall through to the rotations
                } else {
                    self.hull[sibling as usize].colour = 2;
                    x = parent;
                    let up = self.hull[x as usize].parent;
                    if up < 0 {
                        return;
                    }
                    parent = up;
                    sibling = if self.hull[parent as usize].left == x {
                        self.hull[parent as usize].right
                    } else {
                        self.hull[parent as usize].left
                    };
                    continue;
                }
            } else if pc == 2 && sc == 1 && lc == 1 && rc == 1 {
                self.hull[sibling as usize].colour = 2;
                self.hull[parent as usize].colour = 1;
                return;
            }
            if self.hull[parent as usize].left == x {
                if rc == 1 {
                    self.hull[sibling as usize].colour = 2;
                    if ln >= 0 {
                        self.hull[ln as usize].colour = 1;
                    }
                    self.rotate_right(sibling);
                    sibling = self.hull[sibling as usize].left;
                }
                self.hull[sibling as usize].colour = self.hull[parent as usize].colour;
                self.hull[parent as usize].colour = 1;
                let far = self.hull[sibling as usize].right;
                if far >= 0 {
                    self.hull[far as usize].colour = 1;
                }
                self.rotate_left(parent);
                return;
            }
            if self.hull[parent as usize].right == x {
                if lc == 1 {
                    self.hull[sibling as usize].colour = 2;
                    if rn >= 0 {
                        self.hull[rn as usize].colour = 1;
                    }
                    self.rotate_left(sibling);
                    sibling = self.hull[sibling as usize].right;
                }
                self.hull[sibling as usize].colour = self.hull[parent as usize].colour;
                self.hull[parent as usize].colour = 1;
                let far = self.hull[sibling as usize].left;
                if far >= 0 {
                    self.hull[far as usize].colour = 1;
                }
                self.rotate_right(parent);
                return;
            }
            return;
        }
    }

    fn alloc_hull(&mut self) -> Result<i32, Error> {
        if self.next_hull >= self.hull.len() {
            return Err(Error::TooManyHulls);
        }
        let i = self.next_hull;
        self.next_hull += 1;
        self.hull[i] = Hull::default();
        Ok(i as i32)
    }

    fn book_face(&mut self, hull: i32) -> Result<i32, Error> {
        if self.next_face >= self.faces.len() {
            return Err(Error::TooManyHulls);
        }
        let k = self.next_face;
        self.next_face += 1;
        self.faces[k] = hull;
        Ok(k as i32)
    }

    /// The splice shared by ops 1–5: two twins threaded between `h.prev` and the
    /// closure `c`. `a` takes the middle vertex, `b` the closure's, and the
    /// writes are in the client's order — `c.prev` and `h.prev` are read first.
    fn splice(&mut self, h: i32, c: i32) -> Result<(i32, i32), Error> {
        let a = self.alloc_hull()?;
        let b = self.alloc_hull()?;
        let hv = self.hull[h as usize].vertex;
        let cv = self.hull[c as usize].vertex;
        let cold = self.hull[c as usize].prev;
        let hprev = self.hull[h as usize].prev;
        self.hull[a as usize].twin = b;
        self.hull[b as usize].twin = a;
        self.hull[a as usize].vertex = hv;
        self.hull[b as usize].vertex = cv;
        self.hull[a as usize].next = c;
        self.hull[c as usize].prev = a;
        self.hull[a as usize].prev = hprev;
        if hprev >= 0 {
            self.hull[hprev as usize].next = a;
        }
        self.hull[b as usize].next = h;
        self.hull[h as usize].prev = b;
        self.hull[b as usize].prev = cold;
        if cold >= 0 {
            self.hull[cold as usize].next = b;
        }
        Ok((a, b))
    }

    fn op_of(&self, hull: i32) -> i32 {
        let v = self.hull[hull as usize].vertex;
        self.ws[v as usize].op
    }
}

// ---------------------------------------------------------------------------
// The conversion
// ---------------------------------------------------------------------------

/// `BrushTreeToMesh` (`0x142cf23c0`) with the emission variant `GeometryBuild`
/// selects — the node-type-2 route stores `1` in the word the walk reads as
/// `+0x58`.
pub fn tree_to_mesh(brush: &Brush) -> Result<Mesh, Error> {
    tree_to_mesh_variant(brush, 1)
}

/// `BrushTreeToMesh` with an explicit `+0x58`. `0` selects `FUN_142cef7f0`,
/// which is **not** implemented (`docs/formats/brush-mesh.md`); any other value
/// selects `FUN_142ceeee0`.
pub fn tree_to_mesh_variant(brush: &Brush, variant: i32) -> Result<Mesh, Error> {
    if variant == 0 {
        return Err(Error::MalformedRing { start: 0 });
    }
    let stage = hull_stage(brush)?;
    let n = stage.vertices.len();
    if n == 0 {
        return Err(Error::NoNodes);
    }
    // The client counts the rings *after* the root, so a single-ring brush is 0.
    let extra_rings = stage.rings.len().saturating_sub(1);
    // A ring of two nodes gives no triangles at all; the client's allocation is
    // empty and its emission walk runs off the chain (measured: it faults), so
    // there is no mesh to answer with.
    if n + extra_rings * 2 < 3 {
        return Err(Error::MalformedRing { start: 0 });
    }
    let tri = n + extra_rings * 2 - 2;
    let mut ctx = Ctx {
        ws: stage
            .vertices
            .iter()
            .map(|v| Ws {
                pos: [0.0; 4],
                x: v.x,
                y: v.y,
                divisor: v.divisor,
                op: v.op as i32,
            })
            .collect(),
        hull: vec![Hull::default(); tri * 3],
        pairs: Vec::with_capacity(n),
        faces: vec![-1; tri],
        groups: Vec::with_capacity(tri),
        root: -1,
        next_hull: n,
        next_face: 0,
    };
    for (i, h) in stage.hulls.iter().enumerate() {
        ctx.hull[i].prev = h.prev as i32;
        ctx.hull[i].next = h.next as i32;
        ctx.hull[i].vertex = h.vertex as i32;
        ctx.hull[i].pair = i as i32;
        ctx.pairs.push(h.pair);
    }

    // ---- the op dispatch, in sweep order
    for &wi in &stage.order {
        let hull = wi as i32;
        match ctx.ws[wi as usize].op {
            0 => {
                ctx.tree_insert(hull);
                ctx.hull[hull as usize].closure = hull;
            }
            1 => {
                let p = ctx.hull[hull as usize].prev;
                if p < 0 {
                    return Err(Error::MalformedRing { start: 0 });
                }
                let c = ctx.hull[p as usize].closure;
                if c >= 0 && ctx.op_of(c) == 5 {
                    ctx.splice(hull, c)?;
                }
                ctx.tree_erase(p);
            }
            2 => {
                let q = ctx.hull[hull as usize].prev;
                if q < 0 {
                    return Err(Error::MalformedRing { start: 0 });
                }
                let c = ctx.hull[q as usize].closure;
                if c >= 0 && ctx.op_of(c) == 5 {
                    ctx.splice(hull, c)?;
                }
                ctx.tree_erase(q);
                ctx.tree_insert(hull);
                ctx.hull[hull as usize].closure = hull;
            }
            3 => {
                let t = ctx.tree_lookup(hull);
                if t < 0 {
                    return Err(Error::MalformedRing { start: 0 });
                }
                let c = ctx.hull[t as usize].closure;
                if c >= 0 && ctx.op_of(c) == 5 {
                    let (a, _b) = ctx.splice(hull, c)?;
                    ctx.hull[t as usize].closure = a;
                } else {
                    ctx.hull[t as usize].closure = hull;
                }
            }
            4 => {
                let t = ctx.tree_lookup(hull);
                if t < 0 {
                    return Err(Error::MalformedRing { start: 0 });
                }
                let c = ctx.hull[t as usize].closure;
                if c < 0 {
                    return Err(Error::MalformedRing { start: 0 });
                }
                let (a, _b) = ctx.splice(hull, c)?;
                ctx.hull[t as usize].closure = a;
                ctx.tree_insert(hull);
                ctx.hull[hull as usize].closure = hull;
            }
            _ => {
                // op 5: `FUN_142cf01b0` (0x142cf01b0)
                let q = ctx.hull[hull as usize].prev;
                if q < 0 {
                    return Err(Error::MalformedRing { start: 0 });
                }
                let c = ctx.hull[q as usize].closure;
                if c >= 0 && ctx.op_of(c) == 5 {
                    ctx.splice(hull, c)?;
                }
                ctx.tree_erase(q);
                let t = ctx.tree_lookup(hull);
                if t < 0 {
                    return Err(Error::MalformedRing { start: 0 });
                }
                let c2 = ctx.hull[t as usize].closure;
                if c2 >= 0 && ctx.op_of(c2) == 5 {
                    let (a, _b) = ctx.splice(hull, c2)?;
                    ctx.hull[t as usize].closure = a;
                } else {
                    ctx.hull[t as usize].closure = hull;
                }
            }
        }
    }

    // ---- decode every workspace vertex's position (`0x142ced510`)
    for i in 0..n {
        let v = ctx.ws[i];
        let p = crate::brush::Vertex {
            x: v.x,
            y: v.y,
            divisor: v.divisor,
            index: 0,
        };
        ctx.ws[i].pos = p.position().unwrap_or([0.0; 4]);
    }

    // ---- clear, then group the chains
    for h in ctx.hull.iter_mut() {
        h.closure = -1;
    }
    for i in 0..ctx.next_hull {
        if ctx.hull[i].closure >= 0 {
            continue;
        }
        let start = i as i32;
        ctx.groups.push(start);
        let mut at = start;
        loop {
            ctx.hull[at as usize].closure = 0; // the face array's base pointer
            let next = ctx.hull[at as usize].next;
            if next < 0 {
                return Err(Error::MalformedRing { start: 0 });
            }
            if next == start {
                break;
            }
            at = next;
        }
    }

    // ---- triangulate each group
    for g in 0..ctx.groups.len() {
        triangulate_group(&mut ctx, g)?;
    }

    // ---- write the mesh (`FUN_142cf0ae0`)
    write_mesh(&ctx, n, tri)
}

// ---------------------------------------------------------------------------
// The emission
// ---------------------------------------------------------------------------

/// `FUN_142ceeee0` (`0x142ceeee0`): tag a chain's vertices, collect the ears,
/// clip them, then clip what is left.
fn triangulate_group(ctx: &mut Ctx, g: usize) -> Result<(), Error> {
    let start = ctx.groups[g];
    let n = ctx.hull[start as usize].next;
    let p = ctx.hull[start as usize].prev;
    if n < 0 || p < 0 {
        return Err(Error::MalformedRing { start: 0 });
    }
    if ctx.hull[n as usize].next == p {
        // exactly three hulls
        let slot = ctx.book_face(p)?;
        ctx.hull[p as usize].closure = slot;
        ctx.hull[start as usize].closure = slot;
        ctx.hull[n as usize].closure = slot;
        return Ok(());
    }
    // ---- pass 1: tag every vertex and collect the ears
    let mut prev = p;
    let mut cur = start;
    let mut next = n;
    let mut head: i32 = -1;
    let mut count = 0i32;
    loop {
        let cand_from = ctx.hull[next as usize].next;
        let tag = ear_tag(ctx, prev, cur, next, cand_from, prev);
        let cv = ctx.hull[cur as usize].vertex;
        ctx.ws[cv as usize].op = tag;
        ctx.hull[cur as usize].ear_prev = -1;
        ctx.hull[cur as usize].ear_next = -1;
        if tag == 8 {
            head = ear_append(ctx, head, cur);
        }
        count += 1;
        prev = cur;
        cur = next;
        next = ctx.hull[next as usize].next;
        if next < 0 {
            return Err(Error::MalformedRing { start: 0 });
        }
        if cur == start {
            break;
        }
    }
    if head < 0 {
        final_clip(ctx, start, count)?;
        return Ok(());
    }
    // ---- pass 2: clip the collected ears
    let mut remaining = count;
    let mut from;
    let mut e = head;
    loop {
        let en = ctx.hull[e as usize].next;
        let pn = ctx.hull[e as usize].prev;
        let nn = ctx.hull[en as usize].next;
        let pp = ctx.hull[pn as usize].prev;
        if en < 0 || pn < 0 || nn < 0 || pp < 0 {
            return Err(Error::MalformedRing { start: 0 });
        }
        // unlink e from the ear list
        if ctx.hull[e as usize].ear_next == e {
            head = -1;
        } else {
            let ep = ctx.hull[e as usize].ear_prev;
            let ex = ctx.hull[e as usize].ear_next;
            ctx.hull[ex as usize].ear_prev = ep;
            ctx.hull[ep as usize].ear_next = ex;
            head = ex;
        }
        let a = ctx.alloc_hull()?;
        let b = ctx.alloc_hull()?;
        let pv = ctx.hull[pn as usize].vertex;
        let nv = ctx.hull[en as usize].vertex;
        ctx.hull[a as usize].twin = b;
        ctx.hull[b as usize].twin = a;
        ctx.hull[a as usize].vertex = pv;
        ctx.hull[b as usize].vertex = nv;
        let slot = ctx.book_face(pn)?;
        ctx.hull[pn as usize].closure = slot;
        ctx.hull[e as usize].closure = slot;
        ctx.hull[b as usize].closure = slot;
        ctx.hull[e as usize].next = b;
        ctx.hull[b as usize].prev = e;
        ctx.hull[b as usize].next = pn;
        ctx.hull[pn as usize].prev = b;
        ctx.hull[pp as usize].next = a;
        ctx.hull[a as usize].prev = pp;
        ctx.hull[a as usize].next = en;
        ctx.hull[en as usize].prev = a;
        from = a;
        remaining -= 1;
        if remaining == 3 {
            final_clip(ctx, from, remaining)?;
            return Ok(());
        }
        // re-test the triangle at `pn` (the chain is pp → a → en → … → pp, and
        // `a` shares `pn`'s workspace vertex, so the tag lands on the same one)
        let old_p = ctx.ws[pv as usize].op;
        let tag_p = ear_tag(ctx, pp, pn, en, nn, pp);
        ctx.ws[pv as usize].op = tag_p;
        head = ear_update(ctx, head, pn, a, old_p, tag_p, true);
        // re-test the triangle at `en` (the chain is pn → en → nn → … → pn)
        let old_n = ctx.ws[nv as usize].op;
        let nn_next = ctx.hull[nn as usize].next;
        if nn_next < 0 {
            return Err(Error::MalformedRing { start: 0 });
        }
        let tag_n = ear_tag(ctx, pn, en, nn, nn_next, pn);
        ctx.ws[nv as usize].op = tag_n;
        if tag_n == 8 {
            if old_n != 8 {
                head = ear_append(ctx, head, en);
            }
        } else if old_n == 8 {
            if ctx.hull[en as usize].ear_prev == en {
                head = -1;
            } else {
                let ep = ctx.hull[en as usize].ear_prev;
                let ex = ctx.hull[en as usize].ear_next;
                ctx.hull[ep as usize].ear_next = ex;
                ctx.hull[ex as usize].ear_prev = ep;
                if head == en {
                    head = ex;
                }
            }
        }
        if head < 0 {
            final_clip(ctx, from, remaining)?;
            return Ok(());
        }
        e = head;
    }
}

/// The three ear tests, from `0x142ceeee0`'s `SHUFPS` lane order: `6` when the
/// corner is reflex, `7` when another vertex of the chain lies inside the
/// triangle, `8` when it is an ear.
fn ear_tag(ctx: &Ctx, prev: i32, cur: i32, next: i32, mut cand: i32, stop: i32) -> i32 {
    let pv = ctx.hull[prev as usize].vertex as usize;
    let cv = ctx.hull[cur as usize].vertex as usize;
    let nv = ctx.hull[next as usize].vertex as usize;
    let u = sub(ctx.ws[cv].pos, ctx.ws[pv].pos);
    let v = sub(ctx.ws[nv].pos, ctx.ws[cv].pos);
    if cross2(u, v) < 0.0 {
        return 6;
    }
    let pu = perp(u);
    let pvperp = perp(v);
    let pac = perp(sub(ctx.ws[pv].pos, ctx.ws[nv].pos));
    let mut steps = 0usize;
    while cand >= 0 && cand != stop {
        let xv = ctx.hull[cand as usize].vertex as usize;
        let w = sub(ctx.ws[xv].pos, ctx.ws[cv].pos);
        if dot3(w, pu) < 0.0 && dot3(w, pvperp) < 0.0 {
            let far = sub(ctx.ws[xv].pos, ctx.ws[pv].pos);
            if dot3(far, pac) < 0.0 {
                return 7;
            }
        }
        cand = ctx.hull[cand as usize].next;
        steps += 1;
        if steps > ctx.hull.len() {
            break;
        }
    }
    8
}

/// Append `h` to the ear list, a circular list through `+0x50`/`+0x58`.
fn ear_append(ctx: &mut Ctx, head: i32, h: i32) -> i32 {
    if head < 0 {
        ctx.hull[h as usize].ear_next = h;
        ctx.hull[h as usize].ear_prev = h;
        return h;
    }
    let tail = ctx.hull[head as usize].ear_prev;
    ctx.hull[tail as usize].ear_next = h;
    ctx.hull[h as usize].ear_prev = tail;
    ctx.hull[h as usize].ear_next = head;
    ctx.hull[head as usize].ear_prev = h;
    head
}

/// The re-test's ear-list maintenance for the triangle's middle corner.
///
/// When the corner was an ear and still is, the client **moves** the ear link
/// from `node` to `fresh`: after the splice the ring around `node` no longer
/// matches the triangle, while `fresh`'s does (they share the workspace vertex).
fn ear_update(
    ctx: &mut Ctx,
    head: i32,
    node: i32,
    fresh: i32,
    old: i32,
    new: i32,
    move_link: bool,
) -> i32 {
    if new == 8 {
        if old == 8 {
            if move_link && fresh != node {
                let ep = ctx.hull[node as usize].ear_prev;
                let ex = ctx.hull[node as usize].ear_next;
                ctx.hull[fresh as usize].ear_prev = ep;
                ctx.hull[fresh as usize].ear_next = ex;
                if ep >= 0 {
                    ctx.hull[ep as usize].ear_next = fresh;
                }
                if ex >= 0 {
                    ctx.hull[ex as usize].ear_prev = fresh;
                }
                return if head == node { fresh } else { head };
            }
            head
        } else {
            ear_append(ctx, head, fresh)
        }
    } else if old == 8 {
        if ctx.hull[node as usize].ear_prev == node {
            return -1;
        }
        let ep = ctx.hull[node as usize].ear_prev;
        let ex = ctx.hull[node as usize].ear_next;
        if ep >= 0 {
            ctx.hull[ep as usize].ear_next = ex;
        }
        if ex >= 0 {
            ctx.hull[ex as usize].ear_prev = ep;
        }
        if head == node {
            return ex;
        }
        head
    } else {
        head
    }
}

/// The last-three path of `FUN_142ceeee0` (`0x142cef6c4` … `0x142cef7e6`).
fn final_clip(ctx: &mut Ctx, from: i32, count: i32) -> Result<(), Error> {
    let mut rbx = from;
    let mut rdi = ctx.hull[rbx as usize].next;
    let mut remaining = count;
    while remaining > 3 {
        let a = ctx.alloc_hull()?;
        let b = ctx.alloc_hull()?;
        let next_next = ctx.hull[rdi as usize].next;
        if next_next < 0 {
            return Err(Error::MalformedRing { start: 0 });
        }
        ctx.hull[a as usize].twin = b;
        ctx.hull[b as usize].twin = a;
        let av = ctx.hull[next_next as usize].vertex;
        let bv = ctx.hull[rbx as usize].vertex;
        ctx.hull[a as usize].vertex = av;
        ctx.hull[b as usize].vertex = bv;
        let slot = ctx.book_face(rbx)?;
        ctx.hull[rbx as usize].closure = slot;
        ctx.hull[rdi as usize].closure = slot;
        ctx.hull[a as usize].closure = slot;
        ctx.hull[rbx as usize].next = rdi;
        ctx.hull[rdi as usize].prev = rbx;
        ctx.hull[rdi as usize].next = a;
        ctx.hull[a as usize].prev = rdi;
        rdi = next_next;
        ctx.hull[a as usize].next = rbx;
        ctx.hull[rbx as usize].prev = a;
        rbx = b;
        remaining -= 1;
    }
    let r9 = ctx.hull[rdi as usize].next;
    ctx.hull[rbx as usize].next = rdi;
    ctx.hull[rdi as usize].prev = rbx;
    if r9 >= 0 {
        ctx.hull[r9 as usize].next = rbx;
    }
    ctx.hull[rbx as usize].prev = r9;
    let slot = ctx.book_face(rbx)?;
    ctx.hull[rbx as usize].closure = slot;
    if rdi >= 0 {
        ctx.hull[rdi as usize].closure = slot;
    }
    if r9 >= 0 {
        ctx.hull[r9 as usize].closure = slot;
    }
    Ok(())
}

/// `FUN_142cf0ae0` (`0x142cf0ae0`): the mesh the collider reads.
fn write_mesh(ctx: &Ctx, n: usize, tri: usize) -> Result<Mesh, Error> {
    let mut out = Mesh {
        vertices: ctx.ws[..n]
            .iter()
            .map(|v| v.pos.map(f32::to_bits))
            .collect(),
        triangles: Vec::with_capacity(tri),
    };
    for k in 0..tri {
        let h = ctx.faces[k];
        if h < 0 {
            // The client dereferences the unbooked slot and faults at
            // 0x142cf0b6b.
            return Err(Error::UnbookedFace { slot: k });
        }
        let mark = |x: i32| -> i32 {
            if x < 0 {
                return -1;
            }
            let twin = ctx.hull[x as usize].twin;
            if twin < 0 {
                return -1;
            }
            let slot = ctx.hull[twin as usize].closure;
            if slot < 0 {
                return -1;
            }
            let slot_hull = ctx.faces[slot as usize];
            if slot_hull == twin {
                slot * 3
            } else if slot_hull >= 0 && ctx.hull[slot_hull as usize].next == twin {
                slot * 3 + 1
            } else {
                slot * 3 + 2
            }
        };
        let next = ctx.hull[h as usize].next;
        let prev = ctx.hull[h as usize].prev;
        if next < 0 || prev < 0 {
            return Err(Error::MalformedRing { start: 0 });
        }
        out.triangles.push([
            ctx.hull[h as usize].vertex,
            mark(h),
            ctx.hull[next as usize].vertex,
            mark(next),
            ctx.hull[prev as usize].vertex,
            mark(prev),
        ]);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brush::build_from_instances;

    fn brush(points: &[[f32; 2]]) -> Brush {
        let inst: Vec<[f32; 4]> = points.iter().map(|p| [p[0], p[1], 0.0, 0.0]).collect();
        build_from_instances(&inst, [1.0, 1.0]).expect("accepted")
    }

    fn bits(f: f32) -> u32 {
        f.to_bits()
    }

    /// The square, pinned against `dev/brush_mesh_oracle.py`'s run of
    /// `0x142cf23c0` on the same brush. Every vertex bit pattern and every
    /// triangle record, including the half-edge marks.
    #[test]
    fn the_square_mesh_matches_the_client() {
        let m = tree_to_mesh(&brush(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]))
            .expect("converted");
        assert_eq!(
            m.vertices,
            vec![
                [bits(0.0), bits(0.0), 0, 0],
                [bits(1.0), bits(0.0), 0, 0],
                [bits(1.0), bits(1.0), 0, 0],
                [bits(0.0), bits(1.0), 0, 0],
            ]
        );
        assert_eq!(
            m.triangles,
            vec![[3, -1, 0, -1, 1, 3], [3, 2, 1, -1, 2, -1]]
        );
    }

    /// The triangle: the three-node chain path, one face, no markers.
    #[test]
    fn the_triangle_mesh_matches_the_client() {
        let m = tree_to_mesh(&brush(&[[0.0, 0.0], [2.0, 0.0], [0.5, 1.5]])).expect("converted");
        assert_eq!(
            m.vertices,
            vec![
                [bits(0.0), bits(0.0), 0, 0],
                [bits(2.0), bits(0.0), 0, 0],
                [bits(0.5), bits(1.5), 0, 0],
            ]
        );
        assert_eq!(m.triangles, vec![[2, -1, 0, -1, 1, -1]]);
    }

    /// The five-point L: one reflex corner, two clipped ears and the final
    /// clip. This is the case that pins the ear predicate, the ear list, both
    /// re-tests and the interlocking half-edge marks.
    #[test]
    fn the_ell_mesh_matches_the_client() {
        let m = tree_to_mesh(&brush(&[
            [0.0, 0.0],
            [2.0, 0.0],
            [2.0, 1.0],
            [1.0, 1.0],
            [1.0, 2.0],
            [0.0, 2.0],
        ]))
        .expect("converted");
        assert_eq!(
            m.vertices,
            vec![
                [bits(0.0), bits(0.0), 0, 0],
                [bits(2.0), bits(0.0), 0, 0],
                [bits(2.0), bits(1.0), 0, 0],
                [bits(1.0), bits(1.0), 0, 0],
                [bits(1.0), bits(2.0), 0, 0],
                [bits(0.0), bits(2.0), 0, 0],
            ]
        );
        assert_eq!(
            m.triangles,
            vec![
                [5, -1, 0, -1, 1, 3],
                [5, 2, 1, -1, 2, 10],
                [3, -1, 4, -1, 5, 9],
                [3, 8, 5, 5, 2, -1],
            ]
        );
    }

    /// The five-point arrow: one op-5 splice cuts the ring into a four-hull
    /// chain plus a stub, so the emission's tag pass sees four hulls and the
    /// three faces come out of the last-three fan. Pinned against the oracle.
    #[test]
    fn the_arrow_mesh_matches_the_client() {
        let m = tree_to_mesh(&brush(&[
            [0.0, 0.0],
            [2.0, 0.0],
            [2.0, 2.0],
            [1.0, 1.0],
            [0.0, 2.0],
        ]))
        .expect("converted");
        assert_eq!(
            m.triangles,
            vec![
                [3, 6, 0, -1, 1, 3],
                [3, 2, 1, -1, 2, -1],
                [0, 0, 3, -1, 4, -1]
            ]
        );
        assert_eq!(m.vertices.len(), 5);
    }

    /// A clockwise ring: the sweep's first vertex in order is an op 3, the
    /// lookup answers the null tree root, and the client faults at
    /// `0x142cf265f`. Measured; answered as a refusal here.
    #[test]
    fn a_clockwise_ring_is_refused() {
        let b = brush(&[[1.0, 0.0], [0.0, 0.0], [0.0, 1.0]]);
        assert!(tree_to_mesh(&b).is_err());
        let b = brush(&[[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]);
        assert!(tree_to_mesh(&b).is_err());
    }

    /// A two-node ring: the emission's `next.next == prev` test is written for a
    /// three-node chain, the walk runs past it, and the client faults the same
    /// way as for a clockwise ring.
    #[test]
    fn a_two_node_ring_is_refused() {
        let mut b = brush(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        b.nodes.truncate(2);
        b.nodes[0].next = 1;
        b.nodes[1].next = 0;
        b.nodes[0].prev = 1;
        b.nodes[1].prev = 0;
        b.vertices.truncate(2);
        assert!(tree_to_mesh(&b).is_err());
    }

    /// The emission variant the other brush appender can select (`+0x58 == 0`,
    /// `FUN_142cef7f0`) is not implemented, and says so instead of guessing.
    #[test]
    fn the_other_variant_is_not_implemented() {
        let b = brush(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        assert!(tree_to_mesh_variant(&b, 0).is_err());
        assert!(tree_to_mesh_variant(&b, 1).is_ok());
    }
}

# The brush → mesh walk

`BrushTreeToMesh` (`0x142cf23c0`) is the only place a brush becomes triangles.
This file records what was read of it: the **hull stage**, the **status tree**
(insert fixup, erase and rebalance, both rotations, the three-way insert key),
the **six op bodies**, the **emission** (ear predicate, clipping tail, face
writer) and the **model descriptor `C`** whose instance points the whole route
starts from (encoding read, class open). The hull stage and the whole
conversion are implemented, and **all four** pinned meshes reproduce bit for bit
against the client's own code run under the emulator.

It is the continuation of `formats/brush-collision.md`, which stops at the
per-node op classification. Every number below is a current-image address
(`0x140000000` + RVA). Evidence class is marked on each claim: **decompiled** =
read from the Ghidra program `owmod_clean`, `Overwatch.exe`; **emulated** = the
client's own code was executed under `dev/verify_harness/emu_oracle.py` and its
output bytes pinned; **install** = measured on the operator's installation.

## Status in one line

The conversion is **read and implemented**. The hull stage
(`ow_tank::brush_mesh::hull_stage`), the status tree, the six ops and the emission
are in `ow_tank::brush_convert`; the client's own code was run under the emulator
end to end (`dev/brush_mesh_oracle.py`) and **all four** of its meshes reproduce
bit for bit, half-edge marks included:

| brush | client output | port |
|---|---|---|
| square ccw | `(3,−1,0,−1,1,3)`, `(3,2,1,−1,2,−1)` | exact |
| triangle ccw | `(2,−1,0,−1,1,−1)` | exact |
| five-point L | four faces, two clips, interlocking marks | exact |
| five-point arrow | `(3,6,0,−1,1,3)`, `(3,2,1,−1,2,−1)`, `(0,0,3,−1,4,−1)` | exact |

The arrow needed one correction rather than a divergence: ops 1, 2 and 5 all read
the **`+0x28`** link, and an earlier pass had ops 2 and 5 on `+0x30`. That is
invisible until an op splices; on the arrow the client's op 2 does, cutting the
ring into a four-hull chain plus a stub, so its tag pass sees four hulls and the
faces come out of the last-three fan. Corrected, all four meshes match. The
settling evidence and the emulator probe are under "The six ops".

Three inputs fault and are refused as `Error`, never a mesh: a clockwise ring
and a two-node ring (both at `0x142cf265f`), a two-ring brush at
`0x142cf0b6b` (the allocation `nodes−2+2·rings` books more slots than the
chains fill), and the `+0x58 == 0` emission variant is not implemented.

`C`'s instance encoding is read; **`C`'s class is not**. The store census was run
with `disp8` as well as `disp32`, and it reaches a wall rather than an answer: no
function in the image stores both `+0x186` and `+0x98`, and the one candidate
that writes the parameter-block field set (`0x14208edf6`) has **no static
reference anywhere** — no call, no tail jump, no pointer. A displacement is not a
type, so the next anchor is the producer of `[instance+0x48]`, not another
census.

## The context the whole walk shares (decompiled)

`BrushTreeToMesh(out, args)` builds one stack context and passes it to every
callee. `args` is two words: `args[0]` the `0x28`-byte brush snapshot record
(`{vertices, nodes, u32 vertexCount @ +0x10, u32 nodeCount @ +0x14, u32 root @
+0x18, u32 pad, u32 index @ +0x20}`) and `args[1]` an arena, `0` for "use the
image's global allocator" (`0x142cadee0` / `0x142cadf00`). The context:

| offset | meaning |
|---|---|
| `+0x00` | workspace-vertex array, `0x30` bytes each, count = **node** count |
| `+0x08` | hull array, `0x60` bytes each, count = `3 × triangles` |
| `+0x10` | face array, 8 bytes each (a hull pointer), count = `triangles` |
| `+0x18` | workspace-vertex pointer array, 8 bytes each, sorted in place |
| `+0x20` | edge-pair array, `0x10` bytes each, count = node count |
| `+0x28` | group array, 8 bytes each (the first hull of a chain) |
| `+0x30` | u32 mesh vertex count = `record+0x14` (the **node** count) |
| `+0x34` | u32 next free hull index, starts at the node count |
| `+0x38` | u32 face counter (faces emitted so far) |
| `+0x3c` | u32 group counter |
| `+0x40` | u32 hull count = `3 × triangles` |
| `+0x44` | u32 node count |
| `+0x48` | u32 triangle count |
| `+0x4c` | u32 op-dispatch index |
| `+0x50` | the status tree's root (ops 0/1/2/4, and the lookup at `0x142cefd70`) |
| `+0x58` | **emission variant**, `0` or non-zero, written by the hull stage from `args[2]` |
| `+0x5c` | u32 ring count, from the walk through `next_ring` |

Two counts are worth stating exactly because the older note approximated them.
The **triangle count is `nodeCount − 2 + 2 × rings`** and the mesh **vertex
count is the node count**, not the brush's vertex count: a source brush may have
more nodes than vertices, and the workspace array is sized from the node count.
`GeometryBuild` (`0x14213c440`) passes `args[2] = 1` — it stores a `1` in the
word just past its two-word argument struct — so the node-type-2 route uses the
**non-zero variant** of the emission; the other brush appender passes its own
value there and is not read.

## The hull stage (`FUN_142cf0360`, `0x142cf0360`) — read and implemented

Read end to end, **decompiled**, and pinned **emulated**.

One workspace vertex and one hull per ring node, laid out in ring-walk order:

- rings are walked from `record+0x18` (the root) through each node's `+0x04`
  (`next`) until the walk returns to the ring's first node, then from that node's
  `+0x10` (`next_ring`) to the next ring, `−1` ending the list. The global index
  is the ring-walk position and indexes every one of the five arrays;
- the **workspace vertex** (`0x30`) is `{x: i64; y: i64; divisor: i64}` — a
  straight copy of the brush vertex record's first `0x18` bytes — plus the owning
  hull pointer at `+0x20` and the op at `+0x28`. `+0x18` is left uninitialised.
  The **position is not decoded here**; `BrushTreeToMesh` overwrites the first
  `0x10` bytes with `0x142ced510`'s `vec4` afterwards, which is why this stage's
  comparator works on integers;
- the **hull** (`0x60`): `+0x00` parent, `+0x08`/`+0x10` children, `+0x18`
  colour (all zero out of this stage; ops 0/1/2/4 fill them), `+0x20` twin (zero
  until the emission pairs a half-edge), `+0x28` `prev` and `+0x30` `next`,
  `+0x38` the workspace vertex, `+0x40` the closure pointer (zero), `+0x48` this
  node's edge pair. `+0x50`/`+0x58` are the emission's scratch ear list;
- `prev` starts at `max(0, i − 1)` and `next` at `min(nodeCount − 1, i + 1)` over
  the **global** index, and then each ring's two ends are closed: the ring's
  first hull's `prev` becomes the ring's last hull and the last hull's `next`
  becomes the ring's first. So a one-ring brush is a plain cycle and a multi-ring
  brush is several cycles joined only by `next_ring` order;
- the **edge pair** (`0x10`) is `{i32 x; i32 y; i32 prev_x; i32 prev_y}` from
  node `+0x14`/`+0x18`/`+0x1c`/`+0x20` — the two endpoints of the edge this node
  starts, already in the ordering pass's order;
- the **op** is classified as described in `formats/brush-collision.md`, with one
  correction: the table has **four** rows, not three, and the fourth is
  own = predecessor = `1` → **op 2**. The two cross products:

  ```text
  cross1 = (py − y) · (x_p − px_p) − (y_p − py_p) · (px − x)
  cross2 = (y − py) · (px_p − x_p) − (py_p − y_p) · (x − px)
  ```

  where the unsubscripted names are this node's `+0x14`…`+0x20` fields and `_p`
  are its ring predecessor's. Each parenthesised difference is a **32-bit**
  subtraction sign-extended to 64 bits, the two products are subtracted in 64
  bits, and the test is `> 0` (the client widens to double and compares against
  `0.0`, which answers the same for a value of this width). Which branch each
  cross product picks: `own = 1, predecessor = 0` gives op 4, or op 0 when
  `cross1 > 0`; `own = 0, predecessor = 1` gives op 5, or op 1 when
  `cross2 > 0`;
- finally the workspace-vertex pointer array is sorted with the image's
  `std::sort`-shaped routine `0x142ceeb40` and the comparator `0x142ceee80`,
  and `+0x4c` is zeroed and `+0x58` is written from `args[2]`.

The sort's **key** is `FUN_142ceca80` (`0x142ceca80`), which compares two
vertices as the exact rationals `x / divisor` and `y / divisor` with the
products taken in **signed 128 bits** (`SEXT816` of each `i64`, so `divisor · y`
cannot wrap) and the answer's sign flipped when the two divisors' sign bits
differ. The two keys do not run the same way, and that is the client's own
asymmetry: **`y` descending, then `x` ascending**. `FUN_142ceee80` answers
`true` for the raw `1`, and an equal key falls back to comparing the two
workspace vertices' **addresses**. Because the array is filled in ring order at a
`0x30` stride, address order is index order, and because the addresses are
distinct the comparator is a **total order** — so the sorted sequence is unique
and any correct sort reproduces it, which is why the implementation can use
`sort_by` instead of copying the image's introsort.

`ow_tank::brush_mesh::hull_stage` is the read expression of all of the above.
`dev/brush_mesh_oracle.py` runs `0x142cf0360` under the emulator on four
synthetic brushes (a square, a triangle, a two-ring brush and a five-node
zigzag) and prints the workspace vertices, the hull ring links, the edge pairs,
the op of every node and the sorted order. The seven unit tests pin that output
field for field, including all six op classes (the square gives 2, 1, 3, 0; the
two-ring brush 0, 5, 4, 1, 3; the zigzag 2, 1, 0, 5, 4).

## The six ops (decompiled + assembly: read)

`BrushTreeToMesh` dispatches on the op of each node **in sorted order** — the
ops are applied in sweep order, not in ring order. Every op receives the
workspace vertex and reaches its hull through `+0x20`. Three arrays are appended
to as the ops run: two hulls per allocation (`+0x34 += 2`) and one face slot per
triangle (`faces[+0x38] = …`). Nothing else in the walk allocates.

The **splice** shared by ops 1, 2, 3, 4 and 5 allocates two hulls `A`, `B` at
`+0x34` and `+0x34 + 1`, zeroes their tree links and colour, makes them twins
(`A.twin = B`, `B.twin = A`), gives `A` the current node's workspace vertex and
`B` the **closure's** workspace vertex, and rewires four links:

```text
A.next = C            C.prev = A           (C = the closure being split)
A.prev = h.prev       h.prev.next = A      (h = the current node's hull)
B.next = h            h.prev = B
B.prev = C.prev       C.prev.next = B
```

The write order matters, because the link slots are not independent: the client
reads `C.prev` and `h.prev` into two locals **first**, then overwrites `h.prev`
with `B`. So the pair is threaded between `h.prev` and `C`: the chain
`h.prev → A → … → C` and the stub `h → B → (old C.prev)` replace the edge that
ran into `h`. That is why a spliced hull's `prev` becomes a fresh hull while its
`next` is untouched — the emission's face writer depends on exactly that (see
the clipping below).

| op | body | rule |
|---|---|---|
| 0 | `case 0` of `0x142cf23c0` | insert the current hull into the status tree (`FUN_142cf07d0`, an intrusive red-black insert that compares the hulls by geometry) and set its closure `+0x40` to **itself**. No allocation, no splice. This is the "start" vertex: the node begins an edge that runs to its right. |
| 1 | `case 1` | `C = h.prev.closure`; if that closure's workspace vertex has op 5, do the splice with `C`; then **erase `h.prev`** — the hull saved *before* the splice — from the status tree (`FUN_142cf0cc0`). The closure is not re-set. |
| 2 | `case 2` | `C = h.prev.closure`; if its vertex has op 5, splice; then **erase `h.prev` from the tree, insert `h`**, and set `h.closure = h`. |
| 3 | `case 3` | find `T = lookup(workspace vertex)` in the tree (`FUN_142cefd70`, a plain binary search down `+0x00`/`+0x08`/`+0x10`); `C = T.closure`; if its vertex has op 5, splice and set `T.closure = A`; **otherwise `T.closure = h`**. No insert, no erase. |
| 4 | `case 4` | save `h`, find `T = lookup(vertex)`, `C = T.closure`; do the splice (unconditionally — this case has no op-5 test); set `T.closure = A`; then **insert `h`** into the tree and set `h.closure = h`. |
| 5 | `FUN_142cf01b0` (`0x142cf01b0`) | `C = h.prev.closure`; if its vertex has op 5, splice with that closure; **erase `h.prev`** from the tree; then `T = lookup(vertex)` again, `C' = T.closure`; if `C'`'s vertex has op 5 splice a **second** pair and set `T.closure = A'`, otherwise `T.closure = h`. The only body that allocates **four** hulls. |

**Correction (measured, this round).** Ops 1, 2 and 5 all read the **`+0x28`**
link, not `+0x30`. An earlier pass of this file had ops 2 and 5 on `+0x30`,
which is invisible on any brush whose ops never splice and wrong on one that
does: it made the arrow's op 2 erase `h.next` where the client erases `h.prev`,
so the status tree kept a different node, the op-5 lookup answered a different
hull, no closure ever pointed at an op-5 vertex, and the splice never fired. The
emulator settles it: hooking the calls shows `erase(hull2)` from op 5 on hull3
(`hull3.prev`) and `erase(hull4)` from op 2 on hull0 (`hull0.prev`), and the
resulting ring is `0 → 1 → 2 → 6 → 0` with the twin pair `5/6` — exactly what the
corrected link produces and what the arrow's four-hull tag pass implies.

Read carefully:

- `lookup` (`FUN_142cefd70`, `0x142cefd70`) is a binary search of the status tree
  keyed by the workspace vertex: at each hull it compares the vertex against the
  hull's edge pair with `FUN_142cecb60` and descends `+0x08` or `+0x10` on the
  sign, returning the last hull visited (a loop that runs to a null child, so it
  always returns the nearest node, not "not found");
- `FUN_142cecb60` (`0x142cecb60`) is a side-of-edge predicate: given the edge's
  two `i32` points `A`, `B` and the vertex's `{x, y, divisor}` it answers the
  sign of `(B − A) × (P − A)` with `P` read as the homogeneous point
  `(x / divisor, y / divisor)` — the cross product is formed as
  `(P.y − A.y)·(B.x − A.x) − (P.x − A.x)·(B.y − A.y)` after multiplying through by
  the divisor, in **signed 128 bits**, and the answer is multiplied by the
  divisor's sign. `FUN_142ceca20` (`0x142ceca20`) is the same record's equality
  test (the two points are the same rational point).
- **The tree's ordering is now read.** The two addresses an earlier pass of
  this file called unreadable were tool artefacts, not image gaps. `0x142ced450`
  is a 13-byte argument-reorder thunk (it swaps `rcx` and `rdx` and jumps) into
  **`FUN_142ced380`** (`0x142ced380`, 170 bytes, a real `.pdata` function whose
  body Ghidra holds); `0x142cecb10` is a **no-`.pdata` leaf** of the 226 this
  image has (`docs/ghidra-project.md`) and is decoded linearly from the flat map.
  Evidence class: **assembly** — addresses and conclusions only here.

  `FUN_142ced380(a, b)` answers *"does `a` sort before `b`"*, and its two keys do
  not run the same way:

  ```text
  dy = b.div·a.y − a.div·b.y        (signed 128-bit)
  dy > 0   -> the answer is "same sign bit on a.div and b.div"
  dy < 0   -> the answer is "different sign bits"
  dy == 0: dx = b.div·a.x − a.div·b.x
            dx < 0 -> "same sign bit"      dx > 0 -> "different sign bits"
            dx == 0 -> false
  ```

  So `y` is descending and `x` ascending — the same order the hull stage's
  comparator produces — with the sign-bit normalisation that makes a negative
  divisor meaningful. The thunk is what makes the insert read backwards: the
  call `0x142ced450(X, Y)` **is** `FUN_142ced380(Y, X)`, so each of the three
  calls in the insert asks about the pair in the opposite order to its argument
  list.

  The insert's key (`FUN_142cf0900`) is a three-way choice on the two hulls'
  *representative* points, where a hull's representative is the lesser of its
  own vertex and its `+0x30` neighbour's under `FUN_142ced380`. If the two
  representatives are the **same rational point** (`FUN_142ceca20`) the answer is
  `cross(pair[0] − pair[1], new_pair[1] − pair[1]) < 0` through `0x142cecb10`; if
  the node's representative sorts first it is
  `sign(cross(pair[0] − pair[1], other − pair[1])) == 1` through `FUN_142cecb60`;
  otherwise the same predicate on the **new** hull's pair with the opposite sign.
  `true` descends `+0x10` (right), `false` `+0x08` (left).

  `0x142cecb10(A, B, C)`, all three plain `i32` point pairs, answers `−1` when
  `(C.y − A.y)(B.x − A.x) − (B.y − A.y)(C.x − A.x) > 0`, `+1` when it is `< 0`,
  `0` when it is `= 0` — the negative of `cross(B − A, C − A)`'s sign.

- **The insert fixup is read** (`FUN_142cf07d0`, `0x142cf07d0`): colours are
  **1 = black, 2 = red**; the loop stops when the parent is black; the uncle
  case sets parent and uncle black with the grandparent red and moves up; the
  rotation branch sets the (possibly re-pointed) `x` **black** and the old
  grandparent **red**, with `0x142cf1060` = left rotate and `0x142cf10a0` = right
  rotate. That is the textbook insert fixup, so insert shape is settled. (An
  earlier pass of this file recorded those two recolours as inverted; that was
  the decompiler naming the wrong pair, not the client — the assembly writes
  `x` and the grandparent, in that order.)
- **The erase is read** (`FUN_142cf0cc0`, `0x142cf0cc0`, rebalance
  `FUN_142cefe30`, `0x142cefe30`), from the instruction stream, because its
  decompile drops the rotation call arguments exactly as the insert fixup's
  colours were dropped. The case split:

  | case | rule |
  |---|---|
  | root pre-fix | if the node is the root, the root slot becomes its left child, else its right child |
  | leaf | detach; **red** → done; **black** → rebalance from a null `x`, with the parent's remaining child as the sibling |
  | one child | the child takes the node's place and is coloured **black**; done |
  | two children | the **successor** — the left spine of the right subtree — takes the node's place with its children and colour; then successor red → done, successor's right child non-null → that child blackened and done, otherwise rebalance from that child |

  The rebalance takes `(x, sibling, parent)` with the sibling *precomputed* as
  "the parent's child on the other side of `x`": a **red** sibling recolours
  (parent red, sibling black) and rotates the parent, then the sibling is
  re-read; a **black** parent with a black sibling and two black nephews
  recolours the sibling red and moves up, stopping when there is no grandparent;
  a **red** parent with that same shape recolours the sibling red and the parent
  black and stops; anything else runs the two rotations that finish the case
  (near nephew black → rotate the sibling, then recolour and rotate the parent,
  blackening the far nephew). Every path then refreshes the root slot by
  following parents from wherever it points — that **is** the client's root
  update, which is why the rotations never touch it.

  One detail matters and is easy to get wrong: the erase clears **only** the
  four tree fields of the removed hull (`+0x00`, `+0x08`, `+0x10`, `+0x18`). Its
  ring links and vertex survive, because the emission still walks them.

Two consequences worth recording because they are easy to get wrong:

- the ops never *unlink* a hull. "Erase" means "remove from the status tree"; the
  ring keeps every node, and the closure pointers are what change. That is why
  the ring the emission finally walks is several chains and not one;
- a hull's `+0x20` twin is written **only** by the splice and the emission, so an
  edge with no twin is an **open** edge and ends up as `−1` in the triangle
  record.

## The emission (decompiled + assembly: read)

After the ops, `BrushTreeToMesh` does four more things.

1. **Clear.** Every hull's `+0x40` is set to `0` for `0 … nextFreeHull − 1`.
2. **Group.** For each hull index in order, if its `+0x40` is `0` it becomes a
   group (`groups[g++] = &hull[i]`) and the whole chain reachable through
   `+0x30` is marked by writing the face array's base pointer into `+0x40`. So a
   group is one closed chain — one ring after the ops have cut it, or the whole
   ring when the ops cut nothing.
3. **Decode.** For each of the `nodeCount` workspace vertices,
   `0x142ced510` writes the position `vec4` over the first `0x10` bytes. This is
   the only place the quantised record becomes a coordinate, and it happens
   **after** the sort and **after** the ops, so every op key is an integer.
4. **Triangulate one group at a time**, with the variant chosen by `+0x58`:
   - `+0x58 == 0` → `FUN_142cef7f0` (`0x142cef7f0`);
   - otherwise → `FUN_142ceeee0` (`0x142ceeee0`) — the variant `GeometryBuild`
     selects.
   Then `FUN_142cf0ae0` (`0x142cf0ae0`) writes the mesh.

`FUN_142ceeee0` per group, **decompiled + assembly** (the arithmetic below is
the `SHUFPS` lane order read from the immediates, cross-checked against the
decompiler's own algebra):

- if the chain is exactly three hulls (`h.next.next == h.prev`), one face is
  booked (`faces[+0x38] = h.prev`), all three hulls' `+0x40` point at that slot,
  and nothing is allocated;
- otherwise it makes **one pass** around the chain, writing an ear tag into the
  workspace vertex of each node as it goes (it reuses the `+0x28` op field):

  ```text
  cur = the chain node,  A = cur.prev,  B = cur,  C = cur.next
  u = pos(B) − pos(A)        v = pos(C) − pos(B)
  reflex = cross2(u, v) < 0            -> tag 6, no candidate scan
  otherwise tag 8, then scan every hull from C.next up to (not including) A:
      w = pos(cand) − pos(B)
      require  w · perp(u) < 0
      require  w · perp(v) < 0
      require  (pos(cand) − pos(A)) · perp(pos(A) − pos(C)) < 0
      any hull passing all three  -> tag 7 (a point inside, so not an ear)
  cross2(u, v) = u.x·v.y − u.y·v.x  and  perp(x, y) = (y, −x)
  ```

  Every vertex tagged 8 is appended to a scratch circular list threaded through
  the hulls' `+0x50`/`+0x58` (zeroed at each test). `FUN_142cf0ae0` later reads
  the tag as nothing at all — the tags exist only for this loop.
- then it clips the collected list: per ear it allocates the twin pair
  (`+0x34 += 2`), books a face slot, and rewires the ring so that the face's
  **stored** hull is the ear's `prev` and that hull's `prev` becomes the fresh
  hull carrying the ear's `next`'s vertex. So the written triangle is
  `(ear.prev, ear, ear.next)` even though it is stored on `ear.prev` — the
  writer always reads a face as `(h, h.next, h.prev)` and the splice exists to
  make those three slots be the ear's three corners;
- after each clip it re-tests the triangle at the ear's `prev` with the same
  three tests, and re-links the ear list, and finally clips the last three.

**The clipping loop, read.** After the splice the algorithm re-tests the two
neighbours and repairs the ear list, and the client's three cases per neighbour
are:

| old tag | new tag | action |
|---|---|---|
| 8 | 8 | **move** the ear link from the neighbour to the fresh hull that now carries its triangle (`A` for the `prev` side); the list head follows if it was the neighbour |
| not 8 | 8 | append the fresh hull (the `prev` side) or the neighbour itself (the `next` side) to the tail |
| 8 | not 8 | unlink the neighbour; the head becomes its `+0x58` successor, or the list empties |

The last-three path (`0x142cef6c4` … `0x142cef7e6`) then fans the remaining chain
one node at a time: while more than three hulls remain it books a face on the
running hull, splices a twin pair whose vertices are that hull's and its
`next.next`'s, and advances; the final triple is booked with the three hulls'
closures pointing at the last slot. `+0x34 === 3` (or an empty ear list) is what
selects it.

That is the whole emission as read, and it reproduces **all four** pinned meshes
exactly, including the arrow's last-three fan — which only comes out right once
ops 2 and 5 read `h.prev` (above) and the op-2 splice cuts that ring to four
hulls.

`FUN_142cf0ae0` is plain scalar code and **is** read. It writes the mesh the
collider gets:

- **vertices**: `nodeCount` × `0x10` bytes, the workspace vertex's decoded
  `vec4` — so `z = w = 0` and the brush is still flat;
- **triangles**: `0x18` bytes each, six `i32` at `+0x00 … +0x14`:

  ```text
  +0x00 index(faces[k])            +0x04 mark(twin(faces[k]))
  +0x08 index(faces[k].next)       +0x0c mark(twin(faces[k].next))
  +0x10 index(faces[k].prev)       +0x14 mark(twin(faces[k].prev))
  ```

  where `index` is `(workspaceVertex − workspaceBase) / 0x30` — the ring-walk
  index, i.e. the same index the workspace was filled in — and `mark` is the
  **adjacent half-edge**: `−1` when the twin is null, otherwise
  `3 · (slot − facesBase) / 8` when the twin *is* the hull stored in that slot,
  `+1` when the slot's hull's `next` is the twin, `+2` otherwise. This is the
  same six-word record `MeshToColliderFaces` reads its open-edge marks from
  (`+0x04`/`+0x0c`/`+0x14`, `−1` = open), so the two reads agree.

Two facts about the triangle list that the questions asked for:

- **the triangle is `(h, h.next, h.prev)`**, where `h` is the hull stored in the
  face slot — the same three nodes in the same rotational order for every face,
  so the winding is decided by the order the ops and the emission leave the
  chain in, not by any reordering at write time. The three-node chain case
  pins it: the oracle's triangle brush writes `(2, 0, 1)` for a ring whose hulls
  are `0 → 1 → 2 → 0`, and the booked hull is `0.prev = 2`, so the columns are
  `(h, h.next, h.prev)` and nothing else;
- **winding is not normalised.** The square's two faces come out `(3, 0, 1)` and
  `(3, 2, 1)`, one counter-clockwise and one clockwise. That is consistent with
  the consumer: `MeshToColliderFaces` emits every face twice (outward and
  inward) and stitches, so a brush face is an unoriented surface;
- **a vertex can appear twice in one triangle.** The clipping books a face on a
  hull whose `prev` has just been replaced by a fresh hull, and the three-node
  chain path books `h.prev`; where the ops have already closed a chain onto
  itself the two neighbour slots can name the same workspace vertex, and the
  client does not filter it. `MeshToColliderFaces` counts a triangle whose
  *second* mark is `−1` as "flattened", and `ColliderTriangleDegenerate`
  (`0x142cd56e0`, already a verified unit) is what later rejects it.

**Final triangle order**: faces are appended in the order the emission books
them — the groups in group order, and inside a group the ears in the order they
were collected, then the final three. There is no sort of the faces.

## The whole conversion, pinned by the emulator

`dev/brush_mesh_oracle.py` now runs **both** stages. `mesh()` calls
`0x142cf23c0` itself and returns the mesh struct
(`{vertices, u32 count, pad, triangles, u32 count}`) plus every vertex's four
`u32` bit patterns and every triangle's six `i32`. The fixture is generated by a
Python copy of `0x142ced620` (`build_brush`, with `roundf` as two `f32` steps and
the same ordering pass), so the node records are ones the client's own builder
would write — including multi-ring brushes, which the hand-written hull-stage
cases cannot exercise.

The pinned results (evidence class: **emulated** — the client's own code):

| brush | nodes | vertices | triangles |
|---|---:|---:|---:|
| square, ccw | 4 | 4 | 2 |
| triangle, ccw | 3 | 3 | 1 |
| five-point L (one reflex corner) | 6 | 6 | 4 |
| five-point arrow (one reflex tip) | 5 | 5 | 3 |

A sixth fixture, a square with a triangular hole, does **not** produce a mesh —
see the faults below — so the multi-ring path is pinned by the hull stage's
two-ring test only.

The square's faces are `(3, −1, 0, −1, 1, 3)` and `(3, 2, 1, −1, 2, −1)` — the
six columns being vertex index and adjacent half-edge index, `−1` for an open
edge — and the ear-clipping cases come out with the marks interlocking
(`(5, 2, 1, −1, 2, 10)`, `(3, 8, 5, 5, 2, −1)`), which is what the `mark`
definition above predicts. Nothing in the code consumes these numbers yet; they
are the target the implementation will be diffed against.

### Three inputs that make the client fault (emulated, measured)

All three are real client behaviour caught by the emulator, and all three matter
to anyone who feeds a brush through:

- a **clockwise** ring. A clockwise triangle and a clockwise square both make
  `0x142cf23c0` read address `0x40` and fault, at `0x142cf265f` — immediately
after the op-3 lookup returns **0**. `FUN_142cefd70` returns the tree root
unchanged when the tree is empty, and a clockwise ring's first vertex in sweep
order is an op 3 (the sweep's "merge" class) rather than an insert. So the sweep
assumes one orientation: counter-clockwise.
- a **two-node ring**. The emission's `h.next.next == h.prev` test is meant for a
  three-node chain; with two nodes the walk reads past it and faults the same
  way.
- a **second ring**, with this fixture's ring order: it faults at `0x142cf0b6b`,
  inside the mesh writer, reading a **null face slot**. The allocation is
  `nodeCount − 2 + 2·rings = 7` triangles, but the two chains triangulate
  independently to `2 + 1 = 3`, so four face slots are never booked and the
  writer dereferences the first of them. The face count formula only holds once
the ops have **bridged** the hole into the outer ring, and this input's op
  sequence does not. Whether a differently ordered two-ring brush bridges (and
therefore whether this is the client's own bug or a fixture the client would
  never see) is **open** — it needs a ring order taken from a real brush, which
  is the `C` read below.

So a caller must not assume "any ring triangulates". An implementation must
reproduce these faults or refuse the input — it must not emit a mesh the client
never would.

## The model descriptor `C` (decompiled: encoding read; class open)

`GeometryBuild` (`0x14213c440`) reaches the brush's input through three loads
with no indirection of their own:

```text
geometry+0x80            the scene node
  [node+0x38]            the node's object
    [+0x40]              the object's model instance
      [+0x48]            C
```

and then, **read exactly**:

- `n = C+0xa0`. Load `p = [C+0x98]`, a pointer to a **two-word** record;
- if `n == 1` the two words are **`{points, count}`**; otherwise they are
  **`{count, points}`** — in both cases the *count* is a `u32` in the low half of
  its word and the *points* word is a pointer;
- the guard before use is "count ≠ 0", read from the same word either way
  (`C+0x98` + 8 as an `i32` when `n == 1`, `C+0x98` + 0 as an `i32` otherwise);
- the points are handed to `FUN_142ced620` (`0x142ced620`, the ring builder
  `ow_tank::brush::build_from_instances` implements) with the count and a
  two-lane scale of `s, s` where `s = [geometry+0x88]+0xe0`, **clamped up to
  `1.0`** when it is below `1.0`.

So the encoding question in `formats/brush-collision.md` is settled *as a rule*:
`C+0x98` points at a two-word descriptor and `C+0xa0 == 1` selects
pointer-then-count; any other value selects count-then-pointer. What is **not**
settled is:

1. **which of the two this install's models use** — `C+0xa0`'s value is written
   by whatever builds `C`, and that writer is not read;
2. **what class `C` is.** `C` carries a reference block at `+0x24`/`+0x28`
   resolved by `0x141b5cfb0`, a value copied to `geometry+0x90` from `+0xf0`, the
   collision parameters `GeometryCopyParams` (`0x142132cf0`) copies
   (`+0xc0` a rotation, floats at `+0x110 … +0x16c` with that function's clamps,
   two descriptor words at `+0x178`/`+0x186` through `0x141b53c40`, and the two
   bytes at `+0x194`/`+0x195`), and the instance descriptor above — a
   consistent picture of a **runtime object**, not a chunk. No vtable for it is
   reached from the read: `GeometryBuild` never loads a vtable from `C`, and the
   image has **no RTTI** (no `.?AV` class-name strings), so the class cannot be
   named from type information either.

**Where this read stops, exactly.** The producer of `[instance+0x48]` was not
read, so: (a) `C`'s class is unknown; (b) the value of `C+0xa0` is unknown; and
(c) therefore no real brush can be decoded from the install yet. The same
function also contains one access that cannot be followed statically at all:
the node's fallback transform is read through a **decrypted pointer** — the word
at `[geometry+0x80]+0x8` is folded with a two-step rotate/xor and a constant and
then dereferenced — so a branch of `GeometryBuild` reads memory whose address
comes from data. That access is used only when the node's scale is below `1.0`
and does not block the brush itself, but it means `GeometryBuild` is not
readable "purely", and any later attempt to decode `C` must handle it.

**The store census was run again, with `disp8`, and it reaches a wall.**
Evidence class: **measured**.

1. The repo's `dev/find_field_writes.py` now scans `disp8` as well as `disp32`
   (it only matched `disp32` before, and every displacement below `0x80` is
   assembled as `disp8`, which is why it used to report **zero** stores for
   `+0x48`). With both forms scanned and the sites grouped by containing
   `.pdata` function, over the whole `.text`:

   | field | stores | | field | stores |
   |---|---:|---|---|---:|
   | `+0x98` | 5 789 | | `+0x178` | 931 |
   | `+0xa0` | 4 866 | | `+0x186` | **5** |
   | `+0xc0` | 3 367 | | `+0x194` | 136 |
   | `+0xf0` | 2 392 | | `+0x195` | **1** |
   | `+0x110` | 1 993 | | | |

2. Intersecting by containing function: `{+0x98, +0xc0, +0xf0, +0x110}` leaves
   **180** functions; `{+0x98, +0xa0, +0x194, +0xc0, +0xf0}` leaves **13**;
   `{+0x98, +0xa0, +0x194, +0xc0, +0x110, +0x186}` leaves **0**; and
   `{+0x98, +0x194, +0x195}` leaves **0**. So **no function in the image stores
   both `+0x186` and `+0x98`** — the two halves of `C`'s layout are never filled
   by one writer.
3. The single `+0x195` store is at `0x142915c1f`, in `0x142915b60`, whose object
   is a different layout (`+0x168 … +0x214`): a coincidence of displacement, not
   `C`.
4. The five `+0x186` sites are `0x14081a99c`, `0x142090594`, `0x140dd307a`,
   `0x14194b68c`, `0x140a1572b`. The second one's function
   (`0x14208edf6 … 0x14209087f`) does write a near-perfect match for the field
   set `GeometryCopyParams` reads (`+0xc0`, `+0xf0`, `+0x110`, `+0x11c`, `+0x120`,
   `+0x130`, `+0x13c`, `+0x140`, `+0x14c`, `+0x150`, `+0x160`, `+0x170`, `+0x174`,
   `+0x17c`, `+0x186`) — but it does **not** write `+0x98`/`+0xa0`, and it has
   **no static reference at all**: a real `.pdata` range, no Ghidra function, and
   an image-wide scan finds no `E8` call, no `E9` tail jump and no 8-byte pointer
   to it anywhere.

So the previous round's claim that this function "builds `C`'s
collision-parameter block" is **downgraded**: it writes that field set, and
nothing shows it is reachable, so it cannot be confirmed as `C`'s writer. That is
where the displacement route stops, and the reason is structural: **a
displacement is not a type.** Several classes share offsets, which is exactly why
the tighter intersections collapse to zero while the loose ones stay in the
hundreds. The next anchor is the *producer of `[instance+0x48]`*, read backwards
from `GeometryBuild`'s chain (`[node+0x38] → [+0x40] → [+0x48]`), not another
census over offsets.

## The whole conversion, pinned by the emulator

`dev/brush_mesh_oracle.py` now runs **both** stages. `mesh()` calls
`0x142cf23c0` itself and returns the mesh struct
(`{vertices, u32 count, pad, triangles, u32 count}`) plus every vertex's four
`u32` bit patterns and every triangle's six `i32`. The fixture is generated by a
Python copy of `0x142ced620` (`build_brush`, with `roundf` as two `f32` steps and
the same ordering pass), so the node records are ones the client's own builder
would write — including multi-ring brushes, which the hand-written hull-stage
cases cannot exercise.

The pinned results (evidence class: **emulated** — the client's own code):

| brush | nodes | vertices | triangles |
|---|---:|---:|---:|
| square, ccw | 4 | 4 | 2 |
| triangle, ccw | 3 | 3 | 1 |
| five-point L (one reflex corner) | 6 | 6 | 4 |
| five-point arrow (one reflex tip) | 5 | 5 | 3 |

A sixth fixture, a square with a triangular hole, does **not** produce a mesh —
see the faults below — so the multi-ring path is pinned by the hull stage's
two-ring test only.

The square's faces are `(3, −1, 0, −1, 1, 3)` and `(3, 2, 1, −1, 2, −1)` — the
six columns being vertex index and adjacent half-edge index, `−1` for an open
edge — and the ear-clipping cases come out with the marks interlocking
(`(5, 2, 1, −1, 2, 10)`, `(3, 8, 5, 5, 2, −1)`), which is what the `mark`
definition above predicts. Nothing in the code consumes these numbers yet; they
are the target the implementation will be diffed against.

### Three inputs that make the client fault (emulated, measured)

All three are real client behaviour caught by the emulator, and all three matter
to anyone who feeds a brush through:

- a **clockwise** ring. A clockwise triangle and a clockwise square both make
  `0x142cf23c0` read address `0x40` and fault, at `0x142cf265f` — immediately
after the op-3 lookup returns **0**. `FUN_142cefd70` returns the tree root
unchanged when the tree is empty, and a clockwise ring's first vertex in sweep
order is an op 3 (the sweep's "merge" class) rather than an insert. So the sweep
assumes one orientation: counter-clockwise.
- a **two-node ring**. The emission's `h.next.next == h.prev` test is meant for a
  three-node chain; with two nodes the walk reads past it and faults the same
  way.
- a **second ring**, with this fixture's ring order: it faults at `0x142cf0b6b`,
  inside the mesh writer, reading a **null face slot**. The allocation is
  `nodeCount − 2 + 2·rings = 7` triangles, but the two chains triangulate
  independently to `2 + 1 = 3`, so four face slots are never booked and the
  writer dereferences the first of them. The face count formula only holds once
the ops have **bridged** the hole into the outer ring, and this input's op
  sequence does not. Whether a differently ordered two-ring brush bridges (and
therefore whether this is the client's own bug or a fixture the client would
  never see) is **open** — it needs a ring order taken from a real brush, which
  is the `C` read below.

So a caller must not assume "any ring triangulates". An implementation must
reproduce these faults or refuse the input — it must not emit a mesh the client
never would.

## The model descriptor `C` (decompiled: encoding read; class open)

`GeometryBuild` (`0x14213c440`) reaches the brush's input through three loads
with no indirection of their own:

```text
geometry+0x80            the scene node
  [node+0x38]            the node's object
    [+0x40]              the object's model instance
      [+0x48]            C
```

and then, **read exactly**:

- `n = C+0xa0`. Load `p = [C+0x98]`, a pointer to a **two-word** record;
- if `n == 1` the two words are **`{points, count}`**; otherwise they are
  **`{count, points}`** — in both cases the *count* is a `u32` in the low half of
  its word and the *points* word is a pointer;
- the guard before use is "count ≠ 0", read from the same word either way
  (`C+0x98` + 8 as an `i32` when `n == 1`, `C+0x98` + 0 as an `i32` otherwise);
- the points are handed to `FUN_142ced620` (`0x142ced620`, the ring builder
  `ow_tank::brush::build_from_instances` implements) with the count and a
  two-lane scale of `s, s` where `s = [geometry+0x88]+0xe0`, **clamped up to
  `1.0`** when it is below `1.0`.

So the encoding question in `formats/brush-collision.md` is settled *as a rule*:
`C+0x98` points at a two-word descriptor and `C+0xa0 == 1` selects
pointer-then-count; any other value selects count-then-pointer. What is **not**
settled is:

1. **which of the two this install's models use** — `C+0xa0`'s value is written
   by whatever builds `C`, and that writer is not read;
2. **what class `C` is.** `C` carries a reference block at `+0x24`/`+0x28`
   resolved by `0x141b5cfb0`, a value copied to `geometry+0x90` from `+0xf0`, the
   collision parameters `GeometryCopyParams` (`0x142132cf0`) copies
   (`+0xc0` a rotation, floats at `+0x110 … +0x16c` with that function's clamps,
   two descriptor words at `+0x178`/`+0x186` through `0x141b53c40`, and the two
   bytes at `+0x194`/`+0x195`), and the instance descriptor above — a
   consistent picture of a **runtime object**, not a chunk. No vtable for it is
   reached from the read: `GeometryBuild` never loads a vtable from `C`, and the
   image has **no RTTI** (no `.?AV` class-name strings), so the class cannot be
   named from type information either.

**Where this read stops, exactly.** The producer of `[instance+0x48]` was not
read, so: (a) `C`'s class is unknown; (b) the value of `C+0xa0` is unknown; and
(c) therefore no real brush can be decoded from the install yet. The same
function also contains one access that cannot be followed statically at all:
the node's fallback transform is read through a **decrypted pointer** — the word
at `[geometry+0x80]+0x8` is folded with a two-step rotate/xor and a constant and
then dereferenced — so a branch of `GeometryBuild` reads memory whose address
comes from data. That access is used only when the node's scale is below `1.0`
and does not block the brush itself, but it means `GeometryBuild` is not
readable "purely", and any later attempt to decode `C` must handle it.

**The store census was run this round, and it narrows the producer to one
function.** Evidence class: **measured**. Three caveats and results, in order:

1. `dev/find_field_writes.py` cannot answer a field this small — it only accepts
   `mod = disp32` encodings, and any displacement below `0x80` is assembled as
   `disp8`, so it reports **zero** stores for `+0x48`. That is a tool limit, not
   a fact about the image.
2. With `disp8` and `disp32` both scanned and the sites grouped by the
   containing `.pdata` function: `+0x98` has 5 789 stores, `+0xa0` 4 866, `+0x194`
   136 and `+0x186` **5** over the whole `.text`. `+0x186` is one of C's two
   descriptor words, so five sites is a sharp enough signal to read one by one.
3. Those five sites are `0x14081a99c`, `0x142090594`, `0x140dd307a`,
   `0x14194b68c`, `0x140a1572b`. The second one's function
   (`0x14208edf6 … 0x14209087f`) stores, off one base register: `+0xc0` (C's
   rotation), `+0xf0`, `+0x110`, `+0x11c`, `+0x120`, `+0x130`, `+0x13c`, `+0x140`,
   `+0x14c`, `+0x150`, `+0x160`, `+0x170`, `+0x174`, `+0x17c`, `+0x186` — a
   near-perfect match for the field set `GeometryCopyParams` reads out of `C`
   (`+0xc0`, `+0xf0`, `+0x110…+0x16c`, `+0x178`, `+0x186`). It is the strongest
   candidate for **C's collision-parameter writer**.

It does **not** settle the question, because that function does not store
`+0x98` or `+0xa0` off the same base: the instance-point descriptor is filled
elsewhere, and `C`'s class is still not nameable. So the read stops one step
further in than before but at the same place: no `C` value, no install brush,
no class name. What is now banked is the *site list* — five addresses, one of
which builds the parameter block — so the next read is "who fills `+0x98` of the
object `0x14208edf6` fills", not a blind census.

## The two emission variants

`+0x58` is written by the hull stage from `args[2]`, so the caller picks the
triangulator. `GeometryBuild` passes `1` (the word after its two-word argument
struct is set to `1` immediately before the call), so the node-type-2 route runs
`FUN_142ceeee0`. The other brush appender's value is not read, so whether
`FUN_142cef7f0` is live in this build is **open**; it is the same shape of
group-at-a-time triangulator with its own ear criterion and its own chain
walking, and it is read only structurally.

## The implementation and what it is checked against

`ow_tank::brush_convert::tree_to_mesh` is the read expression of `0x142cf23c0`
end to end, with the phase order the client uses: hull stage → sweep sort → the
six ops in sweep order → position decode → clear and group → one triangulation
per group → the face writer. `ow_tank::brush_mesh::hull_stage` remains its first
stage. Both differentials are `dev/brush_mesh_oracle.py`, which runs the client's
own code under the emulator with the bump-allocator stub at `0x1442a0158`.

The measured status, brush by brush:

| brush | nodes | client | this implementation |
|---|---:|---|---|
| square, ccw | 4 | 2 faces, `(3,−1,0,−1,1,3)`, `(3,2,1,−1,2,−1)` | **identical**, vertices and marks |
| triangle, ccw | 3 | 1 face, `(2,−1,0,−1,1,−1)` | **identical** |
| five-point L | 6 | 4 faces, `(5,−1,0,−1,1,3)`, `(5,2,1,−1,2,10)`, `(3,−1,4,−1,5,9)`, `(3,8,5,5,2,−1)` | **identical** |
| five-point arrow | 5 | 3 faces, `(3,6,0,−1,1,3)`, `(3,2,1,−1,2,−1)`, `(0,0,3,−1,4,−1)` | **identical** |

The L is the strong case: it has a reflex corner, it clips two ears, it exercises
both re-tests and the ear-list repair, and its four faces' half-edge marks
interlock (`2`, `10`, `9`) exactly as the client's do. The square pins the
three-node chain path and the two-colour split of the faces.

**The arrow, and what it cost.** The arrow was the last divergence, and it was
not the tag strictness it looked like: it was the `+0x28`/`+0x30` link misreading
corrected above. On the arrow the client's op 2 *splices* — its `h.prev.closure`
is the op-5 hull, which only becomes true once op 5 has re-closed that hull onto
itself — the ring becomes a four-hull chain (`0→1→2→6→0`) plus the stub
(`3→4→5→3`) with twins `5/6`, the tag pass therefore sees four hulls, and the
three faces come from the last-three fan. With `h.next` the splice never fires,
the pass tags five hulls, three of them ears, and the mesh is the wrong one the
earlier pass produced.

It was settled by hooking the client's own call sites — `0x142cf0900` insert,
`0x142cf0cc0` erase, `0x142cefd70` lookup — and reading their arguments, then
dumping the post-op hull array at the end of the tag pass. The arguments show
`erase(hull2)` from op 5 on hull3 (`hull3.prev`) and `erase(hull4)` from op 2 on
hull0 (`hull0.prev`); the dump shows `hull0.prev = 6` whose vertex is `3`, which
is why the first tag iteration's `u = cur − prev` is `(−1,−1)` and its reflex
term is `2.0` where a five-hull ring would give `4.0`. The tag values themselves
were also read (`8` on four hulls, untouched on the fifth), which is what
disproved the collinearity hypothesis.

### The refusals

Each of these is a measured client fault, answered here as an `Error` and never
as a mesh the client would not emit:

| input | client | here |
|---|---|---|
| clockwise ring (triangle or square) | reads address `0x40` at `0x142cf265f` — the op-3 lookup answers the null tree root, because a clockwise ring's first vertex in sweep order is an op 3 | `Err` |
| a ring of two nodes | the same place: the emission's `next.next == prev` test is written for three hulls | `Err` |
| a two-ring brush whose chains do not fill the allocation | reads address `0x20` in the face writer at `0x142cf0b6b` — `nodes − 2 + 2·rings` slots are allocated, the independent chains book fewer | `Err` |
| `+0x58 == 0` (the other emission variant) | `FUN_142cef7f0` | `Err`, not implemented |

## Checks

| claim | check |
|---|---|
| the record sizes and the six context array positions | `brush_mesh::tests`, and the oracle's printed pointers |
| the hull ring, its close and both `prev`/`next` clamps | `brush_mesh::tests::the_square_hull_stage_matches_the_client`, `the_triangle_hull_stage_matches_the_client` |
| the `next_ring` walk and per-ring close | `brush_mesh::tests::two_rings_walk_and_close_independently` |
| all six op classes | `the_square_hull_stage_matches_the_client` (2,1,3,0), `two_rings_walk_and_close_independently` (0,5,4,1,3), `the_zigzag_covers_all_but_one_op` (2,1,0,5,4), `the_op_table_is_the_client_one` |
| the sweep order is `y` descending then `x` ascending on rationals | `the_zigzag_covers_all_but_one_op`, `the_comparator_is_the_rational_one` |
| the edge pairs are the node's own four `i32`s | the hull-stage tests' `pair` vectors |
| the two op cross products are 64-bit with 32-bit subterms | `the_op_table_is_the_client_one` |
| the square mesh, bit for bit | `brush_convert::tests::the_square_mesh_matches_the_client` |
| the triangle mesh (three-node chain) | `the_triangle_mesh_matches_the_client` |
| the L mesh (reflex corner, two clips, both re-tests, interlocking marks) | `the_ell_mesh_matches_the_client` |
| the arrow mesh (op-5 re-close, op-2 splice, four-hull chain, last-three fan) | `brush_convert::tests::the_arrow_mesh_matches_the_client` |
| a clockwise ring and a two-node ring are refused | `a_clockwise_ring_is_refused`, `a_two_node_ring_is_refused` |
| the unimplemented variant is refused | `the_other_variant_is_not_implemented` |
| the sweep ordering predicate, the side tests, the ear predicate, the erase | read from the flat map (evidence class **assembly**); exercised end to end by the three matching meshes |
| `+0x98`/`+0xa0` encoding rule | this file and `GeometryBuild`'s guard; **not tested** — nothing builds a `C` |

## What is open

1. **`C`'s class and the value of `C+0xa0`, and why a displacement census cannot
   reach them.** The census has now been run with `disp8` as well as `disp32`
   over the whole `.text` (the repo tool was extended this round). Counts:
   `+0x98` 5 789 stores, `+0xa0` 4 866, `+0xc0` 3 367, `+0xf0` 2 392, `+0x110`
   1 993, `+0x194` 136, `+0x178` 931, `+0x186` **5**, `+0x195` **1**. Requiring
   every one of `{+0x98, +0xc0, +0xf0, +0x110}` leaves **180** functions;
   `{+0x98, +0xa0, +0x194, +0xc0, +0xf0}` leaves **13**;
   `{+0x98, +0xa0, +0x194, +0xc0, +0x110, +0x186}` leaves **0**, and
   `{+0x98, +0x194, +0x195}` leaves **0**. So **no function in the image stores
   both `+0x186` and `+0x98`**, and the one `+0x195` store (`0x142915c1f`) is in
   `0x142915b60`, whose object is a different layout entirely (`+0x168…+0x214`).
2. **`0x14208edf6` has no static reference at all.** It has a real `.pdata` range
   (`0x14208edf6..0x14209087f`) and no Ghidra function, and an image-wide scan
   finds **no `E8` call**, **no `E9` tail jump** and **no 8-byte pointer** to it
   anywhere. Its field set matches `C`'s parameter block, but nothing can be shown
   to reach it, so "it builds `C`" cannot be confirmed from references and the
   identification is downgraded to "a writer of that field set". That is where the
   displacement route stops: **a displacement is not a type** — two classes share
   offsets, which is exactly why the combined filters collapse to zero. The next
   anchor must be the *producer of `[instance+0x48]`* read backwards from
   `GeometryBuild`'s chain (`[node+0x38] → [+0x40] → [+0x48]`), not another
   census.
3. **Whether a two-ring brush can bridge.** The hole fixture faults in the face
   writer; whether a real brush's op sequence bridges instead is unknown until a
   real brush is decoded (item 2).
4. **`FUN_142cef7f0`** (the `+0x58 == 0` variant) is read only structurally; the
   node-type-2 route does not select it.
5. **A `dev/verify_harness` unit for either stage.** Both differentials are the
   emulator probe (the stages allocate through the image's allocator hook, so the
   probe installs the stub at `0x1442a0158`). Making it a cargo unit needs an
   entry in `crates/ow-tank/src/bin/verify_probe.rs`, which this session did not
   own.

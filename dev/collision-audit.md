# Collision parsing: what we do, what the client does that we do not, and what is proven

Audit date: 2026-11-06. Every address is a current-image address in
`dev/ghidra_run/owmod_clean` / `Overwatch.exe`.

This file answers three questions for the collision data path only (bytes on
disk -> meshes -> query):

1. what our code parses and what it refuses;
2. what the client's own code does at each stage that our code does not do;
3. which stages are *proven* (differential against the client's bytes) and
   which are only *read*, *measured* or *inferred*.

Nothing here changes code. Where this audit disagrees with a doc, the doc is
named so it can be fixed.

---

## 0. The pipeline, and where our coverage stops

```text
MAP_CHUNK_COLLISION (kind 0x11) / additive (0x10)     MAP_CHUNK_PATHING (kind 0x12/0x23)
  0x38 header + A[0x58] + B[0x38]                         STUD -> STU_9199BF6B -> 220 cells x 17 arrays
  FUN_142e492a0 : fixup only                              MapChunkCellBytesFixup 0x141ff51a0 : fixup only
        |                                                        |
        |  NO CLIENT CONSUMER FOUND (open)                      |  NO CLIENT CONSUMER FOUND (open)
        v                                                        v
  collision_level.rs: one prebuilt BVH (inference)         collision.rs: cells kept as 17 opaque slices
        |                                                        |
        +------------------+     +-------------------------------+
                           |     |
  MODEL asset: STU_537E56E4 holder meshes (read route)   BRUSH route (node type 2 / GUID type 0x0A)
  model_body.rs -> mesh_shape.rs -> mesh_world.rs          brush.rs -> brush_mesh.rs -> brush_convert.rs
        |                                                        |
        |  type 3 mesh, arrays as stored (client converts none)  |  shell thickening = collider.rs::shell
        v                                                        v
  ow_sim::collide::mesh_sweep (0x142caed30)  <- verified       MeshToColliderFaces 0x142cf2010 (read)
        |                                                          |
        v                                                          v
  hit record -> ow_sim::walk / ow-client                      *** our coverage ENDS here ***
                                                             0x14213acf0 / 0x14213a510 ingest, 0x14213c240
                                                             accept, 0x1421360a0 entry, 0x14213d0b0
                                                             finalize, ShapeAddToBody: all unimplemented
```

Two independent producers of collision geometry exist in the client and we
implement the *second half of one of them*, plus a scene-side query path that
the character sweep does not even use. The gaps are listed in section 2.

---

## 1. Stage by stage: our code vs the client

### 1.1 Chunk kind table and the collision payload — `crates/ow-tank/src/chunkkind.rs`

**Parsed.** The 0x24-entry kind table at `0x1439aa320` (stride `0x20`) with the
name decoder `0x1404c19b0`; the collision payload's `0x38` header
(`{u64, i32 countA, i32 countB, 4 x RVA, u64}`); array A (`0x58` stride) and
array B (`0x38` stride) as byte ranges; and `fixup_in_place`, the read
expression of `FUN_142e492a0`.

**Client behaviour, re-read today (assembly + decompiled):**

- `0x142e492a0` rebases four header qwords `+0x10`, `+0x18`, `+0x20`, `+0x28`
  (a zero stays zero), then calls a per-element helper for every element.
- Helper `0x141b53a70` (array A, `0x58`): `ADD [rcx+0x18], base`,
  `ADD [rcx+0x8], base`, `ADD [rcx+0x10], base`, `ADD [rcx], base`.
- Helper `0x141b53a60` (array B, `0x38`): `ADD [rcx], base`,
  `ADD [rcx+0x8], base`, `ADD [rcx+0x10], base`.
- Both element passes are **unconditional**: an element field that is `0` in
  the file becomes `base`, not null. Our `fixup_in_place` does this.
- The loops run `if (0 < count)`, so a **negative** count silently does
  nothing. Our `Header::read` rejects a negative count with an error. That is a
  deliberate stricter guard, but it is *not* the client's behaviour: for a
  payload the client accepts and ignores, we fail. Worth knowing before a
  future differential.

**Not parsed (client data we do not name):** header `+0x00` and `+0x30`; every
sector float at `+0x20..+0x28`, `+0x38`, `+0x3C`, `+0x40`, `+0x48` and the two
dwords `+0x4C`/`+0x50`; array B's fields above `+0x10`; the 8 bytes at
`+0x38..+0x40`; the kind table's dwords at `+0x10`/`+0x14`. Recorded in
`docs/formats/chunk-kinds.md` section 6, still open.

**Proof status.** `fixup_in_place` has **no differential unit**; its exactness
is argued from the decompile plus synthetic edge cases and the real payload
(`crates/ow-tank/tests/chunkkind_real.rs`). `docs/formats/chunk-kinds.md` says
so itself: *"Not tested against the shipped bytes of `0x142e492a0`."* The
per-element helper addresses quoted in the doc are confirmed above by
disassembly.

### 1.2 The pathing chunk's cell container — `crates/ow-tank/src/collision.rs`

**Parsed.** The `STUD` v1 blob, the single `STU_9199BF6B` instance
(class hash `0x9199BF6B`, size `0x78`), the `m_cellBytes` field at `+0x68`,
the cell block list, and each cell's 17 self-relative boundaries plus the 17
slices. `fixup_cell_bytes` is the read expression of
`MapChunkCellBytesFixup` (`0x141ff51a0`), and `tables()` derives the strides of
the other two array fields (`+0x10` nested, `+0x58`) purely from exact tiling.

**Client behaviour.** `FinishMapChunkAsset` (`0x141ff54e0`) for slot `0x1e`
(kind `0x12`): `MapChunkHeaderFixup`, `StuLoadSubAssets(*payload) + 0x68`,
`MapChunkCellBytesFixup`. For slot `0x22` (kind `0x23`, additive pathing):
`FUN_142e49270`, `StuLoadSubAssets(*payload) + 8`, `MapChunkCellBytesFixup`.
The fixup turns 17 relative qwords into pointers and has allocating branches
for two encodings.

**Gaps.**

1. **The 17 arrays' meanings are unread.** This is the single largest parse
   hole in the repo: 220 cells whose 17 arrays include quantised vertices and
   `0xFF`-separated index runs, exposed as raw `&[u8]` with a comment saying so.
   Nothing in the client was found that reads them
   (`docs/formats/maps.md`, "The cell path's consumer is not in the image").
2. **The file-to-runtime conversion of the cell entries is not implemented.**
   `{block_size, record_off}` -> `{record pointer, 1}` happens before the fixup
   runs; our unit takes the post-conversion form. The harness entry records this
   as an observation, not a verification.
3. **The two allocating fixup branches are declared unreachable** (header mode
   0, entry mode 0) because they allocate and then dereference the zeroed
   allocation. Fine as a refusal, but it is a modelled-out client path.
4. **Kind `0x23` (additive pathing) is parsed by nothing.** Its cell field is at
   `sub-asset + 8`, not `component + 0x68`; `collision::parse` would reject the
   payload. If any map ships an additive pathing chunk, we silently do not read
   it (we never even try).

**Proof status.** Differential unit `fn/MapChunkCellBytesFixup`: verified,
12/12 mutants, 33% branch coverage, four acknowledged gaps - the non-allocating
encodings only, the cell-entry conversion outside the unit, and *"the
cell-to-shape decoder, the 0x100-byte sector entries, ShapeAddToBody and the
live collision world remain open."*

### 1.3 The level collider from the collision chunk — `crates/ow-tank/src/collision_level.rs`

**What it is.** The last element of array B is a prebuilt BVH in exactly the
*runtime* record layout: `0x10`-byte vertices, `0x20`-byte nodes, `0x20`-byte
triangles, plus a mask word. Practice range: 203,999 / 236,957 / 343,474
elements, 21.8 MB, root box `(-103.469, -38.510, -161.028) .. (206.159, 51.071,
148.317)`, mask `0x1FFF`.

**Honest classification: this is an inference, not a read.** The client's
`0x142e492a0` only fixes the three pointers. No function in the image was found
that hands this payload to a shape record, so we cannot say the client *uses*
this mesh. Our validation is structural and quite strong (the three runs tile the
payload at exactly the runtime strides, every triangle corner indexes a real
vertex, the node box is the level extent with a 90 m vertical span, every node's
mask is a subset of `0x1FFF`, every internal node's second child is in range,
every leaf's triangle run is in range, >1000 leaves and >1000 internal nodes) -
but that is evidence the *data* is a collision BVH, not evidence of the route.

**Doc bug found:** `collision_level.rs`'s module-doc table lists `+0x00` as
vertices `0x40` and `+0x08` as nodes `0x31CE30`. It is the other way round:
`tail+0x00` is the node run (`0x31CE30`, 236,957 x `0x20`) and `tail+0x08` the
vertex run (`0x40`, 203,999 x `0x10`). The code (`mod tail { NODES = 0,
VERTICES = 8 }`) and `chunkkind_real.rs` are right; only the prose is wrong.

**Consequences of the inference:**

- `parse` **requires `count_b == 1`**. A map whose collision chunk carries more
  than one array-B element (which is the natural shape for an additive chunk,
  or for a map with several collision bodies) is rejected with an error rather
  than read. This is the most likely first real-data failure outside the
  practice range.
- the sector array (`count_a` elements) is not used at all beyond counting.
- the whole level mesh is placed at the origin with the identity transform, on
  the assumption that its vertices are already world-space.

**Proof status.** No client function is implemented here, so no differential is
possible. `crates/ow-tank/tests/collision_level_real.rs` pins the practice
range's numbers (36 sectors, 203,999 / 236,957 / 343,474 elements, mask
`0x1FFF`, root box to 0.01) and the per-node/per-triangle structural invariants.
"Not tested against client behaviour" by construction.

### 1.4 Brush records and the ring builder — `crates/ow-tank/src/brush.rs`

**Parsed.** `0x20`-byte brush object, `0x20`-byte vertex (`{i64 x, i64 y, i64
divisor, u32 ordinal}`), `0x28`-byte node ring record, `Vertex::position`
(`(x/divisor)*0.0025` as integer quotient + scaled remainder in f32),
`Brush::rings`, `quantise`, and `build_from_instances` = the read expression of
`0x142ced510` + `0x142ced620` (steps 1-5 in `docs/formats/brush-collision.md`).

**Missing.** The **ingest** (`0x14213a510`, `AppendCollisionEntryFromBrushSource`)
is cited but not implemented - it copies `count<<5` vertex bytes and `count*0x28`
node bytes into a `0x28` snapshot record and appends a `0x100`-byte entry. Its
choice of which brush becomes a shape (the `leaf` byte, the source's `+0x24` /
`+0x34` guards, `0x1420a02f6`'s `holder+0x2a == 1` filter) is not modelled.

**Proof status.** Emulator oracle `dev/brush_vertex_oracle.py` runs
`0x142ced510` and `0x142ced620` and the unit tests pin its output field for
field (square, reversed square, triangle, degenerate triple, non-uniform scale,
two half-way rounding cases). 11 unit tests in `brush.rs`. Not a
`dev/verify_harness` unit (the builder allocates through the image's allocator
hook).

### 1.5 Brush tree -> mesh — `brush_mesh.rs` + `brush_convert.rs`

**Implemented, and this is the surprise given `docs/formats/brush-collision.md`:**
the hull stage (`0x142cf0360`), the six ops, the red-black status tree
(insert/fixup/lookup/erase/rebalance/rotations), the ear-clipping emission
(`0x142ceeee0`) and the face writer (`0x142cf0ae0`) are all transcribed.
`tree_to_mesh` returns a mesh of triangles.

**Missing.**

- `fun_142cef7f0`, the `+0x58 == 0` emission variant, is read structurally
  only; `tree_to_mesh_variant` returns an error for it.
- A two-ring brush whose chains do not fill the `n - 2 + 2*rings` allocation
  faults in the client's face writer (`0x142cf0b6b`); we return an error and do
  not know whether real brushes bridge instead.
- No `dev/verify_harness` unit (both differentials are the emulator probe).

**Proof status.** `dev/brush_mesh_oracle.py` runs *both* stages of the client's
`0x142cf23c0`; four meshes are pinned bit for bit (square, triangle, five-point
L, five-point arrow), the multi-ring path at the hull stage only, and three
measured client faults are reproduced as errors. 25 unit tests across the two
modules.

**Doc debt:** `docs/formats/brush-collision.md` still states *"Until those are
read, no brush-to-mesh conversion is implemented"* and *"`brush.rs` does not
convert a brush to a mesh"*. That is stale - `brush_convert.rs` has existed
since 2026-11-03. `docs/formats/brush-mesh.md` is the current file.

### 1.6 Model collision mesh block and shape record — `mesh_shape.rs`, `mesh_world.rs`

**Parsed, and this matches the client exactly (I re-read the unpack today).**
`0x141b53a90` (the array unpack used by `0x141b53af0`):
`cmp [rdx+0x10], 1` / `lea rax,[r8+8]` / `cmovnz` -> take the header at the
pointer when the tag is `1`, at `pointer + 8` otherwise; then four array slots
at `+0x08`/`+0x18`/`+0x28` (tag at `+0x10`/`+0x20`/`+0x30`) and the `u32` at
`+0x38` straight into the `0x30`-byte source. `MeshShapeFill` (`0x142cae540`)
writes the `0x4c`-byte / 19-dword record our `shape_record` documents.

**Correct by design:** the client converts nothing - the three arrays are passed
by pointer to the type-3 query. So there is nothing to implement here beyond the
record.

**Missing.** The 19 dwords are transcribed but no differential unit pins them
(4 unit tests). The `+0x48` scaled float, the `1/scale` lanes and the `+0x44`
source float are exact arithmetic worth a fixture.

**Proof status.** `show`/`install` tests only (`mesh_world_real.rs`,
`model_mesh_real.rs`); the *walk* is what is verified, not the fill.

### 1.7 The per-model body build — `model_body.rs`

**Read and implemented:** the six guards, the mask-word select on
`[component+0x420]` bit 25, the holder array at `[model+0x140]` with its tag at
`+0x148`, the per-holder filter (`mode byte +0x2a == 0`, `mesh != 0`,
`mask == MAX || world_mask & mask`), the shape-mode map (`0`->type 3, `1`->2,
`2`->1, `4`->0 + second body), the scale rule (`maxss(scale, 0.01)` when the
three entity scales agree within `0.01`, else `1.0`).

**Missing (quoted from its own doc):** *"The descriptor's bounds quads, filter
words and the `+0x5c..+0x62` ids come from the holder's parameter instance
(`+0x18`), the component's own state and the entity's transform; this module
deliberately does not convert them."* Also unread: the inertia block
`0x14201c507`..`0x14201c817`.

**Consequence:** the shape's layer/filter that the query tests is currently a
constant (`world_mask` all ones) instead of the descriptor's real words, and the
body's placement (`+0xd0` quaternion / `+0xf0` position, writer open) is not
applied. On this map every holder is all-ones so it happens not to matter; it
would matter on any map with filtered collision.

**Proof status.** Unit tests for each predicate plus `tests/model_body_real.rs`
over the range's 229 models / 279 holders / 272 mesh blocks. No differential
unit for `0x14201ae60` itself.

### 1.8 Domino world slot and body handle — `domino_world.rs`

Two integer encodings only: the world-table decode (`0x1442a0bc0`,
`owner_create_body`'s `>= 0x3fff` bound) and the handle
`((slot)<<0x30)|((gen)<<0x20)|(index)`, with the refusal sentinel
`0xffff_0000_ffff_ffff`. 7 unit tests. The pools, `ShapeEntryFromDescriptor`
(`0x142cb7f10`), `ShapeAddToBody` (`0x142cb3940`) and the body tick are read in
docs and **not implemented** - a declared deviation (`range_world` uses a shape
list instead).

### 1.9 The query side — `crates/ow-sim/src/collide.rs`

Implemented: sphere (type 0), capsule (type 1) no-support paths, the type-3 mesh
walk with both the analytic path and the support-point hull + closest-point
solver, `shape_world_transform`, `shape_narrowphase`, the sweep bounds, the
layer filter, the query driver fold, `collider.rs::shell` (z +/- radius
thickening and the stitch triangles).

Not implemented: **type 2** (`0x142cd3c80`, convex hull as face planes) and
**type 4+** (`0x142cd0ae0`, height-field grid) - read, transcribed in part, but
`Narrowphase::UnreadType` is the honest answer; `ShapeDispatch` returns false
for type 3 in that unit; the half-edge adjacency pass of `ColliderFaceAppend`
(`0x142cca460`, `+0x44/+0x48/+0x4c`) is left at its seeds; the sink push
`0x142cc1450` and the hit callback's return path are unverified; the
character sweep's depenetration/slide (`0x142cbcee0` -> `0x142cf3f80`) is a
declared deviation replaced by a projected residual.

Implemented but data-starved: `ow_sim::policy` is the hit policy
`0x14202bc90` (three outcomes `1`/`2`/`3`, the `+0x30` score, the `+0x38`/`+0x39`
flags, the two key-decode constants, and the dead presence-mask term kept
provably dead), and `ow_sim::fan` is the wish writer's candidate fan that
consumes sweeps. Both take their geometry through a trait: what they need is the
cell/sector-to-shape ingest, which is the open parse stage - not a missing
query.

---

## 2. What the install does that our parsing does not — ranked

### 2.1 The second and third collision-entry producers (biggest gap)

`docs/formats/maps.md` and `character-controller.md` name `GeometryBuild`
(`0x14213c440`) as *the* collision producer. There are at least three appenders,
and we implement only the second half of the first:

| route | entry point | what it does | ours |
|---|---|---|---|
| A | `0x14213c440` node-type-2 build | brush tree -> mesh -> `MeshToColliderFaces` -> BVH, appends a `0x60` cell at `geometry+0x108` (`0x142136010`) | brush->mesh yes, shell yes, BVH partly (`BuildBvh` verified, adjacency missing), ingest **no** |
| B | `0x14213acf0` over the source array at `geometry+0xb0` (stride `0xb0`) | per source: guards, chord test, per-node distance clamp, `BrushTreeToMesh`, accept test, `0x100`-byte sector entry at `geometry+0x118` (`0x1421360a0`) | **nothing** |
| C | `0x14213a510` `AppendCollisionEntryFromBrushSource` | prebuilt brush -> `0x100` entry, GUID type `0x0A` gate | **nothing** |

I decompiled route B today. Its steps, in order (all un-implemented in our repo,
no address of it appears in any `.rs`):

1. **Source guards.** `geometry+0x28 <= source+0x28`, `source+0x34 == 0`,
   `source+0x24 == 0`.
2. **Chord test** `0x142cee720` -> `0x141b33060` (a `0x30`-byte object) ->
   `0x142d19ce0(object, source)` and on success `0x142cee360(object)`. If it
   fails the whole source is skipped.
3. **Per node** (`0x28` stride) a distance is chosen: `geometry+0x2c` when the
   node's `+0x20` index is `-1`, else the float at `vertex + 4` of the record
   `*(source[0]+0x10) + idx*0xc`; then clamped against the source's float at
   `+0x20`. If both are `>= 0`, the distance is the smaller one and the "exactly
   zero" byte (`+0x24` of the entry) is `1` when that value is `0` **and**
   `geometry+0x30` is `0`; if the larger of the two is `< 0` the distance becomes
   `-1.0`; a negative pair frees the node.
4. `BrushTreeToMesh` per node, then the **accept test** `0x14213c240`. That
   function is itself a two-stage filter I had not seen documented as such:
   `0x142cf1160(ctx, mesh, 0x10, geometry+0x24 ^ 0x80000000)` clips/merges the
   mesh (the XOR flips the sign bit of a float, so the second argument is a
   negated distance/limit), then per produced polygon
   `0x142cd75d0(ctx, mesh_vertices + idx*0x10, count, geometry+0x20)` builds an
   **extruded prism**: `2n` vertices at `p +/- (0,0,radius)`, a `2n`-normal
   array with the `0x80000000`/`-1.0` seed lanes, a `2n*6` index array with the
   `1, 0xff` record form, and a `2n+2`-entry offset array. If no polygon
   survives, the arrays are freed and the mesh is dropped.
5. **The `0x100`-byte entry fill** at `geometry+0x118`: node transform
   (0x20 bytes), `+0x20` distance, `+0x24` the "exactly zero" byte, `+0x28` a
   value derived from the mesh, `+0x30`/`+0x34` the accepted polygon array and
   its count, the four extrude arrays at `+0x40`/`+0x50`, the entity words at
   `+0xc0`/`+0xd0`, the `u16` id arrays through `0x142139080`, an array copy
   through `0x141c66b30`, and a two-lane `rsqrtss` normalisation written back at
   `+0xd0`/`+0x34`.
6. `GeometryFinalize` (`0x14213d0b0`) then remaps every `u16` through
   `0x142cee0e0` and calls `ShapeAddToBody` with the constant package
   (`100.0`, `1.0`, `0.1`, `FLT_MAX`, `0xffff0000`, `0x3000100`).

We have the brush *records*, the vertex decode, the ring builder and the mesh
conversion - i.e. steps before this pipeline - and none of the pipeline itself.
So for a real map's brush collision we would emit **the wrong geometry**: the
client extrudes each accepted, clipped polygon by `geometry+0x20` and drops
polygons the accept test rejects; we would take every triangle of the raw mesh.

### 2.2 The cell (pathing) 17-array semantics

Nothing reads the 17 arrays. If the map's navigation/pathing collision really is
in there, we cannot build it. This is the item `docs/open-questions.md` item 6
calls the "chunk decoder", and it is the one parse stage with **no code at all**
on our side - not a partial implementation.

### 2.3 The collision chunk's non-BVH data

The sector array (`0x58`, 36 elements on the range) has four runs we only
measure: 12-byte vertices, 1-byte attributes, 16-byte records, 4-byte indices
(`docs/formats/maps.md`). We do not decode them, and the client has no reader we
could follow. Additive collision (`kind 0x10`) goes through the same fixup but
was **never opened on a sample** (`chunk-kinds.md` section 6.3), so we do not
even know whether its `count_b` is 1 - which our level parser requires.

### 2.4 The C descriptor (model collision) - the two open links

`C` is class `0x564B1018`; the instance-point array is field `0x88FCECD7` at
`C+0x90` read through `C+0x98`/`C+0xa0`; seven type-`0x112` assets are decoded
and their 4-point rings pinned through the brush builder. Open: **the writer of
`[node+0xe8]`** (the object that owns `+0x48` -> `C`) and **the writer of
`[geometry+0x88]+0xe0`** (the ring's scale). Both were searched with
displacement/GUID-resolver intersections and found zero matches; the scale is
the reason every decoded ring sits at the clamp floor.

### 2.5 Filter/descriptor fields that decide what a query sees

`model_body` stops before the descriptor's `+0x00`/`+0x10` bounds quads, its
filter words, and its `+0x5c..+0x62` ids. We substitute all-ones. Also missing:
the entity's placement transform (`+0xd0`/`+0xf0`, writer open). On the practice
range these are inert; on any map with layered collision they are the difference
between hitting a wall and walking through it.

### 2.6 Narrowphase coverage -> parsing coverage

Types 2 and 4+ are read but unimplemented, so a parsed shape of those types
cannot be queried. Conversely the practice range is entirely type 3, which is
why this has not blocked the walk.

### 2.7 Non-mesh shape modes are silently dropped

`range_world.rs:394-397` keeps a holder only when its shape mode maps to
`Shape::Mesh` (mode `0`, descriptor type 3). The client's mapping in
`0x14201ae60` is `0`->3, `1`->2 (convex hull), `2`->1 (capsule), `4`->0
(sphere, and it also creates the second body). Every other mode is skipped with
no log line. On the practice range all 272 mesh holders are mode `0`, so there
is no loss today - on any map with capsule/sphere/point-cloud holders our world
is silently missing that collision, and the only reason it could not be swept
anyway is that types 2 and 4+ report `UnreadType` (2.6). Two gaps that mask each
other; worth one line in the load report so the next map's miss is visible.

### 2.8 Where we are *stricter* than the client (a 1:1 risk, not a gap)

- negative `count_a`/`count_b`: client no-ops, we error;
- cell boundaries must be sorted and tile the block list: client reads them
  unchecked; we reject;
- level mesh: exact tiling, root box sane, every corner in range; client does
  not check any of that (it has no reader at all here);
- `count_b != 1` on a collision chunk: client would fix up all of them, we
  refuse the payload.

Guards are good engineering; they are not 1:1. For every one of these, a real
payload the client accepts would make our client fail. Worth an explicit
decision in `docs/deviations.md`.

---

## 3. Verified vs not

### 3.1 Gated differential units (`dev/verify_harness`, client bytes vs Rust)

| unit | coverage | mutants | declared gaps that matter here |
|---|---|---|---|
| `fn/MapChunkCellBytesFixup` | 33% | 12/12 | non-allocating encodings only; cell-entry conversion outside; cell->shape, `0x100` entries, `ShapeAddToBody`, live world open |
| `fn/MeshSweep` | 61% | 11/12 | *stale text*: says the support-point path is unread; it is implemented and differenced separately (`dev/mesh_support_diff.py`). Genuine: BVH axis >= 2, depth-64 stack realloc, callback block |
| `fn/ShapeNarrowphase` | 100% | 11/12 | sink push `0x142cc1450`, callback return, types 2 and 4+ |
| `fn/ShapeWorldTransform` | 100% | 9/9 | which pool a real world fills; `record+0x2b8` array + per-type dispatch outside |
| `fn/ShapeDispatch` | 75% | 10/10 | type 3 not implemented in that unit, types > 4 uncased |
| `fn/CollisionSphere` / `fn/CollisionCapsule` | 89% / 94% | 12/12 | only the `count <= 0` paths; support-point path, other types, hit policy, ingest open |
| `fn/CollisionSweepBounds` / `fn/CollisionShapeFilter` | 100% | 11/11 / 12/12 | driver, transform and primitives outside |
| `fn/QueryDriver` | 100% | 11/12 | worklist push/free not modelled; callback stubbed; no type-3 leaf (support hull + emulator dependence); wrapper return not modelled |
| `fn/ClosestPointGjk` | 74.5% | 12/12 | rigid path separate; table/record synthetic |
| `fn/BoxTriangleOverlap` / `fn/TriangleDegenerate` / `fn/BuildBvh` | 100% | 12/12, 9/9, 10/11 | `TriangleDegenerate`'s two callers unread; `BuildBvh` neighbours/flattening in another unit |
| `fn/CharacterPosition` / `fn/RigidPosition` / `fn/VariantPosition` / `fn/WorldPosition` | 100% | 4/5, 4/5, 4/4, 1/1 | synthetic tables; **rigid tag path `0x142cb5310` not implemented** |
| `fn/MoveCharacterController` / `...Shape` / `fn/ApplyCharacterControllerMove` | 100% / 83% / 100% | 4/4, 8/10, 8/10 | only the no-shape path; the shape query and its contact event `0x142cf32d0` are open |
| `fn/MeshGather` | 90% | 28/28 | gather only |

Ledger timestamps: 2026-11-01 .. 2026-11-05. `crates/ow-sim/src/collide.rs` was
last touched 2026-11-06 and `dev/mesh_support_diff.py` exists (2026-11-03), so
**two ledger entries are behind the code** (`MeshSweep`, `QueryDriver` gap text).
Re-run those two units before quoting their gaps anywhere.

### 3.2 Emulator oracles (client bytes executed, output pinned, Rust compared)

| oracle | covers | cases |
|---|---|---|
| `dev/brush_vertex_oracle.py` | `0x142ced510` + `0x142ced620` | square, reversed square, triangle, degenerate triple, non-uniform scale, two rounding half-way cases |
| `dev/brush_mesh_oracle.py` | `0x142cf0360` **and** `0x142cf23c0` | 4 meshes exact; multi-ring at hull stage only; 3 client faults reproduced as errors |
| `dev/mesh_support_diff.py` | `0x142caed30` support/hull path + `0x142cc9460` handoff vs the shipped probe | 18 queries, 0 failures |
| `dev/gjk_closest_diff.py` | `0x142cc9460` body | 17 captured + 120 seeded descriptors |
| `dev/gjk_simplex_diff.py` | `0x142cc7740` four-vertex body | every region, block/count/barycentrics exact |

### 3.3 Read + hand-pinned only (no differential, no oracle)

| module / function | evidence |
|---|---|
| `chunkkind::fixup_in_place` (`0x142e492a0`) | doc says "not tested against the shipped bytes"; synthetic + real payload test |
| `collision_level` (level BVH) | no client code; structure checks + install numbers |
| `mesh_shape::shape_record` (`0x142cae540`) | 4 unit tests, no fixture |
| `mesh_shape::array_header` (`0x141b53af0`) | 4 unit tests; unpack instruction stream verified in this audit |
| `model_body` (`0x14201ae60`) | unit + install tests; filters yes, descriptor no |
| `model_collision::ArrayRecord` (`0x88FCECD7` tag rule) | unit tests; 7 install assets decoded |
| `domino_world` | unit tests of integer encodings only |
| `collider.rs::faces` | half-edge adjacency **not implemented**; runtime face copy not copied |
| `collider.rs::shell` | matches `MeshToColliderFaces` read; **no differential** (allocates internally) |

### 3.4 Not implemented at all (client code with no counterpart in the repo)

- `0x14213acf0` (route B ingest), `0x142cee720` (chord test),
  `0x14213c240` / `0x142cf1160` / `0x142cd75d0` (clip + extrude + accept),
  `0x1421360a0` / `0x142136010` (entry appenders), `0x14213d0b0`
  (`GeometryFinalize`), `0x14213a510` (route C ingest),
  `0x142cb3940` / `0x142cb7f10` (shape add / shape entry),
  `0x142cd4ef0` (`ShapeAddToOwner`), the Domino tick and pool management,
  the 17-array cell decoder (no address known),
  types 2 and 4+ narrowphases, the rigid character-controller tag
  (`0x142cb5310`), the contact event `0x142cf32d0`.

(The hit policy `0x14202bc90` and the candidate fan are implemented - see 1.9 -
but every real use of them needs the cell/sector ingest above, so they only run
over synthetic or model-mesh geometry today.)

---

## 4. Doc debt this audit found

1. `docs/formats/brush-collision.md:163-165` - "Until those are read, **no
   brush-to-mesh conversion is implemented**" / "`crates/ow-tank/src/brush.rs`
   does not convert a brush to a mesh and its module doc says so" - false since
   `crates/ow-tank/src/brush_convert.rs` (2026-11-03). The module doc of
   `crates/ow-tank/src/brush_mesh.rs` carries the same stale sentence ("the
   rest of the conversion ... is **not** implemented: ... Nothing here emits a
   triangle" is true of that module, false of the crate).
2. `dev/verify_harness/ledger.json` `fn/MeshSweep` and `fn/QueryDriver` gap text:
   says the support-point path is unread; `collide.rs` implements it and
   `dev/mesh_support_diff.py` differences it. Re-run the units.
3. `docs/formats/static-collision.md` "What this leaves" and
   `docs/formats/character-controller.md` "The collider build" both describe the
   brush ingest (`0x14213acf0`, `0x14213c240`) but no code cites either address;
   a reader could think the route is implemented. A one-line "not implemented"
   marker per stage would stop that.
4. `chunk-kinds.md` section 6.3 "The additive collision payload was not opened"
   - unchanged, and it now matters because `collision_level::parse` hard-requires
   `count_b == 1`.
5. `shape-ingest.md` section 7 concludes "the brush remains the only producer"
   from an exclusive-caller census of `ShapeAddToBody` (`0x142cb3940`), yet that
   target has 27 sites in 12 function groups and one of them is the per-model
   builder `0x14201ae60`. `python dev/pool_scan.py calls 0x142cb3940` either
   confirms or weakens the sentence - one command, worth running before the
   next round of work builds on it.
6. **Twelve cross-doc contradictions** about the same addresses are catalogued
   in `dev/collision-docs-claims.md` section 12 (produced in this audit). That
   section is the cheapest starting point for the next doc-cleanup pass - each
   item is two files disagreeing about one address, so one of them is wrong by
   definition.
7. `collision_level.rs` does not say in so many words that its module is an
   inference with no client reader. Its doc says the arrays are used as stored
   and cites `static-collision.md` check 4; it should say "no client consumer
   found" as loudly as `chunkkind.rs` does.

---

## 5. Suggested order of work (smallest read that unblocks most)

1. **Open one additive collision chunk** (`kind 0x10`) and record `count_a`,
   `count_b` and the tail layout. Closes `chunk-kinds.md` 6.3 and tells us
   whether `collision_level`'s `count_b == 1` is a real limitation.
2. **Read `0x142cca7d9`, `0x142ccb2db`, `0x142ccbc1c`** - the three verified
   callers of `BuildBvh` (`0x142ccb1b0`). `chunk-kinds.md` itself proposes them
   as candidates for a sector reader; that is the one route to the kind-`0x11`
   chunk's consumer that does not depend on finding a virtual dispatch.
3. **Read `GeometryFinalize` (`0x14213d0b0`) end to end** and the `0x100`-byte
   entry (`geometry+0x118`, stride `0x100`, field by field). That is the last
   document-only stage of the brush route and it names what the query sees.
4. **Read `0x142cd75d0`'s output contract as a `0x40`-byte record** and the
   clip's `param_4` sign convention (`geometry+0x24 ^ 0x80000000`). Without it
   the extrude is guesswork even if the code is transcribed.
5. Then implement route B's per-node loop, chord test and accept test as one
   `GeometryIngest` unit, with the accept test as its own differential against
   `0x14213c240` (it is a leaf: `0x142cf1160` + `0x142cd75d0` + an allocator
   stub, exactly the shape `dev/brush_mesh_oracle.py` already handles).
6. Only then touch the cell decoder: it needs a consumer first, and no consumer
   exists in the read set.

## 6. Code defects found while auditing (not doc bugs)

These are in the source, not the prose. All six were re-checked by hand.

1. **`domino_world::WORLD_TABLE = 0x1442_0bc0` is the wrong address.** The
   client's table is `0x1442a0bc0` (read from `OwnerCreateBody` `0x142cc3d90`
   and the allocator `0x142cc3c80`); the constant is `0x14420bc0`, `0x9000` low.
   It is currently **unused** (one declaration, no reads), so nothing
   misbehaves - but it is a wrong address sitting in source under a doc comment
   that quotes the right one, which is the trap the address rule exists to
   prevent. Fix the literal or delete the constant.
2. **`mesh_shape::array_header` does not implement its own rule.** Its body is
   `let _ = header_offset(slot[1]); header` - it ignores the tag at `slot[1]`
   and returns the caller's `header` unchanged. The tag rule is real (I
   disassembled `0x141b53a90`: `cmp [rdx+0x10],1` / `lea rax,[r8+8]` /
   `cmovnz`), and `header_offset` models it, but this function does not apply
   it. It is **unused** (one definition, no calls), so the parsed blocks are
   unaffected. Either read the header at `slot[0] + header_offset(slot[1])` or
   delete it; a documented-but-inert function is worse than none.
3. **`collider::build`'s doc names a variant that does not exist:**
   "*... returns [`Unread::BinCost`]*" (`collider.rs:321`) - the enum has no
   `BinCost`. The link does not resolve, so the one place the code says *why* a
   path is unread points at nothing.
4. **`model_collision`'s module doc links `[instance_array]`,** which is not an
   item in that module (the type is `ArrayRecord`, the rule is the tag rule).
   Broken intra-doc link.
5. **`ow_tank::brush` has no production consumer.** `Brush`,
   `build_from_instances` and `rings` are exercised by unit tests and
   `tests/brush_real.rs` only; nothing in `range_world`, `ow-sim` or `ow-client`
   calls into the module. The brush route's *input half* is proven and wired to
   nothing - consistent with 2.1, and worth stating plainly in its module doc.
6. **`ow_sim::walk` runs on fitted numbers that no doc lists.**
   `SLIDE_ITERATIONS = 3` (`walk.rs:70`), `WALKABLE_SLOPE = 0.7` (`:87`), the
   `normal[1] > 0.5` ground threshold (`:282`, `:382`) and the `1e-6` residual
   break (`:336`). The client's iteration cap is `step_state+0x94` clamped by
   `body[0x1a88]`, and its contact response is the unported Domino pair
   bookkeeping. `docs/open-questions.md` 1b declares the slide and `GRAVITY` as
   non-client but does not name these four numbers; `docs/deviations.md` has no
   row for them. For a live path this is the most consequential honesty gap in
   the collision set: the walk is exact until the first contact and fitted
   afterwards.

---

## 7. How to reproduce this audit

```bash
# two collision units, against the client's own bytes (harness)
python dev/verify_harness/cli.py MapChunkCellBytesFixup
python dev/verify_harness/cli.py MeshSweep
python dev/verify_harness/cli.py list            # every unit id

# the emulator oracles (need dev/data/offline_recover/module_offline.bin)
python dev/brush_mesh_oracle.py
python dev/mesh_support_diff.py

# install-side collision numbers (read-only)
cargo test --release -p ow-tank --test chunkkind_real -- --nocapture
cargo test --release -p ow-tank --test collision_level_real -- --nocapture
cargo test --release -p ow-tank --test range_world_real -- --nocapture
```

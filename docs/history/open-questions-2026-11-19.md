# Open questions archive - moved 2026-11-12

Two blocks moved verbatim out of `docs/open-questions.md`:

- the closed chain-fragment answer and the image-repair backlog (12-69);
- the character-collision round log (918-2139), including the closed
  "the character's own shape is a capsule" section.

Durable copies: the chain repair in `docs/ghidra-image-audit.md`; the
collision findings in `docs/formats/character-controller.md`, `maps.md`,
`static-collision.md`, `brush-collision.md`, `chunk-kinds.md` and
`brush-mesh.md`. The live residual is one entry in `docs/open-questions.md`.
The numbers are frozen at their session; a correction is a new entry in the
live file, never an edit here.

The prose is verbatim, including its links: they are written from the
repository root, because they were written in `docs/open-questions.md`.

## Chain fragments whose seam the parent never reaches - answered 2026-11-08

Closed by measurement, not by argument. Walking the CFG from each entry over the
function's own flow inside its `.pdata` union: 25,597 of 25,638 roots enter
fragment bytes, and only **41** never do (none named). The 1,615 figure in the
audit was "last instruction does not transfer" - an upper bound, since an earlier
branch usually enters the fragment. The 41 were reverted to their real extent by
`MergeChainBodies.java revert` and are in `C:/Temp/owaudit/seam_unreached.txt`.
The merge now carries the check itself (`NO-SEAM` gate), so a re-run cannot
re-widen them.

## Before anything else: the image repair backlog (2026-11-04)

**Status: the no-risk repairs are done in the live project (saved, from a
restorable copy at `C:/Temp/ghidra_snapshot/owmod_clean.rep`). Numbers, evidence
and the repair procedure are in
[`ghidra-image-audit.md`](ghidra-image-audit.md), section 11.**

Done. No `.pdata` extent, chain count or function count changed; the one thing
that did move was the over-reach of three bodies (§11, same cause unresolved):

- **110 of the 179 `noreturn` flags are cleared** - the flagged function's own
  body contains a `RET`, so it returns on that path. They blinded 11,552 call
  sites in 6,355 functions; the program now reports 69 flags and 28,412 call
  sites. Restorable with `ClearFalseNoReturn.java restore`.
- **The analyzers that caused it are off**: `Non-Returning Functions - Known`,
  `PDB Universal` (no symbol file ships with the install) and `... Discovered.
  Repair Flow Damage`.
- **The function-creation paths are closed.** `RecoverPointerFunctions.java`
  now creates a function only at a real `.pdata` start (measured: it would have
  created up to 72,786, now 4), and `dev/gh_apply_names.py` renames only - it
  was calling `disassemble_bytes` plus `create_function` when a rename failed,
  which is how chain-start and mid-body "functions" appeared. The 38 real
  starts that have no object are repaired by the same guarded script, not by
  the analyzer.
- **The 10 malformed `function-names.csv` rows are re-quoted** and four rows that
  named a chained continuation now carry `container=<real start>`, which
  `dev/frame_check.py` rule 4 re-verifies against the unwind records
  (`docs/function-names.csv`, the front matter explains the column).
- **Still open:** the merge of 25,184 chained fragments, the map repair (283
  chain-start objects, 38 missing starts, 251 multi-range bodies), the 118,578
  body/extent mismatches, data typing, prototypes, 47 shared names and the 79
  RTTI names with no pointer. The merge is not "low risk": every body it
  changes must be diffed, so it starts with `BodySnapshot.java`.
- **Function-less code targets stay open** (33,684 of them, 382 with ten or more
  references, worst 4,225): they have no `.pdata` entry, so a body would be an
  inference. This is the same rule as the one that bit the project before -
  more "functions" is not a better image.

`ImageWorklist.java` prints all of these lists in one run.

No gameplay read should be trusted from a function whose body stops at a
chained continuation: the decompile is truncated at a boundary the client
itself declares. That is why the merge is the next repair and not a cosmetic
one.

---

## 6. Character collision

**Question.** The client's character step/ledge/slope collision is unread. The
map mesh is currently visual only; the default client draws the practice-range
geometry but has no character collision. `--no-map` still offers the flat
stand-in plane and boxes for viewer work.

**What is read now (2026-11-01).** The sweep is `0x142cbcee0`
(`RunCharacterControllerSweep` -> `0x142cf5660` -> `0x142cf3f80`), called from
`MoverAdvance` with `([body+0x1ad8], world, &block)`; the resolved position
comes back from `0x142cbcd10`. The block is `{desired vec4, scale_b,
body[0x628]*scale_b, 0.005, step_state[0x94] clamped by body[0x1a88],
step_state[0x9a]}`. The sweep's contacts are `[world+0x200]`, count
`[world+0x20c]`, stride `0x20`; the advance projects each contact's first quad
onto the normalised service vector `[service+0x40]` (fallback `(0,1,0,0)`),
keeps the minimum in `body+0x19b8`, and calls the service object's `+0x40`
slot per contact. The `>= 4` contacts branch (`0x140ca8643`) is unread, as is
the `+0x40` response slot. The sweep reads precomputed triangle records
(normal `+0xc0`, count `+0xf6`, 0x30-byte vertices from `+0x18`), never map
chunk bytes.

**What would settle it.** Read the `+0x40` response slot, the `>= 4` contact
branch, and the map's collision chunks (`MAP_CHUNK_COLLISION`,
`teMAP_CHUNK_ADDITIVE_COLLISION`) into the shape records the sweep consumes.
The gap is the map-chunk-to-shape ingest: the client's spatial structure is
built by the command triple, and the producer of the triangle records from
chunk bytes is not read.

**The live collider is the model's brush tree, not the 17-array chunk
(2026-11-02).** `GeometryBuild` (`0x14213c440`) is read end to end and is the
only producer of a `0x60` cell: it takes the node's model
(`node+0x88` -> `+0x38` -> `+0x40` -> `+0x48`), copies the collision parameters
(`0x142132cf0`), pushes one cell from `geometry+0x108` (`0x142136010`, which has
this single call site), builds the mesh from the object's `+0x98` instance array
(`0x142cec9e0` / `0x142ced620` / `0x142ced990`), runs `BrushTreeToMesh`
(`0x142cf23c0`) and then `MeshToColliderFaces` (`0x142cf2010`) with the
collision radius and the object's `+0x0a` id, stamps the brush's two `u16` ids
into each `0x20`-byte face at `+0x18`/`+0x1a`, appends the cell's `u16` array
(`0x142139080`), normalises `geometry+0x42` by `sqrt(a*a + b*b)`, creates the
body (`OwnerCreateBody`) and finalises (`0x14213d0b0`, whose pool-A push
`0x142136130` also has one call site). Nothing else in the image pushes a cell
or a pool-A record, so the shape records the sweep walks are built from
**brush trees carried by models**, and the chunk's 17 arrays are pointer-fixed
at load with no reader found (round 2 reached the same place from the component
side). The decoder question therefore becomes two data questions: which model
the map's collision comes from, and how its brush tree is encoded. The
`0x142cca460` appender (`ColliderFaceAppend`) and the `0x142ccc120` wrapper
(`ColliderMeshEmit`) are read: the appender walks the `0x14`-byte index
records, reads the three vertices, skips a record `ColliderTriangleDegenerate`
rejects, and fills one `0x60` face record plus the running area; the wrapper
calls it and frees its three arrays.

**`MeshToColliderFaces` detail corrected (2026-11-02).** The thickening is
**z only**: the first vertex block is `(x + 0.0, y + 0.0, z + radius, w + 0.0)`
and the second `(x - 0.0, y - 0.0, z - radius, w - 0.0)` — `x`, `y` and `w` pass
through the signed zero add, which only turns `-0.0` into `+0.0` in the first
block. The index array is three blocks: every source face outward (as written),
every source face inward (`i2 + n, i1 + n, i0 + n`), then — only when
`radius >= 0.005` — one stitching pair per marked edge in face order, entry 0
pairing `i0`/`i1`, entry 1 `i1`/`i2`, entry 2 `i2`/`i0`, each pair being
`(a, a + n, b + n)` and `(b + n, b, a)`. Every `0x14` record carries `+0x0c = 0`
and the caller's surface id at `+0x10`. `crates/ow-sim/src/collider.rs::shell`
now matches this and its tests are updated; a differential unit for it is not
built yet (**not tested**), because the function allocates its two arrays
internally and the harness needs a pinned allocator region to compare them.

**The wish producer's evaluator is this query, and the chain is now read to
its narrowphase.** `0x1420328c0` (a wrapper) takes `[body+0x240]` — the
spatial world — and calls `0x14203dae0`, whose reachable body is read; it
resolves the geometry at `world+0x620`, takes the `u16` handle at
`[geometry+0x31f8]`, and calls `0x142cc45a0`, which decodes the handle through
`0x1442a0bc0` to a domino record and calls **`0x142cbd510`** — the collision
query driver. That driver is read: four passes (one per bit of the caller's
flag byte), each building its sweep with `0x142cd5540` (verified as
`fn/CollisionSweepBounds`), rejecting shapes with an AABB test and a
moving-box separation test, pushing child indices with `0x140d24b60`, and
routing leaves to `0x142cc1610`, which builds the entry's world transform
(`0x142cb8b80`), applies the layer filter (`0x142cbb350`) and dispatches on
the type byte (`0x142cb8d60`) to five per-type primitives (`0x142ccdf40`,
`0x142ccd7b0`, `0x142cd3c80`, `0x142caed10`, `0x142cd0ae0`). Type 0's
analytic path is now read and verified (`fn/CollisionSphere`,
`ow_sim::collide::sphere_sweep`), and the wrapper's layer filter is verified
as `fn/CollisionShapeFilter`, and the type-1 capsule as
`fn/CollisionCapsule`; type 0 and type 1's support-point **handoff** to
`0x142cc9460` is still open, but the solver body itself is now verified as
`fn/ClosestPointGjk` (264 cases, every channel of its `0x40`-byte output, 74.5%
branch coverage, 12/12 mutants); types 2 and 4+ are read but not implemented,
and type 3 is `fn/MeshSweep`.

**The next read is the geometry, not more query code.** The five per-type
primitives are inventoried with their sizes and their two-path shape
(`formats/character-controller.md`), and the shape world transform
`0x142cb8b80` is read **and verified** as `fn/ShapeWorldTransform`
(`ow_sim::collide::shape_world_transform`: 328 cases, every channel, coverage
100%, 9/9 mutants). The narrowphase wrapper `0x142cc1610` is read **and
verified** too, as `fn/ShapeNarrowphase` (`ow_sim::collide::shape_narrowphase`:
224 cases, every channel, coverage 100%, 11/12 mutants); its record layout, its
`context+8` inline filter and its dispatch table are in
`formats/shape-narrowphase.md`. The dispatch label `0x142cb8d60` is a real
entry point inside the transform's extent and takes a **fifth** argument (the
query block), which the earlier note's four-argument reading missed. What a
real candidate still needs is
(a) the collision geometry the query walks (`record+0x2b8` entries and the
per-type pools at `+0x2d0..+0x330`), which is the map's collision chunks,
(b) the hit policy's live collision world (`0x14202c580` -> `0x14202bc90`).
The type-3 composition question is **closed**: there is no single mesh-world
pointer. The `0x50`-byte element is `{scale[4] at +0x00, 1/scale[4] at `+0x10`,
root at `+0x20`, vertices at `+0x28`, triangles at `+0x30`}` with absolute
pointers, so the world is the address space they live in, and the arm is now
exercised by `fn/ShapeNarrowphase`'s corpus (6 type-3 hits). `block+0x70` is the
`u16` mask each triangle's `+0x1c` is tested against, which every earlier
fixture left zero. Types 2 (`0x142cd3c80`) and 4+ (`0x142cd0ae0`) are now
**read** (2026-11-07) but not implemented and not differenced: type 2 is a
convex hull handed over as face planes with an analytic plane-entry walk, a
`tmin`/`tmax` reject and a GJK fallback; type 4+ is a single kind, a
height-field grid walked by a rectangle scan plus a 2D DDA over cells. Layouts
and algorithms are in `formats/shape-narrowphase.md`; the working note is
`dev/data/notes/narrowphase_types_2_4.md`. The practice range is entirely
type 3, so those two do not block it, and `Narrowphase::UnreadType` stays the
honest answer until each has a differential unit.
Without those,
a candidate cannot be scored: the fan reads a record only the live hit policy
fills. So the next anchor is the chunk ingest in `formats/maps.md`, coupled
with the primitive bodies.

**The node-type table is enumerated (2026-11-02).** The "walk the 100 thunks"
plan was blocked by a project defect: the registry add `0x141c5ff40` had no
function, so all of its callers read as "0 callers". It is
`RegisterNodeType` (52 bytes) and has **144** call sites; creating it in the
project image made all 144 visible at once. The type table is **static data**
0x28 bytes apart from `0x143b8e5c8` (`{next, u16 type, class_registrar,
context}`, linked through head `0x144188170`), so it can be read from the
image without disassembly; `dev/probe_node_types.py` does exactly that. Each
class registrar obfuscates its own name (`name[i] ^= key[i & 7]`, key at
`len-8`), but the strings are generic — type `0x13` decodes to `"Unknown"` —
so **no name identifies the collision node**. The remaining discriminator is
the class size handed to `RegisterTypeWithSize` and the class vtable's build
slot: the chunk build must fill the `0x30`-byte type-3 mesh source from the
cell's quantized vertices (`arr13`/`arr14`) and its `0xFF`-separated index
runs (`arr3`/`arr11`/`arr12`) without calling `BrushTreeToMesh`.

**The pool side is read (2026-11-01).** The fixture/pool write path is now
whole: `DmWorldConstruct` sizes every pool from the world config
(`config+0x20` bodies, `+0x24` owners, `+0x28` shape entries, `+0x2c..+0x3c`
the five shape-type pools, `+0x70` islands); `ShapePoolAlloc` (`0x142cb00c0`)
is the per-element free-list pop (it takes the *pool record's* address, which
is why the earlier `+0x2cc`/`+0xa0` intersection failed);
`ShapeEntryFromDescriptor` (`0x142cb7f10`) copies a source descriptor into a
0xc0-byte shape entry and allocates the per-type record by the source's type
byte at `+0x67`; and `ShapeAddToBody` (`0x142cb3940`) / `ShapeAddToOwner`
(`0x142cd4ef0`) are the two "add a shape to a world" handles. So the only
missing Data step is the **descriptor producer**: no function yet found turns
the collision chunk's sector data into those descriptors. The chunk payload
itself is read as data (`formats/maps.md`, "The collision chunk payload"):
two STUD v1 blobs, a registered 0x78-byte component class `0x9199BF6B` with
three RVA fields into a 220-sector table, and a second, unregistered component
`0x99F089FE`. The next anchor is exactly that converter, plus the two open
narrowphases (types 2 and 4; the shared support-point solver `0x142cc9460` is
now `fn/ClosestPointGjk`).

**Two routes were tried and settled (2026-11-01).** (a) *Field hashes:* the
class descriptor's field table (`desc+0x28 = 0x143c3c620`, count 4 at
`desc+0x98`) is **all zero in the installed image**, and `0x1421cdb90` only
sorts it (`0x1421cce50`); the registered class name decodes to **`Unknown`**
(the XOR name format is recorded in `formats/maps.md`). So the four field
hashes are built at runtime and a search for hash immediates cannot find the
consumer. (b) *Geometry instantiation:* `0x14202c660` is the **owner's**
initialise step (`owner+0x620` = the 0x3210-byte geometry, two worlds from
`config[0x18]`), and its class is reached through the node-type registry:
`FUN_1404f6e10` registers `FUN_142029f00` as node type 2 in `0x141c5ff40`;
that factory builds a 0x30-byte type descriptor (vtable `0x1433654a0`) and
registers a 0x910-byte class via `0x141c4b050` → `TypeRegistrationCore`. The
descriptor's slot 2 (`0x1407803f0`) is a **release**, not an instantiate (round
12 below): it returns the instance to the allocation strategy, so the earlier
"key that selects the geometry class" is withdrawn. The creation is slot 1
(alloc) followed by the node's slot 8 with the record; the next read is the
asset-preload service's `vtable+0x38` (the all-preloads-done callback). The
consumer reads the fixed-up component offsets `+0x10`/`+0x58`/`+0x68` directly,
so it will not appear in a hash search either.

**The STU load path is read (2026-11-01), and it explains the fixup**
(`formats/stu.md`, "The load path"): `FUN_1421d26d0` resolves each sub-record
by class hash (`FUN_1421c14f0`), constructs it in place, and runs
`FUN_1421d3c10`/`FUN_1421d3710`, which convert self-relative offsets by the
class's **field types** (plain offset, array of offsets, array of pointers,
nested instance, hash map, string). With the shipping build's field tables
empty, nothing is converted generically — which is why the collision chunk's
completion calls its own fixed-up pass `FUN_141ff51a0`: **17 self-relative
qwords per `m_cellBytes` record**.

**The cell container is decoded and its consumer chain is read (2026-11-01).**
`formats/maps.md`, "The cell container, decoded": the `+0x68` field points at
a `{count, pad, data_off}` header; the 220 entries are `{cell_off, 0}`; each
cell is `{block_size, record_off}`; each record is 17 self-relative qword
offsets plus a 24-byte header, and the 17 arrays tile `record..record+
block_size`. All 220 validate and tile the payload with no gaps. The runtime
array encoding was **observed** under the oracle (`dev/probe_cellbytes.py`):
mode 1 = `{entries_ptr, u32 count}`, entries `{Q, 1}`, record `*Q`; mode 2 =
`{u32 count, pad, entries_ptr}`, entries `{Q, mode}`, record `*Q` when mode is
1 else `*(Q+8)`; offset 0 → null. The consumer chain runs
`GeometryFinalize` (`0x14213d0b0`) → `OwnerCreateBody` (`0x142cc3d90`) →
`ShapeAddToBody`, one body per `0x100`-byte runtime cell entry and one shape
per `0x40`-stride matrix in it.

**The container stage is implemented and the fixup is verified (2026-11-02).**
`ow_tank::collision::parse` reads the chunk's first `STUD` blob, the
`0x78`-byte `0x9199BF6B` component, the `m_cellBytes` header, the 220 cells and
each cell's 17 array slices, and checks that every record and boundary stays
inside the payload and that the cells tile it. The install test
`crates/ow-tank/tests/collision_cells_real.rs` pins the count, the first and
last cells and every array extent. `ow_tank::collision::fixup_cell_bytes` is
`0x141ff51a0` operation for operation; `fn/MapChunkCellBytesFixup` (264
iterations + edge pool, 12/12 mutants) verifies outer modes 1 and 2/3 and entry
modes 1 and 2/3, and declares the two allocate-then-deref-null mode-0 branches
dead. The **cell decoder** — who turns the 17 arrays into the `0x60` cell
entries (or the `0x100` sector entries) — is still the open link, so the
container is not yet a collider.

**The consumer's contract is read; the array overlay is excluded (2026-11-02).**
`GeometryIngestCandidateB` (`0x14213528a`) shows the decoder's output form: a
`0x30`-byte mesh source holding **three `{pointer, count}` pairs** whose arrays
hold **`0x60`-byte records** (copied by `func_141b53b70` at a `0x60` stride), one
body per entry and one type-3 shape per record. Measuring the practice range's
220 cells excludes the cheap reading: no array is a fixed-stride view (`arr7`
divides by `0x60` in 44/220 cells, `arr12` in 14, `arr14` in 22), and every
array is 4-byte granular. So the records are **built**, as on the brush path.
Full measurement in `formats/maps.md`.

**The brush ingest is read; the cell path is not that array (2026-11-02).**
`AppendCollisionEntryFromBrushSource` (`0x14213a510`) is read end to end: it
snapshots a brush source (`geometry+0xb0`, stride `0xb0`), meshes it with
`BrushTreeToMesh`, applies a leaf-or-chord-scale area test, and appends a `0x60`
entry (`MeshToColliderFaces` into `entry+0x20`, per-face stamp writes, handle to
`geometry+0xf8`). The geometry object's vectors are `{data, u32 capacity, u32
count}` headers at `+0xf8`, `+0x108`, `+0x118`, `+0x128`, `+0x138`. A whole-image
scan of stores to the entry count `+0x114` finds **three** sites: the brush
ingest, a clear, and the build's reset. **No cell code appends a `0x60` entry.**
The `+0x118` vector is not an alternative either: its append helper
`FUN_1421360a0` builds **`0x100`-byte** elements (`capacity << 8`), and that
helper has exactly **one caller** — the brush ingest. So this whole node class is
brush-only, and **the chunk's cells belong to a different node class**. Next
anchor: the class that owns the chunk component the bytes-fixup writes
(`sub+0x68` for slot `0x1e`, `sub+0x08` for slot `0x22`).

Two candidates are now excluded rather than pending. `FUN_1420c63b0` (10 500
bytes, four `ShapeAddToBody` sites) has **no code caller** and no plain pointer;
its three data xrefs are internal switch tables whose neighbouring words point
inside the same function, and its only call xref is a self-recursion. And the
sub-asset resolver `0x1421d26d0` has just **14 call sites** image-wide, only one
of which adds `0x68` (the chunk completion itself), so the consumer does not
re-resolve the collision sub-asset that way — it must be handed the cells from
the chunk work record (`work+0x60`, the payload the completion stores).

Note for the next reader: three callees were falsely marked `noreturn`
(`0x141bb00e0`, `0x142ced8f0`, `0x142efe2b0`), which stored this function as 167
bytes instead of 2011 and made several live addresses read as "no function".
The mechanical audit recipe is in `ghidra-project.md`.

**The collider build chain is verified except its two ends (2026-11-02).**
Read and proven this session: `MeshToColliderFaces` (`0x142cf2010`, the
double-sided shell), `ColliderFaceAppend` (`0x142cca460`, the `0x60`-byte face
records), the triangle skip test (`0x142cd56e0`, `fn/TriangleDegenerate`), the
BVH builder (`0x142ccb1b0`, `fn/BuildBvh`) and the type-3 narrowphase (earlier,
`fn/MeshSweep`). The builder's split rule had been mis-recorded: it bins the
**centroid** spread over 64 buckets on the widest axis and picks the cheapest
`area(left)*count(left) + area(right)*count(right)` boundary, taking the array
half only when every centroid extent is below `0.005`; the partition is the
swap pass at `0x142ccbc17`, not a stable shift. Still open on this chain: the
appender's weld bookkeeping (its neighbour links have no reader), the
flattening `0x142ccad80` (the node order and padding the implementation still
mirrors rather than reads) and `ColliderMeshEmit` (`0x142ccc120`).

**The cell entry is 0x60 bytes and the consumer is exact (2026-11-01).**
`GeometryClear` (`0x1421384e0`) fixes the geometry object's arrays: `+0xf8`/
`+0x104` 0x50-stride transforms, `+0x108`/`+0x114` **0x60-stride cell
entries**, `+0x118`/`+0x124` 0x100-stride entries, `+0xb0`/`+0xc0` and
`+0xc8`/`+0xd8` 0xb0 record pools. `GeometryFinalize` copies each 0x60 entry's
five `vec4`s into a 0xb0 record, steals its `{ptr,count,flag}` u16 array from
`+0x50/+0x58/+0x5c`, creates one body (`OwnerCreateBody`) and one **type-3
mesh** shape per entry (`ShapeAddToBody`, mesh source = record+0x20, scale
1.0, type byte at +0x67), and stores the shape handle at record+0x50.

**The node-type-2 path is brush-built, not chunk-built (2026-11-01).**
`GeometryBuild` (`0x14213c440`) reads the node's model
(`node+0x88` -> `+0x38` -> `+0x40` -> `+0x48`, a brush tree), copies the
collision params (`0x142132cf0`), allocates one 0x60 cell entry, builds its
transform, converts the brush tree (`0x142cf23c0`) and thickens it
(`0x142cf2010`), then creates the body and finalises. So the 220-cell STU
chunk has a **different** consumer: another node type. The registry is now
enumerated (2026-11-02): `0x141c5ff40` is a linked-list add to the global head
`0x144188170`, and its 144 caller thunks each pass a `{u16 type; factory}`
record. The types run `1..=0xb4` with holes; the factories are the
`NodeTypeNFactory` functions (type 1 class size `0x300`, type 2 `0x910`, type 3
`0x5c0`). No factory has yet been matched to the collision chunk; the next
attempt can start from that full (type, factory) table instead of the size
guess.

**The 0x30-byte mesh source is the decoder's output form (2026-11-02).**
`MeshShapeFill` (`0x142cae540`) copies it; a 0x60 cell entry holds it at
`+0x20` (`GeometryFinalize` passes `record+0x20`); and the brush path fills it
with `MeshToColliderFaces` (`0x142cf2010`), whose only two callers are
`GeometryBuild` (`0x14213c440`, at `0x14213c9d7`) and the builder that runs
from `0x14213a995`. So whatever the chunk decoder is, its output is
`{u64 nodes; u64 second; u64 third; u32 count; 5 floats}` handed to
`ShapeAddToBody`'s type-3 branch. The remaining unknown is the **BVH builder**
for mesh nodes (the narrowphase walks 0x20-byte nodes with the layout in
`crates/ow-sim/src/collide.rs`); find its caller and the decoder is one stack
up. `BrushTreeToMesh` (`0x142cf23c0`) has exactly four callers
(`0x14213a7ab`, `0x14213af82`, `0x14213c2c8`, `0x14213c9c0`), all brush paths,
so the chunk path does not go through the brush tree.

**The type-3 shape's producers are mapped (2026-11-02).** `MeshShapeFill`
(`0x142cae540`) has exactly five call sites: `ShapeEntryFromDescriptor`
(`0x142cb7f10`, from `0x142cb80d4`) — the generic descriptor-to-shape route,
itself reached only through two data tables (`0x14473d368`, `0x143829a20`) and
two calls, so nothing in that chain names the decoder; `FUN_14210bac0` (from
`0x14210bc96`), which divides the filled vertices by a scale; and **three sites
in code Ghidra never defined** (`0x140c28288`, `0x14207879f`, `0x1420a06eb`).
`GeometryFinalize` and `OwnerCreateBody` likewise have no code references: they
are reached indirectly. The chunk decoder therefore cannot be found by walking
callers from the finalize side; it sits in an undefined region and the next
attempt has to read it as raw disassembly (`dev/data/read/d.py`). Two of the
three sites are already informative: `0x140c28288` stores the new shape record
at `owner+0x50` and increments the counter at `owner+0xcd0`; `0x14207879f`
fills a **0x58-stride** record array — `record+0x50..+0x5c` from
`[src+8/+0xc/+0x10/+0x14]`, mesh source passed as `base + 0x20 + i*0x58` — so a
second, 0x58-byte mesh-bearing record shape exists beside the 0x60 cell entry,
with its vertices at `+0x20` just like the cell entry.

**Chunk kind dispatch is not the collision chunk (2026-11-02).** `0x140c66540`
switches on a chunk kind byte at `[chunk+0x13]`: kind `8` calls `[vtable+0x78]`
then `0x142076ab0`; kind `0x1a` calls `[vtable+0x88]`, copies a quad returned by
`0x141c5b950` to `payload+0x2040`, then `0x142075d00`, which stages three
records (kinds `1`, `0x15`, `0x40`) into the object at `+0x88`/`+0x2a8`/`+0x808`
under a hashed key (`func_0x141b32e50`). Those are placeable/spawn records, so
kind `0x1a` is a content chunk, not the 220-cell collision chunk.

**The collider surface is a shell, and the emitter is the last unread link
(2026-11-02).** `MeshToColliderFaces` (`0x142cf2010`) is read in full (detail in
`dev/data/read/decoder-notes.md`): it allocates an index array and a vertex
array, writes the vertices twice (`v + epsilon`, then `v - epsilon`), emits two
index records per face with the index order reversed in the second half, and in
its flag mode adds side-wall record pairs along boundary edges; then it calls
`ColliderMeshEmit` (`0x142ccc120`), which applies `ColliderTriangleDegenerate`
(`0x142cd56e0`, verified) before appending each triangle, and frees both
arrays. So a brush collider is a closed double-sided shell around the brush
plane, not the plane itself. `0x142ccc120` is now the next unit to read: it
holds the face append (`ColliderFaceAppend`, `0x142cca460`) and the pool A
cell writes.

**Search map for the decoder (2026-11-02), so the dead ends are not repeated.**
The chunk asset's completion path only uses `ChunkSlotForGuid` to pick the
fixup branch (its two call sites are `FinishMapChunkAsset` and the slot-8
walker), so the chunk kind is **not** a geometry selector. The asset
class-handler registry (`0x1421bc930`, keyed by `[class+0xb8]`, backing store
`0x143aa51b8`) is the level streamer: its only registered handler
(`0x1421a45b0`) handles asset types `0x0F` and `0x19` and calls
`FUN_14219aab0`/`FUN_14219cd50` for the map asset's dependency list, not the
collision cells. The class hash `0x9199BF6B` occurs in executable memory only
at the registration site; no consumer compares it. Node type 1 (`0x141ff9750`,
class `0x300`) is a level/map node (its constructor is `0x141ff90a0`, its size
virtual `0x141ff90d0`), type 2 is the `0x910` geometry. The geometry class's
method table at `0x143395050` has `GeometryBuild` at slot `0x80`; the sibling
slots (`0x14213a71a`, `0x14213bbe6`, `0x14213b995`, `0x14213b8bd`,
`0x14213bfd0`, and the 0x100-entry finaliser `0x142135030`) are the next
candidates, together with the writer of `geometry+0x118`/`+0x124`. The 0x100
entry itself is read: body config at `+0x50`/`+0x58`, a `0x40`-stride matrix
array at `+0x30` (count `+0x38`), the mesh source at `+0x18`/`+0x20` (three
vec4s from the 0x60 entry), a stolen u16 array at `+0xe0`/`+0xe8`/`+0xec`, and
a transform/terrain flag at `+0x60`/`+0x70..0x9c`. The 17 cell arrays
(`arr3`/`arr11` u16 runs, `arr12` interleaved, `arr13`/`arr14` 4-byte
quantized vertices) are the likely mesh source; the connection between them
and the 0x100 entry is exactly what is not read.

**Old note, superseded:** the writer of `geometry+0x108`/`+0x114`.  Its direct
caller is the unanalyzed geometry builder that runs from `0x14213bf60` (the
region before `GeometryFinalize`; no function boundary exists there yet). Read
that region with `create_function`/disassembly after the `Alloc` analysis fix
(`docs/ghidra-project.md`), then follow its caller to the collision-component
consumer. The 0x60 entry's own layout is fixed: five `vec4`s at `+0x00`..
`+0x50`, u16 array pointer at `+0x50`, count at `+0x58`, flag at `+0x5c`, id
at `+0x0a`.

**Three search routes are now closed (2026-11-01)**, so they need not be
retried: `imul reg, reg, 0xc0` finds nothing in the collision module, a raw
disp32 scan for the pool displacements is worthless (they are ordinary stack
offsets), and the image contains no `COLLISION`/`ADDITIVE` strings — those
names are the install data's, not the client's. What *is* read is
`DmBroadphaseReinit` (`0x142cbdc60`): it is a BVH builder over the fixture set,
rebuilt on demand, writing each pass's root index at `+0x44` through a
`0x100`-entry stack node pool and calling the verified `0x142cd5540` for the
swept bounds. The tree is therefore a function of the fixtures, and the single
remaining unknown is one function: **who fills `[record+0x2b8]`'s fixtures.**

**The world and its pools are read; the fixture *pop* is not.**
`0x142cc3c80` creates a world (0x4d0-byte header + eight strided pools, handle
encoded into `0x1442a0bc0`, self-index at `record+0x194`), the pool table is
`record+0x288 + 0x18*i`, and the fixture record's `+0xa0` dword doubles as the
free-list link. Release and update/insert are read
(`0x142cb0d08`/`0x142cd4555`, `0x142cd4610`), and the fill/publish side is
`0x141d71200`. What is missing is the free-list pop, and it cannot be found by
displacement scans: no `inc`/`add` of `record+0x2c0` exists in the image even
though release decrements it. Next: `0x141d71200`'s head and the caller of its
vtable slot `+0x40` (`0x142f744a0`).

**The geometry side is read; the chunk side is not (2026-11-02).** The `0x60`
cell entries and the `0x100` sector entries are written by one driver at
`0x14213bbc0` (vtable slot `+0x78`), which walks pool A (container at `GEO+0xb0`,
stride `0xb0`, count `+0xc0`) and pool B (`GEO+0xc8`, count `+0xd8`) and calls
builder #1 `0x14213a510(GEO, index, desc, force, out)` at `0x14213bd61` and
builder #2 `0x14213acf0` at `0x14213beaf`. A pool A record already holds
decoded geometry — vertex array at `+0x00` (`0x20`-byte elements), triangle
array at `+0x08` (`0x28`-byte elements), counts at `+0x10/+0x14/+0x18` — which
builder #1 copies into a mesh record (allocations `count*0x20` and
`count*0x28`), converts with `BrushTreeToMesh` (`0x142cf23c0`), then pushes a
cell: `+0x00..0x1f` two `vec4` from the mesh record, `+0x20` the `0x30`-byte
mesh source via `MeshToColliderFaces` (`0x142cf2010`), the `(GEO+0xa8, GEO+0xaa)`
index pair into every triangle at `[cell+0x30] + k*0x20 + 0x18/+0x1a`, and
`+0x50/+0x58/+0x5c` the `{u16* list, count, flag}` face list from
`0x142139080`. Sectors are filled the same way with a transform, a distance and
a limit. `GeometryFinalize` (`0x14213d0b0`) then walks `GEO+0x108`
(stride `0x60`, count `+0x114`), skips the first entry, copies five `vec4` into
a fresh `0xb0` record, takes over the face list and creates one body plus one
type-3 mesh shape. The brush path (`0x14213c440`) shares those cell-fill steps
and then calls `OwnerCreateBody` and `GeometryFinalize` itself. What is still
open is the **input**: no code in the geometry class reads `chunk+0x2428`, so
the function that fills the two record pools from the 17 fixed-up arrays
remains unidentified; the record layout it must produce is now known exactly
(section 4.1 of `dev/data/read/decoder-notes.md`, which also holds the search
map and every dead end).

**Round 2: the chunk container is not the collision source for movement
(2026-11-02).** Two facts changed the picture. (a) The pool A records — the
only input of the cell/sector builders — are created by a **primitive
importer** at `0x1421363d1` (pool `push_back` `0x142136130`, grow path
`0x142136160`), which fills a record's `+0x00/+0x08/+0x10/+0x14` through
`0x14213a6aa` (reached only from `0x14213ad6b`, inside builder #2). Its inputs
are a local `0x30`-byte mesh descriptor at `+0x20` (via `0x14212c210` ->
`0x141bce330`, element stride `0x2c`, counts `+0x14`/`+0x18`, physics index
`+0x28`), a rig node chosen from `[GEO+0x88]` by the record's `+0x70` slot
index, and two rb-trees (`0x142cbb020`/`0x142cba920` for triangles, stride
`0x4c`, first dword = count; `0x142cbb300`/`0x142cbacc0` for vertices, stride
`0x34`). Its output form: vertices `0x20` stride = 3 f32 position + 2 f32
(halved) + 4 f32 at `+0x18`; triangles `0x28` stride = 3x u16 at `+0x00` plus
3x u16 physics indices at `+0x06`. The source object is class type **9**, not
the type 10 the driver validates. (b) The 17-array container's only in-image
contact is its fixup (`0x141ff51a0`, single caller `0x141ff57f7`) plus a
type registration at `0x1400fdcb0` (class hash `0x9199bf6b`, record size
`0x78`, alignment `8`, **array element `0x50`**, eight stub vtable slots and
empty name strings, registered through `0x1421cd360`). No geometry-class code
reads `chunk+0x2428`. So the runtime colliders that `fn/MeshSweep` queries are
built from **brush/model primitives**, and the 220-cell container is not on
that path. **Consequence for movement:** the read chain that matters is the
primitive side — `0x142cec9e0` / `0x142ced620` / `0x142ced990` / `0x142ceca00`
(brush tree build), `BrushTreeToMesh` (`0x142cf23c0`), `MeshToColliderFaces`
(`0x142cf2010`), the pool A importer `0x1421363d1`, then `GeometryFinalize`.
None of those four is read yet, and the 17-array container's consumer stays an
open stage per the hard rule.

**Round 3: the cell consumer (2026-11-02, census corrected).** The first version
of this paragraph said the component's class was "streamed, never queried" and
that the decoder was open "by absence". That conclusion is **withdrawn**: it
rested on Ghidra's caller metadata, which is incomplete for this image
(`ghidra-project.md`: `.pdata` entries chain and fragment, and undefined
regions hide call sites). Ghidra reported 3 callers for `ShapeAddToBody`
(`0x142cb3940`) where a verified `E8 rel32` scan finds **27** sites in 13
functions, and the "two functions read `+0x10`/`+0x58`/`+0x68` and call both
builders" claim was simply wrong (`0x14208d010`/`0x142090e00` call neither;
`0x14208d109`/`0x1420919b7` hold those sites and their `+0x58`/`+0x68`
operands are `f32` radii, not 16-byte array headers).

What the corrected census *does* establish: for `ShapeAddToBody`,
`OwnerCreateBody`, `ShapeAddToOwner`, `ShapeEntryFromDescriptor` and
`MeshShapeFill` the union of Ghidra's callers and the verified scan is 28
producing functions (27/17/1/2/5 sites), and **none of them calls
`StuLoadSubAssets`, none loads a qword from `+0x60` (the chunk work record's
payload field), and none reads the component's `{pointer, mode}` headers** —
the only qword read at `+0x68` is `ShapeEntryFromDescriptor`'s own descriptor
field. The whole-copy route is checked too: image-wide there is exactly one
`memcpy(…, 0x78)` among 8 923 `memcpy`/`memmove` call sites
(`0x142402824` in `FUN_142401de0`) and it copies a diagnostic string fragment.
Table and evidence: `formats/maps.md`, "The cell path's consumer: open". The
byte-level fact that stands is narrow: the `STUD` magic `0x53545544` is
mentioned only by the loader `StuLoadSubAssets` (`0x1421d26d0`) and the writer
`FUN_1421d2ab0`.

So the decoder is **open**: a reader that builds no shape through those five
handles, or one reached through a route this census does not enumerate, is not
excluded. The three anchors named here were then read (2026-11-02): `0x1420c63b0`
consumes a `0x38`-stride node/transform record array at `[owner+0xd0]` plus a
`0x20`-stride transform-handle array at `[owner+0xe0]`; `0x140c2813f` fills its
mesh from a **brush collider** (`ColliderMeshEmit`); `0x142078562` computes a
box from a **stored mesh source's bounds** (`0x142cae600`) into a `0x58`-stride
record; `0x1420a02f6` is a **mesh-source assembler** over the owner's
descriptor container at `+0x140`/`+0x148`, indexing `0x60`- and `0x40`-stride
arrays; and `0x142cdf9f2` appends `0x40`-byte entries to the **Domino owner's
body vector** and calls the handle setter `0x142cb4f20`. None of the five
touches a chunk payload or a `0x9199BF6B` field, and **none is reachable**: no
`E8`/`E9`, no immediate, no 8-byte pointer, and no obfuscated-pointer encoding
of any of their addresses exists in the image — their only references are the
`.pdata`-copy and code-range tables that every function start has
(`ShapeAddToBody` carries one of those too, beside its 27 call sites). So the
chunk path is neither opened nor closed by them; the single next reader is the
**producer of the owner's mesh-descriptor container at `+0x140`/`+0x148`**
(`formats/maps.md`, "The three next readers").

**Round 4: the brush route, and the chunk labels (2026-11-02).** The second
producer is now read from the node down and implemented where the reads allow:

* `GeometryBuild` (`0x14213c440`) resolves the node's model
  (`geometry+0x80` → `[node+0x38]` → `[+0x40]` → `[+0x48]` = `C`, the model's
  collision descriptor) and builds the brush **at runtime** from `C`'s instance
  points; the four "leaf builders" are not a loader: `0x142cec9e0` header init,
  `0x142ceca00` mesh-struct init, `0x142ced620` the ring builder (1/400 units,
  mask `{1,1,0,0}`, `roundf`), `0x142ced990` the edge list, `0x142ced510` the
  vertex decoder.
* Records read and implemented in `ow_tank::brush` (11 units): brush `0x20`,
  vertex `0x20` `{i64 x, i64 y, i64 divisor, u32 index}` with
  `position = (x/divisor) * 0.0025`, node `0x28`, ring walk `+0x04`, next ring
  `+0x10`. Every arithmetic expectation is the client's own output:
  `dev/brush_vertex_oracle.py` executes `0x142ced510` and `0x142ced620` under
  the emulator (bump-allocator stub at the hook word `0x1442a0158`) and the
  tests pin those bits.
* `BrushTreeToMesh` (`0x142cf23c0`) is read to its per-node **six-op**
  classification; the op bodies and the triangle emission are **open**, so no
  brush→mesh conversion is implemented. `MeshToColliderFaces` (`0x142cf2010`)
  is read and already implemented (`ow_sim::collider::shell`).
* **The install has no brush asset.** The content manifests this install opens
  carry **0** records of GUID type `0x00A` (1,425,794 records, 177 types;
  1,514,352 with `WinPrism_` forced), while `0x00C`/`0x00D` are populated;
  300 model assets' chunk tags are `MDLC/MRNM/MSTU/MESD/mskl/CLTH`, no brush
  container. Pinned by `crates/ow-tank/tests/brush_real.rs`. So the format is
  read but no instance of it is reachable from this install yet: `C`'s class
  and the `+0x98`/`+0xa0` inline-singleton-vs-array encoding are the next read,
  together with whether type-`0x00A` assets are synthesised.

**The chunk labels are read now, and they were inverted (2026-11-02).** The
kind table `0x1439aa320` and its name decoder `0x1404c19b0` were read (length =
low 24 bits of the dword at `name-4`, key = eight bytes at `name-0xC`, byte `i`
XORed with `key[i & 7]`; all 36 names decode, and the eight placeable names the
project already knew come out exactly). The answer:

| kind | slot | name |
|---|---|---|
| `0x11` | `0x1D` | `MAP_CHUNK_COLLISION` |
| `0x12` | `0x1E` | `MAP_CHUNK_PATHING` |
| `0x10` | `0x21` | `teMAP_CHUNK_ADDITIVE_COLLISION` |
| `0x23` | `0x22` | `teMAP_CHUNK_ADDITIVE_PATHING` |

The per-slot dispatch agrees: the two collision kinds are the two sent to
`FUN_142e492a0` (slots `0x1D`/`0x21`), the two pathing kinds are the two sent
to `StuLoadSubAssets` + `MapChunkCellBytesFixup` (`0x1E`/`0x22`). So the
220-cell `STU_9199BF6B` container this project spent two sessions on is the
**pathing** chunk, which explains why the 28-function producer census found no
reader for it: a client-side pathing consumer need not exist. The container and
fixup reads stand; only the label was wrong.

The **collision** chunk is `0x0DD0001100000688` (21,858,688 bytes). Its
container is read and implemented (`ow-tank::chunkkind`, 7 tests):
header `count_a` at `+0x08` (36) and `count_b` at `+0x0C` (1), array A
`0x58`-stride at `+0x20` (`0x14D3810`), array B `0x38`-stride at `+0x28`
(`0x14D8948`), the in-place fixup `FUN_142e492a0` (header null-preserving,
element qwords unconditional), and a per-sector tiling over four runs
(`12 B × c0`, `1 B × c1`, `16 B × c1`, `4 B × c2`, counts at
`+0x2C`/`+0x30`/`+0x34`) that holds for all 36 sectors.

**The collision consumer is the open link, and it is now specific: who fills
the Domino shape-entry pool at `[record+0x2b8]`.** The dispatch only fixes the
payload up, the handoff `FUN_141fe37a0` stores it at `work+0x60`, the fixup is
called only from `FinishMapChunkAsset`, and no function in the producer census
sees a sector array. One earlier claim needs correcting: `BuildBvh`
(`0x142ccb1b0`) is **not** callerless — a verified `E8` scan finds five call
sites in three functions (`0x142cca7d9`, `0x142ccb2db`, `0x142ccbc1c`), so the
collider builder is reachable and its callers are candidates.

Full detail: `formats/chunk-kinds.md`, `formats/maps.md` ("Which chunk kind is
which: read", "The pathing container's consumer: open") and
`formats/brush-collision.md`.

**Round 5: the shape pools are enumerated (2026-11-02).** The world record's
pool table is read (`DmWorldConstruct 0x142cbeb70`: eight records at
`+0x288 + 0x18*i`, `{ptr, used, cap, high, free}`; shape entries are index 2 →
`+0x2b8`, element `0xc0`). A verified store census (119,802 decoded function
groups, with the `-1` free-head sentinel as the discriminator) finds the only
writers are construct, clear and reset; the pool **contents** can only be
created through `ShapeAddToBody`/`ShapeAddToOwner` → `ShapeEntryFromDescriptor`.
Every caller of those that appends collision material is a brush-mesh producer
(`0x142136010` one caller `GeometryBuild`; `0x1421360a0` two, both brush;
`BrushTreeToMesh`'s four callers are the four appenders). So the collision
chunk is **not** the Domino shape source; the only loophole is an inline
appender that no decoded store shows. `BuildBvh`'s three callers are split
wrappers over the collider-face array and all unreachable; "a sector reader must
end in a BVH" is false.

**Correction (2026-11-11): "BuildBvh's three callers are unreachable" was a
fragment artifact, and the chain is now named end to end.** The three
addresses (`0x142cca7d9`, `0x142ccb2db`, `0x142ccbc1c`) are not three
functions: the first is a fragment of `ColliderFaceAppend` (`0x142cca460`) and
the other two are interior points of `BuildBvh` (its own recursion).
`BuildBvh` has exactly one external caller, and that one is reachable:

```text
0x142137480 -> 0x14213bbc0 (the geometry driver)
                 -> 0x14213a510 AppendCollisionEntryFromBrushSource
                 -> 0x14213acf0 (the 0x60-entry builder)
0x14213c440 GeometryBuild -> 0x142cf2010 MeshToColliderFaces
                            -> 0x142ccc120 ColliderMeshEmit
                            -> 0x142cca460 ColliderFaceAppend
                            -> 0x142ccb1b0 BuildBvh (fn/BuildBvh, verified)
```

**The conclusion of round 5 stands** - the source is the brush, not the
collision chunk - but it now rests on named functions instead of on an
unreachability that was an artifact of counting fragments as functions. The
shape record itself is now proven too: `MeshShapeFill` (`0x142cae540`) is
verified as `fn/MeshShapeFill` (96 differential cases, 8/8 mutants killed),
and the crate's `ow_tank::mesh_shape::shape_record` is that expression dword
for dword.

With the chunk excluded, the live producer is the **brush**, and its
tree→mesh walk is now read and implemented: the hull stage, the status tree
(insert fixup, erase + rebalance, both rotations), the six ops, the ear
predicate from the `SHUFPS` immediates, the clipping tail and the face writer
are in `ow_tank::brush_mesh` / `brush_convert` (25 tests). The client's own
code was run end to end under the emulator and **all four** pinned meshes
reproduce bit for bit, half-edge marks included. The arrow's divergence was
one misread link, not a tag strictness: ops 1, **2** and 5 all read the `+0x28`
link, and the port had 2 and 5 on `+0x30` — invisible until an op splices,
which the arrow's op 2 does. Found by single-stepping the client's tag pass and
hooking the tree calls; the collinearity hypothesis was wrong. Faults are
refused, never approximated: a clockwise ring and a two-node ring (both at
`0x142cf265f`), the two-ring allocation at `0x142cf0b6b`, and the `+0x58 == 0`
variant.

`C`'s instance encoding is read (`+0xa0 == 1` ⇒ `{points, count}`, otherwise
`{count, points}`); **its class is open**, and the census now gives a
structural negative rather than a lead: with disp8 scanned, **no function in
the image stores both `+0x186` and `+0x98`**, and the function that writes a
near-perfect match for `GeometryCopyParams`' field set
(`0x14208edf6..0x14209087f`) has **no static reference at all** — no call, no
tail jump, no pointer — so its "builds `C`" reading is downgraded. The lesson
is recorded in `formats/brush-mesh.md`: a displacement is not a type.

The chain was then walked backwards (2026-11-02): the geometry is created by
the node-type factory `FUN_1420d0ef0` (two classes on `[node+0x29]`),
`geometry+0x80` is written by the class's own vtable slot `+0x08`
(`0x142138ce0`), and the node-type-2 update (`0x14204cbce..0x14204cfc9`) moves
`[node+0xe8]` into `[node+0x38]` and reads the three links exactly as
`GeometryBuild` does. The writers of `[node+0xe8]`, `[object+0x40]` and
`[instance+0x48]` were **not found**: three probes came back negative (the
field-set fingerprint `{+0x28,+0x29,+0xe8}` hits only a hash-table relocator; the
"calls the GUID resolver ∩ stores `[reg+0xe8]`" intersection is empty; no
function in the node-type module writes `+0xe8`). So `C`'s class, the `+0xa0`
encoding value and an install brush remain open.

Correction on the way: `0x14208edf6` was called reachable through an RVA table,
but the `0x14469xxxx` records are **`.pdata` copies**, not dispatches; the only
computed jump is the switch at `0x14208e74e` that selects fragments of one
function body. That is a fragment dispatch, not an external call, so the
reachability claim is withdrawn.

**`C` is named by type identity (2026-11-02).** The release path showed that
`[node+0x38]`/`[node+0x40]` are refcounted resources whose class is only a
runtime vtable (`0x141b251d0` calls vtable slot `+0x18`), so the class came
from the registration direction instead:

* **`C` = registered class `0x564B1018`, size `0x198`, vtable `0x1431dd118`** —
  the client's own registration `0x1402db070` passes `r8d = 0x564B1018` and
  `size 0x198` to `0x1421cd360`, and 22 of the class's 36 declared field hashes
  are immediates in the client's two field-dispatch functions
  (`0x141334ea0`/`0x141335e60`); `0x077CF4AA` (`+0x186`), `0x88FCECD7` (the
  array at `+0x98`) and `0x06A1888C` (the inline sub-object at `+0x90`) all
  appear in the client's own code. OWLib's registry was used only as a check
  (size and the unique `+0x186` field), not as the source.
* `C+0x98` is a pointer to a 16-byte array header, `C+0xa0` a whole-qword tag,
  `1` selecting `{points, count}` and anything else `{count, points}`; the array
  is a `Vec3A[]` of the inline sub-object at `C+0x90` — the model's instance
  points. Implemented in `ow_tank::model_collision` (5 tests).
* `+0x48` is an **asset reference**: `C` is the loaded instance of
  `teStructuredDataAssetRef<STU_564B1018>`, declared at `+0x08` of the entity
  component `STU_09CB5A77` (size `0x30`) and `+0x28` of `STU_2FD9D8D6` (size
  `0x38`). `0x48 = 0x08 + 0x40`.

**The one read still missing** is the class at `[node+0x38]` (the producer of
`[node+0xe8]`), which nothing on the chain names. The next anchor is data, not
code: parse the model's `MSTU` blob for an `InstanceInfo` of type `0x564B1018`
and read its `0x88FCECD7` array through the already-pinned
`brush`/`brush_mesh`/`brush_convert` pipeline; that yields the practice range's
real brush with no new code. Also recorded: the node-type-2 update is **one
function at `0x14204cb40`** (its second half starts at `0x14204cbce`), with one
static caller, the dispatch at `0x14204824a`.

**But no model carries `C` (2026-11-02).** All 32,278 readable model assets in
the install were scanned for the class hash: **zero** mention it. `C` lives in
**entity** assets instead — GUID type `0x112`, chunks `FRAC` + `FSTU` (a
fracturable's collision), reached from an entity component `0x09CB5A77` field
`0xA7ED6024`. Nine entity assets name seven `C` assets, and the practice
range's 229 models, 79 placed assets and 34 chunks carry none. So the brush
route is the **fracturable** path, not the map's static collision; the pinned
brush from a `C` asset (4 points -> 4 vertices, 2 triangles, conditional on the
`max(1.0, [W+0xe0])` scale floor) is real but not the range's geometry.

That reopens the static-collision question with two concrete leads: (a) the
practice range's **550 entity references** were not checked against the seven
`C` assets — if one is among them the range does carry a `C`; (b) `C+0x08` is a
`STU_537E56E4` holding a **prebuilt collision mesh** (7-node array, 8 points,
16 triangles, `nodeCount` 7, `height` 4) — the same class may sit in static
models' `MSTU`, and the chunk's sector arrays (12-byte vertices, 16-byte
records, 4-byte indices, 36 sectors) look like a mesh cache too. The chunk
consumer search should also be re-run with the 4-byte-RVA dispatch method that
the `0x14208e74e` case established, on `0x1420a02f6` (the mesh-source assembler
whose input is the owner's descriptor container).

**The three checks are in (2026-11-02).**

1. **The range does not carry a `C`.** The kind-`0x0B` chunk
   `0x0DD0_000B_0F69_002E` (550 placeables, 202,688 B) was searched byte-level
   and field-level against the nine referrer entities and seven `C` assets: **0
   hits**, over 80 distinct parsed entity GUIDs. So the brush route is dead for
   this map.
2. **The range's models do carry a prebuilt collision mesh.** 229/229 have an
   `MSTU`; **161 of 229** carry `STU_537E56E4` (272 instances) inside holders of
   class `0x2DD96D6D` (279 instances), reached as
   `STUModel 0x871BD3D0 -> field 0xCB4D298D (+0x118) -> 0x2DD96D6D ->
   0xB7C8314A -> STU_537E56E4` (nodes/points/triangles/nodeCount/height).
   `GeometryBuild` does **not** read it, so the missing read is **the consumer
   of `STUModel`'s `0xCB4D298D`**. That is now the static-collision route.
3. ~~The chunk's consumer is unreachable by the validated RVA method.~~
   **Withdrawn (2026-11-02):** the address was a `.pdata` **chained fragment**.
   Its chain root is **`0x14209fc90`** (four entries to `0x1420a075d`) and it has
   one `E8` at **`0x1420251f5`** inside **`0x142024d30`**, called from the
   collision component's per-frame update slot (`0x142019944`, slot
   `0x142f7d688`) and from `0x14201fbe3`. The RVA census measured the fragment
   start, so its zero is a fragment artifact. **The chunk assembler is on the
   component-update path.** But it is **not the chunk's consumer**: it reads the
   **model** (`[[node+0x110]+0x28]`, else `0x141b3d1c0(node)`) and walks
   `STUModel+0x140` with `holder+0x2a == 1` -- the same list the per-model body
   builder reads with `== 0`; its `0x50`-byte context is scratch. On the range
   the plan is empty (279 holders, **0** with the mode byte;
   `ow_tank::model_shape_source` + install test), and the byte is a declared
   holder field (`0x89AABA94` at `+0x2a`, `0x141510382`), default 0. So the
   **chunk's own consumer is unknown again.**

**Next anchor: the consumer of `STUModel+0x118`.** The node-type-2 factory
`FUN_1420d0ef0` picks a **second** geometry class (`Alloc(0xf0)`,
`0x142132f20`) when `[node+0x29] != 0`; that class is the natural consumer
candidate, since `GeometryBuild` is the first class and does not read the model
mesh.

**The model-mesh consumer is read (2026-11-02).** The second class is **not**
the consumer (its 20 vtable slots never read `+0x118`/`+0x140`). The offset was
wrong in the reference: the client registers `STUModel` at **`0x200` bytes**
(`0x14034af00`) and its field registration passes **`0x140`** for
`0xCB4D298D`, not `0x118` — proved by the same helper's offsets for `C`
(`0x3963555A → 0x110`, `0x12B475D4 → 0x194`). The consumer is
**`FUN_14210bac0`** (array-header idiom at `0x14210bb6b`/`0x14210bc04`):
elements are pointers to holders, it requires `holder+0x20 == 0` (mode) and
`holder+0x08 != 0` (mesh block), and its entry chain is
`param_2+0xe8 → object; object+0x28 → STUModel; object+0x08 → attachment table`.
It calls the unpacker `0x141b53af0` and `MeshShapeFill` (`0x142cae540`, read
exactly: a `0x4c`-byte record with scale, `1/scale`, three pointers, four words,
`scale × m_5BD33E19`), consumed by `0x142caf6c0`.

**There is no conversion: the mesh is passed by pointer.** Implemented in
`ow_tank::mesh_shape` (4 tests). The block in the install: 161/229 range
models, 272 blocks; first block (model `0x0d0000000000078c`): 288 B of 9×32
nodes, 8 points, 384 B of 16×24 triangles, `nodeCount` 9, `+0x3c` 12, `height`
5, `+0x44` 96.0f; a 4-unit box.

**The classification question is now decisive.** `FUN_14210bac0` contains **no**
`ShapeAddToBody`/`ShapeEntryFromDescriptor`/`OwnerCreateBody`, and its caller
is a broadphase `FUN_1421088e0` (four bucket arrays at `+0x1e8`, sphere cull
against `model+0x450`) with caller `0x14212e070`. So the model mesh may be a
**query-only** structure, not the character's Domino world. The next read is
`0x142caf6c0` (what the `0x4c` record fills) and that caller chain, to classify
the system: character sweep, or raycast/overlap. If query-only, the character's
world source is still missing and the remaining candidate is the second class's
slot `+0x50` (`0x142134f70`, which does call `OwnerCreateBody`).

**The classification verdict (2026-11-02): the model-mesh path is query-only, and
no read path fills the character's Domino world with the range's static
collision.** `0x142caf6c0` writes **nothing persistent** — it bounds the query
points, collects candidate triangles into an inline `0x80`-entry stack buffer,
and runs `0x142cc6740` per triangle; no world, body or shape entry. Its owner
`FUN_14212e070` is a scene-side static-collision **query** owner (0x18-byte
entries at `+0x2b0`, count at `+0x2b8`, six `0x50` bounding volumes, a lazily
built broadphase at `+0x2d8`), not the Domino world (whose `+0x2b8` is a
`0xc0`-stride pool). The `f0` class's slot `+0x50` (`0x142134f70`) reads
`[node+0x38] → [+0x40] → [+0x48]` = `C` and builds the **fracturable** body.
And the character's own ingest entry `0x142cbcd40` has exactly six callers —
`0x140801c24`, `0x140ca6123` (inside `MoverAdvance`), `0x140dc6e14`,
`0x140eb7b22`, `0x140f1528b`, `0x140f17f4a` — all in the mover/AI ranges
(`0x140c`..`0x140f`), none a map loader; `RunCharacterControllerSweep`'s nine
`E8` sites are likewise all in `0x140c`..`0x140f`.

So the ingest chain `0x142cbcd40 → 0x142cf35a0 → ShapeAddToOwner → 0x142cd4350
→ ShapeEntryFromDescriptor` is character-only, and the map-side shape
producers we read (`GeometryBuild`, the brush sources) need a `C` the range
does not have. **The remaining read is the sweep's world argument**: does
`RunCharacterControllerSweep` query the scene-side static-collision owner
(via a vtable) rather than the Domino world? That would make the model mesh the
character's collision after all. If it does not, the client's local collision
for the range has no source in the read paths and the movement plan needs a
scope decision.

**The sweep's world is the Domino world — no vtable, no scene owner
(2026-11-02).** `RunCharacterControllerSweep` (`0x142cbcee0`) takes the world
struct as `param_1`; its one real call is the 10-byte thunk `0x142cf5660`
(`test rcx,rcx; jne 0x142cf3f80`), body `0x142cf3f80` (four `.pdata` chunks).
The world struct's **first dword is a variant tag**: `1` selects handle
`[world+0x10]` and the `0x142cb5xxx` accessors, otherwise handle `[world+0x08]`
and `0x142cd4fxx`/`0x142cd5xxx`. Every accessor decodes the handle exactly like
the query driver (`slot = handle>>0x30`, rejected ≥ `0x3fff`, then
`*(0x1442a0bc0 + slot*8)` XOR/add decode), and `FUN_142cd4d90` walks
`[world+0x2b8]` (stride `0xc0`, next `+0x54`, link `+0x48`) and
`[world+0x348]` (stride `0x58`) — the Domino shape entries and pairs. All four
chunks contain **zero indirect calls** and no call to `FUN_14212e070`,
`FUN_1421088e0`, `FUN_14210bac0`, `0x142caf6c0` or `0x142caea70`.

**So the character's collision world is the Domino world, and the model mesh is
not in it.** Combined with the ingest census (six callers, all mover/AI), the
brush needing a `C` the range does not carry, and the chunk being unreachable,
**no read path inserts the practice range's static collision into the character's
world.** That is a scope statement, not a missing instruction: either the client
receives that collision from outside the binary (server-authoritative
movement), or it is inserted by a system not in any path read so far. One
concrete unread anchor remains if the search continues: the pool-A importer
`0x1421363d1`, named in the geometry-object chain but never read.

**Round 6 (2026-11-02): the per-model route is live.** The operator's framing
was right: collision is a property of the model. The builder is
**`0x14201ae60..0x14201c887`** (a six-entry `.pdata` chain; the earlier
`0x14201af49`/`0x14201b1cd` were **fragment starts** inside it, which is why the
first reference search found nothing). Its root has **four `E8` callers** —
`0x142022030`, `0x1420195e0`, `0x142014b50`, `0x142015f70` (call sites
`0x142022219`, `0x14201985c`, `0x142014d07`, `0x142016129`), all runtime
state-sync functions of one collision component, two of them interface slots of
the table at `0x142f7d320` driven by the per-frame update `0x142019880`. It
walks `[model+0x140]` (tag `+0x148`), filters `[holder+0x2a] == 0`,
`[holder+0x08] != 0` and the mask word (bit 25 of `[component+0x420]` selects
`+0x580` else `+0x470`), and for each selected holder emits a Domino shape
through `ShapeAddToBody` (`0x142cb3940`) on a body from `OwnerCreateBody`
(`0x142cc3d90`); mode `0` is descriptor type 3 (the mesh, unpacked by
`0x141b53af0`), modes `1`/`2`/`4` are types 2/1/0. Read and implemented in
`ow_tank::model_body` (8 unit tests; install test `model_body_real`).

**The range's plan:** 163 of 229 models carry a holder list (279 holders, 272
mesh blocks); every mask is all ones, and the filter byte and shape mode are at
the class default `0`, so **all 272 mesh holders become type-3 mesh shapes** over
161 models.

**Open:** which nodes receive the component (its interface table has no RTTI or
self-pointer; the leading facts are node `+0x1c` bits `0x20`/`0x40`, byte
`+0x242` bit 7, and the owner path `+0x620 → +0x31f8`), whether the range's
static props receive it and whether that owner is the world the sweep uses, and
the descriptor's bounds/filter/`+0x5c..+0x62` sources (documented, not
converted). No runtime body count has been observed — everything above is the
plan from the install's bytes plus the read filter.

**Round 7 (2026-11-02): the owner id is a Domino world slot.** `0x14202f800`
(no-`.pdata` leaf) returns the `u16` at `[[node+0x620]+0x31f8]`;
`OwnerCreateBody` (`0x142cc3d90`) rejects `>= 0x3fff` and the empty slot, decodes
`(*(u64*)(0x1442a0bc0 + id*8) + 0xb3269b88d44d5fb2) ^ 0x77600ba5adcc265c`, and
allocates in `[world+0x288]` at stride `0x1c0`. That is exactly the sweep's
handle decode (check 7). The world allocator `0x142cc3c80` stores the inverse,
and the two constant sums are 0 mod 2^64, so the empty slot is `encode(0)` —
`ow_tank::domino_world` (7 tests). A node allocates **two worlds** per
`0x14202c660` (`+0x31f8`/`+0x31fa`) and creates its own static body from
`+0x31f8` at `0x14202d268` (handle at `+0x31fc`), so per-model bodies are
siblings of the node's own body in the node's world.

**The attachment gate, read field by field:** `[node+0x454]` bit `0x200` (copied
from the node creation record's `+0x10`; without it the node has no `+0x620`
record, hence no id), pending node `+0x490` promoted to `+0x250`, node `u16` at
`+0x1c` with bit `0x20` set and bit `0x40` clear, `[node+0xe8]` model object with
`+0x84` bit `0x80`, `[second+0x242]` bit 7 clear, and model `+0x28` non-null.
**None of these is model-asset data**, so whether the range's placed props pass
is the unread node-creation record — not a data test. That is stated, not faked.

**Open (one place):** which class allocates the component and pairs it with a
node — its interface table `0x142f7d320` is COMDAT-folded (shared base, no RTTI,
no self-pointer; the interface `this` is 0x20 past the worker object), and the
node-binding entry `FUN_142017010` (12 `E8` callers) takes eight arguments from
an unread table. Also: whether the character's own handle
(`[controller+0x1ad8]`) names the same world slot. Descriptor bounds/filter
fields remain documented, not converted.

**Round 8 (2026-11-02): the component is named, and the character's world is the
node's world.** Constructor `0x14200a9a0` (`0xad0` bytes; primary vtable
`0x142f7d608`, base interface **`0x142f7d648`** at `+0x20` — the earlier
`0x142f7d320` is a neighbouring folded table; third vtable `0x142f7d458` at
`+0x118`); registration `0x14200bab0` (`RegisterTypeWithSize`, kind `0x14`,
size `0xad0`, name decodes to `Unknown`); type-object vtable `0x1433633c8`,
slot 1 the factory `0x14200a5f0`; pair-with-node `0x14200f840` ->
`0x14201cf40(object, entity)`. The attach hooks take the node from
`[entity+0x250]` and test no asset-derived condition.

The character's `[controller+0x1ad8]` is written only at `0x140ca6132` from
`0x142cbcd40`'s return, and `RunCharacterControllerSweep` is called with it at
`0x140ee8ad1`. The mover's descriptor `+0x46` is `0x14202f800(node)` — the same
`u16` at `[node+0x620]+0x31f8` the per-model builder reads — and `0x142cbcd40`
builds the character's shape/body in that world (`0x142cf35a0` ->
`0x142cc3f50` for tag 0, `OwnerCreateBody` for tag 1). **So the character and the
per-model bodies share the node's world slot.**

**Open (round 9):** whether a placed node passes the node gate (`[node+0x454]`
bit `0x200`, copied from the node creation record's `+0x10` at `0x14202c6e4`).
**Round 13 addendum (2026-11-03):** the record is now read structurally as
`{+0x10 flags, +0x18 world/transform descriptor}`; `+0x18` is read at its
`+0x10` into `[node+0x630]` (`0x14202d1c6`) and handed to the world allocator
`0x142cc3c80` twice for the two world slots, so the gate word and the node's
placement arrive in the same record. Its producer is still the anchor.
The creation entry is **`WorldBase` slot 8** (vtable `0x143367630`,
`0x143367630 + 8*8 = 0x143367670`; the earlier "slot 32" measured `0x100` from a
neighbouring table base). `WorldBase` is `0x910` bytes (ctor `0x1420271d0`,
allocator `0x1420266d0`), and NodeType2's factory `0x142029f00` registers that
`0x910` class with kind `0x14`.

**Tool defect fixed:** `call qword ptr [reg+disp]` has a **disp8** form
(`FF 5x 40`). The image has **0** disp32 `+0x40` call forms and **505** disp8
byte patterns, so a disp32-only slot scan reports a false negative for every
slot below `0x80`; `dev/call_slot_scan.py` now scans both. None of the sites
passes a creation record.

**The count, re-measured 2026-11-17 by `dev/corpus.py index` (decode-anchored,
instruction boundaries only): 523 `call [base+0x40]` instructions = 485 on a
register base + 38 on `rsp`/`r12` (SIB, a stack-local call-through, not a
vtable slot). Against that, `call_slot_scan.py` finds 505 raw byte patterns and
confirms 492 by back-decoding; 473 sites are in both sets, 12 are decode-only
and 19 are byte-scan-only (a `FF 5x 40` at an address that is not an
instruction boundary in the linear stream - the desync class this file warns
about elsewhere). The **485** is now the pinned number (`GOLDEN_SLOT40` in
`dev/corpus.py`), the base register is stored per site, and `rax` carries 434 of
them.

**Kind `0x14` is a family, not an identity:** 160 `RegisterTypeWithSize` sites
use it with sizes `0x18` .. `0x31868`; the identity is the **size `0xad0`**
(exactly one site, `0x14200bbb1`). A kind-based install census would match
hundreds of unrelated classes, so none was written.

**Still open:** the slot-8 transfer that hands the record over, including
**register-indirect** forms (`mov reg,[vt+0x40]; call reg`), and the writer of
the record's `+0x10`/`+0x18`.

**Round 10 (2026-11-02): the support-point hull is exact; the GJK accept is
open.** `0x142caed30`'s support branch is read operation for operation and
implemented in `ow_sim::collide::mesh_sweep`: `conj(q) ⊗ block_q` (conjugate
`XORPS` with `(-0,-0,-0,+0)`, never normalised), rotation rows with
`1-(2s·s)-(2s·s)` and lane-3 zeroing, translation from `start - position`,
`((p.x·row0 + t) + p.y·row1) + p.z·row2`, then the record's inverse scale; fold
`MINPS`/`MAXPS`; `start = centre = (max+min)*0.5`, `end += centre - start`,
`step = (max-min)*0.5 + expansion + 0.02` (unconditional). The client's triangle
test `0x142cce430` takes the read path only when `count == 0` and all scaled
expansion lanes are `≤ 0.005` (ordered compare, so NaN fails); otherwise it
calls **`0x142cc9460`**, a closest-point GJK with a cached simplex, ≤ `0x13`
iterations, subfunctions `0x142cc8d80` (3-simplex) and `0x142cc7740`
(4-simplex). **`dev/mesh_support_diff.py`: 17 cases, 0 failures, 11 exact, 6
open (the triangle accept).** The client's contact for the translated capsule
case is known (tri 1, ids `0x12`/`0x22`, point `(0.75,0.9,3.01)`, normal
`(0,0,1)`, fraction `0x3df5c290`) but not reproduced. Next: read `0x142cc9460`
and the two simplex functions and pin those 6 cases.

**Round 11 (2026-11-02): the GJK accept is read and the mesh cases all agree.**
`0x142cce430` is a chain root (`0x9e` B) whose slow path is the fragment
`0x142cce5bb..0x142cce82b` (the earlier function reading missed it);
`0x142cc9460` is read operation for operation (radii floored at `0.01`,
`r = radA + radB - 0.01 + 0.00125`, `|XMM2|² < r²` convergence, support argmaxes
onto `XMM3`, fraction refine with `1.1920929e-07`, reduce dispatch, cap `0x14`),
and `0x142cc8d80` (3-simplex) is fully read: six signed dots, the face vector,
three plane terms, the eight regions in order, face weights = plane terms over
their sum, per-edge surviving-vertex permutations. Implemented in
`ow_sim::collide`; `dev/mesh_support_diff.py` is **18 cases, 0 failures, 0
open**, and the pinned contact (tri 1, ids `0x12`/`0x22`, point
`(0.75,0.9,3.01)`, normal `(0,0,1)`, fraction `0x3df5c290`) is reproduced.
`dev/gjk_simplex_diff.py` 30 seeded 3-simplices agree; `dev/gjk_closest_diff.py`
17 live + 120 seeded, 0 failures.

**Round 13 (2026-11-03): the 4-simplex `0x142cc7740` is read and implemented.**
`0x1632` B, the three-vertex body one dimension up: an orientation fix-up that
swaps vertices 1 and 2 when `dot(rot3(rot3(c-a)*(b-a) - rot3(b-a)*(c-a)), a-d)`
is negative in every lane; four vertex regions (`count 1`, direction `-a` etc.);
six edge regions (`count 2`, barycentrics from the two endpoint dots over their
sum, direction the double rotation of the pair sum about the edge); four face
regions (`count 3`, weights = the face's three triple products over their own
sum, direction the face vector or its negation whichever faces the origin,
record copies into slots 0/1/2 per face); and the interior case (`count 4`, the
four plane terms as lambdas over their sum, direction zero, `AL = 0` when the
sum is non-positive). `dev/gjk_simplex_diff.py` runs seeded quadruples through
the client's bytes: every region is hit, the block, count and barycentrics agree
exactly on all 60 cases, and the four-vertex direction agrees within one ULP on
its low lanes. One genuine quirk is recorded in
`formats/static-collision.md`: the `a-b-d` face's stored vector is the sign
opposite of the vector its own guard and weights are written against. No state
is left unimplemented; the residual is the direction's last bit on entered
tetrahedra, and a real penetration simplex from the narrowphase is `not tested`
(none of the 18 mesh queries builds one).

**Round 12 (2026-11-02): two false leads are removed; the next anchor is the
map-load path.** (a) The class static descriptor's slot 2
(`0x1407803f0`, formerly named `TypeDescriptorInstantiate`) is a **release**, not an
instantiate: it calls the instance's first vtable entry and returns the memory
through the allocation strategy's slot `0x58` (`0x141c5f420`, a per-block
free-list return; the allocator is slot `0x50`, `0x141c5ef30`, reached by
`0x141c4b270`). So the "key that selects the geometry class" is withdrawn —
the decoded word is the instance handle. (b) The "world-creation path is
obfuscated" row is withdrawn: `0x14055a0f0` is the map-load entry (it registers
the CVar whose name decodes to `debug.showMapLoadAfterMS`), truncated in the
project image by a false `noreturn` (`0x141bb8d40` returns); its `0x140be1570`
builds a `0x910` `WorldBase` plus a `0x2ad0` camera-view object for the
map-load path, and is the **only** `E8` caller of `WorldBaseCtor`. Node
instances go through the reflected type allocator `0x1420266d0`, which has no
`E8` caller and one 8-byte pointer (`0x1433654a8`). (c) A third holder consumer
exists: `0x142090e00` (13 contiguous `.pdata` entries, span
`0x142090e00..0x142094d34`, stored as a 186-byte stub) creates a Domino world
and body per streaming owner through `0x142cdf8e0`/`0x142cdf930` ->
`0x142cde350` -> `0x142cc3c80`, sets the world record's `+0x19b` byte, and
walks holders with `+0x2a == 4`; the range has none. **Next anchor:** `[[service]+0x38]`, the all-preloads-done callback in
`0x141fe0580` (reached from `MapPreloadAssets` `0x14077b7d0` and
`0x14077b5e0`). The state object is the `0xf8`-byte global `_DAT_143b796a8`
(ctor `0x140544de0`): three GUID maps at `+0x88`/`+0x90`/`+0x98`, a lock at
`+0x28`, total `+0x68`, done `+0x6c`. The service class is identified: vtable `0x142f8d970`, size `0xf8` (deleting
destructor `0x14077afa0`), and slot `+0x38` is `0x141fe0f80`, which sets the
loaded byte at `+0x78` and clears `+0x70` -- the callback only flips the state.
The level start is the **poller of `service+0x78`**, the next read. The service
pointer is the encoded field at `context+0x78` (decoded by `0x14054a010` with
the anti-tamper qword at `0x143b79598+0x2f`). `0x140554220` is the engine
subsystem constructor, not the map load.
Full detail: `formats/static-collision.md` check 17.

**Round 13 (2026-11-03): the evaluator runs end to end offline, on a built
world.** `dev/probe_candidate_evaluator.py` seeds the two prologue globals, the
guard block `Q = [0x143b79598]`, the `world+8` service decode, a domino record
and one type-3 shape, then runs `0x1420328c0` under the harness oracle. It
reaches the wrapper, the obfuscated body, the handle dispatcher, the query
driver, the narrowphase, the type-3 mesh walk and the hit policy, and fills the
answer record: point `(0.75, 0.9, 0)`, normal `(0, 0, 1)`, fraction `0.5`,
`word34 = 1`, ids `0x12`/`0x22`. The guard's "two encodings" question is
**closed**: the key byte is the constant `0x26`, the table is at
`0x142f7d158`, and the equal branch is a null-byte read trap (r12 is zeroed at
`0x14203db79`), so one body exists. The record layouts (pass sub-records at
`+0x1b0`, entries at `+0x2b8`, transform pool at `+0x2a0`, type-3 pool at
`+0x318`) are in `formats/character-controller.md`.

**The real geometry now runs too.** `crates/ow-tank/examples/mesh_world_dump.rs`
writes the assembled world (272 distinct shapes, 4,797,872 bytes) and the
probe's `--mesh-world` builds the record, entries, pools and BVH around it. With
the pinned shape alone, the client reproduces `mesh_world_real`'s pinned hit
exactly (point `(0.99500012, 4.0, 0.99500012)`, normal `(0, 1, 0)`, fraction
`0x3e8051b8`, triangle `7`).

**Round 20 (2026-11-06): the placeable transform is read and applied.** The
dump now carries **3,017 placed instances** — one per model-group entry, each
with the entry's `translation`/`rotation` — and the probe writes them into the
owner pool `[record+0x2a0]`, which is where `ShapeWorldTransform`
(`0x142cb8b80`) reads a shape's world frame. With all 3,017 in one BVH the
pinned query returns **the same answer record byte for byte** as the
single-instance run (point `(23.947973, -19.990925, 31.369896)`, normal
`(0.360226, 0.926024, 0.112766)`, fraction `0x3e8051b8`, triangle `6`). Before
the placement the same query hit a different shape (y `5.843`, triangle
`7054`), so the local frames were the whole of the error. The chain is:
`STUModelComponentInstanceData`'s `m_pose` triple -> the scene component's
`+0x20` vtable slot `0x142003720` -> `0x142000620` -> component `+0xd0`
(quaternion) / `+0xf0` (position) -> the collision builder -> `OwnerCreateBody`
`0x142cc3d90` -> body `+0x20`/`+0x10` -> `ShapeWorldTransform`.
`docs/formats/character-controller.md` carries the addresses.

**Round 19 (2026-11-03): the map blob carries the chunk payload pointers.**
`LoadMapAssetChunks` (`0x141ff5c80`) stores one handle per chunk in the map
asset blob; the completion `0x142e49400` decodes them and writes the payload
pointers at `+0x2F8` (collision, kind `0x11`), `+0x300` (pathing), `+0x308`
(streaming), `+0x310`/`+0x318`/`+0x320` (slots `0x20`/`0x1F`/`0x23`), with the
extra-chunk payload array at `+0x2F0` and the "loaded" byte at `+0x410`.
`DmWorldConstruct` sizes a `0x58`-stride pool at `world+0x348` from the
description's `+0x40 + +0x44`; `0x142cc2ae0` walks it with `0x58` stride and
`0xC0` records at `world+0x2B8`. The collision chunk's `0x58` sectors are
pointer-based, the world's pool records are index-based, so they are not the
same record. `dev/field_index.py` gains a `map-asset` profile; no
first-argument consumer of the blob exists in the map/scene/collision bands
(the completion itself is a naked thunk target without `.pdata`), so the
sector consumer is likely a job lambda reached through the job table.

**Round 18 (2026-11-03): the node record is read field by field, and the map's
entity components are named by data.** The record consumer `0x14202c660` is
decompiled whole (`formats/static-collision.md` check 18): `+0x00` id ->
state record `+0x170`; `+0x10` flags -> `[node+0x454]`; `+0x18` a **Domino
settings description** whose `+0x10` block is copied to `[node+0x630]` and
which is passed to `0x142cc3c80` twice for two worlds; `+0x28`..`+0x48`
copied; `+0x50` an asset reference decoded with `ror 0xf`/add/xor; `+0x60`..
`+0x8f` -> node `+0x878`..`+0x8a4`; `+0x90` copied by `0x141c60250`. Each flag
bit has a read site (`0x20` state record, `0x200` geometry + worlds, `0x01`,
`0x02`, `0x04`, `0x40`, `0x80`, `0x4000`, mask `0x15`). The description is a
**capacity template** (19 pool counts at `+0x20`..`+0x70` in
`DmWorldConstruct`; rotation `+0x10`/w `+0xac`, layer count `+0x20`, child
list `+0x140`/`+0x160` in `ConstructDominoWorld`), not map geometry, so a
node's worlds start empty of shapes. The init's own `OwnerCreateBody`
descriptor is constant (no shape source): the init creates a body record but
no shape. The 0x300 object it allocates on flag `0x20` is a separate **state
record** (`0x142026e20`, no vtable, id at `+0x170`), not the builder's scene
component. Separately, the map's entity component records are `{u32 component
id, u32 offset}`, and the STU asset at the offset names the class
(`crates/ow-tank/examples/map_entity_probe.rs`): 550 entities, 644 records,
13 ids, 6 named (`0x02` `STUModelComponentInstanceData` on 475, `0x03`
`STUStatescriptComponentInstanceData`, `0x06` `STUWaypointInstanceData`,
`0x0b` `STUTargetTagInstanceData`, `0x10` `STUECAISpawnerInstanceData`,
`0x11` `STUECHintPointInstanceData`); none of the 13 is a collision
component, matching the collision component attaching at the node.

Next for this item: (a) ~~the placeable transform~~ — **read and applied**: the
builder's second argument is the **0x300-byte scene component** (ctor
`0x141ff90e0`, kind `0x14`, vtables `0x1433632e0`/`0x143363320`), attached to
the node's component list `[node+0x520]`/`[node+0x52c]`; its vtable `+0x20`
slot is `0x142003720`, which copies three quads out of the component's instance
data (`+0x160` position, `+0x150` scale, `+0x140` quaternion) and hands them to
`0x142000620`, the writer of `+0xd0`/`+0xe0`/`+0xf0`. The map chunk carries the
same triple per placed model (`ModelGroupEntry`), and the probe reproduces the
client's answer with all 3,017 instances placed; (b) the record's producer, now
constrained to a writer that supplies a Domino **settings** description with
the flags word; (c) a Rust implementation of the four-pass driver, so the
probe's oracle can become a `fn/CandidateHitPolicy` unit; (d) the geometry's
variant table for negative ids; (e) the **serialised** mapping from the
`STUModelComponentInstanceData` stream to the `(position, scale, quaternion)`
triple — the client reads it through the component's accessor, so a Rust
reader that wants the component's own value rather than the map chunk's must
read `m_pose` (class `0x936A1A00`, not in the reference class list).

### The character's own shape: a capsule, and both links are closed (2026-11-07)

`ow_sim::walk`'s `0.35` m sphere is a **declared proxy**. The read that replaces
it is now complete on the shape side; what is left is three runtime scalars.

**The shape is a capsule, not a sphere.** The chain is
`0x142cbcd40` (record) -> `0x142cf5440` (`Alloc(0x590)`) -> `0x142cf35a0`
(descriptors) -> `0x142cc3f50` (owner pool) + `0x142cd4ef0` ->
`0x142cb7f10` `ShapeEntryFromDescriptor`. The record builder `0x142cbcd40` was
read in full: the mode at `+0x00` is **always `0`** (it writes `local_b8[0] = 0`),
so the character always takes the shape-add path and never the body path.

**Where the capsule comes from (all read).** `0x140ca5db0` is the single caller
of `0x142cbcd40`; it builds the `0x60`-byte source from the mover object and
`0x142cbcd40` copies it field for field. The capsule is
`A = (0, r, 0)`, `B = (0, r + d, 0)` with

- `r = max(0.02, [mover+0x5a4])`,
- `d = max(0.1*r, [mover+0x5a0] - ([mover+0x5a8] + 2*r))`,

so it is a **vertical capsule of radius `r` spanning `y` in `[0, 2r + d]`**, in
the character's local space, with the lower sphere centre `r` above the origin.
`record+0x30` carries `r`; `record+0x10`/`+0x20` carry `A`/`B` as `vec4`.
`record+0x40` is the mover's world point and `record+0x50` is the constant
`(0,0,0,1)`; `record+0x80` is `{shape id at +0x46 of the source, 0x10b9 or
0x10a1}` and `record+0x88` is `0x3fd55555` (`1/1.5`); `record+0x98` is the
physics object pointer.

**The type-1 element (link 1, closed).** `ShapeEntryFromDescriptor` does write
`entry+0x24`: it calls `ShapePoolAllocType1(world+0x2e8)`, stores the index, and
fills the `0x30`-byte element directly with `{P0, P1, radius}` from descriptor
`+0x70`/`+0x80`/`+0x90`, plus `entry+0x5c` = the radius and `entry+0x60` =
`max(|P0|, |P1|)`. The type byte is at **descriptor `+0x67`** (the builder writes
`0x0100` at `+0x66`), which is what selects this branch. `entry+0x20` = the type,
`entry+0x24` = the index.

**The radius (link 2, closed).** `record+0x30` -> descriptor `+0x90` -> element
`+0x20`, which is exactly where `0x142ccd7b0` reads it.

**The descriptor is two structs, not one.** `0x142cf35a0` builds an **owner**
descriptor `{+0x00 qword, +0x10 quad, +0x20 quad, +0x30/0x32 u16}` for
`0x142cc3f50` and a **shape** descriptor - the generic
`ShapeEntryFromDescriptor` source - for `0x142cd4ef0`. Modelling them as one
struct with the kind at `+0x34` was wrong; `crates/ow-tank/src/character_collision.rs`
now models both and its 10 tests pin every offset.

**The values (closed 2026-11-07).** The three mover scalars are copies of
`STUCharacterMoverComponent` fields: class `+0x78` `0x1972C687` (extent, maxed
by `ConstructMoverBody` against `2.1 * radius + offset`), `+0x7c` `0x7A70E3E3`
(**the radius**) and `+0x80` `0x4A576BC2` (the height the segment is derived
against). `ow_tank::character_collision::capsule_from_mover` is the client's
construction and `ow_tank::entity::mover_capsule_of` reads it per hero; the
install test `every_hero_declares_a_collision_capsule` measures 54 playable
heroes (37 at `0.5 / 1.5`, 15 at radius `0.7`, two outliers, and one landing
exactly on the `0.1 * r` floor). On the query side,
`ow_sim::walk::World::sweep_capsule` sweeps that shape through the verified
mesh leaf (`collide::mesh_sweep_with_points` supplies the two centres).

**The step limits (closed 2026-11-07).** `ConstructMoverBody` (`0x140ca9730`)
stores the mover's vertical step range at `+0x688`/`+0x68c`:
`min = -max(c84 + c8c, c78 + c80)` and `max = max(0, 2 * c_a4)`, with both
capsule extents maxed first. `ow_tank::character_collision::step_limits_from_mover`
is that construction, `ow_tank::entity::mover_step_limits_of` reads it per hero
(36 of 54 at `2.5 / 1.0`, the large ones at `2.8 / 1.0`), and `ow-client`'s
floor probe now uses it instead of an invented `1.5` m. The second capsule
triple (`0x84`/`0x88`/`0x8c`) is crouch-sized (`1.3 / 0.5 / 0.25`) but which
state selects it is still unread.

**The capsule is now swept (2026-11-11).** `ow-client`'s walk no longer calls
the sphere form. `ow_sim::walk::Capsule` carries the shape, `advance_capsule`
and `snap_to_floor_capsule` are the capsule's two queries (the same fold, the
same depenetration, the same slide - only the shape in the query differs), and
the walker's position is the capsule's **origin**, at its feet. A picked hero's
own capsule comes from `ow_tank::entity::mover_capsule_of`; a launch with no
pick uses the install-wide common values (`extent 2.0`, `radius 0.5`,
`offset 0.5` -> `r = 0.5`, `d = 0.5`, height `1.5`). Five units in `ow-sim`
pin the difference, and `crates/ow-client/tests/walk_real.rs` pins the walk's
numbers on the install.

**What is still open.** The client's character-vs-world pass `0x142cf3f80`
(the Domino solver) is read on the **shape** side and on the **resolve
loop**: it iterates the entry's paired shapes (`0x142cd4d90`, the pair
getter `0x142cbb260`), collects the contacts, and resolves the position with
`0x142cd5270` until the **squared** correction is below `eps = 0.005` (the
bound is the sweep block's `+0x1c`, which the advance fills from
`step_state[0x94]`), with the resolved position at `body+0x620` and the
iteration count at `body+0x664`. **The client's resolve has no slide**: it puts
the body where it was asked to go and lets the Domino runtime push it out.

What is not ported is the runtime's own depenetration (`0x142cd4610` and the
contact collection behind it) and the pair build's world structures (the
fixture list, the handle encoding, the body records). So the walk's `8 m`
grid, its slide and its floor stage remain declared deviations - and the
slide is now known to be the *wrong* rule, not just a different one. That is
the next narrowphase-side anchor.

---

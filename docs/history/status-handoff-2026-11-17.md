# STATUS-HANDOFF archive - rotated 2026-11-17

Two blocks moved verbatim out of `STATUS-handoff.md` when the page was cut back
to the live state:

- the older half of "Current state" - the collision, brush and chunk narrative,
  which `docs/movement-plan.md` and `docs/formats/` now carry as tables and pages;
- "Exact today", the per-unit evidence ledger (test counts and coverage per
  differential unit).

Live replacements: `docs/systems-coverage.md` is the per-system state, and
`docs/open-questions.md` is the queue. This file is cold storage - its numbers are
as of 2026-11-17 and are not updated.

The prose is verbatim. The one change is mechanical: a relative link inside the
moved text was re-pointed one directory up, because the file now sits in
`docs/history/`.

## Current state (older half, to 2026-11-17)

The **collision answer** is read end to end: the fan's candidate evaluator
`0x1420328c0`, the handle dispatcher `0x142cc45a0`, the query driver
`0x142cbd510`, the narrowphase `0x142cc1610` and the hit policy `0x14202bc90`
are read (the policy's `ADD`/`ROR` chain is a pointer decode, not a jump) and
implemented (`ow-sim::policy`, `ow-sim::fan`, `ow_sim::collide`). What is
missing is **data, not code** — and the two producers are both blocked:

* the map's cell chunk (`0x0DD0001200000688`, 8.79 MB, 220 cells) is loaded and
  pointer-fixed, but a 28-function census of every shape producer finds **no
  reader that can see its payload**; the remaining Domino-side consumers are
  unreachable by every address spelling checked;
The **fan's mesh-face refinement** is read too, and it is the one place where a
candidate direction is actually tested against geometry: `FUN_140cc2290`
(one caller, `MoverWishWriter` `0x140cb86c0`) is a sphere-vs-mesh
nearest-triangle query with the radius as `param_3`, an initial acceptance of
`3r²`, a box-BVH gather `0x142caea70` whose accept is a box-vs-triangle SAT
(`0x142cd58a0`), and Ericson's closest-point `0x142cc5bc0` as the distance.
Two signatures there were false: the refinement returns a `bool` and
`0x142cc5bc0` returns its point in `XMM0`, both printed as `void`. The
remaining reads are the triangle record `0x142cae760`, the point transform
`0x142cb4a30` and the handle accessors; the geometry is in
`docs/formats/character-controller.md`.

The overlap test `0x142cd58a0` now has a lane-for-lane port
(`ow_sim::collide::box_triangle_overlap`) and it is **in the gate** as
`fn/BoxTriangleOverlap`: 413 fixture cases on the whole return register, 100%
of the collected branch sites, 12/12 mutants killed. The three divergences
that held it back were three transcription errors in the edge blocks (the
third edge's cross terms, and the second and third edges' support term), all
found by the differential and then read back out of the instruction stream.

The **gather that feeds it** is verified too: `0x142caea70` is `fn/MeshGather`
(`ow_sim::collide::mesh_gather`), a box-BVH walk that compares the caller's
container (capacity, count and the inline id array) byte for byte over 330
cases, 90% of the collected branch sites with two dead edges declared and
28/28 mutants killed. Its `.pdata` entry is truncated at `0x65` bytes while the
body is `0x299`, so the unit declares the true extent. Nothing calls either
port yet: the refinement `0x140cc2290` around them is read but not implemented,
which is what keeps the mesh-face branch of the wish solver from running.

* the brush route (node type 2 → model collision descriptor → brush → mesh) is
  read and implemented end to end — `ow_tank::brush`, `brush_mesh` and
  `brush_convert` (25 tests, client-emulated meshes pinned). **All four**
  fixture meshes now reproduce bit for bit; the arrow's divergence was two
  wrong link reads (ops 2/5 use `+0x28`, not `+0x30`), found by single-stepping
  the client. `C` is **named by type identity**: registered class `0x564B1018`, size
  `0x198`, vtable `0x1431dd118` (the client's own registration `0x1402db070`
  and field dispatch `0x141334ea0`/`0x141335e60`). `C+0x98` is a 16-byte array
  header over the inline sub-object's `Vec3A[]` and `C+0xa0` the encoding tag;
  `+0x48` is an asset reference (`teStructuredDataAssetRef<STU_564B1018>`).
  Implemented in `ow_tank::model_collision` (5 tests). **But no model carries
  it**: all 32,278 model assets scanned, zero hits. `C` lives in entity assets
  (GUID type `0x112`, `FRAC`+`FSTU`, a fracturable's collision), and the
  practice range's models carry none — so the brush route is the fracturable
  path, not the map's static collision. The static-collision question is open
  again with two leads: the range's 550 entity references were not checked
  against the seven `C` assets (checked: 0 hits in the range's 550 placeables),
  and `C+0x08` is a prebuilt collision mesh class (`STU_537E56E4`) that **is**
  in static models: 161 of the range's 229 models carry 272 instances through
  the holder `0x2DD96D6D` at `STUModel+0x118` (field `0xCB4D298D`). The missing
  read is the consumer of `STUModel+0x118`; the second geometry class
  (`0x142132f20`, chosen when `[node+0x29] != 0`) is **not** it (20 slots, no
  `+0x118`/`+0x140` read). The consumer is **`FUN_14210bac0`**, reading the list
  at **`+0x140`** — the client registers `STUModel` at `0x200` bytes, not the
  reference's 472 — and handing the mesh by **pointer** (`0x141b53af0`,
  `MeshShapeFill` -> `0x142caf6c0`, `ow_tank::mesh_shape`, 4 tests, no vertex
  conversion). Its caller is a broadphase (`FUN_1421088e0`, four buckets at
  `+0x1e8`, sphere cull) reached from `0x14212e070`, and it calls **no**
  shape-add — so the model mesh may be query-only, not the character's world.
  Classified: `0x142caf6c0` writes nothing persistent (inline stack buffer of
  `0x80` candidates, per-triangle `0x142cc6740`), and its owner
  `FUN_14212e070` is a scene-side **query** owner, not the Domino world. The
  character's own ingest `0x142cbcd40` has six callers, all in the mover/AI
  ranges, and `RunCharacterControllerSweep`'s nine `E8` sites likewise. The
  sweep's world was then read: it is the **Domino world** (variant tag at
  `[world+0]`, handle decode through `0x1442a0bc0`, `[world+0x2b8]` `0xc0`-entry
  and `[world+0x348]` `0x58` walks), with no vtable and no scene-owner call. So
  **no read path fills the character's Domino world with the range's static
  collision** — a **scope statement**: either the client receives that
  collision from outside the binary (server-authoritative movement), or an
  unread system inserts it. **That system is now found, and it is per model**
  (2026-11-02): the builder `0x14201ae60..0x14201c887` (a six-entry `.pdata`
  chain; the earlier `0x14201af49`/`0x14201b1cd` were fragment starts inside it)
  walks `[model+0x140]`, filters the holders, and emits one Domino shape per
  selected holder through `ShapeAddToBody`/`OwnerCreateBody`. It has four `E8`
  callers, all runtime state-sync functions of one collision component
  (`0x142022030`, `0x1420195e0`, `0x142014b50`, `0x142015f70`); the teardown is
  `0x14201e790`. Read and implemented in `ow_tank::model_body` (8 unit tests,
  install test `model_body_real`): for the range, 163 of 229 models carry a
  holder list, 279 holders, 272 mesh blocks, and **all 272 mesh holders become
  type-3 mesh shapes** (mask all ones, filter byte and shape mode at the class
  default 0). The owner id is **settled as a Domino world slot**: `0x14202f800`
  returns the `u16` at `[[node+0x620]+0x31f8]`, `OwnerCreateBody` decodes it
  through the same table/addends/XOR as the sweep (`0x1442a0bc0`, bound
  `0x3fff`), and the encoding's inverted constant makes the empty slot exactly
  `encode(0)` (verified: the addends sum to 0 mod 2^64; `ow_tank::domino_world`,
  7 tests). A node allocates **two worlds** (`0x14202c660`, `+0x31f8`/`+0x31fa`)
  and creates its own static body from `+0x31f8` at `0x14202d268`, so a per-model
  body is a sibling of the node's own body in the node's world.
  **The component is named:** constructor `0x14200a9a0` (size `0xad0`, primary
  vtable `0x142f7d608`, base interface `0x142f7d648` at `+0x20`, third vtable
  `0x142f7d458` at `+0x118`), registration `0x14200bab0`
  (`RegisterTypeWithSize`, kind `0x14`, name decodes to `Unknown`), type object
  vtable `0x1433633c8` whose slot 1 is the factory `0x14200a5f0`, and the
  pair-with-node `0x14200f840` -> `0x14201cf40(object, entity)`; the attach hooks
  take the node from `[entity+0x250]` and test **no asset-derived condition**.
  **The character's world slot is the node's slot:** the mover descriptor's
  `+0x46` gets `0x14202f800(node)` (the `u16` at `[node+0x620]+0x31f8`, same
  accessor the per-model builder uses), and `0x142cbcd40` builds the character's
  shape/body in that world (`0x142cf35a0` -> `0x142cc3f50`). **So per-model
  bodies and the character share the node's world.** **Open:** whether a placed
  node passes the node gate (`[node+0x454]` bit `0x200`, copied from the node
  creation record's `+0x10`; node `u16 +0x1c` bits `0x20`/`0x40`, model `+0x84`
  bit `0x80`) -- the record's producer is unread and is runtime, not asset, data.
  The descriptor's bounds/filter fields are documented but not converted.
  **The chunk's consumer is reachable after all** (2026-11-02): the assembler
  `0x1420a02f6` is a `.pdata` **fragment**; its chain root **`0x14209fc90`**
  (four entries to `0x1420a075d`) has one `E8` at **`0x1420251f5`** inside
  **`0x142024d30`**, called from the collision component's per-frame update slot
  (`0x142019944`, slot `0x142f7d688`) and from `0x14201fbe3`. The earlier
  "unreachable" verdict measured the fragment start; the RVA zero is a fragment
  artifact. **But it is not the chunk's consumer**: it reads the **model** at
  `[[node+0x110]+0x28]` (else `0x141b3d1c0(node)`) and walks `STUModel+0x140`
  with the mode filter `holder+0x2a == 1` -- the per-model builder uses the same
  list with `== 0`. Its `0x50`-byte context is scratch, allocated and freed
  around the call. On the range the plan is **empty**: 279 holders, **0** carry
  the mode byte (`ow_tank::model_shape_source` + install test
  `model_shape_source_real`; counterfactual mode 1 gives 272 mesh entries). So
  the chunk's own consumer remains unknown. The type-`0x00A` absence affects
  only the other brush source, not this route.

The chunk labels are **read now, and they were inverted**: kind `0x11` is
`MAP_CHUNK_COLLISION`, kind `0x12` is `MAP_CHUNK_PATHING` (the 220-cell
container), kind `0x10` additive collision, `0x23` additive pathing — from the
kind table `0x1439aa320` and the name decoder `0x1404c19b0`, and corroborated
by the per-slot dispatch. The collision chunk (`0x0DD0001100000688`, 21.8 MB)
is read and implemented (`ow-tank::chunkkind`, 7 tests: header counts 36/1,
array A `0x58`-stride, array B `0x38`-stride, the `FUN_142e492a0` fixup and the
per-sector tiling). **Its consumer is not the Domino shape pool**: a verified store census of the
world's pools (`formats/shape-ingest.md`) shows their contents can only be
created through the shape-add paths, and every caller that appends collision
material is a brush producer. The chunk route is therefore closed *for the
shape query* except for an inline appender no decoded store shows. `formats/chunk-kinds.md` has the
detail and the next anchors (`docs/open-questions.md` item 6, round 4).

## Exact today

- **CASC oracle:** eleven verified units, `python dev/verify_harness/cli.py casc`
  passes 10/10: `Salsa20Xor`, `Salsa20StateInit`, `Md5Transform`,
  `DynamicIndexHash`, `DynamicIndexKeyHash`, `EncodingHeaderParse`,
  `EncodingFindCKey`, `EncodingFindEKey`, `EncodingPageIndexSearch` and
  `CdnIndexFooterParse`. The oracle found and fixed
  the zero-length and trailing-twelve-byte index-hash defects.
- **Build-154504 manifest:** `CmfKey154504` (`0x142e48a20`) and
  `CmfIv154504` (`0x142e48700`) use the table at `0x1436100c0`, read from the
  installed `Overwatch.exe` at runtime. The encrypted manifest decrypts to
  1,301,598 GUID-sorted records. The two routines now have differential units
  (`fn/CmfKey154504`, `fn/CmfIv154504`): 364 synthetic cases each, every
  channel, all key and IV bytes; the coverage stage cannot measure the body
  (acknowledged gap, gate 0), so the row stays in `ow-tank-1to1.md` section B.
- **Bundle assets:** the CMF record byte's bit 7 selects the encoded store or
  a type-`0x09C` bundle; the entry lookup is read from `FUN_141fd5820`
  (`0x141fd5820`), implemented in `ow-tank::bundle`, and is a verified unit
  (`fn/BundleLookup`: 364 synthetic cases, 100% branch coverage, 12/12
  mutants). All 305 type-`0x09F` map headers open: 42 from the store, 263 from
  bundles.
- **Bundle directory:** the per-type directory **lookup** is read from
  `BundleDirectoryLookup` (`0x141fd5160`), implemented as
  `ow_tank::bundle::{Directory, directory_lookup, guid_type_key}` and verified
  by `fn/BundleDirectoryLookup` (364 synthetic cases, 93.3% coverage with one
  named guard gap). The directory's **producer** is still open; the install
  scan in `BundleIndex` remains our own.
- **Texture parsing:** the format switch (`0x142e372b0`), descriptor builder
  (`0x142e2d900`) and descriptor size walk (`0x142e2dbb0` over `0x142e37070`)
  are verified units (`fn/TextureFormatFamilyBpp` 374 cases, 12/12;
  `fn/TextureDescriptor` 364 cases, 12/12;
  `fn/TextureDescriptorSizeWalk` 364 cases, 11/11 with one declared equivalent
  mask widening), all at 100% branch coverage. The format dispatch table is
  `0x142e373d0..0x142e37491`; the size walk's is `0x142e37164..0x142e37189`.
  The descriptor unit compares the descriptor/flags prefix; bookkeeping past
  `+0x10` is outside it. BC6H, R16G16B16A16_FLOAT and R16_FLOAT decode to
  exact half words and upload as `Rgba16Float` (a documented viewer
  substitution for the client's DXGI 95/96/10/54). R16_UNORM keeps its exact
  `u16` words and uploads as `R16Unorm` when the device has
  `TEXTURE_FORMAT_16BIT_NORM`; without it the viewer keeps the clamped 8-bit
  view. The value, gate and staged bytes are tested, but a live GPU
  upload/readback is not. UINT, the BC CPU decode and the sampler remain open.
- **Map resolver:** `ow-tank::mapresolve` follows a header's paired mode and
  variation arrays, the variation's type-`0x002` scene map, the map asset's
  stored chunk references and the 31 synthesised chunks. The practice range
  requests and opens 34/34 chunks: 1,358 placeables (229 model-group
  references, 550 entity references), bounds pinned bit-exact. The mode
  entry's `m_A9253C68` is read as a `STUIdentifier` (30 identifiers across the
  install), and the replay header's map GUID is proven to be a base map, not a
  scene map.
- **Client:** 229 distinct models, 2,449 model-group placements, 872 draws.
  Materials report 289 looks, 594 entries, 120 decoded textures, 817 textured
  submesh draws, 55 placeholders, 148 flat fills and 3 no-sRGB entries.
- **Movement leaves:** twenty-seven differential units now pin the read mover
  arithmetic: `fn/MoverRampVector` (`0x140ca75b0`, 264 cases, 100% branch
  coverage), `fn/MoverRampScalar` (`0x140ca75f0`),
  `fn/MoverRampScalarOffset` (`0x140ca7570`),
  `fn/BuildMoverPhysicsCommand` (`0x140cb74a0`),
  `fn/MoverWishSpring` (`0x140cc0dc0`, 264 cases, 90% branch coverage, 12/12
  mutants killed), `fn/MoverWishForward` (`0x140cb5510`, 264 cases, 100%
  branch coverage, 5/5 mutants), `fn/MoverWishInverse` (`0x140cb5350`,
  264 cases, 100% branch coverage, 5/5 applicable mutants),
  `fn/MoverWishCopy` (`0x140ca7700`, 264 cases, 100% coverage, 5/5 mutants),
  `fn/StateTransform` (`0x140cb83b0` + the `FUN_140c71e10` leaf, 264 cases,
  11/12 mutants, 20% branch coverage with the mismatch path declared) and
  `fn/MoverStateVector` (`0x140ca5c50`, 264 cases, 100% coverage, 5/5
  mutants), `fn/DefaultMoverSettings` (`0x1405a87d0`, 264 cases, 4/4
  mutants), `fn/ApplyMoverSettings` (`0x140c862e0`, 264 cases, 11/12
  mutants, 33% branch coverage with the vector-growth and `+0x298` edges
  declared), `fn/ApplyStateModifiers` (`0x140cbcbe0`, 336 cases, 12/12
  mutants, 76.5% branch coverage), `fn/MoverGate` (`0x140ca7d80`, 126
  cases, 7/8 mutants, 90.9% branch coverage), `fn/CharacterPosition`
  (`0x142cd4ff0`, 264 cases, 100% branch coverage, 4/5 mutants),
  `fn/RigidPosition` (`0x142cb4770`, 264 cases, 100% branch coverage, 4/5
  mutants), `fn/VariantPosition` (`0x142cf5630`, 264 cases, 100% branch
  coverage, 4/4 mutants), `fn/WorldPosition` (`0x142cbcec0`, 264 cases,
  100% of the wrapper branch, 1/1 mutant), `fn/MoveCharacterController`
  (`0x142cd4bc0`, 36 cases, 100% of the no-shape branches, 10/10 mutants),
  `fn/ApplyCharacterControllerMove` (`0x142cd5370`, 19 cases, 100% branch
  coverage, 8/10 mutants) and `fn/SetCharacterControllerVelocity`
  (`0x142cbd1b0`, 12 cases, 100% of the wrapper branch, 7/7 mutants), and the
  four shape-bound leaves `fn/ShapeSphere` (`0x142ccd410`),
  `fn/ShapeCapsule` (`0x142ccd380`), `fn/ShapeBox` (`0x142ccf460`),
  `fn/ShapePoints` (`0x142cd3720`) and `fn/ShapeDispatch` (`0x142cb9030`,
  4 cases, 75% branch coverage with the type-3 edge open, 10/10 mutants);
  `fn/MoveCharacterControllerShape` (`0x142cd4bc0` shape path, 9 cases,
  83.3% branch coverage with the contact-event edge open, 8/10 mutants)
  proves the quaternion matrix, shape-list walk and bounds copy.
  `fn/CollisionSweepBounds` (`0x142cd5540`, 328 cases, 100% of the 3
  reachable branch sites, 11/11 mutants) proves the collision sweep's
  start/end/expansion stage: the quaternion rotation, the support-point
  transform and fold, and the centre/half-extent split.
  `fn/CollisionSphere` (`0x142ccdf40`, 328 cases, 88.9% of the reachable
  branch sites with the support-point path declared, 12/12 mutants) proves
  the type-0 narrowphase's analytic path: the inside test, the local-frame
  segment fold, the ray/sphere solve, the hit point and normal, and the
  fraction clamp. `fn/CollisionShapeFilter` (`0x142cbb350`, 328 cases, 100%
  branch coverage, 12/12 mutants) proves the query's layer filter, comparing
  the whole return register because the leaf leaves the second mask's high
  byte in `EAX`. `fn/CollisionCapsule` (`0x142ccd7b0`, 328 cases, 94.4% of the
  reachable branch sites with the support-point path declared, 12/12
  mutants) proves the type-1 capsule: the line-against-segment closest
  approach, both endpoint clamps, the inside report and the three sphere-leaf
  recursions. `fn/MeshSweep` (`0x142caed30`, 328 cases, 61.1% of the reachable
  branch sites with the support-point path declared, 12/12 mutants) proves the
  **type-3 mesh** narrowphase: the BVH walk, both child pushes, the
  AABB/swept-hull tests, the triangle mask, the per-triangle test
  (`0x142cce430`'s fast path), the `0xFF` index-run grouping with its
  single-corner carry, the last-accepted-triangle rule, the published
  0x44-byte hit and both walk counters (`collide::mesh_sweep`). The chunk
  cells become type-3 meshes, so this was the last unread query primitive the
  practice range needs (`crates/ow-sim/src/collide.rs`).
  `fn/ClosestPointGjk` (`0x142cc9460`, 264 cases, 74.5% of the swept branch
  sites with 12 declared, 12/12 mutants) proves the closest-point solver that
  `MeshSweep` and the type-0/1 leaves hand support-point queries to; its
  four-vertex body `0x142cc7740` is executed by the unit's tetrahedron cases.
  The wish spring proves the
  input copy, the `+0x10fc`/`+0x1100`
  spring, both state-id branches and the 1/1024 quantiser; the forward and
  inverse units prove the 4x4 column-major transform with the interior
  `RBX`/`RSP` harness extension (`det = 39/256` in the inverse fixture, so the
  `rcpps` path is exercised); the copy and state-vector units run the wrappers
  against a synthetic state record; the state unit implements the gate, the
  bitmap/rank/table index, the entry decode and the matrix copy in
  `ow-sim::state`; the settings units implement the default constructor and
  the asset apply in `ow-sim::settings`; the modifier unit implements the
  37-kind evaluator in `ow-sim::modifiers`; the gate unit implements the
  `+0x690` record predicate in `ow-sim::gate`; the world units implement the
  slot/variant dispatch, both handle-position leaves, the no-shape controller
  move and the character velocity wrapper in `ow-sim::world`; the shape-bound
  leaves and the type-0/1/2/4 dispatcher are in `ow-sim::shapes`. The
  shape-query loop is now verified on the contact-id `-1` path; the contact
  event (`0x142cf32d0`), the type-3 mesh path and the rigid velocity path are
  open. They do not prove the record's
  parent/pose paths, the settings vector
  growth path, the fan writer or Domino. `ow-sim` is a workspace member, so
  the gate runs its tests. The position units use the harness's new `vec4`
  return channel (XMM0 as 128 bits, lanes compared exactly).
  The modifier differential also corrected the shared SSE NaN helpers:
  `minss`/`maxss` return the source on any NaN, and `mulss`/`addss` prefer a
  quiet first source over a quiet second and over a signaling NaN.
- **Config path (2026-11-01):** the ramp/spring fields are read at the value
  level. `0x1405a87d0` sets the defaults (`+0x5a8/+0x5ac/+0x5b4 = 1.0`,
  `+0x5c0 = 0.0`, `+0x5c4..+0x5c6 = 1`, `+0x5d0 = -2`, `+0x5d4 = 1.0`,
  `+0x5dc = 0x10000`) and is verified; `0x140c862e0` applies a movement asset
  to the same object and is verified as `fn/ApplyMoverSettings`. Its
  asset-field map is in `formats/character-controller.md`.
  `+0x688`/`+0x68c` come from
  `ConstructMoverBody` (`0x140ca9730`) and `+0x10ec` from `FUN_140cc079d`
  (`0x140cc079d`). Still open: the asset producer and the point where the
  live body receives its copy of the block. The Domino controller is a
  shape/broadphase layer; the integrator is `MoverAdvance`, and `+0x1ad8` is
  a generation-checked handle, not a pointer. Collision reads precomputed
  triangle records; the map-chunk ingest is unread.
- **Movement caller chain (2026-11-01):** the live per-frame update is
  `MoverUpdateFrame` (`0x140cbb260`): `ApplyStateModifiers` ->
  `SubmitMoverPhysicsCommand` -> input build (`FUN_140cba030`) -> wish spring
  (`FUN_140cc0dc0`) -> state vector (`FUN_140ca5c50`) -> ramp
  (`FUN_140ca75b0`) -> `SetCharacterControllerVelocity`. `MoverStateMachine`
  is a separate record handler and stops at the submit. `SubmitMoverPhysicsCommand`
  does **not** call the controller; `FUN_1420163b0` is its dispatch.
  `FUN_140cba030` is an interior `.pdata` chunk, not a real entry. The wish
  spring is verified with a seeded service object (`body+8` encodes
  `T = 0x100000000`, state descriptor at `T+0x28`).
- **Per-frame velocity law (2026-11-01):** both live paths hand
  `state_transform(wish) + ramp_vector(body)` to the controller —
  `MoverUpdateFrame` at `0x140cbbdc2..0x140cbbe0b` (`addps state + ramp`) and
  `MoverUpdateCommand` at `0x140cb5890..0x140cb58b4` (`addps ramp + state`,
  gated on `input+0x4c != 0`). Both pass `body+0x10d0` as the ramp's third
  argument, so the ramp reads its flags at `body+0x10d0` and its offset at
  `body+0x10fc`. The composition is implemented as `ow-sim::frame`; its
  operands are the verified `fn/MoverStateVector` and `fn/MoverRampVector`,
  and the composition itself has no client unit. `MoverAdvance`'s sweep block
  and contact pass are read and written up in
  `docs/formats/character-controller.md` (desired
  `(scale_a*dir + wish)*scale_b + pos`; `[world+0x200]` contacts, stride
  `0x20`, count `[world+0x20c]`; the `>= 4` branch and the `+0x40` response
  slot are open).
- **Wish source (2026-11-01):** the first-person view object (vtable
  `0x142f87810`, slot `+0x250` = `0x140654530`) writes the actor command at
  `+0x1210` (yaw `+0x04`, pitch `+0x06`, buttons `+0x0c`, flags `+0x10`) and
  the ring at `+0xe30`; `FUN_140689930` is probably driven from
  `[actor+0x38]+0x178`; `FUN_14068cf10` -> `FUN_140cbb260` copies the command
  to `body+0x0d0/+0xe0`. The state-record path reaches
  `FUN_140cb63c0`/`FUN_140cb86c0`, which writes source `+0x00`/`+0x10`/`+0x40`.
  The packer is now read exactly: `0x14068cf10`'s tail copies the current
  0x14-byte record and overlays the **next** record's yaw (`+0x1228`) and
  pitch (`+0x122a`) onto `+0x04`/`+0x06`, passing `[actor+0xe14] + 1` as the
  sequence (`ow_sim::frame::pack_command`/`command_sequence`). And
  `FUN_140cb63c0` is read end to end: the state id comes from
  `[state_object+0xbc]` (default `1`), `body[0x10d0] & 0x800000` returns
  before the fan, the fan gets `dt = 0` and stack flag `1`, and the service
  object's `+0x58/+0x60/+0x68` hooks run under the `0x80000c` mask. That
  fixes the shape of the remaining gap: the fan's input direction is
  `body+0x1140`, written by the spring from the previous frame's own wish, so
  **no stage converts the buttons into a direction** - the wish is a closed
  loop whose only seed is a collision-scored candidate. That route is now
  **closed by enumeration** (2026-11-01): the six stores to `body+0x1140` are
  the spring, the two transform wrappers, `0x140cbf950` (shared body of both
  update paths, storing `0x140ca7650`'s output, i.e. a *world* vector run back
  through the inverse transform), and two ability/animation writers. Combined
  with the fan being the sole writer of `source+0x00`, the client cannot wish
  without collision. The chunk's container and runtime fixup are now
  implemented and verified; next is the cell-to-shape decoder
  (`docs/formats/maps.md`).

- **Collision world structure (2026-11-01, all read):** a world is a
  **0x4d0-byte** header plus eight strided pools. `0x142cc3c80` is the only
  creator: it finds a free `u16` handle in `0x1442a0bc0`, allocates the header,
  calls `DmWorldConstruct` (`0x142cbeb70`), encodes the pointer
  (`^ 0x77600ba5adcc265c`, `+ 0x4cd964772bb2a04e`) into the table and writes
  `record[0x194] = handle`; `0x142cc3d90` decodes. The pool table is
  `record+0x288 + 0x18*i` = `{ptr, count, capacity, x, free_head}` with element
  strides `0x1c0` (bodies), `0x50` (transforms), `0xc0` (fixtures), then
  `0x20`, `0x30`, `0x50`, `0x50`, `0x50` (shape types 0..4+). The fixture's
  `+0xa0` dword is the shape ids when live and the **free-list next** when
  free; release is `0x142cb0d08`/`0x142cd4555`, update/insert is
  `0x142cd4610` with commit helpers `0x142cb3330`/`0x142cb3245`. The geometry
  object (`0x14202c660`) creates **two** worlds from one config (handles at
  `+0x31f8`/`+0x31fa`, decoded pointer `+0x31fc`) and installs an interface at
  `+0x31e8` via `0x142cc4af0`, which sets `record+0x168` only while the world
  is empty. **The fill side is read (2026-11-01, corrected):** `0x141d71200`
  is the class *constructor* (it zero-fills ~0x45a0 bytes and stores vtable
  `0x14331c570`); the pool writes it appeared to make are zeros. The real
  write path is `ShapePoolAlloc` (`0x142cb00c0`, the free-list pop, taking the
  *pool record's* address), `ShapeEntryFromDescriptor` (`0x142cb7f10`, the
  0xc0-entry writer with the per-type dispatch on `source+0x67`),
  `ShapeAddToBody` (`0x142cb3940`) and `ShapeAddToOwner` (`0x142cd4ef0`).
  `DmWorldConstruct` sizes every pool from the world config: `+0x20` bodies,
  `+0x24` owners, `+0x28` shape entries, `+0x2c..+0x3c` the five types,
  `+0x40..+0x6c` the auxiliary pools, `+0x70` islands, with the free chain
  written at create. The chunk container and its 17-pointer fixup are now
  implemented and verified (below). **Still open: the descriptor producer** —
  who turns the chunk's decoded cells into the `source` records
  `ShapeAddToBody` consumes.
  The wish is rotated through `FUN_140ca5b90`/`FUN_140cb5350`/`FUN_140cb83b0`/
  `FUN_140c71e10`, not copied from the command. The transform is a 4x4
  column-major affine: forward `v' = x*c0 + y*c1 + z*c2 + c3`
  (`FUN_140cb5510`), inverse `v' = R^-1*(v-c3)` with `det = c0·(c1×c2)` and
  `rcpps` + Newton (`FUN_140cb5350`). `FUN_140cb86c0` is a fan solver over
  `body[0x5bc]` directions (`2π/N`), `0x1420328c0` composition and a
  back-transform; the per-iteration algebra is unread. The command byte
  meanings, `actor+0x38` vtable and `+0x58/+0x60/+0x68` virtuals remain open;
  the `0x14068f1aa` variant is a dead duplicate.
- **Collision chunk payload (2026-11-01, data read):** the practice range's
  kind-`0x12` chunk `0x0DD0001200000688` is 8,792,256 bytes: a container with
  two STUD v1 blobs and a trailing matrix. The first blob's single component
  is class `0x9199BF6B`, registered at `0x1400fdcb0` with size `0x78`, ctor
  `0x140f91550` and four fields whose RVAs point to a hash table (`+0x98`), a
  per-sector table (`+0x2358`, `{u32 7, quad bounds}` records) and a
  `0x2428` table holding a count `0xDC` (220) and an ascending block-RVA
  list. The second blob's component `0x99F089FE` has no registration in the
  image (skipped). `FinishMapChunkAsset` (`0x141ff54e0`) resolves the header
  RVAs in place through `0x142e49360`/`0x142e49270`/`0x142e492a0` and loads
  the blob with `FUN_1421d26d0`, then runs the 17-pointer fixup
  `FUN_141ff51a0`. The per-sector blocks are not decoded and the component's
  consumer is still open. **Those zeros are BSS, not damage:** the install's
  `.data` raw size ends at RVA `0x3b79600`, so the descriptor's field table
  (`0x143c3c620`) and type-info buffer (`0x143c3efc0`) are in the zero-fill
  tail — runtime-only pages the file does not contain (`dev/pe_layout.py`,
  now a gate step). The class name decodes (XOR with the 8 bytes before a
  `[len|flag]` header) to `Unknown`; the client carries no readable names.
  **Real names come from the reference registry by hash:** `STU_9199BF6B`,
  size `0x78`, fields at `+0x08`/`+0x30`/`+0x58`/`+0x68`, with
  `m_cellBytes` at `+0x68` — every offset matches the payload, so field 4 is
  the 220 per-cell payloads and field 3 the bounds/cell table that goes with
  them (record layout not decoded yet). **The world's creation
  path is mapped:**
  `0x14202c660` is the owner's initialise (`owner+0x620` = the 0x3210-byte
  geometry, two worlds from `config[0x18]`), and its class is reached through
  the node-type registry: `0x1404f6e10` registers `0x142029f00` as node type
  2, which builds a type descriptor (vtable `0x1433654a0`) whose slot 2
  (`0x1407803f0`) is a **release**, not an instantiate (check 17: the allocator
  is slot 1, the strategy's `0x50`, and the "key" is the instance handle). The
  next read is the poller of `service+0x78`: the all-preloads-done callback
  (`0x141fe0f80`, service vtable `0x142f8d970` slot `+0x38`) only sets the
  loaded byte; `0x140554220` is the engine subsystem constructor, not the map
  load.
  **The STU load path is read (2026-11-01):** `FUN_1421d26d0` resolves each
  sub-record by class hash (`FUN_1421c14f0`), constructs it in place and runs
  `FUN_1421d3c10`/`FUN_1421d3710`; that resolver converts self-relative
  offsets by the **field type** (plain offset, array of offsets, array of
  pointers, nested instance, hash map, string). With the shipping field
  tables empty, nothing converts generically — which is why the chunk's
  completion calls `FUN_141ff51a0`, **17 self-relative qwords per
  `m_cellBytes` record**. `MeshShapeFill` (`0x142cae540`) and
  `ShapeFillType2` (`0x142cd31f0`) give the type-3 and type-2 source layouts
  (`docs/formats/domino.md`).
- **Collision cells decoded (2026-11-01):** the chunk's `m_cellBytes` field
  points at `{u32 count=220; u32 pad; u32 data_off}`; the entries are
  `{cell_off, 0}`; a cell is `{block_size, record_off}`; a record is 17
  self-relative qwords plus a 24-byte header, and its 17 arrays tile
  `record..record+block_size`. All 220 validate and tile the payload with no
  gaps (`docs/formats/maps.md`, "The cell container, decoded"). The runtime
  array encoding was observed under the oracle (`dev/probe_cellbytes.py`):
  mode 1 `{entries_ptr, count}`, entries `{Q,1}`, record `*Q`; mode 2
  `{count, pad, entries_ptr}`, record `*Q` or `*(Q+8)`. The consumer chain is
  `GeometryFinalize` (`0x14213d0b0`) -> `OwnerCreateBody` (`0x142cc3d90`) ->
  `ShapeAddToBody`, one body per 0x100-byte runtime cell entry.
- **Collision container and fixup (2026-11-02):** the container stage is
  implemented as `ow_tank::collision::parse` and checked on the install
  (`crates/ow-tank/tests/collision_cells_real.rs`): the practice range's
  `0x0DD0001200000688` yields 220 cells, the first record `0x3208` with block
  `0x67F8` and boundaries `0xA0..0x65F8`, the last cell ending at `0x862700`,
  and every array between adjacent boundaries. The **runtime 17-pointer
  fixup** is the read expression of `MapChunkCellBytesFixup`
  (`0x141ff51a0`) in `ow_tank::collision::fixup_cell_bytes` and is verified by
  `fn/MapChunkCellBytesFixup` (264 iterations + edge pool, 12/12 mutants);
  outer modes 1 and 2/3 and entry modes 1 and 2/3 are covered, the two
  allocate-then-deref-null mode-0 branches are declared dead. The `STUD` magic
  is the u32 `0x53545544` the loader compares (`StuLoadSubAssets`), stored as
  the bytes `DUTS`. **Still open: the cell decoder** (who writes
  `geometry+0x108`/`+0x114` from the 17 arrays, or fills the 0x100 sector
  entries), so the container is not yet a collider. The candidates and the
  dead ends tried on 2026-11-02 are listed in `docs/open-questions.md` item 6,
  "Search map for the decoder" — narrowed further on 2026-11-02: every
  `MeshShapeFill` caller but the generic `ShapeEntryFromDescriptor` route lives
  in code Ghidra never defined, and `GeometryFinalize`/`OwnerCreateBody` have no
  code references at all, so the decoder has to be read as raw disassembly from
  those three call sites rather than reached by walking callers.
- **The cell reader is OPEN, and the first census was wrong (2026-11-02,
  corrected).** The earlier wording here ("no reader in this image", "open by
  absence") is **withdrawn**: it rested on Ghidra's caller metadata, which this
  project's notes already call incomplete (chained/fragmented `.pdata`,
  undefined regions). Ghidra reported 3 callers for `ShapeAddToBody`
  (`0x142cb3940`); a verified `E8 rel32` scan finds **27** sites in 13
  functions. The claim that only `0x14208d010`/`0x142090e00` read
  `+0x10`/`+0x58`/`+0x68` and call both builders was also wrong — they call
  neither; the functions holding those sites are `0x14208d109` and
  `0x1420919b7`, and their `+0x58`/`+0x68` operands are `f32` radii of the
  mover settings. What stands is a corrected census: for `ShapeAddToBody`
  (27 sites), `OwnerCreateBody` (17), `ShapeAddToOwner` (1),
  `ShapeEntryFromDescriptor` (2) and `MeshShapeFill` (5) the union of Ghidra's
  callers and the verified scan is 28 producing functions, and **none calls
  `StuLoadSubAssets`, none reads the chunk work record's payload at `+0x60`,
  and none reads the component's `{pointer, mode}` headers**; the only qword
  read at `+0x68` is `ShapeEntryFromDescriptor`'s own domino descriptor. The
  whole-copy route is empty too: one `memcpy(…, 0x78)` in the whole image
  (`0x142402824` in `FUN_142401de0`, copying a diagnostic string fragment)
  among 8 923 `memcpy`/`memmove` call sites. The container work stands and is
  unchanged: the three array fields tile the blob with no gap (`0x98` 222 x
  `0x28`; `0x2358` 6 x `0x20`; `0x2428` 220 x `0x10` ending at the first cell
  `0x31F8`), strides by exact division — `ow_tank::collision::tables`, two unit
  tests and `collision_cells_real::the_collision_component_holds_three_tiled_arrays`.
  Details and the per-function table: `docs/formats/maps.md`, "The cell path's
  consumer: open".
- **The three follow-up producers are Domino/brush-side and unreachable
  (2026-11-02).** `0x1420c63b0` consumes a `0x38`-stride node/transform record
  array at `[owner+0xd0]` with a `0x20`-stride transform-handle array at
  `[owner+0xe0]`, writes its mesh into the caller's records and ends in one
  `OwnerCreateBody` + four `ShapeAddToBody`; `0x140c2813f` builds its mesh from
  a **brush collider** (`ColliderMeshEmit` `0x142ccc120`); `0x142078562`
  computes a box from a **stored mesh source's bounds** (`0x142cae600`) into a
  `0x58`-stride record; `0x1420a02f6` is a **mesh-source assembler** over the
  owner's descriptor container at `+0x140`/`+0x148` (mode `+0x20`, `0x60`- and
  `0x40`-stride arrays, pair unpack `0x141b53b70`/`0x141b53af0`); and
  `0x142cdf9f2` appends `0x40`-byte entries to the **Domino owner's body
  vector** (`OwnerCreateBody(word[owner+0x58], ...)` + `0x142cb4f20`). None
  reads a chunk payload or a `0x9199BF6B` field, and none has an invocation
  route: no `E8` (except `0x1420c63b0`'s own recursion), no `E9`, no immediate,
  no 8-byte pointer, no obfuscated-pointer encoding. Their only references are
  the image's `.pdata`-copy and code-range tables, which every function start
  carries (`ShapeAddToBody` has one beside its 27 call sites). So the chunk path
  is **neither opened nor closed**; the single next reader is the producer of
  the owner's **mesh-descriptor container at `+0x140`/`+0x148`**. Write-up:
  `docs/formats/maps.md`, "The three next readers: Domino-side, and
  unreachable".
- **The geometry object side is now read end to end (2026-11-02).** The writer
  of the 0x60 cell entries and the 0x100 sector entries is the driver
  `0x14213bbc0` (vtable slot `+0x78`), which walks pool A (`GEO+0xb0`, stride
  `0xb0`, count `+0xc0`) and pool B (`GEO+0xc8`, count `+0xd8`), calling builder
  #1 `0x14213a510(GEO, index, desc, force, out)` at `0x14213bd61` and builder #2
  `0x14213acf0` at `0x14213beaf`. Pool records already hold decoded geometry:
  `+0x00` vertex array (0x20-byte elements), `+0x08` triangle array (0x28-byte
  elements), counts `+0x10/+0x14/+0x18`, a 0x30-byte mesh source at `+0x20`,
  position/extent at `+0x78/+0x84`, a u16 face list at `+0x90/+0x98`, and the
  record's own slot index at `+0xa0`. Builder #1 copies the arrays into a mesh
  record, converts with `BrushTreeToMesh` (`0x142cf23c0`), pushes a cell
  (header `vec4`s, mesh source via `MeshToColliderFaces` `0x142cf2010`, the
  `(GEO+0xa8, GEO+0xaa)` index pair at `[cell+0x30] + k*0x20 + 0x18`, face list
  via `0x142139080`) and then a sector (transform, distance, limit, accept-test
  outputs). The brush path (`0x14213c440`) reuses the same cell fill and calls
  `OwnerCreateBody` + `GeometryFinalize` itself. **Still open:** who fills the
  pools from the 17 fixed-up arrays — no code in the geometry class reads
  `chunk+0x2428`. Full detail in `dev/data/read/decoder-notes.md`.
- **Collider query leaves verified (2026-11-02).** `fn/MeshSweep` (the type-3
  BVH walk plus triangle test) and `fn/TriangleDegenerate` (the per-triangle
  skip test `0x142cd56e0` that `MeshToColliderFaces` applies) both pass the
  differential, coverage and mutation stages. Read but **not implemented**:
  `MeshToColliderFaces` (`0x142cf2010`), `ColliderMeshEmit` (`0x142ccc120`),
  `ColliderFaceAppend` (`0x142cca460`), `BrushTreeToMesh` (`0x142cf23c0`), the
  pool record creator (`0x1421363d1`). `MeshToColliderFaces` and
  `ColliderFaceAppend` are read in full except the weld's neighbour links (no
  reader found): the collider is a **double-sided shell** (`v +- eps`, two
  oppositely wound triangles per face, walls when `eps >= 0.005`) whose faces
  are welded across the two surfaces by a vertex-pair hash and packed into a
  BVH. The BVH builder `0x142ccb1b0` is now **read and verified**
  (`fn/BuildBvh`): leaf at `<= 4` faces; otherwise the split follows the
  centroid spread — array half when every centroid extent is below `0.005`,
  else a 64-bin `area*count` sweep along the widest axis with the earliest
  cheapest boundary, then an in-place **swap** partition (not a stable shift).
  Implemented in `crates/ow-sim/src/collider.rs` together with the shell, the
  face records and the flattening. Next: the flattening `0x142ccad80`,
  `ColliderMeshEmit` (`0x142ccc120`), then the cell decoder.
- **The collider build is read end to end (2026-11-02):** `GeometryBuild`
  (`0x14213c440`) is the **only** cell writer (`0x142136010`, one call site)
  and `GeometryFinalize` (`0x14213d0b0`) the only pool-A writer
  (`0x142136130`, one call site); both build from the node's brush tree, so a
  map's collider comes from **brush trees carried by models**, not from the
  collision chunk's 17 arrays (no reader found; the fixup only rebases them).
  `ColliderFaceAppend` (`0x142cca460`) is read including the half-edge
  twin pass: face records 0x60 hold box min/max/centre, `+0x30 = -1`, area at
  `+0x34`, `{v0, v1, v2, 0, 0, -1}` at `+0x38..+0x4c` (the last three are
  per-edge adjacency, filled by the twin pass), and three `u16` ids at
  `+0x50..+0x56`; the vertex array is `(x, y, z, 0)` with count `maxIndex+1`.
  `MeshToColliderFaces` was re-read with the decompiler: the thickening is
  **z only** (x/y/w pass `+-0.0`) and the index array is outward block, inward
  block, then the stitching pairs. `crates/ow-sim/src/collider.rs::shell` was
  corrected to that read and the half-edge adjacency is still **not
  implemented** (matters only if the response reads it). Next: the brush
  tree -> mesh data (`BrushTreeToMesh` input) and the ingest.
- **Geometry layout read (2026-11-01):** `GeometryClear` (`0x1421384e0`) fixes
  the arrays: `+0xf8`/`+0x104` 0x50-stride transforms, `+0x108`/`+0x114`
  **0x60-stride cell entries**, `+0x118`/`+0x124` 0x100-stride entries,
  `+0xb0`/`+0xc0` and `+0xc8`/`+0xd8` 0xb0 record pools. `GeometryFinalize`
  copies each 0x60 entry's five vec4s into a 0xb0 record, steals its
  `{ptr,count,flag}` u16 array at `+0x50/+0x58/+0x5c`, and creates one body
  plus one type-3 mesh shape per entry. Also fixed the analysis blocker:
  `Alloc` (`0x141baff30`) was marked noreturn in Ghidra, truncating every
  caller; prototype + `setNoReturn(false)` + `clear_flow_and_repair` restores
  the full bodies (`docs/ghidra-project.md`).
- **Node type 2's geometry is brush-built (2026-11-01):** `GeometryBuild`
  (`0x14213c440`) reads the node's model brush tree, copies the collision
  params (`0x142132cf0`), allocates one 0x60 cell entry, converts the brush
  tree (`0x142cf23c0`) and thickens it into a closed collider
  (`0x142cf2010`), then `OwnerCreateBody` + `GeometryFinalize`. The 220-cell
  STU chunk therefore feeds a **different** node type; the node registry
  `0x141c5ff40` has 100 registration thunks to enumerate.
- **Where the brush build writes (2026-11-02):** `MeshToColliderFaces`
  (`0x142cf2010`) is called with `entry+0x20` and fills the 0x60 entry's
  **0x30-byte type-3 mesh source** (`{u64, u64, u64, u32 count, 5 floats}`,
  the same layout `ShapeEntryFromDescriptor` copies); a loop then stamps the
  brush's two u16 ids (`node+0xa8`/`+0xaa`) into every 0x20-byte face at
  `+0x18`/`+0x1a`, `0x142139080` appends the entry's u16 array, and
  `GeometryFinalize` (`0x14213cb52` is its only call site) turns the entry into
  one body plus one mesh shape. So the brush path and the chunk path must build
  the **same** 0x30-byte source: the chunk decoder's job is to turn the
  quantized vertex arrays and the `0xFF`-separated index runs into it.
- **D3D11 renderer (D1-D5):** the production-path device and swap chain (D1),
  the back-buffer texture with its explicit RTV/SRV/UAV and the resize order
  (D2), the per-frame step order (D3), the 64-opcode replay dispatch (D4) and
  the read clear/draw/viewport handlers (D5) are implemented in
  `crates/ow-render`. D1 carries the read values: `11_1`-first six-level
  fallback, the DEBUG/SINGLETHREADED/PREVENT_ALTERING_LAYER expression,
  `UNKNOWN` driver type, null software module, SDK 7, and a
  `DXGI_SWAP_CHAIN_DESC1` of DXGI 28 / 3 buffers / usage `0x30` / swap effect 0.
  D2 pins the 64-entry view-format table `0x1434965d0`, the six special cases,
  the texture-2D descriptors and the usage gates (SRV when bit 0, UAV when
  bit 1); the default view creates an RTV and an SRV. D3 pins the order frame
  begin -> GPU wait -> scene prepare -> upload finish (flush + blocking wait) ->
  submit/replay -> fence wait -> per-view submit -> frame end -> present; only
  `Present` has a complete executed body, every other step is logged as open.
  A fourth shared slot, 132 (`0x1433585e8` → `FUN_141efa380`), advances the
  frame index and flushes the two batch buffers; its outer order is open.
  D4 implements the record header (`+0x10` opcode, `+0x14` size), the 64-entry
  target table `0x142671154`, the 64-opcode enum, the missing-opcode path and
  the executor. D5 implements `ClearRenderTargetView` (`0x07`),
  `ClearDepthStencilView` (`0x08`), `Draw`/`DrawInstanced` (`0x25`),
  `DrawIndexed`/`DrawIndexedInstanced` (`0x26`), `RSSetViewports` (`0x29`),
  `RSSetScissorRects` (`0x2a`) and `Dispatch` (`0x30`); `0x00` is end and
  `0x1d` is no-op. `0x04` is a Map/Unmap record, not a depth clear; the depth
  clear is `0x08` (`ClearDepthStencilView`, `+0x1a8`, view `+0x18`, flags
  `+0x20`, depth `+0x24`, stencil `+0x28`). The earlier "no depth clear"
  statement was wrong. `0x0c` is the render-target bind: it clears/releases
  each RTV entry, clears the DSV, and calls `OMSetRenderTargets(count, array,
  dsv)` at `0x14266e54d`; `0x0d` resolves MSAA entries and unbinds at
  `0x14266e6ba`. `0x0c` and `0x0d` are implemented in the executor and
  `ow-render::state`; the alias-unbind, the per-stage SRV/UAV cache ranges, the
  six SRV flushes, `apply_render_targets` and the pending resolve/release pass
  are modelled with synthetic tests. The `0x16..0x1b` cache population and the
  queued-SRV binder remain. The other 53 opcodes are open by opcode. The
  `0x0c..0x1b` batch is read: `0x0c`/`0x0d` are implemented; the other fourteen
  are open because they dereference an out-of-band state block or descriptor
  array, and the six constant-buffer / six shader-resource binds defer to the
  end-record flush.
  `FUN_142674980` is a per-draw UAV/CB/SRV state flush, not the bind; it is
  called from `0x25`/`0x26`/`0x27`/`0x28`. The engine ring is a separate
  stream: `0x387` is a batch A/B submission (A 16-byte CPU→GPU, B 8-byte
  GPU→CPU), not a draw, and there is no record-id -> replay-opcode
  translation; the ring consumer is open.
  `ow-client --renderer d3d11 --d3d11-frames N` drives the step list and prints
  the D2 views; the default stays the wgpu viewer. The smoke run created the
  device and views on this machine's RTX 4060 at `D3D_FEATURE_LEVEL_11_1`. A
  RenderDoc v1.46 capture of **our** D3D11 path shows the 1280x720
  `R8G8B8A8_UNORM` back buffer and exactly `ClearRenderTargetView` + `Present`,
  matching the executed D1-D3 steps; it is a self-capture, not the client.
  **A client capture is impossible under the anti-cheat**, so there is no
  call-trace or image oracle and the read stages are not capture-compared. The
  three flag inputs, the sync-interval byte and the adapter preference sort are
  runtime values whose shipped defaults are **open**; the pass list, outer
  frame-slot caller, D3D11 fence body, draw-list builder, cache population and
  material joins are open.
- **Engine batch records:** `0x387` (0x28 bytes), `0x60` (0x18), `0x30` (4),
  `0x1be` (4) and `0x352` (0x18) are read. `ow-render::batch` models the six
  `0x387` fields, the ignored padding dwords, the A/B offsets/capacities, the
  16-byte CPU→GPU upload and the 8-byte GPU→CPU readback with the `-2 -> -3`
  state filter. `ow-render::state` adds the alias predicate, cache ranges,
  flushes, `apply_render_targets` and the `0x0d` resolve pass.
  `cargo test -p ow-render` is 69/69. The engine-ring consumer and the item
  meanings remain open; there is no record-id -> replay-opcode translation.
- **Replay state caches:** the per-stage SRV/UAV cache layout, the 64-bit live
  bitmap, `TestViewAlias` (`0x14265fb90`), the six SRV flushes, the `0x0c`
  alias-unbind / final bind and the `0x0d` resolve pass are modelled in
  `ow-render::state` with synthetic tests. The queued-SRV binder, the
  `0x16..0x1b` population and the out-of-band RTV/DSV table remain open.
- **Other verified client functions:** `AnimTimeWrap`,
  `BuildPerspectiveMatrix`, `InitProjectionDescriptor`,
  `OrthonormalizeCameraBasis` (stateful), `MapChunkGuid` (`fn/ChunkGuid`,
  the first `ow-tank` tier-2 unit), `BundleEntryLookup`
  (`fn/BundleLookup`, the second `ow-tank` tier-2 unit: 100% coverage,
  12/12 mutants) and `BundleDirectoryLookup` (`fn/BundleDirectoryLookup`,
  the third: 93.3% coverage with one named guard gap, 35/35 mutants killed).
  The build-154504 key/IV units
  (`fn/CmfKey154504`, `fn/CmfIv154504`) pass differential and mutation but
  keep an acknowledged coverage gap. `CurveKeySampleDual` is inconclusive at
  75% coverage; the two `ow-sim` units cannot run while `ow-sim` is parked.
- **Ghidra ground truth:** the working project is
  `dev/ghidra_run/owmod_clean` (program `Overwatch.exe`), the clean `.pdata`
  map: 120,028 function symbols = 119,802 real `.pdata` + 226 no-`.pdata`
  (55 named leaves/loader + 171 analysis import/thunks), 0 chained-fragment
  starts, 0 inside-real starts, 6,676,292 instructions. All 331
  `function-names.csv` rows are present (326 functions + 5 labels).
  The pre-clean backup is `dev/ghidra_run/owmod` (236,519 symbols, including
  the `Function Start Search` artifacts). The two offcut name rows are removed;
  `cargo xtask frame-check` rejects new ones. The migration pipeline is
  [`docs/ghidra-migration.md`](../ghidra-migration.md).


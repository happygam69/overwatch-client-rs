# STATUS

The living state: what works, what is open, and what comes next.
`MODLOG.md` is the milestone history; `docs/formats/` holds findings and
addresses; `docs/open-questions.md` is the work queue.

## The gate

```sh
cargo xtask verify            # fmt, clippy, tests, tool index, deviation and address checks
cargo xtask verify --fast     # without the install-reading tests
```

`cargo xtask verify` is green on 2026-11-06, and the earlier failure of the
install-reading `cdn_index_real` is understood: the install holds a **staged
patch**. Its CDN config names a `265,946,028`-byte archive-group index written
2026-10-31, while `_retail_/Overwatch.exe` is still the 2026-10-20 build that
the pinned `260,173,908` byte index belongs to. The test now skips the pinned
geometry for any named index newer than the executable and prints why, so it
never pins a build that is not installed. When the agent activates that patch,
the install and the Ghidra image must be re-checked together **before** any pin
is updated.

The gate had in fact been **red** for a different reason: the workspace member
`crates/ow-engine` carried two fixture defects in `interface_map` (a free-list
hook that left the slot in its bucket chain, so `find` walked a cycle forever,
and a rehash test that expected a chain to survive a mask change that splits
it). Both are fixed; `cargo xtask verify --fast` and the full suite are green.
`verify --fast` now means "every library target" - the old three-crate list
left `ow-sim`, `ow-render`, `ow-scene`, `ow-client` and `ow-engine` unrun - and
CI runs the full gate on `windows-latest`, because `ow-render` is
`#![cfg(windows)]`. See `MODLOG.md` under 2026-11-06.

## Current state

**The engine layer has its first read crate.** `crates/ow-engine` models the
`0x470`-byte engine object at `[0x143b7f410]`: `WorldBaseCtor` `0x141c507c0`
and `WorldCtor` `0x1405838c0` field for field, the six value codecs with their
inverses and round-trip proofs, and `RegisterWorldInterface` `0x141c52d10`
with the array growth `0x140767da0` (the capacity word's bit 31 is a
"borrowed" tag). It also carries `ConstructEngineSubsystems`' schedule as a
per-row decode: the 65 slot stores of `+0x220..+0x448` in store order (source,
producer, allocation size, vtables, store and registration site) and the
21-entry tail attach list; `World` refuses the five offsets the function never
writes. On top of that, `ow_engine::classes` carries the **35 constructors and
their 96 vtable installs** read from each producer's own body: 70
constructor-installed tables + 47 block-stored tables (disjoint, 117), the
shared stub shown to be a base constructor rather than a class, and the
six-sub-object aggregation at `+0x2f0`. 23 unit tests plus 14 schedule, 12
layout and 8 class tests; `docs/formats/engine.md` is the page and
`dev/engine_classes.md` the dossier. Three fields are written twice
and each pair is "codec at zero, then the real value" — the arithmetic that
proves which constants are encoded nulls. Next engine
step is naming a class: the unit is a slot's own 15-entry table (44 of the 117
tables are exactly 15 entries), and a name needs a method in those entries that
calls a known subsystem API. The 20 registered constructions that never reach a
slot are still unread.

**The one client launches the real practice range.** `ow-client` and
`ow-scene` are workspace members. The default launch resolves a
`STUMapHeader` by `m_mapName` (`Maps\TrainingRoom\Training Room - Practice`),
takes the first paired variation whose `STUGameMode.displayName` is exactly
`Practice Range`, and loads the 34 chunks the client's map loader requests for
scene map `0x080000000f69002e`. No chunk seed, capture or synthetic course is
the default. This selection is a **viewer default**: the current image has no
client-side mode→variation selector (see "Open").

The client is a **viewer**. It makes no movement, combat or camera-behaviour
claim. `ow-sim` holds the verified movement, Domino body and camera-basis
arithmetic, the read per-frame velocity law (`ow-sim::frame`), the fan's
composition and the type-3 mesh narrowphase. No frame can be stepped yet, and
the reason is now singular: **the collision world the query walks is never
filled**, because neither producer of its shapes is reachable end to end.

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

- **CASC oracle:** eight verified units, `python dev/verify_harness/cli.py casc`
  passes 8/8: `Salsa20Xor`, `Salsa20StateInit`, `DynamicIndexHash`,
  `DynamicIndexKeyHash`, `EncodingHeaderParse`, `EncodingFindCKey`,
  `EncodingFindEKey` and `CdnIndexFooterParse`. The oracle found and fixed
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
  [`docs/ghidra-migration.md`](docs/ghidra-migration.md).

## Open

- **Map variation choice:** the client has no mode→variation selector. The
  field hashes `0xD97BC44F`/`0x78715D57` occur only in the registration
  functions and the header class hash only in the asset-type descriptor init;
  the map arrives as type-`0x002` GUIDs in the level-load message (Jam
  `0x501B`, `0x14063b3f0` → `0x140618b30` → `0x14077b7d0`). What is open is
  the producer of that message, including which input it uses. A
  `(mode, identifier)` pair can name two scene maps, so the practice-range
  launch affinity is a viewer default, not a missing client rule.
- **Bundle directory producer:** our bundle index scans the manifest's
  type-`0x09C` records; the client builds a directory blob in its content
  database (`FUN_141fd53a0`, `0x141fd53a0`). One non-1:1 stage remains.
- **Streaming policy:** which chunks load when is not read.
- **Gameplay:** no playable movement yet. The mover's arithmetic leaves are
  ground-truth verified (above), including the wish spring and the state-lookup
  leaf, and the live per-frame velocity law is read and implemented
  (`ow-sim::frame`). What is still open is the wish producer
  (`FUN_140cb86c0`'s fan evaluation and selection), the velocity→position
  stage, the Domino velocity dispatch and collision. Combat, camera
  application, view model, retarget, Pose2d VM and fire loop/damage remain
  open. The old approximations are deleted.
- **Materials:** the viewer is not the client's renderer; 55 placeholder draws
  remain diagnostics, not a finished material path. Texture parsing is now
  differentially verified, but the loader `0x141dacf00` and the chunk accessors
  (`0x142e38730`/`40`/`50`, `0x142255e10`) are read/install-checked rather than
  oracle-emulated, and the sampler and material application remain open.
- **D3D11 renderer stages D6-D10:** the 53 open replay opcodes (deferred
  constant-buffer/shader-resource caches, the per-draw `FUN_142674980` state
  flush, resource copies and the query/annotation path), the cache population,
  the outer caller of the shared frame slots, the
  D3D11 fence/query wait body, the engine-ring consumer, the draw-list builder
  and the writer of the batch item arrays, the input-element producer, the
  material -> shader-group -> tier join, constant contents and the adapter
  preference sort. The batch record layout and transfers are implemented in
  `ow-render::batch`, but the consumer that turns them into draws is open.
  `0x0c`/`0x0d` are implemented; the queued-SRV binder remains. A client
  D3D11 capture is **impossible** under the anti-cheat, so there is no
  captured-state or image oracle. D4/D5 are read and synthetically tested; D6-D10
  are not implemented, and D10 is not provable.
- **CASC CDN index:** read and install-tested. `CdnIndexFooterParse`
  (`0x1424d9af0`) is a verified unit: fixed 20-byte footer, header MD5,
  `pageSize = footer[3] << 10`, `entrySize = offset + size + key`,
  `pageCount = ceil(elements / entriesPerPage)`, exact total length.
  `CdnIndexFileParse` (`0x1424d9c90`) checks the terminal
  `MD5(last keys + checksums)` and `MD5(terminal + footer)` against the
  index's own key; `CdnIndexPageChecksums` (`0x1424d9f20`) compares every
  page MD5. All four config-named indexes match exactly: archive group
  260,173,908 B / 63,149 pages / 9,914,328 entries, patch group 48,616,028 /
  11,800 / 1,923,358, file index 576,828 / 140 / 28,374, patch file index
  90,668 / 22 / 4,426. The CDN client reads the footer and tail by Range GET;
  no per-page HTTP call site was found. **The last-key page-selection search is
  not found in the image; the row is open.** Searches: every 16-byte `memcmp`
  caller (317 functions, 25 with a loop), every function carrying the four
  footer fields (`0x14`/`0x1c`/`0x20`/`0x24` → the parsers only), every
  `SHL ...,4` function with a `0x24` reference, the handler classes at
  `0x1424e7cb0`/`0x1424e8db0`/`0x1424d1f40`, and the index builder at
  `0x1424f04f0`/`0x1424f0920`. The downloaded index goes to a callback
  (`0x1424b0850` → `0x1424bbe60`) whose consumer was not found; the search may
  live in the launcher/agent. The unit's final length-gate status is a declared
  gap (client writes only `AL`).
- **Remaining CASC units:** the CDN index page selection is the first read
  item and is open; the read list is **not** empty. The archive page cache is
  read and install-tested: 16 slots of `0x10000` bytes from the `.cmf` open
  params, key `(archive ordinal, page-aligned archive offset)`, a
  head/whole-page-run/tail plan, minimum-stamp eviction, and short-read
  zero-fill. The page-cache plan
  (`PageReadPlan`, `0x1424faa20`)
  passed 364 differential cases but cannot reach coverage: the
  null handle the oracle can build leaves the three refcount branches
  unexecuted (3/6), so it is not among the verified units. The encoding page
  lookup is read and verified:
  `EncodingPageIndexSearch` (`0x142498880`), `EncodingFindCKey`
  (`0x142498540`), `EncodingFindEKey` (`0x142498610`) and `EncodingResolve`
  (`0x142498700`). The search picks the last page whose first key is `<=` the
  key; the ckey scan stops on a zero low count byte; the ekey scan stops on a
  `0xFFFFFFFF` espec index; the resolve takes the ckey `u32` as the decoded
  size and the ekey `u40` low 32 bits as the stored size. The read path does
  **no** page-hash check. Units `fn/EncodingFindCKey` and
  `fn/EncodingFindEKey` are verified (100% coverage, 100% mutants);
  `0x142498880` has no standalone unit because its fifth argument is on the
  stack and the oracle supports four register arguments. The encoding section
  read (`EncodingSectionsRead`, `0x1424d1c60`) is read and implemented: five
  sections in the client's order, the client's 32-bit wrapping arithmetic, the
  range gate with a status-14 short read, and the espec tail check plus NUL
  count (`FUN_142498a70`, status 8). The install walk covers 16,025 + 10,519
  pages, 1,714,571 ckey entries, 1,714,573 ekeys, 1,714,573 espec entries,
  29,694 strings and 26,544 MD5 page hashes
  (`crates/ow-casc/tests/encoding_real.rs`, about 3 s in a debug build), plus
  193,863,820,123 decoded bytes and 99,356,850,082 stored bytes. It has no
  harness unit: the allocator vtable, the five I/O requests and the container
  lock are not a plain buffer. The segment parse/load
  (`DynamicIndexSegmentParse` `0x14250fcf0` / `DynamicIndexSegmentLoad`
  `0x14250fc60`) is read and mirrored: mode 2 is a synchronous `ReadFile` into
  an allocator buffer, the other modes map a read-only view, and both share the
  parse body; `ow-casc`'s `fs::read` is the mode-2 path. The validation order
  and the `block_size / 18` floor match, with synthetic rejection tests and all
  sixteen files parsed. The page-table writer (`DynamicIndexPageInsert`,
  `0x142511d20`) is read and documented but not implemented: the type-3
  callers pass a null range and write location/size from uninitialised stack
  slots, the writer is allocator/I/O state, and the read path never calls it.
  The BLTE frame header and `N`/`Z`/`E`/`4` blocks are read
  and install-tested; type `0x10` is absent and no outer `4` block was
  observed, so `4` is synthetic-only here (E inner modes are unreadable), and
  `A`/`F` are not block modes. The bucket
  fold, segment/archive selection, the 18-byte entry search with the
  first-duplicate choice, the key-state interval merge/resident resolve
  (`0x142520c60`/`0x142520700`/`0x1425208d0`/`0x1425115d0`), the page-table
  layout/hash/linear lookup, and the BLTE frame/block path are read and tested;
  all 2,560 install pages are zero, so the page hit path is synthetic-only.
  `docs/casc-1to1.md` is the register.

## Next

0. **The engine object — read and modelled; its subsystems are not.**
   `crates/ow-engine` models the client's engine object: the `0x470`-byte
   allocation `0x140585220` creates and publishes in `[0x143b7f410]`, both
   constructors (`0x141c507c0`, `0x1405838c0`) with all 44 field offsets and
   every written value, the address codecs and their inverses
   (`0x140543a40`, `0x1405674e0`, `0x1405437f0`, `0x1405838a0`, plus the inline
   one in `0x141c52d10`), the interface array at `+0x188` with its
   half-again growth and borrowed tag, and `ConstructEngineSubsystems`'
   schedule (`0x140554220`: 65 slot stores decoded row by row, plus the
   21-entry tail attach list), and the 35 constructors with their 96 vtable
   installs. 23 unit, 14 schedule, 12 layout and 8 class tests,
   `cargo test -p ow-engine`.
   **Not implemented:** the *meaning* of the schedule — a name for any of the
   35 constructors or 117 tables. The unit is a slot's own 15-entry table; 44
   of the 117 are exactly 15 entries and two lengths are still unread. Also
   unread: the 20 registered objects that never reach a slot, and the creation
   step's five callees (`0x141c57c40`, `0x141c52fc0`, the
   `0x141b33e20`/`0x141c61a80` debug pair, `0x141c51cc0`, `0x141c56970`).
   Details: `docs/formats/engine.md`, `dev/engine_classes.md`, queue entry 0 in
   `docs/open-questions.md`.

0. **Movement, the wish producer — `solve` exists; four stages still open.**
   `ow-sim::fan::solve` runs the read composition: the `body+0x659` gate, both
   endpoints (`point + seed*dir`, `point - speed*dir`, `body+0xcc` scaling the
   `w` lane), `cos(turn_limit)` as the facing threshold, the ring → refine →
   back-transform order, and both reflection bytes (`|=` then assignment).
   The five helpers are read: `0x1426cd3d0` is `cosf`, `0x140ca7400` is the
   cone angle in degrees clamped to `[0, 1.5707844]`, `0x140c9f110` returns a
   zero quad (the extrapolation is zero), `0x141ae3eb0` is the shape's box
   corner + extent, `0x140cc2290` is the mesh-face refinement.
   Still **not** implemented, and each is behind the seam rather than faked:
   (a) the evaluator `0x1420328c0` in Rust - it now **runs offline**, on the
   range's real geometry: `dev/probe_candidate_evaluator.py` drives the
   client's whole chain (`0x1420328c0` -> `0x14203dae0` -> `0x142cc45a0` ->
   `0x142cbd510` -> `0x142cc1610` -> `0x142caed30` -> `0x14202bc90`) against a
   built world. `crates/ow-tank/examples/mesh_world_dump.rs` emits the 272
   distinct shapes **and 3,017 placed instances** of them (one per model-group
   entry, each with the entry's translation and rotation), and the probe writes
   each placement into the owner pool `[record+0x2a0]`, which is where
   `ShapeWorldTransform` reads a shape's world frame. With the pinned shape
   alone the run reproduces `mesh_world_real`'s pinned hit exactly (point
   `(0.99500012, 4.0, 0.99500012)`, normal `(0, 1, 0)`, fraction
   `0x3e8051b8`, triangle `7`); with **all 3,017 in one broadphase** the answer
   record is **byte for byte the same** as the single-instance run, now in map
   coordinates (point `(23.947973, -19.990925, 31.369896)`, normal
   `(0.360226, 0.926024, 0.112766)`, fraction `0x3e8051b8`, triangle `6`). The
   guard's "two encodings" question is closed (one body, one null-byte canary).
   The **placeable transform is read**: the scene component's vtable `+0x20`
   slot `0x142003720` -> `0x142000620` -> component `+0xd0`/`+0xf0` ->
   `OwnerCreateBody` `0x142cc3d90` -> `ShapeWorldTransform` `0x142cb8b80`;
   `docs/formats/character-controller.md` carries the addresses. The leaf is
   now **implemented and verified** as `ow_sim::collide::shape_world_transform`
   / `fn/ShapeWorldTransform` (328 cases, every channel, branch coverage 100%,
   9/9 mutants killed), and it returns `[position, quaternion]` - the exact
   `xform` the three narrowphase primitives take. Two emulator NaN fidelities
   are recorded there rather than worked around: unicorn answers an invalid
   operation with `0x7fc00000` where x86 gives `0xffc00000`, and it does not
   always return the destination's NaN for two-NaN `ADDPS`/`SUBPS`; the
   hardware answer is pinned by a Rust test. Still open
   here: the serialised mapping from the `STUModelComponentInstanceData` stream
   to that triple (class `0x936A1A00` is not in the reference class list) and
   the geometry variant table for negative ids. The **Rust four-pass driver
   exists**:
   `ow-sim::collide::query_driver` walks the island's `0x40`-byte shape entries
   behind a `QueryWorld` seam, with both rejections, the worklist order, the
   `1`/`2` answers and the two folds, and 5 tests. It returns the folded motion
   quad, not a hit count - there is no counter in `0x142cbd510`.
   (b) pass 0's `MoverWishQueryCached` cache (`0.004` per lane — the test is
   `fan::memo_matches`, the cache is not); (c) the shape-ring branch behind the
   global `_DAT_1439b07b8`; (d) the mesh-face pass. 145 `ow-sim` units.
   Pass 3 is now read exactly (`ow-sim::fan::back_snap`, 2 tests): one snap of
   the accepted direction's `y` lane onto the plane through the direction with
   the direction as its normal - **not** a five-candidate loop. It is not
   called by `solve` yet: the pentagon loop (`0x140cb96c1`) precedes the
   `2*pi/n` ring loop (`0x140cb9ae0`) in address order while the snap reads the
   ring loop's endpoints, so the pass order inside `0x140cb86c0` is unsettled.
   **Next:** the probe with the range's 272 assembled shapes (item 6), then
   the `ow-client` wiring.

0a. **The map blob holds the chunk payloads (2026-11-03).**
   `LoadMapAssetChunks` (`0x141ff5c80`) stores one handle per chunk in the map
   asset blob, and the completion `0x142e49400` decodes each handle into the
   payload pointer: **`+0x2F8` is the collision chunk**, `+0x300` pathing,
   `+0x308` streaming, `+0x310`/`+0x318`/`+0x320` slots `0x20`/`0x1F`/`0x23`,
   and `+0x2F0` the extra-chunk payload array. `+0x410` is the "loaded" byte.
   So the collision payload is reachable from the map blob without another
   asset lookup, and `+0x2F8` is the exact handoff the missing sector consumer
   must read. `DmWorldConstruct` sizes a `0x58`-stride pool at `world+0x348`
   (from description `+0x40 + +0x44`) and `0x142cc2ae0` walks it with `0x58`
   stride and `0xC0`-stride records at `world+0x2B8`; the chunk's `0x58`
   sector records are pointer-based, the world's are index-based. Next: the
   consumer of `map+0x2F8` (likely a job lambda without `.pdata`, like the
   completion itself), and the map blob's full header (bounds + the three
   asset refs at `+0xB8`).

0a. **The node record is read field by field (2026-11-03).**
   `0x14202c660` (`WorldBaseInitFromRecord`) reads the record at `+0x00` (id to
   the state record `+0x170`), `+0x10` (flags to `[node+0x454]`), `+0x18` (a
   Domino **settings** description: rotation copied to `[node+0x630]`, two
   worlds allocated from it), `+0x28`..`+0x48` (copied), `+0x50` (decoded asset
   ref), `+0x60`..`+0x8f` (node `+0x878`..`+0x8a4`) and `+0x90` (copied).
   `DmWorldConstruct` reads the description as a pool count per element at `+0x20`..`+0x70`
   plus a rotation at `+0x10`; `ConstructDominoWorld` reads the same object plus
   its layer count, child list and pointers. So the description is a capacity
   template, not the map's geometry, and a node's worlds start empty of shapes.
   The init's own `OwnerCreateBody` descriptor is constant (no shape source),
   so the node's body gets no shape from the init. The 0x300 object the init
   allocates on flag `0x20` is a separate state record (`0x142026e20`, no
   vtable, id at `+0x170`), not the builder's scene component. The record's
   **producer** is still the anchor; it must now be a writer that supplies a
   settings description, not a placement.

0a. **The map's entity components are named by data (2026-11-03).** Each
   entity placeable's component record is `{u32 component id, u32 offset}`; the
   STU asset at the offset names the component's data class.
   `crates/ow-tank/examples/map_entity_probe.rs` reads the range: 550 entities,
   644 records, 13 ids, 6 named (`0x02` `STUModelComponentInstanceData` on 475
   entities, `0x03` `STUStatescriptComponentInstanceData`, `0x06`
   `STUWaypointInstanceData`, `0x0b` `STUTargetTagInstanceData`, `0x10`
   `STUECAISpawnerInstanceData`, `0x11` `STUECHintPointInstanceData`). None of
   the 13 data classes is a collision component, which matches the collision
   component attaching at the node rather than from entity data. The 0x300
   scene component (the collision builder's second argument) is the **model
   component** by role and by the 475-entity count, recorded as a hypothesis.
   Next: the node-side attach driver (the type allocator's caller).

0a. **Cells → shapes: the brush path is closed, the cell path is a specific
   question now.** `AppendCollisionEntryFromBrushSource` (`0x14213a510`) is read
   end to end — brush snapshot, `BrushTreeToMesh`, the leaf/chord-scale area
   test, then one `0x60` entry per mesh piece with `MeshToColliderFaces` filling
   `entry+0x20` and the handle going to `geometry+0xf8`. The geometry object's
   layout is recovered (`+0xb0` sources, five vector headers at `+0xf8`,
   `+0x108`, `+0x118`, `+0x128`, `+0x138`). The cheap reading of the 17 cell
   arrays is excluded by measurement, and a whole-image scan shows the entry
   count `+0x114` is written in only three places, none of them on the cell
   path — so **the chunk does not feed the `0x60` entries**. The `+0x118` vector
   is no alternative: its append helper `FUN_1421360a0` builds `0x100`-byte
   elements and has exactly one caller, the brush ingest. This node class is
   brush-only; next anchor is the class that owns the chunk component the
   bytes-fixup writes (`sub+0x68` slot `0x1e`, `sub+0x08` slot `0x22`).
   Three false `noreturn`s (`0x141bb00e0`, `0x142ced8f0`, `0x142efe2b0`) were
   truncating every allocating function in the geometry module; the mechanical
   audit recipe is in `docs/ghidra-project.md`. `FinishMapChunkAsset`
   (`0x141ff54e0`) was truncated the same way and is now 964 bytes that
   decompile end to end, confirming the slot `0x1e`/`0x22` cell dispatch.
   Two consumer hypotheses are ruled out: the geometry object (brush-only) and
   `FUN_1420c63b0` (no code caller, no plain pointer — its data xrefs are
   internal switch tables). **Handoff for this thread:** the fixup rewrites the
   cells in place, the cell block is `StuLoadSubAssets(cache, chunk->payload[0],
   0) + 0x68` (slot `0x1e`) or `+ 0x08` (`0x22`), so the consumer is whatever
   reads that sub-asset and walks 17 arrays — and `ow_tank::collision` already
   has that shape from real data to check it against.

1. Read the producer of the level-load GUID lists (`docs/open-questions.md`
   item 11.1): who resolves a map request to Jam `0x501B`'s four vectors, and
   whether the local practice-range session builds it in-process.
2. Movement: the ramp, command, wish-spring, forward-, inverse-transform,
   copy-wrapper, state-lookup, state-vector, settings-default,
   settings-apply and 37-kind modifier-evaluator leaves are verified, and the
   live per-frame velocity law (`state_transform(wish) + ramp_vector(body)`)
   is read and implemented as `ow-sim::frame`. The state lookup is implemented
   in `ow-sim::state` for the leaf path; its parent `+0x298` and pose paths
   are still open. The config path is
   read at the value level: `0x1405a87d0` defaults, `0x140c862e0` asset apply,
   `ConstructMoverBody` for the `+0x688`/`+0x68c` clamp pair and
   `FUN_140cc079d` for `+0x10ec`. Still open there: the asset producer and the
   body's copy of the block. The settings apply and the 37-kind
   `ApplyStateModifiers` are verified; the world slot resolves both handle
   kinds, and the character velocity chain (`0x142cbd1b0` -> `0x142cf5810`
   -> `0x142cd5370` -> `0x142cd4bc0`) is verified on the no-shape path, and
   the four shape-bound leaves, the `0x142cb9030` dispatcher and the
   shape-list walk with contact id `-1` are verified.

   **The three movement gaps left, in the order worth attacking:**
   1. *The wish producer* — `FUN_140cb86c0` (`0x140cb86c0`): the fan's
      arithmetic is now read and implemented (`ow-sim::fan`: both seed
      terms, the `+0.1` bias, the speed
      `((min(|body[0x1150].xyz|, max3(cap)) * 1.1) * (body[0xe8]*1e-6 * E))`
      floored by `body[0x5cc]`, the radius
      `max(body[0x65c], 0.35) * body[0x5b8] - 0.015`, the 72° and `2π/N`
      rotations, both acceptance rules, the gate and the three exits). The
      **candidate evaluator `0x1420328c0` is a collision query**: it wraps
      `0x14203dae0`, whose reachable code is now mapped (prologue
      `0x14203dae0..0x14203dbd9` + body `0x14203de51..0x14203e0fe`; the rest
      is junk) and read: geometry from `world+0x620`, the result record the
      fan reads back, a 0x71-byte query struct from the descriptor object,
      then `0x142cc45a0` (62-byte handle dispatcher) or `0x14212db80`
      (78 bytes). Its opaque guard is pointer-keyed (the world pointer's low
      byte through the nibble table `0x1433d158`). The query is now read down
      to its narrowphase: `0x142cc45a0` decodes the `u16` handle from
      `[geometry+0x31f8]` through `0x1442a0bc0` to a domino record, builds the
      `{record, [obj+0x58], hit_policy, [obj+0x60], island_slot}` descriptor
      and the `0x78`-byte query block, and calls **`0x142cbd510`**, the
      four-pass sweep over one island's shape list: `0x142cd5540` (the
      start/end/expansion, verified as `fn/CollisionSweepBounds`), an AABB
      test, a moving-box separation test (`|d x b - e*h - b x d| - 2*(e*h) >
      0.005`), `0x140d24b60` (the query's own worklist push, not a broadphase
      grow) for children, and `0x142cc1610` for leaves (world transform
      `0x142cb8b80`, layer filter `0x142cbb350`, type dispatch `0x142cb8d60`
      to `0x142ccdf40`/`0x142ccd7b0`/`0x142cd3c80`/`0x142caed10`/
      `0x142cd0ae0`). The five primitives are inventoried (0x4ec, 0x78a, 0x3f0,
      a 0x1a thunk to `0x142caed30`, 0xc00) and `0x142cb8b80` is read (a
      static pool copy, or a normalised pose quaternion). The evaluator's hit
      policy is the constant `0x14202c580` -> `0x14202bc90`. The thunk is
      byte-exact and the narrowphase fixes the contract (`callback(&hit,
      packedShapeDescriptor, answerRecord)`, permuted into `(hit, packed,
      record)`), so the policy's **arguments are read**; its body is **read
too** (2026-11-02): the `ADD`/`ROR 0x22` chain is a **pointer decode**, the far
      branch targets are junk-filler obfuscation, and a linear sweep decodes
      the 1797 bytes to exactly their end (427 instructions, no data islands).
      The contract is `(hit, packed, ctx)` -> 1 no hit / 2 accepted / 3
      rejected, with three gates (`world = 0x1420c2360(ctx)`, the
      `0x1420af950` layer+exclusion filter, the decoded-key dedupe against
      `[world+0x240]`), a mode switch on `hit+0x34` (0 = presence, 1 = fill the
      0x50-byte record at `[hit+0x10]`, >= 2 = classify through `hit[0]`), and
      three outputs (`hit+0x30` score, `hit+0x38` flag, `hit+0x39`). What is
      left is **data, not code**: the policy queries a Domino world, and the
      range's world is filled from the **model mesh** holders (161 models,
      272 shapes; `ow_tank::model_body` + `mesh_world_real` already assemble
      them) only if the node gate `[node+0x454] & 0x200` passes. The gate's
      word comes from the node creation record's `+0x10`, to be read at the
      asset-preload service's `vtable+0x38` (the all-preloads-done callback;
      check 17 in `formats/static-collision.md`), so **a candidate cannot yet
      be scored**
      for real and the wish producer and character collision are the same
      read. Also open: the generic helpers
      `0x1426cd3d0`/`0x1426de990`, the per-pass record layouts and the
      `0x141c5d780(object, byte)` query chain.
   2. *The velocity→position stage* — who turns the controller velocity into
      a sweep: the Domino tick's kinematic branch or a per-frame
      `0x142cbcee0` call. The character body's Domino parameters (mass,
      inv-mass, damping, gravity) and the tick dt/substep counts are open.
   3. *Collision* — the contact response slot `[service+0x40]`, the `>= 4`
      contact branch (`0x140ca8643`), the type-3 mesh path (`0x142cae5a0`),
      the contact event (`0x142cf32d0`) and the **descriptor producer** that
      turns the collision chunk's sector table into shape descriptors. The
      chunk payload is now read as data (`formats/maps.md`, "The collision
      chunk payload"); the sweep itself, the contact list layout and the
      projection into `body+0x19b8` are read.

   `MoverAdvance` is reachable only from `MoverInvoker`, which has no code
   caller (single vtable reference), so its integration expression is not on
   the live path; it stays open, not dead, until an indirect-call sweep
   proves it.
3. Then camera, animation retarget, combat and collision in that order.
4. Renderer: `0x0c`/`0x0d` are implemented; next is the `0x16..0x1b` cache
   population, then the queued-SRV binder, the pass writer and the draw list.

## Key files

- `docs/rebuild-plan.md` - stage order and current gates.
- `docs/open-questions.md` - the work queue.
- `docs/formats/character-controller.md` - mover component, current-image path.
- `docs/movement-plan.md` - the movement pipeline, the stage table and what
  `ow-client` needs.
- `docs/formats/domino.md` - physics tick and integration.
- `docs/formats/weapons.md` - combat data and statescript join.
- `docs/formats/camera.md` - camera surface, FOV, near/far, input.
- `docs/formats/animation.md` - clips, sampler, blend tree, view model.
- `docs/formats/maps.md` - map header, bundle, resolver and practice range.
- `docs/formats/engine.md` - the engine object, its constructors and codecs.
- `crates/ow-engine/` - the engine object model.
- `docs/deviations.md` - legacy non-1:1 code.
- `docs/systems-coverage.md` - every system and its next anchor.
- `dev/INDEX.md` - analysis tools.

# MODLOG

Milestones only. Findings and addresses live in `docs/formats/`; current state
in `STATUS-handoff.md`; the work queue in `docs/open-questions.md`.

## 2026-11-02 - Collision: the cell container and the runtime fixup

- `ow_tank::collision::parse` reads the collision chunk's STUD blob, the
  `0x9199BF6B` component, the `m_cellBytes` table and all 220 cells with their
  17 arrays. The install test `crates/ow-tank/tests/collision_cells_real.rs`
  pins the tiling, the first/last records and every array extent.
- `ow_tank::collision::fixup_cell_bytes` is `MapChunkCellBytesFixup`
  (`0x141ff51a0`) and is verified by `fn/MapChunkCellBytesFixup`: 264
  iterations plus the edge pool, 12/12 mutants, outer modes 1 and 2/3 and entry
  modes 1 and 2/3. The two mode-0 allocation branches are declared dead.
- The v1 `STUD` magic is the u32 `0x53545544` the loader compares; on disk that
  is `DUTS`. `ow-stu`'s `MAGIC_V1` and its synthetic builder are corrected.
- The node-type registry is enumerated: `0x141c5ff40` is a linked-list add to
  `0x144188170` with 144 `{u16 type; factory}` registrations, types `1..=0xb4`.
- Open: the cell-to-shape decoder. Candidates and dead ends are in
  `docs/open-questions.md` item 6, "Search map for the decoder".

## 2026-11-01 - Collision: world, pools, query and primitives

- World create is `0x142cc3c80`; a world is a `0x4d0` header plus eight
  strided pools. `ShapePoolAlloc`, `ShapeEntryFromDescriptor`,
  `ShapeAddToBody` and `ShapeAddToOwner` are read. The descriptor producer is
  still open.
- The collision chunk payload is decoded as data: two STUD v1 blobs, the
  `0x9199BF6B` component, a per-sector table and 220 cells whose 17 arrays
  tile each record.
- The query chain is read to the narrowphase: `0x1420328c0` -> `0x14203dae0`
  -> `0x142cc45a0` -> `0x142cbd510` (four-pass BVH) -> `0x142cd5540` +
  `0x142cc1610` -> five per-type primitives.
- Verified: `fn/CollisionSweepBounds`, `fn/CollisionSphere`,
  `fn/CollisionShapeFilter`, `fn/CollisionCapsule`, and the type-0/1 analytic
  paths. The support-point path and types 2, 3 and 4+ remain open.
- The 0x100 sector entry is read (body config, 0x40-stride matrix array,
  stolen u16 array, transform flag); the writer of `geometry+0x118`/`+0x124`
  is the next anchor.

## 2026-11-01 - Movement: the live velocity law

- Both live update paths compose `state_transform(wish) + ramp_vector(body)`
  and hand it to `SetCharacterControllerVelocity`; implemented as
  `ow-sim::frame`. The ramp and state-vector leaves are verified.
- The wish source is a closed fan over collision-scored candidates, not a
  button-to-direction conversion. The fan arithmetic is read and implemented
  in `ow-sim::fan`; its candidate evaluator is the collision query above.
- Verified units: `fn/MoverRampVector`, `fn/MoverRampScalar`,
  `fn/MoverRampScalarOffset`, `fn/BuildMoverPhysicsCommand`,
  `fn/MoverWishSpring`, `fn/MoverWishForward`, `fn/MoverWishInverse`,
  `fn/MoverWishCopy`, `fn/StateTransform`, `fn/MoverStateVector`,
  `fn/DefaultMoverSettings`, `fn/ApplyMoverSettings`,
  `fn/ApplyStateModifiers`, `fn/MoverGate`, `fn/CharacterPosition`,
  `fn/RigidPosition`, `fn/VariantPosition`, `fn/WorldPosition`,
  `fn/MoveCharacterController`, `fn/ApplyCharacterControllerMove`,
  `fn/SetCharacterControllerVelocity`, the four shape-bound leaves,
  `fn/ShapeDispatch` and `fn/MoveCharacterControllerShape`.
- Open: the wish producer's evaluator, the velocity-to-position stage, and the
  map-chunk-to-shape ingest. `MoverInvoker` is likely dead but not proven.

## 2026-11-01 - Tooling: clean Ghidra map and the harness

- The clean Ghidra map has 120,028 function symbols and 6,676,292
  instructions; `.pdata` starts are the authority for function entries.
- `Alloc` (`0x141baff30`) is no longer marked noreturn; truncated callers
  decompile in full.
- The movement corpus has 1,544 `.pdata` bodies with linear disassembly and
  694 decompiles. The harness gained `startRegs`, MS x64 stack arguments,
  range reads, `vec4` returns and state snapshots.

## 2026-10-31 - Renderer and replay

- D3D11 is the production path; D1-D5 are implemented in `ow-render` (device
  and swap chain, targets, frame order, the 64-opcode replay dispatch and the
  clear/draw/viewport handlers).

## 2026-10-30 - CASC: CDN index, page cache and encoding lookup

- The CDN index footer/page layout, the archive page cache and the encoding
  page lookup are read; the index parser is install-tested.
- Verified: `fn/CdnIndexFooterParse`, `fn/EncodingHeaderParse`,
  `fn/EncodingFindCKey`, `fn/EncodingFindEKey`.

## 2026-10-29 - Texture chain and exact uploads

- The format switch, descriptor builder and descriptor size walk are verified
  (`fn/TextureFormatFamilyBpp`, `fn/TextureDescriptor`,
  `fn/TextureDescriptorSizeWalk`).
- BC6H, RGBA16F and R16_FLOAT keep exact half words; R16_UNORM keeps its exact
  `u16` words. UINT, BC CPU decode, the sampler and the live GPU readback stay
  open.

## 2026-10-28 - CASC: BLTE and index writes

- The BLTE frame header, block reader, key-state intervals, page table and
  index entry search are read. `ow-casc` follows the client's validation order
  and status split.

## 2026-10-27 - Bundle, map and manifest units

- `fn/ChunkGuid`, `fn/BundleLookup`, `fn/BundleDirectoryLookup`,
  `fn/CmfKey154504` and `fn/CmfIv154504` are differential units.
- The client has no mode-to-variation selector; the map arrives as type-`0x002`
  GUIDs in the level-load message. The practice-range launch is a viewer
  default.
- The one client launches the practice range: 34/34 chunks, 1,358 placeables,
  229 models, 550 entities, bit-exact bounds.

## 2026-10-26 - Manifest, CASC oracle and the rebuilt gate

- The build-154504 manifest decrypts to 1,301,598 GUID-sorted records; the key
  and IV are derived from the installed client's own table.
- Eight CASC leaf units are verified; the client's zero-length and
  trailing-twelve-byte index-hash defects were fixed.
- The runtime crates were parked and the workspace reduced to the data layer.
  The practice-range chunk seed and the synthetic map claim were removed;
  `cargo xtask verify` became the one gate.

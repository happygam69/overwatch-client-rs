# overwatch-client-rs

A clean-room Rust reimplementation of the Overwatch client's gameplay:
movement, combat, the view model and the camera. It reads data from the
operator's own installation and behaviour from the complete current-build
`Overwatch.exe` image. Nothing from the game ships here.

**The rule: 1 to 1 or not at all.** A gameplay feature is implemented only when
the client's own code or value has been read and the Rust result is proven
identical. There is no "close enough" gameplay system. If the client's
expression has not been read, the feature is listed as open and is not
implemented. The current non-1:1 code is legacy debt in
[`docs/deviations.md`](docs/deviations.md); it is being removed.

**This repository contains no Blizzard content.** No art, models, sounds, maps,
textures, locale data, extracted tables or decompiled code — only original Rust
source and documentation. The tools open the installed game read-only and never
modify it. See [CREDITS.md](CREDITS.md).

## Sources

- **Behaviour** comes from one place: the complete `Overwatch.exe` Ghidra
  image (base `0x140000000`, every `.text` page, the install's plaintext
  `.pdata`). Findings are recorded as an address and a conclusion, never as
  disassembly. `docs/ghidra-project.md` is the entry point, and
  `docs/ghidra-image-audit.md` states how far the analysis is trusted (the
  bytes are exact; three analysis defects are measured and listed for repair).
- **Data** comes from the operator's installation: formats, registries,
  geometry, values. Findings live in `docs/formats/` and are checked by tests
  that read the install.
- Only the current image and the operator's installation are sources.

## Status

**The one client launches the real practice range.** `ow-client` and `ow-scene`
are workspace members; `ow-sim` is now a workspace member too and holds the
verified movement, Domino body and camera-basis arithmetic. The default
launch selects the practice header by its `m_mapName`
(`Maps\TrainingRoom\Training Room - Practice`), takes the first variation whose
paired mode's display name is exactly `Practice Range`, and loads the 34 chunks
the client's map loader requests for that variation's scene map
`0x080000000f69002e`. That choice is a viewer default: the current image has no
client-side mode→variation selector.

- **Read in the current image:** the mover advance/invoker path, the Domino
  tick driver, the camera surface and projection builders, the FP camera pose
  path, the animation sampler and clip runtime, the statescript executor, the
  manifest's bundle flag and bundle entry lookup, and the map-to-chunk loader.
- **Hero selection, as a declared viewer default:** the practice mode's own
  roster is read (its teams carry the empty marker, so every hero asset is
  offered), and the hero asset's own display name (`STUHero.m_0EDCE350`, type
  `0x07C`), playable marker (`m_64DC571F`; 135 hero assets, 54 playable and 81
  NPC) and movement row (`ow_tank::entity::MoverRow`) are read.
  `ow-client --list-heroes` prints them with forward speeds. A live launch with
  a terminal asks for a pick (`--hero <name|index>`, `--no-hero`); the pick
  places the hero's own first-person model and weapons, uses its forward speed
  as the wish magnitude and its terminal fall as the clamp. The real pick is
  still a CEF page value (`docs/formats/frontend-pipeline.md` 4c).
- **Working today:** the map header → variation → scene map → chunk resolver
  (`ow-tank::mapresolve`), the default client rendering the practice range, and
  **the character walking in it**: the map's own level collider (the
  `MAP_CHUNK_COLLISION` body: 203,999 vertices, 236,957 BVH nodes, 343,474
  triangles, world coordinates, `ow-tank::collision_level`), swept by the
  verified type-3 mesh leaf (`ow-sim::walk`) under the read per-frame velocity
  law (`ow-sim::frame`). The per-model blocking volumes (`ow-tank::range_world`)
  are not in the walk world: their read consumer is a query and their component
  route is unread, and adding them was measured to drift and climb
  (`docs/formats/character-controller.md`). `--walk-test` prints and asserts the
  numbers.
  The practice scene requests and opens 34/34 chunks, walks 1,358 placeables
  (229 model-group references, 550 entity references), and places 2,449 model
  instances from the model groups' 304 groups.
- **Movement arithmetic, verified:** `fn/MoverRampVector` (`0x140ca75b0`),
  `fn/MoverRampScalar` (`0x140ca75f0`), `fn/MoverRampScalarOffset`
  (`0x140ca7570`), `fn/BuildMoverPhysicsCommand` (`0x140cb74a0`),
  `fn/MoverWishSpring` (`0x140cc0dc0`), `fn/MoverWishForward`
  (`0x140cb5510`), `fn/MoverWishInverse` (`0x140cb5350`),
  `fn/MoverWishCopy` (`0x140ca7700`), `fn/StateTransform` (`0x140cb83b0` +
  the `FUN_140c71e10` leaf), `fn/MoverStateVector` (`0x140ca5c50`),
  `fn/DefaultMoverSettings` (`0x1405a87d0`), `fn/ApplyMoverSettings`
  (`0x140c862e0`), `fn/ApplyStateModifiers` (`0x140cbcbe0`),
  `fn/MoverGate` (`0x140ca7d80`), `fn/CharacterPosition` (`0x142cd4ff0`),
  `fn/RigidPosition` (`0x142cb4770`), `fn/VariantPosition` (`0x142cf5630`)
  `fn/WorldPosition` (`0x142cbcec0`), `fn/MoveCharacterController`
  (`0x142cd4bc0`), `fn/ApplyCharacterControllerMove` (`0x142cd5370`) and
  `fn/SetCharacterControllerVelocity` (`0x142cbd1b0`) and the four
  shape-bound leaves (`0x142ccd410`, `0x142ccd380`, `0x142ccf460`,
  `0x142cd3720`), the shape dispatcher (`0x142cb9030`), the shape-list
  walk (`fn/MoveCharacterControllerShape`), the collision sweep's bounds
  stage (`fn/CollisionSweepBounds`, `0x142cd5540`), the type-0 narrowphase's
  analytic path (`fn/CollisionSphere`, `0x142ccdf40`) and the query's layer
  filter (`fn/CollisionShapeFilter`, `0x142cbb350`) and the type-1 capsule
  (`fn/CollisionCapsule`, `0x142ccd7b0`), the type-3 mesh narrowphase
  (`fn/MeshSweep`, `0x142caed30`), the mesh gather's per-triangle accept
  (`fn/BoxTriangleOverlap`, `0x142cd58a0`), the box-BVH gather that feeds it
  (`fn/MeshGather`, `0x142caea70`), the closest-point solver the triangle
  test falls into on the live walk (`fn/ClosestPointGjk`, `0x142cc9460`)
  and the collision chunk's runtime
  17-pointer cell fixup (`fn/MapChunkCellBytesFixup`, `0x141ff51a0`) are
  differential units against the client's bytes, and the chunk's container
  (220 cells, 17 arrays each) is parsed and pinned on the install
  (`ow-tank::collision`, `collision_cells_real`). The wish spring proves the input copy, the
  `+0x10fc`/`+0x1100` spring and the 1/1024 quantiser; the forward and
  inverse units prove the 4x4 column-major transform, including the `rcpps` +
  Newton step; the copy and state-vector units prove the wrappers against a
  synthetic state record; the state unit implements the record lookup in
  Rust; the settings units implement the default constructor and the
  movement-asset apply, the modifier unit implements the 37-kind evaluator,
  the gate unit implements the movement predicate, and the world unit
  resolves both handle kinds to their positions, publishes the character
  velocity on the no-shape path, computes the four shape bounds plus the
  type-0/1/2/4 dispatch, and walks the shape list on the contact-id `-1`
  path. The live per-frame law is read and implemented in `ow-sim::frame`:
  both update paths publish `state_transform(wish) + ramp_vector(body)` to the
  controller, and the two `ADDPS` operand orders are pinned. With the range's
  collision assembled from its own model meshes, this law **is** the live walk
  in `ow-client`; what stays open around it is the wish producer, the character
  sweep's Domino aftermath, the movement asset (speed, jump, gravity) and the
  player spawn selector (`docs/open-questions.md` item 1b), the hero pick's
  binding (runtime variable 940) and the spawn write that consumes it. **Walls stop the
  walk**: the slide cannot climb a contact steeper than 45.6 degrees and the
  floor snap only ever lowers the body, proven against the range's own level
  collider (`ow-client --walk-test`, `--wall-audit`). **Jump works**: the
  take-off is read and shipped (`ow_sim::jump`: button bit 6, the state file
  word `body+0x10d0` and its five transitions, the gate, and the impulse
  `local.y := body+0x60c` = component `0xEC289987` = `6.0` roster-wide) and so
  is the arc (`ow_sim::air`: per airborne tick the terminal drag
  `MoverAirDrag` `0x140cb2010`, then `body+0x1150 += dt * (-10.0 *
  body[0x610]) * up` - `-17.5 m/s^2` from the read gravity scale `1.75` -
  through `MoverAccumulateWorldVector` `0x140c993e0`). `--walk-test` asserts the
  arc against the law's own numbers. What is still open around it is the
  **ground** vector resolve through the `body+0x960` record table and the swept
  solve's contact-to-position rule (`docs/open-questions.md`, "The jump law"). The practice range's
  viewer default is the map's own spawn-room volume, not a scene centroid. The collision side moved: the **placeable transform is read** (the
  scene component's vtable `+0x20` slot `0x142003720` copies the component's
  instance data and `0x142000620` writes the local `+0xd0` quaternion /
  `+0xf0` position, which `OwnerCreateBody` `0x142cc3d90` takes as the shape's
  body and `ShapeWorldTransform` `0x142cb8b80` reads back), so the range's
  collision world can be assembled in map coordinates: the offline probe
  places all **3,017** model-group instances and the client's own query
  returns the same answer record byte for byte as for the single pinned shape.
  The wish writer's fan solver is read and implemented in
  `ow-sim::fan` (both seed terms, the `+0.1` bias, the speed, the radius, the
  72°/`2π/N` candidate rotations, both acceptance rules and the gate); its
  candidate evaluator `0x1420328c0` is open, so no wish is produced yet; it is
  a wrapper over the collision query `0x14203dae0`, whose reachable body is
  now read (geometry from `world+0x620`, the result record, a 0x71-byte query
  struct, `0x142cc45a0`/`0x14212db80`), and the query is read down to its
  narrowphase (`0x142cbd510`'s four passes, the verified `0x142cd5540` sweep
  bounds, the AABB and moving-box tests, the worklist push `0x140d24b60`, and
  `0x142cc1610`'s transform/filter/type dispatch to five per-type
  primitives), so the input side and collision are the same read; those five
  primitives are open.
- **Open:** the producer of the level-load GUID lists (the client has no
  mode→variation selector; the map arrives as type-`0x002` GUIDs, so the
  practice-range launch is a viewer default, not a claim that a client caller
  is read), the streaming policy, the client's producer of the bundle
  directory, character collision, the mover's wish source
  (`FUN_140654730`/`FUN_140cb86c0`), the live service object, the state-record
  transform, the Domino velocity dispatch and the per-frame camera look, the FP
  retarget, the Pose2d VM, the weapon fire loop, damage application and the
  client material application (55 placeholder draws remain diagnostics).
- **Data-side reads:** CASC/BLTE, the CDN index footer, page layout, page
  selection and per-page read, the install-wide chunk-mode byte census,
  STU v2, entities and health pools,
  the model chunk table and its version gate, `MDLC`'s material list, `MRNM`'s
  descriptor layer (fixup, runtime size and runtime-block load are three
  differential units), `MSTU`'s header, skeleton and clip decoding (the
  `mskl` Lookup is the mesh slot → bone map: proven on all 7,328 skinned
  install models, with the client's own table sizes read from `MsklTableBuild`;
  the join's client consumer and the 454 models with a second blend-index set
  stay open), weapon
  graphs, map chunks, effect chunks (the `EFCT` root, the client's 0x34-entry
  kind table at `0x143610490` and the per-tag component layouts the builder
  `FUN_142e4a780` copies; the reference's effect structs are pre-2.x and every
  one of them is short), and the
  bundle-aware material/texture chain (the texture format switch, descriptor
  builder and descriptor size walk are differential-verified against the
  current image; R16_UNORM keeps its exact words and uploads as `R16Unorm`
  where the adapter exposes `TEXTURE_FORMAT_16BIT_NORM`). All 305 map headers
  are readable: 42 from the encoded store
  and 263 from bundles, as the client reads them. The model *vertex-element
  decode* and mesh assembly are the viewer's own reading; they are listed as
  open, not as client behaviour.

The rebuild order is [`docs/rebuild-plan.md`](docs/rebuild-plan.md).

The renderer's 1:1 target is **D3D11** (the production path); D3D12 is the
client's beta path. `crates/ow-render` has stages **D1-D4 and the D5
clear/draw/viewport handlers**: the client's D3D11 device and swap chain (D1),
the back-buffer texture with its explicit RTV/SRV/UAV and the resize order
(D2), the per-frame step order (D3), the 64-opcode replay dispatch (D4) and the
read clear/draw/viewport handlers (D5), each with the read values pinned by
synthetic tests. The D5 `0x0c` render-target set is implemented
(`OMSetRenderTargets` at `0x14266e54d`) and its alias-unbind is modelled in
`ow-render::state`; `0x0d` is implemented as the pending resolve/release pass.
The cache population and the queued-SRV binder remain. It creates the device,
swap chain and views and runs the frame step list; only
`Present` has a complete executed body and every other frame step is logged as
open (no pass list or draw call is read). The replay executor and the eight
read context handlers are tested with synthetic record streams; the other 53
opcodes are open by opcode. The engine's per-frame batch records (`0x387`,
`0x60`, `0x352`) and the A/B upload/readback layout are modelled in
`ow-render::batch`; the ring consumer is open. **A client capture is
impossible under the anti-cheat**, so there is no capture comparison and no
image oracle; D10/pixels are not provable. The richer `wgpu` viewer remains
viewer-only (`FIT-PRE-02`).

## Running it

```sh
export OW_INSTALL='E:\Overwatch'                        # folder with .build.info
export OW_EXE='E:\Overwatch\_retail_\Overwatch.exe'     # optional; found under OW_INSTALL anyway
export OW_TACTLIB='path/to/TACTLib/TACTLib'             # older-build manifest key tables
export OW_OWLIB='path/to/OWLib'                         # optional, STU class names
export OW_SUPPORT_KEYRING='path/to/Overwatch.keyring'   # optional

cargo xtask verify                                      # fmt, clippy, tests, index, deviations, addresses
cargo xtask verify --fast                               # without the install-reading tests

cargo run --release -p ow-client                        # the practice range; WASD walks
cargo run --release -p ow-client -- --walk-test         # the movement trace and its assertions
cargo run --release -p ow-client -- --free-cam          # the old free camera
cargo run --release -p ow-client -- --walk-column X Z --render-column   # every floor in one column, collision vs render
cargo run --release -p ow-client -- --list-maps         # headers and variation modes
cargo run --release -p ow-client -- --shot out.png --width 1280 --height 720
cargo run --release -p ow-client -- --tour out --count 4
cargo run --release -p ow-client -- --renderer d3d11 --no-map --d3d11-frames 2  # D1-D4 views + step log
cargo run -p ow-render --example d3d11_smoke            # D1-D5 smoke: views, step log, resize
cargo run -p ow-tank --bin look -- --why 0x0790000000000688  # the practice header
cargo run -p ow-tank --bin look -- --why 0x080000000f69002e  # its scene map
```

`--chunks`/`--chunks-from` remain explicit chunk diagnostics; they are not the
default and a chunk list is not a map. `--no-map` is the explicit stand-in
course for viewer work.

Data probes:

```sh
cargo run -p ow-tank --example blendset_probe -- 3    # FP items, clips, slots, tree
cargo run -p ow-tank --bin look -- --help             # asset/material probe
cargo run -p ow-tank --bin anim -- --help             # clip probe
```

## Layout

```text
crates/ow-casc   container: build info, config, keyring, buckets, page cache, BLTE, encoding, root
crates/ow-stu    structured data, typed STU reads, class registry
crates/ow-tank   game semantics: GUIDs, manifests, entities, weapons, graphs, animation, maps, mapresolve
crates/ow-scene  map geometry from the resolver's chunks and the models they place
crates/ow-client the one launchable client: renderer, HUD, free camera
crates/ow-render native D3D11 backend, stages D1-D4 + D5 handlers, engine batch records
crates/ow-sim    workspace member: verified mover ramp/command, the read per-frame velocity law, Domino body and camera arithmetic
crates/xtask     the project's own commands; `cargo xtask verify` is the gate
docs/            current findings, plans and open questions
dev/             read-only analysis and verification tools (tracked); output is not
```

## Licence

MIT or Apache-2.0, at your option. That covers this code only — not the game,
its data, or anything either of them produces.

## Contributing

Start with [CONTRIBUTING.md](CONTRIBUTING.md) and [AGENTS.md](AGENTS.md). The
short version: one gate decides whether a change is done —

```sh
cargo xtask verify --fast   # what CI runs; drop --fast once you have an install
```

— and the work queue is [`docs/open-questions.md`](docs/open-questions.md).

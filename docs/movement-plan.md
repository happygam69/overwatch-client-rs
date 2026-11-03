# Movement: the implementation plan for `ow-client`

What it will take to put a read, 1-to-1 movement step into the client. Every
address is a current-image address (`0x140000000` + RVA). The stage table at
the bottom is the honest status; the plan is the order the remaining reads and
the code have to happen in.

## The read per-frame pipeline

This is the client's live path, in order. Each line is a read expression or a
call with its address; the arrows are the data flow.

```text
1. input            view object (vtable 0x142f87810, slot +0x250 = 0x140654530)
                    -> 0x140654730 writes the 0x14-byte usercmd at actor+0x1210
                       (yaw +0x04, pitch +0x06, buttons +0x0c, flags +0x10)
                    -> 0x140689930 (probable) -> 0x14068cf10 -> body+0xd0/+0xe0
                    -> body+0xf0 records / FUN_140cc1060 (fallback)

2. wish source      FUN_140cb63c0 / the input builder 0x140cba3d0
                    -> FUN_140cb86c0 (the fan solver)
                         seed  = dot(pos - M*wish, body+0xc0)
                                 (+ dot(query - (pos - ramp), dir) when the
                                  state id is not the sentinel)
                         speed = ((min(|body[0x1150].xyz|, max3(cap)) * 1.1)
                                  * (body[0xe8]*1e-6 * E)) floored by body[0x5cc]
                         radius = max(body[0x65c], 0.35) * body[0x5b8] - 0.015
                         three passes: 5-way at 72°, 5-way at 72°, N-way at 2π/N
                         each candidate tested by 0x1420328c0 -> 0x14203dae0
                         -> 0x142cc45a0 -> 0x142cbd510 (collision)
                         -> source+0x00 (wish), +0x40 (gate), u16 index

3. input object     input+0x60 = source+0x00, input+0x98 = source+0x40

4. wish spring      FUN_140cc0dc0: body+0x1140 = input+0x60 (gated), then the
                    +0x10fc/+0x1100 spring against +0x5c0/+0x5c4, the
                    +0x688/+0x68c clamps and the 1/1024 quantiser

5. state vector     FUN_140ca5c50: body+0x1140 through the state record's 4x4
                    (FUN_140cb5510); unchanged on the sentinel

6. ramp vector      FUN_140ca75b0: (minss(body[0x10ec], 1) * scale
                    + body[0x10fc]) * body[0xc0..0xcc]

7. publish          v = addps(state, ramp); SetCharacterControllerVelocity
                    ([body+0x1ad8], &v) -> 0x142cf5810 -> 0x142cd5370
                    -> 0x142cd4bc0: entry+0x10 = v, entry+0x00 = normalised
                    quat, record+0x190 = 1

8. world            the Domino step / broadphase refit consumes record+0x190
```

`MoverUpdateCommand` (`0x140cb5760`) is the second path: steps 5-7 with the
`ADDPS` operands swapped, gated on `input+0x4c != 0`. `MoverStateMachine`
(`0x140cbacc0`) stops at the physics command and never publishes.
`MoverAdvance` (`0x140ca7f30`) is only reachable from `MoverInvoker`
(`0x1407ffc50`), which has no code caller, so its integrator is not on the
live path.

## What the published value is

`entry+0x10` is read back by `0x142cbcec0` -> `0x142cf5630` (the same value the
fan calls "position"), and the fan's seed is `dot(pos - M*wish, dir)`, i.e. it
treats `M*wish` as a **position** it compares against the current one. The
setter writes exactly that field. So the live path publishes a **position**:
the fan chooses a reachable target, the spring smooths it, the state transform
maps it to world space and the ramp adds the per-frame offset.

That reading is consistent with `MoverAdvance`'s dead path, where
`get(handle) + scale_b*wish` is also a position. It is **not** yet proven by a
consumer read of the Domino step, so it stays in the table as *read, not
proven*: the two candidate readings are "position publish" (all the evidence)
and "velocity publish" (the function's name).

## The blocking reads, in order

1. **The narrowphase primitives.** The driver `0x142cbd510` is read
   (`formats/character-controller.md`): four passes, one per bit of the
   caller's flag byte, each calling `0x142cd5540` (verified as
   `fn/CollisionSweepBounds`), then walking that pass's shape tree with a
   stride-`0x40` entry (min/max, id, two child indices): the swept-AABB
   rejection, the moving-box separation test (`|d x b - e*h - b x d| -
   2*(e*h) > 0.005`), a worklist push for children, and
   `0x142cc1610(D, &hit, B, id)` for leaves. `0x142cc1610` builds the entry's
   world transform (`0x142cb8b80`), applies the layer filter
   (`0x142cbb350`), and dispatches on the entry's type byte (`0x142cb8d60`)
   to one of five per-type primitives: `0x142ccdf40` (0), `0x142ccd7b0` (1),
   `0x142cd3c80` (2), `0x142caed10` (3), `0x142cd0ae0` (4+). Type 0's analytic
   path is read and verified (`fn/CollisionSphere`); what is left of the
   primitives is type 0's support-point path (`0x142cc9460`) and types 1, 2, 3
   and 4+, plus the transform's dynamic branch.
2. **The geometry the query reads** (`world+0x620`, `[geometry+0x31f8]` as a
   `u16` handle into `0x1442a0bc0`, the shape entries at `[record+0x2b8]`
   stride `0xc0`, and the per-type pools at `[record+0x2d0]..[record+0x330]`).
   The pool side is **read** (2026-11-01): `DmWorldConstruct` sizes every pool
   from the config, `ShapePoolAlloc` (`0x142cb00c0`) is the free-list pop,
   `ShapeEntryFromDescriptor` (`0x142cb7f10`) writes a 0xc0 entry and allocates
   the per-type record by the source's type byte at `+0x67`, and
   `ShapeAddToBody` (`0x142cb3940`) / `ShapeAddToOwner` (`0x142cd4ef0`) are the
   add handles. What is still missing is the **descriptor producer**: the
   converter from the map's collision chunk (payload read in
   `formats/maps.md`) into those descriptors. `0x140d24b60` turned out to be
   the query's own worklist push, not a broadphase grow.
3. **The wish's seed — settled as "there is none" (2026-11-01).** The packer
   is read exactly (`0x14068cf10`'s tail) and the state-record pass is read,
   but the route "an external producer writes the wish from the buttons" is
   **closed**: `source+0x00` has one writer (the fan), and `body+0x1140` has
   the spring plus `0x140cbf950`, which stores `0x140ca7650`'s output — a
   world vector converted *back* through the inverse transform. The movement
   direction therefore exists only as a fan candidate scored against collision,
   remembered one frame later. This makes the collision geometry the only
   remaining route to a wish, and it is why item 2 cannot be skipped.
4. **The settings/asset producer**: which install asset builds the object
   `0x140c862e0` applies, and where the live body receives its copy of the
   `+0x5d0..+0x67c` block.

## The stage table

| stage | state | evidence |
|---|---|---|
| usercmd writer (`0x140654730`) | read | fields and clamps in `formats/camera.md` |
| command packer (`0x14068cf10` tail) | read + implemented | current record with the next record's yaw/pitch overlaid; `ow_sim::frame::pack_command`; the `0x140689930` driver is still open |
| state-record pass (`0x140cb63c0`) | read | state id from `[state_object+0xbc]` (default 1), the `0x800000` skip, the fan call with `dt=0`, the service `+0x58/+0x60/+0x68` hooks |
| fan seed, speed, radius, rotations, acceptance | read + implemented | `ow-sim::fan`, 17 tests |
| fan evaluator body (`0x14203dae0`) | read | result record + query struct layouts |
| handle dispatcher (`0x142cc45a0`) | read | handle decode, the driver context, the 0xaa0-byte observation ring; the ring is gated on `request+0x71`, whose writer is not read |
| collision query driver (`0x142cbd510`) | read | four passes, sweep AABB, the three per-entry masks, the worklist, the leaf call |
| sweep bounds (`0x142cd5540`) | verified | `fn/CollisionSweepBounds` |
| narrowphase wrapper (`0x142cc1610`) | read | transform, layer filter, type dispatch, and the `callback(&hit, packed, record)` contract |
| layer filter (`0x142cbb350`) | verified | `fn/CollisionShapeFilter` |
| shape world transform (`0x142cb8b80`) | read | static pool copy and the normalised pose quaternion; not implemented |
| type-0 narrowphase (`0x142ccdf40`, 0x4ec) | analytic path verified | `fn/CollisionSphere`; the support-point path (`0x142cc9460`) is open |
| type-1 capsule (`0x142ccd7b0`, 0x78a) | verified | `fn/CollisionCapsule`; the support-point path (`0x142cc9460`) is open |
| narrowphases `0x142cd3c80` (0x3f0), `0x142caed10` (0x1a thunk), `0x142cd0ae0` (0xc00) | **open** | inventoried, two-path shape known |
| hit policy (`0x14202c580` -> `0x14202bc90`) | **read** | contract `(hit, packed, ctx)` -> 1/2/3; world gate, `0x1420af950` layer/exclusion filter, decoded geometry-key dedupe, 0x50-byte record fill for mode 1, classifier `hit[0]` for mode >= 2; only the map-data tables it reads are missing (item 6 ingest) |
| geometry ingest (map chunks → descriptors) | **collision container read; its consumer open** | kinds are read: `0x11` = `MAP_CHUNK_COLLISION` (container read in `ow_tank::chunkkind`, 7 tests) and `0x12` = `MAP_CHUNK_PATHING` (the 220-cell container, read/verified as a container, consumer not found — expected for pathing); the collision **consumer** is the open link: who fills the Domino shape-entry pool at `[record+0x2b8]` |
| brush route (node type 2 → descriptor → brush → mesh) | **read and implemented; meshes exact** | `ow_tank::brush` + `brush_mesh` + `brush_convert` (25 tests, client-emulated): hull stage, status tree, six ops, ear predicate, clipping, face writer; **all four** pinned meshes bit-exact (the arrow's divergence was ops 2/5 reading `+0x28` vs `+0x30`); `C` **named** (registered class `0x564B1018`, size `0x198`, vtable `0x1431dd118`; `+0x98` a 16-byte array header over a `Vec3A[]`, `+0x48` an asset reference), encoding read and implemented (`ow_tank::model_collision`); **no model carries it** (32,278 scanned) — `C` is an entity/fracturable descriptor (`FRAC`), not the range's static collision, so the static route is now the **model mesh**: 161 of the range's 229 models carry `STU_537E56E4` (272 instances) via holder `0x2DD96D6D` at `STUModel+0x118`; its consumer is `FUN_14210bac0` (list at the client's `+0x140`, not the reference's `+0x118`), which hands the mesh by pointer (`0x141b53af0`, `MeshShapeFill`, `0x142caf6c0`) and calls no shape-add; its caller is a scene-side query owner and `0x142caf6c0` writes nothing persistent, so it is **query-only**; the character's ingest `0x142cbcd40` has six callers, all mover/AI, so no read path fills the Domino world with the range's static collision. The last read is whether `RunCharacterControllerSweep` queries the scene owner via a vtable. The range carries no `C` (0 hits in 550 placeables) and the chunk is not RVA-dispatch reachable |
| shape pools: alloc / entry build / add | read | `DmWorldConstruct` (`0x142cbeb70`), `ShapePoolAlloc` (`0x142cb00c0`), `ShapeEntryFromDescriptor` (`0x142cb7f10`), `ShapeAddToBody` (`0x142cb3940`), `ShapeAddToOwner` (`0x142cd4ef0`) |
| wish spring (`0x140cc0dc0`) | verified | `fn/MoverWishSpring` |
| state transform (`0x140ca5c50`) | verified | `fn/MoverStateVector` (leaf path) |
| state record parent/pose | **open** | `[object+0x298]`, `FUN_140c71e10` `param_3` |
| ramp (`0x140ca75b0`) | verified | `fn/MoverRampVector` |
| settings default/apply | verified | `fn/DefaultMoverSettings`, `fn/ApplyMoverSettings` |
| 37-kind modifiers | verified | `fn/ApplyStateModifiers` |
| publish chain (`0x142cbd1b0` -> `0x142cd4bc0`) | verified | `fn/SetCharacterControllerVelocity`, `fn/MoveCharacterController` |
| published value's meaning | read, not proven | see above; the new `0x140cbf950` evidence shows the value is re-imported as a world vector and converted back into the wish space |
| Domino step / broadphase refit | read | `formats/domino.md` |

## Implementation checklist

Ordered by dependency. Each line names the file to write, what proves it, and
what data it still needs. The rule is unchanged: an item is done when its unit
is green, not when the code compiles.

**1. `crates/ow-sim/src/policy.rs` — the hit policy. DONE (12 units green).**

Landed as written below; `cargo test -p ow-sim policy` runs 12 tests and the
gate is green. The harness unit `fn/CandidateHitPolicy` is still pending — it
needs a seeded geometry object, so it comes after item 5.

```rust
pub const NO_HIT: u8 = 1;
pub const ACCEPTED: u8 = 2;
pub const REJECTED: u8 = 3;
pub struct PackedShape { point: [f32;4], normal: [f32;4], fraction: f32,
                         word30: u32, id: i16, sub: u16, key: u16 }
pub struct HitRecord { /* 0x50 bytes, field offsets as read */ }
pub struct HitFilter  { required: u32 /*+0x08*/, forbidden: u32 /*+0x10*/,
                        excluded: &[u64] /*+0x20/+0x28*/, arg470: u64 }
pub struct Hit { classifier, filter, record, optional, mode, score, flag, flag2 }
pub struct World { layer_mask /*+0x280*/, identity, geometry: Option<&dyn Geometry> }
pub trait Geometry { slot_word, base_index, table_entry, variant_table,
                     key_mix, key_salt }
pub trait VariantLookup { fn resolve(&self, index: u16) -> Option<(u16,u16)> }
pub fn candidate_hit_policy(hit: &mut Hit, packed: &PackedShape,
                            ctx: u64, world: Option<&World>) -> u8
```

The world is `Option` because the client's null-world `je` skips both the
geometry decode and the optional filter and runs the mode switch directly;
the geometry object is supplied as a trait so the policy runs without a map.
The three gates, the mode switch and the record fill are in the client's
order. Two obfuscation details are reproduced, not simplified away:
`decode_geometry_pointer` (`0x14202beba`) with its constants pinned by a
round-trip test against the documented inverse, and the entry/key chain in
`geometry_key` including the two anti-tamper globals (`[0x143b79598+0x198]`,
`byte[0x1438480f3]`) taken from the geometry as `key_mix`/`key_salt`.

`dead_presence_term` / `dead_key` keep the **first** call site's binding, whose
presence mask is `(word & 0x0f) >> 4` — always zero. That is why the dedupe
scan over `[world+0x240]` is unreachable in this build, and
`the_first_sites_presence_term_is_always_zero` asserts it over all 65536 slot
words. The live binding (mode `1`) takes the presence bit from `0x10`.

**One value is not bit-identical by construction:** with no world the client
never executes the decode's load, so `rec+0x20` receives a stale register. A
Rust caller has no stale register and writes `0`. It is documented on
`HitRecord::geometry`; the real evaluator always has a world, so the path is
unreachable from `0x1420328c0`.

**2. `crates/ow-sim/src/fan.rs` — the seam and the refinement. DONE, minus the
outer walk.**

Landed: `CandidateSolver` (`evaluate(a, b) -> Option<SweepHit>`), `SweepHit
{ score, normal }`, `facing(dir, normal)` (from the refinement's
comparison at site `0x140cb9770`: `y`, `x`,
`z`, summed left to right), and the refinement pass —
`horizontal_unit`, `back_direction`, `refine_direction`, `refined_offset`,
`refined_sweep`, `accepts_refine`, `refine_pentagon`. `policy::PolicySolver`
implements the trait, so the chain runs end to end against a synthetic world.

What is still unwritten is `fan::solve` itself — the **outer** walk over the
writer's four evaluator call sites. The pieces it needs are all read and
tested; the composition (which site runs, what the query carries, what the
`+0x1140` write takes from the record) is not. `open-questions.md` 1a carries
it with its anchors.

**3. `crates/ow-sim/src/frame.rs` — compose.** Already implemented
(`state_vector`, `ramp_from_body`, `frame_velocity`, `update_frame`); no change.
End-to-end test: a synthetic body plus a synthetic world must produce the same
velocity the two client call sites compute, bit-for-bit.

**4. `crates/ow-client/` — live wiring.** Order from `MoverUpdateFrame`:
`ApplyStateModifiers` -> `SubmitMoverPhysicsCommand` -> input-source pass ->
`FUN_140cc0dc0` (spring) -> state vector -> ramp -> `SetCharacterControllerVelocity`.
Needs a body record (`ow-sim::body`) and a world (item 5). Nothing here is a
read; it is assembly, so it lands last.

**5. Cells → shapes: OPEN (2026-11-02; first census corrected).** Three of the
four parts are done: `ow-tank::collision` parses the 220 cells,
`fn/MapChunkCellBytesFixup` proves the pointer fixup, and the component's three
array fields are decoded structurally (`ow_tank::collision::tables`: the arrays
at container `0x98`/`0x2358`/`0x2428` tile the blob with no gap, so their
`0x28`/`0x20`/`0x10` strides follow from the tiling by exact division). The
fourth is **open**: no client code that reads a cell has been found, but the
first version of this item overstated that as "the reader is not in this
image". That claim rested on Ghidra's caller metadata, which is incomplete for
this image (chained/fragmented `.pdata`, undefined regions), and it is
withdrawn — Ghidra reported 3 callers for `ShapeAddToBody` where a verified
`E8` scan finds 27 sites in 13 functions. The corrected census of all 28 shape
producers is in `formats/maps.md`, "The cell path's consumer: open"; none of
them calls `StuLoadSubAssets`, none reads the chunk work record's payload at
`+0x60`, and none reads the component's `{pointer, mode}` headers. So there is
still no `fn/MapChunkToShapes` to write and `WorldView` stays synthetic, but
the stage is **open**, not excluded. Closing the movement blocker needs either
the reader found (next anchors: `0x1420c63b0`'s input, the three
`MeshShapeFill` sites `0x140c2813f`/`0x142078562`/`0x1420a02f6`, and the
domino-side body filler `0x142cdf9f2`) or a plan decision to build the
collision the query walks from the model brush trees (the node-type-2 path,
whose leaf builders `0x142cec9e0`/`0x142ced620`/`0x142ced990`/`0x142ceca00`
are unread). All three anchors were then read: they are Domino/brush-side (a
`0x38`-stride node array, a brush collider, a stored mesh source's bounds, the
owner's `+0x140` mesh-descriptor container, the owner's `0x40`-byte body
vector) and **none is reachable from any code in the image** — no call, no
tail jump, no immediate, no pointer, no encoded pointer. The one next reader is
the producer of that `+0x140` container (`formats/maps.md`, "The three next
readers").

**5a. The brush route: read where it matters, blocked on data (2026-11-02).**
The second producer is no longer a plan: `GeometryBuild` (`0x14213c440`)
resolves the node's model (`geometry+0x80` → `[node+0x38]` → `[+0x40]` →
`[+0x48]` = `C`, the collision descriptor) and builds the brush at runtime;
the record formats, the vertex decode (`0x142ced510`) and the ring builder
(`0x142ced620`) are read and implemented in `ow_tank::brush` (11 units, every
expectation pinned to the client's bytes by `dev/brush_vertex_oracle.py`).
`MeshToColliderFaces` (`0x142cf2010`) is read and already implemented. Three
things keep it from producing a world: the six op bodies and the emission in
`BrushTreeToMesh` (`0x142cf23c0`) are unread; `C`'s class and its
`+0x98`/`+0xa0` inline-singleton-vs-array encoding are unread; and the install
opens **0** assets of the GUID type the brush driver requires (`0x00A`, 0 of
1,425,794 records), so no brush instance is reachable from this install.
Full detail: `formats/brush-collision.md`.

**5b. Which chunk is collision: READ, and it was inverted (2026-11-02).**
Kind `0x11` is `MAP_CHUNK_COLLISION` (the 21.8 MB `0x0DD0001100000688`), kind
`0x12` is `MAP_CHUNK_PATHING` (the 220-cell container this project had been
calling the collision chunk), `0x10` additive collision and `0x23` additive
pathing — from the kind table `0x1439aa320` plus the name decoder `0x1404c19b0`
(all 36 names decode; the eight known placeable names come out exactly), and
corroborated by the per-slot dispatch. The collision container is read and
implemented (`ow_tank::chunkkind`, 7 tests): counts 36/1, array A `0x58`-stride
at `0x14D3810`, array B `0x38`-stride at `0x14D8948`, the in-place fixup
`FUN_142e492a0`, and the four-run tiling that holds for all 36 sectors. **The
consumer is not the shape pool**: a store census of the world's pools shows
their contents are created only through the shape-add paths, and every caller
that appends collision material is a brush producer (`formats/shape-ingest.md`).
The remaining loophole is an inline appender no decoded store shows. `BuildBvh`
(`0x142ccb1b0`) has five call sites, but they are split wrappers over the
collider-face array and all unreachable.

**6. Verify.** `cargo xtask verify --fast` gates everything above except the
install reads; the full `cargo xtask verify` runs `collision_cells_real`. The
movement units that must stay green while this lands: `fn/MoverWishForward`,
`fn/MoverWishInverse`, `fn/MoverWishCopy`, `fn/MoverWishSpring`,
`fn/MoverStateVector`, `fn/MoverRampVector`, `fn/MoverRampScalar`,
`fn/MoverAdvance`, `fn/MoveCharacterController`, `fn/CollisionShapeFilter`,
`fn/CollisionSphere`, `fn/CollisionCapsule`, `fn/CollisionSweepBounds`,
`fn/MapChunkCellBytesFixup`.

**What is implementable now:** items 1–3 against a synthetic world — the whole
producer loop runs, deterministically, with no map. **What is not:** any
scored candidate on a real map, which is item 5 alone.

## What goes into `ow-client` when the reads land

The client already has the frame loop, the map and the scene. Movement adds:

1. `ow-sim::frame::update_frame` (already implemented) — the velocity/position
   law from a body slice, a flags block and the resolved state matrix.
2. `ow-sim::fan` (already implemented) — the candidate generation and
   acceptance. It needs the evaluator as an injected closure so the collision
   query can be swapped in once it is read: `fan::select(candidates, |c|
   query(c))`.
3. A `CharacterState` in `ow-client`: the body slice (`+0xc0`, `+0x5a8`,
   `+0x5b4`, `+0x5c0`, `+0x5c4`, `+0x5cc`, `+0x5d0..`, `+0x688`, `+0x68c`,
   `+0x10d0`, `+0x10ec`, `+0x10fc`, `+0x1140`), the state record, and the
   geometry handle.
4. The input source: the usercmd from the window (already read in
   `formats/camera.md`), fed into the fan once the command producer is read.
5. A test harness: a golden per-step timeline — the same body, the same input,
   N frames, compared first-divergence against the emulated client chain.

The client stays a viewer until every row above is read; a movement step with
an invented wish or an invented geometry is exactly what the project forbids.

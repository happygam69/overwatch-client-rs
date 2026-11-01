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
| handle dispatcher (`0x142cc45a0`) | read | decode, descriptor + block layout, island branch dead on this caller |
| collision query driver (`0x142cbd510`) | read | four passes, AABB + moving-box tests, worklist |
| sweep bounds (`0x142cd5540`) | verified | `fn/CollisionSweepBounds` |
| narrowphase wrapper (`0x142cc1610`) | read | transform, layer filter, type dispatch |
| layer filter (`0x142cbb350`) | verified | `fn/CollisionShapeFilter` |
| shape world transform (`0x142cb8b80`) | read | static pool copy and the normalised pose quaternion; not implemented |
| type-0 narrowphase (`0x142ccdf40`, 0x4ec) | analytic path verified | `fn/CollisionSphere`; the support-point path (`0x142cc9460`) is open |
| type-1 capsule (`0x142ccd7b0`, 0x78a) | verified | `fn/CollisionCapsule`; the support-point path (`0x142cc9460`) is open |
| narrowphases `0x142cd3c80` (0x3f0), `0x142caed10` (0x1a thunk), `0x142cd0ae0` (0xc00) | **open** | inventoried, two-path shape known |
| hit policy (`0x14202c580` -> `0x14202bc90`) | **open** | thunk and argument order read; needs a live collision world |
| geometry ingest (map chunks → descriptors) | **open** | payload format read; the converter is the next anchor (`formats/maps.md`) |
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

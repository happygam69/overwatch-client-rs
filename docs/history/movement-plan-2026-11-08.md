# Movement plan - what landed on 2026-11-08 (archived 2026-11-12)

The "What landed in `ow-client`" section, moved verbatim out of
`docs/movement-plan.md` once the walk was live: the per-frame narrative
duplicated `docs/systems-coverage.md` and `docs/formats/character-controller.md`.
The numbers are frozen at 2026-11-08.

## What landed in `ow-client` (2026-11-08)

Movement is live. The default launch builds the practice range's own collision
world, stands a character on it, and runs the read frame law every frame:

1. **The world**, from two producers, both the install's own data:
   - **the map's level collider**, which is what a character actually walks on:
     the `MAP_CHUNK_COLLISION` body's single tail element names a vertex array
     (203,999 x `0x10`), a node array (236,957 x `0x20`) and a triangle array
     (343,474 x `0x20`), in the client's own mesh record layout and already in
     world coordinates (`ow_tank::collision_level`; evidence and the exact
     tiling in `formats/chunk-kinds.md` section 4b; install test
     `collision_level_real`). This is the piece whose absence made most floors
     pass-through.
   - **the per-model blocking volumes**: the holder filter of `0x14201ae60`
     (`ow_tank::model_body`), the `STU_537E56E4` meshes (272 shapes over 161
     models) and the map chunk's per-entry `(translation, scale, rotation)`,
     which is the map's local transform and therefore the shape's world
     transform (`fn/ShapeWorldTransform`). Install test `range_world_real`
     (273 shapes, 3,018 placed instances, 32/32 downward sweeps). The
     placement's rotation is handed to the sweep as the conjugate, because the
     leaf rotates the query *into* the shape
     (`walk::tests::the_shape_transform_rotates_the_query_into_the_shape`).
2. **The step** - `ow_sim::walk::World` is one call per shape to the verified
   type-3 mesh leaf (`fn/MeshSweep`, `0x142caed30`) over the verified query
   block (`fn/CollisionSweepBounds`, `0x142cd5540`), with a uniform grid over
   the shapes' world boxes as the broadphase, the driver's fold (travel the hit
   fraction, re-seat, and stop after the fixed count), and a slide that
   projects the residual off the contact normal.
3. **The frame** - `ow-client`'s `walk::Walker` writes the local wish at the
   client's own offset (`body+0x1140`) and calls `ow_sim::frame::update_frame`,
   which is `ADDPS(state_vector, ramp)`: `MoverStateVector` under the character's
   rotation, `MoverRampVector` from `+0x5a8`/`+0x5b4`/`+0x10fc`/`+0x10ec`, and
   then `MoverAdvance`'s `(state + ramp) * dt + pos`. The camera follows the
   body's eye.

`--walk-test` is the numbers-only proof: a scripted trace (walk, idle, strafe, a
six-second walk with the view turning, then a twenty-metre drop) that prints
what it does and asserts forward travel, zero idle drift, strafe travel,
finiteness, that the six-second walk never drops more than six metres, and that
the drop lands. `--free-cam` keeps the old
viewer camera; `--chunks` (a per-chunk diagnostic view) always does.


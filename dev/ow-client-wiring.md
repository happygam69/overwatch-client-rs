# `ow-client`'s live wiring vs the install

Audit date: 2026-11-06. Addresses are current-image addresses in
`dev/ghidra_run/owmod_clean` / `Overwatch.exe`. This is the follow-up to
`dev/collision-audit.md`: that file asks what the *data path* does, this one asks
whether the *live loop* is wired the way the client's own loop is.

**Answer in one line: no. The collision *leaf* is exact; the wiring around it is
a set of declared stand-ins, and six of the eighteen stages below are
substitutes.**

## The install's per-frame chain (re-read today)

```text
MoverUpdateFrame 0x140cbb260  /  MoverUpdateCommand 0x140cb5760
   state modifiers -> submit -> input build -> MoverWishSpring 0x140cc0dc0 -> body+0x1140
MoverStateMachine 0x140cbacc0
   reads flags/timer/state id, writes body+0x1a79, transforms the local wish
MoverAdvance 0x140ca7f30
   target = pos + scale_b * (wish + scale_a * body[0xc0..0xcc])
   BuildMoverPhysicsCommand 0x140cb74a0 (verified unit)
   RunCharacterControllerSweep 0x142cbcee0 (character, world, block)
RunCharacterControllerSweep 0x142cbcee0  (read in full today)
   block: {target quad, scale_b @0x10, body[0x628]*scale_b @0x14,
           0.005 @0x18, step cap @0x1c, step byte @0x20}
   solver 0x142cf5660 with a 512-byte contact buffer (16 x 0x20)
                      and a 1024-byte pair buffer   (32 x 0x20)
   publishes: contacts -> world+0x200, count +0x20c, stride 0x20
              pairs    -> world+0x610, count +0x61c
              resolved position -> world+0x620 (quad) and world+0x630 (quad)
              world+0x640, world+0x644
MoverAdvance, after the sweep
   per contact i: d = dot(contact[i][0..2], wish_normalised)
                  body+0x19b8 = min(body+0x19b8, d)          (no position effect)
   if contacts >= 4 (branch 0x140ca8643): service vtable +0x40 per contact,
                  passed &contact[i] + 0x14
```

The position the character ends at is written by the **sweep** into
`world+0x620`, not by `MoverAdvance`; the mover reads it back through the
character handle (`CharacterPosition` `0x142cd4ff0`, verified as
`fn/CharacterPosition`). Gravity, acceleration, friction and step are **not** in
the controller path (`character-controller.md`, "The open ramp" item 4): the
controller is a shape carrier and broadphase updater.

## `ow-client`'s per-frame chain

```text
update(dt) in crates/ow-client/src/main.rs:688
   keys -> walk::wish_local(forward, strafe, walk_speed)      (normalised, m/s)
   walker.step(wish, camera.yaw, dt)                          crates/ow-client/src/walk.rs:161
      body+0x1140 = wish                                     (written directly)
      ow_sim::frame::update_frame = state_vector(wish,R_y) + ramp
      delta = velocity*dt ; delta[1] += fall*dt
      ow_sim::walk::snap_to_floor(world, pos+0.2, 0.35, 1.5)  or gravity
      ow_sim::walk::advance(world, pos, delta, 0.35)
         <= 3 iterations of World::sweep (8 m grid) + residual projection
      pos = result
   camera.position = walker.eye() = pos + (0, 1.25, 0)
```

## Stage by stage

| # | stage | install | `ow-client` | verdict |
|---|---|---|---|---|
| 1 | buttons -> local wish | unread input stage -> `MoverWishSpring` `0x140cc0dc0`: spring against `+0x5c0/+0x5c4`, clamps `+0x688/+0x68c`, `1/1024` quantiser | keyboard -> normalised wish x speed, written straight into `body+0x1140` | **substituted**: no spring, no clamp, no quantiser, instant response |
| 2 | state transform | `MoverStateMachine` `0x140cbacc0` reads the state id and transforms the local wish | `state_vector(wish, R_y(yaw))`, identity state | matches only for the identity state; the state-id path is a verified unit that is not wired |
| 3 | ramp | `MoverRampVector` `0x140ca75b0` with the timer and scale driven by the state machine, direction `body+0xc0..0xcc` | ramp hard-zeroed (timer `0`, offset `0`, both scales `1.0`; direction `(0,1,0,0)`) | the term is disabled. Equal iff the client's ramp is zero during a ground walk - **unverified** |
| 4 | integrator | `MoverAdvance`: `pos + scale_b*(wish + scale_a*body[0xc0])`, gated by `body+0x1a78 != 0 && body+0x658 != 0` | `pos + velocity*dt` | same form; the two gates are not modelled |
| 5 | sweep entry | `RunCharacterControllerSweep` `0x142cbcee0` with the six-field block including the **step cap** and **0.005** epsilon | `World::sweep(start, end, radius)` per shape, no block | **different**: the block, the step state and the epsilon never reach the leaf |
| 6 | what is swept | the character's own **shape entry** (`0x142cc3f50`) against the world's shape entries. The block carries **no radius** | a point with expansion `0.35` against a `0x4c` `MeshShapeFill` record | **different geometry**: a sphere proxy stands in for the character's capsule/cylinder, whose type and radii are unread |
| 7 | iterations | internal, `eps = 0.005`, cap `step_state+0x94` clamped by `body[0x1a88]`; returns **all** contacts | at most 3 outer iterations, one contact each | different |
| 8 | contact output | 16 x `0x20` contacts (normal at `+0x00`, second quad at `+0x14`), 32 x `0x20` pairs, entity handle | `Sweep { fraction, normal, point, shape }` | **the pairs and the second quad are never produced** |
| 9 | contact response | min `dot(normal, wish_hat)` -> `body+0x19b8` (a number, no motion); `>= 4` contacts -> service vtable `+0x40` per contact | project the residual off the normal, zero the vertical lane on a wall, re-seat by `CONTACT_SKIN = 0.005` | **unread client code replaced by a fitted slide** (3 iterations, `WALKABLE_SLOPE = 0.7`, `1e-6` break) |
| 10 | vertical | `MoverApplyForces` `0x140cb1990` / `MoverGravityStage` `0x140cb2b00`: airborne only, `MoverAirDrag` `0x140cb2010` then `MoverAccumulateWorldVector` `0x140c993e0` adds `dt * (-10.0 * body[0x610]) * up`; `FUN_140cc1440` is the jump / crouch stage, not vertical (`body+0x10fc` is the stand/crouch height shift) | `ow_sim::air` (drag + gravity, the take-off in `ow_sim::jump`), floor snap | **read and wired**; the ground vector resolve (`FUN_140cb2220` + the `body+0x960` records) and the contact response are still stand-ins |
| 11 | step / ledge | `step_state+0x94` and `+0x9a` consumed inside the sweep | `snap_to_floor(reach 1.5)`, `normal.y > 0.9`, no-lift guard | **substituted** |
| 12 | the world | Domino world slot from `[node+0x620]+0x31f8`; bodies from `0x14201ae60`; shape entries carry the descriptor's mask/filter/ids; transforms from the scene component (`+0xd0`/`+0xf0`, writer open) | `RangeWorld`: 272 holder meshes + one inferred level BVH, flat list, mask `0xffff`, transform from the map chunk | different container; **shape modes 1/2/4 are dropped** (`range_world.rs:394-397`) |
| 13 | broadphase | query driver `0x142cbd510`: four passes over the flag byte's bits, AABB and moving-box rejection, worklist `0x142cadf00` | 8 m grid hash, radius grow, dedupe, sorted | different structure, same intent |
| 14 | masks | command filter triple -> query mask; per-shape inline filter; triangle mask `+0x1c` | `MESH_MASK = 0xffff`, no filter | the level mesh's masks are all within `0x1FFF`, so the level is unaffected; a model mesh's masks are asset data and the client's runtime mask comes from the mover/shape layer (unread), so `0xffff` accepts triangles the client may exclude |
| 15 | world tick | `DmTickKeeper`, catch-up loop `FUN_142cdfc50`, substep driver `0x142cc3630` | none - the geometry is static | fine while nothing moves; no moving platforms, no dynamic bodies |
| 16 | resolved position | the sweep writes `world+0x620`; read back through the handle | returned directly as `out.pos` | equivalent read-back, not the client's record |
| 17 | camera | the FP pose consumer (`FIT-ANI-02`), eye height unread (`FIT-CAM-05` = 1.75 m for the viewer) | `pos + 1.25` (`EYE_ABOVE_CENTRE`) | **fitted**, and a different number from the viewer's |
| 18 | dt | the client's own frame step (`scale_b`) and the world's substeps | wall clock, clamped to `0.1 s` (`main.rs:1159`) | same kind, not the same source |

## What is exactly the install's

Four leaves in this chain are differentially verified against the client's own
bytes and are used with their real inputs:

- the type-3 mesh leaf `0x142caed30` (`fn/MeshSweep`, plus
  `dev/mesh_support_diff.py` for the support-point hull);
- the closest-point solver `0x142cc9460` (`fn/ClosestPointGjk`);
- the shape world transform `0x142cb8b80` (`fn/ShapeWorldTransform`);
- the narrowphase wrapper `0x142cc1610` (`fn/ShapeNarrowphase`, sink push
  excepted).

Three more are read and transcribed but have no differential: `MeshShapeFill`
`0x142cae540` (the record every shape carries), `MoverAdvance`'s target
expression, and the `MeshToColliderFaces` shell.

## The three that matter most

1. **The swept geometry is wrong in kind, not just in value.** The client sweeps
   the character's shape entry; we sweep a point inflated by `0.35`. Every
   doorway, ledge and wall contact at the character's edge is therefore measured
   against a sphere the client does not have. This is the single reason
   "`--walk-test` passes" cannot be read as "the walk matches the client".
2. **The response is invented, and the client's is unread in two places.** The
   client's `MoverAdvance` only computes a number (`body+0x19b8`); the actual
   response is inside the solver `0x142cf3f80` (unread) and the service `+0x40`
   slot (unread). Our slide is a projection with three tuned constants.
3. **The input is bypassed, not just sourced differently.** The spring
   `0x140cc0dc0` is verified as a unit and is skipped entirely, so
   start/stop/sharpen behaviour differs from the client even with a perfect
   keyboard map.

## A stale record this audit hit

`crates/ow-tank/tests/mesh_world_real.rs:22-23` says "the capsule's
support-point handoff into the solver is still **open**", while
`docs/formats/static-collision.md` (check 16, later) says it is shipped and
differenced by `dev/mesh_support_diff.py`, and `crates/ow-sim/src/collide.rs`
implements it. Third instance of the same pattern (with `brush-collision.md` and
the `fn/MeshSweep` ledger note): a file records an earlier state of the work and
nothing re-runs it.

## What would move the answer from "no" to "mostly"
1. Read what type and radii the character's shape entry carries
   (`0x142cc3f50` and the descriptor it copies) - then the proxy sphere can be
   replaced by the client's shape and stage 6 closes.
2. Read the solver `0x142cf3f80`'s contact -> position rule, and the service
   vtable `+0x40` slot it calls per contact - stages 8, 9, 11 close together.
3. Read the producer of the five inline record vectors built at
   `0x140ca37d0` (`body+0x690`/`0x760`/`0x870`/`0x960`/`0xa70`) - `FUN_140cb2220`
   consumes them as the per-frame movement resolve, which is what closes stage
   10 and the jump's arc. The take-off itself is already read (`ow_sim::jump`);
   `FUN_140cc1440` is not a vertical stage.
4. Wire `MoverWishSpring` `0x140cc0dc0` and `ApplyStateModifiers`
   `0x140cbcbe0` between the keyboard and `body+0x1140` - stage 1 closes and the
   input stage becomes the only declared default.

---

## Strip list: what is invented, and what replacing it costs

Every row is a stage where the live path substitutes something the client's code
does not say. "Needs" is the read that would let the row be replaced rather than
deleted. **No row has a verified replacement in the tree today** - each one is
one or two reads away, and none is a research project.

| # | invented item | where | needs (read) |
|---|---|---|---|
| 1 | character swept as a point + `expansion 0.35` | `ow-client/src/walk.rs:43` (`RADIUS`) | **input read, 2 links open.** The record and the descriptor are parsed (`ow_tank::character_collision`, 11 tests) from `0x142cf5440` -> `0x142cf35a0` -> `0x142cc3f50`. Still unread: the type-1 element index (`entry+0x24`'s writer) and the radius (`0x142ccd7b0` reads element `+0x20`) |
| 2 | wish = normalise(keys) x speed | `ow-client/src/walk.rs:361` | `wish_spring` is already implemented and verified (`fn/MoverWishSpring`); missing are the input object's `+0x60..0x6c` producer and the parameters `body+0x5c0/+0x5c4`, clamps `+0x688/+0x68c` |
| 3 | ramp pinned to zero | `ow-client/src/walk.rs:157-163` | `MoverStateMachine` `0x140cbacc0` + `ApplyStateModifiers` `0x140cbcbe0` (37-way switch, read in outline) |
| 4 | slide: 3 iterations, `WALKABLE_SLOPE 0.7`, `1e-6`, `CONTACT_SKIN 0.005` | `ow-sim/src/walk.rs:328` | `RunCharacterControllerSweep`'s solver `0x142cf3f80` (unread, >29 KB) and the service vtable `+0x40` slot `MoverAdvance` calls per contact |
| 5 | floor snap: `reach 1.5`, `normal.y > 0.9`, no-lift | `ow-sim/src/walk.rs:402` + `ow-client` | `step_state+0x94` and `+0x9a`, consumed inside the sweep; no other reader |
| 6 | gravity | `ow-sim/src/air.rs` | **read 2026-11-16**: `0x14202fb00` returns the image constant `-10.0` (`0x1431161f8`) or `0.0`, scaled by `body+0x610` (component `+0xe8` = `0xC2C40969`, `1.75` -> `17.5 m/s^2`) along `body+0xc0..0xcc`; the drag is quadratic on the up lane with terminal `body+0x614`/`body+0x618` |
| 7 | eye height `1.25` | `ow-client/src/walk.rs:48` | the FP pose consumer (the viewer's `1.75` is `FIT-CAM-05`, itself unread) |
| 8 | `MESH_MASK 0xffff`, no layer filter | `ow-sim/src/walk.rs:41` | `BuildMoverPhysicsCommand` `0x140cb74a0` is verified but needs the body's mask bytes, which nothing in the tree writes from a read source; the shape entry's own filter is `descriptor+0x24..0x4c` (sources unread) |
| 9 | 8 m grid broadphase | `ow-sim/src/walk.rs:90` | not behaviour *if* proven a superset of the shapes a sweep can touch; a brute-force equality test would settle it without any read |
| 10 | level BVH placed at the origin | `ow-tank/src/collision_level.rs` + `range_world` | the kind-`0x11` chunk's consumer (open; the one large unread block) |
| 11 | placements from the map chunk transform | `ow-tank/src/range_world.rs` | the scene component's `+0xd0` (quaternion) / `+0xf0` (position) writer |
| 12 | all-ones holder mask | `ow-tank/src/range_world.rs` (`holder_selected(.., u64::MAX)`) | the holder parameter instance's filter words (`holder+0x18`, sources read to the offset, not to the field) |

What survives a strict strip unchanged: `mesh_sweep` `0x142caed30`,
`closest_point` `0x142cc9460`, `shape_world_transform` `0x142cb8b80`,
`shape_narrowphase` `0x142cc1610`, `sweep_bounds` `0x142cd5540`, the
`MeshShapeFill` record, the `MoverAdvance` target form, `TERMINAL_FALL 30.0`,
and the whole parser layer that is read (`brush`, `brush_mesh`,
`brush_convert`, `collision`, `collision_level`'s *data*, `chunkkind`,
`mesh_shape`, `mesh_world`, `model_body`, `model_collision`).

### The fork

- **(A) strict.** Delete rows 1-7 and 10-12. `ow-client` then has no collision
  query it can issue (row 1 is the shape), so it cannot walk at all: `build_walker`
  reports the open stages and the FP camera has no body. This is the letter of
  the hard rule ("an open stage has no gameplay implementation at all"). It
  removes ~1,100 lines (`ow-sim/src/walk.rs` minus `World`, `ow-client/src/walk.rs`,
  the `main.rs` walk wiring, 14 tests) and `--walk-test` becomes a report.
- **(B) read, then delete.** Close row 1 (two functions, both already located),
  then rows 4+5 (one solver function), then rows 2/3/6/7. Each step deletes its
  stand-in when its read lands. The client keeps walking throughout.
- **(C) default-off.** Keep the proxy walk behind an explicit flag and make the
  default refuse, so nothing invented is on the default path. Smallest change,
  but the code stays.

Recommended: **(B) starting with row 1** - it is the only row whose read is
already in hand (`0x142cf5440` -> `0x142cf35a0` -> `0x142cc3f50`), and it removes
the biggest behavioural lie (a 0.35 m sphere). Rows 5, 6 and 8 *may* fall out of
the same descriptor - it carries a `0.7`, a `10.0`, a `0.1` and a type word that
look like the slope, step and skin numbers the walk guesses - but that meaning is
**not read**, so treat it as a lead, not a result.

# Domino: the physics schema, read out in full

## Current state

- **Runtime schema: read in full** - 21 classes, 179 field records, every
  offset.
- **Integration: read.** Fixed 1/30 s steps with catch-up repetition and a
  *commanded* step count; the world has no vtable; broadphase, pools and the
  contact cache are read.
- **dt: read** - the module dt table uses 1/30 s; the step count comes from
  the owner config (`ceil(seconds * 30 * 1)`), not a per-frame accumulator.
- **Open:** the runtime-built caller tables for the tick builder (cold
  pages); otherwise this row is closed.

**Verify:** the readings are addresses in the standing capture; no
runtime test exists for the client's solver.


The client's physics engine is a Box2D-lineage system called Domino (`dm*`
classes). This document is the result of a decompile round against the v2b
capture (build `2.25.0.0.154088`, see `docs/systems-plan.md` for the capture
set and the ASLR deltas) that **recovered the complete runtime schema** — every
class, every field, every offset: 21 classes, 179 field records. This is the
Q1 artefact `docs/rewrite-plan.md` was blocked on.

## How it was recovered, because the route matters

Three earlier approaches are now known to be dead ends and are recorded so
they are not re-walked:

1. **The `0x7ff7c536xxxx` "schema-reading cluster"** the previous round banked
   as the next target is **narrative-viewer UI serialization and Wwise audio
   code**. Its functions build hash-keyed schemas for `narrative_viewer_*`
   widgets — keys `"title"`, `"header"`, `"description"`, `"thumbnail"`,
   `"chapter_guid"`, `"subtitle"`, `"image"` — and one site is
   `CAkLEngine::PostRunVpls` (Wwise). The address previously written down as
   "the `bcPQTransform` field-type object at `0x7ff7c7d200b0`" **is the string
   `"title"`**: the label was a coincidence of an earlier offset-probe. The two
   remaining recorded sites (`0x7ff7c7a38c95`, `0x7ff7c7a5b1bd`) were the real
   ones.
2. **The referrer to the `dmBody` name string is the class's descriptor
   getter**, not a reflection consumer walk: `0x7ff7c7a38c40` (Ghidra
   `FUN_7ff7c7a38c40`) fills a static descriptor with literal stores. The code
   just past it, `FUN_7ff7c7a38cbf`, is the **field-array filler** — it writes
   `{name pointer, offset, type code}` triples with immediate offsets. That is
   the authoritative key to the record layout.
3. **Reading the *registry* directly beats hunting the initializers.** The
   descriptors and their field arrays all live in one `.data` region
   (`0x7ff7c90e7000`, protect `0x4`) and are populated at startup, so the
   capture holds them. A scan for the record shape finds all 21 classes at
   once; no code search required. The one quirk: the record's third dword
   column reads `0x1` for `dmWorld` (a singleton registered differently) where
   every other class carries its byte size — the field count column is right
   for all of them.

**Descriptor record** (stride 0x20, registry `0x7ff7c90eb700`..`0x7ff7c90ebb40`):

```
+0x00 qword  class-name pointer (into the name pool, plaintext)
+0x08 qword  field-array pointer
+0x10 u32    class byte size (dmWorld: a flag instead)
+0x14 u32    field count
+0x18 qword  per-class hook (a tiny function; see below)
```

**Field record** (stride 0x18):

```
+0x00 qword  field-name pointer, or a code pointer when the field has a
             custom accessor (printed `fun@…`)
+0x08 u32    offset in the class
+0x10 u32    type code (legend below)
```

## Type-code legend

Confirmed where the slot size implied by the *next* field's offset matches the
named shape's own `size` column (that cross-check is exact for every marked
row); the rest are single-field reads.

| code | meaning | basis |
|---|---|---|
| `0x1` | byte / bool | bool fields at consecutive byte offsets |
| `0x2` | byte enum | `bodyType`, `ownerType` |
| `0x3` | small int (8-bit) | `groupIndex`, `boneIndex` |
| `0x4` | int32 | many |
| `0x5` | small int | `toiCount`, `debugBin` |
| `0x6` | int16 | `m_index` (+4 bytes incl. pad), `materialId`, `gameFlags` |
| `0x7` | u32 stamp | `islandStamp`, `keyA/keyB` |
| `0x8` | u64 | `userData` in `dmDynamicTree::Node`, `key` in a manifold point |
| `0x9` | float32 | all scalar floats |
| `0xa` | bcVec3 (16-byte stride) | velocity/size gaps |
| `0xb` | bcAABB (0x20) | `aabb` |
| `0xc` | string (16-byte slot) | `name` |
| `0xd` | bcPQTransform (0x20) | confirmed, self-registered |
| `0xe` | bcMtx44 (0x40) | confirmed |
| `0xf` | dmSweep (0x60) | confirmed |
| `0x11` | pointer to array | `nodes` |
| `0x12` | array (inline) | `trees` |
| `0x13` | dmBroadPhase (0xd8) | confirmed |
| `0x14/0x18/0x19/0x1a/0x1c/0x22/0x25` | typed arrays: bodies / fixtures / spheres / capsules / polytopes / contacts / convex contacts | `m_*` families |
| `0x16` | dmFilter (6) | confirmed |
| `0x17` | pointer | fixture `userData` |
| `0x1b` | array of dmSubEdge | `m_subEdges` |
| `0x1f` | inline dmManifoldPoint[4] | `points` at +0, normal at +0xc0 |
| `0x20/0x21/0x23/0x24` | arrays of manifold / contact-edge / GJK cache / SAT cache | offsets and sizes |

## The class census and the two classes the movement work needs

| class | size | fields | | class | size | fields |
|---|---|---|---|---|---|---|
| bcPQTransform | 0x20 | 2 | | dmManifoldPoint | 0x30 | 6 |
| bcMtx44 | 0x40 | 4 | | dmManifold | 0x100 | 7 |
| dmSweep | 0x60 | 6 | | dmContact::Edge | 0xc | 3 |
| dmWorld | (flag) | 16 | | dmContact | 0x58 | 11 |
| dmDynamicTree::Node | 0x40 | 6 | | dmGJKCache | 0xc | 3 |
| dmDynamicTree | 0x28 | 7 | | dmSATCache | 0x8 | 4 |
| dmBroadPhase | 0xd8 | 2 | | dmConvexContact | 0x18 | 3 |
| dmBody | 0x1c0 | 40 | | dmFilter | 0x6 | 3 |
| dmFixture | 0xc0 | 35 | | dmSphere | 0x20 | 2 |
| dmCapsule | 0x30 | 3 | | dmPolytope | 0x50 | 11 |
| dmSubEdge | 0x4 | 4 | | | | |

### dmBody — 0x1c0 bytes

| # | field | offset | type | | # | field | offset | type |
|---|---|---|---|---|---|---|---|---|
| 0 | index | 0x00 | int32 | | 20 | maxRadius | 0x184 | float |
| 1 | transform | 0x10 | PQ | | 21 | approximateRadius | 0x188 | float |
| 2 | linearVelocity | 0x30 | vec3 | | 22 | massStabilizationScale | 0x18c | float |
| 3 | angularVelocity | 0x40 | vec3 | | 23 | islandStamp | 0x190 | stamp |
| 4 | sweep | 0x50 | dmSweep | | 24 | awakeIndex | 0x194 | int32 |
| 5 | `I` | 0xb0 | Mtx44 | | 25 | revision | 0x198 | int16 |
| 6 | invI | 0xf0 | Mtx44 | | 26 | toiCount | 0x19a | i8 |
| 7 | force | 0x130 | vec3 | | 27 | debugBin | 0x19b | i8 |
| 8 | torque | 0x140 | vec3 | | 28 | name | 0x19c | string16 |
| 9 | fixtureList | 0x158 | int32 | | 29 | bodyType | 0x1ac | enum8 |
| 10 | fixtureCount | 0x15c | int32 | | 30 | enableSleeping | 0x1ad | bool |
| 11 | jointListKey | 0x160 | int32 | | 31 | isAwake | 0x1ae | bool |
| 12 | mass | 0x164 | float | | 32 | isEnabled | 0x1af | bool |
| 13 | invMass | 0x168 | float | | 33 | isStabilized | 0x1b0 | bool |
| 14 | gravityScale | 0x16c | float | | 34 | useContinuous | 0x1b1 | bool |
| 15 | linearDamping | 0x170 | float | | 35 | isBullet | 0x1b2 | bool |
| 16 | angularDamping | 0x174 | float | | 36 | isSubset | 0x1b3 | bool |
| 17 | inertiaScale | 0x178 | float | | 37 | isFast | 0x1b4 | bool |
| 18 | sleepTime | 0x17c | float | | 38 | allowFastRotation | 0x1b5 | bool |
| 19 | minRadius | 0x180 | float | | 39 | characterMover | 0x1b6 | bool |

`characterMover` is a **byte flag** at the end of the struct, not a
reference — the body carries a flag that a character mover drives it; the
link to the mover object itself is not a field of `dmBody`.

### dmWorld — the root object

| field | offset | type |
|---|---|---|
| m_index | 0x194 | int16 |
| **m_inv_dt0** | **0x198** | **float** |
| **m_gravity** | **0x1a0** | **vec3** |
| m_broadPhase | 0x1b0 | dmBroadPhase |
| m_bodies | 0x288 | array |
| m_fixtures | 0x2b8 | array |
| m_spheres | 0x2d0 | array |
| m_capsules | 0x2e8 | array |
| m_polytopes | 0x300 | array |
| m_contacts | 0x348 | array |
| m_convexContacts | 0x360 | array |
| m_awakeBodies | 0x480 | int32 |
| m_awakeContacts | 0x490 | int32 |
| m_useSleep | 0x4c4 | bool |
| m_useWarmStarting | 0x4c5 | bool |
| m_useContinuous | 0x4c6 | bool |

### The shapes and the contact layer (what a character collides against)

* `dmSphere` {m_p @0x0 vec3, m_radius @0x10 float} — 0x20
* `dmCapsule` {m_p1 @0x0, m_p2 @0x10 vec3, m_radius @0x20 float} — 0x30
* `dmPolytope` {m_centroid vec3, m_vertices, m_planes, m_faceSubEdges,
  m_subEdges, counts, m_volume, m_area, m_lengthScale} — 0x50
* `dmFixture` — 35 fields, 0xc0: `aabb`, `type`, `shapeIndex`, `userData`,
  contact list key/count, `ownerIndex/ownerNext`, `density`, radii, `friction`,
  `restitution`, `rollingResistance`, the explosion/wind/drag/lift factors,
  `aabbMargin`, **`characterPush` @0x8c**, soft-contact frequency/damping,
  `filter`, `proxyId`, `materialId`, `gameFlags`, `boneIndex`, `ownerType`,
  `isSensor`, `useContinuous`, `clientDebugDraw`.
* `dmManifold` — 0x100: `points[4]` (4 × 0x30), `normal` @0xc0,
  `tangentImpulse` @0xd0, `rollingImpulse` @0xe0, `twistImpulse` @0xf0,
  `materialIdB` @0xf4, `pointCount` @0xf6.
* `dmContact` — 0x58: `index` @0xc, `edges` @0x14, `manifolds` @0x30,
  `subIndex`, friction/restitution/rollingResistance, `awakeIndex`, `type`,
  `isSolid`, `isTouching`.

## The 1/60 correction

`systems-plan.md` recorded `0x3C888889` (1/60) as a **candidate fixed tick**.
That candidate is now **closed — falsified**: the function that consumes the
run (`FUN_7ff7c7a5aee0`, exec-translatable) is a **mass-properties
computation** — signed-tetrahedron integration over a mesh's triangles
(volume, first moments, second moments), producing mass, centre of mass and
the inertia tensor — and 1/60 multiplies the accumulated volume there. It is a
**density** (mass per unit volume), not a tick rate. The values around it
(1/50, 1/100) live in the same constants block and are the same kind of
quantity.

The tick rate therefore remains **open**, and the right question is now: who
writes `m_inv_dt0` (`dmWorld + 0x198`)? That writer is the world
constructor/stepper, and it is a small, findable function once the reference
scan over the *wider* code range is run (the m_* string references found so
far cluster in the `0x7ff7c7a4xxxx`–`0x7ff7c7a6xxxx` band, which is the
physics code region — the same band that holds the mass-properties function).
**Answered** (v2b addresses; exec = v2b − `0x71820000`): a displacement scan
for float stores to `+0x198` in the physics band leaves a tight cluster at
`0x7ff7c7a4c2xx`–`0x7ff7c7a4c7xx`, and the decompile shows two functions:

* **`FUN_7ff7c7a4c150` is the writer of `dmWorld.m_inv_dt0`.** Its shape (all
  offsets match the dmWorld map): reads the current `+0x198` and multiplies it
  into the four-lane dt vector; computes `divps(1, dt)` with the same
  lane-valid mask idiom (`movmskps == 0xf`); **stores the result to
  `world+0x198`** when all lanes are valid; calls `FUN_7ff7c7a47aa0(world)`,
  conditionally `FUN_7ff7c7a4a9c0` / `FUN_7ff7c7a4bbe0` (gated on `+0x192`,
  `+0x4c6`), and finishes with `FUN_7ff7c7a7c460(world+0x1b0)` — the
  **broadphase update**.
* **`FUN_7ff7c7a4c730` is the substep driver** the solver chain already
  named: it validates the world's active-body list (indices against the count
  at `world+0x294`, body array at `world+0x288`, **body stride `0x1c0`** —
  the dmBody size — stamping `body+0x1b3`), computes the same `divps(1, dt)`
  into its **local** dt-struct (not into the world), loops `param_5` substeps
  through `FUN_7ff7c7a4b610(world, dt, bulk)`, then clears the stamps. So the
  doc's earlier "writes inv_dt into the timeStep" is the driver's local
  struct; the **world field is written by `FUN_7ff7c7a4c150` only**.

So `m_inv_dt0` is refreshed per step by the world's own stepper, which is also
where the broadphase update is invoked; the substep count and the active-body
validation are the driver's. The world's *constructor* path is read in the
next section.

### The world's constructor and its owner

**The owner chain.** The world is created by a factory, `dmWorld::Create`
(v2b `0x7ff7c7a4cd70`; four call sites: `0x7ff7c54a22a1`, `0x7ff7c6dac688`,
`0x7ff7c6dac69f`, `0x7ff7c7a674dc`). The two adjacent sites in the
`0x7ff7c6daxxxx` band are the game-level one, inside `FUN_7ff7c6dac1b4`: it
takes the world descriptor from `[r14+0x18]`, caches the descriptor's gravity
(`+0x10`) at `[rbx+0x630]`, calls Create twice and stores the two returned
**handles** at `+0x31f8` and `+0x31fa` of the object at `[rbx+0x620]` — so the
game keeps two physics worlds side by side. (The other two callers are
`0x7ff7c54a22a1` and `0x7ff7c7a674dc`.)

**The factory.** `dmWorld::Create(settings)`:

* destroys the previous global world (an import thunk, IAT slot
  `0x7ff7c7cb6ac8`) if the global world pointer at `0x7ff7c90eaac8` is set;
* scans the **handle table** at `0x7ff7c90eaac0` for a free slot: entries are
  obfuscated pointers — `slot = (ptr + 0x4ba63c5a743d751e) ^ 0x87567f996cb1c8b`
  — and a free slot decodes to zero. The table holds 0x3fff slots
  (indices 0..0x3ffe, the resolver's bound);
* allocates **0x4d0 bytes** (align 0x10) and calls the constructor
  `FUN_7ff7c7a47c70(this, settings)`;
* stores the encoded pointer back into the slot, writes the slot index into
  `m_index` (`+0x194`), increments a counter at `0x7ff7c90eaad0`, calls a
  second import thunk (IAT `0x7ff7c7cb6ab8`) with the previous global world,
  and returns the slot as the world's handle.

The per-handle API uses the same decode. `FUN_7ff7c7a4cce0(handle)` is
**ClearForces**: it decodes the handle, returns if the world's `+0x15c` gate is
nonzero, and walks `m_bodies` (`+0x288`, count `+0x298`, stride 0x1c0) zeroing
`force` (`+0x130`) and `torque` (`+0x140`). `FUN_7ff7c7a4d910(handle, x,
settings)` is the per-handle **reconfigure**: it decodes the handle, builds a
context `{world, …}` plus a copy of the settings, and calls the big per-world
re-init `FUN_7ff7c7a46d60` with `world+0x1b0` (the broadphase) — that function
re-sets the three flags from the settings and zeroes `m_inv_dt0` again.

**The constructor** (`FUN_7ff7c7a47c70`, 5,025 bytes, read in full through REA
over a range-capture PE — the v2b dump has its first page zero-filled because
the module maps code lazily). It takes `(world, settings)` and, in order:

* sizes the world's own pool: `settings+0x6c` if positive, else an estimate
  built from every array count (`(sum of counts · stride) · 1.5`, floor
  0x400); constructs the broadphase (`this+0x1b0`, callee v2b
  `0x7ff7c7a7c1b0`), the pool (`this+0x90`) and the block at `this+8`, and
  three allocator stacks at `this+0xb8`/`+0xd8`/`+0xf8` with type ids
  0x100/0x200/0x300;
* installs the shared **physics schema block** at `this+0x170` (v2b
  `0x7ff7c8978228`: pointers to the per-class schema rows — the region that
  holds the `dmWorld`, `dmBody`, `dmFixture`, `dmPolytope` … names with their
  per-class data — followed by the type ids 1..11 and a `"team"` record);
* allocates and **free-list-links every array** from the settings counts,
  with exactly the strides the solver chain measured: bodies 0x1c0, fixtures
  0xc0, contacts 0x58, joints 0xe0, spheres 0x20, capsules 0x30, polytopes
  0x50, and the eight constraint arrays 0x60/0x110/0x120/0x1d0/0x2e0/0x310/
  0x180/0x200. Four more arrays the field map did not list are also built:
  `+0x2a0`, `+0x318`, `+0x330` (stride 0x50), `+0x360` (0x18), `+0x378`
  (0x40), `+0x3d8` (4). Each array is a next-index free list ending
  `0xffffffff`, and the awake-body / awake-contact lists live at
  `+0x480`/`+0x490`;
* the **descriptor's field map**, as the constructor consumes it: gravity at
  `+0x10..0x1c` (copied to `m_gravity`), per-array counts at `+0x20..+0x68`
  (bodies `+0x20`; fixtures `+0x28`; spheres `+0x2c`; capsules `+0x30`;
  polytopes `+0x34`; contacts `+0x40 + +0x44`; joints
  `+0x4c+0x54+0x5c+0x60+0x64+0x68`; the remaining arrays take `+0x24`,
  `+0x38`, `+0x3c`, `+0x48`, `+0x4c`, `+0x50`, `+0x54`, `+0x58`, `+0x5c`,
  `+0x60`, `+0x64`, `+0x68` in allocation order), pool size `+0x6c`,
  contact-cache count `+0x70`, `m_useContinuous` `+0x74`, `m_useSleep`
  `+0x75`;
* copies gravity from `settings+0x10..0x1c` into `m_gravity`
  (`+0x1a0..0x1ac`); zeroes the `+0x15c` gate under a lock; sets the three
  bytes `+0x190`/`+0x191`/`+0x192` to 0/0/1; sets `m_useSleep` from
  `settings+0x75`, **hardcodes `m_useWarmStarting = 1`**, sets
  `m_useContinuous` from `settings+0x74`; zeroes the island revision
  (`+0x18c`); allocates the 0xaa0-stride contact-cache blocks (`+0x4a0`,
  count `settings+0x70`); and finally sets `m_index = 0xffff` (the factory
  overwrites it with the handle).

**The dt answer.** The constructor takes **no dt**: `m_inv_dt0` (`+0x198`) is
zeroed, and the settings struct carries only counts, gravity, the two flag
bytes, the pool size and the cache count. So the world does not pick a tick
rate at construction — its `1/dt` is written **per step** by the stepper
(`FUN_7ff7c7a4c150`) from the dt its caller passes, which is where the
player's tick rate actually enters.

**Correction: the world has no vtable.** The constructor installs none, the
factory installs none, and the object is a plain 0x4d0-byte struct — which
also matches the last documented field (`m_useContinuous` at `+0x4c6`). The
vtable earlier rounds attributed to `dmWorld` (`0x7ff7c8192710`, "stored next
to the schema name") belongs to small descriptor classes instead: its three
installers are deleting destructors with object sizes **8** and **0x88** and
an embedded subobject at `+0x3160`, and the function stored beside the
`dmWorld` name (`0x7ff7c6da75c0`) is the 8-byte one. The heap objects found
by scanning for that vtable were therefore not worlds — which explains the
"world frame does not close" symptom (plausible gravity, `m_inv_dt0` ±0.0)
recorded in the false-positive lessons above.

### The step call site: 1/30 from the module table

The chain from the frame tick down to the stepper, now read end to end in v2b
coordinates (exec = v2b − `0x71820000`):

```
FUN_7ff7c6e0fc70  (system tick; step context at [param+0x40])   [exec 7ff7555efc70]
  └─ FUN_7ff7c7a68d80(stepCtx, dt, maxIter, 3.0)                [exec 7ff756248d80]
       loop while residual < 0.5 and iter < maxIter:
         ├─ FUN_7ff7c7a4dbb0(handle, dt, posIters, velIters, flag)  → stepper FUN_7ff7c7a4c150
         └─ FUN_7ff7c7a4dcb0(handle, dt, posIters, velIters, flag)  → driver  FUN_7ff7c7a4c730
```

* **The two wrappers** are per-handle and structurally identical: bounds-check
  the handle (`< 0x3fff`), decode it through the handle table, take the
  world's `+0x15c` **busy flag with a try-lock** (read, set 1, and only step if
  it was 0 — a world cannot be stepped re-entrantly), broadcast the scalar dt
  into a 4-lane vector, call the stepper/driver, then release the flag. This
  also explains `ClearForces`: it refuses while `+0x15c` is set, i.e. while a
  step is in progress. The wrappers' iteration counts come from the step
  context at `+8`/`+0xc` — the **8 position / 3 velocity iterations** the
  solver chain recorded.
* **The step entry** (`FUN_7ff7c7a68d80`) takes dt as a plain float argument
  and passes it unchanged to the wrapper, to the residual query
  (`FUN_7ff7c7a68840`) and to the loop condition: it repeats while the
  residual is below **0.5** (`DAT_7ff7c7d0c068`) and the iteration count is
  below `maxIter`. The world handle is `*(u16*)(stepCtx+0x58)`.
* **The tick** (`FUN_7ff7c6e0fc70`) calls it with the step context at
  `[param+0x40]`, `maxIter = max(*(int*)(param+0xef8), *(int*)(param+0xefc))`
  (two counters on the tick object, so the substep count is dynamic), `3.0`
  (`DAT_7ff7c7d0c070`), and **dt = `DAT_7ff7c7d5d84c` = 0.0333333351 = 1/30 s**
  — the module dt table's `+0x0c` entry, read from the shipped image. So the
  player's physics runs in **fixed 1/30-second steps**, hard-coded from the
  module's table rather than taken from frame time, with the number of steps
  per frame decided by the tick object's counters. (The same table's
  `+0x04`/`+0x08`/`+0x10` are 1/365, 1/60, 1/24 and `+0x14` is 0.4666667.)
* The tick's own caller is `FUN_7ff7c6d92691` (one call site), the next level
  up in the frame.
* **The sim adopts this stride.** `ow-sim`'s `movement::TICK_RATE_HZ` is now
  **30** (it was a 60 Hz placeholder), so the controller integrates the body on
  the client's own fixed step; `ow-game`'s fixed-tick shell takes its `DT` from
  the same constant. The controller's fitted ramps are per-second rates, so the
  ramp timing is unchanged — only the integration cadence now matches the
  client's one write per 1/30 s.

### How many 1/30 s steps run per frame

The tick object and its counters are now read, and the answer is: **one
1/30-second integration per frame**, with a startup catch-up and a residual
loop that can extend it.

**The tick object.** Fields the three functions use: the step context at
`+0x40`; a **pair of dwords at `+0xef8`/`+0xefc`** (written together as one
64-bit value, so they are configured as a unit); a flags byte at `+0xf08`
(bit `0x10` = "steps pending", bit `0x20` set by the startup path); a mode int
at `+0x1b4`; and a child list reached through `[+8]+0x2a8` (per-child flag
bit 48 at `+0x280`, back-pointer at `+0x268`).

* **The frame calls the tick at most once.** The frame-level
  `FUN_7ff7c6d92691` (one call site, from `FUN_7ff7c6d77190`) runs three other
  per-frame systems and then calls the physics tick **once**, gated on
  `(+0xf08 & 0x10) != 0` and `+0x1b4 == 1`. There is no frame-time
  accumulator: the number of steps is decided by the counters, not by
  elapsed time.
* **The tick's step count** is `max(+0xef8, +0xefc)`: it passes that as the
  step entry's iteration cap, so at least the larger of the two. `+0xef8` is a
  **pending budget** — `FUN_7ff7c6e0fb50(tick, 0)` (the tick's own trailing
  call) clears the pending flag and zeroes it, so it is consumed by the tick
  it raised. `+0xefc` is the other half of the configured pair; the tick's
  constructor `FUN_7ff7c6e028ec` writes both from one 64-bit argument (and the
  flags byte from another), so the *configured* value comes from whoever
  builds the tick object. That constructor has no plain reference in the
  captured pages (no qword and no `lea` — obfuscated pointer table or a cold
  page), so the literal configured number is the one piece still unread; the
  mechanism below fixes its effect.
* **Startup catch-up.** `FUN_7ff7c6e186d5` preloads `+0xef8 = 150` — **five
  seconds at 1/30** — when `+0xefc <= 0` and a global force flag is set, then
  calls `FUN_7ff7c6e0fb50(tick, 1)` to raise the pending flag. So the first
  tick after that point may run up to 150 integrations.
* **The residual loop.** Each iteration of the step entry calls the wrapper
  (stepper or driver — the active-list branch picks which) with the same
  `dt = 1/30`, then asks the residual query (`FUN_7ff7c7a68840`) and repeats
  **while the residual is below 0.5** and the iteration count is below the
  cap. Multiple iterations therefore integrate 1/30 each (catch-up by
  repetition), not a subdivided step.
* **The hierarchical part.** `FUN_7ff7c6e0fb50` also walks the tick's child
  list and, for each child whose time value (`FUN_7ff7c7a68d30`) is behind
  the parent's, recurses into it — so sub-worlds (the second physics world the
  game keeps, and per-entity worlds) catch up to the same budget.

Net reading: in a normal frame the client's physics advances by exactly
**1/30 s**, once, inside the single gated tick call; the substep machinery
exists to finish the position residual (0.5) and to burn the startup budget
(150 = 5 s), not to scale with frame time.

### Who builds the tick object, and the pair's runtime value

The tick object's construction and the configured pair are now read as far as
the captures allow, and the pair's value was taken from a live object.

* **The object is 0xf10 bytes**, allocated by `FUN_7ff7c6918e00(0xf10)` and
  freed by `FUN_7ff7c6918e60(ptr, 0xf10)`. The frees appear in three *owner*
  destructors (`0x7ff7c6d9fee0`, `0x7ff7c6da0cac`, `0x7ff7c6da4d80`), each of
  which also clears its field at **`+0x128`** — so an owner holds its tick
  object at `+0x128`, and at least three owners have one (the world manager
  and two siblings).
* **The builder is `FUN_7ff7c6da3da4`** (an owner method, ~0x1100 bytes): it
  allocates the 0xf10 bytes, calls the constructor `FUN_7ff7c6e028f0`
  (105 bytes) with only `rcx` set, binds the new object
  (`FUN_7ff7c6e08510(new, owner, &owner->+0xdc)`), stores it at `owner+0x128`
  and increments a count at `owner+0x130`. The constructor writes the pair
  from **`rsi` = the builder's second argument** and ORs its flags byte
  (`al`) with 0x40 — so the configured pair is a *runtime argument* of the
  builder, not a constant.
* **The static-reference negative.** None of the three functions — the
  builder, the constructor, or the frame caller — has an `E8`, `E9`, qword or
  `lea` reference anywhere in the captured pages. They are reached only
  through runtime-built tables, the same pattern the mover's registrar
  showed. The literal configured pair therefore cannot be read statically
  from this capture set.
* **The pair's value, read from the live object instead.** Exactly one object
  in all 2,437 MB of the v2b heap capture matches the tick object's full
  signature (step context embedded at `+0xf0` with `+8`/`+0xc` = 8/3 and
  world handle 0 at `+0x58`, mode at `+0x1b4`, flags byte at `+0xf08`, the
  pair, and the `+8`/`+0x268`/`+0x280`/`+0x2a0`/`+0x2a8` list fields):
  `A = 0x21a3a7f90f8`, with **`+0xef8 = 2` (pending) and `+0xefc = 0` (cap
  unconfigured)**. Its `+0xf08 = 0x98` has **bit `0x10` set** — exactly the
  flag the frame gate tests, and consistent with the keeper's rule ("set
  bit `0x10` iff `max(pending, cap) > 0`": 2 > 0). Its mode `+0x1b4` was 0, so
  the frame gate would have skipped the tick at capture time.
* **No arithmetic on the pair exists anywhere in the image** — no `add`,
  `inc` or `sub` with displacement `0xef8`/`0xefc`. The pair is
  **assignment-only**: the constructor's argument, the 150 startup preload,
  and (at runtime) further assignments from code outside the captured pages,
  which is where the observed `pending = 2` must have come from. So the tick
  count is *commanded*, not accumulated — there is no frame-time accumulator
  even now that the pair's producers are known to be assignments.

The step context's placement is a layout fact worth keeping: it is
**embedded in the tick object at `+0xf0`** (the tick's `+0x40` points to its
own `+0xf0`), and it is the object that carries the world handle (`+0x58`)
and the iteration counts (`+8`/`+0xc` = 8/3), and the object the residual
query (`FUN_7ff7c7a68840`) is given.

### The pair's producers: the cap is a duration × 30, the pending is a budget

Both halves of the pair now have named producers, and the answer came from the
**range** capture: the cap's only store in the whole module lives in a page
that is cold in the v2b image (`FUN_7ff7c6e08510`, the bind call the builder
makes), which is why earlier displacement scans over v2b found nothing.

**The cap (`+0xefc`) has exactly one store in the module** — inside the bind
function, and it is computed, not configured directly:

* the owner's **step-config block** is at `owner+0xdc`; the bind copies it into
  the tick at `+0x18c` (16 bytes), `+0x19c` (16 bytes) and `+0x1ac` (dword);
* the cap = **`ceil(config.float@+0x10 × 30.0 × 1.0)`** — the two constants
  read from the image are **30.0** (v2b `0x7ff7c7d20898`) and **1.0** (v2b
  `0x7ff7c88c9dcc`). So the config field is a **duration in seconds** and the
  cap is that duration converted into **1/30-second steps**;
* the same bind also stores the owner at `tick+0x10`, installs a 16-byte
  helper object at `tick+0x48` (table v2b `0x7ff7c81bf1b0`, back-pointer to
  the tick at its `+8`) and clears bits 0..2 of the second flags byte
  (`+0xf09`).

**The pending (`+0xef8`) has three producers and one consumer, all counted:**
the constructor (the builder's second argument — the initial budget), the
startup preload (`FUN_7ff7c6e186d5`: 150 = 5 s × 30 when the cap is ≤ 0 and
the global flag at `0x7ff7c88231b0`, which is **statically initialized to 1**
in the module data, is set), and construction-time clears; the consumer is the
keeper, which **zeroes it** after the tick (assembly-verified: the store is
`mov [rcx+0xef8], r12d` with `r12d = 0`, so there is no subtraction anywhere —
the pending is a one-shot budget).

**How the tick then steps every frame.** Since the tick runs
`max(pending, cap)` integrations, the **cap acts as a floor**: a configured
duration of one 1/30 s gives cap = 1 and therefore **exactly one 1/30 s
integration per frame**, while a nonzero pending adds catch-up steps and is
consumed. With the duration unset (cap = 0) and no pending, the tick does not
step at all — which is what the captured session showed (cap 0, pending 2,
mode 0), and what the 150-step preload exists to paper over at startup.

**The keeper's exact semantics** (assembly, not decompiler output): if the
tick has no step context it returns immediately; with flag ≠ 0 it raises bit
`0x10` of `+0xf08` iff `max(pending, cap) > 0`; with flag = 0 it clears the bit
and zeroes the pending; then it walks the manager's child list
(`[[tick+8]+0x2a8]`, next at `+0x2a0`, gate bit 48 at `+0x280`) and, for each
child that has a tick object (`[child+0x268] − 0x20` → `+0x128`) with a step
context, compares the two contexts' time values (`FUN_7ff7c7a68d30`) and
recurses into the child when the child's value is the greater one — the
sub-world catch-up.

**Capture lesson recorded:** "no store in the captured pages" must always name
the capture. The cap's producer is invisible in v2b and visible in range; the
bind function, the builder's callees and the frame functions are reached only
through runtime-built tables, so a *third* capture (or a live dump of the
relevant pages) is what any further producer hunt needs.


## What this unlocks

* **Lane B can now decode live client bodies.** A heap scan for a `dmWorld`
  instance (or any body) can be typed field-by-field: `linearVelocity` at
  `+0x30`, `gravityScale` at `+0x16c`, `characterMover` at `+0x1b6`,
  `m_gravity` at `+0x1a0`. The trace differ's "extra" fields can be assembled
  from this table.
* **The movement consumer hunt is now an offset-based search with real
  offsets.** `dev/sweep_consumers.py` (which failed because it had to guess
  displacements) can be re-run against the true dmBody displacements — the
  mover that writes `linearVelocity` (`+0x30`) with input terms is the target.
* **The shape layer names what a character controller collides against**:
  capsule shapes (`dmCapsule`, p1/p2/radius), `characterPush`, `filter`,
  `ownerType` — enough to reconstruct collisions approximately even before
  the contact solver is read.

## Locating bodies in a live client (and three false-positive lessons)

Finding the player's live body went three rounds, each recorded so it is not
re-walked:

1. **Shape-only body scanning floods.** The dmBody field map is all floats, so
   any invariant set that accepts a real body also accepts bone/transform
   pools and uniform filler: 4,000 candidates in the first 2 GB of a live
   process, none real (`(0.5,0.5,0.5,0.5)` has unit norm; `1.0`/`1.0` pairs
   satisfy the mass window). Dead end as a *primary* locator.
2. **Value-based world scanning floods worse.** A signature on the world's
   field *values* (m_index small, m_inv_dt0 in range, gravity mostly-y, flag
   bytes) produced 29,079 candidates over the v2b capture and zero genuine
   worlds. Single float fields cannot discriminate; every value exists
   everywhere.
3. **Structural tree scanning finds trees, not necessarily the world.**
   `dmBroadPhase` is inline in dmWorld at `+0x1b0` with four inline
   `dmDynamicTree` instances (`0x30 + 4·0x28 = 0xd0`, exactly the distance to
   `proxyCount`), and tree invariants (`count ≤ capacity`, power-of-two
   capacity, `freeList/root < capacity`, `proxyCount ≤ count`, node AABB
   `min ≤ max`) are strong. The v2b scan found 782k tree-shaped sites, and one
   physics-plausible tree (`cap=65536, proxy=5052`, nodes array in heap) —
   but the world frame around it does not close (`m_inv_dt0` reads ±0.0,
   container counts inconsistent). dmDynamicTree is generic: renderers and
   audio use spatial trees too, so a tree is not proof of a physics world.

The lesson that stands: **the reliable locator is behavioural, not
structural** — sample candidate state twice and keep what moves with
walking-speed consistency (the three-sample `--watch` in `dev/live_trace.py`),
then verify against the player's known motion. Shape and value signatures are
useful only *after* a behavioural anchor says where to look.

The other half of the lesson is coverage: a scan that stops at a candidate cap
before covering the address space reports absence it never tested. Both scans
now run to completion (or a documented MB cap) before concluding anything.

## Ghidra housekeeping (module_v2full.bin)

Created this session: `FUN_7ff7c7a38c40` (dmBody descriptor getter),
`FUN_7ff7c7a38cbf` (dmBody field-array filler — the record layout's
authority), `FUN_7ff7c7a398b0` (dmBody hook: `*(this+0x150) = 0`),
`FUN_7ff7c7a4d910` (per-handle reconfigure). `FUN_7ff7c7a5aee0` =
mass-properties computation. The program has unsaved changes; save it when
the bridge is responsive.

New tool from the tick round: `dev/find_tick.py` (gitignored) — a two-pass
heap scan that collects step contexts (`+8`/`+0xc` = 8/3) and then finds
objects whose `+0x40` points at one, printing the candidate's whole field set;
it also supports the sharper self-embedded-context signature
(`[A+0x40] == A+0xf0`) that produced the unique tick object in the v2b heaps.

The constructor round added a second target: **REA over a range-capture PE**
(`dev/work/r_phys.exe`, `mkpe.py`, base `0x7ff64cb86000`, window
`0x7ff64cb86000..0x7ff64cb8e000` sliced from `module_mapped.bin`), because the
v2b dump has the physics constructor's first page zero-filled — the module
maps code lazily, so a function can be half-present in one capture. Range
addresses translate to v2b by `+0x17AEC0000` and to exec by `+0x71820000`;
the constructor is `0x7ff7c7a47c70` in v2b and `0x7ff64cb87c70` in range.

Artifacts (gitignored): `dev/dump_domino_schema.py` (the registry reader),
`dev/dump_domino_desc.py`/`dump_domino_fields.py` (single-descriptor probes),
`dev/domino_schema.txt` (the full 21-class dump), `dev/outline_sites.py`.
`dev/find_refs.py` now honours `OW_DUMPS` like the rest of the toolchain.

## The mover's interfaces and the volley's registrar (a narrowing session)

Four probes in one session, all against the exec capture (`module.bin`, base
`0x7ff753000000`, 70,839 functions; range translation `+0x1096A0000`). Three
are negative results with a now-complete answer; the fourth opens the string
table.

### The mover's three vtables are entirely infrastructure

The constructor (`InitializeMoverComponent`, exec `0x7ff7546622e0`) installs
vtables at `+0`, `+8`, `+0x48`; in the range capture they sit at
`0x7ff64d0431f8`, `0x7ff64d0430c8`, `0x7ff64d043160` (= exec `0x7ff7566e31f8`,
`0x7ff7566e30c8`, `0x7ff7566e3160`) and hold **real pointers** (the v2b heap
copy of the same region reads obfuscated — do not read these from v2b).
Every mover-specific slot decompiles to infrastructure:

| slots | function(s) (exec) | reading |
|---|---|---|
| 1–3 (per vtable) | `0x7ff7541def10`…`f480`, `ef50`/`ef60`/`ef80` | `return &DAT_…` — descriptor getters |
| 4 | `0x7ff7541dc230` (+`1dc140`, `1dc170`, `1dc1d0`, `1dc200`, `1dc290`, `3e43620`) | deleting destructors; the mover's frees **0x130** — the STU size exactly |
| 19–25 | `0x7ff7541dc140`…`1dc290`, `1df5d0`/`f600`/`f660` | registration-instance membership: `[obj+0x88] == table-entry`, or walk the instance table |
| 6, 7, 9–12, 16–18 | `0x7ff75572aa60`, `0x7ff753a4a220`, `0x7ff75572a7d0`/`a8d0`/`ae80`/`b150` | vtable-forwarding shims (`call [[*this]+0x28]` then a shared helper) — the component-base interface, shared across vtables |
| 8 (per vtable) | `0x7ff754662660`/`662670`/`662770` | the per-class introspection attach (Phase 4f) |

Two shared slots (`0x7ff753a51700`, `0x7ff75572a8d0`) are not function-defined
in `module.bin`'s analysis (a flatten gap, not a capture gap); the pattern of
their neighbours makes them shims too, but they are recorded unread rather
than assumed. **The mover class computes nothing with its own fields** — no
update, no accessor that touches gameplay state. Together with the identity
census (Phase 4h) this closes the class-route question: the consumer is not
reachable through the component, its vtables or its descriptor.

### The `numShotsPerSecond` site is the registrar, not the fire loop

`docs/rewrite-plan.md` Q5 banked the one `0x02F3F5D4` code site as the fire
loop's anchor. Read out, exec `0x7ff754ddf1a1` is the volley's **field
registrar**:

```
lea  rcx, [0x7ff757728fa8]      ; the field record being filled
mov  r8d, 0x2F3F5D4             ; numShotsPerSecond
xor  r9d, r9d
mov  [rsp+0x20], 0x1C0          ; slot
call 0x7ff75573c7a0             ; register
mov  ecx, 0x1C8 … XOR-decode name blob at 0x7ff756fa945c
mov  r8d, 0xA55BC904 ; [rsp+0x20] = 0x1C8 ; call   ; the next field
```

So the slot pairs are readable straight off the registrar (this is the
`volley_slots.txt` source), and the name blobs at `0x7ff756fa9450`-style
records (`{key[8]; u32 count|flag<<24; enc[count]}`) decode the field names.
The fire loop does **not** read the field by hash — gameplay reads the slot —
so hash-immediate searches find reflection code, by construction. The
`castLength` sites (`0x7ff754df43ab`, `0x7ff754df9494`, `0x7ff754dfd1a5`) are
the same story: a descriptor-getter dispatcher (`cmp ecx, 0xFEC435C6; jz →
lea rax,[0x7ff75732d060]; ret`).

### The physics-arithmetic sweep (tooling, unread candidates)

Two new sweep tools instrument the offsets rather than the classes:

* `dev/sweep_physics.py` — ranks exec functions by co-occurrence of the real
  dmBody/dmWorld displacements (`+0x30` velocity, `+0x16c` gravityScale,
  `+0x170` linearDamping, `+0x1b6` characterMover, `+0x198` `m_inv_dt0`,
  `+0x1a0` gravity) → `dev/physics_funcs.txt` (39,043 functions touch at least
  one; the top rows write the whole struct — reflection initialisers).
* `dev/sweep_mover_body.py` — the intersection of mover movement slots and
  physics-body fields → `dev/mover_body_funcs.txt` (7,375 functions; top rows
  again write `FLT_MAX` fill patterns).

Filtering `physics_funcs.txt` for genuine float arithmetic (`mulss`/`addss`/
`mulps` on velocity/gravity/damping, not immediate stores) yields a shortlist
of ~20 candidates, unread as of this session. Two were decompiled and are
**not** the integrator: `FUN_7ff7562702a0` (a 4×4 rotation-inertia solver) and
`FUN_7ff755e3ce70` (a large body-state/transform builder). The remaining
shortlist is the next round's lead; the discipline that made it is the filter,
not the offset list.

### The string table: `{u64 hash; u32 len; chars}`

Module data carries hashed string records — `u64 hash`, `u32 length`, then the
plaintext bytes. Verified on pairs:

| string | record (range) | hash |
|---|---|---|
| `player.GenerateRandomMovement` | `0x7ff64d7d7e08` | `0xED0E91CE41B5AA6D` |
| `SPACE` | `0x7ff64d7d7e78` | `0xB4803E093906F3F7` |

Movement-relevant names in the same table (future anchors): 
`player.GenerateRandomMovement` ("For local player generate movement
commands", `0x7ff64d7d7e14`), `debug.DisableErrorCorrection` ("Disable
movement error correction for local players", `0x7ff64d7de6ec`),
`camera.moveSpeed`, `input.MouseScaleX/Y`, `input.ForceInputMode`,
`ECSingletonInput_C WorkingBindings Input Handler`.

The hash function is **not** FNV-1a/1 (32 or 64), Jenkins one-at-a-time,
Jenkins `hashlittle`, CRC32, or xxhash64/32 — tested against both records and
against the known STU field hashes (`projectileMotions` `0x351BD93F`,
`numShotsPerSecond` `0x02F3F5D4`). Recorded so it is not re-tried: the
name→immediate bridge is not yet available, and the config-var storage route
(record → storage pointer → consumer) does not surface as a qword pointer in
the capture (scanned for both name strings; only the `Movement` service-table
record at `0x7ff64db0ffe8` references a name pointer, and it is a different
table).

### New tooling this session

`dev/sweep_physics.py`, `dev/sweep_mover_body.py` (above);
`dev/physics_funcs.txt`, `dev/mover_body_funcs.txt` (gitignored outputs). The
Ghidra program `module.bin` gained the mover interface family
(`0x7ff7541dc140`–`0x7ff7541df660`) and the `0x7ff7561cbe00` copy-constructor
as function definitions, bookmarks on the volley registrar and the mover
vtable, and was **saved**.

## Q2 and Q4 answered: the solver chain, the integration arithmetic, and the dt table

A decompile round following the island/solver structure of the Domino library
(exec capture `module.bin`; range = exec − `0x1096A0000`). The chain, from the
top:

```
FUN_7ff7555efc70  (a system tick; reads a physics-world handle at +0x40)
  └─ FUN_7ff756248d80(world, dt = DAT_7ff75653d84c, maxSubsteps, 3.0)
       loop: FUN_7ff75622dcb0(worldIndex, dt, 8, 3, 1, activeList) while residual < 0.5
         └─ FUN_7ff75622c730   (solve driver: validates the active-body list,
         │                      writes inv_dt = 1/dt (4-wide divps) into the
         │                      timeStep, flags from world+0x4c5 useWarmStarting,
         │                      loops the substeps)
         │    └─ FUN_7ff75622b610  (island builder: walks contact/joint lists,
         │         │                stamps bodies with world+0x18c revision,
         │         │                collects islands)
         │         └─ FUN_7ff756261300  (island Solve — the integration lives here)
         │              └─ FUN_7ff756242d00  (per-constraint dispatcher, cases 1–9)
         │                   └─ per-type solvers (FUN_7ff75626b840/26bc30/26cbe0/
         │                      26d3e0/2702a0/2735f0/2767a0/2781a0)
         └─ FUN_7ff75625e790   (a character-shaped step: positionIterations 8,
                                velocityIterations 3, dt from param_3[8])
```

**Runtime layout facts confirmed from the code, not the registry** (island
builder, `FUN_7ff75622b610`): `m_bodies` at world+0x288 with stride **0x1c0**;
body fields `invMass` +0x168, `invI` +0xf0, `force` +0x130, `torque` +0x140,
`linearVelocity` +0x30, `angularVelocity` +0x40, `islandStamp` +0x190,
`isAwake` +0x1ac, `isFast`/`isSubset` +0x1b3, `sleepTime` +0x17c; fixtures at
world+0x2b8 (stride 0xc0), contacts at world+0x348 (stride 0x58), joint arrays
at world+0x390 (stride 0xe0); constraint arrays for the dispatcher at
world+0x3a8/0x3c0/0x3f0/0x408/0x420/0x438/0x450/0x468 with strides
0x60/0x110/0x120/0x1d0/0x2e0/0x310/0x180/0x200. A byte flag at body **+0x1b7**
gates the inertia/rotation block (beyond the registry's last field; the
registry is not the whole struct).

### The velocity integration, in the client's own order

From `FUN_7ff756261300`, per dynamic body (assembly at `0x7ff7562616e0`; this is
the authority, the decompiler's left-to-right rendering differs):

```
movss  xmm5, [body+0x16c]      ; gravityScale
mulss  xmm5, [body+0x164]      ; × mass                       → t1
movss  xmm0, [body+0x168]      ; invMass
shufps xmm5, xmm5, 0           ; broadcast t1 to 4 lanes
shufps xmm0, xmm0, 0           ; broadcast invMass
mulps  xmm0, xmm11             ; invMass × dt (4 lanes)        → t4
mulps  xmm5, xmm14             ; t1 × gravity (4 lanes)        → t2
addps  xmm5, [body+0x130]      ; + force                       → t3
mulps  xmm5, xmm0              ; × t4                          → t5
addps  xmm5, [body+0x30]       ; + linearVelocity              → v'
```

So, per component `i` (four lanes; lane 3 is the pad):
`v'_i = ((gravityScale·mass · g_i + force_i) · (invMass·dt_i)) + v_i`
with `gravityScale·mass` computed once as an f32 scalar (`mulss`), and
`invMass·dt_i` a precomputed 4-lane product. **Six roundings in this order** —
a fused or reassociated version will drift within a few ticks.

Angular velocity integrates in the same block: `ω' = ω + dt · (invI · torque)`
with the 3×3 inverse-inertia product decomposed into `shufps` × row vectors.

### Damping is linear, not Box2D's `1/(1+dt·d)`

Immediately after, per body (the same function):

```
factor_linear  = clamp(1 − linearDamping  · dt, 0, 1)   ; min with 1.0, max with 0.0
factor_angular = clamp(1 − angularDamping · dt, 0, 1)
v'' = factor_linear · v'      ω'' = factor_angular · ω'
```

`1 − d·dt`, `minps` with the unit vector, `maxps` with zero (the same ordering
appears for both), then the 4-lane multiply. This is a **different formula from
b2World's** and is the kind of thing a fitted model gets wrong by exactly the
second-order term.

### Position integration, quaternion renormalize, displacement clamp

The tail of the same function integrates the island's scratch positions:
`x += dt · v` per lane; `q += 0.5·dt·(ω × q)` written out per component, then
`if (|q|² > 1.17e-35) q *= 1/sqrt(|q|²)` (numerator `1.0`, threshold
`DAT_7ff7566e39a8`); velocities whose per-step displacement squared exceeds
**16.0** (`DAT_7ff756922400`) are clamped to unit scale before the update —
a 4-metre-per-step translation limit. A sleep pass accumulates `sleepTime`
(body+0x17c) against `DAT_7ff7564ec068` = **0.5 s** and other consts at
`0x7ff756c0b260` (0.00761543587 — five identical lanes).

### The dt table, and which step uses which

Module data (`0x7ff75653d840`, exec):

| offset | value | reading |
|---|---|---|
| +0x04 | 0.0027397261 | 1/365 |
| +0x08 | 0.0166666675 | **1/60** (11 code refs) |
| +0x0c | 0.0333333351 | **1/30** (54 code refs) — the value the system tick passes into the step entry (see "The step call site" below) |
| +0x10 | 0.0416666679 | 1/24 |

The character-shaped step (`FUN_7ff75625e790`) instead takes dt from its caller
and solves with **positionIterations 8, velocityIterations 3**. The substep
driver uses a residual threshold of 0.5 and a max-substeps argument of 3.0.
Which of these runs for the *player's* mover is the remaining link (Q3): the
mover control (input → velocity) is not in this chain.

### Config-var anchors for the tick

The config string table (range `0x7ff64d7d0000`–`0x7ff64d945000`, records
`{name ptr, desc ptr, u64, u64 value}` with stride 0x20; e.g. the
`tank_TournamentModeTickRate` record `0x7ff64d937c58`, name `0x7ff64d936854`,
desc "Start up this application using tournament mode tick rate", value 0) also
carries `client.MaxFPS` ("Set the max framerate of the client simulation"),
`app.IgnoreDormantMode` ("Turns on and off effects of dormant mode, eg slower
tick rate"), `player.GenerateRandomMovement` ("For local player generate
movement commands") and `debug.DisableErrorCorrection` ("Disable movement error
correction for local players"). Their consumers are the next round's anchors:
each is a debug/limit switch on the tick or on the mover path.

One hash-join for the naming side: the field-default provider at
`0x7ff7546243xx` maps hash `0xE9566B91` → `60f` and the OWLib class file
confirms `STU_85B2644A.m_E9566B91 = 60f` (offset 96, bit-exact) — the provider
and the class file agree, so provider reads can be named by joining hashes
against the OWLib checkout.

### The mover control: still not found, and why not

Two more sweeps this round, both recorded as the method's false-friend list:

* functions reading a movement slot with arithmetic **and** storing a velocity
  (19 hits) — the survivors are curve/spline code: e.g. `0x7ff754203000` reads
  `[rsi+0xc0]`/`[rsi+0xbc]` but `rsi` is a `{count, vec3[]}` spline object, and
  `0x7ff755e1d6c0`'s "velocity store" is a B-spline subdivision over an array
  of 12-byte vec3;
* functions combining the **jump struct** with speed slots (10 hits) — the
  same curve families again.

The lesson from `docs/systems-plan.md` holds one level deeper: the mover's
displacements (`0x50`–`0x64`, `0xac`–`0xf0`) are exactly the displacements
curve and list code uses, so displacement co-occurrence cannot separate them.
The mover control needs either the operator's live trace (Lane B) or a
watchpoint-class capture on the player's `dmBody`; the config-var consumers
(`app.IgnoreDormantMode`, `client.MaxFPS`) surface only as registration
helpers (XOR name blobs + register call), not as readers, so they are not the
entry point either. Recorded so the next round starts from the trace, not from
another displacement sweep.

### The world is not in the full-process capture, and the mover control needs a trace

Two more attempts, both with strong signatures rather than displacement
sweeps, both negative:

* **dmWorld in `dumps_v2b`.** A scan of all 32,018 regions (10.6 GB) for a
  u32 equal to a plausible `m_inv_dt0` **rate** (60/30/62.5/24/…, the field is
  inv_dt, not dt) with a gravity triple at +0x1a0 found 46 candidates, every
  one a coincidental `-1.0` neighbour. The world instance is not in that
  capture's regions — so `m_gravity` and the live tick cannot be read out of
  it, and the gravity *value* the client uses remains open (the formulas that
  consume it are extracted; the constant is not).
* **The mover control.** Displacement co-occurrence is now exhausted as a
  method for it (19 hit families all curve/spline code, `[rsp+…]`-collision
  noise, or schema writers), and the config-var names surface only at their
  registration sites (XOR-decoded name blobs + register call), never as
  readers.

The unblock is one operator run: `dev/live_trace.py record` now writes the
**key timeline** with each sample, so a single walking/jumping trace is a
complete `(input, state)` recording; `diff_trace_keys` replays it script-free
and the ramps are fitted from the velocity response (`docs/rewrite-plan.md`
Lane B). The `1/30` vs `1/60` question is answered by the same trace's
timestamps.

### Live-session findings: the module maps code lazily, and the mover is config

A live attach to the running client (build 2.25.0.0.154088, pid 11592) settled
four things that had been open, and killed the entire family of data-shape
searches for the mover control. Recorded here so no later round repeats them.

**The module maps code lazily, page by page.** The image is `MEM_MAPPED` at
`0x7ff70e520000` (standing-capture base `0x7ff649e60000`, uniform delta
`+0xC46C0000`, re-verified byte-identical). Inside the allocation, pages that
have not executed since the match started are `PAGE_NOACCESS`: of 77 MB,
only ~15 MB of RX code and ~13 MB of R data are reachable; everything else is
committed-but-unreadable. `ReadProcessMemory` on those pages fails, so a
process-wide byte search simply does not see them — which is exactly what the
earlier "0 hits" for the volley registrar (`41b8d4f5f302…`) and the mover
provider (`b00181fa02466b5c77…`) were: not a content difference, but a cold
page. Executed pages read back byte-identical to the standing captures
(`live 0x7ff70f1fef20 == range 0x7ff64ab3ef20 == 48 8d 05 b9 19 14 03 c3`).
Any live verification must therefore target pages the game actually runs
during the session; anything else has to come from a standing capture.

**The 224 live `STUCharacterMoverComponent` instances are configuration.**
A vtable-pointer scan of the object heap (`0x024080000000+256 MB`, three
vtables discovered earlier) lists 224 components; each has exactly one
referrer in a small window after it (the owner field). With movement
*screenshot-verified* (scancode injection, see below), **zero u32 changed** in
the components, in their owners, or in one pointer level out of the owners
(527 KB watched at 40 Hz through idle → hold S+D → idle → hold W → idle).
Whatever holds the live character state, it is not reachable from the mover
component in one hop — the component carries the config (`jump=(20,20,0.98,
0.7,0.7,0.2)`, `f@D0=(6.2,2,0,5.5)`) and nothing moves at all.

**The live heap's moving-data false friends**, catalogued so they stop
matching future motion searches:

* range targets: records shaped `(0, ±1, railPos, height)` whose third float
  ramps at 4.6 m/s — periodic, pauses at the ends (a 0.5 s idle gate admits
  them, a 2.5 s one does not);
* bot bone transforms: `yaw-only basis (cos,0,sin / 0,1,0 / -sin,0,cos),
  pos, pos-copy at +0x10`; the root pair is bit-frozen (e.g. `(64.2461,
  7.0422, -30.6250)`) while the head pair breathes (`y ±0.033`, period 1.9 s);
* renderer palette slots with the same pos-copy shape: content is rewritten
  with different actors' transforms mid-run (one slot read 75 m of
  "movement" in 3 s and then teleported), so displacement over multi-second
  windows is meaningless for them;
* a 1.09 TB private heap (`0x00fda0800000+`) full of map-coordinate triples
  (`(56.704, 2.098, 25.592)`) amid float tables — value-shape scans drown in
  it (943,878 raw "walking velocity" candidates over 13 GB).

**Input instrumentation works and is verified.** `keybd_event` with scancodes
plus the `AttachThreadInput` foreground dance moves the player — backpedal
`S` over 4 s pans the view across the whole range (screenshot-verified), and
all hunts above were run under that verified movement. CUA `click` fires the
weapon (ammo 6→4→2 observed) but does **not** rotate the camera (cursor
warps carry no raw deltas), so mouse-look cannot be driven from outside; turn
the player with movement keys instead.

**RX-by-key coverage is real but needs a quiet baseline.** Because pages
become readable exactly when executed, diffing the module's RX map around a
keypress names the pages that key ran. Pressing `d` after a 2 s idle lit 38
new pages (1.16 MB) against 12 ambient regions (369 KB); the first page
examined turned out to be GPU/device config init (Intel-vendor check
`0x8086/0x4f6` + a config global wall), i.e. ambient churn from the input
waking the renderer, not movement. The instrument works; its selectivity
needs a minute-scale idle baseline and the measurement to be
"first movement after a long idle", not "any key".

**What remains open, and the honest path to it.** The live player
position/velocity object was not found: the mover chain is out (above), the
dmBody-shaped scans found nothing walking, and every shape-free motion search
lands in the false-friend families. The remaining options are (a) a **fresh
standing capture** taken while the character-control pages are warm — open
the hero-select/preview or simply move for a few seconds, then dump; the
capture needs the same build and must cover the module's NOACCESS pages, or
(b) a watchpoint-class capture, which the read-only rule forbids. Until one
of those exists, the M0 trace gate should be fed by bots: the bot bone pairs
found here (`0x0240810629a0` etc.) are the same character controller, and a
trace of one of them during a verified walk is the same `(input, state)`
recording the gate needs.

### Capture request for the next standing dump

The one capture that closes the remaining gaps, to be taken when the operator
next runs a session (any tool, same build `2.25.0.0.154088`):

1. **Warm the pages first.** In a match or the practice range: walk a few
   seconds, jump, crouch, fire, take damage if convenient. The module
   decrypts a page on first execution, so these actions are what makes the
   character/weapon/volley pages readable.
2. **Dump the whole module once, immediately after** — one linear pass over
   the module allocation (77 MB; `dev/dump_process.py dump` with the module
   scope does it). Missing from every existing capture and needed here:
   * mover provider/ctor `range 0x7ff64afc2000`–`0x7ff64afc3000`;
   * volley registrar `range 0x7ff64b73f000`–`0x7ff64b740000`;
   * volley behaviour slots `range 0x7ff64b75b000`–`0x7ff64b75c000` and
     `range 0x7ff64b5f0000`–`0x7ff64b5f1000`;
   * anything the character path touches that is still `PAGE_NOACCESS` in
     the two existing captures.
3. **Then dump the object heap family** `0x024080000000`+256 MB while
   standing in-match: it holds the mover components, the owners, the bot
   bone pairs, and — the open item — presumably the live player state. A
   warm heap snapshot with the player mid-walk is the input the shape-free
   searches could never get from the running process alone.

Live attachments cannot substitute: read-only access sees only executed
pages, and the executed set drifts with the session's activity.

### The player node found, and the first real keyed divergence

Q3's live half is unblocked. After a practice-range re-entry (the module maps
code lazily, so a fresh match is what makes the character path readable
again), a straight-walk scan of the mover band under a known key schedule
found the player transform node in two mirrored arenas:

* `0x0241ddb61880` and `0x0241e33f9680` (same record, 0x1e000000 apart);
* layout: orientation basis rows, then **position (feet) at +0xd0**,
  **yaw in degrees at +0xdc** (170.57 at spawn), **quaternion at +0xe0**,
  **scale (1,1,1) at +0xf0**; the position is repeated at +0x108, +0x138,
  +0x178 and as a capsule-centre variant (feet + ~1.71 m) at +0x1b8, +0x1d8;
  a movement scratch vector sits at +0x1e0..+0x1e8 (holds the last input
  direction × speed briefly, then zeroes — useful as an intent signal, not
  as the physics velocity).

Verified behaviour from a 19 s scripted recording
(`D:\ow-dumps\trace\player_node.jsonl`, 1115 samples @ ~58 Hz, keys per
sample): W moves the node at 3.96 m/s along −z, S reverses at 4.20 m/s, D
strafes, and a jump phase lifts y by +0.75 m. An exact-bit search for the
position triple finds ~90 copies across the heaps (renderer, animation,
gameplay arenas) — the mover band copy above is the authoritative one for
tracing.

The keyed replay closes the loop with the Rust side:
`D:/ow-dumps/trace/player_keyed.jsonl` (same recording, converted to the
trace format with the node's feet position) runs through
`cargo run -p ow-sim --bin trace_diff -- keys <file>`:

```
FIRST DIVERGENCE  sample 87  pos.x  client 53.89746  model 53.98913  24030 ulp  (|d|=0.091667)
```

Sample 87 is the first tick after the walk key goes down: the model applied
5.5 m/s instantly, the client ramps. That single line was the acceleration
model's fitting target; the ramp is now measured directly and the differ is
clean (see § "The movement controller: measured per-frame" below). The
physics-side velocity question is closed negative in § "The mover velocity
link".

### The movement controller: measured per-frame, gated write-by-write

The controller is now driven by measurements from the keyed captures rather
than by a 30/60 Hz fit (`ow-sim/src/movement.rs`; the full table and the code
anchors are in `formats/character-controller.md` § "The mover integrates per
frame"):

* **Direction convention (verified to the trace's precision).** With the node's
  heading yaw θ, forward = (sin θ, cos θ), right = (−cos θ, sin θ); the flat
  trace's straight plateaus reproduce both to 0.006° (w: −170.4387 against
  yaw 189.5668; d: −90.0018; a: +89.9979; s: 180 ± 0.29).
* **Travel speed 5.5 m/s, backpedal 4.95 m/s** — the per-hero slot values
  exactly. The earlier "5.331" came from assuming a 1/30 s stride: the client
  writes once per *frame*, and the frame dt in these captures was 0.032309 s
  (6.46 samples at 200 Hz over 189 write intervals, where a 1/30 s stride
  would read 6.67), which makes the plateau step exactly 5.5 × dt. Diagonals
  run at full speed (the input direction is normalised).
* **Ramp: velocity, not teleport.** 3.9216 m/s gained in the first full frame
  from rest, so ACCEL = 121.4 m/s²; release sheds a constant 2.345 m/s per
  frame, so FRICTION = 72.6 m/s². The approach is along the velocity *vector*
  (the traced ramp keeps the wish direction exactly; a per-axis clamp would
  swing the direction during a turn).
* **Vertical: measured, not published.** Gravity 17.37 m/s² (a constant
  0.5613 m/s shed per frame, variance tracking the frame dt), jump impulse
  5.33 m/s (fit to the whole traced arc), terminal 30 m/s from the slot.
  Airborne the horizontal velocity is preserved (no air accel, no decay).
* **Input latency: one write** (measured, unchanged). The replay consumes the
  previous write's keys.

The gate runs per *client write*, each advanced with its segment's own frame
dt (the 200 Hz sample clock gives the segment average; the per-frame jitter
itself is not observable from outside the process), and the frames inside a
write gap — a standing character writes nothing — are advanced too, with the
held input, so a standstill's friction decay is not skipped:

```
cargo run -p ow-sim --bin trace_diff -- keys <trace.jsonl> --by-state
```

On the keyed trace (flat ground):

```
no divergence across 101 compared write(s): 0 exact, 101 in the client's
frame-jitter band, 0 settling, 75 environment-affected, 20 skipped at input
transitions; max step deviation 0.0018616 m; max vertical step deviation
0.0000000 m; max vertical delta error 0.0000000 m
```

The residual is at the frame-jitter floor: the band's two measured sources are
the client's ~0.6% frame-time slips (±1.1 mm on a 0.178 m step) and the
harness's ±0.9 mm dt-estimate quantisation. Writes are classed as before:
exact; inside the band; settling (stop-transition root motion below 1 cm — the
traced node is the animation-side transform); environment-affected (a wall
slide, a slope, or terrain — an interior frame moving vertically more than
1 cm, or a sustained one-signed vertical over 75% of a segment, screens the
segment; the model has no terrain); skipped (the first two or three writes
after an input edge, whose partial-frame fraction depends on sub-write input
timing). The differ also prints a per-segment coverage table (`--by-state`) so
the screened share is visible segment by segment.

### The mover velocity link: a closed negative

The fitted mover needed its physics-side counterpart to be linked to the
Domino integration. Four sweeps settled it, all negative:

* a heap-wide search for the **walking velocity vector** (the exact measured
  components, ±0.02) while backpedaling: one candidate, and it was a static
  constant (unchanged through idle/S/W);
* a search for **speed and per-tick-displacement scalars** (4.797, 5.331,
  0.1599, 0.1777, 0.08) followed by an S-versus-W cross-match of all 400
  candidates: zero addresses change between the two speeds;
* the **Domino body layout** (position field, `invMass` ~1/82 kg, the
  character-mover byte) tested at every offset around all 55 exact copies of
  the current player position: no hit at any offset;
* the earlier any-mass body sweep found no body tracking the player either.

So the local character persists **only its position** (the traced node and its
copies); velocity, ramp state and the integration are computed per tick inside
the mover and consumed within it. That is consistent with the identity the
fitted controller already reproduces byte-for-byte: position advances by
`v x dt` where `v` ramps at `ACCEL`, decays at `FRICTION`, and caps at the
fitted speed. The Domino solver remains the body-side arithmetic for other
entities; the local character's path is the mover's per-tick displacement.

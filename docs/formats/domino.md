# Domino: the physics schema and integration

## Current state

- **Runtime schema: read in full** - 21 classes, 179 field records, every offset.
- **Integration: read.** Fixed 1/30 s steps with catch-up repetition and a
  commanded step count; the world has no vtable; broadphase, pools and the
  contact cache are read.
- **The local character is not a Domino body.** The mover computes its own
  displacement per frame; Domino is the body-side solver for other entities
  (`formats/character-controller.md`).
- **Open:** the mover control (tier 3), the literal configured tick pair, and a
  live `dmWorld`/gravity constant. The code for all three is in the complete
  image; only the runtime values are missing.

Addresses are v2b unless marked `exec`; `exec = v2b - 0x71820000`.

## The registry

The class descriptors and their field arrays live in one `.data` region
(`0x7ff7c90e7000`) and are populated at startup, so a capture holds them. A scan
for the record shape finds all 21 classes; the registry is
`0x7ff7c90eb700..0x7ff7c90ebb40`.

**Descriptor record** (stride 0x20):

```
+0x00 qword  class-name pointer
+0x08 qword  field-array pointer
+0x10 u32    class byte size (dmWorld stores a flag instead)
+0x14 u32    field count
+0x18 qword  per-class hook
```

**Field record** (stride 0x18):

```
+0x00 qword  field-name pointer, or a code pointer for a custom accessor
+0x08 u32    offset in the class
+0x10 u32    type code (legend below)
```

The descriptor getter `0x7ff7c7a38c40` fills a static descriptor with literal
stores; `0x7ff7c7a38cbf` writes the `{name, offset, type}` triples and is the
authority for the layout.

## Type codes

| code | meaning | basis |
|---|---|---|
| `0x1` | byte / bool | consecutive byte offsets |
| `0x2` | byte enum | `bodyType`, `ownerType` |
| `0x3` | small int (8-bit) | `groupIndex`, `boneIndex` |
| `0x4` | int32 | many |
| `0x5` | small int | `toiCount`, `debugBin` |
| `0x6` | int16 | `m_index`, `materialId`, `gameFlags` |
| `0x7` | u32 stamp | `islandStamp`, `keyA/keyB` |
| `0x8` | u64 | `userData`, manifold `key` |
| `0x9` | float32 | all scalar floats |
| `0xa` | bcVec3 (0x10) | velocity/size gaps |
| `0xb` | bcAABB (0x20) | `aabb` |
| `0xc` | string (0x10) | `name` |
| `0xd` | bcPQTransform (0x20) | confirmed |
| `0xe` | bcMtx44 (0x40) | confirmed |
| `0xf` | dmSweep (0x60) | confirmed |
| `0x11` | pointer to array | `nodes` |
| `0x12` | inline array | `trees` |
| `0x13` | dmBroadPhase (0xd8) | confirmed |
| `0x14/0x18/0x19/0x1a/0x1c/0x22/0x25` | typed arrays | bodies / fixtures / spheres / capsules / polytopes / contacts / convex contacts |
| `0x16` | dmFilter (6) | confirmed |
| `0x17` | pointer | fixture `userData` |
| `0x1b` | dmSubEdge array | `m_subEdges` |
| `0x1f` | inline dmManifoldPoint[4] | `points` at +0, normal at +0xc0 |
| `0x20/0x21/0x23/0x24` | arrays | manifold / contact-edge / GJK cache / SAT cache |

## Classes

| class | size | fields | class | size | fields |
|---|---|---|---|---|---|
| bcPQTransform | 0x20 | 2 | dmManifoldPoint | 0x30 | 6 |
| bcMtx44 | 0x40 | 4 | dmManifold | 0x100 | 7 |
| dmSweep | 0x60 | 6 | dmContact::Edge | 0xc | 3 |
| dmWorld | (flag) | 16 | dmContact | 0x58 | 11 |
| dmDynamicTree::Node | 0x40 | 6 | dmGJKCache | 0xc | 3 |
| dmDynamicTree | 0x28 | 7 | dmSATCache | 0x8 | 4 |
| dmBroadPhase | 0xd8 | 2 | dmConvexContact | 0x18 | 3 |
| dmBody | 0x1c0 | 40 | dmFilter | 0x6 | 3 |
| dmFixture | 0xc0 | 35 | dmSphere | 0x20 | 2 |
| dmCapsule | 0x30 | 3 | dmPolytope | 0x50 | 11 |
| dmSubEdge | 0x4 | 4 | | | |

### dmBody (0x1c0)

| # | field | offset | type | # | field | offset | type |
|---|---|---|---|---|---|---|---|
| 0 | index | 0x00 | int32 | 20 | maxRadius | 0x184 | float |
| 1 | transform | 0x10 | PQ | 21 | approximateRadius | 0x188 | float |
| 2 | linearVelocity | 0x30 | vec3 | 22 | massStabilizationScale | 0x18c | float |
| 3 | angularVelocity | 0x40 | vec3 | 23 | islandStamp | 0x190 | stamp |
| 4 | sweep | 0x50 | dmSweep | 24 | awakeIndex | 0x194 | int32 |
| 5 | `I` | 0xb0 | Mtx44 | 25 | revision | 0x198 | int16 |
| 6 | invI | 0xf0 | Mtx44 | 26 | toiCount | 0x19a | i8 |
| 7 | force | 0x130 | vec3 | 27 | debugBin | 0x19b | i8 |
| 8 | torque | 0x140 | vec3 | 28 | name | 0x19c | string16 |
| 9 | fixtureList | 0x158 | int32 | 29 | bodyType | 0x1ac | enum8 |
| 10 | fixtureCount | 0x15c | int32 | 30 | enableSleeping | 0x1ad | bool |
| 11 | jointListKey | 0x160 | int32 | 31 | isAwake | 0x1ae | bool |
| 12 | mass | 0x164 | float | 32 | isEnabled | 0x1af | bool |
| 13 | invMass | 0x168 | float | 33 | isStabilized | 0x1b0 | bool |
| 14 | gravityScale | 0x16c | float | 34 | useContinuous | 0x1b1 | bool |
| 15 | linearDamping | 0x170 | float | 35 | isBullet | 0x1b2 | bool |
| 16 | angularDamping | 0x174 | float | 36 | isSubset | 0x1b3 | bool |
| 17 | inertiaScale | 0x178 | float | 37 | isFast | 0x1b4 | bool |
| 18 | sleepTime | 0x17c | float | 38 | allowFastRotation | 0x1b5 | bool |
| 19 | minRadius | 0x180 | float | 39 | characterMover | 0x1b6 | bool |

`characterMover` is a byte flag; the body does not point at the mover.

### dmWorld (0x4d0)

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

### Shapes and contacts

* `dmSphere` {m_p @0x0, m_radius @0x10} - 0x20.
* `dmCapsule` {m_p1 @0x0, m_p2 @0x10, m_radius @0x20} - 0x30.
* `dmPolytope` {m_centroid, m_vertices, m_planes, m_faceSubEdges, m_subEdges,
  counts, m_volume, m_area, m_lengthScale} - 0x50.
* `dmFixture` - 0xc0: `aabb`, `type`, `shapeIndex`, `userData`, contact list,
  `ownerIndex/ownerNext`, density, radii, friction, restitution, rolling
  resistance, explosion/wind/drag/lift factors, `aabbMargin`,
  `characterPush` @0x8c, soft-contact frequency/damping, `filter`, `proxyId`,
  `materialId`, `gameFlags`, `boneIndex`, `ownerType`, `isSensor`,
  `useContinuous`, `clientDebugDraw`.
* `dmManifold` - 0x100: `points[4]` (4 x 0x30), `normal` @0xc0,
  `tangentImpulse` @0xd0, `rollingImpulse` @0xe0, `twistImpulse` @0xf0,
  `materialIdB` @0xf4, `pointCount` @0xf6.
* `dmContact` - 0x58: `index` @0xc, `edges` @0x14, `manifolds` @0x30,
  `subIndex`, friction/restitution/rollingResistance, `awakeIndex`, `type`,
  `isSolid`, `isTouching`.

## Tick and dt

The frame-to-stepper chain:

```
FUN_7ff7c6e0fc70  (system tick; step context at [param+0x40])
  └─ FUN_7ff7c7a68d80(stepCtx, dt=1/30, maxIter, 3.0)
       loop while residual < 0.5 and iter < maxIter:
         ├─ FUN_7ff7c7a4dbb0(handle, dt, 8, 3, flag) -> stepper FUN_7ff7c7a4c150
         └─ FUN_7ff7c7a4dcb0(handle, dt, 8, 3, flag) -> driver  FUN_7ff7c7a4c730
```

* **1/30 s is the module table's `+0x0c` entry** (`0x7ff7c7d5d84c` =
  0.0333333351). The table also holds 1/365, 1/60 and 1/24; those are other
  systems.
* **The stepper `0x7ff7c7a4c150` writes `m_inv_dt0`.** It reads `+0x198`,
  computes `divps(1, dt)`, stores the result when all lanes are valid, then runs
  the broadphase update `0x7ff7c7a7c460`. The substep driver
  `0x7ff7c7a4c730` writes only its local dt struct.
* **The world constructor `0x7ff7c7a47c70` takes no dt.** It zeroes
  `m_inv_dt0`; the rate enters per step from the tick.
* **`dmWorld::Create` is `0x7ff7c7a4cd70`.** Handles live in the table at
  `0x7ff7c90eaac0` (0x3fff slots, encoded pointers), and `ClearForces`
  (`0x7ff7c7a4cce0`) refuses while the step busy flag `+0x15c` is set.
* **The world has no vtable.** The 0x4d0-byte object is a plain struct; the
  vtable once attributed to it belongs to small descriptor classes.

### How many steps run

**One 1/30 s integration per frame**, with a startup catch-up and a residual
loop.

* The frame calls the tick **once** (`FUN_7ff7c6d92691` -> `FUN_7ff7c6e0fc70`),
  gated on `(+0xf08 & 0x10)` and `+0x1b4 == 1`. There is no frame-time
  accumulator.
* The tick runs `max(pending +0xef8, cap +0xefc)` integrations. The cap is a
  configured duration: the bind `FUN_7ff7c6e08510` computes
  `ceil(config.float@+0x10 * 30.0 * 1.0)`. The pending is a one-shot budget:
  `FUN_7ff7c6e0fb50` zeroes it after the tick.
* The startup path `FUN_7ff7c6e186d5` preloads **150** (5 s x 30) when the cap
  is unset and a global flag is set. The first tick may then run 150 steps.
* The tick object is 0xf10 bytes, built by `FUN_7ff7c6da3da4`; the constructor
  `FUN_7ff7c6e028f0` takes the configured pair from the builder's argument.
  The pair is assignment-only - no arithmetic on `+0xef8`/`+0xefc` exists.
* The step context is embedded at `+0xf0`: iteration counts at `+8`/`+0xc`
  (8/3), world handle at `+0x58`. The residual query is
  `FUN_7ff7c7a68840`; the threshold is `0.5`
  (`DAT_7ff7c7d0c068`).

The literal configured value is still unread: the builder, constructor and bind
are reached only through runtime-built tables, so the value comes from a live
object, not from a static reference.

## Integration arithmetic

Per dynamic body, in `FUN_7ff756261300` (island solve; `exec`, assembly is the
authority):

```
v' = ((gravityScale * mass * gravity + force) * (invMass * dt)) + v
```

`gravityScale * mass` is one f32 multiply; `invMass * dt` is a precomputed
4-lane product. Six roundings in this order; a fused or reassociated version
drifts within a few ticks.

Angular velocity: `w' = w + dt * (invI * torque)`, with the 3x3 product
decomposed into row vectors.

Damping is **linear**, not Box2D's `1/(1 + dt*d)`:

```
factor = clamp(1 - damping * dt, 0, 1)
v'' = factor * v'
```

Position: `x += dt * v`; quaternion
`q += 0.5*dt*(w x q)`, renormalised when `|q|^2 > 1.17e-35`; a per-step
displacement squared above **16.0** is clamped (`DAT_7ff756922400`, exec) -
a 4 m/step translation limit. Sleep accumulates `sleepTime` against
`DAT_7ff7564ec068` = 0.5 s (exec).

The 1/60 constant once suspected as a tick is a **density** in the
mass-properties function `FUN_7ff7c7a5aee0` (signed-tetrahedron volume,
first/second moments), not a tick rate.

## The local character

The local player is **not** in the Domino body list. Its physics-side link is a
closed negative:

* no walking-velocity vector (exact measured components) changes with input;
* no speed/per-tick-displacement scalar changes between S and W;
* the Domino body layout (`invMass` ~1/82 kg, `characterMover`) is not found at
  any offset around the 55 exact copies of the player position;
* the any-mass body sweep found no body tracking the player.

What persists is the position node: basis rows, **feet position at +0xd0**,
**yaw in degrees at +0xdc**, quaternion at +0xe0, scale at +0xf0, with copies
at +0x108/+0x138/+0x178 and capsule-centre variants at +0x1b8/+0x1d8. A
movement scratch vector at +0x1e0..+0x1e8 holds the last input direction x
speed briefly.

Measured from keyed traces (`D:/ow-dumps/trace/player_node.jsonl`):

* travel **5.5 m/s**, backpedal **4.95 m/s**, diagonals at full speed;
* acceleration **121.4 m/s²** (3.9216 m/s gained in the first full frame);
* friction **72.6 m/s²** (2.345 m/s shed per frame);
* gravity **17.37 m/s²**, jump impulse **5.33 m/s**, terminal **30 m/s**;
* airborne horizontal velocity preserved; input latency one write.

`trace_diff --keys --by-state` replays the keyed trace with per-write frame dt
and reports no divergence across 101 compared writes (101 in the client's
frame-jitter band). The residual is the client's ~0.6% frame-time slips plus
the harness's dt quantisation; the model has no terrain, so walls/slopes are
screened as environment-affected.

## Ruled out

* **`0x7ff7c536xxxx` as a Domino schema reader** - narrative-viewer UI
  serialization and Wwise audio; the address once labelled a `bcPQTransform`
  field object is the string `"title"`.
* **1/60 as the fixed tick** - a mass density in the mass-properties function.
* **Shape-only body scans** - 4,000 candidates in 2 GB of live process, none
  real; uniform filler and bone/transform pools satisfy every float invariant.
* **World value signatures** - 29,079 candidates over the v2b capture, zero
  worlds; single float fields cannot discriminate.
* **Tree scans alone** - generic spatial trees are used by renderers and audio;
  782k tree-shaped sites, one physics-plausible tree whose world frame did not
  close.
* **Mover control via displacement co-occurrence** - 19 hit families, all
  curve/spline code or schema writers; the mover's offsets are shared with
  generic vector code.
* **The mover's own vtables** - all infrastructure (descriptors, destructors,
  registration, forwarding shims); the class computes nothing with its fields.
* **The volley hash sites** - the `numShotsPerSecond` site is the field
  registrar; gameplay reads the slot, not the hash.

The reliable locator for live physics state is behavioural: sample twice and
keep what moves with walking-speed consistency. Shape/value signatures are only
useful after a behavioural anchor says where to look.

## Open

* **The tick pair's literal value.** Runtime-only; read it from a live tick
  object or the builder's argument.
* **The mover control (tier 3).** The rates are measured; the code is
  obfuscated, plausibly a VM layer. A keyed trace is the current substitute.
* **The live world/gravity constant.** The formula is read; the object was not
  found in the captures. The measured 17.37 m/s² is the movement-side value.

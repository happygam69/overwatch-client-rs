# The character mover

The mover is the client's first-person character controller: a component that
configures a larger character object, and an advance/invoker pair that
integrates the per-frame step.

## Current state

- **Data: read.** The component schema, the per-hero slots and the jump struct
  are read from the install.
- **Update path: partly read, partly verified.** The component registration,
  the character object, the advance, the invoker and the state machine are
  located in the current image. The ramp leaves
  (`MoverRampVector`/`MoverRampScalar`/`MoverRampScalarOffset`) and the command
  builder (`BuildMoverPhysicsCommand`) are differential-verified against the
  client's bytes.
- **Open:** the input→wish spring (`FUN_140cc0dc0`), the
  `SubmitMoverPhysicsCommand` velocity assembly, the Domino character
  controller and collision. The invoker's caller is still not located and may
  be a dead path.
- **Collision, 2026-11-02:** the collider query leaves are proven — the type-3
  mesh sweep (`fn/MeshSweep`: the BVH walk plus the triangle test) and
  `ColliderTriangleDegenerate` (`0x142cd56e0`, the skip test
  `MeshToColliderFaces` applies per triangle) are differential-verified. The
  rest of the collider-mesh chain is read but not implemented:
  `MeshToColliderFaces` (`0x142cf2010`), `ColliderMeshEmit` (`0x142ccc120`),
  `ColliderFaceAppend` (`0x142cca460`), `BrushTreeToMesh` (`0x142cf23c0`), the
  pool record creator (`0x1421363d1`) and the driver `0x14213bbc0` that turns
  pool records into the `0x60` cell and `0x100` sector entries and hands them
  to `ShapeAddToBody` -> `GeometryFinalize`. Geometry data reaches those pools
  from **brush/model primitives**, not from the 17-array chunk container.
- **The collider mesh pipeline is read end to end (2026-11-02).** brush tree ->
  `BrushTreeToMesh` (`0x142cf23c0`) -> `MeshToColliderFaces` (`0x142cf2010`,
  which duplicates every vertex as `v + eps` and `v - eps` and emits two
  oppositely wound triangles per face, i.e. a closed double-sided **shell**,
  adding side walls when `eps >= 0.005` (`0x142f897c4`)) -> `ColliderMeshEmit`
  (`0x142ccc120`) -> `ColliderFaceAppend` (`0x142cca460`), which writes
  `0x60`-byte face records (`min`, `max`, `(min+max)/2`, `{-1, area, i0, i1}`,
  `{i2, -1, -1, -1}`, `{mode, a, b, 0}`), welds the shell's two surfaces through
  a hash of undirected vertex-pair keys (bucket count
  `round_up_pow2(3 * faces)`), and builds a BVH (`0x142ccb1b0`, leaf at `<= 4`
  faces, `0x50`-byte build nodes: `min` `+0x00`, `max` `+0x10`, `+0x20`/`+0x28`
  zero, `first_face` `+0x28`, `+0x30` axis, `+0x34`/`+0x38` children, `+0x3c`
  face count of a leaf, `+0x40` height, `+0x44` the id word). The split rule is
  decided by the **centroid** spread (`0x142ccb2a2`): when all three centroid
  extents are below `0.005` the faces are clustered and the node splits at the
  array half (`count >> 1`, `0x142ccb314`) with its axis named by the largest
  face-box extent; otherwise a 64-bin surface-area heuristic runs along the
  largest centroid extent — bin = `clamp(trunc((c - low) * 64 / extent), 0, 63)`
  (`0x142ccb680`), prefix and suffix boxes give
  `cost(i) = area(prefix_i) * count(prefix_i) + area(suffix_i+1) *
  count(suffix_i+1)` with `area = 2 * (x*(z+y) + z*y)`, the earliest cheapest
  boundary wins, and the array is then partitioned **in place by swapping** each
  qualifying face into the next free slot (`0x142ccbc17`) — so the taken faces
  keep their order and the displaced face moves to the source's old slot, which
  is what the client's own output shows. `fn/BuildBvh` proves all of it.
  The conversion to the `0x20`-byte runtime nodes (`0x142ccad80`) and the
  appender `0x142cca460` are still unread, as is the pool importer
  `0x1421363d1`; the weld's neighbour links have no known reader.
- **Implementation status:** no playable movement is implemented. The old
  trace-measured approximation was deleted with `ow-sim/movement.rs`; the
  client's own control law is the only acceptable source. The verified leaves
  are arithmetic, not a per-frame step.

## The component

`RegisterMoverComponent` (`0x14017a680`) registers
`STUCharacterMoverComponent`: type hash `0x6EFDBD73`, component id `0x26`,
size `0x130`, 52 fields. The constructor (`0x14111b120`) installs the three
interface vtables at component `+0`, `+8` and `+0x48`.

At runtime every scalar's slot is its STU offset **+ 4**:

| STU offset | runtime slot | meaning |
|---|---|---|
| `0x08` | `0x0c` | first scalar (schema header) |
| `0x48` | `0x4c` | jump struct (`STU_97288E54`) |
| `0x68` | `0x6c` | second inline struct |
| `0xb0` | `0xb4` | forward speed |
| `0xb4` | `0xb8` | side speed |
| `0xb8` | `0xbc` | back speed |
| `0xbc` | `0xc0` | crouch speed |
| `0xec` | `0xf0` | terminal fall speed |

The per-hero values are read by `ow-tank`'s mover reader. The named anchors:

| hash | role | install value |
|---|---|---|
| `19452F15` | forward base speed | `5.5`; `6.0` for three heroes |
| `CFD3E635` | sideways base speed | equal to forward |
| `61D6B4E9` | backpedal base speed | `4.95`; `5.4` for the fast trio |
| `0DDD18C3` | crouch speed | `3.0`; `2.0`/`2.5` for a few |
| `B74245E8` | terminal fall speed | `30` |

The remaining 40+ scalars stay hash-labelled; none is guessed into a role.

## The jump struct

`STU_97288E54` at STU offset `72` carries the vertical configuration. The
client's default comparator hardcodes six fields, bit-exact against the class
file:

| hash | runtime slot | default |
|---|---|---|
| `16882D22` | `0x50` | `20f` |
| `746B3248` | `0x54` | `20f` |
| `F6B890EC` | `0x58` | `0.98f` |
| `D68B7D62` | `0x5c` | `0.7f` |
| `9FCDBE78` | `0x60` | `0.7f` |
| `766AFA5A` | `0x64` | `0.2f` |

## The character object

The component configures a 0x1c00-byte character object. `ConstructMoverBase`
(`0x140ca37d0`) builds the 0x1b70-byte base subobject and installs the base
vtable at `0x1431101e8`; `DestructMoverBody` (`0x1407ff270`) frees it.

Fields the movement code reads:

| offset | role |
|---|---|
| `+0x10d0` | u32 flags word |
| `+0x10ec` | float timer |
| `+0x1140..0x114c` | 4-float **local wish vector** |
| `+0x1520`/`+0x1524` | u32 state-id pair |
| `+0x1a77..0x1a79` | movement state bytes |
| `+0x1a90..0x1acc` | 4x4 world transform (translation at `+0x1ac0`) |
| `+0x1ad8` | physics world slot: `[slot]` is the variant object |
| `+0x1b80..0x1bbf` | committed transform: position quad + three basis vectors |
| `+0x1cd0` | null-sentinel pointer |

## The update path

| function | address | role |
|---|---|---|
| `MoverAdvance` | `0x140ca7f30` | the per-frame position integrator (base vtable slot 15); **scaffold** |
| `MoverInvoker` | `0x1407ffc50` | calls the advance, stores the new position at `+0x1b80`, dispatches contact normals; single vtable reference, likely unreachable |
| `MoverStateMachine` | `0x140cbacc0` | reads flags/timer/state id, writes the state byte `+0x1a79`, transforms the local wish vector; calls `ApplyStateModifiers` then `SubmitMoverPhysicsCommand` |
| `MoverWishUpdate` | `0x140ca5940` | reads `+0x1140..0x114c`, resolves the state-id pair, writes the result back |
| `MoverUpdateFrame` | `0x140cbb260` | the per-frame body update: modifiers, submit, input build, wish spring, then direct `SetCharacterControllerVelocity` |
| `MoverUpdateCommand` | `0x140cb5760` | the command-driven body update: input build, wish spring, direct velocity |
| `MoverWishSpring` | `0x140cc0dc0` | the wish copy and `+0x10fc`/`+0x1100` spring inside `MoverUpdateFrame` / `MoverUpdateCommand` |
| `MoverRampVector` | `0x140ca75b0` | the four-lane ramp leaf: `(minss(timer,1) * scale + body[0x10fc]) * body[0xc0..0xcc]`; **verified** |
| `MoverRampScalarOffset` | `0x140ca7570` | the same expression's scalar part with the `+0x10fc` add; **verified** |
| `MoverRampScalar` | `0x140ca75f0` | the scalar `minss(timer,1) * scale`; **verified** |
| `BuildMoverPhysicsCommand` | `0x140cb74a0` | the six command bytes from the body state and gate; **verified** |

`MoverAdvance` has two paths:

- **plain body:** `out_i = (a * X_i + pos_i) * b + off_i`, with `X` the
  **static local offset** at `+0xc0..0xcc`;
- **character:** the collision sweep, using the walk-state bytes at
  `+0x1a79`/`+0x1a7c`, the step state and the body/world handle at `+0x35b`.

`MoverInvoker` is advance plus collision notification, not the control law.
The per-frame step's source is the wish vector at `+0x1140` and the transform
at `+0x1a90`, with the advance's `param_5`/`param_6` scales.

**Verified movement leaves (2026-10-31, updated 2026-11-01).** Twenty-seven
differential units now pin the read arithmetic against the client's bytes:

- `fn/MoverRampVector` (`0x140ca75b0`): `t = minss(body[0x10ec], 1.0)`,
  `scale = body[0x5b4]` when `body[0x10d0] & 2` else `body[0x5a8]`,
  `ramp = t * scale + flags[0x2c]`, `out[i] = ramp * body[0xc0 + 4*i]`.
  The third argument is the caller's `body + 0x10d0` block. 264 cases, 100%
  branch coverage, every mutant killed or declared.
- `fn/MoverRampScalar` (`0x140ca75f0`) and `fn/MoverRampScalarOffset`
  (`0x140ca7570`): the scalar forms used by `FUN_140cc0dc0` and the tail of
  `MoverAdvance`.
- `fn/BuildMoverPhysicsCommand` (`0x140cb74a0`): the six command bytes from
  `+0x1a7b`, `+0x1a75`, `+0x1a60`, `+0x1a79`, `+0x652`, `+0x1a7a`, `+0x1a7f`
  and `+0x23c`, with the movement gate pinned to its no-record path.
- `fn/MoverWishSpring` (`0x140cc0dc0`): the input-object copy into `+0x1140`,
  the three difference/scale terms, the `+0x10fc`/`+0x1100` spring against
  `+0x5c0/+0x5c4`, the `+0x688/+0x68c` clamps, the `FUN_140ca02d0` pair
  update and the `+0x110c` 1/1024 quantiser. 264 cases, 90% branch coverage,
  12/12 mutants killed. The unit pins both state-id branches; the state-id
  sentinel makes the `FUN_140ca5d00`/`FUN_140ca7700` transforms identity, and
  `body+0x1ad8 = 0` makes `FUN_142cbcec0` return the image's zero vector. The
  missed edge is the `|next| <= 1/512` + `dt <= eps` quantiser edge.
- `fn/MoverWishForward` (`0x140cb5510`): the forward 4x4 column-major
  transform `v' = ((y*c1 + x*c0) + z*c2) + c3`, 264 cases, 100% coverage,
  5/5 mutants. The oracle starts at the interior block `0x140cb5534` with the
  vector in `RBX` and the matrix at `[RSP+0x30]`; the harness `startRegs`
  extension seeds `RBX`/`RSP` and the state block holds the fixed matrix. The
  `FUN_140cb83b0` lookup branch before the block is the acknowledged gap.
- `fn/MoverWishInverse` (`0x140cb5350`): the inverse
  `v' = R^-1 * (v - c3)`, with the cross products, `det = c0 . (c1 x c2)`,
  `rcpps` + `(2 - rcp*det)*rcp`, the row broadcasts, the sign-mask XOR and the
  final add transcribed lane for lane from the image. 264 cases, 100%
  coverage, 5/5 applicable mutants. The oracle starts at the interior block
  `0x140cb537b`; the fixed matrix has `det = 39/256` (not a power of two), and
  the fixture still matches bit for bit because the harness's Unicorn `rcpps`
  is exact IEEE `1/x`. The client transforms **all four lanes**: the row
  vectors carry a shuffled w component, so `v'.w` is not simply zero (the
  identity + translation fixture gives `w = 3`). The `FUN_140cb83b0` lookup
  and its failure path before the block are the acknowledged gap.
- `fn/MoverWishCopy` (`0x140ca7700`): the copy + conditional forward
  transform. 264 cases, 100% coverage, 5/5 mutants. The oracle runs the real
  entry against a synthetic state record (below); the candidate receives the
  resolved matrix, so the lookup itself stays open.
- `fn/StateTransform` (`0x140cb83b0` + the `FUN_140c71e10` leaf): the
  state-record lookup in `ow-sim::state::state_transform`. 264 cases, 11/12
  mutants killed (92%), 20% branch coverage. The tested edges are the valid
  leaf and both sentinels (`0` and `0xffffffff`); the uncovered edges are the
  non-returning handle-mismatch path and the rest of the gate that the
  sentinel returns skip. The fixture uses record A (bitmap 2, rank 0) and
  record B (bitmap 3, rank 1), so the index and both table slots are
  exercised; sentinel cases prove the output buffer is left untouched.
- `fn/CharacterPosition` (`0x142cd4ff0`): the character-handle position leaf.
  264 cases, 100% branch coverage (7/7), 4/5 mutants (80%). Cases cover every
  failure edge (space bound, sentinel index, null record, empty record,
  negative index, index at/over count, entry-index mismatch, generation
  mismatch) and live entries. The unit needed the harness's new **`vec4`
  return kind**: XMM0 is captured as 128 bits and compared lane-exactly.
- `fn/RigidPosition` (`0x142cb4770`): the rigid-handle position leaf. 264
  cases, 100% branch coverage (7/7), 4/5 mutants (80%). Same failure set, with
  the rigid layout `[record+0x288]`, count `+0x294`, stride `0x1c0`, index
  `+0x00`, generation `+0x198`.
- `fn/VariantPosition` (`0x142cf5630`): the tag dispatcher. 264 cases, 100%
  branch coverage (2/2), 4/4 mutants. Null variant -> zero vec4; tag 1 ->
  `[variant+0x10]` through the rigid leaf; any other tag -> `[variant+8]`
  through the character leaf.
- `fn/WorldPosition` (`0x142cbcec0`): the body world-slot wrapper. 264 cases,
  100% of its one wrapper branch, 1/1 mutant. It reads `[slot]`, then calls
  the dispatcher; the tail-jumped dispatcher and leaves are outside the
  contiguous coverage window and are measured by the three units above.
- `fn/MoveCharacterController` (`0x142cd4bc0`): the controller move, no-shape
  path. 36 cases, 100% of the three reachable branches (the shape-index edge
  is declared dead for this fixture), 10/10 mutants. The quaternion is
  normalised as `1 / sqrt((q0*q0 + q1*q1) + (q2*q2 + q3*q3))`; entry `+0x00`
  gets the normalised quat, `+0x10` the new velocity, and record `+0x190` the
  published flag. The early-out when xyz is unchanged, including the two
  quat comparisons, is covered. The shape-query loop is open.
- `fn/ApplyCharacterControllerMove` (`0x142cd5370`): the character apply.
  19 cases, 100% branch coverage (9/9), 8/10 mutants. It resolves the
  character handle, rejects an inf/NaN exponent in xyz (w is ignored), gates
  on record `+0x15c == 0`, then calls the move. Cases cover changed and
  unchanged velocities, inf/nan in each lane, the record gate, null/count-0
  records, count/entry/generation failures and the space bound.
- `fn/SetCharacterControllerVelocity` (`0x142cbd1b0`): the slot wrapper's
  character path. 12 cases, 100% of the wrapper branch, 7/7 mutants. The slot
  points at the variant; tag 0 uses `[variant+8]`, tag 1 is the rigid path and
  is open. The fixture's tag-1 case uses an invalid rigid handle so the rigid
  leaf declines without a write, while the tag-0 cases verify the publish.
- `fn/ShapeSphere` (`0x142ccd410`), `fn/ShapeCapsule` (`0x142ccd380`),
  `fn/ShapeBox` (`0x142ccf460`) and `fn/ShapePoints` (`0x142cd3720`): the
  four shape-bound leaves. Each transforms the shape's point(s) by the
  column-major matrix and writes eight floats (lower/upper) to the output.
  Sphere uses `center +0x00`, `radius +0x10`; capsule `point0 +0x00`,
  `point1 +0x10`, `radius +0x20`; box `point0 +0x10`, `point1 +0x20`; the
  point cloud uses `points +0x10`, `count +0x30`, stride 0x10, starts from
  `+FLT_MAX`/`-FLT_MAX`, expands by 0.01 and adds the translation after the
  min/max fold. All four units pass with 100% branch coverage and 90–100%
  mutation kill; the point-cloud count-0 edge is covered.
- `fn/ShapeDispatch` (`0x142cb9030`): the shape-record dispatcher. 4 cases,
  75% branch coverage (3/4; the type-3 edge is the open one), 10/10 mutants.
  It reads the type at `record+0x20` and the index at `+0x24`, selects the
  array from `world+0x2d0/0x2e8/0x300/0x330`, runs the type-0/1/2/4 leaf and
  copies the 0x20-byte bounds back into the record. Types above 4 follow the
  client's final box path.
  The type-3 mesh path (`0x142cae5a0`) was open here; it is now read and
  verified separately as `fn/MeshSweep` against the walk its dispatcher calls
  (`0x142caed30`, `crates/ow-sim/src/collide.rs::mesh_sweep`), so this unit's
  remaining open edge is only the dispatcher's own type-3 branch, which
  `fn/ShapeDispatch` elects not to cover (the 0x2f0 pool record path is not
  fixture-reachable without the allocator vtable).
- `fn/MoveCharacterControllerShape` (`0x142cd4bc0`, shape path): 9 cases,
  83.3% branch coverage (5/6; the contact-event edge is open), 8/10 mutants.
  It builds the quaternion matrix
  `[1-2(y²+z²), 2(xy+zw), ...]` with the new velocity as translation, then
  walks the `record+0x2b8` shape list (stride 0xc0, next at `+0x54`), calls
  the bound dispatcher and copies the 0x20-byte result into each shape record.
  Contact id `+0xa4 != -1` is the open event path; the fixture uses `-1`.
  The same unit covers the no-shape `entry+0x24 == -1` branch.
- `fn/MoverStateVector` (`0x140ca5c50`): the state transform in place. 264
  cases, 100% branch coverage, 5/5 mutants. The oracle runs the real wrapper
  with the synthetic record; the cases cover the transform path, the sentinel
  with the flag set, and the flag-clear `FUN_140c9f000` path (handle 0 plus
  sentinel), so both edges of the flag branch run.
- `fn/DefaultMoverSettings` (`0x1405a87d0`): the settings default
  constructor. 264 cases, 4/4 mutants. The object is pinned at a known arena
  address because four fields are pointers into the object itself; every case
  starts from a different byte fill, so unwritten bytes must survive. It pins
  the ramp/spring defaults `+0x5a8/+0x5ac/+0x5b4 = 1.0`, `+0x5c0 = 0.0`,
  `+0x5c4..+0x5c6 = 1` and the parameter seeds `+0x5d0 = -2`,
  `+0x5d4 = 1.0`, `+0x5dc = 0x10000`.
- `fn/ApplyMoverSettings` (`0x140c862e0`): the movement-asset apply. 264
  cases, 11/12 mutants (92%), 33% branch coverage with the vector-growth,
  `+0x298` assert and decode-null edges declared. The cases cover the null
  state (direct copy), the transform record A, the inline capacity boundary
  (8 entries) and record C (bitmap 1, masked decode zero, direct copy). The
  appended ids are tagged `0x0d80000000000000` (not `0xd800...`); the three
  gate fields are byte writes and `+0x5e2`/`+0x5dd` are bytes too.
- `fn/MoverGate` (`0x140ca7d80`): the movement gate. 126 cases, 7/8 mutants
  (88%), 90.9% branch coverage with the null-record edge declared
  unreachable. The body's `+0x690` array points at synthetic 0x30-byte
  records whose `+0x28` objects hold fake vtables with a `+0x78` stub
  returning 1 or 0; the candidate receives those results as a byte slice.
- `fn/ApplyStateModifiers` (`0x140cbcbe0`): the 37-kind modifier evaluator.
  336 cases, 12/12 mutants, 76.5% branch coverage. The cases cover every
  live kind with a set record and with `op > 5`, every op 0..5 per kind, the
  holes, kind/op out of range, window edges, NaN values and bounds, the dash
  prologue windows (`t < 20`, `20..63`, `>= 63`, flags on/off, `d8` at and
  around the start), the guarded crouch block, and the `0xa70` scan's found,
  not-found and restore paths. The body is pinned in the arena; the real
  vtable's slots are exe addresses, so the unit builds a fake table whose
  `+0x68`/`+0x70` entries are the stubs' v2b addresses.

## The synthetic state record (2026-11-01)

`FUN_140cb83b0` gates on `state_id == *(u32*)(handle+0x138)` and then calls
`FUN_140c71e10(matrix_out, handle, state_id)`. A synthetic record is
seedable:

| offset | meaning |
|---|---|
| `+0x080` | qword pointer to a dense entry array |
| `+0x110` | presence bitmap; bit 1 = key 1 present (the first lookup uses mask `2`) |
| `+0x130` | per-group base index into the dense array |
| `+0x138` | u32 record id; must equal the body's `+0x1520` |

The table entry is decoded through the leaf at `0x140543480`:
`[RCX] = ROR64((ROR64([RCX],0x14) ^ 0x8b084bd971b946cc) -
0x32c1c6a5a278ca6f, 0x26)`, then `ROL64(x,0x2b)` gives the object address.
The object's `+0x60..0x9f` is the 4x4 matrix and `+0x298` must be zero for the
leaf path. The decode also reads `Q = [[0x143b79598]+0x198]` and
`byte[0x1438480f3]` (0x42); the static image holds `[0x143b79598] = 0`, so a
fixture must patch it to a readable block with a zero `+0x198`.

The unit's record lives at the v2b address of `[0x143b79598]`:
`[GLOBAL] = GLOBAL` (so `Q = 0`), record at `+0x100`, table at `+0x280`,
object at `+0x300`, service descriptor at `+0x480`. The
oracle copied the matrix and `FUN_140ca5c50` transformed `(1,2,3,1)` to
`(11,22,33,1)` in the subagent's emulation; the `fn/MoverWishCopy` unit
reproduces the transform path through the real wrapper.

`body+0x19b0` is written by `FUN_140ca9680` at `0x140ca96a7`/`0x140ca96ae`
(gated by `+0x1a7c`); `+0x1a77` means "cached handle filled", not "valid".
`FUN_140c9f000` can return a non-zero registry entry at `0x140c9f0ee`; its
decompiler "return 0" is only the miss path.

The ramp leaf is **not** the input→velocity control law by itself: it turns a
timer, a state scale and the static direction at `+0xc0` into the velocity-like
quad. The wish spring now proves how the input object reaches `+0x1140` and
the `+0x10fc`/`+0x1100` state; the live path still needs the state-record
transform (`FUN_140ca5c50`), the `FUN_140cb63c0` wish writer and the Domino
velocity dispatch.

## The corrected live chain (2026-11-01)

The earlier chain was wrong in three places. The current-image read is:

```text
FUN_140cbb260 (0x140cbb260, MoverUpdateFrame)
  -> ApplyStateModifiers           0xcbb533
  -> SubmitMoverPhysicsCommand     0xcbb560
  -> input-source pass             0xcbb690 / 0xcbb6d8
  -> FUN_140cba030 (input build)   0xcbba4f
  -> FUN_140cc0dc0 (wish spring)   0xcbbb2c
  -> FUN_140ca5c50 (state vector)  0xcbbdd6
  -> FUN_140ca75b0 (ramp vector)   0xcbbde6
  -> SetCharacterControllerVelocity 0xcbbe0b and 0xcbbf9a
```

- `SubmitMoverPhysicsCommand` (`0x140cc03d0`) calls `BuildMoverPhysicsCommand`
  and `FUN_1420163b0`; it does **not** call `SetCharacterControllerVelocity`.
- `MoverStateMachine` (`0x140cbacc0`) calls `ApplyStateModifiers` and
  `SubmitMoverPhysicsCommand`; it never sets the controller velocity.
- `FUN_140cb5760` (`MoverUpdateCommand`) is the second update path: input
  build -> wish spring -> direct velocity at `0x140cb58b4`.
- `FUN_140cc0dc0` (`MoverWishSpring`) is reached only from
  `MoverUpdateFrame`/`MoverUpdateCommand`, never from `MoverStateMachine`.
  It copies the input object's `+0x60..0x6c` to body `+0x1140` when
  `input+0x98 != 0` and `(body+0x10d0 & 0x80000c) == 0`, then updates the
  `+0x10fc`/`+0x1100` spring against `+0x5c0/+0x5c4`, clamps `+0x688/+0x68c`
  and quantises `+0x110c` by `1/1024`.

The input object is a stack local built by `FUN_140cba030`: source `+0x00`
(16-byte wish) -> input `+0x60`, source `+0x20` -> input `+0x70`, source
`+0x40` byte -> input `+0x98` (the gate). The source struct is filled by the
body `+0xf0` state records' vtable slot 8, with `FUN_140cc1060` as the
fallback. The external command that seeds it comes from `FUN_14068cf10`
building a 0x14-byte block from the `R15` object's `+0x1210`, `+0x1220`,
`+0x1228/+0x122a` and `+0xe14`; the producer and the remaining gaps are in
"The wish source" below.

### The wish has no button seed: the loop is closed (2026-11-01)

Route "is there an external producer of the wish?" is now **answered, and the
answer is no**. Every writer of `body+0x1140` in the image was enumerated (a
byte scan for the store forms at that displacement, six sites):

| site | function | what it writes |
|---|---|---|
| `0x140ca5a01`, `0x140ca5b42` | `0x140ca5940`, `0x140ca5a90` | the wish copy/transform wrappers |
| `0x140cc0e02` | `0x140cc0dc0` | `MoverWishSpring` (verified) |
| `0x140cbfbfd` | `0x140cbf950` | `body+0x1140 = *0x140ca7650(body, &local, &world)` |
| `0x140eef02b`, `0x141d5ffe6` | ability/animation code | out of the movement path |

`0x140ca7650` is a forward-of-`0x140ca7700` twin: it copies the input vector
to the output, resolves the state object (`body+0x19b0` when `body+0x1a77`
is set, else `0x140c9f000(body+0x10d0)` and the hashed lookup at
`[body+8]+0x28`), then calls the **inverse** transform `0x140cb5350`. So the
only non-spring writer stores a *world* vector converted back into the body's
local wish space.

`0x140cbf950` (entry found by walking back to the prologue; it is not a
`.pdata` function) is called from exactly two places — `0x140cbbd88` inside
`MoverUpdateFrame` and `0x140cb5f4c` inside `MoverUpdateCommand` — i.e. it is
the shared body of both update paths. Along the way it:

- branches on `body[0x1210]`'s **first byte** as a command *kind*
  (`0x140cbfc91..0x140cbfca5`: `1` when the byte is `2, 3, 4` or `5`),
  with `[body] & 0x80` selecting `body+0x120` versus the passed pointer;
- publishes at `0x140cbfbd8` (`SetCharacterControllerVelocity([body+0x1ad8], &v)`)
  where `v` is the block at `[RBP-0x80]`, or `scale*body[0xc0] + that block`
  when `R12B` is set (`0x140cbfba0..0x140cbfba7`);
- calls body vtable `+0x98`, then writes `body+0x1140` from `0x140ca7650`.

So the wish is fed back from the **resolved world vector**, and the only other
source is the previous frame's own wish through the spring. Combined with the
fan being the sole writer of `source+0x00`, the conclusion is:

> The client has **no stage that turns buttons into a direction**. The movement
> direction is the fan's collision-scored candidate, remembered one frame later
> through `body+0x1140`. Without the collision world there is no wish to spring.

The move-parameter block used by that pass is `body+0x5a0..0x5b4` with the same
bit-1 selection the ramp uses (`0x140cc0804`/`0x140cc0822`: `+0x5b0/+0x5ac/
+0x5b4` when flags bit 1 is set, `+0x5a4/+0x5a0/+0x5a8` when clear), and
`0x140cc0700` is its preparation pass (it clamps `body[0x65c]` between
`0x1439b1230`/`0x1439b1270`, gates on `body[0x10ec]`, `body[0x1a83]` and
`body[0xe4] == body[0xb0]+1`, and builds the local query structs including
the `-1` index at `[RBP-0x25]`).

### The command packer, read exactly (2026-11-01)

`FUN_14068cf10` ends by packing the block `MoverUpdateFrame` consumes. The
read expression, instruction by instruction (`R15` is the actor, `body` is in
`RSI`):

```text
0x14068d1a6  movups xmm0, [r15 + 0x1210]      ; the current 0x14-byte record
0x14068d19b  mov    eax,  [r15 + 0x1220]      ; its flag dword at +0x10
0x14068d1cd  movups [rbp - 0x70], xmm0        ; command + 0x00..+0x0f
0x14068d1c2  mov    [rbp - 0x60], eax         ; command + 0x10..+0x13
0x14068d1d5  movzx  eax, word [r15 + 0x1228]  ; NEXT record's yaw
0x14068d1dd  mov    word [rbp - 0x6c], ax     ; command + 0x04
0x14068d1c5  movzx  eax, word [r15 + 0x122a]  ; NEXT record's pitch
0x14068d1d1  mov    word [rbp - 0x6a], ax     ; command + 0x06
0x14068d1ae  mov    r8d, [r15 + 0xe14]        ; actor sequence
0x14068d1bf  inc    r8d
0x14068d1e6  call   0x140cbb260              ; MoverUpdateFrame(body, &cmd, seq, tick, 0)
```

So the block is **the current record verbatim with the next record's yaw and
pitch overlaid**: the newest aim wins, the buttons and flag dword stay with
the record being consumed. `+0x1228`/`+0x122a` are `next + 0x04`/`+0x06` —
the same yaw/pitch slots the current record carries at `+0x1210`. The sequence
argument is `[actor+0xe14] + 1`, the fourth argument is the global qword at
`0x143871e08` (the same value the function later divides by, so it is the tick
frequency, not a delta), and the fifth is `0`. Implemented as
`ow_sim::frame::pack_command` and `ow_sim::frame::command_sequence`.

### The state-record pass is read (2026-11-01)

`FUN_140cb63c0` is the pass that turns the body into the fan call. It takes
`(body, source)` and:

```text
state object  = resolve(body+8, ...)            ; the hashed-container lookup
state id      = state_object ? u16[state_object+0xbc] : 1     ; 0x140cb6558
word[RBP+0x40] = state id
if body[0x10d0] & 0x800000                      ; 0x140cb6571 BT 23
    return                                      ; no fan at all
service object = resolve(body+8, ...) again
R14B = service != 0 && body[0x1ad0] > 0
if flags & 0x80000c == 0 && !R14B && service && byte[service+0xd40] != 0
    if !virtual_0x58(service, body[0xe4]) -> skip the fan   ; 0x140cb671e
fan: 0x140cb86c0(body, source, &word[RBP+0x40], xmm3 = 0.0)  ; 0x140cb6737
if state_object: u16[state_object+0xbc] = word[RBP+0x40]
if service: virtual_0x60(service, source)                    ; 0x140cb675a
if flags & 0x80000c == 0 && !R14B && service && byte[service+0xd40] != 0
    virtual_0x68(service, source, body[0xe4])                ; 0x140cb6785
```

Two consequences matter for movement:

- **The fan is the only wish producer on this path**, and it is called with
  `dt = 0` and the stack flag `1`. Its input direction is `body+0x1140`, which
  the wish spring wrote from the *previous* frame's `source+0x00`, so the wish
  is a **closed loop seeded by its own last value**. Nothing here converts
  WASD buttons into a direction.
- The service object's `+0x58/+0x60/+0x68` virtuals are hooks around the fan
  (can-move, post-source, post-source-with-tick), gated by the same
  `body[0x10d0] & 0x80000c` mask and `body[0x1ad0] > 0`.

`FUN_140cba030` is an interior `.pdata` chunk, not a real entry: the image's
`.pdata` is chunked throughout, and Ghidra models each chunk as a function.
The real entry of the input builder was not established; treat
`FUN_140cba030` as an address inside the parent function, not a callable
entry. The same chunking explains why `0x140cbbaef` and `0x140cb5b65` show
`unaff_*` registers; the real entries are `0x140cbb260` and `0x140cb5760`.

## The live per-frame velocity law (2026-11-01)

Both live update paths end by handing one 128-bit vector to
`SetCharacterControllerVelocity` (`0x142cbd1b0`). The vector is
`state_transform(wish) + ramp_vector(body)`, built in this order:

```text
MoverUpdateFrame (0x140cbb260)
  0x140cbbdc2  movups xmm0, [rsi + 0x1140]   ; the wish vector
  0x140cbbdd6  call   FUN_140ca5c50          ; state transform, in place
  0x140cbbde6  call   FUN_140ca75b0          ; ramp vector -> [rsp + 0x50]
  0x140cbbdf5  addps  xmm0, [rsp + 0x50]     ; state + ramp
  0x140cbbe0b  call   SetCharacterControllerVelocity([rsi + 0x1ad8], &v)

MoverUpdateCommand (0x140cb5760)
  0x140cb5897  call   FUN_140ca75b0          ; ramp vector -> [rbp - 0x10]
  0x140cb589c  movaps xmm0, [rbp - 0x10]
  0x140cb58a4  addps  xmm0, [r14 + 0x20]     ; ramp + state
  0x140cb58b4  call   SetCharacterControllerVelocity([r15 + 0x1ad8], &v)
```

`FUN_140ca5c50` is the verified state transform (`fn/MoverStateVector`) and
`FUN_140ca75b0` the verified ramp leaf (`fn/MoverRampVector`), so the law adds
no new arithmetic: it fixes the **order** of the two terms and the operand
order of the `ADDPS`.

- **The two paths add in opposite operand order.** `MoverUpdateFrame` adds
  `state + ramp` (`xmm0` is the state vector, the memory source is the ramp);
  `MoverUpdateCommand` adds `ramp + state` (`xmm0` is the ramp, the memory
  source is the state vector at `input + 0x20`). `ADDPS` is commutative for
  finite values and differs only in which NaN payload survives, so the two
  paths are not bit-identical on a NaN lane.
- **Both pass `body + 0x10d0` as the ramp's third argument.**
  `0x140cbb890` (`lea r15, [rsi + 0x10d0]`) sets it in `MoverUpdateFrame`;
  `0x140cb5889` (`lea r8, [r15 + 0x10d0]`) passes it directly in
  `MoverUpdateCommand`. The ramp therefore reads its flags byte at
  `body + 0x10d0` and its offset at `body + 0x10fc` on both paths.
- **The state vector is a no-op when the lookup declines.** `FUN_140ca5c50`
  leaves the wish vector untouched on the sentinel id, so the velocity is the
  raw wish plus the ramp on that path.
- **`MoverUpdateCommand` gates the velocity on `input + 0x4c != 0`**
  (`0x140cb5882`); `MoverUpdateFrame` does not re-gate here (its gate is the
  input build's `+0x98` byte, already applied by the wish spring).
- **What the published vector *is* is not settled.** `0x142cbd1b0` forwards to
  `0x142cf5810`, which dispatches on the variant tag (`[variant] == 1` ->
  `0x142cb5310`, else `0x142cd5370`), and both end in a setter that writes the
  vector to the entry's `+0x10` and the normalised quaternion next to it
  (`0x142cd4bc0` writes `entry+0x10` / `entry+0x00`; `0x142cb22a0` writes
  `entry+0x10` / `entry+0x20`). The matching getter `0x142cbcec0` ->
  `0x142cf5630` reads the same `+0x10` (character entry through
  `0x142cd4ff0`, rigid through `0x142cb4770`). For the rigid case that record
  is a `dmBody` (stride `0x1c0`, index `+0x00`, generation `+0x198`), whose
  `+0x10` `formats/domino.md` records as the solve writeback's **aux vector**
  while the body's position is `+0xf0` and its linear velocity `+0x30`. So
  `+0x10` is read and written as one value by a get/set pair, but whether the
  game treats it as a position-like transform or as a velocity is **open**;
  the name `SetCharacterControllerVelocity` is the client's own symbol
  convention, not evidence. This matters: it decides whether the live path
  needs an integrator at all, so no position or velocity claim is made here.

`ow-sim::frame` implements this composition: `state_vector`, `ramp_from_body`,
`frame_velocity` / `command_velocity` (the two operand orders) and
`update_frame`. The composition has **no client-side unit** — its two operands
are separate verified units — so the Rust is tested against the read
expressions only; the doc claims the order, the units claim the arithmetic.

This settles what the control law *is* on the live path. It does not settle
the wish's producer (`FUN_140cb86c0` below) or how a velocity becomes a
position (the Domino tick and the collision geometry).

## The wish source (2026-11-01)

The source of the input object's wish vector is now read:

```text
first-person view object (vtable 0x142f87810, slot +0x250 = 0x140654530)
  -> 0x140654730
       writes actor+0x1210..+0x1223 (0x14-byte usercmd):
         +0x04 yaw, +0x06 pitch (s16 fixed-point; pitch clamped to +-0x3fff),
         +0x08..+0x0b packed bytes, +0x0c button/flag dword, +0x10 flag word
       pushes 0x1c-byte entries into the ring at actor+0xe30 (0x140c12fb0)
       records the sequence at actor+0xe14

FUN_140689930 (probable driver 0x14214170e in FUN_142141460)
  -> [actor+0x38] + 0x178 -> FUN_14068cf10
  -> 0x14-byte command -> FUN_140cbb260 copies it to body+0xd0/+0xe0

body+0xf0 state-record slot 8 / FUN_140cc1060
  -> FUN_140cb63c0 -> FUN_140cb86c0
       source+0x00 16-byte wish, source+0x10 secondary, source+0x40 gate
  -> FUN_140cba030 copies source+0x00 -> input+0x60, +0x40 -> input+0x98
  -> MoverWishSpring
```

The wish vector is **not** a direct copy of the actor command.
`FUN_140cb86c0` does not read `body+0xd0/+0xe0`; it builds an input vector from
`body+0x1140`, the controller position (`0x142cbcec0([body+0x1ad8])`) and
`body+0xc0`, then rotates it through the state block:

```text
FUN_140cb86c0 -> FUN_140ca5b90 -> FUN_140cb5350 -> FUN_140cb83b0
  -> FUN_140c71e10 (copies 16 floats from the hashed state object +0x60..0x9f)
  -> rotate the input vector in place
```

The transform is a 4x4 **column-major affine**, not a quaternion:
`M = [c0 c1 c2 c3]`, and the forward map is
`v' = x*c0 + y*c1 + z*c2 + c3` (`FUN_140cb5510`, SIMD at
`0x140cb5534..0x140cb5563`). The inverse (`FUN_140cb5350`) computes
`det = c0 · (c1 × c2)`, `R^-1` rows `(c1×c2)/det`, `(c2×c0)/det`,
`(c0×c1)/det`, then `v' = R^-1 * (v.xyz - c3.xyz)`; it uses `rcpps` plus the
`(2 - rcp*x)*rcp` refinement, so the approximation is part of the exact
result.

`FUN_140cb86c0` is **not** a closed-form wish: it builds the seed
`s = d.x*body[0xc0] + d.y*body[0xc4] + d.z*body[0xc8]`, where
`d = controller_position - M*(body[0x1140].xyz,1)`, then runs a fan loop over
`body[0x5bc]` directions (`2π/N`), composes candidates through `0x1420328c0`,
and stores the chosen world candidate transformed back by `FUN_140ca5b90` to
source `+0x00`. The per-iteration fan algebra is still unread. The gate is
written `1` at `0xcb9f91` normally, and `0` at `0xcb9fb5` when
`body[0x610] <= 0 && dt <= 0` or `R12B == 0` (input inactive). The two call
sites pass different `dt`: `FUN_140cb63c0` passes 0.0, `FUN_140cba030` passes
10.0.

The state object is the state record's transform object: `FUN_140c71e10`
copies 16 floats from `state_object+0x60..0x9f`; the selector is deterministic
(`0x142001f80` is a fixed setter, not random), and the object is found from
`[state_record+0x110]`, the table at `[state_record+0x80]` and
`byte[state_record+0x130]`, then decoded with the obfuscation helper and the
globals. A synthetic state record can make the writer run under emulation;
the blockers are the helper, the decode globals and the body fixture.
`FUN_140cb63c0` calls the object resolved from `body+8`: slot `+0x58` can skip
the normal computation, `+0x60` receives the source struct, and `+0x68` is
called last when `object+0x1a8 != 0`. Their concrete targets depend on the
runtime service object and remain unread.

The candidate driver `0x14214170e` is **not** ruled out by the
`0x142f88c90` value seen in the actor constructor: `[actor+0x1660]` is a
different field from `[actor+0x38]`, and the constructor does not write
`+0x38`, so the object is runtime-attached. The call site is **probable, not
proven**; the vtable of the object at `actor+0x38` is the remaining proof gap.

`0x14068f1aa -> 0x140cbd260` is a **dead/obfuscated duplicate**: its function
`FUN_14068e530` has no callers or data refs, its block is not a branch target,
and `0x140cbd260` is a thunk into the middle of `ApplyStateModifiers`. Only the
`0x14068cf10 -> 0x140cbb260` path should be implemented.

**Corpus pass (2026-11-01).** The movement ranges are in `dev/corpus.py`,
stored at `dev/data/corpus` (gitignored): nine tags, 1,544 `.pdata` bodies,
every one with a linear disassembly from the image, 694 with a Ghidra
decompile from the pinned `Overwatch.exe` program. The decompile of `FUN_140cb86c0` is truncated (it ends at a
non-returning call and omits the fan loop), but the disassembly is not:
`show 0x140cb9263 --window` reads the loop locally, and the same command
works outside every `.pdata` chunk, such as the ramp leaf at `0x140ca75b0`.
The corpus confirmed the writer's two time steps — `FUN_140cb63c0` passes 0
and `FUN_140cba030` passes `0x41200000` (10.0) — and the actor command
producer: the view-object body pushes `+0x1210` through `0x140c12fb0` into the
ring at `+0xe30`, and `0x140c135d0` installs the next record at `+0x1224`.
The truncated head still names the helper ring `0x140ca7400`, `0x140ca7430`,
`0x140ca72b0`, `0x140c9e670` and the compose/collision calls `0x1426cd3d0`,
`0x1426de990`; those are the next reads. Field greps add `+0x610` readers at
`0x140ca72d0`, `0x140cb2010`, `0x140cb3cf0`, `0x140cb4f00`, the `+0x12b0`
consumer `0x1406895d0`, and three more constructors carrying the same service
constant (`0x140cac6e0`, `0x140caca70`, `0x140cae700`).

**Next units in order.** (1) the forward transform `FUN_140cb5510`, (2) the
inverse `FUN_140cb5350`, (3) the `FUN_140ca7700` wrapper, (4) a narrowed
`FUN_140cb86c0` with the full body/state fixture. Both transform leaves are
inlined leaf blocks; a unit starting inside the block needs the vector pointer
in `RBX` (not `RCX`) and, for the forward block, `AL = 1` past the state-lookup
branch. The current harness only seeds `RCX/RDX/R8/R9` and zeroes `RAX`, so
the leaves need either a register-argument extension or a synthetic state
object that lets the real entry run.

## The fan solver inside the wish writer (2026-11-01)

The writer `FUN_140cb86c0` (`0x140cb86c0`) takes
`(body, source, &u16 index, dt)` — `rcx` body, `rdx` source, `r8` the index
out, and `xmm3` the time step — and fills the input source struct from a
small search. The read structure, in order:

```text
0x140cb8747  A = 0x140ca7400(body)        ; clamp(body[0x5c8]*pi/180, 0, 1.5707844)
0x140cb8750  B = 0x1426cd3d0()            ; generic helper (unread), the score threshold
0x140cb8761  pos = 0x142cbcec0(body[0x1ad8])
0x140cb877f  MoverWishCopy(body, &transformed, &body[0x1140])
0x140cb878e  s = dot(pos - transformed, body[0xc0])      ; (y+x)+z order
0x140cb87c3  if body[0x10d0] & 0x80000c: speed = 0     ; no search
             else: speed = fan_speed (below)
0x140cb888c  s += 0.1
0x140cb8891  0x140c9e670(body+0x10d0, &query)           ; state query
0x140cb88cd  0x142cbcea0(handle, &block)               ; {0,0,0,0, 0,0,0,0, 0, -1}
0x140cb88d9  pos2 = 0x142cbcec0(body[0x1ad8])
0x140cb88e7  if body[0x1520] != sentinel:               ; 0x1441880e8
0x140cb88fd    ramp = 0x140ca75b0(body, body+0x10d0)
0x140cb892c    s += dot(query - (pos2 - ramp), body[0xc0])
```

The fan's speed (`0x140cb87d6..0x140cb8874`):

```text
len2 = (vy*vy + vx*vx) + vz*vz                 ; v = body[0x1150]
len  = len2 > 1.17549435e-36                   ; FLT_MIN, 0x142f7aad0
       ? len2 * ((3 - len2*rcp*rcp) * (rcp*0.5)), rcp = rsqrtss(len2)
       : 0
cap   = max3(body[0x5d0], body[0x5d4], body[0x5d8])   ; flags bit 1 -> +0x5dc..+0x5e4
speed = ((minss(len, cap) * 1.1) * (body[0xe8] * 1e-6 * E(A)))
speed = maxss(speed, body[0x5cc])
```

- `0x140ca7400` is `clamp(body[0x5c8] * 0.017453292, 0, 1.5707844)`: a turn
  limit in radians.
- `0x140ca7430` is a **max3** of the three slots, not a median: it tests
  `a >= b`, `a >= c`, then `b >= a`, `b >= c`, and returns `c` when neither
  pair holds. `COMISS`/`SETNC` makes an unordered comparison false, so a NaN
  never wins.
- `0x140ca72b0` is `(f32)(i64)body[0xe8] * 1e-6` — a frame counter in
  seconds. `E` is `0x1426de990(A)`, a generic helper (82 call sites) that is
  not read.

Three search passes score candidates through the evaluator `0x1420328c0`
(`(body, &out, &block, &record)` returning a bool) and keep the best. The
evaluator is itself a thin wrapper: it fills a small descriptor (a `1.0` at
`+0x50`, a `1` at `+0x54`, a `0` at `+0x58`, the out pointer and the block)
and calls `0x14203dae0` (`0x14203292b`), which returns the result.

**The evaluator is a collision query.** Its first argument is
`[body+0x240]` — the same spatial world the mover's other geometry paths pass
to `0x14202fb00` (`0x140ca72d0`, `0x140cb1fc1`, `0x140cb20a5`, `0x140cb8b33`,
`0x140cb8e4e`) — so a candidate is accepted only if the character can reach
it. The caller seeds the index from `body+0xbc` before the call
(`0x140cba3d0`), passes `dt = 10.0` (`0x142f84b3c`, the input builder's time
step) and reads back `source+0x00` (the wish) and `source+0x40` (the gate).

`0x14203dae0` is **1158 bytes in the image's obfuscated region**. A trace of
the function (below) shows the *reachable* code is small — the prologue
`0x14203dae0..0x14203dbd9`, the world-table lookup `0x14203dc51..0x14203ddf1`
(entered only when `[0x143b795b0]` and `[0x143b795a0]` are non-null), and the
body `0x14203de51..0x14203e0fe` — and the scattered blocks (`0x14203f5bb`,
`0x1420404ab`, `0x142046b12`, … `0x1420543f2`) are junk that real data does
not reach. The lookup reads a sorted `i32` table: `count` at the object's
`+0x00`, array pointer at `+0x08`; a miss continues to the guard.

**The guard is constant-keyed, and the equal branch is a trap (2026-11-03).**
The entry at `0x14203de51` is a misaligned decode of the preceding instruction:
it **writes** the constant `0x26` into its own stack slot `[rsp+0x20]`, then
feeds that byte through two one-instruction stubs (`0x14053de90` = `add byte
[rcx], 0x84; ret`, `0x14053be10` = `add byte [rcx], 0x7c; ret`, net +0x100)
and back into the slot, so the value is unchanged. The decode is
`nibble(0x26) = 0x96`, then `^ [0x143847d3c]` (`0x17`), `ror 5`, then `^
[Q+0xdd0]`, compared with `0x40`; the nibble table is at **`0x142f7d158`**
(the older `0x1433d158` was a digit short). The guard therefore depends on the
anti-tamper block `Q = [0x143b79598]`, **not on the world pointer** — the
earlier "pointer-keyed" reading is withdrawn.

The equal branch (`0x14203deb6`) reads `[r12]`, and `r12` is zeroed at
`0x14203db79` on every path that reaches the guard, so that branch is a
**null-byte read trap**: it is not a second encoding of the body. The two
branches do not "both agree"; there is one body and one canary. A probe that
seeds `Q`, a world and a shape now runs the whole chain and returns a scored
candidate (see below), which is the test the earlier note asked for.

**The body is read.** With `param_1` the world, `param_2` the descriptor and
`param_3` the caller's block:

```text
0x14203debb  rax = [world+8]; hash -> [.. +0x28] -> the service object
0x14203df15  cmp [world+0x728], <global slot>     ; a cached handle check
0x14203df20  call 0x14054ce70, cmp [rax+0x20], world
0x14203df33  geometry = [world+0x620]
0x14203df3d  if (geometry == 0) return 0
0x14203df48  if (word [geometry+0x31f8] == 0xffff) return 0
             ; fill the result record `rbx = [descriptor+0x10]`:
0x14203df76  [rbx+0x10] = const quad;  [rbx] = const quad
0x14203df84  [rbx+0x28] = global qword
0x14203df88  [rbx+0x30] = 1.0f
0x14203df8f  [rbx+0x34] = -1 (qword)
0x14203df97  [rbx+0x40] = 0;  [rbx+0x3c] = 1;  [rbx+0x48] = 0
0x14203dfa6  [rbx+0x20] = 0
             ; build a 0x71-byte query struct at rbp-0x79 from the descriptor
0x14203dfaf  if (word [desc_obj+2] == 0) goto 0x14203e048
0x14203e035  call 0x142cc45a0(ecx = u16 [geometry+0x31f8],
                              rdx = a constant, r8 = &struct)
0x14203e048  ... opaque check on [world+0x6a8], then
0x14203e078  if (byte [desc_obj+0x18] == 0) goto exit
0x14203e07e  if (byte [desc_obj+0x1c] == 0) goto exit
0x14203e0b3  call 0x14212db80(rcx = <opaque>, rdx = &struct)
0x14203e0b8  sil = (byte [rbp-0x41] != 0)
0x14203e0d6  return 0
0x14203e0d0  return sil
```

The result record `rbx` is exactly what the fan reads back (`+0x34` u32,
`+0x38`/`+0x3a`/`+0x3c`/`+0x3e` u16, `+0x40` qword, `+0x48` byte), so the
evaluator's output contract is now known even though its arithmetic is not.
The descriptor object (`[param_2+8]`) supplies `+0x450`/`+0x460` (two vec4),
`+0x480` (vec4), `+0x490` (qword), `+0x498` (u32), `+0x49c` (f32),
`+0x4a4` (byte), `+0x18`/`+0x1c` (bools) and `+0x00`/`+0x02`; the world
supplies `+8` (the hashed service slot), `+0x620` (the geometry object),
`+0x6a8` and `+0x728` (a cached handle).

`0x142cc45a0` is a **handle dispatcher** that resolves a `u16` handle through
the table at `0x1442a0bc0` (the same table the position lookups use; the
decode is `(0xb3269b88d44d5fb2 + entry) ^ 0x77600ba5adcc265c`, i.e. the
subtract form the docs already record). It rejects `handle > 0x3fff` and the
sentinel `0xc439701cd97ec6aa`, then:

```text
record+0x1b0                -> the query's first argument
record+0x4a0 (array), +0x4a8 (count), +0x4ac (cursor),
record+0x4b0 (wrap), +0x4b4 (counter)  -> an island-pool cursor (stride 0xaa0)
builds a 0x30-byte descriptor: {record, block[0x58], const, block[0x60],
                               island slot, 0}
builds a 0x78-byte struct at rsp+0x50 from the caller's block
call 0x142cbd510(record+0x1b0, &descriptor, &struct, r9b = block[0x5e])
```

The island-slot branch is gated on `byte [block+0x71]`, which the evaluator
always writes as `0` (`0x14203e022`), so it is dead on this caller; the
`0x142cadf00` thunk it would call is a dynamic-import trampoline
(`rax = [global]; jmp rax` else `jmp 0x141b328e0`), not the query.

**The evaluator runs offline on a built world (2026-11-03).** `dev/probe_candidate_evaluator.py`
now runs `0x1420328c0` end to end under the harness oracle and prints the
answer record. It seeds the two globals the prologue tests, the guard block,
the service chain, a domino record and one type-3 shape, then calls the
wrapper. With a two-quad synthetic mesh and a sweep from `(0.75, 0.9, 5)` to
`(0.75, 0.9, -5)` the client returns **1** (accepted) and fills the record
with point `(0.75, 0.9, 0)`, normal `(0, 0, 1)`, fraction `0.5`,
`word34 = 1`, ids `0x12`/`0x22`, key `0`, slot `0xffff` — the same triangle
the `fn/MeshSweep` unit pins, reached now through the whole evaluator.

What the probe had to build, all from the client's own reads:

```text
world+8 : the service slot, encoded; the body's decode is
          add 0x1e2421a4798a328, ror 0x22, xor 0xe1379e3cb308170d, ror 0x2b,
          then [p+0x28], sub 0x72184667dd2c7821, xor 0x8ef3269b22f53799,
          add 0xb9e8ebfc25914471, load. Encoding a target is the inverse.
world+0x620 : geometry; u16 handle at +0x31f8
0x1442a0bc0 : holds a pointer to the handle table (not the table itself)
record+0x1b0 + 0x28*pass : +0x30 BVH array, +0x44 root index (-1 = empty)
record+0x2b8 : 0xc0 shape entries
record+0x2a0 : transform pool, 0x50 stride: quat at +0x00, position at +0x10
               (static path reads element +0x50, not +0x24)
record+0x318 : type-3 pool, 0x50 stride: scale, 1/scale, root, vertices,
               triangles
entry : type byte +0x20 (3 = mesh), type index +0x24, transform index +0x50,
        layer masks +0x98 (four u16), mode byte +0xb0 (0 = static)
BVH node (0x40) : box min +0x00, box max +0x10, shape id +0x20,
                  -1 leaf marker +0x2c
```

The query block the driver receives is built by `0x142cc45a0` from the
evaluator's 0x71-byte struct: start/end quads, motion splat, support pointer
and count, a constant rotation quad, the descriptor object's `+0x480` splat as
expansion at `+0x50`, the constant `0.005` splat at `+0x60`, the layer mask at
`+0x70` and the byte at `+0x72`. The pass flags `r9b` are byte 6 of the
descriptor object's `+0x00` qword, which is the same qword whose first two
`u16` lanes are the layer filter — so one 64-bit field carries masks, flags
and the `+0x02` word the handle path tests.

**The range's real geometry runs, and the pinned hit is reproduced
(2026-11-03).** `crates/ow-tank/examples/mesh_world_dump.rs` writes the world
`ow_tank::mesh_world` assembles (272 shapes, 4,797,872 bytes, base
`0x1_0000_0000`) plus each shape's three array addresses; the probe's
`--mesh-world` builds the record, entries, both pools and a BVH around it. With
the one pinned shape in the BVH and the `mesh_world_real` query (a quarter of
the way in on x and z in f32, swept from `high_y + 2` to `low_y - 2`), the
client returns the pinned hit **exactly**: point `(0.99500012, 4.0,
0.99500012)`, normal `(0, 1, 0)`, fraction `0x3e8051b8`, triangle `7`.

Two details the record exposes:

* the answer record's `+0x3c`/`+0x3e` are the policy's **resolved** id and
  kind, not the triangle's ids. Triangle id `0x8000` is a negative `i16`, so
  the policy looks it up in the geometry's variant table and falls back to the
  pending id `1` when that table is not built — which is what the record shows.
  A positive id passes through (the synthetic case's `0x12`/`0x22`).
* with `--all-shapes` the pinned query hits **the same placed surface as the
  single-instance run**, byte for byte: every shape carries its placement, so
  the broadphase selects the instance under the query instead of whichever
  overlaid shape came first. The point is a map coordinate
  (`(23.948, -19.991, 31.370)` on the range), not a local one. Before the
  placement was applied the same query hit another shape at y `5.843`,
  fraction `0.0208`, triangle `7054`.

**What is still open here.** (a) ~~the placeable transform~~ — **read**: the
  scene component's `+0xd0`/`+0xf0` come from its instance data through
  `0x142003720` and `0x142000620`, and the map chunk carries the same
  `(translation, scale, rotation)` triple per placed model (see "The shape
  world transform is read" below); (b) a Rust implementation of the four-pass driver
(the narrowphase and the policy are already in `ow-sim`), which is what a
`fn/CandidateHitPolicy` unit needs; (c) the geometry's variant table, which
would let the policy resolve negative ids instead of returning `1`.

**`0x142cbd510` is the collision query driver**: 877 bytes
(`0x142cbd510..0x142cbd87c`), four passes over one island's shape list. It
reads the eight quads of the block at `+0x00`/`+0x10`/`+0x20`/`+0x30`/
`+0x40`/`+0x50`/`+0x60`/`+0x70` and calls:

```text
0x142cd5540  once per pass   the sweep's start/end/expansion (verified unit)
0x140d24b60  per child       the worklist push (container +0x408 size,
                             +0x40c capacity, u32 elements)
0x142cc1610  per leaf shape  shape transform + layer filter + narrowphase
0x142cadf00  optional        island-slot release (dead on this caller)
0x1426c3d40  once            the stack-cookie check
```

So the chain from the fan to collision is now complete at the call level:

```text
FUN_140cb86c0 (fan, three passes)
  -> 0x1420328c0 (wrapper; descriptor {0, out, record, 1.0, 1, 0})
  -> 0x14203dae0 (body; geometry = [world+0x620], handle = u16[geometry+0x31f8])
  -> 0x142cc45a0 (handle -> domino record via 0x1442a0bc0)
  -> 0x142cbd510 (query driver, 4 passes)
  -> 0x142cd5540 (sweep bounds; verified)
  -> 0x142cc1610 -> 0x142cb8b80 / 0x142cbb350 / 0x142cb8d60 (per-type narrowphase)
```

**The driver is read (2026-11-01).** With `A = record+0x1b0` (the first
argument), `D` the 0x30-byte descriptor and `B` the 0x78-byte block, the
driver makes four passes, one per bit of the flag byte `r9b` (bit 0 first; a
clear bit skips the pass). Each pass owns a 0x28-byte sub-record at
`A + 0x28*pass`: the shape index at `+0x44` seeds the worklist (`-1` means
this pass has no shapes) and `+0x30` is the shape-entry array base.

The worklist is the container at `[rbp+0x20]`: a u32 array (inline at
`+0x08`, 0x100 entries) with its size at `+0x408` and capacity at `+0x40c`. It
is popped with `size -= 1` and pushed by `0x140d24b60(container, &index)`,
which appends the u32 and grows the array to `capacity + capacity/2` (minimum
2) through the allocator at `0x142f7aa00`. So `0x140d24b60` is the worklist
push, not a broadphase bucket grow.

Per shape entry (stride `0x40`: min `+0x00`, max `+0x10`, id `+0x20`, child
indices `+0x2c`/`+0x30`):

1. `0x142cd5540(D, &expansion, &start, &end)` builds the sweep; `delta = end -
   start`; the swept box is `min(start, start + motion*delta)` ..
   `max(start, start + motion*delta)` with `motion = B+0x20` (a per-axis
   fraction, a splat of the caller's scalar) and the epsilon
   `|delta| + FLT_EPSILON` (`0x1435ecad0`).
2. Axis-aligned rejection: reject when `(entry.max + expansion) - swept_low`
   or `swept_high - (entry.min - expansion)` is negative on x, y or z (the w
   lane is ignored).
3. Moving-box separation: with `mid = (max + min) * 0.5`, `half = (max - min)
   * 0.5` over the expanded box, `b = start - mid`, `d = delta` and `e = |d| +
   FLT_EPSILON`, the client forms the cross-product lanes `d x b` and `b x d`
   and rejects when `|(d x b) - e*half - (b x d)| - 2*(e*half)` exceeds `0.005`
   (`0x1435ecaf0`) on any of x, y, z. The lane order, the `SHUFPS` wiring and
   the `MINPS`/`MAXPS` NaN rule are reproduced in `ow-sim::collide`.
4. `entry+0x2c == -1` marks a leaf: `0x142cc1610(D, &hit, B, entry+0x20)` runs
   the narrowphase. Its return drives the driver: `1` ends the sweep, `2` is a
   hit, anything else continues. The hit is folded into **two** running quads,
   not one (read 2026-11-06): `XMM6` is a per-lane `minps` over every `2`
   answer (`0x142cbd7a1`) and is what the driver returns (`0x142cbd7ff`,
   `XMM0 = XMM6`); `[rbp-0x40]` is the operand the four-lane test compares
   against (`CMPLTPS hit, [rbp-0x40]` at `0x142cbd799`) and is rewritten to the
   hit itself only when all four lanes improved (`0x142cbd7ae`). So after a
   partial hit the minimum keeps shrinking while the comparison operand stays
   at the older, larger value. Each pass that runs re-seeds the operand from
   the minimum (`0x142cbd603`, `MOVAPS [rbp-0x40],XMM0` where `XMM0` is the
   previous pass's return), so the two only diverge inside one pass. The
   all-lane branch also re-seats the swept box on `start + hit*delta`
   (`0x142cbd7a9..0x142cbd7c3`).
5. otherwise `entry+0x2c` and `entry+0x30` are pushed as child indices.

Two details the earlier note had wrong. The flag byte is shifted once per pass
whether or not the pass ran (`SHR R15B,0x1` at `0x142cbd80f`, on the shared
tail), and the driver's return value is the folded **motion quad**, not a hit
count - there is no counter in the function. `ow-sim::collide::query_driver`
carries both folds (`fold_min` returned, `fold_cmp` the operand) and five
tests pin the walk: the child order, the AABB prune, the flag-bit skip, the
`1` answer ending a pass, and the per-lane minimum.

So `0x142cbd510` is a bounding-volume hierarchy walk over one island's shape
list: the four passes sweep the character's local support points, the per-shape
AABB and moving-box tests prune, and only leaves reach a narrowphase.

**The driver is verified against the client's bytes** (`fn/QueryDriver`, 208
cases, 100% branch coverage with one declared dead edge, 11/12 mutants
killed). Two limits are recorded rather than hidden. The worklist's growth
path is unreachable under emulation: past the inline `0x100` entries the push
reallocs through the global allocator object at `0x144188200`, which is zero
in the offline image, so branch site `0x142cbd7f3` carries a declared dead
edge and the realloc's copy and the free stay unproven — the push order and
the pop order are pinned by the passes that stay inside the inline buffer. The
hit policy is a constant-returning stub, so the four verdicts `0`..`3` are
supplied: the unit proves that the wrapper's answer selects continue,
end-the-pass and fold, not what the real policy decides. The corpus also
showed that a long-lived emulator instance carries state between calls (after
about ninety calls every pass stops folding), so each case runs on a fresh
instance; nothing in the client's own code accounts for that, it is an
emulator artifact and it is why the unit pins `freshOraclePerCase`.

**The narrowphase wrapper is read.** `0x142cc1610(D, &hit, B, id)`:
`entry = [record + 0x2b8] + id*0xc0` with `record = [D+0x00]`;
`0x142cb8b80(entry, &xform, record)` builds the entry's world quaternion (from
its own stored rotation, or from the owner pose at `[record+0x288] +
entry[0x50]*0x1c0` when `record[0x191] != 0` and `record[0x192] == 0`);
`0x142cbb350(&D+0x08, entry+0x98)` is the layer filter: two 16-bit mask
intersections (`a[2]&b[0]` and `a[0]&b[2]`) and a signed-id rule. With the
query's `+7` mode byte set the test is the first mask alone. Without it the
ids decide: the zero pair passes, equal magnitudes pass only for two positive
ids, and different magnitudes pass only when `x <= 0` and `y <= 0` - a
negative id never matches itself, because the client's `TEST`/`JNZ` skips the
equality shortcut for a non-zero `x`. Verified as `fn/CollisionShapeFilter`
(100% branch coverage, 12/12 mutants, and the whole return register compared,
since the leaf leaves the second mask's high byte in `EAX` above the bool in
`AL`). A filtered shape returns 3 after appending a
contact event when the descriptor has an event list. Otherwise
`0x142cb8d60(entry, record, &xform, &out, B)` dispatches on the shape type at
`entry[0x20]`, indexing the pool by `entry[0x24]`:

| type | primitive | pool base | stride |
|---|---|---|---|
| 0 | `0x142ccdf40` | `[record+0x2d0]` | 0x20 |
| 1 | `0x142ccd7b0` | `[record+0x2e8]` | 0x30 |
| 2 | `0x142cd3c80` | `[record+0x300]` | 0x50 |
| 3 | `0x142caed10` | `[record+0x318]` | 0x50 |
| 4+ | `0x142cd0ae0` | `[record+0x330]` | 0x50 |

A primitive hit returns `D[0x10](&out, {entry[0xa0], entry[0xa8],
record[0x194]}, D[0x18])` - the descriptor's own hit policy, a constant code
address the evaluator passes in - copies the narrowphase's third output quad to
the driver's hit vector, and optionally appends a contact event
(`0x142cc1450`). Note that this is a **different** shape encoding from the
controller's: `0x142cb9030` dispatches the character shape list by a type byte
with its own bounds leaves, while this tree's entries carry a type at `+0x20`,
pool index at `+0x24`, filter words at `+0x98` and ids at `+0xa0..0xae`.

**The driver's arguments are read from `0x142cc45a0`.**
`0x142cc45a0(handle, struct, descriptor_object)` rejects `handle > 0x3fff` and
the sentinel `0xc439701cd97ec6aa`, decodes the record as
`(0xb3269b88d44d5fb2 + [0x1442a0bc0][handle]) ^ 0x77600ba5adcc265c`, and
builds the `0x30`-byte descriptor `{record, [obj+0x58], hit_policy,
[obj+0x60], island_slot}` (`hit_policy` is the caller's `rdx`, a constant code
address) and the `0x78`-byte block `{start, end, splat(motion), points, count,
quaternion, splat(radius), splat(height), obj[0x58] u16 + obj[0x70] byte}` from
the caller's 0x71-byte struct. The island-slot branch (`obj[0x71] != 0`) takes
a slot from `record+0x4a0` (stride 0xaa0, cursor `+0x4ac`, count `+0x4a8`) and
copies the struct's eight quads into it; on this caller the byte is always 0,
so the branch is dead here.

**Verified unit.** `fn/CollisionSweepBounds` runs the real `0x142cd5540`
against `ow_sim::collide::sweep_bounds`: 264 iterations + the edge pool (328
cases), every channel (the block pointee and all three output quads), 100% of
the 3 reachable branch sites, 11/11 mutants killed. The image's `.pdata` entry
for the leaf is truncated at 0x42 bytes, so the unit declares the exact extent
(`0x1a0`) as its coverage window.

What is still unread is the narrowphase proper: the five per-type primitives,
the shape world transform's dynamic branch, and the geometry that fills the
pools at `record+0x2b8` and `record+0x2d0..0x330`. That is now a bounded set of
named functions instead of an unknown.

**The primitive inventory (2026-11-01).** Sizes from `.pdata`, all open:

| target | `.pdata` size | shape of the body |
|---|---|---|
| `0x142ccdf40` (type 0) | 0x4ec | record `{center +0x00, radius +0x10}`; a support-point path that calls the shared `0x142cc9460`, and an inline swept path (record centre subtracted after a quaternion rotation) |
| `0x142ccd7b0` (type 1) | 0x78a | same two-path shape, its own inline test |
| `0x142cd3c80` (type 2) | 0x3f0+ | a fast path gated on `expansion == 0` (all lanes `<=` the constant at `0x1435ecf20`) **and** `count == 0`; otherwise a general path |
| `0x142caed10` (type 3) | 0x1a | a wrapper: zeroes two stack qwords and tail-calls `0x142caed30` (the mesh path) |
| `0x142cd0ae0` (type 4+) | 0xc00 | the largest of the five |

The shared support-point routine is `0x142cc9460`; the type-3 mesh body is
`0x142caed30`. Both are unread.

**The type-0 primitive is read and verified (2026-11-01).** `0x142ccdf40`
takes `(record, &out, block, &xform)`. The record is `{centre +0x00, radius
+0x10}`, the transform is `{position +0x00, world->local quaternion +0x10}`
(the order `0x142cb8b80` writes), and the block is the query block. The leaf
has two paths:

* `block[0x38] > 0` (support points): it packs a `0x80`-byte info block and
  calls `0x142cc9460`; **open**.
* `block[0x38] <= 0`: the analytic swept-sphere test. The local start is
  `p = rotate(start - position) - centre`. If `|p|^2 < (r + expansion)^2` the
  sweep starts inside: it reports only when `block[0x72]` is set, with the
  start position and the normalised segment as the normal. Otherwise it folds
  the segment into the local frame (`d = rotate(end - start)`, `len = |d|`,
  `dir = d/len`), takes the closest approach `w = p - (dir.p)*dir`, and
  requires `(r+E)^2 >= |w|^2`, `t = -(dir.p) - sqrt((r+E)^2 - |w|^2) >= 0`
  and `t <= motion*len`; the hit is `p + t*dir`, the normal is
  `hit/|hit|` (or `-dir` when the hit lands on the local origin), the contact
  is `normal*r + centre` rotated back to world, and the third output quad is
  `minps((1/len)*t, motion)` with `1/len` masked to zero below the epsilon at
  `0x143391030`.

The leaf carries four lanes through every step (the fourth lane is
`r + expansion.w`), uses `COMISS + JBE` for the three `> eps` guards (so a
NaN takes the "no direction" side), and leaves its output untouched on a
miss. Constants: the squared-length epsilon `0x143110818` = `0x057a0000`,
the zero quad `0x142f84a00`, the sign mask `0x142f817a0`, the unit quad
`0x142fa62f0`, the scalar unit `0x142f7aae4`.

`fn/CollisionSphere` verifies this path against the real leaf: 328 cases,
every channel (the whole `0x44`-byte output pointee, including the trailing
ids), 88.9% of the reachable branch sites (the one miss is inside the
support-point path, declared), 12/12 mutants killed. The implementation is
`ow_sim::collide::sphere_sweep`.

**The type-1 primitive is read and verified (2026-11-01).**
`0x142ccd7b0` is the capsule (`record = {P0 +0x00, P1 +0x10, radius +0x20}`,
pool stride `0x30`). It has three paths:

1. **Degenerate capsule** (`|P1-P0|^2 <= eps`): it builds a temporary sphere
   record `{midpoint, radius, -1}` on the stack and calls the verified sphere
   leaf `0x142ccdf40`, so a zero-length capsule is exactly a sphere at the
   midpoint.
2. **Support points** (`block[0x38] > 0`): the same `0x142cc9460` handoff as
   type 0 - **open**.
3. **Analytic swept test** (`count <= 0`): the swept point is folded into the
   capsule's local frame (`w = rotate(start - position) - P0`), the sweep
   direction is `d = rotate(end - start)`, and the test is the standard
   line-against-segment closest approach:
   * the start is inside the **infinite cylinder** when
     `|w - (dir_axis.w) dir_axis|^2 < (r+E)^2`;
   * the axis parameter is clamped, `u = maxps(0, minps(dir_axis.w, L))` with
     `L = |P1-P0|`, and the capped volume is the test
     `|w - u*dir_axis|^2 < (r+E)^2`;
   * when the start is inside the cylinder but not inside the caps it
     **recurses into the sphere leaf** with a synthesised record
     `{P0 + u*dir_axis, radius, -1}` - the same trick as the degenerate path;
   * when it is inside both, the report is the start position and the
     normalised sweep, gated on `block[0x72]` (type 0's rule);
   * otherwise the two ends' axis coordinates are screened against
     `-(r+E)` and `(r+E) + L` (`dir_axis.w` and
     `dir_axis.(w + d*motion)`), a parallel-sweep guard rejects
     `(d_hat.dir_axis)^2 < epsilon`, and the closest approach between the
     sweep line and the axis gives `t` and the perpendicular offset, which are
     then clamped by `motion`. On a hit the contact is
     `axis_point + radius * normal` rotated back to world, the normal is the
     perpendicular direction, and the third quad is again
     `minps((1/len)*t, motion)`.

So the capsule's arithmetic is a superset of the sphere's: same local-frame
fold, same clamp-by-motion, two sphere recursions instead of one. Its
constants are the sphere's plus `0x1435ecd70` (0.5, for the degenerate
midpoint) and `0x1435ecd80` (the parallel-sweep epsilon).

The exact parameters: with `c = dir_sweep . dir_axis`, `a = dir_axis . w` and
`b = dir_sweep . w`, the closest approach is `s = (c*a - b)/(1-c^2)` on the
sweep and `u = (a - b*c)/(1-c^2)` on the axis; the entry's sweep parameter is
`s - sqrt(((r+E)^2 - |off|^2)/(1-c^2))` and its axis parameter `a + entry*c`,
which is what the two endpoint clamps test. The contact is
`w + entry*dir_sweep + P0 - normal*expansion`, rotated back to world, so the
expansion is subtracted rather than added.

`fn/CollisionCapsule` verifies it against the real leaf: 328 cases, every
channel, 94.4% of the reachable branch sites (the one miss is the
support-point path, declared), 12/12 mutants killed. The implementation is
`ow_sim::collide::capsule_sweep`, and the two sphere recursions call the
verified `sphere_sweep`.

**The shape world transform is read.** `0x142cb8b80(entry, &out, record)`
writes two quads (0x20 bytes) and is `0x150` bytes. **Both branches index with
`entry[0x50]`**, which is the attached record's own index — not
`entry[0x24]`, which is the shape's index in the `+0x2b8` entry pool:

* `entry[0xb0] != 1` (the owner path, and the path a hand-built static shape
takes): `out[0x00] = pool[idx][+0x10]`, `out[0x10] = pool[idx][+0x00]`, with
`pool = [record+0x2a0]` and `idx = entry[0x50]`, stride `0x50`. So the owner
record carries the **quaternion at `+0x00` and the position at `+0x10`**.
* `entry[0xb0] == 1` (the body path): `idx = entry[0x50]`, `pose =
  [record+0x288] + idx*0x1c0`. When `record[0x191] == 0` or
  `record[0x192] != 0` it copies `pose[+0x10]` and `pose[+0x20]`; otherwise it
  builds a normalised quaternion from `pose[+0x60]`..`pose[+0x90]`
  (`(2q*q)` magnitude fold, `sqrtss`+`divss`, then the cross-product terms).
  The two modes are selected per record, not per shape.

**The transform is implemented and verified.** `ow_sim::collide::shape_world_transform`
is the leaf, operation for operation, and `fn/ShapeWorldTransform` proves it
against the client's bytes: 328 cases, every channel, branch coverage 100%
(3/3), 9/9 mutants killed. It returns `[position, quaternion]`, which is the
`xform` the narrowphase primitives take (`position` at `+0x00`, `quaternion`
at `+0x10`), so `sphere_sweep` / `capsule_sweep` / `mesh_sweep` and the
transform now share one layout.

**Two emulator NaN fidelities, measured and recorded.** The verify harness's
unicorn is not faithful to x86 in two places the composed branch can reach,
so the unit's corpus does not depend on either and the hardware answer is
pinned by a Rust test instead:

* an invalid operation answers `0x7fc00000` in the emulator; the x86 QNaN
  indefinite is `0xffc00000`. The composed branch reaches one when the origin
  is infinite, through `(origin - pivot) * 0`;
* when both operands of an `ADDPS`/`SUBPS` are NaN the emulator does not always
  return the destination's NaN, which is what hardware does.

Both were measured on this machine with a runtime SSE probe (`black_box` on the
operands, so nothing is constant-folded: a folded probe answers `0x7fc00000`
for `inf * 0` and is wrong). Non-finite corpus values are therefore NaN only,
and only in the quaternion.

**The narrowphase wrapper `0x142cc1610` is read.** It is `void` and takes
`(ctx, &out_quad, block, shape_index)`:

* `ctx[0]` is the world record, `ctx+0x08` the layer filter's arguments,
  `ctx+0x10` the hit callback, `ctx+0x18` that callback's context, and
  `ctx+0x20` the sink (`0` = none, which skips the push entirely).
* `entry = [record+0x2b8] + shape_index*0xc0`; then `ShapeWorldTransform(entry,
  &xform, record)` and `ShapeQueryFilter(ctx+0x08, entry+0x98)`.
* filter false -> the sink record's status dword is `0` and `&out_quad` is
  **not** written.
* filter true -> a **`0x40`-byte hit record** is built on the frame:
  `+0x00..+0x20` zeroed, `+0x30 = -1`, `+0x34 = 0`, `+0x38 = 0xffff` (u16),
  `+0x3a = 0`, `+0x3c = 0`; then `0x142cb8d60(entry, record, xform, &record)`.
* the dispatch's return `0` -> sink status `2`; non-zero -> the callback runs,
  `*out_quad` becomes the record's **`+0x20` quad**, `+0x34` is overwritten
  with `entry[0xaa]` **unless the type byte is 3 or 4**, `+0x38` with
  `entry[0xae]` (u16), and the sink status is `1`.

**The type dispatch `0x142cb8d60` is a six-way tail jump**, not a function
body (there is no `.pdata` entry at that address; it is `0x92` bytes of
`MOVSX`/`CMP`/`JMP` ending at `0x142cb8df2`). The type byte is **signed**
`entry[0x20]` and the pool index is `entry[0x24]`:

| type | primitive | pool base | stride |
|---|---|---|---|
| `0` | `0x142ccdf40` sphere | `[record+0x2d0]` | `0x20` |
| `1` | `0x142ccd7b0` capsule | `[record+0x2e8]` | `0x30` |
| `2` | `0x142cd3c80` (**unread**) | `[record+0x300]` | `0x50` |
| `3` | `0x142caed10` mesh | `[record+0x318]` | `0x50` |
| other | `0x142cd0ae0` (**unread**) | `[record+0x330]` | `0x50` |

The register mapping the jump leaves in place is `RCX` = the pool element,
`RDX` = the hit record, `R8` = the **block**, `R9` = the xform. `R8` is
`0x142cc1610`'s own third argument, forwarded through its shadow-space slot
`[RSP+0x20]`; the callee reads it back as `[RSP+0x28]`. So every primitive has
the shape `(element, out, block, xform)`.

**Composition is not yet possible, and the reason is a signature mismatch, not
a missing read.** `mesh_sweep`'s Rust form is `(record, world, block, xform)`:
its second parameter is the mesh world and it *returns* the hit, whereas the
client's second parameter is the `0x40`-byte output the primitive writes. The
verified unit bridges this in its probe (`world = { base: 0x50, bytes:
&element[0x50..] }`), which means the fixture, not the client, supplies the
world's address. The narrowphase must supply it from the record itself, and
**which field of the `0x50`-byte type-3 element holds the world pointer is not
read**. Until it is, the type-3 path cannot be composed 1:1 and
`shape_narrowphase` is not implemented.

**The two attachment paths are read, and they are what select the branch.**

* `ShapeAddToBody` `0x142cb3940` decodes the handle (`generation<<48 |
gen<<32 | index`), checks `[record+0x288][index][+0x00] == index` and
`[+0x198] == generation`, then calls `FUN_142cb0a20(body, record, descriptor)`.
That allocates an entry from `+0x2b8`, calls `ShapeEntryFromDescriptor`, sets
**`entry[0xb0] = 1`**, then `entry[0x50] = body[0x00]` (the body's own index)
and `entry[0x54] = body[0x158]`, and pushes the entry onto the body's list
(`body[0x158] = entry_index`, `body[0x15c] += 1`).
* `ShapeAddToOwner` `0x142cd4ef0` does the same against `+0x2a0`/`+0x2ac` and
`FUN_142cd4350(owner, record, descriptor)`, which sets **`entry[0xb0] = 2`** and
`entry[0x50] = owner[0x20]` (the owner's own index). `ShapeWorldTransform`'s
`!= 1` test therefore sends owner-attached shapes down the `+0x2a0` branch.
* `ShapeEntryFromDescriptor` `0x142cb7f10` itself writes `entry[0x50] =
  -1` and never touches `entry[0xb0]`; the attach functions are the only
  writers of both fields.

**Where the transform comes from.** `OwnerCreateBody` `0x142cc3d90(owner_id,
config)` takes eight floats — `config[0..3]` the quaternion, `config[4..7]` the
position — validates them (no infinities, `|q|^2 > 1.17e-35`), allocates with
`FUN_142cbff20`, and returns the handle. For a **placed model** the config is
the scene component's instance data, read end to end:

* the component's `+0x20` vtable slot (vtable `0x1433632e0`, slot 4) is
  `0x142003720`, the instance-data apply. It copies three quads from
  `*(accessor(instance_data, 1) + 0x10)` into the component: `+0x160` = the
  **position** quad, `+0x150` = the **scale** quad, `+0x140` = the
  **quaternion** quad, then calls `0x142000620`.
* `0x142000620` is the local-transform writer. With `component[0x298] == 0`
  (no parent) it takes those three quads as the local transform — quaternion,
  scale, position, each accepted only when it differs from the current value
  (`q·q_new < 0.9999999`, `|pos-pos_new|^2 > 1e-8`, same for scale) — builds
  the 3x3 at `+0x60`/`+0x70`/`+0x80`, and writes **`+0xd0` the quaternion,
  `+0xe0` the scale, `+0xf0` the position**. With a parent it hands over to
  `0x141ffab90`, which composes `+0xd0`/`+0xf0` with `+0x298`.
* the collision builder reads `[component+0xd0]` and `[component+0xf0]` as the
  `OwnerCreateBody` config, so the shape's world frame is that
  `(quaternion, position)` pair.

The map chunk already carries the same triple: `teMapPlaceableModelGroup`'s
`Entry` is `{translation[3], scale[3], rotation[4]}`
(`ow_tank::mapchunk::ModelGroupEntry`), and a model-group placeable's own
transform is identity. `crates/ow-tank/examples/mesh_world_dump.rs` therefore
places one shape instance per entry, and the probe reproduces the client's own
answer with all 3,017 placed instances in one broadphase
(`dev/probe_candidate_evaluator.py --mesh-world ... --all-shapes`).

**The evaluator's hit contract is read.** `0x14203dae0` has two query paths
and two results:

1. the handle path: `0x142cc45a0(handle, &struct, &desc_obj)` — its observable
   effect is through the hit policy the descriptor carries; after it the
   evaluator tests `[R13+0x39]` and, when set, reads the score from
   `record[0x30]` and returns 1.
2. `0x14212db80([opaque], &struct)` — reached when `desc_obj[0x18] != 0` and
   `desc_obj[0x1c] != 0`; it reads the struct's start/end through the
   descriptor object, forms `|end - start|^2` and compares it with
   `0x142f7aad4`; the evaluator's return value comes from the struct byte at
   `+0x38` afterwards.

The hit policy is a **constant code address**: the evaluator does
`lea rdx, [0x14202c580]` before the call. That address is a three-instruction
thunk that reorders the arguments (`(context, &hit, &ids)`) and jumps to
`0x14202bc90`, which decodes the ids into a live collision-world index and
writes the world's contact state. It is the only writer of the values the fan
reads back, so the fan's candidate score is produced there, not by the
narrowphase arithmetic.

**What blocks a real candidate.** The fan reads its score from a record that
only the live hit policy fills, and every primitive needs the collision
geometry the world holds (`record+0x2b8` entries and the pools). Both are
runtime state: the geometry comes from the map's collision chunks, which is
the `MAP_CHUNK_COLLISION` / `teMAP_CHUNK_ADDITIVE_COLLISION` ingest of
`formats/maps.md` (unread), and the hit policy needs a live collision world.
So the fan cannot select a real candidate by more query reading alone; it needs
the geometry ingest, which is a separate system. That is now recorded as the
top of `docs/open-questions.md` item 6.

**The trace tool.** `dev/probe_evaluator.py` runs any address under the
harness's Unicorn oracle with zeroed arena pages and logs the executed runs,
the image reads/writes and the arena accesses. On `0x1420328c0` it shows the
87 executed runs, the two stubs, the four globals (`0x143b795b0`,
`0x143b79598`, `0x143847d3c`, `0x143ac5f40`) and a fault at
`[0x143b795b0]+0xdd0` — the `Q` pointer is null in the offline image, so the
function cannot run offline until that pointer is supplied. That is evidence,
not a read: it is how the block graph was found, and every claim above comes
from reading the bytes the trace identified.

A 1:1 read of the evaluator therefore needs (a) the descriptor object's
layout and producer, (b) `0x142cc45a0`'s target (`0x142cadf00`/
`0x142cbd510`), and (c) `0x14212db80`. So this function is **open**, and that
couples the input side to collision: the wish producer cannot be finished
before the query that evaluates a candidate is read
(`docs/open-questions.md` item 6).

1. `0x140cb9020..0x140cb9206`, five iterations (`r13d = 5`), records at
   `rbp+0x5b0` stride `0x10`;
2. `0x140cb96c1..0x140cb9869`, five iterations, the pair rotated by
   `(cos 72°, sin 72°) = (0.30901697, 0.95105654)` each step
   (`0x143110824`/`0x143110828`), the same evaluator;
3. `0x140cb9ae0..0x140cb9cf6`, `N = body[0x5bc]` iterations
   (`0x140cb9263`), the pair rotated by `2π/N` (`0x142f88c18`) each step.

The first two passes accept a candidate when `|score| < best_score` **and**
`y > best_y`, both strict, starting from `best_score = +FLT_MAX`
(`0x142f844d8`) and `best_y = -FLT_MAX` (`0x142fb8e58`), so the first
candidate that passes wins. The fine pass only requires
`|score| < best_score` (`0x140cb9c0b..0x140cb9c16`), so it improves the earlier
best without testing the height. The absolute value is the `0x7fffffff`
mask at `0x142f7aaf0`.

The fan's radius (`0x140cb927a..0x140cb92e2`):
`maxss(body[0x65c], 0.35) * body[0x5b8] - 0.015`, and the candidate `y` is
the constant `0.015` (`0x3c75c28f`, written at `0x140cb9379`). The radius and
the speed share the `0.015`.

The exits (`0x140cb9f2e`, `0x140cb9f68`, `0x140cb9f9e`):

- **accepted, with the back-transform** — `0x140ca5b90` (the verified inverse
  transform) maps the chosen world candidate into the source space, then
  `source+0x00 = wish`, `source+0x10 = secondary`, `source+0x38 = qword`,
  `source+0x30 = u32`, `source+0x34 = index`, `source+0x40 = 1` (gate),
  `*param_3 = u16 index`;
- **accepted, without it** (`0x140cb9d68` with a null service object) — the
  same writes from the untransformed candidate;
- **inactive** — all four lanes zero, gate `0`, `source+0x38 = rcx`.

The gate is `body[0x610] > 0 || dt > 0` (`0x140cb9d42..0x140cb9d59`) and at
least one accepted candidate. Before the exit, `0x140cb9d75..` runs a chain of
`0x141c5d780(object, byte)` queries with ids `0x55`, `0x23`, `0x2e`, `0x22`,
`0x68`; they choose the exit path and are not read.

`source+0x41`/`+0x42` are flag bytes OR-ed (`0x140cb90e0`) or stored
(`0x140cb90ea`) from a stack slot the evaluator path fills (`[rbp+0x8e]`):
bit 0 goes to `+0x41`, bit 2 to `+0x42`. They feed the input object's `+0x98`
gate and `+0x70` secondary; the slot's producer is not read.

**What is implemented.** `ow-sim::fan` implements the read arithmetic: the
turn limit, the max3 cap, the frame time, the speed, the radius, the seed
(both terms and the `+0.1` bias), the rotation, the two fan generators and
both acceptance rules, with unit tests. The evaluator wrapper `0x1420328c0`,
the handle dispatcher `0x142cc45a0`, the query driver `0x142cbd510` and the
narrowphase `0x142cc1610` are read (below). Still **open**: the policy body
`0x14202bc90` (obfuscated, see its subsection), the two generic helpers
`0x1426cd3d0`/`0x1426de990`, the per-pass record layouts and the
`0x141c5d780` query chain, and the geometry ingest that fills the world
record. The module takes their outputs as arguments rather than inventing
them, so no wish can be produced yet.

**The query object's layout, its producer and the record (2026-11-02).**
Three reads close the descriptor half of the contract above.

*Clone.* `0x140a01560 (crow, copy)` is the query clone: it enumerates every
live field, so its last written field is the object's last field. The query is
**0x4a5 bytes** (0x4a8 stride):

| offset | field |
|---|---|
| `+0x00`, `+0x08`, `+0x10` | three qwords, copied verbatim |
| `+0x04` | u16, the fan writes `body[0x23c]` |
| `+0x06` | u16 mode: `0xf` from the fan, `0xe` from the nested cached query, `1` from its outer call |
| `+0x08` | u64, the fan writes `0x0003000000000000` |
| `+0x18`, `+0x1a`, `+0x1c` | u32 / u16 / byte; the fan writes `0`, `0x30e0`, `0` |
| `+0x20` | inline vector 1: `{ptr = +0x30, count +0x28, cap +0x2c = 0x80000020}` |
| `+0x130` | inline vector 2: `{ptr = +0x140, count +0x138, cap +0x13c}`, same cap |
| `+0x240` | inline vector 3: `{ptr = +0x250, count +0x248, cap +0x24c}`, copied by `0x1407e1710` |
| `+0x450` | float4 — segment point A |
| `+0x460` | float4 — segment point B |
| `+0x470`, `+0x480`, `+0x488`, `+0x490`, `+0x498` | qwords copied verbatim |
| `+0x49c` | u32, copied verbatim |
| `+0x4a4` | byte, copied verbatim |

The two inline-vector caps carry bit 31 (`0x80000020`), so bit 31 is the
"storage is inline" marker of this container type and 0x20 is the capacity.
The clone is shallow for the header and deep for the three vectors only.

*What the fan puts in the segment.* `MoverWishWriter` builds
`P = M * wish` (`0x140c9e670`, below), then writes
`A = P + S + (seed + 0.1) * basis` into `+0x450` and
`B = P + S - speed * basis` into `+0x460`, where `S = body[0xe8] * 1e-6 *
helper0(block) * 0x143872ce8` is the per-frame scale along the same four-lane
`basis` (`body+0xc0/+0xc4/+0xc8/+0xcc`). So the query is a **sweep segment
from behind the mover to `seed + 0.1` ahead of it**, both endpoints in world
space, and the w lane rides the basis w. The `+0.1` is the same bias the seed
takes.

*The record the query answers with.* The fan's per-candidate record and the
memo wrapper's copy agree on one 0x50-byte layout: `+0x00` float4 position,
`+0x10` float4 direction (the fan dots it with the basis and takes `|.|` —
this is the fan's score), `+0x20` qword hit block (`0` = no hit), `+0x28`
qword initialised to `0xffff0000ffffffff`, `+0x30` f32 initialised to `1.0`,
`+0x34` qword initialised to `-1`, `+0x3c` u32 initialised to `1` (bit 16 is
tested by the fan), `+0x40` qword, `+0x48` byte.

*The memo front-end.* `0x140cc1870 (body, &record, &query)` is the second
caller of the evaluator and is reached from the fan when `body[0x659] != 0`.
It caches one record at `body+0x1b08` for the state id `body+0x1a60` copied to
`body+0x1b00`, and reuses it when the query's `+0x450` and `+0x460` xyz lanes
are each within **0.004** of the cached `body+0x1ae0..0x1aec` and
`body+0x1af0..0x1af8`. On reuse it prefers the cached record when its `+0x30`
is `>=` the fresh one. It reaches the evaluator through a clone whose mode
byte `+0x06` is `0xe` (outer) and `1` (inner), and stores the evaluator's
return byte at `body+0x1b13` / `+0x1b12`.

*The hit-policy thunk, corrected.* `0x14202c580` is not three instructions
but four moves and a tail jump: it rotates `(ctx, a, b)` into `(b, ctx, a)`
before `0x14202bc90`. So the policy body's first parameter is the descriptor
pointer the dispatcher passed third, not the context. `0x14202c580` has no
function in the image (the bytes after it are `int3` padding), which is why
the address looked like code without a body.

**The wish-to-world transform (2026-11-02).** `0x140c9e670 (block, out4)` is
the fan's `M = M * wish` step and the reason the fan's seed is a world-space
projection. It copies `block+0x70..0x7f` to `out4`, then calls
`0x140c9e740 (block, &m)`; when that returns 1 it overwrites `out4` with
`v.x*m[+0x00] + v.y*m[+0x10] + v.z*m[+0x20] + m[+0x30]` (a row-vector affine
with w ignored — the same convention as `MoverWishForward`). When it returns 0
the copy stands, so the identity case costs nothing.
`0x140c9e740` resolves the matrix: the id at `block+0x450` must not be the
sentinel `0x1441880e8` and must not have `0x1fffffff` set; the parent chain
comes from `0x140c9f000 (block)`; with no parent, `block+0x40 == 0` means "no
matrix" and it writes a default and returns 0, otherwise it copies the 4x4 at
`block+0x3f0..0x42f` and returns 1. For the fan the block is `body+0x10d0`, so
those are `body+0x1110` (flag), `body+0x14c0..0x14ff` (matrix), `body+0x1520`
(id) and `body+0x1528` (state object) — the id is the same one the ramp term
tests.

### The writer's candidate loops (2026-11-02; the outer walk is still open)

The wish writer calls the evaluator at **four** points, inside three enclosing
loops. Two of them are now read at the arithmetic level:

**The ring pass.** Site `0x140cb9b6e`. Gated by `body+0x659` and by the memo
`0x140cc1870(body, &center, &query)` returning non-zero, and additionally
entered only when `uVar21 == 0 || evaluator != 0`. Its bindings:

- radius `fVar35 = max(body+0x65c, 0.35) * body+0x5b8` — the same value
  `ow-sim::fan::fan_radius` computes;
- count `n = body+0x5bc`, and the step from `0x1426c40f0(0xffff0000ffffffff,
  6.2831855/n)`: `sin` in the low half, `cos` in the high half, matching
  `ring_step`;
- the offset is **one scalar**, `offset = pair_component * radius`, splatted to
  all four lanes (`movss` + `shufps 0xc9`) and stored at `[rbp-0x20]`; the pair
  itself starts at `(1, 0)` and is rotated by the step after every candidate;
- two endpoints, written to `[rbp+0x550]` and `[rbp+0x560]` before the call:
  `A = (A + B) + splat(offset)` and `B = (B - C) + splat(offset)`, where `B` and
  `C` are built from the query's two points and `A` from the previous
  iteration's result — so the loop **carries the first endpoint forward**;
- after a non-zero return, the alignment test is
  `dl = (byte[rbp-0x52] & 1) != 0 && sum4(candidate_dir * body[0xc0..0xcc]) >
  [rbp-0x40]` — a **four-lane** sum of the products against a threshold, and
  the `(b & 1)` term is a gate from the hit record's flags byte, not part of
  the arithmetic;
- the output flags are **OR**s, not assignments:
  `out+0x41 |= (b & 1)` and `out+0x42 |= ((b >> 2) & 1)`.

That alignment dot is **not** the refinement's `facing`: it multiplies the
candidate direction by the mover's direction quad and sums all four lanes,
where `facing` multiplies the direction by the hit **normal** in the y, x, z
order. Two different tests, two different call sites; do not conflate them.

**The refinement pass** (`0x140cb9842..0x140cb9a20`) is read in full and
implemented (`ow-sim::fan::refine_pentagon`): five pentagon candidates at
`offset*0.02 + refined*0.005`, the ground probe `0.04` lower, and the keep-first
/ replace-on-facing / break-on-facing rule.

**Superseded (2026-11-02).** The paragraph that used to stand here said the
four call sites and the `+0x1140` write were unread and `fan::solve` did not
exist. Both are now read and implemented — see "The writer's composition, read"
immediately below and `ow-sim::fan::solve`. The shape-ring branch behind the
global toggle and `0x140cc2290`'s mesh-face refinement remain behind the
`CandidateSolver` seam; those are inputs, not the loop.

### The writer's composition, read (2026-11-02)

The writer `0x140cb86c0` is no longer a black box: its prologue, its four
candidate loops and its output write have all been read. Read as one list, the
composer runs these stages over a single `CandidateQuery`:

1. **Scales.** `point = MoverWishWorldPoint(body+0x10d0)` advanced by
   `frame_frac * shape_dir * _DAT_143872ce8` from `FUN_140c9f110`; the seed
   scalar `A` is the biased seed, the speed scalar `B` is
   `fan_speed + frame_dt`. Both are splatted across x/y/z plus the `w` lane
   with `body+0xcc`. So `A = point + seed*dir` and `B = point - speed*dir` —
   the two endpoint quads are built from **two different scalars**, seed and
   speed, which is why they are not symmetric.
2. **Seed floor.** When the geometry pointer resolves and `body+0x655` is
   clear, the seed is raised to twice the hit shape's radius:
   `seed = max(seed, 2 * [shape+0x2d2c])`. The shape comes from the world
   handle with `FUN_140d4c510` + the seam `0x142030030`.
3. **Pass 0, the cached query.** When `byte[body+0x659]` is set,
   `MoverWishQueryCached(body, &out_dir, &query)` runs one query with
   `query.mask = word[body+0x23c]` and a shape-set pair chosen by
   `body+0x1a60` (`0x10a1` + `8`/`0x10`/`0x18`, permuted by
   `(byte[body+0x1a79]+0x59)^0x71`; the no-settings case is `0x10a10002`).
4. **Pass 1, the ring.** When `byte[body+0x659]` is set and
   `n = [body+0x5bc] > 0` and `_DAT_1439b07b8` is clear: `n` quads, each the
   previous direction plus an offset `(pair.x*r, 0, pair.y*r, 0)` with
   `r = max([body+0x65c], 0.35) * [body+0x5b8]`, the pair rotated by
   `(cos, sin) = 0x1426c40f0(6.2831855/n)`. Acceptance is `accepts_ring`:
   facing, then a strictly smaller `|projection|` than the best so far.
   The alternative branch (`_DAT_1439b07b8` set) builds **five** quads from
   the shape's own basis instead: `(cos*(seed-0.015), 0.015,
   sin*(seed-0.015), 0)` rotated by the same trick; `0x3c75c28f` is `0.015`.
5. **Pass 2, the pentagon refine.** Five steps around the accepted direction:
   `dir*0.02 + normal*0.005` with the y lane shifted by `-0.04`, the pair
   rotated by `(0.95105654, 0.30901697)`; the first hit is taken, a facing
   pass replaces it, and a facing pass breaks. A `kind == 3` hit with
   `_DAT_1439b08d0` set hands the whole refine to the obfuscated indirect
   `0x140cc2290` instead.
6. **Pass 3, the back-transform (re-read 2026-11-06).** It is **not** a
   five-candidate loop. Once a hit has been accepted, one snap runs at
   `0x140cb9884..0x140cb99db` with `n` = the accepted direction
   (`[rbp-0x80]`, copied from the candidate at `0x140cb9834`), `start`/`end`
   the sweep's two endpoints (`[rbp+0x550]`/`[rbp+0x560]`) and `delta = end -
   start`:

   ```text
   s  = -dot(n, delta)                 ; the hsum negated by 0x142f817a0 (0x80000000 x4)
   if s < 1.1920928955e-4 skip         ; 0x142f7aad8 = 0x38fa0000, not FLT_EPSILON
   t0 = dot(n, start) - dot(record+0x00, n)   ; two hsums, then one SUBSS
   r  = RCPSS(s)                       ; 0x140cb993e, the client's reciprocal idiom
   t  = (2*r - r*r*s) * t0             ; 0x1431107f0 is 2.0f, one Newton step
   if t < 0 or t > 1.0 skip            ; 0x142f7aae4 is 1.0f
   record+0x04 = (start + t*delta).y   ; lane 1 of the record; copied out later
   ```

   The plane is `dot(p - out_dir, n) = 0` with `n = out_dir`: the snap seats
   the direction's `y` at the point of the sweep whose projection on the
   direction equals the direction's own. It writes only lane 1 of the
   **record** (`MOVSS [rsp+0x74]`, `0x140cb99b9`; the record sits at
   `rsp+0x70`, so the store is `record+0x04`) and the record is copied to the
   output afterwards, which is why the other three lanes are untouched. The
   `s` test is `jb`, unordered-or-below, so a NaN sweep length skips like a
   zero one.
   The endpoints it reads are the ones written at `0x140cb92f3`/
   `0x140cb92fa`, not the ring loop's (`0x140cb9b5f`/`0x140cb9b66`, which come
   later in address order). That is no longer a puzzle: those two stores are
   the **batch ring's** endpoints and the incremental ring cannot reach the
   snap at all - see *The stage order* below. The `out+0x41`/`out+0x42` bytes are written by the caller of this
   block (`0x140cb98e2`/`0x140cb98f7`), from `word[rbp-0x52]`: `|= key & 1` and
   `= (key >> 2) & 1`.

   The five-candidate loop with `s = dot(scale, base - cand)`,
   `A = s*scale + cand`, `E1 = A + seed*scale`, `E2 = A - speed*scale`, the
   `|dot(E1, scale)| < best` test and the `out_dir.y < 0` gate is the **shape
   branch** (`_DAT_1439b07b8` set), not pass 3: it starts from
   `FUN_141ae3eb0`'s corner `local_b38` and extent `local_b28` and runs five
   iterations over the quads `local_628`/`local_618`/`local_608`/`local_5f8`/
   `local_5e8`.
7. **The write.** `out+0x00..0x1f` = direction then normal quads;
   `out+0x30` = the record's tag word; `out+0x34` = the resolved id;
   `out+0x38` = the record's `local_c40`; `out+0x40` = 1. The no-hit path
   zeroes `0..0x1f`, sets `out+0x30 = _DAT_1441880e8`, `out+0x34 = 0`,
   `out+0x40 = 0`, `out+0x38 = 0xffff0000ffffffff`, and the word output to 0.
   A tagged hit (`0x141c5d780(obj, 0x55 / 0x23 / 0x2e / 0x22 / 0x68)`) takes
   the variant path and, when it resolves, runs `MoverWishTransform`
   `0x140c9e740` on the direction before writing it.

**The five helpers, now read.** `0x1426cd3d0` is `cosf`: `0x140ca7400`
returns the cone half-angle in degrees clamped to `[0, 1.5707844]` (the
constants are `0x142f844b8 = 0.017453292` and `TURN_LIMIT_MAX`), and the
prologue calls `cosf` on it, so the threshold `local_c18` is
`cos(turn_limit(body))` — the same `turn_limit` the fan already reads.
`FUN_140c9f110` writes a zero quad and returns it, so **the extrapolation term
is zero** and the query point is `MoverWishWorldPoint(body+0x10d0)` unchanged.
`func_141af0070` is a three-argument thunk (`MOV RAX,RDX; MOV RDX,RCX; MOV
RCX,RAX; JMP 0x141ae61b0`) into `0x141ae61b0`, the shape transform; the caller's
`(&out, shape, position)` arrives as `(shape, position, &out)`. `FUN_141ae3eb0`
is **not** a rotation basis: it takes the shape's point from `0x141ae3fc0`, then
writes two quads — the box's low corner `point - (half, 0, half, 0)` with
`half = [shape+0x2d28] * 0.5` and a `-0.004` on `y`, and that corner plus
`([shape+0x2d28], [shape+0x2d2c], [shape+0x2d28], 0)`. So `local_b38` is the
corner and `local_b28` the extent, and pass 1''s five candidates are the box's
corners and edges. `0x140cc2290` is the mesh face pass: it walks the shape's
face list (vtable `+0xb0`), takes each triangle's centroid and normal,
normalizes, tests `threshold < dot(normal, dir)` — the same strict rule — and
keeps the best, writing lanes `+2/+5/+3/+7` and the word at `+0x3e`.

**The stage order inside `0x140cb86c0`, read (2026-11-07).** The body has one
`.pdata` entry, no chained fragments, and four arguments that matter: `rcx`
body, `rdx` source, `r8` the `u16` output index, `xmm3` the frame `dt`; `r9` is
never read. Two of the earlier notes were wrong about the branches, so the
settled order is set out here with the gate that selects each stage:

1. `0x140cb8747` the threshold (turn limit → `cosf`), both endpoints, the seed
   and the speed - every call, before any branch.
2. `0x140cb8d4a` **pass 0**, gated by `byte[body+0x659]`: one
   `0x140cc1870(body, &record0, &block0)` query. On a hit with `(~flags & 1)`
   set it stores `out+0x41 = ~flags & 1`, `out+0x42 = (flags >> 2) & 1` and
   `*index = word[record0+0x3c]` - an **assignment**, where every later stage
   ORs into `+0x41`.
3. `0x140cb8df5` the result byte goes to `byte[shape+0x1016]`, then the
   discard rule `!body[0x1a82] && !byte[shape+0xd42]` clears it; `0x140cb8e26`
   exits on it, and on `byte[body+0x659] == 0`.
4. `0x140cb8e3c` **shape pass** when a shape resolved and `byte[body+0x655]`
   is clear; **ring** otherwise, i.e. when the shape is null or `+0x655` is
   set. The two never mix: the shape pass has no route to the pentagon.
5. `0x140cb8e4e` the shape pass builds the corner and extent via
   `0x141ae3eb0` and probes **five** quads (the mover quad, the corner quad,
   `(c.x, c.y, e.y, 0)`, `(e.x, c.y, c.z, 0)`, `(e.x, c.y, e.y, 0)`).
   Each probe's acceptance is `(~flags & 1) && hsum(record0+0x10 * dir) >
   threshold`, then a strictly smaller `|hsum((point - record0+0x00) * dir)|`,
   then `record0+0x00.y > -FLT_MAX` (**not** an `out_dir.y < 0` gate).
6. `0x140cb9263` `n = body[0x5bc]`, `r = maxss(body[0x65c], 0.35) *
   body[0x5b8]` - shared by both ring flavours.
7. `0x140cb9273` the **batch ring**, when `byte[0x1439b07b8] != 0`: the
   endpoints are overwritten to `point + FLT_MAX*dir` and `point - speed*dir`,
   `n` quads `(B*r', 0.015, A*r', 0)` with `r' = r - 0.015` plus one zero quad
   are rotated by `0x1426c40f0(2*pi/n)`, and **one** evaluator call runs at
   `0x140cb949b`. Then `0x142cba720(record0+0x28)` picks the tail: `kind <= 1`
   → the snap, otherwise the pentagon.
8. `0x140cb9ae0` the **incremental ring**, when that byte is zero: `n` single
   evaluations, each with the pair offset `(B*r, carry, A*r, carry)` added to
   both endpoints, acceptance `(flags & 1)` and then the smallest
   `|hsum((point - record0+0x00) * dir)|`. **No pentagon, no snap** - so the
   snap's endpoints are only ever the batch ring's and the old "address-order
   conflict" was void.

   Read to the bottom on 2026-11-07, the incremental ring's loop body is
   `0x140cb9ae0..0x140cb9cf6` and its step is hand-rolled, not `0x1426c40f0`:
   with the pair's two lanes in `xmm11`/`xmm14`, `cos` in `xmm15` and `sin` at
   `[rbp+0xc0]`, the pair becomes `A' = A*sin + B*cos` and
   `B' = B*sin - A*cos`, i.e. a rotation by **minus** the step angle in the
   usual convention. The step offset multiplies those lanes by `xmm10`, and
   `xmm10` is identified earlier in the function as the **floored seed**
   (`max(seed, 2 * shape[0x2d2c])`, `0x140cb8b56`), so the offset is
   `pair * seed` and the ring radius `r` must already be folded into the pair
   at the ring's entry - which is **not yet read**. (An earlier draft of this
   paragraph called `xmm10` the radius; the definition at `0x140cb8b65` is
   direct evidence against that.) Each iteration also moves the running point
   to `record0+0x00 - offset` (`xmm13`), and the ring writes `out+0x42 =
   (flags >> 2) & 1` as well as `out+0x41 |= flags & 1`. The acceptance is the
   shape pass's: `(~flags & 1)` sets the candidate flag only after the facing
   term is positive, then the projection must beat `xmm12`, which starts at
   `FLT_MAX` and is replaced by each accepted value.
9. `0x140cb96c1` the **pentagon refine**, batch ring only: five iterations at
   `off = 0.02*pair + 0.005*n2` (pair in lanes 0/2), the `y` lane of `B`
   shifted by `-0.04`, the pair rotated by `(0.95105654, 0.30901697)`;
   first-hit keeps, a facing pass replaces and breaks; `kind == 3` with
   `byte[0x1439b08d0]` set hands the whole loop to `0x140cc2290`.
10. `0x140cb988d` the **snap**, batch ring only, reading the batch ring's own
    endpoints.
11. `0x140cb99c5` `record0+0x00 = point.x`, `record0+0x08 = point.z`, then the
    `xmm9 < body[0x610] || dt > xmm9` gate, then the write-out.
12. `0x140cb9d46` **the write**: `out+0x00`/`out+0x10` the two quads,
    `+0x30` tag, `+0x34` id, `+0x38` `record0+0x28`, `+0x40 = 1`, and
    `*index = word[record0+0x3c]`. The no-hit path writes `+0x38 =
    0xffff0000ffffffff`, `+0x40 = 0`, zeros the quads and leaves `*index`
    untouched.

### The batch ring's candidate list: what is proven, and what is not (2026-11-07)

Steps 7, 9 and 10 are read; only step 8's *consumer* is not. What is proven:

- the block carries a **ptr/count pair** at `+0x490`/`+0x498` and the ring fills
  it with `n` quads plus one zero quad;
- the evaluator the composer calls, `0x1420328c0`, is a thunk that builds a
  stack descriptor and forwards to the dispatcher `0x14203dae0`;
- the dispatcher's own `.pdata` entry (`0x14203dae0..0x14203df66`) is a
  prologue stub - the function continues to `0x14203e0d6` - and it flattens the
  block into a stack copy before dispatching;
- the driver `0x142cc45a0` has the same shape (a 0x3e-byte root entry covering
  four `.pdata` entries, `0x142cc45a0..0x142cc47c5`, read as one function) and
  re-flattens into a sweep-options struct that carries a ptr/count pair onward
  into the sweep driver `0x142cbd510`.

What is **not** proven, and why it must stay open: the offset that proves the
carry-through was found by grepping `dev/data/corpus/asm/`, and that corpus is
a **linear** sweep over a `.pdata` range, not a flow-based decode. The
`014203dae0_FUN_14203dae0.asm` listing desyncs at `0x14203db2f` - the bytes
there decode as `rcr byte ptr [rax+0x7f], 0x1c` followed by `jle`, which is a
jump table read as code - so any operand a grep finds after that point is not
attributable to a function, and any operand it *misses* proves nothing either.
The 350-entry `.pdata` run over `0x14203db00..0x142054500` is exactly the kind
of table-rich region where this happens.

So the honest statement is: **the list is carried through two block flattens to
the sweep driver `0x142cbd510`, but that was read from a desynced listing and
must be re-established with Ghidra's flow-based decode before the batch ring
can be implemented.** Until then `+0x490`'s consumer stays unread, and the
one-candidate-at-a-time `CandidateSolver` seam is a known limitation rather than
a design choice.

### The batch tail is closed

Step 8's dispatch is read and ported: `0x142cba720(record0+0x28)` returns a
kind, `0x140cb9864` compares it with `1` and `jg` selects the pentagon, so kinds
`<= 1` take the snap. That is `ow_sim::fan::batch_tail` with
`BATCH_TAIL_SNAP_MAX_KIND` and the `BatchTail` enum, tested. Both sub-passes
below it are read and already ported (`fan::refine_pentagon`,
`fan::back_snap`), so blocker 5 is closed at the code level; what remains
there is only its position in the composer, which needs the batch ring.

`out+0x41` is worth one line of its own because it is three different writes
in three stages: an assignment in pass 0, `|= ~flags & 1` in the shape pass,
and `|= flags & 1` in both rings.

**The shape pass, read in part (2026-11-07).** `0x140cb8e4e` is reached only
when `byte[body+0x659]` is non-zero, the shape pointer `r14` is non-null and
`r13b` is zero; a pass-0 hit jumps straight to `0x140cb9d42` instead, so the
static hit short-circuits the whole dynamic chain. The pipeline is
`0x140d4c510(shape)` → an id, `0x142030030(body+0x240, id)` → the shape's
collision record (null bails to `0x140cb9255`), a centre quad taken from
`[shape+0xf60]` when `byte[shape+0x10c0]` is set and otherwise computed by
`0x140c3c120(shape, &out)`, then `0x141af0070(&xform, record, centre)` and
`0x141ae3eb0(record, &xform, &pair)` — the corner builder, whose two output
quads are the corner at `+0x00` and the far corner at `+0x10` (the pair the
earlier note called ``corner`` and ``corner plus half``).

Five probe quads are built in the frame at `rbp+0x5b0..0x5ff` - one per
`0x10` bytes, walked in order with `r13d = 5` and `r14 = &rbp+0x5b0`:

```text
0x5b0  centre                      (as returned by the centre path)
0x5c0  splat(min.z)                one `movaps` of the `0xaa` shufps
0x5d0  (min.z, min.y, far.z, 0)
0x5e0  (far.x, min.y, min.z, 0)
0x5f0  (far.x, min.y, far.z, 0)
```

The lane sources are worth naming because they are easy to mis-index: the
`movaps` at `0x140cb8f9e` writes all of `0x5c0..0x5cf`, so quads 1 to 4 start at
`0x5c0`, `0x5d0`, `0x5e0` and `0x5f0` - not at `0x5b0`, `0x5d0`, `0x5e0` and
`0x5f0`. The lane-3 halves of quads 1 to 3 are zeroed by the three
`mov dword ..., r15d` stores; quad 1's lane 3 keeps the broadcast's `min.z`,
because the `0xaa` shufps runs after that store and writes all four lanes.

The loop at `0x140cb9020` is the ring's shape: `s = hsum((point - quad[i]) *
dir)` with the direction at `[body+0xc0]`, `base = quad[i] + s*dir`,
`A = base + m10*dir` at `rbp+0x550` and `B = base - m12*dir` at `rbp+0x560`,
where `m10` and `m12` are two register splats, neither identified yet. The
evaluator is `0x1420328c0(manager, &record0, &block0)` and `al == 0` moves to
the next probe. On a hit the flags byte is `byte[rbp-0x52]` and the pass
writes `out+0x41 |= flags & 1`, `out+0x42 = (flags >> 2) & 1` and a `u16` from
`word[rbp-0x54]`; acceptance is then
`fabs(hsum((m11 - record0+0x00) * dir)) < FLT_MAX` (`0x142f844d8`) **and**
`record0+0x04 > 3.3068924` (`0x142f88e58`). The `FLT_MAX` comparison can only
reject a NaN, so the real gate is the altitude one, which is what makes a
probe hit meaningful at all.

This `m10`/`m11`/`m12` block replaces the earlier "still open" paragraph: all
three registers are identified (seed endpoint, floored seed, speed), so the
shape pass has no unidentified operand left.
The three splats are now identified, and they are the same three the prose at
the top of this section already named: **`xmm11` is the seed endpoint**
`point + seed*dir` (built at `0x140cb8b08..0x140cb8b25`), **`xmm10` is the
seed** after its floor (`max(seed, 2 * shape[0x2d2c])` at `0x140cb8b56..
0x140cb8b72`), and **`xmm12` is the speed** (`max(x, body[0x5cc]) + xmm10` at
`0x140cb8874`/`0x140cb888c`). So each probe sweeps from
`quad[i] + (s + seed)*dir` to `quad[i] + (s - speed)*dir`, and the accept
term `|hsum((xmm11 - record0+0x00) * dir)| < FLT_MAX` is a NaN reject on the
gap between the hit point and the seed endpoint - it can never fail for finite
values, which is why the `record0+0x04 > 3.3068924` altitude test is the real
gate.

The accept tail after `0x140cb9150` is read too: it recomputes the record's
point as `s + hsum(((xmm11 - point)*dir - s)*dir) * dir`, sets `r12b = 1` for
the caller, and stores the winning probe's index into `word[rbp-0x54]`. The
one remaining unknown in this pass is the intent of that projection step;
every operand in it is identified, so it can be ported literally.

`ow_sim::fan` has
`corner_pair` and `shape_probes` (the box and the five quads, both ported and
unit-tested against the store map) but no loop, because the loop's three
splats are not yet identified.

What the store map now *does* settle: the probes are built from the box corners
`0x141ae3eb0` returns, not from a rotating pair, so the earlier note that the
shape pass built its five quads "from the shape's own basis" was right about
the source (`min` and `far` are the shape's box) and wrong about the form (there
is no rotation in it). `0x141ae3eb0` is
`min = centre - (half_x, 0, half_x, 0)`,
`far = min + (half_x, half_y, half_x, 0)`, with `half_x = shape+0x2d28 * 0.5`
and `half_y = shape+0x2d2c` - **not** a radius. The result is asymmetric in x
and z (`far.x == centre.x`, `far.z == centre.z`, `min.z == centre.z - half_x`),
and the function's own ladder writes `0.001` into its y lane only for the
following `movaps` to overwrite that lane before the subtraction; the `0.001`
is dead. `ow_sim::fan::corner_pair` reproduces all of it, and
`shape_probes` is the store order: `centre`, `splat(min.z)`, `(min.z, min.y,
far.z, 0)`, `(far.x, min.y, min.z, 0)`, `(far.x, min.y, far.z, 0)`.

**Pass 0 is closed (2026-11-07).** `0x140cc1870` (`MoverWishQueryCached`, body
`0x140cc1870..0x140cc1cae` across three `.pdata` entries) is the static pass and
it is small:

1. The **cache key** is `dword[body+0x1a60] == dword[body+0x1b00]` and six
   lane comparisons `|body[0x1ae0 + 4k] - state[0x450 + 4k]| <= 0.004` over
   `k` in `{0,1,2,4,5,6}` - the two x/y/z triples at `+0x1ae0` and `+0x1af0`
   against the state's at `+0x450` and `+0x460`, never touching `+0x1aec`.
   On a match the answer is replayed from the state's `+0x440` and
   `body[0x1b13]` supplies the hit flag, so the cache is `<=`-inclusive and
   lane-for-lane - exactly `ow_sim::fan::memo_matches` and `MEMO_TOLERANCE`.
2. On a miss it **clones the state into a query block** with `0x140a01560`,
   a plain copy constructor (`+0x00..+0x18`, the three containers at `+0x20`,
   `+0x130`, `+0x240`, the quads at `+0x450`/`+0x460`, the tail from `+0x470`
   to `+0x4a4`), and overrides one byte: `block+0x06`, the **layer mask**,
   which the composer's own template sets to `0x0f`.
3. It then runs the evaluator **twice** on the same state with `block+0x06 =
   1` and `block+0x06 = 0x0e`, and keeps the second record only when its
   `+0x30` is **strictly** smaller than the first's.
4. A freshly built record's `+0x30` is `1.0`, so a **miss scores 1.0**. A hit
   above `1.0` therefore loses to a miss, and the pass returns the miss. That
   is not a tolerance and not an oversight: it is the comparison the image
   makes, and `ow_sim::fan::pass_zero` reproduces it (`PASS_ZERO_MASKS` is
   `[1, 0x0e]`, and the miss's score defaults to `1.0` rather than infinity).
5. The answer is stored back into the cache at `[body+0x1b08]` with
   `byte[body+0x1b13]` and the key refreshed - but **only when
   `record+0x20 == 0`**, the shape-object pointer. A static hit against a
   shape is deliberately not cached.

The records the two probes fill are the same 0x50-byte template the composer
uses: `+0x00`/`+0x10` zero quads (`[0x14328cd40]`), `+0x20` = 0, `+0x28` =
`0xffff0000ffffffff` (`[0x143110810]`), `+0x30` = 1.0, `+0x34..+0x3b` = `-1`,
`+0x3c` = 1, `+0x40` = 0, `+0x48` = 0.

Note also that the 2026-11-02 composition list above predates the stage map and
its item 4 (``five quads from the shape's own basis``) is the **shape pass**
that section describes, not the batch ring; its ordering claims are superseded
by the stage map at the top of this section.

**The record and block templates, read (2026-11-07).** The composer's two stack
objects are the ones every later comparison reads, and they are initialised at
`0x140cb8c19..0x140cb8d46` (block) and `0x140cb8cfc..0x140cb8d46` (record).
`record0` sits at `rsp+0x70` (`rbp-0x90`) and `block0` at `rbp+0x100`; `rbp` is
`rsp+0x100`, so both are plain offsets from the caller's frame.

```text
record0+0x00  = point            ; the query point quad, also the pre-hit value
record0+0x10  = point            ; the normal lane, overwritten by the evaluator
record0+0x20  = shape object ptr, or 0
record0+0x28  = qword [0x143110410] = 0x45df5c7532d86ce3   ; a fixed value
record0+0x30  = 1.0f
record0+0x34  = 0xffffffffffffffff (all ones through +0x3b)
record0+0x3c  = 1
record0+0x40  = 0
record0+0x48  = byte 0

block0+0x000  = u16 r8w, else 0x10, else 0x18 by body[0x1a60] (0 / 1 / other)
block0+0x002  = u16 0x10a1, or 0x10b1 / 0x10a9 / 0x10b9 when the byte
                ((body[0x1a79] + 0x59) ^ 0x71) + 0x14, rotated right by 1,
                is non-zero (0x140cb8c9d)
block0+0x004  = u16 mask, word[body+0x23c]
block0+0x006  = 0xf
block0+0x008  = 0x0003000000000000     ; movabs at 0x140cb8be0
block0+0x010  = 0, block0+0x018 = byte 0, block0+0x01c = byte 0
block0+0x450  = A = point + seed*dir
block0+0x460  = B = point - speed*dir
block0+0x470  = 0
block0+0x480  = point             ; the batch ring overwrites this with 0.015
block0+0x490  = 0 (list pointer)  ; the batch ring fills it
block0+0x498  = 0 (list count)    ; the batch ring fills it
block0+0x49c  = 1.0f
block0+0x4a0  = the caller's u16 index as a dword
block0+0x4a4  = byte 0
```

Two consequences worth stating. First, the shape pass's comparison
`|hsum((point - record0+0x00) * dir)|` reads a **hit point** the evaluator wrote,
not the candidate: at template time `record0+0x00 == point`, so a probe that
never writes the record would compare zero and win every time. Second,
`block0+0x490`/`+0x498` are null and zero in every stage except the batch ring,
which is the only caller that asks one evaluator call for `n` candidates.

**In code (`crates/ow-sim/src/fan.rs`).** `solve` implements the gate, both
endpoints, the threshold, the memo test, the ring → refine → back-transform
order and both reflection bytes; the helpers `facing_threshold`,
`memo_matches`, `endpoints`, `projection`, `project_back`, `back_snap` and
`reflection_flags` carry the arithmetic with `sse_*` rounding. The order is now
settled (above), and it is not a straight line: pass 0 and the shape pass are
exclusive of the rings, the ring flavour is exclusive of itself, and the
pentagon and the snap are each reachable only from the batch ring, in that
order. `back_snap` is pass 3 and is **not** called by `solve` yet only because
the rings and the shape pass are not written - not because its position is
unknown. Still behind the seam: the evaluator, pass 0's cache, both ring
flavours, the pentagon and the mesh-face refinement - `SolveContext` carries
their inputs rather than inventing them.

**The ring's comparison operand (2026-11-06).** `accepts_ring` is followed by a
strictly smaller `|projection|`, and the projected quad is the **candidate**
`A` the step just built, not the query point: `A` is the only value that
changes between the `n` steps, and a loop-invariant operand would make the
acceptance equivalent to "the first facing hit", which the pass then hands to
the refine. `projection(candidate, dir)` is now what `solve` evaluates, pinned
by `the_ring_keeps_the_closest_candidate_not_the_first`, and
`an_aligned_ring_hit_flows_through_refine_and_the_back_transform` derives the
closest candidate instead of assuming the first.

**Still open in the composer:** pass 3 re-evaluates **five** candidate quads
(this page's step 6) and the port re-evaluates only the ring's winning `A`;
the `last_dot < 0` gate and the per-candidate `s` are not implemented. Pass 0's
cache, the shape-ring branch (`_DAT_1439b07b8`), the mesh-face refinement
`0x140cc2290` and the tagged-hit `MoverWishTransform` path are open as before.

### The collision query driver and the hit contract (2026-11-02)

Reading the wrapper's tail to its driver closes the shape of the whole
collision answer. The wrapper `0x1420328c0` hands the query to the handle
dispatcher `0x142cc45a0`, which builds a **driver context** and calls
`CollisionQueryDriver 0x142cbd510`; the driver runs the broadphase itself and
reaches the narrowphase `0x142cc1610`, which is what finally calls the policy
through the thunk.

**The dispatcher `0x142cc45a0..0x142cc47ce`** (0x22d bytes, leaf, no calls):

1. `if (handle >= 0x3fff) return;` — an unsigned tail-loop bound test.
2. `raw = table[handle]` from `_DAT_1442a0bc0`; if `raw` is the sentinel
   `0xc439709cd97ec6aa` it returns.
3. `record = (raw + 0xb3269b88d44d5fb2) ^ 0x77600ba5adcc265c` — the same
   obfuscated pointer decode the evaluator wrapper uses, so a handle resolves
   to the same world record on both paths.
4. If `request[0x71] != 0` and `record[0x4a8] > 0`, it appends the request to
   a ring of **0xaa0-byte slices** at `record[0x4a0]`: the index comes from
   `record[0x4ac]` while it is below `record[0x4a8]`, otherwise it wraps with
   `record[0x4b0] = (i+1) % n`. The slice's `+0x80` is an inline vector whose
   pointer is compared against `slice+0x80`, reset by `0x142cadf00` when it
   has moved to the heap, its capacity is `0x20` and `+0xa90` takes
   `record[0x4b4]++`. The 0x80-byte request is then copied to `slice+0x00`.
   So `0x142cadf00` is the **inline-vector release/reset of that slice**, not
   a query helper (correcting the earlier note that listed it as one of two
   dispatcher targets).

**The driver context** is built on the dispatcher's stack and is exactly five
fields:

| offset | field |
|---|---|
| `+0x00` | the world record from the handle |
| `+0x08` | `request+0x58` (u16 promoted) |
| `+0x10` | the callback — the thunk `0x14202c580` |
| `+0x18` | `request+0x60` — the answer record the fan reads back |
| `+0x20` | the observation slice, or `0` |
| `+0x28` | `0` |

The query handed to the driver is the request itself, all `0x73` bytes, and
`request+0x5e` becomes its `param_4`: a **pass mask**. The driver is one
`do { ... } while (true)` over four passes, `param_4 >>= 1` at the bottom, and
`piVar17` walking `record+0x44 + 0x28*i`.

**One pass** (`0x142cbdf10..`, the body of the `if (param_4 & 1)` block):
`CollisionSweepBounds(segment, &plane, &t0, &t1)` splits the request's two
float4 endpoints into `local_5f8..fStack_5ec` (A) and `local_5e8..uStack_5e0`
(B), gives `plane = ±request[0x40..0x4f]` and the clip parameter triple in
`local_558`, and yields a sweep AABB. The AABB is then expanded per lane:
`min(A + clip*t0 - B, B + clip*t0 - A)` kept as `auVar56`, its max as
`auVar57`, and `fVar58..fVar61 = B - A` (`BF`).

Of the four field langs, **only family 1 is implemented in the image**:

- `record[0x44 + 0x28*i]` — `int*`, the island's shape-list count, `-1` means
  the island is empty and the whole pass is skipped;
- `piVar17[-5]` — the island's shape array;
- the worklist is the container at `record+0x44+0x28*i+0x3c` = `record+0x80+0x40*i`,
  seeded with one entry and cap `0x100` on the driver's own stack frame and
  grown by `CollisionWorklistPush 0x140d24b60` — called once with the index
  alone (a branch child) and once with `base+0x30+offset` (a sibling entry).
  The driver releases it with `0x142cadf00` only when the data left its inline
  buffer.

Per entry (stride `0x40`): the local AABB is `[base+0x00..0x0f] - lane` and
`[base+0x10..0x1f] + lane`. Three saturating masks decide it: (1) the box
against the sweep AABB on xyz; (2) a slab test with a **0.005** slack,
`abs(BF x (P - A)) > abs(BF)*halfExtent` per axis; (3) `*(i32*)(base+0x2c)`
`== -1` selects a leaf, anything else a branch. A leaf runs
`ShapeNarrowphase(ctx, &hitPoint, &query, *(u32*)(base+0x20))`; a return of
`2` is a hit, `1` aborts the island. On a hit the point is tested against the
clip plane (`movmskps` of `hitPoint < clip`); full containment re-aims the
sweep's plane and AABB from that point — a data-dependent branch whose two
outcomes are documented above but not yet read as one expression.

**`ShapeNarrowphase 0x142cc1610..0x142cc1813`** (516 bytes) is the only caller
of the policy, and therefore the definition of its arguments:

- `lVar4 = *(u64*)(world + 0x2b8) + id*0xc0`; `ShapeWorldTransform(shape,
  &basis)`; `ShapeQueryFilter(ctx+0x08, shape+0x98)` gates it.
- `func_0x142cb8d60(shape, world, &basis, &hit)` fills the hit struct.
- then, with `p = ctx`:
  `uVar2 = (*(code *)p[2])(&hit, CONCAT26(u16 world[0x194], CONCAT24(u16
  shape[0xa8], u32 shape[0xa0])), p[3])` — i.e.
  **`callback(&hit, packedShapeDescriptor, answerRecord)`**.
- when `p[4] != 0`, the contact point is appended to `p[4]+0x80` by
  `0x142cc1450`, tagged `1` on a hit or `2` on a reject.

The hit struct is: `+0x00` and `+0x10` the two float4 the narrowphase writes,
`+0x20` the contact point, `+0x30` u32 preset to `0xffffffff` (then
`shape+0xaa` unless the shape type is 3 or 4), `+0x38` u16 `shape+0xae`.

**The thunk, byte-exact** (`0x14202c580`, five instructions, then `int3`):
`MOV RAX,R8 / MOV R8,RDX / MOV RDX,RCX / MOV RCX,RAX / JMP 0x14202bc90`. It
permutes `(RCX,RDX,R8)` into `(R8,RCX,RDX)`, so the policy body
`0x14202bc90` receives **`(hit, packed, record)`**: parameter 1 is the hit
struct, parameter 2 the packed shape descriptor, parameter 3 the answer
record. The earlier note that its first parameter was "the descriptor the
dispatcher passed third" is superseded — the dispatcher never calls the
thunk, the narrowphase does.

**The policy body `0x14202bc90..0x14202c394` (1797 bytes) is read (2026-11-02),
and it is not a computed-jump dispatcher.** That earlier reading is superseded
— and so is its stated body end `0x14203d94`, which was a transcription error
(Ghidra's `body_end` is `0x14202c394`). A linear sweep of the recovered image
decodes the whole body to exactly `0x14202c395` — 427 instructions, no data
islands, no indirect jumps (`dev/probe_policy_blocks.py`) — so the live path
is contiguous plain code.

Why the decompiler failed anyway: the body is **junk-filler obfuscated**. Six
dead regions sit between basic blocks, each a run of nonsense instructions
ending in an opaque-predicate branch to an address in another function
(`0x14202be2c`/`0x14202be36`/`0x14202be41` → `0x14203c7ff`, `0x14202dfaf`,
`0x142030dc2`; `0x14202c2c2`/`0x14202c2cb`/`0x14202c2de` → `0x14202e840`,
`0x14202df4a`, `0x142049e89`). Ghidra follows those branches into the junk and
then into other functions, which is where `in_stack_00000039`, `unaff_R12D`
and the blocks at `0x142049f49`/`0x14207a112` came from. The targets are dead:
the flags are set by junk instructions the live path never reaches.

Three obfuscation idioms are used, all now resolved:

1. **Junk between blocks** — skip the dead run, continue at the next real
   address (`0x14202be5b`, `0x14202c0db`, `0x14202c2ec`, `0x14202c33d`).
2. **Constant-injection stub** — `0x142018990` is not a real callee: it stores
   `0x10`/`0xf`/`4`/`0` through its four pointer arguments and returns. The
   caller permutes the four slots differently on each call, so the "four
   out-params" are the constants `index = 0`, `shift = 4`, `mask = 0x10`,
   `mask = 0x0f`.
3. **Pointer and value decode chains** — pure arithmetic, recorded below.

**The contract.** `policy(hit, packed, ctx)` (the narrowphase orders the
arguments `(hit, packed, ctx)`; the thunk `0x14202c580` rotates them into
`(hit, packed, record)`). Returns **1 = no hit**, **2 = hit accepted**,
**3 = rejected**.

**The gates, in order.**

```text
world = 0x1420c2360(ctx)                    -- kind dispatch; 0 -> straight to classify
0x1420af950(hit+0x08, world) == 0           -> return 3
packed+0x38 == 0xffff                       -> skip the dedupe scan
```

`0x1420af950` (62 bytes) is the relevance filter: `mask = world[0x280]`; the
hit is rejected when `(x[8] & mask) == 0`, rejected again when
`(x[0x10] & mask) != 0`, and rejected when `world` appears in the
`x[0x20]` array of `x[0x28]` qwords. So `hit+0x08` points at a filter object
with a required mask, a forbidden mask and a world-exclusion list.

**The key.** `rdi = decode([world+8])` with
`decode(x) = ror64(ror64(x + 0x1e2421a4798a328, 0x22) ^ 0xe1379e3cb308170d, 0x2b)`.
The table walk is
`t = [rdi + 0x110]`, `idx = popcount(t & m) + byte[rdi+0x130]`,
`v = [[[rdi+0x80] + idx*8]]`, then
`v = ror(v,0x38) + 0x4d79452d486f9431; v ^= [0x143b79598 + 0x198];
v ^= 0xec99e783d64e4c83; v -= 0x327aecc3f519246f; v ^= byte[0x1438480f3]`,
and `0x140543440(&v)` decodes in place:
`v = ror64((ror64(zero32(v),0x14) ^ 0x8b084bd971b946cc) - 0x32c1c6a5a278ca6f, 0x26)`
(its entry `0x142543440..0x142543480` is junk; live code starts at
`0x140543481`). The presence bit is `(t & 0x10) >> 4`, and the key is
`h = ror64(v, 0x15) & -(int)presence`, a `neg`/`movsxd` mask that yields 0
when the bit is clear.

**The two call sites bind the stub's constants differently, and that has a
consequence.** The stub stores `0x10`/`0x0f`/`4`/`0` into the four slots; the
first site (`0x14202bd39`) reads the popcount mask from `0x10` and the
presence mask from `0x0f`, so its presence term is `(t & 0x0f) >> 4`, which is
**always zero** — the dedupe scan over `[world+0x240]`/`[world+0x248]` at
`0x14202be78..0x14202beae` is therefore **unreachable in this build**, and so
is the `[h+0x250]` variant lookup that hangs off it. The second site
(`0x14202bfc7`, mode 1 only) swaps them: popcount from `0x0f`, presence from
`0x10`, so **bit 4 of `[rdi+0x110]` is exactly the "shape id is an index"
bit** — set, the low 15 bits of `packed+0x34` are resolved through
`[[h+0x250]]` with `0x141b3c890(obj, &out, packed[0x34] & 0x7fff)` into
`out.id & 0x7fff` / `out.kind`. Record both facts as the shipped behaviour:
the dedupe is dead code, the variant resolution is live.

**The optional filter.** If `hit+0x18` is non-null it is called as
`hit+0x18(rdi, hit+0x20, hit+0x28)`; a false return is **3**.
**The mode switch** on `hit+0x34`:

- **0** — `hit+0x39 = 1`, return **1** (presence test, no record).
- **1** — fill the caller's record `rec = [hit+0x10]`:
  `rec+0x00`/`rec+0x10` = 16 bytes each from `packed+0x00`/`packed+0x10`
  (point and normal), `rec+0x20 = rdi`, `rec+0x28 = ctx`, `rec+0x30` = the f32
  at `packed+0x20` (the hit fraction the fan scores), `rec+0x34 = packed+0x30`,
  `rec+0x38 = word packed+0x38`, `rec+0x3c = word packed+0x34`,
  `rec+0x3e = word packed+0x36`, `rec+0x40 = 0`. When `word packed+0x34` is
  **negative** it is an index: `rec+0x3c = 1` first, then, only when `rdi` is
  non-null and bit 4 of `[rdi+0x110]` is set, the id is resolved through
  `[[h+0x250]]` with `0x141b3c890(obj, &out, packed[0x34] & 0x7fff)`
  and written back as `rec+0x3c = out.id & 0x7fff`, `rec+0x3e = out.kind`.
  Returns **2**.
- **≥2** — build a local record on the stack (same fields, plus `+0x3a` =
  `0xffff` and a zero byte at `+0x48`), resolve a negative shape id the same
  way, then classify:
  `class = hit+0x00(&record, [hit+0x08] + 0x470)`. If `class == 1`, the score
  `packed+0x20` is written to `hit+0x30` and the return is **2**; if
  `class == 2` the return is **3**; otherwise `hit+0x38 = 1` and the return is
  **1**.

So the policy writes exactly three outputs on the hit object — `hit+0x30` (the
score the fan's acceptance rules compare), `hit+0x38` (a one-byte flag) and
`hit+0x39` — plus the caller's 0x50-byte record. Everything it reads that is
not a field of `hit`/`packed` comes from the decoded geometry object `rdi`
(`[rdi+0x80]` a table, `[rdi+0x110]`/`byte[rdi+0x130]` its index inputs,
`[rdi+0x250]` a variant table), which is **map data**: the ingest is item 6.

**What is left of this stage.** No code. The remaining gap is data: the
contents of `[rdi+0x80]`/`[rdi+0x110]`/`[rdi+0x250]`, i.e. the geometry the
map supplies, so the policy can be implemented and unit-tested against
synthetic worlds, but a candidate still cannot be scored for real until the
collision ingest exists. `dev/probe_policy_blocks.py` reproduces the sweep.

The earlier "emulate the decode" anchor is **withdrawn**: there is no
decode to emulate.

**Tooling note (2026-11-02).** `0x140c9e670` carried a wrong `noreturn` mark in
project, which truncated `MoverWishWriter`'s decompilation at the transform
call and hid the whole fan body from the decompiler (the earlier readings of
this function came from the listing). The mark is cleared in the project
image; the fan now decompiles end to end, and the body matches the
instruction addresses already recorded above.

## What the current image shows (2026-10-25)

`MoverAdvance` is not a leaf. Its character path computes a desired position
and then runs the collision sweep:

```text
desired_i = (param_5 * body[0x18_i] + wish_i) * param_6 + param_4_i
RunCharacterControllerSweep(body[0x35b], step_state, desired)
new_pos   = FUN_142cbcd10(body[0x35b], old_pos)
```

`param_3` is the local wish vector; `param_4` is the current position;
`param_5` and `param_6` are the per-frame scales. `MoverInvoker` passes eight
arguments through to it:

```text
MoverInvoker(mover, out_pos, wish, pos, scale_a, scale_b, step_state, world)
```

`MoverInvoker` is reached through the interface vtable `0x142fdbf40` slot 9.
The function that computes `wish`, `scale_a`, `scale_b` and the step state for
a given input frame is **not yet located**. That is the ramp/control law.

Two more read pieces are on the same path:

- `ApplyStateModifiers` (`0x140cbcbe0`) evaluates the mover body's 0x38-byte
  state-modifier records at `+0x870..` and writes the movement parameter block
  at `+0x5d0..+0x67c` (base/effective pairs) plus the flags at
  `+0x652`/`+0x659`/`+0x1a81`.
- `SubmitMoverPhysicsCommand` (`0x140cc03d0`) calls
  `BuildMoverPhysicsCommand` (`0x140cb74a0`) and dispatches the command to
  Domino through `SubmitCharacterControllerCommand` (`0x142cf5770`).

So the control law is one layer above `MoverInvoker`, and the movement
parameters it consumes are the body block `+0x5d0..+0x67c`. The next read is
the caller of `MoverInvoker` and the producer of that block.

## The open ramp

The field index finds **no per-frame writer** for the body's `+0xc0..0xcc`;
the stores are constructors and bulk copies. The verified ramp leaf reads
`+0xc0..0xcc` as the four-lane **direction** and multiplies the scalar ramp by
it, so `+0xc0` is not a stored velocity. The state modifier is an inline
vector: the pointer at body `+0x870` points at `+0x880`, the count is at
`+0x878`, and the capacity at `+0x87c`. Its records are populated at runtime.

What is still open is the layer that produces the ramp's inputs and the
state-record vector:

1. `FUN_140cc0dc0` (`0x140cc0dc0`): writes the wish vector at `+0x1140` from
   an input object's `+0x60..0x6c`, then updates the `+0x10fc`/`+0x1100`
   spring against the parameter block at `+0x5c0`/`+0x5c4` and the clamps at
   `+0x688`/`+0x68c`. The spring itself is **verified** (`fn/MoverWishSpring`);
   the remaining gap is its input source, "The wish source" above.
2. `ApplyStateModifiers` (`0x140cbcbe0`): a **37-way** state-modifier switch
   (kinds `0x00–0x10`, `0x16–0x21`, `0x27–0x2e`, holes between), six ops
   (add/mul/set/min/max/clamp) over the 0x38-byte records at `+0x870` (count
   `+0x878`). Parameter block base is `body+0x5d0`; `+0x5d0..+0x5dc` and
   `+0x638..+0x644` are master-scale four-lane vectors, `+0x5e8..+0x608` are
   per-kind multipliers, `+0x60c..+0x620` assigns, `+0x65c` a clamped assign,
   `+0x652/+0x658/+0x659/+0x1a7b` threshold bools, `+0x66c..+0x680` angle
   fields. It calls the body vtable at `+0x68`/`+0x70`; a body-only fixture is
   feasible. The base table's two slots are stubs (`0x14053a420` returns 0,
   `0x14053a070` returns 1).
   **Fixture note:** the oracle maps the raw image at the v2b base and does
   not apply PE relocations, so the table's stored entries are exe addresses
   and fault. A body-level unit must build a fake table in the arena whose
   `+0x68`/`+0x70` slots hold the stubs' v2b addresses
   (`0x14053a420`/`0x14053a070` rebased), and point `body+0` at it. The
   end-of-function notifications at `+0x660`/`+0x664` are skipped when those
   two floats equal the values written by the no-modifier path (`1.0`); a
   case that changes them would call the service lookup `0x140c28eb0`.
3. `SubmitMoverPhysicsCommand` (`0x140cc03d0`): builds the command and
   dispatches it through `FUN_1420163b0`; it does **not** call
   `SetCharacterControllerVelocity`. The command is six bytes — three u16
   (block mask, collide-with mask, signed group) written to Domino record
   `+0x98/+0x9a/+0x9c` — and `BuildMoverPhysicsCommand` (`0x140cb74a0`) is its
   producer. The dispatch is not unit-verified.
4. The Domino character controller is a **shape carrier and broadphase
   updater**, not an integrator: `SetCharacterControllerVelocity`
   (`0x142cbd1b0`) -> `0x142cf5810` -> `ApplyCharacterControllerMove`
   (`0x142cd5370`) -> `MoveCharacterController` (`0x142cd4bc0`) publishes
   velocity/quaternion and re-registers the shape. Gravity, acceleration,
   friction, step and slope are not in this path; the integrator is
   `MoverAdvance` (`0x140ca7f30`): `target = pos + scale_b*(wish +
   scale_a*body[0xc0..0xcc])`, gated by `body+0x1a78 != 0 && body+0x658 != 0`.
   The body's `+0x1ad8` is a **slot**; `[slot]` is the variant object, whose
   tag selects a `{u32 index, u16 generation, u16 space}` handle. The handle
   resolves through the table at `0x1442a0bc0` with a generation check; a
   fabricated handle cannot pass it.
5. The wish source: the first-person view object (`vtable 0x142f87810`, slot
   `+0x250` = `0x140654530`) writes the actor command record at `+0x1210`;
   `FUN_140689930` is probably driven from `[actor+0x38]+0x178`; the
   state-record pass reaches `FUN_140cb63c0`/`FUN_140cb86c0`, which builds the
   source struct's wish and gate. The exact `in_vec` expression in
   `FUN_140cb86c0`, the actor `+0x38` vtable, the `+0x58/+0x60/+0x68`
   virtuals and the command byte meanings are still unread.

The `MoverInvoker` slot-9 caller is still not found. It may be dead in this
build: the live per-frame path observed through `MoverStateMachine` drives the
Domino velocity handle directly. That is not proven either way, so it stays
open rather than being replaced by a guess.

The verified leaves are not a replacement for the control law: they are the
arithmetic that the control law feeds. The remaining candidates are the four
above, in that order. Whichever is read, the implementation must follow it
operation for operation, not match a trace.

## The ramp search (2026-10-31; open)

`MoverInvoker` is slot 15 of the body vtable at `0x142fdbf10`: the open
question's "interface vtable `0x142fdbf40` slot 9" is the same entry
(`0x142fdbf40 + 0x48 = 0x142fdbf88`). Read from the offline image's `.rdata`
(file offset == RVA): the surrounding entries hold `DestructMoverBody`
(`0x1407ff270`), a size virtual returning `0x1c00` (`0x1407ff1e0`),
`GetMoverStateDescriptor` (`0x1407ff4e0`) and `GetMoverStateId`
(`0x1407ff4d0`) (evidence: read).

The only installer of that vtable in `.text` is `0x1407ff1f0` (called from
`0x1407ff1b0`), which calls `ConstructMoverBase` and then installs
`0x142fdbf10` at `+0` and `0x142fdbfc8` at `+0x20`. No `LEA` in `.text`
targets `0x142fdbf40` exactly (offline byte scan of all REX-prefixed
`LEA reg,[rip+disp]`; evidence: read). There is no separate interface
object carrying it; the slot-9 framing is interior to the body vtable.

`MoverInvoker`'s only reference is the vtable entry: its bridge xrefs are
three `DATA` hits interior to unwind info plus the slot, and
`MoverAdvance`'s only code caller is `MoverInvoker` (evidence: read).

A whole-image offline scan found 428 `call [reg+0x48]` sites (328
functions) and 408 `call [reg+0x78]` sites (302 functions); intersected
with mover-body field functions (nine seed registers, `0x140000000` to
`0x143000000`), no function with mover state calls slot 9. The slot-15
sites near mover code are accounted for: the record dispatch inside
`MoverAdvance` (`0x140ca8017`), `ConstructMoverBody` (`0x140ca9ea1`), the
`0x140cbfbc3` / `0x140cbff6a` / `0x140cc0112` stubs, a handle-release loop
(`0x141ea14e0`) and unrelated interfaces (evidence: read).

`FUN_140cc0dc0` (`0x140cc0dc0`, a `.pdata` start) writes the wish vector at
body `+0x1140..+0x114c` from its second argument's `+0x60..+0x6c` (gated on
that argument's `+0x98` and flags `& 0x80000c == 0`), then computes
velocity-like terms scaled by body `+0xc0/+0xc4/+0xc8` with a `0.001953125`
deadband, a `6.2831855`-per-second filter against the parameter block at
`+0x5c0/+0x5c4`, and clamps from `+0x688/+0x68c` (evidence: read). Its
direct callers are `0x140cbbb2c` (inside `MoverUpdateFrame`, entry
`0x140cbb260`) and `0x140cb5de3` (inside `MoverUpdateCommand`, entry
`0x140cb5760`) (bridge xrefs; evidence: read).

The live per-frame path drives Domino directly rather than the invoker:
`MoverStateMachine` (`0x140cbacc0`) calls `ApplyStateModifiers` then
`SubmitMoverPhysicsCommand` (it never sets the controller velocity), while
`MoverUpdateFrame` calls `ApplyStateModifiers`, `SubmitMoverPhysicsCommand`,
the wish spring, the state/ramp vectors, and then
`SetCharacterControllerVelocity` on the world handle at `0xcbbe0b` and on the
list at `0xcbbf9a`. `MoverUpdateCommand` sets it at `0x140cb58b4`.
`MoverStateMachine`'s direct callers are `0x14068b158`/`0x14068b2a4`,
`0x14068a312`, `0x1409d70b0`, `0x1409e4aed` and `0x14068b5ea` (bridge xrefs;
evidence: read); two of those ranges return bridge fragments or misaligned
decompiles (64-byte-window artifacts) and are not readable through the
bridge.

Blocker: the caller that builds the invoker's eight arguments is not
located. Either it holds the body in a register or alias chain the offline
scan misses, or the slot has no in-image caller (the sweep path may be
dead in this build, superseded by the velocity path) — neither is proven.
The verified ramp and command leaves do not locate it.

**2026-11-01 evidence.** The 8-byte value `0x1407ffc50` occurs exactly once in
the whole image: at `0x142fdbf88`, the body vtable's slot 15. There is no
direct `call`/`jmp` and no second pointer table. `MoverAdvance`'s only caller
is `MoverInvoker`, and the body vtable is installed only by `FUN_1407ff1f0`.
That makes the invoker path **very likely unreachable**, but a complete
indirect-call sweep was not run, so it stays open, not dead. The live update
is `MoverUpdateFrame`/`MoverUpdateCommand`, which call the controller directly.

Next: the wish source's remaining reads — `FUN_140654730`'s command field
producers, `FUN_140cb86c0`'s exact `in_vec` expression, the actor `+0x38`
vtable and the `+0x58/+0x60/+0x68` virtuals. `MoverWishSpring` is verified
with a seeded service object; the live service object is still open. A
headless `McpDecompile.java` batch over the caller ranges with
`DumpFunctionRanges` entries (needs the GUI project closed), or
`McpCallers.java` for the slot-15 sites, remains the fallback for the invoker
caller.

## The world handle (2026-11-01)

`body+0x1ad8` is a **slot**: `P = [body+0x1ad8]`; `[P]` is the variant
object `V`, not the handle itself. `0x142cbcec0(P)` returns the zero vec4
when `P` is null, reads `V = [P]`, and `0x142cf5630(V)` dispatches on
`[V]`: `1` selects `[V+0x10]` (rigid), anything else selects `[V+8]`
(character). A null `V` also returns the zero vec4.

Both handles are encoded ids: `space = handle >> 0x30`,
`index = (u32)handle`, `generation = (u16)(handle >> 32)`. Both resolve
through the **same** global at `0x1442a0bc0`, which holds the table pointer:
`table = [global]`, `encoded = [table + space*8]`,
`record = (encoded - 0x4cd964772bb2a04e) ^ 0x77600ba5adcc265c`.
The character layout is array `[record+0x2a0]`, count `[record+0x2ac]`,
stride `0x50`, index `+0x20`, generation `+0x34`; the rigid layout is array
`[record+0x288]`, count `[record+0x294]`, stride `0x1c0`, index `+0x00`,
generation `+0x198`. Both put the position at entry `+0x10`. Every failure
returns the zero vec4.

`SetCharacterControllerVelocity` (`0x142cbd1b0`) takes the same variant,
copies the vec4 and forwards it to `0x142cf5810`.

## Collision

The character path is a collision sweep, not a bare integrator. `MoverAdvance`
calls `RunCharacterControllerSweep` (`0x142cbcee0`) -> `0x142cf5660` ->
`0x142cf3f80` (a >29 KB solver). The sweep takes no radius/height/mask
arguments: the shape is the character object's own record array and the mask
is the command's filter triple. It iteratively depenetrates with `eps = 0.005`,
an iteration cap from `step_state+0x94`, and stops when the three-lane
correction squared is below `eps` squared. It writes per-contact planes
`(nx,ny,nz,separation)` plus an entity handle into the caller's block, and the
resolved position at `+0x620`.

`MoverAdvance`'s `world` argument is a **result buffer**, not a collision
world. The sweep reads precomputed triangle records (normal `+0xc0..+0xc8`,
count `+0xf6`, 0x30-byte vertex stream from `+0x18`) from the spatial
structure the command triple maintains, never map-chunk bytes. The
map-chunk-to-shape ingest is **not read**; that is the gap between
`formats/maps.md` and the query. The client has no gameplay collision
implementation yet; the map mesh is visual only.

## The collider build (2026-11-02)

The shape the sweep walks is a 0x60-byte record built by
`ColliderFaceAppend` (`0x142cca460`) and then flattened into the two-level
runtime arrays. Read end to end, with constants resolved against
`dev/data/offline_recover/module_offline.bin`.

**Inputs.** `A` (rcx) is the build object: `+0x20` face array, `+0x28` face
capacity, `+0x2c` face count, `+0x30` build nodes (0x50 stride), `+0x38` nodes
used, `+0x3c` node capacity. `B` (rdx) is zeroed and written as the 0x30-byte
result. `C` (r8) is the source: `+0x00` vertex array (0x10-byte `vec4`),
`+0x08` face array (**0x14-byte** records), `+0x10` face count.

**The face record.** The face count is clamped to `0x4000000`. Each source
face gives the three `i32` vertex indices at `+0x00/+0x04/+0x08` and three
`u16` ids at `+0x0c/+0x0e/+0x10`; a face `ColliderTriangleDegenerate` rejects
produces no record and no area. The record written at `A[0x20] + count*0x60`:

| offset | content |
|---|---|
| `+0x00`, `+0x10` | `vec4` box min, max (lane-wise `minps`/`maxps` of the three vertices) |
| `+0x20` | `vec4` centre `(min + max) * 0.5` |
| `+0x30` | `-1`, then `+0x34` = f32 area `0.5*|cross(v1-v0, v2-v0)|` |
| `+0x38`, `+0x3c`, `+0x40` | the three vertex indices `v0`, `v1`, `v2` |
| `+0x44`, `+0x48`, `+0x4c` | per-edge adjacency, seeded `0`, `0`, `-1` |
| `+0x50`, `+0x52`, `+0x54` | the three `u16` ids |

`B+0x28` accumulates the areas. Then a half-edge pass builds one bucket per
directed edge (`(v[i], v[(i+1)%3])`, `(i+1)%3` by the `0x55555556` magic
divide) keyed by `start | end<<32` and hashed
`t = (k<<0x12) + ~k; t ^= t>>0x1f; t *= 0x15; t ^= t>>0xb; t *= 0x41;
t ^= t>>0x16`, buckets 0x20 bytes chained through dword `+0x10` with the key at
`+0x08` and a paired latch at `+0x18`. A hit writes the **vertex opposite the
shared edge** into each face's `+0x44/+0x48/+0x4c`; the edge without a twin
keeps its seed.

**The result.** `B+0x08` is the vertex array, `count = maxIndex + 1` elements
of `(x, y, z, 0.0)` (lane 3 forced to zero); `B+0x1c` is that count. The build
node array is `2*faceCount - 1` entries of 0x50 bytes; `0x142ccb1b0` builds the
tree and `0x142ccad80` flattens it into `B+0x00` (runtime nodes, 0x20 bytes,
count `B+0x18`) and `B+0x10` (runtime faces, 0x20 bytes, count `B+0x20`),
writing `B+0x24` = tree height and padding each runtime box by `max(0.01,
0.005)` = `0.01` per lane. The runtime face carries `{v0, v1, v2, 0, 0, -1}` at
`+0x00..+0x14` and the three `u16` ids at `+0x18/+0x1a/+0x1c`; the adjacency
words are **not** copied into it.

`crates/ow-sim/src/collider.rs::faces` matches this except for the half-edge
adjacency (it leaves the seeds) — that pass is **not implemented and not
tested**, and it matters only if the response reads `+0x44/+0x48/+0x4c`.

## Where a map's cells come from (2026-11-02)

`GeometryBuild` (`0x14213c440`) is the only writer of a cell
(`0x142136010`, one call site) and `GeometryFinalize` (`0x14213d0b0`) is the
only writer of a pool-A record (`0x142136130`, one call site). Both build from
the geometry node's **brush tree**:
`BrushTreeToMesh` (`0x142cf23c0`) -> `MeshToColliderFaces` (`0x142cf2010`) ->
face records -> BVH. Nothing materialises a cell from the chunk's 17 arrays:
`MapChunkCellBytesFixup` (`0x141ff51a0`, one caller `0x141ff57f7`) only rebases
the 17 qwords at `+0x00..+0x80` from relative to absolute, the collision-chunk
component (`0x140f8e000`..`0x140f94000`, class hash `0x9199bf6b`) never touches
`+0x68`, and no cell/sector producer reads the container. The sweep's triangle
records therefore come from brush trees carried by models; the chunk decoder is
**open** and unlocated (see `docs/open-questions.md` item 6). The corrected
28-function census of every shape producer, and the reason the first version of
this claim overstated its evidence (Ghidra's caller metadata is incomplete for
this image), are in `maps.md`, "The cell path's consumer: open".

**The cell** (`GEO+0x108`, stride 0x60): `+0x00/+0x10` two `vec4` copied from
the mesh record, `+0x20` the 0x30-byte type-3 mesh source (vertex base at
`+0x10`, triangle base at `+0x20`), `+0x30 + k*0x20 + 0x18/+0x1a` a `u16` pair
per triangle, `+0x50/+0x58/+0x5c` the face-list pointer, count and flag.
`GeometryFinalize` remaps every `u16` through `0x142cee0e0` and calls
`ShapeAddToBody` (`0x142cb3940`) with constants `100.0`, `1.0`, `0.1`,
`FLT_MAX`, `0xffff0000`, `0x3000100`.

**The sector** (`GEO+0x118`, stride 0x100): `+0x00..0x1f` transform, `+0x20`
distance, `+0x24` the "distance exactly zero" byte, `+0x28` limit,
`+0x30`/`+0x38` two words from the accept test `0x14213c240`, `+0x40`/`+0x50`
two `vec4`, `+0x60` byte, `+0xa0` 16 bytes, `+0xe0/+0xe8/+0xec` three words.


### The advance's contact pass (2026-11-01)

`MoverAdvance` (`0x140ca7f30`) builds the sweep's argument block and then walks
the sweep's contact list. The read expression, in order:

```text
; the desired vector, at [rbp-0x60]
0x140ca809a  movss  xmm0, [rbp+0x70]      ; scale_a (5th argument)
0x140ca80a3  shufps xmm0, xmm0, 0
0x140ca80aa  mulps  xmm0, [rdi+0xc0]      ; scale_a * direction
0x140ca80be  addps  xmm0, [rsi]           ; + wish      (rsi = the 3rd argument)
0x140ca80c1  mulps  xmm0, xmm6           ; * scale_b   (xmm6 = broadcast of [rbp+0x78])
0x140ca80c4  addps  xmm0, [r9]            ; + pos       (r9 = the 4th argument)

; the sweep block at [rbp-0x50]
+0x00..+0x0f  the desired vector above
+0x10         scale_b
+0x14         body[0x628] * scale_b               (the rip constant is 1.0)
+0x18         0x3ba3d70a (0.005)
+0x1c         step_state[0x94], clamped by body[0x1a88] when the global is set
+0x20         step_state[0x9a] (byte)
0x142cbcee0([body+0x1ad8], world, &block) ; the sweep
0x142cbcd10([body+0x1ad8], &v)            ; the resolved position -> *out_pos
```

So the advance's four-lane term is `(scale_a*dir + wish)*scale_b + pos` — the
same `+0xc0` direction the ramp leaf reads, and the parameters are the two
scale arguments the invoker passes, not stored fields. `MoverAdvance` is
reached only from `MoverInvoker` (`0x1407ffcac`), and `MoverInvoker` has no
code caller, so this expression is **not on the live path** (see "The ramp
search").

After the position read the advance walks the sweep's contacts:

```text
count = [world+0x20c]                     ; contacts
if count > 0:
  body[0x10d0] |= 0x20000
  n = [ (body+8 -> hashed service object) + 0x40 ]      ; 4 lanes
  len2 = n.x*n.x + n.y*n.y + n.z*n.z                    ; (x*x + y*y) + z*z
  if len2 > 1.17549435e-36:                             ; 0x142f7aad0
    r = rsqrtps(len2); r = 0.5*r*(3 - len2*r*r)         ; one Newton step
    n = n * r
  else:
    n = (0, 1, 0, 0)                                    ; 0x14328cdd0
  for i in 0..count:
    record = [world+0x200] + i*0x20
    v = record[0..0x10] * n                             ; mulps, lane-wise
    s = (v.x + v.y) + v.z                               ; the three-lane sum
    body[0x19b8] = minss(body[0x19b8], s)
    call [[service_object] + 0x40](service_object, record+0x14, &delta,
                                   &out, body[0x64c])
else if count >= 4:                                     ; 0x140ca8643
  the four-or-more path (body[0x19b8] and a rotation loop, still unread)
```

Contact records are 0x20 bytes; the first 0x10 bytes are the plane and the
rest is the entity/response data. `n` is the service object's vector at
`+0x40`, normalised with one `rsqrtps` Newton step and falling back to
`(0, 1, 0, 0)`, so `s` is the contact plane's projection onto that direction.
The minimum is accumulated into `body+0x19b8`, the field `MoverUpdateCommand`
reads at `0x140cb593c`. The per-contact call is virtual through the service
object's `+0x40` slot, which is still open (it is the response step).

## The configuration path (2026-11-01)

The verified ramp reads `+0x5a8` or `+0x5b4` (selected by `+0x10d0 & 2`) and
`+0x10ec`; the verified spring reads `+0x5c0`, `+0x5c4`, `+0x688` and
`+0x68c`. Those fields are now read at the value level:

- `0x1405a87d0` default-constructs the settings object. It writes
  `+0x5a8 = +0x5ac = +0x5b4 = 1.0`, `+0x5c0 = 0.0` (through the qword store
  at `+0x5bc`), `+0x5c4/+0x5c5/+0x5c6 = 1` and `+0x5c8 = 0`, plus the
  parameter-block seeds `+0x5d0 = -2`, `+0x5d4 = 1.0` and `+0x5dc = 0x10000`.
  Four fields are pointers into the object itself (`+0x10`, `+0x70`,
  `+0x280`, `+0x558`), so the constructor is address-relative. This is
  `fn/DefaultMoverSettings`.
- `0x140c862e0` (the real entry; `.pdata` starts a chained fragment at
  `0x140c862f1`) applies a movement asset to the settings object. The asset
  fields map as follows: `+0x78` -> `+0x60`, `+0x7c` -> `+0x8`, `+0xe8` ->
  `+0x5a4`, `[+0x100]` -> `+0x5a8`/`+0x5ac`, `+0xf0` (u16) -> `+0x5b0`,
  `+0xf4` -> `+0x5c0`, `[+0xd0]` bytes 0..2 -> `+0x5c4..+0x5c6` as bools,
  `+0xec` -> `+0x5c8`, `+0x80` -> `+0x51c`, `+0x84..+0x8c` -> `+0x4a0`,
  `+0x9c..+0xa4` -> `+0x4b0`, `+0xa8..+0xc8` -> `+0x4e0`/`+0x4f0`/`+0x500`,
  `+0xf2` (u16, tagged) -> `+0x530`, `+0xf8` -> `+0x5e2`, `+0x130` ->
  `+0x5dd`, and `[+0x118]`/`+0x120` -> the `+0x10` vector. With a null state
  record it copies `+0x90..+0x98` into `+0x490`; otherwise it rotates those
  three values through the state record's transform, including the
  `[object+0x298]` parent path.
- The apply writes **none** of `+0x5b4`, `+0x5b8`, `+0x5bc`, `+0x68c` or
  `+0x10ec`. `+0x5b4` has no non-copy writer in the whole image: it keeps the
  default `1.0` unless the settings object is copied.
- `+0x688`/`+0x68c` are computed in `ConstructMoverBody` (`0x140ca9730`):
  `+0x688 = min(-max(+0x448 + +0x440, +0x454 + +0x44c), 0)` and
  `+0x68c = max(2 * [+0x46c], 0)`. These are the clamp pair the spring
  applies at `0x140cc1002..0x140cc1022`; `FUN_140ca02d0` (`0x140ca02d0`)
  normalises the same pair.
- `+0x10ec` is a per-frame accumulator. `FUN_140cc079d` (`0x140cc079d`)
  writes it after the ramp call; the ramp reads `min(+0x10ec, K)` times the
  selected scale.
- Copy routines `0x1405a7dc0`, `0x1405a8640`, `0x14067e5d0` and `0x14078a5e0`
  copy the block field by field, and `0x140545240` copies a settings object
  into the object created by `0x140542b40`.

**What remains open on this path** is the asset *producer*: which install data
builds the object that `0x140c862e0` reads, and where the live body receives
its copy of the settings block. The field mapping, the defaults and the apply
are read and differentially tested.

**Measured SSE NaN rules (2026-11-01).** The `fn/ApplyStateModifiers`
differential exposed two rules that the earlier notes had backwards, and a
direct instruction probe on the image confirmed them:

- `minss`/`maxss`: **any NaN yields the source operand** (the second
  operand), including a NaN destination. `maxss(0, NaN)` is NaN, not 0.
- `mulss`/`addss`: a quiet NaN in the **first** source wins over a quiet NaN
  in the second; a quiet NaN beats a signaling NaN in the other position; a
  lone NaN is quieted.

`ow-sim::movement::{minss, maxss, sse_mul, sse_add}` implement these and the
unit tests pin them. They change the ramp's both-NaN lane: the first source
(the scale) wins, not the direction lane.

## The fan's mesh-face refinement (2026-11-07)

`FUN_140cc2290` has one caller, `MoverWishWriter` `0x140cb86c0`, and it is the
last input behind `ow-sim::fan`'s `CandidateSolver` seam. It is **not a debug
draw**: it writes the query back and returns a `bool` in `EAX`
(`MOVZX EAX, R14B` at `0x140cc2c08`, `XOR AL, AL` on the other path), which is
why Ghidra's `void` must not be trusted here.

Its body is `0x140cc2290..0x140ce0ad0` (6183 bytes) **after the repair
described in `docs/ghidra-project.md`** - the stored extent was 125 bytes
because the cookie check was flagged non-returning. A bad-disassembly spot at
`0x140ce0863`/`0x140ce0866` (two instructions overlap) still truncates the far
end of the decompile, so the blocks past `0x140ce0800` are not read yet.

Arguments: `(owner, query, radius, mask, threshold)`. The query record is
`+0x00` a point (two quads), `+0x10` the normal it writes, `+0x28` the shape
handle, `+0x3e` a `u16` it writes.

Gate. `query+0x28` must not be the 8-byte sentinel `(-1, -1, 0, ...)` and
`0x142cba720(query+0x28)` must answer `3`, so **the ring only runs for a type-3
mesh shape** - the type the practice range's static collision is made of.

The whole function is one **sphere-vs-mesh nearest-triangle query**:

```text
handle  = 0x142cba2a0(query+0x28)        // the shape's body handle
local   = 0x142cb44c0(handle, query)     // the query point in the body's frame
best    = radius * radius * 3.0          // the initial acceptance distance
quat    = 0x142cb48a0(handle)            // the query's orientation
basis   = 2*row + (0, 1, 0)              // the facing row built from that quat
object  = 0x142cba680(query+0x28)
ids     = 0x142caea70(object, inline, local - radius, mask)
                                         // fills an inline 0x80-entry container
                                         // (count at +0x04); freed through
                                         // 0x142cadf00, the same container the
                                         // query driver's worklist uses
```

`0x142cb44c0` is `ResolveBodyLocalPoint(handle, point) -> [x, y, z]` and it is
worth its own note because it pins the body record: `handle >> 0x30` must be
below `0x3fff`, `(int)handle` must not be `-1`, the table is
`[0x1442a0bc0] + slot*8` decoded with `^ 0xb3269b88d44d5fb2 ^ 0x77600ba5adcc265c`,
`(int)handle` must be below `[record+0x294]` (the body count), the entry is
`[record+0x288] + i*0x1c0` with `[entry+0x00] == i` and
`(handle >> 0x20) & 0xffff == [entry+0x198]` (a version word), and the answer is
`(point - entry[+0x10])` rotated by the quaternion at `entry+0x60`. So the
`0x1c0` pose stride, the `+0x10` position and the `+0x60` quaternion are the
same three the placeable transform reads.

`0x142caea70` (`FUN_142caea70`, 101 bytes) is the gather itself, and it is a
**point-in-mesh walk**, not a ray or a sphere test: the box it takes is 8 floats
(min corner in `0..3`, max corner in `4..7`) and both corners are multiplied
componentwise by the object's quad at `+0x10`, so that quad is a scale into the
object's local space. Then:

```text
root = object[0x20]; if root == null -> nothing
if (object[0x44] >= 0x80) -> nothing      // the 128-entry traversal stack
stack[0] = root
node:
  if ((node[0x1c] u16 & mask) == 0)  -> skip
  if (!(point inside node's box))    -> skip   // min +0x00, max +0x0c, xyz only
  if (node[0x1f] byte == 0)          -> push node + (int)node[0x18] * 8, descend
  else for i in 0 .. node[0x1f]:              // children at object[0x30] + i*0x20
    tri = child; if ((tri[0x1c] u16 & mask) == 0) -> next
    v0,v1,v2 = vertices[tri[0]], vertices[tri[1]], vertices[tri[2]]
                                          // object[0x28], float4 each
    if (0x142cd58a0(&point, &v0, &v1, &v2)) push (float)triangle_index
```

The container is `+0x200` data / `+0x208` capacity / `+0x20c` count, doubles its
capacity, copies with `0x142efe2b0` and frees with `0x142cadf00`. It holds
**floats that are triangle indices** - which is why the caller reads the count
from the container's `+0x0c`, and why the practice range's static collision is a
plain triangle soup behind a box BVH.

`0x142cd58a0` (985 bytes) is the gather's accept: a **box-vs-triangle overlap**
test, in separating-axis form. Its first argument is the same 8-float box the
gather holds - `0..3` the min corner, `4..7` the max corner - which it splits
into `centre = (max + min) * 0.5` and `half = (max - min) * 0.5`; the other three
are the triangle's vertices, float4 each. The axes it tries, in order, are the
three slab axes, the triangle's normal, and the three triangle edges crossed
with the box's own axes, each with a `0.005` tolerance. It returns
`(movmskps & 7) == 0`, so the `w` lanes never vote.

`0x142cc5bc0` (1229 bytes) is Ericson's `ClosestPtPointTriangle` - the
Voronoi-region form, not the seven-branch `if` form. `param_1` is three vertex
quads at a `0x10` stride, `param_2` the query point's three floats. It forms
`a = v0 - p`, `b = v1 - p`, `c = v2 - p` and then

```text
d1 = dot(ab, a)   d2 = dot(ab, b)   d3 = dot(ac, a)
d4 = dot(ac, c)   d5 = dot(bc, b)   d6 = dot(bc, c)
va, vb, vc       = the three Voronoi numerators, from the face normal
```

and selects the region with `d1 < 0 && d3 < 0` (vertex A), `d2 > 0 && d5 > 0`
(B), `d6 > 0 && d4 > 0` (C), `va <= 0` (edge BC), `vb <= 0` (CA), `vc <= 0`
(AB), otherwise the interior. The weight is a `DIVPS` by the region's
denominator, then blended by `CMPLTPS` against the constants at `0x142f7aaf0`
and `0x143391030` (a degenerate-denominator guard), and the three vertex quads
are `MULPS`-ed by it and summed.

Ghidra prints the function as `void` and drops the last blocks, so the tail had
to be read from the instruction stream: `ADDPS XMM0, XMM3` at `0x142cc6049`,
then the register restore and `MOV RSP, R11` at `0x142cc6089`, so **the answer
is returned in `XMM0` as a float4**. The `w` lane is carried through the whole
computation untouched, which is why every region test is `movmskps & 7`.

The gather's data model, read from `0x142cae760` (which is not a function entry
in the image - it is a three-way inlined body, so it was read from the
instruction stream):

```text
object[0x00] = a float4 scale quad, multiplied into every vertex
object[0x28] = the vertex array, float4 at a 0x10 stride
object[0x30] = the triangle array, 0x20 stride:
  +0x00  i32  vertex index 0
  +0x04  i32  vertex index 1
  +0x08  i32  vertex index 2
  +0x0c  i32  optional vertex 3, -1 when absent
  +0x10  i32  optional vertex 4, -1 when absent
  +0x14  i32  optional vertex 5, -1 when absent
  +0x18  u16  flag, copied to record +0x80
  +0x1a  u16  flag, copied to record +0x82
  +0x1c  u16  flag, copied to record +0x84
```

A triangle index is fetched as `vertices[(i32)tri[k]] * object[0x00]` - the
index is doubled and then scaled by 8, which is the `0x10` stride. The record
`0x142cae760` fills is at least `0x86` bytes, not the `0x50` an earlier note
claimed:

```text
record[0x00]  v0          record[0x30]  optional v3, or stale if tri[0x0c] == -1
record[0x10]  v1          record[0x40]  optional v4
record[0x20]  v2          record[0x50]  optional v5
record[0x60]  tri[0x00]   record[0x78]  present bit for v3
record[0x64]  tri[0x04]   record[0x79]  present bit for v4
record[0x68]  tri[0x08]   record[0x7a]  present bit for v5
record[0x6c]  tri[0x0c]   record[0x7c]  the triangle index itself
record[0x70]  tri[0x10]   record[0x80]  tri[0x18]
record[0x74]  tri[0x14]   record[0x82]  tri[0x1a]
                          record[0x84]  tri[0x1c]
```

When an optional index is `-1` the vertex slot is **not** written, so it keeps
whatever the caller's `-1` fill left there; only the present bit is cleared.
That matters for bit-exactness: the caller zeroes and `-1`-fills the record
first, so the absent slots read as `-1.0` floats. A third variant at
`0x142cae910` swizzles `object[0x10]` with `SHUFPS ..., 0xc9` (a 90-degree lane
rotation) before the same three vertices, so there is a second scale quad.

The refinement's `0x8000` sub-shape test reads a `u16` that an earlier note
placed at record `+0x2a`; no field is written there, so that offset is wrong and
the real one is one of `+0x80`/`+0x82`/`+0x84`. **Not confirmed.**

Per candidate id, in container order: a 0x50-byte record is zeroed and
`-1`-filled, then `0x142cae760(object, &record, id)` fills it. A `0x8000` bit in
the record's `+0x2a` `u16` means the id selects a sub-shape, which
`FUN_141b3c890(resolver, out, id & 0x7fff)` resolves; the resolved `u16` is what
the acceptance writes back. `resolver` is `[target+0x250]`, where `target` is
the same hashed state object the state transform decodes
(`WriteIndexConstants`, `DecodeKeyInPlace`, `[0x143b79598]+0x198`,
`[0x1438480f3]`).

What runs unconditionally is the refinement, under `record.flags & 0x18`:

1. `0x142cc5bc0(&record, &local)` gives the closest point on the triangle; its
squared distance is compared against `best` with an epsilon of `1.1920929e-05`
whose **sign flips with the previous acceptance** (`accepted ? best - e :
best + e`).
2. Inside that, the acceptance is cleared, and unless bit 0 of the resolved
`u16` is set the triangle normal is recomputed, normalised, and tested with
`threshold < dot(normal, basis)`.
3. On acceptance the normal is written to `query+0x10..0x1f` and the resolved
`u16` to `query+0x3e`, and the closest-point distance becomes the new `best`.

The visualiser is a separate block gated on the byte at `0x143c03a3d` and on
`(*(*owner + 0xb0))(owner) != 0`: it draws the `local +/- radius` segment
(colour `(1.0, 0.5)`), the three triangle edges through `0x142cb4a30` (the
point transform), one of two colour pairs `(1.0, 0.3)` chosen by `resolved & 1`
and `record.flags & 0x18`, and a `0.1`-long normal ray from the centroid. It is
**not** part of the result.

### The overlap test's port is unverified

`ow_sim::collide::box_triangle_overlap` is a lane-for-lane port of
`0x142cd58a0`, driven through `box_triangle_overlap` in
`crates/ow-sim/src/bin/verify_probe.rs`, with the fixture at
`dev/verify_harness/box_triangle_fixture.py` and the unit at
`dev/verify_harness/units/scaffolds/fn-BoxTriangleOverlap.json`. **It is not a
claim and it is not in the gate.** The port agrees with the client on 346 of
349 cases and diverges on three where the triangle only touches the box: a
triangle lying in the box's own face plane, a zero-size box sitting at a
triangle's interior point, and one special-value case. Two transcription
errors were found and fixed on the way - the third edge's two cross terms were
swapped - and a fourth, still unfound, disagreement remains in that same
family, so the sign or the operand order of one of the `|a|*0.5 - |b|*0.5`
terms is probably still wrong. Until all 349 agree, the read above describes
the client and the Rust beside it is an attempt.

Still open, and each is a single-function read:

| address | what it answers |
|---|---|
| `0x142cb4a30` | the point transform (and whether it is the 4x4 of `0x140cb5510`) |
| `0x142cba720`, `0x142cba2a0`, `0x142cba680` | the shape handle's type, handle and query object |
| `0x142cb49a0`, `0x142cb48a0` | the geometry slot and the query quaternion |

## Verification

The twenty-seven read movement units are differential, not fitted:

```text
python dev/verify_harness/cli.py fn/MoverRampVector
python dev/verify_harness/cli.py fn/MoverRampScalar
python dev/verify_harness/cli.py fn/MoverRampScalarOffset
python dev/verify_harness/cli.py fn/BuildMoverPhysicsCommand
python dev/verify_harness/cli.py fn/MoverWishSpring
python dev/verify_harness/cli.py fn/MoverWishForward
python dev/verify_harness/cli.py fn/MoverWishInverse
python dev/verify_harness/cli.py fn/MoverWishCopy
python dev/verify_harness/cli.py fn/StateTransform
python dev/verify_harness/cli.py fn/MoverStateVector
python dev/verify_harness/cli.py fn/DefaultMoverSettings
python dev/verify_harness/cli.py fn/ApplyMoverSettings
python dev/verify_harness/cli.py fn/ApplyStateModifiers
python dev/verify_harness/cli.py fn/MoverGate
python dev/verify_harness/cli.py fn/CharacterPosition
python dev/verify_harness/cli.py fn/RigidPosition
python dev/verify_harness/cli.py fn/VariantPosition
python dev/verify_harness/cli.py fn/WorldPosition
python dev/verify_harness/cli.py fn/MoveCharacterController
python dev/verify_harness/cli.py fn/ApplyCharacterControllerMove
python dev/verify_harness/cli.py fn/SetCharacterControllerVelocity
python dev/verify_harness/cli.py fn/ShapeSphere
python dev/verify_harness/cli.py fn/ShapeCapsule
python dev/verify_harness/cli.py fn/ShapeBox
python dev/verify_harness/cli.py fn/ShapePoints
python dev/verify_harness/cli.py fn/ShapeDispatch
python dev/verify_harness/cli.py fn/MoveCharacterControllerShape
cargo test --release -p ow-sim
```

`fn/MoverRampVector`, `fn/MoverWishForward`, `fn/MoverWishInverse` and
`fn/MoverWishCopy` run 264 cases with 100% branch coverage and kill or declare
every mutant. `fn/MoverWishSpring` runs 264 cases, 90% branch coverage, 12/12
mutants killed; its one missed edge is named in the unit. `fn/StateTransform`
runs 264 cases, 11/12 mutants (92%), with the non-returning mismatch path and
the sentinel-skipped gate edges declared. The two scalar units and the command
builder pass the same five stages. These units prove the read leaves, not
per-frame movement: the `FUN_140cb86c0` fan writer, the Domino velocity
dispatch and collision are still open, so the client remains a viewer. A
golden per-step timeline is required before any movement claim; until then the
verified arithmetic is not a playable step.

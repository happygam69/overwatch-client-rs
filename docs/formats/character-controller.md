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
`0x14203dae0..0x14203dbd9` plus the body `0x14203de51..0x14203e0fe` — and the
scattered blocks (`0x14203f5bb`, `0x1420404ab`, `0x142046b12`, …
`0x1420543f2`) are junk that real data does not reach.

**The guard is pointer-keyed, not data-keyed.** The body starts by reading the
caller's home slot for `RCX` (`[rsp+0x20]` in its frame, i.e. the low byte of
`param_1`), adding `0x84` and `0x7c` through two one-instruction stubs
(`0x14053de90` = `add byte [rcx], 0x84; ret`, `0x14053be10` = `add byte
[rcx], 0x7c; ret`), then decoding that byte through the nibble table at
`0x1433d158`, XORing with the build byte `0x143847d3c` and `[Q+0xdd0]`, and
comparing to `0x40`. So the opaque predicate selects a variant from the
**pointer value**, which is why a straight decompile is unusable: the two
variants are different encodings, and an emulator can be made to take either
by choosing the world pointer's low byte. Both variants must agree — the
predicate is obfuscation, not behaviour — but that is a claim to test, not to
assume.

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
   hit (the hit quad is folded into `minps` over the block's motion; when all
   four lanes are below the current motion it becomes the new motion and the
   swept end moves to `start + hit*delta`), anything else continues.
5. otherwise `entry+0x2c` and `entry+0x30` are pushed as child indices.

So `0x142cbd510` is a bounding-volume hierarchy walk over one island's shape
list: the four passes sweep the character's local support points, the per-shape
AABB and moving-box tests prune, and only leaves reach a narrowphase.

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
writes two quads (0x20 bytes) and is `0x150` bytes:

* `entry[0xb0] != 1` (the path every static shape takes):
  `out[0x00] = pool[idx][+0x10]`, `out[0x10] = pool[idx][+0x00]`, with
  `pool = [record+0x2a0]` and `idx = entry[0x24]`, stride `0x50`.
* `entry[0xb0] == 1`: the pose path. `idx = entry[0x50]`, `pose =
  [record+0x288] + idx*0x1c0`. When `record[0x191] == 0` or
  `record[0x192] != 0` it copies `pose[+0x10]` and `pose[+0x20]`; otherwise it
  builds a normalised quaternion from `pose[+0x60]`..`pose[+0x90]`
  (`(2q*q)` magnitude fold, `sqrtss`+`divss`, then the cross-product terms).
  The two modes are selected per record, not per shape.

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

**Still open:** which of the four call sites runs under which input (the
`uVar21` / `cVar6` preconditions differ per site, and sites two and four are
unread), the `B` endpoint's scale quad, and the `+0x1140` write (the block is
only ever *read* at `0x140cb86c0`'s head in the decompiled body, so the write
lives in a callee or the caller). None of that is guessed in `ow-sim`:
`fan::solve` does not exist, and `CandidateSolver` is documented as the seam
rather than the loop.

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
6. **Pass 3, the back-transform.** Five candidate quads are re-evaluated:
   `s = -dot(cand, dir)`, `A = s*dir + cand + seed*dir`, `B = s*dir + cand -
   speed*dir`; on a facing pass with `|proj| < best` **and** `last_dot < 0`
   the output direction becomes `s*dir + cand` (that is `back_direction`),
   the normal becomes the record's quad, and `out+0x41 |= (key>>8)&1` while
   `out+0x42 = (key>>10)&1` (OR for the first, assignment for the second).
7. **The write.** `out+0x00..0x1f` = direction then normal quads;
   `out+0x30` = the record's tag word; `out+0x34` = the resolved id;
   `out+0x38` = the record's `local_c40`; `out+0x40` = 1. The no-hit path
   zeroes `0..0x1f`, sets `out+0x30 = _DAT_1441880e8`, `out+0x34 = 0`,
   `out+0x40 = 0`, `out+0x38 = 0xffff0000ffffffff`, and the word output to 0.
   A tagged hit (`0x141c5d780(obj, 0x55 / 0x23 / 0x2e / 0x22 / 0x68)`) takes
   the variant path and, when it resolves, runs `MoverWishTransform`
   `0x140c9e740` on the direction before writing it.

**What is still unread, and it is exactly five helpers.** `solve` needs
`FUN_1426cd3d0` (the facing threshold `local_c18`), `MoverWishQueryCached`
(pass 0), `FUN_140c9f110` (the extrapolation vector), `func_141af0070` +
`FUN_141ae3eb0` (the shape basis for pass 1'), and `0x140cc2290` (the `kind ==
3` refine). None is large, all five are named here with their call sites, and
until they are read `fan::solve` stays unwritten — the stages above are the
table it will be written from.

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
**open** and unlocated (see `docs/open-questions.md` item 6).

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

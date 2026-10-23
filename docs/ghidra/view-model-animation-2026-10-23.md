# View-model animation: subagent findings (2026-10-23)

Reference record of two read-only Ghidra subagent reports, kept separate
from the curated state in `docs/formats/animation.md` and
`docs/fitted-ledger.md`. They answer: does `--play`'s first-person
view-model animation match the client?

**Program:** `module_v21_clean.bin`, build `2.25.0.1`, Ghidra base
`0x7ff7c4d20000` (v2b), after the 2026-10-23 page fill (488 `.text`, 372
`.rdata`, 16+13 `.data`, 4 `.pdata` pages). **Evidence class:** read from the
image; no runtime trace was taken. **Not tested:** the rendered result was
not eye-checked.

## Provenance and address caveats

- The image's PE base is `0x7ff736e60000`; the Ghidra program is rebased to
  v2b. Absolute pointers in `.rdata` belong to the capture's session; the
  filled pages' pointers were rebased to the image PE base, not to v2b.
- `.pdata` is encoded in this build; it was **not** used to find function
  starts.
- The reports use both current-build function addresses and the older v2b
  doc addresses. `docs/function-names.csv` carries the program/doc mapping;
  the pair is repeated below where the subagent supplied it.
- These are raw findings. The curated/confirmed statements are in
  `docs/formats/animation.md` and `docs/fitted-ledger.md`.

## Report A: first-person animation runtime

### Functions

| Address | Role | Basis |
|---|---|---|
| `0x7ff7c5a4f060` | `FpViewmodelConsumer`: resolves type-0x35 FP component -> view-model entity/model; calls the bind | code |
| `0x7ff7c5a4faa0` | `FpViewmodelBind`: stores two STU refs, one handle, resets view-model state; does not read `m_position` or the scalars | code |
| `0x7ff7c5a51bf0` | per-frame slot `+0xe0`: loops six `0x110` slots and runs the event player | code |
| `0x7ff7c5a522f0` | per-frame slot `+0xe8`: loops the same slots, second event/state player | code |
| `0x7ff7c5a49c60` | `FpViewmodelEventPlayer`: advances event timers by dt, fires masks at 0.25/0.33/0.5 of duration, sorts | code |
| `0x7ff7c5a4a250` | `AnimEventMaskDispatch` | code |
| `0x7ff7c5a47410` | `FpViewmodelEventSort` | code |
| `0x7ff7c5a496a0` | picks the newest active `0x50`-byte event record | code |
| `0x7ff7c5a5a020` | `FpViewmodelEventPlayerB`: time/id -> stepped phase, writes nine dwords at `+0x8b8+i*0x24` | code |
| `0x7ff7c6e1eca0` (v2b doc `0x7ff7c6e84610`) | `AnimSamplerRule` / per-child pose fold | code |
| `0x7ff7c6e19930` | section accessor `stream + (id+3)*0x10`; id 7 = sampler list | code |
| `0x7ff7c603efb0` | lane-presence test against `child+0x10` | code |
| `0x7ff7c58b1ef0` (v2b doc `0x7ff7c58e0ed0`) | time wrap/clamp | code |
| `0x7ff7c58b0c00` (v2b doc `0x7ff7c58dfbe0`) | key search / per-key time lookup | code |
| `0x7ff7c58b09d0` (v2b doc `0x7ff7c58df9b0`) | `CurveKeySampleDual` | code |
| `0x7ff7c6e205d0` (v2b doc `0x7ff7c6e85f40`) | section-1 builder: child[0] copy + weighted fold | code |
| `0x7ff7c6b4d5c0` | `ClipRuntimeConstruct`: clip runtime record + timing descriptor | code |
| `0x7ff7c6a0a0b0` | curve/value evaluator (kind dispatch, noise, wrap shapes) | code |
| `0x7ff7c6842c6e` / `0x7ff7c6842d5e` | registration reads of `anim.EnableProxyRetarget` / `anim.EnableFinalRetarget` | code |

### Q1: the six slots and the per-frame updates

The six `0x110`-stride slots at `viewmodel+0x60` are **animation-event
timelines**, not animation players. Each begins with a `{ptr@+0, count@+8}`
list of `0x50`-byte event records; `+0x6c0` is a seventh/global dispatch
list.

- `FpViewmodelUpdateA` passes `dt` to the event player, which advances
  `event.time += speed*dt`, wraps it, fires `0x100/0x200/0x400` masks at
  0.25/0.33/0.5 of `event.duration`, and sorts. It writes only event state
  and `slot+0x100`.
- `FpViewmodelUpdateB` reads the newest record, resolves two ids, and writes
  a nine-dword state block at `viewmodel+0x8b8+i*0x24`.
- **Neither calls `AnimSamplerRule`, a clip/key/curve function, or writes a
  matrix/transform.** Callee sets are `{0x7ff7c5a4b9c0, 0x7ff7c5a49c60}` and
  `{0x7ff7c5a496a0, 0x7ff7c5a5a020}`.
- The view-model object stores its animation source at `+0x880` and pose sink
  at `+0x888` through vtable `+0xd8`; the pose is produced elsewhere.

### Q2: item, time, wrap, rate

- Item 3 is the idle group (c71, c50, d4, bc, 680); category 5 -> c71,
  category 9 -> c50. The authored LayerChild weight is 0.0, so at file
  defaults the tree output is c71.
- `0x7ff7c58b1ef0`:
  - if `byte[0xc] & 1`, `t = t - trunc(t / f32[1]) * f32[1]` (sign-preserving
    `fmod` by the object's period);
  - else `t = clamp(t, f32[0], f32[1])` (hold at the end).
  The wrap descriptor is per clip/node (own period, min, loop/clamp bit).
- The clip-time increment/entry point was **not** pinned. The event tracks
  scale dt by `record[+0x14]`; `ClipRuntimeConstruct` takes a rate/scale from
  the source descriptor. The idle rate is unread.
- Whether the d4/bc Pose2d layers apply at runtime is **not proven**: the
  gate only applies a layer above 0.5, the authored LayerChild weight is 0.0,
  and the per-tick writer is the unread dataflow VM.

### Q3: exact sampler semantics

List is section 7, entries `{child@+8, weight@+0x10}` stride `0x18`.

1. `count == 0`: no change.
2. **Child 0 is copied wholesale and unconditionally** (channels, vec4
   rotation/position/scale, optional `+0x50/+0x60`, scalars `+0x70..+0x7c`).
   Child 0's weight is ignored.
3. For each child i >= 1, per lane present in that child's channel list:
   - vec4 rotation/translation/scale (and optional lanes): `w > 0.5` takes
     the new lane, otherwise keeps the old. **Winner-take-all, no lerp.**
   - four scalars: `v = (new - old) * w + old` (lerp at any weight).
   - an absent lane keeps the previous value.
4. Weights are not normalized; the fold is order-dependent; the replacement
   is strict `>` (exactly 0.5 keeps the old).
5. The construction-time section-1 fold uses the same rule
   (`0x7ff7c6e205d0`: child 0 via `0x7ff7c6e201a0`, i>=1 via
   `0x7ff7c6e202e0`).

The `0.5` gate constant is in the table at `0x7ff7c7ca0ff8` (`{0.5, 2, 3,
5}` in this build; old doc address `0x7ff7c7d0c068`).

### Q4: clip P lane, reference skeleton, 2.395x

- The pose slot has a translation lane (`+0x30`) and the sampler applies it.
- The client has a **reference-skeleton retarget stage**: the two config
  registrations above are described as "proxy pose re-target between
  (inherited) blend trees with different reference skeletons" and "final pose
  re-target between reference skeleton and model skeleton".
- Therefore the card P lane is the **reference-skeleton local**, not the FP
  model local; the final stage maps reference -> model.
- The shoulder rows at `2.395x` the FP model bind are consistent with the
  reference rig's local. A correct final retarget at rest leaves the model's
  own bind there; direct application stretches the arms.
- **Not found:** the retarget implementation body, and whether the two config
  flags default on. Do not invent a rest matrix.

### Q5: proven mismatches against the viewer

- **Per-channel vs whole-bone:** client replaces each of R/T/S independently
  when that lane is present and `w > 0.5`; viewer replaces the whole
  `(rotation, translation)` pair for a bone (`anim_tree.rs`), so a
  rotation-only layer also overwrites translation.
- **Absent lanes:** client keeps the previous value (child 0 / earlier
  layer); viewer falls back to the clip's character bind, or to bind.
- **Scalars:** client lerps four scalars at `+0x70..+0x7c`; viewer has no
  equivalent.
- **Gate input:** client compares the sampler entry's own f32 weight; viewer
  computes `base * gain * (wx*|x| + wy*|y|)`.
- **Pose2d:** client has per-axis drivers and a spatial blend; viewer
  collapses it to a scalar product and does not model the spatial blend.
- **Clamp mode:** client can clamp a non-looping clip at `[min, period]`;
  viewer always wraps.
- **Reference retarget:** omitted by the viewer.
- **Node coverage:** client has LayerAdditive / plain Animation / generic
  folds; viewer models only BranchByCategory + LayerChild/Pose2d.
- **Playback rate:** client has per-record speed and a clip rate field;
  viewer advances by `dt` only.
- **Match:** the 0.5 constant, strict `>` replacement, item 3/category 5 ->
  c71, child-0 full strength, and per-frame dt event advancement.

### Report A unresolved

1. Per-tick clip-time increment/entry point.
2. Root dataflow VM entry that writes item-3 slots (branch category, x/y,
   LayerChild weight).
3. Whether d4/bc apply in the idle.
4. Reference-retarget formula and default state of the two config flags.
5. Semantic identity of the six `0x110` slots (which layer/part each drives).
6. The concrete pose producer/sink implementation behind `+0x880/+0x888`.

## Report B: view-model transform and attachments

### Functions

| Function | Address | Role |
|---|---|---|
| `UpdateFirstPersonCameraPose` | `0x7ff7c56b7b90` | Camera descriptor: position `anim+0x1350`, forward `+0x1310`, up `+0x1320`; FOV from 103° horizontal; calls `ValidateAndNormalizeCameraDescriptor` |
| `ApplyFirstPersonCamera` | `0x7ff7c568c5b0` | Selects FP camera, FOV/near/far, applies pose |
| `ConstructFirstPersonCamera` | `0x7ff7c56b7640` | vtable `0x7ff7c7d79318`; anim handle `+0x260`; `+0x284 = 70.0` |
| `BuildFirstPersonCameraVariant` | `0x7ff7c56b7c90` | vtable `0x7ff7c7d79448` |
| `EvaluateFirstPersonCameraAnimation` | `0x7ff7c568cb30` | Evaluates the FP animation; writes screen-space dx/dy/scale; applies pose FOV |
| `FUN_7ff7c5703dd6` | `0x7ff7c5703dd6` | Writes anim `+0x1300` (orientation quat) and `+0x1350` (position) |
| `FUN_7ff7c54a9720` | `0x7ff7c54a9720` | Builds `+0x1310` forward and `+0x1320` up rows from the quaternion |
| `FUN_7ff7c58e6240` / `0x7ff7c58e6390` | | Set basis rows from a pose delta/blend |
| `ApplyFirstPersonOffsetTransform` | `0x7ff7c539ddd5` | Applies `firstPerson.Offset*` to the pose `+0x1380`; described "offset the first person model" |
| `FUN_7ff7c539c0d0` | `0x7ff7c539c0d0` | Applies view-model bone/spring state (`+0x8b0`, hardpoint `0x7d`) and offset |
| `FUN_7ff7c53ab620` | `0x7ff7c53ab620` | Per-frame drag spring on `+0x1350` toward `+0x1370`; coefficients `0.235`/`0.48`, k ramp `600.0/s`, settle threshold `0.004 m` |
| `FpViewmodelConsumer` / Bind | `0x7ff7c5a4f060` / `0x7ff7c5a4faa0` | as Report A |
| `ConstructFirstPersonComponent` | `0x7ff7c602d0b0` | vtable `0x7ff7c7ef5c38` |
| `FUN_7ff7c6010b50` | `0x7ff7c6010b50` | Type-0x35 field schema registration |
| `FirstPersonComponentFieldDefault` | `0x7ff7c6032f30` | Authoring defaults: `0x816B830F=1.15`, `0xCF5055F1=1.0`, `0xE29993A5=1.55` |

### Placement chain

Read facts:

- FP camera pose = the animation node's pose:
  - position `anim+0x1350` -> camera descriptor `+0x10`
  - forward `anim+0x1310` -> camera `+0x20`
  - up `anim+0x1320` -> camera `+0x30`
  - source quaternion `anim+0x1300`
- The FP model is feet-origin (bone 44 bind `(0.440, 0.928, -0.016)`;
  shoulders ~1.306; neck ~1.7104). It is not an eye-origin rig.
- The view-model entity/model is a separate entity (`STUFirstPersonComponent.m_entity`,
  `0x0400000000000010` on hero 3).

Consequent chain (the model-entity transform setter is unresolved):

1. clip local pose -> FP skeleton (79 bones; 64 shared ids copy locals;
   44/74 use the folded character chain `284*44` / `285*74`);
2. skin -> `bone_world_fp * inverse_bind_fp` in FP model space;
3. model entity -> separate view-model entity in the player/feet frame;
   the 1.75 m the viewer subtracts is the model's eye/neck offset, not a
   camera offset;
4. camera -> the FP animation pose (`+0x1350`, `+0x1310`, `+0x1320`);
5. view-model offset -> `firstPerson.Offset*` config (defaults 0.0) to pose
   `+0x1380`; the descriptions say model offset, not world camera.

The client frame is X-right, Y-up, Z-forward. The camera rows are
`[A x B; B; A; e]` with `A=+Z`, `B=+Y`; `A x B = -X`, and the projection's
negative x scale cancels it. A raw copy of client-local arm/weapon data into
Bevy's RH frame can mirror/point the guns.

### `m_position`, the four scalars, type-0x35

- The type-0x35 consumer `FpViewmodelConsumer` only:
  - reads the component,
  - type-checks against `0x7ff7c8b70fa0`,
  - reads the view-model **entity ref pair at `+0x40/+0x48`**,
  - reads a ref at `+0x38` with type `0x7ff7c8b67398`, then `[ref+0x28]`,
  - calls the bind.
- It does **not** read `m_position`, the 64/16/-1 spring, or the four
  scalars. No other readable consumer applies them.
- Field schema:

| Field | Hash | Bag offset |
|---|---|---|
| `m_position` | `0xA02DD607` | `+0x10` (16-byte vec) |
| `m_entity` | `0xA83C2C26` | `+0x40` (four entity refs) |
| spring | `0xF486DCF2` | `+0x50` (64/16/-1) |
| scalar | `0x4B75D84E` | `+0x5C` (50) |
| scalar | `0xE29993A5` | `+0x60` (1.25) |
| scalar | `0x816B830F` | `+0x64` (0.875) |
| scalar | `0xCF5055F1` | `+0x68` (0.8) |

- `m_position` (0.1375, 0.23375, 0.20625) is therefore **not applied** in
  the captured code. It is not a source-frame/sign question: there is no
  reader. The same applies to the four scalars; `50` is not the FP FOV.

### Camera and viewer

- Client FP camera: animated pose; FOV **70.53°** vertical at 16:9; near/far
  **0.01/1638.4 m**; roll/scale field `+0x40=0`, `+0x44` clamp `0.05..3.0`.
- Viewer: world eye + fixed yaw/pitch + fitted `--fp-cam-drop 0.35` /
  `--fp-cam-back 0.30`. Those are not client constants; the exact camera
  pose must come from the FP animation (`+0x1350` relative to the player).
- The real drag spring is on `+0x1350` toward `+0x1370`
  (`0.235/0.48`, k ramp `600.0/s`, settle threshold `0.004 m`), not the
  component's 64/16/-1. Report A's earlier `0.008`/`0.00012` reading was
  corrected by Report B.

### Weapons

- Pistols attach to FP model hardpoints `0x0dc:0a/0x0b`, which hang off FP
  skeleton weapon bones **44/74**, not hand bones 13/14/54/55 and not the
  centre bone 126.
- FP bind 44/74 is world-identical to character 44/74, but parents differ:
  character `44 <- 284 <- 28`, FP `44 <- 28`; character `74 <- 285 <- 58`,
  FP `74 <- 58`. The identity `bind_fp(44)=bind_char(284)*bind_char(44)`
  (and 285*74) holds exactly.
- Hardpoint local is applied in the FP weapon-bone frame; the weapon is a
  rigid model in its own bind. World = FP bone world * hardpoint local.
- The client's FP-skeleton weapon source is now the viewer default; the old
  character-c71 path remains behind `--weapon-fold 1` for A/B. The omitted
  reference retarget still makes the animated local approximate.

### `camera.FirstPersonAnimationWeight`

- `0x7ff7c856b808` (text `0x7ff7c856b814`), description "how much animation
  contributes to first person camera". Siblings:
  `camera.FirstPersonAnimationRollWeight` (`0x7ff7c856b894`) and
  `camera.ShowFirstPersonAnimationOffset` (`0x7ff7c856b914`).
- Camera-only blend weight, not an arm/weapon weight; no per-bone
  world-vs-FP distribution was found. The static object candidate
  `0x7ff7c856b734` (value 1.0) has no direct code reference; exact
  application is unresolved.

### Report B unresolved

1. Exact view-model entity transform setter/parent (player/feet frame).
2. Application of `m_position`, the 64/16/-1 spring and the four scalars
   (no reader in the image).
3. Numeric FP camera height in the c71 pose (component 1.25 is a candidate,
   not a read consumer).
4. `camera.FirstPersonAnimationWeight` blend formula / registry read.
5. Exact target of `+0x1380` (generic transform; model offset, not world
   camera).

## Cross-report state

- Both reports agree: the six view-model slots are event/state timelines, not
  pose; the FP weapon source is FP bones 44/74, not the character c71 rig
  (now the viewer default); the 64/16/-1 component spring has no reader.
- Report A says the reference-retarget body is unread; Report B says the
  view-model entity transform setter is unread. Both are **not** implemented.
- Report B's camera placement is consistent with `UpdateFirstPersonCameraPose`
  in `docs/formats/camera.md`; the viewer's drop/back remains fitted.
- The exact sampler channel rule is **read**; the viewer's whole-bone
  replacement is the first safely fixable mismatch.
- **Post-report fixes (same day):** one-link layer bones now apply rotation
  and translation independently (`anim_tree::sample_lanes`), and the default
  weapon source is the FP skeleton's 44/74 (`--weapon-fold 1` keeps the
  character-c71 A/B). The reference retarget, FP camera/spring target,
  Pose2d dataflow VM, loop bit, scalar/scale lanes and event effects remain
  unread and are not faked. The ledger marks the omitted retarget
  (`FIT-ANI-08`), the weapon source (`FIT-ANI-04`), the sampler/driver
  (`FIT-ANI-05`) and the spring (`FIT-CAM-01`).

## Where this is reflected

- `docs/formats/animation.md` - the 2026-10-23 exactness table and open
  problems.
- `docs/fitted-ledger.md` - FIT-ANI-04, FIT-ANI-05, FIT-ANI-08, FIT-CAM-01.
- `docs/open-questions.md` - "View-model animation exactness (2026-10-23)".
- `MODLOG.md` - the check entry.

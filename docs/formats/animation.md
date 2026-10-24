# Animation

Skeleton, clip format, runtime pose path, first-person blend tree and the
view-model camera. Evidence classes are stated per section: `install`,
`decompiled`, `live capture`, `published-check`, `fitted`, `tier 3`.

## Current state

- **File format: solved.** Type-`0x006` clips decode: family-tagged header,
  32-byte metadata rows with six lands, packed-u16 rotations, position/scale
  tracks. The family-agnostic row reader covers every readable set family; the
  card path covers `0x0053xxxx`.
- **Runtime consumer: read.** Rows are sampled per tick; there is no load-time
  expansion. Pose slots carry rotation and translation lanes; the feed applies
  the local pose.
- **FP blend tree: implemented.** Item 3 (idle) selects c71/c50/d4/bc/680;
  category 5 -> c71, category 9 -> c50. `--anim auto` evaluates it.
- **FP retarget: measured, one stage open.** Character 200 bones, FP 79, 64
  shared ids with identical binds; weapon hooks fold dropped ancestors exactly
  (`build_chains`). The clip's shoulder rows carry the source rig's 2.395x arm
  length and the reference-skeleton/rest data is still not found, so that
  conversion is the open stage.
- **FP camera: data read, consumer partly found.** FP near/far, the per-hero
  `STUFirstPersonComponent` values and the live component layout are read; the
  type-0x35 consumer (`FpViewmodelConsumer`, RVA `0xd2f060`) and its per-frame
  slots are identified, but the per-frame offset/scalar application is not. The
  drop/back framing stays fitted.
- **Play-slot names: tier 3.** The slot descriptors are hashes with no readable
  member names; state assignment is shape-based.
- **Mechanically verified functions (2026-10-24).** `dev/verify_harness/`
  differential-tests a reconstruction against the emulated real bytes
  (`D:/ow-dumps/offline_recover/module_offline.bin`). `AnimTimeWrap` (`0x7ff7c58b1ef0`,
  wrap-or-clamp a time into a range) and `CurveKeySampleDual`
  (`0x7ff7c58b09d0`, dual-track key interpolation) are verified on every
  channel at 100% branch coverage and 100% mutation kill. The harness found
  two Ghidra-output bug classes in the decompiles, both now corrected from the
  bytes: a float-to-uint conversion where the code does a raw bit test /
  dword load, and IEEE-ordered `if` chains where the code is SSE
  `minss`/`maxss` (NaN yields the second source). The C decompiles live in
  `dev/decompiles/` (untracked).

## Skeleton and rigs

### Component chain

Read from the install (hero 2/Reaper example):

- hero asset -> gameplay entity;
- `STUModelComponent` (`0x191835F9`), field `0x2D124BE2` -> character model
  (`0x00C`) and its skeleton;
- `STUFirstPersonComponent` (`0x9D28963F`), field `0xA83C2C26` -> a separate
  view-model entity, whose own `STUModelComponent` names the FP model through
  the same `0x2D124BE2` field.

The view model is a parallel entity with its own model, not a flag on the
character model. `STUModelComponent` also carries
`m_animBlendTreeSet` (`0x523339C1`), the join to the FP animation set.

### `mskl` skeleton

`mskl` is the model skeleton. Header: thirteen `i64` section offsets
(hierarchy, matrices, inverse matrices, bind pose, deltas, ids, names, remap,
a second hierarchy), an `i32`, five `u16` counts (`bones_abs`, `bones_simple`,
`bones_cloth`, `remap_count`, `id_count`).

- hierarchy: 6 bytes per bone, `i16` parent, `-1` at the root;
- bind pose: 48-byte records (quaternion, scale, pad, translation, pad);
- bone ids: `u32` hashes, used as the engine's bone vocabulary;
- remap: maps the model's bones to the clip's bone set.

Measured examples: Reaper character 229 bones / 71 FP; Tracer 200 / 79. The
FP rig's neck sits at the same `y = 1.7104 m` as the character's, and its weapon
is 0.40 m forward.

The `MRNM` handler (`FUN_7ff7c68f1de0`, v2b) also builds runtime parent-indexed
tables from the model payload: header counts, `0xA0`-stride nodes, `0xF8`-stride
hierarchy, `0x18`-stride auxiliary table, and a used-bone mask. `mskl` and
`MRNM` are different containers feeding the same hierarchy; do not treat the
`mskl` key/value block as the whole story.

## Clip container

Type `0x006`, header common to all families:

| offset | type | meaning |
|---|---|---|
| `+0x00` | u64 | family word; low 32 bits select the reader |
| `+0x0c` | u16 | frame count |
| `+0x0e` | u16 | fps |
| `+0x14` | f32 | duration = frames / fps |
| `+0x18` | f32 | 1 / duration |
| `+0x1c` | f32 | fps as float |
| `+0x20` | f32 | 1 / fps |
| `+0x24` | u32 | clip id |
| `+0x28` | u32 | slot/index within the family |
| `+0x30` | 10 x u64 | section offsets; `-1`/`0` = absent |

Section 1 begins at `0x80` on the measured families. It carries one `u32` per
record: the record's bone id, plus one extra word (a size annotation) in every
family measured.

Families seen: `0x00533C00`, `0x00532400` (FP blend cards), `0x006D2000`
(object/prop idle), `0x00020400` (two-table FP camera bob), plus walk/trio/zone
families (`0x00130404`, `0x00110404`, `0x000A0404`, `0x001F0404`,
`0x00140404`, `0x004C2400`, `0x004C2C00`, and the item-5/item-14 families).
Dispatch is on the family word; the general row reader is family-agnostic once
the row envelope is found.

### Runtime record table

The decompiled consumer sees 20-byte runtime records:

| offset | content |
|---|---|
| `+0x00` | u16 value (0, 1, or 631) |
| `+0x02` | u16 node index |
| `+0x04` | u16 value |
| `+0x06` | u16 node index |
| `+0x08` | u16 value |
| `+0x0a` | u16 node index |
| `+0x0c` | flags |
| `+0x0e` | evaluator kind |

The u16s at `+0x02/+0x06/+0x0a` are node indices into the runtime graph, not
file offsets. The node table is built by the loader. This table is distinct
from the 32-byte file metadata row below.

### File row and track layout

A file row is 32 bytes:

```
{u16 scale_count, u16 position_count, u16 rotation_count, u16 flags,
 u32 scale_key_land, position_key_land, rotation_key_land,
 u32 scale_data_land, position_data_land, rotation_data_land}
```

- All six lands are cursor words. The data lands are relative to the row's own
  address: `rec_at + land * 4`. This is confirmed across every readable family;
  base-relative decoding is a strict subset and was a probe bug.
- Keys are frame numbers: `frame = abs(index) % InfoTableSize`, where
  `InfoTableSize = int(fps * duration) + 1`.
- Index width: `u8` while `InfoTableSize <= 255`, otherwise `i16`.
- Values are per-frame absolute samples, then interpolated; the runtime sampler
  reads them per tick.

Rotation tracks:

- old form: three `u16` per keyed frame = 6 bytes; top bits carry axis order,
  low 15 bits scale by `sqrt(2)`, `w` derived, components reordered by axis;
- new form (`flags & 0x400`): 8 bytes with bit-packed mantissa fragments;
- every verified track decodes as a unit quaternion.

Position and scale:

- position: legacy `3 x f32` (12 bytes) when `flags & 0x200`, otherwise
  `3 x f16 / 32`;
- scale: `u16 / 1024`;
- new-scale compression (`flags & 0x100`) is unimplemented and no sample
  carries it.

The record list is `[u8 bone ids, 0-terminated ascending][pad][key list,
0-terminated][gap][values]`. The gap is per record because S/P/R have different
lengths; the decoder searches for the value offset and requires every keyed
frame to decode.

### Coverage

- `decode_rotation_tracks`: 309/350 parsed clips, 3,907 tracks, 38,527 keyed
  frames.
- `decode_general_records`: all 36 readable hero-3 set clips, 0 failures.
- Card path (`0x0053xxxx`): 83 records, 249 tracks (`83 x S/P/R`), 0 unit
  failures on c50/c71; c50's position span is `(0.2200, 1.4898, 0.4000)`,
  matching measured bone placements.
- Set-wide pairing: cards 71/83 records to FP skin ids; walk 19/15/15,
  trio 10/10/5, zone 31/27/17 and 20/18/7, item 5 45-48 fully paired,
  slot items 76/64/76 (75%).

## Runtime pose path

The runtime leaf sampler (`FUN_7ff7c6e84610`, v2b) reads 0x18-stride
`{child, weight}` entries and the three vec4 lanes at child `+0x20/+0x30/+0x40`
(rotation/position/scale). The gate is the f32 array at v2b
`0x7ff7c7d0c068` = `{0.5, 2, 3, 5}`: `weight > 0.5` takes the source,
otherwise `(new - old) * weight + old`. Absent fields keep the previous slot
value.

- The section accessor is `FUN_7ff7c6e7f1e0(stream, id) = stream + (id+3)*0x10`.
- The section-1 builder `FUN_7ff7c6e85F40` walks 0x18-stride entries, probes
  six child class descriptors at v2b `0x7ff7c8da1848..1f28`, and blends with
  the same gate.
- No load-time row expansion exists; every consumer is a per-tick sampler.

The pose slot has a **translation lane**. The rotation helpers are quaternion-
only, but the update (`FUN_7ff614ef08c0`, clipobj wrap) writes the per-bone
rotation and position lanes against the skeleton bind. The card P lane is the
file's local translation and the feed applies it. The earlier "hold translations
at bind" rule is retired.

Skinning: a pose replaces the bind's local matrix
(`local' = T(pose_t) * R(pose_r) * S(local_s)`); the absolute-rotation path
remains for the rotation-only case. This is the measured local path, not a
parent-relative delta.

## Kind dispatch and evaluator

The kind byte at record `+0x0e` has 31 entries. Only kinds 2, 3 and 10 have real
handlers; 6, 9, 11-14, 16, 17 and 20-30 land on a default returning 0. Kind 10
is double-indirect through sub-op tables at `0xBCC000` (cold in the captures).

Kind 2 (`AnimKind2_Eval`, v2b `0x7ff7c58ea110`) resolves the node, branches on
its kind byte, then runs:

1. time wrap/clamp (`0x7ff7c58e0ed0`);
2. keyframe search + lerp (`0x7ff7c58dfbe0`);
3. dual sample (`0x7ff7c58df9b0`);
4. noise from a static 1024 x vec4 table at v2b `0x7ff7c8f86320`;
5. flags at node `+0xc`: `0x8000` scalar, `0x4000` blend, `0x2000` replace,
   `0x1000` vec4;
6. wrap shaping at node `+0x24`: 0 none, 2 mirror, 3 ping-pong, 4 square,
   5 inverse.

Cold-kind handlers still need a capture with the pages resident.

## FP blend tree

Join: `STUModelComponent.m_animBlendTreeSet` (`0x523339C1`) -> Tracer's set
`0x0040000000000004` (type `0x021`, 10,517 B, 47 instances).

- The true items are `STUAnimBlendTreeSet_BlendTreeItem` (`0xDDD18945`); the
  `0x3B150012` rows are its embedded payloads.
- Item 3 is the idle group: clips `c71, c50, d4, bc, 680`; 19 floats, vec2
  `(-0.4, 0.4)`, three u8 flags.
- Its graph is `0x0F8000000000000F` (`STUAnimBlendTree`, 28 instances), bound
  to item 3 by CRC. Root -> branch-by-category -> layer children; Pose2d nodes
  3/8 drive d4/bc; animation nodes 19/23/10 drive c71/c50/680.
- Category 5 -> c71 (base idle); category 9 -> c50 (rest). The layer weight's
  file default is 0.0, so the tree output at defaults is c71; its runtime
  dataflow writer is unread.
- `m_63DEDAC0` is a 1-based slot id: on an animation wrapper it indexes the
  item's clip list; on a parameter object it indexes the item's float array.
  The item-3 slot table (19 slots) is recorded in the probe output and matches
  the tree defaults (`d4` 0.75+0.75, `bc` 0.15+0.10, etc.).
- The 0.5 sampler gate applies after the tree: `weight > 0.5` replaces lanes,
  otherwise lerps.

Implemented in `ow-tank::blendtree` and `ow-view::anim_tree`; `--anim auto`
selects it and falls back to a single clip for heroes without a set. Flags:
`--anim-item` (default 3), `--anim-category` (default 5), `--tree-x`,
`--tree-y`, `--tree-gain`, `--tree-layer`.

At the defaults the tree output equals c71. Forcing `--tree-layer 1` with x/y
inputs takes the arms off-screen: d4/bc are pose layers, not idle content. This
confirms the remaining FP pose difference is not the tree.

The Pose2d dataflow sources (`0x048...07/08/18/96/8be`) do not resolve as STU
assets; the per-tick VM that produces x/y and the branch category is unread.
That is the tree's only missing runtime piece.

## FP retarget and attachments

Measured on hero 3:

- character rig 200 bones, FP rig 79, 64 common ids.
- For every common bone the bind world matches: worst translation delta
  `0.0059 m`; arm chain `0.000 m` / `0.0 deg` (ids 13/14/44/54/55/74).
- No basis-change retarget is needed for shared bones; copying local
  transforms is exact there.
- Weapon hooks: character 44/74 have dropped intermediate ancestors; the FP
  hooks are the composed chain. The identity
  `bind_fp(44) = bind_char(284) * bind_char(44)` (and 285 -> 74) holds to
  `0.00000 m` / `0.00 deg`. `build_chains`/`sample_chain` compose those
  ancestors; ordinary bones are one-link chains.

The unresolved stage is the **reference skeleton**. The card's shoulder rows
(ids 13/54) carry `P = 2.395x` the model bind in the same direction in c50,
c65 and c71; every other row is within 1% of bind. No installed model carries
the clip's 83-id set (best coverage 50/83), and the models that do carry the
0.2227 shoulder local do not match those ids; the reference rig is therefore
carried by the clip/asset side or built by retarget code, not a single model
asset. Direct local application is the
current feed and is wrong by construction at the shoulders.

The clip speaks the **character-rig id space**, not the FP rig's: of 83 record
ids, 71 resolve to the FP skin, 11 are character-rig extras and one is the
section-size word. The 11 unpaired hand/weapon-side ids are not the missing
fingers; the FP arms mesh's finger vertices are weighted to 8 bones the card
never names and stay at bind under the posed hand. Do not filter them.

Weapon attachments:

- FP entity child `0x0400000000000017` names the pistol twice at hardpoints
  `0x0A`/`0x0B`; on the FP model those are skeleton weapon bones 44/74
  (bind `(+/-0.4396, 0.9283, -0.0160)`), origin/identity.
- Pistol model `0x0d00000000000413` (10 bones, 5,366 weighted vertices, two
  textured submeshes) is drawn rigid in its own bind at palette slot 0.
- Leaving the hook bones at bind avoids the character clip turning the pistols
  inward; the guns still converge slightly because the parent arm pose is
  character-rig data. `--weapon-fold 1` exposes the folded character frame for
  A/B; it points the guns outward and is not the default.

## FP camera and view model

Read values:

- `HorizontalFOV` 103.0 deg (name v2b `0x7ff7c87f2f9c`); vertical FOV derived
  from aspect (70.53 deg at 16:9).
- `camera.FirstPersonNearZ` / `FarZ` = `0.01 / 1638.4 m`; default record at
  rva `0x3cc6860`. The FP pass uses this projection; depth clearing between
  passes is fitted.
- `STUFirstPersonComponent` hero 3: `m_4B75D84E` 50, `m_E29993A5` 1.25,
  `m_816B830F` 0.875, `m_CF5055F1` 0.8, offset `(0.1375, 0.23375, 0.20625)`.
  The consumer semantics are unread.
- Drag spring `m_F486DCF2` (`STU_6AE5A305`): 64 / 16 / -1. The form is fitted
  as a zero-rest offset spring driven by camera angular velocity:
  `off'' = -64*off - 16*off' - omega_camera`. The absolute-angle follower is
  falsified (72 deg lag at 5 rad/s); the fitted form gives 4.5 deg at 5 rad/s.
- The view model is camera-relative; camera yaw and pitch carry the arms and
  guns. Camera motion itself is exact, with no whole-view lag.
- The renderer-side FP camera transform is unread. The framing is fitted:
  FP pass camera lowered 0.35 m and backed 0.30 m along the camera's own up/view
  (`--fp-cam-drop`, `--fp-cam-back`), world camera unchanged. `--fp-eye 1.25`
  put the arms too high, so 1.25 is unlikely to be the eye height.
- The hero's two `0x00020400` clips are `c15a` (installed) and `c2ef` (not
  installed). `c15a` is a 170-frame camera bob: table 0 has 169 position keys,
  13 rotation keys; table 1 is static. It is not a body animation.
- `retarget_probe` c71 worlds: hands 14 `(0.155, 1.367, 0.091)` and 55
  `(-0.210, 1.364, 0.092)`; weapons 44 `(0.113, 1.441, 0.348)` fwd `+x` and
  74 `(-0.143, 1.506, 0.350)` fwd `-x`. The **bind** hardpoints are at
  `+/-0.44, 0.93`; c71 brings both weapons toward the centre and points them
  outward. At eye `1.75` / FOV `29.4` the grips are outside the view (only the
  tips); the component's `1.25` eye height with FOV `50` brings hands and guns
  into view. The client attaches at the FP skeleton's 44/74; the viewer now
  defaults to that source, with `--weapon-fold 1` keeping the character-c71
  A/B path.

### Runtime FP component objects (2026-10-21)

`D:/ow-dumps/ow_heap_full.dmp` (main base `0x7ff6a2d80000`) contains the live
`STUFirstPersonComponent` data. The exact float pattern
**64.0 / 16.0 / -1.0** followed by **50.0 / 1.25 / 0.875 / 0.8** occurs at heap
VA `0x1bb9614d97c`; a second object at `0x1bb273444c0` carries
`65 / 1.25 / 0.875 / 1.0`, and a third at `0x1bb6dfc38d0` carries
`47 / 1.25 / 0.875 / 1.0`. These are per-hero runtime components, not data
defaults: the 64/16/-1 drag-spring triple and the four scalars are present in
the same object.

The shared interface vtable pointer is `0x7ff6a5f55f30` (static
`0x7ff7c7ef5f30`); the vtable has a type-info entry and generic component
methods, not FP-specific camera code. The class hash `0x9d28963f` maps to
**type index `0x35`** in `FUN_7ff7c66aa450`; its registration block is
`0x7ff7c4fe71b9..0x7ff7c4fe7266` and its descriptor is
`0x7ff7c8c9c640`. The component is therefore **data**, and its consumer is a
system keyed by type `0x35` (or by the entity member offset), not a vtable
method. That system is the next anchor.

The STU schema (`fpcomp_probe 3`) is seven fields, size `0x70`:

- `m_4B75D84E` = 50.0
- `m_816B830F` = 0.875
- `m_A02DD607` = inline `STU_0xE43E9C08`, `m_position =
  (0.1375, 0.23375, 0.20625, 0.0)`
- `m_entity` = reference to the **view-model entity** (`0x0400000000000010`
  on hero 3)
- `m_CF5055F1` = 0.8
- `m_E29993A5` = 1.25

`m_position` is the strongest candidate for the **view-model offset from the
camera**; the current viewer's `--fp-cam-drop/back` fit is a substitute for it.
The four scalars have no resolved names yet; 50 is the FOV candidate, and the
consumer that assigns the roles is still unread.

The runtime object at `0x1bb9614d87c` confirms the layout: the interleaved
`m_position` floats are at `+0xc0` (`0.1375`, `0.23375`, `0.20625`), the
64 / 16 / -1 spring at `+0xf0`, the four scalars at `+0x100`
(50 / 1.25 / 0.875 / 0.8), and the shared vtable pointer at `+0x110`. The
consumer reads these offsets; the data-side values and the object layout are
now known.

The **consumer path is found**. In the live main image, `FpViewmodelConsumer`
(RVA `0xd2f060`, static `0x7ff7c5a4f060`) calls `GetComponent(type 0x35)`,
checks the component against the type descriptor, reads the component's
view-model entity at `+0x8/+0x9`, resolves the view-model entity's model
(`+7/+0x28`), and calls the bind `FUN_00d2faa0` (RVA `0xd2faa0`). The bind
stores the view-model entity/model refs and resets the view-model state; it
does **not** read `m_position` or the four scalars. So the type-0x35 component
and the separate view-model entity are confirmed, but the **per-frame**
consumer that applies the offsets/scalars is still open.

> **Frame note (2026-10-22).** The RVAs in this section are the **live main
> image's** (the session flat dump), not the v2b standing image. In build
> 2.25.0.1 - the build the install runs now - the whole first-person path
> sits at these v2b addresses, and every one of them is named in the Ghidra
> project (`docs/function-names.csv`):
>
> | what | live-image RVA | v2b address (2.25.0.1) |
> |---|---|---|
> | the consumer | `0xd2f060` | `0x7ff7c5a4f060` |
> | the bind | `0xd2faa0` | `0x7ff7c5a4faa0` |
> | per-frame update A (slot `+0xe0`) | `0xd31bf0` | `0x7ff7c5a51bf0` |
> | per-frame update B (slot `+0xe8`) | `0xd322f0` | `0x7ff7c5a522f0` |
> | the event player | `0xd29c60` | `0x7ff7c5a49c60` |
> | the event sort | `0xd27410` | `0x7ff7c5a47410` |
>
> The addresses come from the view-model class's own vtable (the live image's
> RVA `0x3115ef0`): slot `+0x20` is the consumer, `+0xd8` a setter, `+0xe0`
> and `+0xe8` the per-frame updates.

The view-model object's class vtable is at RVA `0x3115ef0` (run `105102`,
session base `0x7ff736e60000`). Slot `+0x20` is `FpViewmodelConsumer`; slot
`+0xd8` (`0xd318c0`) is a setter; slots `+0xe0` (`0xd31bf0`) and `+0xe8`
(`0xd322f0`) are the **per-frame updates**. They walk **six** 0x110-stride
slots and call `FUN_00d29c60` / `FUN_00d3a020`.

Those functions are an **animation event/state player**, not the pose:
`FUN_00d2a250` dispatches event masks (`0x100/0x200/0x400/0x800`) against an
event list, and `FUN_00d27410` sorts 0x50-byte event records by a `short`
priority. No bone transform is written here. The view-model **pose** remains
the clip/blend/retarget path, and the FP camera/FOV consumer is still unread.
Implementing these six slots would add animation events, not move the arms.

The viewer now poses the two **weapon instances** from the selected **FP
skeleton's 44/74** and applies the hardpoint local and camera, matching the
client's attachment. `--weapon-fold 1` keeps the old character-`c71` source
for A/B. The card-local retarget is still missing (FIT-ANI-08).

## 2026-10-23 exactness check (Ghidra)

The `--play` view-model animation is **not exact**. Read against
`module_v21_clean.bin` (the filled image; base `0x7ff7c4d20000`); the same
code is complete in `Overwatch.exe` (base `0x140000000`, address =
`0x140000000 + (v2b - 0x7ff7c4d20000)`):

| stage | client (read) | viewer |
|---|---|---|
| per-child sampler | `AnimSamplerRule` v2b `0x7ff7c6e84610`: child 0 is copied **unconditionally**; for each later child, rotation/translation/scale lanes are replaced only when that child's weight is `> 0.5`, never lerped; four scalars are lerped at any weight; an absent lane keeps the previous value | fixed for one-link layer bones: `anim_tree.rs` now replaces rotation and translation independently; dropped-ancestor chains still fall back to the composed pair; scalar lanes are not decoded |
| reference skeleton | `anim.EnableProxyRetarget` / `anim.EnableFinalRetarget` are registered with descriptions "proxy pose re-target between (inherited) blend trees with different reference skeletons" / "final pose re-target between reference skeleton and model skeleton" | no reference stage; the clip P lane is applied directly as the FP model local |
| Pose2d driver | dataflow VM writes branch category, x/y and the LayerChild weight | fixed `x=y=0`, layer weight 0 (file default); `--tree-*` are experiments |
| time wrap | `FUN_7ff7c58b1ef0`: sign-preserving `fmod` by the clip period if loop bit 0 is set, otherwise clamp to `[min, period]` | always wraps `(seconds*fps).rem_euclid(frames)+1` |
| view-model update slots | six `0x110` slots at `+0x60` are **event/state timelines**; `FpViewmodelUpdateA/B` advance event timers and dispatch masks, no clock and no bone write | no animation events/timelines |
| weapons | hardpoints `0x0dc:0a/0b` hang off the **FP skeleton's** weapon bones 44/74; pose is `FP bone world * hardpoint local` using the selected FP skeleton/retarget; character → FP chain is `284*44` / `285*74` | default now uses the selected FP skeleton's 44/74; `--weapon-fold 1` keeps the old character-c71 source for A/B; the reference retarget is still missing |
| FP camera | `UpdateFirstPersonCameraPose` (`0x7ff7c56b7b90`) takes position `anim+0x1350`, forward `+0x1310`, up `+0x1320`, FOV 70.53° and applies that to the camera | world eye + yaw/pitch + fitted `--fp-cam-drop 0.35` / `--fp-cam-back 0.30` |
| drag spring | `FUN_7ff7c53ab620` on anim `+0x1350` toward `+0x1370`: coefficients `0.235` / `0.48`, k ramp `600.0/s`, settle threshold `0.004 m` | `vm_spring_step` 64 / 16 / -1 (the component spring has **no reader** in the image; the target writer is still unread) |
| component fields | `m_position` (0.1375, 0.23375, 0.20625), `50`, `1.25`, `0.875`, `0.8` have **no readable consumer** (the type-0x35 consumer only reads the view-model entity/model refs) | not applied; `50` was correctly rejected as the FP FOV |

What **does** match: the `0.5` gate constant (`0x7ff7c7ca0ff8`), strict
`>` replacement, item 3 / category 5 -> c71 at the authored defaults, the
id join for the 64 common bones, the FP hardpoint attachment points, the
103°→70.53° FOV and the FP clip planes; the per-channel sampler now matches
for ordinary one-link layer bones, and the weapon source now defaults to the
FP skeleton.

## What works

- `--anim auto` plays the FP idle through the blend tree; default category 5
  selects c71. The tree is the authored idle, but the reference retarget,
  camera pose, weapon source and sampler channel rules above are not exact.
- `--anim <guid>` plays a single clip for every family the row readers handle;
  the walk/trio/zone and slot families are playable.
- `--play-th` prints the tree summary and per-clip pairing/movement counts for
  headless checks.
- `--weapon-fold`, `--fp-fov`, `--fp-eye`, `--fp-cam-drop/back`,
  `--vm-k/c/g`, and `--tree-*` expose the fitted/diagnostic knobs.
- `blendset_probe 3` prints the true FP items, clips, weights, slots,
  retarget params and tree.

## Open problems

1. **Reference-skeleton retarget.** The client registers
   `anim.EnableProxyRetarget` and `anim.EnableFinalRetarget`; the card P lane
   is the reference local, and the final stage maps reference -> model. The
   viewer applies it directly, which stretches the shoulders by the source
   rig's `2.395x` grip width. The retarget body is not in the readable pages;
   do not invent a per-bone rest matrix.
2. **FP camera pose setter.** The camera is the animated FP entity pose
   (`anim+0x1350`, `+0x1310`, `+0x1320`); the viewer's `eye`/`yaw`/`pitch`
   plus drop/back is a fit. The model entity's transform setter is still
   unresolved. `camera.FirstPersonAnimationWeight` is camera-only.
3. **Pose2d dataflow VM** (x/y inputs, category selection, per-tick weights).
4. **Per-channel sampler.** The client rule is read; the viewer's
   `TreeAnim::evaluate` must represent rotation/translation/scale separately
   (and child 0's unconditional copy) before layer experiments are meaningful.
5. **Weapon source.** The client uses the FP skeleton's 44/74 with the
   hardpoint local; the viewer uses the character c71 rig. Use the selected
   FP pose once the retarget is read.
6. **View-model event timelines.** The six `0x110` slots are event/state
   timelines; the viewer has no events. They do not write bone transforms, so
   this is state/effect fidelity, not the arm pose.
7. Cold evaluator kinds; needs a capture with those pages resident.
8. Walk item's 8 not-installed clips; tier-3 play-slot names.

## Evidence boundary

Fitted entries in this area are listed in `docs/fitted-ledger.md`: the omitted
reference-skeleton retarget, the FP camera drop/back framing, whole-rig FP
pitch, the character-c71 weapon source, the per-channel sampler/driver math,
and the depth clear between passes. The component's 64/16/-1 spring is **not**
the client's `+0x1350` spring; the latter is read. Addresses in this file name
their capture or build; do not promote a fit without one.

## Commands

```sh
cargo test --release -p ow-tank
cargo test -p ow-view
cargo run --release -p ow-view -- --play --hero 3 --anim auto
cargo run -p ow-tank --example blendset_probe -- 3
```

Eye-checks: `--anim auto` (idle sway, grips separated),
`--anim 0x0a00000000000050` (rest), `--anim 0x0a00000000000065` (reload),
and walk/trio/zone clips `f4`, `958`, `2eef`.

## Retired readings

- The early 32-byte *pose-record* grid was an index table, not key records.
- "16-byte stride unit quaternions" and "77-85 byte per-channel bit-packed
  blocks" were artifacts; the packed rotation sample is 6 or 8 bytes.
- A T+F unit is one channel, not one frame; positions are per-frame absolute.
- The "168/170-bone FP clip" is wrong: `0x00020400` carries two track tables,
  not a 170-bone list.
- The shoulder rows are not a mirror reordering and not a misread constant;
  `2.395x` is the source rig's grip width.
- Holding the P lane at bind made the hands overlap; the pose slot has a
  translation lane and the feed applies it.
- Positional record->bone pairing is wrong; the join is by id.
- The centre weapon bone is wrong; pistols use hardpoints on bones 44/74.
- The blend items are `0xDDD18945`; `0x3B150012` rows are payloads.
- The whole camera does not lag; only the view model drags.
- c65 is Tracer's reload, not the idle; c71 is the idle.
- The hero gallery does not initialise the gameplay animation gate; a
  match/practice-range session does.

# Animation and the view model

Skeleton, clip format, runtime pose path, first-person blend tree and the
view-model attachments.

## Current state

- **File format: solved.** Type-`0x006` clips decode: family-tagged header,
  32-byte metadata rows, packed-u16 rotations, position/scale tracks.
- **Runtime consumer: read.** Rows are sampled per tick; there is no load-time
  expansion. The pose slot has rotation and two vector lanes.
- **FP blend tree: read.** Item 3 selects the idle clips; `--anim auto`
  evaluates the tree.
- **FP retarget: partly read.** The rigs and the dropped-ancestor fold are
  measured from the install; the reference-skeleton stage is open.
- **FP camera: read path.** See `formats/camera.md`.
- **Open:** the Pose2d dataflow VM, the reference retarget body, the per-frame
  component consumer.

## Skeleton and rigs

- hero asset → gameplay entity;
- `STUModelComponent` (`0x191835F9`), field `0x2D124BE2` → character model
  (`0x00C`) and its skeleton;
- `STUFirstPersonComponent` (`0x9D28963F`), field `0xA83C2C26` → a separate
  view-model entity, whose own `STUModelComponent` names the FP model through
  the same field.

The view model is a parallel entity with its own model, not a flag on the
character model. `STUModelComponent` also carries `m_animBlendTreeSet`
(`0x523339C1`), the join to the FP animation set.

`mskl` is the model skeleton. Header: thirteen `i64` section offsets
(hierarchy, matrices, inverse matrices, bind pose, deltas, ids, names, remap, a
second hierarchy), an `i32`, five `u16` counts. Hierarchy entries are 6 bytes
(`i16` parent, `-1` at the root); the bind pose is 48-byte records; bone ids are
`u32` hashes.

Measured: Reaper character 229 bones / 71 FP; Tracer 200 / 79. The FP rig's
neck sits at the same `y = 1.7104 m` as the character's.

`MrnmHandler` (`0x141b88f10`) builds the render-mesh runtime block from the
model payload -- `0xA0`-stride submesh, `0xF8`-stride vertex-buffer and
`0x18`-stride index-buffer records, plus a `0x00` qword that is the OR of each
submesh's `+0x28` -- and is verified as `fn/MrnmHandler`
(`formats/model.md`). It is **not** a skinning hierarchy: the earlier
"parent-indexed tables / used-bone mask" reading does not survive the verified
offsets. `mskl` is the skeleton; how its tables relate to `MRNM`'s block is
unread.

## Bone indices (read, 2026-11-05)

The chain from a mesh vertex to a bone is three steps: a raw **blend index** in
the vertex stream, a **Lookup** table in `mskl`, and a bone index in
`[0, bones_abs)`. An earlier version of this page said the Lookup "maps the
model's bones to the clip's bone set" — that is wrong, and the data says so.

**The file side.** A vertex element with semantic 4 is the four blend indices
(format `0x07`, four `u16`); semantic 5 is the four weights (u8, summing to
`255`). A census over all 32,280 model assets finds 8,761 semantic-4 element
records: 8,307 at `index 0` and **454 at `index 1`** — a second four-wide set,
always format `0x07`, and on those models the eight weights still sum to
exactly `255` on every vertex (measured on all 454) and the second set's raw
indices are inside the Lookup on all 454. No other semantic index occurs.

**The client side, `MsklTableBuild` (`0x141b894d0`).** `ModelChunkLoad`
(`0x141b5df70`) calls it for the `mskl` chunk (version `0x20`) and stores the
returned block at **model `+0x08`**. The function copies the payload's arrays
into one block and, in doing so, fixes each table's element size:

| runtime | source | element | count |
|---|---|---|---|
| `+0x00` | `mskl +0` | 6 B chain record | `bones_abs` (108) |
| `+0x08` | `mskl +8` | 64 B matrix | `bones_abs` |
| `+0x10` | `mskl +16` | 64 B inverse matrix | `bones_abs` |
| `+0x18` | `mskl +24` | 48 B bind pose | `bones_abs` |
| `+0x20` | `mskl +32` | 48 B delta | `bones_abs` |
| `+0x28` | `mskl +40` | 32 B | `bones_abs` |
| `+0x30` | `mskl +48` | 4 B, or null | `bones_abs` |
| `+0x38` | `mskl +56` | 4 B id | **`bones_simple` (110)** |
| `+0x40` | `mskl +72` | 2 B | payload `+130` |
| `+0x48` | `mskl +80` | 2 B | `remap_count` (114) |
| `+0x50` | `mskl +88` | 2 B | `bones_abs` |
| `+0x58` | `mskl +96` | 8 B | `remap_count` |

It also stores the counts at `+0x64` (`bones_abs`), `+0x66` (`bones_simple`),
`+0x68` (`bones_cloth`) and `+0x6a` (`remap_count`), and copies payload
`116..130` verbatim to `+0x6c` (seven `u16`s the client does not read again).
Three consequences for a reader: the **id array is `bones_simple` long** (the
reference calls its count `IDCount` at 116; the two agree on every install
model), the **Lookup is `remap_count` `u16`s at offset 80**, and the array at
offset 88 is a *second* table of `bones_abs` `u16`s.

**What the tables are.** The Lookup at `+80` is the **mesh slot → bone** map:
load it, and a vertex's raw blend index names a slot, not a bone. The array at
`+88` is its **exact inverse** — for every bone, the slot that names it, or
`0xFFFF` for a bone no slot names (on Tracer's character `bones_simple` +
`bones_cloth` = `bones_abs`, 130 + 99 = 229, and 30 bones have no slot). The
array at `+96` is `remap_count` 8-byte records and is all `0xFFFFFFFF` on the
four pinned rigs; nothing here consumes it.

**Measured over every model asset in the install** (`ow-tank --example
bonemap_probe -- --all`; 32,280 assets, 7,359 parsed, 7,328 with skinned
meshes and a Lookup):

- the Lookup is a **permutation** on all 7,328 — distinct, in
  `[0, bones_abs)`, no `0xFFFF`; 815 of them are the identity;
- `remap_count` is the largest raw blend index over **every weighted lane**
  plus one on 7,308 models, and the other 20 sit lower (spare slots at the
  end, none overflowing) — so **no raw index ever falls past the table**, and
  the two unbound branches are unexercised: over `96,140,954` dominant-
  influence vertices, `0` name no bone through the raw index and `0` through
  the Lookup;
- the inverse table at `+88` is the **exact inverse** on all 7,328 models and
  holds `0xFFFF` exactly on the `bones_abs - remap_count` bones no slot names
  (Tracer: 30 of 229); `0xFFFF` appears there and in no Lookup;
- every submesh of every model carries **`BoneIdOffset` = 0** (payload `+0x6a`,
  runtime `+0x82`), so the reference's `raw + BoneIdOffset` and a bare `raw`
  are indistinguishable in this install;
- the geometry agrees: the dominant influence's bone sits a median `0.122 m`
  from its vertices through the Lookup versus `0.591 m` through the raw index
  on Tracer's character mesh (`0.0529` vs `0.3417` on the FP rig, `0.0511` vs
  `0.4040` on a view model, and `0.0743` vs `0.0828` on the 10-bone pistol,
  where the permutation is nearly the identity).

**Still open.** The consumer is not read: the file's own tables are read here
and the rule is proven by the data, but no client function that joins a mesh
index to a bone (CPU-side or as an uploaded remap buffer) has been found, so
whether it adds `BoneIdOffset`, how it bounds the index, and whether it treats
`0xFFFF` as "unbound" stay the reference's rules — all three unexercisable on
this install. And the viewer reads only the **first** four-wide set: the 454
models with a second `semantic 4 / index 1` element lose those four influences
(`ow-tank-1to1.md`).

## Clip container

Type `0x006`, common header:

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

Section 1 begins at `0x80` on the measured families: one `u32` per record (bone
id) plus a size annotation.

Families seen: `0x00533C00`, `0x00532400` (FP blend cards), `0x006D2000`
(object/prop idle), `0x00020400` (two-table FP camera bob), plus the walk,
trio and zone families.

### File row

A file row is 32 bytes:

```text
{u16 scale_count, u16 position_count, u16 rotation_count, u16 flags,
 u32 scale_key_land, position_key_land, rotation_key_land,
 u32 scale_data_land, position_data_land, rotation_data_land}
```

All six lands are cursor words. Data lands are relative to the row's own
address: `rec_at + land * 4`. Keys are frame numbers:
`frame = abs(index) % InfoTableSize`, where
`InfoTableSize = int(fps * duration) + 1`. Index width is `u8` while
`InfoTableSize <= 255`, otherwise `i16`.

Rotation tracks: old form 6 bytes (three `u16`, top bits axis order, low 15
bits scale by `sqrt(2)`, `w` derived); new form (`flags & 0x400`) 8 bytes with
bit-packed mantissa fragments. Position: legacy `3 x f32` when `flags & 0x200`,
otherwise `3 x f16 / 32`. Scale: `u16 / 1024`.

## Runtime pose path

`AnimSamplerRule` (`0x1420feca0`) reads 0x18-stride `{child, weight}` entries.
Child 0 is copied unconditionally. For each later child, each present
rotation/translation/scale lane is replaced only when that child's weight is
`> 0.5`; four scalars are lerped at any weight; absent lanes keep the previous
value.

The pose slot is three 16-byte lanes:

| lane | slot offset | blend | normalise |
|---|---|---|---|
| rotation | `+0x00` | nlerp, shortest-path negation when `dot ≤ -1.19e-7` | `rsqrtps` unit |
| vector | `+0x10` | weighted sum `+= w·src` | `×= 1/total` |
| vector | `+0x20` | weighted sum `+= w·src` | `×= 1/total` |

The division by the total weight happens once at the end, over both vector
lanes. The functions are `0x141b812c0` (blender), `0x141b82d70`
(normaliser) and `0x141b81240` (slot allocator; scale rests at 1, third lane
at 0).

Skinning: a pose replaces the bind's **local** matrix
(`local' = T(pose_t) * R(pose_r) * S(local_s)`), for the rotation-only case
(`Skin::posed_world`) and the full case (`Skin::posed_world_full`) alike.
The install measures it: the blend-card clips' *position* tracks land on
`skin.local`'s translations (`crates/ow-tank/tests/blend_bind_check.rs`), so
the pose is a local transform and its rotation is the bone's own local
rotation. The earlier absolute-rotation formulation
(`absolute' = T(bind_t) · R(pose) · S(bind_s)`, then re-deriving every bone's
local from the posed absolutes) is **wrong**: for an unposed child of a posed
parent it computes `inverse(absolute'(p)) · absolute(c)`, so the parent's pose
cancels and a posed bone's children did not follow.

## Kind dispatch

The kind byte at record `+0x0e` has 31 entries. Kinds 2, 3 and 10 have real
handlers; several others land on a default returning 0. Kind 2 resolves the
node, branches on its kind byte, then runs time wrap/clamp, keyframe search +
lerp, dual sample, and optional noise from a static table. Flags at node
`+0xc`: `0x8000` scalar, `0x4000` blend, `0x2000` replace, `0x1000` vec4.
Wrap shaping at `+0x24`: 0 none, 2 mirror, 3 ping-pong, 4 square, 5 inverse.
**Open:** the function that reads `+0x24` has not been located. The sampler
`FUN_140b9a790` (the only direct caller of `AnimTimeWrap`, `0x140b91ef0`) does
not read `+0x24` at all, and the keyframe-search helper it calls (`0x140b90c00`)
takes only `(out, keys, time)`. Treat the shaping as open until the reader is
named. The wrap itself is read and shipped: `AnimTimeWrap` is a min/max clamp
(`maxss(minss(t, max), min)`) when the node flag dword's bit 0 is clear, and a
truncating modular wrap (`t - (float)(int)(t / max) * max`) when it is set; the
range is the node record's own first 16 bytes (`+0x00` min, `+0x04` max,
`+0x08` slot2/noise index, `+0x0c` the flag dword).

## FP blend tree

Join: `STUModelComponent.m_animBlendTreeSet` (`0x523339C1`) → Tracer's set
`0x0040000000000004` (type `0x021`, 10,517 B, 47 instances).

- The true items are `STUAnimBlendTreeSet_BlendTreeItem` (`0xDDD18945`); the
  `0x3B150012` rows are embedded payloads.
- Item 3 is the idle group: clips `c71, c50, d4, bc, 680`; 19 floats, vec2
  `(-0.4, 0.4)`, three u8 flags.
- Its graph is `0x0F8000000000000F` (`STUAnimBlendTree`, 28 instances), bound
  to item 3 by CRC. Root → branch-by-category → layer children; Pose2d nodes
  3/8 drive d4/bc; animation nodes 19/23/10 drive c71/c50/680.
- Category 5 → c71 (base idle); category 9 → c50. The layer weight's file
  default is 0.0, so the tree output at defaults is c71. Its runtime writer is
  open.
- `m_63DEDAC0` is a 1-based slot id: on an animation wrapper it indexes the
  item's clip list; on a parameter object it indexes the item's float array.
- The 0.5 sampler gate applies after the tree.

The Pose2d dataflow sources do not resolve as STU assets; the per-tick VM that
produces x/y and the branch category is open.

## FP retarget and attachments

Measured on hero 3:

- character rig 200 bones, FP rig 79, 64 common ids;
- every common bone's bind world matches: worst translation delta `0.0059 m`;
  arm chain `0.000 m` / `0.0°` (ids 13/14/44/54/55/74);
- copying local transforms is exact for shared bones.

Weapon hooks: character 44/74 have dropped intermediate ancestors; the FP
hooks are the composed chain. The identity
`bind_fp(44) = bind_char(284) * bind_char(44)` (and 285 → 74) holds to
`0.00000 m` / `0.00°`.

Weapon attachments:

- FP entity child `0x0400000000000017` names the pistol twice at hardpoints
  `0x0A`/`0x0B`; on the FP model those are skeleton weapon bones 44/74
  (bind `(+/-0.4396, 0.9283, -0.0160)`), origin/identity.
- Pistol model `0x0d00000000000413` (10 bones, 5,366 weighted vertices, two
  textured submeshes) is drawn rigid in its own bind at palette slot 0.

### Hardpoints: the class, not the chunk, and no consumer

**There is no `MHRP` chunk in this image, and the model load would refuse
one.** `ModelChunkLoad` (`0x141b5df70`) gates exactly seven tags (`MDLC`
`0x165`, `MRNM` `0x1f`, `mskl` `0x20`, `MOCL` `0x14`, `MESD` `2`, `CLTH`
`0x18`, `MSTU` `2`); `ModelChunkSize` (`0x141b5e1e0`) returns `6` for any other
tag and `7` on a version mismatch, and the load pass runs only when the size
pass returns `0`, so a model carrying an `MHRP` chunk fails to load. The
literal `MHRP` also occurs **zero** times in the 75,726,848-byte image in
either byte order, while the control tags occur 2-3 times.

**The class.** A hardpoint is a `STUModel` instance-array element. Class
`0xBE9DDFDF` registers at `0x14034ba50` through
`RegisterComponentTypeDescriptor` (`0x1421cd360`) with size `0x58`,
align `8`, and its parent is `0xF0E34581` (the class descriptor's `+0x20` is
set to the base descriptor by `0x1403501f0`; the base registers at
`0x14034d940` with the same size, the same five fields and a 16-byte `m_name`
at `+0x08`). The five own fields are registered by the block at
`0x14150d6b0`, each `0x1421d4f30` call carrying the hash in `r8d` and the
runtime offset on the stack:

| hash | offset | field |
|---|---|---|
| `0x888BCD8B` | `+0x18` | bone-label asset ref (16 B) |
| `0xEDF0511C` | `+0x28` | id (`u64`) |
| `0xFF592924` | `+0x30` | bone guid (`u64`) |
| `0xAF9D3A0C` | `+0x38` | rotation (quat, 16 B) |
| `0x7DC1550F` | `+0x48` | position (vec3, 12 B) |

The setter `0x14151e580` writes `(0, 0, 0, 1)` for `0xAF9D3A0C` and the getter
`0x14151a160` compares against it, so identity is the client's own default.
`STUModel` (`0x871BD3D0`) registers `m_hardPoints` `0xC45F5F6F` at `+0x130`
with **no type argument**, while its `0xFD232466` array is typed
`STU_F0E34581[]`; the hardpoint array's element class is not declared. The
sibling `0xCB79EE74` (size `0x60`, same parent, one extra field) is a second
subclass.

**No consumer.** Every occurrence of `0xC45F5F6F`, `0xEDF0511C`, `0xFF592924`,
`0xAF9D3A0C` or `0x7DC1550F` in `.text` is either a field registration
(`mov r8d, imm32`) or a `cmp edx/ecx, imm32` inside the two shared accessors;
there is no third kind, and the class hashes occur only in their registrations
and descriptor plumbing. So this image never reads a hardpoint: the
composition (rotation then translation, then the bone's world, then decompose)
and the bone rule (match the guid's low 32 bits against the skeleton ids, `0`
= unbound) are the reference tooling's conventions, consistent with the
install but **not** client behaviour.

Install census: FP model `0x0d000000000000f1` carries 135 instances
(`0xBE9DDFDF` 44, `0xCB79EE74` 90), Tracer's character model
`0x0d000000000002b1` carries 384 (`0xBE9DDFDF` 50, `0xCB79EE74` 215); the base
class itself never appears. `ow-tank::hardpoint` collects by class, so it sees
the 44/50 and not the siblings; the reference reads the array, so the two
readers differ on everything outside `0xBE9DDFDF`.

The unresolved stage is the **reference skeleton**. The card's shoulder rows
(ids 13/54) carry `P = 2.395x` the model bind in the same direction in c50,
c65 and c71; every other row is within 1% of bind. No installed model carries
the clip's 83-id set (best coverage 50/83). Direct local application is
therefore wrong by construction at the shoulders; the reference rig is carried
by the clip/asset side or built by retarget code.

The clip speaks the **character-rig id space**, not the FP rig's: of 83 record
ids, 71 resolve to the FP skin, 11 are character-rig extras and one is the
section-size word. Do not filter the 11.

## View model and FP camera

Read values:

- `HorizontalFOV` 103.0°, vertical 70.53° at 16:9.
- FP near/far `0.01 / 1638.4 m`.
- `STUFirstPersonComponent` hero 3: `m_4B75D84E` 50, `m_E29993A5` 1.25,
  `m_816B830F` 0.875, `m_CF5055F1` 0.8, offset `(0.1375, 0.23375, 0.20625)`.
  The consumer semantics are open.
- Drag spring `m_F486DCF2` (`STU_6AE5A305`): 64 / 16 / -1. The client's
  per-frame spring is on animation `+0x1350` toward `+0x1370`; its target
  writer is open. The 64/16/-1 form is not applied as client behaviour.
- The view model is camera-relative; camera yaw and pitch carry the arms and
  guns.
- The hero's two `0x00020400` clips are `c15a` (installed) and `c2ef` (not
  installed). `c15a` is a 170-frame camera bob: table 0 has 169 position keys,
  13 rotation keys; table 1 is static. It is not a body animation.

## Open

1. **Bone-index consumer.** The `mskl` Lookup is the mesh slot → bone map and
   the install proves it (permutation, size fit, stored inverse, geometry), but
   the client code that joins a vertex's blend index to a bone is not read:
   whether it adds the submesh `BoneIdOffset`, how it bounds the index and
   whether a `0xFFFF` entry means "unbound" are the reference's rules. All
   three are unexercisable on this install (every offset is `0`, no Lookup
   holes), so nothing here can settle them by measurement.
2. **Second blend-index set.** 454 model assets declare `semantic 4 / index 1`
   as well as `index 0`, and the eight weights sum to exactly 255 on every
   vertex of those models; `ow-tank::model` reads only the first set, so the
   viewer drops those influences. `ow-tank-1to1.md` records the gap.
3. **Reference-skeleton retarget.** Read `anim.EnableProxyRetarget` and
   `anim.EnableFinalRetarget`; then apply the final stage.
4. **Pose2d dataflow VM.** Read the driver inputs, category and layer weights.
5. **FP component consumer.** Read the type-`0x35` system that applies
   `m_position` and the four scalars.
6. **FP spring target.** Read the writer of the `+0x1350`/`+0x1370` target.
7. **View-model event timelines.** The six `0x110` slots are event/state
   timelines; the viewer has no events. They do not write bone transforms.

## Verification

```text
cargo test --release -p ow-tank
cargo run -p ow-tank --example blendset_probe -- 3
cargo run -p ow-tank --example bonemap_probe              # the four pinned rigs
cargo run -p ow-tank --example bonemap_probe -- --all     # every model asset
```

`ow-client` is built and draws the resolved practice range; it does not claim
the client's animation runtime. The animation paths above remain read or open
as marked, not a posed-character result.

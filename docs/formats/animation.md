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
`u32` hashes; the remap maps the model's bones to the clip's bone set.

Measured: Reaper character 229 bones / 71 FP; Tracer 200 / 79. The FP rig's
neck sits at the same `y = 1.7104 m` as the character's.

`MrnmHandler` (`0x141b88f10`) builds runtime parent-indexed tables from the
model payload: `0xA0`-stride nodes, `0xF8`-stride hierarchy, `0x18`-stride
auxiliary table, used-bone mask. `mskl` and `MRNM` feed the same hierarchy.

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

Skinning: a pose replaces the bind's local matrix
(`local' = T(pose_t) * R(pose_r) * S(local_s)`); the absolute-rotation path
remains for the rotation-only case.

## Kind dispatch

The kind byte at record `+0x0e` has 31 entries. Kinds 2, 3 and 10 have real
handlers; several others land on a default returning 0. Kind 2 resolves the
node, branches on its kind byte, then runs time wrap/clamp, keyframe search +
lerp, dual sample, and optional noise from a static table. Flags at node
`+0xc`: `0x8000` scalar, `0x4000` blend, `0x2000` replace, `0x1000` vec4.
Wrap shaping at `+0x24`: 0 none, 2 mirror, 3 ping-pong, 4 square, 5 inverse.

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

1. **Reference-skeleton retarget.** Read `anim.EnableProxyRetarget` and
   `anim.EnableFinalRetarget`; then apply the final stage.
2. **Pose2d dataflow VM.** Read the driver inputs, category and layer weights.
3. **FP component consumer.** Read the type-`0x35` system that applies
   `m_position` and the four scalars.
4. **FP spring target.** Read the writer of the `+0x1350`/`+0x1370` target.
5. **View-model event timelines.** The six `0x110` slots are event/state
   timelines; the viewer has no events. They do not write bone transforms.

## Verification

```text
cargo test --release -p ow-tank
cargo run -p ow-tank --example blendset_probe -- 3
```

`ow-client` is built and draws the resolved practice range; it does not claim
the client's animation runtime. The animation paths above remain read or open
as marked, not a posed-character result.

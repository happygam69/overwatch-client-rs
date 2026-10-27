# Open questions: the work queue

Every open question, in the order worth attacking. An entry is closed when the
answer has an address or a value, the finding is in the relevant
`docs/formats/<system>.md`, and the code that uses it has a test.

The rule behind the queue: a question is never closed by a guess. If it is not
read, it stays here and the gameplay feature stays unimplemented.

---

## 1. The mover's input→velocity ramp

**Question.** `MoverInvoker` (`0x1407ffc50`, interface vtable `0x142fdbf40`
slot 9) passes eight arguments to `MoverAdvance` (`0x140ca7f30`): the mover,
the output position, the local wish vector, the current position, two scales,
the step state and the world. Which function computes those arguments from the
input frame? That caller is the ramp/control law.

**What is known.** `MoverAdvance`'s character path computes
`desired = (scale_a * body[0x18..] + wish) * scale_b + pos`, then runs
`RunCharacterControllerSweep`; the search for a writer of `+0xc0..0xcc` is a
dead end because that is a static local offset. `ApplyStateModifiers`
(`0x140cbcbe0`) writes the movement parameter block at `+0x5d0..+0x67c`;
`SubmitMoverPhysicsCommand` (`0x140cc03d0`) sends the command to Domino.

**What would settle it.** Find the vtable-9 call site and read the caller that
produces the wish/position/scales. Then add a state unit for the ramp.

**Next anchor.** `formats/character-controller.md`,
`dev/verify_harness/units/fn-MoverAdvance.json`, `dev/field_index.py`.

---

## 2. The first-person camera consumer and per-frame look

**Question.** `STUFirstPersonComponent` carries `m_position` and four scalars.
`FpViewmodelConsumer` (`0x140d2f060`) and `FpViewmodelBind` (`0x140d2faa0`)
store the view-model entity/model references but do not read them. Which system
applies the offset and scalars? Separately, which function consumes the five
`*InterpolationSpeedInverse` vars and four `*Speed` rates per frame?

**What is known.** The component is type `0x35`; its consumer is a system keyed
by type or entity offset, not a vtable method. The FP camera setup
`ApplyFirstPersonCamera` (`0x14096c5b0`) and
`UpdateFirstPersonCameraPose` (`0x140997b90`) build the pose from the FP
animation entity. The config vars and defaults are read; their per-frame reader
is not.

**What would settle it.** Read the type-`0x35` system's per-frame path and the
camera interpolation consumer. Then the FP pose and the component values can be
applied exactly.

**Next anchor.** `formats/camera.md`, `formats/animation.md`,
`FpViewmodelConsumer`, `UpdateFirstPersonCameraPose`.

---

## 3. The reference-skeleton retarget

**Question.** The client registers `anim.EnableProxyRetarget` and
`anim.EnableFinalRetarget` ("proxy pose re-target between (inherited) blend
trees with different reference skeletons" / "final pose re-target between
reference skeleton and model skeleton"). The clip's P lane is a reference-rig
local. Where is the reference→model stage?

**What is known.** Character rig 200 bones, FP rig 79, 64 shared ids with
identical binds; the weapon-hook fold is exact. The card's shoulder rows carry
`2.395x` the model bind, so direct application is wrong by construction at the
shoulders. The retarget body is not the sampler.

**What would settle it.** Read the two retarget registrations and the final pose
stage, then implement the reference-skeleton transform.

**Next anchor.** `formats/animation.md`, `AnimSamplerRule` (`0x1420feca0`),
`crates/ow-tank/src/blendtree.rs`.

---

## 4. The Pose2d dataflow VM

**Question.** What drives the first-person blend tree: the branch category, the
x/y driver inputs and the LayerChild weight? At the authored defaults the tree
output is the base clip (c71).

**What is known.** The tree structure, item slots and sampler gate are read.
The Pose2d nodes are authored; the per-tick VM that produces their inputs is
not.

**What would settle it.** Read the Pose2d VM in the animation runtime and the
LayerChild weight writer.

**Next anchor.** `formats/animation.md`, `crates/ow-tank/src/blendtree.rs`.

---

## 5. The weapon fire loop and damage application

**Question.** How does a volley fire, and where does per-shot damage come from?
The volley runtime carries `numShotsPerSecond` at `+0x1c8`,
`numProjectilesPerShot` at `+0x1c0`, `projectileSpeed`, `projectileLifetime`,
`castLength` and the typed projectile motions. The graph data carries
`STUStatescriptModifyHealth.m_amount` literals on 627 graphs. The join from
weapon → fire → hit → damage is open.

**What is known.** Firing runs through the statescript executor, not the volley
class's vtable. `StatescriptDriveWalk` (`0x140a264c0`),
`StatescriptNodePush` (`0x142141f70`) and
`StatescriptSlot43Executor` (`0x140a9ba70`) are read. The per-node
implementation behind the type index is not.

**What would settle it.** Read the executor's node dispatch and the
`ModifyHealth` applying function, then join the weapon graph to its ability /
impact graph and the resolved `amount`.

**Next anchor.** `formats/weapons.md`, `crates/ow-tank/src/graph.rs`.

---

## 6. Character collision

**Question.** The client's character step/ledge/slope collision is unread. The
map mesh is currently visual only; the default client draws the practice-range
geometry but has no character collision. `--no-map` still offers the flat
stand-in plane and boxes for viewer work.

**What would settle it.** Read the step query and sweep path from
`MoverAdvance`/`MoverInvoker` down to the world query, and read the map's
collision chunks (`MAP_CHUNK_COLLISION`, `teMAP_CHUNK_ADDITIVE_COLLISION`).

**Next anchor.** `formats/character-controller.md`, `formats/maps.md`.

---

## 7. Domino schema and integration in the current image

**Question.** The current image has `DmWorldConstruct` (`0x142cbeb70`),
`DmWorldInvDt0Writer` (`0x142cc3050`),
`DmWorldSubstepDriver` (`0x142cc3630`), `DmBroadphaseReinit`
(`0x142cbdc60`), `ConstraintDispatch` (`0x142cd9c90`),
`RotationInertiaSolve` (`0x142d07170`), `DmResidualQuery` (`0x142cdf710`) and
`DmTickKeeper` (`0x14208c070`). Which fields, offsets and operation order hold
in this build?

**What would settle it.** Read the world/body constructors and the island
solve in the current image; write the schema and the arithmetic with addresses.

**Next anchor.** `formats/domino.md`.

---

## 8. The view-model spring target

**Question.** The per-frame spring is on animation `+0x1350` toward `+0x1370`
with coefficients `0.235`/`0.48`, a k ramp of `600.0/s` and a settle threshold
of `0.004 m`. Who writes the target, and in what space?

**What would settle it.** Read the spring function's caller and the target
writer.

**Next anchor.** `formats/animation.md`, `crates/ow-tank/src/firstperson.rs`.

---

## 9. Camera modes, 3P camera and shake

**Question.** Read the first/third/spectator mode selection, the 3P camera
consumer (`camera.3PPositionOffsetY.*`, `camera.3PDistanceSmoothing.chaseRate`)
and the shake math behind `localhiteffects.3PcameraShake`.

**What would settle it.** Read the camera system update, the 3P pose builder
and the local hit-effect update in the current image.

**Next anchor.** `formats/camera.md`, `UpdateCameraSystem` (`0x14203c4e0`),
`BuildThirdPersonCameraPose` (`0x14206e400`).

---

## 10. Bots and abilities

**Question.** The practice-range bots are boxes and markers. What is the
client's bot controller and ability activation?

**What would settle it.** Read the bot/entity spawning path and the ability
graph activation from the current image.

**Next anchor.** `formats/weapons.md`.

---

## 11. The map variation selector and the bundle directory producer

**Question.** Two parts of the map/asset path are not 1:1 yet.

1. **The variation selector.** A `STUMapHeader` pairs `m_D97BC44F` mode entries
   with `m_78715D57` variations by index. Each variation's `m_BF231F12` names
   its registered type-`0x002` scene map. The client function that takes a
   requested game mode and chooses a variation - and whether it loads one
   variation or a layer set - is not read. The practice header has one
   variation per mode (`0x18` -> 0, `0x143` -> 1, `0x5a` -> 2), but header
   `0x07900000000000d4` has 63 variations over 20 modes and one mode names six.
2. **The bundle directory producer.** The client keeps a per-type directory
   blob and binary-searches it in `FUN_141fd5160` (`0x141fd5160`); the blob is
   handed to the job at `FUN_141fd53a0` (`0x141fd53a0`) from the content
   database (`FUN_141fcbb10` -> `FUN_141fe81d0`, fields `+0x70`/`+0x264`).
   Where that blob is produced is not read. `ow-tank` builds the same relation
   by scanning the manifest's type-`0x09C` records.

**What is known.** The bundle flag, entry table and all 305 type-`0x09F`
headers are read; the practice header `0x0790000000000688` opens from bundle
`0x0d90968e484f8c33` at offset 3593 and parses as `STUMapHeader` with
`mapName` `Maps\TrainingRoom\Training Room - Practice`. The practice launch
uses the header's `m_mapName` and the paired mode's `STUGameMode.displayName`
(`0x5DB91CE2`), taking the first `Practice Range` in header order (mode `0x18`,
variation 0). Both mode `0x18` and mode `0x5a` are named `Practice Range`; the
launch takes 0 and logs the ambiguity. This is a data-driven launch affinity,
not the client's selector. The resolved scene map
`0x080000000f69002e` requests 34 chunks, all open; the walk yields 1,358
placeables and bit-exact bounds (`tests/map_resolve_real.rs`).

**What would settle it.** For (1), find the caller that reads
`m_D97BC44F`/`m_78715D57` together (header `+0xF0`/`+0x110`; variation scene
map `+0xD0`) and record whether it selects one entry or loads layers. For (2),
read the content-database producer of the directory blob and replace the scan.

**Next anchor.** `formats/maps.md`, `crates/ow-tank/src/mapresolve.rs`,
`crates/ow-tank/src/bundle.rs`, `FUN_141fcbb10` (`0x141fcbb10`),
`FUN_141fe81d0` (`0x141fe81d0`), `FUN_141ff5c80` (`0x141ff5c80`).

---

## 12. Material and texture resolution on the practice range

**Question.** The viewer's practice-range scene reports 55 placeholder draws,
148 flat fills and 3 material entries with no sRGB base colour. Which stage of
the client's material/texture path is not reproduced, and is each case an
unresolved reference or a renderer rule that is not read?

**What is known.** Models, looks, materials and textures open through
`GameData::open_asset`; 289 looks, 594 material entries and 120 textures decode.
The viewer's counts are diagnostics, not a finished material path.

**What would settle it.** Read the client's material application and texture
sampler path, then account for every submesh. Do not assume missing install
content; the bundle-aware path opens the assets.

**Next anchor.** `formats/materials.md`, `crates/ow-tank/src/material.rs`,
`crates/ow-scene/src/lib.rs`.

---

## 13. v1 STU, replay payload, audio decode

**Question.** v1 STU has synthetic tests only; no v1 asset ships here. The
replay frame payload is Zstd and is not decoded. Wwise audio metadata is read,
but no waveform is decoded.

**What would settle it.** A v1 asset; a decoded replay frame; an audio
dependency and the Wwise event-posting path.

**Next anchor.** `formats/stu.md`, `formats/replay.md`, `formats/sound.md`.

---

## How an entry is closed

1. The answer has an address or a value in `Overwatch.exe` or the install.
2. The finding is written in the relevant `docs/formats/<system>.md`.
3. The code that uses it has a test; if it replaces a `FIT-*` row, the row is
   removed from `docs/deviations.md`.
4. The entry is removed from this list, and `STATUS-handoff.md` is updated.

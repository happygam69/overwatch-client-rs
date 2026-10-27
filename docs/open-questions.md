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

## 11. The map request producer and the bundle directory producer

**Question.** Two parts of the map/asset path are not 1:1 yet.

1. **The map request producer.** A `STUMapHeader` pairs `m_D97BC44F` mode
   entries with `m_78715D57` variations by index, and each variation's
   `m_BF231F12` names its registered type-`0x002` scene map. The client has
   **no selector**: it does not read either array at runtime. The map arrives
   as type-`0x002` GUIDs in the level-load message (Jam `0x501B`,
   `0x14063b3f0` dispatch, `0x140618b30` handler, `0x14077b7d0` preload; four
   GUID vectors at message `+0x78`, `+0x90`, `+0xA8`, `+0xC0`). The remaining
   question is **who builds that message**: the server, or the local-session
   setup for an offline map such as the practice range, and which input
   (a variation index, a scene-map GUID, or the mode plus a variant identifier)
   it uses. The client's replay header records a base-map GUID, not a scene
   map, so the client's own map identity is the base map.
2. **The bundle directory producer.** The client keeps a per-type directory
   blob and binary-searches it in `FUN_141fd5160` (`0x141fd5160`); the blob is
   handed to the job at `FUN_141fd53a0` (`0x141fd53a0`) from the content
   database (`FUN_141fcbb10` -> `FUN_141fe81d0`, fields `+0x70`/`+0x264`).
   Where that blob is produced is not read. `ow-tank` builds the same relation
   by scanning the manifest's type-`0x09C` records.

**What is known.** The bundle flag, entry table and all 305 type-`0x09F`
headers are read; the practice header `0x0790000000000688` opens from bundle
`0x0d90968e484f8c33` at offset 3593 and parses as `STUMapHeader` with
`mapName` `Maps\TrainingRoom\Training Room - Practice`. The lookup *consumer*
of the directory blob is a verified cross-check (`fn/BundleDirectoryLookup`,
`BundleDirectoryLookup` at `0x141fd5160`), but it does not answer where the
blob comes from: the scan in `ow-tank` is still our own. The mode entry's
`m_A9253C68` is a `STUIdentifier` (`0xD800000000000000 | u32`, class
`0x9CADF2EC`), not an asset; a census of 305 headers / 1,612 entries finds 30
distinct identifiers. The practice header's three modes share
`0xD800000000008F9D`; header `0x07900000000000d4` has 63 variations over 20
modes and three identifiers (21 entries each), and mode
`0x0230000000000016` names six entries - two scene maps per identifier - so
neither a mode nor a `(mode, identifier)` pair identifies one scene map.

**The static proof of "no client selector".** The field hashes `0xD97BC44F`
and `0x78715D57` occur in `.text` only at the registration sites
(`0x14143ba49`, `0x14143bc95`); the class hash `0x02514B6A` occurs only at
`RegisterStuMapHeader` (`0x14031c5e2`) and in the asset-type descriptor init at
`0x1403216a0` (which stores type `0x09F` / class `0x02514B6A` at
`0x143ed6948`/`0x143ed694c`). The field descriptors and the class definition
are referenced only by registration/init code; no consumer that names the class
or either field exists. No differential harness unit can be written for (1):
there is no client function to mirror; the choice is made by the producer of
the message, so any unit would be a test of our own invention, not of the
client.

**What would settle it.** For (1), find the producer of the `0x501B` message
and read how it resolves a map request to the GUID vectors - including whether
the local practice-range session builds it in-process. For (2), read the
content-database producer of the directory blob and replace the scan.

**Next anchor.** `formats/maps.md` ("Who chooses a variation"),
`crates/ow-tank/src/mapresolve.rs`, `crates/ow-tank/src/bundle.rs`,
`FUN_14063b3f0` (`0x14063b3f0`), `FUN_140618b30` (`0x140618b30`),
`FUN_14077b7d0` (`0x14077b7d0`), `FUN_141fcbb10` (`0x141fcbb10`),
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

## 14. CASC: the container path after the five verified leaves

**Question.** The five leaf units (`Salsa20Xor`, `Salsa20StateInit`,
`DynamicIndexHash`, `DynamicIndexKeyHash`, `EncodingHeaderParse`) are verified,
and the bucket hash and segment/archive selection are now read and
install-tested, but the path that uses them is not. Which client functions
remain, and what do they change?

**What is known.** `DynamicKeyStateLookup` (`0x142501b40`) is
`((xor9 ^ (xor9 >> 4)) + salt) & 0xf`; the read path's salt is 0 at all three
callers. `DynamicIndexOpen` (`0x14250e100`) and
`DynamicIndexScanSegments` (`0x14250e720`) select the highest
`<bucket><segment>.idx` per bucket, with no container-header field and no
merge; `DynamicIndexSegmentOpen` (`0x14250e400`) opens that one file and the
parser rejects a header-bucket mismatch. `DynamicContainerRead` adds `0x1e`
to the packed entry location, and `DynamicContainerReadPage` derives the
archive as `(packed + 0x1e) >> 30`; `DynamicContainerOpenArchive`
(`0x14250c8f0`) opens `data.%03d`. The install test matches 1,147,132 real
keys at salt 0 and the 1,488 archive-header placeholders at salt 1, with no
unmatched entry. The open functions are `DynamicIndexSegmentParse`
(`0x14250fcf0`; header/section/entry only, no page table),
`DynamicIndexFindEntry` (`0x1425111a0`; the equal-range and duplicate-key
choice is unread, and this install lists 806 keys twice), `DynamicKeyStateLookup`'s
resolve (`0x142511540`, `0x1425115d0`), `DynamicIndexDecodeEntries`
(`0x1425209a0`), `BlteFrameHeaderParse` (`0x1424a90e0`), `BlteBlockRead`
(`0x1424d61b0`), `BlteBlockReadLz4` (`0x1424d6e50`), `DynamicContainerRead`
(`0x1424f48d0`; no page cache) and `EncodingSectionsRead` (`0x1424d1c60`).
The shipped decoder accepts only BLTE type `0x0F`, does not check
`12 + 24*count`, skips the frame/chunk MD5s, and has no LZ4/ARC4/frame modes.

**What would settle it.** Read each remaining function in the current image,
implement it operation for operation, and add a differential unit where the
function is pure. `DynamicIndexFindEntry` and the frame header are the first
candidates; the container read and the page-table lookup need a synthetic
two-segment fixture.

**Next anchor.** `docs/casc-1to1.md`, `formats/casc.md`,
`crates/ow-casc/src/index.rs`, `crates/ow-casc/src/container.rs`,
`crates/ow-casc/src/blte.rs`.

---

## How an entry is closed

1. The answer has an address or a value in `Overwatch.exe` or the install.
2. The finding is written in the relevant `docs/formats/<system>.md`.
3. The code that uses it has a test; if it replaces a `FIT-*` row, the row is
   removed from `docs/deviations.md`.
4. The entry is removed from this list, and `STATUS-handoff.md` is updated.

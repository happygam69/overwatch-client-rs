# Open questions: the work queue

Every open question in the project, in the order it is worth attacking. Each
entry says what the question is, what would settle it, and where to start. The
state of the project as a whole is in [`../STATUS-handoff.md`](../STATUS-handoff.md);
the per-system state is in [`systems-coverage.md`](systems-coverage.md).

Rules for an entry: a question is *settled* when it has a recorded address or
a measured number, not when it has a plausible guess. A guess becomes a row in
[`fitted-ledger.md`](fitted-ledger.md) and stays a guess.

**Do we have the captures for these?** Yes, and page coverage no longer needs
a new capture either. Plan A recovered all `.text` from the installed
executable's disk ciphertext; `.pdata` is already plaintext in that executable
and was copied verbatim. The project holds the complete image as the Ghidra
program `Overwatch.exe` (151,260 functions). Questions 2-13 are
analysis/measurement work over code and function extents we now fully hold.

---

## 1. The page-key (`w`) derivation - coverage solved offline, picker tier 3

**Question.** The on-disk `.text` is XORed with a 4 KB window drawn from a
static 16 KiB cycle. The window's start (`w`) is a build-time uniform draw.
Where is `w` computed?

**What is known.** The XOR transform is exact (2,285 current-build oracle rows
verify). The applier is `0x14380e0` (loop `0x144c839`, caller `0x1015a94`),
live-verified in `cpu_20261021_124450.etl`. The picker is inside the loader's
own obfuscated VM/dispatcher, not Denuvo (`0x2511e0` dispatcher): the selector
chain `0x1ce59b0 -> 0x3fa280 ->
0x1494090` hashes the page VA and takes `hash % count` over a 1,439-entry table
of type-A page records. No stored or hashed `w` table was found in the disk
loader, the live loader, the live main module, the private ghost loader copy
or the full heap dump.

**Coverage result (2026-10-23).** `dev/offline_page_recover.py` recovered
**12,076/12,076 `.text` pages** from the on-disk ciphertext and copied the
install's **plaintext `.pdata`** (201,246 entries). Held-out oracle
2,284/2,284; warm images 4,887/4,888 and 5,350/5,351. One page keeps a
declared 16-byte tail (`0x540000`). Image:
`D:/ow-dumps/offline_recover/module_offline.bin` (preferred base
`0x140000000`), imported as the Ghidra program `Overwatch.exe` and analysed
to 151,260 functions; report: `D:/ow-dumps/offline_recover/REPORT.md`. Read a
v2b address there as `0x140000000 + (v2b - 0x7ff7c4d20000)`.

**What would settle the picker.** Static reverse/emulation of the loader VM
at `0x2511e0`/`0x40bee0..0x411536`; a fault-synchronous capture is no longer
needed for coverage, only for naming the derivation.

**Command.** `python dev/offline_page_recover.py --stage all`

**Impact.** A new build re-extracts the pad and reruns the blind solver; no
fresh oracle is required. The picker stays tier 3 until the VM is read.

---

## 2. The first-person camera consumer and the per-frame camera tick

**Question.** `STUFirstPersonComponent` is read: the per-hero values
(`64/16/-1` then `50/1.25/0.875/0.8`), the schema (`0x70`, 7 fields), the
`m_position` offset, and the consumer path (`FpViewmodelConsumer`, RVA
`0xd2f060` -> bind `0xd2faa0`). The bind stores the references but never reads
`m_position` or the four scalars. Where are they applied per frame? The
**camera interpolation tick** is also unlocated: the five
`*InterpolationSpeedInverse` vars and the four `*Speed` rates have no direct
reader outside their registration stubs. The camera code pages are warm, so
the reader is reached indirectly or through a copied settings block; this is
not a cold-code gap.

**What would settle it.** A read of the type-`0x35` system's per-frame path
(the vtable slots `+0xe0` / `+0xe8` are an animation event player, not the
camera). The consumer is a system keyed by type `0x35`/entity offset, still
unread. For the tick, a capture with the registry-reader pages warm, or a live
read trace of the static var objects listed in `formats/camera.md`.

**In the current build (2.25.0.1) the path is located and named:** the
consumer is v2b `0x7ff7c5a4f060`, the bind `0x7ff7c5a4faa0`, the two
per-frame slots `0x7ff7c5a51bf0` and `0x7ff7c5a522f0`, the event player
`0x7ff7c5a49c60` - all read from the view-model class's own vtable, all
present in the capture, all named in `docs/function-names.csv`. The
per-frame slot A walks the six 0x110-stride animation slots and runs the
event player on each, which is what the doc says; the camera consumer is
still the open part.

**The FP camera itself is a separate path (read 2026-10-23).** The setup is
v2b `0x7ff7c568c5b0` (RVA `0x96c5b0`), readable but not a defined Ghidra
function. It sets a **60.0 deg initial plane angle** (`0x7ff7c8582b0c`), the
FP near/far pair, an animation-blended FOV and the camera pose. The pose comes
from `UpdateFirstPersonCameraPose` (`0x7ff7c56b7b90`), which fills the FP
camera FOV from `FUN_7ff7c5700f50(0)` (a full angle clamped to `[0.05, 3.0]`
rad). That function calls `GetHorizontalFov` (`0x7ff7c6850fc0`) through the
aspect-correction helper `FUN_7ff7c684b940`, then converts the 103-deg
horizontal setting with `tan`/`atan` and the 9/16 factor: **70.53 deg
vertical** at 16:9. The `50` scalar is **not** the FP FOV, and the `60.0` is a
fallback. The FP FOV is therefore the same as the world FOV; only the camera
pose/drop/back remains fitted (`--fp-cam-drop`, `--fp-cam-back`).

**Next anchor.** `docs/formats/camera.md` (first-person section),
`docs/formats/animation.md` (the first-person section) and
`docs/formats/input-camera-audio.md`.

**Impact.** The FP camera framing stays fitted (`--fp-cam-drop`,
`--fp-cam-back`); the eye height and FOV in the ledger are read values but
their application is not. The interpolation behind `--cam-look-inverse`
stays fitted.

---

## 2b. View-model animation exactness (2026-10-23 check)

**Question.** The viewer plays the authored idle tree, but is it the client's
exact pose? The check against `module_v21_clean.bin` says no. Proven gaps:

- **Reference retarget.** The client registers `anim.EnableProxyRetarget` /
  `anim.EnableFinalRetarget`; the card P lane is a reference-skeleton local and
  is mapped to the model by the final stage. The viewer applies it directly,
  stretching shoulders by 2.395x.
- **Sampler channels.** `AnimSamplerRule` (`0x7ff7c6e84610`) copies child 0
  unconditionally and replaces each present rotation/translation/scale lane
  independently at `w > 0.5`; scalars lerp; absent lanes keep the previous
  value. The viewer now does this for one-link layer bones; dropped-ancestor
  chains fall back to the composed pair and scalar lanes are not decoded.
- **FP camera.** `UpdateFirstPersonCameraPose` uses the animated FP pose
  (`+0x1350/+0x1310/+0x1320`); the viewer uses the player eye/yaw/pitch plus a
  fitted drop/back. The pose's target writer is unread.
- **Weapons.** The pistols are on FP hardpoints/bones 44/74 with the folded
  chain; the viewer now defaults to that FP source (`--weapon-fold 1` keeps
  the old character-c71 A/B).
- **Spring.** The per-frame spring is on `+0x1350` toward `+0x1370` with
  `0.235/0.48`, k ramp `600.0/s`, settle threshold `0.004 m`; the viewer's
  64/16/-1 form has no reader and the target writer is unread.
- **Events.** The six `0x110` slots are event/state timelines; the viewer has
  no animation events, but they do not write pose.

**What would settle it.** Read the reference-retarget body, the FP camera/spring
target writer and the Pose2d dataflow VM. The per-channel sampler fix and the
FP weapon source are applied.

**Next anchor.** `docs/formats/animation.md` (2026-10-23 exactness check) and
`docs/fitted-ledger.md` FIT-ANI-04/05/08, FIT-CAM-01.

---

## 2c. Mechanically verify the next rung of client functions (2026-10-24)

**Question.** `dev/verify_harness/` now decides, against the emulated real
bytes, whether a reconstruction of a client function is behaviourally exact.
Two zero-callee functions are verified (`AnimTimeWrap` `0x7ff7c58b1ef0`,
`CurveKeySampleDual` `0x7ff7c58b09d0`); the decompiles had two bug classes the
harness caught (conversion instead of bit-load; IEEE ifs instead of SSE
minss/maxss). Which functions come next, and can the 1-3 callee rung be
verified by emulating callees inside the mapped image?

**Shipped-code path (2026-10-25).** Tier 2 no longer verifies a copy:
`crates/ow-sim/src/bin/verify_probe.rs` is built by cargo and imports the real
crate functions. The ledger binds to `candidate_sources` + `crate_rev`.
`fn/OrthonormalizeCameraBasis-cargo` and `-state` are verified this way, and
the state unit proves the snapshot/state-channel path for stateful functions.
`scaffold_unit.py` turns an address into a `.pdata` extent, a decompile and a
unit template under `units/scaffolds/`; the mover template is
`units/fn-MoverAdvance.json`.

**What is known.** Three other zero-callee decompiles are not verifiable as
they stand: `AnimWritesOrientationPosition` and `DecodeCameraNodeTypeNames`
are comment-only stubs (need a Ghidra re-decompile first);
`DispatchFpCameraSlotTime` has an indirect call and an unrecovered jumptable.
The second rung (`AnimFovHelper` `0x7ff7c5700f50`, `AnimDragSpring`
`0x7ff7c53ab620`, `FpViewmodelEventSort` `0x7ff7c5a47410`, вЂ¦) has 1-3 named
callees whose bodies sit in the same mapped image; the oracle's emulator can
follow them if the fixture provides the globals they read, and the
reads-outside-args check will name any undeclared state dependency.

**What would settle it.** Re-decompile the two stubs and the jumptable
function in Ghidra; add a unit per second-rung function with a fixture that
satisfies its callees; run `python dev/verify_harness/cli.py all` to gate.
Prefer a `"cargo"` unit whose probe entry is the real crate function; a
stateful one declares a `state` block and a `guest` pointer.

**Next anchor.** `dev/decompiles/address_list.txt`, the unit JSONs under
`dev/verify_harness/units/`, and `dev/verify_harness/scaffold_unit.py`.

---

## 3. The Pose2d dataflow VM

**Question.** What drives the first-person blend tree: the driver inputs and
the layer weights. At the authored defaults the tree output is the base clip
(c71).

**What would settle it.** A read of the Pose2d VM in the animation runtime
(the clip/blend/retarget path is measured; the VM that feeds it is not).

**Next anchor.** `docs/formats/animation.md`.

---

## 4. Per-shot damage and armour mitigation

**Question.** Damage is not in the files; it is supplied by the operator.
Damage *order* and armour mitigation are assumptions.

**What would settle it.** A capture of the damage path (the statescript
damage nodes are read, the per-class code is metaobject only).

**Next anchor.** `docs/formats/weapons.md` and `docs/formats/statescript.md`.

---

## 5. Material keys for the blocked assets

**Question.** The model -> look -> texture chain works where the keyring has
the keys. Several hero body materials and many map props are blocked by the
`0x008` keyring wall.

**What would settle it.** A capture of the runtime material structure (the
on-disk shape is not what runs) or a key for the blocked assets.

**Next anchor.** `docs/formats/materials.md`.

---

## 6. The per-frame camera look application

**Question.** The input tick writes look deltas to entity `+0x1294`/`+0x1298`;
the camera basis at `+0x1310`/`+0x1320` is written by `FUN_7ff7c54a9720` from a
quaternion. The function that turns the look deltas into that quaternion is
not located, even though the look code pages are warm. The FOV, near/far and
projection are settled: 103 horizontal -> **70.53 vertical** at 16:9,
`0.1/16384` and `0.01/1638.4`, with the x double negative cancelled.

**What is known.** The input tick page `0x7ff7c5301000` is 3733/4096 nonzero.
The five interpolation-inverse vars and four speed rates have no direct reader
outside registration, so the consumer is reached indirectly or through a
copied settings block. The camera component carries read pitch limits of
**-89/+89 deg** at `+0x94`/`+0x98`, but no consumer is located.

**What would settle it.** Trace the warm indirect dispatch / animation-event
path from the input tick to the camera basis, or a live trace of the look
accumulators and the camera basis.

**Next anchor.** `docs/formats/camera.md` (input to camera) and
`docs/formats/input-camera-audio.md`.

---

## 7. The steep-slide rule

**Question.** Movement is measured and gated by `trace_diff`; the steep-slope
slide is not modelled.

**What would settle it.** A keyed trace on a steep slope, read with
`dev/ctrl_probe.py` / `dev/trace_probe.py`.

**Next anchor.** `docs/formats/character-controller.md`.

---

## 7b. The mover velocity-ramp writer (FIT-MOV-01)

**Question.** `MoverAdvance` (`0x140ca7f30`, base vtable
`0x1431101e8` slot 15) integrates a **static local offset** at
`+0xc0..0xcc` with the wish/transform; `MoverInvoker` (`0x1407ffc50`) stores
the result. Which function turns the wish vector (`+0x1140`) and transform
(`+0x1a90`) into that per-frame step?

**What is known.** The component is `RegisterMoverComponent` (`0x14017a680`,
hash `0x6EFDBD73`, id `0x26`); its interface vtables are plaintext in the
current build. The advance's plain-body formula is
`out_i = (a * X_i + pos_i) * b + off_i`; the character path is the collision
sweep. The advance is the integrator, not the ramp. The install's `.pdata` is
plaintext and complete; Ghidra's function boundaries are fragmented in
places, so use Ghidra's entry points (`docs/ghidra/DumpFunctionRanges.java`)
for decompilation, not a `.pdata` begin alone. `dev/field_index.py` is the
struct-aware finder: it separates the mover component, the character body and
dmBody, and follows biased bases. The `+0xc0` stores in the complete image are
only constructors and bulk copies, so `+0xc0` is a **static local offset**, not
a stored velocity; the per-frame motion is the wish vector and transform. The
state modifier's `param_1 + 0xc0` is byte `+0x600` (the decompiler's
`longlong *` type), so the state table's field IDs must be mapped to byte
offsets before it is read.

**What would settle it.** Read the state modifier table (`[body+0x10e]`,
0x38-byte records) and/or the movement statescript graph with `ow-tank`'s
graph/parameter tools; determine whether the acceleration/friction is a
per-field add/mul modifier, a graph value, or a force into `dmBody.force`
(`+0x130`). Then add a `mover_ramp` entry to
`crates/ow-sim/src/bin/verify_probe.rs` and a state unit, or gate the
integrated behaviour with the keyed traces. Only then remove the `FIT-MOV-01`
marker and its ledger row.

**Next anchor.** `docs/formats/character-controller.md`, the scaffolder
template `dev/verify_harness/units/fn-MoverAdvance.json`, and
`dev/field_index.py`.

---

## 8. The v1 STU path

**Question.** v1 is implemented and synthetic-tested only; no v1 asset has been
read from an install.

**What would settle it.** A v1 asset in an install.

**Next anchor.** `docs/formats/stu.md`.

---

## 9. The replay frame payload

**Question.** The `phl`/`prp` container, header and roster are read; the frame
payload is a Zstd stream that is not decoded.

**Next anchor.** `docs/formats/replay.md`.

---

## 10. Wwise audio decode

**Question.** Wwise metadata and WEM headers are read; the audio itself is
not decoded.

**Next anchor.** `docs/formats/sound.md`.

---

## 11. Bots and abilities

**Question.** They are test stand-ins: boxes and markers, no client AI.

**Next anchor.** `docs/systems-coverage.md` (rows 9-10) and the statescript
docs.

---

## 12. The two standing 2.25.0.0 images disagree

**Question.** `module_v2full.bin` (the docs' Ghidra basis) and
`module_after_diff_v2.bin` (the 17-run warm merge, which carries the only
readable `.pdata`) are both declared 2.25.0.0, yet a page census over their
10,582 shared nonzero pages (2026-10-21) reads **5,961 equal and 4,621
different** - at least one merge placed captured pages wrongly, or the two
capture sets decode pages differently. The docs' v2b addresses answer to
`module_v2full.bin` only (verified: the dmWorld::Create prologue the deliverable
describes sits at `0x7ff7c7a4cd70` in v2full and not where after_diff holds
bytes), so byte matching for the docs frame now reads from `module_v2full.bin`
(`dev/naming_seed.py`), and numbers derived from
`module_after_diff_v2.bin`-besides the ones already decompiled-verified - are
suspect until explained.

**What would settle it.** Rebuild both images from the same runs with
provenance per page attached, or diff run-by-run to find which side carries
foreign bytes.

**Next anchor.** The merge scripts behind `module_after_diff_v2.bin` and
`module_v2full.bin` in `D:/ow-dumps`; `dev/live_page_diff.py` /
`dev/warm_poll.py --all-regions` output files list which run produced each
page.

---

## 13. CASC: the decode gaps left after the client check

**Question.** Which parts of the client's container reader are still not
implemented, and does any of them matter for this install?

**What is known (2026-10-25).** Salsa20, the BLTE frame header, the `N`/`Z`/`E`
block paths, the bucket entry layout, the 30-byte header and the encoding
header/section order were checked against the client's own functions and now
match (`docs/formats/casc.md`). Still different: LZ4 mode `4`
(`BlteBlockReadLz4`, `0x1424d6e50`), frame type `0x10`
(`BlteFrameHeaderParse`, `0x1424a90e0`), ARC4 `A` and frame `F` chunks, the
dynamic index's section/entry hashes (`0x14250fcf0` verifies both), and
`.flavor.info` row selection (`BuildInfoSelectRow`, `0x1424aec30`). A
20,000-record sample found no `4`, `0x10`, `A` or `F`.

**What would settle it.** Implement the hash first: it is the only gap that can
change the answer for a *present* install (a corrupt index). LZ4/`0x10`/ARC4/F
need a build that uses them; a scan of every first-chunk mode across all 1.1 M
records would say whether this install ever does. `.flavor.info` needs a
multi-branch install to test; on the one-row reference install the choice is
already the same.

**Next anchor.** `crates/ow-casc/src/index.rs`, `blte.rs`, `encoding.rs`;
`docs/formats/casc.md`; the named functions above in `Overwatch.exe`.

---

---

## How an entry is closed

1. The answer has a number or an address, and the address names its capture.
2. The finding is written in the relevant `docs/formats/<system>.md`.
3. If it replaces a fit, the row in `fitted-ledger.md` is removed and the code
   says so; if it is still a fit, the row stays and gains the evidence.
4. The entry is removed from this list, and `STATUS-handoff.md` is updated.

# Fitted, assumed and stand-in implementations

*Living ledger of every place where the simulation behaves like the client by
inference instead of a read. `docs/provenance.md` defines the evidence classes;
this file is the inverse index — what is **not** tier 1/2, and why.*

**How to use it.** Add a row in the same change that introduces a fit. When a
capture or a decompile settles a row, change its status here and in the
`formats/` doc it points at. A row with no source is a bug in this ledger.

**Every row has an id** (`FIT-CAM-01`, `FIT-COM-04`, ...) and the same id
stands next to the code that carries the fit, so a reader of the code can see
whether a number is read or chosen, and a reader of this file can find the
code. `cargo xtask fit-check` fails when the two sides disagree: a fit in the
code with no row here, or a row here whose code lost its marker.

Statuses used below:

- **fitted** — the form or curve was chosen to match measurements or an
  eye-check; the client's own expression is unread.
- **assumed** — a rule or constant with no client source (provenance
  *assumed*).
- **tier 3** — the client's stage is deliberately declared unrecoverable
  (`docs/rewrite-plan.md`), and the simulation uses a measured replacement.
- **stand-in** — harness-only geometry or presentation; not claimed as client
  behaviour.
- **supplied** — an explicit caller input; not a fit and not data dressed up
  as a reading.

## Camera and projection

| Feature | What we implement | Status | Source / why | Where | Risk and next anchor |
|---|---|---|---|---|---|
| View-model drag spring (`FIT-CAM-01`) | A **zero-rest offset** spring driven by the camera's angular velocity: `off″ = −k·off − c·off′ + gain·ω`, applied as a yaw/pitch offset to the view model. Constants `k=64`, `c=16`, `gain=-1`. | fitted (wrong form) | The component triple is read, but **no readable consumer uses it**. The client's per-frame FP spring is `FUN_7ff7c53ab620` on anim `+0x1350` toward `+0x1370`: coefficients `0.235` / `0.48`, k ramp `600.0/s`, settle threshold `0.004 m`. | `crates/ow-view/src/play.rs` — `vm_spring_step`, the redraw spring block | The viewer's form is not the client's. Replace it once the `+0x1350` input/space is read; the 64/16/-1 form should not be presented as the client's spring. |
| Camera look filter (`FIT-CAM-02`) | Exponential follow of the rendered camera toward the mover's exact yaw/pitch, time constant `camera.lookInterpolationSpeedInverse` = 0.1 s. Opt-in via `--cam-look-inverse`; default `0.0` (exact). | fitted | The default **0.1** is read (v2b `0x7ff7c86aeec0`); the consumer is cold. The earlier whole-view lag was reverted in session 19g, so it is opt-in. Movement and fire keep the exact controller angles. | `crates/ow-view/src/play.rs` — look block before the view-model spring | If the eye-check accepts it, flip the default to 0.1. If not, leave it as a comparison knob. |
| First-person pass depth clear (`FIT-CAM-03`) | The view model and its weapons are drawn in a second render pass with the client's FP projection; depth is cleared between the world and FP passes. | fitted | The separate pass and `camera.FirstPersonNearZ/FarZ` = 0.01/1638.4 m are read; clearing depth is the standard way to keep a view model out of wall geometry, but the client's exact pass boundary/depth policy is unread. | `crates/ow-view/src/main.rs` — `Renderer::render`, the "first person" pass | The client may use a depth range or a separate target instead of a clear; the visible result is the same for arms-in-front. Verify against a live frame if the page is ever captured. |
| Eye height (`FIT-CAM-05`) | `EYE_HEIGHT = 1.75` m, used for the camera and the view-model anchor. | assumed | The traced node is the feet; the client's own eye height was not measured. The FP rig's neck is 1.7104 m (+4 cm to the eye), which is a consistency check, not a source. | `crates/ow-scene/src/lib.rs` — `EYE_HEIGHT` | Crouch/eye height may differ. A live camera-position trace would settle it. |
| First-person pitch (`FIT-CAM-06`) | The whole FP rig is rotated by the camera pitch. | fitted | `camera.FirstPersonAnimationWeight` = 1.0 is read; the per-bone pitch distribution is not (the FP animation path is separate). | `crates/ow-scene/src/lib.rs` — `view_model_matrix`; `crates/ow-view/src/play.rs` | Whole-rig pitch is exact only for a rig with no spine/arm distribution. The client's per-bone weights are the next read if the FP animation graph is decoded. |
| First-person pitch clamp (`FIT-CAM-07`) | Look pitch is clamped to +/-1.45 rad (83.1 deg). | fitted | The camera component constructor (`module_v21_clean.bin` `0x7ff7c6dc3340`) writes **-89/+89 deg** in radians to component `+0x94`/`+0x98` from the int globals `0x7ff7c87c0e70`/`0x7ff7c87c0e74`. The look integrator that consumes those fields is not in the warm pages, so the clamp remains the eye-check fit. | `crates/ow-view/src/play.rs` — `PlayState::frame` | The client may stop at +/-89 deg. Reading the cold look integrator, or tracing a look to the stop, settles the value. |
| Mouse look scale (`FIT-CAM-08`) | `0.00172785` rad per raw mouse count; `--sens` overrides. | fitted (scale) | `K` is **read** from the on-disk executable's `.rdata` at `0x7ff7c7ccc4b0` (`0x3C3CBADB` = `0.011519` rad/count); the merged image zeroed that `.rdata` page. The default input scale is `0.15` (old-build value), giving `0.00172785`. | `crates/ow-view/src/play.rs` — `PlayApp.sensitivity` | The `0.15` scale is the remaining fit; a live read of `input+0x2238` at default settings settles it. Winit counts may not equal the client's raw counts. |

## Movement

| Feature | What we implement | Status | Source / why | Where | Risk and next anchor |
|---|---|---|---|---|---|
| Mover arithmetic (`FIT-MOV-01`) | Per-frame (not fixed-tick) integration: accelerate toward the wish speed at `ACCEL=121.4`, shed speed at `FRICTION=72.6`, gravity `17.37`, jump `5.33`, airborne `AIR_ACCEL=18.5`, slot speeds from the per-hero mover, slope walk along the surface. | open read + fitted | The **advance and invoker are now read** in the current build: `MoverAdvance` at `0x140ca7f30` (base vtable `0x1431101e8` slot 15) integrates the velocity-like quad at `+0xc0..0xcc`; `MoverInvoker` at `0x1407ffc50` stores the result and dispatches contact normals. The component registration is `FUN_14017a680` (hash `0x6EFDBD73`, id `0x26`). The **control law that writes `+0xc0` is still unlocated**. The current image is complete and plaintext, so this is a discovery gap, not an obfuscation claim; the rates remain fits to the client keyed traces until the writer is read. | `crates/ow-sim/src/movement.rs`; anchor addresses in `docs/formats/character-controller.md` | The fit uncertainty is ±0.7 on `ACCEL`; crouch, air and slope transitions are measured on one/few traces. The trace gate (`trace_diff keys --by-state`) is the check. Next: locate the writer of `+0xc0`, add a state unit, and only then remove this row. |
| Steep slide (`FIT-MOV-02`) | Not modelled: the steep-slide case at 4.6895 m/s is left out. | tier 3 | It is not a slot speed and the rule was not recovered. | `crates/ow-sim/src/movement.rs` — header, "steep-slide case" | Any steep-slope movement is the wrong speed. Needs a keyed steep-slope trace. |
| Crouch speed label (`FIT-MOV-03`) | The per-hero crouch field is read and used; the published "3.0 for all" is treated as mostly confirmed. | read value, provisional label | A few heroes read 2.0/2.5 in the dump; the field identity is named mostly-confirmed, not proven. | `crates/ow-sim/src/movement.rs` — `FIELD_CROUCH` | If the label is wrong, crouch uses another scalar. A live crouch trace on an exception hero settles it. |
| Player collision (`FIT-MOV-04`) | A stand-in: one ground plane plus axis-aligned boxes; capsule radius `0.4 m`, step `0.55 m`. The map mesh is not used as collision. | stand-in + assumed | The client's character step/ledge/slope collision is unread; the map is visual geometry, not a collision mesh. The capsule radius and step are harness assumptions. | `crates/ow-view/src/play.rs` — `Sandbox` | Movement on real map geometry (ledges, slopes, walls, stairs) is not the client's. The client's collision/step code is the next anchor. |
| Projectiles and shots vs. the map (`FIT-MOV-05`) | Combatants are hit by capsules; map chunks are not collision. | stand-in | The harness has no world-collision representation for the map. | `crates/ow-view/src/play.rs`; `crates/ow-sim/src/lib.rs` | Shots/projectiles pass through walls/floor. Correct only after the collision work. |

## Combat and damage

| Feature | What we implement | Status | Source / why | Where | Risk and next anchor |
|---|---|---|---|---|---|
| Damage order (`FIT-COM-01`) | Shields, then armour, then health. | assumed | No install reading; both orders are plausible in the genre. | `crates/ow-sim/src/rules.rs` — `DamageOrder::default` | Wrong pool order changes time-to-kill. A statescript/health-component read is the next anchor. |
| Armour mitigation (`FIT-COM-02`) | For hits below 10: reduce by 30%; larger hits: flat 5. | assumed | The two behaviours are both seen in the game's history; neither is verified here. | `crates/ow-sim/src/rules.rs` — `Rules::default`, `ArmourMitigation` | Numbers are provisional. Read the damage/armour path in the client. |
| Shield regeneration (`FIT-COM-03`) | Shields are a plain pool and do not regenerate during a fight. | assumed | Regeneration timing is not established. | `crates/ow-sim/src/rules.rs` | Long fights understate shields. | 
| Per-shot damage (`FIT-COM-04`) | Supplied by `--damage`. | supplied | The damage value could not be extracted; it is an explicit input, not a guessed literal. | `crates/ow-view/src/play.rs` — `Supplied`; `ow-sim` weapon setup | Not a fit, but the user must supply a number. The install's modify-health literals are candidates, not labelled. |
| Spread, falloff, ammo (`FIT-COM-05`) | Defaults: spread 0, no falloff, no ammo limit. | unread defaults | The files declare none of these for the harness's volley; they are explicitly left unread. | `crates/ow-view/src/play.rs` — `hero_weapon` | DPS/range behaviour is optimistic. Needs the weapon's graph/fields. |
| Cast-length interpretation (`FIT-COM-06`) | A volley with no speed but a cast length is hitscan with that range; with neither, 200 m. | assumed | A named `projectileSpeed` outranks the `?`-assumed `castLength?`; the fallback is the harness default. | `crates/ow-view/src/play.rs` — `volley_mode` | A cast length may mean something else. The volley figures are printed; the next read is the fire mode's real field. |
| Bots (`FIT-COM-07`) | Static target dummies drawn as boxes/markers; no client AI, abilities, or hero movement. | stand-in | The simulation has no bot controller; hit shapes are `0.8 × 1.8 × 0.8` boxes until the real hit shapes are extracted. | `crates/ow-game/src/sandbox.rs` — `bot_box`; `crates/ow-view/src/play.rs` markers | Combat tests are against boxes, not heroes. Hit-shape extraction is the next anchor. |

## Animation and view model

| Feature | What we implement | Status | Source / why | Where | Risk and next anchor |
|---|---|---|---|---|---|
| Playback source (fallback) (`FIT-ANI-01`) | Heroes without a blend set/tree, or an explicit `--anim <guid>`, still play one clip (`Anim::Bones` / `Anim::Blend`). The live path is the blend-tree row below. | read path (harness fallback) | `--anim auto` evaluates the real tree; the single-clip path remains for sets without a tree and for clip diagnostics. | `crates/ow-view/src/play.rs` - `Anim`, `resolve_idle_clip` | none |
| First-person camera framing (`FIT-ANI-02`) | The FP pass's camera sits `--fp-cam-drop` (default 0.35 m) below the world eye and `--fp-cam-back` (0.30 m) behind it **in the camera's own up/forward frame**; the world camera is unchanged. | fitted (values; frame is read) | The measured idle hands sit ~0.38 m below the eye and ~0.09 m forward, so a level eye camera puts them under the screen. The FP camera path is now read: `UpdateFirstPersonCameraPose` (`module_v21_clean.bin` `0x7ff7c56b7b90`) builds a pose descriptor from the FP animation entity's basis/position (`+0x1310`/`+0x1320`/`+0x1350`), and `ApplyFirstPersonCamera` (`0x7ff7c568c5b0`) applies that pose to the world camera. The client has no fixed drop/back; the viewer's drop/back is a stand-in because the read animation data does not yet supply that camera bone. The first cut used world Y for the drop, which made the hands vanish looking down and fly away looking up; `Camera::up()` fixes the frame. | `crates/ow-view/src/main.rs` — `fp_camera_drop`/`fp_camera_back`, `Camera::up`; `play.rs` flags | The exact drop/back is an eye-check fit; tune the two flags. A live FP-pass capture would settle it. |
| First-person camera/view-model parameters (`FIT-ANI-03`) | The viewer prints `STUFirstPersonComponent`'s per-hero values and exposes `--fp-fov` (FP-pass vertical FOV) and `--fp-eye` (FP camera height, mapped to the old drop). The FP FOV defaults to the world FOV. | FOV read; eye height/pose fitted | The component values are read (`fpcomp_probe`): hero 3 = 50 / 1.25 / 0.875 / 0.8 with m_position (0.1375, 0.23375, 0.20625). The FP FOV is **not** the `50` scalar: `UpdateFirstPersonCameraPose` calls `FUN_7ff7c5700f50`, which reads `GetHorizontalFov` (103), halves it to radians (`0.00872665`), applies `tan`, the 9/16 aspect factor and `atan`, and doubles: **70.53 deg vertical at 16:9**, the same as the world FOV. `ApplyFirstPersonCamera` sets that on the world camera; the `60.0` angle is a fallback. | `crates/ow-view/src/play.rs` - `print_first_person_component`, `--fp-fov`, `--fp-eye`; `crates/ow-view/src/main.rs` - `fp_fov`; `crates/ow-tank/examples/fpcomp_probe.rs` | The FP FOV need not be fitted now; the remaining fit is the camera **pose** (the FP animation camera bone and drop/back), not the FOV. The 1.25-1.55 eye-height candidate is still unread. |
| Weapon attachment bones (`FIT-ANI-04`) | The two weapon instances use the selected **FP skeleton's 44/74** and the hardpoint local by default; `--weapon-fold 1` keeps the old character-c71 source for A/B. | read source; reference retarget missing | The client attaches the pistols to FP hardpoints `0x0dc:0a/0b` on **FP weapon bones 44/74**; pose is `FP bone world * hardpoint local`, using the selected FP skeleton (folded char 284/285 chain), not the character rig. `retarget_probe` c71: 44 `(0.11,1.44,0.35)` fwd `+x`, 74 `(-0.14,1.51,0.35)` fwd `-x`. | `crates/ow-view/src/play.rs` - weapon palette/`char_weapon_world`; `crates/ow-scene/src/lib.rs` - `character_skin`; `crates/ow-tank/examples/retarget_probe.rs` | The source is now right; the omitted reference retarget still makes the animated FP local approximate. `--weapon-fold 1` compares the old character-c71 path. |
| First-person blend tree (`FIT-ANI-05`) | `--anim auto` evaluates the hero's real `STUAnimBlendTree`: BranchByCategory picks the base Animation by category asset (5 → c71, 9 → c50), LayerChild attaches the Pose2d layers (d4/bc), and the client's per-child sampler gate is applied. Single-clip resolution remains the fallback for heroes without a tree. | read structure + fitted driver math, **viewer channel mismatch** | **Read** (`ow-tank/src/blendtree.rs`, `blendtree_probe`): the node graph, clip indices/slot ids, the item's 19 slot values, and `AnimSamplerRule` (`0x7ff7c6e84610`). The rule copies **child 0 unconditionally**, then for each later child replaces each present rotation/translation/scale lane only when that child's weight is `> 0.5` (no lerp), while four scalars lerp at any weight; absent lanes keep the previous value. **Fitted**: Pose2d driver inputs are x=y=0, the LayerChild weight is the file default 0.0, the Pose2d spatial blend is not modelled, LayerAdditive is skipped. **Viewer**: one-link layer bones now replace rotation and translation independently; dropped-ancestor chains fall back to the composed pair and scalar lanes are not decoded. | `crates/ow-tank/src/blendtree.rs`, `crates/ow-view/src/anim_tree.rs`, `play.rs` - `build_tree_anim` | At the file defaults the idle pose is c71. The driver VM (`0x048...`) and the LayerChild weight writer are still unread; before those, fix the viewer's per-channel representation and non-loop clamp. Category 5 as default is a guess; `--anim-category 9` compares c50. |
| FP blend-item order and playback (`FIT-ANI-06`) | The true map is read (`STUAnimBlendTreeSet_BlendTreeItem` 0xDDD18945 → `m_F6E6D4B1` clips + `m_274F833F` slot/weight), and item 3's `m_C0214513` is the `0x0F8` `STUAnimBlendTree`. The tree is now evaluated (row above); the single-clip path is the fallback. | read mapping + fitted driver math | `blendset_probe` prints the items, clips, weights and slots; `blendtree_probe` prints the tree node by node. Item 3 is the idle group (c71/c50/d4/bc/680) with 19 floats, a vec2 `(-0.4, 0.4)` range and three u8 flags. `m_retargetParams` is absent (defaults 0.0/1.0) on every Tracer item. | `crates/ow-view/src/play.rs` - `read_blend_set` / `resolve_idle_clip` / `build_tree_anim`; examples `blendset_probe.rs` + `blendtree_probe.rs` | The driver evaluation and retarget remain unread; do not fit the curve beyond the documented knobs. |
| Animation-to-skin join (`FIT-ANI-07`) | Records pair to skin bones by bone id; records whose ids are not on the skin are dropped. | read join + fitted extras | The id join is measured (mean error 0.0069 m, 51/71 within 5 mm); the surplus ids live on neither rig. | `crates/ow-tank/src/clip.rs`; `crates/ow-view/src/play.rs` | Dropped ids mean some authored motion is missing. The surplus-id owner is open in `docs/formats/animation.md`. |
| Reference-skeleton retarget (`FIT-ANI-08`) | The clip's P lane is applied directly as the FP model local; the reference->model stage is omitted. | omitted | The client registers `anim.EnableProxyRetarget` and `anim.EnableFinalRetarget` with descriptions for reference-skeleton retargeting; the card P is the reference local. The viewer's direct application stretches the shoulders by the source rig's 2.395x. | `crates/ow-view/src/anim_tree.rs` - `sample_chain`; `play.rs` - `palette_full` | The retarget body is not in the readable pages. Read it before changing the P-lane application; do not invent a rest matrix. |

## Presentation stand-ins (not behaviour)

| Feature | What we implement | Status | Source / why | Where | Risk |
|---|---|---|---|---|---|
| Materials/textures (`FIT-PRE-01`) | Readable material joins are textured; unreadable/ambiguous ones fall back to a white placeholder (UV checker under `--uv-check`). | stand-in | The keys for some materials are absent from the install/captures; the fallback is deliberate. | `crates/ow-scene/src/lib.rs` | Visual only; a surface may be untextured by design, not a parsing failure. |
| Shading/lighting (`FIT-PRE-02`) | An 89-line WGSL key/fill/sky model; the installed lighting chunks are parsed but unused. | stand-in | No client shading model was recovered. | `crates/ow-view/src/scene.wgsl`; `docs/formats/rendering.md` | Visual only. |
| Bots/projectiles (`FIT-PRE-03`) | Drawn as boxes/markers. | stand-in | No hero models/animations for bots; projectiles are markers. | `crates/ow-view/src/play.rs` | Visual only. |

## Recently confirmed — no longer fits

- **World/base FOV:** `HorizontalFOV` **103.0°** read from the settings
  registration (`0x7ff7c51dd6a8` → `0x7ff7c8f83db8`); vertical derived from
  the aspect.
- **First-person near/far:** **0.01 / 1638.4 m** read from
  `camera.FirstPersonNearZ/FarZ`.
- **First-person animation weights:** **1.0 / 1.0** and
  `ShowFirstPersonAnimationOffset` **0** read; the camera stays exact, so no
  fit is needed there.
- **First-person static offsets:** every `firstPerson.Offset*` /
  `WeaponviewOffset*` default is **0**, matching the harness's camera-relative
  placement.
- **Drag constants:** 64/16/−1 are read; only the spring *form* remains in the
  table above.

## Known gaps (not fitted on purpose)

These are visible missing behaviour, not silent guesses:

- **arm spread and the visible upper-arm cut:** both follow the
  character-authored pose and the missing FP retarget; the current FP camera
  drop/back and hook skip are temporary fits, and no further visual tuning
  is applied until the retarget / FP-pass camera is read;
- camera shake, first-person damage feedback, camera roll target, and the 3P
  camera (data names read, code not implemented);
- world camera near/far and the per-frame camera-update consumer (cold);
- real map collision and character step/ledge/slope handling;
- bot AI and abilities; reload/ammo; damage/armour rules;
- Wwise audio (middleware call path, not a DSP reimplementation).

# Fitted, assumed and stand-in implementations

*Living ledger of every place where the simulation behaves like the client by
inference instead of a read. `docs/provenance.md` defines the evidence classes;
this file is the inverse index — what is **not** tier 1/2, and why.*

**How to use it.** Add a row in the same change that introduces a fit. When a
capture or a decompile settles a row, change its status here and in the
`formats/` doc it points at. A row with no source is a bug in this ledger.

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
| View-model drag spring | A **zero-rest offset** spring driven by the camera's angular velocity: `off″ = −k·off − c·off′ + gain·ω`, applied as a yaw/pitch offset to the view model. Constants `k=64`, `c=16`, `gain=-1`. | fitted | Constants are **read** from `STUFirstPersonComponent.m_F486DCF2` (`STU_6AE5A305`, 64/16/−1, v2b hero entity). The form is inferred: an absolute-angle follower with the same constants trails `c·ω/k` = 72° at 5 rad/s, so it cannot be the client's drag. The consumer page is cold. | `crates/ow-view/src/play.rs` — `vm_spring_step`, the redraw spring block | Gain and units are the uncertainty. Eye-check; `--vm-g`; a live capture of the consumer page is the exact fix. |
| Camera look filter | Exponential follow of the rendered camera toward the mover's exact yaw/pitch, time constant `camera.lookInterpolationSpeedInverse` = 0.1 s. Opt-in via `--cam-look-inverse`; default `0.0` (exact). | fitted | The default **0.1** is read (v2b `0x7ff7c86aeec0`); the consumer is cold. The earlier whole-view lag was reverted in session 19g, so it is opt-in. Movement and fire keep the exact controller angles. | `crates/ow-view/src/play.rs` — look block before the view-model spring | If the eye-check accepts it, flip the default to 0.1. If not, leave it as a comparison knob. |
| First-person pass depth clear | The view model and its weapons are drawn in a second render pass with the client's FP projection; depth is cleared between the world and FP passes. | fitted | The separate pass and `camera.FirstPersonNearZ/FarZ` = 0.01/1638.4 m are read; clearing depth is the standard way to keep a view model out of wall geometry, but the client's exact pass boundary/depth policy is unread. | `crates/ow-view/src/main.rs` — `Renderer::render`, the "first person" pass | The client may use a depth range or a separate target instead of a clear; the visible result is the same for arms-in-front. Verify against a live frame if the page is ever captured. |
| World camera near/far | `0.01 / 8000.0` for the world pass. | stand-in | No world near/far variable or projection code is in the captures; only the FP pair is read. | `crates/ow-view/src/main.rs` — `Renderer::render` | Distant/near world clipping can differ from the client. Needs the renderer/world-projection page. |
| Eye height | `EYE_HEIGHT = 1.75` m, used for the camera and the view-model anchor. | assumed | The traced node is the feet; the client's own eye height was not measured. The FP rig's neck is 1.7104 m (+4 cm to the eye), which is a consistency check, not a source. | `crates/ow-scene/src/lib.rs` — `EYE_HEIGHT` | Crouch/eye height may differ. A live camera-position trace would settle it. |
| First-person pitch | The whole FP rig is rotated by the camera pitch. | fitted | `camera.FirstPersonAnimationWeight` = 1.0 is read; the per-bone pitch distribution is not (the FP animation path is separate). | `crates/ow-scene/src/lib.rs` — `view_model_matrix`; `crates/ow-view/src/play.rs` | Whole-rig pitch is exact only for a rig with no spine/arm distribution. The client's per-bone weights are the next read if the FP animation graph is decoded. |

## Movement

| Feature | What we implement | Status | Source / why | Where | Risk and next anchor |
|---|---|---|---|---|---|
| Mover arithmetic | Per-frame (not fixed-tick) integration: accelerate toward the wish speed at `ACCEL=121.4`, shed speed at `FRICTION=72.6`, gravity `17.37`, jump `5.33`, airborne `AIR_ACCEL=18.5`, slot speeds from the per-hero mover, slope walk along the surface. | tier 3 + fitted | The control **code** is obfuscated and closed as tier 3 (`docs/formats/character-controller.md`). The rates are fits to the client's own keyed traces; the shape is the trace-measured rule, not a decompiled control law. | `crates/ow-sim/src/movement.rs` | The fit uncertainty is ±0.7 on `ACCEL`; crouch, air and slope transitions are measured on one/few traces. The trace gate (`trace_diff keys --by-state`) is the check. |
| Steep slide | Not modelled: the steep-slide case at 4.6895 m/s is left out. | tier 3 | It is not a slot speed and the rule was not recovered. | `crates/ow-sim/src/movement.rs` — header, "steep-slide case" | Any steep-slope movement is the wrong speed. Needs a keyed steep-slope trace. |
| Crouch speed label | The per-hero crouch field is read and used; the published "3.0 for all" is treated as mostly confirmed. | read value, provisional label | A few heroes read 2.0/2.5 in the dump; the field identity is named mostly-confirmed, not proven. | `crates/ow-sim/src/movement.rs` — `FIELD_CROUCH` | If the label is wrong, crouch uses another scalar. A live crouch trace on an exception hero settles it. |
| Player collision | A stand-in: one ground plane plus axis-aligned boxes; capsule radius `0.4 m`, step `0.55 m`. The map mesh is not used as collision. | stand-in + assumed | The client's character step/ledge/slope collision is unread; the map is visual geometry, not a collision mesh. The capsule radius and step are harness assumptions. | `crates/ow-view/src/play.rs` — `Sandbox` | Movement on real map geometry (ledges, slopes, walls, stairs) is not the client's. The client's collision/step code is the next anchor. |
| Projectiles and shots vs. the map | Combatants are hit by capsules; map chunks are not collision. | stand-in | The harness has no world-collision representation for the map. | `crates/ow-view/src/play.rs`; `crates/ow-sim/src/lib.rs` | Shots/projectiles pass through walls/floor. Correct only after the collision work. |

## Combat and damage

| Feature | What we implement | Status | Source / why | Where | Risk and next anchor |
|---|---|---|---|---|---|
| Damage order | Shields, then armour, then health. | assumed | No install reading; both orders are plausible in the genre. | `crates/ow-sim/src/rules.rs` — `DamageOrder::default` | Wrong pool order changes time-to-kill. A statescript/health-component read is the next anchor. |
| Armour mitigation | For hits below 10: reduce by 30%; larger hits: flat 5. | assumed | The two behaviours are both seen in the game's history; neither is verified here. | `crates/ow-sim/src/rules.rs` — `Rules::default`, `ArmourMitigation` | Numbers are provisional. Read the damage/armour path in the client. |
| Shield regeneration | Shields are a plain pool and do not regenerate during a fight. | assumed | Regeneration timing is not established. | `crates/ow-sim/src/rules.rs` | Long fights understate shields. | 
| Per-shot damage | Supplied by `--damage`. | supplied | The damage value could not be extracted; it is an explicit input, not a guessed literal. | `crates/ow-view/src/play.rs` — `Supplied`; `ow-sim` weapon setup | Not a fit, but the user must supply a number. The install's modify-health literals are candidates, not labelled. |
| Spread, falloff, ammo | Defaults: spread 0, no falloff, no ammo limit. | unread defaults | The files declare none of these for the harness's volley; they are explicitly left unread. | `crates/ow-view/src/play.rs` — `hero_weapon` | DPS/range behaviour is optimistic. Needs the weapon's graph/fields. |
| Cast-length interpretation | A volley with no speed but a cast length is hitscan with that range; with neither, 200 m. | assumed | A named `projectileSpeed` outranks the `?`-assumed `castLength?`; the fallback is the harness default. | `crates/ow-view/src/play.rs` — `volley_mode` | A cast length may mean something else. The volley figures are printed; the next read is the fire mode's real field. |
| Bots | Static target dummies drawn as boxes/markers; no client AI, abilities, or hero movement. | stand-in | The simulation has no bot controller; hit shapes are `0.8 × 1.8 × 0.8` boxes until the real hit shapes are extracted. | `crates/ow-game/src/sandbox.rs` — `bot_box`; `crates/ow-view/src/play.rs` markers | Combat tests are against boxes, not heroes. Hit-shape extraction is the next anchor. |

## Animation and view model

| Feature | What we implement | Status | Source / why | Where | Risk and next anchor |
|---|---|---|---|---|---|
| Playback source (fallback) | Heroes without a blend set/tree, or an explicit `--anim <guid>`, still play one clip (`Anim::Bones` / `Anim::Blend`). The live path is the blend-tree row below. | read path (harness fallback) | `--anim auto` evaluates the real tree; the single-clip path remains for sets without a tree and for clip diagnostics. | `crates/ow-view/src/play.rs` - `Anim`, `resolve_idle_clip` | none |
| First-person camera framing | The FP pass's camera sits `--fp-cam-drop` (default 0.35 m) below the world eye and `--fp-cam-back` (0.30 m) behind it **in the camera's own up/forward frame**; the world camera is unchanged. | fitted (values; frame is read) | The measured idle hands sit ~0.38 m below the eye and ~0.09 m forward, so a level eye camera puts them under the screen. No separate FP view matrix was found in the path followed: `FirstPersonCameraSetup` sets the FP near/far on camera objects, and the view model most likely uses the world camera plus the FP projection. The drop/back therefore compensates for the unread pose, not for a missing camera. The first cut used world Y for the drop, which made the hands vanish looking down and fly away looking up; `Camera::up()` fixes the frame. | `crates/ow-view/src/main.rs` — `fp_camera_drop`/`fp_camera_back`, `Camera::up`; `play.rs` flags | The exact drop/back is an eye-check fit; tune the two flags. A live FP-pass capture would settle it. |
| First-person camera/view-model parameters | The viewer prints `STUFirstPersonComponent`'s per-hero values and exposes `--fp-fov` (FP-pass vertical FOV) and `--fp-eye` (FP camera height, mapped to the old drop). Defaults keep the previous fit. | read data, fitted mapping | The component values are read (`fpcomp_probe`): hero 3 = 50 / 1.25 / 0.875 / 0.8 with m_position (0.1375, 0.23375, 0.20625); different heroes differ. The consumer that assigns each float is unread (no direct xref to the near/far setting cell; the component hash only appears in factory/serializer code). | `crates/ow-view/src/play.rs` - `print_first_person_component`, `--fp-fov`, `--fp-eye`; `crates/ow-view/src/main.rs` - `fp_fov`; `crates/ow-tank/examples/fpcomp_probe.rs` | Candidate readings: 45-55 = FP FOV, 1.25-1.55 = eye height. An A/B run decides; then bake the winner as the default. |
| Weapon attachment bones | The two weapon instances are posed from the **character c71** rig at 44/74 (character skin + `BlendPose` c71), then the hardpoint local and camera; the FP hands use the selected animation. | read structure + chosen frame (c71 idle) | `retarget_probe` c71: 44 `(0.11,1.44,0.35)` fwd `+x`, 74 `(-0.14,1.51,0.35)` fwd `-x` (outward); at bind the hooks read forward. The FP rig drops char 284/285 and folds the hook, so the bind fold matches but the animated FP local crosses. | `crates/ow-scene/src/lib.rs` - `character_skin`/`character_weapon_pose`; `crates/ow-view/src/play.rs` - `char_weapon_world`; `crates/ow-tank/examples/retarget_probe.rs` | The c71 source is a stand-in for non-c71 clips; use the selected character pose for the guns when non-c71. Eye-check pending. |
| First-person blend tree | `--anim auto` evaluates the hero's real `STUAnimBlendTree`: BranchByCategory picks the base Animation by category asset (5 → c71, 9 → c50), LayerChild attaches the Pose2d layers (d4/bc), and the client's per-child sampler gate is applied (vec4 lanes winner-take-all above weight 0.5). Single-clip resolution remains the fallback for heroes without a tree. | read structure + fitted driver math | **Read** (`ow-tank/src/blendtree.rs`, `blendtree_probe`): the node graph, clip indices and slot ids (`m_63DEDAC0` = 1-based clip/slot index), the item's 19 slot values (Pose2d d4 weights 0.75/0.75, bc 0.1/0.15, scales 4/14, damping 0/0.2), and the sampler rule (`FUN_7ff7c6e84610`). **Fitted**: the Pose2d driver inputs are fixed at x=y=0 (`--tree-x`/`--tree-y` are the experiment knobs) because the dataflow VM is unread; the Pose2d spatial blend is not modelled (its clip applies at the driver weight); the LayerChild weight uses the file default 0.0 (`--tree-layer` overrides) because the runtime writes it from dataflow; LayerAdditive nodes are skipped (their delta data is not decoded). | `crates/ow-tank/src/blendtree.rs`, `crates/ow-view/src/anim_tree.rs`, `play.rs` - `build_tree_anim` | At the file defaults the idle pose is the base clip (c71), i.e. identical to the old heuristic; layers move only with supplied driver inputs. Category 5 as the default is a guess (the category dataflow is unread); `--anim-category 9` compares c50. Next anchors: the driver VM (`0x048...07/08/18`) and the LayerChild weight writer. |
| FP blend-item order and playback | The true map is read (`STUAnimBlendTreeSet_BlendTreeItem` 0xDDD18945 → `m_F6E6D4B1` clips + `m_274F833F` slot/weight), and item 3's `m_C0214513` is the `0x0F8` `STUAnimBlendTree`. The tree is now evaluated (row above); the single-clip path is the fallback. | read mapping + fitted driver math | `blendset_probe` prints the items, clips, weights and slots; `blendtree_probe` prints the tree node by node. Item 3 is the idle group (c71/c50/d4/bc/680) with 19 floats, a vec2 `(-0.4, 0.4)` range and three u8 flags. `m_retargetParams` is absent (defaults 0.0/1.0) on every Tracer item. | `crates/ow-view/src/play.rs` - `read_blend_set` / `resolve_idle_clip` / `build_tree_anim`; examples `blendset_probe.rs` + `blendtree_probe.rs` | The driver evaluation and retarget remain unread; do not fit the curve beyond the documented knobs. |
| Animation-to-skin join | Records pair to skin bones by bone id; records whose ids are not on the skin are dropped. | read join + fitted extras | The id join is measured (mean error 0.0069 m, 51/71 within 5 mm); the surplus ids live on neither rig. | `crates/ow-tank/src/clip.rs`; `crates/ow-view/src/play.rs` | Dropped ids mean some authored motion is missing. The surplus-id owner is open in `docs/formats/animation.md`. |

## Presentation stand-ins (not behaviour)

| Feature | What we implement | Status | Source / why | Where | Risk |
|---|---|---|---|---|---|
| Materials/textures | Readable material joins are textured; unreadable/ambiguous ones fall back to a white placeholder (UV checker under `--uv-check`). | stand-in | The keys for some materials are absent from the install/captures; the fallback is deliberate. | `crates/ow-scene/src/lib.rs` | Visual only; a surface may be untextured by design, not a parsing failure. |
| Shading/lighting | An 89-line WGSL key/fill/sky model; the installed lighting chunks are parsed but unused. | stand-in | No client shading model was recovered. | `crates/ow-view/src/scene.wgsl`; `docs/formats/rendering.md` | Visual only. |
| Bots/projectiles | Drawn as boxes/markers. | stand-in | No hero models/animations for bots; projectiles are markers. | `crates/ow-view/src/play.rs` | Visual only. |

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

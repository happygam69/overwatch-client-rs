# The movement and combat rewrite plan

**Goal, stated exactly:** reimplement the client's movement and combat
simulation in Rust — a pure, deterministic core in `ow-sim` driven by a Bevy
shell — so that, fed an identical input timeline, its per-tick state matches
the real client's **bit for bit**, and prove it by diffing against traces
taken from the operator's own installed copy.

This document sequences that work. The whole-project accounting (assets,
renderer, maps) stays in [`roadmap.md`](roadmap.md); the capture-side findings
that feed this plan are in [`history/systems-plan.md`](history/systems-plan.md); the method
rules are [`provenance.md`](provenance.md)'s. Everything below is against
build `2.25.0.0.154088`, with the per-capture ASLR deltas already recorded in
`history/systems-plan.md` (range→exec `+0x1096A0000`, range→v2b `+0x17AEC0000`,
v2b→exec `0x71820000`).

## What "1 to 1" verifies, concretely

Identical *code* bytes is not the target and cannot be — different language,
toolchain and compiler. The identity this project claims and verifies is
**state-trajectory identity**: for the same sequence of inputs, the f32 bit
patterns of the mover's state (position, velocity, grounded, and the combat
state) at each tick are equal to the client's. That is what "plays the same"
means when it is measurable. Three tiers, so a partial result is never
presented as a full one:

| tier | claim | verified by |
|---|---|---|
| 1 — required | each pure stage (one ramp step, one integration step, one shot) is bit-exact | unit golden vectors sampled from the client |
| 2 — the goal | a whole scripted timeline replays bit-exact (per-tick snapshots equal) | trace diff, first-divergence report |
| 3 — documented fallback | a stage proven unrecoverable is fitted empirically with bounded drift | trace diff with tolerance + a written boundary |

A stage is only downgraded to tier 3 after the decompilation route for it is
exhausted, and the M6 claim run states which stages hold which tier.

## The evidence ledger as it stands

What the rewrite can already stand on, and how each fact is known. Status
vocabulary per `provenance.md`: *decompiled* (read out of the capture's code),
*capture-data* (read out of the capture's data regions), *install* (read from
the files), *published-check* (a value shaped by an external figure — usable
to confirm, never to create), *assumed* (a placeholder the rewrite must
replace).

| fact | status | established |
|---|---|---|
| mover runtime slot map (`0x50` jump-struct start, `0x58` jump apex 0.98, `0xAC` side, `0xB0` back, `0xB4` forward, `0xC0` crouch, `0xE0` speed, `0xF0` fall terminal; size `0x130`) | decompiled + capture-data | `formats/character-controller.md` |
| jump-struct defaults bit-exact: 20 / 20 / **0.98** / 0.7 / 0.7 / 0.2 | decompiled (comparator + provider) | `history/systems-plan.md` § defaults |
| per-hero speed family (5.5 base, 6.0 trio, back = 90%, crouch ≈ 3, fall terminal 30) | install, shape-checked | `ow-sim/src/movement.rs` header |
| Domino physics schema: `dmBody` (`linearVelocity`, `gravityScale`, `linearDamping`, `characterMover`, `mass`, `invMass`, …), `dmWorld` (`m_inv_dt0`, `m_gravity`, body/fixture/contact lists, sleep/warm-start/continuous flags), Box2D lineage | capture-data (plaintext pool) | `history/systems-plan.md` § Domino route |
| 1/60 as a 4-wide float run in the physics defaults (1/50, 1/100 also present) | capture-data — **candidate, not consumer-read** | `history/systems-plan.md` |
| the dt table `{1/365, 1/60, 1/30, 1/24}` at `0x7ff75653d840`; a system tick passes **1/30** into the substep solver; the character-shaped step solves with 8/3 iterations | decompiled | `formats/domino.md` § Q2/Q4 |
| the solver chain (system tick → substep driver → island builder → island Solve → constraint dispatch) and the dmBody runtime layout verified from code (`m_bodies` +0x288, stride 0x1c0, invMass +0x168, invI +0xf0) | decompiled | `formats/domino.md` § Q2/Q4 |
| velocity integration op order `v' = ((gScale·mass·g + force) · (invMass·dt)) + v`; linear damping `clamp(1 − d·dt, 0, 1)` (not Box2D's `1/(1+dt·d)`); position `x += dt·v`; quaternion renormalize; 4 m/step displacement clamp | decompiled (assembly) | `formats/domino.md` § Q2/Q4 |
| **physics step rate: 30 Hz** (was recorded as "tick rate 60 Hz, assumed") | **decompiled** (`DAT_7ff7c7d5d84c` = 0.0333333351, the dt table's `+0x0c` entry); one step per frame, *commanded* not accumulated. This is the physics step, **not** a network/server rate — the network rate is a separate quantity and is not established here | `formats/domino.md` § Q2/Q4, `movement.rs` |
| jump impulse **5.33** m/s, fall accel **17.37** m/s² | **capture-data** (measured): impulse from the traced jump arc's first-frame displacement and discrete ascent sum; gravity from the constant 0.5613 m/s shed per frame ÷ dt. Both replaced the earlier published-figure guesses of 5.72 / 17.5. | `movement.rs`, `STATUS-handoff.md` § crouch/air/reverse |
| damage pool order + armour mitigation | **assumed** | `ow-sim/src/rules.rs` |
| hero capsule 0.4 m radius / 1.8 m height | **assumed** | `ow-sim` `Combatant::new` |
| no gameplay code references a component class/descriptor/vtable; the update must be reached through the physics body | decompiled (identity census, negative result) | `history/systems-plan.md` |

The two **assumed** rows above are the debt this plan exists to retire. A third
was retired on 2026-10-15: `ACCEL` 121.4, `FRICTION` 72.6, `AIR_ACCEL` 18.5 and
the crouch/back/terminal slots are all **capture-data** now, not published
figures. What remains unread is the mover's velocity-ramp writer. The old
tier-3 label was based on partial captures; `Overwatch.exe` is the complete,
plaintext current image, so the writer is an open read task, not an
obfuscation claim (`formats/character-controller.md` § Q3).

## The questions the rewrite still depends on

Each question with the evidence its answer needs — the working checklist, in
[`history/systems-plan.md`](history/systems-plan.md)'s sweep order and REA's evidence
discipline.

* **Q1 — dmBody/dmWorld field offsets.** Needed to read and write live bodies
  and to recognise the integrator in code. Evidence: decompile of the
  schema-reading cluster (addresses under Lane A), cross-checked against a
  live body whose fields change coherently while the hero moves.
* **Q2 — the integration step's arithmetic.** Order of gravity, force
  accumulation, damping application, velocity and position updates; the
  damping formula; whether fused multiply-adds are used. Evidence: the step
  function, located by re-running `sweep_consumers.py` with Q1's real offsets
  (the tool exists; it lacked the offsets).
* **Q3 — the character mover.** Input → desired velocity: acceleration and
  friction ramps, air control, crouch transition, diagonal composition, how
  the jump struct's six scalars are applied. Evidence: the consumer that
  reads mover slots *and* writes dmBody fields; corroborated by trace fitting.
* **Q4 — the tick.** Fixed dt value, substep count, accumulator and
  interpolation phase. Evidence: code near the 1/60 defaults run's referrer;
  trace timestamps.
* **Q5 — the volley fire loop.** How `numShotsPerSecond` (hash `0x02F3F5D4`,
  exactly one code site) times shots; shot phase and burst shape. Evidence:
  that site's function.
* **Q6 — projectile motion.** Motion types in the graphs' `projectileMotions`,
  gravity on projectiles, homing. Evidence: the consumers of
  `projectileSpeed`/`projectileLifetime` (three sites each).
* **Q7 — damage application.** Pool order, armour mitigation formula,
  headshot multiplier. Evidence: the `ModifyHealth.m_amount` consumers
  (20 sites, all reflection dispatchers — recorded); damage-event traces.
  **Data side done:** the node's literals are now readable from the install
  (`formats/weapons.md` § "The damage nodes are in the graph data after
  all", tool `ow-tank --bin params`); the weapon→node join and the operation
  semantics (the `Enum_D4BAD8E7` interpretation) are the open pieces.
* **Q8 — hit shapes.** Per-hero capsule dimensions. Evidence: per-hero
  `dmCapsule`/`dmSphere` fixtures read from the entity, or the hero data.

## Lane A — decompilation (constants and order)

The primary route for every "what exactly does it compute" question. The
standing captures answer the first targets; no new capture is needed to start.

Immediate targets (v2b addresses, in `dev/module_v2full.bin`, Ghidra base
`0x7ff7c4d20000`):

| address | what it reads | question it serves |
|---|---|---|
| `0x7ff7c5365fa5` | the 0x10-stride schema table `0x7ff7c7d201e0` | Q1 |
| `0x7ff7c536825c`, `0x7ff7c5368690` | the field-type object `0x7ff7c7d200b0` (`bcPQTransform`) | Q1 |
| `0x7ff7c53a3e3b`, `0x7ff7c54abbda` | same type object, farther out | Q1 |
| `0x7ff7c76a8050` | the schema blob start | Q1 |
| `0x7ff7c7a38c95` | the `dmBody` name record | Q1 |
| `0x7ff7c7a5b1bd` | the 1/60 defaults run | Q4 |

Operating notes, learned the hard way and kept here so they are not relearned:

* `module_v2full.bin`'s Ghidra save may not have persisted (the server timed
  out saving the 70 MB program). Re-save when the bridge responds; if it stays
  unresponsive, **flatten smaller windows instead** — e.g.
  `0x7ff7c5300000..0x7ff7c54c0000` plus the two data pages — into 2–4 MB
  programs that import, analyse and save quickly. The full-module program was
  a convenience, not a requirement.
* Check capture coverage (`space.read`) before decompiling any pointer; a
  zero-fill gap decompiles to plausible-looking garbage.
* The REA skill's provider is the second lane when the Ghidra MCP bridge is
  unresponsive: `open_binary` on the same flattened slices (raw-binary,
  x86-64, entry at the cluster page), `analyze_function` for pseudocode, and
  its evidence records keep the observation/inference/unknown split honest.
  Both lanes read the same bytes; the deltas translate any finding.
* Discipline (provenance rule 2): record **addresses and behaviour
  conclusions** in `docs/formats/`, never code text. Each fact lands as a
  Ghidra rename/plate comment the same session it is learned.

## Lane B — live traces (verification, and the fallback for constants)

The primary route for *verification*, and the fallback for any constant the
decompilation route cannot pin. Runs in parallel with Lane A from M0 on.

* `dev/live_trace.py`: read-only `OpenProcess`, samples per tick the dmBody
  fields **and the movement key state** (`GetAsyncKeyState` — no injection,
  nothing written to the target). With keys recorded, a trace is a complete
  `(input, state)` timeline and replays **without a hand-written script**:
  `trace_diff keys <trace.jsonl> [--speed X] [--edges]` and
  `ow-game --trace <trace.jsonl>` both replay it (`ow_sim::trace::
  diff_trace_keys`). The velocity response to the recorded keys is what the
  mover's ramps (Q3) are fitted from; `--edges` selects the held-vs-rising-
  edge reading of space, which Q3 leaves open, so run both. Attaching to a
  live session carries the Warden risk `roadmap.md` states — it stays the
  operator's decision, made per capture, never quietly.

  Mover instances are found by pointer identity, not shape
  (`live_trace.py mover`, matching the three vtables the component's
  constructor installs); the watch mode prints which slots move when the
  player moves, which is the other half of Q3's evidence. **Measured in the
  live session: they do not move.** 224 components, their owners and one
  pointer level out held zero changed u32 through screenshot-verified
  walking, so the mover component is configuration only and the live
  character state sits elsewhere (`formats/domino.md`, "Live-session
  findings"). The live player object is therefore *not* reachable by the
  shape searches Lane B was carrying. **Unblocked the same session:** a
  practice-range re-entry re-runs the character path (the module maps code
  lazily), a straight-walk scan then found the player transform node
  (`0x0241ddb61880`, feet position at +0xd0, yaw at +0xdc —
  `formats/domino.md`, "The player node found"), and a 19 s scripted
  recording with keys now replays through this differ:
  `FIRST DIVERGENCE sample 87 pos.x client 53.89746 model 53.98913 24030
  ulp` — the first post-key tick, i.e. the acceleration ramp. That trace
  (`D:/ow-dumps/trace/player_keyed.jsonl`) was Q3's fitting target. **Now
  measured, not fitted** (see `formats/character-controller.md` § "The mover
  integrates per frame"): the client writes once per *frame* with the frame's
  own dt, so the plateau step is exactly 5.5 x 0.032309 = 0.1777 m — the slot
  value, not a fitted 5.331 (that was the artefact of assuming a 1/30 stride);
  backpedal 4.95 = the slot; ACCEL 121.4 m/s^2 and FRICTION 72.6 m/s^2 from
  the traced ramps; the ramp is along the velocity *vector*, diagonals run at
  full speed, airborne horizontal is preserved, gravity 17.37 m/s^2 and the
  jump impulse 5.33 m/s from the traced arc, and slopes hold the slot speed
  along the surface. The gate `trace_diff keys <trace> --by-state` now
  compares the horizontal *and* the vertical per client write: no divergence
  across 101 writes on the flat trace (max step deviation 0.0018616 m) and
  across 18 on the jump trace (max vertical delta error 0.0014 m), with terrain
  screened by step and sustained-vertical tests and the coverage printed per
  segment. **The client's own expressions are not yet read**: the advance and
  invoker are read in the current build, but the writer of the velocity quad is
  not located. The current image is complete and plaintext, so this is a
  discovery gap; the numbers stand as trace-measured until the writer is found.
  Remaining open: crouch transition timing (needs one operator capture
  holding `ctrl`; all six existing captures lack it) and the steep-slope slide
  (|v| = 4.6895 over a straight 24.89° descent, not a slot), which belongs to the unlocated ramp or terrain. **The mover-velocity link was searched and
  closed as a negative**: no live dmBody holds a readable velocity for the
  local character — a heap-wide search for the exact walking velocity vector,
  for speed/displacement scalars (cross-checked S vs W), and for the
  Domino body layout at every exact position copy all came up empty, so the
  local mover computes its displacement per tick and persists only the
  position node (`formats/domino.md`). **Q5 moved**: the volley registrar was
  already captured and now decodes to the class's 57-field runtime table
  (`numShotsPerSecond` at +0x1C0); the live vtable's class-specific slots were
  captured (zeros in every standing capture) and are reflection/RTTI infra,
  so the fire loop runs through the statescript graph VM — the next hunt
  (`formats/weapons.md`, "The volley class, read from the captures").
* The trace differ: a small binary (in `ow-sim`, so it ships with the core)
  that replays a recorded input timeline through the simulation and compares
  per-tick snapshots bit-for-bit, reporting the **first divergence**: tick,
  field, and ULP distance. A trace that never diverges is tier 2; the report
  is how every "almost" gets root-caused instead of hand-waved.
* Golden vectors: `(input, state_in, state_out)` triples for pure stages,
  promoted to unit tests. They are small derived facts and committable; whole
  traces are capture data and stay gitignored.
* System identification: when a constant resists decompilation, fit it
  against traces and *then* the trace diff verifies the fit. A fitted value is
  written down as fitted — the ledger's status column exists for exactly that.

## The Rust side

### The core stays pure

`ow-sim` keeps its no-IO, no-engine-dependencies shape and becomes the
prediction core. Module layout the captured behaviour implies:

* `body` — the Domino-equivalent state a mover drives: transform, linear
  velocity, gravity scale, damping, sweep. Field-for-field the dmBody layout
  Q1 recovers, so the trace differ reads client and simulation symmetrically.
* `mover` — input → body control: the ramps, air control, crouch and jump
  application Q3 recovers. Replaces the current `Controller` internals.
* `combat` — volley timing, projectile motion, hit shapes, damage; absorbs
  what `rules.rs` currently assumes.
* `tick` — the fixed-step driver executing Q2's order and Q4's rate.

Every **assumed** row in the ledger above is retired by an edit that cites
the doc row that replaced it. The crate's header comment ("what is extracted
and what is assumed") shrinks accordingly and is the visible debt meter.

### Determinism rules for the core

Bit-exactness is an engineering constraint on every line of the tick:

* f32 everywhere inside the tick; the existing hand-rolled `Vec3` ops stay —
  no `glam`/Bevy math inside the simulation, only in the shell.
* Operation order copied from the decompile, expression by expression. If the
  client emits fused multiply-adds (`vfmadd*`), mirror them with explicit
  `f32::mul_add` at exactly those sites and nowhere else.
* No contraction or fast-math: Rust does not contract floating-point by
  default and the build must not add flags that do.
* No wall-clock, no frame-rate coupling, no iteration-order dependence: the
  world advances only through `tick(input)`, and entity update order is
  explicit wherever the client's order is known.

### Bevy's role and boundary

Bevy is the shell, never the simulation. A new `ow-game` crate (Bevy pinned
at import time; the version is recorded when chosen) owns the window, input,
rendering and HUD, and drives `ow-sim` from a fixed-timestep accumulator —
the sim owns the clock, Bevy renders it. Per-tick input is snapshotted before
the tick runs, which is also what makes replays and trace diffs exact.
`ow-view` remains the offline inspector until `ow-game` renders the range;
keeping the core engine-agnostic is what makes Bevy-version churn a shell
problem instead of a rewrite.

### Data flow from the install

Per-hero mover specs, volley graphs and health pools are read at runtime from
the operator's install via `ow-tank`, path-configured by environment — the
same stance as the rest of the project: the repository ships code and method,
the user's own copy supplies every number, and nothing from the install is
committed (provenance rule 1). The reimplementation is original Rust written
from behaviour; that is the line that keeps this a clean reconstruction
rather than a transcription, and it is why the docs record what the code does
instead of the code itself.

## Phases and gates

Each gate is a check that fails when the work is wrong, per `provenance.md`'s
house rule. Movement first, as the goal states; combat follows the same
shape.

* **M0 — the verification harness, deliberately first.** `live_trace.py`,
  the trace differ, the golden-vector format. *Gate: one client trace is
  captured, replayed through the current `ow-sim`, and diffed with a
  first-divergence report.* It will diverge immediately — that measurement is
  the baseline the later phases close, not a failure of M0.
* **M1 — Domino facts (Q1, Q2, Q4).** Decompile the cluster; write
  `docs/formats/domino.md` (offsets table, integration order, tick structure,
  each row citing its evidence). *Gate: the offsets cross-check against a
  live dmBody whose fields change coherently while the hero moves.*
* **M2 — movement (Q3) implemented.** The consumer decompile replaces
  `Controller`'s assumed internals. *Gate: golden vectors bit-exact, and a
  walk/strafe/crouch/jump/fall trace matches per-tick bits — or its first
  divergence is identified, root-caused and re-gated.* **Status: met for
  walk/strafe/jump/fall**, and **met for crouch and air control** as of the
  2026-10-15 captures (`player_ctrl*.jsonl`): the crouch slot, its 1-2 frame
  transitions, the airborne ramp (`AIR_ACCEL` = 18.5 m/s^2) and the reverse
  (ACCEL + FRICTION while the wish opposes) are measured and implemented, and
  the gate on `player_ctrl4.jsonl` reports max vertical delta error 0.0019 m
  over 300 s. The decompile route is still open: the advance and invoker are
  read, the writer is not; the gate is carried by measurement plus the differ
  until it is found.
* **M3 — combat facts (Q5–Q8).** Volley timing, projectile motion, hit
  shapes, damage rules, each into `formats/weapons.md` /
  `formats/character-controller.md` with evidence.
* **M4 — combat implemented.** *Gate: a firing timeline's trace matches the
  client's local simulation; damage outcomes agree with the data and observed
  events.*
* **M5 — the Bevy shell.** `ow-game` plays the sandbox: move, jump, crouch,
  fire, with a divergence HUD (the client trace as a ghost, live error
  readout). *Gate: the harness runs end-to-end from the game, not just tests.*
* **M6 — the claim run.** Scripted timelines recorded, replayed, diffed;
  results written back here with build number, capture ids, and the tier each
  stage holds. This is the section that answers "is it 1 to 1" with evidence
  instead of adjectives.

### M6 claim run: movement

**Build:** Overwatch `2.25.0.0.154088`. **Captures:** `player_fast3_keys.jsonl`
(200 Hz, 2607 samples, flat ground), `player_keyed.jsonl` (58.4 Hz, 1115
samples, the jump and the slopes), `player_ctrl4.jsonl` (200 Hz, 60002 samples,
the crouch/air/reverse schedule in a custom map), plus the module captures
named in `STATUS-handoff.md`. **Replay:** `cargo run -p ow-sim --bin trace_diff -- keys
<trace> --by-state`; **golden vectors:** `cargo test` (224 tests workspace-wide,
49 of them in `ow-sim`).

| stage | tier | evidence and written boundary |
|---|---|---|
| direction convention | **1** | the traced step direction is `(sin yaw, cos yaw)` — the flat trace's plateaus agree with its recorded yaw to 0.006°, and the sim's camera and mover share one convention (unit test) |
| travel speeds (5.5 / 4.95, diagonals at full speed) | **3** | the plateau step equals the slot x frame dt; boundary = the client's own frame-time jitter, measured ±0.6% and absorbed by the differ's band |
| ramp (ACCEL 121.4, FRICTION 72.6, vector approach) | **3** | measured from the traced step sequences; boundary = the transition frames carry sub-frame input timing no sampling can see, so the differ skips them |
| gravity 17.37, jump impulse 5.33 | **3** | the traced arc's per-frame deltas match to 0.0014 m; boundary = the take-off phase (the key lands inside a frame), so the absolute dy of a jump is compared by deltas only; a jump *from* or landing *on* a slope is unmeasured (no trace has one — the traced jump is from flat ground — and the sim takes off at the impulse along world-up, keeping the slope's share only on the take-off frame's position) |
| airborne horizontal | **3** | *no input*: the traced step holds its exact value through the fall (the ledge walk-off, and bit-identical steps in the crouch/air captures). *Direction held*: the step grows by `AIR_ACCEL * dt^2` to the slot — 17.1-19.2 m/s^2 over five runs, `AIR_ACCEL` = 18.5; boundary = the frame-phase split at each end of the run and the client's frame-time jitter |
| crouch (slot and transition) | **3** | the crouch-walk plateau reads 2.960-3.011 m/s = the 3.0 slot across five clean segments, and both transitions are the ordinary ACCEL-limited approach in 1-2 frames (5.5 -> 3.46 -> 2.97, 2.97 -> 4.16 -> 5.54 m/s); boundary = the key lands inside a frame, so the transition's exact first frame is not resolvable |
| the reverse (wish opposes the velocity) | **3** | the traced reversal crosses zero in one write at 210-231 m/s^2, where ACCEL alone would take three; implemented as friction *plus* accel while opposing (194 m/s^2); boundary = the same frame-phase limit, and the residual 2-5% step differences at reversal writes sit at the 200 Hz sampling floor |
| slope-following (|v| = the slot along the surface) | **3** | the traced 17.9° climb is 4.940 = the 4.95 slot to 0.2%, and its heading is the wish's (9.7° against the forward run's −170.5°, the exact reverse to 0.2°); boundary = the steep-slide case (|v| = 4.6895) is not reproduced, and a wish *off* the fall line would distinguish uniform scaling from a true projection (no trace has one) |
| the client's own expressions | **open read** | the advance and invoker are read; the writer of the velocity quad is not yet located. The current image is complete and plaintext, so the old "obfuscated" evidence was a capture-coverage artefact, not an unrecoverable-stage boundary; numbers stand as trace-measured until the writer is found |

**Gate results:** flat trace — no divergence across 101 compared writes (75
environment-affected and 20 input-transition writes screened; max step
deviation 0.0018616 m); jump trace — no divergence across 18 compared writes
(max vertical delta error 0.0014 m); `player_ctrl4.jsonl` (crouch/air/reverse,
300 s) — 3696 compared writes, 3600 inside the client's frame-jitter band, 2124
environment-screened (including the new stall rule), max vertical delta error
**0.0019 m**, first horizontal divergence 0.0080 m at a reversal write; shell —
determinism PASS (17 HUD lines identical across two runs).

## When traces do not match

The expected condition during M2/M4, handled by method rather than alarm:

1. **Bisect by stage.** Golden vectors isolate the first pure stage that
   diverges; fix it; repeat. Most mismatches are single stages.
2. **ULP-growth analysis.** Stages passing in isolation while the trace
   drifts means the *order* between stages differs — the classics are damping
   applied before vs after integration, and gravity before vs after the
   position update. The decompile's order is the referee.
3. **Declare the boundary.** If a stage resists recovery (a protected path, a
   server-side RNG), write the boundary down, fit empirically, mark it tier 3,
   and keep every other stage bit-exact. A honest tier map beats a vague
   "mostly matches".

## Where this stands

**M1 — Q1 is done.** The complete Domino schema was read out of the live
capture: 21 classes, 179 field records, name/offset/type each — `dmBody`
(40 fields, `characterMover` @0x1b6, `linearVelocity` @0x30) and `dmWorld`
(`m_inv_dt0` @0x198, `m_gravity` @0x1a0) included. Full record and evidence:
[`formats/domino.md`](formats/domino.md); the earlier `0x7ff7c536xxxx` lead is
closed there as narrative-viewer UI code. Also corrected: the 1/60 constant
is a density in the mass-properties computation, **not** the tick; Q4 now
reads "who writes `dmWorld.m_inv_dt0`". Next decompile step: the reference
scan over the physics code band (`0x7ff7c7a4xxxx`–`0x7ff7c7a6xxxx`) for the
world constructor, and `sweep_consumers.py` re-run with the real dmBody
offsets for the movement consumer.

**M0 — the instrument is built and self-validated.** `dev/live_trace.py`
(read-only sampler: `scan` finds dmBody-shaped bodies using the recovered
offsets; `record` writes the trace format) and `ow-sim`'s `trace` module +
`trace_diff` bin (bit-exact comparison, first-divergence report, `selftest`
that proves the instrument round-trips and locates a single-bit perturbation).
`cargo test -p ow-sim` green (49 tests). **Traces have since been captured and
run through the gate** — `D:/ow-dumps/trace/player_fast3_keys.jsonl` and
`player_keyed.jsonl`, with results in `STATUS-handoff.md` § movement. Capturing
a *new* trace remains the operator's decision with the Warden cost stated in
`roadmap.md`.

**M5 — the Bevy shell exists.** `ow-game` (Bevy 0.14, pinned in its manifest)
drives `ow-sim` from a fixed-tick accumulator with per-frame input snapshots;
`ow-scene` (new) owns the map-geometry builder both shells share, so the
sandbox stands on the install's real geometry and collider when `OW_INSTALL`
answers and on a flat test range when it does not. The replay engine moved
into `ow-sim::trace` so `ow-game --trace …` shows the divergence in its HUD:
keyed traces replay through the *gate's* differ (`diff_trace_keys_states`) and
print a line character-identical to `trace_diff keys --by-state`, scripted
traces through `diff_trace` (bit-exact, `parse_script`) — the M5 gate ("the
harness runs end-to-end from the game") is structurally met, and waits on the
same client trace as M0. The shell's own determinism check is
`ow-game --verify-determinism` (scripted run twice, every HUD line compared);
its heads-less run is `ow-game --headless <seconds>`.

**A precise capture request (from the coverage audit).** The class-specific
vtable slots of `STUStatescriptStateWeaponVolley` and of the graph runner were
read out (range `0x7ff64d1cac30` and `0x7ff64d182288`, 40 slots each). The
**volley's three behaviour slots fall in a coverage hole in every capture**:
range `0x7ff64b75b0d0`, `0x7ff64b75b7f0`, `0x7ff64b5f0900` (= exec
`0x7ff754dfb0d0`, `0x7ff754dfb7f0`, `0x7ff754c90900`) are absent from
`dumps_range`, `dumps_v2b` and `dumps_exec`, and `dumps`/`dumps_v2` are
different enough that even the registrar signature is absent there. The graph
**runner's** slots are fully covered but are introspection (descriptor getters,
registered-instance checks, an array destructor over inline containers at
`+0x90…+0x180`, stride 0x10). So the volley's fire logic — Q5 — needs exactly
those three ranges (a few KB) in a future capture, and the runner is the VM
host whose tick path is not on its own vtable.

**M1 — Q1, Q2 and Q4 answered; Q3 (the mover control) remains.** The solver
chain from the system tick down to the per-constraint solvers is read out, the
integration arithmetic is extracted in the client's own operation order
(assembly authority), damping is proven *linear* (`clamp(1−d·dt,0,1)`), and the
dt table `{1/365, 1/60, 1/30, 1/24}` is located with the systems that consume
it — the sub-step solver is called with 1/30. The mover's input→velocity ramps
are still unread, so `Controller` keeps its assumed internals for now; the
config-var anchors (`tank_TournamentModeTickRate`, `client.MaxFPS`,
`app.IgnoreDormantMode`, `player.GenerateRandomMovement`,
`debug.DisableErrorCorrection`) are the next round's entry points.

**M1 (earlier round) — the class-route hunt is narrowed, not finished.** The Domino
schema is read out (`formats/domino.md`), the mover's own interfaces are now
read out too and are *all* infrastructure — the class computes nothing — and
the volley's lone `numShotsPerSecond` site turned out to be its registrar, so
hash-immediate searches find reflection code by construction. Two new sweeps
(`dev/sweep_physics.py`, `dev/sweep_mover_body.py`) plus a float-arithmetic
filter produce a ~20-function shortlist for the integrator; the two read this
session are not it. The next round's options, in order: the shortlist; a
watchpoint-class capture on a live dmBody if the operator chooses one; and the
example the docs already carry, sampling mover state while moving.

## Scope boundaries, stated once

* **Server authority.** Movement prediction and the local fire/projectile
  simulation are client-side and fully in scope. Authoritative damage
  resolution is server-side and not in any capture: combat "1 to 1" means the
  client's own local simulation matches bit-for-bit, and damage numbers agree
  with the extracted data and observed client events.
* **New captures.** Attaching to a live session is the operator's decision
  with the Warden cost stated (`roadmap.md`). The standing captures already
  carry Lane A's first targets; Lane B's traces are the only planned new
  captures, and they are taken when the operator chooses.
* **Rendering** is out of this plan entirely — `roadmap.md`'s
  "not achievable" note on shaders stands.
* **Version drift.** A client update re-bases addresses but not method: the
  deltas are recorded per capture session, and this plan's questions are
  re-asked against the new build only where the answers changed.

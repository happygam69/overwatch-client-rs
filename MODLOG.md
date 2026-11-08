# MODLOG
Milestones only. Findings and addresses live in `docs/formats/`; current state
in `STATUS-handoff.md`; the work queue in `docs/open-questions.md`.

Rotation: this file holds the current entries. Older ones move verbatim to
`docs/history/modlog-YYYY-MM.md` (one file per month) at each rotation; no
entry is rewritten after the fact.

## 2026-11-17 - both living ledgers rotated into `docs/history/`

- **`MODLOG.md` 162 KB -> 34 KB, `STATUS-handoff.md` 104 KB -> 55 KB.** The
  ledgers had drifted from "milestones, not a diary" into append-only history:
  every clone pays for the whole tail, and the tail is what `git log` already
  holds.
- **The split is by date and by liveness, and it is verbatim.** `MODLOG.md`
  keeps the entries from 2026-11-10 on (14); the older 86 move to one file per
  month - `docs/history/modlog-2026-10.md` (6 entries, 2026-10-26 .. 10-31) and
  `docs/history/modlog-2026-11.md` (80 entries, 2026-11-01 .. 11-09), which the
  rest of November joins at the next rotation. `STATUS-handoff.md` keeps the
  live page (gate, current state, `Open`, `Next`, key files) and loses the two
  blocks that were the record of finished work: the older half of "Current
  state" and "Exact today", both to
  `docs/history/status-handoff-2026-11-17.md`.
- **Nothing was rewritten to achieve it.** 100 of 100 MODLOG entries are present
  exactly once across the hot file and the two archives; both retired STATUS
  blocks appear verbatim in the archive, and the kept blocks in the page. The
  check is mechanical (substring containment on the pre-rotation text), not an
  eyeball.
- **The rotation is now a rule, not a one-off** (`AGENTS.md`, "Keep these files
  updated"): MODLOG holds the current period and moves older entries to
  `docs/history/modlog-YYYY-MM.md`; a STATUS section that stops being live moves
  to `docs/history/status-<date>.md` with a pointer left behind. Moved text is
  never edited - a correction is a new entry.
- **The live replacements already existed**, which is why the cut is safe:
  `docs/systems-coverage.md` is the per-system state and `docs/open-questions.md`
  is the queue. The archives are cold storage; frame_check still scans them, and
  they carry no runtime-frame or out-of-extent citation.

## 2026-11-17 - Apache-2.0 ships as the whole text

- **`LICENSE-APACHE` was a 958-byte stub** that named the licence and a URL.
  Apache-2.0 section 4(a) requires a *copy of this License* to go out with any
  redistribution, so a distro package carrying that stub is legally short and
  gets bounced at review. It is now the canonical `LICENSE-2.0.txt` (201 lines,
  all 9 sections, END OF TERMS, APPENDIX), copied byte-for-byte from a
  distributed copy and cross-checked against a second one - never retyped, and
  not edited.
- **The stub's project note was redundant**, not lost: "this code only, not the
  game or its data" already stands in `README.md` (Licence) and `CREDITS.md`
  (Licence), which is where a scope caveat belongs; the licence file now holds
  only the licence.
- **Two docs claimed `--fast` is what CI runs.** It is not - CI runs the full
  `cargo xtask verify`, and the tests that read an install skip themselves there
  (`README.md`, `CONTRIBUTING.md` corrected).

## 2026-11-17 - CI runs at all, and on a clean checkout

- **The workflow never ran on `master`.** `.github/workflows/ci.yml` filtered
  `push` to `branches: [main]`; the only branch is `master`, so direct pushes
  triggered nothing and the gate only existed on pull requests of a solo repo.
  The filter is gone rather than corrected - a hardcoded name is what switched
  the gate off, and a rename must not be able to do it again.
- **`dev/pe_layout.py` failed on a runner with no install** (exit 2), which is
  every CI run. It now reports the skip and passes in `--check` mode like
  `dev/frame_check.py`, still fails when the image is present and its section
  table disagrees, and takes `OW_EXE` like the other tools.
- **Two checks could never pass on a clean checkout** - invisible until the
  workflow actually ran. `dev/make_index.py` indexed `dev/**/*.py` on disk,
  ignored analysis output included, so the committed INDEX.md carried 48 rows a
  fresh clone does not have (`--check` failed); it now indexes what git ships,
  223 rows -> 175. `collide::mesh_case0::the_python_fixture_case_hits`
  (`crates/ow-sim/src/collide.rs:4555`) unwrapped fixtures from `dev/data/read/`
  that the whitelist `.gitignore` never ships, failing `verify --fast` too; it
  reports the skip now, and that case is **not tested on CI**.
- **Green on `master`, run 42463833551.** CI checks: fmt, clippy (warnings are
  errors), the whole workspace, the tool index, the deviations table and the doc
  address frame. It does **not** check: anything that reads the install or the
  recovered image (frame entry check, PE layout, the 59 install-reading tests
  skip themselves). Proven before pushing in a clean clone
  (`git clone --local . target/ci_sim`), which is the only local model of CI.

## 2026-11-17 - the corpus indexes functions, not `.pdata` entries

- **`dev/corpus.py` schema 3: the unit is a function.** Entries are grouped by
  their transitive `CHAININFO` root (`dev/fn_extent.py` is the one walker), so a
  cold half is a piece of its function and never a function of its own. New
  `verify` pins 201,246 entries / **119,802 functions** / 81,444 chained halves /
  25,649 roots with a cold part, 0 gaps, 0 pieces claimed twice, 44,567,808
  body bytes.
- **The operator's index was migrated, not rebuilt.** From the image: 1,057
  cold-half rows dropped (their `.c` were funclet decompiles), 329 bodies
  widened to their union, their 269 stored decompiles marked `c_cut`, 513 rows
  already exact -> 842 rows. Schema is checked on open and a stale version now
  fails instead of shadowing a corrected row.
- **`make-args` is no longer destructive.** It emits chain-root unions only,
  refuses a chained start, and a whole-image range must produce exactly 119,802
  pairs. Before this it emitted 201,246 pairs, and `McpDecompile.java` creates a
  function at any entry Ghidra lacks - a sweep would have created one at each of
  the 81,444 cold halves and trimmed the parents, which is how the 251
  multi-range bodies in `docs/ghidra-image-audit.md` section 4 were made.
- **The 11-root discrepancy in §5.2 is reconciled:** the image has 25,649 chain
  roots; the audit's 25,638 are those with a Ghidra function object, and the 11
  extra carry exactly the 11,366-byte continuation difference.
- **The relationship index exists** (`dev/corpus.py index`, image bytes only):
  `fn` 119,802 functions, `edges` 1,867,334 (994,637 jcc / 648,166 call / 216,891
  jmp / 7,134 `FF 15` API calls / 506 `FF 25`) and `slot_sites` 35,835. A code
  transfer whose target is outside `.text` is dropped (410 such decodes) and its
  function is marked `opaque` - 915 functions, 0.76%, whose linear listing must
  be read in the decompiler.
- **Three documented censuses are now checks, not prose:** the chunk
  assembler's caller (`0x1420251f5` -> `0x14209fc90`), the 485 register-slot
  `+0x40` sites, and `ShapeAddToBody`'s **27 sites in 12 functions**
  (`docs/formats/shape-ingest.md`; the older "13" counted a cold half as a
  function). Each fails on a planted mutation, so the pin is not vacuous.
- **The +0x40 slot count is re-measured and the doc corrected**
  (`docs/open-questions.md`): `call_slot_scan.py`'s "505" is raw byte patterns,
  not sites; the decode-anchored count is 523 = 485 register-slot + 38 SIB
  stack-local, 473 in both sets, 12 decode-only, 19 byte-scan-only. 485 is
  pinned as `GOLDEN_SLOT40`; `rax` carries 434 of them.

## 2026-11-16 - the jump: the arc is read, the jump is shipped

- **The read law: `MoverApplyForces` (`0x140cb1990`), called once per body tick
  from `0x140ee5b41` with `(body, command, dt)`, and its copy
  `MoverGravityStage` (`0x140cb2b00`).** Gated on `body[0x10d0] & 0x80000c`
  (airborne), the stage runs `MoverAirDrag` (`0x140cb2010`) first and then, in
  the airborne arm, `body[0x1500..0x150c] += dt * g` and
  `MoverAccumulateWorldVector(body+0x10d0, dt * g)`.
- **The gravity constant is a constant.** `MoverGravityVector` (`0x140ca72d0`)
  = `(gate * body[0x610]) along body[0xc0..0xcc]`, `gate` from `0x14202fb00`,
  a decoded lookup that returns the image constant **`-10.0`**
  (`0x1431161f8`) or `0.0`; its key argument is dead. `body+0x610` is the mover
  component's `+0xe8` (`0xC2C40969`, `1.75`), so gravity is `-17.5 m/s^2`.
- **The drag** is `delta = drag_dt * -abs(gate*body[0x610]/terminal^2 * up_lane)
  * (up_lane * axis)`, terminal `body+0x614`/`body+0x618` by the sign of the up
  lane, clamped to `|local|`; its time step is `0x140ca72b0`'s
  `(f32)(i64)body[0xe8] * 1e-6`, not the caller's `dt`.
- **Shipped:** `ow_sim::air` (15 tests: the golden 22-frame trace, the drag
  clamp, the channel split, the gravity vector, the accumulator) and the walk
  wiring in `crates/ow-client/src/walk.rs` (the take-off stage, then the air
  step, then the walk's sweep and floor snap). `--walk-test` now lands the
  jump: apex `0.979` m at frame 19 (law `0.979`), air `0.667` s (law `0.669`).
- **Docs corrected on sight**: `FUN_140cb2220` is *a* per-frame producer of
  `body+0x1150`, not the only one, and it is not a gravity integrator; the
  vertical law had been recorded as unread.

## 2026-11-16 - the jump: the take-off is read, the arc is not

- **Corrected a record.** The page said `FUN_140cc1440` (`0x140cc1440`) was
  control-flow-flattened and "is the vertical stage", so which of six floats was
  the impulse was unknown. It is read end to end now, and it is the **jump /
  crouch** stage: no gravity, no fall, no landing rule.
- **Read:** `MoverUpdateFrame` (`0x140cbb260`) splits the button dword at
  `body+0xdc` into bit 6 (jump) and bit 7 (crouch); the handler chain
  `FUN_140cc1060` (`0x140cc1060`) -> `FUN_140cc1440` with its six arguments; the
  state file word `body+0x10d0` and its five transitions (`0x140ca9100`,
  `0x140ca9120`, `0x140ca9150`, `0x140ca9190`, `0x140ca91e0`, all transcribed);
  the gate (property `0x14` > `0.5`, `body+0x10d0 & 0x80000c == 0`, latch bit 0,
  a live world at `body+0x1ad8`); and the impulse
  `target = M^T*body[0x1150] + body[0x1510]`, `target.y := body+0x60c`,
  `body[0x1150] = M*(target - anchor)`. `body+0x1510` has **no writer anywhere
  in `.text`**, so the impulse reduces to `local.y := body+0x60c`.
- **The magnitude is a component field.** The body is a copy of the mover
  component at `+0x528` (`CopyMoverComponentFields` `0x140ca4680` /
  `FUN_140ca4980`), so `body+0x60c` is class `+0xe4` = hash `EC289987`, which
  reads **`6.0` for all 55 playable heroes**. Five further offsets corroborate
  the mapping, the terminal fall `B74245E8` (`+0xec` -> `body+0x614`) among them.
  The six `STU_97288E54` floats are **not** the impulse.
- **Shipped:** `ow_sim::jump` (flag word, five transitions, button split, gate,
  impulse, the body readers) with 13 tests, and
  `ow_tank::entity::mover_takeoff_lane_of` + a real-install test over the whole
  roster. No differential unit yet.
- **Still open, and why no jump is wired:** the per-frame consumer
  `FUN_140cb2220` (`0x140cb2220`) resolves `body+0x1150` through the 0x40-byte
  record table at `body+0x960`; its producer is unread (five parallel record
  vectors are built at `0x140ca37d0` with `count = ctor arg`, capacity 4, and
  nothing in the read set fills them). `MoverAdvance` (`0x140ca7f30`) is the read
  integrator but its only caller has no code caller, so the live position comes
  from the swept solve, whose contact-to-position rule is unread. Also recorded:
  the verified ramp unit and the capsule page disagree about `body+0x5a8`/
  `+0x5b4`, so one of those two writers runs last and it is not read.
- `ow-client`'s Space key still declares the jump open, now naming the take-off
  as read and the arc as the missing piece.

## 2026-11-16 - the jump's leaves are verified; two of the first cut were wrong

- **Six differential units, all green** (`branch coverage 100%`, mutation kill
  rate `100%`): `fn/MoverOnTakeoff` (`0x140ca9190`), `fn/MoverOnLanding`
  (`0x140ca9120`), `fn/MoverOnGround` (`0x140ca91e0`), `fn/MoverEnterAir`
  (`0x140ca9150`), `fn/MoverPropertyRecords` (`0x140cb3990`, the 0x38-byte
  single-property applier the jump stage calls three times) and
  `fn/MoverVectorRecords` (`0x140cb3b10`, the 0x40-byte four-lane applier).
- **They found two real defects, not style nits.** `NOT_ON_GROUND` included bit
  19; the client's mask is `0x0080000c` (bits 23, 3, 2). And `MoverOnTakeoff`
  left bit 10 (`0x400`) alone when `dl` was 0, where the client's
  `btr eax,0xa` / `cmove` **clears** it. Both fixed; the tests that encoded the
  wrong law are rewritten.
- **Read sharper:** the impulse's `rise` has two branches
  (`(along + lane) - along` when airborne and rising, else `lane - along`), and
  both lane sums are three-lane `(p1+p0)+p2`, not four. The airborne prologue
  (`0x140cc148f..0x140cc155b`) forces `MoverOnGround` when a request arrives
  while airborne and below property `0x26`, arming the `body+0x1ad0` budget -
  the same property chain the appliers serve. `ow_sim::jump` now reports the
  frame stamp and the `+0x1a76` latch separately (`Transition`), because the
  client's conditions for them differ.
- **Not a unit, and said so:** the whole of `FUN_140cc1440`. Its middle third is
  obfuscated dispatch and the impulse arithmetic is inline, so the take-off's
  arithmetic is transcribed and tested, not bit-exactly proved.
- **Still open, unchanged:** the arc. `FUN_140cb2220`'s record table has no read
  producer, and the live position comes from the swept solve.

## 2026-11-16 - the per-frame vector resolve is read: it has no gravity in it

- **Read end to end:** `FUN_140cb2220` (`0x140cb2220`), the only per-frame
  producer of `body+0x1150`. It is a chain, not an integrator: property `0x22`
  scales against the vector length, property `0x23` again, property `0x24` acts
  on the y lane alone, then `body+0x60c` caps the y lane (divide, property
  `0x25`, multiply back), then two 0x40-record passes behind
  `(*body->vtable)[0xa8](body) != 2`, then `MoverOnLanding` when the world lane
  rose by more than `0.004`.
- **Two conclusions that change the plan.** `body+0x60c` is the *lateral cap*
  here and the *take-off magnitude* in `FUN_140cc1440` - one component field,
  two uses. And there is **no gravity term anywhere in the chain**, so the arc
  cannot be produced by porting this function.
- **Not implemented, and why.** Both normalisations use `RSQRTSS` plus one
  Newton step, so the scale factors are not portable bit-exactly; and phase 5's
  gate is a virtual call while its hit path is a five-way plane-projection
  dispatch read only at the skeleton level. Also settled: the five inline record
  vectors are filled by the constructor's *arguments* - `0x140ca37d0`'s only
  callers are the object constructor `0x1407ff1f0` and a factory `0x1407ff1b0`
  referenced only from a data pointer at `0x142fdbee8`, so the content is
  runtime data, not code.
- Recorded in `formats/character-controller.md` "The per-frame vector resolve"
  with the constants as raw words and the instruction ranges, and in
  `open-questions.md` items 1, 1a, 1b.

## 2026-11-16 - the vector resolve is implemented and verified; both blockers closed

- **`RSQRTSS` measured, not assumed.** A two-instruction stub (`rsqrtss` +
  `movd`) run through the oracle shows the value is exactly
  `f32(1.0 / f32_sqrt(x))` on 220/220 sampled inputs; the double-rounding forms
  miss 44/220. So the client's one-Newton-step square root is portable: it
  returns one ulp below `3.0` on `9.0` and must not be replaced by `sqrt` - a
  74-input sample through a hand-assembled copy of the client's own instruction
  sequence agrees with the Rust bit for bit.
- **The differential corrected the reading.** The phase-2 shuffle is a mask,
  not the identity: it builds `(x, 0, z, w)`, so property `0x23` acts on
  `x^2 + z^2` and scales only `x` and `z`; `y` and `w` keep their phase-1
  values. A `movss xmm7,xmm6` before the `shufps` pair is what makes it so.
- **Shipped:** `crates/ow-sim/src/resolve.rs` (phases 1-4, `rsqrtss`,
  `sqrt_newton`, `landing_rise`, 13 tests) and the unit
  `fn/MoverResolveVector` driving the real `FUN_140cb2220` with the 0x40-byte
  count at zero: 100% branch coverage over the phases-1-4 window, 7/7 mutants
  killed, two boundary mutants declared observationally equivalent with the
  argument recorded.
- **Still open:** phase 5's plane-projection hits, the bit-24-clear transform
  path, the 0x40-byte record producer (runtime data), and the live position.
  The arc still cannot be produced, but the per-frame *vector* law is now exact.

## 2026-11-15 - the walk's world: the map's collision, not the models' meshes

- **Operator report:** `ow-client` "always bumps into invisible obstacles" and
  feels different from the install. Measured, not guessed: the walk world held
  the level collider **and** 3,017 per-model `STU_537E56E4` holder shapes.
- **Measured (`--walk-test`, practice range, spawn room):** with the level
  collider only, the forward leg is **5.500 m** at 5.5 m/s with **0.000 m**
  drift and the six-second tour keeps its height to 1 mm; with the model shapes
  added the leg is 5.189 m, drifts 0.461 m and the tour **climbs 0.78 m** in the
  flat room. A column at `(70.00, 61.10)` shows why: the level mesh has a ramp
  (normals `(-0.55, +0.31, +0.78)`), the model shapes put a coarse copy over it
  with `(-0.53, +0.41, -0.74)` and **vertical** faces `(-0.58, 0.00, -0.82)`
  that the walker slides on.
- **Read:** the per-model mesh's consumer in this image is a query that writes
  nothing (`FUN_14210bac0` -> `0x142caf6c0`), and the per-model Domino body
  builder `0x14201ae60` is reached from the collision component, whose attach is
  decided by an unread engine driver. The walk therefore uses
  `RangeWorld::level()` (the `MAP_CHUNK_COLLISION` / additive bodies) only, and
  `walk_world` errors when the range carries none.
- **Found and measured in the stand-in:** `snap_to_floor`'s "only down" guard
  compared the probe's answer with the probe origin (`body + 0.2`), so the snap
  lifted the body up to one probe lift per frame - `+0.118`, `+0.229`, `+0.165` m
  in three consecutive frames, `0.78` m total. The snap also teleports down by
  up to the 2.5 m reach and is load-bearing for the landing (the verified leaf
  reports nothing for a sweep that starts on or inside a surface), so it stays
  **open** rather than being patched into another guess: `open-questions.md`
  item 1b, `formats/character-controller.md`.
- **Corner clip fixed (operator's second report).** Straight into a wall stopped
  but a corner let the body through. Reproduced in a unit test: the first frame
  crossed the second wall by **3 cm**. Cause: the slide leaves the body exactly
  tangent to the second wall, and the verified leaf reports nothing for a sweep
  that starts on or inside a surface, so the residual was applied in full.
  `World::overlap` now asks the leaf's own "already inside" mode (`block+0x72`)
  at `radius - CONTACT_SKIN` (a resting contact reads as inside at the full
  radius, so the cut separates resting from penetrating), `depenetrate` pushes
  the body out, and a miss moves to the depenetrated target instead of applying
  the residual. `--wall-audit`'s near-vertical tunnelled frames: **174 -> 27**
  of 8,741 legs (loose `|up| < 0.6`: 289 -> 71); the audit now prints that count
  separately.
- **Also:** `--walk-log` and `--walk-test` now print each contact's normal and
  the shape that produced it (`ow_sim::walk::Advanced::contact_shape`,
  `ow_client::walk::Step::contact`), so a blocked frame names its cause.

## 2026-11-14 - hero select: the mode's roster, the hero's own name and row, and a pick

- **Read: the hero asset names itself.** `STUHero.m_0EDCE350` (class offset 48)
  is a type-`0x07C` display string; `ow_tank::hero_select::hero_name` decodes
  the `teString` payload (two header bytes, text, final NUL) and strips the
  trailing space and the non-breaking space, the same two characters the
  reference's `GetCleanString` strips. The install reads `Tracer`, `D.Va`,
  `Lúcio`, `Torbjörn`, `Soldier: 76`.
- **Read: the playable marker.** `STUHero.m_64DC571F` (class offset 904, one
  byte; the reference's `IsHero`) separates player heroes from NPC heroes. An
  open mode's roster is every type-`0x075` asset, and the practice range's is
  **135 hero assets: 54 playable and 81 NPC**. The range's `Training Bot`
  (index 363) is a *named* NPC, so the marker, not the name, is the filter.
- **Read: the movement row.** `ow_tank::entity::MoverRow` types the named
  `STUCharacterMoverComponent` floats at the class's own offsets: forward
  `0x19452F15` (`0xB0`), side `0xCFD3E635` (`0xA8`), back `0x61D6B4E9` (`0xAC`),
  crouch `0x0DDD18C3` (`0xBC`), terminal fall `0xB74245E8` (`0xEC`), plus the
  unnamed `0x031FE7EE` (`0xDC`). The install pins Tracer/Genji/Anran at 6.0 m/s
  forward and the rest at 5.5; the unnamed 5.5 scalar is **not** the forward
  row.
- **Reader and tests.** `hero_select::selectable_heroes` returns the mode's
  playable roster (an absent or unknown pool class is an error, never a widened
  roster); `resolve_hero` matches a name with case, spaces and punctuation
  folded or an asset index, never a default. `tests/hero_select_real.rs` checks
  the practice (open), Escort (29-hero whitelist), Stadium (mixed) and Mei (one
  hero) rosters, every playable hero's name/marker/row, and the negative picks.
- **The client's stage (a declared viewer default).** `ow-client --list-heroes`
  prints the practice roster with forward speeds; a live launch with a terminal
  prompts for a pick (`--hero <name|index>` names it directly, `--no-hero`
  skips), then the scene places the hero's **own first-person model and
  weapons**, the walk's wish magnitude is the hero's forward base speed (6.0
  m/s for Tracer) and the fall clamp is its terminal fall. The view model is
  re-placed at the camera's own transform each frame (pinned by
  `camera_tests::the_view_model_frame_is_the_camera_frame_at_every_pose`).
  `--walk-test --hero tracer` runs the whole proof at 6.0 m/s and passes.
  The pick itself is **not** claimed: the real pick is a CEF page value and
  variable 940's writer is open (`docs/formats/frontend-pipeline.md` 4a-4c).

## 2026-11-13 - CASC: the page selection is read, eleven verified units, byte census

- **Verified.** `fn/EncodingPageIndexSearch` (`EncodingPageIndexSearch`,
  `0x142498880`, 146 bytes): the search runs unchanged in `ow-casc` as
  `encoding::page_index_search` and the unit drives it directly with its five
  arguments, the fifth on the stack. Differential over 364 cases of the
  return, the index buffer and the key buffer; 4/4 branch edges (100%); 11/11
  mutants killed. The fixture pins the `0xFFFFFFFF` miss for zero pages, the
  lower-middle `mid` via an all-equal index, the raw `key_len` compare, and
  `key_len` 0..`width + 1`. The one declared gap is a page count whose 32-bit
  entry-offset product wraps - the arena cannot hold that index.
- **Verified.** `fn/Md5Transform` (`Md5Transform`, `0x1426ab530`): the nested
  `transform` in `blte.rs` is now `pub fn md5_transform`, called by `md5()`, so
  the unit tests shipped code. The 16 state bytes are compared and the block
  must be unchanged; the body has no conditional branch, so coverage reports
  0/0 = 100% and 18/18 mutants carry the unit. `md5()`'s RFC 1321 tests still
  pass, so every digest is bit-identical after the hoist.
- **Corrected.** `fn/EncodingFindCKey` and `fn/EncodingFindEKey` said the
  search's branch sites were unmeasured "because the oracle rejects stack
  arguments". It does not: the oracle writes argument 5 at `[rsp+0x28]`, and
  five other units already rely on that. Their notes now point at the search
  unit instead.
- **Reading note.** Ghidra's `Md5Transform` listing stops after 64 bytes and
  the rest of the straight-line body belongs to no function; the body runs to
  its return at `0x1426abca8` (1913 bytes, no conditional branch).
- **Read: the CDN index page selection, the last open container item.**
  `CdnIndexPageSelect` (`0x1424da730`, 469 bytes) lower-bounds the last-key
  array at `index+0x88` (stride `+0x5e`) with `min(keyLen,keyBytes)` and an
  early equal exit, misses at `page == pageCount`, makes the page resident and
  scans `min(entriesPerPage, elements - entriesPerPage*page)` records, decoding
  the size (u32 BE) and offset (u64 BE) by the footer widths.
  `CdnIndexPageResident` (`0x1424da590`) is pointer arithmetic in whole-file
  mode, or an HTTP range read (request `0x142522870`, WinHTTP `0x142521d70`)
  with a full-page requirement and a per-page MD5 reject (status 8) in
  streamed mode - the per-page request the docs had recorded as missing.
  `CdnResolveLocation` (`0x1424b0cc0`) splits the 6/5-byte offset into
  ordinal/offset and falls back to the file index. `ow-casc`'s `find_page` now
  follows the expression; a committed test pins the duplicate-last-key early
  exit. A one-off install check walked all 63,149 + 11,800 last keys and 606
  records; each selects its own page.
- **Verified.** `fn/CdnIndexPageSelect` (`0x1424da730`, whole-file branch):
  100% of the reachable branch edges, 14/14 mutants, every channel compared
  (AL, `out_offset`, `out_size`, `out_key16`, the whole index object, the key
  buffer). The streamed resident branch, a null `out_key16` and a short page
  body are declared unreachable/uncovered; the streamed branch is read but not
  implemented by the port.
- **Verified (harness soundness).** The mutation stage no longer counts a
  build/transport failure as a kill: an unevaluated mutant is retried once and
  then makes the unit `inconclusive`. A timeout is retried at twice the budget
  and then killed (a non-terminating mutant is a divergence). Two stale
  line-keyed equivalent declarations were corrected after code drift
  (`fn/EncodingFindEKey` 931->991, `fn/Salsa20Xor` 641/662->652/673); both
  units are verified again.
- **Verified (footer status).** The harness gained a `u8` return kind that
  masks RAX to AL. `fn/CdnIndexFooterParse` now compares the client's status
  (success 0, every reject 8), so the final length gate is covered: 12/12
  mutants, no declared gap for it.
- **Measured.** `cargo run --release -p ow-casc --bin census` (full stride-1
  scan): 1,146,326 records, 84,463,293,804 index bytes, 84,373,987,087 chunk
  bytes, N/Z/E = 10.50%/85.96%/3.54%. The 43,061 `E` chunks are 3.54% of chunk
  bytes and 3.42% of decoded bytes; their 11 key ids are absent from the 63-key
  keyring, so 22,506 records (1.96%) cannot be decoded. `blte_real` now asserts
  the per-mode byte sum equals the frame headers' total.
- **Corrected.** `PageReadPlan` was called a pure leaf; it takes the container
  lock twice and AddRefs the archive handle, which is why it has no unit.
- **Gate.** `cargo xtask verify` green; `python dev/verify_harness/cli.py
  casc` green on eleven units.

## 2026-11-12 - effect chunks: the components are read, and the reference is stale

- **Read.** An effect's root chunk is `EFCT` (version 5, payload ≥ 0x80). The
  client's kind table at `0x143610490` holds 0x34 `{u32 tag, u32 slot}` records;
  the builder `FUN_142e4a780` looks a tag up there, writes the slot to the
  runtime record's `+0xb8`, and copies a fixed set of payload fields per tag
  (`payload = record + 0x10`). `FUN_142e4e8e0` is the offset→pointer fixup and
  gates some tags on the chunk version; `FUN_142e4fbe0`/`FUN_142e50110` handle
  the array sub-chunks.
- **Fixed in `ow-tank`.** `effect.rs` was a port of OWLib's effect structs,
  which describe pre-2.x payloads: `ECMD` 24 B vs **32**, `ECEF` 8 vs **32**,
  `ECEN` 16 vs **80**, `ECEC` 17 vs **64** (identifier moved `+0x08`→`+0x10`),
  `ECSO` 8 vs **32**, `ECVS` 8 vs **48**, `ECRR` 8 vs **64**, `ECPR` 40 vs
  **112**, `ECAT` 8 vs 16, `ESHT` 8 vs 32. Every reader was replaced by the
  client's copies: `Effect::parse` walks the tree, `Chunk` carries the tag, the
  kind slot, the version and the payload, and the seven component kinds with a
  GUID have typed accessors at the client's offsets.
- **Verified.** `tests/effect_real.rs` parses the six effect types and asserts
  every component's GUID type (`ECMD` model `0x00C`, look `0x01A`, animation
  `0x006`; `ECEF` `0x00D`; `ECEN` entity `0x003`; `ECEC` animation `0x006`,
  identifier `0x01C`; `ECSO` `0x02C`; `ECVS` `0x078`; `ECRR` model `0x00C`),
  every payload size and every measured version. Unit tests pin the kind table
  round trip and the short-payload rejections.
- **Closed as impossible from this image.** `ECMP`, `ECSS`, `ECAT`, `ESHT` and
  `ECCV` have **no read site**: their four-byte tag ids occur nowhere in either
  byte order, and they are absent from the kind table and the builder's chain.
  OWLib's readers for them are reconstructions of data the client never reads.
- Page: [`docs/formats/effects.md`](docs/formats/effects.md). Tests:
  `cargo test -p ow-tank --lib effect` and
  `cargo test -p ow-tank --release --test effect_real -- --nocapture`.
- **Also fixed: the model chunk tags.** The client's two model dispatchers
  compare `MOCL` (`0x4d4f434c`) and `CLTH` (`0x434c5448`); an earlier version of
  `docs/formats/model.md` wrote `LCOM` and `HTLC`, which are the same words with
  their bytes in the wrong order, and `HTLC` occurs nowhere in the image. The
  page and the glossary are corrected.
- **Deleted: the `MHRP` hardpoint parser.** The tag has no constant anywhere in
  the image and a census of all 32,280 models finds no `MHRP` chunk. This
  build's hardpoints are `STUModelHardPoint` instances in `MSTU`/look, which
  `hardpoint.rs` already reads and `ow-scene` uses; the reference-only chunk
  parser (`parse`, `parse_payload`, the 96-byte record) is removed rather than
  kept as a reader for a chunk the client cannot read.

## 2026-11-11 - model parsing: the descriptors are the client's, the vertex decode is not

- **Read.** The model asset has two parallel dispatchers over the same chunk
  table: the sizing pass `FUN_141b5e1e0` and the load pass `FUN_141b5df70`.
  Both gate every chunk on `*(u16 *)(record + 0xe)`: `MDLC` `0x165`, `MRNM`
  `0x1f`, `mskl` `0x20`, `LCOM` `0x14`, `MESD` `2`, `HTLC` `0x18`, `MSTU`
  `2`. `MdlcMaterialRead` (`0x141b88cb0`) takes the material count from the
  `u16` at `+0x68` and the GUIDs from the pointer at `+0x08`. The MSTU header
  is `{u64 offset; u32 size; u32 unused}`: `FUN_141b88df0` passes
  `*(int *)(payload + 8)`, four bytes, to the STU v2 loader `FUN_1421d88c0`.
- **Fixed in `ow-tank`.** `model.rs` read a 4x4 `bounding_box` from `MDLC`
  offsets `0x10..0x50`; the client copies scalar fields there, and no such box
  exists (field removed; it was unused). `mstu.rs` read the size as an `i64`;
  the client reads a `u32` (fixed, install 60/60 still parses). Both readers
  now enforce the client's version gate instead of parsing a future layout,
  and the descriptor offsets are the verified constants `ow-tank::mrnm`
  already carries instead of copied literals.
- **Verified.** `crates/ow-tank/src/mrnm.rs` is pinned by three shipped-code
  differential units, all passing after the change: `fn/MrnmFixup` (7/7
  mutants, the three header slots, the element slot, the two stream slots
  only when `flags & 0x10` is clear, the index data slot), `fn/MrnmRuntimeSize`
  (6/6, the 32-bit size expression) and `fn/MrnmHandler` (12/12, the whole
  runtime block, header, submesh `0xa0`, vertex-buffer `0xf8`, index-buffer
  `0x18`, the `-1` roots, the `+0x7b` default byte and the OR'd `+0x00` mask).
  The handler's stream branch was the one that had been guessed backwards:
  it **zeroes** the runtime `+0xa0/+0xa8` for in-chunk streams and copies the
  two GUIDs only for a separate asset.
- **Declared open, not faked.** The vertex-element decode (semantic/index/
  format bytes, the packed offset `(packed >> 12) & 1023`, the format table)
  and the index remap/compact vertex array are the viewer's own reading; the
  client's consumer of the element array is the open IA stage
  (`docs/formats/renderer.md` R10). `MDLC` has no differential unit yet (the
  harness cannot express its two absolute source pointers). MESD, HTLC, LCOM
  and mskl are not parsed.
- Page: [`docs/formats/model.md`](docs/formats/model.md). Tests: `cargo test -p
  ow-tank --lib model:: mstu::`, `--test model_mesh_real --test mstu_real`.

## 2026-11-10 - walls stop the walk: the slide and the floor snap both lifted

- The operator's report ("i can go through walls") reproduced on the range's own
  data: a walk into the level collider at the spawn climbed `0.40 m` up the wall
  and ended `0.245 m` past its plane. Two height leaks were the cause, both in
  code this project wrote (the slide and the step are the client's *open* part,
  `RunCharacterControllerSweep`'s Domino aftermath):
  - the residual was projected onto the contact plane, so a wall whose normal
    carries `0.09` of up (the range's walls report `(-0.576, -0.086, -0.813)`)
    acted as a ramp and each frame raised the body;
  - `snap_to_floor` lifted the body onto any up-facing surface inside its reach.
    A downward probe that *starts inside* a surface's expansion reports a
    fraction that is not a distance, so the "resting height" came out `0.15` and
    `0.19 m` **above** the body on two consecutive frames of the 48-degree
    bevels.
- The rules now: a contact steeper than `walk::WALKABLE_SLOPE` (`0.7`, 45.6
  degrees) cannot raise a walk, and the floor snap only ever lowers the body.
  No new magic constant for the height itself. `0.7` is our number (the client's
  slope limit is unread) and is listed in `docs/open-questions.md`.
- Proven by `walk::tests::a_horizontal_walk_cannot_climb_a_tilted_wall` (a
  `0.0995`-up normal, 240 frames, height change `0.000 m`) and by the client's
  own headless harness, which now walks at every flat wall within 12 m of the
  spawn and asserts a contact, no climb and no frame whose *centre path* crosses
  a surface: `ow-client --walk-test` (before: climb `+0.40 m`; after: `+0.01 m`).
  `ow-client --wall-audit` adds a map-wide version: 1,271 standable points,
  8,645 legs, 312 small crossings, all at points where the audit places a body
  at or inside geometry.
- Jump is **not** implemented, and now says so in the window: the command half
  of the jump is read (`0x140cbb260` splits command bits 6/7 out of `body+0xdc`
  into `FUN_140cc1060` -> `FUN_140cc1440`), but `0x140cc1440`'s vertical
  arithmetic is control-flow-flattened and not read, so Space prints
  "jump: not implemented" rather than inventing a velocity. Anchors in
  `docs/open-questions.md` ("The jump law").

**Archive.** Entries older than 2026-11-10 are kept verbatim in `docs/history/modlog-2026-10.md` (2026-10-26 .. 2026-10-31, 6 entries), `docs/history/modlog-2026-11.md` (2026-11-01 .. 2026-11-09, 80 entries).

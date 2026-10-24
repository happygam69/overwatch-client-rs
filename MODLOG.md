# MODLOG

Newest first. Milestones only; details and addresses live in `docs/formats/`,
current state in `STATUS-handoff.md`, fits in `docs/fitted-ledger.md`, the
work queue in `docs/open-questions.md`.

## 2026-10-24 - Ghidra project pruned: three programs deleted

- Deleted `camera_warm.exe` (never analysed, 0 functions; its code is
  byte-identical across sessions and reads in `Overwatch.exe`),
  `module_v21_clean.bin` (superseded by the complete `Overwatch.exe`: it
  held every page, while 2,351 of the v21 program's `.text` pages were
  zero) and `ldr-warm/loader_after_diff.bin` (the flat warm loader,
  superseded by its own wrap). The images stay in `D:/ow-dumps`, so the
  docs' citations can be re-derived.
- Before deleting, the 166 `module_v21_clean.bin` names in
  `docs/function-names.csv` were translated to the exec frame
  (`0x140000000 + RVA`) and applied to `Overwatch.exe`; the check reports
  **174 named, 0 missing**, and the program was saved. `dev/naming_seed.py`
  now writes new rows for `Overwatch.exe` in that frame.
- The three `regions/` wraps were **byte-checked and kept**: they are
  old-build warm captures, not current-build duplicates. `mover_ctl` fills
  704 and `speed_readers` 384 sampled windows where `module_v2full.bin` is
  cold zero; `clipobj` lives in an old session frame whose citations
  resolve only there. **10 programs remain** in `dev/ghidra_run/owmod`.
- Debloat: the 2026-10-23 page-fill narrative in `docs/ghidra-project.md`
  was compressed to a summary (the detail lives in MODLOG/STATUS and the
  `D:/ow-dumps` manifests); the 4.1 GB pre-prune snapshot
  (`owmod_snapshot_20261021_preprune`) and the root scratch dumps
  `L_ctx.txt` / `L_settings_refs.txt` were deleted. `STATUS-handoff.md`,
  `docs/artifacts.md`, `docs/capture-coverage.md`, `docs/formats/camera.md`
  and `AGENTS.md` updated.

## 2026-10-24 - Differential verification harness; two functions verified

- `dev/verify_harness/` runs a five-stage ladder (load, determinism,
  differential, coverage, mutation) against **the emulated real bytes**
  (`D:/ow-dumps/offline_recover/module_offline.bin`, the complete
  offline-recovered image, via Unicorn), never against a decompile.
  Tier 1 compiles the Ghidra C (`dev/decompiles/`, renamed from
  `anim_decompiles/`) through `ghidra_shim.h`; tier 2 runs `rust_probe.rs`
  through a subprocess protocol. Digest-bound ledger; `cli.py selftest`
  injects 10 defects and requires 10 detections, plus a broken-Rust entry.
  Subagent workflow: `dev/verify_harness/AGENT-PROTOCOL.md`.
- **`fn/AnimTimeWrap` (`0x7ff7c58b1ef0`) and `fn/CurveKeySampleDual`
  (`0x7ff7c58b09d0`) are verified** on every channel at 100% branch coverage
  and 100% mutation kill (3 declared observationally equivalent, with
  rationales). The harness found two classes of Ghidra-output bug, both
  fixed in the decompiles: a float-to-uint *conversion* where the bytes do
  a raw bit test / dword load, and IEEE-ordered `if` chains where the bytes
  are SSE `minss`/`maxss` (NaN yields the second source). `fn/AnimTimeWrap-rust`
  verifies the same oracle against the Rust probe.
- Three other zero-callee decompiles are **not verifiable as they stand**:
  `AnimWritesOrientationPosition` and `DecodeCameraNodeTypeNames` are
  comment-only stubs; `DispatchFpCameraSlotTime` has an indirect call and an
  unrecovered jumptable.

## 2026-10-23 - Complete image imported into Ghidra

- The Plan A PE (`D:/ow-dumps/offline_recover/module_offline.bin`, preferred
  base `0x140000000`, all 12,076 `.text` pages, 201,246 `.pdata` entries) is
  now the Ghidra program **`Overwatch.exe`**, auto-analysed to **151,260
  functions**. This closes the 2,351 zero `.text` pages left in
  `module_v21_clean.bin`; the same code is at
  `0x140000000 + (v2b - 0x7ff7c4d20000)` (checked at RVA `0x1000` and at
  `FpViewmodelConsumer`, RVA `0xd2f060`).
- The current-build names remain applied to `module_v21_clean.bin`; the new
  program is unnamed so far. Docs updated: `docs/ghidra-project.md`,
  `docs/capture-coverage.md`, `docs/artifacts.md`,
  `docs/formats/page-crypt.md`, `docs/formats/camera.md`,
  `docs/formats/animation.md`, `docs/open-questions.md`,
  `docs/architecture.md`, `STATUS-handoff.md`, `AGENTS.md`.

## 2026-10-23 - Offline .text / .pdata recovery (Plan A)

- `dev/offline_page_recover.py` recovers all 12,076 `.text` pages from the
  on-disk ciphertext with the loader pad, no live capture.  Pad check: 2,284
  single-window oracle rows plus the one 16-zero two-window splice
  (`0x53a000`, windows 1524/1512, split 1056), 2,285/2,285.  Held-out oracle
  top-1 2,284/2,284; warm images match 4,887/4,888 and 5,350/5,351 pages.
  `.pdata` is plaintext (201,246 sorted entries) and byte-identical to both
  warm images.  One page (`0x540000`) keeps a 16-byte non-cipher tail; its PE
  entry function is a 32-byte plaintext island recovered from the disk.
  Artifacts and `REPORT.md` live under `D:/ow-dumps/offline_recover/`.

## 2026-10-23 - View-model sampler and weapon source fixes

- `anim_tree::evaluate` now applies layer rotation and translation
  independently for one-link layer bones, matching `AnimSamplerRule`
  (`0x7ff7c6e84610`). Dropped-ancestor chains still use the composed pair and
  scalar lanes are not decoded (FIT-ANI-08/05).
- The default weapon source is the selected **FP skeleton's** bones 44/74 and
  its hardpoint local; `--weapon-fold 1` keeps the old character-c71 A/B path
  (FIT-ANI-04). `docs/formats/animation.md`, the fitted ledger and the
  reference findings note were corrected (spring `600.0/s` ramp, `0.004 m`
  threshold, target `+0x1370`).
- The reference retarget, FP camera pose target and Pose2d dataflow VM remain
  unread; the gate passes with the new code and ledger rows.

## 2026-10-23 - View-model animation exactness check

- Ghidra read against `module_v21_clean.bin`: `AnimSamplerRule`
  (`0x7ff7c6e84610`) copies child 0 unconditionally and replaces each present
  rotation/translation/scale lane only at `w > 0.5`, scalars lerp; the two FP
  per-frame slots advance **event** timelines, not pose; the client registers
  `anim.EnableProxyRetarget`/`anim.EnableFinalRetarget`; the FP camera is the
  animated pose (`+0x1350/+0x1310/+0x1320`); pistols use FP bones 44/74, not
  the character rig; the real spring is on `+0x1350` (`0.235/0.48`, decay
  `0.008`).
- The viewer is **not exact**: reference retarget omitted, per-channel
  sampler not implemented, FP camera/weapon source/spring still fitted or
  wrong. `docs/formats/animation.md` has the comparison table;
  `docs/fitted-ledger.md` FIT-ANI-04/05/08 and FIT-CAM-01 were corrected.
  No behavior code was changed in this check.

## 2026-10-23 - Ghidra image: missing pages filled

- `module_v21_clean.bin` was missing whole sections. Filled in place: 488
  `.text` and 13 `.data` pages from `module_v21d.bin`; 372 `.rdata`, 16
  `.data` and 4 `.pdata` pages from the on-disk `Overwatch.exe` (same
  `TimeDateStamp`). Pointers were rebased with the executable's `.reloc` table
  to the image's PE base (`0x7ff736e60000`), not copied from another session;
  6 live pages were kept. `K` = 0.011519 and the FP half-angle constant
  0.00872665 now read correctly in Ghidra; function count 92,728.
- Full image and manifest: `D:/ow-dumps/module_v21_clean_full.bin` and
  `module_v21_clean_full.manifest.txt`; `docs/ghidra-project.md` records the
  sources, pointer rule and the remaining 2,351 zero `.text` pages.
- The per-frame camera look consumer was not among the newly analysed pages;
  the input tick and the large render update are the only warm readers of the
  look accumulator offsets.

## 2026-10-23 - World camera uses the read FOV/clip planes

- `ow-view`'s world pass uses the read **0.1 / 16384.0** clip planes and the
  **103 deg horizontal -> 70.53 deg vertical** FOV; `FIT-CAM-04` is removed.
  The `.rdata` constants are read from the **on-disk executable**, not the
  merged image: the merged `module_v21_clean.bin` zeroed its `.rdata` pages.
- The projection builder is read (`BuildPerspectiveMatrix`
  `0x7ff7c6d95a70`: row-major, left-handed, depth 0..1). The view's x row is
  `A x B` and the projection's x scale is negative, so the signs cancel; the
  viewer's right-handed look/projection pair gives the same image.
- The FP FOV formula is decoded: `UpdateFirstPersonCameraPose`
  (`0x7ff7c56b7b90`) calls `FUN_7ff7c5700f50`, which reads `GetHorizontalFov`
  (103), halves it to radians (`0.00872665`), applies `tan`, the 9/16 factor
  and `atan`, and doubles: **70.53 deg vertical at 16:9**. The `60.0` angle is
  a fallback and the per-hero `50` scalar is not the FOV. The FP pose
  (animation camera bone, drop/back) stays fitted.
- The mouse `K` is read from the on-disk `.rdata` at `0x7ff7c7ccc4b0`
  (`0x3C3CBADB` = 0.011519); with the old-build default scale 0.15 the viewer
  uses 0.00172785 (`FIT-CAM-08`, scale still fitted). The pitch clamp remains
  fitted; the component's `-89/+89` limits are recorded.

## 2026-10-23 - Camera surface read; world near/far settled; tick not located

- `docs/formats/camera.md` is written: the 2.25.0.1 camera var surface
  (`camera.*`, `camera3Panim.*`, 3P, `firstPerson.*`, `localhiteffects.*`) is
  mapped with registration groups, record blocks, static objects and defaults.
  The defaults match the old build, so they are true statics.
- The world camera pair is read: `DefaultNearZ` **0.1 m**, `DefaultFarZ`
  **16384.0 m** (registration `0x7ff7c51e2980` / `0x7ff7c51e2890`, getters
  `0x7ff7c686cbc0` / `0x7ff7c686cba0`); the first-person pair is exactly one
  tenth (`0.01 / 1638.4`). This closes the near/far half of open question 6.
- The **first-person camera setup** is v2b `0x7ff7c568c5b0` (read, not a
  defined Ghidra function): six 0x110 animation slots, a 60.0 deg angle, FP
  near/far and an animation FOV blend. It is not the type-`0x35` component
  consumer; `m_position` and the four scalars remain unread.
- **Negative result, recorded:** the five interpolation-inverse vars and four
  speed rates have no direct reader outside registration. The camera code
  pages are warm (the input tick at `0x7ff7c5301000` is 3733/4096 nonzero), so
  the tick is reached indirectly or through a copied settings block; it stays
  fitted. The 64-bit name table holding `Camera` and `CameraShake` is an
  animation/effect name dictionary, not a class registry; it has no code
  references.
- **Fourteen camera functions are now named** in `module_v21_clean.bin` and in
  `docs/function-names.csv` (63 rows total): the near/far getters and
  registration, the FOV registration, the plane setter, the two mouse-look
  deltas, the FP clip-plane registration, the FP slot dispatcher, the FP offset
  transform, the input poll, force-input mode, the replay proto merge and the
  view-params constructor. Names are **not** applied where the entry is not a
  function or the role is inferred; `camera.md` section 8 lists both sets.
- Dead ends and false leads (name table, message-helper functions,
  mis-bounded functions) are recorded in `camera.md` section 9.

## 2026-10-22 - 49 function names; the two 2.25.0.0 standing images split

- `docs/function-names.csv` now names **49 settled functions**: the previous
  19 plus 30 current-build rows seeded by `dev/naming_seed.py` from the docs'
  decompiled-path findings (animation, statescript, materials chain, domino
  physics, mover shell, netcode, debug panels). Every one of the 30 targets
  was confirmed to sit exactly at a real function begin of
  `module_v21_clean.bin` through the Ghidra program before writing; 88 further
  candidates were refused honestly (`not matched`, ambiguous needles, cold
  windows, holes), so the skips are recorded as unfound, never guessed.
- `DmHandleDecode` is corrected: domino.md:264 shows the handle-level function
  at `0x7ff7c7a4cce0` zeroes force and torque behind the +0x15c gate, so the
  row is now `DmWorldClearForces`.
- One v2b address carried two doc claims: the MRNM chunk deserializer
  (materials.md:203) and the MRNM runtime-table builder (animation.md:64) are
  the same function; the v21 row is `MrnmHandler` with both docrefs.
- `module_v2full.bin` and `module_after_diff_v2.bin` are **not** the same
  bytes: over 10,582 shared nonzero pages, 5,961 agree and **4,621 differ**
  (the dmWorld::Create prologue sits where the docs say only in v2full). The
  docs frame is `module_v2full.bin`; byte translation into the current build
  reads from it now. The cause is open question 12.

## 2026-10-21 - the client updated itself: the builds do not mix

- The install is now build **2.25.0.1.154504**; every address in the docs
  belongs to **2.25.0.0.154088** (the 12-20 October captures). The update
  moved the code: of 15,297 non-empty pages, **0 keep their address** between
  the two builds, so the docs' addresses are the old build's and the
  conclusions are unaffected.
- The first re-merge of all 17 runs **mixed the two builds** - the image's
  bytes at a given address came from different builds. It is deleted, and
  the standing image is kept per build: `module_after_diff.bin` (2.25.0.0,
  the docs' basis) and `module_v21_clean.bin` (2.25.0.1, the current
  build, from the 21 October runs).
- The Ghidra project now holds **one build per program**: `module_v21_clean`
  for new work, `module_v2full` (2.25.0.0) for the docs' addresses, and the
  session images for pointers.
- The 2.25.0.0 captures have **no `.pdata`** at all (the unwind table was
  never resident in those sessions); the 2.25.0.1 capture has all of it, so
  the current build's function extents are exact.
- `dev/build_translate.py` carries an address from one build to the next.

## 2026-10-21 - the captures are checked, per system

- `docs/capture-coverage.md` answers "do we have the captures we need?"
  with measurements: **every code address the open questions need is warm in
  the standing image** (41 of 41 checked, across animation, camera, movement,
  combat and physics). The open questions are analysis work, not capture work.
- **The standing image was stale.** Re-merging the 17 runs we already have
  (`dev/warm_merge.py`, no new capture) takes the client's unread `.text`
  from 21% to 11%, and makes `.pdata` - the function table of the whole
  client - fully readable. The merged image is
  `D:/ow-dumps/module_after_diff_v2.bin`.
- The two capture gaps the docs recorded are **closed**: the mover
  component's four methods and the volley class's region are warm now, and
  the renderer's code was warm already (the docs' own 2026-10-19 update).
- What is left cold (11% of `.text`) is code no run ever saw decrypted; the
  pages no run captured are `.pdata`/`.reloc` metadata and uniform filler,
  not code.
- The **Ghidra project holds a 12 October image** (38% cold), so every
  decompile sees less of the client than the captures hold. Refreshing it to
  the re-merged image is the one follow-up worth doing.
- The only question that genuinely needs a new capture is the page cipher's
  `w`: it wants runtime state (the faulting rip), not code.
- `dev/capture_coverage.py` makes the check repeatable: per address, per
  block, per gap, and what each run would add to the image.

## 2026-10-21 - the Ghidra project is tidied and explained

- The project stays in the repo folder (`dev/ghidra_run/`, on the SSD because
  Ghidra reads it hard) and is git-ignored: it is the client's own code.
  `docs/ghidra-project.md` is the guide - every program, its base address,
  what it answers, how an address in a doc maps onto it, and how to add a
  capture.
- Eleven spent programs are gone (21 -> 10, 4.2 GB -> 2.2 GB): the
  superseded `module.bin` and `module_v2.bin`, the two range-capture wraps
  and the animation/evaluator region wraps, whose questions the docs already
  answer. What is left is the reference set: `module_v2full.bin` (the
  analysed program, v2b base `0x7ff7c4d20000`), the standing capture's
  mapped and heap images, the newest live main image, the camera region, the
  three region wraps the docs cite, and the two loader programs. The region
  wraps moved into a `/regions` folder so the root holds only the reference
  images.
- A snapshot of the project as it was before the cleanup is at
  `D:/ow-dumps/owmod_snapshot_20261021_preprune`.
- **The decompiler now speaks the docs' language.** `docs/function-names.csv`
  ties a function's address to the name the docs already use for it, and
  `dev/gh_apply_names.py` applies the names through the MCP bridge: 13
  functions are named (the FP view-model consumer, the animation sampler
  rule, the Domino physics chain, and the loader's page-XOR applier with its
  caller and the two pad families). A row is added when a function's role is
  understood, never before.
- The loader's code is only readable in the warm image, and a function
  created in that 37 MB flat image makes Ghidra follow the flow across all of
  it (a 23 MB "function" that killed the decompiler). The four regions that
  matter are now wrapped into one small PE at the client's own addresses
  (`dev/build_loader_wrap.py`), so the page cipher can be read and named
  without the decompiler choking.

## 2026-10-21 - the project gets a gate, a toolbox and a ledger

- **The rewrite is in git.** The whole product (60k lines of Rust across seven
  crates, the format docs, the tools) had never been committed: `HEAD` held 59
  files and the worktree held ~140 more. It now lands in three commits, each of
  which builds.
- **The gates are green.** `cargo fmt --check` and
  `cargo clippy --workspace --all-targets` pass (the lint policy that matches
  the client's own shapes is declared once in the workspace manifest), and the
  real-install test that failed - the shader group parser, which had lost the
  group's last two arrays - now parses 200 of the first 400 groups again.
- **`cargo xtask verify` is the gate**: fmt, clippy with warnings as errors,
  the tests, the tool index and the fitted-ledger check.
- **The toolbox is pruned and tracked.** 70 scripts are gone (one-off clip
  probes, superseded loader scans, closed keystream hypotheses, tools whose
  question the docs already answer). The 121 that remain are listed in
  `dev/INDEX.md`, generated from their own headers, and they are tracked so a
  re-derivation survives a lost machine. The captures, the Ghidra project and
  the multi-MB outputs moved out to `D:/ow-dumps`; nothing that came out of
  the game (disassembly, extracted strings, raw asset bytes) is tracked.
- **The plans are one plan.** The finished plans (the cleanup plan, the
  systems-extraction sweep) moved to `docs/history/`;
  `docs/open-questions.md` is the work queue: each open question with what
  would settle it and the command that would do it.
- **The fitted ledger is checkable.** Every fitted, assumed, stand-in and
  supplied implementation has an id (`FIT-CAM-01` and friends) next to its code
  and in the ledger, and `cargo xtask fit-check` fails when the two drift.
- 37 public functions that no reader, probe, test or document called are gone,
  with the imports they kept alive.

## 2026-10-21 - page decrypt: candidate live-verified; `w` still open

- Operator ran an elevated WPR CPU trace (`cpu_20261021_124450.etl`, 2.98 GB)
  and a full minidump (`ow_heap_full.dmp`, 13.5 GB) in one practice session;
  loader base `0x7ffc52d50000`, main base `0x7ff6a2d80000`.
- Raw qword scan of the ETL found the candidate's dword XOR loop `0x144c839`
  as a **leaf PC** (6 samples), with `0x1015a99` (return from the
  `call 0x14380e0` at `0x1015a94`) directly above it: `0x144c839 ->
  0x1015a99 -> 0x163f6b1 -> 0x1a29511`. The candidate is the page-XOR
  applier.
- The candidate's post-`VirtualProtect` return `0x14462cb` is live too (26
  frames), from the other caller `0x1a0d536` (`0x1a0a3f0`). PadB's dword
  loops `0x10c7692` / `0x10c9835` are also live, but from `0x8a4ad0`
  (`0x8a46d0`), a separate path.
- The full dump contains a private **ghost loader copy** at `0x1ba39390000`:
  pad and `.eidsig` present, `.eid` byte-identical to disk, `.data` has
  runtime pointers. No fixed-stride `w` table in the ghost or the full dump.
- The dump also holds a **0x90-byte page metadata table** at `0x1ba36b90000`
  (1,845 records; type B page VA at `+0x00`, type A at `+0x88`). The likely
  5th-arg table is `0x1ba37437d58` (1,439 pointers to type-A records); its
  selector `0x1ce59b0` -> `0x3fa280` hashes `rdi` and returns `hash % count`,
  so it is a hash table. For 860 oracle pages, shifts, `% 4096`, xor/add/mul,
  `fmix64`, seeded Murmur3/CRC32, every field and constant xor/sub/add all
  fail (best 3/860); `w` is not plainly stored there. The selector is
  VM-wrapped: `0x1ce59b0` -> `0x3fa280` -> `0x1494090` -> `0x2511e0`, with
  `0x3fa280` also calling `0x40cd87`; the native page loop is reached from the
  same caller.
- Found the live **`STUFirstPersonComponent`** objects in `ow_heap_full.dmp`:
  `64/16/-1` then `50/1.25/0.875/0.8` at heap VA `0x1bb9614d97c`
  (`65/...` and `47/...` for other heroes). The class hash `0x9d28963f` maps
  to type index `0x35`; the shared vtable `0x7ff7c7ef5f30` is generic, so the
  consumer is a type-`0x35`/entity-offset system, not a vtable method. The STU
  schema (`0x70`, 7 fields) gives `m_position = (0.1375, 0.23375, 0.20625)`,
  `m_entity` -> the view-model entity, and the four scalars 50 / 0.875 / 0.8 /
  1.25; `m_position` is the view-model offset candidate.
- Found the type-0x35 **consumer path** in the live main image:
  `FpViewmodelConsumer` (RVA `0xd2f060`) gets component 0x35, resolves the
  view-model entity/model, and calls the bind `FUN_00d2faa0` (RVA `0xd2faa0`).
  The bind stores the refs; it does not read `m_position`/the four scalars.
  The view-model class vtable is RVA `0x3115ef0`; slots `+0xe0` (`0xd31bf0`)
  and `+0xe8` (`0xd322f0`) are the per-frame updates and walk six
  `0x110`-stride animation slots (`FUN_00d29c60` / `FUN_00d3a020`). Those are
  an animation event/state player (event masks/lists, 0x50-byte priority
  records), not the pose or the camera.
- `retarget_probe` now prints the c71 hand/weapon worlds: hands 14
  `(0.155,1.367,0.091)` / 55 `(-0.210,1.364,0.092)`; weapons 44
  `(0.113,1.441,0.348)` fwd `+x` / 74 `(-0.143,1.506,0.350)` fwd `-x`; bind
  hardpoints `+/-0.44,0.93`. At eye 1.75 / FOV 29.4 only tips are visible; the
  component 1.25 eye height + FOV 50 brings the hands/guns in. The viewer now
  poses the two weapon instances from the character c71 rig at 44/74
  (`ow-scene` loads the character skin + c71; `ow-view` applies the hardpoint
  and camera); the FP arms keep the selected animation. This replaces the
  bind/FP-parent gun path that crossed the pistols. Test:
  `cargo run --release -p ow-view -- --play --hero 3 --anim auto --fp-eye 1.25 --fp-fov 50`.
- Declared the **`w` derivation tier 3**: the picker is inside the loader's
  own obfuscated VM/dispatcher (`0x2511e0` dispatcher, 22 KB
  `0x40bee0..0x411536`). The earlier "Denuvo VM" label was wrong. The XOR
  transform is exact and the current-build `w` is read, but a new build needs
  a fresh oracle. No fit is standing in for it.
- Correction: the **disk** `.pdata` is encrypted; only the **live** `.pdata`
  is valid. The previous entry's "`.pdata` is intact" refers to the live image.
- The dump tool's private-RW default returns `E_INVALIDARG` on the game;
  `--normal` and `--full` work. New raw ETL tools: `dev/etw_raw_scan.py`,
  `dev/etw_count.py`.

## 2026-10-21 - page decrypt: static refinement, candidate still CANDIDATE

- Pad-base census is ten refs, not three: eight in the candidate region, one
  in `0x10b72f0`, one in `0x10c1e20`. All three take three small keys plus a
  data pointer, so the family may be data/string decoders.
- Corrected a parser-bound error: loader `.pdata` is intact and
  `0x14380e0..0x144ee19` is one function. Its page-XOR state is
  `0x14388e7` -> `0x144968f`, with `w` taken from `edx` at `0x14481c1`.
- `0x10c1e20` is the only region with the 4-byte-granular `and 0x3ffc` pad
  mask, matching the measured 4-byte window alignment; added as a secondary
  watch target.
- `w` is build-stable: 1,676 pages captured in `run_20261021_105102` match the
  `run_20261021_033126` oracle with zero conflicts (597 gave no window). This
  overrides the earlier "per-fault draw" wording and keeps the stored/derived
  table question open.
- No stored or hashed `w` table in the disk loader, live loader image or live
  main-module image; page/hash/bitfield derivations remain chance-level.
- Added an untested elevated WPR CPU-sample capture
  (`dev/etw_cpu_capture.ps1`) and scanner (`dev/etw_scan.py`): a sampled PC in
  the pad family would name the applier, but the probe is probabilistic.
- Added read-only minidump tools (`dev/dump_process.py`, `dev/scan_dump.py`)
  for the build-stable `w` table in the heap; tested on a Python child, not on
  the game. Details: `page-crypt.md`.

## 2026-10-21 - docs cleanup

- Condensed `STATUS-handoff.md`, `MODLOG.md`, `page-crypt.md` and `animation.md`
  to current state and durable evidence. Removed repeated session diaries,
  superseded readings and stale tooling notes; kept addresses, runbooks,
  evidence classes, negative results and open boundaries. Docs only; no code
  change.

## 2026-10-21 - page decrypt: live gate passes, applier still not isolated

- Phase-marked run `run_20261021_105102` (pid 4100, 90 s, 1,597 flips;
  phases `{0:1449, 1:51, 2:97}`) passes the correlation gate as
  `ATTRIBUTABLE-CANDIDATE`. Same-session cookie run `105145`; loader base
  stable across the pair.
- The gate is necessary, not sufficient: `events.csv` has protection flips
  but no faulting rip, handler or `VirtualProtect` arguments. Candidate
  `0x14380e0` remains **CANDIDATE**. The only unrun lever is an
  operator-approved fault-synchronous run. Details: `page-crypt.md`.

## 2026-10-21 - page decrypt: data half solved; code half closed to one candidate

- The keystream is a 4 KB window drawn uniformly from a static 16 KiB cycle in
  the loader's `.rdata` (`0x1d77f80..0x1d7bf80`, shipped rotated by 19 bytes).
  Window start is not RVA/content/hash/region/counter derived; it is a
  per-fault i.i.d. draw. All 2,285 current-build oracle rows verify; a 16-zero
  splice at `0x53a000` is part of the cycle.
- Static sweeps exhausted: no VEH, no `NtContinue` ref, no page walker, no xor
  loop, no static consumer of the table. The import band is runtime heap
  thunks; live TEB/PEB resolution recovers 286/286, and the shared VP slot
  `0x1dce438` is `KERNEL32!VirtualProtect`.
- Candidate `0x14380e0` carries the pad arms, both VP indcalls, the xor leaves
  and the `and 0x3fff` window mask. Rival `0x442110` shares VP but not the
  applier structure. Details: `page-crypt.md`.

## 2026-10-21 - page decrypt: offline packer and static hunt negative

- `dev/emu_tls.py` runs the loader TLS callback offline under Unicorn: ~130M
  instructions, reads nine static `.data` cookie pages, writes nothing, never
  touches `.eid` or the main module. Replaying with live pages matches the disk
  baseline. The decryptor is not in the offline TLS path.
- Current-build disk loader sweep (5,701 pdata functions, ~1.73M instructions):
  no VEH registration, no 4 KB page walker outside CRT `memmove`, no ARX
  routine tied to page extents, no crypto/exception mixing function.
- Corrected an earlier CSV decimal/hex misread that had produced false
  4 KB-loop hits. Details: `page-crypt.md` sessions 9-13.

## 2026-10-20 - page decrypt: disk loader is ground truth; build mismatch

- `D:/ow-dumps/loader_disk_rva.bin` (37.8 MB, offset == RVA) is byte-identical
  to the installed `Overwatch_loader.dll` (7,507/7,520 `.text` pages). The disk
  loader is complete plaintext code.
- The warm `loader_after_diff.bin` is an older build (`.text` `0x1d5af46` vs
  `0x1d539e6`, `.pdata` 5,701 vs 5,679); warm and disk `.text` share no pages.
  Cross-build comparisons are invalid, including the earlier "7,178 identical
  loader pages" claim.
- TLS callback: dd[9] -> array at `0x1dcd140` = `[0x48ffd0, 0x1d2cf24]`.
  Imports, exports, load config and `.gxfg` are mangled decoys. Static code
  uses runtime-only gs/fs walks; no `.eid` refs, cipher constants or absolute
  address immediates. Details: `page-crypt.md` session 6.

## 2026-10-20 - page decrypt: deterministic per-page XOR stream captured

- `X = C_disk ^ P_warm` is identical across two independent captures
  (5,714 pages, zero mismatches): per-page stream, deterministic for this
  install, no per-boot nonce. The 0xC3 filler for non-hot pages was identified
  as an anti-tamper artifact and excluded.
- Built `D:/ow-dumps/pagepak/keystream.npy` (9,449/12,180 pages, 77.6%) and a
  later current-build oracle of 2,285 rows. Details: `page-crypt.md` session 5.

## 2026-10-20 - page capture: tight tier proves transient decrypt windows

- Tight-tier run `run_20261020_124815` (300 s) caught 1,188 decrypted pages:
  831 `.eid` pages (sorted 4-byte ID tables) and 357 engine pages. No window
  closed during the run, so plaintext persists; later gameplay runs meet the
  same pages on the cipher side because execution is brief.
- Fixed two tier bugs: module-relative span math and lost CLI flags. Added a
  byte-based result-drop guard, startup surge mode, retry-ring/stripe fixes and
  `stats.csv`. Menu capture converged at ~12.7k pages; gameplay is required for
  the rest. Details: `page-crypt.md` sessions 3-4.

## 2026-10-19 - FP blend graph, live capture, renderer and camera

- FP idle item 3 is an `STUAnimBlendTree` (`0x0F8`, 28 instances), not a flat
  clip: Pose2d nodes for `d4`/`bc`, branch-by-category, five clips
  `c71/c50/d4/bc/680`, 19 driver floats, `(-0.4, 0.4)` input range. Slot rule:
  `m_63DEDAC0` is a 1-based index into the item's float array.
- `dev/live_page_diff.py` became the preferred read-only capture (protection
  changes, retries, `encrypted.csv`). Merged images refreshed; renderer
  D3D12/DXGI code is present and decompiles.
- Camera: client `HorizontalFOV` 103 deg, FP near/far `0.01/1638.4 m`, opt-in
  look interpolation; view-model drag is a zero-rest velocity spring
  (`64/16/-1`). Details: `animation.md`, `rendering.md`, `input-camera-audio.md`.

## 2026-10-18 - animation feed, retarget probes, weapons, play harness

- `BlendPose` decodes the FP blend-card family (`0x0053XXXX`): 83 `{S,P,R}`
  records, record-relative lands, id-paired to 71/83 skin bones. The pose slot
  has a translation lane; file P is applied. Absolute-rotation skinning fixed.
- `--anim auto` resolves the idle through the blend-tree set. Weapon hooks are
  entity hardpoints `0x0a`/`0x0b` on bones 44/74, not a centre weapon bone.
- Blend-tree module and `--anim auto` evaluation landed; FP retarget probed:
  character 200 bones vs FP 79, 64 common ids with identical binds; weapon
  intermediates fold exactly. Details: `animation.md`.

## 2026-10-17 - typed STU, content surfaces, textures

- Typed STU v1, class registry and full field readers (arrays, maps, embedded
  refs, strings, math structs, defaults).
- Content surfaces: skin themes (1,752/1,752 readable), custom themes, voice,
  progression, loot boxes, stats/catalogs; 6 of 9 parse.
- BC6H decoded and checked block-for-block; BC1-5/7 already cross-checked.
  Closed five OWLib gaps (placeables, model/cloth, MSTU, shaders, lighting);
  type-`0xD4` skin trees parse, payloads do not.

## 2026-10-16 - physics, mover, animation evaluator

- Domino physics: island solve, tick path, dt and construction; golden vectors.
- Mover: control code declared tier 3 after all readable routes were exhausted;
  arithmetic remains trace-measured. Crouch, air control and reverse rule read.
- Animation evaluator: full kind-to-handler map, kinds 2/10 decompiled, shared
  curve pipeline, noise bank and wrap shaping. Clip runtime record
  `{scale, position, rotation}` and file-side channel decode implemented; unit
  quaternions verified.

## 2026-10-15 - movement, camera, view model, textures, clips

- Movement traces: per-frame integration, speeds, acceleration, friction,
  gravity, jump and slope measured; `trace_diff` gate ships.
- Camera/view model: FP config family read (near/far, animation weight, all
  offsets zero); FP rig anchors on the character; view model rides the camera.
- `mskl` skeleton, MDLC/MRNM models, BC1-5/7 textures; one hero renders fully
  textured. Idle entities, clip container and channel values implemented;
  runtime pose found as 3x4 matrices. FP animation is a separate path.

## 2026-10-12 to 2026-10-14 - foundations

- CASC, BLTE, Salsa20, retail manifest key derivation, keyring and asset
  registry; build info, bucket index, root file, config, content manifest.
- STU bags, instance/field layouts, asset-reference deobfuscation.
- Map chunks, placeables, models, materials, textures; first real map geometry;
  practice-range chunk set identified.

## Dead ends and corrections

- **Renderer is not absent.** The old claim described `module_v2full`, not the
  merged warm image.
- **Whole-view camera lag is not the drag.** Only the view model lags;
  `lookInterpolationSpeedInverse` on the world camera was reverted.
- **The drag is not an absolute-angle follower.** That gives ~72 deg of lag at
  5 rad/s; use the zero-rest velocity spring.
- **The FP pose P lane is applied.** "Hold translations at bind" made the hands
  overlap.
- **The centre weapon bone is wrong.** Tracer's pistols use hardpoints on bones
  44/74.
- **Blend items are not `0x3B150012` rows.** Those are embedded payloads; the
  items are `STUAnimBlendTreeSet_BlendTreeItem` (`0xDDD18945`).
- **The hero gallery does not initialise the gameplay animation gate.** Only a
  match/practice-range session does.
- **A surface material is not a GUID join.** The `(008)` ids are a shared
  non-asset key.
- **One capture is not the whole module.** `.text` is encrypted at rest and
  decrypted lazily; cold pages are normal.
- **No static page decryptor exists in the disk loader.** VEH, `NtContinue`,
  page-walk, ARX-loop, cipher-constant and table-consumer sweeps are all
  negative; the applier is runtime-resolved.
- **The `--help` grab footgun.** `dev/ldr_cookie_grab.py --help` performs a
  read-only grab. Do not use it for a dry print.
- **The 16-zero splice at `0x53a000` is real,** not a capture artifact; all
  other oracle rows are exact.

- 2026-10-21a: project understandability overhaul (docs-only, no behaviour
  change). Added docs/README.md (map), glossary.md, architecture.md,
  artifacts.md (D:/E: index), tools.md, history/README.md; split
  systems-coverage.md into a one-line-per-system index plus a verbatim
  history/coverage-detail.md; added '## Current state' headers to the eight
  big formats docs (character-controller, domino, statescript, materials,
  weapons, rendering, input-camera-audio, maps-streaming); scoped the four
  plan docs in docs/README; dev/ first cleanup pass (13 large unreferenced
  dumps moved to D:/ow-dumps/devdata, 19 one-offs to dev/retired,
  dev/README.md added).

- 2026-10-21b: overhaul steps 5-6. One parameterised headless decompiler
  (docs/ghidra/McpDecompile.java, plus McpCallers.java) replaces the nine
  McpAnimChainN copies; old scripts moved to ghidra_scripts/retired; the
  workflow is documented in docs/tools.md and tested on hmod4. Extracted
  ow-tank::firstperson (Params + read) so fpcomp_probe and the viewer share
  one reader and class defaults are applied (spring 64/16/-1); FP probe
  values verified. Tests: ow-view 17 pass, ow-tank library 198 pass; the
  pre-existing tests/shader_real failure is environmental (build-154504
  shader manifest key table unavailable), unrelated to these changes.

- 2026-10-21c: overhaul steps 7-8 (docs reconciliation). animation.md's
  Current state now matches the latest consumer/retarget findings; the
  README works/open list was corrected (blend tree implemented; remaining
  Pose2d VM, reference-skeleton rest data, per-frame FP consumer); the
  fitted-ledger 'Playback source' row now describes the single-clip
  fallback instead of claiming no blend evaluation; every system doc gained
  an explicit Verify command. Link check clean; ow-view 17 / ow-tank 198
  tests pass; tests/shader_real remains a pre-existing environmental
  failure (build-154504 manifest key table).

# STATUS

The living state: what works, what is open, what is next. `MODLOG.md` is the
milestone history, `docs/formats/` carries the findings and addresses,
`docs/fitted-ledger.md` carries every fit, and **`docs/open-questions.md` is
the work queue** - the next thing to work on is at the top of it.

## The one gate

```sh
cargo xtask verify            # fmt, clippy, the tests, the tool index, the ledger
cargo xtask verify --fast     # the same, without the tests that read the install
```

That is what "done" means here. The eye-check is still the operator's:
`cargo run --release -p ow-view -- --play --hero 3 --anim auto`.

## Verification harness: shipped code, state, scaffolds

- **Tier 2 runs the crate.** `crates/ow-sim/src/bin/verify_probe.rs` is built
  by `cargo build --release` and imports the real `ow-sim` functions. A unit
  declares `"candidateLoader": "cargo"`; the ledger records
  `candidate_sources` (the function's own source file) and `crate_rev`.
- **Stateful units.** A `state` block preloads a snapshot; `guest` pointer
  args index it and the whole region is the `state` channel. The verified
  `fn/OrthonormalizeCameraBasis-state` is the worked example.
- **Scaffolds are not claims.** `python dev/verify_harness/scaffold_unit.py
  <address> --name <Name>` produces a template under `units/scaffolds/`
  (`scaffold: true`); `cli.py all` skips it. The mover scaffold is
  `dev/verify_harness/units/fn-MoverAdvance.json`.
- **Run one unit:** `python dev/verify_harness/cli.py fn/<Name>`; the ledger
  and the five-stage ladder decide, not the author.

## The build changed under us (2026-10-21)

**The install is now build `2.25.0.1.154504`; every address in the docs
belongs to `2.25.0.0.154088`.** The client updated itself between the
20 October and the 21 October captures, and the update moved the code:
page for page, **0 of 15,297** non-empty pages keep their address between
the two builds. The docs' conclusions still hold (they are about behaviour),
but their *addresses* are the old build's.

What this means in practice:

- **Read code in the build the capture belongs to.** The project holds
  `Overwatch.exe` (2.25.0.1, the **complete** image - all `.text` recovered
  offline, with the current-build names) and `module_v2full.bin`
  (2.25.0.0, the basis of the docs' addresses).
  `module_v21_clean.bin` was deleted 2026-10-24 (superseded by
  `Overwatch.exe`; its image stays at
  `D:/ow-dumps/module_v21_clean_full.bin`). The earlier live merge
  `D:/ow-dumps/module_v21c.bin` (five runs of 2.25.0.1, 23.5% cold) is
  superseded.
- **A fresh client is worth more than a long session.** A capture that
  attaches to a just-started process catches the startup storm: one such run
  rescued 9,091 pages from ciphertext, where a run on an already-warm client
  rescued none. Restart the client before a capture.
- **Never merge the two builds' pages.** The first re-merge did exactly that
  and produced a Frankenstein image: 11% of the code was "recovered" but the
  bytes at a given address came from different builds. It is deleted.
- **A capture is worth nothing without its build.** The runs name their
  build only implicitly; the standing image's build is the one its version
  string says (`2.25.0.0` in the 20.10 image, `2.25.0.1` in the 21.10 one).
- **The addresses still need carrying across.** `dev/build_translate.py`
  matches one build's pages and functions against the next; the function
  level match needs the old build's function list (the 2.25.0.0 captures
  have **no** `.pdata`, so the extents come from the analysis).
- **Naming crosses the builds now (2026-10-22; camera added 2026-10-23).**
  `docs/function-names.csv` carries **186 settled names** (187 lines,
  2026-10-25); the camera rows cover the near/far and FOV getters/registration,
  the plane setter, the mouse-look deltas, the FP clip planes and offset
  transform, the input poll and force-input mode, the replay proto merge, the
  view-params constructor, and the view/projection and FP-pose builders.
   `dev/naming_seed.py` seeds rows from the docs'
  decompiled-path findings and byte-matches the v2full frame into the
  current-build image, confirming every hit at a real Ghidra function begin
  before it writes (since 2026-10-24 the rows tag `Overwatch.exe` at the
  exec address `0x140000000 + RVA`). 88 candidates were refused honestly
  (not matched,
  ambiguous needles, cold windows). The two standing 2.25.0.0 images disagree
  on **4,621 of 10,582** shared nonzero pages, so docs-frame matching reads
  from `module_v2full.bin` only - `docs/open-questions.md` question 12 tracks
  the cause.

## Where we are

- Retail install: `2.25.0.1.154504` (the 21 October captures). The docs were
  written against `2.25.0.0.154088` (the 12-20 October captures); see the
  section above.
- Newest live runs: page-diff `run_20261021_105102` (phase-marked, gate passes)
  and cookie `run_20261021_105145` (same pid, loader base `0x7ffc52d50000`). The
  full-coverage oracle run remains `run_20261021_033126` (15,636 pages, 443
  flips) with cookie `run_20261021_094721`.
- Page-decrypt data artifact: `D:/ow-dumps/pagepak/keystream_current.npy`
  (2,285 current-build rows). The older `keystream.npy` (9,449 rows) is the
  previous build; the `ks_*` probes read the current one.
- Merged images (outside the repo): `D:/ow-dumps/module_after_diff.bin` (v2b
  base `0x7ff7c4d20000`) and `loader_after_diff.bin`; the current-build
  plaintext loader is `D:/ow-dumps/loader_disk_rva.bin`.
- The Ghidra project is `dev/ghidra_run/owmod` (on the SSD, git-ignored, 10
  programs); `docs/ghidra-project.md` says which program answers what.
- On 2026-10-23 `module_v21_clean.bin` was filled in place: **488 `.text`**
  and **13 `.data`** pages from `module_v21d.bin`; **372 `.rdata`**, **16
  `.data`** and **4 `.pdata`** from the on-disk executable (same build,
  `.reloc`-rebased pointers); 6 live pages kept. Function count **92,728**;
  **2,351 `.text` pages remained zero**. Full image/manifest:
  `D:/ow-dumps/module_v21_clean_full.bin` and
  `module_v21_clean_full.manifest.txt`.
- **The complete image is imported.** The Plan A PE
  (`D:/ow-dumps/offline_recover/module_offline.bin`, preferred base
  `0x140000000`, all 12,076 `.text` pages, 201,246 `.pdata` entries) is the
  Ghidra program `Overwatch.exe`, auto-analysed to **151,260 functions**. Read
  a v2b address there as `0x140000000 + (v2b - 0x7ff7c4d20000)`; this closes
  the 2,351-page gap. The names were migrated from `module_v21_clean.bin` to
  `Overwatch.exe` on 2026-10-24, the day that program was deleted.
- The view model, FP camera and animation retarget are the active area.
  Movement, combat, scene and most data systems are usable. The camera's
  per-frame look consumer is still unlocated even though its code pages are
  warm; `docs/formats/camera.md` records the read surface and the boundary.

## What works

- **Container/data:** CASC, BLTE, manifests, content registry, both STU
  versions, typed class registry and field readers, shader groups and code
  blobs. Local locations come only from `data/*.idx`; `indices/` answers CDN
  routes, and `ecache`/`pro`/`shmem` hold no content (measured 2026-10-25).
  The core decode was checked against the client's own functions on
  2026-10-25: Salsa20 (emulated), BLTE framing and block modes, bucket entry
  layout, the 30-byte header (including header-only empty files), and the
  encoding header/section order. Names are in `docs/function-names.csv`;
  the remaining gaps (LZ4 `4`, frame `0x10`, index hashes, `.flavor.info`)
  are listed in `docs/formats/casc.md`.
- **Movement:** trace-measured mover arithmetic, verified by `trace_diff`;
  crouch/air/slope rules measured. The mover component, advance and invoker
  are read in the current build (`RegisterMoverComponent` `0x14017a680`,
  `MoverAdvance` `0x140ca7f30`, `MoverInvoker` `0x1407ffc50`); the per-frame
  step/wish path is still unlocated, and the `+0xc0` field is a static local
  offset (constructors only), not a stored velocity. The install's `.pdata` is
  plaintext and complete; the remaining issue is Ghidra's fragmented function
  boundaries and the state-table/statescript data, not missing code.
  `dev/field_index.py` is the struct-aware finder. The mover verification unit
  is a scaffold until the ramp is read.
- **Combat:** pools, fire intervals, volley figures, projectile motion,
  hitscan. Damage is supplied; damage order/armour mitigation are assumed.
- **Scene/view:** real map geometry, hero models, skeletons, skinning, texture
  decoding; `ow-view` and the `--play` harness.
- **Camera:** world near/far `0.1/16384 m`; world/FP FOV **103 deg horizontal
  -> 70.53 deg vertical at 16:9** (`GetHorizontalFov` -> `FUN_7ff7c5700f50`,
  tan/atan with the 9/16 factor; the camera component's 70.0 is the rounded
  fallback). First-person near/far `0.01/1638.4 m`, separate first-person
  pass, zero-rest drag spring (`64/16/-1`), opt-in look filter
  (`--cam-look-inverse 0.1`). The mouse `K` is read from the on-disk
  executable's `.rdata` (`0.011519`). The camera var surface/registration,
  the FP setup (60 deg fallback, FP clip planes, animation FOV blend), the
  input->command-frame path and the view/projection builders are read. The
  per-frame look application and the shake math are open. Details:
  `docs/formats/camera.md`.
- **View model:** camera-relative placement, animation feed (single clip or
  `--anim auto` with the blend tree), FP hardpoint attachment on bones 44/74
  (the old character-c71 source is `--weapon-fold 1`), dual-pistol hardpoints.
  The per-channel layer sampler now matches for ordinary bones. The 2026-10-23
  Ghidra check still finds **no exact client pose**: the reference retarget,
  animated FP camera pose and the `+0x1350` spring target are unread, and the
  clipped `2.395x` shoulder reference still differs. Details:
  `docs/formats/animation.md`.
- **Animation format:** clip header and packed-u16 rotation/position/scale
  tracks, the general family-agnostic row reader, the FP blend-tree item/tree
  decode and evaluation.
- **Function verification (2026-10-24):** `dev/verify_harness/` decides
  mechanically whether a reconstruction of a client function behaves
  identically to the shipped bytes. The oracle is the real code emulated
  offline over `D:/ow-dumps/offline_recover/module_offline.bin` (the
  complete offline-recovered image, every `.text` page); tier 1
  compiles the Ghidra C from `dev/decompiles/`, tier 2 runs
  `rust_probe.rs` through a subprocess protocol. Five stages: load,
  determinism, differential (return bits, arg writes, undeclared-read
  check), branch coverage on the oracle, mutation of the candidate.
  Digest-bound ledger; `python dev/verify_harness/cli.py selftest` injects
  known defects and requires detection before any unit may be called
  verified. **Verified so far:** `fn/AnimTimeWrap` (`0x7ff7c58b1ef0`),
  `fn/CurveKeySampleDual` (`0x7ff7c58b09d0`), and the Rust probe against
  the AnimTimeWrap oracle. The harness already caught two Ghidra-output
  bug classes (float-conversion-instead-of-bit-load; IEEE ifs instead of
  SSE minss/maxss) and both decompiles were corrected from the emulated
  bytes. Three other zero-callee decompiles are not verifiable as they
  stand (two are comment-only stubs; `DispatchFpCameraSlotTime` has an
  indirect call and an unrecovered jumptable). **Subagent workflow:**
  `dev/verify_harness/AGENT-PROTOCOL.md`. Run all units with
  `python dev/verify_harness/cli.py all` (needs gcc, rustc, unicorn,
  capstone, and the capture on D:).
- **Capture:** protection-aware `dev/live_page_diff.py`; `dev/warm_poll.py`
  fallback; phase marking and encrypted-page logging. The tool list is
  `dev/INDEX.md`.
- **Page-decrypt data:** the 16 KiB cycle, window/draw model and oracle are
  fully characterised.

## The page-decrypt hunt (coverage solved offline)

**Coverage is solved without a capture (2026-10-23).**
`dev/offline_page_recover.py` recovered all **12,076** `.text` pages from the
installed `Overwatch.exe` using the known pad and ciphertext-only scoring, and
copied the install's **plaintext `.pdata`** table (201,246 entries). The
held-out oracle is 2,284/2,284; the warm images match 4,887/4,888 and
5,350/5,351 readable pages. One page keeps a declared 16-byte tail
(`0x540000`). The full image is
`D:/ow-dumps/offline_recover/module_offline.bin` (preferred base
`0x140000000`); `REPORT.md` has the validation. It is imported as the Ghidra
program `Overwatch.exe` (151,260 functions, every `.text` page initialized).
The `w` **picker** is still unread, but it is no longer needed for coverage.

**The data half is solved.** Each on-disk `.text` page is XORed with a 4 KB
window drawn uniformly from one static 16 KiB cycle in the loader's `.rdata`
(`0x1d77f80..0x1d7bf80`, shipped rotated by 19 bytes). All 2,285 current-build
oracle rows verify; a 16-zero splice at `0x53a000` is part of the stream. The
window start is not derived from the RVA, page content, hash, region or a
modular counter; it is a build-time i.i.d. uniform draw, stable across runs
(1,676/1,676 pages match with zero conflicts).

**The code half is live-verified.** The ETW raw qword scan found the candidate's
dword XOR loop `0x144c839` as a leaf PC (6 samples) with `0x1015a99`, the
return from the `call 0x14380e0` at `0x1015a94`, directly above it in the
stack. The chain is `0x144c839 -> 0x1015a99 -> 0x163f6b1 -> 0x1a29511`.
The candidate's post-`VirtualProtect` return `0x14462cb` is also live (26
frames), from the other caller `0x1a0d536`. PadB `0x10c1e20` is live too, but
its dword loops are called from `0x8a4ad0`, a separate path.

**Candidate (LIVE-VERIFIED):** `0x14380e0` (93 KB), which alone holds
eight pad-base arms, two VP indcalls (`0x14462c5`, `0x144df0a`), nine
pad-indexed byte xor leaves, two dword xor leaves and the `and 0x3fff` window
mask `0x144bfc6`. Rival `0x442110` shares the VP slot but has no pad arms,
window mask or load-xor-store leaves. Full watchlist and per-site caveats:
`docs/formats/page-crypt.md`.

**Static refinement.** The pad-base census is ten refs, not three: eight in the
candidate region, one in `0x10b72f0`, one in `0x10c1e20`. All three regions
take three small keys plus a data pointer, so the family may be data/string
decoders. The **live** `.pdata` is intact: `0x14380e0..0x144ee19` is one
function, and its page path is the state at `0x14388e7` -> `0x144968f` using a
descriptor whose `w` field is `edx` at `0x14481c1`. The **main executable's
disk `.pdata` is plaintext** (201,246 entries, byte-identical to the offline
image's at RVA `0x450b000`); the earlier "disk `.pdata` is encrypted" note was
a parser-bound error, and the loader's table is valid at RVA `0x1f00000`
(5,701 entries). Function extents for the main image come from its own
`.pdata`, not from a live capture. `0x10c1e20` is the only region with the
4-byte-granular `and 0x3ffc` pad mask, which fits the measured 4-byte window
alignment better
than the candidate's `0x3fff`. No stored or hashed `w` table was found in the
disk loader, the live loader image, the live main-module image, the private
ghost loader copy, or at any fixed stride in the full heap dump.

**Heap dump (2026-10-21).** `D:/ow-dumps/ow_heap_full.dmp` (13.5 GB) is a
same-series full dump: main base `0x7ff6a2d80000`, loader `0x7ffc52d50000`.
It contains a **private ghost copy of the loader** at `0x1ba39390000`: the pad
and `.eidsig` are present, `.eid` is byte-identical to disk, `.data` has
runtime pointers. The raw pad is also copied there unrotated. A fixed-stride
search for the `w` run over the full dump and over the ghost `.data`/`.rdata`/
`.eid`/`.pdata` found nothing; the `w` values are not stored in page order at
a fixed stride. The dump does hold a **page metadata table** at
`0x1ba36b90000` (1,845 records of 0x90 bytes; type B starts with the page VA,
type A carries it at `+0x88`). The likely 5th-argument table is at
`0x1ba37437d58` - 1,439 pointers to type-A records. Its selector is a hash:
`0x1ce59b0` wraps `0x3fa280`, which hashes `rdi` via `0x1494090` and returns
`hash % count` (`div` at `0x3fa3b3`), so it is a hash table, not a page-index
array. For the 860 records whose page has an oracle `w`, plain shifts,
`% 4096`, xor/add/mul, `fmix64`, seeded Murmur3/CRC32, every field and a
constant xor/sub/add of any 16/32-bit field all fail (best 3/860). `w` is
computed or decrypted inside the VM, not stored plainly in the record. The
selector chain is VM-wrapped: `0x1ce59b0` -> `0x3fa280` -> `0x1494090` ->
`0x2511e0`, with `0x3fa280` also calling the 22 KB `0x40cd87`; the native
page loop `0x144c839` is reached from the same caller, so `w` crosses from VM
state into the native descriptor.

**Tier 3 boundary.** The `w` **derivation** is inside the loader's own
obfuscated VM/dispatcher (`0x2511e0` dispatcher, 22 KB
`0x40bee0..0x411536`; `0x40cd87` is a mid-function handler). The earlier
"Denuvo VM" label was wrong; it is the loader's own code. The XOR transform
is exact and the blind solver recovers this build's pages without the oracle;
the picker itself is not recovered. A new build needs a fresh pad extraction
and a rerun of the blind solver; no live oracle is required.

**Live evidence.** The newest phase-marked run
`D:/ow-dumps/live_page_diff/run_20261021_105102` (pid 4100, 90 s, 1,597 flips;
phases `{0:1449, 1:51, 2:97}`) passes the gate as `ATTRIBUTABLE-CANDIDATE`.
That is necessary, not sufficient: `events.csv` records protection flips only -
there is still no faulting-rip, handler or VP-argument source. The earlier run
`run_20261021_104439` was unmarked (120 flips, all phase 0).

**Levers:** an operator-approved, phase-marked, fault-synchronous run remains
the definitive source for the faulting rip/first handler and the `w` value.
The ETW CPU trace is now tested and hit: `dev/etw_cpu_capture.ps1` produced
`cpu_20261021_124450.etl`, and `dev/etw_raw_scan.py` / `dev/etw_count.py`
found the live candidate leaves. The minidump path is tested too: on the game
the private-RW default returns `E_INVALIDARG`; `--normal` (446 KB) and `--full`
(13.5 GB) work. Per AGENTS.md a new live capture is the operator's decision
every time.

### Runbook (when the operator is ready)

0. Read the fresh loader base from the new run's `meta.json` and resolve the
   watchlist with `python dev/faultsync_resolve.py <loader_base_hex>`.
   Never reuse an absolute watchlist from another session: those addresses are
   per-session, and only the RVA watchlist travels.
1. Confirm the run is phase-marked and passes the gate:
   `python dev/faultsync_correlate.py <new-run>/events.csv`
   must print `ATTRIBUTABLE-CANDIDATE`, not `UNATTRIBUTABLE`.
2. Capture 90 s of practice-range gameplay, first-person, marking
   menu -> hero select -> in-range with Enter:
   `python dev/live_page_diff.py --seconds 90 --out D:/ow-dumps/live_page_diff
   --all-modules --surge-seconds 90 --include-data --focus-file
   D:/ow-dumps/live_page_diff/run_20261021_033126/gaps.focus`
3. Run `python dev/ldr_cookie_grab.py` in the same session; it dumps
   `iat.bin` + `stubs.bin` (the live heap-thunk band) and now `teb.bin` +
   `peb.bin`. Never pass it flags expecting a dry print - `--help` performs a
   grab.
4. A hit counts only on same-fault flip correlation. A CAND VP hit counts only
   with no same-fault RIV-A hit (`0x4431cf` / `0x448460`); a hit on both stays
   `UNATTRIBUTABLE`. Record a same-session heap/module map for the
   `0x1BB72DCxxxx`-style thunk band; the band is per-session.
5. Combined ETW + heap session (operator choice, 2026-10-21):
   - elevated: `powershell -ExecutionPolicy Bypass -File dev/etw_cpu_capture.ps1`
     **before** launching Overwatch; play 30-60 s in the range; Enter to stop;
   - normal shell: `python dev/dump_process.py Overwatch.exe
     D:\ow-dumps\ow_heap_full.dmp --full` while the game is still in the range
     (the full dump freezes the game for ~20 s; the private-RW default fails on
     the retail process);
   - scan: `python dev/etw_raw_scan.py <etl> --base <loader_base>` and
     `python dev/scan_dump.py <dmp> --pattern w` (then `pad`, `eid`).
   The ETL names a sampled applier PC if caught; the dump can close the
   build-stable `w` storage.

Do not re-run read-only polls: sessions 18/19/22/26-28 exhausted them.

## Do we have the captures we need?

Yes, with one exception, and it is measured rather than assumed
(`docs/capture-coverage.md`):

- **Every code address the open questions need is warm** in the standing
  image - 41 of 41 checked, across animation, camera, movement, combat and
  physics. The open questions are analysis work.
- **The standing image was stale.** Re-merging the 17 existing runs (no new
  capture) takes the client's unread `.text` from 21% to 11% and makes
  `.pdata` - the function table of the whole client - fully readable. The
  result is `D:/ow-dumps/module_after_diff_v2.bin`.
- The two capture gaps the docs recorded (the mover component's four methods,
  the volley class's region) are **closed**.
- **No capture is needed for page coverage any more:** Plan A recovered all
  `.text`/`.pdata` offline; the picker itself remains a VM-reversing target,
  not a blocker.
- **The Ghidra step is done:** the offline image is the program `Overwatch.exe`
  (preferred base `0x140000000`), auto-analysed; the 2,351 pages that were cold
  in `module_v21_clean.bin` now read as code. `docs/ghidra-project.md` has the
  address rule.

## Open problems

The queue with the full detail is **`docs/open-questions.md`**. In order:

1. **Page-decrypt picker.** Coverage is solved offline by
   `dev/offline_page_recover.py`; the applier is live-verified as `0x14380e0`
   (loop `0x144c839`, caller `0x1015a94`). The faulting rip/first handler and
   the `w` derivation remain unread, but they no longer block reading the
   executable (tier 3 VM/dispatcher; not Denuvo).
2. **FP pose / reference-skeleton retarget.** `retarget_probe 3` settles the
   bind question: character rig **200**, FP rig **79**, **64 common ids**,
   and the binds are **identical** (worst translation delta 0.0059 m at vm
   bone 25). The Pose2d dataflow VM remains unread.
3. **Gun attachment.** Hooks are identity/zero on bones 44/74. The `c71` fold
   gives weapon world 44 `(0.11, 1.44, 0.35)` forward `(0.98, 0.14, 0.16)`
   and 74 forward `(-0.87, 0.40, -0.29)`; the FP rig folds to the character
   world. The convergence is the hook bind, not a missing retarget.
4. **FP camera transform / component consumer.** The runtime
   `STUFirstPersonComponent` objects are found in the full dump
   (`64/16/-1` then `50/1.25/0.875/0.8` at heap VA `0x1bb9614d97c`; other
   heroes show `65/...` and `47/...`). The class hash `0x9d28963f` maps to
   **type index `0x35`**; the shared vtable `0x7ff7c7ef5f30` is generic, so
   the consumer is a system keyed by type `0x35`/entity offset, still unread.
   The consumer path *is* found: `FpViewmodelConsumer` (RVA `0xd2f060`) gets
   type `0x35`, resolves the view-model entity/model, and calls the bind
   `FUN_00d2faa0` (RVA `0xd2faa0`). The bind stores the refs but does not read
   `m_position`/the scalars. The view-model class vtable is RVA `0x3115ef0`;
   slots `+0xe0` (`0xd31bf0`) and `+0xe8` (`0xd322f0`) are the per-frame
   updates and walk six `0x110`-stride animation slots (`FUN_00d29c60` /
   `FUN_00d3a020`) - an animation event/state player, not the pose or camera.
   The FP camera pose/FOV path is now read: `UpdateFirstPersonCameraPose`
   (`0x7ff7c56b7b90`) builds the pose from the FP animation basis/position, and
   `ApplyFirstPersonCamera` (`0x7ff7c568c5b0`) applies it. The type-0x35
   consumer for `m_position`/the scalars is still unread, and which scalar
   feeds the pose FOV (`FUN_7ff7c5700f50`) is unread. `retarget_probe` c71 worlds:
   hands 14 `(0.155,1.367,0.091)`, 55 `(-0.210,1.364,0.092)`; weapons 44
   `(0.113,1.441,0.348)` fwd `+x`, 74 `(-0.143,1.506,0.350)` fwd `-x`; the bind
   hardpoints are `+/-0.44,0.93`. At eye `1.75` / FOV `29.4` the grips are
   outside the view (only tips); the component's `1.25` eye height with FOV
   `50` brings hands and guns in. Test `--fp-eye 1.25 --fp-fov 50`.
5. **World camera near/far and FOV.** Read and in use: `0.1/16384 m` and
   103 deg horizontal -> 70.53 deg vertical at 16:9 (`FIT-CAM-04` removed).
   The same FOV is used by the FP pose. The projection builder is read; its
   view-row and projection x signs cancel (`docs/formats/camera.md`).
6. **Encrypted pages.** `encrypted.csv` lists pages never read as plain; a
   targeted second run with `--focus-file gaps.focus` still recovers
   stragglers.
7. **Broader gaps:** material keys (hero body/map props), per-shot damage,
   steep-slide rule, v1 STU (synthetic only), replay Zstd payload, Wwise audio
   decode, bot AI/abilities.

## Evidence boundary

Read values cite their capture and address in `docs/formats/`. Fitted,
assumed, tier-3 and stand-in behaviour is listed in `docs/fitted-ledger.md`,
each with an id that also stands next to the code. Current entries include the
drag-spring form, the FP camera drop/back, the weapon-hook skip, the
single-clip playback heuristic, eye height, the pitch clamp, the mouse scale,
the sandbox collision, damage rules and combat defaults. Do not move one of these into
"read" without an address or a capture that settles it.

## Key files

- `docs/open-questions.md` - the work queue: what is open and what would
  settle it.
- `docs/ghidra-project.md` - the decompiler project: its programs, their base
  addresses, and how an address in a doc maps onto one.
- `docs/formats/page-crypt.md` - page-keystream data, candidate applier,
  watchlist and runbook.
- `docs/formats/animation.md` - skeleton, clips, evaluator, FP blend tree,
  retarget, camera and view-model findings.
- `docs/formats/input-camera-audio.md` - camera config surface and defaults.
- `docs/formats/rendering.md` - renderer/RHI findings.
- `docs/formats/character-controller.md` - mover pipeline and measured rules.
- `docs/fitted-ledger.md` - every fit, assumption and stand-in.
- `docs/systems-coverage.md` - every client system and next anchor.
- `dev/INDEX.md` - every analysis tool and the question it answers.
- `crates/ow-view/src/play.rs` - camera, drag spring, animation feed.
- `crates/ow-view/src/main.rs` - renderer and first-person pass.
- `crates/ow-tank/examples/blendset_probe.rs` - FP item/curve probe.

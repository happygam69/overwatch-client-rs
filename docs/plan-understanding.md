# Plan: make the project understandable, and clean it up

Goal: a reader (the operator, or a new contributor) can answer four
questions for any system in at most three hops:

1. What does the client actually do (current understanding)?
2. How do we know (evidence class, capture/address, or data source)?
3. Where does our implementation live (crate/module/function)?
4. How do I verify or reproduce it (test/probe command)?

Everything below is about making existing knowledge legible; it adds no
new claims.

## Status

- [x] Step 1 - `docs/README.md`, `glossary.md`, `architecture.md`,
      `artifacts.md`, `tools.md`, `history/README.md`.
- [x] Step 2 - current-state headers on the eight big formats docs
      (animation and page-crypt already had them).
- [x] Step 3 - `systems-coverage.md` is a scannable index;
      `history/coverage-detail.md` keeps the full ledger.
- [~] Step 4 - dev cleanup started: large unreferenced dumps to
      `D:/ow-dumps/devdata`, one-offs to `dev/retired`, `dev/README.md`.
      Remaining: more one-off consolidation after review.
- [x] Step 5 - probes/Ghidra consolidated: one parameterised
      `McpDecompile.java` (+ `McpCallers`) tracked in `docs/ghidra/`, old
      per-run scripts in `ghidra_scripts/retired/`, workflow in
      `docs/tools.md`.
- [x] Step 6 - `ow-tank::firstperson` reads the FP component once (probe and
      viewer both use it, class defaults included); crate docs updated.
- [x] Step 7 - reconciliation pass: animation Current state aligned with the
      latest consumer findings, README works/open list corrected, fitted-ledger
      playback row updated.
- [x] Step 8 - Verify lines added to the eight system docs; animation and
      page-crypt already carry Commands/runbook sections.

## Diagnosis of the current state

- Knowledge sits in three layers that are mixed together: **chronological
  research logs** (`formats/*.md`), **living ledgers** (`STATUS-handoff`,
  `systems-coverage`, `fitted-ledger`), and **code**. The logs are valuable
  but contain retractions inline, so current truth is hard to see.
- Overlapping plan documents: `systems-plan.md`, `roadmap.md`,
  `rewrite-plan.md`, `systems-coverage.md`, `STATUS-handoff.md` all carry
  project state with different scopes.
- `systems-coverage.md` cells are multi-paragraph; it is accurate but not
  scannable.
- `dev/` holds **253 files**: one-off variants (`clip_kinds1..7`, ...),
  superseded probes, one-off patch scripts, and multi-MB dumps
  (`all_guids.txt` 28 MB, `consumers_mover.txt` 21 MB).
- Probes are spread across `ow-tank/examples`, `dev/`, and
  `C:\Users\Happy\ghidra_scripts` (six `McpAnimChainN` copies and more).
- No glossary; no index of `D:/ow-dumps` / `E:/ow-dumps` artifacts; no
  single place naming the crates' data flow.
- Some current source files (e.g. `crates/ow-view/src/play.rs`,
  `anim_tree.rs`, `ow-tank/src/blendtree.rs`) are untracked in git
  (whitelist allows them; they were never added). Commit decisions are the
  operator's.

## Workstreams

### 1. Documentation architecture

New top level under `docs/`:

- `README.md` - the map: reading order, conventions (evidence classes,
  tier 3, fitted), links to everything.
- `glossary.md` - plain-language terms: STU, RVA/v2b, standing image,
  session image, blend tree, sampler, retarget, keystream, tier 1/2/3,
  fitted/assumed/stand-in, capture, hot/flip/diff.
- `architecture.md` - crates and data flow:
  install -> CASC -> STU -> systems (mover/weapons/animation/camera) ->
  sim -> scene -> viewer; plus the evidence pipeline
  (captures -> session images -> Ghidra).
- `artifacts.md` - index of every `D:/ow-dumps` and `E:/ow-dumps`
  artifact: name, what it is, how it was built, which doc uses it.
- `tools.md` - the canonical tool table (see Workstream 3).
- `systems/<system>.md` - one doc per system, each with:
  1. "Current understanding" (one paragraph, no history);
  2. status table: read / fitted / tier 3, and the evidence class;
  3. data anchors (assets, addresses, captures) and code entry points;
  4. open questions and the next anchor;
  5. link to the history.
- `history/<system>.md` - the existing chronological entries moved
  verbatim, dated, retractions included; no current claims live here.
- `history/sessions.md` - the STATUS session log moved here.

Changes to existing files:
- `formats/*.md`: keep the current-state section at the top (rewritten as
  the truth today); move the dated narrative to `history/`. This is the
  biggest single readability win.
- `systems-coverage.md`: one line per system (state, doc link, next
  anchor); details live in `systems/`.
- `STATUS-handoff.md`: rewrite as <=1 page - current state, blockers,
  next three actions.
- `README.md`: accurate "what works / what does not" + quick start with
  exact commands.
- `fitted-ledger.md`: keep; sweep for stale rows in the same pass.

### 2. Code organisation

- Each crate's `lib.rs` gets a `//!` block: purpose, inputs (install
  files/captures), outputs, what is read vs fitted.
- Extract shared readers into libraries so probes and the viewer use the
  same code:
  - `ow-tank::firstperson` - `STUFirstPersonComponent` values (currently
    duplicated in the probe and play.rs).
  - the retarget chain builder currently in `ow-view::anim_tree` is game
    logic; consider `ow-tank::anim` (decision after the docs pass).
- Keep `ow-view` thin: harness (`play`), renderer (`main`), animation
  evaluation (`anim_tree`).
- Name conventions documented in the glossary: "v2b" = standing image
  space; "session image" = one run; "RVA" = module-relative.
- Delete dead/superseded code paths only when the docs no longer cite
  them; otherwise mark deprecated.

### 3. Tooling cleanup

- `dev/` split:
  - `dev/tools/` - active, documented, read-only analysis tools;
  - `dev/retired/` - kept-for-history one-offs (or delete);
  - big data dumps move to `D:/ow-dumps/devdata/` and are indexed in
    `docs/artifacts.md` (nothing large stays in the repo).
- Canonical tool list (to be finalised during cleanup):
  - captures: `live_page_diff.py`, `tight_report.py`, `warm_merge.py`,
    `session_merge.py`, `coverage_gap.py`;
  - page cipher: `pagepak_build.py`, `page_crypt_*.py`, `ks_linear.py`,
    `ks_key_test.py`, `ks_hash_test.py`;
  - images/pointers: `trace_chain.py`, `dump_qwords.py`,
    `dump_object.py`, `imm_scan.py`, `xref_rva.py`, `pdata_lookup.py`;
  - loader: `stub_extract.py`, `stub_filter.py`, `ldr_*.py`;
  - animation: `anim_chain_args.py`, `anim_map_check.py`,
    `dump_clip_sections.py`.
- `dev/tools/README.md` (or `docs/tools.md`): tool -> question it answers
  -> inputs -> outputs, and the read-only guarantee.
- Probes (`ow-tank/examples`): keep only documented diagnostics
  (`stu_probe`, `blendset_probe`, `blendtree_probe`, `retarget_probe`,
  `clip_chan_probe`, `fpcomp_probe`, `refskel_probe`, `refclip_probe`);
  retire duplicates; every file gets a header saying what it answers and
  which doc cites it.
- Ghidra scripts: one parameterised `McpDecompile.java` (args file +
  output path) replacing the `McpAnimChainN` copies, plus `McpCallers`;
  document the headless workflow (project paths, stripped convention,
  decompile timeouts) in `docs/tools.md`.

### 4. Evidence hygiene

- One reconciliation pass over `formats/`: mark stale claims as retracted
  in history, ensure the current section matches the code and ledger.
  Known items: FP retarget (hierarchy fold; no basis change), FP camera
  data read (component/near-far/bob; consumer unread), page erasure,
  blend tree implemented, the "captures lack animation" narrative closed.
- `fitted-ledger.md`: every current fit present; resolved rows removed.
- Every system doc ends with the exact verification command.

### 5. Git hygiene

- List untracked source files; with the operator's approval, stage them
  (no commit unless asked).
- Keep the whitelist semantics; verify no large dump is whitelisted.

## Execution order (each step revertable)

1. Add `docs/README.md`, `docs/glossary.md`, `docs/architecture.md`,
   `docs/artifacts.md` (pure additions; no content moves yet).
2. Split `animation.md` and `page-crypt.md` into current + history; then
   the camera section of `input-camera-audio.md`.
3. Slim `systems-coverage.md`; rewrite `STATUS-handoff.md`; archive the
   session log to `history/sessions.md`.
4. dev cleanup: move dumps out, retire one-offs, write the tools index.
5. Probes/examples cleanup + canonical Ghidra script + workflow doc.
6. Code extraction (`firstperson`) + crate docs.
7. Reconciliation sweep of `formats/`, ledger, README.
8. Run all tests; write the "how to verify" lines into each system doc.

## Acceptance criteria

- A new reader starts at `docs/README.md` and reaches any system's
  current truth, evidence, code and verification command in <=3 hops.
- No document mixes history with current claims; retractions live only in
  history.
- `dev/` contains only documented tools; no file above ~1 MB; no
  one-off patch scripts.
- One canonical decompile script; the workflow is documented.
- `README.md` and `STATUS-handoff.md` fit on one screen each and are
  accurate.
- Every animation/camera claim has a tier and a reproduce command.

## Out of scope

- No new client behaviour is derived during this cleanup.
- Captures and decompiled artifacts stay outside the repo; they are
  indexed, not moved into it.

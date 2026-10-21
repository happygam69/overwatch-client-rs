# Architecture

Two pipelines meet in this project: **data** (what the install says) and
**behaviour** (what the client does). Both are reconstructed read-only.

## The two pipelines

```text
install files (CASC)                       client process (read-only)
        |                                          |
     ow-casc  (keys -> bytes)                dev/live_page_diff.py
        |                                          |
     ow-tank  (bytes -> assets)                run/ pages + warm.csv
        |                                          |
   STU / models / clips / blend trees      session_merge.py -> session image
        |                                          |
        +---------------------+             warm_merge.py -> standing image
                              |                    |
                         ow-sim (rules)      Ghidra headless (stripped import,
                              |               args files -> .c exports)
                         ow-scene (map/skin)       |
                              |                 docs/probes/decompiles
                         ow-view / ow-game
```

- **Data** answers: values, schemas, registries, geometry, clip data.
  `ow-tank` exposes it; `ow-sim` consumes it as plain specs.
- **Behaviour** answers: how the client computes. Captures give decrypted
  pages and runtime state; Ghidra over coherent images gives code. Every
  behavioural claim names its capture or session.

## Crates

| crate | role | inputs | key modules |
|---|---|---|---|
| `ow-casc` | container layer: manifests, BLTE, CASC index | install | `install`, `container`, `encoding`, `index` |
| `ow-stu` | STU v1/v2 reader: instances, fields, typed values | asset bytes + class registry | `types`, `lib` |
| `ow-tank` | asset semantics: GUIDs, STU classes, models, skeletons, clips, blend trees, entities, weapons, settings | asset bytes | `guid`, `classreg`, `classes`, `entity`, `blendtree`, `clip`, `skin`, `skeleton`, `model`, `hardpoint`, `fieldnames`, `keytable` |
| `ow-sim` | the simulation: movement, combat, projectiles | plain specs from `ow-tank` | `movement`, `rules`, `trace` |
| `ow-scene` | map + model assembly: chunks, placeables, skinning | install via `ow-tank` | `lib` (scene build, `Skin`, `view_model_matrix`) |
| `ow-view` | wgpu viewer and play harness | `ow-scene` + `ow-tank` + `ow-sim` | `main` (renderer), `play` (harness/state), `anim_tree` (blend-tree evaluation), `hud` |
| `ow-game` | Bevy shell over the same scene | `ow-scene` | `main`, `sandbox`, `sim` |

Binary/example entry points live in `crates/ow-tank/src/bin` (`hero`,
`model`, `anim`, `stu`, `look`, `weapon` helpers) and
`crates/ow-tank/examples` (read-only probes). The canonical tool list is
[`tools.md`](tools.md).

## Runtime state in the viewer

- `ow-scene::Scene` owns geometry, instances and the skeleton (`Skin`).
- `ow-view::play` owns the player state (mover, combat, pools), the camera
  and the animation selection (`--anim`): a single clip (`Anim::Blend` /
  `Anim::Bones`) or the real blend tree (`Anim::Tree`).
- `anim_tree::TreeAnim` reads the item's clips/slots/tree once, then
  evaluates per frame: category -> base clip, layers, the >0.5 sampler
  gate, chain composition for dropped ancestors.
- The renderer draws the world and a separate first-person pass (FP
  projection, depth clear). Camera framing knobs and their status are in
  `fitted-ledger.md`.

## Evidence pipeline in detail

1. **Capture** (`dev/live_page_diff.py`, read-only):
   watches protections, reads pages on transitions, records
   `pages/`, `warm.csv`, `encrypted.csv`, `events.csv`, `meta.json`.
2. **Coherent images** (`dev/session_merge.py`): one run at one base, for
   pointer chasing. Merged images (`dev/warm_merge.py`) union pages across
   sessions and must **not** be used for pointer values (mixed bases).
3. **Standing image**: `D:/ow-dumps/module_after_diff.bin` (base
   `0x7ff7c4d20000`), `module_after_stripped.bin` for Ghidra (rebased
   `-0x10000`).
4. **Ghidra headless** (`E:/ghidra/hproj*`, script
   `McpDecompile.java`): import the stripped image, decompile function
   ranges listed in an args file, export `.c` into `D:/ow-dumps`.
5. **Probes** cross-check data and runtime values (see `tools.md`).

The loader (`Overwatch_loader.dll`) is **plaintext on disk**; it is
indexed separately (`loader_disk_rva.bin`) and needs no capture.

## Address conventions

- `v2b address` = standing-image base `0x7ff7c4d20000` + RVA; file offset
  in a flattened image equals the RVA.
- Session address = `v2b + (session base - 0x7ff7c4d20000)`. Session bases
  are in each run's `meta.json`.
- Ghidra stripped addresses = `RVA - 0x10000` (the first 64 KiB are
  removed on import).
- The loader uses its own image bases (disk `0x180000000`, captured
  sessions differ); loader RVAs are named explicitly.

## Read vs fitted, enforced

Every module says which side it is on. Read: STU readers, clip decode,
blend-tree graph, sampler rule, FP component values, the retarget fold.
Fitted/assumed: FP camera framing, Pose2d driver inputs, LayerChild weight
source, world near/far. The inverse index with reasons is
[`fitted-ledger.md`](fitted-ledger.md); the evidence classes are in
[`provenance.md`](provenance.md).

## Tests and verification

`cargo xtask verify` is the gate: fmt, clippy (warnings are errors), the
tests, the tool index and the fitted-ledger check. `--fast` leaves out the
tests that read the install. Behind it:

- `cargo test --release -p ow-casc -p ow-tank` - data layer (the `*_real`
  tests read the install and skip themselves when there is none).
- `cargo test -p ow-view` - harness camera/spring math.
- `cargo test -p ow-scene` - skin/pose gates.
- Movement/combat: `trace_diff` over golden traces (see
  `formats/character-controller.md`).
- Eye-check: `cargo run --release -p ow-view -- --play --hero 3 --anim auto`.

## Repo layout

```text
crates/     the workspace (see table)
docs/       current state, history, plan, artifacts (this tree)
dev/        the analysis tools (tracked); their output is not
D:/E: dumps captures, images, decompiles (outside the repo)
```

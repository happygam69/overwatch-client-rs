# Artifacts index

Captures, images, decompiles and reports live **outside the repository**.
This file is the map: what each artifact is, how it was produced, and which
doc leans on it. Nothing here ships with the project.

## Module images (main exe)

All are flattened images at the v2b base `0x7ff7c4d20000` unless noted.

| file | what it is |
|---|---|
| `module_after_diff.bin` | **the standing image**: union of 2026-10-20 sessions, base `0x7ff7c4d20000`, 76,947,456 B |
| `module_after_diff_20261020a.bin`, `_20261020b.bin` | backups taken before the 2026-10-20 refreshes |
| `module_after_diff_20261019.bin` | older standing image, 70,123,520 B |
| `module_v2full.bin` | older short flatten (`0x42ea000`), imported by the GUI Ghidra project; pages beyond its end are missing |
| `module_warm.bin`, `warm_best.bin` | older cumulative warm captures |
| `module_mapped.bin`, `module_v2.bin` | older full-size captures (100 MB images) |
| `module_all_20261020_fixed.bin` | all-module merge from the 2026-10-20 sessions |
| `module_after_stripped.bin` | standing image rebased `-0x10000`, the headless import |
| `session_run_20261020_*_Overwatch_exe.bin` (+ `.json`) | **session-coherent** images for the three gameplay runs, with base sidecars (`dev/session_merge.py`) |

Session bases: 124815 `0x7ff67c0c0000`, 113932 `0x7ff658ff0000`,
103512 `0x7ff7a68e0000`.

## Loader images

| file | what it is |
|---|---|
| `loader_disk_rva.bin` | **the on-disk loader placed at its RVAs** (`E:/Overwatch/_retail_/Overwatch_loader.dll`); the loader is plaintext on disk, so this needs no capture |
| `loader_after_diff.bin`, `_20261020b.bin` | merged captures of the loader at runtime (some pages are zero/erased) |
| `loader_all_20261020.bin` | all-module loader merge from 2026-10-20 |
| `loader_warm_stripped.bin` | captured loader rebased `-0x10000` (headless import) |
| `ldr_flat_rt.bin` | loader runtime flatten used by the loader-band sweep |

## Page cipher / keystream

| file | what it is |
|---|---|
| `pagepak/keystream.npy` | `[12182, 4096]` per-page keystream (`disk XOR plain`), 9,449 pages filled (77.6%) |
| `pagepak/keystream_meta.json` | coverage, sources, disk `.text` layout, the equality-across-sessions note |
| `page_crypt_survey.csv` | per-page cipher survey from the battery |
| `dev/ks_linear.py`, `dev/ks_key_test.py`, `dev/ks_hash_test.py` | keystream tests (BM complexity; standard cipher/hash schemes - all negative) |

## Decompiled exports

Produced by headless Ghidra argument files (`anim_args*.txt`) and
`McpDecompile`/`McpCallers` scripts.

| file | what it is |
|---|---|
| `anim_chain.c` .. `anim_chain7.c` | animation chain rounds: samplers, builder, pose matrix, attribute descriptors, Pose2d drivers, wrapper/class schema, the evaluator candidate |
| `anim_callers.c` | callers of the animation helpers (the node update is the only sampler caller) |
| `anim_camera.c` | renderer-window decompile (55 functions): **control-flow-flattened junk**, kept as a negative result |
| `loader_tls.c` | loader TLS callbacks + entry (disk loader, base `0x180000000`) |
| `ldr_band111.c` | 111 loader-band functions (obfuscated but partly readable) |
| `ldr_band_decomp.c` | whole-loader `.text` export (long-running; grows) |
| `ldr_cluster.c` | loader cluster export |
| `hless_*.log` | headless run logs (created/failed counts, pcode warnings) |

## Region wraps (small PEs for Ghidra)

| file | what it is |
|---|---|
| `range_code.exe`, `range_code2.exe` (+ `.bin`) | range-capture wraps for cold pages |
| `anim_reg2.exe`, `anim_reg3.exe`, `anim_live.exe` (+ `_region.bin`) | animation region wraps |
| `eval_region.exe`, `helper_region.exe` | evaluator/helper region wraps |
| `clipobj.exe`, `clipobj.bin` | clip-object runtime wrap |
| `anim_region.exe`/`.bin`, `anim_wide.bin`, `band_raw.bin` | older region extracts |
| `emu_stack.bin` | emulation scratch |

## Heap and runtime state

| file | what it is |
|---|---|
| `ow_heap_full.dmp` (13.5 GB), `ow_heap.dmp`, `ow_heap_norm.dmp` | full process heap dumps (runtime state; not code) |
| `module_heap.bin` | heap-region capture |
| `pose_trace.bin` | live pose matrices trace (the representation-agnostic hunt) |
| `veh_hunt.py`, `veh_hunt.json` | VEH-handler hunt probe + result |

## Clip and asset dumps

| file | what it is |
|---|---|
| `asset_0a0000000000*.bin` | whole clip assets written by `ow-tank-anim --raw` (many samples) |
| `pc_0x0a000000000000*.bin` | per-clip payload extracts |
| `curve_table_1024x4.bin` | the static 1024xvec4 curve/noise bank from the module |
| `model_11f.bin`, `model_137_fp.bin`, `model_reaper_fp.bin` | model assets for skeleton/probe work |
| `clipobj.bin` | loaded clip-object copy |
| `stubs_113932.csv` | the 10,498-entry class-factory stub table from run 113932 |

## Reports and merges

| file | what it is |
|---|---|
| `warm_merge_20261020c.csv`, `warm_merge_anim_20261020e.csv`, `warm_merge_loader_20261020d.csv` | per-page merge reports (which pages were added/replaced) |
| `gap_124815.csv`, `gap_113932.csv`, `gap_103512.csv` | coverage-gap audits: cold `.text` ranges + `.pdata` function counts per run (`dev/coverage_gap.py`) |
| `module_all_20261020_fixed.csv`, `loader_all_20261020.csv` | all-module sweep results |
| `page_crypt_survey.csv` | cipher survey output |
| `selftest*.txt`, `new_code_*.txt`, `warm_best.csv` | capture-tool self-tests and page-change runs |

## Eye-check images (PNG)

`hero*.png`, `arms*.png`, `viewmodel*.png`, `fp_*.png`, `vm_*.png`,
`final_*.png`, `sandbox_*.png`: windowed screenshots used for eye-checks
(pose, framing, guns). They are evidence of *what was seen*, not game data.

## Capture runs (`E:/ow-dumps/live_page_diff/`)

| run | contributed |
|---|---|
| `run_20261020_101548` | first session capture (pre-tight-tier) |
| `run_20261020_103512` | gameplay; 12,701 main pages, base `0x7ff7a68e0000` |
| `run_20261020_113932` | gameplay; 14,247 main pages, base `0x7ff658ff0000`; class-stub table |
| `run_20261020_122137` | tier flags lost by the shell (`tight_cli` meta used to diagnose) |
| `run_20261020_123427` | tier built 0 pages (base-subtraction bug, fixed) |
| `run_20261020_124815` | tight tier success: 1,510 loader pages, 1,188 plaintext accepted (831 `.eid` + 357 band + 163 gap pages); base `0x7ff67c0c0000` |

## Ghidra projects and scripts

- `E:/ghidra/hproj/hmod` - captured loader (band work, whole-text export).
- `E:/ghidra/hproj2/hmod2` - loader band 111 export.
- `E:/ghidra/hproj4/hmod4` - standing main image (`module_after_stripped.bin`); animation/camera rounds.
- `E:/ghidra/hproj5/hmod5` - **disk loader** (`Overwatch_loader.dll`, base `0x180000000`); TLS/entry.
- `C:/Users/Happy/ghidra_scripts/McpDecompile.java` / `McpCallers.java` -
  the canonical headless scripts (tracked copies in `docs/ghidra/`); the old
  per-run scripts are in `ghidra_scripts/retired/`.

## Moved development dumps (D:/ow-dumps/devdata/)

Large text/log outputs that used to clutter dev/ now live here:
ll_guids.txt (28 MB), consumers_mover.txt (21 MB),
ind_broadphase_v2b.log (70 MB), mover_offset_funcs.txt,
ind_dmworld_v2b2.log, material_guids.txt, mamount_hits.txt,
weapon_raw.csv, ange_chunk.bin, heap_ptr_runs.txt,
mover_values.txt, entity22.raw.txt, ind_dmworld_v2b.log.

Referenced dumps intentionally stay in dev/ because docs cite them:
class_registry.txt, class_descriptors.txt, class_inheritance.txt,
physics_funcs.txt, mover_body_funcs.txt, domino_schema.txt,
component_router_paired.txt.

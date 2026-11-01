# Tools

All analysis tools are **read-only**: they open the install and the Ghidra
project, never write to the game.

- **[`../dev/INDEX.md`](../dev/INDEX.md)** — every tool in `dev/`, generated
  from the tools' own headers. The gate checks it is current.
- **`dev/verify_harness/`** — differential verification of client functions:
  the oracle is the client's own bytes emulated offline (`Overwatch.exe`); the
  candidate is Ghidra C or a Rust probe. Five stages (load, determinism,
  differential, coverage, mutation), digest-bound ledger, selftest.
  `python dev/verify_harness/cli.py all` runs every unit; `casc` runs only
  the CASC units and fails if one is not verified;
  `selftest` proves the verifier still detects injected defects. `all` still
  includes the inconclusive `CurveKeySampleDual` unit and the parked `ow-sim`
  units; `casc` is the green CASC gate. The unit format and the five stages
  are in `dev/verify_harness/runner.py`; the mutation stage's measured wall
  time and its quality proof are in `dev/verify_harness/PERFORMANCE.md`.
  `--jobs N` sets how many mutant probes run at once (default 4) and
  `--mutation-timeout S` bounds one mutant probe call (default 10 s; the
  baseline candidate keeps 300 s). Neither flag skips a stage or weakens a
  gate.
- **`dev/gh.py`** — the Ghidra MCP bridge client used by the naming tools.
- **`dev/gh_apply_names.py`** — applies `docs/function-names.csv` to the
  Ghidra program.
- **`dev/field_index.py`** — struct-aware field-access index over the complete
  image; separates the mover component, character body and `dmBody`.
- **`dev/pdata_lookup.py`** — `.pdata` function bounds for an RVA.
- **`dev/audit_image.py`** — the ground-truth audit: `.pdata` facts from the
  oracle image, bridge counts, and the name-table reconciliation
  (`--registry` for the full function-registry sweep).
- **`dev/corpus.py`** — the standing corpus, inside the project at
  `dev/data/corpus` (ignored): `build` a `.pdata` range, `show`/`grep`/`list`
  it, `make-args` for the headless batch, `purge` a tag. Every `.pdata` body
  gets an authoritative linear disassembly (`.asm`) taken from the image,
  including bodies Ghidra has no function for. Ghidra's decompile (`.c`) is
  added when it exists; it can truncate and is never a verification source.
  `show <addr> --window` disassembles an interior or unlisted address, and
  `build` is incremental (`--force` re-fetches decompiles, `--asm-only`
  refreshes disassembly without the bridge). Storage is shared
  (`asm/<chunk>.asm`, `c/<entry>.c`); tag membership lives in the `tagged`
  table, so a chunk can belong to several ranges and purging one range never
  disturbs another's files. The bridge call pins `Overwatch.exe`; a transport
  error is retried and never recorded as a miss.
- **`dev/frame_check.py`** — the address-frame gate (`cargo xtask frame-check`):
  docs citations, the name table, and function-entry sanity.
- **`dev/gh_apply_types.py`** — applies `docs/struct-layouts.csv` to the
  Ghidra program (struct import, then the `prototype` column).
- **`dev/imm_scan.py`, `dev/xref_rva.py`** — immediate and
  reference searches in the current image.

The table below is the practical set by task.

## Data probes

| tool | answers |
|---|---|
| `ow-tank-hero` | hero list, health, entity, loadout |
| `ow-tank-model` | model, skeleton, chunks |
| `ow-tank-anim --raw <guid>` | one clip asset |
| `ow-tank example blendset_probe -- [hero]` | blend-set items, clips, slots, weights, retarget params, posed arm/weapon positions |
| `ow-tank example blendtree_probe -- [hero] [item] [tree]` | a blend tree's instances with class/field names and typed values |
| `ow-tank example retarget_probe -- [hero]` | character vs view-model bind/hierarchy deltas |
| `ow-tank example clip_chan_probe -- [clip]` | a clip's sampled rotation/position channels vs the rig bind |
| `ow-tank example fpcomp_probe -- [hero]` | `STUFirstPersonComponent` values |
| `ow-tank example stu_probe -- <guid> <instances>` | raw STU instances, layouts and payload bytes |
| `ow-tank example hardpoint_probe` | the hardpoint transforms a model carries |
| `ow-tank-skel` | which chunk of a model carries the skeleton |
| `ow-tank-modelanim` | model → animation linkage |
| `ow-tank-combo` | component-level associations |
| `ow-tank-assoc` | asset-level associations, JSON export |
| `ow-tank-stu --help` | STU instances and typed fields |
| `ow-tank-look --help` | asset/material probe |
| `ow-tank-tex --help` | texture decode and the BC6H self-test |
| `ow-tank-shader --help` | shader groups, code blobs and instances |
| `ow-tank-sound --help` | Wwise metadata, banks and voices |
| `ow-tank-replay --help` | replay container, header and roster |
| `ow-tank-content --help` | content surfaces |
| `ow-tank-combat --help` | per-hero combat table: pools, graphs, volley literals |
| `ow-tank-params --help` | graph parameters and `ModifyHealth` literals |
| `ow-tank-refwalk --help` | reference walks from a hero or asset |

## Verification

| tool | answers |
|---|---|
| `cargo test --release -p ow-casc -p ow-stu -p ow-tank -p ow-scene` | data layer, map resolver and scene tests |
| `cargo test --release -p ow-client` | client-side camera math |
| `python dev/verify_harness/cli.py all` | every differential unit |
| `python dev/verify_harness/cli.py casc` | the verified CASC subset |
| `python dev/verify_harness/cli.py fn/<Name>` | one unit |
| `cargo xtask frame-check` | docs addresses, the name table and function-entry sanity |

`cargo xtask verify` does not run the differential harness; `python
dev/verify_harness/cli.py casc` is the CASC gate, and `fn/<Name>` is the
per-function gate.
| `python dev/verify_harness/scaffold_unit.py <address> --name <Name>` | a scaffold for a new client function |
| `cargo run --release -p ow-client` | the practice-range eye-check |
| `cargo run --release -p ow-client -- --list-maps` | map headers and variation modes |

## Decompilation (Ghidra)

The working project is `dev/ghidra_run/owmod_clean` (on the SSD, git-ignored). The
only program is `Overwatch.exe`; [`ghidra-project.md`](ghidra-project.md)
gives the base, the address rule and the workflow. A clean analysis PE (the
install's headers plus the recovered `.text`) is built with
`python dev/patch_clean_pe.py`; the output stays under the git-ignored
`dev/data/offline_recover/`.

Canonical scripts are tracked in [`ghidra/`](ghidra/) and deployed to
`C:/Users/Happy/ghidra_scripts/` (Ghidra only loads scripts from there):

- `McpDecompile.java <args-file> <out-file> [timeout-sec]` — one
  parameterised decompiler. The args file is whitespace-separated `start end`
  pairs in the program's own address space; whole-line `#` comments are
  allowed. Functions are created if Ghidra has none.
- `McpCallers.java <args-file> <out-file>` — callers of each target, with
  call-site context.
- `FunctionCensus.java` — read-only `.pdata`/function census, for checking a
  clean import against the live project.
- `DisassemblePdataFunctions.java` — fill the real `.pdata` bodies after a
  `-noanalysis` clean import.
- `ExportAnnotations.java`, `ApplyAnnotations.java`, `CreateMissingNames.java`,
  `CheckNames.java`, `ImportStructs.java` and `ConfigureQualityAnalysis.java` —
  the clean-map migration pipeline: export and copy annotations, create the
  no-`.pdata` leaf names, verify the CSV, import the structs and run the
  reference/data/string analysis without function-start or body-repair
  heuristics.

Example:

```text
analyzeHeadless.bat E:\overwatch-client-rs\dev\ghidra_run\owmod_clean Overwatch.exe \
  -noanalysis -scriptPath C:\Users\Happy\ghidra_scripts \
  -postScript McpDecompile.java E:/overwatch-client-rs/dev/data/corpus/args.txt E:/overwatch-client-rs/dev/data/corpus/out.c
```

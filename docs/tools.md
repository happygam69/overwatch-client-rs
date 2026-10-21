# Tools

All analysis tools are **read-only**: they open the install and the captures,
never write to the game, never attach with write rights. Captures are a new
operator decision every time.

- **[`../dev/INDEX.md`](../dev/INDEX.md)** - every tool in `dev/`, with the
  question it answers. Generated from the tools' own headers; the gate checks
  it is current.
- **[`../docs/artifacts.md`](../docs/artifacts.md)** - what lives on
  `D:/ow-dumps`.

The table below is the same set by task, with the Rust probes.

## Capture and coverage

| tool | answers |
|---|---|
| `dev/live_page_diff.py` | which pages are readable/decrypting during a session; writes a run directory (read-only attach) |
| `dev/tight_report.py` | decrypt-window analysis over a run's `events.csv` |
| `dev/warm_merge.py` | union many runs' pages into one image, with a per-page report |
| `dev/session_merge.py` | one run -> one base-coherent image (+ base sidecar), for pointer chasing |
| `dev/coverage_gap.py` | per-run gap audit: cold pages by section, `.text` ranges attributed to `.pdata` functions |
| `dev/warm_poll.py` | older continuous warm-page sweep (fallback capture) |
| `dev/dump_process.py` | read-only minidump of the running client (`--normal`, `--full`) |
| `dev/etw_cpu_capture.ps1` | elevated read-only WPR CPU-sample capture |
| `dev/etw_raw_scan.py` | raw qword scan of an ETL: finds the loader base and counts pad-family addresses |
| `dev/etw_count.py` | exact counts and offsets of known loader addresses in an ETL |

## Page cipher

| tool | answers |
|---|---|
| `dev/pagepak_build.py` | builds/extends `pagepak/keystream_current.npy` from captured pages |
| `dev/page_crypt_battery.py`, `dev/page_crypt_x.py`, `dev/page_crypt_survey.py` | structural tests on the page cipher |
| `dev/ks_linear.py` | Berlekamp-Massey linear complexity of the keystream |
| `dev/ks_key_test.py`, `dev/ks_hash_test.py` | candidate key/cipher/hash schemes against the known stream |
| `dev/emu_tls.py`, `dev/emu_tls_replay_fresh.py` | offline Unicorn harness for the loader's TLS callback |

## Image and pointer analysis

| tool | answers |
|---|---|
| `dev/trace_chain.py` | follows qwords inside a coherent image; identifies foreign-session pointers |
| `dev/dump_qwords.py`, `dev/dump_object.py` | qword/object dumps with in-module pointer resolution |
| `dev/imm_scan.py` | finds 4/8-byte immediate constants in `.text` |
| `dev/xref_rva.py`, `dev/find_refs.py`, `dev/refs_image.py` | finds code references to given RVAs or addresses |
| `dev/pdata_lookup.py` | `.pdata` function bounds for RVAs |
| `dev/stub_extract.py`, `dev/stub_filter.py` | extracts/filters the class-factory stub table |
| `dev/ldr_*.py` family | loader scans: band functions, xrefs, `.gxfg`, loop shapes (each one is in `dev/INDEX.md`) |
| `dev/pe_probe.py`, `dev/mkpe.py` | identify a module dump; wrap captured regions as a PE |

## Live-process work (read-only)

| tool | answers |
|---|---|
| `dev/hunt_actor.py`, `dev/hunt_node.py` | find the mover and the player node in a live process, by behaviour |
| `dev/find_moving_objects.py`, `dev/vel_scan.py`, `dev/relaxed_scan.py` | find what moves at walking speed, and what holds a transform |
| `dev/wall_watch.py`, `dev/wall_diff.py` | find the heap offsets that only change under a held key |
| `dev/live_trace.py`, `dev/ctrl_probe.py`, `dev/trace_probe.py` | record the mover and measure crouch, jump, air control |
| `dev/find_dmworld.py` | find the physics world and enumerate its bodies |
| `dev/focus_window.py` | bring the game's window to the foreground |

## Animation data (read-only)

| tool | answers |
|---|---|
| `ow-tank-anim --raw <guid>` | writes a clip asset whole |
| `ow-tank-anim --list <family> --raw` | clips of a family |
| `ow-tank example blendset_probe -- [hero]` | blend-set items, clips, slots, weights, retarget params, posed arm/weapon positions |
| `ow-tank example blendtree_probe -- [hero] [item] [tree]` | a blend tree's instances with class/field names and typed values; the item's clips and float slots |
| `ow-tank example retarget_probe -- [hero]` | character vs view-model bind/hierarchy deltas; the dropped-ancestor fold identity |
| `ow-tank example clip_chan_probe -- [clip]` | a clip's sampled rotation/position channels vs the rig bind |
| `ow-tank example fpcomp_probe -- [hero]` | `STUFirstPersonComponent` values (offset + four floats) |
| `ow-tank example refskel_probe` | scans model skeletons for the clip's reference-rig proportions |
| `ow-tank example refclip_probe -- [clip]` | a `0x00020400` clip through the reference-table decoder |
| `ow-tank example stu_probe -- <guid> <instances>` | raw STU instances, layouts and payload bytes |
| `ow-tank example hardpoint_probe` | the hardpoint transforms a model carries |
| `ow-tank-skel` | which chunk of a model asset carries the skeleton |
| `ow-tank-modelanim` | the model -> animation linkage, read from the class table |
| `ow-tank-combo` | association at component granularity (what a component references) |
| `ow-tank-assoc` | association at asset granularity, with a JSON export |
| `dev/clip_lab.py`, `dev/clip_layout.py`, `dev/clip_structure.py` | the clip chunk: headers, reference graph, block sizes |

## Other data probes

| tool | answers |
|---|---|
| `ow-tank-look --help` | asset/material probe |
| `ow-tank-tex --help` | texture decode and the BC6H self-test |
| `ow-tank-shader --help` | shader groups, code blobs and instances |
| `ow-tank-sound --help` | Wwise metadata, banks and voices |
| `ow-tank-replay --help` | replay container, header and roster |
| `ow-tank-content --help` | content surfaces (themes, conversations, unlocks) |
| `ow-tank-combat --help` | the per-hero combat table: pools, graphs, volley literals |
| `ow-tank-stu --help` | STU instances and typed fields |
| `ow-tank-hero`, `ow-tank-model`, `ow-tank-light`, `ow-tank-params`, `ow-tank-refwalk` | heroes, models, lighting, config vars, reference walks |

## Decompilation (Ghidra, headless)

- Projects: `E:/ghidra/hproj*` (see `artifacts.md`); the working project moved
  to `D:/ow-dumps/dev_ghidra_run`.
- Canonical scripts are tracked in [`ghidra/`](ghidra/) and deployed to
  `C:/Users/Happy/ghidra_scripts/` (Ghidra only loads scripts from there):
  - `McpDecompile.java <args-file> <out-file> [timeout-sec]` - one
    parameterised decompiler (replaces the old `McpAnimChainN` copies).
    The args file is whitespace-separated `start end` pairs in the
    program's own address space. For a stripped import pass
    `rva - 0x10000`; for a PE import pass `image base + rva`. Whole-line
    `#` comments are allowed. Functions are created if Ghidra has none;
    each body is written under `// ===== sub_<start> =====`.
  - `McpCallers.java <args-file> <out-file>` - callers of each target,
    with call-site context.

Example invocation:

```text
analyzeHeadless.bat E:\ghidra\hproj4 hmod4 \
  -process module_after_stripped.bin -noanalysis \
  -scriptPath C:\Users\Happy\ghidra_scripts \
  -postScript McpDecompile.java D:/ow-dumps/anim_args_s.txt D:/ow-dumps/anim_chain.c
```

Workflow, addresses and conventions: `architecture.md` ("Evidence
pipeline in detail", "Address conventions").

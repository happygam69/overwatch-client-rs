# Tools

All analysis tools are **read-only**: they open the install and captures,
never write to the game, never attach with write rights. Captures are a
new operator decision every time.

The canonical set lives in `dev/` (gitignored) plus `ow-tank` binaries and
examples. This table says what each answers; detailed usage is in the file
headers.

## Capture and coverage

| tool | answers |
|---|---|
| `dev/live_page_diff.py` | which pages are readable/decrypting during a session; writes a run directory (read-only attach) |
| `dev/tight_report.py` | decrypt-window analysis over a run's `events.csv` |
| `dev/warm_merge.py` | union many runs' pages into one image, with a per-page report |
| `dev/session_merge.py` | one run -> one base-coherent image (+ base sidecar), for pointer chasing |
| `dev/coverage_gap.py` | per-run gap audit: cold pages by section, `.text` ranges attributed to `.pdata` functions |
| `dev/warm_poll.py` | older continuous warm-page sweep (fallback capture) |

## Page cipher

| tool | answers |
|---|---|
| `dev/pagepak_build.py` | builds/extends `pagepak/keystream.npy` from captured pages |
| `dev/page_crypt_battery.py`, `dev/page_crypt_x.py`, `dev/page_crypt_survey.py` | structural tests on the page cipher |
| `dev/ks_linear.py` | Berlekamp-Massey linear complexity of the keystream |
| `dev/ks_key_test.py`, `dev/ks_hash_test.py` | candidate key/cipher/hash schemes against the known stream |

## Image and pointer analysis

| tool | answers |
|---|---|
| `dev/trace_chain.py` | follows qwords inside a coherent image; identifies foreign-session pointers |
| `dev/dump_qwords.py`, `dev/dump_object.py` | qword/object dumps with in-module pointer resolution |
| `dev/imm_scan.py` | finds 4/8-byte immediate constants in `.text` |
| `dev/xref_rva.py` | finds RIP-relative/immediate code references to given RVAs |
| `dev/pdata_lookup.py` | `.pdata` function bounds for RVAs |
| `dev/stub_extract.py`, `dev/stub_filter.py` | extracts/filters the class-factory stub table |
| `dev/ldr_*.py` | loader scans: band functions, xrefs, `.gxfg`, loop shapes |

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

## Decompilation (Ghidra, headless)

- Projects: `E:/ghidra/hproj*` (see `artifacts.md`).
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

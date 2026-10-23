# Capture coverage: do we have what the project needs?

*Measured, not assumed. Every number here comes from the captures on this
machine: `dev/capture_coverage.py` (per address), `dev/coverage_gap.py` (per
section and per function range), and the runs' own `warm.csv` /
`encrypted.csv`.*

**The short answer:** yes, for the systems that are open. Every code address
the project still needs is warm in the standing image - 41 of 41 checked,
across animation, camera, movement, combat and physics. Page coverage is now
**complete offline and in Ghidra**: Plan A recovered all 12,076 `.text` pages
and the `.pdata` table from the installed executable without a live capture
(one declared 16-byte tail), and the project holds that image as the
`Overwatch.exe` program (151,260 functions). The page cipher's picker remains
a reversing target, not a capture requirement.

## What we have

| capture | what it is |
|---|---|
| `D:/ow-dumps/live_page_diff/run_*` | 17 page-diff runs, 19-21 October, 8k-22k pages each, with `warm.csv`, `encrypted.csv`, `events.csv` and a phase marker |
| `D:/ow-dumps/warm_poll/run_*` | 2 warm-poll runs (19 October) |
| `D:/ow-dumps/module_after_diff.bin` | the **standing image** the docs are written against (v2b base `0x7ff7c4d20000`, 20 October) - build **2.25.0.0** |
| `D:/ow-dumps/module_v21c.bin` | the current build's image (build **2.25.0.1**), merged from every 21-22 October run of that build: **23.5%** of `.text` is still cold |
| `D:/ow-dumps/module_v2full.bin` | the 12 October image, build **2.25.0.0** - the basis of the docs' addresses |
| `D:/ow-dumps/offline_recover/module_offline.bin` | the **complete current-build code image**: all 12,076 `.text` pages and the `.pdata` table recovered offline, preferred base `0x140000000` (build **2.25.0.1**) |
| `D:/ow-dumps/loader_after_diff.bin` | the loader with its decrypted pages (flat; a file offset is an RVA) |
| `D:/ow-dumps/ow_heap_full.dmp` | the 13.5 GB full heap dump (21 October) |
| `D:/ow-dumps/dev_ghidra_run`-derived images | the region wraps and the merged images |
| the Ghidra project `dev/ghidra_run/owmod` | 13 programs: the complete `Overwatch.exe` image, the analysed module, the standing capture's mapped and heap images, the live image, the camera region, the loader, the loader's XOR regions, three region wraps |

## The build changed: the captures are not all the same build

The client updates itself. The 12-20 October captures are build
**2.25.0.0.154088** (the one the docs describe); the 21 October captures and
the current install are build **2.25.0.1.154504**. The update moved the
code: of 15,297 non-empty pages, **0 keep their address** between the two
builds. The image's build is the one its version string says
(`2.25.0.0` in the 20 October image, `2.25.0.1` in the 21 October one).

So a merge across the two is meaningless - it produces an image whose bytes
at a given address come from different builds. The rule is: **merge within a
build, and say which build the image is.**

### The current build, captured

Five runs of build 2.25.0.1 (21-22 October) are merged into
`D:/ow-dumps/module_v21c.bin`. The cold `.text` went **28.9% -> 23.5%**
(3,491 -> 2,840 pages). The runs that paid were the ones that attached to a
**freshly started client**: a new process re-encrypts everything, and the
startup storm is where the pages come from (9,091 pages rescued from
ciphertext in one run, 0 in a run that attached to an already-warm client).

## The addresses moved: the translation table

Of the **355** distinct v2b addresses the docs cite, in build 2.25.0.1:

| outcome | how many | what it means |
|---|---|---|
| same address | 1 | the code did not move |
| moved | 90 | the same bytes, a new address (51 matched on the whole 96-byte window, 39 on the first 24) |
| rewritten | 179 | the patch changed the code there (most are data - vtables, tables, var names) |
| outside the old image | 77 | the address was in another frame, not the v2b one |

The table is `D:/ow-dumps/build_2.25.0.0_to_2.25.0.1.txt`; it is made by

```sh
python dev/build_translate.py --old D:/ow-dumps/module_after_diff.bin \
  --new D:/ow-dumps/module_v21_clean.bin --addresses <file-of-addresses>
```

A moved address is a *lead*, not a proof: the match is on bytes, so a
function that was rewritten is not found, and a 24-byte match is weaker than a
96-byte one. The reliable way to re-derive a finding in the new build is to
find it again in `module_v21_clean.bin` - the Ghidra project now holds the
current build for exactly that.

## The standing image is stale, and re-merging is free

`dev/warm_merge.py` over the 17 existing runs - **no new capture** - produces
`module_after_diff_v2.bin`:

| section | before | after re-merge |
|---|---|---|
| `.text` | 2,648 cold pages (21%) | **1,404 cold pages (11%)** |
| `.rdata` | 66 cold | 2 cold |
| `.data` | 462 cold | 428 cold |
| `.pdata` | 298 cold (50%) | **0 cold** |

The `.pdata` result matters: the function table of the *whole* client is
readable, so every function's extent is known. Plan A later showed the
**main executable's disk `.pdata` is plaintext** (201,246 sorted entries);
the earlier "disk `.pdata` is encrypted" note was the loader's table, not the
main module.

The pages that stay cold after the re-merge were never seen as code by any
run: they are pages of uniform filler (readable memory, not code) and
regions of `.text` that never became resident in any captured session. A
re-merge cannot recover them; **Plan A's offline solver can**, and did for all
12,076 `.text` pages.

## The systems, one by one

Each row: the behaviour that is still open, and whether we hold the code it
lives in. "warm" is measured on the standing image.

### Animation and the first-person path - **warm**

| what is still open | the code | state |
|---|---|---|
| the FP camera consumer (type `0x35`) | `FpViewmodelConsumer` v2b `0x7ff7c4ff0060` | warm |
| its per-frame slots (`+0xe0`, `+0xe8`) | v2b `0x7ff7c5011bf0`, `0x7ff7c50122f0` | warm |
| the Pose2d dataflow VM (x/y, category, weights) | the animation runtime: v2b `0x7ff7c6d92691`, `0x7ff7c6dac1b4`, `0x7ff7c6e0fc70`, `0x7ff7c6e0fb50`, `0x7ff7c6e08510`, `0x7ff7c6e028ec` | warm |
| the blend-tree evaluation | `AnimSamplerRule` v2b `0x7ff7c6e84610`, `AnimKind2_Eval` `0x7ff7c58ea110`, the lerp chain `0x7ff7c58dfbe0`, `0x7ff7c58df9b0` | warm |
| the clip object update | the region wrap `regions/clipobj.exe` | present |

**Verdict: an analysis problem, not a capture problem.** The VM that produces
the Pose2d drivers is in the image; the work is to find it.

### Camera - **warm**

| what is still open | the code | state |
|---|---|---|
| the FP camera consumer's per-frame application | the type-`0x35` system; the vtable slots above | warm |
| the world camera near/far | the camera code around the var block (v2b `0x7ff7c86957cc`) and the renderer's projection path | warm |
| the input handler chain | v2b `0x7ff7c688efc9`, the var block `0x7ff7c86957cc` | warm |

**Verdict: analysis.**

### Movement - **warm, and the control is obfuscated rather than missing**

| what is still open | the code | state |
|---|---|---|
| the mover's control law (declared tier 3) | the mover class's band v2b `0x7ff7c55405d0..`, the constructor `0x7ff7c5540750`, the interface tables `0x7ff7c7f03030` | warm |
| the four mover-component methods (a recorded capture gap) | range `0x7ff64afc2160..0x7ff64afc24b0` | **warm - the gap is closed** |
| the steep-slide rule | the same band, plus a keyed trace | warm |

**Verdict: the control code is present and VM-obfuscated.** That is why it is
tier 3, not why it is unread. The rates were measured from traces instead.

### Combat - **warm**

| what is still open | the code | state |
|---|---|---|
| the per-shot damage (not in the files) | the statescript runtime: v2b `0x7ff7c6eda1c0` (the var pool), `0x7ff7c570c740` (the read site), `0x7ff7c56b4719` (the dataflow update) | warm |
| the damage order and armour mitigation | the same runtime; the `ModifyHealth` node is a data class (`0x826613B7`) | warm |
| the volley class (a recorded capture gap) | range `0x7ff64b735xxx` and the fire method `0x7ff64b75d000..` | **warm - the gap is closed** |

**Verdict: the damage is a graph-reading problem.** The docs' two
possibilities stand (it is hidden in a generic `value` parameter, or it is
computed when the graph fires); either way the code that would settle it is
in the image, and the graph data is on disk.

### Physics (Domino) - **read, and warm**

The solver chain (v2b `0x7ff7c7a47c70`, `0x7ff7c7a4c150`, `0x7ff7c7a4c730`,
`0x7ff7c7a46d60`, `0x7ff7c7a4cce0`) is warm and already read.

### The renderer - **warm** (the docs' own 2026-10-19 update)

The RHI is in the warm captures: 43 RIP-relative references to the renderer
strings at v2b `0x7ff7c73b7xxx`, the D3D12/DXGI functions around
`0x7ff7c73b7e30` and `0x7ff7c82ca310` are all warm. The older "no capture
holds the renderer's code pages" paragraph describes the *old* standing dump.

## The loader did not change with the client

The page cipher is the client's loader, not the client. The loader on disk
today has the **same PE timestamp and the same section layout** as the copy
the 21 October capture used - but its `.text`, `.pdata`, `.rdata`, `.eid` and
`.eidsig` are all different bytes, because the loader re-encrypts its own
code on disk. The *code* it decrypts to is the same build, so:

- the loader addresses in `docs/formats/page-crypt.md` (`0x14380e0`,
  `0x144c839`, ...) still hold;
- the `w` table (`keystream_current.npy`) is a per-build draw and still
  applies.

This is the one part of the project the client update did **not** disturb.

## The capture question (coverage resolved offline)

**The page cipher's `w` derivation is no longer a coverage blocker.**
`dev/offline_page_recover.py` recovered all 12,076 `.text` pages and the
`.pdata` table from the installed `Overwatch.exe` without any live capture:
2,284/2,284 held-out oracle pages and 4,887/4,888 + 5,350/5,351 warm pages
match; one page keeps a declared 16-byte tail. The picker itself is still
unread inside the loader's own obfuscated VM/dispatcher (not Denuvo), but it
is a reversing target, not something the coverage needs. A new build needs
the pad re-extracted and the blind solver rerun; no fault-synchronous capture
is required (`docs/formats/page-crypt.md`, Offline recovery).

## Ghidra coverage is complete (2026-10-23)

**The Ghidra project now holds the complete image.** The Plan A image is
imported as the program `Overwatch.exe`: preferred base `0x140000000`, all
12,076 `.text` pages, 201,246 `.pdata` entries and **151,260 functions** after
auto-analysis. The 2,351 pages that were zero in `module_v21_clean.bin` read
as code there; RVA `0x1000` is zero in the old program and a real `.pdata`
function in the new one. Read a doc's v2b address as
`0x140000000 + (v2b - 0x7ff7c4d20000)`. Names are still applied to
`module_v21_clean.bin`; the same bytes are at the mapped address in
`Overwatch.exe`.

## How to reproduce this check

```sh
python dev/capture_coverage.py dev/needed-code.txt   # per address: warm or not
python dev/capture_coverage.py --map                 # the image, block by block
python dev/capture_coverage.py --gaps                # what a re-merge would recover
python dev/capture_coverage.py --merge-gain          # what each run would add
python dev/coverage_gap.py D:/ow-dumps/module_after_diff_v2.bin \
  --exe "E:/Overwatch/_retail_/Overwatch.exe"        # per section and per function range
python dev/offline_page_recover.py --stage all       # rebuild the complete .text/.pdata
```

`dev/needed-code.txt` is the list of addresses the project still needs, one
per system, with the reason.

# Handoff: finish `ow-casc` (30 entries without a unit)

## This session (2026-11-19): three more units, three walls closed

The assignment below was the previous session's. This session continued the
same loop on the top of the playbook queue and took three functions:

| unit | address | result |
|---|---|---|
| `fn/ConfigKeyEntryParse` | `0x1424cebd0` (481 B) | 87.5% both-edge (14/16 of 21 sites, 5 dead edges), 11/11 mutants + 1 declared equivalent |
| `fn/DynamicIndexResolveRange` | `0x1425115d0` (291 B) | 100% (7/7), 5/5 |
| `fn/PageSlotFind` | `0x1424fa440` (247 B) | 100% (12/12), 12/12 |

`cli.py casc` is **30/30 verified**. `cli.py all` is **98 verified, 3
inconclusive, 1 divergent, 1 scaffold of 103 units** (98 of 99 distinct
addresses); the five non-verified are the pre-existing ones listed below.
`cargo xtask verify --fast` is green (fmt, clippy, 828 lib tests, tool index,
deviation/frame/layout checks). `cargo xtask verify` (the install-reading
half) does not finish on this machine because of a pre-existing slow test -
see "Measured, not fixed" at the bottom.

`crate_coverage.py ow-casc`: **28 verified / 30 `entry` without a unit / 84
interior / 8 code / 3 data** over 153 cited addresses.

## The assignment (previous session)

"lets try to finish the rest with subagents, delegate them to work on the
functions, they dont need to use /verify_harness/cli.py because they can block
each other. when one batch is done you can verify yourself with cli.py and if
we got problems reopen subagent to fix it. work autonomously with that loop
until full casc is implemented and verified"

So the loop is: **delegate a batch → when the batch finishes, run the harness
myself → if a unit fails, re-dispatch a fixer → repeat until `ow-casc` is
fully implemented and verified.**

## Current state (measured, `python dev/crate_coverage.py ow-casc`)

- 25 verified / 33 `entry` without a unit / 48 interior / 7 code / 3 data
  (116 cited addresses).
- `cli.py casc` is 27/27 PASS in `dev/verify_harness/ledger.json`; the unit
  count went 20 → 27 since the first draft of this page.
- Newly verified since this page was written: `fn/BuildInfoSelectRow`,
  `fn/EncodingEspecStringIndex`, `fn/DynamicIndexAppendInterval`,
  `fn/DynamicIndexDecodeEntries`, `fn/DynamicIndexEntrySink`,
  `fn/DynamicIndexFindEntry`, `fn/HexPairDecode`.
- Last results this session: `fn/ParseBoolString` (9/9 branches, 10/10
  mutants, 2 declared equivalent) and `fn/DynamicIndexNormalizeIntervals`
  (8/8, 12/12).

**Governing rule — playbook §10**: a system is done when every function on its
path has a unit and a fresh `verified` verdict, **or is declared open for a
specific read reason**. So the end state may legitimately include "read, open,
because X" rows.

## The five walls (already read, already documented — do not re-read)

**Walls 2, 4 and 5 are closed (2026-11-19); walls 1 and 3 stand.** Each closed
one failed only on a path that can be declared a gap: the vector grow (2), the
single-pass rewrite that was missing from `pagecache.rs` (4), and a
"marshalling job" that turned out to be four flat fields (5).

In `docs/casc-1to1.md` ("The same wall, measured (2026-11-17)"):

1. `BlteFrameHeaderParse` `0x1424a90e0` — chunk table allocated via three IAT
   calls (`0x1424a92a4`, `0x1424a9301`, `0x1424a936c` → import slot
   `0x143aafdc9`; type-`0x10` extras via `0x143aafdd0`). Oracle maps no heap.
2. `ConfigKeyEntryParse` `0x1424cebd0` — grows its vector through an allocator
   vtable (`[rax+0x10]` at `0x1424cec83`; free at `0x1424cecaf`). It does call
   the verified `ParseHexKeyId` (`0x14248f040`).
3. `PageCacheInvalidateAll` `0x1424fab50`, `PageCacheInvalidateRange`
   `0x1424fabd0`, `PageCacheClose` `0x1424f9eb0`, `PageCacheInit` `0x1424f8790`
   — execute the client mutex `0x142694c20`/`0x142694c50` inside the window.
4. `PageSlotFind` `0x1424fa440` — lock-free, but the client does the key-hit
   test and the LRU-victim choice in **one** pass that returns the chosen slot,
   touches it and writes a `failed` flag; the crate splits it into
   `PageCache::find` + `PageCache::claim` (called from `read`). A unit needs the
   client's single-pass function written into the shipped `pagecache.rs` first.
5. `DynamicIndexResolveRange` `0x1425115d0` — operates on the whole segment
   object; the crate's `IndexSegment::resolve_range` builds the key state
   itself. A marshalling job, not a body read.

Plus the already-declared: `CdnIndexPageResident` `0x1424da590` (HTTP range
read in streamed mode) and `PageReadPlan` `0x1424faa20` (container lock).

## Measured, not fixed (2026-11-19)

- `crates/ow-casc/tests/index_find_real.rs::find_range_and_duplicate_choice_match_the_install`
  takes **1154 s (19 min) and passes**. It walks every install key group and
  calls `Store::location_of` + `Store::resolve_range` twice per group; each call
  rebuilds the segment's whole 18-byte entry buffer
  (`SegmentEntries::key_state`) and a fresh `KeyState`, so a 1.1M-key install
  means tens of GB of copying. It is the reason plain `cargo xtask verify`
  needs ~25 minutes here, not a failure. Pre-existing (the test predates this
  session and nothing here touches `key_state`), and not fixed: the honest fix
  is a `key_state`-free resolve path, which is a perf change, not a port.
- The `ow-tank` install tests (`component_payload_sizes_match_the_client`,
  `effect_assets_parse_and_every_component_keeps_the_clients_widths`,
  `a_model_carries_no_brush_container_chunk`) are the same shape: each runs
  for minutes, so a non-`--fast` `cargo xtask verify` is a ~30-minute job on
  this machine even when everything passes.

## Next: work in batches, not one unit at a time

Six hours produced three units, and four of those hours were gate runs, not
porting. The loop for the remaining 30 `ow-casc` entries:

1. Take 6-8 leaves from `crate_coverage.py ow-casc --queue` at once. A
   delegate per function: read the body, transcribe to Rust, write the
   fixture, write the unit JSON. Delegates do **not** run the harness.
2. The main session owns the two shared files (`verify_probe.rs` arms,
   `candidate_rust.py` rows) and runs `cli.py fn/<Id>` per unit as results
   arrive (20-60 s each).
3. One `cli.py casc` at the end of the batch, detached to a log. Never
   `cli.py all` for progress: stage 5 rebuilds the crate per mutant, so a full
   run is about an hour.
4. `cargo xtask verify --fast` per batch; the full one once, detached, at
   handoff.
5. An entry blocked by an IAT import (allocator or the client mutex
   `0x142694c20`/`0x142694c50`) is written `open` with its address and one line
   of why, and the batch moves on.

## The candidate list (33 entries)

From `python dev/crate_coverage.py ow-casc --top 100`, the `entry` rows not
yet verified. Addresses are **exe-frame** (image base `0x140000000`).

| addr | name | impl file | note |
|---|---|---|---|
| 0x14269ab80 | HexPairDecode | install | **DONE** (`fn/HexPairDecode`) |
| 0x1424aec30 | BuildInfoSelectRow | install | **DONE** (`fn/BuildInfoSelectRow`; the real extent runs to `0x1424aed7b`+) |
| 0x14248f2a0 | BuildInfoLoadFromInstall | install | path building; likely FS/vtable-dependent; verify |
| 0x1424c1030 | — | install | chained fragment (0x890 stack frame); verify extent |
| 0x142498a70 | EncodingEspecStringIndex | encoding | **DONE** (`fn/EncodingEspecStringIndex`; the vector grow is the declared gap) |
| 0x1424d1c60 | EncodingSectionsRead | encoding | read |
| 0x1424d1b40 | EncodingHeaderRead | encoding | 281 B; read |
| 0x1425208d0 | DynamicIndexNormalizeIntervals | index | **DONE** (`fn/DynamicIndexNormalizeIntervals`) |
| 0x142520700 | DynamicIndexAppendInterval | index | **DONE** (`fn/DynamicIndexAppendInterval`; the capacity grow is the declared gap) |
| 0x142520590 | DynamicIndexEntrySink | index | **DONE** (`fn/DynamicIndexEntrySink`) |
| 0x1425209a0 | DynamicIndexDecodeEntries | index | **DONE** (`fn/DynamicIndexDecodeEntries`) |
| 0x14250fcf0 | DynamicIndexSegmentParse | index | 1240 B; read |
| 0x1425111a0 | DynamicIndexFindEntry | index | **DONE** (`fn/DynamicIndexFindEntry`) |
| 0x142501b40 | DynamicKeyStateLookup | index | 0x13e; read |
| 0x14250e400 | DynamicIndexSegmentOpen | index | 0x5c; read |
| 0x14250c8f0 | DynamicContainerOpenArchive | cdn | 0x1e4; read |
| 0x1424f9bf0 | PageSlotPublish | pagecache | 0xe; tiny |
| 0x1424a6df0 | — | pagecache | 0xc; trampoline — read the whole body (`0x1424a6df0`→`lea r8,[rcx+0x58]`+…) |
| 0x1424a7d70 | — | pagecache | 0x33; read |
| 0x1424d6e50 | BlteBlockReadLz4 | blte | calls external LZ4 (`0x142efecf0`-style); likely open |
| 0x1424a90e0 | BlteFrameHeaderParse | blte | **wall 1** |
| 0x1424cebd0 | ConfigKeyEntryParse | install | **wall 2** |
| 0x1424da590 | CdnIndexPageResident | container | **open** (HTTP) |
| 0x1424fab50 | PageCacheInvalidateAll | pagecache | **wall 3** |
| 0x1424fabd0 | PageCacheInvalidateRange | pagecache | **wall 3** |
| 0x1424f9eb0 | PageCacheClose | pagecache | **wall 3** |
| 0x1424f8790 | PageCacheInit | pagecache | **wall 3** |
| 0x1424fa440 | PageSlotFind | pagecache | **wall 4** |
| 0x1425115d0 | DynamicIndexResolveRange | index | **wall 5** |
| 0x1424faa20 | PageReadPlan | pagecache | **open** (container lock) |
| 0x1424fb1c0 | PageCacheRead | pagecache | read |
| 0x1424fb1c0 | (same) | | |
| 0x1424fa540 | PageLoad | pagecache | read |
| 0x1424fbf00 | ContainerReaderInit | pagecache | read |
| 0x1424fcd20 | DynamicContainerReadPage | pagecache/cdn | read |
| 0x1424f48d0 | DynamicContainerRead | cdn | read |
| 0x1424ccc00 | BuildConfigParse | install | 6784 B, 256 call sites — the big one; last |

The ordering hint from the queue tool (proved callees first, then smallest
body): `CdnIndexPageResident`, `BlteFrameHeaderParse`,
`ConfigKeyEntryParse`, `DynamicIndexSegmentParse`, `EncodingHeaderRead`,
`DynamicIndexAppendInterval`, `DynamicIndexDecodeEntries`, `BuildConfigParse`,
then the small leaves.

## Method (from `docs/port-playbook.md`)

For each function:
1. **Read** it: `python dev/fn_extent.py <addr>` (`.pdata` extent + chains),
   `python dev/rd.py <start> <end>` (capstone disassembly), and the merged
   Ghidra decompile via `dev/ghidra_run`. Follow the machine, not the C.
   **Chained fragments matter**: several `.pdata` extents are tiny because the
   real body lives further down (e.g. `0x1424aec30` runs to `0x1424aed7b`+).
2. **Transcribe** into the crate file named above, operation for operation:
   same widths, wrapping arithmetic, byte views for structures, floats as raw
   bits. If a callee is outside the system (allocator, CRT, HTTP, mutex),
   declare it an open stage with the address — never a plausible default.
3. **Wire the probe**: one match arm per entry in
   `crates/ow-casc/src/bin/verify_probe.rs`, and register in
   `dev/verify_harness/candidate_rust.py` (`ENTRY_SOURCES` = files the ledger
   binds, `ENTRY_MUTATION` = the symbol mutants edit).
4. **Fixture** `dev/verify_harness/<name>_fixture.py` with `make_args(i)`:
   declared cases first (`i < len(SPECIALS)`) sitting exactly on branches and
   constants, then deterministic generation. Pointers are arena offsets;
   lengths/counts must stay tied to buffers (the client has no bounds checks).
5. **Unit** `dev/verify_harness/units/fn-<Name>.json`. Addresses are **v2b**
   (the v2b frame base plus the RVA; the delta is stated at the top of
   `dev/verify_harness/runner.py`); the exception — a function dereferencing an
   absolute pointer baked into image data — uses
   `{"address": {"exe": "0x14…"}, "base": "0x140000000"}` (see
   `fn/ParseBoolString`). `coverageWindow` = the function's own byte extent.
   `allowedImageReads` lists every image address the code reads.
6. **Run** `python dev/verify_harness/cli.py fn/<Name>`.

### IMPORTANT: no two agents in the same checkout at once

Sub-agents share the working tree and the harness binds verdicts to the
working-tree contents, so **never run more than one in parallel**. Do them
one at a time.

- The sub-agent writes: crate source, probe arm, `candidate_rust.py`,
  fixture, unit JSON.
- It does **not** run `cli.py` for other units; it may run its own unit
  (and `cargo build --release --bin verify_probe -p ow-casc`).
- The main session (me) runs `cli.py casc`, then `cargo xtask verify` and
  the docs, and re-dispatches a fixer if a unit fails.
- Updates: `docs/formats/casc.md`,
  `docs/casc-1to1.md` (the verified table), `docs/systems-coverage.md`,
  `docs/open-questions.md`, `STATUS-handoff.md`, `MODLOG.md`;
  `python dev/make_index.py` after adding tools; `python dev/crate_coverage.py
  ow-casc` to see the count move.

### Per-unit loop (main session)
1. Pick next from the table (proved callees first).
2. `acp_delegate` a `worker` with a self-contained task: the address, the
   playbook path, the crate file, the five required files, and the rule
   "if it cannot be a five-stage unit, say which address blocks it and put
   the read in the docs instead".
   **Watch for**: the agent's result file arriving as a completion
   notification — fetch with `acp_delegate_wait(runId)`; a failed run keeps
   its output file and can be resumed with `resumeFrom`.
3. When it returns, run `python dev/verify_harness/cli.py fn/<Name>`
   (my own run, ~2–5 min with a warm probe).
4. If it fails: `acp_delegate` a `reviewer`/`worker` with the exact failure
   text (stage, divergence, branch site, survivor mutant, and the playbook
   §8 symptom table), fix, re-run.
5. If it passes: docs + count + next.

### Standing rules
- Never commit (AGENTS.md hard rule 4). Never stage beyond the task.
- No new `FIT-*` markers (`cargo xtask verify` fails on them).
- `.gitignore` is a whitelist.
- Docs record addresses + conclusions, never disassembly/decompiled code.
- "not tested" if not tested. If two attempts fail, write STATUS-handoff.md.

### Hard rules from AGENTS.md (project)
1 to 1 or not at all: a feature is implemented only if the client's code was
read from the complete `Overwatch.exe` Ghidra image, the implementation
follows it operation for operation, and a test proves identity. Behaviour
source is the current image only; data source is the operator's installation.

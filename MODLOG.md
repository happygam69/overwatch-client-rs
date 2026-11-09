# MODLOG
Milestones only. Findings and addresses live in `docs/formats/`; current state
in `STATUS-handoff.md`; the work queue in `docs/open-questions.md`.

Rotation: this file holds the current entries. Older ones move verbatim to
`docs/history/modlog-YYYY-MM.md` (one file per month) at each rotation; no
entry is rewritten after the fact.

## 2026-11-19 - Three CASC functions ported: the walls that were not walls

- **`fn/ConfigKeyEntryParse` (`0x1424cebd0`, 481 B), verified.** 87.5%
  both-edge (14/16 of 21 sites, five dead edges declared), 11/11 mutants plus
  one declared equivalent. The `key-<id> = <key>` line read operation for
  operation: the 32-bit span difference, the token scan bounded by `key_end`,
  the 24-byte record appended *before* either parse, `ParseHexKeyId` then
  `ParseHexKey`, and the disposition (`1` for a good id with 32 hex digits,
  `0` for a short value). The vector grow is the declared gap - it recomputes
  the capacity by multiplying the eight stack bytes holding the `key-` literal,
  upper dword never written. It was previously logged as "cannot be a unit".
- **`fn/DynamicIndexResolveRange` (`0x1425115d0`, 291 B), verified.** 100% of
  seven branches, 5/5 mutants. The whole status machine (2 no entry, 4 twice,
  3 not resident, 0 found), the `0xffffffff` length form, and the write order:
  the out range before the resident check, the entry only on success and only
  when non-null. Four flat segment fields, not the marshalling job the docs
  feared.
- **`fn/PageSlotFind` (`0x1424fa440`, 247 B), verified.** 100% of twelve
  branches, 12/12 mutants, after shipping the client's single pass as
  `pagecache::slot_find`: a match is returned even when the slot is busy (with
  `out_usable` 0), and only otherwise does the same pass pick the largest
  wrapping `clock - stamp`, ties to the lower index. The typed
  `PageCache::find` + `claim` split stays until `PageCacheRead` (`0x1424fb1c0`)
  is read; that gap is declared in the unit.
- **The CASC suite is 30/30 verified.** `fn/EncodingFindEKey` was red for a
  non-port reason: its `knownEquivalentMutants` entry is line-keyed and the
  line had drifted from 991 to 1122, so the stage reported a survivor. The
  declaration now names 1122 and says why it moved.
- **Two stale claims corrected on sight.** `encoding.rs` said the helper at
  `0x1426cce30` returns a string's length; the machine returns the address of
  its NUL terminator (its callers only test it for zero).
  `fn/CdnIndexPageSelect` said `CdnIndexPageResident` is "82 bytes of .pdata
  extent whose whole-file path lives past that extent"; the merged register
  measures 401 bytes with the whole-file path inside it.

## 2026-11-18 - CASC state reconciled: 27 units, and the gate's frame check is green

- **The CASC record now matches the machine.** `docs/casc-1to1.md` said fifteen
  units, `docs/port-playbook.md` said 83 of 88 descriptors, and
  `STATUS-handoff.md` said 18 verified / 36 open. Measured today:
  `cli.py casc` is 27/27 verified in the ledger; `crate_coverage.py ow-casc`
  reports 25 verified / 33 entries without a unit / 48 interiors / 7 code /
  3 data over 116 cited addresses.
- **`fn/HexPairDecode` has a row.** It was verified (100%, 12/12) but only
  mentioned in prose; `docs/casc-1to1.md` section A now names the twenty-
  seventh unit, its 318-byte extent and its declared gaps.
- **`frame_check.py` is green again.** 18 citations in `docs/formats/casc.md`
  and `STATUS-handoff.md` spelled the v2b address literally; both are outside
  the history files the check exempts, so the gate failed. They are now
  `<frame base> + RVA` forms. One of them, the `EncodingHeaderRead` v2b value,
  was also wrong (below the v2b base); it is now derived from `0x1424d1b40`.
- **`dev/INDEX.md` regenerated** (194 tools; the six newest fixtures were
  missing), so `make_index.py --check` passes too.

## 2026-11-17 - the espec string index

- **`EncodingEspecStringIndex` (`0x142498a70`) is ported and wired** as
  `encoding::espec_string_index`, with `fn/EncodingEspecStringIndex` wired
  (probe arm, fixture, unit; the main session runs `cli.py`). It is the
  encoding open's post-read step: last-byte gate returning 8 (an empty
  block reads the byte at `base - 1`), a 32-bit NUL count, a vector
  reserve to that count, and one walk appending every espec string start
  after the length helper at `0x1426cce30`. `EncodingFile::parse` now
  uses it for the tail check and entry count (29,694 on the install,
  unchanged). **Declared gap:** the vector grow at `0x142388210` through
  the allocator vtable; every fixture case pins the reserve and appends
  to the no-grow path, tightest at equality.

## 2026-11-17 - the boolean parser and the interval merge

- **`ParseBoolString` (`0x1424d8800`) is verified** by
  `fn/ParseBoolString`: the client's two eight-entry spelling tables, the
  prefix rule, the `A..=Z`-only fold and the tri-state flag, at 9/9 branches
  and 10/10 mutants (two fold-range mutants declared equivalent: no spelling
  contains `z`). The shipped `install::parse_bool` stand-in - trimmed, exact
  match, six spellings - is replaced by the client's expression; its call site
  already trimmed, so no shipped behaviour moved except the accepted spellings
  and the prefix rule.
- **The harness gained an exe-frame unit**: `address.exe` plus
  `base: 0x140000000` runs the image at its own link addresses, which a
  function needs when it dereferences absolute pointers baked into image data
  (the spelling tables). `crate_coverage` now counts such units.
- **`DynamicIndexNormalizeIntervals` (`0x1425208d0`) is verified** by
  `fn/DynamicIndexNormalizeIntervals` against the shipped
  `IntervalSet::normalize`: 8/8 branches, 12/12 mutants, with pre-sorted
  inputs (the client's own sort leaves its tail in an implementation-defined
  order and `0x14251ff30` is not ported - declared).
- Five functions were read and **cannot** be units: `BlteFrameHeaderParse`
  (IAT allocator), `ConfigKeyEntryParse` (allocator vtable), the page-cache
  entry points (the client's mutex), `PageSlotFind` (the crate splits the
  client's single pass in two) and `DynamicIndexResolveRange` (needs the whole
  segment object). Addresses and reasons are in `docs/casc-1to1.md`.

## 2026-11-17 - the text key parsers

- **`ParseHexKey` (`0x14248efd0`) and `ParseHexKeyId` (`0x14248f040`) are
  ported and verified** by `fn/ParseHexKey` and `fn/ParseHexKeyId`: 364
  differential cases each over the return, the read-only text, the target block
  and the flag byte, 100% branch coverage (4/4 each), 5/5 mutants each. The
  client's contract replaces the crate's `from_str_radix` loop: leading
  whitespace is skipped, the decode stops at a zero byte or a non-hex
  character, and the character count comes back to the caller. The keyring
  field parser now decodes through `install::parse_hex_key`.
- The `ParseHexKeyId` unit's first run left one surviving mutant
  (`hex_decode(out, 8, ..)` widened to 9, invisible while every case has at
  most 8 pairs); a case with 12 pairs now kills it.

## 2026-11-17 - the whole-file CDN index validator

- **`CdnIndexFileParse` (`0x1424d9c90`, 642 B) is ported and verified** by
  `fn/CdnIndexFileParse`: 364 differential cases over every channel (AL, the
  read-only key and file, and the 0x30-byte derived block), 100% branch
  coverage (10/10), 5/5 mutants. The shipped `parse_index_file` composes
  `parse_index_footer`, the terminal check, the key check and
  `verify_page_checksums`; the two new helpers (`terminal_checksum_matches`,
  `key_checksum_matches`) are now also what the streaming
  `PageIndex::open_with_key` uses, so each check has one expression instead of
  two. The unit pins the stage order and the derived block's bytes, including
  the hash bytes a header rejection leaves zeroed.
- **`CdnIndexPageResident` (`0x1424da590`, 401 B) is read but cannot be a
  unit.** Its streamed branch is an HTTP range read plus a per-page MD5 of the
  returned page, so those branch sites cannot run in the oracle; its
  whole-file branch is two stores already exercised through
  `fn/CdnIndexPageSelect`. Recorded as read, not verified, in
  `docs/casc-1to1.md` and `docs/open-questions.md`.
- Fixed `docs/open-questions.md`'s claim that `CdnIndexPageChecksums` "ignores
  a mismatch" - it returns 8.

## 2026-11-17 - the CDN page check, and a TEB for the oracle

- **`CdnIndexPageChecksums` (`0x1424d9f20`) is ported and verified** by
  `fn/CdnIndexPageChecksums`: 364 differential cases, 100% over 4/4 reachable
  branch sites of 6 (two dynamic-allocation guards have one dead edge each,
  proved dead for every byte-wide checksum size), 7/7 mutants. The status
  machine is now the crate's: 3 when `len` is shorter than the 32-bit
  `pageCount * pageSize`, 8 when a page's MD5 disagrees with the stored
  checksum array at `lastKeyBytes + bodies`, 0 when all match. It hashes
  through the three MD5 stages verified earlier, so that cluster now has its
  consumer. `docs/formats/casc.md` and `docs/casc-1to1.md` said a mismatch was
  "ignored"; it is a rejection - corrected.
- **The oracle gained a fake TEB (`HARNESS_VERSION` 6).** The mismatch path is
  a CRT error report that allocates two stack buffers with `__chkstk`
  (`0x1426c3ce0`), which reads the stack limit at `gs:[0x10]`; an unmapped
  `gs:` faulted as `unmapped read at 0x10`, so the status-8 edge could not be
  emulated at all. The oracle now maps one TEB page, points `gs:` at it and
  zeroes it before every call, so `__chkstk` returns without probing. No gate
  changed, but the oracle did, so the version bump turns every older ledger
  entry stale and `cli.py all` was re-run: **83 verified, 4 open, 1
  scaffold** - the same four pre-existing opens (the two tier-1 C units
  whose `gcc` exits silently, `fn/MeshSweep` at 61.1% coverage,
  `fn/OrthonormalizeCameraBasis-state` divergent), so no verdict turned
  red because of the TEB.
- **`IndexFooter::from_descriptor`** is public now: it decodes the client's
  0x30-byte derived-footer block at its own offsets, which is the descriptor
  `CdnIndexPageChecksums` reads (`+0x07`, `+0x14`, `+0x20`, `+0x24`).
- The read also found the zero-length compare rule: with `checksumSize` 0 the
  client's `memcmp` never dereferences the checksum pointer, so a descriptor
  whose array address lies past the buffer still returns 0. The Rust keeps that
  case out of the slice machinery instead of faulting.

## 2026-11-17 - MD5 finished: the client's three-stage context, verified

- **`Md5Init` (`0x1426a5100`), `Md5Update` (`0x1426a4f40`) and `Md5Final`
  (`0x1426a5010`) are ported at the client's own offsets** and verified by
  `fn/Md5Init`, `fn/Md5Update` and `fn/Md5Final`: 364 differential cases each on
  every channel, 100% branch coverage (0/0, 4/4 and 3/3 sites), 6/6, 8/8 and
  9/9 mutants (two declared observationally equivalent — each only widens a
  guard so an empty range is filled).
- **The 0x58-byte context is now a declaration, not prose**: count at `+0x00`,
  the four chaining words at `+0x08`, the 64-byte buffer at `+0x18`, and the
  `count << 3` length word over buffer `+0x38`. `md5()` in `blte.rs` is the
  client's composition of the three stages, so the RFC 1321 vectors run the
  client's staging and the streaming path exists for callers that hash in
  pieces.
- **Reading notes recorded**: `Md5Init` has no `.pdata` extent (44 call sites
  still target it; the coverage window is the measured 36-byte body), and
  `Md5Update` takes `fill` from the pre-add count's low dword while advancing a
  64-bit count.
- **New tool `dev/crate_coverage.py`** answers "which client functions is this
  crate about": every image address cited in a crate's source, resolved to
  verified / entry / interior / code / data, with `--queue` printing the
  unverified entries in the playbook's order. `ow-casc` cites 65 addresses (12
  verified, 41 function entries pending); `ow-tank` cites 234 (22 verified, 83
  pending).
- **`python dev/verify_harness/cli.py casc`** is the row's gate and is green;
  `docs/casc-1to1.md`, `docs/formats/casc.md`, `docs/systems-coverage.md` and
  the MD5 entry in `docs/open-questions.md` are updated.

## 2026-11-17 - README re-ordered, and the status moved one click away

- **The 173-line `## Status` section moved verbatim** to
  [`docs/status-in-depth.md`](docs/status-in-depth.md). Its text is identical to
  the old section except one link: `docs/rebuild-plan.md` became
  `rebuild-plan.md`, because a README-relative path breaks one directory down.
- **README top now answers the visitor's first questions in order**: one
  paragraph (what it is, the 1:1 rule, no Blizzard content), the practice-range
  image, a four-command quick start, an 8-row `What works today` table, then
  the pointers to the evidence. Every claim in the table is a summary of a
  sentence that moved; nothing was added or dropped.
- **Screenshot slot**: `docs/img/range.png` is a generated 1280x720 placeholder
  (grid, "screenshot placeholder", the shot command), so the README renders
  today. The caption says it is a placeholder; the last quick-start command
  regenerates it from the operator's install. Nothing captured from the game is
  committed, and the whitelist already tracks `docs/img/` (`git check-ignore`
  confirms the file is not ignored).
- **`docs/README.md`** gained the start-here row for `status-in-depth.md`.
- **Checked:** every relative link in `README.md`, `docs/README.md`,
  `docs/status-in-depth.md` and `STATUS-handoff.md` resolves; the moved body
  was diffed line by line against the old section.

## 2026-11-17 - rustdoc reaches zero warnings: 26 links repaired

- **Census before:** 10 links from public docs to private items, 13 unresolved
  targets, 2 redundant explicit targets, 1 unclosed HTML tag — 26 across 19
  files. (The audit's 41 also counted the three `verify_probe` bin headers,
  which `doc = false` now excludes.)
- **Private helpers stay private.** The brackets came off (`MAX_REDIRECTS`,
  `mesh_triangle`, `compose_pose`, `quad_length_sq`, `depenetrate`, `Hull`,
  `VertexBuffer::stride`, `read_ref_array` x2, `decode_r16_unorm`); no public
  API was widened for a doc link.
- **Re-pointed where the item really moved:** `Kind::detect` ->
  `MapChunk::detect` (`ow-tank/src/mapchunk.rs:603` is an `impl MapChunk`
  method), `HullStage::op` -> `WorkspaceVertex::op` (the op is per workspace
  vertex, `ow-tank/src/brush_mesh.rs:75`), `hero_setting_defaults` ->
  `HERO_SETTING_FLOAT_DEFAULTS` (only the float settings declare defaults).
- **Nested-module links need a path:** `graph::hero_state`,
  `entity::loadout`, `sound::types` and `unlocks::types` are child modules, so
  their links became `super::LITERAL_HERO_VARS`, `super::CONFIG_VAR_DYNAMIC`,
  `super::Ability::params`, `super::survey_sound`, `super::survey_content`.
- **`ow-sim` has no dependencies by design**, so
  `crate::character_collision::Capsule` cannot resolve there; it is now the
  plain code span `ow_tank::character_collision::Capsule`.
- **`tests` is not an item rustdoc can link** (private, cfg-gated) — three links
  became code spans.
- **Protocol grammars** in `ow-casc`/`ow-tank` `verify_probe` and
  `ow-scene/ground_probe` were indented lines directly under a paragraph, which
  CommonMark reads as paragraph text: `<guid list>` parsed as HTML. All three
  are now fenced `text` blocks, so the tokens render literally.
- **Checked:** `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`
  exits clean (29 files generated), `cargo fmt --check` passes, `cargo clippy
  --workspace --all-targets` reports 0 warnings.

## 2026-11-17 - `cargo doc --workspace` builds again: three bins stop documenting

- **The error:** `cargo doc --workspace --no-deps` died with "document output
  filename collision" — `verify_probe` exists in `ow-casc`, `ow-sim` and
  `ow-tank`, and three bins cannot write the same doc path.
- **The fix:** an explicit `[[bin]]` with `doc = false` in each of
  `crates/ow-casc/Cargo.toml`, `crates/ow-sim/Cargo.toml` and
  `crates/ow-tank/Cargo.toml`. The bins are throwaway diagnostics driven by
  `dev/verify_harness/`, not API, so suppressing their docs loses nothing. No
  other bin name collides (`anim`, `census`, `ground_probe`, ... are unique).
- **Checked:** `cargo doc --workspace --no-deps` finishes and writes
  `target/doc/ow_casc/index.html` and 28 more; `cargo check --workspace --bins`
  still builds all 21 bins, so auto-discovery is intact.
- **Still open, warning only:** 26 rustdoc warnings across the workspace
  (broken or private intra-doc links, e.g. `ow-tank/src/mapchunk.rs:18`
  `Kind::detect`, `ow-casc/src/cdn.rs:60` `MAX_REDIRECTS`). They do not fail the
  build now, but a doc job that runs `-D warnings` would fail on them.

## 2026-11-17 - corrupted characters repaired in the clip and unlock docs

- **`crates/ow-tank/src/clip.rs`**: 166 non-ASCII runs were double-encoded —
  UTF-8 bytes once read as code page 1251 and written back as UTF-8 — so one
  em dash became three bytes of Cyrillic-and-punctuation junk, and likewise
  `− × ÷ § ² … → ≥`. Reversed run by run; the ASCII text is byte-identical to
  HEAD, so only those characters moved. Comment text only, no code. The
  corruption is **not** from `6e44bb7`: the same sequences are in the file's
  first commit `f5608db` and every one since.
- **`crates/ow-tank/src/unlocks.rs`**: 91 `U+FFFD` replacement characters on
  90 lines (one line carries two), all spaced where an em dash joins two
  clauses; replaced with `U+2014`, the same dash the file already uses 4 times.
- **`docs/formats/character-controller.md:711`**: 1 `U+FFFD` in the same
  position, same fix.
- **`crates/ow-tank/src/lib.rs` and `crates/ow-client/src/main.rs`**: the same
  loss in ASCII clothing - 8 runs of three literal question marks where a
  separator dash belongs, all in doc comments (`lib.rs` five module bullets,  `main.rs:395`, `main.rs:642` and two on `main.rs:618`). Those question marks
  are the intended character's UTF-8 bytes dropped by a converter, so the em
  dash is restored. One is not a dash: the code span
  `[identity, bone 0, bone 1, ...]` (`main.rs:618`) lists three bones of a
  256-slot palette, so it takes the ellipsis. My first sweep missed these
  because it only looked at non-ASCII characters and `?` is ASCII - recorded
  here so the next sweep checks both.
- Checked: no `U+FFFD`, no Cyrillic and no run of three question marks left
  in any tracked `.rs`/`.md`/`.py`; `cargo fmt --check`, `cargo check -p
  ow-tank --lib` and `cargo check -p ow-client` pass.

## 2026-11-17 - `ow-sim` is not parked: three stale pages corrected

- **The tooling map was wrong.** `AGENTS.md` called `ow-sim` "parked reference;
  not built until a read gameplay layer needs it". It is a workspace member
  (`Cargo.toml`), `cargo xtask verify` builds and tests it
  (`cargo test --release --workspace`, `--lib` on the fast path), the
  differential harness ships its probe
  (`crates/ow-sim/src/bin/verify_probe.rs`), and the crate holds the read
  arithmetic: mover leaves, the per-frame velocity law, jump/air, the swept
  collision query, the fan solver, `body`, `camera`.
- **Two other pages repeated the same stale fact:** the pipeline's step 4 in
  `docs/architecture.md` ("`ow-sim` stays parked") and `docs/tools.md` ("the
  parked `ow-sim` units"). The harness wording is now stated as it behaves:
  `all` fails on any unit that is not verified, `casc` is the green gate.
  `dev/engine_knowledge_brief.md` quoted both stale sentences, so it is fixed
  too.
- **README's crate table** named only the ramp and camera arithmetic; it now
  matches the crate and the rest of the page (jump/air, collision query, fan).

## 2026-11-17 - the port procedure written down: `docs/port-playbook.md`

- **One page for the whole method**, from "which unit next" to "a system is
  done": the inventory (3,864 functions, 1,541 units, triage states), how to read
  (corpus asm first, decompile from `C:/Temp/owread`, `DecompScope`/`ProbeFlow`
  before trusting a short output), the transcription rules that keep Rust 1:1
  (widths, byte views, bit patterns, `minss`/`comiss` NaN semantics, self-masking
  `u32` shifts), the probe contract, the fixture rules, every unit-descriptor
  field and what it gates, how to read the five stages and each verdict, a
  failure-to-work table, cluster order, the definition of done per system, the
  tool map and four copy-paste checklists.
- **It records today's tool state honestly**: 79 verified of 83 descriptors,
  the merged project's seam numbers, the broken local `gcc`, and the one
  `pe_layout.py: ok` failure mode that is really a stale `dev/INDEX.md`.
- `docs/port-plan.md` now points at it instead of implying the method is
  obvious from one worked example.

## 2026-11-17 - the unit suite run end to end: 79 of 83, and three real causes

- **`python dev/verify_harness/cli.py all` now reads 79 verified, 4 open.** The
  four are known causes, not mysteries: two tier-1 C units cannot compile
  (`gcc`/`cc1.exe` on this machine exits 1 with no output even for
  `int f(void){return 1;}` - MSYS2 install damaged), one state-mode unit lacks
  the probe's guest-pointer translation, and `fn/MeshSweep` sits at 61.1%
  branch coverage against an 80% gate.
- **Empty compiler errors are gone.** `candidate_c.py` reported
  `candidate compile failed:` with nothing after the colon; it now prints
  whichever stream has text, or the exit code and the full command. That is how
  the broken gcc was identified in one run instead of an afternoon.
- **`fn/OrthonormalizeCameraBasis-cargo` is verified again.** It had been
  failing as `candidate fault: unknown-entry-orthonormalize_camera_basis`: the
  mapping in `candidate_rust.py` and `camera.rs` existed, the probe arm in
  `crates/ow-sim/src/bin/verify_probe.rs` did not. Added; five stages pass.
  The `-state` sibling stays open on purpose - its fixture passes a basis inside
  the state snapshot and the arm rejects a guest pointer with a named error
  rather than guessing an offset.
- **`fn/TextureCountBitsShifted` verifies** after declaring one equivalent
  mutant instead of chasing it: widening `(bias+2) & 0x1f` to `& 0x3f` cannot be
  observed because the shifted value is u32 and Rust's `>>` masks the shift to
  5 bits exactly as the client's `shrl` does. Rationale recorded in the unit.

## 2026-11-17 - the chain merge gets its seam test, and 41 bodies go back

- **The open class from the last entry is measured, not argued.** Walking the CFG
  from each root's entry over the function's own flow (fall-through, jump, jcc;
  a call's return path is followed, its callee is not) inside the `.pdata` union:
  **25,597 of 25,638** roots enter fragment bytes. The audit's 1,615 was "the last
  instruction does not transfer" - an upper bound, since an earlier branch usually
  enters the fragment. Only **41** roots never enter a fragment at all, and none
  of them is named.
- **Those 41 are reverted.** `MergeChainBodies.java revert` set their bodies back
  to the real extent and saved; `ImageAudit` now reports `in_body=25588`,
  `named_flowed=0`. A dry run afterwards reports `already=25588 no_seam=41
  would_widen=0`, so the merge is idempotent and cannot undo the verdict.
- **The rule now lives in `apply`:** a root is widened only when its own flow
  enters the fragment, printed `NO-SEAM` otherwise. Two bugs on the way there,
  both caught by the numbers not moving: `COMMIT` was gated on `apply` only, so
  the first revert rolled back while reporting success; and the walk `continue`d
  on call instructions, which dropped the return path and classified 6,220 roots
  as no-seam (they were functions whose prologue calls `__chkstk`).
- Net effect of this session's three passes: 25,588 of 25,638 chain roots own
  their full contiguous code with a measured seam, 41 are open by measurement,
  9 are open on a missing `.pdata` extent (previous entry), 11 roots have no
  function object at all.

## 2026-11-17 - chain merge pass 2, and the decompiler fact that shrinks the claim

- **25,629 of 25,638 roots now own their chained code** (was 25,586). The 52
  roots that pass 1 skipped were blocked by 16 function bodies that had outgrown
  their own `.pdata` extent; `FixOuterBodies.java` set those back to the extent
  (16 fixed, 1 refused: `0x140d83010`'s own extent collides with a neighbour),
  and the re-run widened 43 more bodies, removing 1 more inner object.
  `ImageAudit`: `body_flows_into_chain 51 -> 9`, `named_flowed=0`.
- **The 9 remaining are open with names and byte counts**, not glossed: every
  conflict left is a function object with **no `.pdata` extent** (12 of them,
  e.g. `140a401d0` overlaps 6,179 B of `140ccce40`). Clipping them was measured
  and rejected: `140d02c80` would go 1,392 -> 11 B, `140b2fdf0` 1,273 -> 12 B.
  `ChainConflicts.java` now emits the full table; `FixOuterBodies.java apply
  clip` exists for whoever judges them later.
- **Correction to section 8.2's premise, measured.** Ghidra's decompiler follows
  flow *outside* a function body unless another function's entry blocks it. So
  the merge does not lengthen most decompiles: `0x140ca6ac0` has a 29-byte body
  on the evidence project, 858 B after the merge, and the C is byte-identical
  (4,583 chars / 120 lines) on both. `0x142cd4bc0` likewise (320 vs 464 B, 4,272
  chars identical); only `0x142032060` grew 596 -> 615 lines. The audit's "25,184
  decompiles end early at the seam" was an overstatement - 8.2 is a map fix, and
  the readable-text fix is the 110 false `noreturn` flags plus the 320 inner
  objects (~1.3% of the widened roots).
- Four `c_cut` decompiles from the corpus (269 exist) were re-read from the
  merged copy: `ExpireMoverRecords` 120, `MoveCharacterController` 125,
  `DispatchCameraRequest` 616, `DmWorldSubstepDriver` 109 lines.

## 2026-11-17 - S2 applied: 25,533 bodies merged, and the decompiler trap behind them

- **The merge is real.** `MergeChainBodies.java` ran on a copy (`C:/Temp/owread`,
  evidence project untouched) and saved: bodies 152 -> 1,797 B for the mover,
  47 -> 20,886 B for a second sample, 25,533 widened, 320 inner objects removed,
  body bytes 29,259,101 -> 44,422,830. `ImageAudit` on the copy: `in_body=25586`
  of 25,638, `named_flowed=0` (was 157), `body_flows_into_chain=51` (was 25,184).
- **52 roots are skipped on purpose.** A foreign function object whose entry is
  outside the union still reaches into it; widening would delete its extent. The
  script prints `OUTER <root>: ... e.g. <entry>` and is idempotent, so they are
  a follow-up, not a blocker. Dry run and apply disagree on the count (64 vs 52)
  because removing one over-extended fragment unblocks another root - benign.
- **The merge alone does not lengthen a decompile, and that cost hours to see.**
  After the merge the decompiler is handed the full body (`DecompScope.java`:
  `dec=20886B/1R` for a root whose output is 28 lines) and still prints the old
  text, because the 110 false `noreturn` flags make it prune the cold part as
  dead code. `ClearFalseNoReturn.java clear` on top of the merge: mover 175 ->
  315 lines with 13 references into the cold part, `0x140c8ea20` 28 -> 2,586
  lines. Order for every readable sweep is now merge, then clear, then read.
- **Two harness bugs fixed by measuring.** `getFunctionsOverlapping` returns
  `Iterator<Function>` here, not `FunctionIterator`; and headless needs
  `-process Overwatch.exe` or `currentProgram` is null and every script dies.
  Both are in the recipes (`docs/port-plan.md` S2, `docs/ghidra-project.md`).
- New read-only tools: `ProbeFlow.java <addr>` (why a decompile stops) and
  `DecompScope.java <addr>` (what body the decompiler was handed).

## 2026-11-17 - both living ledgers rotated into `docs/history/`

- **`MODLOG.md` 162 KB -> 34 KB, `STATUS-handoff.md` 104 KB -> 55 KB.** The
  ledgers had drifted from "milestones, not a diary" into append-only history:
  every clone pays for the whole tail, and the tail is what `git log` already
  holds.
- **The split is by date and by liveness, and it is verbatim.** `MODLOG.md`
  keeps the entries from 2026-11-10 on (14); the older 86 move to one file per
  month - `docs/history/modlog-2026-10.md` (6 entries, 2026-10-26 .. 10-31) and
  `docs/history/modlog-2026-11.md` (80 entries, 2026-11-01 .. 11-09), which the
  rest of November joins at the next rotation. `STATUS-handoff.md` keeps the
  live page (gate, current state, `Open`, `Next`, key files) and loses the two
  blocks that were the record of finished work: the older half of "Current
  state" and "Exact today", both to
  `docs/history/status-handoff-2026-11-17.md`.
- **Nothing was rewritten to achieve it.** 100 of 100 MODLOG entries are present
  exactly once across the hot file and the two archives; both retired STATUS
  blocks appear verbatim in the archive, and the kept blocks in the page. The
  check is mechanical (substring containment on the pre-rotation text), not an
  eyeball.
- **The rotation is now a rule, not a one-off** (`AGENTS.md`, "Keep these files
  updated"): MODLOG holds the current period and moves older entries to
  `docs/history/modlog-YYYY-MM.md`; a STATUS section that stops being live moves
  to `docs/history/status-<date>.md` with a pointer left behind. Moved text is
  never edited - a correction is a new entry.
- **The live replacements already existed**, which is why the cut is safe:
  `docs/systems-coverage.md` is the per-system state and `docs/open-questions.md`
  is the queue. The archives are cold storage; frame_check still scans them, and
  they carry no runtime-frame or out-of-extent citation.

## 2026-11-17 - Apache-2.0 ships as the whole text

- **`LICENSE-APACHE` was a 958-byte stub** that named the licence and a URL.
  Apache-2.0 section 4(a) requires a *copy of this License* to go out with any
  redistribution, so a distro package carrying that stub is legally short and
  gets bounced at review. It is now the canonical `LICENSE-2.0.txt` (201 lines,
  all 9 sections, END OF TERMS, APPENDIX), copied byte-for-byte from a
  distributed copy and cross-checked against a second one - never retyped, and
  not edited.
- **The stub's project note was redundant**, not lost: "this code only, not the
  game or its data" already stands in `README.md` (Licence) and `CREDITS.md`
  (Licence), which is where a scope caveat belongs; the licence file now holds
  only the licence.
- **Two docs claimed `--fast` is what CI runs.** It is not - CI runs the full
  `cargo xtask verify`, and the tests that read an install skip themselves there
  (`README.md`, `CONTRIBUTING.md` corrected).

## 2026-11-17 - CI runs at all, and on a clean checkout

- **The workflow never ran on `master`.** `.github/workflows/ci.yml` filtered
  `push` to `branches: [main]`; the only branch is `master`, so direct pushes
  triggered nothing and the gate only existed on pull requests of a solo repo.
  The filter is gone rather than corrected - a hardcoded name is what switched
  the gate off, and a rename must not be able to do it again.
- **`dev/pe_layout.py` failed on a runner with no install** (exit 2), which is
  every CI run. It now reports the skip and passes in `--check` mode like
  `dev/frame_check.py`, still fails when the image is present and its section
  table disagrees, and takes `OW_EXE` like the other tools.
- **Two checks could never pass on a clean checkout** - invisible until the
  workflow actually ran. `dev/make_index.py` indexed `dev/**/*.py` on disk,
  ignored analysis output included, so the committed INDEX.md carried 48 rows a
  fresh clone does not have (`--check` failed); it now indexes what git ships,
  223 rows -> 175. `collide::mesh_case0::the_python_fixture_case_hits`
  (`crates/ow-sim/src/collide.rs:4555`) unwrapped fixtures from `dev/data/read/`
  that the whitelist `.gitignore` never ships, failing `verify --fast` too; it
  reports the skip now, and that case is **not tested on CI**.
- **Green on `master`, run 42463833551.** CI checks: fmt, clippy (warnings are
  errors), the whole workspace, the tool index, the deviations table and the doc
  address frame. It does **not** check: anything that reads the install or the
  recovered image (frame entry check, PE layout, the 59 install-reading tests
  skip themselves). Proven before pushing in a clean clone
  (`git clone --local . target/ci_sim`), which is the only local model of CI.

## 2026-11-17 - the first ported unit: f32 to Q12, verified against the client's bytes

- **`fn/FloatToQ12` (`0x141c6c830`) is VERIFIED** - the first unit of the port
  campaign. 145 bytes over 3 pieces, 53 call sites, no callees, no global reads.
  364 cases, every channel, coverage 5/5, mutation 11/12 with the survivor
  declared equivalent (`>=` vs `>` at `exponent >= 138`: both shift by zero).
- **What it does, read from the body:** scales a run of `f32` by 4096 to Q12
  integers by integer bit manipulation - `(mantissa|2^23) >> (138-exp)` in the
  normal range, `<<` above, `0` when the shift exceeds 23, `0x7fffffff` when the
  left shift reaches 8 - then negates by two's complement. Saturation therefore
  yields `i32::MAX` and **`-i32::MAX`**; `i32::MIN` is unreachable. Shipped as
  `ow_sim::fixed::{scale_q12, quantize_q12}` with two unit tests.
- **The seven-step procedure is now written down** in `docs/port-plan.md`
  ("Worked example"), with the per-unit size: ~50 lines of Rust, ~35 of fixture,
  ~25 of unit JSON, ~15 of probe arm.
- **`ScaleFloatRunToQ12`** added to `docs/function-names.csv` (tag `runtime`,
  prototype declared). `gh_apply_names.py` was not run: the Ghidra bridge
  answered with an empty body, so the program still shows `FUN_141c6c830`.

## 2026-11-17 - S2 is staged: the body merge exists, dry-run numbers pinned

- **`docs/ghidra/MergeChainBodies.java`** resolves each `.pdata` chain to its
  transitive root (the CHAININFO walk from `ImageAudit`, DWORD-padded
  unwind-code array included), asserts the union is contiguous, removes the
  function objects sitting inside a cold part, and widens the root's body in one
  transaction. `dry` prints what would change; `apply` writes.
- **The dry run is pinned from the image, not estimated:** 25,649 roots with a
  cold part, all contiguous; 25,638 have a function object, 11 have none;
  **283 inner objects removed** - exactly the audit's `AT_CHAIN` count; body
  bytes of those roots 2,572,571 -> 18,610,692 (+16,038,121), i.e. the audit's
  predicted 28.7 -> 44.7 MB total.
- **The mover control-law question is narrowed by measurement, not read:** the
  call graph has **zero** inbound edges to `MoverInvoker` (`0x1407ffc50`), which
  is reached only through vtable slot 9 - 423 `call [reg+0x48]` sites image-wide.
  The caller therefore needs the vtable-store enumeration (next step), not an
  `E8` search. `MoverInvoker` is also an **11-piece** body; its linear listing
  stops at `0x1407ffce2 jle 0x14080033c`, which is why the union matters.
- **Reading hygiene:** `show` and `grep` now flag a hit whose function is
  `opaque` (914 bodies, 0.76%): their linear listing desyncs, so the decompiler
  is the read. `grep` counts those hits separately instead of mixing them in.
- The evidence project is not touched: the recipe copies `owmod_clean.rep` to
  `C:/Temp/owread` first (`docs/port-plan.md`, "S2"). E: has 3.6 GB free, so the
  copy goes to C: (115 GB free).

## 2026-11-17 - the port scope is measured: 3,864 gameplay functions, 1,541 units

- **`python dev/corpus.py triage` computes the scope instead of choosing it:**
  the forward closure of the 238 gameplay anchors in `docs/function-names.csv`
  is **3,864 functions / 2,301,019 bytes** (3.2% of the image). States:
  238 `entry`, 1,303 `unit`, 2,312 `inline`, 6 `thunk`, 5 `trivial`. By system:
  camera 2,391, map 1,457, mover 1,140, domino 915, input 462, anim 256,
  statescript 229, model 215, hero 4. `docs/port-plan.md` records the rules.
- **Why 1,541 units and not 3,864:** a unit is written for an entry point and
  carries its single-caller helpers; branch coverage proves they ran. 2,312 of
  the 3,864 have exactly one caller in the scope.
- **`python dev/corpus.py proto`** infers a candidate argument count from the
  callee's own entry reads, folding 32-bit aliases (`mov eax, ecx` uses `rcx`);
  the alias folding alone moved agreement with the 44 stated unit signatures
  from 22 to 28. It is a weighted reading aid, never a `function-names.csv` row.
- **Harness hygiene:** all 56 unit JSONs that still named
  `D:\ow-dumps\offline_recover\module_offline.bin` now name the repo copy, and
  `emu_oracle.IMAGE_SHA256` pins the bytes, so a unit proved against a different
  image fails instead of silently passing. `fn/ClosestPointGjk` still PASSes.
  `fn/AnimTimeWrap` fails at the C-candidate compile because `gcc`/`cc1` in this
  environment exits 1 with no output on any input, including a trivial file — 2
  of 83 units use that loader (77 cargo, 4 rust), so this is environmental, not
  a repo defect.

## 2026-11-17 - the corpus indexes functions, not `.pdata` entries

- **`dev/corpus.py` schema 3: the unit is a function.** Entries are grouped by
  their transitive `CHAININFO` root (`dev/fn_extent.py` is the one walker), so a
  cold half is a piece of its function and never a function of its own. New
  `verify` pins 201,246 entries / **119,802 functions** / 81,444 chained halves /
  25,649 roots with a cold part, 0 gaps, 0 pieces claimed twice, 44,567,808
  body bytes.
- **The operator's index was migrated, not rebuilt.** From the image: 1,057
  cold-half rows dropped (their `.c` were funclet decompiles), 329 bodies
  widened to their union, their 269 stored decompiles marked `c_cut`, 513 rows
  already exact -> 842 rows. Schema is checked on open and a stale version now
  fails instead of shadowing a corrected row.
- **`make-args` is no longer destructive.** It emits chain-root unions only,
  refuses a chained start, and a whole-image range must produce exactly 119,802
  pairs. Before this it emitted 201,246 pairs, and `McpDecompile.java` creates a
  function at any entry Ghidra lacks - a sweep would have created one at each of
  the 81,444 cold halves and trimmed the parents, which is how the 251
  multi-range bodies in `docs/ghidra-image-audit.md` section 4 were made.
- **The 11-root discrepancy in §5.2 is reconciled:** the image has 25,649 chain
  roots; the audit's 25,638 are those with a Ghidra function object, and the 11
  extra carry exactly the 11,366-byte continuation difference.
- **The relationship index exists** (`dev/corpus.py index`, image bytes only):
  `fn` 119,802 functions, `edges` 1,867,334 (994,637 jcc / 648,166 call / 216,891
  jmp / 7,134 `FF 15` API calls / 506 `FF 25`) and `slot_sites` 35,835. A code
  transfer whose target is outside `.text` is dropped (410 such decodes) and its
  function is marked `opaque` - 915 functions, 0.76%, whose linear listing must
  be read in the decompiler.
- **Three documented censuses are now checks, not prose:** the chunk
  assembler's caller (`0x1420251f5` -> `0x14209fc90`), the 485 register-slot
  `+0x40` sites, and `ShapeAddToBody`'s **27 sites in 12 functions**
  (`docs/formats/shape-ingest.md`; the older "13" counted a cold half as a
  function). Each fails on a planted mutation, so the pin is not vacuous.
- **The +0x40 slot count is re-measured and the doc corrected**
  (`docs/open-questions.md`): `call_slot_scan.py`'s "505" is raw byte patterns,
  not sites; the decode-anchored count is 523 = 485 register-slot + 38 SIB
  stack-local, 473 in both sets, 12 decode-only, 19 byte-scan-only. 485 is
  pinned as `GOLDEN_SLOT40`; `rax` carries 434 of them.

## 2026-11-16 - the jump: the arc is read, the jump is shipped

- **The read law: `MoverApplyForces` (`0x140cb1990`), called once per body tick
  from `0x140ee5b41` with `(body, command, dt)`, and its copy
  `MoverGravityStage` (`0x140cb2b00`).** Gated on `body[0x10d0] & 0x80000c`
  (airborne), the stage runs `MoverAirDrag` (`0x140cb2010`) first and then, in
  the airborne arm, `body[0x1500..0x150c] += dt * g` and
  `MoverAccumulateWorldVector(body+0x10d0, dt * g)`.
- **The gravity constant is a constant.** `MoverGravityVector` (`0x140ca72d0`)
  = `(gate * body[0x610]) along body[0xc0..0xcc]`, `gate` from `0x14202fb00`,
  a decoded lookup that returns the image constant **`-10.0`**
  (`0x1431161f8`) or `0.0`; its key argument is dead. `body+0x610` is the mover
  component's `+0xe8` (`0xC2C40969`, `1.75`), so gravity is `-17.5 m/s^2`.
- **The drag** is `delta = drag_dt * -abs(gate*body[0x610]/terminal^2 * up_lane)
  * (up_lane * axis)`, terminal `body+0x614`/`body+0x618` by the sign of the up
  lane, clamped to `|local|`; its time step is `0x140ca72b0`'s
  `(f32)(i64)body[0xe8] * 1e-6`, not the caller's `dt`.
- **Shipped:** `ow_sim::air` (15 tests: the golden 22-frame trace, the drag
  clamp, the channel split, the gravity vector, the accumulator) and the walk
  wiring in `crates/ow-client/src/walk.rs` (the take-off stage, then the air
  step, then the walk's sweep and floor snap). `--walk-test` now lands the
  jump: apex `0.979` m at frame 19 (law `0.979`), air `0.667` s (law `0.669`).
- **Docs corrected on sight**: `FUN_140cb2220` is *a* per-frame producer of
  `body+0x1150`, not the only one, and it is not a gravity integrator; the
  vertical law had been recorded as unread.

## 2026-11-16 - the jump: the take-off is read, the arc is not

- **Corrected a record.** The page said `FUN_140cc1440` (`0x140cc1440`) was
  control-flow-flattened and "is the vertical stage", so which of six floats was
  the impulse was unknown. It is read end to end now, and it is the **jump /
  crouch** stage: no gravity, no fall, no landing rule.
- **Read:** `MoverUpdateFrame` (`0x140cbb260`) splits the button dword at
  `body+0xdc` into bit 6 (jump) and bit 7 (crouch); the handler chain
  `FUN_140cc1060` (`0x140cc1060`) -> `FUN_140cc1440` with its six arguments; the
  state file word `body+0x10d0` and its five transitions (`0x140ca9100`,
  `0x140ca9120`, `0x140ca9150`, `0x140ca9190`, `0x140ca91e0`, all transcribed);
  the gate (property `0x14` > `0.5`, `body+0x10d0 & 0x80000c == 0`, latch bit 0,
  a live world at `body+0x1ad8`); and the impulse
  `target = M^T*body[0x1150] + body[0x1510]`, `target.y := body+0x60c`,
  `body[0x1150] = M*(target - anchor)`. `body+0x1510` has **no writer anywhere
  in `.text`**, so the impulse reduces to `local.y := body+0x60c`.
- **The magnitude is a component field.** The body is a copy of the mover
  component at `+0x528` (`CopyMoverComponentFields` `0x140ca4680` /
  `FUN_140ca4980`), so `body+0x60c` is class `+0xe4` = hash `EC289987`, which
  reads **`6.0` for all 55 playable heroes**. Five further offsets corroborate
  the mapping, the terminal fall `B74245E8` (`+0xec` -> `body+0x614`) among them.
  The six `STU_97288E54` floats are **not** the impulse.
- **Shipped:** `ow_sim::jump` (flag word, five transitions, button split, gate,
  impulse, the body readers) with 13 tests, and
  `ow_tank::entity::mover_takeoff_lane_of` + a real-install test over the whole
  roster. No differential unit yet.
- **Still open, and why no jump is wired:** the per-frame consumer
  `FUN_140cb2220` (`0x140cb2220`) resolves `body+0x1150` through the 0x40-byte
  record table at `body+0x960`; its producer is unread (five parallel record
  vectors are built at `0x140ca37d0` with `count = ctor arg`, capacity 4, and
  nothing in the read set fills them). `MoverAdvance` (`0x140ca7f30`) is the read
  integrator but its only caller has no code caller, so the live position comes
  from the swept solve, whose contact-to-position rule is unread. Also recorded:
  the verified ramp unit and the capsule page disagree about `body+0x5a8`/
  `+0x5b4`, so one of those two writers runs last and it is not read.
- `ow-client`'s Space key still declares the jump open, now naming the take-off
  as read and the arc as the missing piece.

## 2026-11-16 - the jump's leaves are verified; two of the first cut were wrong

- **Six differential units, all green** (`branch coverage 100%`, mutation kill
  rate `100%`): `fn/MoverOnTakeoff` (`0x140ca9190`), `fn/MoverOnLanding`
  (`0x140ca9120`), `fn/MoverOnGround` (`0x140ca91e0`), `fn/MoverEnterAir`
  (`0x140ca9150`), `fn/MoverPropertyRecords` (`0x140cb3990`, the 0x38-byte
  single-property applier the jump stage calls three times) and
  `fn/MoverVectorRecords` (`0x140cb3b10`, the 0x40-byte four-lane applier).
- **They found two real defects, not style nits.** `NOT_ON_GROUND` included bit
  19; the client's mask is `0x0080000c` (bits 23, 3, 2). And `MoverOnTakeoff`
  left bit 10 (`0x400`) alone when `dl` was 0, where the client's
  `btr eax,0xa` / `cmove` **clears** it. Both fixed; the tests that encoded the
  wrong law are rewritten.
- **Read sharper:** the impulse's `rise` has two branches
  (`(along + lane) - along` when airborne and rising, else `lane - along`), and
  both lane sums are three-lane `(p1+p0)+p2`, not four. The airborne prologue
  (`0x140cc148f..0x140cc155b`) forces `MoverOnGround` when a request arrives
  while airborne and below property `0x26`, arming the `body+0x1ad0` budget -
  the same property chain the appliers serve. `ow_sim::jump` now reports the
  frame stamp and the `+0x1a76` latch separately (`Transition`), because the
  client's conditions for them differ.
- **Not a unit, and said so:** the whole of `FUN_140cc1440`. Its middle third is
  obfuscated dispatch and the impulse arithmetic is inline, so the take-off's
  arithmetic is transcribed and tested, not bit-exactly proved.
- **Still open, unchanged:** the arc. `FUN_140cb2220`'s record table has no read
  producer, and the live position comes from the swept solve.

## 2026-11-16 - the per-frame vector resolve is read: it has no gravity in it

- **Read end to end:** `FUN_140cb2220` (`0x140cb2220`), the only per-frame
  producer of `body+0x1150`. It is a chain, not an integrator: property `0x22`
  scales against the vector length, property `0x23` again, property `0x24` acts
  on the y lane alone, then `body+0x60c` caps the y lane (divide, property
  `0x25`, multiply back), then two 0x40-record passes behind
  `(*body->vtable)[0xa8](body) != 2`, then `MoverOnLanding` when the world lane
  rose by more than `0.004`.
- **Two conclusions that change the plan.** `body+0x60c` is the *lateral cap*
  here and the *take-off magnitude* in `FUN_140cc1440` - one component field,
  two uses. And there is **no gravity term anywhere in the chain**, so the arc
  cannot be produced by porting this function.
- **Not implemented, and why.** Both normalisations use `RSQRTSS` plus one
  Newton step, so the scale factors are not portable bit-exactly; and phase 5's
  gate is a virtual call while its hit path is a five-way plane-projection
  dispatch read only at the skeleton level. Also settled: the five inline record
  vectors are filled by the constructor's *arguments* - `0x140ca37d0`'s only
  callers are the object constructor `0x1407ff1f0` and a factory `0x1407ff1b0`
  referenced only from a data pointer at `0x142fdbee8`, so the content is
  runtime data, not code.
- Recorded in `formats/character-controller.md` "The per-frame vector resolve"
  with the constants as raw words and the instruction ranges, and in
  `open-questions.md` items 1, 1a, 1b.

## 2026-11-16 - the vector resolve is implemented and verified; both blockers closed

- **`RSQRTSS` measured, not assumed.** A two-instruction stub (`rsqrtss` +
  `movd`) run through the oracle shows the value is exactly
  `f32(1.0 / f32_sqrt(x))` on 220/220 sampled inputs; the double-rounding forms
  miss 44/220. So the client's one-Newton-step square root is portable: it
  returns one ulp below `3.0` on `9.0` and must not be replaced by `sqrt` - a
  74-input sample through a hand-assembled copy of the client's own instruction
  sequence agrees with the Rust bit for bit.
- **The differential corrected the reading.** The phase-2 shuffle is a mask,
  not the identity: it builds `(x, 0, z, w)`, so property `0x23` acts on
  `x^2 + z^2` and scales only `x` and `z`; `y` and `w` keep their phase-1
  values. A `movss xmm7,xmm6` before the `shufps` pair is what makes it so.
- **Shipped:** `crates/ow-sim/src/resolve.rs` (phases 1-4, `rsqrtss`,
  `sqrt_newton`, `landing_rise`, 13 tests) and the unit
  `fn/MoverResolveVector` driving the real `FUN_140cb2220` with the 0x40-byte
  count at zero: 100% branch coverage over the phases-1-4 window, 7/7 mutants
  killed, two boundary mutants declared observationally equivalent with the
  argument recorded.
- **Still open:** phase 5's plane-projection hits, the bit-24-clear transform
  path, the 0x40-byte record producer (runtime data), and the live position.
  The arc still cannot be produced, but the per-frame *vector* law is now exact.

## 2026-11-15 - the walk's world: the map's collision, not the models' meshes

- **Operator report:** `ow-client` "always bumps into invisible obstacles" and
  feels different from the install. Measured, not guessed: the walk world held
  the level collider **and** 3,017 per-model `STU_537E56E4` holder shapes.
- **Measured (`--walk-test`, practice range, spawn room):** with the level
  collider only, the forward leg is **5.500 m** at 5.5 m/s with **0.000 m**
  drift and the six-second tour keeps its height to 1 mm; with the model shapes
  added the leg is 5.189 m, drifts 0.461 m and the tour **climbs 0.78 m** in the
  flat room. A column at `(70.00, 61.10)` shows why: the level mesh has a ramp
  (normals `(-0.55, +0.31, +0.78)`), the model shapes put a coarse copy over it
  with `(-0.53, +0.41, -0.74)` and **vertical** faces `(-0.58, 0.00, -0.82)`
  that the walker slides on.
- **Read:** the per-model mesh's consumer in this image is a query that writes
  nothing (`FUN_14210bac0` -> `0x142caf6c0`), and the per-model Domino body
  builder `0x14201ae60` is reached from the collision component, whose attach is
  decided by an unread engine driver. The walk therefore uses
  `RangeWorld::level()` (the `MAP_CHUNK_COLLISION` / additive bodies) only, and
  `walk_world` errors when the range carries none.
- **Found and measured in the stand-in:** `snap_to_floor`'s "only down" guard
  compared the probe's answer with the probe origin (`body + 0.2`), so the snap
  lifted the body up to one probe lift per frame - `+0.118`, `+0.229`, `+0.165` m
  in three consecutive frames, `0.78` m total. The snap also teleports down by
  up to the 2.5 m reach and is load-bearing for the landing (the verified leaf
  reports nothing for a sweep that starts on or inside a surface), so it stays
  **open** rather than being patched into another guess: `open-questions.md`
  item 1b, `formats/character-controller.md`.
- **Corner clip fixed (operator's second report).** Straight into a wall stopped
  but a corner let the body through. Reproduced in a unit test: the first frame
  crossed the second wall by **3 cm**. Cause: the slide leaves the body exactly
  tangent to the second wall, and the verified leaf reports nothing for a sweep
  that starts on or inside a surface, so the residual was applied in full.
  `World::overlap` now asks the leaf's own "already inside" mode (`block+0x72`)
  at `radius - CONTACT_SKIN` (a resting contact reads as inside at the full
  radius, so the cut separates resting from penetrating), `depenetrate` pushes
  the body out, and a miss moves to the depenetrated target instead of applying
  the residual. `--wall-audit`'s near-vertical tunnelled frames: **174 -> 27**
  of 8,741 legs (loose `|up| < 0.6`: 289 -> 71); the audit now prints that count
  separately.
- **Also:** `--walk-log` and `--walk-test` now print each contact's normal and
  the shape that produced it (`ow_sim::walk::Advanced::contact_shape`,
  `ow_client::walk::Step::contact`), so a blocked frame names its cause.

## 2026-11-14 - hero select: the mode's roster, the hero's own name and row, and a pick

- **Read: the hero asset names itself.** `STUHero.m_0EDCE350` (class offset 48)
  is a type-`0x07C` display string; `ow_tank::hero_select::hero_name` decodes
  the `teString` payload (two header bytes, text, final NUL) and strips the
  trailing space and the non-breaking space, the same two characters the
  reference's `GetCleanString` strips. The install reads `Tracer`, `D.Va`,
  `Lúcio`, `Torbjörn`, `Soldier: 76`.
- **Read: the playable marker.** `STUHero.m_64DC571F` (class offset 904, one
  byte; the reference's `IsHero`) separates player heroes from NPC heroes. An
  open mode's roster is every type-`0x075` asset, and the practice range's is
  **135 hero assets: 54 playable and 81 NPC**. The range's `Training Bot`
  (index 363) is a *named* NPC, so the marker, not the name, is the filter.
- **Read: the movement row.** `ow_tank::entity::MoverRow` types the named
  `STUCharacterMoverComponent` floats at the class's own offsets: forward
  `0x19452F15` (`0xB0`), side `0xCFD3E635` (`0xA8`), back `0x61D6B4E9` (`0xAC`),
  crouch `0x0DDD18C3` (`0xBC`), terminal fall `0xB74245E8` (`0xEC`), plus the
  unnamed `0x031FE7EE` (`0xDC`). The install pins Tracer/Genji/Anran at 6.0 m/s
  forward and the rest at 5.5; the unnamed 5.5 scalar is **not** the forward
  row.
- **Reader and tests.** `hero_select::selectable_heroes` returns the mode's
  playable roster (an absent or unknown pool class is an error, never a widened
  roster); `resolve_hero` matches a name with case, spaces and punctuation
  folded or an asset index, never a default. `tests/hero_select_real.rs` checks
  the practice (open), Escort (29-hero whitelist), Stadium (mixed) and Mei (one
  hero) rosters, every playable hero's name/marker/row, and the negative picks.
- **The client's stage (a declared viewer default).** `ow-client --list-heroes`
  prints the practice roster with forward speeds; a live launch with a terminal
  prompts for a pick (`--hero <name|index>` names it directly, `--no-hero`
  skips), then the scene places the hero's **own first-person model and
  weapons**, the walk's wish magnitude is the hero's forward base speed (6.0
  m/s for Tracer) and the fall clamp is its terminal fall. The view model is
  re-placed at the camera's own transform each frame (pinned by
  `camera_tests::the_view_model_frame_is_the_camera_frame_at_every_pose`).
  `--walk-test --hero tracer` runs the whole proof at 6.0 m/s and passes.
  The pick itself is **not** claimed: the real pick is a CEF page value and
  variable 940's writer is open (`docs/formats/frontend-pipeline.md` 4a-4c).

## 2026-11-13 - CASC: the page selection is read, eleven verified units, byte census

- **Verified.** `fn/EncodingPageIndexSearch` (`EncodingPageIndexSearch`,
  `0x142498880`, 146 bytes): the search runs unchanged in `ow-casc` as
  `encoding::page_index_search` and the unit drives it directly with its five
  arguments, the fifth on the stack. Differential over 364 cases of the
  return, the index buffer and the key buffer; 4/4 branch edges (100%); 11/11
  mutants killed. The fixture pins the `0xFFFFFFFF` miss for zero pages, the
  lower-middle `mid` via an all-equal index, the raw `key_len` compare, and
  `key_len` 0..`width + 1`. The one declared gap is a page count whose 32-bit
  entry-offset product wraps - the arena cannot hold that index.
- **Verified.** `fn/Md5Transform` (`Md5Transform`, `0x1426ab530`): the nested
  `transform` in `blte.rs` is now `pub fn md5_transform`, called by `md5()`, so
  the unit tests shipped code. The 16 state bytes are compared and the block
  must be unchanged; the body has no conditional branch, so coverage reports
  0/0 = 100% and 18/18 mutants carry the unit. `md5()`'s RFC 1321 tests still
  pass, so every digest is bit-identical after the hoist.
- **Corrected.** `fn/EncodingFindCKey` and `fn/EncodingFindEKey` said the
  search's branch sites were unmeasured "because the oracle rejects stack
  arguments". It does not: the oracle writes argument 5 at `[rsp+0x28]`, and
  five other units already rely on that. Their notes now point at the search
  unit instead.
- **Reading note.** Ghidra's `Md5Transform` listing stops after 64 bytes and
  the rest of the straight-line body belongs to no function; the body runs to
  its return at `0x1426abca8` (1913 bytes, no conditional branch).
- **Read: the CDN index page selection, the last open container item.**
  `CdnIndexPageSelect` (`0x1424da730`, 469 bytes) lower-bounds the last-key
  array at `index+0x88` (stride `+0x5e`) with `min(keyLen,keyBytes)` and an
  early equal exit, misses at `page == pageCount`, makes the page resident and
  scans `min(entriesPerPage, elements - entriesPerPage*page)` records, decoding
  the size (u32 BE) and offset (u64 BE) by the footer widths.
  `CdnIndexPageResident` (`0x1424da590`) is pointer arithmetic in whole-file
  mode, or an HTTP range read (request `0x142522870`, WinHTTP `0x142521d70`)
  with a full-page requirement and a per-page MD5 reject (status 8) in
  streamed mode - the per-page request the docs had recorded as missing.
  `CdnResolveLocation` (`0x1424b0cc0`) splits the 6/5-byte offset into
  ordinal/offset and falls back to the file index. `ow-casc`'s `find_page` now
  follows the expression; a committed test pins the duplicate-last-key early
  exit. A one-off install check walked all 63,149 + 11,800 last keys and 606
  records; each selects its own page.
- **Verified.** `fn/CdnIndexPageSelect` (`0x1424da730`, whole-file branch):
  100% of the reachable branch edges, 14/14 mutants, every channel compared
  (AL, `out_offset`, `out_size`, `out_key16`, the whole index object, the key
  buffer). The streamed resident branch, a null `out_key16` and a short page
  body are declared unreachable/uncovered; the streamed branch is read but not
  implemented by the port.
- **Verified (harness soundness).** The mutation stage no longer counts a
  build/transport failure as a kill: an unevaluated mutant is retried once and
  then makes the unit `inconclusive`. A timeout is retried at twice the budget
  and then killed (a non-terminating mutant is a divergence). Two stale
  line-keyed equivalent declarations were corrected after code drift
  (`fn/EncodingFindEKey` 931->991, `fn/Salsa20Xor` 641/662->652/673); both
  units are verified again.
- **Verified (footer status).** The harness gained a `u8` return kind that
  masks RAX to AL. `fn/CdnIndexFooterParse` now compares the client's status
  (success 0, every reject 8), so the final length gate is covered: 12/12
  mutants, no declared gap for it.
- **Measured.** `cargo run --release -p ow-casc --bin census` (full stride-1
  scan): 1,146,326 records, 84,463,293,804 index bytes, 84,373,987,087 chunk
  bytes, N/Z/E = 10.50%/85.96%/3.54%. The 43,061 `E` chunks are 3.54% of chunk
  bytes and 3.42% of decoded bytes; their 11 key ids are absent from the 63-key
  keyring, so 22,506 records (1.96%) cannot be decoded. `blte_real` now asserts
  the per-mode byte sum equals the frame headers' total.
- **Corrected.** `PageReadPlan` was called a pure leaf; it takes the container
  lock twice and AddRefs the archive handle, which is why it has no unit.
- **Gate.** `cargo xtask verify` green; `python dev/verify_harness/cli.py
  casc` green on eleven units.

## 2026-11-12 - effect chunks: the components are read, and the reference is stale

- **Read.** An effect's root chunk is `EFCT` (version 5, payload ≥ 0x80). The
  client's kind table at `0x143610490` holds 0x34 `{u32 tag, u32 slot}` records;
  the builder `FUN_142e4a780` looks a tag up there, writes the slot to the
  runtime record's `+0xb8`, and copies a fixed set of payload fields per tag
  (`payload = record + 0x10`). `FUN_142e4e8e0` is the offset→pointer fixup and
  gates some tags on the chunk version; `FUN_142e4fbe0`/`FUN_142e50110` handle
  the array sub-chunks.
- **Fixed in `ow-tank`.** `effect.rs` was a port of OWLib's effect structs,
  which describe pre-2.x payloads: `ECMD` 24 B vs **32**, `ECEF` 8 vs **32**,
  `ECEN` 16 vs **80**, `ECEC` 17 vs **64** (identifier moved `+0x08`→`+0x10`),
  `ECSO` 8 vs **32**, `ECVS` 8 vs **48**, `ECRR` 8 vs **64**, `ECPR` 40 vs
  **112**, `ECAT` 8 vs 16, `ESHT` 8 vs 32. Every reader was replaced by the
  client's copies: `Effect::parse` walks the tree, `Chunk` carries the tag, the
  kind slot, the version and the payload, and the seven component kinds with a
  GUID have typed accessors at the client's offsets.
- **Verified.** `tests/effect_real.rs` parses the six effect types and asserts
  every component's GUID type (`ECMD` model `0x00C`, look `0x01A`, animation
  `0x006`; `ECEF` `0x00D`; `ECEN` entity `0x003`; `ECEC` animation `0x006`,
  identifier `0x01C`; `ECSO` `0x02C`; `ECVS` `0x078`; `ECRR` model `0x00C`),
  every payload size and every measured version. Unit tests pin the kind table
  round trip and the short-payload rejections.
- **Closed as impossible from this image.** `ECMP`, `ECSS`, `ECAT`, `ESHT` and
  `ECCV` have **no read site**: their four-byte tag ids occur nowhere in either
  byte order, and they are absent from the kind table and the builder's chain.
  OWLib's readers for them are reconstructions of data the client never reads.
- Page: [`docs/formats/effects.md`](docs/formats/effects.md). Tests:
  `cargo test -p ow-tank --lib effect` and
  `cargo test -p ow-tank --release --test effect_real -- --nocapture`.
- **Also fixed: the model chunk tags.** The client's two model dispatchers
  compare `MOCL` (`0x4d4f434c`) and `CLTH` (`0x434c5448`); an earlier version of
  `docs/formats/model.md` wrote `LCOM` and `HTLC`, which are the same words with
  their bytes in the wrong order, and `HTLC` occurs nowhere in the image. The
  page and the glossary are corrected.
- **Deleted: the `MHRP` hardpoint parser.** The tag has no constant anywhere in
  the image and a census of all 32,280 models finds no `MHRP` chunk. This
  build's hardpoints are `STUModelHardPoint` instances in `MSTU`/look, which
  `hardpoint.rs` already reads and `ow-scene` uses; the reference-only chunk
  parser (`parse`, `parse_payload`, the 96-byte record) is removed rather than
  kept as a reader for a chunk the client cannot read.

## 2026-11-11 - model parsing: the descriptors are the client's, the vertex decode is not

- **Read.** The model asset has two parallel dispatchers over the same chunk
  table: the sizing pass `FUN_141b5e1e0` and the load pass `FUN_141b5df70`.
  Both gate every chunk on `*(u16 *)(record + 0xe)`: `MDLC` `0x165`, `MRNM`
  `0x1f`, `mskl` `0x20`, `LCOM` `0x14`, `MESD` `2`, `HTLC` `0x18`, `MSTU`
  `2`. `MdlcMaterialRead` (`0x141b88cb0`) takes the material count from the
  `u16` at `+0x68` and the GUIDs from the pointer at `+0x08`. The MSTU header
  is `{u64 offset; u32 size; u32 unused}`: `FUN_141b88df0` passes
  `*(int *)(payload + 8)`, four bytes, to the STU v2 loader `FUN_1421d88c0`.
- **Fixed in `ow-tank`.** `model.rs` read a 4x4 `bounding_box` from `MDLC`
  offsets `0x10..0x50`; the client copies scalar fields there, and no such box
  exists (field removed; it was unused). `mstu.rs` read the size as an `i64`;
  the client reads a `u32` (fixed, install 60/60 still parses). Both readers
  now enforce the client's version gate instead of parsing a future layout,
  and the descriptor offsets are the verified constants `ow-tank::mrnm`
  already carries instead of copied literals.
- **Verified.** `crates/ow-tank/src/mrnm.rs` is pinned by three shipped-code
  differential units, all passing after the change: `fn/MrnmFixup` (7/7
  mutants, the three header slots, the element slot, the two stream slots
  only when `flags & 0x10` is clear, the index data slot), `fn/MrnmRuntimeSize`
  (6/6, the 32-bit size expression) and `fn/MrnmHandler` (12/12, the whole
  runtime block, header, submesh `0xa0`, vertex-buffer `0xf8`, index-buffer
  `0x18`, the `-1` roots, the `+0x7b` default byte and the OR'd `+0x00` mask).
  The handler's stream branch was the one that had been guessed backwards:
  it **zeroes** the runtime `+0xa0/+0xa8` for in-chunk streams and copies the
  two GUIDs only for a separate asset.
- **Declared open, not faked.** The vertex-element decode (semantic/index/
  format bytes, the packed offset `(packed >> 12) & 1023`, the format table)
  and the index remap/compact vertex array are the viewer's own reading; the
  client's consumer of the element array is the open IA stage
  (`docs/formats/renderer.md` R10). `MDLC` has no differential unit yet (the
  harness cannot express its two absolute source pointers). MESD, HTLC, LCOM
  and mskl are not parsed.
- Page: [`docs/formats/model.md`](docs/formats/model.md). Tests: `cargo test -p
  ow-tank --lib model:: mstu::`, `--test model_mesh_real --test mstu_real`.

## 2026-11-10 - walls stop the walk: the slide and the floor snap both lifted

- The operator's report ("i can go through walls") reproduced on the range's own
  data: a walk into the level collider at the spawn climbed `0.40 m` up the wall
  and ended `0.245 m` past its plane. Two height leaks were the cause, both in
  code this project wrote (the slide and the step are the client's *open* part,
  `RunCharacterControllerSweep`'s Domino aftermath):
  - the residual was projected onto the contact plane, so a wall whose normal
    carries `0.09` of up (the range's walls report `(-0.576, -0.086, -0.813)`)
    acted as a ramp and each frame raised the body;
  - `snap_to_floor` lifted the body onto any up-facing surface inside its reach.
    A downward probe that *starts inside* a surface's expansion reports a
    fraction that is not a distance, so the "resting height" came out `0.15` and
    `0.19 m` **above** the body on two consecutive frames of the 48-degree
    bevels.
- The rules now: a contact steeper than `walk::WALKABLE_SLOPE` (`0.7`, 45.6
  degrees) cannot raise a walk, and the floor snap only ever lowers the body.
  No new magic constant for the height itself. `0.7` is our number (the client's
  slope limit is unread) and is listed in `docs/open-questions.md`.
- Proven by `walk::tests::a_horizontal_walk_cannot_climb_a_tilted_wall` (a
  `0.0995`-up normal, 240 frames, height change `0.000 m`) and by the client's
  own headless harness, which now walks at every flat wall within 12 m of the
  spawn and asserts a contact, no climb and no frame whose *centre path* crosses
  a surface: `ow-client --walk-test` (before: climb `+0.40 m`; after: `+0.01 m`).
  `ow-client --wall-audit` adds a map-wide version: 1,271 standable points,
  8,645 legs, 312 small crossings, all at points where the audit places a body
  at or inside geometry.
- Jump is **not** implemented, and now says so in the window: the command half
  of the jump is read (`0x140cbb260` splits command bits 6/7 out of `body+0xdc`
  into `FUN_140cc1060` -> `FUN_140cc1440`), but `0x140cc1440`'s vertical
  arithmetic is control-flow-flattened and not read, so Space prints
  "jump: not implemented" rather than inventing a velocity. Anchors in
  `docs/open-questions.md` ("The jump law").

**Archive.** Entries older than 2026-11-10 are kept verbatim in `docs/history/modlog-2026-10.md` (2026-10-26 .. 2026-10-31, 6 entries), `docs/history/modlog-2026-11.md` (2026-11-01 .. 2026-11-09, 80 entries).

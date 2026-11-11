# verify_harness performance

Measured on the operator's machine (12 cores, Windows, repo on `E:`), 2026-10-27.
The gate is unchanged: same five stages, same corpus (`iterations`, the 48-case
mutation corpus), same coverage and mutation thresholds. Only *when* work runs
changed, never *what* is measured.

## The numbers

| unit | before | after | speedup |
|---|---:|---:|---:|
| `fn/BundleLookup` (12 mutants) | 1310.6 s | 77.9 s | 16.8x |
| `fn/BundleDirectoryLookup` (35 mutants) | ~1480 s (reconstructed, see below) | 158.1 s | ~9.3x |
| `fn/ChunkGuid` (3 mutants) | - | 15.9 s | - |
| `casc` (5 units, 106 mutants) | - | 128.5 s | - |

`fn/BundleDirectoryLookup`'s "before" was not re-run with the old harness (it
would have taken another ~25 minutes and the tree had already changed). It is
reconstructed from the same measurements that produced the BundleLookup
number: 35 mutants at the measured per-mutant full build (28.5-31.1 s, four
concurrent) is 9 build rounds x ~29 s = ~261 s, and the after-run's own log
shows four mutants that need the 300 s call budget: 4 x 305 s = 1220 s, plus
~1 s of real work. That agrees with the ~25 minutes the unit was observed to
take before this change.

The "after" numbers are with the mutant build slots already warm. On a first
ever run each of the four slots pays the dependency compile once: measured as
47.7 s instead of 36.4 s for the first eight mutant builds, i.e. about 11 s
extra in total, and every later run reuses the slots.

## Where the time went (before)

`fn/BundleLookup`, 1310.6 s total:

| phase | wall time | share |
|---|---:|---:|
| load + determinism + differential + coverage (364 cases) | 0.6 s | 0.05% |
| 12 mutant builds, four at a time, ~29 s each (own target dir, full dependency rebuild) | 90 s | 7% |
| mutant runs, **serial**, 4 of them hitting `DEFAULT_CALL_TIMEOUT = 300 s` | 1220 s | 93% |

The runs themselves are trivial: the slowest non-hanging mutant call took less
than 10 ms, and the baseline probe's 364 calls took 30 ms in total. The wall
time was 300-second waits on broken code that was already caught at its first
differing case.

## What changed

1. **Mutation-only call timeout.** `run_unit(..., mutation_call_timeout=...)`,
   unit option `mutationCallTimeout`, CLI `--mutation-timeout S`. A mutant
   probe call is bounded by 10 s (default); the baseline candidate keeps the
   300 s budget, and the probe's startup handshake keeps the 300 s budget too,
   so a cold first launch is never mistaken for a defect. Measured margin:
   every non-hanging call took <10 ms, i.e. >1000x. A timeout still reaches the
   same `detected = True` path as any other fault, so it is killed exactly as
   before, and the mutant's log line names the timeout.

2. **Parallel mutant execution.** One `ThreadPoolExecutor(min(4, active))`;
   each worker builds *and* runs one mutant, then closes it. The oracle results
   for the 48-case mutation corpus are gathered first, in the main thread, from
   the stage-3 cache - the emulator is never shared with a worker. `pool.map`
   keeps the report in mutant-generation order, so the log and the survivor
   list are deterministic. `--jobs N` changes the pool size only.

3. **Warm mutant build cache.** Mutant builds share four cargo target
   directories per crate (`target/verify_probe/<crate>/_mutants/slotN`), so the
   dependency graph (registry crates, `ow-casc`, `ow-stu`) is compiled once per
   slot instead of once per mutant. A slot is checked out for one build at a
   time; measured build cost drops from 28.5-31.1 s to ~8 s alone (~17 s with
   four slots busy). Two safeguards keep this honest:
   - `cargo clean -p <package>` before every mutant build (0.2-0.3 s) removes
     that package's artifacts and fingerprints from the slot, so the mutant is
     always recompiled from the source just written. Cargo's freshness check is
     mtime-based and was measured reusing an artifact for identical content;
     the clean removes any dependence on that.
   - after the build the artifact's `(mtime_ns, size)` must have changed, and
     the binary is copied into the mutant's own work dir before the slot is
     returned. A stale artifact is a hard error, never a silent probe.

## Quality proof

`C:\Temp\ledger-before.json` holds the five units' ledger entries before the
change. After re-running them (`python dev/verify_harness/cli.py <unit>`) and
the `casc` suite:

| unit | status | coverage | mutants | divergences | match |
|---|---|---|---:|---:|---|
| `fn/BundleLookup` | verified -> verified | 100.0 -> 100.0 | 12/12 -> 12/12 | 0 -> 0 | yes |
| `fn/BundleDirectoryLookup` | verified -> verified | 93.333... -> 93.333... | 35/35 -> 35/35 | 0 -> 0 | yes |
| `fn/ChunkGuid` | verified -> verified | 100.0 -> 100.0 | 3/3 -> 3/3 | 0 -> 0 | yes |
| `fn/CmfKey154504` | verified -> verified | 0.0 -> 0.0 | 1/1 -> 1/1 | 0 -> 0 | yes |
| `fn/CmfIv154504` | verified -> verified | 15.151... -> 15.151... | 5/5 -> 5/5 | 0 -> 0 | yes |

The oracle, options and surfaces digests of every entry are byte-identical
before and after; the candidate digest differs only because the crate
revision (`HEAD`) had already moved past the saved ledger before this work
(the first run printed the harness's own "previous verdict is stale" note).
`HARNESS_VERSION` was bumped 4 -> 5 because the mutation stage's execution
model changed.

The `casc` suite's five units (106 mutants: 12/24/39/24/7) were re-run too and
all five matched their saved entries as well (verified, same coverage, same
killed/total, no divergences). In total 162 mutants across ten units produced
the same classification. The ledger's other units were not re-run here; they
report `stale` under the new harness version, which is the intended behaviour
of the version bump, not a change of their verdicts.

`fn/ChunkGuid --jobs 1` gives the same 3/3 verdict, so the pool size does not
change a classification. The harness's own tests
(`test_harness_smoke.py`, `test_run_unit_e2e.py`), `cargo xtask verify --fast`
and `cargo xtask verify` pass.

## What did not change

- `iterations`, `mutationCount`, `coverageThreshold`, `mutationKillRate`, the
  48-case mutation corpus, the fixture modules, and the five stages.
- No unit JSON was edited. No mutant was declared equivalent.
- `DEFAULT_CALL_TIMEOUT` for the baseline candidate is still 300 s.
- The C (tier-1) candidate is called in-process, so `mutationCallTimeout`
  does not apply to it; its mutants were already fast, and a C mutant that
  never returns would still wedge the harness. That is a pre-existing
  limitation, unchanged here.

## 2026-11: fresh-verdict replay, parallel units, slots knob (harness v7)

The remaining wall of `cli.py all` was structural, not per-call: ~1300 mutant
cargo builds at ~8-17 s each over four slots per crate is ~45 minutes no
matter how fast any single probe is, and `all` re-proved all 102 units every
time even when nothing on either side had changed. Three changes, none of
which skips a stage inside a run, weakens a gate, or edits a unit JSON:

1. **Fresh-verdict replay.** The ledger already binds every input of a
   verdict (oracle window sha, candidate artifact sha, options sha, surfaces
   sha, harness version) - that is its staleness contract. `all` and `casc`
   now use that contract as a cache: a `verified` entry whose digests all
   still match is replayed (reported `(cached)`) instead of re-running its
   stages. Re-running would reproduce the recorded verdict by construction:
   the recorded run proved the oracle deterministic on identical inputs, and
   the digests cover every input the five stages consume. The ledger is not
   rewritten on a replay, so an entry keeps the timestamp of the run that
   actually proved it. Only `verified` entries replay; stale, divergent and
   inconclusive units always re-run. `--force` re-proves everything. A single
   named unit re-runs by default (`--cache` opts it back in).
   Consequence for the crate-rev binding: a commit (any commit) changes
   `candidate_sha256` of every cargo unit, so the cache survives *edits* but
   not *commits* - unchanged behaviour from the ledger's own rules.
2. **Unit-level parallelism.** `--unit-jobs N` (default `min(4, cpus)`, 1 =
   the old serial behaviour) runs whole units in a thread pool. A unit is
   never split: each thread runs the full five-stage ladder for one unit.
   The heavy half of a run is subprocess work (cargo builds, probe
   processes), which threads overlap fine; stages 1-4 are ~0.6 s per unit
   (measured above) so GIL contention is noise. The per-crate mutant slot
   queues were already global and lock-guarded, so concurrent units of one
   crate share the four slots instead of multiplying them. Concurrent
   baseline probe builds of one crate share `target/verify_probe/<crate>`,
   which cargo already serialises with its own target-dir lock. Ledger
   writes go through a lock (load-then-record is a read-modify-write).
   Three crates ship cargo units (ow-sim, ow-casc, ow-tank), so at
   `--unit-jobs 4` up to 12 mutant builds run at once instead of 4.
3. **Slots knob.** `--slots N` / `VERIFY_MUTANT_SLOTS` sets the mutant build
   slots per crate (default still 4, the measured number above). More slots
   = more concurrent builds AND more disk (each slot holds a full dependency
   tree); the measured 8 s -> 17 s contention at four slots is the reason
   the default did not move.

Naming several units (`cli.py fn/A fn/B fn/C`) runs them as one parallel
batch, which is the subagent-batch workflow's verify step; a typo in any
name is now an error instead of a silently ignored extra argument.

**Not measured on the operator's machine** (this change was written on a
machine without the oracle image, with 2 cores): the real `all` wall time
before/after, and the best `--slots`/`--unit-jobs` for 12 cores. Measure
with, from the repo root:

    python dev/verify_harness/cli.py casc --force   # one full re-run after v7
    python dev/verify_harness/cli.py casc           # expect: all (cached), seconds
    python dev/verify_harness/cli.py all --force --unit-jobs 4 --slots 6
    python dev/verify_harness/cli.py all            # expect: only stale units run

and fill the numbers in here. What *is* proven here, on synthetic images
(`test_cache_and_parallel.py`, 29 checks, all green): a replayed entry never
re-executes a stage and does not touch the ledger; an edited candidate,
edited options, a non-verified verdict and `--force` all re-run; eight
concurrent units keep all eight verdicts in the ledger; a CLI batch runs in
parallel, replays under `--cache` and re-proves under `--force`.
`test_cargo_mutant_builds.py` (needs cargo; skips itself without it) builds
the real ow-sim probe, builds two mutants concurrently on two slots, and
proves each binary carries its own mutation - the slot pool never lets one
mutant's build answer another's probe.
`test_harness_smoke.py` and `test_run_unit_e2e.py` also pass (the two had
been broken since the image sha pin: they build synthetic images, which the
pin rejected; they now register their digests via
`emu_oracle.allow_image_sha256` instead of monkeypatching the pin away - the
production path still refuses any image but the pinned one).

HARNESS_VERSION was bumped 6 -> 7, so the first `all`/`casc` after this
change re-runs every unit once (~the old cost); from then on a run verifies
only units whose digests changed.

## 2026-11: the xtask gate stops walking the tests serially (v8)

The other slow gate was `cargo xtask verify` (~30 minutes when green): one
`cargo test` walks every test binary serially, `index_find_real` alone runs
1154 s, and the `ow-tank` install suites run minutes each. The gate now
builds every binary once (`cargo test --no-run --message-format=json`),
picks the executables out of the compiler-artifact messages, and runs them
in a pool (`--test-jobs`, default `min(4, cpus)`). Cheap checks run first
(fmt, the four python doc checks), `--fail-fast` stops at the first failing
stage, and a failure prints its captured output after the pool drains.

Not measurable on this machine (no install, 2 cores): the win against the
serial walk. On a machine with the install, measure it as:

    cargo xtask verify --test-jobs 1    # the old serial walk
    cargo xtask verify                 # default pool (4 binaries at a time)
    cargo xtask verify --test-jobs 8    # more I/O overlap
    cargo xtask verify -p ow-casc --skip index_find_real   # an iteration loop

The scoping flags (`-p`/`--package`, repeatable; `--skip`, repeatable - drops
a binary whose label matches, else libtest's own per-test filter) do not change
the gate: the unscoped run is still the definition of done. They exist because
the gate's floor is one test in one crate, and a change in `ow-sim` should not
have to pay for it. Measured on a 2-core box with the install: the `ow-casc`
scope is 20 s with `--skip index_find_real` and over 19 minutes without it.

and read the per-binary durations the run now prints. Expected shape: the
wall becomes the longest single binary (~19 min, `index_find_real`) plus
the build, instead of the sum of all binaries. The 1154 s floor itself is
untouched - a `key_state`-free resolve path stays classified as a perf
change, not a port (`STATUS-handoff.md`, "Measured, not fixed").

## 2026-11: coverage-guided corpus growth, `autoCorpus` (harness v8)

The per-unit fixture cost was the bottleneck for the long tail: 105 fixture
modules, 15,078 lines, ~143 lines per unit, and ~800 named functions still
without a unit. About a third of them (flat buffers + scalars: hashes, memcmp,
hex parsers, header readers) never needed a *hand-written* corpus - they
needed a corpus that clears the same gates. `options.autoCorpus` makes that
the harness's job:

1. The base corpus is unchanged: the focused edge pool plus `iterations`
   seeded draws.
2. After the differential loop agrees on every channel, the runner measures
   the branch coverage of those very runs (the same `BranchTracker`, still
   installed before stage 2) and, while it is under the gate, compares more
   draws from the same deterministic `(seed, i)` sequence - 50 per round,
   1200 cases total by default (`{"cap": N, "step": M}` overrides). Growth
   only ever ADDS compared cases through the same `_run_case` body: the same
   channels, the same undeclared-access accounting, the same counterexample
   format, and the same three-divergence stop.
3. The mutation corpus gains the grown tail. The head's 48 cases stay pinned,
   but a mutant can be distinguishable only on the branch a grown case
   uncovered (the constant-edit mutants of a magic-value compare are exactly
   this shape), so `cases[:48] + cases[base:]` is what the mutants are probed
   on. Measured in the test below: the `boundary ±1` mutants of the magic
   compare are killed at the grown case, not by the head.
4. The ledger entry records the realized corpus (`corpus_sha256` over every
   case's stable identity, `corpus_cases`), so an audit can see exactly what
   was compared. It is a fingerprint, not a freshness input: the corpus is a
   deterministic function of what the ledger already binds (args surface,
   options incl. seed/iterations/cap/step, the oracle window, harness
   version), which is what keeps the fresh-verdict replay sound.

What did NOT change: the five stages, every gate, the fixture path (a unit
with an `argFactory` never grows - the factory's corpus is the fixture's
claim, and `autoCorpus` + `argFactory` warns and ignores the option), and
every existing unit's corpus (no unit JSON was edited; `autoCorpus` defaults
to off). `scaffold_unit.py` templates now set it, so a new scaffold for a
flat signature needs args and returns, not a fixture.

Not measured here (no oracle image on this machine): the growth cost on real
units. It is bounded by construction - at most `cap - iterations` extra
oracle+candidate calls at stage 3's per-case cost, which PERFORMANCE.md's own
measurements put at ~0.6 s per 364 cases for stages 1-4 - plus the larger
mutation corpus for grown units only (`cap` cases x mutants, sub-millisecond
calls each).

Quality proof (`test_auto_corpus.py`, synthetic image): a one-branch function
whose taken edge needs draw 352 of seed 4241 (unique in [0, 1200), absent
from the 300-draw base - re-derived from gen.py, not hand-picked) grows
300 -> 400 cases across two rounds and verifies with the `±1` constant
mutants killed by the grown case; the same unit with `{"cap": 320}` stops
below the magic draw and is inconclusive at the coverage gate with the 320-case
corpus recorded; a forced re-run reproduces the identical corpus digest; the
fresh-verdict replay serves the cached entry; toggling `autoCorpus` changes
the options digest so the stale entry re-runs. The pre-existing suites
(`test_harness_smoke.py`, `test_run_unit_e2e.py`, `test_cache_and_parallel.py`)
pass unchanged; `test_cargo_mutant_builds.py` needs cargo and was not run
here (no Rust code and no cargo path changed).

HARNESS_VERSION was bumped 7 -> 8: the first `all`/`casc` after this change
re-runs every unit once (~the old cost); from then on only units whose
digests changed.

## 2026-11: a stale "equivalent mutant" declaration fails the unit (harness v8)

The mutation stage's kill rate is the unit's proof that the differential can
tell a wrong candidate from the right one. A `knownEquivalentMutants` entry is
the unit's claim that one mutation is *not* wrong - so it is skipped. The
claim is written as a line number in the candidate's source, and a refactor
moves lines: a declaration that names nothing skips nothing, the equivalent
mutant is probed again, survives (it is equivalent), and the kill rate falls.
`fn/Salsa20Xor` fell from 100% to 70% and dropped below its 75% gate; eight
other units sat at 75-83% without noticing. The failure is silent in the
other direction too: a stale declaration that still names a line - but the
wrong statement on it - suppresses a mutant that should have been killed.

Three changes:

1. **The guard.** Stage 5 scans every mutation site the entry function has
   (`MUTATION_SITE_SCAN`, 4096) and compares it with the unit's declarations.
   A declaration that names no live site fails the unit with `inconclusive`
   and a note that names the stale entries. The unit is not verified against
   a weaker mutant set; it says why it stopped. One scan per unit, on the
   candidate's own source - no extra build, no extra oracle call.
2. **A finer key.** The mutator now records the *mutated* text of each
   mutant, and a declaration may carry it. One line can hold two mutants of
   the same kind - a decimal and a hex constant each get a boundary shift -
   and kind+line could not tell them apart, so a declaration covering one
   silently covered the other. `fn/BuildMoverPhysicsCommand` is the case that
   needed it: three equivalent mutants and two non-equivalent ones on a
   single line, with the old key unable to skip the right three.
3. **The ledger binds the declarations.** A verdict is replayed only when the
   declarations that produced it are unchanged (kind, line and mutated text -
   not the rationale, which is prose). Changing what the mutation stage skips
   invalidates the verdict; re-wording why does not.

One consequence worth knowing: a tier-1 C unit's candidate is a *generated*
decompile (`dev/decompiles/<Name>_<addr>.c`, written by `dev/gh_decompile.py`),
so regenerating it moves its lines and the unit stops with the guard's note
until its declaration is re-pointed. That is the point - the declaration is a
claim about code, and a regenerated decompile is new code. The unit's own
body is otherwise unchanged.

The cost is one extra source scan per unit (milliseconds) and no change to
the five stages, the gates or the corpora. Proof: `test_harness_smoke.py`
grows a case that shows two same-line boundary mutants are separable by
their mutated text and that a declaration without the text still covers
both; `test_run_unit_e2e.py` runs the whole ladder on a unit whose
declaration names a line the candidate does not have and expects the
inconclusive verdict and the note.

## 2026-11: one build for a unit's mutants, and the install reads cached

Two costs dominated the verification, and both are gone.

### The mutation stage: one crate build per unit, not per mutant

Measured before: a cargo unit's mutants were each a full crate rebuild -
4.4 s with a warm target dir - and a unit has a dozen of them. That was ~80%
of the harness's wall time; a full re-prove of the 107 units cost ~35
minutes, almost all of it in `cargo build`.

Now every mutant of a unit is an **arm** of one dispatch inside the function
under test (`batch.py`):

```rust
match __ow_mutant_index() {
    0 => { <mutant 0's body> }
    1 => { <mutant 1's body> }
    _ => { <the original body> }
}
```

The arms are the original interior with the mutation applied, so a mutant is
byte-for-byte what it was when it was built alone. The harness picks the arm
with `OW_MUTANT_INDEX` in the child's environment, so one build serves every
mutant (one process per mutant; the process is cheap).

Three things keep this honest:

1. **The default arm is the original body**, and the harness compares the
   batch binary (with no index set) against the baseline candidate before it
   trusts the batch. A mismatch falls back to one build per mutant: the
   batch is an optimisation, never a change of what is measured.
2. **A compiler error is attributed to its arm.** The error names the line,
   the line names the arm, the arm is dropped and the batch rebuilt. The
   compiler catching a mutant is a kill, exactly as in the old path. An
   unattributable error falls back to the old path.
3. **The verdicts are unchanged.** Same corpus, same comparisons, same gates.
   All 111 ledger entries agree on the status, the killed count and the
   total, before and after. Ten of the slowest units, the same corpus both
   ways: **324 s without the batch, 85 s with it** (3.8x); a full forced
   re-prove of all 107 units: ~35 minutes -> **542 s**. Per unit:
   `fn/OrthonormalizeCameraBasis-cargo` 23.2 s -> 6.6 s,
   `fn/BuildMoverPhysicsCommand` 17.2 s -> 5.8 s, `fn/Md5Final` 17.0 s ->
   7.5 s.

Two shapes the first version got wrong, both found by the stage timings
rather than by the verdicts:

- The dispatch helper is a free function, so it belongs at the file's
  module scope. A function under test that is *nested* inside another one
  cannot hold it where it was, and the batch silently fell back to one
  build per mutant for those units.
- The dispatch replaces the function's *body*, so a mutation on the
  *signature* (the line carrying the opening brace is part of it) cannot be
  an arm. Dropping it silently cost four units a kill and their gates. A
  mutation outside the body is now simply not in the batch, and the caller
  builds it on its own, as it did before.

`VERIFY_NO_BATCH=1` turns the batch off: the escape hatch for a crate whose
build refuses the generated dispatch, and the way to A/B the two paths.

The stage now prints its own wall time, and so does every other stage: the
cost was not uniform, and "make it quick" is a guess without the numbers.

Proof: `test_batch.py` builds a tiny crate whose function has two swappable
operators and checks that the built binary answers 43 with no index set, 11
with the `*` arm and 41 with the `+` arm; the error-attribution cases are
pure.

### The install-reading suites: two caches in the shipped reader

The Rust gate's cost was seven install-reading suites (2218 s of the 2246 s
of test time). Both hot spots were in `ow-casc`, and both were the same
mistake: a value that is a pure function of the install was rebuilt on every
call.

| what | before | after |
|---|---:|---:|
| the segment's byte form, rebuilt per `key_state` call | 1154 s (`index_find_real`) | 1.5 s |
| the encoding manifest's espec index, rebuilt per asset open | 213 s (`effect_real`) | 4.0 s |

`SegmentEntries::key_state` rebuilt the whole 18-byte-per-entry buffer on
every call, so the install test - which resolves every one of 1.1M keys -
copied tens of gigabytes. The bytes are a pure function of the entries and
the page table (the client keeps its parsed entries in memory for the same
reason), so they are built once.

`Store::open_ckey` re-parsed the decoded encoding manifest on every asset
open, and the parse's expensive half is the espec string index: one u64 per
byte of a 100 MB manifest. The index is a pure function of the block, so it
is built with the manifest (`EncodingFile::parse_with_index` +
`encoding::espec_index_of`) and the parse reuses it.

Measured, same machine, same assertions, every suite still passing:

| suite | before | after |
|---|---:|---:|
| `ow-casc` `index_find_real` | 1154 s | 1.5 s |
| `ow-scene` `skin_pose_gate` | 428 s | 4.4 s |
| `ow-tank` `effect_real` | 213 s | 4.0 s |
| `ow-scene` `practice_launch` | 186 s | 5.2 s |
| `ow-scene` `hero_draws` | 159 s | 2.8 s |
| `ow-tank` `hero_pick_real` | 152 s | 2.8 s |
| `ow-tank` `brush_real` | 134 s | 2.7 s |

Both changes are behaviour-preserving, and the differential harness covers
the changed code (`fn/DynamicIndexFindEntry`, `fn/DynamicIndexResolveRange`,
`fn/EncodingFindCKey`, `fn/EncodingEspecStringIndex`): a behaviour change
fails there, not here.

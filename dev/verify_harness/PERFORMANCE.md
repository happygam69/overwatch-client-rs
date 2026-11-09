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

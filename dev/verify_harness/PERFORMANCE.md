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

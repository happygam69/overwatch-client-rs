# verify_harness — review, bug fixes, and speedups

Scope: everything under `dev/verify_harness/`. Nothing outside it was touched.
The protocol semantics (five-stage ladder, gates, equivalence relation) are
preserved — every change either fixes a defect that made a verdict WRONG,
strengthens detection, or removes wasted work. No gate was lowered.

Deliverables in this directory:

| File | What it is |
|---|---|
| `*.py`, `units/`, fixtures | drop-in improved harness (same layout as upstream) |
| `verify_harness_improvements.patch` | unified diff against the current master (11 files) |
| `tests/test_harness_smoke.py` | 32 synthetic-image tests proving each fix (runs on any OS, no game dump) |
| `tests/test_run_unit_e2e.py` | full five-stage ladder end-to-end on synthetic bytes: correct decompile → `verified`, wrong decompile → `divergent` |

Everything below was verified by executing the code, not by reading it.

---

## 1. Critical bugs (wrong verdicts)

### 1.1 f64/u64 candidate returns were truncated to 32 bits — `candidate_c.py`
`CCandidate` set `self.fn.restype = ctypes.c_uint32` unconditionally, while the
generated wrapper returns `uint64_t` for f64/u64 units. ctypes interprets RAX
according to restype, so **the high half of every f64/u64 candidate return was
discarded**. Against the oracle's full-width bits, every f64/u64 unit either
diverges on garbage or — worse — can agree on the truncated half. There are no
f64/u64 units yet; this would have burned the first one. Fixed: restype is
`c_uint64` for f64/u64, `c_uint32` for f32. Proven by
`candidate_c/f64-retbits-not-truncated` and `candidate_c/u64-retbits-not-truncated`.

### 1.2 The oracle emulated a misaligned caller — `emu_oracle.py`
Entry `RSP = STACK_TOP & ~0xF` gave `RSP % 16 == 0`, but MS x64 guarantees
`RSP % 16 == 8` at function entry (the caller is 16-aligned at the CALL, then
the return address is pushed). Any real function that spills XMM with
`movaps [rsp+X]` after one push faults in the oracle while the native candidate
runs fine — a divergence the harness manufactures itself. (The anchor unit got
lucky: leaf, small frame, no aligned spills.) Fixed: `RSP = STACK_TOP - 8`.
Note: my first patch wrote `(STACK_TOP - 8) & ~0xF`, which rounds the offset
back off — the smoke test caught that, which is exactly why the tests exist.

### 1.3 Runaway emulation returned `ok=True` with garbage — `emu_oracle.py`
The step limit was enforced twice: natively by `emu_start(count=steps)` and in
a Python `UC_HOOK_CODE` hook that only fires when an instruction *starts*. The
native budget stops after exactly `steps` instructions, so the hook's
`n > steps` branch was dead code and **the "step limit exceeded" fault could
never trigger**. An infinite-looping decompile produced `ok=True` plus whatever
XMM0/RAX held — garbage compared against the candidate as if it were the
shipped behaviour. Fixed: no per-instruction hook at all; after
`emu_start` the result is `ok` only if execution ended at the sentinel
(clean return). Proven by `oracle/infinite-loop-is-fault`.

### 1.4 Coverage over-reported taken edges — `coverage.py`
`_sweep_branches` collected **one function-wide set of jump targets**, and the
per-branch "taken" check was `any(t in executed for t in targets if abs(t - cond) < 0x1000)`
— any executed target *anywhere nearby* credited an edge to *this* branch. Two
branches sharing a target, or a hot nearby branch, could mark a never-taken
branch as covered. Since `verified` requires coverage ≥ gate, this weakened
the harness's core claim. Fixed with exact edge attribution: a hook remembers
the previously executed instruction; when it was a discovered branch, the next
executed address **is** the resolved edge. A branch counts only when both of
its edges were observed on the compared runs. Proven by
`coverage/both-edges-100pct`, `coverage/one-edge-is-a-miss`,
`coverage/exact-edge-set`.

### 1.5 Ledger freshness ignored the fixture and the shim — `runner.py`
`surfaces_sha256` hashed only the declared arg names/kinds/ptr_len. Editing
`anim_time_wrap_fixture.py` — the very thing that pins the discriminating
cases — left the verdict "fresh". Hard rule 5 says a verdict expires when the
inputs change; the fixture IS an input. Fixed: the digest now also binds the
argFactory module source and `ghidra_shim.h` (tier-1), plus a
`harness_version` field (see 2.3).

### 1.6 The oracle was stateful between calls — `emu_oracle.py`
Registers and EFLAGS persisted across `call()`s (only arena/stack were reset).
A function reading a register it was never passed gets different values on
call 1 (fresh Uc) vs call N (leftovers from the previous case). The benchmark
in this review hit it live: the same input produced `ret=0x92` on call 1 and
`ret=0x62` on call 41 (41×146 mod 256 — `AL` accumulated). Stage 2's
determinism probe (one input, twice) cannot see this; the corpus can. Fixed:
all GPRs, XMM0–7 and EFLAGS are reset per call. The oracle is now genuinely
stateless, which is also what makes the new result cache sound.

### 1.7 A candidate crash killed the harness — `candidate_c.py` (documented, not fixed)
The docstring claims "a crash inside the DLL must not kill the run", but
`except Exception` around a ctypes call does not catch a native access
violation — on Windows the whole harness process dies, recording nothing.
Recommended fix (follow-up): route tier-1 candidates through the same
subprocess protocol the Rust probe already uses; it also lets candidates run
in parallel with the oracle. Not attempted here because it is a structural
change to a Windows-only path I cannot execute in this environment.

---

## 2. Robustness fixes

2.1 **Rust probe hang** — `candidate_rust.py` did blocking `readline()` with no
timeout; a deadlocked probe hung the harness forever. Now responses are read
on a thread with a per-call timeout (default 300 s) and the probe is killed on
breach.

2.2 **Missing callees fail at load** — `gcc -Werror=implicit-function-declaration`
turns a decompile calling an unprovided sibling into the protocol's
"compile failure is a load-stage failure" instead of a runtime call into
whatever the linker found.

2.3 **Harness version in every ledger entry** — the verifier is an input to its
own verdicts; a harness bugfix changes what `verified` means. Entries now bind
`HARNESS_VERSION` ("2"), and `check_entry` treats a foreign version as stale.
Consequence: **all existing ledger entries go stale once after this upgrade and
must be re-run** — which is the honest outcome of changing the oracle and the
corpus.

2.4 **Unit descriptor validation** — a typo'd key (`"adress"`, `"argfactory"`)
was silently ignored; a unit would quietly verify with the generic generator
instead of its factory. Unknown keys now warn loudly, arg kinds and
`returns` are validated, and `base` accepts int or string.

2.5 **Tolerant ledger loading** — unknown/foreign entry keys no longer crash
the load; they are reported and dropped.

2.6 **Lifecycle cleanup** — the main candidate and every mutant previously
leaked: Rust subprocesses on early-return paths (determinism/coverage
failures), compiled temp dirs forever (one per mutant per run). The runner now
closes candidates and deletes its temp dirs on every path (`CCandidate.close()`
unloads the DLL so Windows can delete it). Verified zero leaked temp dirs
after runs.

2.7 **f64 NaN equivalence** — `_bits_equal` implemented NaN==NaN for f32 only,
while f64 NaN payloads legitimately differ across codegen paths. f64 now
follows the same documented rule (still bit-exact otherwise, ±0 still
distinct).

2.8 **Dead code / cosmetics** — removed the unreachable step hook, the no-op
`if len(mutants) >= count*3: pass`, and the wrong unmapped-access kind map;
`ledger.py` resolves `LEDGER_PATH` at call time so it can be redirected.

---

## 3. Detection-power improvements (gen.py / mutate.py)

3.1 **f64 args drew from the f32 pool** — `generate_args` fed f64 slots through
`_draw_f32`: no DBL_MAX, no DBL_MIN, no double denormal, no 2⁵³ boundary. New
`_draw_f64` with a dedicated pool + log-uniform magnitude sweep.

3.2 **u64 args never exceeded 32 bits** — `getrandbits(32)` only. Now mixed
64-bit draws plus a handle/sentinel edge pool (0, 1, 0xFFFFFFFF,
0x1_0000_0000, 0x7FFF…, 0x8000…, all-ones).

3.3 **The pair sweep excluded NaN and inf** — the two-f32 "wrap shape" sweep
used `F32_EDGE_POOL[:8]` (zeros and small floats only) — the exact (NaN, x)
and (inf, x) pairs that killed the original minss/maxss bug class were never
generated. New 12-value pool including the IEEE specials (144 pairs).

3.4 **Hex gates had no boundary mutants** — the boundary mutator only matched
decimal literals, so `count > 0x3f` never produced a ±1 mutant. Hex literals
now mutate too (guarded against `<<`).

3.5 **Mutation corpus is the front 48 cases by design** (factories pin
discriminating cases there) — unchanged; but the mutation stage now reuses
cached oracle results instead of re-emulating.

---

## 4. Speed

Measured (`scripts/bench_oracle.py`, this repo vs improved, loop summing 4096
pointee bytes — a memory-access-heavy shape):

```
old: 11.98 ms/call   (first-call ret=0x92, 41st-call ret=0x62, stable=False)
new:  0.36 ms/call   (first-call ret=0x92, 41st-call ret=0x92, stable=True)
speedup: 32.9x
```

Where the time went:

| Change | Effect |
|---|---|
| No per-instruction Python `UC_HOOK_CODE` in the oracle; native `count` + clean-return RIP check | removes the biggest Unicorn cost; also fixes 1.3 |
| Range-limited mem hooks (read/write hooks scoped to the image range only) | arena/stack traffic — the overwhelming majority — stays native |
| Coverage measured *on* the stage-3 runs (tracker installed around the differential) instead of re-running the whole corpus | ~2× fewer oracle executions per unit |
| Oracle result cache keyed by input identity (sound since stage 2 + 1.6 make the oracle stateless) | mutation stage no longer re-emulates 48 cases × 12 mutants |
| Mutant builds run concurrently (ThreadPool over gcc/rustc; differential stays serial — one emulator) | mutation wall time ≈ max(compile) + Σ(exec) |
| Image bytes cached per path | selftest used to re-read the ~72 MB image once per injected defect (11+ times) |
| Misc | fixture edge list hoisted out of `make_args`; `bytes(buf)` copy per reloc dropped |

Not yet done (ideas, in order of value): subprocess-isolated C candidates
(also fixes 1.7 and enables parallel candidates); `uc_emu_start(timeout=…)`
as a wall-clock backstop; a `--jobs` flag to run units in parallel processes.

---

## 5. Architecture notes

- `runner.run_unit` is still one long function; the duplicated mem-compare
  logic is now one helper (`_mem_channel`, with a first-differing-byte offset
  in divergence reports instead of two opaque hex blobs), but extracting the
  five stages into small functions returning `(status, payload)` would make
  the ladder independently testable.
- Coverage sweep bounds now come from the image's `.pdata`
  (`_pdata_function_bounds`, file offset == RVA layout) when available, with
  the honest int3/ceiling heuristic as fallback — some captures have no
  .pdata, and those degrade gracefully. `load_function`'s SHA still covers the
  fixed 0x2000 window (neighbouring bytes affect the digest); binding to the
  .pdata end would make re-verification less trigger-happy, at the cost of
  changing the digest again.
- `cli.py` gained a `list` command that also validates descriptors.
- `tests/` is new and additive; the smoke tests run anywhere Unicorn runs and
  are the regression net for everything above.

## 6. Compatibility

- Ledger: `HARNESS_VERSION` bump ⇒ every existing entry reports stale once;
  re-run `cli.py all` to re-bind. The `surfaces_sha256` algorithm also changed
  (fixture + shim bound), which has the same one-time effect.
- Corpus: f64/u64/pair-sweep changes alter generated inputs for the same
  (seed, i) — intentional, that is the detection-power fix.
- Nothing in `units/*.json` or the two fixtures' semantics changed;
  `AGENT-PROTOCOL.md` needs no edits, though the ledger version field is worth
  a line under hard rule 5.

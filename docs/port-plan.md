# The port plan

How the gameplay layer gets rewritten function by function, what it costs, and
how each function is proved. Every number below is printed by a command, not
estimated: re-run the command and the number moves or the check fails.

## What the scope is, measured

`python dev/corpus.py triage` — the forward closure of the 238 named anchors
that carry a gameplay tag in `docs/function-names.csv` (`mover`, `domino`,
`map`, `camera`, `anim`, `hero`, `input`, `statescript`, `model`) over the call
graph from `dev/corpus.py index`:

| | |
|---|---|
| gameplay functions | **3,864** (3.2% of the image's 119,802) |
| code | 2,301,019 bytes |
| harness units needed | **1,541** (238 entries + 1,303 shared functions) |
| proved inside their caller's unit | 2,312 (exactly one caller in the scope) |
| import thunks | 6 |
| trivial hot leaves | 5 |

By system (a function reached from several systems is counted in each):
`camera` 2,391, `map` 1,457, `mover` 1,140, `domino` 915, `input` 462,
`anim` 256, `statescript` 229, `model` 215, `hero` 4.

Rendering (`d3d11`, `texture`, `material`), the data layer (`casc`, `stu`,
`model` assets) and the engine (`engine`, `reflection`) are outside this scope
by the project's own split; they have their own crates and their own evidence.

## The five states

Each function lands in exactly one, in this order of application:

| state | test | what happens |
|---|---|---|
| `entry` | a named gameplay anchor | gets its own harness unit |
| `thunk` | body is one `FF 15`/`FF 25` call and a return | port as a call to the crate that already does it; the caller's unit proves it |
| `trivial` | leaf, ≤ 64 B, ≥ 100 call sites | port directly, no unit of its own |
| `unit` | ≥ 2 caller functions inside the scope | gets its own harness unit |
| `inline` | exactly 1 caller inside the scope | port inside the caller's unit; coverage proves it ran |

There is no `delegate` state: telling a CRT/STL primitive from a client accessor
needs a library name table, which this repo does not have: the function table it
keeps (`dev/data/textfuncs.csv`) carries only `FUN_*` names, and the Ghidra
program's 1,679 names are the project's own plus 24 Function ID matches. There is no `open` state: the scope is a
forward closure, so every in-scope function has a caller by construction; a
function reached only through a slot or a table is *outside* the scope, not open
inside it. Those are the 33,684 `.pdata`-less call targets of
`docs/ghidra-image-audit.md` section 8.8, and `dev/corpus.py` records them as
edge targets with no function object rather than pretending they are functions.

## What a unit is

`dev/verify_harness` runs the client's own bytes for one function under Unicorn
and diffs them against the Rust candidate, then runs 12 mutants of the candidate
and requires a kill rate. A unit is a JSON (`dev/verify_harness/units/`) plus the
Rust body. The image is pinned by sha256 (`emu_oracle.IMAGE_SHA256`); a unit
proved against different bytes is not proved.

A unit is written for an **entry point**, and its helpers with one caller come
along inside it: the entry's corpus drives the whole cluster, and branch
coverage records that the helpers ran. That is why 3,864 functions need 1,541
units rather than 3,864. A helper with two or more callers gets its own unit
because it has an input domain of its own.

## Signatures are inferred, not invented

`python dev/corpus.py proto` — the callee's own reads in its first 512 bytes:
which of `rcx/rdx/r8/r9` (and `xmm0..3`) are read before being written, with
32-bit aliases folded (`mov eax, ecx` uses `rcx`). The result is a *candidate*
stored in the gitignored index with the call-site count as its weight; it is
never written into `docs/function-names.csv`.

Measured against the 44 units that already state an argument list, the integer
count agrees for 28. The misses have three known causes and none of them is a
bug to chase: arguments beyond the fourth are on the stack and are not inferred;
some units pass a context struct instead of the client's own arguments; and a
function that reads an argument only on a late branch is missed. The inference is
therefore a reading aid with a confidence weight, not a prototype.

Prototypes become facts only through §8.5 of `docs/ghidra-image-audit.md`: the
`prototype` column of `docs/function-names.csv`, applied by
`dev/gh_apply_types.py`, argued from the call sites that agree and from the body.

## Definition of done, per system

A system is done when every function in its scope is one of:

- a verified unit in `dev/verify_harness/units` with its case count, coverage and
  mutant kill rate recorded in its `docs/formats/<system>.md` page;
- a function covered by such a unit (an `inline` helper, a `trivial` leaf, a
  `thunk` whose crate call is exercised);
- explicitly declared unverifiable with the reason.

The progress number is `verified units × functions covered × mutant kill rate`,
per system, in `docs/systems-coverage.md`. It is never "percent of 119,802
functions", because 96.8% of the image is other people's code: CRT, STL,
compiler helpers, middleware, rendering.

## S2: the readable decompile project

The evidence project (`dev/ghidra_run/owmod_clean`) is never mutated by this
work. Readable decompiles come from a copy whose function bodies have been
widened to their `.pdata` chain union, because ownership has to be right before
anything is read or swept. Note what the merge does *not* do: Ghidra's
decompiler follows flow outside the body unless another function's entry blocks
it, so the merge changes the C text only where a competing object sat on the
cold part (320 objects) or a false `noreturn` pruned it - measured, see
`docs/ghidra-image-audit.md` section 8.2.

`docs/ghidra/MergeChainBodies.java` does the widening (dry run first).
**Applied 2026-11-08**: `C:/Temp/owread` is merged, audited and saved; the
numbers are in `docs/ghidra-image-audit.md` section 8.2. The commands that
actually work, in order (Ghidra closed; run from PowerShell or with
`MSYS_NO_PATHCONV=1`, because bash rewrites `/E` into a path):

```powershell
robocopy E:\overwatch-client-rs\dev\ghidra_run C:\Temp\owread /E
# every analyzeHeadless call needs -process: without it currentProgram is null
# and the script runs against nothing (measured: NullPointerException).
analyzeHeadless.bat C:\Temp\owread owmod_clean -process Overwatch.exe `
  -noanalysis -readOnly -scriptPath C:\Users\Happy\ghidra_scripts `
  -postScript MergeChainBodies.java dry
```

The dry run prints `roots=25649 chained_entries=81444 unresolvable=0
contiguous=25649 not_contiguous=0 no_function=11 already=53 outer_overlap=64
would_widen=25521 victims_removed=320 failed=0 body_bytes=3222452->18610692`.
`outer_overlap` are roots with a *foreign* body reaching in; they are skipped
and listed with `OUTER <root>: ... e.g. <entry>`. Then apply, snapshotting
before and after in the same run (the merge is in-memory until the save):

```powershell
analyzeHeadless.bat C:\Temp\owread owmod_clean -process Overwatch.exe `
  -noanalysis -scriptPath C:\Users\Happy\ghidra_scripts `
  -postScript BodySnapshot.java before `
  -postScript MergeChainBodies.java apply `
  -postScript BodySnapshot.java after
```

Expected: `functions 120493 -> 120173`, `widened=25533`, `victims_removed=320`,
`body_bytes 29,259,101 -> 44,422,830`, `Save succeeded`. Then, on the copy and
in this order:

```powershell
analyzeHeadless.bat C:\Temp\owread owmod_clean -process Overwatch.exe `
  -noanalysis -scriptPath C:\Users\Happy\ghidra_scripts `
  -postScript ClearFalseNoReturn.java clear
```

Then the second merge pass. `FixOuterBodies.java` resizes the function bodies
that leak into a root's union (16 of them, all real `.pdata` roots whose body
had outgrown its own extent); re-run the merge afterwards and it widens the rest:

```powershell
analyzeHeadless.bat C:\Temp\owread owmod_clean -process Overwatch.exe `
  -noanalysis -scriptPath C:\Users\Happy\ghidra_scripts `
  -postScript FixOuterBodies.java apply
analyzeHeadless.bat C:\Temp\owread owmod_clean -process Overwatch.exe `
  -noanalysis -scriptPath C:\Users\Happy\ghidra_scripts `
  -postScript MergeChainBodies.java apply
analyzeHeadless.bat C:\Temp\owread owmod_clean -process Overwatch.exe `
  -noanalysis -readOnly -scriptPath C:\Users\Happy\ghidra_scripts `
  -postScript ChainConflicts.java
```

Pass 2 widens 43 more; `ImageAudit` then reports `in_body=25629`,
`body_flows_into_chain=9`, `named_flowed=0`. The 9 are open on purpose: every
remaining conflict is a function object with no `.pdata` extent (`ChainConflicts`
lists them), and clipping them was measured to leave 11-12 byte bodies - a
guess, not a repair.

`MergeChainBodies.java revert` then applies the seam rule retroactively: 41 roots
whose entry never enters a fragment went back to their own extent, giving
`in_body=25588`, and a dry run afterwards reports `no_seam=41 would_widen=0`, so
the state is stable.

The `ClearFalseNoReturn` step is **required for reading**, not cosmetic: with the 110 false
`noreturn` flags in place the decompiler prunes cold bodies as dead code even
though the widened function hands them to it in full. Measured on the mover:
175 lines and no reference into the cold part before, 315 lines and 13 references
after; `0x140c8ea20` goes from 28 lines to 2,586.

If a script change is not picked up (`ClassNotFoundException`), restart Ghidra:
its script class cache is not always invalidated. `ProbeFlow.java <addr>` shows
why a decompile stops (body ranges, last instructions, out-of-body targets) and
`DecompScope.java <addr>` shows what body the decompiler was actually handed -
their agreement is the check that the map is right and the text is wrong.

The step-by-step procedure for the whole surface - picking a unit, reading it
from the merged project, transcribing, wiring the probe, building the fixture,
declaring the unit, reading the ladder, triaging a failure, closing a cluster,
and the checklists - is **`docs/port-playbook.md`**. This page holds the scope
and the file layout; the playbook holds the method.

## S3: trace units (built 2026-11-20, pilot green)

The plan above spends one unit per function: 1,551 of them, of which **1,303 are
`unit`-state functions with two or more callers in scope** (`corpus.py triage`).
At the measured rate - 1 to 9 units a day, with one 51-unit day when a *family*
was done - that is a year. The family day is the tell: a repeated shape is
cheap. A trace unit is the same idea applied to control flow.

**What it is.** One unit runs an *entry point* against the client's bytes for N
sequential steps, with a scripted input sequence, and compares the declared
state region after **every** step. One unit then proves the driver and every
leaf it calls, on the step where each leaf is actually reached. AGENTS.md
already admits this evidence class: *first-divergence for a trace*. A leaf that
was invented rather than read diverges on the step that touches it, and the
coverage stage still reports which branches the trace reached.

**What is built.** The mechanism is in the harness and the pilot is green:
`fn/ShapePoolAlloc-steps` calls the client's `ShapePoolAlloc` 1-4 times in a row
on one pool inside a state region - 16 chained pops over 8 pools, 80 corpus
cases, every channel compared after every call, **6 of 6 mutants killed** - in
the same wall clock as a single-call unit (differential 0.2 s for 160 client
calls, mutation ~8 s, one unit).

**The number that shapes the plan.** For that leaf the steps unit is **not
stronger** than the single-call one, because a single-call unit already compares
the whole mutation of one call. `steps` pays only where a call's writes feed the
**next call's control flow** - a driver. Trace units are for drivers, not for
leaves, and the 1,303 shared functions are only worth tracing where a driver
reaches them.

**How it works.**

1. `steps: true` in the unit; the fixture module exports
   `make_steps(i) -> list[list[ArgSpec]]` instead of `make_args(i)`. Each case
   is a list of steps.
2. The oracle keeps its state region between the steps of a case
   (`EmuOracle.persist_state`) and re-seeds it from the snapshot at the start of
   each case. One emulator serves every case: building one costs 12x the calls
   it then runs (measured, the differential stage went from 1.2 s to 0.2 s).
3. The candidate probe keeps its state buffer between calls too, and the harness
   sends a `reset` command before each case (`RustCandidate.reset_state`, and
   the `reset` command in `verify_probe.rs`).
4. The region is addressed by **guest** addresses: a `u64` argument carries a
   guest address and the probe slices its own state buffer there, so the data
   survives the call. A `ptr` argument cannot be used: its pointee bytes are
   re-sent by the wire on every call, which re-seeds them.
5. The mutation stage replays the first case's steps in order on one mutant
   process, so a mutant is killed by the step where it first diverges.

**The pilot.** `fn/ShapePoolAlloc-steps`: the pool record and its entries array
live in the state snapshot, reached through `shape_pool_alloc_guest` in
`crates/ow-tank/src/bin/verify_probe.rs`; the fixture is
`dev/verify_harness/shape_pool_alloc_steps_fixture.py` and prints the snapshot
hex the unit carries. Cases cover chain lengths 1-4, a non-contiguous `2 -> 0`
chain, a saturated used count and a high-water mark that must not fall. The last
pop of a case leaves the free head at `-1`; the call after that is the client's
allocator path, which cannot run offline, so no case goes past the end of its
chain. `formats/domino.md`, "the fill path".

**Pilot two, a real driver (`fn/ApplyCharacterControllerMove-steps`).** The
client's `ApplyCharacterControllerMove` (`0x142cd5370`) called three times in a
row on one character in the state region, with a different velocity each step,
compared after every call - 200 cases, coverage 100% of the compared window
(9/9 branches), **10/10 mutants killed**. It found that the **harness** was
wrong, not the port: the ow-sim entry built its working copy from
`state.snapshot`, so every call started from the snapshot and the client's own
1-ULP double normalization was invisible. The port needed no change.

**Two harness bugs that pilot found, both fixed.**

1. The probe re-seeded its state region *inside the entry* (`state.snapshot`
   instead of the live `state.buf`), so a steps unit's later calls silently saw
   the snapshot. The probe now holds one live buffer, re-seeds it before every
   call unless the unit asks otherwise (`persist on`, which the runner sends for
   a steps unit), and the ow-tank probe follows the same rule - a single-call
   unit must answer from the snapshot every time, because the runner caches
   answers by args. This is why the first run looked like a mover bug: the
   candidate normalized the original quaternion three times while the client
   normalized its own output.
2. The mutation corpus for a steps unit was **case 0 only**, so a mutant that
   only shows on a boundary handle or a NaN payload survived: measured 50%
   (5/10), then 100% (10/10) once the kill loop replays every case and resets the
   mutant's state at each case's first step. A steps unit has far fewer cases
   than a single-call unit, so "case 0 carries the corpus" was silently too
   weak.

**Next.** The mover driver (`fn/MoverUpdateFrame-trace`, 30 frames on a
controlled mover state) needs one more piece: the client's driver calls through
vtables and a world, so the state region has to reach the **arena**, not only
the snapshot - guest translation for `ptr` arguments, which the state-mode pilot
names as missing (`crates/ow-sim/src/bin/verify_probe.rs:1860`). Section S4
measures what that is worth before promising a number for it.

**What it does not do.** It does not make reading optional: a trace proves
equivalence, not that the Rust came from the client's expression. A leaf per
unit still has to be read - but once, inside the driver's read, with its
contract visible at the call site.

## S4: what one unit actually buys (measured 2026-11-20)

S3 argued the shape from the client's graph. This section measures the yield, so
the plan is chosen by numbers instead of by the shape's plausibility. Two tools,
both read-only and reproducible:

```sh
python dev/throughput.py                # static closure per unit, coverage of the scope
python dev/reach.py <unit>... --cases 6 # functions a unit's corpus EXECUTES
```

### The static closure is fiction; execution is the number

`dev/throughput.py` reports the transitive callee closure of each unit's entry.
Summed over 114 verified units it reads as **817 of 3,875 scope functions
(21.1%)**, and the top entries look enormous - `fn/ApplyMoverSettings` 491
functions, 278 KB. `dev/reach.py` runs the unit's own corpus under
`UC_HOOK_CODE` and folds every executed instruction up to its chain root:

| unit | static closure | **executed** | bytes executed |
|---|---|---|---|
| `fn/ApplyMoverSettings` | 491 | **2** | 995 |
| `fn/StateTransform` | 361 | **2** | 3,201 |
| `fn/MoverResolveVector` | 305 | **3** | 2,800 |
| `fn/QueryDriver` | - | **8** | 5,573 |
| `fn/MoverWishSpring` | 32 | **8** | 2,278 |
| `fn/ApplyCharacterControllerMove-steps` | 23 | **3** | 700 |
| `fn/ApplyCharacterControllerMove` | 23 | **3** | 700 |
| `fn/MapChunkCellBytesFixup` | 33 | **1** | 567 |

The 14 units with the largest static closures execute **31 distinct functions
between them** (union, 15.9 KB). The closure counts everything a *branch could
lead to*; a controlled call walks one path through it.

### Why: this graph is dispatch, not calls

| measure (3,875 functions in scope) | value |
|---|---|
| functions with any memory-indirect dispatch (`slot_sites`) | **510** |
| distinct slots per dispatching function | 1 slot: 362, 2-3: 121, 4-7: 24, **8+: 3** |
| largest dispatch fan-out in the scope | **12 slots** (`0x1421482d0`), then 8 (`0x140cbb260` `MoverUpdateFrame`, 14 sites), 8, 7, 6... |
| functions with >= 1 direct in-scope callee | 2,973 (1 callee 1,127; 2: 662; 3: 370; >= 8: 235) |

So the leaves a unit *would* cover sit behind slots whose targets are set at
runtime from a world the unit does not have. That is why `ApplyMoverSettings`
reaches 2 functions and not 491, and why the ceiling on a *perfect* driver trace
is about 10-30 functions, not thousands: `MoverUpdateFrame` has 8 distinct
slots, and only three functions in the whole scope have 8 or more.

### The harness is not the bottleneck

| measure | value |
|---|---|
| fresh verification of 6 units (differential + coverage + mutation) | **90 s wall** -> 15 s/unit |
| per unit: batch build of mutants / mutation stage / differential | ~12 s / ~12 s / 0.2 s |
| all 3,875 scope functions at 15 s/unit | **~16 h of machine time** |
| a 9-unit day (the best measured day) | ~2.3 min of machine time |
| a unit's fixture, the part a human writes | median **116 lines** |

The harness is ~0.5% of a working day. What costs the day is the *read* (the
client's expression, operation for operation) and the fixture that pins its
inputs. That is what `python dev/fn.py packet <addr>` attacks (one command: the
function, its pieces, switch tables, immediates, data refs, callers, callees and
the units that already cite it), and it is why the plan's own measured rate was
1-9 units a day and not 1-9 an hour.

### Where the verified units actually are

| | units |
|---|---|
| verified units in the ledger | 115 |
| entry inside the gameplay scope | **51** |
| entry outside it (formats, crypto, asset plumbing) | 63 |
| in-scope functions touched by those 51 (lower bound: entry + direct callees) | **70** (1.8%) |

Most of the verification effort so far bought data-path behaviour, not gameplay
functions. That is the honest number for "how much gameplay is proven": 1.8% by
the lower bound, ~2-4% by the executed sample, not 21%.

### What this changes

1. **Queue by scope, not by convenience.** A format unit does not move the
gameplay number; the 51/115 split is the cost of that already having happened.
2. **Trace units pay on drivers only, and modestly.** Measured: the same leaf as
   a 3-call trace executes the same 3 functions as its single-call twin. The gain
   exists only where a slot is populated - which needs the world.
3. **The world is the highest-leverage missing piece** because it is what turns
   8 dispatch slots into 8+ executed functions per unit, on every driver at
   once. Below it, every unit stays a 1-4 function unit.
4. **Do not optimize the harness further.** 16 h total for the whole scope;
   the read is the cost.

Side note on the shape: a unit is not a function's *only* proof. 2,313 of the
3,875 are `inline` - they have one caller, so a unit on the caller covers them
without a unit of their own. That is the one place the 1,551-unit plan
over-counted, and it is measurable: `python dev/corpus.py triage` prints the
split (`entry 248`, `unit 1,303`, `inline 2,313`).

## Worked example: one unit, end to end

`fn/FloatToQ12` (`0x141c6c830`), done 2026-11-17. Picked from `triage` because it
is `unit` state, 145 bytes over 3 pieces, 53 call sites, no callees and no global
reads. The seven steps are the whole procedure; each produces one file.

| # | step | artifact |
|---|---|---|
| 1 | read the body | `python dev/corpus.py show 0x141c6c830` (asm; the `.c` with `show --asm` once the range is built) |
| 2 | transcribe it literally | `crates/ow-sim/src/fixed.rs`: `scale_q12` + `quantize_q12`, plus two unit tests |
| 3 | register the probe arm | `crates/ow-sim/src/bin/verify_probe.rs`, `"float_to_q12" => float_to_q12_entry(args)` |
| 4 | bind the ledger digest | `dev/verify_harness/candidate_rust.py`: `ENTRY_SOURCES` and `ENTRY_MUTATION` (`scale_q12`) |
| 5 | declare the unit | `dev/verify_harness/units/fn-FloatToQ12.json` (v2b address, args, `returns: void`, options) |
| 6 | give it an input domain | `dev/verify_harness/q12_fixture.py`: `make_args(i)` with `count` tied to the buffer length |
| 7 | run the ladder | `python dev/verify_harness/cli.py fn/FloatToQ12` |

Result: **PASS** - 364 cases, every channel; coverage 5/5; mutation 11/12 with
the survivor declared observationally equivalent (`>=` vs `>` on
`exponent >= 138`: at exponent 138 both arms shift by zero).

Two things only the ladder could have found:

- the fixture first missed the `count == 0` early exit, so coverage sat at 4/5
  and the unit failed at stage 4 until a zero-count case was declared;
- the transcription was right but the note above it claimed negative saturation
  reaches `i32::MIN`. It reaches `-i32::MAX`: the sign is applied to the
  saturated magnitude, so the extra `-1` is never produced. The differential
  passed; the prose was wrong and was corrected.

Size of one unit, for planning: ~50 lines of Rust, ~35 of fixture, ~25 of JSON,
~15 of probe. `docs/function-names.csv` gained one row (`ScaleFloatRunToQ12`,
tag `runtime`, with a prototype) - a name is added when the role is settled, and
the verified unit is what settles it.

The remaining 1,540 units are picked by `docs/open-questions.md` order, not by
address: a system is ported when the path that needs it is being read.

## What is not part of the plan

- A parallel tree of transcribed-but-unproven Rust. A body without a unit is a
  claim that cannot fail, so it is a `FIT-*` stand-in in a new costume
  (`AGENTS.md`, rule 12). Drafts live in the gitignored corpus; a function lands
  in the crate that owns its system together with its unit.
- Porting by address order. The queue is `docs/open-questions.md`: a system is
  ported when the path that needs it is being read, because that is when its
  integration test can be written.
- Porting the runtime. 7,640 `FF 15`/`FF 25` edges (7,134 calls into the import
  table, 506 thunks) and the 6 gameplay thunks around them are calls into
  Windows; they are delegated, and the unit that calls them proves the
  composition.

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

# Open questions: the work queue

Every open question, in the order worth attacking. An entry is closed when the
answer has an address or a value, the finding is in the relevant
`docs/formats/<system>.md`, and the code that uses it has a test.

The rule behind the queue: a question is never closed by a guess. If it is not
read, it stays here and the gameplay feature stays unimplemented.

---

## Chain fragments whose seam the parent never reaches - answered 2026-11-08

Closed by measurement, not by argument. Walking the CFG from each entry over the
function's own flow inside its `.pdata` union: 25,597 of 25,638 roots enter
fragment bytes, and only **41** never do (none named). The 1,615 figure in the
audit was "last instruction does not transfer" - an upper bound, since an earlier
branch usually enters the fragment. The 41 were reverted to their real extent by
`MergeChainBodies.java revert` and are in `C:/Temp/owaudit/seam_unreached.txt`.
The merge now carries the check itself (`NO-SEAM` gate), so a re-run cannot
re-widen them.

## Before anything else: the image repair backlog (2026-11-04)

**Status: the no-risk repairs are done in the live project (saved, from a
restorable copy at `C:/Temp/ghidra_snapshot/owmod_clean.rep`). Numbers, evidence
and the repair procedure are in
[`ghidra-image-audit.md`](ghidra-image-audit.md), section 11.**

Done. No `.pdata` extent, chain count or function count changed; the one thing
that did move was the over-reach of three bodies (§11, same cause unresolved):

- **110 of the 179 `noreturn` flags are cleared** - the flagged function's own
  body contains a `RET`, so it returns on that path. They blinded 11,552 call
  sites in 6,355 functions; the program now reports 69 flags and 28,412 call
  sites. Restorable with `ClearFalseNoReturn.java restore`.
- **The analyzers that caused it are off**: `Non-Returning Functions - Known`,
  `PDB Universal` (no symbol file ships with the install) and `... Discovered.
  Repair Flow Damage`.
- **The function-creation paths are closed.** `RecoverPointerFunctions.java`
  now creates a function only at a real `.pdata` start (measured: it would have
  created up to 72,786, now 4), and `dev/gh_apply_names.py` renames only - it
  was calling `disassemble_bytes` plus `create_function` when a rename failed,
  which is how chain-start and mid-body "functions" appeared. The 38 real
  starts that have no object are repaired by the same guarded script, not by
  the analyzer.
- **The 10 malformed `function-names.csv` rows are re-quoted** and four rows that
  named a chained continuation now carry `container=<real start>`, which
  `dev/frame_check.py` rule 4 re-verifies against the unwind records
  (`docs/function-names.csv`, the front matter explains the column).
- **Still open:** the merge of 25,184 chained fragments, the map repair (283
  chain-start objects, 38 missing starts, 251 multi-range bodies), the 118,578
  body/extent mismatches, data typing, prototypes, 47 shared names and the 79
  RTTI names with no pointer. The merge is not "low risk": every body it
  changes must be diffed, so it starts with `BodySnapshot.java`.
- **Function-less code targets stay open** (33,684 of them, 382 with ten or more
  references, worst 4,225): they have no `.pdata` entry, so a body would be an
  inference. This is the same rule as the one that bit the project before -
  more "functions" is not a better image.

`ImageWorklist.java` prints all of these lists in one run.

No gameplay read should be trusted from a function whose body stops at a
chained continuation: the decompile is truncated at a boundary the client
itself declares. That is why the merge is the next repair and not a cosmetic
one.

---

## The gate's wall time: seven install suites are 98% of it (2026-11-19)

Measured on the operator's machine (12 cores) with `cargo xtask verify`, the
sequential `cargo test --release --workspace` run: of the 56 binaries whose
timings parse out of the log, **2246 s of test time** and these seven suites
account for 2218 s of it:

| suite | wall |
|---|---:|
| `ow-casc` `index_find_real` | 935.2 s |
| `ow-scene` `skin_pose_gate` | 428.3 s |
| `ow-tank` `effect_real` | 222.8 s |
| `ow-scene` `practice_launch` | 185.5 s |
| `ow-scene` `hero_draws` | 158.6 s |
| `ow-tank` `hero_pick_real` | 151.9 s |
| `ow-tank` `brush_real` | 134.5 s |

Everything else is under 16 s per binary. The full sequential run passed 84
minutes of wall clock (compile + fmt + clippy + the suites) before it was
stopped, so the operator's "verification takes longer than the work" is
arithmetic: a unit is ~12 minutes, the gate is over an hour.

The pool is now in: `crates/xtask/src/json.rs` + `test_runner.rs` +
`main.rs` (applied from `xtask-verify-speedup.patch`) build every binary once
and run them `--test-jobs N` at a time (default `min(4, cpus)`, or
`XTASK_TEST_JOBS`), select artifacts by `profile.test` (not the target's
`test` flag, which is true for every testable bin and once made plain
example/bin `main`s run instead of harnesses), print each binary's duration,
and run doc tests separately. Same stages, same binary sets, same verdict. The
pool pays the **max** of the suites instead of their sum, so the gate's floor
becomes `index_find_real`'s 935 s. It does **not** reduce CPU-seconds - it
still uses every core while those minutes run - so the work-reduction items
below remain the real fix.

Three ways forward, in order of what actually removes work:

1. **Make the seven suites cheap.** `index_find_real` re-derives a binary
   search for every one of ~10M entries; the same property can be proven from
   the sortedness walk plus a deterministic sample of keys (boundaries,
   duplicates, first/last per bucket). The other six re-scan install trees per
   run; a shared, gitignored, install-keyed cache of the derived facts would
   collapse them to seconds.
2. **Split the gate by cost.** Done as scoping, not as a second gate:
   `cargo xtask verify -p <crate>` runs clippy, the tests and the doc tests for
   the named crates only, and `cargo xtask verify --skip <name>` (repeatable)
   drops a test binary whose label matches (`--skip index_find_real` removes
   the 935 s floor; measured: the `ow-casc` scope goes from >19 min to 20 s)
   and otherwise hands libtest's `--skip` to every binary that still runs. A
   scoped run is an iteration loop, **not** the gate: the definition of done is
   still the unscoped `cargo xtask verify`, because the crates you did not name
   are exactly where an unexpected break hides. The four python doc checks stay
   workspace-wide either way - they answer in seconds.
3. ~~Parallel binaries~~ **done** (the pool above). Wall-time win only; no
   CPU saved. Once (1) lands the pool makes the gate a few minutes.

## 0. The engine object and its subsystem schedule

**Status: the object, both constructors, its interface registry and the
subsystem schedule are read and shipped in `crates/ow-engine`; the creation
step's five callees, the meaning of the 65 subsystem rows and the 20 dropped
constructions are open.**

Read (`0x470` bytes, published in `[0x143b7f410]` by `0x140585220`):

- `WorldBaseCtor` `0x141c507c0` and `WorldCtor` `0x1405838c0`: every field
  offset and every written value, including the two codec-encoded seeds, the
  placeholder `INTERFACE_ARRAY(0)` (`0x4daa5a973c0808b6`) at `+0x058`,
  `+0x1f4 = 0.25`, `+0x1f0 = 0x10` and the 71-slot table at `+0x228`.
- `+0x008` is `(the constructor's **fifth** argument == 0)`: entry spills `r9d`
  into the fifth argument's home slot and compares that. The sixth argument
  lands at `+0x178`.
- `+0x018` and `+0x058` are written twice as well. `+0x018` ends as an encoded
  heap pointer (`0x18317a531d116c0c` family, `World::encode_q_18`); `+0x058`
  ends as `((r8 ^ 0x238e31e8cd062f58) ror 0x31) + 0x34b5f414245bf6ef`
  (`World::encode_q_58`). The constants the constructor writes first are
  placeholders, like `+0x460`'s.
- `+0x460` is written twice: first as the address codec at 0, then as the same
  codec applied to the address of a `0x1b8`-byte object the constructor builds
  (`0x140be33c0`) and drops. The field is therefore an encoded heap address.
- `+0x468` copies the u32 at `0x1441880e8` (zero-fill), and `0x140585220` then
  overwrites it with `[0x141c51cc0(...)+0x138]`.
- The codec helpers `0x140543a40` (`EncodeSeed38`) and `0x1405674e0`
  (`EncodeSeedB0`) were created as functions in the Ghidra project; they are
  called directly from `0x141c507c0` but had no boundary.
- `RegisterWorldInterface` `0x141c52d10` (inline codec), the array growth
  `0x140767da0` (half-again, borrowed-tag in capacity bit 31), the index-map
  builder `0x141c553a0` and the insert `0x141c56fb0` (which also stores the
  encoded engine back pointer at `subsystem+0x008`).
- `ConstructEngineSubsystems` `0x140554220`: its 65-row schedule and the
  21-entry tail attach list are a per-row decode of the instruction stream
  (`dev/eng_verify_schedule.py`, reconciled on 1531 instructions, 80 `Alloc`,
  84 registrations, 65 stores and the five empty offsets), modelled in
  `ow_engine::subsystems` with `tests/subsystem_schedule.rs`; `World` refuses a
  slot the client never writes. The bytes agree with the Ghidra project at the
  sites sampled.
- The **35 constructors and their 96 vtable installs** (`dev/eng_class_profile.py`
  -> `ow_engine::classes`, `tests/class_layout.rs`): 70 constructor-installed
  tables + 47 block-stored tables, disjoint (117). All 31 stub slots have a
  block store that overwrites the stub's `0x142f7d778`, so they are derived
  classes sharing one base constructor; the object's `+0x00` table is
  block-stored in 32 slots, constructor-installed in 30, unobservable in 3
  (`+0x220`, `+0x340`, `+0x358`); `+0x2f0` is an aggregation of six sub-objects
  and `+0x308` the one non-stub slot the block re-points. A table's length is
  only read where both ends are known: 44 of 117 are exactly 15 entries, two
  stop at the read cap and are open.

Open, in the order they block progress:

1. **What each subsystem *is*.** The producers and vtables are read (35
   constructors, 117 tables) but not one class is named. The unit to name is a
   slot's own **15-entry table** (44 of the 117 are exactly 15 entries; a
   longer read has run into the neighbouring tables of the bank). A name needs
   a method inside those 15 entries that calls a known subsystem API, or a
   consumer that identifies the subsystem. Four tables contain a curated name
   and none is inside a slot's first 15 entries, so the four are leads, not
   answers. Also start with the three objects whose `+0x00` table is not
   observable in the schedule (`+0x220` decode, `+0x340`/`+0x358` factories)
   and with the `+0x448` adjustor, whose `0x68`-byte object is built inline.
2. The 20 registrations whose objects never reach a slot, including the
   `Alloc(0x980)` object that follows `+0x220` and the anti-tamper block's
   garbage interface.
2. `0x140585220`'s unread callees: `0x141c57c40` (the constructor's second
   argument), `0x141c52fc0` (runs right after publication),
   `0x141b33e20`/`0x141c61a80` (a stack "debug name" pair staging `0x6f`),
   `0x141c51cc0` (the lookup whose `+0x138` becomes `+0x468`), `0x141c56970`.
3. The pointer→index map at `+0x198` is not modelled, only its effect on the
   array's growth.
4. `+0x0e8..+0x1c8` and the other unwritten ranges are assumed zero because
   `Alloc`'s flag-0 path zeroes; not yet proven by a read for this call site.

Where: `docs/formats/engine.md`, `crates/ow-engine/`.

## 1a. The fan's per-candidate refinement block

**Status: the block is read and implemented; the outer walk is open.**

Read and shipped (`ow-sim::fan`, six new units):

- the back-transform direction (`0x140cb9580`) is `-descriptor.xyz` normalized
  **in the horizontal plane** (`len2 = z*z + x*x`, one `rsqrtps` plus one Newton
  step, the `y` lane kept as `n*0.5*0.0`), `w` from `-descriptor.w`, and
  `(0,0,1,0)` when `len2 < 1.17549435e-36` or the approximation's sign test
  fails;
- its `-normal` sibling does the same to the accepted hit's normal and falls
  back to the first direction when it is degenerate;
- the offset is `pentagon*0.02 + refined*0.005` per lane, and two endpoints are
  built from the centre: the raised one and the ground probe `0.04` below;
- the acceptance is `flag_bit_0x10000 || facing > best_y`, with `best_y` = the
  earlier pass's `local_c18` captured before the loop and never updated, and the
  **first** hit kept even when it fails the facing test.

**The outer walk is read and implemented (2026-11-02).** The writer's four
call sites, the query each carries and the `+0x1140` write are read, and
`ow-sim::fan::solve` runs that composition (gate `body+0x659`, both endpoints,
the facing threshold, the memo test, the ring → refine → back-transform order
and both reflection bytes). What remains behind the `CandidateSolver` seam is
not the loop but two inputs: pass 0's `MoverWishQueryCached`, and the shape-ring
branch behind the global toggle (`0x141ae3eb0`'s corner/extent pair,
`0x140cc2290`'s mesh-face refinement). Those are the next anchors.

**The mesh-face refinement is read (2026-11-07).** `0x140cc2290` is the last
input behind the `CandidateSolver` seam, and it is not a draw: it writes the
query's normal to `query+0x10..0x1f` and a `u16` to `query+0x3e`, and it
returns a `bool`. The ring only runs for a type-3 mesh shape
(`0x142cba720(query+0x28) == 3`). The read, the argument and the gates are in
`formats/character-controller.md`; the Ghidra body had to be repaired first
(the cookie check `0x1426c3d40` was wrongly flagged non-returning - see
`ghidra-project.md`), and a two-instruction overlap at `0x140ce0863` still
truncates the far end of that decompile. The whole function is a sphere-vs-mesh nearest-triangle query: radius
`param_3`, initial acceptance `3r²`, the gather `0x142caea70` into an inline
`0x80`-entry container, and `0x142cb44c0` (named `ResolveBodyLocalPoint` in the
project) turning the query point into the body's frame. The gather itself is
read: `0x142caea70` is a box-BVH walk whose accept is `0x142cd58a0`, a
box-vs-triangle separating-axis test with a `0.005` tolerance; both are
**verified** (`fn/BoxTriangleOverlap`: 413 cases, the whole return register,
coverage 100%, 12/12 mutants; `fn/MeshGather`: 330 cases, the container
included, coverage 90%, 28/28 mutants), and the ids it
collects are triangle indices. Five single-function reads are the next anchors:
`0x142cae760` (the triangle record), `0x142cc5bc0` (closest point),
`0x142cb4a30` (the point transform), and the handle accessors
`0x142cba720`/`0x142cba2a0`/`0x142cba680` plus `0x142cb49a0`/`0x142cb48a0`.

**Fixed 2026-11-06: the ring's comparison operand.** `solve` projected the
query point, which is the same for all `n` steps, so the ring kept its first
facing hit. The documented acceptance is a strictly smaller `|projection|`
than the best so far, and the only quad that varies is the candidate `A`; the
code now projects `A`. Two tests pin it (see
`docs/formats/character-controller.md`).

**Closed 2026-11-06: pass 3 is not five candidates.** The composition step 6
was a misreading. Pass 3 is one snap of the accepted direction's `y` lane onto
the plane `dot(p - out_dir, n) = 0` with `n` the accepted direction itself:
`s = -dot(n, end - start)` (skip below `1.1920929e-7`), `t0 = dot(start, n) -
dot(out_dir, n)`, `t = (2r - r*r*s)*t0` with `r = RCPSS(s)` (one Newton step,
`2.0` from `0x1431107f0`), skip unless `0 <= t <= 1`, then `out_dir.y = (start
+ t*delta).y`. Addresses `0x140cb98fa..0x140cb99b9`; the `0x80000000` splat at
`0x142f817a0` is the negation and `0x142f7aad8` is `1.1920928955e-4`
(`0x38fa0000`), **not** FLT_EPSILON. `t0` is two hsums and a `SUBSS`:
`hsum(n*start) - hsum(record+0x00, n)`, not `dot(start - out_dir, n)`. The
five-quad loop with `s = dot(scale, base - cand)` and the
`record+0x00.y > -FLT_MAX` test (not an `out_dir.y < 0` gate) is the **shape
branch**, whose quads come from `FUN_141ae3eb0`'s corner and extent, not from
pass 3.

**Closed 2026-11-07: the stage order in `0x140cb86c0`.** The body is one
`.pdata` entry (`0x140cb86c0..0x140cba02b`) with no chained fragments, so the
composer is fully readable, and it branches rather than running a fixed pass
list: the gate `byte[body+0x659]` → pass 0 → shape pass **(iff** a shape
resolved and `byte[body+0x655]` is clear**)** → ring, which is either the
batch flavour (`byte[0x1439b07b8] != 0`: one evaluator, then `kind <= 1` →
snap, else pentagon) or the incremental flavour (that byte zero: `n` single
evaluations, no pentagon and no snap). Nothing after pass 0 runs unless pass 0
left a result, and the shape pass can never reach the pentagon. So the
"pentagon before the ring" reading in the previous paragraph was an artefact
of address order: the pentagon is only reachable *after* the batch ring, and
the endpoints the snap reads (`0x140cb92f3`/`0x140cb92fa`) are the batch
ring's, which is exactly why the incremental ring's later stores
(`0x140cb9b5f`/`0x140cb9b66`) do not bother it. The authoritative map with
every gate and address is *The stage order inside `0x140cb86c0`* in
`docs/formats/character-controller.md`; the longer working note is
`dev/data/notes/fan_stage_map.md`.

**Still open in the composer:** the shape pass's loop registers, the batch
ring's candidate list, the mesh-face refinement `0x140cc2290` and the tagged-hit
`MoverWishTransform` path. `ow-sim::fan::back_snap` carries the snap and is
ready to be wired once the batch ring exists.

**Closed 2026-11-07, two of those.** *Pass 0* `0x140cc1870` is read and ported:
a keyed memo (`dword[body+0x1a60] == dword[body+0x1b00]` plus six lane
comparisons at `0.004`), a block clone through `0x140a01560`, **two** evaluator
calls at layer masks `1` and `0x0e` (`block+0x06`, which the composer's own
template sets to `0x0f`), and a min-score selection in which a **miss scores
`1.0`** because that is a fresh record's `+0x30` - so a hit above `1.0` loses
to a miss. `ow_sim::fan::pass_zero` / `PASS_ZERO_MASKS` reproduce it and a test
pins the quirk. *The incremental ring* is read to its loop body, including its
hand-rolled rotation `A' = A*sin + B*cos`, `B' = B*sin - A*cos`, the point
update `record0+0x00 - offset`, and the `out+0x42 = (flags >> 2) & 1` write the
stage map had missed. The shape pass's corner geometry is read and ported
(`corner_pair`, `shape_probes`), and its three splats are identified - `xmm11`
the seed endpoint, `xmm10` the floored seed, `xmm12` the speed - so no operand
is left unidentified there; only its loop is not yet written in Rust.

**The batch ring's candidate list is closed as a negative (2026-11-07).** The
batch flavour fills the query block's `+0x490` (pointer) and `+0x498` (count)
with `n` quads plus a zero terminator, `+0x480` with a `0.015` splat, `+0x49c`
with `1.0` and `+0x4a4` with `0`, then makes **one** evaluator call - where the
shape pass and the incremental ring make one call per candidate. Read with the
Ghidra decompiler instead of the linear corpus sweep, the pair travels
block `+0x490`/`+0x498` -> `desc+0x28`/`+0x30` (`0x14203dae0`) -> query
`+0x30`/`+0x38` (`0x142cc45a0`) and is **never read**: `0x142cbd510`
(`CollisionQueryDriver`) reads the query block's `+0x00..+0x18`, `+0x20`,
`+0x40`, `+0x48`, `+0x50..+0x7c` and walks the domino's own island list at
`domino+0x1b0+0x44` - 0x28-byte islands, `lVar18 = 4` passes with
`param_4 >>= 1` and `piVar17 += 10` dwords, 0x40-byte nodes with a min corner
at `+0x00`, a max corner at `+0x10`, a shape index at `+0x20` and a child index
at `+0x2c` (`-1` = leaf, otherwise `CollisionWorklistPush` on a 256-slot
stack).

So there is **no list-shaped seam to add**, and `CandidateSolver`'s
one-candidate interface is the client's own shape. The ledger's earlier
"consumer unread" line came from grepping a linear `.pdata` corpus listing
that desyncs on the jump table at `0x14202c580`; that listing is not a
flow-based decode, so greps over it prove neither presence nor absence in
table-rich regions. The one question left is why the composer fills the list
at all - cheap to settle once the ring is ported: watch whether `+0x498` is
ever non-zero, and check for other callers of `0x14203dae0` that read
`desc+0x28`.

## 1. The mover's input→velocity control law

**Question.** `MoverInvoker` (`0x1407ffc50`, interface vtable `0x142fdbf40`
slot 9) passes eight arguments to `MoverAdvance` (`0x140ca7f30`): the mover,
the output position, the local wish vector, the current position, two scales,
the step state and the world. Which function computes those arguments from the
input frame? That caller is the ramp/control law.

**What is known.** `MoverAdvance`'s character path computes
`desired = (scale_a * body[0x18..] + wish) * scale_b + pos`, then runs
`RunCharacterControllerSweep`; the search for a writer of `+0xc0..0xcc` is a
dead end because the verified ramp leaf reads that quad as a static
**direction**, not a stored velocity. `ApplyStateModifiers` (`0x140cbcbe0`)
writes the movement parameter block at `+0x5d0..+0x67c`.

The arithmetic around the open caller is now ground-truth verified:
`fn/MoverRampVector` (`0x140ca75b0`), `fn/MoverRampScalar` (`0x140ca75f0`),
`fn/MoverRampScalarOffset` (`0x140ca7570`) and
`fn/BuildMoverPhysicsCommand` (`0x140cb74a0`) are differential units with
100% branch coverage and declared/killed mutants.

**Measured 2026-11-17 - the call graph cannot answer this.** `MoverInvoker` has
**zero** inbound `call`/`jmp` edges in the whole image (`dev/corpus.py index`:
`edges` has no row with `target = 0x207ffc50`), which confirms it is only ever
reached through the interface table. The dispatch evidence is
`call [reg+0x48]` (slot 9 x 8 bytes): **423** sites image-wide, of which some
belong to other interfaces. So the caller is found in two hops that are not yet
indexed: (a) the class whose constructor stores `0x142fdbf40` into its object
(the store, not the call), and (b) the 423 sites whose register holds an object
of that class. That is the code-pointer-run + vtable-store enumeration listed as
the next step in `docs/port-plan.md`; until it exists this question stays open,
and searching for an `E8` caller is a dead end by measurement, not by
conjecture. Note also that `MoverInvoker` is an **11-piece** function
(`0x1407ffc50..0x14080033c`, 1,797 bytes): its linear listing stops at
`0x1407ffce2 jle 0x14080033c`, so any read of it must use the union extent.

The corrected live chain (2026-11-01) is:

```text
MoverUpdateFrame (0x140cbb260)
  -> ApplyStateModifiers
  -> SubmitMoverPhysicsCommand (0x140cc03d0)
       -> BuildMoverPhysicsCommand, FUN_1420163b0
  -> input-source pass (body+0xf0 slot 8 / FUN_140cc1060)
  -> FUN_140cba030 (input object: wish +0x60, gate +0x98)
  -> FUN_140cc0dc0 (wish spring)
  -> FUN_140ca5c50 (state vector), FUN_140ca75b0 (ramp vector)
  -> SetCharacterControllerVelocity (0x142cbd1b0)
```

`MoverStateMachine` (`0x140cbacc0`) is a separate record handler: it calls
`ApplyStateModifiers` then `SubmitMoverPhysicsCommand` and never sets the
controller velocity. `SubmitMoverPhysicsCommand` does **not** call
`SetCharacterControllerVelocity`. `FUN_140cb5760` (`MoverUpdateCommand`) is the
second update path and sets the velocity directly at `0x140cb58b4`. The input
object's wish comes from source `+0x00` and its gate from source `+0x40`; the
source is filled by the body `+0xf0` records, with `FUN_140cc1060` as fallback.
`FUN_140cba030` is an interior `.pdata` chunk, not a real entry.

**The velocity law itself is read (2026-11-01).** Both update paths hand
`state_transform(wish) + ramp_vector(body)` to
`SetCharacterControllerVelocity`: `MoverUpdateFrame` at
`0x140cbbdc2..0x140cbbe0b` (`movups` the wish from `+0x1140`, `FUN_140ca5c50`,
`FUN_140ca75b0`, `addps state + ramp`, call) and `MoverUpdateCommand` at
`0x140cb5890..0x140cb58b4` (`FUN_140ca75b0`, `addps ramp + state`, call, gated
on `input+0x4c != 0`). Both pass `body + 0x10d0` as the ramp's third argument,
so the ramp reads its flags at `body+0x10d0` and its offset at `body+0x10fc`.
The two `ADDPS` operand orders differ only in the surviving NaN payload. The
composition is implemented as `ow-sim::frame` (`state_vector`,
`ramp_from_body`, `frame_velocity`/`command_velocity`, `update_frame`); its two
operands are the verified units `fn/MoverStateVector` and
`fn/MoverRampVector`, and the composition itself has no client unit.

So the open part of item 1 is narrowed to the **wish producer** and the
**velocity→position stage**, not the velocity expression.

**The wish producer is read to its last stage (2026-11-02).** The fan's
candidate evaluator `0x1420328c0`, the handle dispatcher `0x142cc45a0`, the
query driver `0x142cbd510` and the narrowphase `0x142cc1610` are read, and the
narrowphase pins the policy's arguments exactly
(`callback(&hit, packedShapeDescriptor, answerRecord)`, permuted by the thunk
`0x14202c580` into `(hit, packed, record)`). The policy body
`0x14202bc90` is **read** (2026-11-02): the `ADD`/`ROR 0x22` chain is a
**pointer decode**, not a jump, and the far branch targets are junk-filler
obfuscation. What is left of the producer is data, not code: the map's
geometry tables, so **no wish is produced** and the client stays a viewer.
Details: `formats/character-controller.md`, "The collision query driver and
the hit contract".

The source is partially read (2026-11-01): the first-person view object
(vtable `0x142f87810`, slot `+0x250` = `0x140654530`) writes the actor command
at `+0x1210` — yaw `+0x04`, pitch `+0x06` (s16 fixed-point, clamped to
`±0x3fff`), packed bytes `+0x08..+0x0b`, button/flag `+0x0c`, flag word
`+0x10` — and pushes 0x1c-byte usercmd entries into the ring at `+0xe30`.
`FUN_140689930` is probably driven from `[actor+0x38]+0x178` (call site
`0x14214170e`), then `FUN_14068cf10` builds the 0x14-byte command and
`FUN_140cbb260` copies it to `body+0xd0/+0xe0`. That packer is now read
exactly (current record with the next record's yaw `+0x1228` / pitch `+0x122a`
overlaid; sequence `[actor+0xe14] + 1`). **The "external wish producer" route
is closed:** every writer of `body+0x1140` and of `source+0x00` is accounted
for, and none of them turns the buttons into a direction - the wish is a
closed loop seeded only by the fan's collision-scored candidate. The state-record path reaches
`FUN_140cb63c0`/`FUN_140cb86c0`, which writes source `+0x00` (wish), `+0x10`
and `+0x40` (gate). The wish is **not** a direct command copy: `FUN_140cb86c0`
builds an input vector from `body+0x1140`, the controller position and
`body+0xc0`, then rotates it through `FUN_140ca5b90`/`FUN_140cb5350`/
`FUN_140cb83b0`/`FUN_140c71e10`. The exact `in_vec` expression, the command
byte meanings, the `actor+0x38` vtable and the `+0x58/+0x60/+0x68` virtuals
remain unread. The `0x14068f1aa` variant is a dead/obfuscated duplicate.

The transform is a 4x4 column-major affine: forward `v' = x*c0 + y*c1 +
z*c2 + c3` (`FUN_140cb5510`, verified as `fn/MoverWishForward`), inverse
`v' = R^-1*(v - c3)` with `det = c0·(c1×c2)` and `rcpps` + Newton refinement
(`FUN_140cb5350`, verified as `fn/MoverWishInverse`; the harness's Unicorn
`rcpps` is exact IEEE `1/x`, and the client transforms all four lanes). The
writer is a fan solver: seed `s = d.x*body[0xc0] + d.y*body[0xc4] +
d.z*body[0xc8]`, `d = controller_position - M*(body[0x1140].xyz,1)`, then a
loop over `body[0x5bc]` directions (`2π/N`), `0x1420328c0` composition, and a
back-transform through `FUN_140ca5b90` to source `+0x00`. The two call sites
pass `dt = 0` (`FUN_140cb63c0`) and `dt = 10` (`FUN_140cba030`); the gate is
1 normally, 0 when `body[0x610] <= 0 && dt <= 0` or no candidate is accepted.

**The fan's own arithmetic is read (2026-11-01)** and written up in
`formats/character-controller.md` ("The fan solver inside the wish writer"):
three passes (two five-way, one `N = body[0x5bc]`-way), the speed
`((min(|body[0x1150].xyz|, max3(cap slots)) * 1.1) * (body[0xe8]*1e-6 * E))`
floored by `body[0x5cc]`, the radius
`max(body[0x65c], 0.35) * body[0x5b8] - 0.015`, the `y = 0.015` lane, the
`72°` and `2π/N` rotations, the `+0.1` seed bias, the second seed term
`dot(query - (pos - ramp), dir)`, and both acceptance rules
(`|score| < best && y > best_y`, then `|score| < best`). `ow-sim::fan`
implements exactly that with unit tests. Still open: the generic helpers
`0x1426cd3d0`/`0x1426de990`, the per-pass record layouts, the
`0x141c5d780(object, byte)` query chain and the `+0x41`/`+0x42` flag slot's
producer. (The evaluator `0x1420328c0` named here on 2026-11-01 is read as of
2026-11-02.)

**What would settle it.** The spring itself is now verified
(`fn/MoverWishSpring`, 264 cases, 90% branch coverage, 12/12 mutants) with a
seeded service object: `body+8` encodes `T = 0x100000000` and the descriptor at
`T+0x28`; the state-id sentinel makes the record transforms identity. The
forward and inverse transforms are verified too (`fn/MoverWishForward`,
`fn/MoverWishInverse`, each 264 cases, 100%, 5/5 mutants), as is the copy
wrapper (`fn/MoverWishCopy`) against a synthetic state record. What remains is
the lookup in Rust, the `R15` wish source (`+0x1210`, `+0x1220`,
`+0x1228/+0x122a`, `+0xe14`), `FUN_140cb63c0`'s writers, the live service
object, the `FUN_140ca5c50` state-record transform and the Domino dispatch. The
invoker caller is still wanted, but the 2026-11-01 scan found only its own
vtable slot; it is likely dead, not proven dead.

**The next read is the candidate evaluator `0x1420328c0` — a collision query.**
It is the last unknown between the read fan arithmetic and a wish: it takes
`(body, &out, &block, &record)` and returns a bool plus the score and the
height the acceptance rules compare. It is a thin wrapper (it fills a
one-float/one-dword descriptor and calls `0x14203dae0` at `0x14203292b`), and
its first argument is `[body+0x240]`, the spatial world the mover's other
geometry paths use (`0x14202fb00`), so a candidate is only accepted when the
character can reach it. `0x14203dae0` is 1158 bytes in the image's obfuscated
region, but a trace (`dev/probe_evaluator.py`) shows its reachable code is
just the prologue `0x14203dae0..0x14203dbd9` plus the body
`0x14203de51..0x14203e0fe`; the scattered blocks are junk. The body is now
read: it resolves the world's geometry object (`world+0x620`), fills the
result record the fan reads back, builds a 0x71-byte query struct from the
descriptor object (`[param_2+8]`) and calls `0x142cc45a0` (62-byte handle
dispatcher, 557 bytes) or `0x14212db80` (78 bytes, not on this path). The
static flow at `0x14202bc90` is **junk-filler obfuscation**, not a computed
jump: dead runs of instructions end in opaque-predicate branches to other
functions (`0x142049e89`, `0x14202df4a`, `0x14202e840`, `0x142030dc2`), which
is what pulled Ghidra's decompile out of the function. A linear sweep decodes
the body to exactly its end (427 instructions, no data islands), so the live
path is plain code and the body is read (see below).
**This couples the input side to collision** — the same query is item 6 — so
the wish producer cannot be finished before it.

**The descriptor and the record are read (2026-11-02).**
`formats/character-controller.md` now has the query object's field table
(0x4a5 bytes, three inline vectors at `+0x20/+0x130/+0x240`, the sweep
segment at `+0x450/+0x460`, mode word at `+0x06`), its producer (the fan
writes `A = P + S + (seed + 0.1) * basis`, `B = P + S - speed * basis` with
`P = M * wish`), the 0x50-byte answer record and the memo front-end
`0x140cc1870` (state id `body+0x1a60`, endpoints within 0.004 of
`body+0x1ae0`/`body+0x1af0`). `0x14202c580` is a four-move thunk rotating
`(ctx, a, b)` into `(b, ctx, a)` before `0x14202bc90`, so the policy body's
first parameter is the **hit** object (`0x14202c580` receives the narrowphase's
`&hit` in RCX). The wish-to-world step is `0x140c9e670`
with `0x140c9e740` (matrix at `body+0x14c0`, flag `body+0x1110`, id
`body+0x1520`). The wrong `noreturn` mark on `0x140c9e670` is cleared in the
project image, so `MoverWishWriter` now decompiles end to end.

Remaining for this item: the geometry ingest that gives the policy a world to
read. The body of `0x14202bc90` is **read** (2026-11-02): contract
`(hit, packed, ctx)` -> 1/2/3, the world gate, the `0x1420af950`
layer/exclusion filter, the decoded geometry-key dedupe, the 0x50-byte record
fill for mode 1 and the `hit[0]` classification for mode >= 2. Its apparent
"computed jumps" were junk-filler obfuscation, not a dispatch table, and its
two "unread callees" were a constant-injection stub (`0x142018990`) and a
junk-prefixed in-place decode (`0x140543440`). So the fan is fully read and
**no code is left unread in it**; what remains is the map data the policy
queries (`[rdi+0x80]`/`[rdi+0x110]`/`[rdi+0x250]`), which is item 6.

**Next anchor:** implement `ow-sim`'s version of the policy against synthetic
hits and worlds and add a `fn/CandidateHitPolicy` differential unit; the
collision ingest (item 6) is what a live candidate needs.

**Next anchor.** `fn/MoverWishForward`, `fn/MoverWishInverse`,
`fn/MoverWishCopy`, `fn/StateTransform` and `fn/MoverStateVector` are
verified. The state lookup is implemented in `ow-sim::state` for the leaf
path: the gate, the bitmap/rank/table index, the `0x140543480` decode and the
object matrix copy. The `[object+0x298]` parent path and the pose path are
still unread.

**Config path read (2026-11-01).** The verified ramp and spring read
`+0x5a8/+0x5b4/+0x10ec` and `+0x5c0/+0x5c4/+0x688/+0x68c`; those values are
now read at the value level. `0x1405a87d0` default-constructs the settings
object (`+0x5a8/+0x5ac/+0x5b4 = 1.0`, `+0x5c0 = 0.0`,
`+0x5c4..+0x5c6 = 1`, `+0x5d0 = -2`, `+0x5d4 = 1.0`, `+0x5dc = 0x10000`) and
is verified as `fn/DefaultMoverSettings`, and the asset apply `0x140c862e0`
is verified as `fn/ApplyMoverSettings` (11/12 mutants, 33% branch coverage
with the vector-growth and `+0x298` edges declared); its asset-field map is in
`formats/character-controller.md`. `+0x688`/`+0x68c` are computed in
`ConstructMoverBody` (`0x140ca9730`) from the body's `+0x440..+0x46c`, and
`+0x10ec` is the per-frame ramp accumulator written by `FUN_140cc079d`
(`0x140cc079d`). `ApplyStateModifiers` (`0x140cbcbe0`) is a 37-way switch
writing the separate `+0x5d0..+0x67c` block and touches **none** of these
fields.

**What remains open on the config path** is the asset *producer* (which
install data builds the object `0x140c862e0` reads) and the point where the
live body receives its copy of the settings block. Also open: the Domino
controller is a shape/broadphase layer, not the integrator — `MoverAdvance`
(`0x140ca7f30`) is. `body+0x1ad8` is a **slot**; `[slot]` is the variant
object, whose tag selects a generation-checked handle. Collision reads
precomputed triangle records and the map-chunk ingest is unread.
`ApplyStateModifiers` is now verified (`fn/ApplyStateModifiers`: 336 cases,
12/12 mutants, 76.5% branch coverage). The world slot, the tag dispatcher,
both position leaves and the character velocity chain are verified on the
no-shape path (`fn/WorldPosition`, `fn/VariantPosition`,
`fn/CharacterPosition`, `fn/RigidPosition`, `fn/MoveCharacterController`,
`fn/ApplyCharacterControllerMove`, `fn/SetCharacterControllerVelocity`); the
four shape-bound leaves (`fn/ShapeSphere`, `fn/ShapeCapsule`, `fn/ShapeBox`,
`fn/ShapePoints`), the shape dispatcher (`fn/ShapeDispatch`) and the shape
list walk (`fn/MoveCharacterControllerShape`, contact id `-1`) are verified
too, and the harness now has a `vec4` return channel (XMM0, 128 bits)
plus the whole-output differential, and the type-3 mesh path is now read and
verified too (`fn/MeshSweep` on `0x142caed30`, the no-support-point walk and
its triangle test; `crates/ow-sim/src/collide.rs::mesh_sweep`). The
next reads are the contact event (`0x142cf32d0`), the mesh support-point path
(`0x142cc9460`), the rigid velocity path
(`0x142cb5310`), the `MoverAdvance` integration and liveness, and the fan
writer. The harness supports `startRegs`, MS x64 stack
arguments, range-form `allowedImageReads`, `vec4` returns and state-snapshot
mutation; a per-case state overlay is still the remaining fixture limitation
(the inverse matrix is fixed).
`formats/character-controller.md`, `dev/verify_harness/units/fn-MoverWishSpring.json`,
`dev/verify_harness/units/fn-MoverWishForward.json`,
`dev/verify_harness/units/fn-MoverWishInverse.json`,
`crates/ow-sim/src/movement.rs`, `MoverUpdateFrame` (`0x140cbb260`),
`MoverUpdateCommand` (`0x140cb5760`), `FUN_140cba030`, `FUN_140cc0dc0`,
`FUN_140cb63c0`, `FUN_14068cf10`.

---

## 1b. The live walk in `ow-client`: the four stages it does not have

**State.** The client walks (`docs/movement-plan.md`, last section; MODLOG
2026-11-08) and jumps (MODLOG 2026-11-16). The world is the range's own
collision (`ow_tank::range_world` + `ow_sim::walk`), the step is the verified
mesh leaf, the frame is the read law, and the airborne arc is the read vertical
law (`ow_sim::air`). Four stages around that chain are open, and each is
declared rather than substituted:

- **The input stage.** No read path in this image turns buttons into a local
  wish vector; the read chain begins at `body+0x1140`. The client's WASD input
  therefore *is* the input source, with its magnitude in metres per second
  (`--speed`). Its default is the *read* value: the mover component's forward
  base speed, field hash `0x19452F15`, whose install-wide common value is 5.5
  (`formats/character-controller.md`). `ow-client` now replaces it with the
  picked hero's own row (2026-11-14; the pick is a declared viewer default and
  the roster and row are read, the wish producer is not). Next anchor: the
  character mover component's input publish path (`MoverUpdateCommand`
  `0x140cb5760` and the actor-side writer `0x140cb86c0` were read; neither
  builds the local wish).
- **The walk's world (2026-11-15).** The walk now collides with the map's own
  `MAP_CHUNK_COLLISION` bodies only (`RangeWorld::level`, `ow-client::walk_world`).
  The per-model `STU_537E56E4` holder meshes were in the world and are not the
  static collision: their read consumer is a query that writes nothing
  (`FUN_14210bac0` -> `0x142caf6c0`), their Domino body route hangs off the
  collision component whose attach is unread, and adding them cost `0.31` m of a
  `5.5` m walk, `0.461` m of drift and a `0.78` m climb in the flat spawn room
  (measurement: `formats/character-controller.md`). What is still open is the
  *entity* route: which nodes carry the collision component, so a dynamic prop's
  holder body can be added back for entities only.
- **The vertical stage (2026-11-15).** The floor snap is a stand-in and it is
  measured: its "only down" guard was taken from the probe origin (`body + 0.2`),
  so it lifted the body up to `0.2` m per frame (`+0.118`, `+0.229`, `+0.165` m
  in three consecutive frames, `0.78` m total) and it teleports down by up to the
  `2.5` m reach in one frame. It is also load-bearing for the landing, because
  the verified leaf reports nothing for a sweep that starts on or inside a
  surface, so a sweep-based fall cannot land by itself. **The corner clip is
  fixed inside the stand-in** (2026-11-15): `World::overlap` asks the leaf's own
  "already inside" mode (`block+0x72`) at `radius - CONTACT_SKIN`, `depenetrate`
  pushes the body out, and a miss now moves to the depenetrated target instead
  of applying the residual - a right-angle corner that carried the body `3` cm
  through the second wall now stops it (unit test
  `a_corner_stops_the_body_at_both_walls`; `--wall-audit`'s near-vertical
  tunnelled frames `174 -> 27` of 8,741). Next anchor: the pair build
  `0x142cd4d90`, the position read `0x142cd5270` and `FUN_142cf5020` in
  `0x142cf3f80`'s iteration, then port the resolve; the step range is already
  read (`+0x688`/`+0x68c`) and `FUN_140cc1440` is the stage that consumes it.
- **The sweep's aftermath.** `RunCharacterControllerSweep` (`0x142cbcee0`) is a
  thunk to `0x142cf3f80`, read in full: a shape-pool walk on the handle at
  `world+0x08`, the `+0x24`/`+0x28` layer/mask test, a transform test against
  the query bounds (`+0x10..+0x1c`), and a dispatch on `+0x50`. Not ported: the
  Domino bookkeeping that writes the character's shape transform and the pair
  contact lanes. The client slides by projecting the residual off the contact
  normal, so the *travel* is the client's and the *slide* is not. Next anchor:
  the shape-position write after the sweep and the pair-array append.
- **The movement asset.** The per-hero rows are read into
  `ow-tank`'s mover reader (forward/side/back/crouch speeds, terminal fall
  `0xB74245E8 = 30.0`, the take-off lane `0xEC289987`, and the jump struct
  `STU_97288E54`'s six defaults), and the walk uses the terminal fall as its
  clamp. Missing: the *gravity* the client integrates with
  (read 2026-11-16: `-17.5 m/s^2` for the install's `1.75`, see the jump
  bullet below) and the per-frame vertical record table (see the jump bullet
  below), so the airborne arc is shipped while the ground resolve is not. Next
  anchor: the component that applies the movement asset's rows to
  the body (`0x140c862e0` was read as the apply) and the producer of the five
  inline record vectors built at `0x140ca37d0`.
- **The jump law (read 2026-11-16; the jump is shipped).** The earlier
  record ("`FUN_140cc1440` is control-flow-flattened, the impulse is unknown")
  is **withdrawn**: the function is read end to end and it is the jump /
  crouch stage, not a vertical integrator. Read and shipped as
  `ow_sim::jump` (16 tests) with six differential units:`
  - the button split, `MoverUpdateFrame` `0x140cbb627`/`0x140cbb63c`: bit 6
    (`0x40`) is jump, bit 7 (`0x80`) is crouch;
  - the handler chain `FUN_140cc1060` `0x140cc1060` -> `FUN_140cc1440`
    `0x140cc1440` with its six arguments (`cmd+0x10`, `cmd[0x40]`, jump,
    crouch, `cmd[0x41]`);
  - the state file word `body+0x10d0` and its five transitions
    (`MoverSetCrouch` `0x140ca9100`, `MoverOnLanding` `0x140ca9120`,
    `MoverEnterAir` `0x140ca9150`, `MoverOnTakeoff` `0x140ca9190`,
    `MoverOnGround` `0x140ca91e0`), with the gate mask `0x80000c` = bits 2, 3
    and 23. Bit 19 is **not** in the mask - `fn/MoverEnterAir` caught that;
  - the gate (property `0x14` > `0.5`, gate mask clear, latch bit 0 clear, a
    live world at `body+0x1ad8`);
  - the impulse: `target = sum_i body[0x1150][i]*row_i + body[0x1510]`, then
    `rise` along `body+0xc0` (the constant up axis `(0,1,0,0)`), then
    `body[0x1150] = (dot3(d,r0), dot3(d,r1), dot3(d,r2), dot3(d,r2))` with
    `d = target - anchor`. `rise` has **two branches**:
    `(flags & 0x80000c) != 0 && along > 0` uses `(along + body[0x60c]) - along`,
    everything else uses `body[0x60c] - along`; both lane sums are three-lane
    `(p1+p0)+p2` as the client's `shufps` pair does them. The anchor is
    **never written** anywhere in `.text`, so on the ground the world lane
    becomes `body+0x60c` and in the air (rising) it gains it.
  - the magnitude is a **component field**: the body is a copy of the mover
    component at `+0x528` (`CopyMoverComponentFields` `0x140ca4680` /
    `FUN_140ca4980`), so `body+0x60c` = class `+0xe4` = hash `EC289987`
    (`6.0` for every playable hero on this install);
    `ow_tank::entity::mover_takeoff_lane_of` reads it. The
    mapping is corroborated by five more offsets, listed in
    `formats/character-controller.md`.
  - the airborne prologue (`0x140cc148f..0x140cc155b`): while airborne, a
    request whose `dot(body+0x1150, cmd+0x10)` is below property `0x26`
    (default `0.001`) calls `MoverOnGround`, sets `body+0x1a7d = 1` and arms the
    take-off budget `body+0x1ad0`; on the ground, no request calls
    `MoverOnLanding` and a request decrements that budget. Property `0x15`
    gates the returned "jumped" bool.
  - the record appliers the stage reads through: `FUN_140cb3990`
    `0x140cb3990` (0x38-byte records, one property) and `FUN_140cb3b10`
    `0x140cb3b10` (0x40-byte records, a four-lane vector), both shipped as
    `ow_sim::modifiers` and both units.
  The units: `fn/MoverOnTakeoff`, `fn/MoverOnLanding`, `fn/MoverOnGround`,
  `fn/MoverEnterAir`, `fn/MoverPropertyRecords`, `fn/MoverVectorRecords` - all
  branch-complete and mutation-clean against the image. They found two real
  defects in the first cut (the mask bit and the uncleared bit 10), both fixed.
  The whole of `FUN_140cc1440` is not a unit: its middle third is obfuscated
  dispatch and the impulse arithmetic is inline, so the impulse is transcribed
  and tested, not bit-exactly proved.
  What is still open, and why no jump is shipped:
  1. **`FUN_140cb2220` `0x140cb2220` is the per-frame consumer of
     `body+0x1150`, and it is now read end to end** - it is *not* a gravity
     integrator. Phase 1 scales by property `0x22` against the vector length,
     phase 2 by property `0x23`, phase 3 applies property `0x24` to the y lane
     alone, phase 4 divides y by `body+0x60c`, lets property `0x25` act and
     multiplies back, phase 5 runs the 0x40-byte record passes (mode 1 then mode
     0, both behind `(*body->vtable)[0xa8](body) != 2`), and the tail calls
     `MoverOnLanding` when the world lane rose by more than `0.004`.
     **There is no gravity term anywhere in it.** The take-off lane
     `body+0x60c` appears only as the phase-4 lateral cap.
  1a. **Settled: `RSQRTSS` is `f32(1.0 / f32_sqrt(x))` in this image's
     oracle** (220/220 sampled; the double-precision forms miss 44/220), so the
     Newton sequence is portable and phases 1-4 are **implemented**
     (`ow_sim::resolve`) and **verified** (`fn/MoverResolveVector`, 100% branch
     coverage, 7/7 mutants). The differential also corrected the reading: phase
     2 masks the y lane out of both the length and the scaling. What is left is
     phase 5 and the bit-24-clear transform path.
  1b. **The 0x40-byte records' producer is runtime data (still open).** The five parallel
     inline record vectors are built at `0x140ca37d0` with `count = ctor arg`
     and capacity 4 (`0x80000004`); the constructor's only callers are the
     character-object constructor `0x1407ff1f0` and a factory `0x1407ff1b0`
     that is referenced **only** from a data pointer at `0x142fdbee8`. So the
     record content is handed in from outside the read set, and phase 5's `kind
     == 3`/`4` plane projections (hit path `0x140cb26a2`, five-way dispatch on
     `record[0x2c]`) are only read at the skeleton level.
  2. **The live position is not `MoverAdvance`.** `MoverAdvance`
     `0x140ca7f30` is the read integrator, but its only caller is
     `MoverInvoker` `0x1407ffc50`, which has no code caller; the live position
     comes from the swept solve (`RunCharacterControllerSweep` `0x142cbcee0`
     -> `0x142cf3f80`), whose contact-to-position rule is unread.
  3. `MoverStateBlockMatrix`'s **copy order against the settings object**
     (`0x1405a87d0` writes `1.0` to `+0x5a8`/`+0x5b4`; the component copy puts
     the capsule floats `0.5`/`0.25` there). One of the two writers runs last
     and it is not read; the ramp unit and the capsule page disagree because of
     it. `body+0x60c` is not one of the settings fields, so the take-off
     magnitude is unaffected.
  Next anchor, in order: (a) find the writer of the five record vectors
  (`body+0x970`, `body+0x880`, `body+0x770`, `body+0x6a0`, `body+0xa80`); (b)
  diff `FUN_140cb2220` into a unit once the records can be synthesised (they
  are plain data, so this may not need (a)); (c) the sweep's position resolve.
- **The level collider's consumer (now the walk's only world).** The
  `MAP_CHUNK_COLLISION` body is the walk's ground (`ow_tank::collision_level`): its tail element names a vertex,
  node and triangle array in the client's own mesh record layout, tiling the
  payload exactly, and the sweep reproduces the rendered floors. What is *not*
  read is the function that turns those arrays into a shape - `BuildBvh`
  (`0x142ccb1b0`, five call sites, "split wrappers over the collider-face
  array, all unreachable") is the likely one. Until it is read, the shape's
  identity placement and the use of the stored BVH as-is are the data's own
  conclusion, not the code's. Next anchor: the five `BuildBvh` call sites' real
  reachability.
- **Spawn choice.** The map's own player spawn is still unread. The practice
  map ships no `MAP_PLACEABLE_SPAWN_VOLUME` (kind `0x19`), dynamic spawn point or
  dynamic spawn group placeables; its only spawn records are the 25 AI bot
  spawn groups (kind `0x16`). The map's two `MAP_PLACEABLE_TYPE_TRIGGER_VOLUME`
  (kind `0x17`) boxes are readable (`+0xA0` yaw axis, `+0xAC` centre, `+0xB8`
  half extents); the one whose centre lies inside a `MAP_PLACEABLE_AREA` box is
  the hero-select room at centre `(71.169, 0.948, 53.110)`, half extents
  `(11.861, 5.179, 7.608)`, floor `y = -4.231`. `ow-client` uses that volume as
  its **declared viewer default** (`--spawn x y z` overrides it; `find_spawn`
  still snaps to the floor), not as the client's rule. Next anchor: the function
  that selects a player spawn from the mode's spawn-room volume.

What *is* proven: `range_world_real` (272 shapes, 3,017 placed instances, 32/32
downward sweeps), `walk`\'s 13 units (sweep fold, wall slide, floor snap, the
rotation convention), and `ow-client --walk-test` (5.5 m forward in one
second at 5.5 m/s from the spawn room's floor, no idle drift, 5.5 m strafe,
finite body).

---

## 2. The first-person camera consumer and per-frame look

**Question.** `STUFirstPersonComponent` carries `m_position` and four scalars.
`FpViewmodelConsumer` (`0x140d2f060`) and `FpViewmodelBind` (`0x140d2faa0`)
store the view-model entity/model references but do not read them. Which system
applies the offset and scalars? Separately, which function consumes the five
`*InterpolationSpeedInverse` vars and four `*Speed` rates per frame?

**What is known.** The component is type `0x35`; its consumer is a system keyed
by type or entity offset, not a vtable method. The FP camera setup
`ApplyFirstPersonCamera` (`0x14096c5b0`) and
`UpdateFirstPersonCameraPose` (`0x140997b90`) build the pose from the FP
animation entity. The config vars and defaults are read; their per-frame reader
is not.

**What would settle it.** Read the type-`0x35` system's per-frame path and the
camera interpolation consumer. Then the FP pose and the component values can be
applied exactly.

**Next anchor.** `formats/camera.md`, `formats/animation.md`,
`FpViewmodelConsumer`, `UpdateFirstPersonCameraPose`.

---

## 3. The reference-skeleton retarget

**Question.** The client registers `anim.EnableProxyRetarget` and
`anim.EnableFinalRetarget` ("proxy pose re-target between (inherited) blend
trees with different reference skeletons" / "final pose re-target between
reference skeleton and model skeleton"). The clip's P lane is a reference-rig
local. Where is the reference→model stage?

**What is known.** Character rig 200 bones, FP rig 79, 64 shared ids with
identical binds; the weapon-hook fold is exact. The card's shoulder rows carry
`2.395x` the model bind, so direct application is wrong by construction at the
shoulders. The retarget body is not the sampler.

**What would settle it.** Read the two retarget registrations and the final pose
stage, then implement the reference-skeleton transform.

**Next anchor.** `formats/animation.md`, `AnimSamplerRule` (`0x1420feca0`),
`crates/ow-tank/src/blendtree.rs`.

---

## 4. The Pose2d dataflow VM

**Question.** What drives the first-person blend tree: the branch category, the
x/y driver inputs and the LayerChild weight? At the authored defaults the tree
output is the base clip (c71).

**What is known.** The tree structure, item slots and sampler gate are read.
The Pose2d nodes are authored; the per-tick VM that produces their inputs is
not.

**What would settle it.** Read the Pose2d VM in the animation runtime and the
LayerChild weight writer.

**Next anchor.** `formats/animation.md`, `crates/ow-tank/src/blendtree.rs`.

---

## 5. The weapon fire loop and damage application

**Question.** How does a volley fire, and where does per-shot damage come from?
The volley runtime carries `numShotsPerSecond` at `+0x1c8`,
`numProjectilesPerShot` at `+0x1c0`, `projectileSpeed`, `projectileLifetime`,
`castLength` and the typed projectile motions. The graph data carries
`STUStatescriptModifyHealth.m_amount` literals on 627 graphs. The join from
weapon → fire → hit → damage is open.

**What is known.** Firing runs through the statescript executor, not the volley
class's vtable. `StatescriptDriveWalk` (`0x140a264c0`),
`StatescriptNodePush` (`0x142141f70`) and
`StatescriptSlot43Executor` (`0x140a9ba70`) are read. The per-node
implementation behind the type index is not.

**What would settle it.** Read the executor's node dispatch and the
`ModifyHealth` applying function, then join the weapon graph to its ability /
impact graph and the resolved `amount`.

**Next anchor.** `formats/weapons.md`, `crates/ow-tank/src/graph.rs`.

---

## 6. Character collision

**Question.** The client's character step/ledge/slope collision is unread. The
map mesh is currently visual only; the default client draws the practice-range
geometry but has no character collision. `--no-map` still offers the flat
stand-in plane and boxes for viewer work.

**What is read now (2026-11-01).** The sweep is `0x142cbcee0`
(`RunCharacterControllerSweep` -> `0x142cf5660` -> `0x142cf3f80`), called from
`MoverAdvance` with `([body+0x1ad8], world, &block)`; the resolved position
comes back from `0x142cbcd10`. The block is `{desired vec4, scale_b,
body[0x628]*scale_b, 0.005, step_state[0x94] clamped by body[0x1a88],
step_state[0x9a]}`. The sweep's contacts are `[world+0x200]`, count
`[world+0x20c]`, stride `0x20`; the advance projects each contact's first quad
onto the normalised service vector `[service+0x40]` (fallback `(0,1,0,0)`),
keeps the minimum in `body+0x19b8`, and calls the service object's `+0x40`
slot per contact. The `>= 4` contacts branch (`0x140ca8643`) is unread, as is
the `+0x40` response slot. The sweep reads precomputed triangle records
(normal `+0xc0`, count `+0xf6`, 0x30-byte vertices from `+0x18`), never map
chunk bytes.

**What would settle it.** Read the `+0x40` response slot, the `>= 4` contact
branch, and the map's collision chunks (`MAP_CHUNK_COLLISION`,
`teMAP_CHUNK_ADDITIVE_COLLISION`) into the shape records the sweep consumes.
The gap is the map-chunk-to-shape ingest: the client's spatial structure is
built by the command triple, and the producer of the triangle records from
chunk bytes is not read.

**The live collider is the model's brush tree, not the 17-array chunk
(2026-11-02).** `GeometryBuild` (`0x14213c440`) is read end to end and is the
only producer of a `0x60` cell: it takes the node's model
(`node+0x88` -> `+0x38` -> `+0x40` -> `+0x48`), copies the collision parameters
(`0x142132cf0`), pushes one cell from `geometry+0x108` (`0x142136010`, which has
this single call site), builds the mesh from the object's `+0x98` instance array
(`0x142cec9e0` / `0x142ced620` / `0x142ced990`), runs `BrushTreeToMesh`
(`0x142cf23c0`) and then `MeshToColliderFaces` (`0x142cf2010`) with the
collision radius and the object's `+0x0a` id, stamps the brush's two `u16` ids
into each `0x20`-byte face at `+0x18`/`+0x1a`, appends the cell's `u16` array
(`0x142139080`), normalises `geometry+0x42` by `sqrt(a*a + b*b)`, creates the
body (`OwnerCreateBody`) and finalises (`0x14213d0b0`, whose pool-A push
`0x142136130` also has one call site). Nothing else in the image pushes a cell
or a pool-A record, so the shape records the sweep walks are built from
**brush trees carried by models**, and the chunk's 17 arrays are pointer-fixed
at load with no reader found (round 2 reached the same place from the component
side). The decoder question therefore becomes two data questions: which model
the map's collision comes from, and how its brush tree is encoded. The
`0x142cca460` appender (`ColliderFaceAppend`) and the `0x142ccc120` wrapper
(`ColliderMeshEmit`) are read: the appender walks the `0x14`-byte index
records, reads the three vertices, skips a record `ColliderTriangleDegenerate`
rejects, and fills one `0x60` face record plus the running area; the wrapper
calls it and frees its three arrays.

**`MeshToColliderFaces` detail corrected (2026-11-02).** The thickening is
**z only**: the first vertex block is `(x + 0.0, y + 0.0, z + radius, w + 0.0)`
and the second `(x - 0.0, y - 0.0, z - radius, w - 0.0)` — `x`, `y` and `w` pass
through the signed zero add, which only turns `-0.0` into `+0.0` in the first
block. The index array is three blocks: every source face outward (as written),
every source face inward (`i2 + n, i1 + n, i0 + n`), then — only when
`radius >= 0.005` — one stitching pair per marked edge in face order, entry 0
pairing `i0`/`i1`, entry 1 `i1`/`i2`, entry 2 `i2`/`i0`, each pair being
`(a, a + n, b + n)` and `(b + n, b, a)`. Every `0x14` record carries `+0x0c = 0`
and the caller's surface id at `+0x10`. `crates/ow-sim/src/collider.rs::shell`
now matches this and its tests are updated; a differential unit for it is not
built yet (**not tested**), because the function allocates its two arrays
internally and the harness needs a pinned allocator region to compare them.

**The wish producer's evaluator is this query, and the chain is now read to
its narrowphase.** `0x1420328c0` (a wrapper) takes `[body+0x240]` — the
spatial world — and calls `0x14203dae0`, whose reachable body is read; it
resolves the geometry at `world+0x620`, takes the `u16` handle at
`[geometry+0x31f8]`, and calls `0x142cc45a0`, which decodes the handle through
`0x1442a0bc0` to a domino record and calls **`0x142cbd510`** — the collision
query driver. That driver is read: four passes (one per bit of the caller's
flag byte), each building its sweep with `0x142cd5540` (verified as
`fn/CollisionSweepBounds`), rejecting shapes with an AABB test and a
moving-box separation test, pushing child indices with `0x140d24b60`, and
routing leaves to `0x142cc1610`, which builds the entry's world transform
(`0x142cb8b80`), applies the layer filter (`0x142cbb350`) and dispatches on
the type byte (`0x142cb8d60`) to five per-type primitives (`0x142ccdf40`,
`0x142ccd7b0`, `0x142cd3c80`, `0x142caed10`, `0x142cd0ae0`). Type 0's
analytic path is now read and verified (`fn/CollisionSphere`,
`ow_sim::collide::sphere_sweep`), and the wrapper's layer filter is verified
as `fn/CollisionShapeFilter`, and the type-1 capsule as
`fn/CollisionCapsule`; type 0 and type 1's support-point **handoff** to
`0x142cc9460` is still open, but the solver body itself is now verified as
`fn/ClosestPointGjk` (264 cases, every channel of its `0x40`-byte output, 74.5%
branch coverage, 12/12 mutants); types 2 and 4+ are read but not implemented,
and type 3 is `fn/MeshSweep`.

**The next read is the geometry, not more query code.** The five per-type
primitives are inventoried with their sizes and their two-path shape
(`formats/character-controller.md`), and the shape world transform
`0x142cb8b80` is read **and verified** as `fn/ShapeWorldTransform`
(`ow_sim::collide::shape_world_transform`: 328 cases, every channel, coverage
100%, 9/9 mutants). The narrowphase wrapper `0x142cc1610` is read **and
verified** too, as `fn/ShapeNarrowphase` (`ow_sim::collide::shape_narrowphase`:
224 cases, every channel, coverage 100%, 11/12 mutants); its record layout, its
`context+8` inline filter and its dispatch table are in
`formats/shape-narrowphase.md`. The dispatch label `0x142cb8d60` is a real
entry point inside the transform's extent and takes a **fifth** argument (the
query block), which the earlier note's four-argument reading missed. What a
real candidate still needs is
(a) the collision geometry the query walks (`record+0x2b8` entries and the
per-type pools at `+0x2d0..+0x330`), which is the map's collision chunks,
(b) the hit policy's live collision world (`0x14202c580` -> `0x14202bc90`).
The type-3 composition question is **closed**: there is no single mesh-world
pointer. The `0x50`-byte element is `{scale[4] at +0x00, 1/scale[4] at `+0x10`,
root at `+0x20`, vertices at `+0x28`, triangles at `+0x30`}` with absolute
pointers, so the world is the address space they live in, and the arm is now
exercised by `fn/ShapeNarrowphase`'s corpus (6 type-3 hits). `block+0x70` is the
`u16` mask each triangle's `+0x1c` is tested against, which every earlier
fixture left zero. Types 2 (`0x142cd3c80`) and 4+ (`0x142cd0ae0`) are now
**read** (2026-11-07) but not implemented and not differenced: type 2 is a
convex hull handed over as face planes with an analytic plane-entry walk, a
`tmin`/`tmax` reject and a GJK fallback; type 4+ is a single kind, a
height-field grid walked by a rectangle scan plus a 2D DDA over cells. Layouts
and algorithms are in `formats/shape-narrowphase.md`; the working note is
`dev/data/notes/narrowphase_types_2_4.md`. The practice range is entirely
type 3, so those two do not block it, and `Narrowphase::UnreadType` stays the
honest answer until each has a differential unit.
Without those,
a candidate cannot be scored: the fan reads a record only the live hit policy
fills. So the next anchor is the chunk ingest in `formats/maps.md`, coupled
with the primitive bodies.

**The node-type table is enumerated (2026-11-02).** The "walk the 100 thunks"
plan was blocked by a project defect: the registry add `0x141c5ff40` had no
function, so all of its callers read as "0 callers". It is
`RegisterNodeType` (52 bytes) and has **144** call sites; creating it in the
project image made all 144 visible at once. The type table is **static data**
0x28 bytes apart from `0x143b8e5c8` (`{next, u16 type, class_registrar,
context}`, linked through head `0x144188170`), so it can be read from the
image without disassembly; `dev/probe_node_types.py` does exactly that. Each
class registrar obfuscates its own name (`name[i] ^= key[i & 7]`, key at
`len-8`), but the strings are generic — type `0x13` decodes to `"Unknown"` —
so **no name identifies the collision node**. The remaining discriminator is
the class size handed to `RegisterTypeWithSize` and the class vtable's build
slot: the chunk build must fill the `0x30`-byte type-3 mesh source from the
cell's quantized vertices (`arr13`/`arr14`) and its `0xFF`-separated index
runs (`arr3`/`arr11`/`arr12`) without calling `BrushTreeToMesh`.

**The pool side is read (2026-11-01).** The fixture/pool write path is now
whole: `DmWorldConstruct` sizes every pool from the world config
(`config+0x20` bodies, `+0x24` owners, `+0x28` shape entries, `+0x2c..+0x3c`
the five shape-type pools, `+0x70` islands); `ShapePoolAlloc` (`0x142cb00c0`)
is the per-element free-list pop (it takes the *pool record's* address, which
is why the earlier `+0x2cc`/`+0xa0` intersection failed);
`ShapeEntryFromDescriptor` (`0x142cb7f10`) copies a source descriptor into a
0xc0-byte shape entry and allocates the per-type record by the source's type
byte at `+0x67`; and `ShapeAddToBody` (`0x142cb3940`) / `ShapeAddToOwner`
(`0x142cd4ef0`) are the two "add a shape to a world" handles. So the only
missing Data step is the **descriptor producer**: no function yet found turns
the collision chunk's sector data into those descriptors. The chunk payload
itself is read as data (`formats/maps.md`, "The collision chunk payload"):
two STUD v1 blobs, a registered 0x78-byte component class `0x9199BF6B` with
three RVA fields into a 220-sector table, and a second, unregistered component
`0x99F089FE`. The next anchor is exactly that converter, plus the two open
narrowphases (types 2 and 4; the shared support-point solver `0x142cc9460` is
now `fn/ClosestPointGjk`).

**Two routes were tried and settled (2026-11-01).** (a) *Field hashes:* the
class descriptor's field table (`desc+0x28 = 0x143c3c620`, count 4 at
`desc+0x98`) is **all zero in the installed image**, and `0x1421cdb90` only
sorts it (`0x1421cce50`); the registered class name decodes to **`Unknown`**
(the XOR name format is recorded in `formats/maps.md`). So the four field
hashes are built at runtime and a search for hash immediates cannot find the
consumer. (b) *Geometry instantiation:* `0x14202c660` is the **owner's**
initialise step (`owner+0x620` = the 0x3210-byte geometry, two worlds from
`config[0x18]`), and its class is reached through the node-type registry:
`FUN_1404f6e10` registers `FUN_142029f00` as node type 2 in `0x141c5ff40`;
that factory builds a 0x30-byte type descriptor (vtable `0x1433654a0`) and
registers a 0x910-byte class via `0x141c4b050` → `TypeRegistrationCore`. The
descriptor's slot 2 (`0x1407803f0`) is a **release**, not an instantiate (round
12 below): it returns the instance to the allocation strategy, so the earlier
"key that selects the geometry class" is withdrawn. The creation is slot 1
(alloc) followed by the node's slot 8 with the record; the next read is the
asset-preload service's `vtable+0x38` (the all-preloads-done callback). The
consumer reads the fixed-up component offsets `+0x10`/`+0x58`/`+0x68` directly,
so it will not appear in a hash search either.

**The STU load path is read (2026-11-01), and it explains the fixup**
(`formats/stu.md`, "The load path"): `FUN_1421d26d0` resolves each sub-record
by class hash (`FUN_1421c14f0`), constructs it in place, and runs
`FUN_1421d3c10`/`FUN_1421d3710`, which convert self-relative offsets by the
class's **field types** (plain offset, array of offsets, array of pointers,
nested instance, hash map, string). With the shipping build's field tables
empty, nothing is converted generically — which is why the collision chunk's
completion calls its own fixed-up pass `FUN_141ff51a0`: **17 self-relative
qwords per `m_cellBytes` record**.

**The cell container is decoded and its consumer chain is read (2026-11-01).**
`formats/maps.md`, "The cell container, decoded": the `+0x68` field points at
a `{count, pad, data_off}` header; the 220 entries are `{cell_off, 0}`; each
cell is `{block_size, record_off}`; each record is 17 self-relative qword
offsets plus a 24-byte header, and the 17 arrays tile `record..record+
block_size`. All 220 validate and tile the payload with no gaps. The runtime
array encoding was **observed** under the oracle (`dev/probe_cellbytes.py`):
mode 1 = `{entries_ptr, u32 count}`, entries `{Q, 1}`, record `*Q`; mode 2 =
`{u32 count, pad, entries_ptr}`, entries `{Q, mode}`, record `*Q` when mode is
1 else `*(Q+8)`; offset 0 → null. The consumer chain runs
`GeometryFinalize` (`0x14213d0b0`) → `OwnerCreateBody` (`0x142cc3d90`) →
`ShapeAddToBody`, one body per `0x100`-byte runtime cell entry and one shape
per `0x40`-stride matrix in it.

**The container stage is implemented and the fixup is verified (2026-11-02).**
`ow_tank::collision::parse` reads the chunk's first `STUD` blob, the
`0x78`-byte `0x9199BF6B` component, the `m_cellBytes` header, the 220 cells and
each cell's 17 array slices, and checks that every record and boundary stays
inside the payload and that the cells tile it. The install test
`crates/ow-tank/tests/collision_cells_real.rs` pins the count, the first and
last cells and every array extent. `ow_tank::collision::fixup_cell_bytes` is
`0x141ff51a0` operation for operation; `fn/MapChunkCellBytesFixup` (264
iterations + edge pool, 12/12 mutants) verifies outer modes 1 and 2/3 and entry
modes 1 and 2/3, and declares the two allocate-then-deref-null mode-0 branches
dead. The **cell decoder** — who turns the 17 arrays into the `0x60` cell
entries (or the `0x100` sector entries) — is still the open link, so the
container is not yet a collider.

**The consumer's contract is read; the array overlay is excluded (2026-11-02).**
`GeometryIngestCandidateB` (`0x14213528a`) shows the decoder's output form: a
`0x30`-byte mesh source holding **three `{pointer, count}` pairs** whose arrays
hold **`0x60`-byte records** (copied by `func_141b53b70` at a `0x60` stride), one
body per entry and one type-3 shape per record. Measuring the practice range's
220 cells excludes the cheap reading: no array is a fixed-stride view (`arr7`
divides by `0x60` in 44/220 cells, `arr12` in 14, `arr14` in 22), and every
array is 4-byte granular. So the records are **built**, as on the brush path.
Full measurement in `formats/maps.md`.

**The brush ingest is read; the cell path is not that array (2026-11-02).**
`AppendCollisionEntryFromBrushSource` (`0x14213a510`) is read end to end: it
snapshots a brush source (`geometry+0xb0`, stride `0xb0`), meshes it with
`BrushTreeToMesh`, applies a leaf-or-chord-scale area test, and appends a `0x60`
entry (`MeshToColliderFaces` into `entry+0x20`, per-face stamp writes, handle to
`geometry+0xf8`). The geometry object's vectors are `{data, u32 capacity, u32
count}` headers at `+0xf8`, `+0x108`, `+0x118`, `+0x128`, `+0x138`. A whole-image
scan of stores to the entry count `+0x114` finds **three** sites: the brush
ingest, a clear, and the build's reset. **No cell code appends a `0x60` entry.**
The `+0x118` vector is not an alternative either: its append helper
`FUN_1421360a0` builds **`0x100`-byte** elements (`capacity << 8`), and that
helper has exactly **one caller** — the brush ingest. So this whole node class is
brush-only, and **the chunk's cells belong to a different node class**. Next
anchor: the class that owns the chunk component the bytes-fixup writes
(`sub+0x68` for slot `0x1e`, `sub+0x08` for slot `0x22`).

Two candidates are now excluded rather than pending. `FUN_1420c63b0` (10 500
bytes, four `ShapeAddToBody` sites) has **no code caller** and no plain pointer;
its three data xrefs are internal switch tables whose neighbouring words point
inside the same function, and its only call xref is a self-recursion. And the
sub-asset resolver `0x1421d26d0` has just **14 call sites** image-wide, only one
of which adds `0x68` (the chunk completion itself), so the consumer does not
re-resolve the collision sub-asset that way — it must be handed the cells from
the chunk work record (`work+0x60`, the payload the completion stores).

Note for the next reader: three callees were falsely marked `noreturn`
(`0x141bb00e0`, `0x142ced8f0`, `0x142efe2b0`), which stored this function as 167
bytes instead of 2011 and made several live addresses read as "no function".
The mechanical audit recipe is in `ghidra-project.md`.

**The collider build chain is verified except its two ends (2026-11-02).**
Read and proven this session: `MeshToColliderFaces` (`0x142cf2010`, the
double-sided shell), `ColliderFaceAppend` (`0x142cca460`, the `0x60`-byte face
records), the triangle skip test (`0x142cd56e0`, `fn/TriangleDegenerate`), the
BVH builder (`0x142ccb1b0`, `fn/BuildBvh`) and the type-3 narrowphase (earlier,
`fn/MeshSweep`). The builder's split rule had been mis-recorded: it bins the
**centroid** spread over 64 buckets on the widest axis and picks the cheapest
`area(left)*count(left) + area(right)*count(right)` boundary, taking the array
half only when every centroid extent is below `0.005`; the partition is the
swap pass at `0x142ccbc17`, not a stable shift. Still open on this chain: the
appender's weld bookkeeping (its neighbour links have no reader), the
flattening `0x142ccad80` (the node order and padding the implementation still
mirrors rather than reads) and `ColliderMeshEmit` (`0x142ccc120`).

**The cell entry is 0x60 bytes and the consumer is exact (2026-11-01).**
`GeometryClear` (`0x1421384e0`) fixes the geometry object's arrays: `+0xf8`/
`+0x104` 0x50-stride transforms, `+0x108`/`+0x114` **0x60-stride cell
entries**, `+0x118`/`+0x124` 0x100-stride entries, `+0xb0`/`+0xc0` and
`+0xc8`/`+0xd8` 0xb0 record pools. `GeometryFinalize` copies each 0x60 entry's
five `vec4`s into a 0xb0 record, steals its `{ptr,count,flag}` u16 array from
`+0x50/+0x58/+0x5c`, creates one body (`OwnerCreateBody`) and one **type-3
mesh** shape per entry (`ShapeAddToBody`, mesh source = record+0x20, scale
1.0, type byte at +0x67), and stores the shape handle at record+0x50.

**The node-type-2 path is brush-built, not chunk-built (2026-11-01).**
`GeometryBuild` (`0x14213c440`) reads the node's model
(`node+0x88` -> `+0x38` -> `+0x40` -> `+0x48`, a brush tree), copies the
collision params (`0x142132cf0`), allocates one 0x60 cell entry, builds its
transform, converts the brush tree (`0x142cf23c0`) and thickens it
(`0x142cf2010`), then creates the body and finalises. So the 220-cell STU
chunk has a **different** consumer: another node type. The registry is now
enumerated (2026-11-02): `0x141c5ff40` is a linked-list add to the global head
`0x144188170`, and its 144 caller thunks each pass a `{u16 type; factory}`
record. The types run `1..=0xb4` with holes; the factories are the
`NodeTypeNFactory` functions (type 1 class size `0x300`, type 2 `0x910`, type 3
`0x5c0`). No factory has yet been matched to the collision chunk; the next
attempt can start from that full (type, factory) table instead of the size
guess.

**The 0x30-byte mesh source is the decoder's output form (2026-11-02).**
`MeshShapeFill` (`0x142cae540`) copies it; a 0x60 cell entry holds it at
`+0x20` (`GeometryFinalize` passes `record+0x20`); and the brush path fills it
with `MeshToColliderFaces` (`0x142cf2010`), whose only two callers are
`GeometryBuild` (`0x14213c440`, at `0x14213c9d7`) and the builder that runs
from `0x14213a995`. So whatever the chunk decoder is, its output is
`{u64 nodes; u64 second; u64 third; u32 count; 5 floats}` handed to
`ShapeAddToBody`'s type-3 branch. The remaining unknown is the **BVH builder**
for mesh nodes (the narrowphase walks 0x20-byte nodes with the layout in
`crates/ow-sim/src/collide.rs`); find its caller and the decoder is one stack
up. `BrushTreeToMesh` (`0x142cf23c0`) has exactly four callers
(`0x14213a7ab`, `0x14213af82`, `0x14213c2c8`, `0x14213c9c0`), all brush paths,
so the chunk path does not go through the brush tree.

**The type-3 shape's producers are mapped (2026-11-02).** `MeshShapeFill`
(`0x142cae540`) has exactly five call sites: `ShapeEntryFromDescriptor`
(`0x142cb7f10`, from `0x142cb80d4`) — the generic descriptor-to-shape route,
itself reached only through two data tables (`0x14473d368`, `0x143829a20`) and
two calls, so nothing in that chain names the decoder; `FUN_14210bac0` (from
`0x14210bc96`), which divides the filled vertices by a scale; and **three sites
in code Ghidra never defined** (`0x140c28288`, `0x14207879f`, `0x1420a06eb`).
`GeometryFinalize` and `OwnerCreateBody` likewise have no code references: they
are reached indirectly. The chunk decoder therefore cannot be found by walking
callers from the finalize side; it sits in an undefined region and the next
attempt has to read it as raw disassembly (`dev/data/read/d.py`). Two of the
three sites are already informative: `0x140c28288` stores the new shape record
at `owner+0x50` and increments the counter at `owner+0xcd0`; `0x14207879f`
fills a **0x58-stride** record array — `record+0x50..+0x5c` from
`[src+8/+0xc/+0x10/+0x14]`, mesh source passed as `base + 0x20 + i*0x58` — so a
second, 0x58-byte mesh-bearing record shape exists beside the 0x60 cell entry,
with its vertices at `+0x20` just like the cell entry.

**Chunk kind dispatch is not the collision chunk (2026-11-02).** `0x140c66540`
switches on a chunk kind byte at `[chunk+0x13]`: kind `8` calls `[vtable+0x78]`
then `0x142076ab0`; kind `0x1a` calls `[vtable+0x88]`, copies a quad returned by
`0x141c5b950` to `payload+0x2040`, then `0x142075d00`, which stages three
records (kinds `1`, `0x15`, `0x40`) into the object at `+0x88`/`+0x2a8`/`+0x808`
under a hashed key (`func_0x141b32e50`). Those are placeable/spawn records, so
kind `0x1a` is a content chunk, not the 220-cell collision chunk.

**The collider surface is a shell, and the emitter is the last unread link
(2026-11-02).** `MeshToColliderFaces` (`0x142cf2010`) is read in full (detail in
`dev/data/read/decoder-notes.md`): it allocates an index array and a vertex
array, writes the vertices twice (`v + epsilon`, then `v - epsilon`), emits two
index records per face with the index order reversed in the second half, and in
its flag mode adds side-wall record pairs along boundary edges; then it calls
`ColliderMeshEmit` (`0x142ccc120`), which applies `ColliderTriangleDegenerate`
(`0x142cd56e0`, verified) before appending each triangle, and frees both
arrays. So a brush collider is a closed double-sided shell around the brush
plane, not the plane itself. `0x142ccc120` is now the next unit to read: it
holds the face append (`ColliderFaceAppend`, `0x142cca460`) and the pool A
cell writes.

**Search map for the decoder (2026-11-02), so the dead ends are not repeated.**
The chunk asset's completion path only uses `ChunkSlotForGuid` to pick the
fixup branch (its two call sites are `FinishMapChunkAsset` and the slot-8
walker), so the chunk kind is **not** a geometry selector. The asset
class-handler registry (`0x1421bc930`, keyed by `[class+0xb8]`, backing store
`0x143aa51b8`) is the level streamer: its only registered handler
(`0x1421a45b0`) handles asset types `0x0F` and `0x19` and calls
`FUN_14219aab0`/`FUN_14219cd50` for the map asset's dependency list, not the
collision cells. The class hash `0x9199BF6B` occurs in executable memory only
at the registration site; no consumer compares it. Node type 1 (`0x141ff9750`,
class `0x300`) is a level/map node (its constructor is `0x141ff90a0`, its size
virtual `0x141ff90d0`), type 2 is the `0x910` geometry. The geometry class's
method table at `0x143395050` has `GeometryBuild` at slot `0x80`; the sibling
slots (`0x14213a71a`, `0x14213bbe6`, `0x14213b995`, `0x14213b8bd`,
`0x14213bfd0`, and the 0x100-entry finaliser `0x142135030`) are the next
candidates, together with the writer of `geometry+0x118`/`+0x124`. The 0x100
entry itself is read: body config at `+0x50`/`+0x58`, a `0x40`-stride matrix
array at `+0x30` (count `+0x38`), the mesh source at `+0x18`/`+0x20` (three
vec4s from the 0x60 entry), a stolen u16 array at `+0xe0`/`+0xe8`/`+0xec`, and
a transform/terrain flag at `+0x60`/`+0x70..0x9c`. The 17 cell arrays
(`arr3`/`arr11` u16 runs, `arr12` interleaved, `arr13`/`arr14` 4-byte
quantized vertices) are the likely mesh source; the connection between them
and the 0x100 entry is exactly what is not read.

**Old note, superseded:** the writer of `geometry+0x108`/`+0x114`.  Its direct
caller is the unanalyzed geometry builder that runs from `0x14213bf60` (the
region before `GeometryFinalize`; no function boundary exists there yet). Read
that region with `create_function`/disassembly after the `Alloc` analysis fix
(`docs/ghidra-project.md`), then follow its caller to the collision-component
consumer. The 0x60 entry's own layout is fixed: five `vec4`s at `+0x00`..
`+0x50`, u16 array pointer at `+0x50`, count at `+0x58`, flag at `+0x5c`, id
at `+0x0a`.

**Three search routes are now closed (2026-11-01)**, so they need not be
retried: `imul reg, reg, 0xc0` finds nothing in the collision module, a raw
disp32 scan for the pool displacements is worthless (they are ordinary stack
offsets), and the image contains no `COLLISION`/`ADDITIVE` strings — those
names are the install data's, not the client's. What *is* read is
`DmBroadphaseReinit` (`0x142cbdc60`): it is a BVH builder over the fixture set,
rebuilt on demand, writing each pass's root index at `+0x44` through a
`0x100`-entry stack node pool and calling the verified `0x142cd5540` for the
swept bounds. The tree is therefore a function of the fixtures, and the single
remaining unknown is one function: **who fills `[record+0x2b8]`'s fixtures.**

**The world and its pools are read; the fixture *pop* is not.**
`0x142cc3c80` creates a world (0x4d0-byte header + eight strided pools, handle
encoded into `0x1442a0bc0`, self-index at `record+0x194`), the pool table is
`record+0x288 + 0x18*i`, and the fixture record's `+0xa0` dword doubles as the
free-list link. Release and update/insert are read
(`0x142cb0d08`/`0x142cd4555`, `0x142cd4610`), and the fill/publish side is
`0x141d71200`. What is missing is the free-list pop, and it cannot be found by
displacement scans: no `inc`/`add` of `record+0x2c0` exists in the image even
though release decrements it. Next: `0x141d71200`'s head and the caller of its
vtable slot `+0x40` (`0x142f744a0`).

**The geometry side is read; the chunk side is not (2026-11-02).** The `0x60`
cell entries and the `0x100` sector entries are written by one driver at
`0x14213bbc0` (vtable slot `+0x78`), which walks pool A (container at `GEO+0xb0`,
stride `0xb0`, count `+0xc0`) and pool B (`GEO+0xc8`, count `+0xd8`) and calls
builder #1 `0x14213a510(GEO, index, desc, force, out)` at `0x14213bd61` and
builder #2 `0x14213acf0` at `0x14213beaf`. A pool A record already holds
decoded geometry — vertex array at `+0x00` (`0x20`-byte elements), triangle
array at `+0x08` (`0x28`-byte elements), counts at `+0x10/+0x14/+0x18` — which
builder #1 copies into a mesh record (allocations `count*0x20` and
`count*0x28`), converts with `BrushTreeToMesh` (`0x142cf23c0`), then pushes a
cell: `+0x00..0x1f` two `vec4` from the mesh record, `+0x20` the `0x30`-byte
mesh source via `MeshToColliderFaces` (`0x142cf2010`), the `(GEO+0xa8, GEO+0xaa)`
index pair into every triangle at `[cell+0x30] + k*0x20 + 0x18/+0x1a`, and
`+0x50/+0x58/+0x5c` the `{u16* list, count, flag}` face list from
`0x142139080`. Sectors are filled the same way with a transform, a distance and
a limit. `GeometryFinalize` (`0x14213d0b0`) then walks `GEO+0x108`
(stride `0x60`, count `+0x114`), skips the first entry, copies five `vec4` into
a fresh `0xb0` record, takes over the face list and creates one body plus one
type-3 mesh shape. The brush path (`0x14213c440`) shares those cell-fill steps
and then calls `OwnerCreateBody` and `GeometryFinalize` itself. What is still
open is the **input**: no code in the geometry class reads `chunk+0x2428`, so
the function that fills the two record pools from the 17 fixed-up arrays
remains unidentified; the record layout it must produce is now known exactly
(section 4.1 of `dev/data/read/decoder-notes.md`, which also holds the search
map and every dead end).

**Round 2: the chunk container is not the collision source for movement
(2026-11-02).** Two facts changed the picture. (a) The pool A records — the
only input of the cell/sector builders — are created by a **primitive
importer** at `0x1421363d1` (pool `push_back` `0x142136130`, grow path
`0x142136160`), which fills a record's `+0x00/+0x08/+0x10/+0x14` through
`0x14213a6aa` (reached only from `0x14213ad6b`, inside builder #2). Its inputs
are a local `0x30`-byte mesh descriptor at `+0x20` (via `0x14212c210` ->
`0x141bce330`, element stride `0x2c`, counts `+0x14`/`+0x18`, physics index
`+0x28`), a rig node chosen from `[GEO+0x88]` by the record's `+0x70` slot
index, and two rb-trees (`0x142cbb020`/`0x142cba920` for triangles, stride
`0x4c`, first dword = count; `0x142cbb300`/`0x142cbacc0` for vertices, stride
`0x34`). Its output form: vertices `0x20` stride = 3 f32 position + 2 f32
(halved) + 4 f32 at `+0x18`; triangles `0x28` stride = 3x u16 at `+0x00` plus
3x u16 physics indices at `+0x06`. The source object is class type **9**, not
the type 10 the driver validates. (b) The 17-array container's only in-image
contact is its fixup (`0x141ff51a0`, single caller `0x141ff57f7`) plus a
type registration at `0x1400fdcb0` (class hash `0x9199bf6b`, record size
`0x78`, alignment `8`, **array element `0x50`**, eight stub vtable slots and
empty name strings, registered through `0x1421cd360`). No geometry-class code
reads `chunk+0x2428`. So the runtime colliders that `fn/MeshSweep` queries are
built from **brush/model primitives**, and the 220-cell container is not on
that path. **Consequence for movement:** the read chain that matters is the
primitive side — `0x142cec9e0` / `0x142ced620` / `0x142ced990` / `0x142ceca00`
(brush tree build), `BrushTreeToMesh` (`0x142cf23c0`), `MeshToColliderFaces`
(`0x142cf2010`), the pool A importer `0x1421363d1`, then `GeometryFinalize`.
None of those four is read yet, and the 17-array container's consumer stays an
open stage per the hard rule.

**Round 3: the cell consumer (2026-11-02, census corrected).** The first version
of this paragraph said the component's class was "streamed, never queried" and
that the decoder was open "by absence". That conclusion is **withdrawn**: it
rested on Ghidra's caller metadata, which is incomplete for this image
(`ghidra-project.md`: `.pdata` entries chain and fragment, and undefined
regions hide call sites). Ghidra reported 3 callers for `ShapeAddToBody`
(`0x142cb3940`) where a verified `E8 rel32` scan finds **27** sites in 13
functions, and the "two functions read `+0x10`/`+0x58`/`+0x68` and call both
builders" claim was simply wrong (`0x14208d010`/`0x142090e00` call neither;
`0x14208d109`/`0x1420919b7` hold those sites and their `+0x58`/`+0x68`
operands are `f32` radii, not 16-byte array headers).

What the corrected census *does* establish: for `ShapeAddToBody`,
`OwnerCreateBody`, `ShapeAddToOwner`, `ShapeEntryFromDescriptor` and
`MeshShapeFill` the union of Ghidra's callers and the verified scan is 28
producing functions (27/17/1/2/5 sites), and **none of them calls
`StuLoadSubAssets`, none loads a qword from `+0x60` (the chunk work record's
payload field), and none reads the component's `{pointer, mode}` headers** —
the only qword read at `+0x68` is `ShapeEntryFromDescriptor`'s own descriptor
field. The whole-copy route is checked too: image-wide there is exactly one
`memcpy(…, 0x78)` among 8 923 `memcpy`/`memmove` call sites
(`0x142402824` in `FUN_142401de0`) and it copies a diagnostic string fragment.
Table and evidence: `formats/maps.md`, "The cell path's consumer: open". The
byte-level fact that stands is narrow: the `STUD` magic `0x53545544` is
mentioned only by the loader `StuLoadSubAssets` (`0x1421d26d0`) and the writer
`FUN_1421d2ab0`.

So the decoder is **open**: a reader that builds no shape through those five
handles, or one reached through a route this census does not enumerate, is not
excluded. The three anchors named here were then read (2026-11-02): `0x1420c63b0`
consumes a `0x38`-stride node/transform record array at `[owner+0xd0]` plus a
`0x20`-stride transform-handle array at `[owner+0xe0]`; `0x140c2813f` fills its
mesh from a **brush collider** (`ColliderMeshEmit`); `0x142078562` computes a
box from a **stored mesh source's bounds** (`0x142cae600`) into a `0x58`-stride
record; `0x1420a02f6` is a **mesh-source assembler** over the owner's
descriptor container at `+0x140`/`+0x148`, indexing `0x60`- and `0x40`-stride
arrays; and `0x142cdf9f2` appends `0x40`-byte entries to the **Domino owner's
body vector** and calls the handle setter `0x142cb4f20`. None of the five
touches a chunk payload or a `0x9199BF6B` field, and **none is reachable**: no
`E8`/`E9`, no immediate, no 8-byte pointer, and no obfuscated-pointer encoding
of any of their addresses exists in the image — their only references are the
`.pdata`-copy and code-range tables that every function start has
(`ShapeAddToBody` carries one of those too, beside its 27 call sites). So the
chunk path is neither opened nor closed by them; the single next reader is the
**producer of the owner's mesh-descriptor container at `+0x140`/`+0x148`**
(`formats/maps.md`, "The three next readers").

**Round 4: the brush route, and the chunk labels (2026-11-02).** The second
producer is now read from the node down and implemented where the reads allow:

* `GeometryBuild` (`0x14213c440`) resolves the node's model
  (`geometry+0x80` → `[node+0x38]` → `[+0x40]` → `[+0x48]` = `C`, the model's
  collision descriptor) and builds the brush **at runtime** from `C`'s instance
  points; the four "leaf builders" are not a loader: `0x142cec9e0` header init,
  `0x142ceca00` mesh-struct init, `0x142ced620` the ring builder (1/400 units,
  mask `{1,1,0,0}`, `roundf`), `0x142ced990` the edge list, `0x142ced510` the
  vertex decoder.
* Records read and implemented in `ow_tank::brush` (11 units): brush `0x20`,
  vertex `0x20` `{i64 x, i64 y, i64 divisor, u32 index}` with
  `position = (x/divisor) * 0.0025`, node `0x28`, ring walk `+0x04`, next ring
  `+0x10`. Every arithmetic expectation is the client's own output:
  `dev/brush_vertex_oracle.py` executes `0x142ced510` and `0x142ced620` under
  the emulator (bump-allocator stub at the hook word `0x1442a0158`) and the
  tests pin those bits.
* `BrushTreeToMesh` (`0x142cf23c0`) is read to its per-node **six-op**
  classification; the op bodies and the triangle emission are **open**, so no
  brush→mesh conversion is implemented. `MeshToColliderFaces` (`0x142cf2010`)
  is read and already implemented (`ow_sim::collider::shell`).
* **The install has no brush asset.** The content manifests this install opens
  carry **0** records of GUID type `0x00A` (1,425,794 records, 177 types;
  1,514,352 with `WinPrism_` forced), while `0x00C`/`0x00D` are populated;
  300 model assets' chunk tags are `MDLC/MRNM/MSTU/MESD/mskl/CLTH`, no brush
  container. Pinned by `crates/ow-tank/tests/brush_real.rs`. So the format is
  read but no instance of it is reachable from this install yet: `C`'s class
  and the `+0x98`/`+0xa0` inline-singleton-vs-array encoding are the next read,
  together with whether type-`0x00A` assets are synthesised.

**The chunk labels are read now, and they were inverted (2026-11-02).** The
kind table `0x1439aa320` and its name decoder `0x1404c19b0` were read (length =
low 24 bits of the dword at `name-4`, key = eight bytes at `name-0xC`, byte `i`
XORed with `key[i & 7]`; all 36 names decode, and the eight placeable names the
project already knew come out exactly). The answer:

| kind | slot | name |
|---|---|---|
| `0x11` | `0x1D` | `MAP_CHUNK_COLLISION` |
| `0x12` | `0x1E` | `MAP_CHUNK_PATHING` |
| `0x10` | `0x21` | `teMAP_CHUNK_ADDITIVE_COLLISION` |
| `0x23` | `0x22` | `teMAP_CHUNK_ADDITIVE_PATHING` |

The per-slot dispatch agrees: the two collision kinds are the two sent to
`FUN_142e492a0` (slots `0x1D`/`0x21`), the two pathing kinds are the two sent
to `StuLoadSubAssets` + `MapChunkCellBytesFixup` (`0x1E`/`0x22`). So the
220-cell `STU_9199BF6B` container this project spent two sessions on is the
**pathing** chunk, which explains why the 28-function producer census found no
reader for it: a client-side pathing consumer need not exist. The container and
fixup reads stand; only the label was wrong.

The **collision** chunk is `0x0DD0001100000688` (21,858,688 bytes). Its
container is read and implemented (`ow-tank::chunkkind`, 7 tests):
header `count_a` at `+0x08` (36) and `count_b` at `+0x0C` (1), array A
`0x58`-stride at `+0x20` (`0x14D3810`), array B `0x38`-stride at `+0x28`
(`0x14D8948`), the in-place fixup `FUN_142e492a0` (header null-preserving,
element qwords unconditional), and a per-sector tiling over four runs
(`12 B × c0`, `1 B × c1`, `16 B × c1`, `4 B × c2`, counts at
`+0x2C`/`+0x30`/`+0x34`) that holds for all 36 sectors.

**The collision consumer is the open link, and it is now specific: who fills
the Domino shape-entry pool at `[record+0x2b8]`.** The dispatch only fixes the
payload up, the handoff `FUN_141fe37a0` stores it at `work+0x60`, the fixup is
called only from `FinishMapChunkAsset`, and no function in the producer census
sees a sector array. One earlier claim needs correcting: `BuildBvh`
(`0x142ccb1b0`) is **not** callerless — a verified `E8` scan finds five call
sites in three functions (`0x142cca7d9`, `0x142ccb2db`, `0x142ccbc1c`), so the
collider builder is reachable and its callers are candidates.

Full detail: `formats/chunk-kinds.md`, `formats/maps.md` ("Which chunk kind is
which: read", "The pathing container's consumer: open") and
`formats/brush-collision.md`.

**Round 5: the shape pools are enumerated (2026-11-02).** The world record's
pool table is read (`DmWorldConstruct 0x142cbeb70`: eight records at
`+0x288 + 0x18*i`, `{ptr, used, cap, high, free}`; shape entries are index 2 →
`+0x2b8`, element `0xc0`). A verified store census (119,802 decoded function
groups, with the `-1` free-head sentinel as the discriminator) finds the only
writers are construct, clear and reset; the pool **contents** can only be
created through `ShapeAddToBody`/`ShapeAddToOwner` → `ShapeEntryFromDescriptor`.
Every caller of those that appends collision material is a brush-mesh producer
(`0x142136010` one caller `GeometryBuild`; `0x1421360a0` two, both brush;
`BrushTreeToMesh`'s four callers are the four appenders). So the collision
chunk is **not** the Domino shape source; the only loophole is an inline
appender that no decoded store shows. `BuildBvh`'s three callers are split
wrappers over the collider-face array and all unreachable; "a sector reader must
end in a BVH" is false.

With the chunk excluded, the live producer is the **brush**, and its
tree→mesh walk is now read and implemented: the hull stage, the status tree
(insert fixup, erase + rebalance, both rotations), the six ops, the ear
predicate from the `SHUFPS` immediates, the clipping tail and the face writer
are in `ow_tank::brush_mesh` / `brush_convert` (25 tests). The client's own
code was run end to end under the emulator and **all four** pinned meshes
reproduce bit for bit, half-edge marks included. The arrow's divergence was
one misread link, not a tag strictness: ops 1, **2** and 5 all read the `+0x28`
link, and the port had 2 and 5 on `+0x30` — invisible until an op splices,
which the arrow's op 2 does. Found by single-stepping the client's tag pass and
hooking the tree calls; the collinearity hypothesis was wrong. Faults are
refused, never approximated: a clockwise ring and a two-node ring (both at
`0x142cf265f`), the two-ring allocation at `0x142cf0b6b`, and the `+0x58 == 0`
variant.

`C`'s instance encoding is read (`+0xa0 == 1` ⇒ `{points, count}`, otherwise
`{count, points}`); **its class is open**, and the census now gives a
structural negative rather than a lead: with disp8 scanned, **no function in
the image stores both `+0x186` and `+0x98`**, and the function that writes a
near-perfect match for `GeometryCopyParams`' field set
(`0x14208edf6..0x14209087f`) has **no static reference at all** — no call, no
tail jump, no pointer — so its "builds `C`" reading is downgraded. The lesson
is recorded in `formats/brush-mesh.md`: a displacement is not a type.

The chain was then walked backwards (2026-11-02): the geometry is created by
the node-type factory `FUN_1420d0ef0` (two classes on `[node+0x29]`),
`geometry+0x80` is written by the class's own vtable slot `+0x08`
(`0x142138ce0`), and the node-type-2 update (`0x14204cbce..0x14204cfc9`) moves
`[node+0xe8]` into `[node+0x38]` and reads the three links exactly as
`GeometryBuild` does. The writers of `[node+0xe8]`, `[object+0x40]` and
`[instance+0x48]` were **not found**: three probes came back negative (the
field-set fingerprint `{+0x28,+0x29,+0xe8}` hits only a hash-table relocator; the
"calls the GUID resolver ∩ stores `[reg+0xe8]`" intersection is empty; no
function in the node-type module writes `+0xe8`). So `C`'s class, the `+0xa0`
encoding value and an install brush remain open.

Correction on the way: `0x14208edf6` was called reachable through an RVA table,
but the `0x14469xxxx` records are **`.pdata` copies**, not dispatches; the only
computed jump is the switch at `0x14208e74e` that selects fragments of one
function body. That is a fragment dispatch, not an external call, so the
reachability claim is withdrawn.

**`C` is named by type identity (2026-11-02).** The release path showed that
`[node+0x38]`/`[node+0x40]` are refcounted resources whose class is only a
runtime vtable (`0x141b251d0` calls vtable slot `+0x18`), so the class came
from the registration direction instead:

* **`C` = registered class `0x564B1018`, size `0x198`, vtable `0x1431dd118`** —
  the client's own registration `0x1402db070` passes `r8d = 0x564B1018` and
  `size 0x198` to `0x1421cd360`, and 22 of the class's 36 declared field hashes
  are immediates in the client's two field-dispatch functions
  (`0x141334ea0`/`0x141335e60`); `0x077CF4AA` (`+0x186`), `0x88FCECD7` (the
  array at `+0x98`) and `0x06A1888C` (the inline sub-object at `+0x90`) all
  appear in the client's own code. OWLib's registry was used only as a check
  (size and the unique `+0x186` field), not as the source.
* `C+0x98` is a pointer to a 16-byte array header, `C+0xa0` a whole-qword tag,
  `1` selecting `{points, count}` and anything else `{count, points}`; the array
  is a `Vec3A[]` of the inline sub-object at `C+0x90` — the model's instance
  points. Implemented in `ow_tank::model_collision` (5 tests).
* `+0x48` is an **asset reference**: `C` is the loaded instance of
  `teStructuredDataAssetRef<STU_564B1018>`, declared at `+0x08` of the entity
  component `STU_09CB5A77` (size `0x30`) and `+0x28` of `STU_2FD9D8D6` (size
  `0x38`). `0x48 = 0x08 + 0x40`.

**The one read still missing** is the class at `[node+0x38]` (the producer of
`[node+0xe8]`), which nothing on the chain names. The next anchor is data, not
code: parse the model's `MSTU` blob for an `InstanceInfo` of type `0x564B1018`
and read its `0x88FCECD7` array through the already-pinned
`brush`/`brush_mesh`/`brush_convert` pipeline; that yields the practice range's
real brush with no new code. Also recorded: the node-type-2 update is **one
function at `0x14204cb40`** (its second half starts at `0x14204cbce`), with one
static caller, the dispatch at `0x14204824a`.

**But no model carries `C` (2026-11-02).** All 32,278 readable model assets in
the install were scanned for the class hash: **zero** mention it. `C` lives in
**entity** assets instead — GUID type `0x112`, chunks `FRAC` + `FSTU` (a
fracturable's collision), reached from an entity component `0x09CB5A77` field
`0xA7ED6024`. Nine entity assets name seven `C` assets, and the practice
range's 229 models, 79 placed assets and 34 chunks carry none. So the brush
route is the **fracturable** path, not the map's static collision; the pinned
brush from a `C` asset (4 points -> 4 vertices, 2 triangles, conditional on the
`max(1.0, [W+0xe0])` scale floor) is real but not the range's geometry.

That reopens the static-collision question with two concrete leads: (a) the
practice range's **550 entity references** were not checked against the seven
`C` assets — if one is among them the range does carry a `C`; (b) `C+0x08` is a
`STU_537E56E4` holding a **prebuilt collision mesh** (7-node array, 8 points,
16 triangles, `nodeCount` 7, `height` 4) — the same class may sit in static
models' `MSTU`, and the chunk's sector arrays (12-byte vertices, 16-byte
records, 4-byte indices, 36 sectors) look like a mesh cache too. The chunk
consumer search should also be re-run with the 4-byte-RVA dispatch method that
the `0x14208e74e` case established, on `0x1420a02f6` (the mesh-source assembler
whose input is the owner's descriptor container).

**The three checks are in (2026-11-02).**

1. **The range does not carry a `C`.** The kind-`0x0B` chunk
   `0x0DD0_000B_0F69_002E` (550 placeables, 202,688 B) was searched byte-level
   and field-level against the nine referrer entities and seven `C` assets: **0
   hits**, over 80 distinct parsed entity GUIDs. So the brush route is dead for
   this map.
2. **The range's models do carry a prebuilt collision mesh.** 229/229 have an
   `MSTU`; **161 of 229** carry `STU_537E56E4` (272 instances) inside holders of
   class `0x2DD96D6D` (279 instances), reached as
   `STUModel 0x871BD3D0 -> field 0xCB4D298D (+0x118) -> 0x2DD96D6D ->
   0xB7C8314A -> STU_537E56E4` (nodes/points/triangles/nodeCount/height).
   `GeometryBuild` does **not** read it, so the missing read is **the consumer
   of `STUModel`'s `0xCB4D298D`**. That is now the static-collision route.
3. ~~The chunk's consumer is unreachable by the validated RVA method.~~
   **Withdrawn (2026-11-02):** the address was a `.pdata` **chained fragment**.
   Its chain root is **`0x14209fc90`** (four entries to `0x1420a075d`) and it has
   one `E8` at **`0x1420251f5`** inside **`0x142024d30`**, called from the
   collision component's per-frame update slot (`0x142019944`, slot
   `0x142f7d688`) and from `0x14201fbe3`. The RVA census measured the fragment
   start, so its zero is a fragment artifact. **The chunk assembler is on the
   component-update path.** But it is **not the chunk's consumer**: it reads the
   **model** (`[[node+0x110]+0x28]`, else `0x141b3d1c0(node)`) and walks
   `STUModel+0x140` with `holder+0x2a == 1` -- the same list the per-model body
   builder reads with `== 0`; its `0x50`-byte context is scratch. On the range
   the plan is empty (279 holders, **0** with the mode byte;
   `ow_tank::model_shape_source` + install test), and the byte is a declared
   holder field (`0x89AABA94` at `+0x2a`, `0x141510382`), default 0. So the
   **chunk's own consumer is unknown again.**

**Next anchor: the consumer of `STUModel+0x118`.** The node-type-2 factory
`FUN_1420d0ef0` picks a **second** geometry class (`Alloc(0xf0)`,
`0x142132f20`) when `[node+0x29] != 0`; that class is the natural consumer
candidate, since `GeometryBuild` is the first class and does not read the model
mesh.

**The model-mesh consumer is read (2026-11-02).** The second class is **not**
the consumer (its 20 vtable slots never read `+0x118`/`+0x140`). The offset was
wrong in the reference: the client registers `STUModel` at **`0x200` bytes**
(`0x14034af00`) and its field registration passes **`0x140`** for
`0xCB4D298D`, not `0x118` — proved by the same helper's offsets for `C`
(`0x3963555A → 0x110`, `0x12B475D4 → 0x194`). The consumer is
**`FUN_14210bac0`** (array-header idiom at `0x14210bb6b`/`0x14210bc04`):
elements are pointers to holders, it requires `holder+0x20 == 0` (mode) and
`holder+0x08 != 0` (mesh block), and its entry chain is
`param_2+0xe8 → object; object+0x28 → STUModel; object+0x08 → attachment table`.
It calls the unpacker `0x141b53af0` and `MeshShapeFill` (`0x142cae540`, read
exactly: a `0x4c`-byte record with scale, `1/scale`, three pointers, four words,
`scale × m_5BD33E19`), consumed by `0x142caf6c0`.

**There is no conversion: the mesh is passed by pointer.** Implemented in
`ow_tank::mesh_shape` (4 tests). The block in the install: 161/229 range
models, 272 blocks; first block (model `0x0d0000000000078c`): 288 B of 9×32
nodes, 8 points, 384 B of 16×24 triangles, `nodeCount` 9, `+0x3c` 12, `height`
5, `+0x44` 96.0f; a 4-unit box.

**The classification question is now decisive.** `FUN_14210bac0` contains **no**
`ShapeAddToBody`/`ShapeEntryFromDescriptor`/`OwnerCreateBody`, and its caller
is a broadphase `FUN_1421088e0` (four bucket arrays at `+0x1e8`, sphere cull
against `model+0x450`) with caller `0x14212e070`. So the model mesh may be a
**query-only** structure, not the character's Domino world. The next read is
`0x142caf6c0` (what the `0x4c` record fills) and that caller chain, to classify
the system: character sweep, or raycast/overlap. If query-only, the character's
world source is still missing and the remaining candidate is the second class's
slot `+0x50` (`0x142134f70`, which does call `OwnerCreateBody`).

**The classification verdict (2026-11-02): the model-mesh path is query-only, and
no read path fills the character's Domino world with the range's static
collision.** `0x142caf6c0` writes **nothing persistent** — it bounds the query
points, collects candidate triangles into an inline `0x80`-entry stack buffer,
and runs `0x142cc6740` per triangle; no world, body or shape entry. Its owner
`FUN_14212e070` is a scene-side static-collision **query** owner (0x18-byte
entries at `+0x2b0`, count at `+0x2b8`, six `0x50` bounding volumes, a lazily
built broadphase at `+0x2d8`), not the Domino world (whose `+0x2b8` is a
`0xc0`-stride pool). The `f0` class's slot `+0x50` (`0x142134f70`) reads
`[node+0x38] → [+0x40] → [+0x48]` = `C` and builds the **fracturable** body.
And the character's own ingest entry `0x142cbcd40` has exactly six callers —
`0x140801c24`, `0x140ca6123` (inside `MoverAdvance`), `0x140dc6e14`,
`0x140eb7b22`, `0x140f1528b`, `0x140f17f4a` — all in the mover/AI ranges
(`0x140c`..`0x140f`), none a map loader; `RunCharacterControllerSweep`'s nine
`E8` sites are likewise all in `0x140c`..`0x140f`.

So the ingest chain `0x142cbcd40 → 0x142cf35a0 → ShapeAddToOwner → 0x142cd4350
→ ShapeEntryFromDescriptor` is character-only, and the map-side shape
producers we read (`GeometryBuild`, the brush sources) need a `C` the range
does not have. **The remaining read is the sweep's world argument**: does
`RunCharacterControllerSweep` query the scene-side static-collision owner
(via a vtable) rather than the Domino world? That would make the model mesh the
character's collision after all. If it does not, the client's local collision
for the range has no source in the read paths and the movement plan needs a
scope decision.

**The sweep's world is the Domino world — no vtable, no scene owner
(2026-11-02).** `RunCharacterControllerSweep` (`0x142cbcee0`) takes the world
struct as `param_1`; its one real call is the 10-byte thunk `0x142cf5660`
(`test rcx,rcx; jne 0x142cf3f80`), body `0x142cf3f80` (four `.pdata` chunks).
The world struct's **first dword is a variant tag**: `1` selects handle
`[world+0x10]` and the `0x142cb5xxx` accessors, otherwise handle `[world+0x08]`
and `0x142cd4fxx`/`0x142cd5xxx`. Every accessor decodes the handle exactly like
the query driver (`slot = handle>>0x30`, rejected ≥ `0x3fff`, then
`*(0x1442a0bc0 + slot*8)` XOR/add decode), and `FUN_142cd4d90` walks
`[world+0x2b8]` (stride `0xc0`, next `+0x54`, link `+0x48`) and
`[world+0x348]` (stride `0x58`) — the Domino shape entries and pairs. All four
chunks contain **zero indirect calls** and no call to `FUN_14212e070`,
`FUN_1421088e0`, `FUN_14210bac0`, `0x142caf6c0` or `0x142caea70`.

**So the character's collision world is the Domino world, and the model mesh is
not in it.** Combined with the ingest census (six callers, all mover/AI), the
brush needing a `C` the range does not carry, and the chunk being unreachable,
**no read path inserts the practice range's static collision into the character's
world.** That is a scope statement, not a missing instruction: either the client
receives that collision from outside the binary (server-authoritative
movement), or it is inserted by a system not in any path read so far. One
concrete unread anchor remains if the search continues: the pool-A importer
`0x1421363d1`, named in the geometry-object chain but never read.

**Round 6 (2026-11-02): the per-model route is live.** The operator's framing
was right: collision is a property of the model. The builder is
**`0x14201ae60..0x14201c887`** (a six-entry `.pdata` chain; the earlier
`0x14201af49`/`0x14201b1cd` were **fragment starts** inside it, which is why the
first reference search found nothing). Its root has **four `E8` callers** —
`0x142022030`, `0x1420195e0`, `0x142014b50`, `0x142015f70` (call sites
`0x142022219`, `0x14201985c`, `0x142014d07`, `0x142016129`), all runtime
state-sync functions of one collision component, two of them interface slots of
the table at `0x142f7d320` driven by the per-frame update `0x142019880`. It
walks `[model+0x140]` (tag `+0x148`), filters `[holder+0x2a] == 0`,
`[holder+0x08] != 0` and the mask word (bit 25 of `[component+0x420]` selects
`+0x580` else `+0x470`), and for each selected holder emits a Domino shape
through `ShapeAddToBody` (`0x142cb3940`) on a body from `OwnerCreateBody`
(`0x142cc3d90`); mode `0` is descriptor type 3 (the mesh, unpacked by
`0x141b53af0`), modes `1`/`2`/`4` are types 2/1/0. Read and implemented in
`ow_tank::model_body` (8 unit tests; install test `model_body_real`).

**The range's plan:** 163 of 229 models carry a holder list (279 holders, 272
mesh blocks); every mask is all ones, and the filter byte and shape mode are at
the class default `0`, so **all 272 mesh holders become type-3 mesh shapes** over
161 models.

**Open:** which nodes receive the component (its interface table has no RTTI or
self-pointer; the leading facts are node `+0x1c` bits `0x20`/`0x40`, byte
`+0x242` bit 7, and the owner path `+0x620 → +0x31f8`), whether the range's
static props receive it and whether that owner is the world the sweep uses, and
the descriptor's bounds/filter/`+0x5c..+0x62` sources (documented, not
converted). No runtime body count has been observed — everything above is the
plan from the install's bytes plus the read filter.

**Round 7 (2026-11-02): the owner id is a Domino world slot.** `0x14202f800`
(no-`.pdata` leaf) returns the `u16` at `[[node+0x620]+0x31f8]`;
`OwnerCreateBody` (`0x142cc3d90`) rejects `>= 0x3fff` and the empty slot, decodes
`(*(u64*)(0x1442a0bc0 + id*8) + 0xb3269b88d44d5fb2) ^ 0x77600ba5adcc265c`, and
allocates in `[world+0x288]` at stride `0x1c0`. That is exactly the sweep's
handle decode (check 7). The world allocator `0x142cc3c80` stores the inverse,
and the two constant sums are 0 mod 2^64, so the empty slot is `encode(0)` —
`ow_tank::domino_world` (7 tests). A node allocates **two worlds** per
`0x14202c660` (`+0x31f8`/`+0x31fa`) and creates its own static body from
`+0x31f8` at `0x14202d268` (handle at `+0x31fc`), so per-model bodies are
siblings of the node's own body in the node's world.

**The attachment gate, read field by field:** `[node+0x454]` bit `0x200` (copied
from the node creation record's `+0x10`; without it the node has no `+0x620`
record, hence no id), pending node `+0x490` promoted to `+0x250`, node `u16` at
`+0x1c` with bit `0x20` set and bit `0x40` clear, `[node+0xe8]` model object with
`+0x84` bit `0x80`, `[second+0x242]` bit 7 clear, and model `+0x28` non-null.
**None of these is model-asset data**, so whether the range's placed props pass
is the unread node-creation record — not a data test. That is stated, not faked.

**Open (one place):** which class allocates the component and pairs it with a
node — its interface table `0x142f7d320` is COMDAT-folded (shared base, no RTTI,
no self-pointer; the interface `this` is 0x20 past the worker object), and the
node-binding entry `FUN_142017010` (12 `E8` callers) takes eight arguments from
an unread table. Also: whether the character's own handle
(`[controller+0x1ad8]`) names the same world slot. Descriptor bounds/filter
fields remain documented, not converted.

**Round 8 (2026-11-02): the component is named, and the character's world is the
node's world.** Constructor `0x14200a9a0` (`0xad0` bytes; primary vtable
`0x142f7d608`, base interface **`0x142f7d648`** at `+0x20` — the earlier
`0x142f7d320` is a neighbouring folded table; third vtable `0x142f7d458` at
`+0x118`); registration `0x14200bab0` (`RegisterTypeWithSize`, kind `0x14`,
size `0xad0`, name decodes to `Unknown`); type-object vtable `0x1433633c8`,
slot 1 the factory `0x14200a5f0`; pair-with-node `0x14200f840` ->
`0x14201cf40(object, entity)`. The attach hooks take the node from
`[entity+0x250]` and test no asset-derived condition.

The character's `[controller+0x1ad8]` is written only at `0x140ca6132` from
`0x142cbcd40`'s return, and `RunCharacterControllerSweep` is called with it at
`0x140ee8ad1`. The mover's descriptor `+0x46` is `0x14202f800(node)` — the same
`u16` at `[node+0x620]+0x31f8` the per-model builder reads — and `0x142cbcd40`
builds the character's shape/body in that world (`0x142cf35a0` ->
`0x142cc3f50` for tag 0, `OwnerCreateBody` for tag 1). **So the character and the
per-model bodies share the node's world slot.**

**Open (round 9):** whether a placed node passes the node gate (`[node+0x454]`
bit `0x200`, copied from the node creation record's `+0x10` at `0x14202c6e4`).
**Round 13 addendum (2026-11-03):** the record is now read structurally as
`{+0x10 flags, +0x18 world/transform descriptor}`; `+0x18` is read at its
`+0x10` into `[node+0x630]` (`0x14202d1c6`) and handed to the world allocator
`0x142cc3c80` twice for the two world slots, so the gate word and the node's
placement arrive in the same record. Its producer is still the anchor.
The creation entry is **`WorldBase` slot 8** (vtable `0x143367630`,
`0x143367630 + 8*8 = 0x143367670`; the earlier "slot 32" measured `0x100` from a
neighbouring table base). `WorldBase` is `0x910` bytes (ctor `0x1420271d0`,
allocator `0x1420266d0`), and NodeType2's factory `0x142029f00` registers that
`0x910` class with kind `0x14`.

**Tool defect fixed:** `call qword ptr [reg+disp]` has a **disp8** form
(`FF 5x 40`). The image has **0** disp32 `+0x40` call forms and **505** disp8
byte patterns, so a disp32-only slot scan reports a false negative for every
slot below `0x80`; `dev/call_slot_scan.py` now scans both. None of the sites
passes a creation record.

**The count, re-measured 2026-11-17 by `dev/corpus.py index` (decode-anchored,
instruction boundaries only): 523 `call [base+0x40]` instructions = 485 on a
register base + 38 on `rsp`/`r12` (SIB, a stack-local call-through, not a
vtable slot). Against that, `call_slot_scan.py` finds 505 raw byte patterns and
confirms 492 by back-decoding; 473 sites are in both sets, 12 are decode-only
and 19 are byte-scan-only (a `FF 5x 40` at an address that is not an
instruction boundary in the linear stream - the desync class this file warns
about elsewhere). The **485** is now the pinned number (`GOLDEN_SLOT40` in
`dev/corpus.py`), the base register is stored per site, and `rax` carries 434 of
them.

**Kind `0x14` is a family, not an identity:** 160 `RegisterTypeWithSize` sites
use it with sizes `0x18` .. `0x31868`; the identity is the **size `0xad0`**
(exactly one site, `0x14200bbb1`). A kind-based install census would match
hundreds of unrelated classes, so none was written.

**Still open:** the slot-8 transfer that hands the record over, including
**register-indirect** forms (`mov reg,[vt+0x40]; call reg`), and the writer of
the record's `+0x10`/`+0x18`.

**Round 10 (2026-11-02): the support-point hull is exact; the GJK accept is
open.** `0x142caed30`'s support branch is read operation for operation and
implemented in `ow_sim::collide::mesh_sweep`: `conj(q) ⊗ block_q` (conjugate
`XORPS` with `(-0,-0,-0,+0)`, never normalised), rotation rows with
`1-(2s·s)-(2s·s)` and lane-3 zeroing, translation from `start - position`,
`((p.x·row0 + t) + p.y·row1) + p.z·row2`, then the record's inverse scale; fold
`MINPS`/`MAXPS`; `start = centre = (max+min)*0.5`, `end += centre - start`,
`step = (max-min)*0.5 + expansion + 0.02` (unconditional). The client's triangle
test `0x142cce430` takes the read path only when `count == 0` and all scaled
expansion lanes are `≤ 0.005` (ordered compare, so NaN fails); otherwise it
calls **`0x142cc9460`**, a closest-point GJK with a cached simplex, ≤ `0x13`
iterations, subfunctions `0x142cc8d80` (3-simplex) and `0x142cc7740`
(4-simplex). **`dev/mesh_support_diff.py`: 17 cases, 0 failures, 11 exact, 6
open (the triangle accept).** The client's contact for the translated capsule
case is known (tri 1, ids `0x12`/`0x22`, point `(0.75,0.9,3.01)`, normal
`(0,0,1)`, fraction `0x3df5c290`) but not reproduced. Next: read `0x142cc9460`
and the two simplex functions and pin those 6 cases.

**Round 11 (2026-11-02): the GJK accept is read and the mesh cases all agree.**
`0x142cce430` is a chain root (`0x9e` B) whose slow path is the fragment
`0x142cce5bb..0x142cce82b` (the earlier function reading missed it);
`0x142cc9460` is read operation for operation (radii floored at `0.01`,
`r = radA + radB - 0.01 + 0.00125`, `|XMM2|² < r²` convergence, support argmaxes
onto `XMM3`, fraction refine with `1.1920929e-07`, reduce dispatch, cap `0x14`),
and `0x142cc8d80` (3-simplex) is fully read: six signed dots, the face vector,
three plane terms, the eight regions in order, face weights = plane terms over
their sum, per-edge surviving-vertex permutations. Implemented in
`ow_sim::collide`; `dev/mesh_support_diff.py` is **18 cases, 0 failures, 0
open**, and the pinned contact (tri 1, ids `0x12`/`0x22`, point
`(0.75,0.9,3.01)`, normal `(0,0,1)`, fraction `0x3df5c290`) is reproduced.
`dev/gjk_simplex_diff.py` 30 seeded 3-simplices agree; `dev/gjk_closest_diff.py`
17 live + 120 seeded, 0 failures.

**Round 13 (2026-11-03): the 4-simplex `0x142cc7740` is read and implemented.**
`0x1632` B, the three-vertex body one dimension up: an orientation fix-up that
swaps vertices 1 and 2 when `dot(rot3(rot3(c-a)*(b-a) - rot3(b-a)*(c-a)), a-d)`
is negative in every lane; four vertex regions (`count 1`, direction `-a` etc.);
six edge regions (`count 2`, barycentrics from the two endpoint dots over their
sum, direction the double rotation of the pair sum about the edge); four face
regions (`count 3`, weights = the face's three triple products over their own
sum, direction the face vector or its negation whichever faces the origin,
record copies into slots 0/1/2 per face); and the interior case (`count 4`, the
four plane terms as lambdas over their sum, direction zero, `AL = 0` when the
sum is non-positive). `dev/gjk_simplex_diff.py` runs seeded quadruples through
the client's bytes: every region is hit, the block, count and barycentrics agree
exactly on all 60 cases, and the four-vertex direction agrees within one ULP on
its low lanes. One genuine quirk is recorded in
`formats/static-collision.md`: the `a-b-d` face's stored vector is the sign
opposite of the vector its own guard and weights are written against. No state
is left unimplemented; the residual is the direction's last bit on entered
tetrahedra, and a real penetration simplex from the narrowphase is `not tested`
(none of the 18 mesh queries builds one).

**Round 12 (2026-11-02): two false leads are removed; the next anchor is the
map-load path.** (a) The class static descriptor's slot 2
(`0x1407803f0`, formerly named `TypeDescriptorInstantiate`) is a **release**, not an
instantiate: it calls the instance's first vtable entry and returns the memory
through the allocation strategy's slot `0x58` (`0x141c5f420`, a per-block
free-list return; the allocator is slot `0x50`, `0x141c5ef30`, reached by
`0x141c4b270`). So the "key that selects the geometry class" is withdrawn —
the decoded word is the instance handle. (b) The "world-creation path is
obfuscated" row is withdrawn: `0x14055a0f0` is the map-load entry (it registers
the CVar whose name decodes to `debug.showMapLoadAfterMS`), truncated in the
project image by a false `noreturn` (`0x141bb8d40` returns); its `0x140be1570`
builds a `0x910` `WorldBase` plus a `0x2ad0` camera-view object for the
map-load path, and is the **only** `E8` caller of `WorldBaseCtor`. Node
instances go through the reflected type allocator `0x1420266d0`, which has no
`E8` caller and one 8-byte pointer (`0x1433654a8`). (c) A third holder consumer
exists: `0x142090e00` (13 contiguous `.pdata` entries, span
`0x142090e00..0x142094d34`, stored as a 186-byte stub) creates a Domino world
and body per streaming owner through `0x142cdf8e0`/`0x142cdf930` ->
`0x142cde350` -> `0x142cc3c80`, sets the world record's `+0x19b` byte, and
walks holders with `+0x2a == 4`; the range has none. **Next anchor:** `[[service]+0x38]`, the all-preloads-done callback in
`0x141fe0580` (reached from `MapPreloadAssets` `0x14077b7d0` and
`0x14077b5e0`). The state object is the `0xf8`-byte global `_DAT_143b796a8`
(ctor `0x140544de0`): three GUID maps at `+0x88`/`+0x90`/`+0x98`, a lock at
`+0x28`, total `+0x68`, done `+0x6c`. The service class is identified: vtable `0x142f8d970`, size `0xf8` (deleting
destructor `0x14077afa0`), and slot `+0x38` is `0x141fe0f80`, which sets the
loaded byte at `+0x78` and clears `+0x70` -- the callback only flips the state.
The level start is the **poller of `service+0x78`**, the next read. The service
pointer is the encoded field at `context+0x78` (decoded by `0x14054a010` with
the anti-tamper qword at `0x143b79598+0x2f`). `0x140554220` is the engine
subsystem constructor, not the map load.
Full detail: `formats/static-collision.md` check 17.

**Round 13 (2026-11-03): the evaluator runs end to end offline, on a built
world.** `dev/probe_candidate_evaluator.py` seeds the two prologue globals, the
guard block `Q = [0x143b79598]`, the `world+8` service decode, a domino record
and one type-3 shape, then runs `0x1420328c0` under the harness oracle. It
reaches the wrapper, the obfuscated body, the handle dispatcher, the query
driver, the narrowphase, the type-3 mesh walk and the hit policy, and fills the
answer record: point `(0.75, 0.9, 0)`, normal `(0, 0, 1)`, fraction `0.5`,
`word34 = 1`, ids `0x12`/`0x22`. The guard's "two encodings" question is
**closed**: the key byte is the constant `0x26`, the table is at
`0x142f7d158`, and the equal branch is a null-byte read trap (r12 is zeroed at
`0x14203db79`), so one body exists. The record layouts (pass sub-records at
`+0x1b0`, entries at `+0x2b8`, transform pool at `+0x2a0`, type-3 pool at
`+0x318`) are in `formats/character-controller.md`.

**The real geometry now runs too.** `crates/ow-tank/examples/mesh_world_dump.rs`
writes the assembled world (272 distinct shapes, 4,797,872 bytes) and the
probe's `--mesh-world` builds the record, entries, pools and BVH around it. With
the pinned shape alone, the client reproduces `mesh_world_real`'s pinned hit
exactly (point `(0.99500012, 4.0, 0.99500012)`, normal `(0, 1, 0)`, fraction
`0x3e8051b8`, triangle `7`).

**Round 20 (2026-11-06): the placeable transform is read and applied.** The
dump now carries **3,017 placed instances** — one per model-group entry, each
with the entry's `translation`/`rotation` — and the probe writes them into the
owner pool `[record+0x2a0]`, which is where `ShapeWorldTransform`
(`0x142cb8b80`) reads a shape's world frame. With all 3,017 in one BVH the
pinned query returns **the same answer record byte for byte** as the
single-instance run (point `(23.947973, -19.990925, 31.369896)`, normal
`(0.360226, 0.926024, 0.112766)`, fraction `0x3e8051b8`, triangle `6`). Before
the placement the same query hit a different shape (y `5.843`, triangle
`7054`), so the local frames were the whole of the error. The chain is:
`STUModelComponentInstanceData`'s `m_pose` triple -> the scene component's
`+0x20` vtable slot `0x142003720` -> `0x142000620` -> component `+0xd0`
(quaternion) / `+0xf0` (position) -> the collision builder -> `OwnerCreateBody`
`0x142cc3d90` -> body `+0x20`/`+0x10` -> `ShapeWorldTransform`.
`docs/formats/character-controller.md` carries the addresses.

**Round 19 (2026-11-03): the map blob carries the chunk payload pointers.**
`LoadMapAssetChunks` (`0x141ff5c80`) stores one handle per chunk in the map
asset blob; the completion `0x142e49400` decodes them and writes the payload
pointers at `+0x2F8` (collision, kind `0x11`), `+0x300` (pathing), `+0x308`
(streaming), `+0x310`/`+0x318`/`+0x320` (slots `0x20`/`0x1F`/`0x23`), with the
extra-chunk payload array at `+0x2F0` and the "loaded" byte at `+0x410`.
`DmWorldConstruct` sizes a `0x58`-stride pool at `world+0x348` from the
description's `+0x40 + +0x44`; `0x142cc2ae0` walks it with `0x58` stride and
`0xC0` records at `world+0x2B8`. The collision chunk's `0x58` sectors are
pointer-based, the world's pool records are index-based, so they are not the
same record. `dev/field_index.py` gains a `map-asset` profile; no
first-argument consumer of the blob exists in the map/scene/collision bands
(the completion itself is a naked thunk target without `.pdata`), so the
sector consumer is likely a job lambda reached through the job table.

**Round 18 (2026-11-03): the node record is read field by field, and the map's
entity components are named by data.** The record consumer `0x14202c660` is
decompiled whole (`formats/static-collision.md` check 18): `+0x00` id ->
state record `+0x170`; `+0x10` flags -> `[node+0x454]`; `+0x18` a **Domino
settings description** whose `+0x10` block is copied to `[node+0x630]` and
which is passed to `0x142cc3c80` twice for two worlds; `+0x28`..`+0x48`
copied; `+0x50` an asset reference decoded with `ror 0xf`/add/xor; `+0x60`..
`+0x8f` -> node `+0x878`..`+0x8a4`; `+0x90` copied by `0x141c60250`. Each flag
bit has a read site (`0x20` state record, `0x200` geometry + worlds, `0x01`,
`0x02`, `0x04`, `0x40`, `0x80`, `0x4000`, mask `0x15`). The description is a
**capacity template** (19 pool counts at `+0x20`..`+0x70` in
`DmWorldConstruct`; rotation `+0x10`/w `+0xac`, layer count `+0x20`, child
list `+0x140`/`+0x160` in `ConstructDominoWorld`), not map geometry, so a
node's worlds start empty of shapes. The init's own `OwnerCreateBody`
descriptor is constant (no shape source): the init creates a body record but
no shape. The 0x300 object it allocates on flag `0x20` is a separate **state
record** (`0x142026e20`, no vtable, id at `+0x170`), not the builder's scene
component. Separately, the map's entity component records are `{u32 component
id, u32 offset}`, and the STU asset at the offset names the class
(`crates/ow-tank/examples/map_entity_probe.rs`): 550 entities, 644 records,
13 ids, 6 named (`0x02` `STUModelComponentInstanceData` on 475, `0x03`
`STUStatescriptComponentInstanceData`, `0x06` `STUWaypointInstanceData`,
`0x0b` `STUTargetTagInstanceData`, `0x10` `STUECAISpawnerInstanceData`,
`0x11` `STUECHintPointInstanceData`); none of the 13 is a collision
component, matching the collision component attaching at the node.

Next for this item: (a) ~~the placeable transform~~ — **read and applied**: the
builder's second argument is the **0x300-byte scene component** (ctor
`0x141ff90e0`, kind `0x14`, vtables `0x1433632e0`/`0x143363320`), attached to
the node's component list `[node+0x520]`/`[node+0x52c]`; its vtable `+0x20`
slot is `0x142003720`, which copies three quads out of the component's instance
data (`+0x160` position, `+0x150` scale, `+0x140` quaternion) and hands them to
`0x142000620`, the writer of `+0xd0`/`+0xe0`/`+0xf0`. The map chunk carries the
same triple per placed model (`ModelGroupEntry`), and the probe reproduces the
client's answer with all 3,017 instances placed; (b) the record's producer, now
constrained to a writer that supplies a Domino **settings** description with
the flags word; (c) a Rust implementation of the four-pass driver, so the
probe's oracle can become a `fn/CandidateHitPolicy` unit; (d) the geometry's
variant table for negative ids; (e) the **serialised** mapping from the
`STUModelComponentInstanceData` stream to the `(position, scale, quaternion)`
triple — the client reads it through the component's accessor, so a Rust
reader that wants the component's own value rather than the map chunk's must
read `m_pose` (class `0x936A1A00`, not in the reference class list).

### The character's own shape: a capsule, and both links are closed (2026-11-07)

`ow_sim::walk`'s `0.35` m sphere is a **declared proxy**. The read that replaces
it is now complete on the shape side; what is left is three runtime scalars.

**The shape is a capsule, not a sphere.** The chain is
`0x142cbcd40` (record) -> `0x142cf5440` (`Alloc(0x590)`) -> `0x142cf35a0`
(descriptors) -> `0x142cc3f50` (owner pool) + `0x142cd4ef0` ->
`0x142cb7f10` `ShapeEntryFromDescriptor`. The record builder `0x142cbcd40` was
read in full: the mode at `+0x00` is **always `0`** (it writes `local_b8[0] = 0`),
so the character always takes the shape-add path and never the body path.

**Where the capsule comes from (all read).** `0x140ca5db0` is the single caller
of `0x142cbcd40`; it builds the `0x60`-byte source from the mover object and
`0x142cbcd40` copies it field for field. The capsule is
`A = (0, r, 0)`, `B = (0, r + d, 0)` with

- `r = max(0.02, [mover+0x5a4])`,
- `d = max(0.1*r, [mover+0x5a0] - ([mover+0x5a8] + 2*r))`,

so it is a **vertical capsule of radius `r` spanning `y` in `[0, 2r + d]`**, in
the character's local space, with the lower sphere centre `r` above the origin.
`record+0x30` carries `r`; `record+0x10`/`+0x20` carry `A`/`B` as `vec4`.
`record+0x40` is the mover's world point and `record+0x50` is the constant
`(0,0,0,1)`; `record+0x80` is `{shape id at +0x46 of the source, 0x10b9 or
0x10a1}` and `record+0x88` is `0x3fd55555` (`1/1.5`); `record+0x98` is the
physics object pointer.

**The type-1 element (link 1, closed).** `ShapeEntryFromDescriptor` does write
`entry+0x24`: it calls `ShapePoolAllocType1(world+0x2e8)`, stores the index, and
fills the `0x30`-byte element directly with `{P0, P1, radius}` from descriptor
`+0x70`/`+0x80`/`+0x90`, plus `entry+0x5c` = the radius and `entry+0x60` =
`max(|P0|, |P1|)`. The type byte is at **descriptor `+0x67`** (the builder writes
`0x0100` at `+0x66`), which is what selects this branch. `entry+0x20` = the type,
`entry+0x24` = the index.

**The radius (link 2, closed).** `record+0x30` -> descriptor `+0x90` -> element
`+0x20`, which is exactly where `0x142ccd7b0` reads it.

**The descriptor is two structs, not one.** `0x142cf35a0` builds an **owner**
descriptor `{+0x00 qword, +0x10 quad, +0x20 quad, +0x30/0x32 u16}` for
`0x142cc3f50` and a **shape** descriptor - the generic
`ShapeEntryFromDescriptor` source - for `0x142cd4ef0`. Modelling them as one
struct with the kind at `+0x34` was wrong; `crates/ow-tank/src/character_collision.rs`
now models both and its 10 tests pin every offset.

**The values (closed 2026-11-07).** The three mover scalars are copies of
`STUCharacterMoverComponent` fields: class `+0x78` `0x1972C687` (extent, maxed
by `ConstructMoverBody` against `2.1 * radius + offset`), `+0x7c` `0x7A70E3E3`
(**the radius**) and `+0x80` `0x4A576BC2` (the height the segment is derived
against). `ow_tank::character_collision::capsule_from_mover` is the client's
construction and `ow_tank::entity::mover_capsule_of` reads it per hero; the
install test `every_hero_declares_a_collision_capsule` measures 54 playable
heroes (37 at `0.5 / 1.5`, 15 at radius `0.7`, two outliers, and one landing
exactly on the `0.1 * r` floor). On the query side,
`ow_sim::walk::World::sweep_capsule` sweeps that shape through the verified
mesh leaf (`collide::mesh_sweep_with_points` supplies the two centres).

**The step limits (closed 2026-11-07).** `ConstructMoverBody` (`0x140ca9730`)
stores the mover's vertical step range at `+0x688`/`+0x68c`:
`min = -max(c84 + c8c, c78 + c80)` and `max = max(0, 2 * c_a4)`, with both
capsule extents maxed first. `ow_tank::character_collision::step_limits_from_mover`
is that construction, `ow_tank::entity::mover_step_limits_of` reads it per hero
(36 of 54 at `2.5 / 1.0`, the large ones at `2.8 / 1.0`), and `ow-client`'s
floor probe now uses it instead of an invented `1.5` m. The second capsule
triple (`0x84`/`0x88`/`0x8c`) is crouch-sized (`1.3 / 0.5 / 0.25`) but which
state selects it is still unread.

**What is still open.** The client's character-vs-world pass `0x142cf3f80`
iterates the entry's paired shapes (`0x142cd4d90` iterator, `0x142cbb260` pair
getter) and divides the delta by `param_3[8]` into the fraction quad at
`+0x640` - a Domino pair walk, not a per-shape re-query. The walk's `8 m` grid
and its slide are therefore declared deviations, and reading that pass is the
next narrowphase-side anchor. `ow-client` still calls the sphere form; adopting
the capsule is a one-line switch per probe once the pass is read, because the
capsule's origin-at-feet height changes the resting position.

---

## 7. Domino schema and integration in the current image

**Question.** The current image has `DmWorldConstruct` (`0x142cbeb70`),
`DmWorldInvDt0Writer` (`0x142cc3050`),
`DmWorldSubstepDriver` (`0x142cc3630`), `DmBroadphaseReinit`
(`0x142cbdc60`), `ConstraintDispatch` (`0x142cd9c90`),
`RotationInertiaSolve` (`0x142d07170`), `DmResidualQuery` (`0x142cdf710`) and
`DmTickKeeper` (`0x14208c070`). Which fields, offsets and operation order hold
in this build?

**What would settle it.** Read the world/body constructors and the island
solve in the current image; write the schema and the arithmetic with addresses.

**Next anchor.** `formats/domino.md`.

---

## 8. The view-model spring target

**Question.** The per-frame spring is on animation `+0x1350` toward `+0x1370`
with coefficients `0.235`/`0.48`, a k ramp of `600.0/s` and a settle threshold
of `0.004 m`. Who writes the target, and in what space?

**What would settle it.** Read the spring function's caller and the target
writer.

**Next anchor.** `formats/animation.md`, `crates/ow-tank/src/firstperson.rs`.

---

## 9. Camera modes, 3P camera and shake

**Question.** Read the first/third/spectator mode selection, the 3P camera
consumer (`camera.3PPositionOffsetY.*`, `camera.3PDistanceSmoothing.chaseRate`)
and the shake math behind `localhiteffects.3PcameraShake`.

**What would settle it.** Read the camera system update, the 3P pose builder
and the local hit-effect update in the current image.

**Next anchor.** `formats/camera.md`, `UpdateCameraSystem` (`0x14203c4e0`),
`BuildThirdPersonCameraPose` (`0x14206e400`).

---

## 10. Bots and abilities

**Question.** The practice-range bots are boxes and markers. What is the
client's bot controller and ability activation?

**What would settle it.** Read the bot/entity spawning path and the ability
graph activation from the current image.

**Next anchor.** `formats/weapons.md`.

---

## 11. The map request producer and the bundle directory producer

**Question.** Two parts of the map/asset path are not 1:1 yet.

1. **The map request producer.** A `STUMapHeader` pairs `m_D97BC44F` mode
   entries with `m_78715D57` variations by index, and each variation's
   `m_BF231F12` names its registered type-`0x002` scene map. The client has
   **no selector**: it does not read either array at runtime. The map arrives
   as type-`0x002` GUIDs in the level-load message (Jam `0x501B`,
   `0x14063b3f0` dispatch, `0x140618b30` handler, `0x14077b7d0` preload; four
   GUID vectors at message `+0x78`, `+0x90`, `+0xA8`, `+0xC0`). The remaining
   question is **who builds that message**: the server, or the local-session
   setup for an offline map such as the practice range, and which input
   (a variation index, a scene-map GUID, or the mode plus a variant identifier)
   it uses. The client's replay header records a base-map GUID, not a scene
   map, so the client's own map identity is the base map.
2. **The bundle directory producer.** The client keeps a per-type directory
   blob and binary-searches it in `FUN_141fd5160` (`0x141fd5160`); the blob is
   handed to the job at `FUN_141fd53a0` (`0x141fd53a0`) from the content
   database (`FUN_141fcbb10` -> `FUN_141fe81d0`, fields `+0x70`/`+0x264`).
   Where that blob is produced is not read. `ow-tank` builds the same relation
   by scanning the manifest's type-`0x09C` records.

**What is known.** The bundle flag, entry table and all 305 type-`0x09F`
headers are read; the practice header `0x0790000000000688` opens from bundle
`0x0d90968e484f8c33` at offset 3593 and parses as `STUMapHeader` with
`mapName` `Maps\TrainingRoom\Training Room - Practice`. The lookup *consumer*
of the directory blob is a verified cross-check (`fn/BundleDirectoryLookup`,
`BundleDirectoryLookup` at `0x141fd5160`), but it does not answer where the
blob comes from: the scan in `ow-tank` is still our own. The mode entry's
`m_A9253C68` is a `STUIdentifier` (`0xD800000000000000 | u32`, class
`0x9CADF2EC`), not an asset; a census of 305 headers / 1,612 entries finds 30
distinct identifiers. The practice header's three modes share
`0xD800000000008F9D`; header `0x07900000000000d4` has 63 variations over 20
modes and three identifiers (21 entries each), and mode
`0x0230000000000016` names six entries - two scene maps per identifier - so
neither a mode nor a `(mode, identifier)` pair identifies one scene map.

**The static proof of "no client selector".** The field hashes `0xD97BC44F`
and `0x78715D57` occur in `.text` only at the registration sites
(`0x14143ba49`, `0x14143bc95`); the class hash `0x02514B6A` occurs only at
`RegisterStuMapHeader` (`0x14031c5e2`) and in the asset-type descriptor init at
`0x1403216a0` (which stores type `0x09F` / class `0x02514B6A` at
`0x143ed6948`/`0x143ed694c`). The field descriptors and the class definition
are referenced only by registration/init code; no consumer that names the class
or either field exists. No differential harness unit can be written for (1):
there is no client function to mirror; the choice is made by the producer of
the message, so any unit would be a test of our own invention, not of the
client.

**What would settle it.** For (1), find the producer of the `0x501B` message
and read how it resolves a map request to the GUID vectors - including whether
the local practice-range session builds it in-process. For (2), read the
content-database producer of the directory blob and replace the scan.

**Next anchor.** `formats/maps.md` ("Who chooses a variation"),
`crates/ow-tank/src/mapresolve.rs`, `crates/ow-tank/src/bundle.rs`,
`FUN_14063b3f0` (`0x14063b3f0`), `FUN_140618b30` (`0x140618b30`),
`FUN_14077b7d0` (`0x14077b7d0`), `FUN_141fcbb10` (`0x141fcbb10`),
`FUN_141fe81d0` (`0x141fe81d0`), `FUN_141ff5c80` (`0x141ff5c80`).

---

## 12. Material and texture resolution on the practice range

**Question.** The viewer's practice-range scene reports 55 placeholder draws,
148 flat fills and 3 material entries with no sRGB base colour. Which stage of
the client's material/texture path is not reproduced, and is each case an
unresolved reference or a renderer rule that is not read?

**What is known.** Models, looks, materials and textures open through
`GameData::open_asset`; 289 looks, 594 material entries and 120 textures decode.
The viewer's counts are diagnostics, not a finished material path.

**Settled on the parsing side (2026-10-29).** The texture format switch
(`0x142e372b0`), descriptor builder (`0x142e2d900`) and descriptor size walk
(`0x142e2dbb0` over `0x142e37070`) are differential units against the current
image: `fn/TextureFormatFamilyBpp`, `fn/TextureDescriptor` and
`fn/TextureDescriptorSizeWalk`, each at 100% branch coverage with every scored
mutant killed. The format dispatch table is
`0x142e373d0..0x142e37491`; the size walk's is `0x142e37164..0x142e37189`.
Parsing the header is no longer the open part. BC6H, R16G16B16A16_FLOAT and
R16_FLOAT keep their exact half values through decode and upload (as
`Rgba16Float`, a viewer substitution for DXGI 10/54/95/96); R16_UNORM keeps its
exact `u16` words and uploads as `R16Unorm` when the device has
`TEXTURE_FORMAT_16BIT_NORM`, with a clamped `Rgba8Unorm` fallback otherwise. The
UINT families are still clamped. The loader `0x141dacf00` is read
but not oracle-emulated, the chunk accessors are install-checked rather than
unit-verified, the R16 upload has no live GPU readback test, and the material
application and sampler path above remain open.

**What would settle it.** Read the client's material application and texture
sampler path, then account for every submesh. Do not assume missing install
content; the bundle-aware path opens the assets.

**Hazard recorded 2026-11-06 (descriptor size walk).** The walk's mip count is
`field - bias` in 32-bit wrapping arithmetic, where `field` is the 4-bit
descriptor word at bits 39..42 and `bias` is the caller's requested mip. A
16-mip chain stores `0`, so any `bias > 0` asks for `2^32 - bias` levels and
the client's loop would spin that long before answering. No caller in the
image was found passing such a bias, so the port keeps the read arithmetic and
`ow-tank`'s `verify_probe` refuses `bias > field` instead of generating a case
no client can complete. If a real caller is found with `bias > field`, that is
the read that settles which of the two operands is a count.

**Next anchor.** `formats/materials.md`, `crates/ow-tank/src/material.rs`,
`crates/ow-scene/src/lib.rs`.

---

## 13. v1 STU, replay payload, audio decode

**Question.** v1 STU has synthetic tests only; no v1 asset ships here. The
replay frame payload is Zstd and is not decoded. Wwise audio metadata is read,
but no waveform is decoded.

**What would settle it.** A v1 asset; a decoded replay frame; an audio
dependency and the Wwise event-posting path.

**Next anchor.** `formats/stu.md`, `formats/replay.md`, `formats/sound.md`.

---

## 14. CASC: the container path after the eleven verified leaves

**Question.** The eleven leaf units (`Salsa20Xor`, `Salsa20StateInit`,
`Md5Transform`, `DynamicIndexHash`, `DynamicIndexKeyHash`,
`EncodingHeaderParse`, `EncodingFindCKey`, `EncodingFindEKey`,
`EncodingPageIndexSearch`, `CdnIndexPageSelect`, `CdnIndexFooterParse`) are
verified,
and the bucket hash, segment/archive selection, entry search, key-state
intervals, resident resolve and CDN index layout are now read and
install-tested. Which client functions remain, and what do they change?

**What is known.** `DynamicKeyStateLookup` (`0x142501b40`) is
`((xor9 ^ (xor9 >> 4)) + salt) & 0xf`; the read path's salt is 0 at all three
callers. `DynamicIndexOpen` (`0x14250e100`) and
`DynamicIndexScanSegments` (`0x14250e720`) select the highest
`<bucket><segment>.idx` per bucket, with no container-header field and no
merge; `DynamicIndexSegmentOpen` (`0x14250e400`) opens that one file and the
parser rejects a header-bucket mismatch. `DynamicContainerRead` adds `0x1e`
to the packed entry location, and `DynamicContainerReadPage` derives the
archive as `(packed + 0x1e) >> 30`; `DynamicContainerOpenArchive`
(`0x14250c8f0`) opens `data.%03d`. The install test matches 1,147,132 real
keys at salt 0 and the 1,488 archive-header placeholders at salt 1, with no
unmatched entry. `DynamicIndexFindEntry` (`0x1425111a0`) is now implemented
as the client's lower-bound walk plus backward/forward equal-range expansion;
the first equal entry is the chosen one, and all 806 duplicate pairs in this
install resolve to the header entry. The page table is now read: it starts at
`(entry_block_size + entry_header + 0x1007) & ~0xfff`, holds 21 24-byte
records per 0x200 page, and `DynamicIndexFindEntry` scans every record in order
until the first zero stored hash. The stored hash is
`DynamicIndexKeyHash(payload, 0x13, 0) | 0x80000000`, so
`DynamicIndexKeyHash` has production callers (`DynamicIndexSegmentParse` and
the page writer `DynamicIndexPageInsert` (`0x142511d20`)), contrary to the old note. All 2,560 install
pages (53,760 slots) have zero hashes and the first zero is page 0 record 0 in
every file, so the positive page path is synthetic-only here. The entry sink's
seven types, the interval merge (`0x142520700`, `0x1425208d0`), the resident
check (`0x142520c60`) and `DynamicIndexResolveRange` (`0x1425115d0`) are now
read and implemented; every salt-0 key's `(0x1e, size - 0x1e)` request resolves
to its payload and the 1,488 salt-1 placeholders return status 2.
`DynamicIndexSegmentParse` (`0x14250fcf0`) is reached only from
`DynamicIndexSegmentLoad` (`0x14250fc60`). Its mode-2 (read-only) path is
synchronous: the file is opened with `GENERIC_READ`, the parser allocates the
file size and does one `ReadFile` at offset 0; every other mode maps a
read-only view. Both paths share the parse body, so `ow-casc`'s `fs::read`
plus parse is the mode-2 path, not an async one. The validation order and the
client's 5/9 status split are read; the entry count is `block_size / 18`
(floor), so a trailing partial entry is accepted and ignored. `ow-casc` now
matches that acceptance and has a synthetic test for each rejection. Not
modelled: writable empty-file creation, the >4 GiB and mode-2 zero-size edges,
and the short-read continuation.

The BLTE path is now read too. `BlteFrameHeaderParse` (`0x1424a90e0`) accepts
type `0x0F`/`0x10`, requires `header_size == 12 + record_size*count`, hashes
exactly the header and compares the caller's digest only when the check flag is
set; the read path passes it clear. `BlteBlockRead` (`0x1424d61b0`) accepts an
`E` IV of 0..8 and dispatches `N`/`Z`/`4`; `BlteBlockReadLz4`
(`0x1424d6e50`) is the stateless LZ4 path. `F`/`O` are status 5 at the
top-level reader `FUN_1424eb510` and `A` is not a block mode. A one-off scan of
all 1,146,326 unique salt-0 keys found 1,146,325 type-`0x0F` containers, one
non-BLTE orphan, 1,714,597 chunks (`N` 284,878, `Z` 1,386,658, `E` 43,061),
and no `0x10`, direct container or outer `4` block; the 63-key keyring lacks
the `E` keys (six distinct ids in the 32nd-key sample), so their inner modes
are not measurable and an inner `4` cannot be ruled out. The committed test
samples every 32nd key and decodes 34 records.

The page-table writer (`DynamicIndexPageInsert`, `0x142511d20`) is now read
operation for operation: first zero hash, the 24-byte record fields, the
payload-then-hash order with a full fence, the type/range status machine
(0/1/2/4/`0xa`/`0xc`/`0xf`), the eight-page flush at record 20 with
`page & 7 == 7`, and the one-retry segment creation in `DynamicIndexKeyInsert`
(`0x1425032d0`). It is documented, not implemented: all five type-3 call sites
pass a null range (for example `FUN_142500430` at `0x142500541`), so the
record's location and size come from uninitialised stack slots, and the writer
is allocator/I/O state that the read path never calls.

The encoding section read (`EncodingSectionsRead`, `0x1424d1c60`) is now read
and implemented. The five sections are the espec block at `0x16`, the
ckey->ekey page index (`(ckey_width + 0x10) * ck_count`), the ckey->ekey page
bodies (`ck_page_bytes * ck_count`), the ekey->espec page index
(`(ekey_width + 0x10) * ek_count`) and the ekey->espec page bodies
(`ek_page_bytes * ek_count`), with every offset and size in the client's
32-bit wrapping arithmetic. The range rule is `offset > len || (offset == len
&& size != 0)`, with a clamped short read reported as status 14 and a rejected
request completing through the no-result lookup as status 3; the espec block's
last byte must be zero, and its NUL bytes count the strings
(`FUN_142498a70`, status 8). The install walk covers all 16,025 + 10,519 pages:
1,714,571 ckey entries, 1,714,573 ekeys, 1,714,573 espec entries, 29,694
strings, and 26,544 page hashes equal to MD5 of the page body. The section
reader has no harness unit: it allocates through a vtable and submits and
reaps five reads through the container's I/O engine and lock, so the
plain-buffer oracle cannot call it; the pure part is only the offset
arithmetic.

The encoding page lookup is now read and closed: `EncodingPageIndexSearch`
(`0x142498880`), `EncodingFindCKey` (`0x142498540`), `EncodingFindEKey`
(`0x142498610`) and `EncodingResolve` (`0x142498700`). The search picks the
page whose first key is the greatest `<= key` (exact first key -> that page;
between pages -> the earlier page; below the first -> `0xFFFFFFFF`). The ckey
scan reads only the low count byte, so a zero low byte ends the page; the ekey
scan ends on a `0xFFFFFFFF` espec index. The resolve takes the ckey record's
`u32` as the decoded size and the ekey record's `u40` low 32 bits as the
stored size. The two scans are harness-verified (`fn/EncodingFindCKey`,
`fn/EncodingFindEKey`, both 100% coverage, 100% mutants). **The read path does
no page-hash check:** the 16-byte hash is only the index stride's padding, so
the measured MD5 relation is a format fact. There is no standalone unit for
`0x142498880`: its fifth argument is on the stack, and the oracle supports four
register arguments only; the two find units compare its result on every case.

`DynamicContainerRead`'s archive page cache is now read and closed.
`DynamicContainerReadPage` (`0x1424fcd20`) opens the archive and calls the
reader built by `ContainerReaderInit` (`0x1424fbf00`); `PageCacheInit`
(`0x1424f8790`) builds 16 slots of `0x10000` bytes from the `.cmf` database open
params (`+0x78 = 0x10`, `+0x7c = 0` -> default `0x10000`). A page is keyed by
`(archive ordinal, page-aligned archive offset)`. `PageReadPlan`
(`0x1424faa20`) splits a range into a head fragment, a whole-page run and a tail
fragment; `PageCacheRead` (`0x1424fb1c0`) serves head and tail from whole-page
loads and reads the whole-page run with one direct read (the cache's
non-blocking flag is 1). `PageSlotFind` (`0x1424fa440`) is a hit only when the
stored length covers the fragment; a miss reloads the usable slot with the
smallest stamp (lowest index on a tie), and busy/failed slots are never evicted.
`PageLoad` (`0x1424fa540`) reads a whole page; the completion
(`0x1424fadf0`/`0x1424f9bf0`) copies `min(len, stored - in_page)` and zero-fills
the rest. A short read is status 0 with the short count; an I/O error is status
8 and resets the slot; `DynamicContainerReadPage` returns 10 for an archive
above `0x3fe` or a cross-archive range. The read-only mode 2 skips the
close/reopen maintenance. `Store::read_payload` is the client's exact
`(packed + 0x1e, size - 0x1e)` range; `Store::read_stored` adds the 30-byte
header with one un-cached read for the decoder. The install test proves a small
record (33,885 stored / 33,855 payload) is one page read then a hit, a large
record (248,182 / 248,152) is head + one direct whole-page run + tail, and 17
distinct pages leave 16 resident with the first evicted.

The CDN index-page formula is now read and closed. `CdnIndexFooterParse`
(`0x1424d9af0`) is the pure footer leaf: a fixed 20-byte footer, header MD5,
version 0/1, checksum width 0..16, `pageSize = footer[3] << 10`,
`entrySize = footer[4] + footer[5] + footer[6]`, `entriesPerPage = pageSize /
entrySize`, `pageCount = ceil(elements / entriesPerPage)`, and the exact
total `checksumSize + 20 + (checksumSize + keyBytes + pageSize) * pageCount`,
all 32-bit. `CdnIndexFileParse` (`0x1424d9c90`) checks the terminal
`MD5(last keys + checksums)`, the total length, and `MD5(terminal + footer)`
against the index's 16-byte key; `CdnIndexPageChecksums` (`0x1424d9f20`)
compares every page MD5 and returns 8 on the first mismatch. The four install indexes the
CDN config names all match the formula exactly (260,173,908 / 48,616,028 /
576,828 / 90,668 B; 63,149 / 11,800 / 140 / 22 pages). The unit
`fn/CdnIndexFooterParse` is verified (100% coverage, 12/12 mutants): the unit
returns `u8`, so the client's AL status (0 accept / 8 reject) is compared and
the final length gate is covered. The only input the oracle cannot represent
is a buffer shorter than 20 bytes; the client reads before the buffer.

**The CDN index page selection is read (2026-11-05).** `CdnIndexPageSelect`
(`0x1424da730`, 469 B) is a method of the index object the footer parser
fills (`footer base = index + 0x58`). It lower-bounds the `pageCount *
keyBytes` last-key array at `index+0x88` with `min(keyLen,keyBytes)` and an
early equal exit, misses when the page equals `pageCount`, then calls
`CdnIndexPageResident` (`0x1424da590`): pointer arithmetic in whole-file mode,
or an HTTP range read (`0x142522870` -> WinHTTP `0x142521d70`) with a
full-page requirement and a per-page MD5 reject (status 8) in streamed mode.
The record scan caps at `min(entriesPerPage, elements - entriesPerPage*page)`
and decodes the size (u32 BE) and offset (u64 BE) by the footer widths.
`CdnResolveLocation` (`0x1424b0cc0`) splits the 6/5-byte offset into ordinal
and offset and falls back to the file index. The earlier searches missed it
because the footer fields sit at `index+0x6c..0x80`, not `0x14..0x24`; the
`memcmp` length is a register, not an immediate 16; the stride is a runtime
`IMUL`, not `SHL`; and one caller was never promoted to a Ghidra function.
`ow-casc::container::find_page` now follows the expression, and the install
check confirms all 63,149 + 11,800 last keys select their own page. Still
open: the batch wrappers `0x1424b0e50`/`0x1424b0ef0` are read only at the
expression level, and the config-to-slot wiring is an inference.

The remaining CASC work is not all unread expressions: the page-table writer
is read but not implemented because all five type-3 call sites pass a null
range and write location/size from uninitialised stack slots; it is
allocator/I/O state the read path never calls. Type `0x10` is absent and no
outer `4` block was observed; the `E` inner modes are unreadable with this
install's 63-key keyring, so LZ4's proof is synthetic only. The page-cache
plan is not a pure leaf: its body does the atomic refcount AddRef/Release on
the archive handle and calls the handle's release vtable slot through
`*(handle)+8` when the count drops to 1, so its unit cannot earn the
five-stage word; the three refcount branches never execute under the null
handle the oracle can construct (coverage 3/6; a branch that never ran cannot
be declared dead). The unit was removed rather
than shipped with a weakened gate. The CDN HTTP transport remains a TACTLib port with no real endpoint test, but
the page request shape is now read.

**Two leaf units added (2026-11-05).** `fn/EncodingPageIndexSearch` verifies
the page-index search (`0x142498880`) directly, including its fifth (stack)
argument: 100% of its four branch edges, 11/11 mutants. It also retires the
false claim that the oracle rejects stack arguments, and the two find units no
longer describe the search's branches as unmeasured.
`fn/Md5Transform` verifies the 64-round block core (`0x1426ab530`): the block
is read only and the 16 state bytes are the whole output; the body has no
conditional branch, so the coverage stage reports 0/0 and the 18 mutants carry
the unit. Both were read from the image: the search's answer is written with a
32-bit `LEA`/`MOV` (zero-extended, `0xFFFFFFFF` for zero pages), and the
transform's Ghidra function listing stops after 64 bytes while the body runs
straight-line to its return and adds each accumulator back into its state word.

**The page selection is verified (2026-11-05).** `fn/CdnIndexPageSelect`
(`0x1424da730`) passes all five stages: 100% branch coverage over the 469-byte
body, 14/14 mutants killed, and every channel compared (AL, out_offset,
out_size, the 16-byte out_key16, the whole index object and the key buffer).
It runs the client's whole-file branch only (`index+0x38 == 0`); the
streamed/HTTP resident branch, a null `out_key16` and a page body shorter
than the footer describes are recorded as unreachable or uncovered. The
duplicate-last-key early exit is also pinned by a committed Rust test
(`container::tests::duplicate_last_keys_take_the_early_equal_page`).

**Two more CASC leaves verified, five walls measured (2026-11-17).**
`fn/ParseBoolString` (`0x1424d8800`, 278 B) and
`fn/DynamicIndexNormalizeIntervals` (`0x1425208d0`, 202 B) pass all five
stages (100% branch coverage, 10/10 + 2 declared-equivalent and 12/12
mutants). `ParseBoolString` is the harness's first **exe-frame** unit
(`address.exe` + `base: 0x140000000`): its two spelling tables hold absolute
link-time pointers, so a v2b copy faults on the first dereference. The port
also corrected a shipped stand-in - `install::parse_bool` trimmed, matched
exactly and had six spellings; the client has eight true and eight false
spellings, a prefix rule and a tri-state result.

The same reading pass measured five walls, now recorded in
`docs/casc-1to1.md` so nobody re-reads them: `BlteFrameHeaderParse`
(`0x1424a90e0`) allocates its chunk table through three IAT calls and cannot
run in the oracle; `ConfigKeyEntryParse` (`0x1424cebd0`) grows its vector
through an allocator vtable; `PageCacheInvalidateAll`/`PageCacheInvalidateRange`/
`PageCacheClose`/`PageCacheInit` all execute the client's mutex
(`0x142694c20`/`0x142694c50`); `PageSlotFind` (`0x1424fa440`) is lock-free but
the crate splits the client's one pass into `PageCache::find` +
`PageCache::claim`, so a unit needs the client's single-pass function in the
shipped code first; and `DynamicIndexResolveRange` (`0x1425115d0`) operates on
the whole segment object (`IndexSegment::resolve_range` builds the key state
itself), which looked like a marshalling job - it is four flat segment fields
and a fresh key state, so it is `fn/DynamicIndexResolveRange` (2026-11-19).

**Three of those five walls are closed (2026-11-19).**
`fn/ConfigKeyEntryParse` (`0x1424cebd0`, 481 B) earns 87.5% both-edge coverage
and 100% of 11 scored mutants with the vector grow as its declared gap - the
grow is the only impure path, and it recomputes the capacity by multiplying the
eight stack bytes holding the `key-` literal, whose upper dword is never
written, so no input can be compared there. `fn/DynamicIndexResolveRange`
(`0x1425115d0`, 291 B) earns 100% of seven branches and 5/5 mutants: it reads
four flat segment fields, builds the fresh key state and calls the same
`KeyState::resolve` that production uses. `fn/PageSlotFind` (`0x1424fa440`,
247 B) earns 100% of twelve branches and 12/12 mutants once the client's single
pass is shipped as `pagecache::slot_find`; the typed `PageCache::find` +
`claim` split stays until `PageCacheRead` (`0x1424fb1c0`) is read, because the
client hands back a *matching busy* slot with usable 0, which the split cannot
express. The CASC suite is 30/30 verified. Still walls: `BlteFrameHeaderParse`'s
three IAT allocations and the four page-cache entry points that take the
client's mutex (`0x142694c20`/`0x142694c50`).

**The text key parsers are verified (2026-11-17).** `fn/ParseHexKey`
(`0x14248efd0`, 102 B) and `fn/ParseHexKeyId` (`0x14248f040`, 106 B) pass all
five stages: 364 differential cases over the return, the read-only text, the
16- (or 8-) byte target and the 1-byte flag; 100% branch coverage (4/4 each);
5/5 mutants each. The contract is now the crate's: zero the target and the
flag, skip `0x20`/`0x09..=0x0d`, decode exactly 16 bytes (8) through the
hex-pair leaf `0x14269ab80`, and return the characters consumed (`whitespace +
32`, `+ 16`) or 0 on a short decode - with the partial decode left in place, as
the client leaves it. `install::parse_hex_key` and `install::parse_hex_key_id`
are the port, and the keyring field parser now decodes through
`parse_hex_key`, so the text path has one hex decoder instead of a
`from_str_radix` loop. `0x14269ab80` has seven call sites and its own branches
sit outside these windows; it is now its own verified unit, `fn/HexPairDecode`.

**The whole-file index validation is verified (2026-11-17).**
`fn/CdnIndexFileParse` (`0x1424d9c90`, 642 B) passes all five stages: 364
differential cases over every channel (AL, the read-only key and file, and the
0x30-byte derived block the client writes), 100% branch coverage (10/10 over
the window's ten sites), and 5/5 mutants killed. It pins the stage order - the
footer block, the terminal `MD5(last keys + checksums)`, `MD5(terminal +
footer)` against the 16-byte key, then `CdnIndexPageChecksums`'s status
returned unchanged - and the derived block's bytes, including the hash bytes a
header-MD5 rejection leaves zeroed. The shipped `parse_index_file` composes
`parse_index_footer`, two new helpers (`terminal_checksum_matches`,
`key_checksum_matches`) and `verify_page_checksums`; the streaming
`PageIndex::open_with_key` now uses the same two helpers, so the checks have one
expression. Two inputs are declared gaps: an entry size larger than the page
size (the client divides by zero and faults; the port returns 8) and a 32-bit
wrap of `pageCount*pageSize` or `lastKeyBytes+checksumBytes`.

**`CdnIndexPageResident` cannot be a five-stage unit (2026-11-17).**
`0x1424da590` (401 B) is read: whole-file mode caches `page` at `index+0xa0`
and points `index+0x98` at `base + page*pageSize`; streamed mode starts an HTTP
range read (`0x142522870`), waits on it (`0x142522380`), requires the full page,
hashes it and compares `checksumSize` bytes against
`index+0x88 + index+0x7c + checksumSize*page`, returning 8 on a mismatch and 1
when the request never starts. Those branch sites can only run with the
network stack present, so the unit could never reach 100% coverage; the
whole-file branch it does contain is already exercised through
`fn/CdnIndexPageSelect`, which calls it and pins both the cache-hit and the
cache-miss edges. It stays read and documented, not verified.

**The batch selection is verified, and the CDN cluster screen is closed
(2026-11-19).** `fn/CdnIndexPageSelectBatch` (`0x1424da910`, 660 B) passes the
five stages: 18/18 reachable branches of 20 sites, two declared dead (the
resident-error exit, and the tail's `i >= count` guard), 11/12 mutants with the
one compile failure counted killed, 300 differential cases plus the edge pool.
It pins three facts the closed-form `CdnIndexPageSelect` note did not: the
record cursor is not reset on a hit (a duplicate key resumes one record past
the hit and is reported missing), the page cursor is monotonic across the
sorted keys (an unsorted caller gets the cursor's answer), and the tail's
scalar zero loop only runs when the caller's found array overlaps the location
array (the unit's fixture aliases them exactly).

The rest of the "CDN transport + index fetch" queue cannot become five-stage
units against this image. The oracle supplies no OS services, and every one of
these functions reaches one of: an IAT slot (`WinHttpReadData` `0x142521d70`
calls `[rip+...]` at `0x142521da2`/`0x142521dac`/`0x142521dd8`; the index
loaders `CdnIndexFooterLoad` `0x1424dabb0`, `CdnIndexFileLoad` `0x1424dad20`,
`CdnIndexBufferAdopt` `0x1424db020` and `CdnIndexFetchState` `0x1424e8f20`
reach `0x142521c70`, a handle close through an IAT slot, and `0x1424d9800`, a
file read), a virtual slot (`CdnClientDtor` `0x1424b0a90` `call [rax+0x30]`;
`CdnIndexBufferStore` `0x1424da4c0` `call rax`), or a runtime lock global
(`CdnResolveComplete` `0x1424b0c50` -> `0x142694c20`/`0x142694c50`,
`CdnClientCtor` `0x1424b0970` -> `0x142694c30`, and `CdnResolveLocation`
`0x1424b0cc0` -> `0x1424b0be0` -> the file read). Each of those sites faults
in the emulator, so the transport stays read and documented, not ported;
`ow-casc` keeps its whole-file reader and the batch/incremental selectors.
Unlocking the set needs an IAT-stub feature (a fake handle table and a read
callback) in the harness, which does not exist today.

Two neighbours are not blocked and are the next candidates:
`CdnConfigParse` (`0x1424ce680`) calls only direct in-image functions
(`0x142388210`, `ParseHexKey`, `0x1424cc390`, `0x1424cc490`, `0x1426a0f20`),
and `CdnIndexInit` (`0x1424da340`, 125 B) is pure stores plus the `-1` handle
initialiser at `0x142521c60` - the latter has no shipped 0xa4-byte object in
the port, so it would prove state no shipped code uses and stays open.

**MD5 is answered (2026-11-17).** `blte.rs`'s `md5()` used to be a single pass
over the input while the client stages `Md5Init` (`0x1426a5100`) -> `Md5Update`
(`0x1426a4f40`) -> `Md5Final` (`0x1426a5010`) over a 0x58-byte context (byte
count, four words, 64-byte buffer). A single pass produces the same digest for
a buffer known in full, so nothing had broken, but the client's buffering rule
(`count & 0x3f`, transform when the input fills the buffer) was unmodelled.
The three stages are now ported at the client's offsets and verified by
`fn/Md5Init`, `fn/Md5Update` and `fn/Md5Final` (364 differential cases each;
100%, 4/4 and 3/3 branch coverage; 6/6, 8/8 and 9/9 mutants with two declared
equivalent), and `md5()` is their composition, so the RFC 1321 vectors run the
client's staging. Two layout facts the units pin: `fill` comes from the
**pre-add** count's low dword while the count advances 64-bit, and the final
message length is `count << 3` written over the buffer's last eight bytes.

**Next anchor.** `docs/casc-1to1.md`, `formats/casc.md`,
`crates/ow-casc/src/pagecache.rs`, `crates/ow-casc/src/container.rs`,
`crates/ow-casc/src/encoding.rs`.

---

## 15. The renderer: D3D11 production path, pass list, draw list and pipeline selection

**Question.** What is the client's per-frame pass sequence on its **D3D11
production path**, how is the world draw list built (visibility, sorting,
instancing, per-object constants), how does a material's shader group pick a
code GUID/tier and a pipeline state, and what vertex/index layout does the input
assembler consume? The D3D11 device, swap chain, back-buffer texture and
RTV/SRV/UAV, the frame step order, shader object, pipeline create,
constant-buffer allocation and the 64-opcode command replay are read; the
engine-side pass sequence, draw-list builder and material→pipeline join are
not. The D3D12 backend is the **beta** path and is mapped secondarily.

**What is known (D3D11).** The operator's decision of 2026-10-31 fixes D3D11 as
the 1:1 target. The platform backend selector `FUN_1426377f0` maps request kind
**2** to `FUN_142660e90`, whose vtable `0x143496d68` is the `pm_dx11` instance;
every other kind goes to `FUN_14263b140`'s `pm_dx12` vtable `0x1434904a8`.
`pm_dx11::Instance::Init` (`0x142673c80`) loads `d3d11.dll` and resolves
`D3D11CreateDevice`; the public create `0x142666ca0` calls
`pm_dx11::Device::Init` (`0x142673810`), which calls `FUN_142666980`
(`0x142666980`), which builds the request and hands it to the vendor wrapper
`0x14267d270`. The first feature level is `11_1` alone; a failure retries with
`11_0, 10_1, 10_0, 9_3, 9_2, 9_1`. The creation flag word is `DEBUG` when
instance `+0x34` is non-zero, `SINGLETHREADED` when the create descriptor
`+0x40` is non-zero, and `PREVENT_ALTERING_LAYER_SETTINGS_FROM_REGISTRY` when
instance `+0x44` is zero. `DriverType` is `UNKNOWN`, `Software` is null and the
SDK version is 7. The adapter is the platform GPU's DXGI adapter
(`platform GPU +0xf0`); `pm_dxgi::Instance::InitGpus` (`0x14262f0e9`)
enumerates from adapter 0 and sorts the list, but the preference order is open.

`pm_dxgi::Instance::InitSwapChain` (`0x14262f9d0`) uses
`CreateSwapChainForHwnd` with a `DXGI_SWAP_CHAIN_DESC1` on the newer-factory
path. The default view (`FUN_141ef6960`) is width × height, format index `0x1f`
→ DXGI **28** (`R8G8B8A8_UNORM`), **3** buffers, buffer usage **`0x30`**
(RENDER_TARGET_OUTPUT + SHADER_INPUT), swap effect **0**, scaling 0, alpha mode
0, flags 0. `pm_dx11::Device::CreateSwapChainResources` (`0x14266a060`) calls
`GetBuffer(0)` and `CreateRenderTargetView` with an explicit texture-2D
descriptor, then creates the SRV when usage bit 0 is set and the UAV when bit 1
is set; the view format is resolved by the D3D11 table `0x1434965d0` (index
`0x1f` → 28). The full table (64 entries) and the six special cases are read
and pinned; the buffer table `0x14348bf10` is byte-identical. Resize is the
read release → `ResizeBuffers` → recreate order. `UpdateSwapChain`
(`0x1426320f0`) writes the view's sync boolean (view `+0x54`) into the present
object `+0x60`, and `Present` (`0x142630790`) uses it as the sync interval; the
boolean's producer is an unresolved config global, so its shipped value is
open.

The D3D11 frame order is read: frame begin (`0x141ef0600`) -> wait/pacing
(`0x141ef9c80`) -> scene prepare/upload (`0x141ef8140`) -> queue finish (flush,
fence signal, blocking wait, `0x142650340` -> `0x142650430`) -> queue submit
(`0x142677dc0` -> replay `0x14266c690`) -> fence wait (`0x14265adf0`) -> per-view
submit (`0x141f0caa0`) -> frame end (`0x141ef0800`) -> present
(`0x14265e8f0` -> `0x142630790`). The order is pinned by
`crates/ow-render::frame`; only `Present` has a complete executed body, the
rest are logged as open. A fourth shared slot, 132 (`0x1433585e8` →
`FUN_141efa380`), advances the frame index and flushes the two batch buffers;
its outer order is open. The outer caller of the shared frame slots, the D3D11
fence/query wait and the engine pass writer are not read. D4/D5 add the replay
executor and the read clear/draw/viewport handlers.

The D3D11 replay (`FUN_14266c690`, reached from
`pm_dx11::CmdQueueImpl::Submit` `0x142677dc0`) dispatches 64 opcodes from
record `+0x10` (size `+0x14`, including the 0x18-byte header) through the table
`0x142671154`. D4 reads all 64 handlers. The read context handlers are `0x07`
`ClearRenderTargetView`, `0x08` `ClearDepthStencilView`, `0x25`
`Draw`/`DrawInstanced`, `0x26` `DrawIndexed`/`DrawIndexedInstanced`, `0x29`
`RSSetViewports`, `0x2a` `RSSetScissorRects` and `0x30` `Dispatch`; `0x00` is
the end/flush record and `0x1d` is a no-op. The earlier names `0x09`
`ClearDepthStencilView`, `0x25` `DrawIndexed`, `0x26` `VSSetShader` and `0x2d`
`PSSetShader` were wrong and are corrected in `formats/renderer.md`; `0x04` is
a **Map/Unmap** record (`context+0x70`/`+0x78`), not a depth clear. The depth
clear is `0x08` (`ClearDepthStencilView`, `+0x1a8`), with view `+0x18`, flags
`+0x20`, depth `+0x24` and stencil `+0x28`; the earlier "no depth clear"
statement was wrong. The render-target bind **is** in the replay: `0x0c` reads
the count at `+0x18`, the RTV array at `+0x20`, the DSV block at `+0x28` and
the state id at `+0x30`, clears/releases each entry, clears the DSV, and calls
`OMSetRenderTargets(count, array, dsv)` at `0x14266e54d`; `0x0d` resolves the
pending MSAA entries and unbinds with `OMSetRenderTargets(0,NULL,NULL)` at
`0x14266e6ba`. `FUN_142674980` is a separate per-draw UAV/CB/SRV state flush,
called from `0x25`/`0x26` at `0x1426701e9`/`0x14267027e` and from
`0x27`/`0x28` at `0x1426703bb`/`0x1426704a4`; it never calls
`OMSetRenderTargets`. The engine ring is a **separate** stream: its records
have the id at `+0x00` and no size field, while the replay has the opcode at
`+0x10` and the size at `+0x14`. No equality dispatch on `0x387` exists, so
there is no record-id -> replay-opcode translation; `0x387` is a batch
submission, not a draw. The `0x0c..0x1b` batch is read. `0x0c` and `0x0d` are
implemented in the executor and `ow-render::state`. The per-stage SRV/UAV
caches at `0x29a0`, `0x49d0`, `0x6a00`, `0x8a30`, `0xaa60`, CS SRV `0xca90`
and CS UAV `0xeef0` are modelled. The other fourteen handlers are open because
each carries a state-block or descriptor array pointer and the per-element
`SyncBufferForUsage`/flush path is not implemented. The other 53 opcodes are
open by opcode. D4's executor and D5's nine context handlers are implemented
and synthetically tested in `crates/ow-render::replay`; the remaining work is
the cache population and the queued-SRV binder.

`pm_dx11::Device::CreateShader` (`0x1426697a0`) takes a DXBC blob whose
descriptor version is 3 and maps the client's type byte to the six
`ID3D11Device::Create*Shader` calls; `pm_dx11::Device::CreateRenderPipeline`
(`0x142667be0`) builds the input layout from the stream element list and the
shader input signature and creates the blend/rasterizer/depth-stencil states;
`pm_dx11::Device::CreateBuffer` (`0x142665750`) and
`pm_dx11::CmdQueueImpl::UploadConstants` (`0x1426798c0`) allocate dynamic
constant buffers with bind flag `0x10000` and 16-byte alignment. The
material -> shader-group -> code-GUID/tier join, the element producer and the
constant contents are open.

**What is known (D3D12 beta).** The D3D12 instance vtable `0x1434904a8` (Init
`0x14267b8b0`), `pm_dx12::Instance::CreateDevice` (`0x14267b5b0`),
`pm_dx12::Device::Init` (`0x142654160`), the `0xb000` flags, the descriptor
heaps, the indirect command signatures, `CreateShader` (`0x14264a940`),
`CreateGraphicsPipelineState` (`0x14265bed0`) and the pipeline library
(`0x14265c720`) are read. The same engine frame loop, swap chain and
command-record substrate apply. **Negative:** no pass-name or render-graph
strings exist; the `GBuffer.*` strings are DLSS parameters. `d3dcompiler` is
absent, so shaders are precompiled blobs. No `0x086` shader-instance asset
exists in this build.

**What would settle it.** (1) The full 64 handlers are now read; `0x0c` and
`0x0d` are implemented. The remaining opcode work is the 53 open opcodes listed in
`formats/renderer.md`, chiefly the deferred constant-buffer/shader-resource
caches, the per-draw `FUN_142674980` state flush and the query/annotation path.
The `0x0c..0x1b` batch is read: `0x0c`/`0x0d` are the bind/unbind pair, and
the other fourteen are open because each dereferences an out-of-band state
block or descriptor array; the six constant-buffer / six shader-resource binds
defer to the end-record flush. The next step is to model those tables and the
`SyncBufferForUsage` resource objects. The `0x0c` alias-unbind test is
`FUN_14265fb90`; the stage cache populate paths are `FUN_1426781b0` (SRV) and
`FUN_142676130` (constant buffers), with the per-stage flushes
`0x142678bf0`/`0x142678570`/`0x142678710`/`0x142678d90`/`0x142678a50`/
`0x1426788b0` and the CB flush `0x142660620`. The alias predicate, cache
ranges, flush ordering and `apply_render_targets` are implemented in
`ow-render::state`; the cache population and the queued-SRV binder are the
remaining read/implementation work. The population path is open:
`FUN_1426781b0` creates an `ID3D11Buffer`; `0x16..0x1b` compare the cache slot
with the element's first qword and enqueue the descriptor, but do not write the
slot. The binder that writes the slot and issues the non-null
`*SetShaderResources` call is not read. (2) Read the engine-ring consumer and
the writer of `renderer+0x1c0`/`+0xc1f0`; the batch A/B record layout and the
CPU→GPU / GPU→CPU transfers are read and modelled in `ow-render::batch`, but
the item arrays and the consumer that turns them into draws are open. (3) Read the engine pipeline-cache caller and the material
group's parallel code/source index rule. (4) Read the model render-mesh
loader's input elements and the stream-list builder. (5) Read the outer caller
that orders the three shared frame slots, the D3D11 fence/query wait body, the
engine pass writer and the swap-chain RTV bind. The render-target bind itself
is now read as `0x0c`/`0x0d`; `FUN_142674980` is a separate per-draw state
flush whose five-stage/UAV details remain open. (6) For proof, a **client
capture is impossible** under the anti-cheat, so there is no call-trace or
image oracle. The proof is the read expression plus synthetic record streams
(the recording sink is the oracle). D1-D4 and the D5 clear/viewport/`0x0c`/
`0x0d` handlers are implemented and synthetically tested. The cache population
and queued-SRV binder remain. They are not capture-compared, and D10 is not
provable.

**Next anchor.** `formats/renderer.md` ("D3D11 staged plan"), `FUN_14266c690`
(`0x14266c690`), `0x142671154` (the opcode table), `FUN_141ef8140`
(`0x141ef8140`), `FUN_141f0caa0` (`0x141f0caa0`), `FUN_142667be0`
(`0x142667be0`), `MrnmHandler` (`0x141b88f10`),
`crates/ow-render/src/batch.rs`, `crates/ow-render/src/state.rs`,
`crates/ow-render/src/replay.rs`, `FUN_14265fb90` (`0x14265fb90`),
`FUN_1426781b0` (`0x1426781b0`), `FUN_142676130` (`0x142676130`).

---

## 16. The model render-mesh consumer: elements to input layout

**What is read and verified.** The model chunk table (`0x141b5e1e0` sizing,
`0x141b5df70` load) with its per-tag version gate; `MDLC`'s material list
(`MdlcMaterialRead` `0x141b88cb0`); `MSTU`'s `{u64 offset; u32 size}` header
(`0x141b88df0` -> `FUN_1421d88c0`, the STU v2 loader); and `MRNM` end to end --
the fixup (`FUN_141b893e0`), the runtime size (`FUN_141b89390`) and the
runtime-block load (`MrnmHandler` `0x141b88f10`), each a differential harness
unit (`fn/MrnmFixup`, `fn/MrnmRuntimeSize`, `fn/MrnmHandler`) against the
shipped `crates/ow-tank/src/mrnm.rs`. Page: `formats/model.md`.

**What is open.**

1. **The element record is read; the value decode is not.** The record is
   `{u8 semantic, u8 index, u8 format, u8 stream, u32 packed}` and the packed
   dword is `offset << 12 | stride << 22`. Both packings are read from the
   client's own render-mesh writer `FUN_14210c450`, which emits this record
   form; the per-format byte sizes are pinned by the file (offset adjacency +
   stride) and checked over 637 records on 68 models with zero disagreements
   (`formats/model.md`). What is not read is the **value conversion**: the
   image contains no CPU half-float conversion instruction, so the client
   hands the format byte to D3D11 and the GPU converts. `ow-tank`'s floats are
   therefore the viewer's reading; its SNORM follows the GPU rule
   `max(x/127, -1)`. The format bytes the client's writer uses are `0x02`,
   `0x03`, `0x04`, `0x09`, `0x0c` and `0x13`; `0x13` occurs in no model file
   and stays undecoded rather than being guessed.
2. **The input-layout builder.** `CreateRenderPipeline` (`0x142667be0`) builds
   the `D3D11_INPUT_ELEMENT_DESC` array from a **prebuilt** list (element 0x20
   bytes, semantic name `+0x10` defaulting to `"Attr"`, engine-format byte
   `+0x04` resolved through the 64-entry table `0x1434965d0`, offset `+0x08`,
   index `+0x00`/`+0x18`); it builds none itself. The list's builder is not
   found, and it is the only missing piece between the file's format byte and
   the engine enum. The searches that came up empty: no 13-entry
   model-format -> engine-format table exists in the image; no code outside the
   renderer shifts `packed >> 12`; and the descriptor arrives at
   `CreateRenderPipeline` through `param_3+0xb8` already filled. Next anchors:
   the caller of the `pm_dx11::Device` vtable slot at `0x143497040`, and the
   0x20-byte element-list allocator (count `+0xc`, array `+0x10`).
3. **The runtime block's non-geometry fields.** The `+0x00` qword (the OR of
   each submesh's `+0x28`, skipping `-1`), the five `0x1c`-byte header blocks
   and the `-1` initialisers are copied by the verified handler but nothing
   read consumes them.
4. **The MDLC differential unit.** The material extraction is a 1:1 copy of
   `MdlcMaterialRead`'s loop and is pinned by a synthetic offset test, but it
   has no harness unit: the function writes a model object whose tail size is
   computed by a function with no Ghidra entry (`0x141b88e30`), and its two
   source pointers are absolute, which the harness's relocation model (payload
   base only) cannot express.
5. **The LOD pick's last link.** The byte at runtime `+0x92` is a level set:
   `0x141b40be0` unions it (skipping `0` and `0x7f`), `0x141b3def0` masks it by
   availability, the lowest/highest set bits become `+0x20e`/`+0x20f`, and
   `0x141b3ce00` picks the level per instance from distance and quality
   (`formats/model.md`). `Model::finest_level` now implements the client's
   finest-level bit test. What is open is the **inclusion predicate**: the
   draw path passes the byte into the draw record at `+0x134`
   (`0x141e20d10`) and no byte reader of that field was found, so `bit i`
   versus `== 1 << i` is inferred from the mask semantics, not read. Next
   anchors: the consumers of the draw record's `+0x134` and the sort that
   compares the per-item float in `0x141f55f60`.
6. **The bone-index join.** `MsklTableBuild` (`0x141b894d0`) is read: it sizes
   every `mskl` table (`bones_abs` 64-byte matrices and 48-byte poses,
   `bones_simple` ids, `remap_count` u16 Lookup, a `bones_abs` inverse table at
   `mskl +88`, a `remap_count` 8-byte table at `+96`) and `ModelChunkLoad`
   stores the block at model `+0x08`. The install then proves what the tables
   *are*: on all 7,328 skinned model assets the Lookup is a permutation of
   bone indices, `remap_count` is the largest raw blend index plus one, the
   inverse table maps every slot back exactly, and the geometry puts the
   dominant influence a median 0.122 m from its vertices through the Lookup
   versus 0.591 m through the raw index (`bonemap_probe`,
   `formats/animation.md`). **What is open is the consumer:** no client
   function that joins a vertex's blend index to a bone has been found, so
   three things stay the reference's: the submesh `BoneIdOffset` addition
   (`raw + +0x82`; `0` on every install submesh), the index bound, and
   `0xFFFF` = "unbound" (no install Lookup holds it). Next anchors: a reader
   of the runtime skeleton's `+0x48` (Lookup) or `+0x50` (inverse), or the
   buffer upload that hands the remap to the vertex shader. Searches that came
   up empty: a u16 load at `+0x6a` (the runtime `remap_count`) occurs in only
   seven functions and none builds a palette or copies the Lookup; a scan for
   `word ptr [reg + reg*2]` (the Lookup element read) against a `+0x48` pointer
   load gives 106 functions, none in the model path; the submesh `+0x82`
   (`BoneIdOffset`) is read as a word by `FUN_141fb8c60` only, which is a
   render-pass gate on a different struct.

**Next anchor.** The pipeline descriptor's builder: walk back from
`CreateRenderPipeline` (`0x142667be0`, reached through the device vtable at
`0x143497040`) and find who fills `param_3+0xb8`'s element lists; that function
is the join between a model's element record and the engine format table at
`0x1434965d0`. `formats/model.md`, `formats/renderer.md` R10.

---

## 17. Effects: the runtime record and the array sub-chunks

**Question.** The client's effect layer is now read on the payload side — the
`EFCT` root, the 0x34-kind table at `0x143610490`, the per-tag payload copies in
`FUN_142e4a780` and the seven components with a GUID (`docs/formats/effects.md`).
What is not read is the destination: the runtime record the builder fills, the
meaning of its fields, and the consumers of the four array sub-chunks.

**What is known.** The builder allocates its own record through the object at
`[param_1+0xa8]+0x30` and fills it per kind; `FUN_142e4fbe0` copies five arrays
(one `u32` and four `u8`, count at payload `+0x28`) and `FUN_142e50110` copies a
`u16` array plus a byte array (count at `+0x51`). The fixup `FUN_142e4e8e0`
converts the stored offsets to pointers before the builder runs. Five tags
(`ECMP`, `ECSS`, `ECAT`, `ESHT`, `ECCV`) have no read site in the image; their
payloads are editor-side data.

**What would settle it.** A caller of `FUN_142e4a780` that consumes the runtime
record (its `+0xb8` kind is what the effect system switches on), and the
consumer of `+0x1c`-style array fields — the same shape as the `MRNM`
element-array search in item 16. Until then the record side stays open and the
typed payloads are the claim.

**Next anchor.** `docs/formats/effects.md`, `crates/ow-tank/src/effect.rs`,
`crates/ow-tank/tests/effect_real.rs`, `FUN_142e4a780` (`0x142e4a780`),
`FUN_142e4e8e0` (`0x142e4e8e0`).

---

## 18. The frontend pipeline: menu, mode, map, character select, spawn

**The question.** Follow the client from the front-end menu to a spawned
character and read each stage: menu -> mode selector -> practice range ->
character selector -> spawn.

**The answer (2026-11-08).** The front end is not native UI. The menu, the mode
selector, the character selector and the custom-game browser are web pages in
an **embedded CEF browser inside the game process**, and the page talks to the
engine through a JSON callback bridge whose whole vocabulary the client names
itself (`onConnected`, `onDataPost`, `onRegisteredFunctionCalled`,
`onPageLoadBegin`/`Completed`, `onJavascriptDialog`, at
`.rdata 0x143616040..0x143616640`; validator `0x142ec6cb0`). The page's content
is fetched over bnet (API-gateway protos `0x143146e60`, the lobby systems), and
the mode/hero rules are the custom-game descriptor
(`CustomGame.proto`, `0x1435f1767`: `CGGameRulesetTeam.heroesArrayIsWhitelist`,
`heroGUIDs`, `numTeamMembers`, `CGPregameOptions.enableHeroDraft*`).

**What is read.** The page<->engine router is read in full (`RouteBrowserMessage`,
`0x142ed1be0`, 4092 bytes, dispatched over three `.pdata` fragments; the middle
fragment's function was created on 2026-11-08 with the operator's approval). It
parses the JSON envelope, requires the integer `type` field (`0x143437678`) and
dispatches **50 numbered messages**: service codes `0x00..0x1a` (host ->
browser: create with `start_url`, navigate, cookie, URL allow-list, register
javascript variable, call javascript callback, zoom, focus, scheme handler) and
client codes `0x1b..0x31` (browser -> host: `OnReady`, `OnPageLoadBegin`,
`OnPageLoadCompleted`, `OnDataPost` `0x21`, `OnRegisteredFunctionCalled` `0x23`,
`OnJavascriptDialog` `0x2e`, `OnJavascriptConsoleMessage` `0x2f`,
`OnSchemeLoadBegin` `0x30`). The full table is in
[`formats/frontend-pipeline.md`](formats/frontend-pipeline.md).

The developer overlay is read: **Dear ImGui, docking branch**, compiled into
the game (`.rdata 0x14328fa10..0x143290b30` carries `imgui.ini`,
`imgui_log.txt`, `Debug##Default`, `HoveredId: 0x%08X`, `Click to break in
debugger!`, `Press ESC to abort picking.`, the dock-node strings and
`##Menu_%02d`/`##Popup_%08x`/`##MainMenuBar`). Its own label text is not
plaintext - the CVar name table is encoded in this image - so the overlay's
feature list is readable only through the client's decoder.

Also read: the map-start sequence `0x14054fbe0` -> `RunMapLoadEntry
0x14055a0f0` -> `0x14055abc0` / `0x14055a8c0` -> `[[0x143b796a8]+0x240]` ->
`0x1406c1bc0`, with `0x14055a0f0`'s body already documented; and the player
array accessor `GetPlayerComponentByIndex 0x14214dcc0` (640 call sites).

**What is open.**

1. **The host's two doors.** `onDataPost` (`+0x218`) and
   `onRegisteredFunctionCalled` (`+0x238`) are the only ways a page click
   reaches the game; whoever fills those two slots of the browser host object
   is the front end's gameplay entry, and that producer is not found. The
   validator `0x142ec6cb0` only reads the config it is handed.
2. `0x14055abc0` and `0x14055a8c0` (452 and 754 bytes) are read only by their
   callees; the map-start tail is obfuscated.
3. The write side of the player component array - who sets a player's position
   and hero - is not found. The accessor is inlined at 640 sites, so caller
   filtering is useless there; the work is a store census.
4. The character selector has **no engine-level implementation to copy**, and
   its *rules are now read as data* (2026-11-09): each team of a mode carries
   `m_availableHeroes` (`0x59C86C8D`) - either the empty marker `STU_D1DC7C55`
   (every hero) or a whitelist `STU_3C14F504` / `STU_C45DE560` whose `m_heroes`
   (`0x056D3E39`) names them. All four pool classes are runtime component types
   (stub block `0x1402dc690`..`0x1402dc990`, interface tables
   `0x143e79a90`/`0x143e79bf0`/`0x143e79eb0`/`0x143e79d50`). Measured: Practice
   Range = empty marker (all heroes), Mei's Snowball Offensive = a one-hero
   whitelist, Stadium = one whitelisted team and one open team. Read and tested
   in `ow_tank::hero_select` (`crates/ow-tank/tests/hero_select_real.rs`);
   written up in `formats/frontend-pipeline.md` section 4a. **The mode's applied
   hero is also read** (2026-11-09): each team's controller graph
   (`m_controllerScript` `0xA2781AA4`) carries one `STUStatescriptStateHero`
   (`0x91428992`) whose `hero` input is `STUConfigVarDynamic` (`0x4B093107`)
   `m_identifier` **940**; install survey: 137 modes -> 35 controller graphs ->
   13 with a hero node -> 12 read variable 940, 1 ("Assault") writes a literal
   hero. The state is a runtime component type (stub `0x140402690`, size
   `0xF0`, interface `0x14404EEC0`). `ow_tank::graph::hero_states_of` +
   `hero_select::mode_team_scripts`, tests `crates/ow-tank/tests/hero_pick_real.rs`,
   section 4b. **The hero asset's own selector fields are read too**
   (2026-11-14): its display name (`m_0EDCE350`, a type-`0x07C` string), its
   playable marker (`m_64DC571F`, the reference's `IsHero`; the practice roster
   is 135 hero assets, 54 playable and 81 NPC), and its movement row
   (`ow_tank::entity::MoverRow`: forward/side/back/crouch/terminal fall).
   `ow-client --list-heroes` prints the roster, and a prompt or `--hero
   <name|index>` picks one; the pick applies the hero's own first-person model
   and its forward speed and fall clamp (section 4c). **Still open: what binds
   runtime variable 940** - the client's dynamic-variable table and the store
   that writes it; that store is the pick. The component's consumer is also
   open. `spawn`, `heroselect`, `modeselect`, `practice`, `training`, `workshop`
   are 0 hits in the whole module image, which is why no native screen can be
   named.

**How it closes.** Item 1 yields the address of the handler that starts
something; item 3 closes by reading the spawn message's consumer. Item 4's
roster, names, marker and movement row are read and `ow-client` applies them as
a declared viewer default; its remaining half closes by reading what the spawn
does with the hero id, not by inventing a selector. Item 1 is the cheapest next
read.

## How an entry is closed

1. The answer has an address or a value in `Overwatch.exe` or the install.
2. The finding is written in the relevant `docs/formats/<system>.md`.
3. The code that uses it has a test; if it replaces a `FIT-*` row, the row is
   removed from `docs/deviations.md`.
4. The entry is removed from this list, and `STATUS-handoff.md` is updated.

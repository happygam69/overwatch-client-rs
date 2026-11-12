# Open questions: the work queue

Every open question, in the order worth attacking. The rule behind the queue:
a question is never closed by a guess. If it is not read, it stays here and the
gameplay feature stays unimplemented.

An entry is closed when:

1. the answer has an address or a value in `Overwatch.exe` or the install;
2. the finding is written in the relevant `docs/formats/<system>.md`;
3. the code that uses it has a test; if it replaces a `FIT-*` row, the row is
   removed from `docs/deviations.md`;
4. the entry is removed from this list (the live state is
   [`systems-coverage.md`](systems-coverage.md), the loop is
   [`../STATUS-handoff.md`](../STATUS-handoff.md)).

---

## The gate's wall time: seven install suites were 98% of it (2026-11-19)

**Closed 2026-11-11: the seven suites are now seconds.** The measurement
below is the "before"; the fix and the "after" are in item 1 of this list
and in MODLOG 2026-11-11. The whole gate now runs in **99.6 s**, green, with
the slowest test binary at 13.2 s (it was `index_find_real` at 935 s).

Measured on the operator's machine (12 cores) with `cargo xtask verify`, the
sequential `cargo test --release --workspace` run: of the 56 binaries whose
timings parse out of the log, **2246 s of test time** and these seven suites
account for 2218 s of it:

| suite | wall (before) | wall (now) |
|---|---:|---:|
| `ow-casc` `index_find_real` | 935.2 s | 1.5 s |
| `ow-scene` `skin_pose_gate` | 428.3 s | 4.4 s |
| `ow-tank` `effect_real` | 222.8 s | 4.0 s |
| `ow-scene` `practice_launch` | 185.5 s | 5.2 s |
| `ow-scene` `hero_draws` | 158.6 s | 2.8 s |
| `ow-tank` `hero_pick_real` | 151.9 s | 2.8 s |
| `ow-tank` `brush_real` | 134.5 s | 2.7 s |

Everything else is under 16 s per binary. The full sequential run passed 84
minutes of wall clock (compile + fmt + clippy + the suites) before it was
stopped, so the operator's "verification takes longer than the work" was
arithmetic: a unit is ~12 minutes, the gate was over an hour.

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

1. ~~Make the seven suites cheap.~~ **Done (2026-11-11), by fixing the
   reader rather than by sampling or caching to disk.** Two values that are a
   pure function of the install were rebuilt on every call, and both hot
   spots were that:
   - `SegmentEntries::key_state` rebuilt the segment's whole 18-byte-per-entry
     buffer on every call, so the per-key sweep copied tens of gigabytes. The
     bytes are now built once per segment: `index_find_real` **1154 s ->
     1.5 s**, with every key still resolved.
   - `Store::open_ckey` re-parsed the decoded encoding manifest on every
     asset open; the parse's expensive half is the espec string index (one
     u64 per byte of a 100 MB manifest). The index is now built with the
     manifest: `effect_real` **213 s -> 4.0 s**.

   The other five suites paid the same two costs through the same reader:
   `skin_pose_gate` 428 s -> 4.4 s, `practice_launch` 186 s -> 5.2 s,
   `hero_draws` 159 s -> 2.8 s, `hero_pick_real` 152 s -> 2.8 s,
   `brush_real` 134 s -> 2.7 s. Every assertion is unchanged - the effect
   census is the same 7,342 assets and 419,488 chunks.

   The weaker proposals are not needed: nothing was sampled away and no
   on-disk cache can go stale, because the cache is the reader's own state.
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
   CPU saved. With (1) done the pool has nothing left to hide: the gate's
   floor is the compile time plus the suites' own seconds.

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
- **The vertical stage (2026-11-15; the resolve read 2026-11-12).** The floor
  snap is a stand-in and it is measured: its "only down" guard was taken from
  the probe origin (`body + 0.2`), so it lifted the body up to `0.2` m per frame
  (`+0.118`, `+0.229`, `+0.165` m in three consecutive frames, `0.78` m total)
  and it teleports down by up to the `2.5` m reach in one frame. It is also
  load-bearing for the landing, because the verified leaf reports nothing for
  a sweep that starts on or inside a surface, so a sweep-based fall cannot land
  by itself. **The corner clip is fixed inside the stand-in** (2026-11-15):
  `World::overlap` asks the leaf's own "already inside" mode (`block+0x72`) at
  `radius - CONTACT_SKIN`, `depenetrate` pushes the body out, and a miss now
  moves to the depenetrated target instead of applying the residual - a
  right-angle corner that carried the body `3` cm through the second wall now
  stops it (unit test `a_corner_stops_the_body_at_both_walls`; `--wall-audit`'s
  near-vertical tunnelled frames `174 -> 27` of 8,741).
  **The client's resolve is now read (2026-11-12)** and ported to
  `ow_sim::controller`: the step stage `0x142cf5020` (a call-free leaf) pushes
  the *target* out of the contact planes - **verified** against the client's
  bytes as `fn/CharacterStepStage` (160 differential cases, 100% branch
  coverage, 8/8 mutants) - the resolve `0x142cd4610` scales the motion by the
  smallest contact factor, and the solver `0x142cf3f80` iterates that to a
  `0.005` fixed point with the cap at the sweep block's `+0x2c`. The
  factor is a **fraction** of the motion - the GJK's swept extension - not a
  depth, and the client's resolve has no residual slide; the slide is the step
  stage's. **The contact source is now shipped** (2026-11-12): the capsule
  narrowphase `0x142cfc090` is `ow_sim::collide::capsule_narrowphase`, unit
  `fn/CapsuleNarrowphase` - the whole contact record, the candidate count and
  the return flag compared against the client's own bytes, both of the
  client's shape paths, 100% branch coverage - and it does pass the GJK the
  second flag `+0xa1 = 1` and therefore carries a shape that is already
  inside the other's radius. What is still open is the **wiring**: the walk's
  contact source is still the mesh leaf, whose own rule is to refuse that
  case, and the walk has to call the narrowphase with the three frames the
  client hands it. Next anchor: the resolve's own factor consumption
  (`0x142cd4610`) and the pair build `0x142cd4d90` that feeds it. **And two
  values the walk
  does not have**: the push limit (the mover's body `+0x628`, a
  state-modifier parameter from the install, times the frame duration) and the
  round cap (`step_state[0x94]`, a runtime field). The client's rule was
  wired into the walk and measured: with the limit at zero the step stage
  cannot push and the body ends 1.5 cm inside the surface after four rounds,
  and with the leaf's own flags it goes through a wall on the second round.
  The walk's advance therefore stays the stand-in until those two values are
  read.
- **The sweep's aftermath.** `RunCharacterControllerSweep` (`0x142cbcee0`) is a
  thunk to `0x142cf3f80`, read in full: a shape-pool walk on the handle at
  `world+0x08`, the `+0x24`/`+0x28` layer/mask test, a transform test against
  the query bounds (`+0x10..+0x1c`), and a dispatch on `+0x50`. Not ported: the
  Domino bookkeeping that writes the character's shape transform and the pair
  contact lanes. The client's slide is **not** a residual projection - it is
  the step stage pushing the target out of the faces the body overlaps - and
  the walk's stand-in projects the residual instead, so the two do not agree.
  Next anchor: the pair walk's face records (the shape record's `+0xc0` normal
  quad, its `+0x18` distance and its `+0xf6` face count) and the narrowphase
  behind the factor.
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
  What is still open:
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

## 6. Character collision: the contact source is the open stage

**Read and implemented.** The sweep, the capsule values, the pair walk and
the resolve loop are read and implemented, and the walk in `ow-client` runs
on the map's level collider. The round-by-round log that got there is
[`history/open-questions-2026-11-19.md`](history/open-questions-2026-11-19.md);
the findings are in [`formats/character-controller.md`](formats/character-controller.md)
and the state in [`systems-coverage.md`](systems-coverage.md).

**What is still open.** The client's character-vs-world pass `0x142cf3f80`
is read on the shape side and on the resolve loop; what is not ported is the
runtime's own depenetration (`0x142cd4610` and the contact collection behind
it) and the pair build's world structures (the fixture list, the handle
encoding, the body records). So the walk's `8 m` grid, its slide and its
floor stage remain declared deviations - and the slide is now known to be the
*wrong* rule, not just a different one.

**The narrowphase itself is now read instruction for instruction
(2026-11-19) and shipped (2026-11-12), with four of its leaves shipped
before it.** `0x142cfc090` is the contact source: it builds the GJK
descriptor, walks the candidate triangles, and accepts the one whose residual
is the smallest. Its descriptor is now pinned field by field
(`docs/formats/character-controller.md`, "The narrowphase's own body, and its
first leaf"), with the corrections that matter: the second shape's rotation
quad is the **other shape's own quaternion** (the earlier "unwritten frame"
reading was wrong - the third to sixth rows of the table were corrected on
2026-11-12), and the motion budget slot doubles as the running minimum, so
the budget shrinks to the last accepted residual.

**The narrowphase is shipped and verified** as
`ow_sim::collide::capsule_narrowphase`, unit `fn/CapsuleNarrowphase`: the
whole `0x40`-byte contact record, the candidate count and the return flag
compared against the client's own bytes, both of the client's shape paths
walked, 100% branch coverage. Four leaves were **shipped and verified**
before it: `0x142cd5c80` (the shape's box carried into the query's frame) as
`ow_sim::collide::transform_shape_box` (`fn/CollisionShapeBox`, 264 cases,
9/9 mutants), `0x142cb8950` (the shape's head) as
`ow_sim::collide::shape_head` (`fn/CollisionShapeHead`, 120 cases, 6/6
mutants), `0x142cae760` (the type-3 mesh's own triangle record) as
`ow_sim::collide::mesh_triangle_record` (`fn/CollisionMeshTriangle`, 120
cases, 6/6 mutants) and `0x142ccf540` (the runtime's own height-field
triangle record) as `ow_sim::collide::runtime_triangle`
(`fn/CollisionRuntimeTriangle`, 120 cases, 8/8 mutants). The gather
(`0x142cfcba0`) and the runtime's height-field gather (`0x142cd0180`) are
ported inside the same unit.

**Next anchor.** `0x142cd4610` - the resolve that scales the motion by the
contact's factor - and the pair build `0x142cd4d90` that feeds it. Then the
walk's slide and floor stage can be replaced by the client's own resolve.

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
express. The CASC suite is 35/35 verified (35 unit descriptors). Still walls: `BlteFrameHeaderParse`'s
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

## 19. The asset-type registry: job id -> the loader that fills it

**Answered 2026-11-11** (the open part is the last paragraph).

The mapping is read. `JobForType` (`0x141fd8300`) indexes the u32 type table
at `0x143a7c3a0` (403 types, `0x00..0x192`) and returns the qword in the job
table at `0x1441b3cb0`. The job table is zero-fill, so the job object behind
a slot exists only at runtime - but `0x141fd89f0` is the call that *fills* a
slot, so its callers are the registration functions, and the descriptor each
one passes holds the job id at `+8`, the **load** callback at `+0x48` and the
**release** callback at `+0x78`.

32 of the 33 jobs are resolved; the whole table is in
`docs/formats/asset-types.md` section 1b and `python dev/fn.py jobs` prints
it. The write was invisible to a displacement scan because the slot address
is computed with a `lea` and the store then uses no displacement.

**What is known.** 33 job ids are used (`0..39`, with gaps); job 10 serves 194
of the 403 types; 163 types map to job 0 (no loader). The type table is a
compile-time constant, so the ids are fixed by the build. The read confirms
two claims the project had made by inference: the material asset (type
`0x008`) is job 2 with the loader `0x141b1a8e0`, and the material data (type
`0x0B3`) is job 28 with the load callback `0x141b1a7b0` and the finalize
callback `0x141b1a790`.

**Still open.** Job 4 (type `0x27`): its registration stores the job id in a
way this read does not resolve. And the descriptor's other fields are not
read - only `+8`, `+0x48` and `+0x78` are. The registration functions also
deobfuscate the asset's **name** with the same XOR-with-a-key loop
`formats/chunk-kinds.md` documents (the first one, at `0x1439a70a4`, is
`MODEL`), so every registered name is decodable - but the names are not
listed yet.

**Next anchor.** `0x1441b3cb0` (the job table), `0x141fd89f0` (the register
call), `0x141b19160` (one registration), `docs/formats/asset-types.md`,
`dev/fn.py jobs`.

---

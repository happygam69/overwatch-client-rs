# MODLOG
Milestones only. Findings and addresses live in `docs/formats/`; current state
in `STATUS-handoff.md`; the work queue in `docs/open-questions.md`.

Rotation: this file holds the current entries. Older ones move verbatim to
`docs/history/modlog-YYYY-MM.md` (one file per month) at each rotation; no
entry is rewritten after the fact.

Rotated 2026-11-12: everything from the `2026-11-19 - gate: the unscoped
verify is suspended` entry down is now in `docs/history/modlog-2026-11.md`
(44 entries, moved verbatim). This file keeps the entries written after them.

## 2026-11-20 - two more through the pipeline: 2 for 2, and the mistakes it caught

Asked to take two functions and see how it goes, `dev/queue.py --next` was
replaced for the occasion by a sharper question - *what has no calls at all, and
what only calls things already proved* - which is the composition order the
playbook §9 asks for but the queue did not express. Answer: **60 pure leaves**
between 30 and 200 bytes, of which one is a wrapper whose only callee had just
been proved.

- **`fn/KeyTableFindMask`** (`0x141b330c0` -> `ow_sim::keytable::find_mask`), the
  consumer of `fn/KeyTableFind`: forwards four arguments, then
  `1 << (index & 63)`, or `0` for `-1`. 304 cases. The corpus gained a 70-record
  table with the match at **index 64**, because that is the only input on which
  the shift's `cl` masking is observable: without it the mutant that drops the
  mask survives, because Rust's `<<` and x86's `shl` disagree exactly there. The
  survivor was then declared equivalent with that reason (x86 masks the count in
  the instruction, so the explicit mask is redundant on this target).
- **`fn/CameraChainScale`** (`0x141ffe590` -> `ow_sim::chain::chain_scale`), a
  recursive product up a parent chain, stopping at a node with no link or with
  bit `0x400000000` set. 184 cases, 100% branch coverage. Six chains in the
  snapshot: one node, a product of three, the stop bit on the middle and on the
  root, a zero scale, and eight deep.

Three mistakes, all caught by the harness rather than by review:

1. **I overwrote `crates/ow-sim/src/camera.rs`** by writing a new module to a
   path that already existed - it holds the verified camera-basis arithmetic.
   `git checkout` restored it and the new code went to `chain.rs`. The lesson is
   mechanical: check the path before `write`, and `git status` caught it in the
   same minute only because the probe stopped compiling.
2. **Argument order.** The chain function takes ONE argument (the node in `rcx`);
   the unit passed a guest base first, so the client read the base as its node
   and returned `0.0` against the port's correct product. Fixed by putting the
   node in `rcx` and the base in `rdx`, which the body never reads - the probe
   then translates the parent links without changing what the client does.
3. **Arena overlap.** The mask fixture's key sat at `ARENA + 0x400`, inside the
   70-record block that starts at `ARENA`. The key overwrote records 34-35, and
   the divergence showed up as a mem channel difference rather than as an
   obvious fixture bug. The key moved to `ARENA + 0x1000`.

Both reads are recorded (`docs/formats/camera.md`, `docs/formats/domino.md`) and
both names are rows in `docs/function-names.csv`, which the batched
`gh_apply_names.py` now holds until 20 accumulate.

## 2026-11-20 - first function through the new pipeline, and the loop made real

`0x141b331f0` ported end to end as `fn/KeyTableFind`
(`ow_sim::keytable::find`) - the first function taken from `dev/queue.py --next`
rather than from a reading session. It is a keyed search over a
`0x30`-byte-stride record array: prefilter on `+0x10`/`+0x18` (and `+0x20` when
the caller's `r9b` is set), a **sticky** partial-match flag armed by any record
with `+8 != 0 && +0 == 0`, and the **last** candidate wins. 304 differential
cases, 100% branch coverage (12/12 sites), 11/12 mutants killed with the twelfth
declared equivalent (`count <= 0` widened to `count < 0` returns -1 either way
at `count == 0`). Read recorded in `docs/formats/domino.md`; named in
`docs/function-names.csv`.

The harness caught one real mistake in the port before it shipped: the client's
`mov eax, ebx` **zero**-extends, so `-1` reads `0xffffffff` in `rax`, and the
probe was sign-extending it to `0xffff...ffff`. Two of my own unit tests were
also wrong in `keytable.rs` (a key whose `+0x00` is zero is *always* partial, so
a differing record is a candidate, not a rejection) - the differential passed
while they failed, which is the right way round.

**The loop is now actually short.** `dev/verify_changed.py` selected by crate
before, so one module change dragged in 60+ units; it now selects by the
**entry's own source file** (`candidate_rust.entry_sources`, the same map the
verdict digest binds to):

```text
crates/ow-sim/src/keytable.rs  ->   1 unit   (fn/KeyTableFind)
crates/ow-sim/src/scope.rs     ->   1 unit   (fn/ScopePop-steps)
crates/ow-sim/src/collide.rs   ->  17 units
dev/verify_harness/runner.py   -> everything (it decides verdicts)
dev/queue.py                   -> nothing
```

One unit's full ladder - differential, coverage, mutation - is **29 s**
measured, which is the cost of a single-module change.

Also changed: `dev/gh_apply_names.py` now applies names in **batches**. The
default run asks the project what it already has and writes only the rows that
differ, and only once `--min 20` (default) have accumulated; `--addr` stays for
a single row and `--all` forces the old behaviour. Renaming is one HTTP call per
row, so a per-row run wasted minutes for nothing - the CSV is the record either
way. Docs that prescribed the per-row call (`AGENTS.md`, `architecture.md`,
`ghidra-project.md`, the playbook) now prescribe the batch.

## 2026-11-20 - the tier refactor: three proofs, one queue, a 30-second loop

The project had one proof procedure and it was both too strong and too slow:
the emulator ladder is right for what the player feels and pointless for a codec,
and running it over everything took an hour that grew with every unit. `docs/tiers.md`
now defines three tiers - **A feel** (emulator, done `verified`), **B translation**
(exact over the install's own data, done `proved`), **C structure** (first
divergence of a recorded run, done `linked`) - and AGENTS.md rule 1 says the
proof is chosen by tier and never skipped. The rule's own text already named the
three instruments; the gate had narrowed it to one.

Two tools carry the loop, and the measured numbers are theirs:

- **`dev/verify_changed.py`** proves only what a change can affect. Measured on
  the tier-B set: **40 of 41 real-data proofs green in 160 s** (`index_find_real`
  needs `--slow`: minutes), against roughly an hour for the full ladder.
- **`dev/queue.py`** turns progress into a per-function number and the work into
  a query: `--next N` screens candidates with `dev/cluster.py`, so a function
  that cannot be proved is visible before it is read. Its first honest number:
  **133 of 3,955 known functions done (3.4%)** - tier A 61, tier B 72, tier C 0 -
  out of the image's 119,802 roots, of which 115,927 carry no system tag and are
  not queued. Two earlier readings were wrong and are recorded here because the
  fixes are the point: a crate lookup that matched the wrong symbol name left
  90 of 121 units unattributable (so a unit fell to a default tier of A and data
  code was counted as emulator-verified), and a proof on a machine with no
  install exits 0, which recorded a green proof that never ran. Units are now
  attributed by `rg` over three symbol shapes; a proof with no install records
  `skipped-no-install`; a tier-B proof may not credit a tier-A function.

Tier-B coverage needed a way for a test to say *what* it proves, so `*_real.rs`
files now declare `//! proof-covers: 0x… ` - a green proof with no header proves
code but contributes no function to the count, which is the honest state rather
than an inferred one.

Docs refactored to match: `docs/port-playbook.md` is now organised by tier (with
every hard-won rule and measured number kept), `docs/systems-coverage.md` carries
a tier column and defines the three done-states, `README.md` attributes the
existing work to its tier, `STATUS-handoff.md`'s loop is the tier loop, and
`docs/open-questions.md` gained a header explaining that it is the record, not
the queue.

## 2026-11-20 - the guard followed to the end: it is the object model

`dev/cluster.py` now resolves the hot-patch guard instead of calling its target
opaque. It follows store site -> setter region -> the setter's callers -> the
last `lea <stored reg>` before the call, and reproduces the manual read exactly:
`0x1442a0158` <- `0x141bb6660`, `0x1442a0160` <- `0x141b328e0`, `0x1442a0168`
<- `0x14053a420`. Three bugs came out of building it (a section-frame mix-up, an
off-by-`0x1000` disassembly base, and capstone's `X86_OP_REG` = 1 not 2), all
caught by asserting the three known values.

**The portable count did not move: 135 of 2,814 (4.8%).** The slot resolves to
`jmp 0x141bb00e0`, whose body is `call [rax+0x30]` - so "call target with no
index entry" (2,276 clusters) and "virtual call" (1,769) are largely the *same*
wall, and the ceiling is the vtable. That corrects the previous entry's claim
that resolving the guard globals was the lever with volume: it is not.

What the change buys is a truthful message (`via 0x142cadee0[slot] ->
0x141bb6660 -> 0x141bb00e0: virtual slot [rax+0x30]`) and two smaller
classification fixes: a pure `jmp [IAT]` thunk is now reported as an import
thunk rather than an unindexed body, and `chain_to_root` names every hop.

## 2026-11-20 - the hot-patch guard resolved: a zero-fill slot with a readable value

Following up why 2,276 clusters stop at "call target with no index entry": the
dominant address is `0x142cadee0`, `mov r10,[0x1442a0158]; test; jmp r10` - a
dispatch through the `.data` zero-fill tail. The slot turned out to be readable
from the image after all. `0x1442a0158` has a writer (`0x142cadf35`) inside the
setter pair `0x142cadf20`/`0x142cadf50`; the setters have one caller,
`0x141ff3190`, registered as a *data pointer* at `0x143360ef8` (an initializer,
not a call), and it installs three `lea` constants: `0x141bb6660` ->
`0x141bb00e0` (virtual call `[rax+0x30]`), `0x141b328e0` -> `0x141baff90`
("call the singleton's vtable slot `+0x40`"), and `0x14053a420` (`ret 0`, a
no-op hook).

So the guard is statically resolvable and the chain ends at a vtable dispatch -
an object-model question, not a data one. The rule this adds to
`docs/ghidra-project.md`: a zero-fill slot is readable from the image when its
writer is in-line code with static operands, and not when it copies an argument
or loops a table (the Wwise bank key at `0x144293db8` has readers and no writer,
so zeros *are* its runtime value).

## 2026-11-20 - why the portable set is 4.8%: the client's three runtime hops

Asked why only two dozen clusters verify, the answer was measured instead of
argued, and the first answer was wrong. `dev/cluster.py` reported 24 (1.5%) on a
narrow window; corrected it is **135 of 2,814 unported heads (4.8%)**, and the
two corrections are both tool bugs, not client facts:

- **A forwarder is not a stub.** `0x141c23490` (`mov ecx,edx; mov edx,r8d; jmp
  0x141bb00e0`) has no `.pdata` extent but tails into a real in-scope function.
  Screening it as opaque blocked 113 clusters. The screen now follows a
  straight-line `jmp <root>` before it blocks.
- **`0xCC` is padding.** Decoding past the end of a function read `int3` as an
  ambient trap and blocked 589 clusters that never execute one. The screen stops
  at `int3`.

What blocks the other 95%, per cluster: a call target with no index entry 2,276
(docminant case: the hot-patch guard `mov r10,[rip+g]; test r10,r10; jmp r10`,
whose `g` sits in the `.data` zero-fill tail and is filled at startup - 34
clusters on that one address alone), a virtual call 1,769, an import thunk 1,694,
an indirect call 1,135, `rdtsc` 662, `cpuid` 598. For scale: **470 of 3,875
scope functions (12%) are individually runnable**; a pure leaf is the exception,
not the rule. So the ceiling on cluster ports is set by three runtime hops - the
CRT, the vtable, and the client's own guard - and a cluster port must cross one.

The one lever with volume is therefore not more reading: it is a declared
platform model (memcpy/malloc-family) plus resolving the guard globals by
finding their initializer in the zero-fill tail.

## 2026-11-20 - §9 and §6.5 dogfooded: six clusters to find one that verifies

The two sections with no tool and no measurement behind them were run for real.
Outcome: `dev/cluster.py` (the missing §9 picker, screened), one new unit
`fn/ScopePop-steps` (the second steps unit, and the first whose feedback is a
*scalar* - the count the previous pop wrote), `ow_sim::scope::scope_pop`, and
guest-address translation in the `ow-sim` probe, which is now the second probe
that resolves guest pointers.

The pick is the finding. Of the 1,578 candidate heads in the scope, **24
clusters (1.5%) are portable**: the rest reach an import thunk, a virtual slot,
a kernel transition or ambient CPU state somewhere in the cluster, and the
blocker is usually *outside* the scope. Four screens separated them, each now
written into §9:

1. A **direct call into code with no `.pdata` extent** - `0x142cee300` calls
   `0x142cadf00`, a guard-dispatch stub with no function root, no edge row and
   no triage state, two hops from an import. An in-scope-only walk never sees it,
   so the screen decodes calls from the body rather than trusting the edge table.
   The first three picks (a QPC timer cluster, `0x142cf7c10`, `0x140dbc940`) all
   died here.
2. **`movaps` needs the fixture's base 16-byte aligned**: an unaligned base
   faults on the rounded address, so the state base moved to a clear one.
3. **The state region must not land on the emulator's own blocks.** The first
   `ScopePop` snapshot put the container inside the oracle's arena and every
   call zeroed it.
4. **The snapshot's regions must not overlap each other** - the first snapshot
   laid the arrays at the container's stride, so the client processed one pair's
   bytes as another pair's elements and diverged in the *unported* flag path.
   Both faults are now asserted in the fixture.

`fn/ScopePop-steps` pins the element flag byte to zero: its non-zero branch
calls the unportable stub, so `unreachableBranches` names the site and calls it
a fixture-pinned edge, and `acknowledgedGaps` names the stub. §9 now says that
is honest only for data the client reads, not for a control-flow edge.

Gate: `cargo xtask verify --fast` green; the ladder for `ScopePop-steps` is
recorded in the unit.

## 2026-11-20 - the playbook, executed on a function nobody had read

`0x142697ac0` taken from §1 to a `verified` unit by following
`docs/port-playbook.md` literally: one command per step, no improvisation. One
new unit, `fn/SegmentAabbTransform` (`ow_sim::aabb::segment_aabb_transform`,
`crates/ow-sim/src/aabb.rs`): 200 cases, every channel, 10/10 mutants. It
transforms a segment's two 4-wide endpoints into the two AABB corners in a
matrix's frame - `ext = (e.y*|m1| + e.x*|m0|) + e.z*|m2|`,
`cen = ((c.x*m0 + t) + c.y*m1) + c.z*m2`, low corner then high, all four lanes.
`formats/static-collision.md` records the read.

The run found nine places where the procedure was thin or wrong, all fixed:

1. §1's queries need `sqlite3`; the fallback is now written out, together with
   the query that implements the stated preference order. Measured while doing
   it: **one** function in the 3,875-function scope has no callee and no
   dispatch, so preference 2 is effectively unavailable.
2. §1.5's screening table had no row for **ambient CPU state**. The first pick
   was `0x142701520`, which is `stmxcsr` + `mov eax, [rsp]` - unprovable, and
   the table would have sent a session into it.
3. §3 gains the NaN rule: `addps`/`subps` return the destination's NaN, scalar
   Rust leaves that to the backend, and the emulator *clears the sign*
   (`0x7fc00001` whichever operand carries it, measured), so a two-NaN case is
   not comparable. Also measured: intrinsics hide the arithmetic from the
   mutation stage (2 mutants, both the `0.5` literal) while the scalar form
   gives 10 and kills all 10.
4. §4 said "return the mutated buffers"; the protocol needs **one buffer per
   `ptr` argument**, and a short vec is `missing pointee in response`.
5. §5 gains the NaN-operand rule and the exponent mask a random tail needs.
6. §6 gains `allowedImageReads` ranges: a 16-byte SIMD constant declared as one
   address fails with `oracle reads outside declared args/stack at +8`.
7. §8 gains three symptom rows for the faults this run actually produced.
8-9. §12's checklist gains the ambient-state screen and the range form.

Tool fix found the same way: `dev/read.py`'s caller window decoded from an
unaligned offset and printed nonsense; it now decodes from the caller's own
chain root (`dev/read.py` `caller_window`).

## 2026-11-20 - the read is now one command, and the procedure is written down

`dev/read.py <addr>` writes the evidence pack a reading session needs, joined:
the extent and pieces, callers/callees, **the field map** (every memory access
folded through a conservative register dataflow to `argN+offset`, with
write/read/addr and `+idx` for indexed bases), **globals classified**
`raw`/`zero-fill`/`unmapped` from the image's section table with read/write
counts and a value preview, switch tables, immediates, the argument setup at
each call site, the units and docs that already cite it, and an evidence
skeleton for `docs/formats/<system>.md`.

Measured: on `MoveCharacterController` the map reproduces the client's C
exactly (arg0+0x000/0x010/0x024, arg1+0x190/0x1b0/0x2b8, arg2+0x000/0x010), and
on `0x140f84590` it flags the known BSS trap `0x143c3c620` plus 14 more globals
- a check that used to be a `pe_layout.py` call per address.

`docs/port-playbook.md` §2 is rewritten around it: the pack first, then reading
the C against the map with the five rules that encode this project's known
traps (BSS is not a value, `addr` is a handed-on pointer, `+idx` hides the stride,
keep the client's f32 association, the disassembly decides `f32` vs `double`).
§1 gains the measured selection criteria from S4 (dispatch fan-out, inline
functions ride on their caller).

## 2026-11-20 - what one unit actually buys, measured

Two new read-only tools: `dev/throughput.py` (static callee closure per unit, and
what the verified set reaches) and `dev/reach.py` (which functions a unit's
corpus **executes**, under `UC_HOOK_CODE`). The measurement corrects the plan's
own arithmetic:

- The static closure is an upper bound and badly one: the 14 units with the
  largest closures execute **31 distinct functions between them**;
  `fn/ApplyMoverSettings` reaches 491 statically and executes **2**.
- Only **510 of 3,875** scope functions contain any memory-indirect dispatch, and
  the largest fan-out is **12 slots** (`MoverUpdateFrame`: 8 slots, 14 sites).
  That is why one call walks one path: the leaves sit behind populated vtables.
- A fresh 6-unit verification takes **90 s** -> 15 s/unit, so all 3,875 would be
  **~16 h of machine time**. The harness is not the bottleneck; the read is.
- Of **115** verified units, **51** have an entry in the gameplay scope; those
  touch **70** in-scope functions (1.8%) by the entry + direct-callee bound.

`docs/port-plan.md` S4 records it and drops S3's un-measured "toward ~400 units"
estimate until the world lands, since the trace gain exists only where a slot is
populated.

## 2026-11-20 - the second steps pilot is a real driver, and the harness was wrong

`fn/ApplyCharacterControllerMove-steps`: the client's `ApplyCharacterControllerMove`
(`0x142cd5370`) called three times in a row on one character inside the state
region, a different velocity each step, every channel compared after every call.
200 cases, coverage 9/9 branches (100%), **10/10 mutants**, verified. The port
needed no change - the first run's 1-ULP divergence was the harness re-seeding
its state region inside the entry (`state.snapshot` instead of `state.buf`), so
three calls each normalized the original quaternion while the client normalized
its own output.

Two harness bugs found and fixed: the probe now keeps one live buffer and
re-seeds it before each call unless the unit sends `persist on` (both probes,
one rule), and the mutation stage replays **every** case of a steps unit instead
of case 0 only, resetting the mutant's state at each case's first step - that
one was worth 50% -> 100% kill rate on this unit, because the discriminating
NaN-payload and boundary-handle cases sat in the tail. `docs/port-playbook.md`
§6.5 carries both traps; `docs/port-plan.md` S3 the numbers.

## 2026-11-20 - steps units exist, and the pilot says where they pay

A harness unit can now call its entry several times per corpus case and compare
the state region after **every** call (`docs/port-plan.md` S3). What landed:
`steps: true` + a `make_steps(i)` fixture, `EmuOracle.persist_state` (the state
region survives between steps and is re-seeded per case), one emulator for all
cases, `RustCandidate.reset_state` + a `reset` command in the probe, and a
mutation stage that replays a case's steps in order.

**Pilot green:** `fn/ShapePoolAlloc-steps` - the shape pool's free list walked
1-4 pops per case on one pool inside a state region: 16 chained pops over 8
pools (chain lengths 1-4, a non-contiguous `2 -> 0` chain, a saturated used
count, a high-water mark that must not fall), 80 cases, every channel per step,
**6/6 mutants killed**, differential 0.2 s for 160 client calls, one unit.
`fn/ShapePoolAlloc` still passes on the old single-call path.

**The finding that changes the plan:** for that leaf the steps unit is not
stronger than the single-call one - a single-call unit already compares the
whole mutation of one call. Steps pay only where a call's writes feed the next
call's control flow, i.e. on a **driver**. Building one emulator per case cost
12x the calls it ran (1.2 s -> 0.2 s differential once it was shared); the
`u64`-guest-address rule and the two `candidate_rust.py` bindings are the two
traps a new steps unit hits, both written into `port-playbook.md` section 6.5.
Next: guest translation for `ptr` arguments so a driver can reach a world, then
`fn/MoverUpdateFrame-trace`.

## 2026-11-20 - a read packet in one command, and the trace-unit plan

`python dev/fn.py packet <addr>` is new: extent and chain pieces, named
callers/callees, instruction shape, immediates ranked with branch targets
excluded, bounds/tags in `2..0x200`, **switch tables whose entries all land
inside the body**, every rip-relative data reference with its printable
string, and whether the client's C is cached. It is the answer to a reading
session that spent ~40 tool calls on archaeology: on `0x14217f060` it prints
the 114-dword table (112 distinct) that makes the body opaque, and on
`0x1423aca10` it prints PCRE's compile-option strings (`UTF8)`, `UCP)`,
`LIMIT_MATCH=`, `Error text not found (please report)`) without a decompile.

Also written: `port-plan.md` section S3, the trace-unit design. Measured
finding that shaped it: a `ptr` argument on the candidate wire carries the
pointee's bytes and every probe arm re-seeds them per call (`verify_probe.rs`
`Arg::Ptr(b, r, g) if r.is_empty() && g.is_none()`), so **state does not carry
between calls through an argument** - which is also why the one state-mode
pilot, `fn/OrthonormalizeCameraBasis-state`, is divergent. The plan fixes that
with a session buffer in the probe (additive to the wire), a `steps: true`
unit flag with a `make_steps` factory, and `fn/ShapePoolAlloc-steps` as the
pilot (pop the free list `2 -> 0 -> -1` twice). Not implemented this session.

## 2026-11-20 - the largest unread body is the reflection value reader

The image's largest body this project has not read is `0x14217f060`
(35,848 B, four `.pdata` pieces) in the reflection value system. It cannot be
decompiled in one pass - Ghidra reports `decompile_failed: timeout` - and the
reason is structural: its **switch table sits inside its own extent** at
`0x142187aa0..0x142187c64`, 113 four-byte RVAs (112 distinct, all targets in
the body, guarded by `cmp eax, 0x71`), so a linear walk decodes the table as
code and desyncs. What is read: the entry contract (a string-carrying object
in, a variant out with an optional output pointer and an early exit on an empty
string), the three callers and the type token `0x143d04610`, the variant record
(tag at `+0x0c`, arrays of `0x10`/`0x20`, tag `0xff` = unspecified), the
destructor `ValueDestroy` (464 callers) and 4 of the 87 helpers it calls.

The find worth carrying: the value system's **format strings are XOR-encrypted
in `.rdata` and decoded in place on first use** (8-byte key, length + decoded
flag dword, ciphertext at `+0x0c`). Decoding them names the whole vocabulary -
`%d`, `%f`, `%.4f`, `%016llx`, `[%f, %f, %f]`, `(%.3f, %.3f, %.3f)`,
`(%d, %d, %d, %d)`, `Array of %d`, `Object`, `Unspecified`, `true`/`false`.
Page: `formats/values.md`; ledger: `systems-coverage.md` (not in scope).

## 2026-11-20 - the image's largest function is PCRE, and it is middleware

The 58,068-byte `0x1423add80` that `dev/fn.py biggest` left open is **PCRE 1's
matcher body**, not gameplay. One caller function (`0x1423bc3c0`, two sites)
rejects a block whose first dword is not `0x50435245` (`PCRE`) and takes the
exec argument list; its sibling `0x1423bd1c0` switches on `PCRE_INFO_*` and
`0x1423aca10` references PCRE's compile-error text table at `0x14344c480..`.
The game-side binding is 11 functions (2,240 B): a holder with the compiled
block at `+0x20`, compile with the fixed option base `0x20000801`
(UCP | UTF8 | CASELESS) plus three caller bits, five exec shapes and four
"match any holder in this array" variants. The size is its own code (three
contiguous pieces, 169 + 55,470 + 2,429). The release is **not pinned** - the
image carries no PCRE version string - and one latent overrun is recorded, not
reproduced: the named-capture path loops over the pattern's capture count but
reads a 15-pair ovector. Page: `formats/regex.md`; ledger: `systems-coverage.md`
(not in scope), `ow-tank-1to1.md` section F.

## 2026-11-20 - the effect builder is read, and the crate's biggest function is measured

`python dev/fn.py biggest ow-tank` is new: it ranks the client functions behind
a crate by true extent, at `--depth 0` (the crate's own citations), N, or `all`.
The answer for `ow-tank`: `ApplyStateModifiers` (11,620 B, already verified) is
the largest it cites, `EffectComponentBuild` `0x142e4a780` (9,432 B) the largest
it cites and had not read, and `0x1423add80` (58,068 B) the largest anywhere in
reach - unnamed, recursive, and not decompilable in one pass, so it stays open
with its call chain written down in `ow-tank-1to1.md` section F.

**`EffectComponentBuild` is read in full.** Its prologue is now in
`formats/effects.md`: the header copy from payload `+0x20..+0x75` into runtime
`+0x60..+0xbb` (with the five payload bytes it skips), the **two f32 runs
quantized to Q12** at runtime `+0xc0` from payload `+0x00`/`+0x08` with the
count at `+0x66`, the 16-byte-aligned component payload whose offset the record
keeps at `+0x8c`, the kind slot at `+0xb8`, and the two optional array
sub-records (`+0x18` through `FUN_142e4fbe0`, `+0x20` as a two-run copy).
The crate's `header::ARRAYS` and `header::DWORDS` were **wrong by 0x10**: they
used record-relative offsets in a payload-relative frame. Fixed, with the
decompile cited.

Two measurements that matter more than the offsets:

* **The two Q12 runs are empty on this install.** The count byte at
  `payload + 0x66` is `0` on all 7,342 sampled effects, so the run path is read
  from the builder and never exercised by this game's data. `effect_real` now
  prints the histogram and asserts the run windows stay inside the payload, so
  a patch that ships runs is visible.
* **The kind table transcription is verified.** Reading `0x143610490` gives
  0x35 eight-byte records `{u32 tag, u8 slot, 3 zero}` with slot `i` at record
  `i`, and the tag stored as a little-endian dword (`BLEE` on disk for `EELB`).
  All 52 non-empty tags match `effect::KIND_TABLE` exactly, in order.

## 2026-11-20 - the hero capsule test caught up with the cap

`hero_select_real::every_hero_declares_a_collision_capsule` had been red since
the body's radius cap was read: it asserted that the five-plus large heroes
carry a `0.7` m capsule, but the client writes `min(0.5, radius)` into the
body's copy (`0x140cb7700`), so every hero that declares `0.7` collides as
`0.5` and the test could never pass. It now reads the **declared** radius from
`STUCharacterMoverComponent` alongside the capped capsule and asserts the cap
itself (`capsule.radius == min(0.5, declared)`) plus the declared `0.7` count
(15 of 54). Two other anchors in the same test were stale for the same reason:
hero 7's `0.7 / segment 0.4` is `0.5 / 0.8`, and hero 657's `0.54` reads back
as the capped `0.5`. And the "hero 458 lands on the `0.1 * r` segment floor"
claim is false under the cap — it measures `0.47` against a `0.05` floor — so
`formats/character-controller.md` now says no hero on this roster reaches the
floor, and the floor rule is asserted as an invariant instead. The crate's
scoped gate is green on this suite again.

## 2026-11-20 - the map chunk bodies are asserted, not just tallied

`docs/formats/maps.md` listed the per-kind placeable counts and said they were
"pinned by `crates/ow-tank/tests/mapchunk_kinds.rs`". They were not: the test
asserted only that *some* placeable and *some* reference existed. It now
asserts, for every kind a chunk's GUID declares: the placeable walk is clean,
every body fits the kind's struct, and every typed reference resolves. The
install meets it on eight kinds — 6,880 model-group, 9,076 light, 39,156
entity, 2,641 sound, 2,734 effect, 587 area, 98 text and 1,862
reflection-point placeables — with two measured exceptions, both now in
`formats/maps.md`.

**The reflection-point body is not a fixed 168 bytes.** One chunk
(`0x0dd000043f7d3ca0`, 62 placeables) carries bodies of 152, 168, 184 and 200
bytes in the same file, all with valid transforms, so 22 of the 1,862
reflection-point placeables have no room for the tail the reference reads
(seven floats and five ints past `+0x78`). The body read is now bounded by each
placeable's declared size, so a short body yields defaults instead of the next
placeable's UUID — the same rule the model body already followed. What the
variable tail *is* needs the client's per-placeable dispatch
(`0x14209fc90`), which is not read.

**The sequence kind (`0x18`) is not a parse failure.** The sweep printed "none
readable" for all twenty sampled sequence chunks; they are all **not
downloaded**, so the kind has never seen a real sequence chunk. The per-kind
report now separates "not on disk" from "could not parse" instead of
conflating them, which is how the difference was found.

## 2026-11-20 - the sound bank container is read, and 1,045 banks parse

Type `0x043` was the one measured parse hole left in the census: 356 of 356
sampled assets opened as `BKHD` and every one was counted *unparsed*, because
the module tried the Wwise **media** container and gave up on the bank. The
container is now read from the client's own code — `WwiseBankHeaderParse`
(`0x1428fc300`) and `WwiseBankLoad` (`0x1428fa3a0`), both named in
`docs/function-names.csv` — and implemented as `ow_tank::wwise_bank`.

What the read settled: the `BKHD` payload at 0..0x28 (version, bank id,
language id, alignment, bank type from version `0x8f`, a 16-byte tail), the
version window `0x76..=0x91` with a media-only rule below `0x91`, the 8-byte
section headers and the eight ids the dispatcher names, and `DIDX` as flat
12-byte records with the client's floor count. The first 16 header bytes are
XORed with a key table at `0x144293db8` that is `zero-fill` `.data` with two
readers and no writer, so the XOR is the identity on this install.

**The install agrees.** All **1,045** readable banks parse: every one is
version `0x91`, `BKHD+HIRC` on 1,044 and one init bank carrying
`INIT+STMG+HIRC+ENVS+PLAT`, one `BKHD` payload declares `0x30` where the
reader consumes `0x28` (the client skips the remainder, and so does the
reader), and **no bank here carries `DIDX`/`DATA`** — this install keeps audio
in the `0x03F`/`0x0BB` payload assets, so that path is synthetic-only and is
said so in `docs/formats/sound.md`. The census line is now `parsed=356,
as-bank=356, unparsed=0`.

Also corrected on the way: `docs/formats/content.md` said the catalog type
`0x063` "parses zero of twenty readable assets". It parses twenty of twenty;
what reads empty is the two **array fields**, because every catalog on this
install is a **version 1** asset and v1 writes no per-instance field bags —
the raw reader has no class layout to place `m_assets` with, while the typed
layer reads 2,918 GUIDs out of one. `docs/open-questions.md` item 13 said "no
v1 asset ships here"; twenty do.

## 2026-11-12 - the capsule narrowphase is shipped and verified

The **capsule narrowphase** `0x142cfc090` - the character's contact source -
is now ported and proven against the client's own bytes. It is
`ow_sim::collide::capsule_narrowphase`, with the candidate gather
(`0x142cfcba0`) and the runtime's height-field gather (`0x142cd0180`) beside
it, and the unit `fn/CapsuleNarrowphase` compares the whole `0x40`-byte
contact record, the candidate count and the return flag against the real
function. Both of the client's shape paths are walked - the type-3 mesh's own
records and the runtime's height field - with real rotations in all three
frames the client is handed, with contacts, without contacts, with the
gather's list empty, and with the count left negative by the gather's "the
old candidates still stand" path.

**The read was corrected on the way.** The descriptor table's third to sixth
rows were wrong a second time: the second shape's rotation quad is the
**other shape's own quaternion** - a real value the caller passes - and the
sweep's direction is the sixteen-byte copy the family dispatch hands over,
not the other way round. The client's `SHUFPS` reads its two sources from
different registers, and the differential is what settles which is which: the
corpus turns the other shape's frame in every case and the client's answers
follow.

**Two more findings the port had to take.** The narrowphase builds its own
transform block and it is **not** the gather's plain inverse rotation: its
first cross product takes the `y` and `z` of the rotated vector from the
other shape's position and its `x` from the frame's own. And the runtime's
height-field gather advances its triangle counter by two for **every** row it
walks, the rows it skips included.

The narrowphase's accept rule - the residual below the running minimum in
all four lanes - has an edge that turns a candidate down, and it is reached
when a residual is exactly zero: the first candidate sets the minimum at zero
and the next one is not below it. The corpus carries a `NaN` in the capsule's
frame to reach it.

With this, the contact source is no longer the open stage of the collision
system. What is left is the **wiring** - the walk still takes its contacts
from the mesh leaf, whose own rule refuses the case the narrowphase carries -
and then the resolve `0x142cd4610` and the pair build `0x142cd4d90`.

## 2026-11-19 - the collision contact source: the narrowphase read, and two of its leaves shipped

The walk's response - where the body ends up after it touches something - is
the open half of the collision system, and the client's own answer comes from
the **capsule narrowphase** `0x142cfc090`. That function is now read
instruction for instruction (1,016 bytes over five `.pdata` pieces), and the
read corrected two rows of the page that had it wrong.

**The GJK descriptor, field by field.** The narrowphase builds the
closest-point solver's descriptor and runs it once per candidate triangle.
The descriptor's layout is the one `fn/ClosestPointGjk` already drives, and
the narrowphase writes it in that order: the triangle is the **first** set
(`FUN_142cc5ba0` builds its head at `+0x00`), the capsule is the **second**
(`FUN_142cb8950`, head at `+0x20`), `+0x40`/`+0x50` are the first set's
translation and rotation, `+0x60`/`+0x70` the second set's, `+0x80` is the
**direction** and `+0x90` the **motion budget**. The page had called `+0x60`
"the motion" and `+0x80` "the other shape's transform", and had attributed
them to the wrong arguments of `FUN_142cc5600`; that is corrected in
`docs/formats/character-controller.md`.

Two findings change the port. **`+0x70` is nobody's value**: `FUN_142cc5600`
copies sixteen bytes out of its sixth argument and hands the narrowphase that
copy, and the narrowphase writes the *next* sixteen bytes into `+0x70` - the
caller's unwritten frame. And **`+0x90` carries two things**: it is the
motion budget the GJK reads, and it is also the narrowphase's running
minimum, so every accepted contact overwrites it and the budget **shrinks** to
the last accepted residual.

**Four leaves of the cluster are shipped and verified.**

- `0x142cd5c80` (95 straight-line SSE instructions, no callee, no branch)
  carries a shape's local box into the query's frame. It is
  `ow_sim::collide::transform_shape_box`, unit `fn/CollisionShapeBox`: 264
  differential cases against the client's bytes, the whole 32-byte output
  compared, 100% branch coverage, 9/9 mutants killed. The port keeps the
  client's order, which is visible in the last bit: the diagonal is
  `1 - |2q1*q1| - |2q2*q2|`, and the two absolute-value masks differ
  (`0x1430a4b40` is `0x7fffffff` on lanes 0..2 and zero on lane 3). Three
  lane-3 mutants are declared equivalent, with the proof that the output's
  fourth lane does not depend on the quaternion's `w`: the rotation's
  fourth-lane terms cancel pairwise and the extents' fourth lane comes from
  the diagonal, whose fourth lane the three-lane mask zeroes.
- `0x142cb8950` builds the shape's **head** - its points, their count and its
  radius - by the shape entry's type byte and index, out of the world's three
  shape pools. It is `ow_sim::collide::shape_head`, unit
  `fn/CollisionShapeHead`: 120 differential cases, the whole `0x20`-byte head
  compared, 100% branch coverage, 6/6 mutants killed. The head carries an
  **absolute** address, so the world's pool pointers are absolute arena
  addresses in the fixture and the probe resolves them back into the blob -
  the pointer is compared, not masked. The port keeps the client's two
  details: the word at `+0x0c` is never written, and the zero quad at
  `+0x10` is the constant at `0x142f84a00`.
- `0x142cae760` is the type-3 mesh's **own triangle record**: the three
  vertices, scaled by the mesh record's quad, and the three layer words. It is
  `ow_sim::collide::mesh_triangle_record`, unit `fn/CollisionMeshTriangle`:
  120 differential cases, the whole `0x86`-byte output compared, 6/6 mutants
  killed. The record is **`0x20` bytes** - the client's own `shl r10, 0x5`
  multiplies the index by 32, not by 0x50 - with the vertex indices at
  `+0x00`, `+0x04` and `+0x08` and the layer words at `+0x18`, `+0x1a` and
  `+0x1c`.
- `0x142ccf540` is the runtime's **own height-field triangle record**: the
  grid's four corners around the index, with the diagonal picked by the
  vertex's kind byte. It is `ow_sim::collide::runtime_triangle`, unit
  `fn/CollisionRuntimeTriangle`: 120 differential cases, 8/8 mutants killed.
  The client's indirect callback at the record's `+0x40` is modelled as a
  closure, and the fixture answers it with a stub in the blob.

**The rest of the cluster is read and named**, with its sizes and roles, in
`docs/formats/character-controller.md` ("The narrowphase's own body, and its
first leaf"): `0x142cc5ba0` (the triangle's head), the candidate gather
`0x142cfcba0` (384 bytes) with the runtime's height-field gather
`0x142cd0180` (288 bytes) and the candidate list's append `0x142cd00c0`
(38 bytes) behind it. The gather's grow path calls the global allocator,
which is zero in the offline image; a fixture that gives the context a
capacity larger than the candidate count never reaches it.

Proof: `python dev/verify_harness/cli.py fn/CollisionShapeBox`,
`fn/CollisionShapeHead`, `fn/CollisionMeshTriangle` and
`fn/CollisionRuntimeTriangle` - all VERIFIED.

## 2026-11-12 - one owner per question, and both ledgers rotate

The docs had nine files each claiming "the state now", two ledgers over a
thousand lines, and a queue that was more than a third closed narrative. This
change gives every question one owner and moves the rest into `docs/history/`
verbatim:

- **`MODLOG.md` 2,023 -> 808 lines**: 44 older entries (from the 2026-11-19
gate work back to 2026-11-10) moved to `docs/history/modlog-2026-11.md`; the
header records the rotation point. No entry was edited.
- **`STATUS-handoff.md` 909 -> 87 lines**: nine session narratives, the
2026-11-19 session, the measured state, the five walls and the 33-entry
candidate list moved verbatim to `docs/history/status-handoff-2026-11-20.md`.
The page is now the pointer table, the batch loop, the delegation rules and the
standing rules; it no longer carries a state table.
- **`docs/open-questions.md` 3,088 -> 1,826 lines**: the closed chain-fragment
answer and the image-repair backlog, plus the 1,221-line character-collision
round log, moved to `docs/history/open-questions-2026-11-19.md`. The closure
rule is now part of the header (its old copy sat at the bottom), and the live
§6 is one entry naming the open stage (`0x142cd4610`, `0x142cf3f80`).
- **Two cold pages moved out of `docs/`**: `status-in-depth.md` (a second copy
of `systems-coverage.md`) and `owlib-comparison.md` (pinned four commits back,
linked from nothing) are in `docs/history/`; both READMEs point at
`systems-coverage.md` for the state.
- **State claims removed, not rewritten**: `docs/roadmap.md` and
`docs/rebuild-plan.md` keep the goal and the stage order and point at
`systems-coverage.md`; `docs/port-playbook.md` lost its duplicate scope table,
its copy of `port-plan.md`'s definition of done, and the
"STATUS-handoff holds state" row; `docs/movement-plan.md` lost its 43-line
"what landed" narrative (archived).
- **Wrong claims fixed on sight**: `docs/ghidra-migration.md` now says the
migration is done; `docs/formats/camera.md` drops `--cam-look-inverse` (a flag
no build has); `docs/formats/maps.md`'s pathing heading carries its historical
"cell path" name so the three citations resolve; `docs/open-questions.md`'s
self-anchor is plain text; the CASC unit count is 35 everywhere now
(`casc-1to1.md`, `systems-coverage.md`, `formats/casc.md`) - counted from the
35 `candidateCrate: ow-casc` descriptors, every one `verified` in this
machine's ledger, instead of the 27/30/32 that three pages each carried; the
missing `ArchiveSetContains`, `BlteBlockReadStored` and `ClientStrlen` are in
the systems-coverage row, and one stray `|` in that table that broke its
column count is escaped. Two more formats fixes from the same audits:
`formats/static-collision.md` called itself "three bounded checks" while the
page carries nineteen `## Check` sections (eighteen checks plus the summary) -
retitled; and `formats/domino.md`'s verification note read as "nothing to
verify by test yet", which is false now that the Domino arithmetic ships
tested in `ow-sim` - it is scoped to its own pass. Two state-owner one-liners: the UI is an embedded CEF
browser with a Dear ImGui developer overlay (the row said "the client uses
imgui"), and the content row no longer says catalogs are "read where shipped"
- the catalog parses zero of its twenty installed assets (`formats/content.md`).

Left for the next pass: a uniform state header on each `docs/formats/` page and
the data/render duplication (two read-only audits, untracked, under
`dev/data/notes/`). `cargo xtask verify` was not run (suspended); the four doc
checks and `cargo fmt --check` were.

## 2026-11-12 - one tool list: docs/tools.md

`dev/INDEX.md` and `docs/tools.md` were two halves of the same answer — the
generated list of scripts, and the hand-written table by task — and the second
had drifted (a table row stranded outside its table, probes the crates no
longer carry). There is now **one** page, `docs/tools.md`, and it is the only
tool list:

- sections for the gate (`cargo xtask`), the differential harness, the
  analysis scripts, the data probes, the client launches and the Ghidra
  scripts;
- three generated tables (dev scripts, harness machinery, unit fixtures) built
  from each tool's own first docstring line by `dev/make_index.py`, which now
  writes between marker comments in the doc instead of its own file; `--check`
  still fails on a stale table, and `cargo xtask dev-index` keeps its name as
  the gate's check;
- `dev/INDEX.md` deleted; `AGENTS.md`, `CONTRIBUTING.md`, `dev/README.md`,
  `docs/README.md`, `docs/provenance.md`, `docs/port-playbook.md` and the CI
  comment point at the one page instead of keeping their own list.

213 tools are listed. `cargo xtask dev-index`, `python dev/deviation_check.py
--check` and `python dev/frame_check.py --check` pass; `cargo xtask verify` was
not run (operator suspension, `AGENTS.md`).

## 2026-11-12 - the capsule narrowphase is read, and the flag that decides the wall

The next link after the resolve is the **capsule narrowphase**
`0x142cfc090` - the body that produces the resolve's factor. It is read now.

**The descriptor.** The closest-point solver `0x142cc9460` takes a descriptor
of `0xa2` bytes, and the narrowphase builds it from four tiny builders, none
of them in the unwind table:

| builder | address | points | count | radius |
|---|---|---|---|---|
| the triangle's | `0x142cc5ba0` | the pointer it is given | `3` | the constant quad |
| the two-point shape's | `0x142cc5b40` | the pointer it is given | `2` | `splat(src[0x20])` |
| the one-point shape's | `0x142cc5b80` | the pointer it is given | `1` | `splat(src[0x10])` |
| the counted shape's | `0x142cc5b60` | `src[0x10]` | `src[0x30]` | the constant quad |

The shape's own head comes from `FUN_142cb8950`, which picks the builder by
the shape record's type byte at `+0x20` and its index at `+0x24`.

**A correction to this morning's note.** The narrowphase's solver flags are
`+0xa0 = 0` and `+0xa1 = 1` - **not** one for both, as the note above said
before the disassembly was checked (`mov byte ptr [rbp + 0xd0], al` with
`al = 0`, and `mov byte ptr [rbp + 0xd1], 1`; the descriptor's base is
`rbp + 0x30`). The second flag is the narrowphase's own difference from the
mesh leaf, which takes the pair from the query block (`+0x72` and zero), and
it is the flag that decides the wall: with it set the solver does not refuse a
shape that is already inside the other's radius - it shrinks its radius and
carries on - while the leaf's pair refuses it. A unit pins the difference
(`the_second_flag_carries_a_shape_that_is_already_inside`), and
`ow_sim::collide::mesh_sweep_with_flags` is the leaf with the flags given
explicitly.

**The walk's resolve is not shippable yet, and the measurement says why.**
The client's rule was wired into `ow_sim::walk::advance` and measured: it needs
three values the walk does not have.

- **The push limit** is the mover's body `+0x628` - a state-modifier parameter
  from the install - times the frame duration. The walk's body copy leaves it
  at zero, so the step stage cannot push at all, and the body ends 1.5 cm
  inside the surface after four rounds.
- **The round cap** is `step_state[0x94]`, a runtime field of the character's
  collision state. It bounds how far a slide goes in one frame.
- **The narrowphase's own answer** for a shape that is already inside: the
  leaf's flags let the body through a wall on the second round (the failure the
  first implementation measured) and the narrowphase's flags need the step
  stage - and the limit - to pull the body back.

So the walk's advance stays the stand-in, and the walk's numbers are
unchanged. What the session adds is the read, the flag, the unit and the
parameterised leaf.

## 2026-11-12 - the client's collision resolve is read, and the factor is a fraction

The operator asked to finish the collision in `ow-client`. The open stage was
the **resolve**: what decides where the character ends up when it hits
something. It is now read from end to end and ported to
`crates/ow-sim/src/controller.rs`.

**The chain**, in the client's own order:

```text
MoverAdvance 0x140ca7f30          builds the sweep block
  -> RunCharacterControllerSweep 0x142cbcee0 -> CollisionSolver 0x142cf3f80
       -> CharacterPairBuild 0x142cd4d90        the world's pairs
       -> the pair walk writes the contact planes
       -> step stage 0x142cf5020               the planes push the target out
       -> CharacterPositionResolve 0x142cd5270
            -> CharacterBodyUpdate 0x142cd4610  the motion scaled by the factor
```

**The factor is a fraction of the motion, not a depth.** The capsule
narrowphase `0x142cfc090` builds the GJK descriptor with the **motion** at
`+0x60`, the budget `1.0` at `+0x90` and the flags `+0xa0 = 0` / `+0xa1 = 1`;
the GJK's output at `+0x20` is the swept extension (`movups [r9 + 0x20],
xmm15`, `0x142cc9f76`), and the resolve scales the motion by the smallest of
those (`minps`, `0x142cd4a79`). The earlier note in
`docs/formats/character-controller.md` - that the factor's value was open and
decided the rule - is closed.

**The slide is not in the resolve.** `0x142cd4610` scales the motion; it does
not project the leftover. The client's slide is the **step stage**
`0x142cf5020`, a call-free leaf: the *target* is pushed out of the faces the
body overlaps, by an accumulator that grows by the penetration each round,
capped by the push limit, until the push is under `0.005` or the 200-round cap
is reached. The walk's stand-in projects the residual instead, which is why
the two do not agree.

**The fixed point** is the solver's own: the correction is a squared length
against `eps * eps` with `eps = 0.005`, measured against the **stepped**
position, and the bound is the sweep block's `+0x2c` (the earlier note in
this file's character-controller page put it at `+0x1c`; the advance's copy
is at `+0x1c` and `RunCharacterControllerSweep` shifts it by `0x10`).

**The sweep block**, for the record: `+0x00` the frame's displacement, `+0x20`
the frame duration, `+0x24` the push limit (`_DAT_143872ebc` = 1.0 times
`body+0x628` times the duration), `+0x28` the epsilon `0.005` and `+0x2c` the
cap `min(step_state[0x94], body+0x1a88])`.

**The step stage is verified.** `fn/CharacterStepStage` runs the client's own
`0x142cf5020` in the harness against `ow_sim::controller::step_stage`: 160
differential cases (the output position quad, the round count and the plane
records' accumulated pushes, byte for byte), 100% branch coverage of the
seven reachable branch sites and 8/8 mutants killed.

**The harness grew one option.** The stage writes a global - the largest
round count the client has seen, `_DAT_1442a1eb0` - and the oracle's
read-only image turned that write into a fault, so a leaf that updates a
statistic in the image's zero-fill region was unverifiable by emulation
alone. `options.allowedImageWrites` declares the globals a unit expects the
write in; the write is still declared per unit and still outside the compared
channels, because the statistic does not feed the callee's answer. That is
harness version 9, so every ledger entry is re-proved once. The suite is
**109 of 111 verified**; the two that are not are the pre-existing ones
(`fn/MeshSweep`, `fn/OrthonormalizeCameraBasis-state`).

**What is still open** is the *contact source*, and the reason is now exact:
the client takes the factor from the capsule narrowphase, whose second solver
flag carries a sweep that starts on a surface on; the walk's contact source
is the mesh leaf, whose own rule is to refuse it, so the client's resolve
cannot be substituted into the walk until the narrowphase is ported. That is
the next link: `0x142cfc090`, the triangle descriptor `FUN_142cc5ba0` builds
and the second flag the mesh leaf does not set.

## 2026-11-11 - finding and reading a client function is one command now

The operator's ask: stop tracing a function down for a long time before it can
be read. The audit found that the information was already in the image and in
the corpus index, but nothing connected them: the whole-image index (119,802
functions, 1.87M call edges, 35,835 slot dispatches) had **no query
interface**, the corpus held only 923 built functions, and the only tools that
named a format were one-off probes.

**`dev/fn.py` is the new front door** (read-only; the image and the corpus
index, no Ghidra session needed):

- `read <addr>` prints everything known about one function: its name and role,
  the differential unit that tests it and that unit's ledger verdict, every
  place `docs/` already writes it down, the true extent (the chain-root union,
  with the `opaque` flag), the whole-image callers and callees, and the
  disassembly decoded from the image. It also reads a body the unwind table
  does not cover - the client has code no `.pdata` entry describes, and
  `MrnmFixup` `0x141b893e0` is one of them - by running the window to the
  next known function.
- `find <query>` searches by name, 4CC tag, string, immediate or asset type.
- `callers`, `callees`, `scope` (the transitive closure: the cluster a
  format's functions form) and `slots` (who dispatches through a vtable slot)
  are queries over the whole-image index.
- `jobs` prints the client's asset-type registry.

**Two client facts were read on the way** (`docs/formats/asset-types.md`):

- `JobForType` `0x141fd8300` maps an asset *type* to a loader *job*: the u32
  type table at `0x143a7c3a0` (403 types, `0x00..0x192`) then the qword job
  table at `0x1441b3cb0`. The job table is zero-fill - the job objects exist
  only at runtime - but the type -> job half is a compile-time constant: 33
  job ids, job 10 serving 194 of the 403 types, 163 types with no loader.
- The client compares a chunk tag as a **big-endian** u32: `MRNM` is the
  immediate `0x4d524e4d` (at `0x141b5e02c`), and `ModelChunkLoad`
  `0x141b5df70` is the model chunk dispatcher. `python dev/fn.py find MRNM`
  finds it without knowing an address; `python dev/fn.py scope 0x141b5df70`
  then lists the 12 functions that implement that format.

The job id -> registration function mapping is **not** read; it is the next
anchor in `docs/open-questions.md` item 19.

**Tried on `ow-tank`, and it measured the gap.** `python dev/fn.py surface
ow-tank` reports the named client functions that are reachable from what the
crate cites but that the crate does not anchor: **86** of them. The ones that
name a real omission: `MoclRead` `0x141b88e50` (the client reads the `MOCL`
chunk; the crate only names the tag), `MapChunkHeaderFixup` `0x142e49360`
(the chunk header-record fixup; the crate has the collision fixup only), and
the eight texture functions `texture.rs` does not cite. The census is
`docs/ow-tank-1to1.md` section F.

**A real bug found on the way.** `dev/crate_coverage.py` counted only the
`0x14…` form of an address, so the addresses a human writes as `FUN_…` in
prose were invisible: 16 of `ow-tank`'s citations (the effect builder
`FUN_142e4a780`, the STU loader `FUN_1421d88c0`, the material job
registration `FUN_141b19160`, the texture-set consumer `FUN_141f1ca80`) and 13
of `ow-casc`'s. The tool now reads both forms: `ow-tank` 238 -> 254 cited
addresses, `ow-casc` 199 -> 212.

**Item 19 answered: the job id -> loader mapping is read.** `0x141fd89f0` is
the call that fills a job slot in the client's runtime job table
(`0x1441b3cb0`, zero-fill), so its callers are the registration functions,
and the descriptor each passes holds the job id at `+8`, the **load** callback
at `+0x48` and the **release** callback at `+0x78`. 32 of the 33 jobs are
resolved; `python dev/fn.py jobs` prints the table and
`python dev/fn.py jobs --crate ow-tank` marks the loaders the crate cites.

The write was invisible to a displacement scan because the slot address is
computed with a `lea` and the store then uses **no displacement** - the
table's offset never appears in an instruction's memory operand. Two claims
the project had made by inference are now the image's own values: the
material asset (type `0x008`) is job 2 with the loader `0x141b1a8e0`, and the
material data (type `0x0B3`) is job 28 with the load callback `0x141b1a7b0`
and the release callback `0x141b1a790`. The shader jobs are read too (23 ->
`0x141e73f30`, 24 -> `0x141e74360`, 25 -> `0x141e73a80`), which is the first
address the shader document has for its loader. `docs/formats/asset-types.md`
section 1b has the whole table; `docs/open-questions.md` item 19 keeps the
two things still open (job 4, and the descriptor's other fields).

**The client's own names for its assets are read too.** Every registration
function deobfuscates the name of the asset it registers, with the same
XOR-with-a-key loop `formats/chunk-kinds.md` documents, and stores it next
to the asset's type id. `python dev/fn.py names` decodes 37 of them and ties
each to its type id, its job and its loader: `MODEL` is type `0x00C`,
`MODELVERTEXDATA` `0x161`, `SKELETON` `0x007`, `MATERIAL DATA` `0x0B3`,
`TXTR` `0x0F1`, `TXTR_PAYLOAD` `0x04D`, `SHADER CODE` `0x087`, `SHADER
REFLECTION DATA` `0x040`, `MAPDATA` `0x002`, `SOUNDBANK` `0x043`,
`VOICEWEMFILE` `0x0B2`, `SOUNDWEMFILE` `0x0BB`, and the three lighting ones.
The same functions register the `SSAO.*` config variables with their help
texts. This is the client's own vocabulary - the crate had taken
`MATERIAL DATA` and `SHADER CODE` from a reference library.

Two names have no resolved type id (`SHADER GROUP` and `STU`), and the
reason is measurable: their name records live in the **zero-fill** region
(`0x1441c2240`, `0x1441b1210` are BSS) and their type id arrives in a
register whose value is a runtime global, so it is not a static constant in
the image. The tool reports them as unknown rather than guessing.
`docs/formats/asset-types.md` section 1c has the table.

## 2026-11-11 - `fn/MapChunkHeaderFixup`: the pathing chunk's header fixup is read, implemented and verified

The operator asked for a full read-and-implement cycle on the `ow-tank` side.
The gap list from `python dev/fn.py surface ow-tank` named the candidate:
`MapChunkHeaderFixup` (`0x142e49360`), the relocator the client's map-chunk
dispatch applies to the pathing kind's header record - a function the crate
neither cites nor implements, while its sibling `0x142e492a0` (the collision
payload's fixup) is `chunkkind::fixup_in_place` and is verified.

**The read** (61 bytes, `0x142e49360..0x142e4939d`, no `.pdata` extent - the
tool's "no .pdata" path read it): the client converts three self-relative
RVAs of the record - at `+0x00`, `+0x08` and `+0x18` - to pointers by adding
the record's own address, and a stored zero stays zero. Each conversion is a
`lea` of the value plus the base, a `test` of the value and a `cmove` back to
zero. `+0x10` is not touched.

**The implementation**: `chunkkind::fixup_header_in_place`, the three
conversions written out one at a time because the machine does them that way.

**The proof**: `fn/MapChunkHeaderFixup` - the client's own bytes against the
shipped code, 160 differential cases on every channel, 4/4 mutants killed.
The client's decision is branchless (a `cmove`), so the coverage stage finds
no branch site and reports 0 of 0; the mutation stage carries the proof
instead, and the three `== 0` comparisons are the function's whole behaviour.
The install check rides along in `collision_cells_real`: on the practice
range's pathing chunk the three stored values are `0x20`, `0x862720` and
`0x862878`, and the fixup turns them into the record's address plus each
value.

The other three relocators of the same dispatch (`0x142e49270`,
`0x142e493a0`, `0x142e493e0`) are read and still unimplemented.

`cli.py all` is now **106 of 108** verified: the new unit is in the suite and
the two that are not are the ones the handoff already named (`fn/MeshSweep`,
inconclusive on its coverage gate, and `fn/OrthonormalizeCameraBasis-state`,
divergent). `cli.py casc` is 35/35 and `cargo xtask verify --fast` is green.

## 2026-11-11 - `fn/MeshShapeFill`: the collision shape record is proved, and the "unreachable" chain is a fragment artifact

The operator asked for the collision blockers. Two of them turned into work
this session; the third narrowed to a correction.

**`fn/MeshShapeFill` - read and proved.** `MeshShapeFill` (`0x142cae540`,
89 bytes) is the only writer of the 0x4c-byte shape record the collision
query consumes: four lanes of the scale, four lanes of `1.0 / scale` (a
`divss` by the constant 1.0 at `0x142f7aae4`), the three data pointers, the
four copied words and the one scaled float. The crate's
`ow_tank::mesh_shape::shape_record` is that expression dword for dword, and
the new unit proves it against the client's bytes: 96 differential cases on
every channel, 8/8 mutants killed, the record compared bit for bit with no
masked slot. The fixture's scale cases include a zero (the reciprocal is
`inf`), a denormal and a huge scale.

**The "unreachable" collision chain was a fragment artifact.**
`docs/formats/shape-ingest.md` §6 said `BuildBvh`'s three callers were split
wrappers and unreachable. Resolving each address to the function that owns it
(`dev/fn.py read`, which walks the `UNW_FLAG_CHAININFO` chain) shows they are
not three functions: `0x142cca7d9` is a fragment of `ColliderFaceAppend`
(`0x142cca460`) and `0x142ccb2db` / `0x142ccbc1c` are interior points of
`BuildBvh` (its own recursion). `BuildBvh` has exactly one external caller
and it is reachable:

```text
0x142137480 -> 0x14213bbc0 (the geometry driver)
                 -> 0x14213a510 AppendCollisionEntryFromBrushSource
                 -> 0x14213acf0 (the 0x60-entry builder)
0x14213c440 GeometryBuild -> 0x142cf2010 MeshToColliderFaces
                            -> 0x142ccc120 ColliderMeshEmit
                            -> 0x142cca460 ColliderFaceAppend
                            -> 0x142ccb1b0 BuildBvh (fn/BuildBvh, verified)
```

The conclusion of round 5 stands - the collision chunk's sectors are not a
producer of Domino shapes; the brush is - but it now rests on named functions
instead of on an unreachability that was an artifact.

The shape-pool side is read and still unimplemented: `ShapePoolAlloc`
(`0x142cb00c0`, the free-list pop: the head at `pool+0x14`, the link at
`entry+0xA0`, the used count at `pool+0x08`, the high-water mark at
`pool+0x10`, the stride `0xC0`, the grow path `cap/2 + cap` with a minimum of
2) is the next implementable link, and its grow branch is unreachable offline
(the allocator's heap vtable is null in the image), so a unit would prove the
pop only.

## 2026-11-11 - `ow-client` gets an install-reading test, and the collision worlds are measured

The operator asked for proper collision in `ow-client` and for everything in
`ow-client` to be verified against the install. `ow-client` had **no**
install-reading test at all - its walk measurements were printed by
`--walk-test` and read by eye.

**The world's source is now selectable and measured.**
`ow-client --walk-test --collision level|models|both` builds each world, and
the three measured rows are in `docs/formats/character-controller.md`:

| world | shapes | forward | drift | strafe | grounded |
|---|---|---|---|---|---|
| the level collider | 1 | 5.500 m | 0.000 | 4.540 m | yes |
| the per-model holder meshes | 3,017 | 5.493 m | 0.005 | **5.500 m** | **no** |
| both (what the client has) | 3,018 | 5.190 m | 0.460 | 2.030 m | yes |

**What the measurement says** (and it corrects the earlier reading): the
per-model meshes are *not* "not the map's collision" - the client has **both**
copies, in the same node's world. The level collider carries the spawn room's
floor (so the walker is grounded and the forward leg is exact); the per-model
copies carry a free strafe but **no floor** under the spawn room (the floor
probe finds one 13.9 m away). The walk survives neither alone nor both,
because its **slide and floor stage are stand-ins**: the client's own solver
(the Domino pair-list walk in `0x142cf3f80`, 29 KB, still unread) is what
resolves the two copies against each other. The level collider's 4.540 m
strafe is the slide stand-in stopping on a vertical face, not a wrong world.

**The install-reading test** `crates/ow-client/tests/walk_real.rs` runs the
built binary against the install and pins: the level collider's exact forward
leg, its zero drift, its zero idle and its grounding, and the per-model
world's free strafe and its missing floor. It skips when the install is not
readable, like the other crates' install tests.

## 2026-11-11 - `fn/ShapePoolAlloc`: the shape pool's free-list pop is implemented and proved; the solver's pair walk is read to the first pass

**`fn/ShapePoolAlloc` - verified.** `ShapePoolAlloc` (`0x142cb00c0`) is the
shape-entry pool's per-element allocator, read in full and now implemented as
`ow_tank::domino_world::shape_pool_alloc`: the head at `pool+0x14`, the
popped entry's own link dword at `entry+0xA0`, the used count at `pool+0x08`,
the high-water mark at `pool+0x10` raised with the client's signed compare, and
the popped entry's link set to its own index. The unit proves it against the
client's bytes: 96 differential cases on every channel, the pool record and
the entries array compared exactly, 5/5 mutants killed, and one declared
equivalent mutant (a maximum written with a non-strict compare writes the same
value when the two are equal - the client's own `cmovg` has the same
property).

**The grow path is not implemented** - it allocates through the client's
runtime allocator (`0x142cadee0`), which the crate does not model - so the
case is refused rather than guessed, and the unit says so. That is also why
the coverage gate is 0: the grow path cannot run offline (the allocator's heap
vtable is null in the image), so its two branches are never reached.

**Correction to `formats/shape-ingest.md` §3:** the grow path copies the old
array's `capacity * stride` bytes, not `used * stride` - the client's copy at
`0x142cb0145` reads the capacity at `pool+0x0C` for its length.

**`0x142cf3f80`, the solver, read to the first pass.** It is 3,575 bytes over
ten `.pdata` pieces with **67 direct calls** and no indirect call. This pass
establishes: the world's variant tag dispatches the mode (`cmp dword ptr
[rcx], 1` at `0x142cf3fa6`); the character path takes the handle at
`[world+0x08]`; the pair array is allocated `count * 0x20` bytes at
`0x142cf43c9` and filled by `0x142cd4d90`; and the pair loop at
`0x142cf43f0` walks the world's fixture list at `[world+0x28]`, calls the
pair getter `0x142cbb260`, decodes the fixture handle with `0x142cb9d60`,
rejects an empty world slot and an invalid body, and looks the body up with
`0x142cc5730`.

**What that says about the port:** the solver is not one transcribable
function - it is the Domino runtime's collision (the world's pools, the
allocator, the handle encoding, the body records, the narrowphase). The next
portable piece is the pair build `0x142cd4d90` and the position resolve
`0x142cd5270`, which decide where the body ends up; the pool's free-list pop
they allocate through is now implemented.

## 2026-11-11 - The walk sweeps the client's capsule, and the resolve loop is read

**The walk's shape is the client's now.** `ow-client` swept a `0.35` m
**sphere** - a shape the client does not have - and the walker's position was
that sphere's *centre*. The client's character collides with a **vertical
capsule** whose origin is at its **feet**: `radius 0.5`, height `1.5` on the
install's common hero, read from the mover component's three class fields
(`0x1972C687`, `0x7A70E3E3`, `0x4A576BC2`) through
`ow_tank::entity::mover_capsule_of`, which was already implemented and
proved.

**And the radius the body carries is capped.** `0x140cb7700` writes the
body's copy of the class radius (`body+0x5a4`) as `min(0.5, radius)`, so the
heroes that declare `0.7` collide with a `0.5` m capsule - the same clamp
`ow-sim`'s `ApplyStateModifiers` (`0x140cbcbe0`) applies on the guarded path.
`capsule_from_mover` carries the cap, and its install-wide test now asserts
`0.5` where it used to assert the declared `0.7`.

The walk now sweeps that shape. `ow_sim::walk` gained `Capsule`,
`advance_capsule` and `snap_to_floor_capsule` (the same fold, the same
depenetration, the same slide - only the query's shape differs), and
`ow-client`'s `Walker` holds the capsule; a picked hero's own capsule comes
from the same read the other mover scalars use. The walker's position is the
capsule's origin, so the spawn resolves to the room's floor (`y = -4.006`)
instead of a radius above it, and the camera's height is unchanged (`1.60` m
above the origin, the world height the sphere proxy had).

**The resolve loop is read.** `0x142cf3f80`'s character path puts the body
where it was asked to go, lets the Domino runtime push it out of what it
overlaps, and repeats until the **squared** correction is below
`eps = 0.005` (the cap is `step_state[0x0b]`); the resolved position lands at
`body+0x620`/`+0x628`/`+0x62c` and the iteration count at `body+0x664`. The
correction is measured against the *requested* position, and **the client's
resolve has no slide**. The walk's stand-in does the opposite - it projects
the residual along the contact normal - which is exactly what makes a body in
a corner drift sideways instead of stopping. The runtime's own depenetration
is not ported, so the stage stays open.

**Measured, the capsule makes the two copies harder - and that is the honest
result.** The walk's numbers on the practice range's spawn room:

| world | forward (sphere) | forward (capsule) | drift | strafe (sphere) | strafe (capsule) |
|---|---|---|---|---|---|
| the level collider only | 5.500 m | **5.500 m** | 0.000 m | 4.540 m | 4.519 m |
| the per-model holder meshes | 5.493 m | 4.978 m | 0.764 m | 5.500 m | 5.500 m |
| both - what the client has | 5.190 m | 4.999 m | 0.628 m | 2.030 m | 1.902 m |

The capsule is `0.15` m wider than the proxy, so it meets coarse holders the
proxy passed and stops a little earlier at the room's wall. The shape is the
client's; the *response* is what is still missing, and the numbers say so.
`crates/ow-client/tests/walk_real.rs` pins all of it, including the "both"
row, so the gap cannot be forgotten.


## 2026-11-11 - `fn.py` grows a decompiler, a callers fallback and a format grouping

The operator asked for the tool's three weak points to be fixed and for the
Ghidra project to be opened when it is not.

**The decompiler is now in the tool.** `python dev/fn.py read <addr>
--decompile` runs the client's own decompiler and prints the C: it drives the
project's headless script (`McpDecompile.java`) against the merged working
copy and caches the result under `dev/data/decompiles` (ignored) - eight
seconds for the first function, a third of a second after. When it cannot run
the tool says **why** (no Ghidra, no script, no project, or no function at
that address) instead of silently omitting it. The paths are the environment's
`GHIDRA_HEADLESS`, `GHIDRA_SCRIPTS` and `GHIDRA_PROJECT` with this machine's
defaults. The merged copy is a separate project file, so the GUI's own open
project does not lock it.

The first decompile confirmed an implementation from the client's own words:
`MapChunkHeaderFixup`'s C is `lVar1 = *param_1 + (longlong)param_1; if
(*param_1 == 0) lVar1 = 0;` - the value plus the record's own address, with a
stored zero kept zero, which is exactly what
`chunkkind::fixup_header_in_place` does.

**`callers` and `callees` work on bodies the unwind table does not cover.**
They used to answer "not inside any function" for every such body - and
several of the collision functions are exactly that, so the reader had to fall
back to `dev/callers.py` every time. The index records the raw target of every
transfer, so a call into such a body is found by its exact target; the tool
now does that.

**`surface` is grouped by the crate's own files**, because a crate file is the
format's vocabulary: `crates/ow-tank/src/brush_convert.rs` is the brush
cluster. The report is the same measurement as before (the named client
functions a crate's cited addresses reach that the crate does not anchor),
now per file: 65 distinct functions across `ow-tank`'s 52 files.

**The Ghidra project could not be opened through the MCP.** The GUI's
FrontEnd holds a different project (the launcher's), and the MCP refuses to
establish a new active project while one is open; the launcher's program was
closed (its edits are unsavable by the tool's own account) and the project
switch still needs one action in the GUI that the tools do not offer. The
headless route over the merged copy is what the tool uses instead, and it
works.

## 2026-11-10 - `fn/CurveKeySampleDual` verifies: the last gcc-blocked unit

The local `gcc` compiles the tier-1 C candidates again, so the two units that
were blocked by it could be re-proved. `fn/AnimTimeWrap` verifies unchanged
(100% coverage, 8/8 mutants). `fn/CurveKeySampleDual` (`0x140b909d0`) now
verifies too, after two reads:

- **The search's back edge needs a 128-key track.** The binary search's
  window is `[0, count-1]` and it halves once per pass, so the loop
  continues only while `(hi - lo) >= 0x40` (`cmp rax,0x40 / jae` at
  `0x140b90a80`, and `0x140b90b70` in the second half). With 80 keys the
  window halves to 40 and the loop never repeats; with 128 keys it leaves 64
  and repeats exactly once. The fixture's new shape is 128 keys in both
  tracks, and the sample has to sit above the middle key for `lo` to
  advance - the fixture's own mode 3 does.
- **Two branch edges are dead.** The linear scan's entry test
  (`comiss xmm4,xmm3 / jae` at `0x140b90a92`, and `0x140b90b82` in the
  second half) can never be taken: the scan is entered with `times[lo]`, and
  `lo` is 0 or a key whose time is strictly below `t` - the caller has
  already compared `t` with `times[0]` (`0x140b90a1d`) and the search only
  advances `lo` on the `t > times[mid]` edge (`0x140b90a6e`). The two sites
  are declared in `options.unreachableBranches` with that reasoning, which
  is what the harness wants for a branch the code cannot take.

The unit is now 100% coverage (14/14 reachable branches of 16 sites, the two
declared dead edges) and 10/10 mutants killed. Three units are not verified
now, not five: `fn/MeshSweep` (61.1% branch coverage), 
`fn/OrthonormalizeCameraBasis-state` (the probe's guest-pointer arm is
missing) and `fn/MoverAdvance` (a declared scaffold).

## 2026-11-11 - one build for a unit's whole mutant set

The mutation stage was ~80% of the harness's wall time: a cargo unit's
mutants were each a full crate rebuild, and a unit has a dozen of them
(measured: 4.4 s per build with a warm target dir). A full re-prove of the
107 units cost ~35 minutes, almost all of it in `cargo build`.

The mutants now share one build. Every mutant of a unit becomes an **arm** of
a dispatch inside the function under test (`dev/verify_harness/batch.py`):

```rust
match __ow_mutant_index() {
    0 => { <mutant 0's body> }
    1 => { <mutant 1's body> }
    _ => { <the original body> }
}
```

The arms are the original interior with the mutation applied, so a mutant is
byte-for-byte what it was when it was built alone; the harness picks the arm
with `OW_MUTANT_INDEX` in the child's environment. The default arm is the
original body, and the harness **checks it against the baseline candidate**
before it trusts the batch: a batch binary with no index set must answer
exactly what the unmutated candidate answers, or the unit falls back to one
build per mutant. The batch is an optimisation, never a change of what is
measured: same corpus, same comparisons, same gates, same verdicts.

A mutant whose code does not compile takes the whole batch down with it. The
compiler names the line, the line names the arm, and that arm is dropped and
the batch rebuilt - the compiler catching a mutant is a kill, exactly as in
the one-build-per-mutant path. An error that cannot be attributed to an arm
falls back to the old path.

Measured, ten of the slowest units, the same corpus and the same verdicts
both ways: **324 s without the batch, 85 s with it** (3.8x). A full forced
re-prove of all 107 units went from ~35 minutes to **542 s**, and every one
of the 111 ledger entries agrees on the status, the killed count and the
total, before and after. Per unit:
`fn/OrthonormalizeCameraBasis-cargo` 23.2 s -> 6.6 s,
`fn/BuildMoverPhysicsCommand` 17.2 s -> 5.8 s, `fn/Md5Final` 17.0 s -> 7.5 s.
Every unit's verdict is unchanged: all 111 ledger entries agree on the
status, the killed count and the total, before and after.

The harness now prints each stage's wall time, because "make it quick" is a
guess without the numbers - and the numbers found two bugs the verdicts
alone did not:

- A unit whose function under test is *nested* inside another one could not
  hold the free dispatch helper (it would have to be a module-level item),
  so the batch silently fell back to one build per mutant. The helper now
  goes to the file's module scope.
- A mutation on the function's *signature* - the line that carries the
  opening brace is part of the signature - cannot be an arm, because the
  dispatch replaces the body. Such a mutant was silently dropped from the
  batch and the unit lost a kill (four units went from 100% to 80% and
  failed their gate). A mutation outside the body is now simply not in the
  batch, and the caller builds it on its own, as it did before.

Both are cases in the batch's own suite (`test_batch.py`).

`VERIFY_NO_BATCH=1` turns the batch off: the escape hatch for a crate
whose build refuses the generated dispatch, and the way to A/B the two
paths.

The harness's own suite grows a case (`dev/verify_harness/test_batch.py`): a
real cargo build of a tiny crate whose function has two swappable operators
must answer 43 with no index set, 11 with the `*` arm and 41 with the `+`
arm.

## 2026-11-11 - the install reads are cached, and the gate's wall time is gone

The Rust gate's cost was seven install-reading suites: 2218 s of the 2246 s
of test time, with `index_find_real` alone at 1154 s. Two caches in the
shipped reader take them to seconds:

- **The segment's byte form is built once** (`ow-casc`'s `SegmentEntries`).
  `key_state` rebuilt the whole 18-byte-per-entry buffer on *every* call, so
  a per-key sweep was quadratic in the segment size: the install test
  resolves 1.1M keys and copied tens of gigabytes. The bytes are a pure
  function of the entries and the page table - the client keeps its parsed
  entries in memory for the same reason - so they are built once and reused.
  `index_find_real`: **1154 s -> 1.5 s**, with every key still resolved and
  every assertion unchanged.
- **The encoding manifest is parsed once** (`ow-casc`'s `Store`). Every asset
  open re-parsed the decoded manifest, and the parse's expensive half is the
  espec string index: one u64 per byte of a 100 MB manifest. The index is a
  pure function of the block, so it is built with the manifest and the parse
  reuses it (`EncodingFile::parse_with_index`).

Measured, same machine, same assertions:

| suite | before | after |
|---|---:|---:|
| `ow-casc` `index_find_real` | 1154 s | 1.5 s |
| `ow-scene` `skin_pose_gate` | 428 s | 4.4 s |
| `ow-tank` `effect_real` | 213 s | 4.0 s |
| `ow-scene` `practice_launch` | 186 s | 5.2 s |
| `ow-scene` `hero_draws` | 159 s | 2.8 s |
| `ow-tank` `hero_pick_real` | 152 s | 2.8 s |
| `ow-tank` `brush_real` | 134 s | 2.7 s |

The effect census is byte-for-byte the same (7,342 assets, 419,488 chunks, 86
distinct tags) and the suite now prints its per-type sweep times, because
the cost was invisible until it was measured.

Both changes are behaviour-preserving: the same bytes, the same search, the
same answers. The differential harness's units cover the changed code
(`fn/DynamicIndexFindEntry`, `fn/DynamicIndexResolveRange`,
`fn/EncodingFindCKey`, `fn/EncodingEspecStringIndex`), so a change in
behaviour would fail there rather than here.

**The whole gate is green in 99.6 s** from cold (43.4 s warm, measured on
the operator's machine): fmt, the four python doc checks, clippy with
warnings as errors, 84 test binaries in a pool, and the doc tests. The
slowest binary is 13.2 s; the compile is 21 s. The gate's floor was 935 s.
`cli.py casc` is 35/35 in 0.9 s warm.

That makes the AGENTS.md gate suspension stale: the reason it gives - "over
an hour of wall clock, 98% of it in seven install suites" - no longer
holds, and the unscoped `cargo xtask verify` is now the cheapest way to
check the tree. The operator lifts the suspension; the section says to
delete it when they do.

## 2026-11-10 - the harness stops accepting a stale "equivalent mutant"

Nine units carried `knownEquivalentMutants` declarations whose line numbers no
longer named the code they described: a refactor had moved the statement under
them. A declaration that names nothing skips nothing, so the equivalent mutant
was probed again, survived (it *is* equivalent), and the kill rate fell - far
enough, for `fn/Salsa20Xor`, to drop the unit below its gate. That is how the
first harness-v8 `cli.py casc` run came back 34/35.

Three changes fix it and stop it:

- the declarations are re-pointed at the live sites: `fn/ArchiveSetContains`
  819 -> 894 (the *first* `h ^= h >> 33`, the one over the u32 archive number;
  the second, after the multiply, is not equivalent and stays probed),
  `fn/Md5Final` 737/741 -> 815/819, `fn/ParseBoolString` 225 -> 226,
  `fn/TextureDescriptorSizeWalk` 305 -> 311,
  `fn/OrthonormalizeCameraBasis-{cargo,state}` 56/76 -> 54/74, the five
  `fn/CurveKeySampleDual` lines in the regenerated C decompile (all seven
  lines later), and `fn/BuildMoverPhysicsCommand` 179 -> 207. A duplicated
  `fn/TextureCountBitsShifted` entry (the same statement at its older line)
  is removed, and `fn/FloatToQ12`'s free-text note becomes a line-targeted
  declaration.
- a declaration may now name the **mutated text** as well as the kind and the
  line, because one line can carry two mutants of the same kind (a decimal and
  a hex constant each get a boundary shift) and the old key could not tell
  them apart. `fn/BuildMoverPhysicsCommand` needed exactly that: its two
  equivalent boundary mutants sit on the same line as two that are not.
- the harness now **fails a unit whose declaration names no live mutation
  site** instead of verifying it against a weaker mutant set, and the ledger
  replays a verdict only when the declarations that produced it are unchanged.

Two of the re-pointed declarations were also wrong in substance and were
rewritten. `fn/ParseBoolString`'s `boundary -1` is the mutator's *hex* shift
of 0x41 to 0x40, not the bound it described; `fn/BuildMoverPhysicsCommand`'s
boundary mutants are on the shift and on the constant, not on the `& 1` term -
and the two siblings on that line (`>> 2` and 0x13) are **not** equivalent, so
they stay probed and killed.

Measured after the fix: `fn/ArchiveSetContains` 9/9, `fn/Md5Final` 9/9,
`fn/ParseBoolString` 10/10 mutants killed - all three were at 82-83% before,
passing their gate by luck. `cli.py casc` is 35/35 again. The harness's own
suites pass, two of them extended: `test_harness_smoke.py` proves the two
same-line boundary mutants are separable by their mutated text, and
`test_run_unit_e2e.py` runs the whole ladder on a unit with a stale
declaration and expects the inconclusive verdict and the note that says why.

## 2026-11-10 - the harness grows its own corpus (`autoCorpus`, harness v8)

A fixture was the price of every unit: 105 modules, 15,078 lines, ~143 lines
each, and the long tail of unverified functions would cost another order of
magnitude of hand-written corpus. A unit without an `argFactory` can now set
`options.autoCorpus`: after the base corpus (edge pool + `iterations` seeded
draws) agrees on every channel, the runner measures the branch coverage of
those very runs and keeps comparing draws from the same deterministic
sequence until the coverage gate is met or the cap (1200 cases, `{"cap",
"step"}` overrides) is reached. Growth only adds compared cases through the
same per-case body - same channels, same undeclared-access accounting, same
counterexamples - and the mutation corpus gains the grown tail, because a
constant-edit mutant can be distinguishable only on the branch a grown case
uncovered. Ledger entries now record the realized corpus (digest + case
count); it is a fingerprint, not a freshness input, since the corpus is a
deterministic function of what the ledger already binds. The five stages,
every gate and the fixture path are unchanged; `autoCorpus` + `argFactory`
warns and ignores the option. `scaffold_unit.py` templates opt in, so a new
flat/scalar scaffold needs args and returns, not a fixture. Proof:
`test_auto_corpus.py` (growth to a magic draw, cap exhaustion, digest
determinism, replay, toggle-staleness); the three existing harness suites
pass unchanged. HARNESS_VERSION 7 -> 8, so the first `all`/`casc` re-proves
once. Design and measurements: `dev/verify_harness/PERFORMANCE.md`'s 2026-11
growth section.

## 2026-11-10 - the page cache's read path, and four audit P0 rows

`PageCacheRead` (`0x1424fb1c0`) is now followed operation for operation, and
reading it overturned the recorded order. The client locks the table, calls
`PageSlotFind` (`0x1424fa440`) for the **head** (`0x1424fb301`), then for the
**tail** (`0x1424fb355`), unlocks (`0x1424fb37e`), and only then calls
`PageLoad` (`0x1424fa540`) for the head (`0x1424fb3a4`) and the tail
(`0x1424fb594`). `docs/formats/casc.md` and the module doc both put the tail
lookup at the tail's load; the disassembly says otherwise. The difference is
observable: `PageSlotFind` bumps `page_table+0x20` and the chosen slot's stamp
on **every** call (`0x1424fa4ff..0x1424fa50a`), so the lookup's pass makes its
slot the freshest and `PageLoad`'s pass skips it. A cold read therefore fills
every other slot - after 17 distinct pages only slots 1,3,5,7,9,11,13,15 are
resident, holding pages 16,9,10,11,12,13,14,15. Three unit tests and the
install test `page_cache_real` encoded the old single-pass behaviour and now
pin the client's. `eviction_is_least_recently_used` keeps its name: the victim
rule is unchanged (largest wrapping `clock - stamp`, ties to the lowest
index), only the stamp sequence differs. The `table+0x30` closed flag is set
to 0 by `PageCacheInit` (`0x1424f88e1`), so the lookups are on.

The same pass closed four rows of `docs/audit-ow-tank.md`:

* **P0-6** - `clip.rs` ships `anim_time_wrap`, the client's `AnimTimeWrap`
  (`0x140b91ef0`) operation for operation: `maxss(minss(t, max), min)` when the
  node flag dword's bit 0 is clear, `t - (float)(int)(t / max) * max` when it is
  set, with SSE NaN fall-through and `cvttss2si`'s integer indeterminate. Both
  `frame_at` implementations route through it over `[0, frames]` with the flag
  clear, and the `+1` is gone - the stored keys are 0-based. The wrap branch
  stays unreachable from `frame_at` because the flag lives in the animation
  node record, which the clip header does not carry; the read range is the
  node's first 16 bytes (`+0x00` min, `+0x04` max, `+0x08` slot2/noise,
  `+0x0c` flags). The `+0x24` wrap shaping is **open** - the sampler
  `FUN_140b9a790` does not read it.
* **P0-7 / P0-8** - `Skin::posed_world` now replaces the bind's **local**
  matrix, the same rule as `posed_world_full`, and the three tests that had
  been adjusted to the old absolute-rotation behaviour assert the read rule
  again. The old path derived every bone's local from the posed absolutes, so
  a posed bone's children did not follow.
* **P0-10** - `read_placeable` bounds its reads by the placeable's declared
  `size`.
* **P0-13** - `Kind::type_id` now returns the client's table id (`Option<u8>`),
  and `Model`/`SingleModel` return `None`: the client's table at `0x1439aa320`
  has no `0x02` and no `0x08`, and its dispatch is by **slot**
  (`ChunkSlotForGuid`, `0x141b32e70`), not by id.

The `Z` block path was also attacked. The client's inflate at `0x1425233d0`
is zlib 1.3 (`inflate 1.3 Copyright 1995-2023` at image `0x143477751`), and a
from-scratch port now sits in `crates/ow-casc/src/inflate.rs` with the
client's `z_stream` and `0x1bf0`-byte `inflate_state` layouts and all five
helpers identified (`0x142528250` crc32, `0x142527e70` adler32,
`0x142525030` updatewindow, `0x142528700` inflate_table, `0x142528cc0`
inflate_fast). Two real bugs were found and fixed - the `CODELENS` pass did
not drop the code's bits, and the Huffman lookup did not re-read the entry
after pulling bytes - but the port still fails on a repetitive payload (two
zero bytes at record 686) and on the install suite (`inflate: -5`). The `Z`
path therefore stays on `flate2`; the port is documented as incomplete in the
module doc and in `docs/formats/casc.md`, with the failing case kept as an
`#[ignore]`d test. No `fn/Inflate` harness unit exists, so nothing about the
port is claimed as 1:1.

Verified: `cargo test -p ow-casc --lib` (135 passed, 1 ignored by design),
`cargo test -p ow-casc --test page_cache_real` (1 passed),
`cargo test -p ow-casc --test blte_real` (1 passed, 238 s), `cargo clippy -p ow-casc --all-targets --
-D warnings` clean, `cargo test -p ow-tank --lib` (389 passed), `cargo test -p
ow-tank --test mapchunk_kinds` (16 passed), `cargo test -p ow-scene --test
skin_pose_gate` (7 passed in 1765 s - the real 200-bone rig, where the two
pose formulations genuinely differ). The unscoped `cargo xtask verify` was
**not run** (suspended).


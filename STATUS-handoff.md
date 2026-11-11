# Handoff: finish `ow-casc` (30 entries without a unit)

## This session (2026-11-11): finding and reading a function is one command

The operator's ask: find the client's functions quickly, and read one without
tracing it down first. The audit: the answers were in the image and in the
corpus index, but nothing joined them.

**What was slow, measured:**

- the whole-image index (`corpus.py index`: 119,802 functions, 1.87M call
  edges, 35,835 slot dispatches) had **no query interface** - "who calls this"
  meant running `callers.py`, which rescans the 50 MB image every time
  (1.3 s) and names the caller only if a human has named it;
- the corpus held **923** built functions (675 with a decompile), so
  `corpus.py show` answered "not in corpus: build the range first" for
  everything else;
- `imm_scan.py` - the one tool that finds a format by its magic - is broken
  for the offline image (it wants a `<image>.json` side file that does not
  exist) and reports raw file offsets instead of addresses;
- the Ghidra bridge is the only source of decompiled C, and it needs
  `Overwatch.exe` open. **Right now it does not have it**: the GUI holds
  `LaunchTG.exe` and no project (`list_project_files` answers "No project is
  currently open"), so the read path is the offline disassembly until the
  operator reopens `dev/ghidra_run/owmod_clean`.

**What is now:** `dev/fn.py`.

```text
python dev/fn.py read 0x141b88f10      # one function, everything known about it
python dev/fn.py find MRNM             # a chunk tag -> the dispatcher
python dev/fn.py find type:0x0b3       # an asset type -> the client's loader job
python dev/fn.py scope 0x141b5df70     # every function a format reaches
python dev/fn.py jobs                  # the client's asset-type registry
```

`read` gives the name and role, the unit that tests the function and that
unit's verdict, every `docs/` mention, the true extent, the whole-image
callers and callees, and the disassembly - in about 0.4 s, with no Ghidra
session and no built corpus range. It reads bodies the unwind table does not
cover (`MrnmFixup` `0x141b893e0` is one).

**Two client facts were read on the way** (`docs/formats/asset-types.md`):
`JobForType` `0x141fd8300` (type -> loader job, through the u32 table at
`0x143a7c3a0` and the qword job table at `0x1441b3cb0`, which is zero-fill
and exists only at runtime), and the fact that the client compares a chunk tag
as a **big-endian** u32 (`MRNM` is `0x4d524e4d`).

**Open:** the job id -> registration function mapping (item 19 of
`docs/open-questions.md`). With it, `scope` over each of the 33 jobs is the
whole client-side surface of `ow-tank` in one query.

**Item 19 is now answered (same session): the job id -> loader mapping is
read.** `0x141fd89f0` is the call that fills a job slot in the runtime job
table, so its callers are the registration functions, and the descriptor each
passes holds the job id at `+8`, the **load** callback at `+0x48` and the
**release** callback at `+0x78`. 32 of the 33 jobs are resolved:

```text
python dev/fn.py jobs                    # job, types, registration, loaders
python dev/fn.py jobs --crate ow-tank    # and whether the crate cites them
```

The write was invisible to a displacement scan because the slot address is
computed with a `lea` and the store uses no displacement. The read confirms
the project's own claims from the image: the material asset (type `0x008`) is
job 2, loader `0x141b1a8e0`; the material data (type `0x0B3`) is job 28, load
`0x141b1a7b0`, release `0x141b1a790`. The shader jobs are read too (23 ->
`0x141e73f30`, 24 -> `0x141e74360`, 25 -> `0x141e73a80`) - the first address
the shader document has for its loader. Whole table in
`docs/formats/asset-types.md` section 1b.

**The crate cross-check is now a command.** `python dev/fn.py jobs --crate
ow-tank` marks each loader the crate cites: of the 32 resolved jobs,
`ow-tank` cites the loaders of **jobs 1, 15, 28 and 31** (`0x141dacf00`,
`LoadMapAssetChunks` `0x141ff5c80`, the material data loader `0x141b1a7b0`,
`FinishMapChunkAsset` `0x141ff54e0`) and none of the other 28 - so the
material, shader, map-asset and generic loaders are unanchored by the crate.

**The client's own names for its assets are read too** (`python dev/fn.py
names`). Every registration function deobfuscates the name of the asset it
registers - the same XOR-with-a-key loop `formats/chunk-kinds.md` documents -
and stores it next to the asset's type id, so the names come straight out of
the image. 37 are decoded; the assets among them: `MODEL` (`0x00C`),
`MODELVERTEXDATA` (`0x161`), `FRACTURABLE` (`0x112`), `SKELETON` (`0x007`),
`MATERIAL` (`0x014`, no loader), `MATERIAL DATA` (`0x0B3`), `TXTR` (`0x0F1`),
`TXTR_PAYLOAD` (`0x04D`), `SHADER REFLECTION DATA` (`0x040`), `SHADER CODE`
(`0x087`), `MAPDATA` (`0x002`), `SOUNDBANK` (`0x043`), `VOICEWEMFILE`
(`0x0B2`), `SOUNDWEMFILE` (`0x0BB`) and the three lighting ones. The same
functions register the `SSAO.*` config variables with their help texts.
`MATERIAL DATA` and `SHADER CODE` are names the crate had taken from a
reference library; they are now the client's. Two names (`SHADER GROUP`,
`STU`) have no resolved type id and are reported as unknown.

**Tried on `ow-tank` (same session), and it measured the gap.** `python dev/fn.py
surface ow-tank` reports the named client functions reachable from what the
crate cites but not anchored by the crate: **86**. `docs/ow-tank-1to1.md`
section F has the table. The ones that name a real omission: `MoclRead`
`0x141b88e50` (the client reads the `MOCL` chunk; the crate only names the
tag in a comment), `MapChunkHeaderFixup` `0x142e49360` (the chunk
header-record fixup; the crate implements only the collision fixup
`0x142e492a0`), `TypeHashLookup` `0x1421c14f0` (the STU loader's hash
lookup), eight texture functions `texture.rs` does not cite, and the client's
shape-pool / collider-emit functions behind `brush_convert.rs`.

**A full read-and-implement cycle is done (same session), on the ow-tank
side.** The gap list named the candidate: `MapChunkHeaderFixup`
(`0x142e49360`) - the relocator the client's map-chunk dispatch applies to
the pathing kind's header record, which the crate neither cites nor
implements, while its sibling `0x142e492a0` is `chunkkind::fixup_in_place`
and is already verified.

- **Read**: 61 bytes, `0x142e49360..0x142e4939d`. Three self-relative RVAs
  (`+0x00`, `+0x08`, `+0x18`) become pointers (value + the record's own
  address); a stored zero stays zero; `+0x10` is not touched. The client's
  decision is branchless - a `cmove`, not a jump.
- **Implemented**: `chunkkind::fixup_header_in_place`, the three conversions
  written out one at a time.
- **Proved**: `fn/MapChunkHeaderFixup` - the client's bytes against the
  shipped code, 160 differential cases, 4/4 mutants killed. The coverage
  stage finds no branch site (0 of 0) because the client uses a `cmove`; the
  mutation stage is the proof, and the three `== 0` comparisons are the
  function's whole behaviour.
- **Install check**: `collision_cells_real` now fixes up the practice
  range's pathing header and pins the three pointers (`0x20`, `0x862720`,
  `0x862878` stored).

The other three relocators of the same dispatch (`0x142e49270`,
`0x142e493a0`, `0x142e493e0`) are read and still unimplemented.

**Where the suite stands** (`python dev/verify_harness/cli.py all`, 222 s):
**106 of 108** units verified, including the new `fn/MapChunkHeaderFixup`.
The two that are not are the ones the handoff already named: `fn/MeshSweep`
(inconclusive, 61.1% branch coverage against its gate) and
`fn/OrthonormalizeCameraBasis-state` (divergent, the missing guest-pointer
probe arm). `cli.py casc` is 35/35.

## This session (2026-11-11): ow-client gets an install-reading test, and the collision worlds are measured

The operator asked for proper collision in `ow-client` and for everything in
`ow-client` to be verified against the install. `ow-client` had **no**
install-reading test - the walk's numbers were printed and read by eye.

**The world's source is selectable and measured now**
(`--walk-test --collision level|models|both`):

| world | shapes | forward | drift | strafe | grounded |
|---|---|---|---|---|---|
| the level collider | 1 | 5.500 m | 0.000 | 4.540 m | yes |
| the per-model holder meshes | 3,017 | 5.493 m | 0.005 | **5.500 m** | **no** |
| both (what the client has) | 3,018 | 5.190 m | 0.460 | 2.030 m | yes |

**This corrects the earlier reading.** The per-model meshes are not "not the
map's collision": the client has **both** copies in the same node's world
(`0x14201ae60` -> `ShapeAddToBody`/`OwnerCreateBody`). The level collider
carries the spawn room's floor (the walker is grounded, the forward leg is
exact); the per-model copies carry a free strafe but no floor under the spawn
room. The walk survives neither alone nor both because its **slide and floor
stage are stand-ins** - the client's solver (`0x142cf3f80`, 29 KB, still
unread) is what resolves the two copies against each other, and the level
collider's 4.540 m strafe is the slide stand-in stopping on a vertical face.

**The install-reading test** `crates/ow-client/tests/walk_real.rs` runs the
built binary against the install and pins the level collider's exact forward
leg, zero drift, zero idle and grounding, plus the per-model world's free
strafe and missing floor. It skips when the install is not readable.

**Still open, and it is the client's own solver**: `RunCharacterControllerSweep`
(`0x142cbcee0` -> `0x142cf5660` -> `0x142cf3f80`), the Domino pair-list walk
that iterates the position to a 0.005 fixed point. Until that is read and
ported, `ow_sim::walk`'s slide stays a stand-in and the walk keeps the level
collider alone.

**The tool grew a decompiler, a callers fallback and a format grouping.**

- `python dev/fn.py read <addr> --decompile` runs the client's decompiler
  (the project's headless `McpDecompile.java` over the merged working copy)
  and prints the C, cached under `dev/data/decompiles`. It says **why** when
  it cannot. Paths: `GHIDRA_HEADLESS`, `GHIDRA_SCRIPTS`, `GHIDRA_PROJECT`.
- `callers`/`callees` now answer for bodies the unwind table does not cover
  (the index's raw transfer targets), which used to fall back to
  `dev/callers.py` every time.
- `surface <crate>` is grouped by the **crate's own files** now, not by the
  name table's tag: 65 distinct unanchored named client functions across
  `ow-tank`'s 52 files.

**One action for the operator:** the MCP's Ghidra cannot reach
`Overwatch.exe` while the GUI's FrontEnd holds the launcher's project. The
launcher's program was closed (its edits are unsavable by the tool's own
account - "open as an in-memory copy of a versioned file ... lost when it
closes"), but **the project switch needs one click in the GUI** (File ->
Close Project), after which `open_project` with `program: /Overwatch.exe`
will serve the decompiler and the xrefs directly. Until then the headless
route over the merged copy works and is what the tool uses.

## This session (2026-11-11): `fn/ShapePoolAlloc`, and the solver's first pass

**`fn/ShapePoolAlloc` - verified.** The shape-entry pool's per-element
allocator `0x142cb00c0` is read in full and implemented as
`ow_tank::domino_world::shape_pool_alloc`: the head at `pool+0x14`, the
popped entry's own link dword at `entry+0xA0`, the used count at `pool+0x08`,
the high-water mark at `pool+0x10` with the client's signed compare, and the
popped entry's link set to its own index. The unit proves it against the
client's bytes: 96 differential cases, the record and the entries compared
exactly, 5/5 mutants killed, one declared equivalent (a maximum with a
non-strict compare writes the same value when the two are equal - the client's
own `cmovg` has the same property).

The **grow path is not implemented** - it allocates through the client's
runtime allocator, which the crate does not model - so the case is refused
rather than guessed. That is also why the coverage gate is 0: the grow path
cannot run offline, so its two branches are never reached.

**Correction to `formats/shape-ingest.md` §3:** the grow path copies the old
array's `capacity * stride` bytes, not `used * stride` (the copy at
`0x142cb0145` reads the capacity at `pool+0x0C`).

**`0x142cf3f80`, the solver, read to the first pass.** 3,575 bytes over ten
`.pdata` pieces, **67 direct calls**, no indirect call. Established: the
world's variant tag dispatches the mode (`cmp dword ptr [rcx], 1` at
`0x142cf3fa6`); the character path takes the handle at `[world+0x08]`; the
pair array is allocated `count * 0x20` bytes at `0x142cf43c9` and filled by
`0x142cd4d90`; and the pair loop at `0x142cf43f0` walks the world's fixture
list at `[world+0x28]`, calls the pair getter `0x142cbb260`, decodes the
fixture handle with `0x142cb9d60`, rejects an empty world slot and an
invalid body, and looks the body up with `0x142cc5730`.

**What that says about the port:** the solver is not one transcribable
function - it is the Domino runtime's collision (the world's pools, the
allocator, the handle encoding, the body records, the narrowphase). The next
portable piece is the pair build `0x142cd4d90` and the position resolve
`0x142cd5270`; the pool's free-list pop they allocate through is now
implemented.

**Suite:** `cli.py all` is 108 of 110 verified - the new `fn/ShapePoolAlloc`,
`fn/MeshShapeFill` and `fn/MapChunkHeaderFixup` are in it, and the two that
fail are the pre-existing `fn/MeshSweep` and `fn/OrthonormalizeCameraBasis-state`.

## This session (2026-11-11): the walk sweeps the client's capsule, and the resolve loop is read

**The walk's shape is the client's now.** `ow-client` swept a `0.35` m
**sphere** - a shape the client does not have - and the walker's position was
that sphere's *centre*. The client gives the character a **vertical capsule**
whose origin is at its **feet**: `radius 0.5`, height `1.5` on the install's
common hero, read from the mover component's three class fields
(`0x1972C687`, `0x7A70E3E3`, `0x4A576BC2`) through
`ow_tank::entity::mover_capsule_of` - already implemented and proved in
`ow-tank`, and simply not used.

- `ow_sim::walk` gained `Capsule`, `advance_capsule` and
  `snap_to_floor_capsule`: the same fold, the same depenetration and the same
  slide, with the client's shape in the query. Five new unit tests pin the
  difference (the origin rests on the floor, the sphere's centre rests a radius
  above it, the floor probe answers with the feet, the capsule stops a radius
  further out).
- `ow-client`'s `Walker` holds the capsule; a picked hero's own comes from the
  same read the other mover scalars use. The position is the capsule's origin,
  so the spawn resolves to the room's floor (`y = -4.006`); the camera's world
  height is unchanged (`1.60` m above the origin - the height the sphere proxy
  had).

**The resolve loop is read** (`docs/formats/character-controller.md`, "The
resolve loop, read"). `0x142cf3f80`'s character path:

```text
again:
    pair_build(handle)                   ; 0x142cd4d90
    collect the contacts (plane, separation, entity)
    resolved = resolve(handle, &pos)     ; 0x142cd5270 -> 0x142cd4610
    if (resolved - pos)·(resolved - pos) >= 0.005^2 and ++iter < cap: goto again
body+0x620/+0x628/+0x62c = resolved ; body+0x664 = iter
```

Four facts: the convergence test is a **squared** length against
`eps = 0.005`; the correction is measured against the **requested** position;
the resolve is the **Domino runtime's** own move (`0x142cd4610` rebuilds the
body's broadphase entry and lets the runtime push it out); and **the client's
resolve has no slide**. The loop's bound is the sweep block's `+0x1c`, which
the advance fills from `step_state[0x94]`.

**Why that matters for the walk.** The walk's stand-in does the opposite: it
projects the *residual* along the contact normal and keeps going, which is
what makes a body in a corner drift sideways instead of stopping. The
runtime's depenetration is not ported, so the stage stays open - but the
shape, the convergence test and the cap are now read.

**Measured, and the honest result is that the capsule makes the two copies
harder.** The walk's numbers on the practice range's spawn room:

| world | forward (sphere) | forward (capsule) | drift | strafe (sphere) | strafe (capsule) |
|---|---|---|---|---|---|
| the level collider only | 5.500 m | **5.500 m** | 0.000 m | 4.540 m | 4.519 m |
| the per-model holder meshes | 5.493 m | 4.978 m | 0.764 m | 5.500 m | 5.500 m |
| both - what the client has | 5.190 m | 4.999 m | 0.628 m | 2.030 m | 1.902 m |

The capsule is `0.15` m wider than the proxy, so it meets coarse holders the
proxy passed and stops a little earlier at the room's wall. The shape is the
client's; the *response* is what is missing. `crates/ow-client/tests/walk_real.rs`
pins all four rows - including the broken "both" row - so the gap cannot be
forgotten.

**The next implementable link, named.** The resolve needs the runtime's body
records and its contact collection (`0x142cc5600` and the triangle fold), and
the pair build (`0x142cd4d90`) needs the world's fixture list and the handle
encoding. Those are the pieces a faithful resolve is made of; the shape stage
is now done.

## This session (2026-11-11): the collision blockers

The operator asked for the collision blockers. Two of them moved:

**`fn/MeshShapeFill` - read and proved.** `MeshShapeFill` (`0x142cae540`,
89 bytes) is the only writer of the 0x4c-byte shape record the collision query
consumes. The crate's `ow_tank::mesh_shape::shape_record` is that expression
dword for dword, and the new unit proves it: 96 differential cases on every
channel, 8/8 mutants killed, the record compared bit for bit with no masked
slot. The five `MeshShapeFill` callers are all named now: the character
controller (`0x140c264b0`), the geometry module (`0x142077580`), the chunk
assembler (`0x14209fc90`), the model route (`0x14210bac0`) and
`ShapeEntryFromDescriptor` (`0x142cb7f10`).

**A doc correction that matters.** `docs/formats/shape-ingest.md` §6 said
`BuildBvh`'s three callers were split wrappers and unreachable. They are not
three functions: `0x142cca7d9` is a fragment of `ColliderFaceAppend`
(`0x142cca460`) and the other two addresses are interior points of `BuildBvh`
(its own recursion). `BuildBvh` has exactly one external caller, and it is
reachable:

```text
0x142137480 -> 0x14213bbc0 (the geometry driver)
                 -> 0x14213a510 AppendCollisionEntryFromBrushSource
                 -> 0x14213acf0 (the 0x60-entry builder)
0x14213c440 GeometryBuild -> 0x142cf2010 MeshToColliderFaces
                            -> 0x142ccc120 ColliderMeshEmit
                            -> 0x142cca460 ColliderFaceAppend
                            -> 0x142ccb1b0 BuildBvh (fn/BuildBvh, verified)
```

Round 5's conclusion stands - the collision chunk's sectors are not a
producer of Domino shapes, the brush is - but it now rests on named functions.

**Still open, with the next implementable link named.** The shape-pool side
is read and unimplemented: `ShapePoolAlloc` (`0x142cb00c0`, the free-list
pop - the head at `pool+0x14`, the link at `entry+0xA0`, the used count at
`pool+0x08`, the high-water mark at `pool+0x10`, the stride `0xC0`, the grow
path `cap/2 + cap` with a minimum of 2) is the next function to implement and
prove; its grow branch is unreachable offline (the allocator's heap vtable is
null in the image), so a unit would prove the pop only. The level geometry's
own driver (`0x14213bbc0`) and its two builders (`0x14213a510`,
`0x14213acf0`) are named but unread.

**A real bug in `dev/crate_coverage.py`**: it counted only the `0x14…` form
of an address, so the `FUN_…` form a human writes in prose was invisible -
16 of `ow-tank`'s citations and 13 of `ow-casc`'s. Fixed; `ow-tank` is now
254 cited addresses (was 238), `ow-casc` 212 (was 199).

## This session (2026-11-11): the verification got cheap

The operator's ask: make the verification cost a small fraction of the time
spent reading and rewriting client functions. Two cost centres, both fixed.

**The differential harness's mutation stage** was ~80% of its wall time: a
cargo unit's mutants were each a full crate rebuild (4.4 s warm), and a unit
has a dozen. Now every mutant of a unit is an **arm** of one dispatch inside
the function under test (`dev/verify_harness/batch.py`), so the crate is
compiled **once** and the harness picks the arm with `OW_MUTANT_INDEX`. The
default arm is the original body and the harness checks it against the
baseline candidate before it trusts the batch; a compiler error is attributed
to its arm (dropped, rebuilt, counted as a kill); anything unattributable
falls back to one build per mutant. Measured per unit:
`fn/OrthonormalizeCameraBasis-cargo` 23.2 s -> 6.6 s,
`fn/BuildMoverPhysicsCommand` 17.2 s -> 5.8 s, `fn/Md5Final` 17.0 s -> 7.5 s,
with the same verdicts. The harness now prints each stage's wall time.

**The Rust gate's seven install-reading suites** (2218 s of the 2246 s of
test time) were two rebuild-per-call mistakes in `ow-casc`:
`SegmentEntries::key_state` rebuilt the segment's whole entry buffer on every
call (the per-key sweep copied tens of gigabytes), and `Store::open_ckey`
re-parsed the 100 MB encoding manifest on every asset open (the parse's
expensive half is the espec string index). Both are now built once:

| suite | before | after |
|---|---:|---:|
| `ow-casc` `index_find_real` | 1154 s | 1.5 s |
| `ow-scene` `skin_pose_gate` | 428 s | 4.4 s |
| `ow-tank` `effect_real` | 213 s | 4.0 s |
| `ow-scene` `practice_launch` | 186 s | 5.2 s |
| `ow-scene` `hero_draws` | 159 s | 2.8 s |
| `ow-tank` `hero_pick_real` | 152 s | 2.8 s |
| `ow-tank` `brush_real` | 134 s | 2.7 s |

Nothing was sampled away and no on-disk cache can go stale: the cache is the
reader's own state. Every assertion is unchanged (the effect census is the
same 7,342 assets and 419,488 chunks), and the differential harness covers
the changed code.

**What the operator can now do:** `cargo xtask verify` is no longer an
hour-long event - it is **99.6 s**, green, with the slowest test binary at
13.2 s (it was `index_find_real` at 935 s) - so the AGENTS.md's gate
suspension can be lifted. The measurements are in MODLOG 2026-11-11 and
`dev/verify_harness/PERFORMANCE.md`'s last section.

**Left open, with the numbers:** the harness's mutation stage is still the
biggest cost (1955 s of a 542 s run, over 106 units), and most of it is the
mutant *runs*, not the builds: a mutant that hangs waits out the call
timeout twice (10 s + 20 s). About ten mutants in the suite hang. The
timeout is a gate - it decides a hang from a slow-but-finite mutant - so it
was not touched.

## This session (2026-11-10): the harness stops accepting a stale declaration

The work was the harness, not a client function. `cli.py casc` came back
**34/35** on the first harness-v8 run: `fn/Salsa20Xor` failed with "70% kill
rate" because its `knownEquivalentMutants` declaration named a line the
candidate's source no longer had (a refactor had moved the statement under
it). A declaration that names nothing skips nothing, so the equivalent mutant
was probed again, survived - it *is* equivalent - and the kill rate fell below
the unit's gate.

Nine units carried the same rot. All are re-pointed at their live sites,
two of them were wrong in substance as well and were rewritten, and the
harness now **fails a unit whose declaration names no live mutation site**
instead of verifying it against a weaker mutant set. A declaration may also
name the *mutated* text, because one line can carry two mutants of the same
kind (a decimal and a hex constant each get a boundary shift) and the old
kind+line key could not tell them apart. The ledger replays a verdict only
when the declarations that produced it are unchanged.

`cli.py casc` is **35/35 verified** again (the suite has 35 units; the
previous session's 31 predates four more). The kill rates of the re-pointed
units went from 75-83% to 100%: `fn/ArchiveSetContains` 9/9, `fn/Md5Final`
9/9, `fn/ParseBoolString` 10/10, `fn/TextureDescriptorSizeWalk` 11/11,
`fn/OrthonormalizeCameraBasis-cargo` 9/9, `fn/BuildMoverPhysicsCommand`
9/9, `fn/FloatToQ12` 11/11, `fn/TextureCountBitsShifted` 2/2,
`fn/Salsa20Xor` 7/7.

Details, addresses and the proof: MODLOG 2026-11-10 ("the harness stops
accepting a stale 'equivalent mutant'"), `dev/verify_harness/PERFORMANCE.md`'s
last section, and the two new harness test cases.

**The local gcc works now.** The "broken `gcc`" that blocked the two tier-1 C
units is gone: `fn/AnimTimeWrap` **verifies** (100% coverage, 8/8 mutants),
and `fn/CurveKeySampleDual` **verifies** too (100% coverage - 14/14 reachable
branches of 16 sites, two of them declared with one dead edge - and 10/10
mutants). Two reads closed it: the binary search's back edge
(`cmp rax,0x40 / jae` at `0x140b90a80` and `0x140b90b70`) needs a 128-key
track, because the window `[0, count-1]` halves once per pass and 80 keys
leave 40; and the linear scan's entry test (`0x140b90a92` / `0x140b90b82`)
can never be taken, because the scan is entered with `times[lo]` and `lo` is
0 or a key strictly below `t` - declared in `options.unreachableBranches`.

## The previous session (2026-11-19): four more units, four walls closed

The assignment below was the previous session's. This session continued the
same loop on the top of the playbook queue and took four functions:

| unit | address | result |
|---|---|---|
| `fn/ConfigKeyEntryParse` | `0x1424cebd0` (481 B) | 87.5% both-edge (14/16 of 21 sites, 5 dead edges), 11/11 mutants + 1 declared equivalent |
| `fn/DynamicIndexResolveRange` | `0x1425115d0` (291 B) | 100% (7/7), 5/5 |
| `fn/PageSlotFind` | `0x1424fa440` (247 B) | 100% (12/12), 12/12 |
| `fn/CdnIndexPageSelectBatch` | `0x1424da910` (660 B) | 100% both-edge (18/18 of 20 sites, 2 dead), 11/12 mutants |
| `fn/Memcmp` | `0x142efecf0` (199 B) | 100% (17/17), 6/6 |

`cli.py casc` is **31/31 verified**, re-proved end to end under harness v7
(the ledger entries were v6, so the first v7 run re-runs every unit).
`cli.py all` was not re-run this session; adding `fn/Memcmp` to the last
count gives **99 verified of 104 units** (99 of 100 distinct addresses),
the same five non-verified ones as before, listed below.
`cargo xtask verify --fast` is green (fmt, clippy, 828 lib tests, tool index,
deviation/frame/layout checks). `cargo xtask verify` (the install-reading
half) does not finish on this machine because of a pre-existing slow test -
see "Measured, not fixed" at the bottom; the xtask change below overlaps
that wall instead of summing it.

### xtask speed (same session): `cargo xtask verify` runs the test binaries in a pool

One `cargo test` walks the ~50 test binaries serially, and each install
suite re-opens the install first, so the gate paid the **sum** of the
suites (~30 min) where the work itself allows the **max**. xtask now
builds every binary once (`cargo test --no-run --message-format=json`),
reads the executables out of the compiler-artifact messages (a dependency-
free JSON reader inside xtask) and runs them in a pool - `--test-jobs` /
`XTASK_TEST_JOBS`, default `min(4, cpus)`; read-only installs make it safe
and the shared page cache makes it faster than the serial walk. Cheap
checks (fmt + the four python doc checks) moved ahead of the compiling
stages, `--fail-fast` ends the run at the first failing stage, failures
print their captured output after the pool drains, and every binary
reports a duration. Same stages, same binary sets for `--fast` and full,
same verdict. The 19-minute `index_find_real` floor is untouched (a perf
change, not a port). Measuring commands:

    cargo xtask verify --test-jobs 1    # the old serial walk
    cargo xtask verify                 # default pool
    cargo xtask verify -p ow-casc --skip index_find_real   # one crate, minus the 19-min suite

A scoped run is an iteration loop, not the gate: `-p`/`--package` (repeatable)
scopes clippy, the test binaries and the doc tests; `--skip` (repeatable) drops
a test binary whose label matches (`--skip index_find_real`) and otherwise is
libtest's own per-test filter for the binaries that still run; both always
print what they left out. Unscoped `cargo xtask verify` remains the definition
of done. A bin-only crate named with `-p` no longer trips the doc stage: xtask
asks `cargo metadata` which named packages have a library at all.

xtask's own 20 unit tests (JSON parser, artifact selection, the pool
against real scripts) pass.

`crate_coverage.py ow-casc`: **29 verified / 29 `entry` without a unit / 84
interior / 9 code / 3 data** over 154 cited addresses.

### The units that are not verified (2026-11-10: three, not five)

| unit | why |
|---|---|
| `fn/MeshSweep` | 61.1% branch coverage, below its gate |
| `fn/OrthonormalizeCameraBasis-state` | missing guest-pointer probe arm (reports `divergent`) |
| `fn/MoverAdvance` | declared scaffold (`"scaffold": true`), reported SKIP |

`fn/AnimTimeWrap` and `fn/CurveKeySampleDual` left this list on 2026-11-10:
the local `gcc` compiles the tier-1 C candidates again, and both units
verify. `fn/CurveKeySampleDual` needed two reads (the search's back edge
wants a 128-key track; the linear scan's entry test has two dead edges,
declared in `options.unreachableBranches`). The harness's own suites are
green again for the same reason.

### Harness speed (same session): `cli.py all` is an iteration gate now

The 45-minute `all` was re-proving every unit on every run. Harness v7
(same five stages, same corpus, same gates; see `dev/verify_harness/
PERFORMANCE.md`'s 2026-11 section): `all`/`casc` replay a ledger entry whose
every bound digest still matches instead of re-running it (`--force`
re-proves, a named unit always re-runs); units run in a thread pool
(`--unit-jobs`); mutant build slots are a knob (`--slots`). Naming several
units runs them as one parallel batch - that is the subagent-batch verify
step above, without the "they can block each other" caveat. First run after
v7 re-proves everything once (entries carry version 6). To measure on the
operator machine:

    python dev/verify_harness/cli.py casc --force     # one full re-run
    python dev/verify_harness/cli.py casc             # expect all (cached)
    python dev/verify_harness/cli.py all --force --unit-jobs 4 --slots 6
    python dev/verify_harness/cli.py all              # expect only stale units

The harness's own tests (`test_harness_smoke.py`, `test_run_unit_e2e.py`,
new `test_cache_and_parallel.py` and `test_cargo_mutant_builds.py`) pass;
the two older ones had been broken since the image sha pin and now register
their synthetic images' digests explicitly (`test_cargo_mutant_builds.py`
skips itself where cargo is absent). The real-suite wall time is **not measured here** (no oracle
image on this machine).

## The assignment (previous session)

"lets try to finish the rest with subagents, delegate them to work on the
functions, they dont need to use /verify_harness/cli.py because they can block
each other. when one batch is done you can verify yourself with cli.py and if
we got problems reopen subagent to fix it. work autonomously with that loop
until full casc is implemented and verified"

So the loop is: **delegate a batch → when the batch finishes, run the harness
myself → if a unit fails, re-dispatch a fixer → repeat until `ow-casc` is
fully implemented and verified.**

## Current state (measured, `python dev/crate_coverage.py ow-casc`)

- 25 verified / 33 `entry` without a unit / 48 interior / 7 code / 3 data
  (116 cited addresses).
- `cli.py casc` is 27/27 PASS in `dev/verify_harness/ledger.json`; the unit
  count went 20 → 27 since the first draft of this page.
- Newly verified since this page was written: `fn/BuildInfoSelectRow`,
  `fn/EncodingEspecStringIndex`, `fn/DynamicIndexAppendInterval`,
  `fn/DynamicIndexDecodeEntries`, `fn/DynamicIndexEntrySink`,
  `fn/DynamicIndexFindEntry`, `fn/HexPairDecode`.
- Last results this session: `fn/ParseBoolString` (9/9 branches, 10/10
  mutants, 2 declared equivalent) and `fn/DynamicIndexNormalizeIntervals`
  (8/8, 12/12).

**Governing rule — playbook §10**: a system is done when every function on its
path has a unit and a fresh `verified` verdict, **or is declared open for a
specific read reason**. So the end state may legitimately include "read, open,
because X" rows.

## The five walls (already read, already documented — do not re-read)

**Walls 2, 4 and 5 are closed (2026-11-19); walls 1 and 3 stand.** Each closed
one failed only on a path that can be declared a gap: the vector grow (2), the
single-pass rewrite that was missing from `pagecache.rs` (4), and a
"marshalling job" that turned out to be four flat fields (5).

In `docs/casc-1to1.md` ("The same wall, measured (2026-11-17)"):

1. `BlteFrameHeaderParse` `0x1424a90e0` — chunk table allocated via three IAT
   calls (`0x1424a92a4`, `0x1424a9301`, `0x1424a936c` → import slot
   `0x143aafdc9`; type-`0x10` extras via `0x143aafdd0`). Oracle maps no heap.
2. `ConfigKeyEntryParse` `0x1424cebd0` — grows its vector through an allocator
   vtable (`[rax+0x10]` at `0x1424cec83`; free at `0x1424cecaf`). It does call
   the verified `ParseHexKeyId` (`0x14248f040`).
3. `PageCacheInvalidateAll` `0x1424fab50`, `PageCacheInvalidateRange`
   `0x1424fabd0`, `PageCacheClose` `0x1424f9eb0`, `PageCacheInit` `0x1424f8790`
   — execute the client mutex `0x142694c20`/`0x142694c50` inside the window.
4. `PageSlotFind` `0x1424fa440` — lock-free, but the client does the key-hit
   test and the LRU-victim choice in **one** pass that returns the chosen slot,
   touches it and writes a `failed` flag; the crate splits it into
   `PageCache::find` + `PageCache::claim` (called from `read`). A unit needs the
   client's single-pass function written into the shipped `pagecache.rs` first.
5. `DynamicIndexResolveRange` `0x1425115d0` — operates on the whole segment
   object; the crate's `IndexSegment::resolve_range` builds the key state
   itself. A marshalling job, not a body read.

Plus the already-declared: `CdnIndexPageResident` `0x1424da590` (HTTP range
read in streamed mode) and `PageReadPlan` `0x1424faa20` (container lock).

## Measured, not fixed (2026-11-19; xtask note 2026-11-19)

- `crates/ow-casc/tests/index_find_real.rs::find_range_and_duplicate_choice_match_the_install`
  takes **1154 s (19 min) and passes**. It walks every install key group and
  calls `Store::location_of` + `Store::resolve_range` twice per group; each call
  rebuilds the segment's whole 18-byte entry buffer
  (`SegmentEntries::key_state`) and a fresh `KeyState`, so a 1.1M-key install
  means tens of GB of copying. It used to make plain `cargo xtask verify`
  need ~25 minutes here; since the xtask pool change it sets the gate's
  floor (~19 min) instead of being paid on top of the other suites. Not a
  failure. Pre-existing (the test predates this session and nothing here
  touches `key_state`), and not fixed: the honest fix is a
  `key_state`-free resolve path, which is a perf change, not a port.
- The `ow-tank` install tests (`component_payload_sizes_match_the_client`,
  `effect_assets_parse_and_every_component_keeps_the_clients_widths`,
  `a_model_carries_no_brush_container_chunk`) are the same shape: each runs
  for minutes, so they used to sum into a ~30-minute non-`--fast`
  `cargo xtask verify`; now they run concurrently and every binary's
  duration shows in the gate log, so the slowest one is named, not hunted.

## Next: work in batches, not one unit at a time

Six hours produced three units, and four of those hours were gate runs, not
porting. The loop for the remaining 30 `ow-casc` entries:

1. Take 6-8 leaves from `crate_coverage.py ow-casc --queue` at once. A
   delegate per function: read the body, transcribe to Rust, write the
   fixture, write the unit JSON. Delegates do **not** run the harness.
2. The main session owns the two shared files (`verify_probe.rs` arms,
   `candidate_rust.py` rows) and runs `cli.py fn/<Id>` per unit as results
   arrive (20-60 s each).
3. One `cli.py casc` at the end of the batch, detached to a log. Never
   `cli.py all` for progress: stage 5 rebuilds the crate per mutant, so a full
   run is about an hour.
4. `cargo xtask verify --fast` per batch; the full one once, detached, at
   handoff.
5. An entry blocked by an IAT import (allocator or the client mutex
   `0x142694c20`/`0x142694c50`) is written `open` with its address and one line
   of why, and the batch moves on.

## The candidate list (33 entries)

From `python dev/crate_coverage.py ow-casc --top 100`, the `entry` rows not
yet verified. Addresses are **exe-frame** (image base `0x140000000`).

| addr | name | impl file | note |
|---|---|---|---|
| 0x14269ab80 | HexPairDecode | install | **DONE** (`fn/HexPairDecode`) |
| 0x1424aec30 | BuildInfoSelectRow | install | **DONE** (`fn/BuildInfoSelectRow`; the real extent runs to `0x1424aed7b`+) |
| 0x14248f2a0 | BuildInfoLoadFromInstall | install | path building; likely FS/vtable-dependent; verify |
| 0x1424c1030 | — | install | chained fragment (0x890 stack frame); verify extent |
| 0x142498a70 | EncodingEspecStringIndex | encoding | **DONE** (`fn/EncodingEspecStringIndex`; the vector grow is the declared gap) |
| 0x1424d1c60 | EncodingSectionsRead | encoding | read |
| 0x1424d1b40 | EncodingHeaderRead | encoding | 281 B; read |
| 0x1425208d0 | DynamicIndexNormalizeIntervals | index | **DONE** (`fn/DynamicIndexNormalizeIntervals`) |
| 0x142520700 | DynamicIndexAppendInterval | index | **DONE** (`fn/DynamicIndexAppendInterval`; the capacity grow is the declared gap) |
| 0x142520590 | DynamicIndexEntrySink | index | **DONE** (`fn/DynamicIndexEntrySink`) |
| 0x1425209a0 | DynamicIndexDecodeEntries | index | **DONE** (`fn/DynamicIndexDecodeEntries`) |
| 0x14250fcf0 | DynamicIndexSegmentParse | index | 1240 B; read |
| 0x1425111a0 | DynamicIndexFindEntry | index | **DONE** (`fn/DynamicIndexFindEntry`) |
| 0x142501b40 | DynamicKeyStateLookup | index | 0x13e; read |
| 0x14250e400 | DynamicIndexSegmentOpen | index | 0x5c; read |
| 0x14250c8f0 | DynamicContainerOpenArchive | cdn | 0x1e4; read |
| 0x1424f9bf0 | PageSlotPublish | pagecache | 0xe; tiny |
| 0x1424a6df0 | — | pagecache | 0xc; trampoline — read the whole body (`0x1424a6df0`→`lea r8,[rcx+0x58]`+…) |
| 0x1424a7d70 | — | pagecache | 0x33; read |
| 0x1424d6e50 | BlteBlockReadLz4 | blte | calls external LZ4 (`0x142efecf0`-style); likely open |
| 0x1424a90e0 | BlteFrameHeaderParse | blte | **wall 1** |
| 0x1424cebd0 | ConfigKeyEntryParse | install | **wall 2** |
| 0x1424da590 | CdnIndexPageResident | container | **open** (HTTP) |
| 0x1424fab50 | PageCacheInvalidateAll | pagecache | **wall 3** |
| 0x1424fabd0 | PageCacheInvalidateRange | pagecache | **wall 3** |
| 0x1424f9eb0 | PageCacheClose | pagecache | **wall 3** |
| 0x1424f8790 | PageCacheInit | pagecache | **wall 3** |
| 0x1424fa440 | PageSlotFind | pagecache | **wall 4** |
| 0x1425115d0 | DynamicIndexResolveRange | index | **wall 5** |
| 0x1424faa20 | PageReadPlan | pagecache | **open** (container lock) |
| 0x1424fb1c0 | PageCacheRead | pagecache | read |
| 0x1424fb1c0 | (same) | | |
| 0x1424fa540 | PageLoad | pagecache | read |
| 0x1424fbf00 | ContainerReaderInit | pagecache | read |
| 0x1424fcd20 | DynamicContainerReadPage | pagecache/cdn | read |
| 0x1424f48d0 | DynamicContainerRead | cdn | read |
| 0x1424ccc00 | BuildConfigParse | install | 6784 B, 256 call sites — the big one; last |

The ordering hint from the queue tool (proved callees first, then smallest
body): `CdnIndexPageResident`, `BlteFrameHeaderParse`,
`ConfigKeyEntryParse`, `DynamicIndexSegmentParse`, `EncodingHeaderRead`,
`DynamicIndexAppendInterval`, `DynamicIndexDecodeEntries`, `BuildConfigParse`,
then the small leaves.

## Method (from `docs/port-playbook.md`)

For each function:
1. **Read** it: `python dev/fn_extent.py <addr>` (`.pdata` extent + chains),
   `python dev/rd.py <start> <end>` (capstone disassembly), and the merged
   Ghidra decompile via `dev/ghidra_run`. Follow the machine, not the C.
   **Chained fragments matter**: several `.pdata` extents are tiny because the
   real body lives further down (e.g. `0x1424aec30` runs to `0x1424aed7b`+).
2. **Transcribe** into the crate file named above, operation for operation:
   same widths, wrapping arithmetic, byte views for structures, floats as raw
   bits. If a callee is outside the system (allocator, CRT, HTTP, mutex),
   declare it an open stage with the address — never a plausible default.
3. **Wire the probe**: one match arm per entry in
   `crates/ow-casc/src/bin/verify_probe.rs`, and register in
   `dev/verify_harness/candidate_rust.py` (`ENTRY_SOURCES` = files the ledger
   binds, `ENTRY_MUTATION` = the symbol mutants edit).
4. **Fixture** `dev/verify_harness/<name>_fixture.py` with `make_args(i)`:
   declared cases first (`i < len(SPECIALS)`) sitting exactly on branches and
   constants, then deterministic generation. Pointers are arena offsets;
   lengths/counts must stay tied to buffers (the client has no bounds checks).
5. **Unit** `dev/verify_harness/units/fn-<Name>.json`. Addresses are **v2b**
   (the v2b frame base plus the RVA; the delta is stated at the top of
   `dev/verify_harness/runner.py`); the exception — a function dereferencing an
   absolute pointer baked into image data — uses
   `{"address": {"exe": "0x14…"}, "base": "0x140000000"}` (see
   `fn/ParseBoolString`). `coverageWindow` = the function's own byte extent.
   `allowedImageReads` lists every image address the code reads.
6. **Run** `python dev/verify_harness/cli.py fn/<Name>`.

### IMPORTANT: no two agents in the same checkout at once

Sub-agents share the working tree and the harness binds verdicts to the
working-tree contents, so **never run more than one in parallel**. Do them
one at a time.

- The sub-agent writes: crate source, probe arm, `candidate_rust.py`,
  fixture, unit JSON.
- It does **not** run `cli.py` for other units; it may run its own unit
  (and `cargo build --release --bin verify_probe -p ow-casc`).
- The main session (me) runs `cli.py casc`, then `cargo xtask verify` and
  the docs, and re-dispatches a fixer if a unit fails.
- Updates: `docs/formats/casc.md`,
  `docs/casc-1to1.md` (the verified table), `docs/systems-coverage.md`,
  `docs/open-questions.md`, `STATUS-handoff.md`, `MODLOG.md`;
  `python dev/make_index.py` after adding tools; `python dev/crate_coverage.py
  ow-casc` to see the count move.

### Per-unit loop (main session)
1. Pick next from the table (proved callees first).
2. `acp_delegate` a `worker` with a self-contained task: the address, the
   playbook path, the crate file, the five required files, and the rule
   "if it cannot be a five-stage unit, say which address blocks it and put
   the read in the docs instead".
   **Watch for**: the agent's result file arriving as a completion
   notification — fetch with `acp_delegate_wait(runId)`; a failed run keeps
   its output file and can be resumed with `resumeFrom`.
3. When it returns, run `python dev/verify_harness/cli.py fn/<Name>`
   (my own run, ~2–5 min with a warm probe).
4. If it fails: `acp_delegate` a `reviewer`/`worker` with the exact failure
   text (stage, divergence, branch site, survivor mutant, and the playbook
   §8 symptom table), fix, re-run.
5. If it passes: docs + count + next.

### Standing rules
- Never commit (AGENTS.md hard rule 4). Never stage beyond the task.
- No new `FIT-*` markers (`cargo xtask verify` fails on them).
- `.gitignore` is a whitelist.
- Docs record addresses + conclusions, never disassembly/decompiled code.
- "not tested" if not tested. If two attempts fail, write STATUS-handoff.md.

### Hard rules from AGENTS.md (project)
1 to 1 or not at all: a feature is implemented only if the client's code was
read from the complete `Overwatch.exe` Ghidra image, the implementation
follows it operation for operation, and a test proves identity. Behaviour
source is the current image only; data source is the operator's installation.

# MODLOG
Milestones only. Findings and addresses live in `docs/formats/`; current state
in `STATUS-handoff.md`; the work queue in `docs/open-questions.md`.

Rotation: this file holds the current entries. Older ones move verbatim to
`docs/history/modlog-YYYY-MM.md` (one file per month) at each rotation; no
entry is rewritten after the fact.

Rotated 2026-11-12: everything from the `2026-11-19 - gate: the unscoped
verify is suspended` entry down is now in `docs/history/modlog-2026-11.md`
(44 entries, moved verbatim). This file keeps the entries written after them.

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


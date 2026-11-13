# Port playbook: implementing a client function, one proved unit at a time

How to move a client function from "read" to done, and how to do that for the
whole client without inventing anything. "Done" has three meanings, one per
proof tier ([`tiers.md`](tiers.md)): what the player feels is proved by the
emulator, a format or a table is proved against the operator's own installation,
the structure is proved by a recorded run. This page is the procedure for all
three; `docs/port-plan.md` holds the scope and the file layout,
`docs/open-questions.md` holds the queue, `docs/systems-coverage.md` holds the
per-system state.

The rule it serves, from `AGENTS.md`: **1 to 1 or not at all.** A feature is
implemented when the client's own code was read from the image, the Rust follows
it operation for operation, and a test proves the result identical. Everything
below exists to make that checkable rather than believable. The tier decides
*which* test proves it; it never decides whether there is one.

## Which procedure applies

| tier | procedure | done state |
|---|---|---|
| **A - feel** movement, camera, collision, weapons, abilities | "Tier A: the emulator ladder" below - screen at §1.5, then §4 to §8 | `verified` |
| **B - translation** formats, registries, tables, asset dispatch | "Tier B: exact over the install" below | `proved` |
| **C - structure** the frame loop, the object model, anything behind a virtual call | "Tier C: first divergence of a recorded run" below | `linked` |

Every procedure starts with the same two steps - §2 (read the client's code) and
§3 (transcribe it) - and the tier decides what proves the result afterwards.
Which tier a system is in is a measurement, not a preference: `python
dev/queue.py --assign-tiers` prints the system-to-tier table, and `docs/tiers.md`
gives the rule behind it.

The loop is `python dev/verify_changed.py`, which re-proves only what a change
can affect (~30 s). The full ladder, `python dev/verify_harness/cli.py all`, is
the **nightly** run (an hour), not the per-change gate.

---

## 0. The shape of the work

The measured scope, its counts and the five function states are in
[`port-plan.md`](port-plan.md) (printed by `python dev/corpus.py triage`, never
typed here). This playbook is the procedure that consumes them.

One unit = one client function address, one Rust function, one descriptor, one
verdict. The verdict is bound by SHA-256 to the oracle bytes, the candidate
source and the harness version (`dev/verify_harness/ledger.py`), so editing any
of them turns the entry `stale` instead of leaving a green lie behind.

A **unit** in that strict sense is the tier-A artefact. Tier B produces a
`*_real.rs` test instead, and tier C a recorded run; neither has an emulator
descriptor, and neither is unfinished for that reason - each is done at its own
tier's state (the table above).

The section numbers in this page (§2, §6.5, ...) are stable labels, not a
reading order: `dev/cluster.py`, `dev/crate_coverage.py` and the format pages
cite them by number.

---

## 1. Choose the next function

```sh
python dev/queue.py                 # progress per tier, counted per function
python dev/queue.py --next 10       # the next ten to port, screened
python dev/queue.py --assign-tiers  # the system -> tier table, with counts
```

`queue.py` counts a function as done only when a *green* proof cites it: a
`verified` ledger entry (tier A), a passing `*_real.rs` test recorded in
`dev/verify_harness/proofs.json` (tier B), or a linked recorded run (tier C).
`--next` orders what no proof cites yet by **tier A before B before C**, then by
**smallest body first**, and screens each candidate through the same callee check
as `dev/cluster.py`, so a body that cannot run yet prints `blocked` with the
reason instead of being offered. **Leaves come first**: a body whose callees all
resolve is offered, a head that reaches a virtual call or an import is not.

Order comes from the queue, not from addresses. A system is begun when the path
that needs it is being read; inside a system, take the cluster head first.

```sh
python dev/corpus.py triage      # the scope: 3,875 functions, counts by state and by system
sqlite3 dev/data/corpus/corpus.db "select printf('0x%x',begin),name,state,size,systems from triage where state='unit' and systems like '%mover%' order by size limit 20"
python dev/corpus.py grep "0x140cb" --tag mover-core   # everything the corpus has for one system
```

`sqlite3` is the CLI, not always installed (it is present here through msys64).
The same query without it:

```sh
python -c "import sqlite3;c=sqlite3.connect('file:dev/data/corpus/corpus.db?mode=ro',uri=True);[print(f'0x{r[0]:x}',*r[1:]) for r in c.execute(\"select begin,name,state,size,systems from triage where state='unit' and systems like '%mover%' order by size limit 20\")]"
```

The preference list below, as one query - smallest call-free bodies first:

```sh
python -c "import sqlite3;c=sqlite3.connect('file:dev/data/corpus/corpus.db?mode=ro',uri=True);[print(f'0x{r[0]:x}',r[1],r[2],r[3]) for r in c.execute(\"select t.begin,t.name,t.size,t.systems from triage t where t.state='unit' and not exists(select 1 from edges e where e.src=t.begin) and not exists(select 1 from slot_sites s where s.src=t.begin) order by t.size\")]"
```

Measured 2026-11-20: **one** function in the whole 3,875-function scope satisfies
"unit, no direct callee, no dispatch". Everything else calls something or
dispatches through a slot, so preference 2 below is almost never reachable and
the queue in practice is preference 1 (proved neighbours).

Prefer, in this order:

1. a function whose **callers and callees are already verified** (the cluster grows
   from a proved core);
2. `unit` state with zero callees and few image reads (the ladder is cheapest);
3. small `entry` heads that unlock a cluster;
4. `inline` bodies only together with the function that owns them.

For the **data crates** (`ow-casc`, `ow-tank`) the corpus triage does not
apply: its scope is the forward closure of the *gameplay* tags, and `casc`/`stu`
are deliberately outside it. Their scope is what the crate itself cites —
`python dev/crate_coverage.py ow-casc` resolves every image address in
`crates/ow-casc/**/*.rs` against the function register and the units, and
`--queue` prints the unverified entries in the order below (most proved
callees, then smallest body). Every address a crate cites is either a function
with a unit, a function without one, an interior of a registered function, or
data — the report says which, so "which functions is this crate about" is a
measurement, not a recollection.

Do not start a unit whose inputs you cannot construct. A unit needs a fixture
that can build every input the client's code branches on.

**What a unit buys, measured** (`docs/port-plan.md` S4): 15 s of harness, and
1-8 client functions executed - the static callee closure is an upper bound that
one controlled call does not walk (`fn/ApplyMoverSettings`: 491 reachable, 2
executed). So the cost of a unit is its read, and the choice that matters is
which read covers the most:

- an entry that **dispatches through slots** is worth more than a leaf: only 510
  of 3,875 scope functions dispatch at all, and the ceiling in the whole scope is
  12 slots (`MoverUpdateFrame` has 8);
- a function with one caller (`inline`, 2,313 of the scope) needs no unit of its
  own - the caller's unit covers it, so port the caller;
- check the queue against the scope before spending a session:
  `python dev/throughput.py --system <name>` and
  `python dev/reach.py fn/<Name> --cases 6` measure both sides.

### 1.5 Screen the callees before you read the body

The queue orders by "proved callees, then size", but the property that decides
whether a unit is cheap is whether the **oracle can execute** what the body
calls. A tier-B function needs none of this screen: the install's data is its
oracle, and the code a format parser runs - the CRT, the allocator - is exactly
the code the emulator cannot enter. The emulator maps the image only: a call into a real function start runs
(proved or not - the ladder compares the top function, and its callees execute),
while these three shapes fault on the first instruction:

| shape in the `.asm` | why it cannot run | example |
|---|---|---|
| `call qword ptr [rip + …]` | an import thunk: the offline IAT slot holds an unrelocated RVA (e.g. `0x03843c96` at `0x142f2d5f8`), not a mapped address, so the call faults | the page-cache locks `0x142694c20` / `0x142694c50` |
| `call qword ptr [reg + off]`, `call reg` | a virtual slot: the object and its table have to be modelled first | `PageReadPlan`'s release at `[rcx+8]` / `call r8` |
| `syscall`, `int 0x2b` | kernel transition | - |
| `stmxcsr`, `ldmxcsr`, `rdtsc`, `cpuid`, `xgetbv`, `rdfsbase` | reads ambient CPU state, which is no argument's property: the emulator's value and the host's are different facts and no mutant can change one | `0x142701520` (`stmxcsr` then `mov eax, [rsp]`, 16 B, returns MXCSR) |

```sh
python dev/corpus.py show <addr> --asm | grep -E "call|syscall"
```

One grep before the deep read separates a 199-byte leaf that will verify in 45
seconds from a 401-byte queue head that cannot verify at all yet. The queue
order is a heuristic; a leaf with zero callees and zero image reads beats a
smaller-looking entry with fifteen.

**Check the address is in the corpus first.** `corpus.py show` answers
`not in corpus: … (build the range first)` for a function outside a built range;
build the disassembly only (no Ghidra bridge, seconds):

```sh
python dev/corpus.py build <start> <end> --asm-only --tag <name>
```

`<end>` is the start plus the byte size `crate_coverage.py --queue` prints; a
function with a `.pdata` entry gets its exact extent in the corpus header - and
`python dev/read.py <addr>` prints it in its identity section, which is the
faster path when the pack is being generated anyway.

### 1.6 Find the function first

The queue answers "what have we already cited". It does not answer "which
functions does the client have for this". `dev/fn.py` does, in one command,
from the image alone:

```sh
python dev/fn.py find MRNM            # the chunk tag -> the dispatcher functions
python dev/fn.py find type:0x0b3      # the asset type -> the client's loader job
python dev/fn.py find name:mover      # the names a human has given
python dev/fn.py scope 0x141b5df70    # every function a format's dispatcher reaches
```

`scope` is the answer to "all the functions of this format": it walks the
whole-image call graph from the dispatcher and lists the cluster, each with
its extent and the unit that proves it. `find <TAG>` works because the client
compares a chunk tag as a **big-endian** u32 (`MRNM` is the immediate
`0x4d524e4d`); see [`formats/asset-types.md`](formats/asset-types.md).

---

## 2. Read it

Reading is the expensive part and the only part that cannot be skipped. Measured
(`docs/port-plan.md` S4): a full unit verification is ~15 s of machine time, so
the harness is ~0.5% of a working day and everything else is this step. The
procedure below is ordered by what actually costs the hours: joining four tools
by hand, guessing which argument owns a member, and trusting a global whose
bytes are not there.

### 2a. The pack: one command before you look at anything

```sh
python dev/read.py 0x142cd4bc0                    # writes dev/reads/<addr>_<name>.md
python dev/gh_decompile.py 0x142cd4bc0            # only if the pack says "not cached"
```

The pack is the evidence a session needs, already joined:

| section | what it saves you |
|---|---|
| identity | extent, chain pieces, `opaque` (linear listing desyncs), the unit and its verdict, the docs lines that already cite it, where the client's C is |
| field map | every memory access **folded to its argument**: `arg0+0x024  read 4B @0x142cd4c4f`, with `write` / `read` / `addr`, and `+idx` when the base is indexed. This is the member layout of every record the function touches, derived from the body, not from the C |
| globals | each riprel target classified **`raw` / `zero-fill` / `unmapped`** from the image's own section table, with read/write counts and a `u32`/`f32`/ascii preview. A `zero-fill` line is the BSS trap, flagged loudly |
| switch tables / immediates | the tables inside the body with their entry counts, and the constants that act as bounds or kind tags |
| argument setup | the instructions before each call site, which is where the real argument *types* come from |
| evidence skeleton | the six lines to fill in `docs/formats/<system>.md` |

Two checks that the map is trustworthy, both reproducible:

- `MoveCharacterController` (`0x142cd4bc0`) maps to `arg0+0x000/+0x010/+0x024`,
  `arg1+0x190/+0x1b0/+0x2b8`, `arg2+0x000/+0x010`; the client's C for the same
  body reads `param_1[0..3]`, `param_1[4..7]`, `param_1[9]`, `*(param_2+0x2b8)`,
  `*(param_2+400)`, `param_3[0..7]` - the same layout, reached two different
  ways.
- the known BSS trap `0x143c3c620` (the "field table" that produced one wrong
  conclusion) appears flagged `zero-fill` in the pack of `0x140f84590`, along
  with 14 more globals in that body - the check that used to be a manual
  `pe_layout.py` call per address.

The field map is a **conservative** dataflow: it follows `mov`/`lea`/`add` and
stops at a call (volatile registers) and at an unconditional `jmp`. An entry it
cannot resolve is reported under the register's own name (`rbx+0x088`) rather
than guessed. When the C and the map disagree, the map is wrong or the extent
is wrong - `opaque: True` in the identity section is the usual cause - and that
is worth one line in the notes, not a guess.

### 2b. Read the C against the map, not alone

1. **Every `param_N[k]` must appear in the map.** `param_2[38]` is `arg1+0x98`;
   if no row has that offset the dataflow hit a call or a jump and dropped the
   origin, or the body is clipped - re-check the extent (2c).
2. **A `zero-fill` global is never a constant.** Its bytes are the loader's
   zeros; the value arrives from a writer at runtime. Read the writer, not the
   table. The pack already says whether *this* body writes each global, so a
   `0 write` line means look elsewhere (`dev/disp_scan.py`, `dev/callers.py`).
3. **`addr` means the pointer is handed on.** `lea rcx, [rsi+0x1b0]` is the
   start of a sub-object passed to a callee; follow the callee's map next.
4. **`+idx` means base + i*size.** The member offset is real; the element stride
   is what you must find (`0xc0` in `MoveCharacterController`'s shape loop).
5. **A byte preview can look like data and be a mask.** The 16 bytes at
   `0x143083290` preview as `inf, inf, inf, inf`; the client uses them as the
   `andps` mask *and* the `cmpeqps` constant, i.e. the test is
   `(v & 0x7f800000) == 0x7f800000`, not a comparison against infinity. The
   difference matters: the mask catches every NaN, a compare to `inf` would not.
   Read the instruction that consumes the address before believing the preview.
6. **Keep the client's arithmetic association.** The decompiler prints an
   expression tree, not an evaluation order: `1.0 / SQRT(a*a + b*b + c*c + d*d)`
   grouped left-to-right is not the same f32 result as `(a*a+b*b)+(c*c+d*d)` -
   1-2 ULP, which a single-call differential does not see and a chained one does
   (`fn/ApplyCharacterControllerMove-steps` is the unit that pins this order).
   If the pack's field sizes say `4B` but the C looks like `double`, the
   disassembly decides.

### 2c. When the decompile stops early, say why before trusting it

The corpus holds a disassembly for the whole game and, where built, a
decompile. A read's decompilation comes from the **merged** project
(`C:/Temp/owread`), which gives every function its full contiguous code; the
evidence project (`dev/ghidra_run/owmod_clean`) clips each body to its own
`.pdata` extent.

```sh
python dev/corpus.py show 0x140cb74a0 --asm     # the disassembly comes first
python dev/corpus.py show 0x140cb74a0           # then the decompile, if cached
sqlite3 dev/data/corpus/corpus.db "select c_cut,file,asm_file from functions where begin=13303072"
```

`c_cut = 1` means the stored decompile was produced before the body merge and
ends at the seam: read it, but re-decompile anything it truncates. A short
decompile is either a `noreturn` pruning (`ClearFalseNoReturn.java`) or a body
seam - never a reason to guess:

```powershell
analyzeHeadless.bat C:\Temp\owread owmod_clean -process Overwatch.exe -noanalysis -readOnly `
  -scriptPath C:\Users\Happy\ghidra_scripts `
  -postScript McpDecompile.java C:\Temp\owaudit\args.txt C:\Temp\owaudit\out.c
```

The args file is whitespace-separated `start end` pairs (`0x140cb74a0 0x140cb760d`).
`-process` is mandatory; without it the script runs with no program.

The same command with a different post-script answers *why* a body stopped
early - keep both, they are the only measure of a decompile's completeness:

```powershell
# what body did the decompiler get, and how long is the text?
  -postScript DecompScope.java 0x1407ffc50
# which instructions leave the body, and where do they go?
  -postScript ProbeFlow.java 0x1407ffc50
``` The merged
project is the reading source: 25,588 of 25,638 chain roots own their full
contiguous code with a measured seam; 41 have no seam (0 named) and 9 are blocked
by objects with no `.pdata` extent (`docs/ghidra-image-audit.md` 5.2, 8.2).

### 2d. Record what you learned

Address + conclusion go into `docs/formats/<system>.md` immediately: "function
at `0x...` does X because of the `comiss`/`jae` at `0x...`". Never disassembly or
decompiled text in the repo. The pack's skeleton is the shape; the evidence
class is `docs/provenance.md`.

---

## 3. Transcribe it

Rules that keep the Rust 1:1 instead of approximate:

- **Follow the instruction, not the C.** The decompiler renders ideas; the
  machine is the contract. Read the `.asm` for every branch and every constant.
- **Same widths.** `u8`/`u16`/`u32`/`u64` and wrapping arithmetic on every path;
  the client never widens silently and neither may the Rust.
- **Byte views for structures.** A unit takes `&[u8]` (or `&mut [u8]`) and reads
  fields with explicit offsets, exactly as the client does. Typed structs
  (`docs/struct-layouts.csv`) come later as a wrapper, not as the test surface.
- **Floats are bit patterns.** Compare raw bits. `-0.0`, NaN payloads and
  denormals are observably different.
- **SSE semantics, not IEEE intuition.** `minss`/`maxss` return the *second*
  operand on NaN; `comiss`+`jae` falls through on unordered, which `x < y` does
  not model. Write the guard as the machine tests it, with an `#[allow]` and a
  comment naming the instruction.
- **A NaN's sign is not transcribable in scalar Rust, and is not comparable
  here.** `addps`/`subps` return the **destination's** NaN when both operands are
  NaN; scalar Rust leaves that choice to the backend, and LLVM may rewrite
  `a - b` as `a + (-b)` (`fadd` is commutative, so it may swap). Measured
  2026-11-20 on `fn/SegmentAabbTransform`: the emulator returns a *sign-cleared*
  NaN whichever operand carries it (`0x7fc00001` for both sign orders), so the
  harness cannot compare that case at all. Keep NaN operands out of the corpus,
  say so in `acknowledgedGaps`, and cover `+-inf` instead - those do agree.
- **Prefer the scalar form when the lanes are independent.** A 4-wide kernel is
  bit-identical as four scalar lanes in the same order with the same grouping,
  and a scalar form gives the mutation stage real sites: the same function
  transcribed with `_mm_*` intrinsics found **2** mutants (both the `0.5`
  literal), the scalar form found **10** and killed all 10. Reach for intrinsics
  only when an instruction's own semantics are the thing being transcribed
  (`shufps` lane selection, `movmskps`).
- **Integer shifts mask themselves.** `u32 >> n` masks `n` to 5 bits, exactly as
  `shrl` does — so a widened `& 0x1f` may be unobservable. That is an
  equivalent mutant, declared with a rationale, not a fixture to pad.
- **Write comparisons so a flip is observable.** `if a != b { return if a < b
  {…} }` makes the `<` → `<=` mutant *observationally equivalent* inside the
  guard, so it can never be killed and must be declared. Two independent tests
  (`if a < b`, `if b < a`) return the same three results and both flips are
  killable. Same category as the shift mask above.
- **Do not build on a stand-in.** If a stage is a declared non-1:1 stand-in, the
  Rust feature is open; replace the stand-in first (`docs/deviations.md`).

Where the code is not readable yet (an indirect call through an object whose
type is unknown), the Rust has a **named open stage**: a function that returns an
error or panics with the address it needed, plus a comment. Never a plausible
default.

---

## Tier A: the emulator ladder

The question this tier answers is *does the player feel it?* - movement, camera,
collision, weapon mechanics, ability state machines. The instrument is the
emulator (`dev/verify_harness`): the client's own bytes and the port execute the
same inputs, and every channel is compared. Done is `verified`, and only a
`verified` unit is done **at this tier** (§7).

The order is: screen the candidate (**§1.5**, before reading anything), read it
(§2), transcribe it (§3), then make the unit - five steps, §4 to §8.

### 4. Wire the probe

The shipped Rust is called by `crates/ow-sim/src/bin/verify_probe.rs` (default) or
a crate-local probe (`crates/ow-tank/src/bin/verify_probe.rs`). One match arm per
entry:

```rust
"mover_physics_command" => mover_physics_command_entry(args, state),
```

```rust
fn mover_physics_command_entry(args: Vec<Arg>) -> Result<(String, Vec<Vec<u8>>), String> {
    let mut out = match args.into_iter().next() {
        Some(Arg::Ptr(b, r, g)) if r.is_empty() && g.is_none() => b,
        _ => return Err("arg0 must be the out buffer".into()),
    };
    ow_sim::movement::build_mover_physics_command(&mut body, &mut out);
    Ok(("-".to_string(), vec![out]))   // "-" = void return; buffers to compare
}
```

- Return **one buffer per `ptr` argument, in order** - the mutated ones and the
  unchanged inputs. A short vec is `candidate fault: missing pointee in
  response` and the differential fails at case 0 (measured).
- Void functions return `"-"`.
- Register the entry in `dev/verify_harness/candidate_rust.py`: `ENTRY_SOURCES`
  (files the ledger binds) and `ENTRY_MUTATION` (the symbol mutants edit).
- `cargo build --release --bin verify_probe -p ow-sim` before running a unit.

---

### 5. Give it an input domain

`argFactory` names a module in `dev/verify_harness/<name>_fixture.py`:

```python
def make_args(i: int) -> List[ArgSpec]:
    ...  # deterministic in i
```

Rules that matter:

- **Declared cases first.** `i < len(SPECIALS)` gets hand-picked inputs that sit
  exactly on the branches and constants; random generation follows. A mutant is
  killed by the case that distinguishes it, so the boundary cases must exist.
- **Pointers are arena offsets.** `ArgSpec(kind="ptr", mem=..., arena_offset=...)`;
  absolute pointers inside the payload are `ARENA_BASE + offset`
  (`from emu_oracle import ARENA_BASE`), and relative pointers are declared with
  `relocs` so both sides rebase them.
- **Lengths and counts stay tied.** The client has no bounds checks; a fixture
  that draws `count` independently of the buffer faults in the emulator and
  proves nothing.
- **Keep mutants inside the buffer, and answering differently.** A mutant that
  panics, overruns or hangs is an *infrastructure* failure, not a kill: the
  probe dies, the runner reports the mutant "not evaluated", and the unit turns
  `inconclusive`. A `while i < count` mutant that becomes `<=` reads one byte
  past `count`, so length the pointee one element past the compared region and
  make that element differ between the buffers: the widened loop reads a real
  byte and returns the wrong answer, which is a kill. A panic is a fixture bug
  even when the mutant is the one that is wrong.
- **State mode.** A unit with a `state` block preloads a snapshot; an argument
  with `guest=<address>` lives inside it, and the whole region is compared after
  the call. Use it when the client reads/writes a surrounding object.
- **Every field the code branches on must be reachable.** If the fixture cannot
  produce a value, the branch is coverage debt, not "not applicable".
- **No NaN operands** (see §3): where two NaNs meet, the emulator clears the
  sign of the one it returns and the instruction does not, so the case is
  uncomparable. `+-inf` is fine. A random tail must mask the exponent to a
  finite value - `(bits & 0x807fffff) | (((x >> 23) & 0xff) % 0xff) << 23` keeps
  sign, mantissa, denormals and range, and drops NaN/inf.

---

### 6. Declare the unit

`dev/verify_harness/units/fn-<Name>.json`:

```json
{
  "id": "fn/BuildMoverPhysicsCommand",
  "title": "one line, what the client function does",
  "address": { "v2b": "<the image address, converted to the v2b frame>" },
  "image": "dev/data/offline_recover/module_offline.bin",
  "entry": "mover_physics_command",
  "candidateLoader": "cargo",
  "candidateCrate": "ow-casc",
  "candidateProbe": "crates/ow-casc/src/bin/verify_probe.rs",
  "suite": "casc",
  "args": [ { "name": "body", "kind": "ptr", "ptr_len": 6800 } ],
  "returns": "void",
  "argFactory": "mover_command_fixture",
  "options": {
    "iterations": 200, "coverageThreshold": 85, "mutationCount": 12,
    "mutationKillRate": 0.75, "seed": 9031, "coverageWindow": 365,
    "allowedImageReads": ["<v2b of every image constant the code reads>"],
    "unreachableBranches": [
      { "site": "<v2b>", "edge": "<v2b>",
        "why": "which edge cannot run, and the instruction that decides it" }
    ]
  },
  "knownEquivalentMutants": [
    { "kind": "boundary +1", "line": 179, "rationale": "why it cannot change a result" }
  ],
  "acknowledgedGaps": ["which paths stay open, and the address that blocks them"],
  "notes": ["the read: constants, order, call site, what the fixture pins"]
}
```

- **Every address in a unit file is in the v2b frame**, not the image frame - the
  two differ by a fixed delta (stated at the top of `dev/verify_harness/runner.py`).
  The exception is a function that dereferences an **absolute pointer baked
  into image data** (a spelling table, a vtable, a string pointer): a v2b copy
  faults on the first dereference because the stored value is the link-time
  address. Such a unit writes `"address": {"exe": "<image address>"}` plus
  `"base": "0x140000000"`, runs the image at its own base, and lists its
  `allowedImageReads` in the exe frame. `fn/ParseBoolString` is the example.
  Copy an address from an existing unit (`units/fn-BuildMoverPhysicsCommand.json`)
  rather than converting by hand; `dev/frame_check.py` rejects v2b addresses in
  prose, which is why this page shows placeholders. `coverageWindow` is the
  function's own byte extent.
- `allowedImageReads` lists every image address the code reads (compiler
  constants included). An entry is one address **or a `start-end` range**, and a
  16-byte SIMD constant needs the range: declaring only `0x...44e0` fails with
  `oracle reads outside declared args/stack at 0x...44e8+8` (measured on
  `fn/SegmentAabbTransform`, whose constants are `0.5` x4 and the `0x7fffffff`
  abs mask). A read outside the list and outside the state block is
  `inconclusive` - undeclared globals are not silently tolerated.
- `allowedImageWrites` lists the image globals the callee may **write**. The
  client keeps some runtime statistics in the image's zero-fill region - the
  character controller's step stage (`0x142cf5020`) updates the global that
  holds the largest round count it has seen - and the oracle's read-only
  image turned such a write into a fault, so a leaf that updates one could
  not be proved at all. The write stays outside the compared channels: the
  statistic does not feed the callee's answer. `fn/CharacterStepStage` is the
  example.
- `unreachableBranches` declares a branch site whose one edge the code cannot
  take, with the reason; the harness requires the site to run.
- `knownEquivalentMutants` is for a mutant no input can kill. The rationale must
  say *why for every input*, not "the fixture misses it". It is **line-keyed**:
  any edit above the mutated statement moves it, and the harness now *says so*
  - a declaration that names no live mutation site fails the unit with
  `inconclusive` and a note naming the stale entries, instead of quietly
  verifying against a weaker mutant set (2026-11-10). Re-state the line and
  say why it moved; do not delete the declaration.
- One line can carry **two mutants of the same kind** (a decimal and a hex
  constant each get a boundary shift), and the kind+line key cannot tell them
  apart. A declaration may add `"mutated": "<the mutated line>"` to name one
  of them exactly; without it the declaration covers every mutant of that kind
  on that line - which can suppress a mutant that *should* be killed.
- `acknowledgedGaps` names what is still open. It is shown in every verdict; it
  is not a pass.
- `scaffold: true` marks a unit that is declared but not yet wired; `cli.py all`
  reports it as SKIP.
- `candidateCrate` / `candidateProbe` choose the crate whose probe is built (the
  default is `ow-sim` / its probe), and `suite` groups the unit: it is in the
  CASC suite when `suite == "casc"` or its crate is `ow-casc`, so an ow-casc
  unit that omits both is silently absent from `cli.py casc`.
- **The smallest complete real unit is `units/fn-Memcmp.json`** (199-byte leaf,
  46 declared cases, 100% of 17 branch sites, 6/6 mutants): copy its shape rather
  than this skeleton's, and read its `notes` as the model for what a unit says.
- `knownEquivalentMutants` is **line-keyed**: any edit above the mutated
  statement silently re-arms the mutant and the unit goes red for a non-port
  reason (`fn/EncodingFindEKey`, 991 -> 1122 -> 1124; the last move was two
  lines added to a doc comment). Re-state the line and say why it moved; do not
  delete the declaration. Since 2026-11-10 the harness fails such a unit with
  a note that names the stale entry, instead of letting the kill rate drift
  down to the gate.

#### 6.5 A steps unit: several calls, one comparison each

A `steps` unit calls one entry several times per corpus case and compares its
channels after **every** call. Use it for a *driver*, not for a leaf: a
single-call unit already compares the whole mutation of one call, so steps only
adds proof where one call's writes feed the next call's control flow
(`docs/port-plan.md` section S3, and the measured note in
`formats/domino.md`).

```json
{
  "id": "fn/<Name>-steps",
  "entry": "<a probe entry that takes GUEST addresses>",
  "args": [ { "name": "pool", "kind": "u64" }, { "name": "entries", "kind": "u64" } ],
  "steps": true,
  "argFactory": "<fixture module>",
  "state": { "base": "0x200000000000", "hex": "<the snapshot, one case for all>" }
}
```

Four rules make it work:

0. **Put the state region outside the emulator's own blocks.** `ARENA_BASE` and
   `STACK_BASE` in `dev/verify_harness/emu_oracle.py` name two guest ranges the
   emulator allocates for itself: a `state` block laid over either is zeroed or
   written by every call. Pick a base clear of both, 16-byte aligned (see
   rule 2: an unaligned base faults `movaps`). And make the snapshot's own regions **not overlap each other** - a
   container at the array's stride lands on the previous pair's elements, the
   client then faithfully processes corrupt inputs, and the symptom is a
   divergence or a fault in the *unported* path, not an obvious fixture bug.
   Assert the layout in the fixture (`assert CONTAINER_AT + PAIRS*STRIDE <= ARRAY_AT`).
1. **Address the data with `u64` guest addresses, not `ptr` arguments.** A `ptr`
   carries its bytes on the wire and the probe re-seeds them on every call, so a
   mutation made by step 0 would be discarded before step 1. A `u64` guest
   address lets the probe slice its own state buffer, which survives the call.
   A `ptr` is still right when it is *input* that differs per step (the velocity
   of `fn/ApplyCharacterControllerMove-steps`), because nothing has to survive
   through it.
2. The fixture exports `make_steps(i) -> list[list[ArgSpec]]`: case `i` is a
   list of steps. The snapshot is **fixed per unit**, so variation across cases
   comes from *which* region the arguments select, not from rebuilding it.
   Every case starts from the snapshot; the oracle gets that by re-seeding.
3. The probe entry takes `&mut State` (see `shape_pool_alloc_guest` in
   `crates/ow-tank/src/bin/verify_probe.rs`, or `scope_pop_guest` in
   `crates/ow-sim/src/bin/verify_probe.rs`, which also shows the guest -> offset
   translation `State::off`): slice `state.buf`, call, write back. **Never clone `state.snapshot` for the working copy** - that is the
   trap that made `fn/ApplyCharacterControllerMove-steps` look divergent for an
   hour: the entry re-seeded from the snapshot on every call, so the candidate
   normalized the original quaternion three times while the client normalized
   its own output. The probe re-seeds `buf` from the snapshot before each call
   on its own and the runner sends `persist on` for a steps unit, so the entry
   must not do it again.
4. Bind the new entry in `dev/verify_harness/candidate_rust.py` twice -
   `ENTRY_SOURCES` and `ENTRY_MUTATION`. Without the binding the mutation stage
   finds no sites and the unit ends `inconclusive` at 0 mutants (this happened).
5. Mutation runs the **whole corpus**, one case at a time on one probe process,
   resetting the state at each case's first step. Do not rely on case 0 to
   discriminate: when the mutation corpus was case 0 only, a payload-NaN case
   and a boundary-handle case in the tail did nothing and
   `fn/ApplyCharacterControllerMove-steps` measured 50% (5/10) instead of 100%.

Then run it like any unit. `fn/ShapePoolAlloc-steps` is the model: 16 chained
pops over 8 pools, 80 cases, 6/6 mutants, one unit.
`fn/ApplyCharacterControllerMove-steps` is the driver shape: the client's move
called three times in a row on one character, 200 cases, 9/9 branches, 10/10.

---

### 7. Run it and read the ladder

```sh
python dev/verify_harness/cli.py fn/BuildMoverPhysicsCommand   # one unit, re-proves it
python dev/verify_harness/cli.py casc                          # the CASC suite
python dev/verify_harness/cli.py all                           # every suite, nightly (an hour)
python dev/verify_harness/cli.py list                          # ids and titles
```

`all` and `casc` replay a `verified` entry whose bound digests still match; a
named unit always re-runs (`--cache` opts it back in, `--force` re-proves a
suite). The knobs `--unit-jobs`, `--jobs`, `--slots` change throughput only.

For a change, do not start here: `python dev/verify_changed.py` reads the diff,
picks the affected units and tier-B proofs, and re-proves those (~30 s). That is
the loop; `cli.py all` is the nightly full run.

| stage | what it proves |
|---|---|
| 1 load | the oracle bytes resolve, the candidate builds and is callable |
| 2 determinism | the oracle answers identically twice; nothing carries over between calls |
| 3 differential | candidate == client on every channel (return bits, pointees, state, fault) over the corpus and the edge pool |
| 4 coverage | both edges of every reachable branch were exercised, or declared dead |
| 5 mutation | injected source mutants are killed, or declared equivalent |

Verdicts: `verified` (all five), `divergent` (a real disagreement), `inconclusive`
(a gate could not be met), `stale` (inputs or harness changed), `SKIP (scaffold)`.
Only `verified` counts as done **at tier A**. A tier-B function is done when its
`*_real.rs` proof passes (`proved`); a tier-C one is done when its recorded run
is linked (`linked`). Neither owes a unit, and neither is half-finished for not
having one ([`tiers.md`](tiers.md)).

**Cost, measured on a 2-core machine with `--unit-jobs 4 --jobs 4 --slots 4`:**
one unit ~20-45 s (stage 5's mutant cargo builds dominate); `cli.py casc`
re-proving all 31 stale units 8 min 33 s; `cargo xtask verify --fast` 2-3 min. A
fresh entry replays in milliseconds, so a suite run costs only what changed.

**The ledger is local.** `dev/verify_harness/ledger.json` is gitignored, so a
fresh checkout has zero verdicts and the first `casc`/`all` re-proves every unit
once. "Verified" is a property of this machine's ledger and the digests it
binds, not of the commit.

---

### 8. Turn a failure into work

| symptom | first move |
|---|---|
| `divergent [ret]` | print both bit patterns; check width/sign/rounding before the algorithm |
| `divergent [mem]` / `[state]` | which byte index diverges; map it to a field offset and read that instruction |
| `[fault] candidate fault: unknown-entry-X` | the probe arm is missing, not the maths |
| `[fault] candidate fault: arg0 must be …` | fixture/probe arg contract (mode, group, guest) |
| `[fault] candidate fault: missing pointee in response` | the probe returned fewer buffers than there are `ptr` args (§4) |
| `INCONCLUSIVE: oracle reads outside declared args/stack at <const>+8` | declare the constant as a `start-end` range (§6) |
| `divergent` only on a NaN input, differing in one sign bit | the emulator clears a NaN's sign where the instruction does not (§3): drop NaN operands, declare it |
| `INCONCLUSIVE: coverage x% < gate` | add declared cases for the missed branch addresses printed |
| `SURVIVOR: …` | either a fixture gap (add the discriminating case) or an equivalent mutant (declare with a proof) |
| `candidate compile failed` with no text | the harness now prints the command and exit code: run it by hand |
| harness self-test red at `candidate compile failed`, no compiler output | the local `gcc` was broken (`cc1.exe` exited 1 silently) until 2026-11-10; it compiles again, so a tier-1 C unit that still stops at the compile is *your* change, not the toolchain |
| `mutant … not evaluated` / unit `inconclusive` after a mutant | the mutant panicked or hung: fix the fixture so a broken candidate answers differently instead of faulting (§5) |
| `make_index.py (python): FAILED`, then `python3: Python was not found` | the second line is the gate's interpreter fallback, not the cause: the generated tables in `docs/tools.md` are stale. `python dev/make_index.py` |
| `gh_apply_names.py` takes minutes, or times out | it is one HTTP call per table row. Run `--pending`, which writes only the rows that differ and waits for `--min 20` of them; `--addr 0x…` for one row; `--check` only reads |
| `stale` | expected after editing the candidate source or the harness; re-run |

A divergence is never resolved by loosening a gate. `docs/port-plan.md` records
what each option may not be used for.

---

## Tier B: exact over the install

The question this tier answers is *does the same bytes produce the same result?*
- format decoding, registry lookup, table construction, asset dispatch. The
instrument is **the operator's own installation**: the port's rule is applied to
the real assets and the outputs are compared. Done is `proved`.

Such a function has no unit and needs none (§7): the install is its oracle.

1. **Read the client's rule** (§2). For a format that is the chunk reader and
   its dispatch; for a table it is the writer of the table plus the reader that
   consumes it; for asset dispatch it is the loader job that `python dev/fn.py
   find type:0x0b3` names. Address and conclusion go into
   `docs/formats/<system>.md` as always (§2d).
2. **Implement it** in the crate that owns the format or registry, under §3's
   rules: a byte view with explicit offsets, the client's widths, floats as bit
   patterns.
3. **Write `crates/<crate>/tests/<name>_real.rs`.** It opens the install, walks
   **every** asset of that kind the registry lists, applies the port, and asserts
   the client's invariants on each one, failing on the first mismatch with the
   asset's GUID and the field that disagreed. `crates/ow-tank/tests/bank_real.rs`
   is the model; it opens all 1,045 readable banks, and
   `crates/ow-tank/tests/mapchunk_kinds_real.rs` walks all 9,076 light chunks.
4. **State the count it opened.** Every such test prints what it saw, e.g.
   `registered 1,148   opened 1,045   not downloaded 103`
   (`docs/formats/sound.md`). A count that goes down is a regression, even when
   every remaining asset passes.
5. **Record the status.** `python dev/verify_changed.py --proofs` runs each proof
   and writes `dev/verify_harness/proofs.json`; `python dev/queue.py` counts a
   function as done at tier B only when that file says `passed`. A green run that
   was never recorded does not move the number.

**A sample is not a proof.** A test that opens part of the corpus is a *smoke
test*, and its own doc comment must say so. `crates/ow-tank/tests/lighting_real.rs`
caps itself at the first 400 manifests and prints `{parsed} of {tried} ... in the
registry` for that reason. A tier-B proof is exhaustive over what ships or it is
a step on the way; the file says which it is.

What tier B does not claim is operation equality: a value built by a table and a
value written as a constant prove identical here. That is the honest trade -
format code is a translator, and a translator is judged by its output
([`tiers.md`](tiers.md)).

---

## Tier C: first divergence of a recorded run

The question this tier answers is *does the same event produce the same state?*
- the frame loop, the object model, anything reached through a virtual call
(`[rax+0x30]`), which the emulator cannot enter at all because the object and its
table are runtime state (§9 counts 1,769 clusters blocked that way). Done is
`linked`.

Four steps, and no fixture:

1. **Record a run outside the repo.** The recording is the evidence and it lives
   on the operator's machine, never in git: the events (input, tick, packet,
   dispatch) and the observable state after each one.
2. **Replay it against the port.** Same events, same order. The port does not
   need the whole frame loop to begin: start at the entry the recording first
   crosses and feed it the recorded state.
3. **Stop at the first divergence.** Not the first difference in a summary - the
   first event after which the two states differ, and inside it the first field.
   Everything before that event is proved; the index is the whole result.
4. **Name the cause.** "event N: the port keeps the value it read before the
   call, the client re-reads it after" - an address, an instruction and a field,
   written into `docs/formats/<system>.md`, never a tolerance.

What tier C gives up is exhaustive per-function equality, and it says so: a run
proves the states it visited, not the branches it did not. What it keeps is that
any behavioural difference shows up at a known event with a known cause. Nothing
is `linked` yet - that column in `python dev/queue.py` is zero by measurement,
not by omission.

---

## Shared rules

These hold whichever tier the function is in.

### 9. Clusters: when one function calls another

A cluster is a head plus the functions it needs. Port it **bottom-up**: the
leaves first, each its own unit, then the head with the ported callees in place.

```sh
python dev/cluster.py                       # portable clusters, smallest first
python dev/cluster.py --head 0x142cf7c10    # one cluster's port order, screened
```

Screen before reading; the pick is usually wrong without it. Measured 2026-11-20
over every unported head in the scope (2,814 with at least one callee): **135
clusters are portable (4.8%)**. The narrow window a first pass used - 2..5
callees, 4 KB total - has 25 of 1,144 (2.2%), so the filter matters as much as
the client does.

Where the other 95% die, counted per cluster (a cluster dies if *any* function
in it reaches one of these):

| blocker | clusters | what it is |
|---|---|---|
| a call target with no index entry | 2,276 | the hot-patch guard `mov r10,[rip+g]; test; jmp r10` or a body with no `.pdata` at all (9.9% of `.text`). The guard's slot *is* resolvable - see below - and what it resolves to is a virtual call |
| a virtual call | 1,769 | `[rax+0x8]`, `[rax+0x30]`: the engine's object model, needs the object's runtime type |
| an import thunk | 1,694 | `jmp [IAT]`: the CRT and Win32 |
| an indirect call | 1,135 | `call rax`, a function pointer the code carries |
| `rdtsc` / `cpuid` | 662 / 598 | a clock and the host CPU's identity |

That is the shape of the client, not a defect in the tool: non-trivial work
leaves through the CRT, a vtable, or its own guard layer, and all three resolve
at runtime. For reference, **470 of the 3,875 functions in the scope (12%) are
individually runnable** - a pure leaf is the exception.

A cluster blocked this way is not an unfinished unit: a body behind a virtual
call is tier-C work, proved by a recorded run, and no amount of fixture work
makes it tier A.

**The guard resolves, and it does not help.** `0x142cadee0` reads its target
from `0x1442a0158`, a slot in the `.data` zero-fill tail; the slot's writer is
`0x142cadf34`, its setter `0x142cadf20` has one caller, `0x141ff3190` (registered
as a data pointer at `0x143360ef8`), and that caller installs three `lea`
constants. `cluster.py` now walks that: store site -> the setter region -> its
callers -> the last `lea <stored reg>` before the call. It returns
`0x141bb6660`, `0x141b328e0` and `0x14053a420` - exactly what a manual read gave.

And the portable count does not move, because the first of those is
`jmp 0x141bb00e0`, whose body is `call [rax+0x30]`. **The guard is a wrapper
around the object model.** So the honest reading of the table is that "no index
entry" and "virtual call" are largely the same wall, and the ceiling is the
vtable, not the data.

What the walk did buy is a truthful message: a cluster blocked here now names
the chain (`via 0x142cadee0[slot] -> 0x141bb6660 -> 0x141bb00e0: virtual slot
[rax+0x30]`) instead of calling the address opaque.

Two false positives the screen had to lose before the number meant anything,
both measured:

- **A forwarder is not a stub.** An address with no `.pdata` extent is often
  `mov ecx,edx; mov edx,r8d; jmp <root>` - an argument-shuffle tail call into a
  real function (`0x141c23490` -> `0x141bb00e0`). Follow the `jmp` before
  blocking; 113 of 186 single-stub clusters were this. A *chain* of forwarders
  whose last hop has no root still blocks.
- **`0xCC` between functions is padding.** Decoding past it read `int3` as an
  ambient trap and blocked 589 clusters that never execute one. Stop at `int3`.

Three shapes the screen must not miss, each of which cost a wrong pick:

1. **A direct call into code with no `.pdata` extent.** `0x142cee300` calls
   `0x142cadf00`, a guard-dispatch stub (`mov rax, [rip+...]; test; jmp rax`) with
   no function root, no edge row and no triage state. It is two hops from an
   import, and an in-scope-only walk never sees it. Decode the calls from the
   body and classify the targets, rather than trusting the edge table.
2. **The leaf's own callee** is in the cluster's plan only if it is in the triage
   scope; anything else has to be classified too, one level out at least.
3. **A head can look small and still be a driver**: `0x142cf7c10` (105 B) tails
   into a 4-call leaf and its path is blocked by (1).

When a cluster is blocked on one branch, the §9 pattern is to pin the *data* the
branch tests and declare the other edge: `fn/ScopePop-steps` pins the element's
flag byte to zero (`unreachableBranches` names the site and the fall-through,
with the reason reading "fixture-pinned edge, not dead code") and
`acknowledgedGaps` names the unported stub. That is honest only when the pinned
value is data the client reads, not a control-flow edge you could otherwise
reach.

- If a callee is outside the system (allocator, CRT, middleware), the unit may
  read it from the image through `allowedImageReads`, or the head declares the
  call as an open stage — the choice is written in `acknowledgedGaps`.
- If a callee is an **indirect call through an object** (a vtable slot), the
  object's type must be read first. Until it is, the Rust function returns a
  named error: `fn/MoverGate`'s record predicate (`entry+0x28` → `+0x78`) is the
  current example, and its unit's gap says so.
- The proven example in this repository: `fn/BuildMoverPhysicsCommand` calls the
  gate; the fixture pins the gate's record count to zero, so the readable paths
  are proved and the indirect path stays explicitly open.

---

### 10. A system is done when

The scope-level definition is [`port-plan.md`](port-plan.md) §"Definition of
done, per system": every function in the system's scope has reached its tier's
done state - a `verified` unit, a passing `*_real.rs` proof (`proved`), or a
linked recorded run (`linked`); failing that, it is covered by another
function's proof, or it is declared unverifiable with the reason. **"No harness
unit" is not a gap when the tier is B or C** ([`tiers.md`](tiers.md)). On top of
that, a session checks:

1. the shipped crate path uses the declarations (not a parallel copy);
2. the gate passes: `cargo xtask verify` - while the operator's suspension holds,
   `--fast` or a scoped run, and say which (`AGENTS.md`);
3. `docs/systems-coverage.md` states what is read, what is verified, what is
   open and the next anchor; `docs/open-questions.md` no longer lists the closed
   question; `MODLOG.md` has the entry.

Progress is reported **per tier and per function** - the number `python
dev/queue.py` prints - with the mutant kill rate of the tier-A units alongside
it. It is never "percent of the 119,802 image functions": 96.8% of the image is
CRT, STL, compiler helpers, middleware and rendering.

---

### 11. Tool map

| need | tool |
|---|---|
| **prepare a read** (fields, globals classified, tables, caller setup) | `python dev/read.py <addr>` -> `dev/reads/<addr>_<name>.md` |
| what a function does | `python dev/corpus.py show <addr> [--asm]`, then `McpDecompile.java` from `C:/Temp/owread` |
| how much one unit proves / costs | `python dev/throughput.py`, `python dev/reach.py <unit> --cases 6` |
| put a body into the corpus | `python dev/corpus.py build <start> <end> --asm-only` (no bridge needed) |
| is the body complete? | `DecompScope.java <addr>`, `ProbeFlow.java <addr>` |
| why is a decompile short? | false `noreturn` (`ClearFalseNoReturn.java`) or a body seam (`MergeChainBodies.java`) |
| who calls / what it calls | `CallersOf.java`, `McpCallers.java`, `dev/callers.py` |
| which chain fragments exist | `dev/chain_placement.py`, `ChainConflicts.java` |
| data formats and values | `crates/ow-tank/src/bin/*`, `crates/ow-tank/examples/*` |
| is an address readable | `python dev/pe_layout.py --addr <addr>` |
| names ↔ addresses | `docs/function-names.csv`, `python dev/gh_apply_names.py [--pending] [--addr 0x…] [--check]` |
| which functions a data crate covers | `python dev/crate_coverage.py <crate> [--queue]` |
| the loop: re-prove only what a change can affect (~30 s) | `python dev/verify_changed.py` (`--dry-run`, `--refs HEAD~1`, `--proofs`, `--slow`) |
| progress per tier, and the next function | `python dev/queue.py`, `--next 10`, `--assign-tiers` |
| did the tier-B proofs pass, and when | `dev/verify_harness/proofs.json`, written by `dev/verify_changed.py` |
| the gate | `cargo xtask verify` (`--fast` skips install-reading tests) |
| tool index | `python dev/make_index.py` after adding a script |

---

### 12. Checklists

**New unit**

1. Screen the callees and the ambient state (§1.5): a `call qword ptr [rip+…]`,
   `[reg+off]`, `syscall`, `stmxcsr` or `rdtsc` is a dead end until the missing
   stage is read. `corpus.py build … --asm-only` if `show` says the address is
   not in the corpus; the pack prints the extent.
2. `corpus.py show <addr> --asm`; decompile from `C:/Temp/owread` when a branch
   is not obvious from the instructions.
3. Write the Rust (`crates/<crate>/src/<file>.rs`), doc comment naming the
   address and the instructions that decide each branch.
4. Probe arm + `cargo build --release --bin verify_probe -p <crate>`.
5. `ENTRY_SOURCES` / `ENTRY_MUTATION` rows in `candidate_rust.py`.
6. Fixture: declared cases on every boundary, then random; a mutant must land
   on a real byte, not outside the buffer (§5).
7. Unit JSON: address, entry, args, options (constant ranges as `start-end`),
   equivalent mutants, gaps, notes.
   `candidateCrate` / `candidateProbe` / `suite` route it to the right probe and
   suite (§6).
8. `cli.py fn/<Name>` until `verified`.
9. Name row in `docs/function-names.csv`, then
   `python dev/gh_apply_names.py --addr <addr>` (one row; the whole table is a
   minutes-long walk, and the CSV row is the durable record either way).
10. `docs/formats/<system>.md`; `docs/systems-coverage.md`; `MODLOG.md`.
11. `python dev/make_index.py` if a `dev/**/*.py` file was added.
12. `cargo xtask verify --fast`, then `python dev/verify_harness/cli.py casc`
    (or the crate's suite) to prove no sibling went stale.

**Closing a cluster**

1. Leaves verified first; the head's gaps shrink as callees land.
2. Remove the open-stage errors only when the callee is verified.
3. Re-run the head and every sibling that shares the callee.

**Nightly and weekly hygiene**

1. `cli.py all` - the nightly full ladder (an hour), never the per-change gate.
   It replays fresh entries, so every `stale` is a unit whose inputs changed.
   `--force` is the full re-proof, and it is the expensive one.
2. `python dev/verify_changed.py --proofs --slow` - every tier-B proof against
   the install, statuses recorded in `dev/verify_harness/proofs.json`; that
   record is what `python dev/queue.py` counts.
3. `cargo xtask verify`.
4. `docs/open-questions.md`: the top item is the only one being worked.
5. Rotate `MODLOG.md` past a few hundred lines.

---

### 13. Where the state lives

This page is procedure and carries no counts. It used to end with a status
section, which meant every session edited numbers into it and the next session
read stale ones. State has an owner:

| question | file |
|---|---|
| what is verified right now, and every verdict's digests | `dev/verify_harness/ledger.json` (local, gitignored) - read by `python dev/verify_harness/cli.py list` |
| which tier-B proof last passed, and how long it took | `dev/verify_harness/proofs.json` (local) - written by `python dev/verify_changed.py`, counted by `python dev/queue.py` |
| what each crate and system covers, function by function | `docs/systems-coverage.md` |
| what is open, what is next | `docs/open-questions.md` (the queue) |
| the loop a session runs, and the rules that bind it | `STATUS-handoff.md` |
| the read behind a unit | `docs/formats/<system>.md` and the unit's `notes` |
| why a unit is red for a non-port reason | §6 (line-keyed mutants) and §8 (the environment rows) |


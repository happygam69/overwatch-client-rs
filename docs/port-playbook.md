# Port playbook: implementing the gameplay side, one proved unit at a time

How to move a client function from "read" to "verified", and how to do that for
the whole gameplay surface without inventing anything. This is the procedure;
`docs/port-plan.md` holds the scope and the file layout, `docs/open-questions.md`
holds the queue, `docs/systems-coverage.md` holds the per-system state.

The rule it serves, from `AGENTS.md`: **1 to 1 or not at all.** A feature is
implemented when the client's own code was read from the image, the Rust follows
it operation for operation, and a test proves the result identical. Everything
below exists to make that checkable rather than believable.

---

## 0. The shape of the work

| unit | count | meaning |
|---|---:|---|
| gameplay functions | 3,864 | the whole gameplay surface (3.2% of the image) |
| units | 1,541 | functions bucketed so each is a testable step |
| triage `unit` | 1,303 | self-contained enough to port on its own |
| triage `entry` | 238 | reachable from outside the system: port as a cluster head |
| triage `inline` | 2,312 | only meaningful inside a caller: port with it |
| triage `thunk` / `trivial` | 6 / 5 | wrappers and one-liners: port with the head |
| verified | whatever this machine's ledger says | `python dev/verify_harness/cli.py list` (the ledger is local, §7) |

One unit = one client function address, one Rust function, one descriptor, one
verdict. The verdict is bound by SHA-256 to the oracle bytes, the candidate
source and the harness version (`dev/verify_harness/ledger.py`), so editing any
of them turns the entry `stale` instead of leaving a green lie behind.

---

## 1. Pick the next unit

Order comes from the queue, not from addresses. A system is begun when the path
that needs it is being read; inside a system, take the cluster head first.

```sh
python dev/corpus.py triage      # the scope: 3,864 functions, counts by state and by system
sqlite3 dev/data/corpus/corpus.db "select printf('0x%x',begin),name,state,size,systems from triage where state='unit' and systems like '%mover%' order by size limit 20"
python dev/corpus.py grep "0x140cb" --tag mover-core   # everything the corpus has for one system
```

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

### 1.5 Screen the callees before you read the body

The queue orders by "proved callees, then size", but the property that decides
whether a unit is cheap is whether the **oracle can execute** what the body
calls. The emulator maps the image only: a call into a real function start runs
(proved or not - the ladder compares the top function, and its callees execute),
while these three shapes fault on the first instruction:

| shape in the `.asm` | why it cannot run | example |
|---|---|---|
| `call qword ptr [rip + …]` | an import thunk: the offline IAT slot holds an unrelocated RVA (e.g. `0x03843c96` at `0x142f2d5f8`), not a mapped address, so the call faults | the page-cache locks `0x142694c20` / `0x142694c50` |
| `call qword ptr [reg + off]`, `call reg` | a virtual slot: the object and its table have to be modelled first | `PageReadPlan`'s release at `[rcx+8]` / `call r8` |
| `syscall`, `int 0x2b` | kernel transition | - |

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
function with a `.pdata` entry gets its exact extent in the corpus header.

---

## 2. Read it

Reading is the expensive part and the only part that cannot be skipped.

**a. Get the body.** The corpus holds a disassembly and, where built, a
decompile for every function:

```sh
python dev/corpus.py show 0x140cb74a0 --asm     # the disassembly comes first
python dev/corpus.py show 0x140cb74a0           # then the decompile, if cached
sqlite3 dev/data/corpus/corpus.db "select c_cut,file,asm_file from functions where begin=13303072"
```

`c_cut = 1` means the stored decompile was produced before the body merge and
ends at the seam: read it, but re-decompile anything it truncates.

The corpus stores the disassembly of the whole game and it is written without
the bridge, so it is the fast path: `--asm-only` re-reads any range in seconds
and needs no Ghidra session.

**b. Read from the merged project, not the evidence project.** The evidence
project (`dev/ghidra_run/owmod_clean`) has each function's body clipped to its
own `.pdata` extent. The merged working copy (`C:/Temp/owread`) gives every
function its full contiguous code, with a measured seam
(`docs/ghidra-image-audit.md` 5.2, 8.2):

```powershell
analyzeHeadless.bat C:\Temp\owread owmod_clean -process Overwatch.exe -noanalysis -readOnly `
  -scriptPath C:\Users\Happy\ghidra_scripts `
  -postScript McpDecompile.java C:\Temp\owaudit\args.txt C:\Temp\owaudit\out.c
```

The args file is whitespace-separated `start end` pairs (`0x140cb74a0 0x140cb760d`).
`-process` is mandatory; without it the script runs with no program. The merged
project is the reading source: 25,588 of 25,638 chain roots own their full
contiguous code with a measured seam; 41 have no seam (0 named) and 9 are blocked
by objects with no `.pdata` extent (`docs/ghidra-image-audit.md` 5.2, 8.2).

**c. When the decompile stops early, say why before trusting it.**

```powershell
# what body did the decompiler get, and how long is the text?
-postScript DecompScope.java 0x1407ffc50
# which instructions leave the body, and where do they go?
-postScript ProbeFlow.java 0x1407ffc50
```

A short decompile is either a `noreturn` pruning (`ClearFalseNoReturn.java`) or a
body seam (merge) — never a reason to guess.

**d. Record what you learned.** Address + conclusion go into
`docs/formats/<system>.md` immediately: "function at `0x…` does X because of the
`comiss`/`jae` at `0x…`". Never disassembly or decompiled text in the repo.

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

## 4. Wire the probe

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

- Return the mutated buffers; the harness compares them byte for byte.
- Void functions return `"-"`.
- Register the entry in `dev/verify_harness/candidate_rust.py`: `ENTRY_SOURCES`
  (files the ledger binds) and `ENTRY_MUTATION` (the symbol mutants edit).
- `cargo build --release --bin verify_probe -p ow-sim` before running a unit.

---

## 5. Give it an input domain

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

---

## 6. Declare the unit

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
  constants included). A read outside the list and outside the state block is
  `inconclusive` — undeclared globals are not silently tolerated.
- `unreachableBranches` declares a branch site whose one edge the code cannot
  take, with the reason; the harness requires the site to run.
- `knownEquivalentMutants` is for a mutant no input can kill. The rationale must
  say *why for every input*, not "the fixture misses it".
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
  delete the declaration.

---

## 7. Run it and read the ladder

```sh
python dev/verify_harness/cli.py fn/BuildMoverPhysicsCommand   # one unit, re-proves it
python dev/verify_harness/cli.py casc                          # the CASC suite
python dev/verify_harness/cli.py all                           # every suite
python dev/verify_harness/cli.py list                          # ids and titles
```

`all` and `casc` replay a `verified` entry whose bound digests still match; a
named unit always re-runs (`--cache` opts it back in, `--force` re-proves a
suite). The knobs `--unit-jobs`, `--jobs`, `--slots` change throughput only.

| stage | what it proves |
|---|---|
| 1 load | the oracle bytes resolve, the candidate builds and is callable |
| 2 determinism | the oracle answers identically twice; nothing carries over between calls |
| 3 differential | candidate == client on every channel (return bits, pointees, state, fault) over the corpus and the edge pool |
| 4 coverage | both edges of every reachable branch were exercised, or declared dead |
| 5 mutation | injected source mutants are killed, or declared equivalent |

Verdicts: `verified` (all five), `divergent` (a real disagreement), `inconclusive`
(a gate could not be met), `stale` (inputs or harness changed), `SKIP (scaffold)`.
Only `verified` counts as done.

**Cost, measured on a 2-core machine with `--unit-jobs 4 --jobs 4 --slots 4`:**
one unit ~20-45 s (stage 5's mutant cargo builds dominate); `cli.py casc`
re-proving all 31 stale units 8 min 33 s; `cargo xtask verify --fast` 2-3 min. A
fresh entry replays in milliseconds, so a suite run costs only what changed.

**The ledger is local.** `dev/verify_harness/ledger.json` is gitignored, so a
fresh checkout has zero verdicts and the first `casc`/`all` re-proves every unit
once. "Verified" is a property of this machine's ledger and the digests it
binds, not of the commit.

---

## 8. Turn a failure into work

| symptom | first move |
|---|---|
| `divergent [ret]` | print both bit patterns; check width/sign/rounding before the algorithm |
| `divergent [mem]` / `[state]` | which byte index diverges; map it to a field offset and read that instruction |
| `[fault] candidate fault: unknown-entry-X` | the probe arm is missing, not the maths |
| `[fault] candidate fault: arg0 must be …` | fixture/probe arg contract (mode, group, guest) |
| `INCONCLUSIVE: coverage x% < gate` | add declared cases for the missed branch addresses printed |
| `SURVIVOR: …` | either a fixture gap (add the discriminating case) or an equivalent mutant (declare with a proof) |
| `candidate compile failed` with no text | the harness now prints the command and exit code: run it by hand |
| harness self-test red at `candidate compile failed`, no compiler output | the local `gcc` is broken (`cc1.exe` exits 1 silently), not your change; the tier-1 C units go `inconclusive` for the same reason |
| `mutant … not evaluated` / unit `inconclusive` after a mutant | the mutant panicked or hung: fix the fixture so a broken candidate answers differently instead of faulting (§5) |
| `make_index.py (python): FAILED`, then `python3: Python was not found` | the second line is the gate's interpreter fallback, not the cause: `dev/INDEX.md` is stale. `python dev/make_index.py` |
| `gh_apply_names.py` takes minutes, or times out | it is one HTTP call per table row (~500). Use `--addr 0x…` for the row just added (§12); `--check` now only reads |
| `stale` | expected after editing the candidate source or the harness; re-run |

A divergence is never resolved by loosening a gate. `docs/port-plan.md` records
what each option may not be used for.

---

## 9. Clusters: when one function calls another

A cluster is a head plus the functions it needs. Port it **bottom-up**: the
leaves first, each its own unit, then the head with the ported callees in place.

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

## 10. A system is done when

1. every function on its path has a unit and a fresh `verified` verdict, or is
   listed as open with the address that blocks it;
2. the declarations exist in Rust (`docs/struct-layouts.csv`, `docs/formats/…`);
3. the shipped crate path uses them (not a parallel copy);
4. the gate passes: `cargo xtask verify`;
5. `docs/systems-coverage.md` states what is read, what is verified, what is
   open and the next anchor; `docs/open-questions.md` no longer lists the closed
   questions; `MODLOG.md` has the entry.

Progress is reported as **verified units × functions covered × mutant kill rate**
per system. It is never "percent of the 119,802 image functions": 96.8% of the
image is CRT, STL, compiler helpers, middleware and rendering.

---

## 11. Tool map

| need | tool |
|---|---|
| what a function does | `python dev/corpus.py show <addr> [--asm]`, then `McpDecompile.java` from `C:/Temp/owread` |
| put a body into the corpus | `python dev/corpus.py build <start> <end> --asm-only` (no bridge needed) |
| is the body complete? | `DecompScope.java <addr>`, `ProbeFlow.java <addr>` |
| why is a decompile short? | false `noreturn` (`ClearFalseNoReturn.java`) or a body seam (`MergeChainBodies.java`) |
| who calls / what it calls | `CallersOf.java`, `McpCallers.java`, `dev/callers.py` |
| which chain fragments exist | `dev/chain_placement.py`, `ChainConflicts.java` |
| data formats and values | `crates/ow-tank/src/bin/*`, `crates/ow-tank/examples/*` |
| is an address readable | `python dev/pe_layout.py --addr <addr>` |
| names ↔ addresses | `docs/function-names.csv`, `python dev/gh_apply_names.py [--addr 0x…] [--check]` |
| which functions a data crate covers | `python dev/crate_coverage.py <crate> [--queue]` |
| the gate | `cargo xtask verify` (`--fast` skips install-reading tests) |
| tool index | `python dev/make_index.py` after adding a script |

---

## 12. Checklists

**New unit**

1. Screen the callees (§1.5): a `call qword ptr [rip+…]` or `[reg+off]` is a
   dead end until the missing stage is read. `corpus.py build … --asm-only` if
   `show` says the address is not in the corpus.
2. `corpus.py show <addr> --asm`; decompile from `C:/Temp/owread` when a branch
   is not obvious from the instructions.
3. Write the Rust (`crates/<crate>/src/<file>.rs`), doc comment naming the
   address and the instructions that decide each branch.
4. Probe arm + `cargo build --release --bin verify_probe -p <crate>`.
5. `ENTRY_SOURCES` / `ENTRY_MUTATION` rows in `candidate_rust.py`.
6. Fixture: declared cases on every boundary, then random; a mutant must land
   on a real byte, not outside the buffer (§5).
7. Unit JSON: address, entry, args, options, equivalent mutants, gaps, notes.
   `candidateCrate` / `candidateProbe` / `suite` route it to the right probe and
   suite (§6).
8. `cli.py fn/<Name>` until `verified`.
9. Name row in `docs/function-names.csv`, then
   `python dev/gh_apply_names.py --addr <addr>` (one row; the full table is a
   minutes-long walk, and the CSV row is the durable record either way).
10. `docs/formats/<system>.md`; `docs/systems-coverage.md`; `MODLOG.md`.
11. `python dev/make_index.py` if a `dev/**/*.py` file was added.
12. `cargo xtask verify --fast`, then `python dev/verify_harness/cli.py casc`
    (or the crate's suite) to prove no sibling went stale.

**Closing a cluster**

1. Leaves verified first; the head's gaps shrink as callees land.
2. Remove the open-stage errors only when the callee is verified.
3. Re-run the head and every sibling that shares the callee.

**Weekly hygiene**

1. `cli.py all` (replays fresh entries); every `stale` is a unit whose inputs
   changed. `--force` is the full re-proof, and it is the expensive one.
2. `cargo xtask verify`.
3. `docs/open-questions.md`: the top item is the only one being worked.
4. Rotate `MODLOG.md` past a few hundred lines.

---

## 13. Where the state lives

This page is procedure and carries no counts. It used to end with a status
section, which meant every session edited numbers into it and the next session
read stale ones. State has an owner:

| question | file |
|---|---|
| what is verified right now, and every verdict's digests | `dev/verify_harness/ledger.json` (local, gitignored) - read by `python dev/verify_harness/cli.py list` |
| what each crate and system covers, function by function | `docs/systems-coverage.md` |
| what is open, what is next, which units are red | `STATUS-handoff.md`, `docs/open-questions.md` |
| the read behind a unit | `docs/formats/<system>.md` and the unit's `notes` |
| why a unit is red for a non-port reason | §6 (line-keyed mutants) and §8 (the environment rows) |


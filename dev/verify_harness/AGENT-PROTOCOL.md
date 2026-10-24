# Subagent work-unit protocol

The contract between a decompilation worker (human or agent) and
`dev/verify_harness/`. Nothing here is optional, and no worker is ever the
authority on whether their own decompile is correct: the harness decides,
and it decides from evidence — the shipped bytes, emulated, executed on the
same inputs the candidate gets.

---

## Why the protocol is shaped this way

The failure mode being designed against: a worker decompiles a function,
transcribes it into C or Rust, tests it against the cases they thought of,
sees it work, and declares it done. The code then diverges from the client
on the input nobody thought of. Nothing in that process is dishonest, and
it still produces code nobody has verified.

So the harness refuses these as evidence:

| Not evidence | Why |
|---|---|
| The worker ran it and it looked right | The worker's own inputs are the ones they already thought of |
| The C file resembles the disassembly | Similar code can differ on one branch or one bit test |
| The tests pass | Passing tests describe the tests, not the code |
| "It's a faithful port" | A claim, not an observation |
| Coverage of the candidate | Measures the new code, not whether the original was probed |

What it accepts: **the shipped machine code and the candidate, executed on
identical inputs in identical sandboxes, producing identical observable
results — over inputs whose reach was measured and whose detection power was
demonstrated.**

The candidate is now **shipped code, not a harness copy**. A `"cargo"` unit
builds `crates/ow-sim/src/bin/verify_probe.rs` with `cargo build --release`;
the probe imports the real crate functions. The ledger binds the verdict to
the function's own source file(s) (`candidate_sources`), the probe source and
the git revision (`crate_rev`), so editing any of them expires the entry. The
old `"rust"` loader (`rust_probe.rs`, built with `rustc`) remains for the
selftest's deliberately-broken entry; a new unit must not use it.

Two bug classes this harness has already caught in real decompiles — expect
them in yours:

1. **Conversion where the bytes do a bit operation.** Ghidra prints
   `(uint)float_val & 1` for what the machine does as
   `test byte [reg+off], 1`. Compiling the C takes a different branch on
   every input where the reinterpretation differs from the conversion.
2. **IEEE-ordered `if` chains where the bytes are SSE min/max.** Ghidra
   prints `if (a <= b) ...` for `minss`/`maxss`, whose NaN rule is
   "unordered → return the second source". The sequential ifs propagate
   NaN; the machine falls to the bound. The divergence appears the moment
   the edge pool includes a NaN.

---

## The loop

```
pick an address from dev/decompiles/address_list.txt
        │
        ▼
re-decompile (Ghidra MCP) if the .c is missing or a stub
        │
        ▼
write the unit descriptor        dev/verify_harness/units/fn-<Name>.json
        │
        ▼
write the fixture                argFactory module, or rely on the generic
                                 generator for scalar-only signatures
        │
        ▼
run the harness                  python dev/verify_harness/cli.py fn/<Name>
        │
        ├─► verified ────── ledger records it + digests; done
        │
        ├─► divergent ───── counterexample printed (surface, iteration,
        │                   both values) → FIX THE DECOMPILE, re-run
        │
        └─► inconclusive ── named reason: fixture gap / coverage gap /
                            blind mutation / undeclared state / cold page
                                   │
                                   └─► close that gap, re-run
```

### Step 1 — pick a function

```
type dev\decompiles\address_list.txt
```

Each line: `<v2b address> <exe address> <name>`. Start with zero-callee
functions (run the callee census below). Do not start a function with an
indirect call or an unrecovered jumptable until its callees are themselves
settled — `DispatchFpCameraSlotTime` is the standing example of why.

Callee census for your function:

```bash
python -c "
import re
src = open(r'dev/decompiles/<Name>_<addr>.c').read()
print('callees:', sorted(set(re.findall(r'FUN_[0-9a-fA-F]+', src))))
"
```

A non-empty list means the emulator will follow those callees inside the
mapped image (that is fine — they are shipped code too), but any global they
read must be provided by your fixture or the stage-3 read check will name it.

### Step 2 — re-decompile if needed

The `.c` file must contain the real body, not a comment stub. If the file
says "Decompilation failed" or "not trustworthy", re-decompile through the
Ghidra MCP (project `dev/ghidra_run/owmod`, program per
`docs/ghidra-project.md`) and replace the file. **Never hand-write a body
you have not seen decompiled** — a hand-written body puts you in the trust
path and makes the exercise circular.

### Step 3 — declare the unit

`dev/verify_harness/units/fn-<Name>.json`:

```json
{
  "id": "fn/<Name>",
  "title": "one line: what this code does",
  "address": { "v2b": "0x7ff7c..." },
  "image": "D:\\ow-dumps\\offline_recover\\module_offline.bin",
  "c_file": "dev/decompiles/<Name>_<addr>.c",
  "entry": "FUN_<addr>",
  "args": [
    { "name": "p", "kind": "ptr", "ptr_len": 16 },
    { "name": "t", "kind": "f32" }
  ],
  "returns": "f32",
  "options": {
    "iterations": 300,
    "coverageThreshold": 85,
    "mutationCount": 12,
    "mutationKillRate": 0.8,
    "seed": 4241
  },
  "notes": ["what a reader needs to know about the layout"]
}
```

Field notes:

- **`address.v2b`** is the v2b-frame address; the harness finds the bytes at
  `v2b - 0x7ff7c4d20000` in `module_offline.bin` (file offset == RVA; the
  v2b delta equals the exe RVA). If the load stage reports a cold page,
  the bytes are zero in this capture — say so, do not work around it.
- **`args`** declare the ABI slots: `f32`, `f64`, `u64`, `ptr`. For `ptr`
  give `ptr_len` — the pointee bytes the fixture must provide. What the
  harness can see is exactly what you declare: a pointer the function
  chases through (a struct with internal pointers) needs an `argFactory`.
- **`argFactory`** names a module in `dev/verify_harness/` exporting
  `make_args(i) -> List[ArgSpec]`. Required whenever the inputs are linked
  structures; see `curve_fixture.py` for the pattern (build regions at
  fixed arena offsets, store **relative** pointers, declare `relocs=(off,…)`
  so both loaders rebase them).
- **`returns`**: `f32`, `f64`, `u64`, or `void`.
- **`candidateLoader`**: omit (or `"c"`) for tier 1 (the decompile);
  `"cargo"` for the shipped tier-2 probe
  (`crates/ow-sim/src/bin/verify_probe.rs`, built by `cargo build --release`);
  `"rust"` for the legacy standalone `rust_probe.rs` (the selftest's
  deliberately-broken entry and old units).
- **`state`**: optional block for a stateful unit. The snapshot is written at
  `base` before every oracle call, the whole region is compared after the call
  (the `state` channel), and the region is restored before the next call. A
  pointer arg that names a `guest` address indexes this snapshot; the probe
  translates the same guest address into its own copy. Reads inside the
  declared region are expected, not "undeclared global state":
  ```json
  "state": { "base": "0x200000000000", "hex": "00…" }
  ```
  or `"image": "path/to/snapshot.bin"` instead of `"hex"`. The state region is
  part of the surfaces digest; editing the file expires the verdict.
- **`scaffold`**: `true` marks a template emitted by `scaffold_unit.py`.
  `cli.py all` skips scaffolds; an explicit `cli.py fn/<Name>` still runs it.
  Never remove the flag until the unit is real.
- **`knownEquivalentMutants` / `acknowledgedGaps`**: see step 6.
- **`options`**: `coverageThreshold` is a percentage of discovered
  conditional branches; `mutationKillRate` is killed/scored. Do not lower
  them to pass — close the gap or declare, step 6. `allowedImageReads`
  accepts an exact address or an inclusive `0xlo-0xhi` range; use a range for
  a declared global block, and keep the list to constants/state the fixture
  actually models.

### Step 4 — write the fixture (the hard part, the part that matters)

The generic generator (`gen.py`) is fine for scalar signatures plus flat
pointee buffers: it draws from a seeded PRNG plus an adversarial edge pool
(sign zeros, NaN, inf, denormals, the constants the neighbourhood uses).
For linked structures you own `make_args(i)`:

1. **Derive the shape from the decompile, not from intuition.** If the
   code reads `param[2]` as a `ushort` count and walks `count` pairs,
   the fixture builds exactly that, with the count matching the array.
2. **Probe every boundary the code compares.** `count == 1`, `count == 2`,
   `count == 0x3f/0x40` if the code branches there, empty ranges, min==max.
   A branch nobody reaches is a coverage gap the harness will report.
3. **Pin discriminating cases at the front** when random draws rarely hit
   them (see `anim_time_wrap_fixture.py`: the flag-bit sweep exists because
   a `mask widen` mutant survived on random inputs).
4. **Keep indices in bounds after the callee's own arithmetic.** If the
   callee adds a base to a stored index, the stored index must be relative —
   an out-of-arena read makes the two sides compare different garbage and
   you learn nothing.
5. **Deterministic.** `make_args(i)` must return the same input for the
   same `i` forever; both sides depend on that.
6. **Stateful units put the struct in the `state` block, not in a fresh
   arena.** Return `ArgSpec(..., guest=base+off)` for the pointer; the
   oracle and probe both index the preloaded snapshot. The whole region is
   the state channel, so a field the function writes outside the pointee is
   still compared. Declare every pointer-shaped field's reloc offset so the
   state channel masks the two different host addresses.

### Step 4b — scaffold the unit from an address

`scaffold_unit.py` mechanises the boring part:

```bash
python dev/verify_harness/scaffold_unit.py 0x142069c60 --name OrthonormalizeCameraBasis
```

It resolves the address to an RVA, reads the `.pdata` function extent from the
flat recovery image, decompiles through the Ghidra bridge into
`dev/decompiles/<Name>_<exe>.c`, and writes a **template** under
`units/scaffolds/` (gitignored) with `"scaffold": true`. The template is not a
verification claim: fill the real argument surface, fixture and oracle, then
move it to `units/` and remove the flag. A scaffold that is never run cannot
turn the gate green.

### Step 5 — run the harness

```bash
python dev/verify_harness/cli.py fn/<Name>
```

Read the verdict:

| Status | Meaning | What to do |
|---|---|---|
| `verified` | All five stages passed | Nothing. The ledger records it. |
| `divergent` | A concrete counterexample exists | **Fix the decompile.** The report gives iteration, channel, both values. Re-run. |
| `inconclusive` | A gate failed or a piece is missing | Read the reason: fixture gap, coverage gap, blind mutation, undeclared state dependency, cold page. Close that specific gap. |
| `invalid` | The oracle disagreed with itself | Report it; do not touch the unit. |

**`inconclusive` is not a pass.** It means the harness could not establish
the claim — different from false, different from true.

### Step 6 — close the gaps the harness names

The harness never lowers a bar to make a unit pass. When it reports:

- **divergence** — the candidate is wrong until proven otherwise. The two
  bug classes at the top of this file are what real divergences looked
  like. Fix the `.c`, re-run. If you believe the *emulator* is wrong (not
  the candidate), say so and show the instruction that contradicts it.
- **coverage below gate** — add probes that reach the missed branches (the
  report prints their addresses). If a region is genuinely unreachable
  from this surface, declare it:

  ```json
  "acknowledgedGaps": [
    { "reason": "binary-search branch unreachable with count < 0x40",
      "coveredBy": "unit that does reach it, or none yet" }
  ]
  ```

- **surviving mutants** — the differential could not notice a real
  behaviour change. Either strengthen the fixture, or prove the mutant
  observationally equivalent:

  ```json
  "knownEquivalentMutants": [
    { "kind": "boundary -1", "line": 45,
      "rationale": "what would have to be observable, and why this unit's contract makes it unobservable" }
  ]
  ```

  The rationale must argue *observational* equivalence for this unit and
  say **what would observe the difference if anything could**. A vague
  rationale is a rejected rationale.
- **reads outside declared args** — the function consumes global state your
  fixture does not model. Either widen the fixture to place that state (the
  report gives the addresses), or conclude the function is not verifiable
  by emulation alone and record that in `notes`/the open questions.

### Step 7 — the selftest is not optional

Before trusting any verdict from your new unit:

```bash
python dev/verify_harness/cli.py selftest
```

It injects known defects into the verified anchor and requires detection,
and it requires the broken Rust entry to be caught. If the selftest fails,
no unit may be called verified — fix the harness first.

---

## Hard rules

1. **Never edit the oracle.** The oracle is the emulated shipped bytes
   from `module_offline.bin`. If you believe it is wrong, prove it against
   the disassembly; do not "fix" the image or the harness to agree with
   your candidate.
2. **Never weaken a gate to make a unit pass.** Fix the fixture, fix the
   decompile, or declare the gap explicitly with a written rationale.
3. **Never claim a unit is done.** Run the harness; the ledger decides.
4. **Never widen the equivalence relation.** NaN==NaN and reloc-slot
   masking are built in and counted; anything you want ignored must be a
   declared equivalent or acknowledged gap, visible in the report — not a
   silent change to `diff`-style comparison.
5. **A verdict expires when the inputs change.** The ledger binds results
   to SHA-256 of the oracle bytes, the candidate source, the options and
   the arg list. Edit any of them and the entry goes stale until re-run.
6. **Two addresses, one function.** v2b (`0x7ff7c...`, capture frame) and
   exe (`0x140...`, offline image frame) differ by a constant; the unit
   takes v2b, the file offset is the same either way. Name the frame when
   you write addresses in docs.

## Definition of done for a unit

The harness prints `verified`, which requires all of:

- the oracle bytes present and non-cold, the candidate compiling;
- for a `"cargo"` unit, the probe built from the real crate and the ledger
  bound to the function source + crate revision;
- the oracle deterministic on identical inputs;
- zero divergences across every channel (return bits, pointee writes,
  state region, no undeclared reads/writes);
- oracle branch coverage at or above the gate, or a named acknowledged gap;
- mutation kill rate at or above the gate, every survivor either fixed or
  declared equivalent with a rationale;
- `selftest` green for the session.

Anything less is not done, and the ledger will not say it is.

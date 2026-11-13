# The Ghidra project: where the client's code is

All behaviour is read from one program: **`Overwatch.exe`**, the complete
current-build image. There is no second image in the workflow.

**Where it lives:** `E:\overwatch-client-rs\dev\ghidra_run\owmod_clean`, inside the
project folder on the SSD. It is git-ignored: it is the client's code, and the
repository holds only original source and documentation.

**The rule:** a program in this project is the client as shipped, never
something we wrote. Every address in `docs/` is an address in this program.

## The program

| program | base | what it is | what it answers |
|---|---|---|---|
| `Overwatch.exe` | `0x140000000` (preferred PE base) | the complete image of the current build: all **12,076 `.text` pages** recovered from the disk ciphertext and the install's **plaintext `.pdata`** (201,246 entries, byte-identical at RVA `0x450b000`); the working project is the clean map with **120,490 function symbols** and **10,736,759 instructions** | every behaviour question; every current-build name in `docs/function-names.csv` |

The image comes from the installed `Overwatch.exe` itself. The `.text` pages
are recovered from the on-disk ciphertext; `.pdata`, `.rdata` and `.data` are
the install's own plaintext sections at the PE's preferred base. No running
process is involved.

**10,030,524 of the image's zero bytes are not data.** `.data` declares
`VirtualSize 0x0CC43BC` with only `0x0333600` raw bytes, so RVA
`0x03B79600..0x0450A3BC` is a BSS tail: the loader maps it zero and the client
builds all 7,956 component-type descriptors there at startup. Reading one of
those slots (an interface table, a vtable slot) from the file always gives
zero; read the `RegisterComponentTypeDescriptor` call site instead. The numbers
and the proof that the raw sections are byte-identical to the install are in
`docs/ghidra-image-audit.md` section 2.1.

## Function census and the `.pdata` rule

The executable's own `.pdata` is the authority for function starts. The
working project is `dev/ghidra_run/owmod_clean`:

Measured 2026-11-04 with [`ImageAudit.java`](ghidra/ImageAudit.java); the full
audit is [`ghidra-image-audit.md`](ghidra-image-audit.md). Re-run the script
after a repair and diff - a jump of more than a few units is a regression.

| item | count |
|---|---:|
| `.pdata` entries | 201,246 |
| real functions (`UNW_FLAG_CHAININFO` clear) | 119,802 |
| chained fragments (pieces of a larger function) | 81,444 |
| function symbols in the working program | 120,490 |
| symbols at a real `.pdata` start | 119,764 |
| symbols at a chained-fragment start | **283** (the census expects 0 - see the audit) |
| real `.pdata` starts with no function symbol | **38** |
| no-`.pdata` functions | 443 (226 at the first census) |
| instructions | 10,736,759 |
| instructions inside a function body | 6,736,303 (62.7%) |
| empty functions | 0 |
| bodies exactly equal to their extent | 118,577 |
| bodies that already include their chained continuation | 454 |
| bodies longer than their extent, extra not a continuation | 26 |
| bodies shorter than their extent | 707 (8 end at a `RET`) |
| multi-range bodies | 251 |
| named functions / named parameters | 1,679 / 1,231 |
| user-defined prototypes | 4 |
| `function-names.csv` rows | 464, 0 missing (326 were present at the first census) |

**The chained continuations are not in the bodies.** 25,638 functions have
continuations linked with `UNW_FLAG_CHAININFO`; 454 include them and 25,184 do
not. In 23,569 of those the body's last instruction jumps or falls straight
into the missing part, so the decompiled function is cut. That is 16.0 MB, 32%
of `.text`, and 157 of the 174 named functions are affected. The cause is
`DisassemblePdataFunctions.java`: it disassembles a chained fragment so jumps
can be read, but sets the body to the real extent only.

**110 `noreturn` flags are wrong.** 179 functions carry the flag; 110 of them
contain a `RET` in their own body, so the flag cannot be right. Those 110 blind
11,552 call sites in 6,355 functions. `Non-Returning Functions - Known` is on
(the `- Discovered` analyzer is off), which is the only analyzer that could
have applied them.

`MoverAdvance` (`0x140ca7f30`, `.pdata` extent `0x140ca7f30..0x140ca88fd`)
decompiles with its full body; the pre-clean map decompiled 233 lines, the
clean map about 440, because the pre-clean map had split the body into inner
fragments.

The quality analysis pass ran with `ConfigureQualityAnalysis.java` and finished
in 1,653 s. It keeps `Function Start Search`, `Aggressive Instruction Finder`,
`Call-Fixup Installer`, `Shared Return Calls`, `Subroutine References`,
`Decompiler Parameter ID`, `Non-Returning Functions - Discovered` and the
`x86 Constant Reference Analyzer` off. The first attempt was stopped when
`FindNoReturnFunctionsAnalyzer` entered `ClearFlowAndRepairCmd` and began
repairing real bodies; the x86 constant analyzer was stopped after it chased an
invalid pointer outside the image. The reference/data/string/demangler/Function
ID/stack/type-archive analyzers completed, and no chained or inside-real
function was created.

The pre-clean backup is `dev/ghidra_run/owmod`. It has 236,519 symbols:
12,901 at chained-fragment starts, 12,034 inside another real function and
91,782 with no `.pdata` range. Those are `Function Start Search` artifacts, not
evidence. The backup is kept for comparison only.

`cargo xtask frame-check` rejects a `function-names.csv` row whose address is
inside a `.pdata` range but not at its start. A no-`.pdata` leaf function is
allowed only when it is recorded as such; it is not an offcut.

**A clean import needs the install's PE headers.** A fresh
`analyzeHeadless -import module_offline.bin -noanalysis` on 2026-10-31 failed in
the PE loader's export/import/exception markup: the recovered flat image's data
directories are not valid (`Large number of functions`, invalid RVAs, and
`Function entryPoint may not be created on defined data`). The working route is
to copy the install's `Overwatch.exe`, overwrite only its encrypted `.text`
section with the recovered image's decrypted `.text` bytes (every other section
is already the install's plaintext), and import that patched PE with
`-noanalysis`. The full runbook is
[`ghidra-migration.md`](ghidra-migration.md).

**The image's identity.** One copy is the ground truth: the repo-local
`dev/data/offline_recover/module_offline.bin` (git-ignored), 75,726,848 bytes,
sha256 `12ACFD11…EB6300` (`python dev/audit_image.py` prints and checks the
full value). `dev/verify_harness/runner.py` and `scaffold_unit.py` use this
copy; a `D:\ow-dumps\offline_recover\module_offline.bin` citation in older
units means the same bytes and is only a fallback path. If the identity differs
the image changed and the analysis stopped being anchored - treat it as a new
image. The clean analysis PE (`Overwatch_clean.exe`) is built from the same
bytes by `dev/patch_clean_pe.py`; it is also git-ignored.

## Addresses

- **Current-image address:** `0x140000000 + RVA`.
- **Function name:** the preferred citation. `docs/function-names.csv` maps
  the name to its address and role.
- **RVA:** the low part of the address; `file offset == RVA` in a flat image.
- `.pdata` entries can be chained or fragmented, so a `.pdata` begin is not
  always the true entry. Use Ghidra's resolved entries
  (`docs/ghidra/DumpFunctionRanges.java`) for decompilation.
- There is one address frame. If an address is not in this program, it is not
  a valid citation.

## The allocator and truncated decompilation (2026-11-01)

`0x141baff30` is a **size-and-flags allocator**: `void *Alloc(size_t size,
unsigned flags)` where bit 0 requests 16-byte alignment and bit 1 clamps the
size to at least 1. Ghidra had it marked **noreturn**, so every caller's
decompilation stopped at the first allocation and the rest of the function was
invisible. Two concrete victims: `NodeType2Factory` (`0x142029f00`) decompiled
to a single `Alloc(0x30,0)` call although its real body is `0x129` bytes, and
`MapChunkCellBytesFixup`'s lazy-init branches read as aborts.

The fix is three steps, and it is worth repeating before trusting any
"incomplete" decompilation:

```text
1. docs/function-names.csv row: prototype "void * Alloc(unsigned long long size, unsigned int flags)"
   then  python dev/gh_apply_types.py
2. clear the flag itself (the prototype does not): run_script_inline with
   Function.setNoReturn(false) on 0x141baff30
3. rebuild the truncated bodies: /clear_flow_and_repair on each seed range
   (it reports instructions_in_seed_before/after and the noreturn boundaries)
```

The allocator's own body is `93` bytes and its aligned path tail-jumps to
`0x141bb0120`; the unaligned path uses the heap at `0x144188200`.

## Finding false `noreturn`s mechanically (2026-11-02)

It happened again, three times, and each occurrence silently truncated every
function that continues after the call. The culprits this time:

| callee | real role |
|---|---|
| `0x141bb00e0` | `Alloc(size, alignment)` — the second allocator, used as `Alloc(0x28, 0x10)` |
| `0x142ced8f0` | a throw helper, called on an invalid state |
| `0x142efe2b0` | the vector-grow memcpy (`dst, src, bytes`) |

`AppendCollisionEntryFromBrushSource` (`0x14213a510`) was stored as 167 bytes;
with the three flags cleared and the body repaired it is 2011 bytes and
decompiles end to end, which is what finally exposed how a brush becomes a
collision entry.

Do not hunt these by eye. `clear_flow_and_repair` in **dry-run** mode refuses a
seed whose repair would delete a defined continuation after a call, and its
error lists each offending pair — call site, callee, and the continuation
address. That list *is* the false-`noreturn` report for that region:

```text
clear_flow_and_repair(start=<function entry>, end=<end of its code>, dry_run=true)
  -> error: Repair would delete defined continuation after no-return call(s):
       [{call_address=..., callee_address=..., next_address=...}, ...]
set_function_no_return(<callee_address>, false)   # repeat for each row
clear_flow_and_repair(start=..., end=..., dry_run=false)
```

A genuinely non-returning helper (a fail-fast) never appears in that list,
because nothing continues after it.

## The stack-cookie check is not non-returning (2026-11-07)

The sentence above used to name `0x1426c3d40` as an example of a genuine
non-returning helper. That was **wrong**, and it was the single most expensive
wrong flag in the project: `0x1426c3d40` is `__security_check_cookie` and it is
called at the end of almost every function in the image.

Its body (`0x1426c3d40..0x1426c3d5d`, 30 bytes) returns when the value in `RCX`
equals `_DAT_143ac5f40` and its high word is zero, and only the mismatch path
reaches `__raise_securityfailure`. So it returns, and a function whose epilogue
follows the call was stored as ending at the call.

`FUN_140cc2290` is the case that exposed it: stored as 125 bytes, refused by
the dry run with

```text
Repair would delete defined continuation after no-return call(s):
  [{call_address=140cc2c22, callee_address=1426c3d40, next_address=140cc2c27}]
```

Clearing the flag (`set_function_no_return(0x1426c3d40, false)`) and then
repairing the seed gave a body of `0x140cc2290..0x140ce0ad0` - 6183 bytes, with
the epilogue, the cookie check and the `RET` present and the return value
readable. The dry run before and after the repair reported 544 instructions in
the seed and a zero delta, so nothing was lost.

The flag is now cleared project-wide. Any function stored short whose body ends
with a `CALL 0x1426c3d40` should be re-repaired with the recipe above; the
clearing is what makes the dry-run report meaningful again, because before it
the report named this call for every function it was pointed at.

## Zeros: the `.data` zero-fill tail (BSS)

`.data` has a **zero-filled tail**: its virtual size (`0xcc43bc`) is larger
than its raw size (`0x333600`), so the loader writes zeros for RVA
`0x3b79600..0x450a3bc` and the client fills what it needs at runtime. A read
there returns zeros, and those zeros are **not data**. The section table from
the install's own `Overwatch.exe`:

```text
.data  va=0x3846000  vsz=0xcc43bc  raw=0x3844800  rsz=0x333600
       raw-backed  0x3846000..0x3b79600   (the install's bytes)
       zero-fill   0x3b79600..0x450a3bc   (BSS: runtime-only)
```

This cost two wrong conclusions before it was checked: the "field table" at
`0x143c3c620` and the "type info" at `0x143c3efc0` are both in the zero-fill
range, so they are empty **by design**, not corrupt. The image is not damaged
and there is nothing to patch: the file simply does not contain those pages.

**The rule:** before reading a value, check which side of `raw_end` the address
falls on. `python dev/pe_layout.py --addr 0x143c3c620` answers it, and
`cargo xtask pe-layout` (`python dev/pe_layout.py --check`) is part of the gate,
so a changed section table fails loudly. If an address is `zero-fill`, the
value is runtime-only: read the **writer** (or emulate the client's init), and
record the finding as "written at runtime by <function>", never as "the value
is zero".

### A runtime-filled slot you can still read (measured 2026-11-20)

Three readers and no writer proves a slot is *not* filled by the code, not that
its value is unknowable. The recipe, and it works whenever the writer takes
static operands:

```sh
python dev/rip_refs.py 0x1442a0158     # readers *and* writers, by opcode
python dev/callers.py 0x142cadf50      # the setter's callers
```

The client's hot-patch guard is the worked case. `0x142cadee0` is
`mov r10,[0x1442a0158]; test r10,r10; jmp r10` - it dispatches through a slot in
the zero-fill tail, which is why 2,276 clusters screen as "call target with no
index entry". But `0x1442a0158` **has** a writer, `0x142cadf35`, inside the
setter pair `0x142cadf20`/`0x142cadf50`; the setters have exactly one caller,
`0x141ff3190`, which is registered as a *data pointer* at `0x143360ef8` (an
initializer, not a call) and passes three `lea` constants:

| slot | value installed | what it is |
|---|---|---|
| `0x1442a0158` | `0x141bb6660` | `jmp 0x141bb00e0`, whose body is a virtual call `[rax+0x30]` |
| `0x1442a0160` | `0x141b328e0` | `jmp 0x141baff90`: "call the singleton's vtable slot `+0x40`" |
| `0x1442a0168` | `0x14053a420` | `ret 0` - a no-op hook |

So the guard is statically resolvable, and reading the installer's operands is
the read: no emulation needed. The chain then ends at a vtable dispatch on a
global singleton, which is a *different* open question (the runtime object
model), not a data one.

The distinction that matters: a slot is readable from the image when its writer
is in-line code with static operands (`lea`/`mov imm`); it is not when the writer
copies from an argument or loops over a table. The Wwise bank XOR key at
`0x144293db8` is the second kind - readers, no writer at all, so its file value
(zeros) is the runtime value.

## Working in it

**In the GUI** (the operator's Ghidra, MCP-connected):

```text
list programs             # Overwatch.exe is the program
open /Overwatch.exe       # complete current-build image
decompile 0x140ca7f30     # MoverAdvance
```

**Headless** (batch decompiles to read offline): the canonical script is
`docs/ghidra/McpDecompile.java`, deployed to
`C:\Users\Happy\ghidra_scripts\`:

```text
analyzeHeadless.bat E:\overwatch-client-rs\dev\ghidra_run\owmod_clean `
  -process Overwatch.exe \
  -noanalysis -scriptPath C:\Users\Happy\ghidra_scripts \
  -postScript McpDecompile.java E:/overwatch-client-rs/dev/data/corpus/args.txt E:/overwatch-client-rs/dev/data/corpus/out.c
```

`-process Overwatch.exe` is not optional: without it headless runs the script
with no program open and every script fails on a null `currentProgram`
(measured 2026-11-08).

The args file is whitespace-separated `start end` pairs in the program's own
address space; whole-line `#` comments are allowed. `McpCallers.java` does the
same for callers, with call-site context.

## Named functions

The decompiler shows `FUN_140ca7f30`; the docs call the same function
`MoverAdvance`. The table that ties the two together is
**`docs/function-names.csv`**: program, address, name and role.

```sh
python dev/gh_apply_names.py --pending # apply the names that differ (>= 20)
python dev/gh_apply_names.py --check  # report what is named and what is not
```

A row is added **when a function's role is understood**, never before, and the
name is the one the docs already use. A function the project has not analysed
yet is created first in a deliberately small window (64 bytes), so its body
stays small and the decompiler can read it.

## Project-local scripts

These are original work, not game content:

- `docs/ghidra/McpDecompile.java <args-file> <out-file> [timeout]` — one
  parameterised decompiler. Functions are created if Ghidra has none.
- `docs/ghidra/McpCallers.java <args-file> <out-file>` — callers of each
  target, with call-site context.
- `docs/ghidra/DumpFunctionRanges.java <start> <end> <csv>` — Ghidra's
  resolved function entries and bodies. Use its entries for decompilation.
- `docs/ghidra/FunctionCensus.java` — read-only census: `.pdata` entries,
  real vs chained, where every function symbol sits, and the instruction count.
- `docs/ghidra/DisassemblePdataFunctions.java` — fill each real `.pdata`
  function body from its own extent after a `-noanalysis` clean import.
- `docs/ghidra/ExportAnnotations.java <out.tsv>` — export the live project's
  names, plates, tags, no-return flags, prototypes and named parameters.
- `docs/ghidra/ApplyAnnotations.java <annotations.tsv>` — copy those
  annotations into a clean project.
- `docs/ghidra/CreateMissingNames.java <function-names.csv>` — create the
  no-`.pdata` leaf names that the clean map does not contain; a name inside a
  real function becomes a label, not a nested function.
- `docs/ghidra/CheckNames.java <function-names.csv>` — verify every row is
  present as a function or a label.
- `docs/ghidra/ImportStructs.java <layouts.c>` — import the C generated from
  `docs/struct-layouts.csv`.
- `docs/ghidra/DisableNoisyAnalyzers.java` — turn off `Function Start Search`,
  `Aggressive Instruction Finder` and `Decompiler Parameter ID` before analysis.
- `docs/ghidra/ConfigureQualityAnalysis.java` — the full quality-pass setup:
  also turns off `Call-Fixup Installer`, `Shared Return Calls`, `Subroutine
  References` and `Non-Returning Functions - Discovered` so no analyzer rewrites
  a real `.pdata` body.
- `docs/ghidra/ResizePdataFunction.java <begin> <end>` — remove inner
  fragments and set the body to the authoritative `.pdata` extent.
- `docs/ghidra/MergeChainBodies.java [dry|apply]` — resolve every
  `UNW_FLAG_CHAININFO` chain to its root and widen the root's body to the union
  of its pieces, removing the function objects that sit inside a cold part.
  Applied to `C:/Temp/owread` on 2026-11-08: 25,533 bodies widened, 320 inner
  objects removed, body bytes 29.26 MB -> 44.42 MB, `ImageAudit` `in_body=25586`,
  `named_flowed=0`. 52 roots are skipped because a foreign body reaches into
  them (`OUTER` lines). It is idempotent - re-run after fixing those.
- `docs/ghidra/ProbeFlow.java <addr> [tail]` — why a decompile stops: body
  ranges, the last instructions with their flow type, and how many branch
  targets leave the body.
- `docs/ghidra/DecompScope.java <addr>...` — the body the decompiler was
  actually handed versus the program's body, plus C length and timeout. Use it
  before blaming the decompiler for a short output.
- `docs/ghidra/BodySnapshot.java <tag>` — body size and range count per function
  to `C:/Temp/owaudit/body_<tag>.txt`; diff two tags to see exactly what a pass
  changed.
- `docs/ghidra/ImageAudit.java` — the whole image audit in one read-only run
  (block hashes, `.pdata` census, chain resolution, function map, instruction
  ownership, flag damage, data coverage). The numbers live in
  [`ghidra-image-audit.md`](ghidra-image-audit.md).
- `docs/ghidra/PdataRepairList.java` — the function-map repair lists: 38
  missing real starts, 283 chain-start objects, 480 over-long and 707 short
  bodies, and the 174 named functions with chained continuations.
- `docs/ghidra/NoReturnProof.java` — per-flag evidence: does the flagged
  function's own body contain a `RET`?
- `docs/ghidra/NoReturnList.java` — the 179 flags with call-site counts and
  name provenance.
- `docs/ghidra/NoReturnImpact.java` — how many call sites a wrong flag hides
  (the false set, its callers, and the instructions behind them).
- `dev/gh.py` — the MCP bridge client used by the naming tools.

## Housekeeping

- **The project is not in git.** `.gitignore` keeps `dev/ghidra_run/` out. Do
  not add it, and do not copy the client's code into the repository.
- **One program.** When a question is answered, write the finding into
  `docs/formats/<system>.md` and add the name to the table. Do not keep
  per-question programs.
- **What Ghidra shows is not the deliverable.** The deliverable is the
  conclusion in the docs: an address, what it does, and why we believe it
  (`docs/provenance.md`).

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
| `Overwatch.exe` | `0x140000000` (preferred PE base) | the complete image of the current build: all **12,076 `.text` pages** recovered from the disk ciphertext and the install's **plaintext `.pdata`** (201,246 entries, byte-identical at RVA `0x450b000`); the working project is the clean map with **120,028 function symbols** and **6,676,292 instructions** | every behaviour question; every current-build name in `docs/function-names.csv` |

The image comes from the installed `Overwatch.exe` itself. The `.text` pages
are recovered from the on-disk ciphertext; `.pdata`, `.rdata` and `.data` are
the install's own plaintext sections at the PE's preferred base. No running
process is involved.

## Function census and the `.pdata` rule

The executable's own `.pdata` is the authority for function starts. The
working project is `dev/ghidra_run/owmod_clean`:

| item | count |
|---|---:|
| `.pdata` entries | 201,246 |
| real functions (`UNW_FLAG_CHAININFO` clear) | 119,802 |
| chained fragments (pieces of a larger function) | 81,444 |
| function symbols in the working program | 120,028 (includes 4 external entries) |
| symbols at a real `.pdata` start | 119,802 |
| symbols at a chained-fragment start | 0 |
| symbols starting inside another real function | 0 |
| no-`.pdata` functions | 226: 55 named leaves/loader + 171 analysis-created import/thunk functions |
| instructions | 6,676,292 |
| empty functions | 3 loader/import entries |
| named functions / named parameters | 1,550 / 571 |
| `function-names.csv` rows | 331: 326 functions + 5 labels, 0 missing |

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

A genuinely non-returning helper (the stack-cookie check `0x1426c3d40`, a
fail-fast) never appears in that list, because nothing continues after it.

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
analyzeHeadless.bat E:\overwatch-client-rs\dev\ghidra_run\owmod_clean Overwatch.exe \
  -noanalysis -scriptPath C:\Users\Happy\ghidra_scripts \
  -postScript McpDecompile.java E:/overwatch-client-rs/dev/data/corpus/args.txt E:/overwatch-client-rs/dev/data/corpus/out.c
```

The args file is whitespace-separated `start end` pairs in the program's own
address space; whole-line `#` comments are allowed. `McpCallers.java` does the
same for callers, with call-site context.

## Named functions

The decompiler shows `FUN_140ca7f30`; the docs call the same function
`MoverAdvance`. The table that ties the two together is
**`docs/function-names.csv`**: program, address, name and role.

```sh
python dev/gh_apply_names.py          # apply every name
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

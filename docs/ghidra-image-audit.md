# Ghidra image audit

What the analysis project actually is, how far it can be trusted, and what is
worth repairing. Measured on 2026-11-04 against the live program
`Overwatch.exe` in `dev/ghidra_run/owmod_clean` with
[`ImageAudit.java`](ghidra/ImageAudit.java); every number below is printed by
that script, so the audit can be re-run and diffed after a repair.

**Verdict.** The recovered bytes are not the weak part: all 75,702,716 image
bytes match the ground-truth copy, the exception directory is intact, and every
function boundary the client declares is there to be used. The weakness is in
what the project *did* with those bytes. Three mechanical defects account for almost all
of the gap between "the image is here" and "the decompiler shows the client's
code":

1. **110 false `noreturn` flags** hide 11,552 call sites in 6,355 functions.
2. **25,184 functions are cut at a chained `.pdata` boundary** - the client's
   own unwind data says the rest of the function follows; the project never
   merged it (16.0 MB, 32% of `.text`, 157 of the 174 named gameplay
   functions).
3. **283 function objects sit on chained fragment starts**, where no function
   may begin, and 251 bodies are fragmented around them.

None of the three is a data problem; all three are fixable from the image
itself, in that order of value.

---

## 1. What the program is

| item | value |
|---|---|
| Ghidra project / program | `dev/ghidra_run/owmod_clean`, `Overwatch.exe` |
| image base | `0x140000000` |
| bytes in the program | 75,708,940 (image 75,702,716 + 6,224 `tdb`) |
| ground truth | `dev/data/offline_recover/module_offline.bin`, 75,726,848 bytes, sha256 `12ACFD11…EB6300` |
| functions | 120,490 |
| instructions | 10,736,759 |
| data types | 516 (165 structs) |
| analyzers | 88 options (32 analyzer groups), 54 options enabled |

Ten memory blocks, all verified byte for byte against the ground-truth file:

| block | start | size | perms | sha256 (first 16) |
|---|---|---:|---|---|
| `Headers` | `0x140000000` | 1,024 | r-- | `50d91a179625e53b` |
| `.text` | `0x140001000` | 49,460,224 | r-x | `cafb456170d7d434` |
| `.rdata` | `0x142f2d000` | 9,539,584 | r-- | `8df599c0b4187ad4` |
| `.data` | `0x143846000` | 13,386,684 | rw- | `d0f6dc49b2d47124` |
| `.pdata` | `0x14450b000` | 2,415,104 | r-- | `49a7b04c2a888847` |
| `_RDATA` | `0x144759000` | 132,096 | r-- | `00229bf3d6aaecb0` |
| `.rodata` | `0x14477a000` | 5,120 | r-- | `b3aba44d0c4e09b6` |
| `.rsrc` | `0x14477c000` | 53,760 | r-- | `294b17432588425c` |
| `.reloc` | `0x14478a000` | 709,120 | r-- | `a8974b4b6d917c89` |
| `tdb` | `0xff00000000` | 6,224 | rw- | Ghidra's own PDB type block, not image bytes |

The `tdb` block is Ghidra bookkeeping (a type database from the PE debug
directory). It is not part of the client and carries no behaviour.

## 2. Byte integrity: exact

The recovery chain is: shard-0 ciphertext from the agent download, decrypted
offline page by page and checked against the warm build before the install's
plaintext `.pdata` and PE headers were grafted back (`docs/ghidra-migration.md`,
`dev/offline_page_recover.py`). The result, as graded here:

- **All ten blocks hash-identical** to `module_offline.bin` (the audit prints
  per-block SHA-256; `dev/audit_image.py` prints the file-level one).
- **`.pdata` is intact**: 201,246 entries, 0 with an unwind record that cannot
  be read, 0 whose `CHAININFO` parent is missing, 0 cycles. 119,802 real
  functions and 81,444 chained fragments.
- **The extents tile the code exactly**: 100% of the 81,444 chained fragments
  begin precisely where some extent ends, and no two extents overlap. This is
  what makes a mechanical body rebuild possible at all.
- **90.11% of `.text` is claimed by a `.pdata` extent** (57.66% real, 32.45%
  chained). The remaining 9.89% is not lost code: 2.43 MB of it is code Ghidra
  disassembled that no unwind entry describes (samples show CRT-style
  prologues and `RET`/`INT3` tails), 2.46 MB is `0xCC` alignment padding,
  2.8 KB is typed data.
- **A 301-function spread sample decompiles 300/301.** The single hard failure
  is `0x1405725d0`, one of the over-extended bodies that run past the end of
  the RAM block ("function body beyond ram:1405725d0"), not a data problem.

So: nothing about the recovered bytes needs to change, and no re-recovery is
worth doing. Every defect below is a Ghidra-side defect.

### 2.1 Zero bytes: the `.data` BSS tail is not data (measured 2026-11-09)

The image is 25.0% zero bytes (18,916,819 of 75,726,848), and **more than half
of that is not data at all**: `Overwatch.exe`'s `.data` declares
`VirtualSize = 0x0CC43BC` but `SizeOfRawData = 0x0333600`, so everything from
RVA `0x03B79600` to `0x0450A3BC` - **10,030,524 bytes**, 13.25% of the image -
has no bytes in the file. Windows maps that range zero and the client fills it
at startup. The recovery builder does the same thing and nothing else: it
starts from an all-zero image of `size_of_image` and copies
`min(size_of_raw, virtual_size)` per section (`dev/offline_page_recover.py`,
the reassembly at the end of `--stage full`).

Measured, so the answer is not an inference:

- the tail is **exactly** zero in `module_offline.bin`: 0 non-zero bytes in
  10,030,524. A lossy recovery leaves garbage or short pages, not a byte-exact
  zero run of 9.5 MiB;
- **every raw-backed section matches the install byte for byte**: `.rdata`
  9,539,584, `.data` raw 3,356,160, `.pdata` 2,415,104, `_RDATA` 132,096,
  `.rodata` 5,120, `.rsrc` 53,760, `.reloc` 709,120 - **0 mismatches**;
- `.text` is the one exception and it is the documented recovery input, not a
  defect: the on-disk section is ciphertext (8.000 bits/byte entropy, uniform
  byte histogram, 0.38% zeros), the image carries the decrypted code (6.654
  bits/byte). All 12,076 pages were recovered, `missing_pages: 0`;
- the image's own `.text` zeros (4,373,421 bytes) are instruction bytes, not
  holes: 2,039,833 zero runs, median run **2** bytes, longest 56, only 12 runs
  of 16 bytes or more, and **no** all-zero page. So no page was silently
  zeroed.

What lives in the tail is live state, and it is built by code, not loaded from
data files: **all 7,956** component-type descriptor structs passed to
`RegisterComponentTypeDescriptor` (`0x1421cd360`) sit in this tail (each of the
7,956 call sites resolved from its `.pdata` root gives a first argument in
`0x143B79600..0x14450A3BC`). That function writes its 18 arguments into the
struct (`*p = &DAT_142f7aa00`, then the sizes, code pointers and explicit
zeros) - it is the startup registration, so the file value of any slot there is
meaningless by construction. The metadata the stubs consume is the opposite: it
lives in raw-backed `.data` (`0x143962df4`, `0x1438eebf4`, XOR-encoded in
place), which is why reading those stubs works.

Consequences for reading:

- an interface table in the tail (`0x143D04610`, `0x14404EEC0`, the four
  hero-pool tables `0x143E79A90`/`0x143E79BF0`/`0x143E79EB0`/`0x143E79D50`)
  can **never** be read from the image; its contents are the registrar call
  site's arguments;
- nothing in the tail comes from the install's CASC data: asset bytes are
  opened at runtime onto the heap, and the tail holds code pointers, sizes and
  zeros;
- `dev/pe_layout.py --addr <address>` answers the classification mechanically
  (`raw` / `zero-fill` / `unmapped`); `--check` pins the three addresses the
  earlier "field table" mistake was made on.

## 3. Where the code lives

Bytes of `.text`, by what owns them:

| part | bytes | share |
|---|---:|---:|
| inside a function body | 28,692,221 | 58.0% |
| chained continuation, outside the body | 14,230,139 | 28.8% |
| chained continuation, still undefined (padding) | 950,641 | 1.9% |
| no `.pdata` extent at all | 4,892,416 | 9.9% |
| padding inside real extents | 589,888 | 1.2% |

(Each line is measured directly; the column sums to 99.8% because instruction
bytes that span a class boundary are counted once, in the class they start in.)

Instruction ownership tells the same story: 6,736,303 of 10,736,759
instructions (62.7%) are in a function body; the rest is disassembled code that
no function owns. Four out of ten `.text` bytes are invisible to the
decompiler's function view even though the bytes and the boundaries are known.

## 4. Function map: what is right and what is wrong

| item | count |
|---|---:|
| functions | 120,490 |
| at a real `.pdata` start | 119,764 |
| real `.pdata` starts with **no** function object | 38 |
| function objects at a **chained** start (must not exist) | 283 |
| function objects with no `.pdata` entry at all | 443 (226 in the first census) |
| empty bodies | 0 |
| body exactly equals its extent | 118,577 |
| body equals extent **plus** its chained continuation (already correct) | 454 |
| body longer than its extent, extra is not a continuation | 26 |
| body shorter than its extent | 707 (8 of them end at a `RET`) |
| multi-range body | 251 |
| named functions / user-defined prototypes / named parameters / thunks | 1,679 / 4 / 1,231 / 24 |

`PdataRepairList.java` prints each list with addresses; the counts above come
from the same run as the audit.

The 38 missing starts are real code, not misread data: each of the four that
were sampled is the target of a jump from a function that does exist (e.g.
`0x1407a3540` is the target of an unconditional jump at `0x1407a416c` inside
`FUN_1407a4150`). A caller that jumps to an address with no function object
gives the decompiler nothing to follow.

The 283 objects at chained starts are artifacts of the pointer-recovery pass:
`docs/ghidra/RecoverPointerFunctions.java` scans every 8-byte slot in
`.rdata`/`.data` for image pointers into `.text` and creates a function at any
target that is not already inside one - and a chained fragment start is outside
every function, because bodies stop at the real extent. `0x140100482`, one of
the 283, is pointed at from two `.rdata` slots (`0x1436d973c`, `0x1436d9750`),
so the scan sees it as a recoverable target. That the script did this is an
inference from its code; the fact is that
`DisassemblePdataFunctions.java` explicitly never creates such a function, so a
later pass did.

The 251 multi-range bodies are consistent with the same cause: Ghidra cannot let
two bodies overlap, so creating a function inside an existing body trims that
body around it, and the parent function ends up in pieces.

## 5. Decompiler health

### 5.1 False `noreturn` flags: 110 wrong, 11,552 call sites blinded

**Repaired 2026-11-04** (section 11): the 110 were cleared and the analyzer that
applied them is off. What follows is the measurement that justified it; the
program now reports `NORETURN flagged=69 call_sites=28412`.

`NoReturnProof.java` checks every flagged function for a `RET` instruction in
its own body: a function with a `RET` can return on that path, so the flag is
wrong for it.

| group | flags | call sites |
|---|---:|---:|
| body contains `RET` - **flag is wrong** | 110 | 11,552 |
| body has no `RET` - consistent with the flag | 69 | 28,418 |

`NoReturnImpact.java` follows the wrong ones: 11,552 call instructions in 6,355
functions call a callee that Ghidra believes never returns, and the caller's
code after the call (up to 1.07 M instructions by a loose upper bound) is
dead code to the decompiler. With `Non-Returning Functions - Discovered` off,
the only analyzer that can apply them is `Non-Returning Functions - Known`
(whose `Repair Flow Damage` option is still on); the worst offenders are ordinary helpers
called from hundreds of sites: `FUN_140bd94b0` (621 call sites),
`FUN_14077d230` (457), `FUN_140d64e90` (368), the library routines `strlen`
`0x1426cce30` (894) and `__chkstk` `0x1426c3ce0` (940), and - the one that
matters for this project - the named gameplay helper
`GetPlayerComponentByIndex` `0x14214dcc0` (718).

The 69 "consistent" flags are not proof of non-return - they only lack a `RET`.
One of them, `FUN_1426c3bcc` with 21,989 call sites, is the CFG dispatch thunk
every indirect call goes through; marking that non-returning is correct in the
"does not come back to this call site" sense.

### 5.2 Functions cut at a chained boundary: 25,184

The client splits a function into hot and cold parts and links the cold part
with `UNW_FLAG_CHAININFO`. `DisassemblePdataFunctions.java` disassembles the
chained part but sets the function body to the real extent only, so the cold
part is code that belongs to the function and is not in it.

| item | count |
|---|---:|
| real functions with chained continuations (resolved through `CHAININFO`) | 25,638 |
| of those, body already includes the continuation | 454 |
| body stops at the extent, continuation missing | 25,184 |
| **continuation is reached by the body's last instruction** (jump or fall-through) | 23,569 (92%) |
| chained pieces that end in `RET` | 25,688 |
| bytes in the missing continuations (the 25,638 roots) | 16,038,121 |
| named gameplay functions affected | 174 (157 of them with the continuation directly reached) |

That 92% is the proof that this is not an academic nicety: the hot body's last
instruction jumps, or falls through, into an address the function does not own.
`MoverInvoker` `0x1407ffc50`, `RunMapLoadEntry` `0x14055a0f0`,
`FirstPersonCommandUpdate` `0x140654730`,
`UpdateCameraAnimationState` `0x14059c6e0` and 170 other already-named
functions are in this set, so the code the project reads for gameplay is
incomplete today.

**The image has 11 more chain roots than this table counts.** Grouping all 201,246 `.pdata` entries by their *transitive*
`CHAININFO` root gives 25,649 roots carrying 16,049,487 bytes of continuation;
the table's 25,638 / 16,038,121 are the same set restricted to roots that have a
Ghidra function object. The 11 roots without one
(`0x140ccce40`, `0x140cfa090`, `0x140d10b90`, `0x140dc7700`, `0x140de7d40`, ...)
carry exactly the 11,366-byte difference, which is why 8.3's "create the 38
missing starts" subsumes them. `python dev/corpus.py verify` pins the image-side
numbers (25,649 roots, 0 gaps, 0 pieces claimed twice), so the two sides can be
re-measured against each other rather than one trusted.

## 6. Data, types and documentation

Data blocks, typed share (`getDefinedData` over the whole block):

| block | size | typed | share | items | strings |
|---|---:|---:|---:|---:|---:|
| `.rdata` | 9,539,584 | 4,807,882 | 50.4% | 412,183 | 13,470 |
| `.data` | 13,386,684 | 252,365 | 1.9% | 29,560 | 599 |
| `.rodata` | 5,120 | 18 | 0.4% | 1 | 1 |
| `_RDATA` | 132,096 | 0 | 0.0% | 0 | 0 |
| `.text` | 49,460,224 | 112,280 | 0.2% | 61,292 (jump tables) | 0 |

`.data`'s 1.9% is not as bad as it looks - 10.1 MB of the block is the
zero-fill tail (`docs/ghidra-project.md`, "Zeros"), and 3.36 MB is real
initialised data of which 252 KB is typed.

The reference view is the better measure of what is missing: of the addresses
referenced from code, 24,245 in `.rdata` and 6,315 in `.data`, **1,819 (7.5%)
and 4,537 (71.8%) are still undefined**. Every one of those is a load or store
the decompiler prints as a raw address. On the string side, a raw scan of
`.rdata` finds 21,634 ASCII runs of six bytes or more while only 13,470 are
typed strings: about 8,600 strings are untyped.

Documentation depth: 1,679 named functions (1.4%), 4 user-defined prototypes,
1,231 named parameters, 1,508 plate comments, 71 pre-comments, 63,724
user-named labels out of 1,605,755 (the rest are `DAT_`/`LAB_`). 24 names came
from Function ID matches; the rest from the project's own reading.

## 7. Analyzer settings that matter

| option | state | consequence |
|---|---|---|
| `Non-Returning Functions - Known` | **on** | source of the 179 flags, 110 of them wrong |
| `Non-Returning Functions - Discovered` | off | |
| `... .Repair Flow Damage` | **on** | the repair path that rewrote real bodies once before; it is still armed |
| `Function ID` | on | 24 library names, and it can attach a library prototype (`strlen` is flagged non-returning) |
| `PDB Universal` | on | no symbol file ships with the install; wasted time |
| `Function Start Search` | off | correct - it is what produced the 12,901 junk starts in the pre-clean map |
| `Call-Fixup Installer`, `Shared Return Calls`, `Decompiler Parameter ID` | off | correct |

## 8. What is worth doing, in order

Each item states its effect and how to prove it. Nothing here needs the image
to be recovered again.

### 8.1 Clear the 110 provably wrong `noreturn` flags - highest value

**Done 2026-11-04** with `ClearFalseNoReturn.java` (section 11). The recipe stays
here because it has to be repeatable on a re-import.

The flag is what makes the decompiler hide a caller's remainder; the caller's
code is present and disassembled. Clearing 110 flags restores 11,552 call sites
in 6,355 functions.

- **How:** `NoReturnProof.java` lists them; clear `Function.setNoReturn(false)`
  for addresses whose body contains `RET` only. Never clear a flag that has no
  `RET` without reading the function (the CFG dispatch thunk is one of them).
- **Prove:** re-run `NoReturnImpact.java`; `false_flags` must be 0 and
  `IMPACT call_to_false` 0. Decompile `FUN_140bd94b0` and a caller of it
  before and after; the caller's line count must grow.
- **Also:** turn off `Non-Returning Functions - Known` afterwards, or it will
  re-apply on the next `-process` run.

### 8.2 Merge the chained continuations into their function bodies

25,184 bodies gain their cold part (16.0 MB, 32% of `.text`), including 157
named gameplay functions. This is the single largest correctness fix: today the
decompiled parent of a hot/cold pair ends at a jump into code the function does
not own.

- **How:** for each real `.pdata` entry, resolve `CHAININFO` to the root
  (recursively, as `ImageAudit.java` does), then set the root function's body to
  the union of the real extent and every chained fragment that resolves to it.
  Read-only preparation: `PdataRepairList.java` prints the 174 named cases.
- **Prove:** `ImageAudit` must report `CHAIN_IMPACT in_body` ~ 25,638 and
  `body_flows_into_chain` ~ 0 (all continuations now in the body); body bytes
  rise from 28.7 MB to ~44.7 MB.
- **Risk:** a chained fragment that is a separate funclet must stay outside the
  body. Merge only the 23,569 functions whose body actually flows into its
  continuation first, verify those, then decide the remaining 1,615 one by one.

### 8.3 Fix the function map

- Delete the 283 function objects at chained starts (`PdataRepairList.java`
  AT_CHAIN). Do it **after** 8.2: once the parent owns those addresses, the
  child cannot come back.
- Create the 38 missing real starts (MISSING_REAL).
- Review the 26 over-extended bodies whose extra bytes are not a chained
  continuation - those are the ones that can eat a neighbour.
- Add the `.pdata` check to `RecoverPointerFunctions.java`: never create a
  function at a chained start, and never at an address inside another
  function's extent.
- **Prove:** `ImageAudit` reports `at_chain_start=0` and
  `real_starts_missing=0`; `FunctionCensus.java` reports the same, and the
  `cargo xtask frame-check` entry check still passes.

### 8.4 Type the data the code reads

- 1,819 + 4,537 referenced-but-undefined targets: start with the ones over 10
  xrefs (`list_globals?type_filter=undefined`).
- 8,600 untyped strings: define them in one pass; it costs nothing and makes
  string xrefs usable.
- `_RDATA` (132 KB) is entirely untyped; it is the read-only data of the
  engine layer and will be needed for the engine work.
- **Prove:** the audit's `DATA` lines, before/after.

### 8.5 Set prototypes for the functions already named

Only 4 user-defined prototypes exist, so every call into a named gameplay
function shows untyped arguments. `docs/function-names.csv` carries a
`prototype` column and `ApplyAnnotations.java`/`ImportStructs.java` already
apply prototypes; wiring the column through is mechanical.
**Prove:** `named_params` rises from 1,231; decompiled call sites show typed
arguments instead of `(int)`/`(void *)`.

### 8.6 Re-arm the right analyzers

Turn off `Non-Returning Functions - Known` and
`... - Discovered.Repair Flow Damage` until 8.1 is verified, then re-enable only
analysis that does not rewrite real bodies. `ConfigureQualityAnalysis.java` is
the place.

### 8.7 Keep the numbers pinned

The census table in `docs/ghidra-project.md` drifted (120,028 functions, 1,550
names, 6,676,292 instructions) because nothing re-measured it. Recommended:
keep `ImageAudit.java` output as the pinned block in this page and run it after
every repair; a drift of more than a few units is a regression. The pre-existing
`cargo xtask frame-check` already rejects names that are not at a `.pdata`
start - the same idea should cover chained-start functions.

### 8.8 Give the `.pdata`-less code targets a function object - 33,684 targets

33,684 distinct `.text` addresses are the target of a call or a jump from inside
a function while neither a function nor a data item covers them. 382 of them
have ten or more inbound references, 814 have forty or more call or jump
references; the six worst have 4,225, 4,204, 3,890, 3,797, 2,531 and 1,899.
None is inside a `.pdata` entry - `dev/fn_extent.py` says so
for `0x141bb6660`, `0x142dfc730`, `0x141b1ab00`, `0x141c23490`, `0x1423fe180`
and `0x141c5b7b0` - so the client's table does not describe them and a body is
an inference, not a read.

- **Why it matters:** a callee with no function object is printed by the
decompiler as an unresolved address, and the registers it takes stay untyped;
the worst one is called 4,225 times.
- **How, safely:** create the function object, keep the body the disassembler
infers, put a plate comment on the entry that states the evidence class ("no
`.pdata` entry; body inferred"), and never add it to
`docs/function-names.csv` - `dev/frame_check.py` verifies names against
`.pdata` starts and would rightly reject it.
- **Prove:** the target count falls; `cargo xtask frame-check` still passes;
the decompile of a caller names the callee.
- **Risk:** an inferred body can overrun a neighbour, so start with the 98
targets that have ten or more references.

### 8.9 RTTI class names are present but undecoded - 79 descriptors

`.data` holds 79 MSVC type-descriptor names (`.?AV...`): the Enlighten GI
middleware (`BaseSystem`, `BaseProbeSet`, `BaseCubeMap`, `CpuSystem`,
`ProbeSetManagerVoxel`, ...), the `Geo` interfaces, two
`DGTFProcessorFXVSA` texture classes and `std` exceptions such as
`logic_error` and `length_error`. None is referenced by an absolute pointer
(0 xrefs to `0x143b78290`, `0x143ad8520`, `0x143b78480`), so the locators that
would lead to the vtables are image-relative (an RVA dword at
`descriptor - 0x10`) or absent. The value is in the engine layer, not in
gameplay, so this ranks below 8.1-8.3.

- **Prove:** a vtable reached through a locator, carrying the class name.

### 8.10 Fix the 47 name collisions

47 names are shared by two or more functions (`scalar_deleting_destructor`,
`WorldBaseCtor`, `operator_new[]`, `FrameUnwindToEmptyState`, `state_case_normal`,
`process`, ...). A shared name makes a call site ambiguous in the decompiler and
silently breaks any name-to-address lookup that assumes uniqueness. The
`frame_check` entry check cannot see this, so it needs its own check.

### 8.11 Give the named functions prototypes

Signature provenance, measured over all 120,490 functions: 119,718 default, 768
imported (Function ID matches), **4 user-defined**. Parameter depth: 747
functions (0.6%) have any parameter at all, 1,784 parameters in total, of which
1,231 are named. So every call into a named gameplay function shows untyped
arguments. `docs/function-names.csv` carries a `prototype` column and
`ApplyAnnotations.java` already accepts `prototype_b64`/`params_b64`, so the
path is mechanical; the work is reading each prototype, not wiring it.

### 8.12 Turn off `Non-Returning Functions - Known` and keep it off

**Done 2026-11-04**: off in the program options and in
`ConfigureQualityAnalysis.java`, together with `PDB Universal` (no symbol file
ships with the install) and `... Discovered.Repair Flow Damage`. It was the only
analyzer that applied the 179 flags, and it survives as a program option, so
leaving it on would have re-created what 8.1 removed on the next analysis pass.

## 9. How to re-run

```sh
# from the Ghidra GUI bridge (see docs/tools.md)
cp docs/ghidra/*.java ~/ghidra_scripts/
# then, in the program:
#   run_script_inline / run_ghidra_script ImageAudit.java
# or headless:
analyzeHeadless dev/ghidra_run owmod_clean -process Overwatch.exe -noanalysis \
  -postScript ImageAudit.java
```

The scripts added by this audit:

- `docs/ghidra/ImageAudit.java` - every number in this page in one run (read-only).
- `docs/ghidra/PdataRepairList.java` - the repair lists (missing starts,
  chain-start functions, over/short bodies, the 174 named chain cases).
- `docs/ghidra/NoReturnProof.java` - per-flag evidence (`RET` in body).
- `docs/ghidra/NoReturnList.java` - the flag list with call-site counts.
- `docs/ghidra/NoReturnImpact.java` - how many call sites a wrong flag hides.
- `docs/ghidra/ImageWorklist.java` - the second-pass worklists: the 33,684
  function-less code targets ranked (8.8), the 47 duplicate names (8.10),
  signature provenance and parameter depth (8.11), the 79 RTTI descriptors and
  whether anything points at them (8.9).
- `docs/ghidra/BodySnapshot.java` - `entry body_bytes range_count` for every
  function, so any pass that changes bodies can be diffed exactly against a
  before-snapshot (read-only).

Two of them change the program, both reversible and both with a dry run:

- `docs/ghidra/ClearFalseNoReturn.java [clear|restore]` - clears a `noreturn`
  flag only when the body contains a `RET`, and writes the cleared list to
  `C:/Temp/noreturn_cleared.txt` so `restore` puts them back.
- `docs/ghidra/RecoverPointerFunctions.java dry|int3|padded|all` - creates a
  function only at a real `.pdata` start; `dry` reports without writing. It is
  the script whose earlier unguarded version produced the invented functions.

## 10. Not tested

- Whether each of the 69 `noreturn` flags without a `RET` is genuinely
  non-returning: only the one that ends in `INT3` and the CFG dispatch thunk
  were inspected.
- Whether merging chained bodies is safe for the 283 artifacts *before*
  deleting them - the order 8.2 then 8.3 is a recommendation, not a proven
  sequence.
- The 2.43 MB of disassembled code with no `.pdata` entry: sampled (CRT-style
  prologues, `0xCC`-padded) but not classified function by function, and the
  33,684 control-flow targets that land in it were counted but not classified.
- Whether the 79 RTTI type descriptors lead to vtables through image-relative
  locators: not scanned.
- Which of the 47 duplicate names are legitimate template instantiations and
  which are import collisions: not read.

## 11. First repair pass, 2026-11-04

Only the items whose risk is none or low were done. Every one is reversible and
every one is proven below.

| item | change | proof |
|---|---|---|
| 8.1 | 110 `noreturn` flags cleared (`ClearFalseNoReturn.java`) | each cleared flag's body contains `RET`; `NoReturnImpact` call sites fall |
| 8.12 | `Non-Returning Functions - Known`, its bookmarks, `... Discovered.Repair Flow Damage` and `PDB Universal` off | `get_program_options`: all four read `false` |
| 8.3 (guard) | `RecoverPointerFunctions.java` creates a function only at a real `.pdata` start, and has a `dry` mode | `dry` run: `extents=201246 real=119802 chained=81444`, `would_create=4`, `skipped_not_pdata_start=72786` |
| table | `dev/gh_apply_names.py` renamed only, never creates - it was the source of *named* chain-start artifacts | four rows pointed at chained starts (`0x14143b5c0`, `0x14143bbe5`, `0x14142dbf0`, `0x14142ee0c`); they now declare `container=<root>` |
| gate | `dev/frame_check.py` rule 4: a name at a chained start is a fault unless the row declares the chain root, and the root is verified by walking `UNWIND_INFO` chain records | `--selftest` covers unmarked, wrong-root and correct-root rows; live run 0 faults |
| table | 10 rows with unquoted commas in the role repaired, `container` column added | `frame_check` 0 faults; `gh_apply_names.py` reads 8 fields per row |

### State after the pass

| metric | before | after |
|---|---:|---:|
| `noreturn` flags | 179 (110 provably wrong) | **69** (0 provably wrong) |
| call sites behind a `noreturn` callee | 39,968 | 28,412 |
| functions / instructions | 120,490 / 10,736,759 | same |
| `.pdata` extents (real/chained) | 201,246 (119,802 / 81,444) | same |
| chain roots with continuation in body | 454 of 25,638 | same |
| bodies exact / short / over / fragmented | 118,577 / 707 / 480 / 251 | 118,578 / 707 / 479 / 250 |

### What the body diff found (unexplained, kept as a control)

`BodySnapshot.java` before and after: 120,490 lines, **3 differ** (the two files
are `C:/Temp/owaudit/probe_base.txt` and `probe_now.txt`):

| function | `.pdata` extent | body before | body after |
|---|---:|---|---|
| `0x140c6e686` | 1,162 B (`0x140c6e686`-`0x140c6eb10`) | 3,276 B, 11 ranges | 1,735 B, 2 ranges |
| `0x140c818c0` | 115 B (`0x140c818c0`-`0x140c81933`) | 309 B, 2 ranges | 115 B, 1 range - **now exact** |
| `0x140dd6fb0` | 1,538 B (`0x140dd6fb0`-`0x140dd75b2`) | 2,782 B, 6 ranges | 2,328 B, 3 ranges |

Every range that disappeared lay beyond the function's own extent: the first
body range of each still covers its whole `.pdata` extent (except a 2-byte gap
at `0x140c6e96e` in `0x140c6e686`, inside its extent, whose state before the
pass is not recoverable from a body-size snapshot). None of the three became
shorter than its extent, so no authenticated code was lost, and no metric that
tracks authenticated code moved: extents, chain pieces, `in_body`, `short`
bodies (707 before and after) and the function count are identical. Aggregate
effect: `over` 480 -> 479, `over_bytes` 682,730 -> 682,082, `exact`
118,577 -> 118,578. The likely cause is a transient function the probe created
and removed at four missing real starts (`0x140c7d7c0`, `0x140de7d40`,
`0x140ea56a0`, `0x142384bb0` - all four absent again now); it is recorded as
unexplained rather than explained away. From now on every body pass gets a
`BodySnapshot` diff against the previous one.

### Snapshot and restore

The project state before the pass is a folder copy at
`C:/Temp/ghidra_snapshot/owmod_clean.rep` (1.7 GB on disk). To roll back: close Ghidra,
replace `dev/ghidra_run/owmod_clean.rep` with it, reopen. The 110 cleared addresses
are in `C:/Temp/noreturn_cleared.txt`; `ClearFalseNoReturn.java restore`
re-applies them.

# Ghidra clean-map migration

**Done.** `owmod_clean` is the working project
([`ghidra-project.md`](ghidra-project.md)); this is the runbook that built it,
kept as the reproducible path for a lost project. The `owmod` project is the
pre-cleanup backup.

The clean map is the executable's own `.pdata` function set, not Ghidra's
byte-pattern function search. This runbook is the reproducible path.

The clean map has **119,805 functions** at import (119,802 real `.pdata`
entries, 0 chained-fragment starts, 0 functions inside another real function,
3 empty loader/entry functions). The live map has 236,519 symbols because it
also contains function-start-search artifacts.

## 1. Build the clean PE

```sh
python dev/patch_clean_pe.py
```

This writes `dev/data/offline_recover/Overwatch_clean.exe`: the install's valid
PE headers with only the encrypted `.text` replaced by the recovered decrypted
bytes. The file is git-ignored. Copy it to a temporary folder named
`Overwatch.exe` so the imported program matches `function-names.csv`:

```sh
mkdir C:\Temp\opencode\clean_import
copy dev\data\offline_recover\Overwatch_clean.exe C:\Temp\opencode\clean_import\Overwatch.exe
```

## 2. Import and fill the bodies

```text
analyzeHeadless.bat E:\overwatch-client-rs\dev\ghidra_run owmod_clean ^
  -import C:\Temp\opencode\clean_import\Overwatch.exe -noanalysis ^
  -scriptPath C:\Users\Happy\ghidra_scripts -postScript FunctionCensus.java

analyzeHeadless.bat E:\overwatch-client-rs\dev\ghidra_run owmod_clean ^
  -process Overwatch.exe -noanalysis ^
  -scriptPath C:\Users\Happy\ghidra_scripts -postScript DisassemblePdataFunctions.java
```

Expected after the first pass: `totalFns=119805 atReal=119802 atChained=0
insideReal=0 nonPdata=3 emptyFns=119805 instructions=0`. The second pass sets
each real body to its `.pdata` extent and disassembles it; expect
`emptyFns=3` and about **6.37M instructions**.

## 3. Import the structs

```sh
python dev/gh_apply_types.py --check    # writes ~/.cache/ow-struct-layouts.c
```

```text
analyzeHeadless.bat ... -process Overwatch.exe -noanalysis ^
  -postScript ImportStructs.java C:/Users/Happy/.cache/ow-struct-layouts.c
```

Expect `IMPORT /MoverBody len=7168` and `IMPORT /MoverComponent len=304`.

## 4. Export the live annotations

Run on the pre-clean `owmod` backup, not the working `owmod_clean`:

```text
run_ghidra_script ExportAnnotations.java C:/Temp/opencode/ow_annotations.tsv
```

The TSV carries user names, plate comments, tags, no-return flags, user
prototypes and named parameters. It is outside the repository.

## 5. Apply the annotations and the leaf names

```text
analyzeHeadless.bat ... -process Overwatch.exe -noanalysis ^
  -postScript ApplyAnnotations.java C:/Temp/opencode/ow_annotations.tsv

analyzeHeadless.bat ... -process Overwatch.exe -noanalysis ^
  -postScript CreateMissingNames.java E:/overwatch-client-rs/docs/function-names.csv

analyzeHeadless.bat ... -process Overwatch.exe -noanalysis ^
  -postScript CheckNames.java E:/overwatch-client-rs/docs/function-names.csv
```

The clean map does not contain every no-`.pdata` leaf; `CreateMissingNames`
creates the ones in a gap and labels the ones inside a real function. Expect
`present=326 labels=5 missing=0 wrong=0` for the 331-row table.

## 6. Analysis without function-start heuristics

```text
analyzeHeadless.bat ... -process Overwatch.exe ^
  -preScript ConfigureQualityAnalysis.java
```

The pre-script turns off the analyzers that invent or repair function starts:
`Function Start Search`, `Aggressive Instruction Finder`, `Call-Fixup
Installer`, `Shared Return Calls`, `Subroutine References`, `Decompiler
Parameter ID` and `Non-Returning Functions - Discovered`. The last one matters:
its `Repair Flow Damage` step runs `ClearFlowAndRepairCmd`, which can rewrite a
real `.pdata` body (the first attempt was stopped for this reason). It also
turns off `x86 Constant Reference Analyzer`: on this image it repeatedly chased
an invalid pointer outside the image. The reference/data/string/demangler/
Function ID/stack/type-archive analyzers stay on, and the analysis bookmarks are
off so the bookmark list stays useful. The completed pass took 1,653 s.

## 7. Verify

```text
analyzeHeadless.bat ... -process Overwatch.exe -noanalysis ^
  -postScript FunctionCensus.java
```

Expect `atChained=0`, `insideReal=0`, `emptyFns=3` and no large jump in
`totalFns`. A jump of tens of thousands means a function-start analyzer was
left on; rebuild the clean project.

As of 2026-11-04 the working project fails the first of those: `atChained=283`
(function objects on chained fragment starts, created after the census) and
`emptyFns=0`. See [`ghidra-image-audit.md`](ghidra-image-audit.md) section 4 for
the repair order; do not treat an `atChained` count as a reason to rebuild -
the bytes and the `.pdata` table are unchanged.

## Measured result (2026-10-31)

The throwaway clean project finished with:

| item | value |
|---|---:|
| functions | 120,028 (includes 4 external entries) |
| real `.pdata` starts | 119,802 |
| chained-fragment starts | 0 |
| inside another real function | 0 |
| no-`.pdata` functions | 226: 55 named leaves/loader + 171 analysis import/thunks |
| instructions | 6,676,292 |
| empty functions | 3 loader/import entries |
| CSV names present | 326 functions + 5 labels, 0 missing |
| applied names / plates / tags | 344 / 1,239 / 322 |
| applied prototypes / named parameters | 2 / 571 |
| quality analysis time | 1,653 s |

`MoverAdvance` decompiles with its full `0x9cd`-byte body and the `MoverBody *`
parameter; the live pre-clean map decompiled 233 lines, the clean map about
440, because the live map had split the body into inner fragments.

## 8. Swap

The clean import is created directly as project `owmod_clean` with program
`Overwatch.exe` (import the copied file named `Overwatch.exe`, so no program
rename is needed). Keep the old project as `owmod` (the pre-clean backup) and
use `owmod_clean` as the working ground truth. Open `owmod_clean` in the GUI
and save. Update `docs/ghidra-project.md`, `docs/tools.md` and `AGENTS.md` to
point at `dev/ghidra_run/owmod_clean`. The old `dev/ghidra_run/owmod` is kept
as the pre-clean map for comparison; it is not the working project.

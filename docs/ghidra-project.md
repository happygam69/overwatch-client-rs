# The Ghidra project: where the client's code is

Everything known about the client's *behaviour* is read in one Ghidra project.
This file says what is in it, which program answers which question, and how
the addresses in the docs map onto it.

**Where it lives:** `E:\OverwatchTest\dev\ghidra_run` - inside the project
folder, on the SSD, because Ghidra reads it hard. It is **not in git**: it is
the client's code, and the repository holds only original source and
documentation (see `CREDITS.md`).

**The one-line rule:** a program in this project is a *capture of the client*,
never something we wrote. A capture is named after the session it came from
and the base address it is mapped at, and every address in `docs/formats/`
names the program (or the capture) it was read from.

## The programs

| program | base | what it is | what it answers |
|---|---|---|---|
| `Overwatch.exe` | `0x140000000` (preferred PE base) | **the complete image of the current build** (2.25.0.1): all **12,076 `.text` pages** and the `.pdata` table recovered offline from the disk ciphertext (Plan A), imported 2026-10-23 and auto-analysed; **151,260 functions** | **every page of the current build**, including the 2,351 `.text` pages that `module_v21_clean.bin` holds zero. Addresses are RVAs: `0x140000000 + (v2b - 0x7ff7c4d20000)`. Read code here; its data is the PE's own at the preferred base, not a session's (see below) |
| `module_v21_clean.bin` | `0x7ff7c4d20000` (v2b) | **the current build's capture image** (2.25.0.1, the 21 October capture), 77 MB; missing pages partly filled 2026-10-23; **2,351 `.text` pages remain zero** | the docs' v2b frame and the program carrying the current-build names: the camera, animation runtime, entity/component system, weapons and statescript code. Prefer `Overwatch.exe` for any page that is zero here. Its data pointers belong to no single session |
| `module_v2full.bin` | `0x7ff7c4d20000` (v2b) | the 12 October flatten, build **2.25.0.0** - the build the docs' addresses belong to | the basis of the docs' findings: every `v2b` address in `docs/formats/` resolves here. It is the *old* build: the client has since updated (see the build note below) |
| `module_mapped.bin` | `0x7ff649000000` | **the pointer image**: the range capture's mapped code regions, at that session's base | chasing pointers in the range session: a pointer read here resolves inside this program |
| `module_heap.bin` | `0x161b0000000` | the same capture's heap regions | runtime structures: the material chain, the FP component, class descriptors - and the heap side of the pointers |
| `live_main_flat.bin` | `0x7ff736e60000` | the newest session's flat main image, at that session's base | the newest session's code **and** its pointers |
| `camera_warm.exe` | `0x7ff7c860e000` | the first-person camera region, warm | the FP camera setup and its consumers |
| `ldr/Overwatch_loader.dll` | `0x180000000` (disk) | the loader **as shipped** - its `.text` is encrypted, so only the headers and `.pdata` are readable | the loader's own structure; the code needs the warm image |
| `ldr-warm/loader_after_diff.bin` | the session's loader | the loader with its decrypted pages, flat (base 0, so a file offset is an RVA) | the loader code that is cold on disk |
| `ldr/loader_xor_regions.exe` | the client's own addresses | four regions of the warm loader (the applier, its caller, the two pad families) wrapped in one PE by `dev/build_loader_wrap.py` | **the page cipher**, in readable code, at the addresses the docs use |
| `regions/mover_ctl.exe` | a wrapped region | the mover's control code | the mover control (declared tier 3; the rates were measured instead) |
| `regions/speed_readers.exe` | a wrapped region | the four speed-reader bands | where the mover's speeds are read |
| `regions/clipobj.exe` | a wrapped region | the clip object | the per-bone animation update |

A **region wrap** is a small PE built around a slice of the client's code, so
that Ghidra can analyse it as code rather than as a raw dump
(`dev/mkpe.py` builds one). The wrap keeps the client's addresses: the
address in the docs is the client's, and it is the same in the wrap.

The loader needs a wrap because its `.text` is encrypted on disk: the only
readable copy of its code is the warm flat image, and a function created in a
37 MB flat image makes Ghidra follow the flow across the whole image (a
23 MB "function" that the decompiler then chokes on). Wrapping the four
regions that matter keeps every function small. Rebuild the wrap with
`python dev/build_loader_wrap.py` after a new capture.

## Which build is which

The client updates itself, and an update moves the code: between the
2.25.0.0 and 2.25.0.1 builds, **0 of 15,297** non-empty pages keep their
address. So each program says which build it is:

| program | build | use it for |
|---|---|---|
| `Overwatch.exe` | 2.25.0.1 (current) | **every `.text` page** of the current build; cold-code reads |
| `module_v21_clean.bin` | 2.25.0.1 (current) | the docs' v2b frame, the named functions, and data reads |
| `module_v2full.bin` | 2.25.0.0 | the docs' old-build addresses, and the findings they record |
| `module_mapped.bin`, `module_heap.bin` | 2.25.0.0 (range session) | pointers in the range session |
| `live_main_flat.bin` | 2.25.0.1 (newest session) | pointers in the newest session |
| `ldr/loader_xor_regions.exe` | the loader of the captures | the page cipher |

**Never merge two builds' pages into one image.** It produces an image
whose bytes at a given address come from different builds - every decompile
in it is a chimera. `dev/build_translate.py` carries an address from one
build to the next instead.

## Code is session-independent; data is not

**The rule:** the merged image is for **code**; the session images are for
**pointers**. Both halves are measured (`dev/pointer_check.py`):

- **Code is identical in every session.** Of the `.text` pages that are warm
  in two sessions of the same build, **7,491 of 7,491 are byte-identical**.
  That is why merging code pages across sessions is exactly safe.
- **Data is not.** Between two sessions of the same build, **289,733** of the
  `.rdata` words and **155,807** of the `.data` words differ, and they are
  pointer-shaped: a global holds the address that was current in whichever
  session captured that page. In a merged image those pointers are a mix, and
  a module pointer from session A does not even land in the v2b frame (the
  session's base differs).

**The offline `Overwatch.exe` is the exception on the data side.** It is not a
merge: its `.rdata`/`.data` are the disk image's own bytes at the PE's
preferred base `0x140000000`, so its static pointers land inside itself. The
`K` at RVA `0x2fac4b0` (`0.011519`) and the FP half-angle at `0x2f93da8`
(`0.00872665`) read correctly there. It still holds no runtime/heap pointer;
for those use a session program.

So in the merged program, trust the **code** - the disassembly, the
decompilation, the xrefs *within* the code - and treat every data pointer as
meaningless. When the work needs a pointer:

1. open the **session** program (`module_mapped.bin` with `module_heap.bin`,
   or `live_main_flat.bin`), where the pointer is the one that session had;
2. translate the doc's v2b address into that session's frame - the delta is
   in the run's `meta.json`, and `docs/architecture.md` gives the rule;
3. cross-reference the result back into the merged program for the code.

The session programs are rebased to their own session's base
(`0x7ff649000000` and `0x161b0000000` for the range capture,
`0x7ff736e60000` for the newest session) so that their pointers land
inside them.

```sh
python dev/pointer_check.py <image-a> <image-b>   # how much the data differs
```

## The 2026-10-23 page fill

`module_v21_clean.bin` was imported from a capture in which whole sections were
zero: 374 `.rdata` pages, 30 `.data`, 4 `.pdata` and 2,839 `.text`. The bytes
were available elsewhere, so the program was filled in place (saved):

- **`.text`: 488 pages** from `module_v21d.bin`. Code is byte-identical
  between sessions of the same build, so this is safe.
- **`.rdata`: 372 pages** and **`.pdata`: 4 pages** from the on-disk
  `E:\Overwatch\_retail_\Overwatch.exe`. The executable has the same
  `TimeDateStamp` (`0x6ac6f464`) and section table as the capture. Its
  `.text` was **not** used (it is encrypted on disk).
- **`.data`: 13 pages** from `module_v21d.bin` (they contain no image/heap
  pointers) and **16 pages** from the executable.

Pointer rule: every DIR64 relocation in a page copied from the executable was
rebased by `image_ib - exe_ib` = `0x7ff5f6e60000`, i.e. from the executable's
preferred base `0x140000000` to the image's PE base `0x7ff736e60000`. The
relocation list is the executable's own `.reloc`; no pointer from another
session was copied. Six pages the Ghidra program already held from a live
capture were **kept** rather than overwritten:
`0x7ff7c8566000`, `0x7ff7c8753000`, `0x7ff7c878c000`, `0x7ff7c8791000`,
`0x7ff7c87cf000`, `0x7ff7c87e9000`.

The full image is `D:/ow-dumps/module_v21_clean_full.bin`; the page list and
the per-page source/relocation counts are
`D:/ow-dumps/module_v21_clean_full.manifest.txt` and `.pages.csv`. The
program's `K` (`0x7ff7c7ccc4b0` = 0.011519) and FP half-angle
(`0x7ff7c7cb3da8` = 0.00872665) now read correctly.

Residual gaps: **2,351 `.text` pages** are zero in both the old image and
`module_v21d.bin`; 2 `.rdata` and 1 `.data` page are zero on disk too. Those
2,351 pages are **no longer a gap in the project**: they are present in the
complete `Overwatch.exe` program below. The patched data pointers are in the
image's PE frame (`0x7ff736e60000`), not the Ghidra v2b frame; for live
pointers use the session programs above.

**Complete offline image (2026-10-23).**
`D:/ow-dumps/offline_recover/module_offline.bin` is a PE at the preferred base
`0x140000000` with **all 12,076 `.text` pages** recovered from the disk
ciphertext and the plaintext `.pdata` (201,246 entries). One page
(`0x540000`) keeps a declared 16-byte tail. Its `.text` is the project's
**`Overwatch.exe`** program: the 49,460,179 recovered bytes hash to
`60fe7a796f23ddf62da9be8bded4c81d9942eda200458d2920aff9ed3fd531af`, exactly
`text.bin`, and the program is auto-analysed to **151,260 functions** with
every `.text` page initialized. Spot-check: RVA `0x1000` (v2b
`0x7ff7c4d21000`) is zero in `module_v21_clean.bin`, and a real function with
a `.pdata` entry (`0x1000..0x106b`) in `Overwatch.exe`; the unnamed function
at RVA `0xd2f060` has the same 24 bytes as the named `FpViewmodelConsumer`.
No live capture was used. Report:
`D:/ow-dumps/offline_recover/REPORT.md`. The current-build names are still
applied to `module_v21_clean.bin`, not to `Overwatch.exe`.

## Named functions

The decompiler shows `FUN_7ff7c5a4f060`; the docs call the same function
`FpViewmodelConsumer`. The table that ties the two together is
**`docs/function-names.csv`**: program, address, the address the docs use,
the name, and what the function does. A name for the complete image carries
program `Overwatch.exe` and address `0x140000000 + RVA`; none are applied
there yet.

```sh
python dev/gh_apply_names.py          # apply every name
python dev/gh_apply_names.py --check  # report what is named and what is not
```

A row is added **when a function's role is understood**, never before, and
the name is the one the docs already use. A function the project has not
analysed yet is created first - in a deliberately small window (64 bytes), so
its body stays small and the decompiler can read it.

Two wrinkles worth knowing:

- The naming tool talks to the operator's Ghidra over the MCP bridge
  (`dev/gh.py`); it saves the program itself, and a save can take a while on
  the big images.
- `create_function` on a *flat* image (not a wrap) follows the flow across
  the whole image. Do not create functions in `loader_after_diff.bin`; use
  the wrap.

## Address conventions

- **v2b** is the standing image space: base `0x7ff7c4d20000`. A v2b address
  in a doc is the address in `module_v2full.bin`, unchanged.
- **A session address** is `v2b + (session base - 0x7ff7c4d20000)`. A
  capture's base is in its `meta.json` (`dev/live_page_diff.py` writes one).
- **A file offset** in a flattened image equals the RVA: `offset = v2b -
  0x7ff7c4d20000`.
- **The offline PE** is at its preferred base `0x140000000`:
  `offline = 0x140000000 + (v2b - 0x7ff7c4d20000)`. The RVA is identical
  because it is the same build. Its Ghidra program is `Overwatch.exe`.
- **The Ghidra stripped import** drops the first 64 KiB, so a headless
  stripped run sees `rva - 0x10000`.
- **The loader** has its own bases (disk `0x180000000`); loader addresses are
  named as loader RVAs, never as v2b.

## Working in it

**In the GUI** (the operator's own Ghidra, MCP-connected):

```text
list programs             # what is here
open /Overwatch.exe       # the complete current-build image (all 12,076 .text pages)
open /module_v21_clean.bin  # the same build's v2b frame and named functions
open /module_v2full.bin   # the old build the docs' addresses were read in
decompile 0x140d2f060     # FpViewmodelConsumer in Overwatch.exe
decompile 0x7ff7c5a4f060   # the same function in module_v21_clean.bin
```

**Headless** (batch, for a capture of decompiles to read offline): the
canonical script is `docs/ghidra/McpDecompile.java`, deployed to
`C:\Users\Happy\ghidra_scripts\`:

```text
analyzeHeadless.bat E:\OverwatchTest\dev\ghidra_run\owmod module_v2full.bin \
  -noanalysis -scriptPath C:\Users\Happy\ghidra_scripts \
  -postScript McpDecompile.java D:/ow-dumps/anim_args_s.txt D:/ow-dumps/anim_chain.c
```

The args file is whitespace-separated `start end` pairs in the program's own
address space; whole-line `#` comments are allowed. `McpCallers.java` does the
same for callers, with the call-site context.

## Adding a capture

1. Capture the pages (read-only; see `docs/formats/page-crypt.md` for the
   runbook) and merge them into a standing image:
   `python dev/warm_merge.py ...` -> `D:/ow-dumps/module_after_diff.bin`.
2. Import it into this project (Ghidra: File -> Import, or
   `ghidra_import_file`), name it after the session, and set the image base
   to the session's base so the addresses match the capture.
3. Record the program in the table above, with its base and what it answers.
4. The image itself stays outside the repository (it is the client's code);
   only the project database is kept here, and it is git-ignored.
5. If the new capture answers a question about a function, add its name to
   `docs/function-names.csv` and run `python dev/gh_apply_names.py`.

## Housekeeping

- **One program per open question.** When a question is answered, the
  program that answered it is deleted and the finding is written down in
  `docs/formats/<system>.md`. The image can always be re-imported from
  `D:/ow-dumps`.
- **The project is not in git.** `.gitignore` keeps `dev/ghidra_run/` out. Do
  not add it, and do not copy the client's code into the repository.
- **A snapshot** of the project as it was before the 2026-10-21 cleanup
  (21 programs) is at `D:/ow-dumps/owmod_snapshot_20261021_preprune`. It is a
  backup, not a second working copy; delete it when the cleanup has proved
  itself.
- **What Ghidra shows is not the deliverable.** The deliverable is the
  conclusion in the docs: an address, what it does, and why we believe it
  (`docs/provenance.md` defines the evidence classes).

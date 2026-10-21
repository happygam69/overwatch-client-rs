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
| `module_v2full.bin` | `0x7ff7c4d20000` (v2b) | the standing main-module image with the warm pages patched in; 70 MB, ~107k functions | **the default.** Every `v2b` address in the docs resolves here: the camera, the animation runtime, the entity and component system, the weapons and statescript code |
| `live_main_flat.bin` | the newest session's flat main image | the live main module from the newest capture | what the newest session's code looks like when the pages are warm |
| `camera_warm.exe` | a wrapped region | the first-person camera region, warm | the FP camera setup and its consumers |
| `module_mapped.bin` | `0x7ff649000000` | the older range capture's mapped code regions | pages the v2b image does not have (v2b is ~52% zero bytes) |
| `module_heap.bin` | `0x161b0000000` | the same capture's heap regions | runtime structures: the material chain, the FP component, class descriptors |
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

## Named functions

The decompiler shows `FUN_7ff7c4ff0060`; the docs call the same function
`FpViewmodelConsumer`. The table that ties the two together is
**`docs/function-names.csv`**: program, address, the address the docs use,
the name, and what the function does.

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
- **The Ghidra stripped import** drops the first 64 KiB, so a headless
  stripped run sees `rva - 0x10000`.
- **The loader** has its own bases (disk `0x180000000`); loader addresses are
  named as loader RVAs, never as v2b.

## Working in it

**In the GUI** (the operator's own Ghidra, MCP-connected):

```text
list programs            # what is here
open /module_v2full.bin  # the one to use by default
decompile 0x7ff7c4ff0060
callers / callees / xrefs
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

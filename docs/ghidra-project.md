# The Ghidra project: where the client's code is

All behaviour is read from one program: **`Overwatch.exe`**, the complete
current-build image. There is no second image in the workflow.

**Where it lives:** `E:\overwatch-client-rs\dev\ghidra_run\owmod`, inside the
project folder on the SSD. It is git-ignored: it is the client's code, and the
repository holds only original source and documentation.

**The rule:** a program in this project is the client as shipped, never
something we wrote. Every address in `docs/` is an address in this program.

## The program

| program | base | what it is | what it answers |
|---|---|---|---|
| `Overwatch.exe` | `0x140000000` (preferred PE base) | the complete image of the current build: all **12,076 `.text` pages** recovered from the disk ciphertext, the install's **plaintext `.pdata`** (201,246 entries, byte-identical at RVA `0x450b000`), auto-analysed to **151,260 functions** | every behaviour question; every current-build name in `docs/function-names.csv` |

The image comes from the installed `Overwatch.exe` itself. The `.text` pages
are recovered from the on-disk ciphertext; `.pdata`, `.rdata` and `.data` are
the install's own plaintext sections at the PE's preferred base. No running
process is involved.

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
analyzeHeadless.bat E:\overwatch-client-rs\dev\ghidra_run\owmod Overwatch.exe \
  -noanalysis -scriptPath C:\Users\Happy\ghidra_scripts \
  -postScript McpDecompile.java D:/ow-dumps/args.txt D:/ow-dumps/out.c
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

# Method and provenance

How a claim is established, how it is recorded, and what is not allowed.

## The rule behind every claim

A gameplay claim is either **read** from the client's own code or it is not
made. A data claim is either **read** from the install or it is not made. There
is no third source and no "probably".

A feature that cannot be read is **open**. It has no gameplay implementation
and no placeholder. This is what the 1-to-1 rule means in practice.

## Evidence classes

| class | meaning | what it looks like in a doc |
|---|---|---|
| **read** | a decompiled instruction path, a data value, or a static table in `Overwatch.exe` | `MoverAdvance` at `0x140ca7f30` reads the static offset at `+0xc0` |
| **install** | a value or layout read from the shipped files | `19452F15` = `5.5` for every readable hero |
| **verified** | a Rust reconstruction proven identical to the client's bytes | `fn/AnimTimeWrap` passes all five harness stages |
| **open** | not read | "the ramp writer is not located" |

There are no other classes. A claim that does not fit one of them is not a
finding; it is an open question.

## Rules for writing findings

1. **No game content is committed.** The `.gitignore` is a whitelist. Art,
   models, sounds, maps, locale data, extracted tables, keyrings and per-build
   key tables stay on the machine.
2. **No decompiled code is committed.** A finding is recorded as "function at
   `<address>` does X, because Y". No disassembly, no pseudocode, no decompiler
   output is copied into `docs/`.
3. **Name the address frame.** Every address is a current-image address:
   `0x140000000 + RVA`, or a named function whose address is in
   `docs/function-names.csv`.
4. **Read the real files rather than guessing.** A format claim is written
   down only when a byte-level check agrees with it.
5. **Every load-bearing claim has a check** that fails if the claim is wrong.
   A table with no check is a note, not a finding.

## What came from where

- **Behaviour — the executable.** All behaviour is read from `Overwatch.exe`:
  the complete current-build image, every `.text` page, the install's plaintext
  `.pdata`. The Ghidra project is the working copy; the image itself is the
  evidence.
- **Data — the installation.** The container, BLTE, encoding file, manifests,
  STU, models, clips, materials and values are read from the operator's own
  install and checked against tests that read the same bytes.
- **External sources — checking only.** Public projects and the wiki can
  confirm a number; they cannot create one. The main checks are OverFast
  (MIT), `workshop-rs` (MIT), the Overwatch Wiki (non-commercial), OverPy
  (GPL) and OSTW. See `CREDITS.md` for the licences.
- **Reference tooling — names and shapes.** OWLib/TankLib and TACTLib are used
  as a source of class and field names and as a second opinion. Where they
  disagree with the install, the install wins.

## Development tools

`dev/` holds the read-only analysis and verification tools: Ghidra bridge
helpers, data probes, format scans, the differential harness and the tool
index. They are original work and are tracked; their output is not. The tool
list is `dev/INDEX.md`.

## Open questions

The queue is [`open-questions.md`](open-questions.md). An entry is closed when
the answer has an address or a value, the finding is written in the relevant
`docs/formats/<system>.md`, and the implementing code has a test.

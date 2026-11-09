# AGENTS.md

## Project

A clean-room Rust reimplementation of the Overwatch client's **gameplay**:
movement, combat, the view model and the camera, with a Bevy shell and a
`wgpu` inspector. The operator's own installation supplies the data. The
client's own code supplies the behaviour.

Nothing from the game ships in this repository: no assets, no extracted files,
no decompiled code.

## The hard rule

**1 to 1 or not at all.**

A gameplay feature is implemented only when all three are true:

1. the client's own code or value has been read from the complete
   `Overwatch.exe` Ghidra image;
2. the implementation follows that code operation for operation; and
3. a test proves the result is identical — bit-exact for state, exact for
   values, first-divergence for a trace.

There is no substitute for a read expression: no placeholder, no "close
enough", no measured replacement. If the client's expression is not read, the
feature is **open** (`docs/open-questions.md`), not implemented.

Existing `FIT-*` code is legacy debt. It is listed in `docs/deviations.md` and
must be deleted or replaced by a read implementation. **No new `FIT-*` marker
may be added**; `cargo xtask verify` fails on one.

## Sources

- **Behaviour: `Overwatch.exe` only.** The complete current-build image in the
  Ghidra project: every `.text` page, the install's plaintext `.pdata`, base
  `0x140000000`. Read code there; record addresses and conclusions, never
  disassembly or decompiled code. See `docs/ghidra-project.md`.
- **Data: the operator's installation.** Read in place, never written to.
  `ow-casc` / `ow-tank` answer format, registry, geometry and value questions.
  See `docs/formats/`.
- **The current image is the only behaviour source.** No other image, no
  process dump, no previous build's addresses. The image is complete; there is
  no missing-code category here.
- **No external game code.** Public sources may be used to check a number, not
  to create one (`docs/provenance.md`).

## Hard rules, never break these

1. **1 to 1 or not at all** — see above.
2. **Never commit game assets, decompiled code, or extracted game files.**
   They stay on this machine, untracked. Extract game data only to a
   gitignored folder.
3. **`.gitignore` is a whitelist.** It ignores everything and includes only
   source files. Do not switch it to a normal ignore list.
4. **Do not run `git commit` unless asked.** Stage nothing beyond the current
   task.
5. **Do not touch anything outside this project folder** unless explicitly
   named. Read the install and the Ghidra project; never write to them.
6. **Never put credentials in the repo or in any readable file:** no API keys,
   tokens, or passwords.
7. **Docs record addresses and conclusions, never disassembly or decompiled
   code.** Write "function at `<address>` does X, because Y". `docs/provenance.md`
   defines the evidence classes.
8. **A stage that cannot be read is declared open, not faked.** Keep every
   other stage exact. An open stage has no gameplay implementation at all.
9. **Addresses are current-image addresses** — `0x140000000 + RVA`, or the
   function name from `docs/function-names.csv`. Do not introduce addresses
   from another build.
10. **The gate decides.** `cargo xtask verify` runs fmt, clippy (warnings are
    errors), the tests, the tool index, the deviation check and the
    address/function-entry check. *Suspended for now — see
    “TEMPORARY GATE SUSPENSION” below.*
11. **No seed lists for content.** A map, chunk set or texture is resolved by
    the client's own path (map header or loader), never by a proximity seed or
    a hand-picked GUID list. A seed that renders overlapping maps is a bug, not
    a map. If the path is not read, the content is open.
12. **Nothing is built on an invention (1 to 1).** If the stage a feature needs
    is a declared stand-in, the feature is open too. Do not extend a stand-in
    and do not add a read feature on top of one — replace or delete the stand-in
    first.
13. **Fix a wrong doc on sight.** When you are certain a recorded address,
    conclusion or state is wrong, correct it in the same change; a stale page
    costs the next session hours.

## TEMPORARY GATE SUSPENSION (operator, until further notice)

**Do not run the unscoped `cargo xtask verify`.** The operator has suspended
it: over an hour of wall clock, 98% of it in seven install suites
(`index_find_real` alone ~16 min), and the decision about that cost is
pending. Do not block a change on it, do not queue it "once at the end",
and do not re-add it to a checklist. This overrides rule 10 until the
operator lifts it.

Use these, and say in the handoff which one was run:

    cargo xtask verify --fast                              # fmt, doc checks, clippy, lib tests
    cargo xtask verify -p <crate> --skip index_find_real   # one crate, minus the slow suites
    cargo xtask verify --skip <long suite>                 # whole gate minus named suites

What still holds without the full gate: `--fast` and the scoped runs include
fmt, clippy with warnings as errors, the four python doc checks and the tests
of whatever was selected. A suite left out has **not** been run — say so
plainly ("`index_find_real` not run"), never imply a green full gate.

Delete this section when the operator lifts it.

## How to work

- **One thing at a time.** Every step should be revertable separately.
- **Read before writing.** If the client's code has not been read, the answer
  is "open", not a guess.
- **A zero read from the image is not a value.** `.data` declares
  `VirtualSize 0x0CC43BC` but only `0x0333600` raw bytes, so RVA
  `0x03B79600..0x0450A3BC` (**10,030,524 bytes**, 13.25% of the image) is a BSS
  tail: no bytes in the file, zeros mapped by the loader, filled by the client
  at startup (all 7,956 component-type descriptors are built there). A table or
  vtable slot in that range always reads zero - read the
  `RegisterComponentTypeDescriptor` (`0x1421cd360`) call site instead. Full
  measurements: `docs/ghidra-image-audit.md` section 2.1; short version:
  `docs/ghidra-project.md`. `python dev/pe_layout.py --addr <addr>` classifies
  an address (`raw` / `zero-fill` / `unmapped`); `dev/callers.py` and
  `dev/rip_refs.py` find the site that writes it. The disk `.text` is
  ciphertext, so only the recovered image is a code source.
- **Keep notes as you go.** Add the address and conclusion to the relevant
  `docs/formats/<system>.md` immediately.
- **Name what you understand.** When a function's role is settled, add a row
  to `docs/function-names.csv` and run `python dev/gh_apply_names.py`.
- **Log, don't look.** Instrument positions, counts, timings and state
  transitions to a log, then verify from the numbers. A windowed eye-check is
  the operator's job.
- **Tell the operator how to test.** After every change, give the exact
  command and what should be seen. "Done" without a test procedure is not done.
- **Ask before large refactors.** If the architecture is wrong, explain why
  and wait.
- **Explain in plain language.** The operator is not the programmer; explain a
  term the first time it is used.

## Verification and honesty

- `cargo xtask verify` is the one gate. `--fast` leaves out the tests that
  read the install.
- Bit-exactness is proven by golden vectors and first-divergence tests; a
  tolerance is not acceptance.
- If something is not tested, write **"not tested"**.
- If you are unsure, say so. A confident wrong answer costs hours.
- After two real attempts at a problem, stop and write up
  `STATUS-handoff.md` instead of trying random variations.

## Tooling map

| tool | what it answers | where |
|---|---|---|
| Ghidra project | behaviour: decompile, callers, value flow | `dev/ghidra_run/owmod_clean`, program `Overwatch.exe` (pre-clean backup: `dev/ghidra_run/owmod`) |
| `ow-tank` bins / examples | data: formats, registry, entities, weapons, graphs, animation | `crates/ow-tank/src/bin/`, `crates/ow-tank/examples/` |
| `ow-client`, `ow-scene` | the one launchable client and map assembly; default practice range through `mapresolve` | `crates/ow-client/`, `crates/ow-scene/` |
| `ow-sim` | the verified movement arithmetic: mover leaves, per-frame velocity law, jump and air law, the swept collision query, the wish fan solver; a workspace member, built and tested by the gate | `crates/ow-sim/` |
| `dev/verify_harness/` | differential proof that a Rust function equals the client's bytes | `dev/verify_harness/` |
| `dev/pe_layout.py` | is an address raw, zero-fill (BSS, runtime-only) or unmapped | `dev/pe_layout.py` |
| `docs/function-names.csv` | the name ↔ current address table | `docs/` |

## Keep these files updated

- `MODLOG.md`: one short entry per change; milestones only. It is a living
  ledger, not an archive: rotate it when it passes a few hundred lines by
  moving entries older than the current period, verbatim, to
  `docs/history/modlog-YYYY-MM.md` (one file per month). Never edit a moved
  entry; a correction is a new entry.
- `STATUS-handoff.md`: what works, what is open, what is next. When a section
  stops being live (the narrative behind a finished system, the per-unit
  evidence ledger), move it verbatim to `docs/history/status-<date>.md` and
  leave a pointer. The live per-system state belongs in
  `docs/systems-coverage.md`, not here.
- `README.md`: the public "what works / what does not" list.
- `docs/systems-coverage.md`: every client system, its state, its next anchor.
- `docs/formats/<system>.md`: findings, addresses, evidence class.
- `docs/deviations.md`: legacy non-1:1 code, to be removed.
- `docs/open-questions.md`: the work queue; next thing at the top.
- `dev/INDEX.md`: generated tool list; `python dev/make_index.py` refreshes it.

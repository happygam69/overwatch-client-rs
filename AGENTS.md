# AGENTS.md

## Project

This project reads the Overwatch install the operator already owns and
reimplements the parts of the client that matter for gameplay - movement,
combat, the view model and the camera - as a clean-room Rust simulation. It is
verified against the client by diffing recorded traces and by eye-checks, never
by shipping anything from the game.

Files give the data. Decompilation of the operator's own memory captures gives
the behaviour. A fitted implementation is always labelled as fitted.

## Hard rules, never break these

1. **Never commit game assets, decompiled code, or extracted game files.**
   They stay on this machine, untracked. Extract game data only to a gitignored
   folder.
2. **`.gitignore` is a whitelist.** It ignores everything and includes only
   source files. Do not switch it to a normal ignore list.
3. **Do not run `git commit` unless asked.** Stage nothing beyond the current
   task.
4. **Do not touch anything outside this project folder** unless explicitly
   named. This includes game installs: read them, never write to them.
5. **Never put credentials in the repo or in any readable file:** no API keys,
   tokens, or passwords.
6. **Docs record addresses and conclusions, never disassembly or decompiled
   code.** Write "function at `<address>` does X, because Y", name the capture
   and the build, so it can be re-derived. `docs/provenance.md` defines the
   evidence classes.
7. **Live tooling is read-only.** Open the client with
   `OpenProcess(VM_READ | VM_QUERY)` and use `ReadProcessMemory` only - no
   writes, no injection, no hooking. **A new live capture is the operator's
   decision every time.**
8. **Captures live outside the repo** (`D:/ow-dumps`, gitignored). They are
   session-specific: the same build maps at different base addresses, so every
   address names its capture and its translation. The one exception is the
   Ghidra project, which stays in `dev/ghidra_run/` on the SSD (Ghidra reads
   it hard) and is git-ignored; `docs/ghidra-project.md` says what is in it.
9. **Never silently mix fitted and read behaviour.** A fitted, assumed,
   stand-in or supplied value carries a `FIT-*` id next to its code *and* a row
   in `docs/fitted-ledger.md`; a read value cites its address.
   `cargo xtask fit-check` fails when the two sides disagree.
10. **A stage that cannot be recovered is declared, not faked.** Mark it tier 3,
    write the boundary down, and keep every other stage exact.

## Capture and behaviour workflow

- **Files answer data questions.** Use `ow-tank` bins, the typed STU layer and
  `docs/formats/` for formats, registries, geometry and values.
- **Ghidra MCP answers behaviour questions** over the captured images. The
  project is `dev/ghidra_run/owmod`: `Overwatch.exe` is the **complete**
  current-build image (all `.text` recovered offline, base `0x140000000`,
  carrying the current-build names), and `module_v2full.bin`
  is the old build the docs' addresses use; `docs/ghidra-project.md` lists
  every program, its base and what it answers. The merged images themselves
  stay in `D:/ow-dumps` (`module_after_diff.bin`, `loader_after_diff.bin`);
  the older `warm_best.bin` remains useful for comparison.
- **Preferred live capture: `dev/live_page_diff.py`.** Read-only; it selects
  `Overwatch.exe` by name (not the largest module), watches page protections,
  reads a page when it becomes readable, retries ciphertext pages, and logs
  accepted code plus `encrypted.csv`. Capture with the client in a practice
  range, first-person, with phase markers.
- **Fallback: `dev/warm_poll.py --all-regions`.** Same read-only rule. It only
  sweeps the game module and drops high-entropy pages, so it can miss pages
  that are decrypted lazily.
- **Cold pages are normal.** The on-disk `.text` is encrypted; the loader
  decrypts pages on demand. A function can be zero-filled or ciphertext in one
  capture and readable in another. Never conclude a function is missing from
  one cold capture.
- **Address algebra:** v2b base is `0x7ff7c4d20000`; file offset in a flattened
  v2b image is `v2b - 0x7ff7c4d20000`; a session address is
  `v2b + (session base - 0x7ff7c4d20000)`. Name the capture.

## How to work

- **One thing at a time.** Do not bundle unrelated changes; every step should
  be revertable separately.
- **Keep notes as you go.** When the shape of something is clear, write it into
  `docs/` immediately. Do not stop for a big plan after every reading.
- **Log, don't look.** You cannot see the game. Instrument positions, counts,
  timings and state transitions to a log, then verify from the numbers. A
  windowed eye-check is the operator's job.
- **Tell the operator how to test.** After every change, give the exact command
  and what should be seen. "Done" without a test procedure is not done.
- **Ask before large refactors.** If the architecture is wrong, explain why and
  wait.
- **Explain in plain language.** The operator is not the programmer; explain a
  term the first time it is used.
- **Behaviour questions go to the decompiler, not to guesswork.** Use Ghidra
  over the captures; use the file-side tools for data. The capture wins for
  behaviour, the install wins for data.
- **Record failures.** A dead end that is written down is worth more than one
  that has to be rediscovered.
- **Name what you understand.** When a function's role is settled, add a row
  to `docs/function-names.csv` and run `python dev/gh_apply_names.py`, so the
  decompiler and the docs say the same thing. A name is only for a function
  whose role is known.

## Verification and honesty

- If something is not tested, write **"not tested"**.
- If you are unsure, say so. A confident wrong answer costs hours.
- After two real attempts at a problem, stop and write up `STATUS-handoff.md`
  instead of trying random variations.
- **One gate decides whether a change is done:** `cargo xtask verify`.
  It runs fmt, clippy (warnings are errors), the tests, the tool index and the
  fitted-ledger check. `--fast` leaves out the tests that read the install.
- The eye-check is still the operator's: `cargo run --release -p ow-view --
  --play --hero 3 --anim auto`.
- Golden vectors and `trace_diff` decide whether a movement stage is exact.

## Tooling map

| Tool | What it answers | Where |
|---|---|---|
| Ghidra MCP bridge | behaviour: decompile, callers/callees, value flow, strings | captures in `D:/ow-dumps`; merged images above |
| `dev/live_page_diff.py` | which pages are actually readable/decrypted during a session | live, read-only; operator's run |
| `dev/warm_poll.py` | continuous warm-page accumulation | live, read-only; fallback |
| `dev/*.py` probes | capture scans, region extraction, PE wrapping, heap/trace work | `dev/INDEX.md` lists all of them |
| `ow-tank` bins / examples | data: formats, class registry, STU bags, entities, weapons, graphs, animation clips | `crates/ow-tank/src/bin/`, `crates/ow-tank/examples/` |
| `ow-sim` / `trace_diff` | movement/combat verification, first divergence | `crates/ow-sim/src/trace.rs` |
| `ow-view` / `ow-game` | the viewer, play harness and Bevy shell | `crates/ow-view`, `crates/ow-game` |

## Keep these files updated

- `MODLOG.md`: one short entry per change; history, not a diary.
- `STATUS-handoff.md`: the living state - what works, what is open, what is
  next.
- `README.md`: the public "what works / what does not" list and quick start.
- `docs/systems-coverage.md`: every client system, its state and next anchor.
- `docs/formats/<system>.md`: the findings, addresses and evidence class.
- `docs/fitted-ledger.md`: every fitted, assumed, tier-3 or stand-in
  implementation. Add the row in the same change that introduces the fit;
  remove it when a capture or decompile settles it.
- `docs/open-questions.md`: the work queue. The next thing to work on is at
  the top of it.
- `dev/INDEX.md`: the tool list, generated from the tools themselves.
  `python dev/make_index.py` refreshes it; the gate checks it is current.

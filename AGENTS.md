# AGENTS.md

## Project

A clean-room Rust reimplementation of the Overwatch client's **gameplay** —
movement, combat, the view model, the camera — with a Bevy shell and a `wgpu`
inspector. The operator's install supplies the data; the client's own code
supplies the behaviour. Nothing from the game ships here: no assets, no
extracted files, no decompiled code.

## The hard rule

**1 to 1 or not at all.**

A function is implemented only when all three hold:

1. the client's own code or value has been **read** from the complete
   `Overwatch.exe` Ghidra image;
2. the implementation follows it **operation for operation**; and
3. a test proves the result is **identical**.

There is no substitute for a read expression — no placeholder, no "close
enough", no measured replacement. If the client's expression is not read, the
function is **open** (`docs/open-questions.md`) and has no implementation at
all.

**100% is the target and the only acceptable state per function.** A function is
either done at its tier or open. "Mostly done", "works in practice", "covered by
neighbouring code" and "matches roughly" are not states. A tolerance is never
acceptance. Progress is counted per function by `python dev/queue.py`; that
number is the truth, not the prose in the docs.

**The proof is chosen by tier, never skipped** (`docs/tiers.md`):

| tier | covers | proved by | done is |
|---|---|---|---|
| **A – feel** | movement, camera, collision, weapons, abilities | the emulator ladder | `verified` |
| **B – translation** | formats, registries, tables, asset dispatch | the install's own data, exhaustively | `proved` |
| **C – structure** | frame loop, object model, anything behind a vtable | a recorded run vs first divergence | `linked` |

A tier decides *which instrument* proves the function and *how wide* the claim
is. It never decides whether the code was read or whether the result was
tested.

## Sources

- **Behaviour: `Overwatch.exe` only** — the complete current-build image in the
  Ghidra project, base `0x140000000`. Record addresses and conclusions, never
  disassembly or decompiled text. `docs/ghidra-project.md`.
- **Data: the operator's installation**, read in place, never written. `ow-casc`
  / `ow-tank` answer format, registry and value questions. `docs/formats/`.
- **No external game code.** A public source may confirm a number, never create
  one. `docs/provenance.md`.

## Hard rules

1. **Never commit game assets, decompiled code or extracted files.** Extract only
   into a gitignored folder.
2. **`.gitignore` is a whitelist** — it ignores everything and includes sources
   only. Never turn it into a normal ignore list.
3. **Commit only when the operator asks**, and stage nothing beyond the task.
4. **Touch nothing outside this project folder** unless explicitly named. The
   install and the Ghidra project are read-only.
5. **No credentials** anywhere in the repo or in any readable file.
6. **A stage that cannot be read is declared open, not faked** — and nothing is
   built on an invention. If a feature needs a declared stand-in, the feature is
   open too. `docs/deviations.md` lists the legacy stand-ins; **no new `FIT-*`
   marker may be added**.
7. **Addresses are current-image addresses** — `0x140000000 + RVA`, or a name
   from `docs/function-names.csv`. Never an address from another build.
8. **A zero read is not a value.** The `.data` tail from RVA `0x03B79600` to
   `0x0450A3BC` is BSS: zero in the file, filled at startup. Read the writer, or
   the registrar call site. `docs/ghidra-project.md` §Zeros;
   `python dev/pe_layout.py --addr <addr>` classifies any address.
9. **No seed lists for content.** A map, chunk set or texture is resolved by the
   client's own path, never by a proximity seed or a hand-picked GUID list.
10. **Fix a wrong doc on sight.** A stale page costs the next session hours.

## The gate

```sh
cargo xtask verify --fast        # fmt, doc checks, clippy, lib tests  ← every change
python dev/verify_changed.py     # only what the change can affect     ← every change
python dev/queue.py              # progress per tier, and --next
python dev/verify_harness/cli.py all   # the full ladder, nightly only
```

The unscoped `cargo xtask verify` is **suspended by the operator**: over an hour,
98% of it in seven install suites. Do not run it, do not queue it, do not
re-add it to a checklist. A suite left out has **not** been run — say so
plainly, never imply a green full gate.

## How to work

- **Read before writing.** No read, no implementation.
- **One revertable step at a time.** Ask before a large refactor.
- **Keep notes as you go.** Address and conclusion go into
  `docs/formats/<system>.md` immediately.
- **Name what you understand** in `docs/function-names.csv`; the Ghidra project
  takes names in batches (`python dev/gh_apply_names.py --pending`, ≥ 20 rows).
- **Log, don't look.** Instrument counts, positions and timings, then verify from
  the numbers. A windowed eye-check is the operator's job.
- **Say how to test it.** Every change ends with the exact command and what
  should be seen.
- **Plain language.** The operator is not the programmer; explain a term once.

## Honesty

- If something is not tested, write **"not tested"**.
- If you are unsure, say so. A confident wrong answer costs hours.
- After two real attempts, stop and write `STATUS-handoff.md` instead of trying
  variations.

## Keep updated

| file | what it holds |
|---|---|
| `MODLOG.md` | milestones, newest first; rotate to `docs/history/modlog-YYYY-MM.md` |
| `STATUS-handoff.md` | the loop a session runs; no state table |
| `README.md` | what works and what does not |
| `docs/systems-coverage.md` | every system: tier, state, next anchor |
| `docs/formats/<system>.md` | findings, addresses, evidence class |
| `docs/open-questions.md` | the work queue |
| `docs/tiers.md` | the three proofs and their done-states |
| `docs/tools.md` | **the** tool list, regenerated by `python dev/make_index.py` |

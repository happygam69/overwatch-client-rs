# Contributing

This is a clean-room gameplay reconstruction: every claim about the client is
written down with its evidence, and one command decides whether a change is
done. Read `AGENTS.md` first — it is the project's working contract and applies
to human contributors the same as to tooling.

## The gate

```sh
cargo xtask verify          # fmt, clippy (-D warnings), every test, dev index, deviations
cargo xtask verify --fast   # the same, without the tests that read an install
cargo xtask verify -p ow-casc   # the same for one crate (repeatable; iteration only)
cargo xtask verify --skip index_find_real   # leave the 935 s suite out
```

`-p` and `--skip` are for the edit-run loop; they scope clippy, the test
binaries and the doc tests, and they always print what they left out. `--skip
<name>` drops a whole test binary when the name matches its label (that is how
`--skip index_find_real` removes the 19-minute suite) and otherwise passes
libtest's own `--skip <name>` to every binary that still runs. A scoped run is
**not** the gate: a change is done when the unscoped `cargo xtask verify`
passes on a machine with the install.

A change is done when the gate passes **and** the docs that describe the
changed behaviour are updated in the same commit. CI runs the full gate
(`cargo xtask verify`); with no Overwatch install there, every test that reads
one skips itself and says so. Use `--fast` locally when you have no install
either; run the full gate on a machine that has one before you consider a
data-reading change finished.

## What this repository will never contain

No Blizzard content: no art, models, sounds, maps, textures, locale data,
extracted tables or decompiled code. Only original Rust source, Python
analysis tools and documentation. The `.gitignore` is a **whitelist**:
everything is ignored by default and tracked files are re-included by name, so
a new tracked file needs a matching `!` line there. Do not convert it to a
normal ignore list.

## How to work here

- **1 to 1 or not at all.** A gameplay feature is implemented only when the
  client's own code or value has been read from `Overwatch.exe` and a test
  proves the result identical. If it is not read, it is open.
- **No new deviations.** Existing `FIT-*` code is legacy debt listed in
  `docs/deviations.md`; `cargo xtask deviation-check` fails on a new id.
- **Behaviour questions go to the decompiler, data questions to the files.**
  `docs/provenance.md` defines the evidence classes (*read*, *install*,
  *verified*, *open*). Write current-image addresses; never paste disassembly
  or decompiled code into the repo.
- **Log, don't look.** Instrument, then verify from numbers. There is no
  client eye-check until the CASC/map oracle passes.
- **Tell the reviewer how to test.** "Done" without a test procedure is not
  done — name the exact command and what it should show.

## Where the work queue lives

`docs/open-questions.md` is the queue: the next thing to work on is at the top,
and each entry says what would settle it. `docs/systems-coverage.md` owns the
per-system state, and `STATUS-handoff.md` is the loop a session runs.

## Files every change keeps updated

- `MODLOG.md` — one short entry per change; milestones, not a diary.
- `docs/open-questions.md` — close or re-point entries your change settles.
- `docs/formats/<system>.md` — the findings, addresses and evidence class.
- `docs/tools.md` — the one tool list; its generated tables come from the
  tools' own headers (`python dev/make_index.py`), and the gate fails when
  they are stale.

## Asking questions

Open an issue and name the system you are looking at. If you can, quote the
doc row you are working from, so the answer can point at the exact evidence
instead of restating it.

## Licence

MIT or Apache-2.0, at your option. That covers this code only — not the game,
its data, or anything either of them produces.

# Contributing

This is a research reconstruction: every claim about the client is written
down with its evidence, and one command decides whether a change is done.
Read `AGENTS.md` first — it is the project's working contract and applies to
human contributors the same as to tooling.

## The gate

```sh
cargo xtask verify          # fmt, clippy (-D warnings), every test, dev index, fit ledger
cargo xtask verify --fast   # the same, without the tests that read an install
```

A change is done when the gate passes **and** the docs that describe the
changed behaviour are updated in the same commit. `--fast` is what CI runs
(CI has no Overwatch install); run the full gate once before you consider a
data-reading change finished.

## What this repository will never contain

No Blizzard content: no art, models, sounds, maps, textures, locale data,
extracted tables or decompiled code. Only original Rust source, Python
analysis tools and documentation. Captures and merged images live outside
the repo (`D:/ow-dumps`). The `.gitignore` is a **whitelist**: everything is
ignored by default and tracked files are re-included by name, so a new
tracked file needs a matching `!` line there. Do not convert it to a normal
ignore list.

## How to work here

- **One thing at a time.** Unrelated changes go in separate commits; every
  step should be revertable on its own.
- **Fitted values are labelled.** Anything fitted, assumed, stand-in or
  supplied carries a `FIT-*` id next to its code **and** a row in
  `docs/fitted-ledger.md`. `cargo xtask fit-check` fails when the two sides
  disagree — add the row in the same change that introduces the fit.
- **Behaviour questions go to the decompiler, data questions to the files.**
  `docs/provenance.md` defines the evidence classes (*read*, *measured*,
  *install*, *fitted*). Write addresses with their capture and build; never
  paste disassembly or decompiled code into the repo.
- **Log, don't look.** Instrument, then verify from numbers. The eye-check
  (`cargo run --release -p ow-view -- --play --hero 3 --anim auto`) is the
  operator's call.
- **Tell the reviewer how to test.** "Done" without a test procedure is not
  done — name the exact command and what it should show.

## Where the work queue lives

`docs/open-questions.md` is the queue: the next thing to work on is at the
top, and each entry says what would settle it. `docs/systems-coverage.md`
tracks every client system's state, and `STATUS-handoff.md` is the living
summary. If a plan doc and the code disagree, the code and its ledger row win
until a change makes them agree again.

## Files every change keeps updated

- `MODLOG.md` — one short entry per change; history, not a diary.
- `docs/open-questions.md` — close or re-point entries your change settles.
- `docs/formats/<system>.md` — the findings, addresses and evidence class.
- `dev/INDEX.md` — generated; `python dev/make_index.py` refreshes it, and
  the gate fails when it is stale.

## Asking questions

Open an issue and name the system you are looking at. If you can, quote the
ledger row or the format doc you are working from, so the answer can point at
the exact evidence instead of restating it.

## Licence

MIT or Apache-2.0, at your option. That covers this code only — not the
game, its data, or anything either of them produces.

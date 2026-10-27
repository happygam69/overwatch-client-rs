# dev/ - the analysis tools

Everything here is **read-only** against the game: the tools open the install
and the Ghidra project, never write to the game.

- **[INDEX.md](INDEX.md)** — every tool and the question it answers. Generated
  by `python make_index.py`; `--check` fails when it is stale.
- **[../docs/tools.md](../docs/tools.md)** — the tool table by task, with the
  probes that live in the Rust crates.
- **[verify_harness/PERFORMANCE.md](verify_harness/PERFORMANCE.md)** — the
  harness's measured wall time, where it goes, and the quality proof for the
  parallel mutation stage (`--jobs`, `--mutation-timeout`).
- **[../docs/ghidra-project.md](../docs/ghidra-project.md)** — the
  `Overwatch.exe` program and the address rule.

## What is tracked

The scripts are original work and are tracked, so a re-derivation survives a
lost machine. What they *produce* is not tracked: decompiles, hex dumps and
multi-MB outputs live outside the repository.

Nothing that came out of the game is tracked: no disassembly, no extracted
strings, no raw asset bytes. A tool that writes those writes them outside the
repository.

## Housekeeping rules

1. A tool stays only if `INDEX.md` lists it and it still answers a question
   someone will ask again. One-off probes whose answer is already in
   `docs/formats/` are deleted, not parked.
2. A tool that is a superset of another is deleted, and the surviving one's
   header says what it replaced.
3. Every tool's header says what question it answers, in one line. That line
   is the index entry.
4. Outputs larger than about 1 MB go outside the repo.
5. The tools must run from this folder (`python dev/<tool>.py`).

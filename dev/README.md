# dev/ - the analysis tools

Everything here is **read-only** against the game: the tools open the install
and the Ghidra project, never write to the game. `patch_clean_pe.py` writes a
patched analysis copy under the git-ignored `dev/data/offline_recover/`; it
never writes to the install.

- **[../docs/tools.md](../docs/tools.md)** — the one tool list: every script in
  this folder, the harness, the data probes, the client and the Ghidra scripts,
  each with the question it answers. Its tables are generated from the tools'
  own headers by `python dev/make_index.py`; `--check` fails when they are
  stale.
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

The housekeeping rules for this folder are in
[`../docs/tools.md#housekeeping-rules`](../docs/tools.md#housekeeping-rules):
a tool stays only while the list names it and it still answers a question,
a superset of another tool is deleted, every header is its table entry, large
outputs stay out of the repo, and tools run from the repository root.

# dev/ - the analysis tools

Everything here is **read-only** against the game: the tools open the install
and the captures, never write to the game, never attach with write rights. A
new capture is the operator's decision every time.

- **[INDEX.md](INDEX.md)** - every tool and the question it answers. Generated
  by `python make_index.py`; `--check` fails when it is stale.
- **[../docs/tools.md](../docs/tools.md)** - the tool table by task, with the
  probes that live in the Rust crates.
- **[../docs/artifacts.md](../docs/artifacts.md)** - what lives on `D:/ow-dumps`.

## What is tracked

The scripts are original work and are tracked, so a re-derivation survives a
lost machine. What they *produce* is not tracked: captures, merged images,
decompiles, hex dumps and multi-MB outputs live under `D:/ow-dumps`.

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
4. Outputs larger than about 1 MB go to `D:/ow-dumps/devdata/` and are
   indexed in `docs/artifacts.md`.
5. The tools must run from this folder (`python dev/<tool>.py`), and they
   resolve their capture directory through `regions.capture_dir`, so a capture
   on `D:` is found without editing the script.

## Capture workflow (short)

1. `python dev/live_page_diff.py ...` - read-only attach, writes a run dir.
2. `python dev/session_merge.py <run>` - one coherent image for pointer work.
3. `python dev/warm_merge.py ...` - union pages into the standing image.
4. `python dev/coverage_gap.py <image>` - what stayed cold, attributed to
   `.pdata` function ranges.
5. Ghidra headless over the stripped image (see `docs/tools.md`).

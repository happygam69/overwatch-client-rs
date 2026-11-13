# Handoff

What works, what is open, and the loop that moves it forward. The state has
one owner and this page does not copy it:

| question | file |
|---|---|
| the state of each system | [`docs/systems-coverage.md`](docs/systems-coverage.md) |
| the next thing to do | [`docs/open-questions.md`](docs/open-questions.md) |
| which tool answers which question | [`docs/tools.md`](docs/tools.md) |
| the findings, with addresses | [`docs/formats/`](docs/formats/) |
| how the project got here | [`MODLOG.md`](MODLOG.md) and [`docs/history/`](docs/history/) |

The session narratives, the measured state, the five walls and the candidate
list that used to sit here are archived verbatim in
[`docs/history/status-handoff-2026-11-20.md`](docs/history/status-handoff-2026-11-20.md);
their numbers are frozen at their session.

## How a session works here

One thing at a time, one revertable step each. Every step is proved at the
**tier** of the function it ports, never at a tier of one's choosing:
[`docs/tiers.md`](docs/tiers.md) defines the three tiers and what "done" is
called at each, [`docs/port-playbook.md`](docs/port-playbook.md) gives the
procedure.

The loop:

1. **Choose.** `python dev/queue.py --next` prints the next function to port,
   screened; `--next N` takes a batch of N. The bare command is the progress
   number per tier, and `--assign-tiers` writes the system → tier table.
2. **Read.** Read the client's function in the current image (or, at tier B,
   the client's rule over the install's data) and put the address and the
   conclusion in `docs/formats/<system>.md`. A stage that cannot be read is
   declared open; it is never faked and nothing is built on top of it.
3. **Implement** it operation for operation.
4. **Prove it at its tier.**
   - **A, feel** (movement, camera, collision, weapons, abilities): the
     emulator ladder — run `cli.py fn/<Name>`; done is `verified`.
   - **B, translation** (formats, registries, tables, asset dispatch): the
     repo's `crates/<crate>/tests/<name>_real.rs`, exact over the operator's
     own install data, exhaustively; done is `proved`.
   - **C, structure** (frame loop, object model, anything behind a vtable): a
     recorded run compared to the first divergence; done is `linked`.
5. **Fast loop.** `python dev/verify_changed.py` re-proves only what your change
   can affect (~30 s): the tier-A units whose crate you touched and that
   crate's tier-B proofs. `--dry-run` shows the plan, `--refs HEAD~1` takes a
   commit range, `--proofs` takes every tier-B proof (slow ones too), `--slow`
   includes the minute-long ones.
6. **Gate.** `cargo xtask verify --fast` — fmt, clippy (warnings are errors),
   the doc checks and the tests that need no install. A scoped run (`-p
   <crate> --skip <slow suite>`) when the crate is the scope. The unscoped
   gate is suspended (see `AGENTS.md`), so never queue it "once at the end".
   `python dev/verify_harness/cli.py all` is the **nightly** full ladder, not a
   per-change step.
7. **Record.** A `MODLOG.md` entry, plus whatever moved in
   `docs/systems-coverage.md` and the crate's own `1to1` page.

### The tier comes from the system, not from preference

A function's tier is derived mechanically from its system tag
(`python dev/queue.py --assign-tiers` writes the column; the rule and the tag
→ tier mapping are in [`docs/tiers.md`](docs/tiers.md)). A: `mover`, `camera`,
`collision`, `weapon`, `ability`, `hero`, `input`, `physics`, `statescript`,
`domino`. B: `asset`, `casc`, `stu`, `bank`, `map`, `effect`, `sound`,
`texture`, `anim`, `registry`, `locale`, `model`, `material`, `chunk`. C:
`frame`, `world`, `entity`, `render`, `net`, `engine`. No system tag means
`unclassified`, and unclassified is not done at any tier. A tier is a claim
about *what proves the function*; if it is wrong, move the row and say why in
the MODLOG.

### `proved` is a done state at tier B

A tier-B function with a green `*_real.rs` proof is **`proved`**, and `proved`
is done. It must not be listed as unfinished merely because it has no emulator
unit: at tier B the instrument is the install's own data, and the claim is
"exact over what ships", not "operation by operation". A tier-B test that
samples is a smoke test and must say so; an exhaustive one is the proof.

### Which command for which job

| command | job | when |
|---|---|---|
| `python dev/queue.py --next` | the next function, and progress per tier | choosing |
| `python dev/verify_changed.py` | prove only what the change can affect (~30 s) | every change |
| `cargo xtask verify --fast` | fmt, clippy, doc checks, install-free tests | before commit |
| `python dev/verify_harness/cli.py all` | everything, from cold (~an hour) | nightly |

## Work in batches, not one unit at a time

The loop for a batch of units:

1. Take 6-8 leaves from `python dev/queue.py --next 8` at once (a per-crate
   view is still `python dev/crate_coverage.py <crate> --queue`). A delegate per
   function: read the body, transcribe to Rust, then write that tier's proof —
   the fixture and unit JSON for tier A, the `*_real.rs` test for tier B.
   Delegates do **not** run the harness.
2. The main session owns the two shared files (`verify_probe.rs` arms,
   `candidate_rust.py` rows) and runs `cli.py fn/<Id>` per tier-A unit as
   results arrive (20-60 s each); a tier-B proof is just its `_real` test.
3. One `cli.py casc` at the end of the batch, detached to a log. Never
   `cli.py all` for progress: stage 5 rebuilds the crate per mutant, so a full
   run is about an hour and it is the nightly.
4. `python dev/verify_changed.py`, then `cargo xtask verify --fast`, per batch.
5. An entry blocked by an IAT import (allocator or the client mutex
   `0x142694c20`/`0x142694c50`) is written `open` with its address and one line
   of why, and the batch moves on.

### IMPORTANT: no two agents in the same checkout at once

Sub-agents share the working tree and the harness binds verdicts to the
working-tree contents, so **never run more than one in parallel**. Do them
one at a time.

- The sub-agent writes: crate source, then that tier's proof (probe arm,
  `candidate_rust.py`, fixture, unit JSON at tier A; the `*_real.rs` test at
  tier B).
- It does **not** run `cli.py` for other units; it may run its own unit
  (and `cargo build --release --bin verify_probe -p ow-casc`), or its own
  `cargo test --release --test <name>_real -p <crate>` at tier B.
- The main session (me) runs `cli.py casc`, then `cargo xtask verify --fast`
  (or a scoped run) and the docs, and re-dispatches a fixer if a unit fails.
- Updates: the crate's `docs/formats/<system>.md`, `docs/<crate>-1to1.md`
  (the verified table), `docs/systems-coverage.md`,
  `docs/open-questions.md`, `STATUS-handoff.md`, `MODLOG.md`;
  `python dev/make_index.py` after adding tools; `python dev/crate_coverage.py
  <crate>` to see the count move.

### Per-unit loop (main session)
1. Pick next from the queue: `python dev/queue.py --next` (proved callees
   first).
2. `acp_delegate` a `worker` with a self-contained task: the address, the
   playbook path, the crate file, the tier and its required files, and the
   rule "if the tier's proof cannot be written, say which address blocks it
   and put the read in the docs instead".
   **Watch for**: the agent's result file arriving as a completion
   notification — fetch with `acp_delegate_wait(runId)`; a failed run keeps
   its output file and can be resumed with `resumeFrom`.
3. When it returns, prove it: tier A runs `python dev/verify_harness/cli.py
   fn/<Name>` (my own run, ~2–5 min with a warm probe); tier B runs its
   `_real` test; tier C waits for a recorded run.
4. If it fails: `acp_delegate` a `reviewer`/`worker` with the exact failure
   text (stage, divergence, branch site, survivor mutant, and the playbook
   §8 symptom table), fix, re-run.
5. If it passes: `python dev/verify_changed.py`, then docs + count + next.

### Standing rules
- Never commit (AGENTS.md hard rule 4). Never stage beyond the task.
- No new `FIT-*` markers (`cargo xtask verify` fails on them).
- `.gitignore` is a whitelist.
- Docs record addresses + conclusions, never disassembly/decompiled code.
- "not tested" if not tested. If two attempts fail, write STATUS-handoff.md.

The project's hard rules are [`AGENTS.md`](AGENTS.md); this page does not
restate them.

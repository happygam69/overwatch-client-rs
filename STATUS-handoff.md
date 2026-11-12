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

One thing at a time, one revertable step each: read the client's function,
transcribe it operation for operation, and prove it with a five-stage
differential unit ([`docs/port-playbook.md`](docs/port-playbook.md)).

## Work in batches, not one unit at a time

The loop for a batch of units:

1. Take 6-8 leaves from `python dev/crate_coverage.py <crate> --queue` at once. A
   delegate per function: read the body, transcribe to Rust, write the
   fixture, write the unit JSON. Delegates do **not** run the harness.
2. The main session owns the two shared files (`verify_probe.rs` arms,
   `candidate_rust.py` rows) and runs `cli.py fn/<Id>` per unit as results
   arrive (20-60 s each).
3. One `cli.py casc` at the end of the batch, detached to a log. Never
   `cli.py all` for progress: stage 5 rebuilds the crate per mutant, so a full
   run is about an hour.
4. `cargo xtask verify --fast` per batch; a scoped run (`-p <crate> --skip
   <slow suite>`) when the crate is the scope. The unscoped gate is suspended
   (see `AGENTS.md`), so never queue it "once at the end".
5. An entry blocked by an IAT import (allocator or the client mutex
   `0x142694c20`/`0x142694c50`) is written `open` with its address and one line
   of why, and the batch moves on.

### IMPORTANT: no two agents in the same checkout at once

Sub-agents share the working tree and the harness binds verdicts to the
working-tree contents, so **never run more than one in parallel**. Do them
one at a time.

- The sub-agent writes: crate source, probe arm, `candidate_rust.py`,
  fixture, unit JSON.
- It does **not** run `cli.py` for other units; it may run its own unit
  (and `cargo build --release --bin verify_probe -p ow-casc`).
- The main session (me) runs `cli.py casc`, then `cargo xtask verify --fast`
  (or a scoped run) and the docs, and re-dispatches a fixer if a unit fails.
- Updates: the crate's `docs/formats/<system>.md`, `docs/<crate>-1to1.md`
  (the verified table), `docs/systems-coverage.md`,
  `docs/open-questions.md`, `STATUS-handoff.md`, `MODLOG.md`;
  `python dev/make_index.py` after adding tools; `python dev/crate_coverage.py
  <crate>` to see the count move.

### Per-unit loop (main session)
1. Pick next from the queue (proved callees first).
2. `acp_delegate` a `worker` with a self-contained task: the address, the
   playbook path, the crate file, the five required files, and the rule
   "if it cannot be a five-stage unit, say which address blocks it and put
   the read in the docs instead".
   **Watch for**: the agent's result file arriving as a completion
   notification — fetch with `acp_delegate_wait(runId)`; a failed run keeps
   its output file and can be resumed with `resumeFrom`.
3. When it returns, run `python dev/verify_harness/cli.py fn/<Name>`
   (my own run, ~2–5 min with a warm probe).
4. If it fails: `acp_delegate` a `reviewer`/`worker` with the exact failure
   text (stage, divergence, branch site, survivor mutant, and the playbook
   §8 symptom table), fix, re-run.
5. If it passes: docs + count + next.

### Standing rules
- Never commit (AGENTS.md hard rule 4). Never stage beyond the task.
- No new `FIT-*` markers (`cargo xtask verify` fails on them).
- `.gitignore` is a whitelist.
- Docs record addresses + conclusions, never disassembly/decompiled code.
- "not tested" if not tested. If two attempts fail, write STATUS-handoff.md.

The project's hard rules are [`AGENTS.md`](AGENTS.md); this page does not
restate them.

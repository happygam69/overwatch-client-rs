# Roadmap: the 1-to-1 gameplay rewrite

## Goal

One Rust client that loads the real practice range from the operator's
installation and reproduces the Overwatch client's gameplay - movement,
combat, the view model and the camera - from the current `Overwatch.exe`.

## The rule

**1 to 1 or not implemented.**

A gameplay feature counts only when:

1. the client's own code or value has been read from `Overwatch.exe`;
2. the Rust implementation follows it operation for operation; and
3. a test proves the result identical.

There is no measured substitute and no fallback. An unread stage is **open**
and has no gameplay implementation. Legacy approximations carry a `FIT-*`
marker in `docs/deviations.md`; no new one may be added.

## What "1 to 1" means

Identity is **state-trajectory identity**, not identical machine code:

- **Pure stage:** one ramp step, integration step, shot or pose evaluation
  produces the same f32 bits.
- **Timeline:** the same input sequence produces the same per-tick state, with
  the first divergence reported.
- **Function:** a reconstructed function passes the differential harness
  against the client's own bytes.

A tolerance is not acceptance.

## Current position

There is one copy of the state: [`systems-coverage.md`](systems-coverage.md),
system by system, with the next anchor for each. The queue is
[`open-questions.md`](open-questions.md) (next thing at the top), the handoff
loop is [`../STATUS-handoff.md`](../STATUS-handoff.md), and the stage order and
gates are [`rebuild-plan.md`](rebuild-plan.md). This page keeps the goal and
what 1-to-1 means; it does not repeat the state.

## Scope

- **Client-side only.** The client's own prediction and local simulation are
  the target. Server-authoritative resolution is not in the image and is not
  claimed.
- **Rendering is not the claim.** The viewer displays the data faithfully
  enough to inspect it; pixel identity with the client's proprietary shaders
  is not a goal.
- **A new build re-bases addresses, not method.** Re-read the current image;
  never carry an old address frame forward.

## Verification

```sh
cargo xtask verify                    # the one gate
cargo xtask verify --fast              # without install-reading tests
python dev/verify_harness/cli.py casc  # CASC oracle
cargo run --release -p ow-client       # practice-range eye-check
```

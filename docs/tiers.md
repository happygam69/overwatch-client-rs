# Proof tiers: what "done" means for a function

Rule 1 already says the proof is *"bit-exact for state, exact for values,
first-divergence for a trace"*. It never said which instrument produces that
proof, and the gate quietly narrowed it to one: an x86 emulator. That is right
for the functions the player feels and wrong for the rest, and the difference is
the whole schedule.

This page defines the three tiers, the instrument each one uses, and what
"done" is called. `docs/port-playbook.md` gives the procedure per tier.

## The tiers

| tier | the question the function answers | instrument | done is called |
|---|---|---|---|
| **A - feel** | *does the player feel it?* movement, camera, collision, weapon mechanics, ability state machines, mover state evaluation | the emulator ladder (`dev/verify_harness`): the client's own bytes against the port, same inputs, every channel compared | `verified` |
| **B - translation** | *does the same bytes produce the same result?* format decoding, registry lookup, table construction, asset dispatch | **the install's own data**: the port and the client's rule are applied to the real assets, and the outputs are compared as digests (or field for field) | `proved` |
| **C - structure** | *does the same event produce the same state?* the frame loop, the object model, anything reached through a vtable | a recorded run: the client's trace and the port's trace, compared to the **first divergence** | `linked` |

Tier A is what this project has been doing for 115 verified units (120 units
exist; the rest are `divergent`, `inconclusive` or `invalid`). Tier B is most of
`ow-tank`, already read and already exercised on the install, previously written
down as "no harness unit". Tier C is the 1,769 vtable clusters plus the 2,276
"no index entry" ones that resolve to a vtable - the emulator cannot reach them
at all, so no amount of fixture work makes them tier A.

## How a function gets its tier

Mechanical, by the system it belongs to (`triage.systems` in the corpus DB), not
by preference. Nothing is stored: `python dev/queue.py --assign-tiers` *derives
and prints* the table below, so the doc and the tool cannot drift apart - if you
change the rule, change it in `dev/queue.py` and this block together.

- **A**: `mover`, `camera`, `collision`, `weapon`, `ability`, `hero`, `input`,
  `physics`, `statescript`, `domino`.
- **B**: any system whose product is *data* - `asset`, `casc`, `stu`, `bank`,
  `map`, `effect`, `sound`, `texture`, `anim`, `registry`, `locale`, `model`,
  `material`, `chunk`.
- **C**: `frame`, `world`, `entity`, `render`, `net`, `engine`, and anything
  already screened as blocked only by a vtable or an import into the engine's
  own object model.

A function with no system tag takes the tier of the crate whose unit or proof
cites it (`ow-sim`/`ow-engine` A, `ow-tank`/`ow-casc`/`ow-stu` B,
`ow-client`/`ow-scene`/`ow-render` C). Failing that it stays `unclassified`,
and **unclassified is never counted as done** - that default is what keeps a
crate-less control unit from promoting data code to the emulator tier.

## What each tier promises, and what it does not

**A - verified.** The client's bytes and the port execute the same inputs and
agree on every channel, both edges of every reachable branch, and injected
mutants are killed. A tier-B proof never credits a tier-A function: a data test
that happens to walk through a camera formula does not prove the camera. This is
the only tier that proves *operation-by-operation*
equality. Cost: minutes per unit, so it is reserved for what the player feels.

**B - proved.** The port and the client agree on **the client's real data**: the
install's own assets, all of them, with the outputs compared. The claim is exact
for values and complete over the corpus, not a sample. It does *not* claim
operation equality - a table-built value and a written constant prove identical
here. That is the honest trade: format code is a translator, and a translator is
judged by its output.

A tier-B proof must be **exhaustive over what ships**, not over a fixture:
`bank_real` opens all 1,045 readable banks, `mapchunk_kinds_real` walks all
9,076 light chunks. A tier-B proof that samples is not a proof, it is a smoke test, and
the file must say which it is.

**C - linked.** The port and the client agree on state up to the first
divergence of a recorded run. The recording is the evidence, and it lives
outside the repo (the operator's own capture). What C gives up is exhaustive
per-function equality; what it keeps is that any behavioural difference shows up
at a known event with a known cause.

## The gate and the loop

Three commands, three jobs. Before this page existed, they were one command and
it took an hour.

```sh
python dev/verify_changed.py    # the loop: only what your change can affect
python dev/queue.py             # progress per tier, and what to do next
python dev/verify_harness/cli.py all   # nightly, everything, from cold
```

`cargo xtask verify --fast` is still the pre-commit gate - fmt, clippy, doc
checks, the tests that need no install. It does not run the ladder.

### Measured, 2026-11-20

```text
python dev/verify_changed.py --proofs   39 of 40 tier-B proofs green in 126 s
                                        (index_find_real needs --slow: minutes)
python dev/queue.py                     the progress table, per tier
                                        tier C 0 (no recorded run yet)
                                        115,927 image roots carry no system tag
```

Two honest notes on those numbers. Tier B counts **only proof-declared
functions** (`//! proof-covers:`): 14 of the 40 proofs are green but declare
nothing, because they assert install layout rather than a client function; and a
proof that finds no install records `skipped-no-install`, never `passed`. The
per-tier totals are whatever `python dev/queue.py` prints - that table, not this
sentence, is the number.

## Why this is not a loosening

Rule 1 needs the client's own expression to be **read**, and the result to be
**proved identical**. Both hold at every tier: the tiers differ in the
*instrument* and in the *scope of the claim*, never in whether the code was read
or in whether the result was tested. A function with no read expression is open
at every tier.

What the tiers fix is a scheduling error: treating a codec like a camera makes
the camera take a week, and treating a camera like a codec ships a game that
feels wrong. `docs/provenance.md` still defines the evidence classes; a tier
says which class the work in front of you must reach.

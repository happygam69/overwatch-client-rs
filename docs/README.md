# Project docs: read this first

Clean-room Rust reimplementation of the parts of the Overwatch client that
matter for gameplay, reconstructed only from the operator's own install and
read-only memory captures. Nothing from the game ships in this repository.

## Where to start

| If you want to... | Read |
|---|---|
| know what the pieces are | [`architecture.md`](architecture.md) |
| look up a term (STU, RVA, tier 3, ...) | [`glossary.md`](glossary.md) |
| see every client system and its state | [`systems-coverage.md`](systems-coverage.md) |
| know what is fitted, assumed or tier 3 | [`fitted-ledger.md`](fitted-ledger.md) |
| know how evidence is classified | [`provenance.md`](provenance.md) |
| find a file on `D:/ow-dumps` | [`artifacts.md`](artifacts.md) |
| know how a subsystem works today | [`formats/`](formats/) (one file per system) |
| understand a specific decision's history | [`history/`](history/) |
| see the current state and next steps | [`../STATUS-handoff.md`](../STATUS-handoff.md) |
| see how the project got here | [`../MODLOG.md`](../MODLOG.md) |

Plans and process docs (different scopes, on purpose):

| doc | scope |
|---|---|
| [`plan-understanding.md`](plan-understanding.md) | this cleanup/organisation plan |
| [`roadmap.md`](roadmap.md) | whole-project rewrite goal and accounting |
| [`rewrite-plan.md`](rewrite-plan.md) | the movement/combat bit-exact plan |
| [`systems-plan.md`](systems-plan.md) | the systems-extraction sweep (engine index) |

Reading orders by task:

- **Animation / view model**: `architecture.md` -> `formats/animation.md`
  (current section) -> `fitted-ledger.md` -> `history/animation.md`.
- **Movement / combat**: `formats/character-controller.md`,
  `formats/domino.md`, `formats/weapons.md` -> `ow-sim` tests.
- **Camera**: `formats/input-camera-audio.md` -> `formats/animation.md`
  (first-person section) -> the FP camera ledger rows.
- **Data formats**: `formats/casc.md`, `formats/stu.md`, `formats/mstu.md`,
  `formats/materials.md`.

## Conventions used everywhere

- **Evidence classes** (`provenance.md`): *read* (a decompiled path or data
  value with a recorded address), *measured* (numbers from a trace or an
  oracle), *install* (a value read from the shipped files), *fitted* /
  *assumed* / *stand-in* (chosen, not read). A fitted value is always in
  `fitted-ledger.md`.
- **Tier 3**: a client stage declared unrecoverable (`rewrite-plan.md`);
  the simulation uses a measured replacement and says so.
- **Addresses**: every address names its image and its conversion. `v2b` is
  the standing image space; a session address is
  `v2b + (session base - 0x7ff7c4d20000)`. See `architecture.md`.
- **Not tested** is written down where it is true.

## Quick start

```text
cargo build --release                      # everything
cargo test --release -p ow-casc -p ow-tank # data layer
cargo test -p ow-view                      # viewer / harness
cargo run --release -p ow-view -- --play --hero 3 --anim auto
```

The install is detected automatically (`OW_INSTALL` can point at the folder
containing `.build.info`). Captures and decompiled artifacts live outside
the repository (`D:/ow-dumps`, `E:/ow-dumps`) and are indexed in
[`artifacts.md`](artifacts.md).

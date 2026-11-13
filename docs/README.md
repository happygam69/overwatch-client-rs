# Project docs

A clean-room Rust reimplementation of the Overwatch client's gameplay:
movement, combat, the view model and the camera. Data comes from the
operator's install; behaviour comes from the complete current-build
`Overwatch.exe` Ghidra image. The rule is **1 to 1 or not implemented**.

## Start here

| If you want to... | Read |
|---|---|
| know the goal and the phases | [`roadmap.md`](roadmap.md) |
| **know the rebuild order** | **[`rebuild-plan.md`](rebuild-plan.md)** |
| see every system and its state | [`systems-coverage.md`](systems-coverage.md) |
| **pick the next thing to work on** | **[`open-questions.md`](open-questions.md)** |
| **implement movement** | **[`movement-plan.md`](movement-plan.md)** |
| know how evidence is classified | [`provenance.md`](provenance.md) |
| work in the decompiler | [`ghidra-project.md`](ghidra-project.md) |
| read how the clean Ghidra image was built (done) | [`ghidra-migration.md`](ghidra-migration.md) |
| find a function's current address | [`function-names.csv`](function-names.csv) |
| know what is not 1 to 1 yet | [`deviations.md`](deviations.md), [`ow-tank-1to1.md`](ow-tank-1to1.md), [`casc-1to1.md`](casc-1to1.md), [`texture-1to1.md`](texture-1to1.md) |
| know the port scope and the unit vocabulary | [`port-plan.md`](port-plan.md) |
| run the port procedure | [`port-playbook.md`](port-playbook.md) |
- [`tiers.md`](tiers.md) - the three proof tiers and what done means for each
| read the `ow-tank` defect audit | [`audit-ow-tank.md`](audit-ow-tank.md) |
| know the crates and data flow | [`architecture.md`](architecture.md) |
| look up a term | [`glossary.md`](glossary.md) |
| know which tool answers a question | [`tools.md`](tools.md) — every tool, and the question it answers |
| read the findings for a system | [`formats/`](formats/) |
| see the current state | [`systems-coverage.md`](systems-coverage.md) |
| run a session by the house loop | [`../STATUS-handoff.md`](../STATUS-handoff.md) |
| read the old long-form status (archived) | [`history/status-in-depth-2026-11-12.md`](history/status-in-depth-2026-11-12.md) |
| read a rotated ledger or snapshot | [`history/`](history/) |
| see how the project got here | [`../MODLOG.md`](../MODLOG.md) |

## Reading orders

- **Movement:** `formats/character-controller.md` → `formats/domino.md` →
  `movement-plan.md` → `docs/open-questions.md`.
- **Combat:** `formats/weapons.md` → `docs/open-questions.md` (fire/damage is
  open).
- **Camera:** `formats/camera.md` → `formats/animation.md` (first-person) →
  `crates/ow-client/src/main.rs`.
- **Map/client:** `formats/maps.md` -> `formats/model.md` ->
  `formats/materials.md` -> `crates/ow-scene/src/lib.rs` ->
  `crates/ow-client/src/main.rs`.
- **View model:** `formats/animation.md` → `crates/ow-tank/src/blendtree.rs`.
- **Data:** `formats/casc.md` → `formats/stu.md` → `formats/materials.md`.

## Conventions

- **Evidence classes** (`provenance.md`): *read* (a decompiled path or a data
  value with a recorded address in `Overwatch.exe`), *install* (a value read
  from the shipped files), *verified* (a Rust implementation proven identical
  to the client), *open* (not read; no implementation).
- **Addresses** are current-image addresses: `0x140000000 + RVA`, or a
  function name from `function-names.csv`. There are no other address frames.
- **A deviation** is legacy code that is not yet 1 to 1. It is listed in
  `deviations.md`, carries a `FIT-*` marker, and is removed when replaced.
- **Not tested** is written down where it is true.

## Quick start

```text
cargo build --release
cargo test --release -p ow-casc -p ow-stu -p ow-tank -p ow-scene
cargo xtask verify
cargo run --release -p ow-client          # the practice range
```

The install is detected automatically (`OW_INSTALL` can point at the folder
containing `.build.info`).

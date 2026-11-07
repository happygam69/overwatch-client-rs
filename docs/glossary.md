# Glossary

Plain-language definitions for terms used in the docs and code. When a term
has a precise meaning here, that meaning is given once.

## Project

- **Client** — the shipped Overwatch client, as represented by the complete
  `Overwatch.exe` Ghidra image.
- **Install** — the operator's own Overwatch installation. It is read in
  place and never written to.
- **Current image** — the `Overwatch.exe` Ghidra program: base `0x140000000`,
  every `.text` page and the install's plaintext `.pdata`; the function census
  is in [`ghidra-project.md`](ghidra-project.md).
- **RVA** — an offset from the image base. In the current image an address is
  `0x140000000 + RVA`.
- **1 to 1** — the Rust implementation follows the client's own code or value
  operation for operation and is proven identical. Anything else is open.
- **Deviation** — legacy code that is not yet 1 to 1. It carries a `FIT-*`
  marker and a row in `docs/deviations.md`; it is not a target.
- **Open** — not read. An open gameplay stage has no implementation.

## Evidence

- **Read** — a decompiled path or a data value with an address in
  `Overwatch.exe`.
- **Install** — a value or layout read from the shipped files.
- **Verified** — a Rust reconstruction proven identical to the client's bytes
  by the differential harness.
- **Not tested** — written exactly as that, where it is true.

## Game data

- **CASC** — Blizzard's content store: content-addressed archives plus an
  index that maps keys to bytes. `ow-casc` resolves keys to bytes and knows
  nothing about game semantics.
- **GUID / asset ref** — an 8-byte asset identifier. Its top bits carry the
  asset *type* (`guid::type_of`); models are type `0x00C`, and each other
  asset family has its own type id.
- **STU** — "structured data": Overwatch's gameplay data format. A file is a
  set of typed instances; each instance sits in a shared buffer with a layout
  decoded from a class registry.
- **Class hash / field hash** — 32-bit identifiers for an STU class and its
  fields. Field names like `m_63DEDAC0` are the client's own generated names;
  many fields are `Unknown` in the game's registry.
- **Chunk** — a named block inside a model/asset file (`mskl` = skeleton,
  `MSTU` = model STU, `MDLC`/`MRNM` = mesh data, `CLTH` = cloth,
  `MOCL` = model collision, hardpoints live in `MSTU`).
- **View model (FP model)** — the separate entity/model drawn in first
  person. It is a subset rig of the character model with an identical bind
  pose.

## Gameplay

- **Mover** — the character controller component (`STUCharacterMoverComponent`,
  hash `0x6EFDBD73`, id `0x26`, size `0x130`) and the 0x1c00-byte character
  object it configures.
- **Advance / invoker** — `MoverAdvance` integrates the per-frame step;
  `MoverInvoker` stores the position and dispatches contact normals.
- **Ramp** — the still-unread control law that turns the wish vector and
  current velocity into the per-frame step.
- **Domino** — the client's physics library: `dmBody`, `dmWorld`, fixtures,
  islands and the fixed-step solver.
- **Tick** — the client's physics step, 1/30 s. It is not a network rate.
- **Statescript** — the visual scripting graph that drives weapons, abilities
  and movement states.
- **Volley** — the weapon state that fires shots: rate, pellet count,
  projectile speed/lifetime, cast length and motions.
- **Blend tree** — the animation node graph for one item:
  `BranchByCategory`, `LayerChild`, `AnimationPose2d`, `Animation`,
  `LayerAdditive`.
- **Slot** — an entry of an item's float array. A node parameter indexes a
  slot or a clip.
- **Sampler** — the runtime function that blends child poses: child[0]
  initialises; rotation/translation/scale lanes are winner-take-all above a
  weight of 0.5; four scalars are lerped (`AnimSamplerRule`).
- **Pose2d** — an aim/lean pose layer node; its two drivers feed weights.
- **Retarget** — converting a pose authored on one rig to another. The FP rig
  drops intermediate bones; the fold rule composes the dropped ancestors'
  locals into the child.

## Camera

- **FP camera / FP pass** — the separate first-person projection used for the
  view model and weapons: client near/far `0.01`/`1638.4`, depth clear, world
  FOV 103° horizontal.
- **View-model transform** — the transform of the FP model relative to the
  camera; driven by `STUFirstPersonComponent` data (per-hero floats and an
  offset).
- **Bob clip** — `0x0a...c15a`, a two-track 170-frame first-person camera bob
  (position + rotation tracks).

## Harness and verification

- **`ow-client`** — the one executable: window, input, renderer, HUD and the
  inspection camera. Its default launch opens the resolved practice range.
- **`--shot` / `--tour`** — offscreen still and tour rendering.
- **`dev/verify_harness/`** — differential verification of client functions:
  the oracle is the client's bytes emulated offline; the candidate is a Rust
  probe or Ghidra C. Five stages (load, determinism, differential, coverage,
  mutation), a digest-bound ledger and a selftest.

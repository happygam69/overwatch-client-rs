# Glossary

Plain-language definitions for the terms used throughout the docs and
code. When a term has a precise meaning in this project, that meaning is
given here once.

## Game data

- **Install** - the operator's own Overwatch installation. It is read in
  place and never written to.
- **CASC** - Blizzard's content store: content-addressed archives plus an
  index that maps keys to bytes. `ow-casc` resolves keys to bytes and knows
  nothing about game semantics.
- **GUID / asset ref** - an 8-byte asset identifier. Its top bits carry the
  asset *type* (`guid::type_of`); models are type `0x00C`, clips `0x00A`,
  entities `0x003`, and so on.
- **STU** - "structured data": Overwatch's gameplay data format. A file is
  a set of typed instances; each instance sits in a shared buffer with a
  layout decoded from a class registry. The class registry (hashes, field
  names, types) comes from the local OWLib checkout.
- **Class hash / field hash** - 32-bit identifiers for an STU class and its
  fields. Field names like `m_63DEDAC0` are the client's own generated
  names; many fields are `Unknown` in the game's own registry.
- **Chunk** - a named block inside a model/asset file (`mskl` = skeleton,
  `MSTU` = model STU, `MDLC`/`MRNM` = mesh data, `HTLC`/`MHRP` =
  hardpoints).
- **View model (FP model)** - the separate entity/model drawn in first
  person (hero 3: `0x0d000000000000f1`). It is a subset rig of the
  character model with an identical bind pose.

## Images and captures

- **Capture** - a read-only recording of process pages made by
  `dev/live_page_diff.py` (or the older `warm_poll.py`). Captures live
  outside the repo (`E:/ow-dumps`, `D:/ow-dumps`).
- **Run** - one capture session directory (`E:/ow-dumps/live_page_diff/
  run_YYYYMMDD_HHMMSS`), with its pages, `warm.csv`, `encrypted.csv`,
  `events.csv`, `meta.json`.
- **v2b / standing image** - the flattened module image used for analysis:
  `module_after_diff.bin`, based at `0x7ff7c4d20000`. "v2b address" means
  `image base + RVA`.
- **RVA** - module-relative offset; `file offset` in a flattened image.
- **Session image** - a *single run's* pages flattened at that run's base
  (`dev/session_merge.py`). Use it for pointer chasing: a merged image mixes
  bases from many sessions.
- **Page states**: `base` (readable at attach), `flip` (protection
  changed), `diff` (content changed), `hot` (re-read), `protect`
  (re-protected), `poll`/`retryrun` (retries). Recorded per page in
  `warm.csv`.
- **Page cipher / keystream** - the main exe's `.text` is encrypted per
  4 KiB page on disk; the loader decrypts pages on first use. The per-page
  keystream is `disk_byte XOR plain_byte`, deterministic per build. The
  collected keystreams live in `D:/ow-dumps/pagepak/keystream.npy`.
- **Erased page** - a page the client fills with `0x90`/`0xC3` after use
  (e.g. the main exe entry page). Captures see the erased state; only the
  disk image plus the keystream can recover the real bytes.

## Evidence and honesty

- **Read** - a decompiled instruction path or a data value with a recorded
  address/capture.
- **Measured** - a number from a trace diff, oracle or census.
- **Install** - a value read from the shipped files with no client running.
- **Fitted** - chosen to match measurements/eye-checks; the client's own
  expression is unread. Must be in `fitted-ledger.md`.
- **Assumed** - a rule or constant with no client source at all.
- **Stand-in** - harness-only geometry/presentation, not claimed as client
  behaviour.
- **Tier 1/2/3** - confidence classes (`rewrite-plan.md`). Tier 3 means the
  client's stage is declared unrecoverable; the replacement is measured and
  labelled.

## Animation

- **Clip** - an animation asset (type `0x00A`). Families differ: the blend
  cards (`0x0053...`) carry per-bone record cards; the `0x00020400` family
  is a two-track camera bob; older families decode through keyed tracks.
- **Blend set (`STUAnimBlendTreeSet`)** - the per-entity animation table:
  items (`0xDDD18945`), each naming clips, parameter slots and a blend
  tree.
- **Blend tree (`STUAnimBlendTree`)** - the node graph for one item:
  `BranchByCategory`, `LayerChild`, `AnimationPose2d`, `Animation`,
  `LayerAdditive`.
- **Slot** - an entry of an item's float array (`0xC486B05D`). A node
  parameter's `m_63DEDAC0` is a 1-based slot index (or a clip index on
  animation wrappers).
- **Sampler** - the runtime function that blends child poses: child[0]
  initialises; rotation/position/scale lanes are winner-take-all above a
  weight of 0.5; four scalars are lerped (`FUN_7ff7c6e84610`).
- **Pose2d** - an aim/lean pose layer node; its two drivers feed weights.
- **Retarget** - converting a pose authored on one rig to another. Here the
  FP rig drops intermediate bones; the fold rule composes the dropped
  ancestors' locals into the child (measured, not fitted).

## Camera

- **FP camera / FP pass** - the separate first-person projection used for
  the view model and weapons: client near/far `0.01`/`1638.4`, depth
  clear, world FOV 103° horizontal unless the client's own FP values say
  otherwise (`fitted-ledger.md`).
- **View-model transform** - the transform of the FP model relative to the
  camera; driven by `STUFirstPersonComponent` data (per-hero floats and an
  offset), whose consumer is still unread.
- **Bob clip** - `0x0a...c15a`, a two-track 170-frame first-person camera
  bob (position + rotation tracks).

## Harness

- **`--play`** - windowed first-person harness (WASD/mouse, shooting).
- **`--play-th`** - headless run printing the HUD trace; same state.
- **`trace_diff`** - golden-trace comparison for movement/combat commits.
- **`blendset_probe` / `blendtree_probe` / `retarget_probe`** - read-only
  animation data probes (see `artifacts.md` and `tools.md`).
- **`dev/verify_harness/`** - differential verification of client
  functions: the oracle is the real bytes emulated offline (Unicorn over
  the captured image), the candidate is the Ghidra C (tier 1) or a Rust
  probe (tier 2); five stages (load, determinism, differential, coverage,
  mutation), digest-bound ledger, selftest. `python
  dev/verify_harness/cli.py all` runs every unit; `selftest` proves the
  verifier still detects injected defects.

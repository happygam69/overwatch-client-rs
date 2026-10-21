# ow-combat-rust

A clean-room Rust rewrite of Overwatch gameplay, built from the operator's own
installed copy. It reads the client's data at runtime and reimplements what the
client does with it: movement, combat, the map, the view model and the camera.

**This repository contains no Blizzard content.** No art, models, sounds, maps,
textures, locale data, extracted tables or decompiled code - only original Rust
source and documentation. The tools open the installed game read-only and never
modify it. See [CREDITS.md](CREDITS.md).

## How the behaviour is established

The game is too large to recover from files alone: the executable's code is
encrypted at rest, pages are decrypted lazily, and much of the behaviour lives
in engine code and generated graphs rather than in the assets.

* **Files give structure.** Formats, registries, schemas, geometry and values
  are read from the install and checked by tests that fail if the reading is
  wrong. Findings live in `docs/formats/`.
* **The decompiled client gives behaviour.** Ghidra is run over the operator's
  own memory captures. Only addresses and conclusions are written down, never
  disassembly or decompiled code. `docs/provenance.md` defines the evidence
  classes; `docs/fitted-ledger.md` lists every place where a behaviour is
  fitted, assumed, tier 3 or a stand-in.
* **Neither substitutes for the other.** A file question that cannot be settled
  from files moves to a capture; a capture reading that is unsure is re-derived
  from the same capture rather than believed.

## Status

Verified against the retail install `2.25.0.0.154088`.

**Works**

* **Container and data:** CASC/encoding/BLTE, manifests and the content
  registry; both STU versions (v1 synthetic-tested only) and the typed class
  registry (classes, fields, defaults, arrays, maps, assets).
* **Movement:** the mover is trace-measured and gated by `trace_diff` - speeds,
  acceleration, friction, gravity, jump, crouch, air control and slope
  following. The control *code* is tier 3; the arithmetic is the measured
  replacement.
* **Combat:** health/armour/shield pools, weapon intervals, volley counts,
  projectile speed/gravity/lifetime and hitscan traces. Damage is supplied by
  the operator because the install does not expose it; damage order and armour
  mitigation are assumptions.
* **Scene and view:** real map geometry, hero models, skeletons, skinning and
  the shipped texture formats; `ow-view` is a direct `wgpu`/`winit` viewer with
  offscreen shots and the `--play` harness.
* **View model and camera:** the first-person model, its own render pass with
  the client's near/far (`0.01 / 1638.4 m`), the client's `HorizontalFOV`
  (`103°` horizontal), a zero-rest drag spring from
  `STUFirstPersonComponent` (`64 / 16 / -1`; constants read, form fitted) and
  the opt-in fitted look filter (`--cam-look-inverse 0.1`).
* **First-person animation:** the clip feed and the real `STUAnimBlendTree`
  are evaluated (`--anim auto`; category 5 -> c71, category 9 -> c50; Pose2d
  layers at their slot weights; the per-child >0.5 sampler gate); the FP
  retarget's dropped-ancestor fold is measured and implemented.
* **Content surfaces:** skin themes, customizable skin themes, voice
  conversations, progression unlocks, loot boxes, stats and catalogs (6 of 9
  surfaces readable on this install).
* **Replay and sound metadata:** the `phl`/`prp` container, header and hero
  roster; Wwise metadata and WEM headers.
* **Capture pipeline:** `dev/live_page_diff.py` (read-only, protection-aware)
  and `dev/warm_poll.py` (fallback); merged images are kept outside the repo.

**Open / blocked**

* **First-person animation:** the Pose2d dataflow VM (driver inputs and layer
  weights) and the reference-skeleton/rest data behind the 2.395x shoulder rows
  are unread; at the authored defaults the tree output is the base clip (c71).
* **The first-person camera transform:** the FP near/far, `HorizontalFOV`, the
  per-hero `STUFirstPersonComponent` values (offset, spring, four scalars) and
  the live component layout are read; the type-0x35 consumer and its per-frame
  slots are identified, but the per-frame offset/scalar application is not. The
  FP drop/back framing stays explicitly fitted (`--fp-cam-drop`,
  `--fp-cam-back`).
* **World camera near/far** are not read (the FP pair is).
* **Materials:** the model-to-look-to-texture chain works where the keyring has
  the keys; several hero body materials and many map props remain blocked.
* **Per-shot damage**, the steep-slide rule, the v1 STU path, the replay frame
  payload (Zstd) and Wwise audio decoding remain open or declared.
* **Bots and abilities** are test stand-ins: boxes/markers, no client AI.

The detailed ledger is `docs/systems-coverage.md` and
`docs/fitted-ledger.md`.

## Running it

```sh
export OW_INSTALL='E:\Overwatch'                        # folder with .build.info
export OW_TACTLIB='path/to/TACTLib/TACTLib'             # per-build manifest key table
export OW_OWLIB='path/to/OWLib'                         # optional, STU class names
export OW_SUPPORT_KEYRING='path/to/Overwatch.keyring'   # optional

cargo test --release                                    # real-install reads are skipped without an install
cargo run --release -p ow-view -- list                  # map chunks worth drawing
cargo run --release -p ow-view -- --play --hero 3 --anim auto
```

Useful harness flags:

* `--no-map` - the sandbox course instead of map chunks (the default for
  `--play` when no chunks are named).
* `--hero N --hero-only` - inspect one hero's model; `--pose bone:x:45`
  articulates it.
* `--view-model [--first-person]` - place the hero's first-person model, in
  front of the camera or at the player's transform.
* `--anim <guid|auto>` - play an animation clip; `auto` resolves the FP idle
  through a heuristic over the client's blend set (it still plays one clip,
  while the true idle item is a 5-clip blend).
* `--fp-cam-drop` / `--fp-cam-back` - tune the fitted FP pass framing.
* `--cam-look-inverse 0.1` - enable the fitted look filter (default exact).
* `--vm-k` / `--vm-c` / `--vm-g` - override the view-model drag spring.
* `--play-th <seconds>` - headless harness for scripted checks.

Live capture (read-only, operator's decision):

```sh
python dev/live_page_diff.py --wait --out D:/ow-dumps/live_page_diff --all-modules
```

Press Enter to mark a phase; a line starting with `\` stops. The tool writes
4 KB pages plus `warm.csv` and `encrypted.csv` outside the repo. The fallback
is `python dev/warm_poll.py --wait --all-regions`.

Data probes:

```sh
cargo run -p ow-tank --example blendset_probe -- 3    # true FP items, clips, curves, retarget params
cargo run -p ow-tank --bin look -- --help             # asset/material probe
cargo run -p ow-tank --bin anim -- --help             # clip probe
```

## Layout

```
crates/ow-casc   container: build info, config, keyring, buckets, BLTE, encoding, root
crates/ow-stu    structured data, typed STU reads, class registry
crates/ow-tank   game semantics: GUIDs, manifests, entities, weapons, graphs, animation, chunks
crates/ow-sim    movement/combat simulation and trace verification
crates/ow-scene  map geometry and model assembly for the renderer
crates/ow-view   the wgpu viewer and the play harness
crates/ow-game   the Bevy shell around the simulation
docs/formats/    verified layouts and decompiled-behaviour findings
docs/fitted-ledger.md       every fitted/assumed/tier-3/stand-in implementation
docs/systems-coverage.md    every client system, state and next anchor
docs/provenance.md          evidence classes and method
dev/                        read-only captures, probes and tooling (gitignored output)
```

Captures and merged images live outside the repo, under `D:/ow-dumps`; nothing
from the install is committed.

## Licence

MIT or Apache-2.0, at your option. That covers this code only - not the game,
its data, or anything either of them produces.

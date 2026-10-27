# overwatch-client-rs

A clean-room Rust reimplementation of the Overwatch client's gameplay:
movement, combat, the view model and the camera. It reads data from the
operator's own installation and behaviour from the complete current-build
`Overwatch.exe` image. Nothing from the game ships here.

**The rule: 1 to 1 or not at all.** A gameplay feature is implemented only when
the client's own code or value has been read and the Rust result is proven
identical. There is no "close enough" gameplay system. If the client's
expression has not been read, the feature is listed as open and is not
implemented. The current non-1:1 code is legacy debt in
[`docs/deviations.md`](docs/deviations.md); it is being removed.

**This repository contains no Blizzard content.** No art, models, sounds, maps,
textures, locale data, extracted tables or decompiled code — only original Rust
source and documentation. The tools open the installed game read-only and never
modify it. See [CREDITS.md](CREDITS.md).

## Sources

- **Behaviour** comes from one place: the complete `Overwatch.exe` Ghidra
  image (base `0x140000000`, every `.text` page, the install's plaintext
  `.pdata`). Findings are recorded as an address and a conclusion, never as
  disassembly. `docs/ghidra-project.md` is the entry point.
- **Data** comes from the operator's installation: formats, registries,
  geometry, values. Findings live in `docs/formats/` and are checked by tests
  that read the install.
- Only the current image and the operator's installation are sources.

## Status

**The one client launches the real practice range.** `ow-client` and `ow-scene`
are workspace members; `ow-sim` remains parked reference code. The default
launch selects the practice header by its `m_mapName`
(`Maps\TrainingRoom\Training Room - Practice`), takes the first variation whose
paired mode's display name is exactly `Practice Range`, and loads the 34 chunks
the client's map loader requests for that variation's scene map
`0x080000000f69002e`.

- **Read in the current image:** the mover advance/invoker path, the Domino
  tick driver, the camera surface and projection builders, the FP camera pose
  path, the animation sampler and clip runtime, the statescript executor, the
  manifest's bundle flag and bundle entry lookup, and the map-to-chunk loader.
- **Working today:** the map header → variation → scene map → chunk resolver
  (`ow-tank::mapresolve`), and the default client rendering the practice range.
  The practice scene requests and opens 34/34 chunks, walks 1,358 placeables
  (229 model-group references, 550 entity references), and places 2,449 model
  instances from the model groups' 304 groups.
- **Open:** the client function that selects a variation for a requested game
  mode (the practice-range launch is a data query, not a claim that the caller
  is read), the streaming policy, the client's producer of the bundle
  directory, character collision, the mover's input→velocity ramp, the
  per-frame camera look, the FP retarget, the Pose2d VM, the weapon fire loop,
  damage application and the client material application (55 placeholder draws
  remain diagnostics).
- **Data-side reads:** CASC/BLTE, STU v2, entities and health pools,
  model/skeleton/clip decoding, weapon graphs, map chunks, and the
  bundle-aware material/texture chain. All 305 map headers are readable: 42
  from the encoded store and 263 from bundles, as the client reads them.

The rebuild order is [`docs/rebuild-plan.md`](docs/rebuild-plan.md).

## Running it

```sh
export OW_INSTALL='E:\Overwatch'                        # folder with .build.info
export OW_EXE='E:\Overwatch\_retail_\Overwatch.exe'     # optional; found under OW_INSTALL anyway
export OW_TACTLIB='path/to/TACTLib/TACTLib'             # older-build manifest key tables
export OW_OWLIB='path/to/OWLib'                         # optional, STU class names
export OW_SUPPORT_KEYRING='path/to/Overwatch.keyring'   # optional

cargo xtask verify                                      # fmt, clippy, tests, index, deviations
cargo xtask verify --fast                               # without the install-reading tests

cargo run --release -p ow-client                        # the practice range
cargo run --release -p ow-client -- --list-maps         # headers and variation modes
cargo run --release -p ow-client -- --shot out.png --width 1280 --height 720
cargo run --release -p ow-client -- --tour out --count 4
cargo run -p ow-tank --bin look -- --why 0x0790000000000688  # the practice header
cargo run -p ow-tank --bin look -- --why 0x080000000f69002e  # its scene map
```

`--chunks`/`--chunks-from` remain explicit chunk diagnostics; they are not the
default and a chunk list is not a map. `--no-map` is the explicit stand-in
course for viewer work.

Data probes:

```sh
cargo run -p ow-tank --example blendset_probe -- 3    # FP items, clips, slots, tree
cargo run -p ow-tank --bin look -- --help             # asset/material probe
cargo run -p ow-tank --bin anim -- --help             # clip probe
```

## Layout

```text
crates/ow-casc   container: build info, config, keyring, buckets, BLTE, encoding, root
crates/ow-stu    structured data, typed STU reads, class registry
crates/ow-tank   game semantics: GUIDs, manifests, entities, weapons, graphs, animation, maps, mapresolve
crates/ow-scene  map geometry from the resolver's chunks and the models they place
crates/ow-client the one launchable client: renderer, HUD, free camera
crates/ow-sim    parked reference: the read body/camera arithmetic (not built)
crates/xtask     the project's own commands; `cargo xtask verify` is the gate
docs/            current findings, plans and open questions
dev/             read-only analysis and verification tools (tracked); output is not
```

## Licence

MIT or Apache-2.0, at your option. That covers this code only — not the game,
its data, or anything either of them produces.

## Contributing

Start with [CONTRIBUTING.md](CONTRIBUTING.md) and [AGENTS.md](AGENTS.md). The
short version: one gate decides whether a change is done —

```sh
cargo xtask verify --fast   # what CI runs; drop --fast once you have an install
```

— and the work queue is [`docs/open-questions.md`](docs/open-questions.md).

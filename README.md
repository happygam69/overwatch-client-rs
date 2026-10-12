# ow-combat-rust

A clean-room Rust rewrite of Overwatch that **reads its data from the operator's
own installed copy at runtime** and **matches the original's actual behaviour**.

Those two halves are the whole project, and every file here exists to serve
them. Anything that does not — one-off experiments, superseded probes,
community-research summaries — is deleted rather than kept around to drift.

**This repository contains no Blizzard content.** No art, models, sounds, maps,
textures, locale data, extracted tables or decompiled code — only original Rust
source and documentation. The tools open an installed copy of Overwatch
read-only and never modify it. See [CREDITS.md](CREDITS.md).

## How the original's behaviour is established

The game is far too large for behaviour to be recovered from its files or from
guesswork: a 65 MB binary whose real code lives in generated and protected
regions, a process holding gigabytes of committed memory, ~1.5 million assets,
and behaviour spread across state graphs, engine code and data the files never
name. **Given that size, matching the original's actual behaviour comes mainly
through decompilation — Ghidra against the operator's own memory capture** —
not from the file formats alone.

The division of labour, which this project follows:

* **Files give structure.** Formats, registries, schemas, geometry, the data an
  asset carries — read from the install and verified by tests that fail if a
  reading is wrong (`docs/formats/`).
* **The decompiled client gives behaviour.** What the code actually does with
  that data — how values are resolved, cached, dispatched, clamped — read from
  the captured process in `dev/` (gitignored) and recorded as addresses plus
  conclusions in the docs, never as disassembly or decompiled code. See
  [docs/formats/materials.md](docs/formats/materials.md) for the worked
  example: one missing link, mapped end to end this way.
* **Neither substitutes for the other.** When a file question cannot be decided
  from files, it moves to the capture; when a capture reading is unsure, it is
  re-derived from the same capture rather than believed.

## Status

Verified against a retail install (`2.25.0.0.154088`).

| | |
|---|---|
| container and manifests | opens; encrypted chunks decrypt; 1,514,319 assets registered |
| structured data | heroes, entities, weapons and state graphs read |
| health pools | 127 of 135 heroes, health / armour / shields kept separate |
| weapon ballistics | 58 weapon graphs with named parameters |
| chunked data | models 38 of 40 (`MDLC`/`MRNM`/`MSTU`), map chunks, placeables |
| models and maps | vertices, normals, UVs, indices; every map reference resolves |
| the view | `ow-view` draws real map geometry from a first-person camera |
| the practice range map | found: 31 distinct chunk contents (method in the roadmap) |
| terminal range | `ow-range` fights with the extracted numbers; a test bench, not the target |

Blocked and documented, each with the test or reading that blocked it:

| | |
|---|---|
| **materials and textures** | the model→material link is opaque; every cheap scheme is ruled out and the client-side map is in progress — [docs/formats/materials.md](docs/formats/materials.md) |
| **per-shot damage** | not in the files; seven file-side places searched, capture-side search prepared (`dev/disasm_field_sites.py`) |
| **character controller, HUD, bots** | not started; geometry and combat data exist |
| **text assets** | manifests list 124,000 records each; almost none downloaded |

## Running it

```sh
export OW_INSTALL='E:\Overwatch'               # folder containing .build.info
export OW_TACTLIB='path/to/TACTLib/TACTLib'    # per-build manifest key table
export OW_OWLIB='path/to/OWLib'                # optional, STU class names
export OW_SUPPORT_KEYRING='path/to/Overwatch.keyring'  # optional

cargo run --release -p ow-probe -- info        # what install was found
cargo run --release -p ow-probe -- health      # per-hero pools from entities
cargo run --release -p ow-probe -- weapons     # weapon graphs per hero
cargo run --release -p ow-probe -- guids 0x008 # registry guids by type
cargo run --release -p ow-view  -- list        # map chunks worth drawing
```

| command | what it does |
|---|---|
| `info` / `types` / `buckets` | install, build, registry; container diagnosis |
| `heroes` / `hero <i>` / `health` | hero references, instances, pools |
| `weapons` / `weapon-params` | state-script graphs and their named parameters |
| `scan` / `dump` / `grep` | find assets by class, read one, search payloads |
| `materials` / `mdl-values` / `resolve-material` | the material-chain questions, as tests |
| `guids` / `first` | registry guids, by type, readable-only options |
| `mapchunk` / `near` / `model` | map chunks, proximity search, model geometry |

Two inputs are deliberately not in the repository: the per-build manifest key
table (read from a reference-library checkout at runtime) and a supplementary
keyring. Neither is ours to vendor; see [docs/provenance.md](docs/provenance.md).

## Layout

```
crates/ow-casc   container: build info, config, keyring, buckets, BLTE, encoding, root
crates/ow-stu    structured data, where gameplay values live
crates/ow-tank   game semantics: GUIDs, manifests, entities, weapons, graphs, chunks
crates/ow-view   the client-side renderer: window, camera, map geometry
crates/ow-sim    combat simulation: ticks, weapons, pools, damage
crates/ow-range  terminal test bench for the combat numbers
crates/ow-probe  command-line probe for everything readable
docs/formats/    verified layouts and the decomp-derived findings
docs/roadmap.md  the accounting of what exists, what blocks, and what is next
dev/             captures and the decomp/analysis toolchain (gitignored)
```

## Method

File-side claims are only accepted when a check would fail if they were wrong:
index entries must land on a `BLTE` payload, deobfuscation must yield GUIDs
that exist in the registry, checksums must reproduce published constants, and
so on. `docs/formats/` records each format at reimplementable detail, including
the traps. Capture-side claims follow the rule above: coordinates and
conclusions only, reproducible from the same gitignored capture.

## Licence

MIT or Apache-2.0, at your option. That covers this code only — not the game,
its data, or anything either of them produces.

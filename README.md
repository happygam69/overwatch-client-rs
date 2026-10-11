# ow-combat-rust

A clean-room Rust reading of Overwatch's gameplay data, taken at runtime from a
copy of the game the user already owns.

**This repository contains no Blizzard content.** No art, models, sounds, maps,
textures, locale data or decompiled code — only original Rust source and
documentation. The tools here open an installed copy of Overwatch read-only and
never modify it. See [CREDITS.md](CREDITS.md).

## Status

The data path works end to end against a retail install (Overwatch
`2.23.0.3.151177`):

```
$ ow-probe info
version      2.23.0.3.151177
branch       eu (product pro)
build name   prometheus-2_23_0_3-151177
index keys   1058289
keyring keys 327
registry     1326302 records from 2 manifest(s)
  Win_SPWin_RDEV_EExt.cmf
  Win_SPWin_RDEV_LenUS_text_EExt.cmf

$ ow-probe heroes
132 heroes
 index  guid                gameplay entity     entity#   colour
     2  0x02e0000000000002  0x0400000000000022  34        (0.369, 0.000, 0.102, 1.000)
     3  0x02e0000000000003  0x0400000000000001  1         (0.871, 0.478, 0.000, 1.000)
     4  0x02e0000000000004  0x04000000000000e4  228       (0.980, 0.949, 0.678, 1.000)
...
132 of 132 hero references resolve to a registered asset

$ ow-probe settings
settings asset 0x0ca0000000000126 (index 294) -- 377 float settings
  42 of 377 leave min/max at the class default
  59 have a default other than 1.0
```

Concretely, this proves: the container opens, encrypted chunks decrypt, the
content and text manifests decrypt to a sorted asset registry, and structured-data
assets parse far enough to resolve obfuscated asset references, read typed fields,
and honour the rule that a field absent from an instance takes its class default.

The combat *simulation* is not implemented, and neither are the base combat
numbers. `settings` reads the per-hero **tuning multipliers** a custom game
exposes — 1.0 meaning normal — not damage or cooldowns. See
[`docs/formats/stu.md`](docs/formats/stu.md) for what is reachable and what is not.

## Running it

```sh
export OW_INSTALL='E:\Overwatch'          # folder containing .build.info
export OW_TACTLIB='path/to/TACTLib/TACTLib'   # for the per-build manifest key table
export OW_OWLIB='path/to/OWLib'               # optional, for STU class names
export OW_SUPPORT_KEYRING='path/to/Overwatch.keyring'  # optional, for newer assets

cargo run --release -p ow-probe -- info
cargo run --release -p ow-probe -- types
cargo run --release -p ow-probe -- heroes
cargo run --release -p ow-probe -- hero 2
cargo run --release -p ow-probe -- settings
```

| command | what it does |
|---|---|
| `info` | which install was found, its build, its registry |
| `types` | asset counts per type, including the combat-relevant ones |
| `heroes` | every hero, with a resolved asset reference and theme colour |
| `hero <index>` | one hero: instances, field layout, resolved fields |
| `settings [n]` | the per-hero tuning multipliers, with bounds resolved |
| `classmap` | how many STU class names were recovered |
| `buckets` | diagnosis for the container alone, before anything else runs |

### Why external paths are needed

Two things are deliberately *not* in this repository, because neither is ours:

* **Per-build manifest key material.** Every game build derives the manifest
  cipher from a different 512-byte table. Rather than vendor those constants,
  the tool reads the one matching the build in front of it from a checkout of
  the reference library (`OW_TACTLIB`), or from a file you supply
  (`OW_KEYTABLE`). See [`docs/provenance.md`](docs/provenance.md).
* **A supplementary keyring.** A handful of assets are encrypted with keys the
  installation's own keyring does not carry. `OW_SUPPORT_KEYRING` points at a
  community keyring file; nothing from it is copied into this repository.

## Layout

```
crates/ow-casc   the container: build info, config, keyring, bucket index,
                 BLTE, encoding file, root file
crates/ow-stu    structured-data assets, which is where gameplay data lives
crates/ow-tank   Overwatch semantics: asset GUID typing, content manifest,
                 class map, and the assembled GameData view
crates/ow-probe  the command line probe
docs/formats/    the file formats, as verified against a real install
docs/provenance.md  what was learned from where
dev/             development scratch: the format probes used to pin things
                 down. Not part of the product, and not tracked.
```

The split mirrors the reference projects: format crates carry no game semantics,
and a game-layer crate sits on top.

## Method

Formats were pinned down by reading real files rather than guessing, and each
claim is checked by something that would fail if it were wrong:

* A candidate index-entry layout is only accepted if following it lands on a
  `BLTE` payload.
* The encoding file's section ordering is confirmed by the root file's content
  key landing exactly on a page boundary.
* Salsa20 is pinned by the published reference keystream, which catches both
  round count and key schedule.
* Asset-reference deobfuscation is confirmed by the resolved GUIDs existing in
  the asset registry; a wrong implementation yields GUIDs that resolve to
  nothing.
* GUID type decoding is confirmed by reproducing the published permutation table
  and by the observed counts (132 heroes on the known hero GUID prefix).

`docs/formats/` records each format at the level of detail needed to reimplement
it, including the traps.

## Licence

MIT or Apache-2.0, at your option. That covers this code only — not the game,
its data, or anything either of them produces.

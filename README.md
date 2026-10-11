# ow-combat-rust

A clean-room Rust reading of Overwatch's gameplay data, taken at runtime from a
copy of the game the user already owns.

**This repository contains no Blizzard content.** No art, models, sounds, maps,
textures, locale data or decompiled code — only original Rust source and
documentation. The tools here open an installed copy of Overwatch read-only and
never modify it. See [CREDITS.md](CREDITS.md).

> **Target, stated properly:** a first-person 3D practice range — Overwatch's
> geometry, bots, HUD and combat, so the systems can be tested as the game tests
> them. What exists today is the data layer and a terminal simulator, which is
> **not** that. [`docs/roadmap.md`](docs/roadmap.md) is the honest accounting of
> the gap, the staged plan, and what the encrypted executable can and cannot
> provide.

## Status

**Working, verified against a retail install (`2.23.0.3.151177`).**

| | |
|---|---|
| the container | opens; encrypted chunks decrypt; 1,326,302 assets in the registry |
| structured data | heroes, entities, weapons and state graphs all read |
| health pools | **124 of 132 heroes**, health / armour / shields kept separate |
| weapon ballistics | 56 weapon graphs, **889 named parameters** extracted |
| the simulation | hitscan, travelling projectiles, spread, falloff, pools, armour |
| the practice range | select a hero, walk, shoot targets, read damage and TTK |

**Not working, and each is documented rather than papered over:**

| | why |
|---|---|
| **per-shot damage** | not in the files. Seven places searched, recorded in `docs/formats/weapons.md`. The range asks for it. |
| **abilities** | the graphs hold them, but a graph is a program and nothing names which node is a cooldown |
| **movement physics** | the speed is read but its semantics are unconfirmed, so nothing moves yet |
| **8 of 132 heroes** | their entities are recorded in the manifest but absent from the install |
| **damage order, armour rules** | game *rules*, not values; assumed in `ow-sim/src/rules.rs` and changeable at runtime |

The data path in detail:

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

$ ow-probe health
 index   health   armour   shields    total
     2      250        -         -      250
     3      150        -         -      150
     7      200      250         -      450
     9      350      200         -      550
    10      175        -         -      175
    22      100        -       100      200
    32       50        -       150      200
    64      700        -         -      700
   104      250        -       225      475
   318      275      275         -      550
...
124 heroes read, 8 not readable
totals min 100 max 3600 median 200
```

Concretely, this proves: the container opens, encrypted chunks decrypt, the
content and text manifests decrypt to a sorted asset registry, structured-data
assets parse far enough to resolve obfuscated references and read typed fields,
and an entity's schema can be walked to its literal values.

`health` is the first **base** combat data: the pools a hero spawns with, read
from the hero's gameplay entity. Armour and shield values match the published
live figures exactly; health values are lower because this installation predates
a health rebalance, so the numbers belong to build `2.23.0.3.151177` and not to
live. See [`docs/formats/entity-schema.md`](docs/formats/entity-schema.md).

`weapons` follows each hero to the state-script graph its weapon behaviour lives
in, and `weapon-params` reads named parameters out of those graphs by joining
each literal to the field that owns it — 889 of them across 54 heroes, including
`projectileSpeed`, `numShotsPerSecond` and `castLength`. **Damage is not among
them**: no weapon-graph field is named `damage`, and the generic `value`
parameters cannot be resolved without reading the graph's links, because the
variable a set-var writes to is a fieldless marker class. See
[`docs/formats/weapons.md`](docs/formats/weapons.md).

`settings` is a different thing: the per-hero **tuning multipliers** a custom game
exposes, 1.0 meaning normal.

The combat *simulation* is not implemented, and the base numbers are only partly
reached: the health pools are read, damage and cooldowns are not. See
[`docs/formats/weapons.md`](docs/formats/weapons.md) for exactly what is missing
and the three cheapest ways to attack it.

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
cargo run --release -p ow-probe -- health
cargo run --release -p ow-probe -- weapons
cargo run --release -p ow-probe -- weapon-params
```

| command | what it does |
|---|---|
| `info` | which install was found, its build, its registry |
| `types` | asset counts per type, including the combat-relevant ones |
| `heroes` | every hero, with a resolved asset reference and theme colour |
| `hero <index>` | one hero: instances, field layout, resolved fields |
| `health` | per-hero health, armour and shields from the entity schema |
| `weapons` | each hero's weapons and the state-script graph each uses |
| `health` | per-hero health, armour and shields from the entity schema |
| `weapon-params [hero]` | named parameters inside those graphs, joined to their owning field |
| `health` | per-hero health, armour and shields from the entity schema |
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
                 class map, field names, entities, weapons, graphs
crates/ow-sim    the combat simulation: fixed tick, weapons, hitscan and
                 travelling projectiles, pools, damage. No IO, unit-tested
crates/ow-range  the practice range: drop a hero in and shoot targets
crates/ow-probe  the command line probe
docs/formats/    the file formats and layers, as verified against a real install
docs/provenance.md  what was learned from where
dev/             development scratch: the format probes used to pin things
                 down. Not part of the product, and not tracked.
```

The split mirrors the reference projects: format crates carry no game semantics,
and a game-layer crate sits on top.

## The practice range

`ow-range` is where the data becomes a fight:

```
$ cargo run --release -p ow-range
> hero 10
hero 10 (0x02e000000000000a): 175 health, 0 armour, 0 shields, 1 weapon(s)
  slot 1: castLength=100 numProjectilesPerShot=1 numShotsPerSecond=2/1
          projectileLifetime=10 projectileSpeed=250 ...
> damage 12
> target add 200 15
> ttk
200 hp at 12.0 damage per shot = 17 shots, 8.50s at 2.00 shots/s
> fire 3
3 shot(s) in 1.50s: 3/3 pellets hit, 36.0 damage applied, 24.0 dps
```

It reads pools, fire rates, projectile speeds and ranges from the install. It
asks for **per-shot damage**, because that could not be extracted. It treats the
damage order and armour mitigation as **named assumptions** in
`ow-sim/src/rules.rs`, changeable at runtime with `order` and `armour`, rather
than burying them. And it surfaces two ambiguities instead of hiding them: a
weapon graph may declare several firing modes with nothing saying which is the
primary, and `numShotsPerSecond = 999` is a sentinel meaning "as fast as input
allows", not a rate.

See [`docs/practice-range.md`](docs/practice-range.md) for what is modelled, what
is not, and why.

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

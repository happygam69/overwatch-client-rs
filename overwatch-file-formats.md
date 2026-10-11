# Overwatch File Formats — Community Research Summary

> Originally compiled 2026-10-11 from a web search.
> **Corrected 2026-10-11** against a retail install
> (`2.23.0.3.151177`) and against the reference tooling's source. The first pass
> was built on third-party claims and got several things wrong in kind, not just
> in detail; see "What the first pass got wrong" at the end.
>
> Where this file summarises the ecosystem, `docs/formats/` carries the actual
> verified layout of each format, and `docs/provenance.md` records which claims
> are verified and by what check.

---

## The one structural fact that reframes everything

**Overwatch assets have no file extensions.** There is no `.m3`, `.txtr` or
`.mat` string anywhere in the container, and no filename at all for the vast
majority of assets. Every asset is addressed by a 64-bit GUID, and its *type* is
a 12-bit number stored inside that GUID with its bits reversed:

```
mangled = (guid >> 48) & 0x0FFF
type    = reverse_12_bits(mangled) + 1
```

So the "extension" table this document originally opened with was describing
community nicknames, not on-disk formats. The numbers are what matter, and they
are what the tooling switches on:

| type | meaning | count in the reference install |
|---|---|---|
| `0x004` | texture | 191,599 |
| `0x00C` | model | 30,402 |
| `0x054` | STU generic settings | 67 |
| `0x075` | hero | 132 |
| `0x09E` | loadout / ability | 848 |
| `0x0C5` | game mode | 129 |
| `0x0CF` | lootbox | 11 |
| `0x134` | talent / perk | 1,350 |

164 distinct types exist in total. Names are community convention attached to
numbers; `0x801`, for instance, is a real type nobody has named.

---

## Layers, outermost first

| layer | what it is | documented in |
|---|---|---|
| **CASC container** | the store: `config/`, 16 bucket index files, `data.NNN` archives | `docs/formats/casc.md` |
| **BLTE** | per-record wrapper: stored, zlib, or Salsa20-encrypted chunks | `docs/formats/casc.md` |
| **Keyring** | on-disk, in `data/casc/config/`, named by `.build.info` | `docs/formats/casc.md` |
| **Encoding file** | content key → encoding key, and decoded sizes | `docs/formats/casc.md` |
| **Root file** | text list naming the manifests and the loose install files | `docs/formats/casc.md` |
| **Content manifest (`.cmf`)** | the asset registry: GUID → content key, AES-encrypted per build | `docs/formats/manifest.md` |
| **Resource graph (`.trg`)** | asset dependency graph | `docs/formats/manifest.md` |
| **STU** | structured data: heroes, loadouts, abilities, settings | `docs/formats/stu.md` |
| **Chunked data** | models, skeletons, maps, effects (M3 lineage) | not yet documented here |

The container formats are fully documented above. STU is documented structurally
but its *meaning* is only partly mapped, and that is the gap that matters for
gameplay work.

---

## Encryption, in two independent layers

The original document said keys "are not publicly documented" and that tools
handle encryption transparently. Both halves of that were wrong.

**Layer 1 — the container's chunk keys.** These are on the user's own disk. A
`.build.info` row carries a `KeyRing` column naming a config file under
`data/casc/config/`, and that file holds the Salsa20 keys as
`key-<16 hex> = <32 hex>` lines. Nothing external is needed. A handful of newer
assets use keys the build's own keyring omits; those come from a supplementary
community keyring file.

**Layer 2 — the manifest cipher.** This is the fragile one and was entirely
missing from the original document. `.cmf` and `.trg` bodies are AES-encrypted
with a key and IV *derived per build* from a 512-byte table plus numbers taken
from the manifest's own clear-text header. The table differs for every build —
252 distinct tables appear in the reference tooling's sources — so a build whose
derivation is not implemented cannot be opened even with correct key material,
and the failure looks like noise rather than a missing key. The reference
library carries one source file per build for exactly this reason; this project
implements one build and reads the table from a checkout at runtime rather than
vendoring the constants.

---

## Tools and libraries

### overtools — the primary ecosystem (MIT)

Note: **`overtools/TankLib` and `overtools/TACTLib` as separate repositories no
longer exist as such.** TankLib was merged into OWLib, which is now a monorepo
carrying `TankLib/`, `TankLibHelper/`, `DataTool/`, `TankPackage/`, `TankView/`
and a `TACTLib` submodule.

| project | what it is |
|---|---|
| [overtools/OWLib](https://github.com/overtools/OWLib) | the monorepo; `DataTool` (CLI extraction), `TankLib` (Overwatch formats), STU class definitions |
| [overtools/TACTLib](https://github.com/overtools/TACTLib) | the CASC layer, including the per-build manifest key generators |
| [overtools/io_scene_owm](https://github.com/overtools/io_scene_owm) | Blender importer for extracted models and maps |

It handles art, audio and unlock extraction well. What it does **not** do: no
shipped tool dumps gameplay numbers. `list-abilities` prints names, icons and
descriptions; `extract-abilities` writes the icon and the info movie. The stat
values are in the data but nothing reads them out.

### Other Blizzard-format libraries

| project | licence | relevance |
|---|---|---|
| [ladislav-zezula/CascLib](https://github.com/ladislav-zezula/CascLib) | MIT | the reference CASC implementation; cited by TACTLib for index and data-header layouts |
| [FernandoS27/WhiteoutLib](https://github.com/FernandoS27/WhiteoutLib) | — | M3 and TXTR handling; **textures only** for Overwatch, no CASC manifest or STU support |
| [nneonneo/libm3](https://github.com/nneonneo/libm3) | — | SC2 M3 reference; dormant since 2016 |

### Rust

There is **no mature Rust parser for Overwatch.** Verified:

| crate | status |
|---|---|
| `cascette-rs` (`blte`, `tact-parser`, `ngdp-crypto`, `casc-storage`, ...) | World of Warcraft. No Overwatch root, manifest or STU support. |
| `casc-lib` / `casc-cli` | WoW-only, explicitly. |
| `whiteoutlib` | Rust binding to the C++ library; Overwatch textures only. |

This project's `ow-casc` / `ow-stu` / `ow-tank` crates exist because that gap was
real.

### Reverse-engineering projects, and their licences

| project | licence | what it covers |
|---|---|---|
| [JnCrMx/overwatch-highlights](https://github.com/JnCrMx/overwatch-highlights) | **none** | highlight `.mp4` trailers: map, hero, skin, highlight type. Docs at [readthedocs](https://overwatch-highlights.readthedocs.io/en/latest/) — the `.github.io` URL is dead. Hero IDs stop at Echo (2020). |
| [lunasorcery/overwatch-highlights](https://github.com/lunasorcery/overwatch-highlights) | **none** | the deepest public work on the highlight and replay container. Explicitly left the raw frame payload undecoded. |
| [overtools ReplayMp4Tool / TankLib Replay](https://github.com/overtools/OWLib) | MIT | replay metadata: a `prp` block with format version, build number, map and gamemode GUIDs, start and end frame, heroes; plus a `phl` highlight block |
| [usernametoolo/ow_scripts](https://github.com/usernametoolo/ow_scripts) | — | format-research scratch scripts |

Both highlight projects being unlicensed matters: they cannot be drawn on, and
their hero-ID tables are why this repository ships no index-to-name table.
`CREDITS.md` lists them under "examined and deliberately not used".

### Combat statistics and parameters

| source | licence | contents |
|---|---|---|
| [OverFast API](https://github.com/TeKrop/overfast-api) | MIT | OW2 hero structure: role, subrole, `hitpoints {health, armor, shields, total}`, ability names and descriptions. Scrapes Blizzard, so no damage or cooldown numbers. |
| [workshop-rs](https://github.com/wrightkit/workshop-rs) | MIT | canonical multi-locale Workshop catalogue of actions, values and parameters |
| [Overwatch Wiki (Fandom)](https://overwatch.fandom.com) | CC BY-NC-SA 3.0 | the actual combat numbers: damage per bullet, falloff, ammo, spread, cooldowns, ultimate cost, movement speed. **Non-commercial** — good for checking a value, unusable as a dataset for a repo. |
| [OverPy](https://github.com/Zezombye/overpy) | GPL-3.0 | custom-game settings schema |
| [OSTW / Deltinteger](https://github.com/ItsDeltin/Overwatch-Script-To-Workshop) | **none** | element documentation |

### Executable

No public Ghidra or IDA write-up of the combat executable exists. Material that
does exist is cheat-oriented and was not consulted. The architecture reference is
Blizzard's own GDC 2017 talk, *Overwatch Gameplay Architecture and Netcode*
(Timothy Ford) — entity-component design with rollback netcode. Tick rate:
community measurements put launch Overwatch near 21 server updates per second and
later builds at 63; there is no Blizzard statement and no measurement for the
current build, so treat any figure as unverified.

---

## Where gameplay data actually lives

This is the part the first pass missed entirely, and it is the heart of the
matter. Gameplay is **data-driven**, in STU assets:

| what | asset |
|---|---|
| hero definition, theme colour, category | `STUHero` (`0x7C3457DC`), type `0x075` |
| an ability or weapon as presented: name, description, button, icon, category | loadout assets, type `0x09E` |
| weapon behaviour | `STUWeaponDefinition` (`0x61386B75`) — a state-script graph |
| damage, cooldowns, projectile speed, health | scalar nodes inside those graphs, e.g. `STUConfigVarFloat`, and components such as `STUHealthComponent` |
| per-hero tuning multipliers, with declared bounds | hero-settings assets, type `0x054`; `STUHeroSettingFloat` carries default, minimum and maximum |

Verified by reading real values: hero theme colours, and every hero's
`m_gameplayEntity` reference (132 of 132 resolve). **Not** yet mapped: which
scalar node means what. That is the open problem, and the hero-settings assets —
whose floats declare their own bounds, so a multiplier identifies itself — are
the cheapest way in.

The `.stu`, `.0bc`, `.00C` extensions this document originally listed are not
formats. "STU" is community shorthand for the structured-data format, whose real
name in the tooling is `teStructuredData`; `.0bc` has no public documentation and
is not a type code either.

---

## What the first pass got wrong

Kept deliberately, because the failure modes are instructive.

1. **It described extensions that do not exist.** Assets are GUID-addressed and
   typed by a number. Every row of the original table was a community nickname
   presented as an on-disk format. This is the kind of error that comes from
   reading tool *output* filenames and assuming they reflect the game's
   internals.
2. **It concluded that binary reverse-engineering was unnecessary** because "the
   overtools ecosystem already handles extraction and parsing". True for art; false
   for gameplay. The ecosystem parses STU structurally and stops there — no
   shipped tool emits a combat number. The real gap was never the container, it
   was the *meaning* of the data inside it.
3. **It said encryption keys are not publicly documented.** The chunk keys are on
   the user's own disk, named by `.build.info`. What is genuinely per-build and
   fragile is the manifest cipher, which the document did not mention at all.
4. **It omitted STU entirely**, which is the layer that matters for a combat
   system.
5. **It presented third-party claims as findings** without marking which were
   verified. Several were wrong, and there was no way for a reader to tell.

The correction that mattered most was not any single fact but the method: read
the real files, and tie each claim to a check that fails if the claim is wrong.

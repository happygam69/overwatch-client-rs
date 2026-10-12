# Credits and provenance

This project reads a game the user already owns. It is not affiliated with or
endorsed by Blizzard Entertainment. Overwatch and all related trademarks are the
property of their respective owners.

## What this repository does not contain

No game content of any kind:

* no models, textures, sounds, maps, animations or locale data,
* no extracted or converted data,
* no decompiled or disassembled code,
* no encryption keys, keyring contents, or per-build manifest key tables.

The `.gitignore` is a whitelist for exactly this reason. Everything the tools
read stays on the machine it was read on.

## Projects studied

The formats and behaviour here were worked out by reading these projects
alongside a real installation. Their licences are reproduced in spirit below;
none of their code is compiled into, generated from, or distributed with this
repository, and no files were copied from them.

### overtools/OWLib and TACTLib — MIT

<https://github.com/overtools/OWLib>, <https://github.com/overtools/TACTLib>

The primary reference for Overwatch's container and asset layer. Used to
cross-check:

* the bucket-index header and entry layout (`ContainerHandler`),
* the encoding file's header, page ordering and entry shapes
  (`EncodingHandler`),
* BLTE chunk modes and the encrypted-chunk layout, including the fact that the
  full 20-round Salsa20 is used with the 16-byte-key constants
  (`BLTEDecoder`, `Salsa20`),
* the content manifest's header, record layout and per-build key derivation
  (`Core/Product/Tank/CMF`, `Core/Product/Tank/TRG`),
* asset GUID typing (`teResourceGUID.FlipTypeBits`) and the `types.txt`
  permutation table,
* STU bags, instance records, field bags, asset-reference deobfuscation and the
  string indirection (`TankLib/STU`),
* the CRC64 used for STU header checksums (`Helpers/Hash/CRC.cs`), from which
  the polynomial in `ow-stu/src/crc.rs` was derived and verified,
* the shape of the generated STU class definitions, which is where the class
  type hashes and field hashes in `ow-tank/src/classes.rs` come from.

Two things are read *from* that checkout at runtime rather than copied here:
the per-build 512-byte manifest key table, and (optionally) the generated STU
class list used for diagnostic names. See `ow-tank/src/keytable.rs`.

### CascLib — MIT

<https://github.com/ladislav-zezula/CascLib>

Consulted indirectly, through the references TACTLib cites, for the CASC index
header and data-header layouts.

### vladtrc/iw4L — Apache-2.0

<https://github.com/vladtrc/iw4L>

Not a source of Overwatch formats. Studied for project structure: how to
separate an engine-agnostic asset layer from per-title gameplay crates, how to
document provenance per module, and how to resolve a user's install without
shipping any of it. Its `NOTICE` file's approach to naming every project read
during development is followed here.

### muffinmxn/gang-beasts-rust — MIT

<https://github.com/muffinmxn/gang-beasts-rust>

Studied for the whitelist `.gitignore` pattern and for the explicit statement
that gameplay constants may be derived from observed behaviour while no game
content is redistributed.

### samwhosung/benilla — MIT OR Apache-2.0

<https://github.com/samwhosung/benilla>

Studied for structure (one crate per format, none of them carrying Bevy or game
semantics), for `docs/METHOD.md`-style hard rules on never committing anything
from the install, and for the dual MIT/Apache-2.0 licence choice.

## Examined and deliberately not used

Projects relevant to this work whose terms do not permit reuse. They are named so
that no one has to rediscover the boundary, and so it is on the record that
nothing from them is in this repository:

* **[JnCrMx/overwatch-highlights](https://github.com/JnCrMx/overwatch-highlights)**
  — highlight-file reverse engineering, including a hero-ID list. **No licence**,
  so all rights reserved. The hero ordering it documents is also independently
  corroborated by the theme colours this project reads directly from the install
  (see `docs/formats/stu.md`), so nothing is lost by leaving it aside.
* **[lunasorcery/overwatch-highlights](https://github.com/lunasorcery/overwatch-highlights)**
  — the deepest public reverse engineering of the highlight and replay container.
  **No licence.** Not consulted for code.
* **Overwatch Wiki (Fandom)** — CC BY-NC-SA 3.0. Non-commercial, and share-alike,
  so it can be used to *check* a value but no table is derived from it here.
* **OSTW / Deltinteger** — no licence file, so all rights reserved.

Cheat-oriented material on the game executable was not consulted at all.

## Tools used

* **Ghidra** — used against the operator's own memory capture of the running
  client (regions recorded by `dev/dump_process.py`, gitignored). No
  disassembly or decompilation is committed; only addresses and behavioural
  conclusions are written down, as coordinates so the reading can be re-derived
  from the same capture (see `docs/formats/materials.md`). The Ghidra MCP
  bridge drives the analysis.
* **overtools/TACTLib** — read as source, and read from at runtime for the
  per-build key material described above. `TankLib/teMaterial`,
  `teStructuredDataAssetRef` and `teResourceGUID` established the material
  asset's four-GUID header and the STU reference deobfuscation used to verify
  the material chain's readable hops (see `docs/formats/materials.md`).

## Community data

None is bundled. Where a figure or name came from community documentation rather
than from the install, it is noted at the point of use in `docs/`.

## Licence

This project's own code: MIT or Apache-2.0, at your option. Vendored
dependencies keep their own licences; `cargo deny` or `cargo license` can
enumerate them.

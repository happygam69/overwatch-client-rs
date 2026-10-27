# `ow-tank`: what is not 1 to 1

The companion to [`deviations.md`](deviations.md). That file is the enforced
`FIT-*` list of legacy code with a marker in the tree. This file is the honest
state of everything else in `ow-tank` that is **not** proven identical to the
client.

**Ground truth is the complete current `Overwatch.exe` image** (base
`0x140000000`). A process capture (`0x7ff6...`) is a different build or a
different run; it is not evidence. Code whose only citation is a capture is
**open**, not read. See `provenance.md`.

**"Verified" has one meaning here:** a differential-harness unit passes all
five stages against the client's bytes. As of this commit the harness ledger
has 13 units and **none targets `ow-tank`** (`casc` targets `ow-casc`; the
legacy probe and `ow-sim` targets are not shipped `ow-tank` code). Every row
below is therefore not 1 to 1 in the strict sense, even where the code is
read, implemented and exact on the install.

## A. Implemented, but not the client's own behaviour

| area | what the tree does now | why it is not 1 to 1 | next anchor |
|---|---|---|---|
| Bundle directory producer | `ow-tank` builds its bundle index by scanning the manifest's type-`0x09C` records | the client builds a directory blob in its content database (`FUN_141fd53a0`, `0x141fd53a0`); the producer is ours | `src/bundle.rs`, `formats/maps.md` |
| Map variation selector | `mapresolve::find_practice_range` queries the install for `m_mapName` and a mode display name, then takes the first variation | the client's mode → variation selector is unread; the practice header has two modes named `Practice Range` (`0x18`, `0x5a`) | `formats/maps.md`, `open-questions.md` item 11 |
| Map chunk bodies | model and entity bodies are read at real offsets; the other nine (model group, single model, light, reflection point, text, sound, effect, sequence, area) are a TankLib `teMapChunk.cs` port | the nine are explicitly "not verified against a real chunk of their kind"; the sweep only reports tallies | `src/mapchunk.rs`, `formats/maps.md` |
| Clip record layout and kind dispatch | the file walk and 31-kind table were derived from process captures `0x7ff613ecf660`, `0x7ff614ee4130`, `0x7ff6164f38f8`, `0x7ff6162f3ae4` | captures are not the current image; kinds 1, 4, 5, 7, 8, 15, 19 and 31 are cold-unverified, and most kinds share a default stub | `src/clip.rs`, `formats/animation.md` |
| Skin evaluation | weights, bind pose and palette are implemented and property-tested | the evaluator is known only from capture `0x7ff614e92700`; the install check is a tolerance (`< 2.0`), not bit-exact | `src/skin.rs`, `formats/animation.md` |
| Animation-to-skin join | `FIT-ANI-07` drops ids not on the skin | legacy deviation, still in `ow-tank` (`src/clip.rs`), despite `deviations.md` saying simulation deviations are gone | `docs/deviations.md` |
| STU class and field layouts | typed readers work against a local OWLib/TankLib checkout | the layouts come from a reference library at runtime, not from the client; the real-data test skips when the checkout is absent | `src/classreg.rs`, `src/fieldnames.rs`, `formats/stu.md` |
| Hero health pools | values are read from the install and checked against a published source | no client read and no test pins the values; the old "pools verified" line was wrong | `src/entity.rs`, `formats/stu.md` |
| Weapon, graph and volley semantics | data records, slots and graph links parse | no address is cited for slot behaviour, per-node dispatch, the fire loop or damage; `weapons` has one trivial test | `src/weapons.rs`, `src/graph.rs`, `formats/weapons.md` |
| Map entity placeable body | placeables are skipped by their size field | the body is not decoded; its reference-shaped qwords do not resolve | `formats/maps.md` |

## B. Read and implemented, but no harness unit

These are the strongest `ow-tank` results. They are exact on the operator's
install and have client addresses, but no differential unit proves the code
equals the client's instructions.

| area | client address | what is proven today | what is missing |
|---|---|---|---|
| Chunk-GUID formula | `0x141b32e20`, kind table `0x1439aa320` | the 31 synthesised GUIDs equal the manifest's own type-`0x0BC` records, 31/31 (practice map) | no harness unit; other maps only by the same formula |
| Map resolver | `0x14143b5c0`, `0x141ff5c80` | 34/34 chunks, 1,358 placeables, 229 models, 550 entities; bit-exact bounds | counts and bounds are regression pins, not a client trace; no harness unit |
| Bundle entry lookup | `0x141fd5820`, `0x141fd4a50` | 305/305 map headers open (42 store, 263 bundle) | unit tests use synthetic bytes; no real-bundle duplicate/absent/offset-zero assertion; no harness unit |
| Manifest build 154504 | `0x142e48a20`, `0x142e48700`, table `0x1436100c0` | decrypts and matches 25 records field-for-field against the plaintext `WinPrism` manifest | not a harness unit; the key table may come from `OW_KEYTABLE`/TACTLib, and the test does not assert the installed exe supplied it; builds 154088 and 150851 are untested |
| Material data join | `0x141b88cb0`, `0x141b894d0`, `0x141b7b9d0` | model → look → material → data → texture chain opens on the install | the client's runtime material application and sampler are unread; the viewer is not the renderer |

## C. Open, with no implementation

| area | state | next anchor |
|---|---|---|
| Streaming policy | which chunks load when is unread | `formats/maps.md` |
| Map chunk kind `0x12` body | not tested | `formats/maps.md` |
| Replay frame payload | container prefix only | `formats/replay.md` |
| Audio decode | Wwise metadata and WEM headers only | `formats/sound.md` |
| Shader instance data | containers parse; real instance layout unvalidated | `formats/shaders.md` |

## Closing a row

A row moves to "verified" only when a `dev/verify_harness/units/fn-*.json`
unit names the current-image address, a `candidateCrate: "ow-tank"` probe, and
the unit passes `python dev/verify_harness/cli.py <unit>` with full coverage
and its mutation gate. A row may instead move to `deviations.md` if it is
legacy code kept temporarily; it may not simply disappear.

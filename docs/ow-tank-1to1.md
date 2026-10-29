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
has 25 units and **twelve target `ow-tank`**, including `fn/ChunkGuid` and
`fn/BundleLookup` (section D), `fn/BundleDirectoryLookup` (section E, a
verified consumer) and `fn/CmfKey154504` / `fn/CmfIv154504` (section B;
differential and mutation pass, but the coverage stage cannot measure the
body). `casc` targets
`ow-casc`; the legacy probe and `ow-sim` targets are not shipped `ow-tank`
code. Every row in sections A–C is therefore not 1 to 1 in the strict sense,
even where the code is read, implemented and exact on the install.

## A. Implemented, but not the client's own behaviour

| area | what the tree does now | why it is not 1 to 1 | next anchor |
|---|---|---|---|
| Bundle directory producer | `ow-tank` builds its bundle index by scanning the manifest's type-`0x09C` records | the client builds a directory blob in its content database (`FUN_141fd53a0`, `0x141fd53a0`); the producer is ours | `src/bundle.rs`, `formats/maps.md` |
| Map variation selector | `mapresolve::find_practice_range` queries the install for `m_mapName` and a mode display name, then takes the first variation | the client has **no mode→variation selector**: `0xD97BC44F`/`0x78715D57` occur only in the registration functions, the map arrives as type-`0x002` GUIDs in the level-load message (Jam `0x501B`), and a `(mode, variant identifier)` pair can name two scene maps | `formats/maps.md` ("Who chooses a variation"), `open-questions.md` item 11.1 |
| Map chunk bodies | model and entity bodies are read at real offsets; the other nine (model group, single model, light, reflection point, text, sound, effect, sequence, area) are a TankLib `teMapChunk.cs` port | the nine are explicitly "not verified against a real chunk of their kind"; the sweep only reports tallies | `src/mapchunk.rs`, `formats/maps.md` |
| Clip record layout and kind dispatch | the file walk and 31-kind table were derived from process captures `0x7ff613ecf660`, `0x7ff614ee4130`, `0x7ff6164f38f8`, `0x7ff6162f3ae4` | captures are not the current image; kinds 1, 4, 5, 7, 8, 15, 19 and 31 are cold-unverified, and most kinds share a default stub | `src/clip.rs`, `formats/animation.md` |
| Skin evaluation | weights, bind pose and palette are implemented and property-tested | the evaluator is known only from capture `0x7ff614e92700`; the install check is a tolerance (`< 2.0`), not bit-exact | `src/skin.rs`, `formats/animation.md` |
| Animation-to-skin join | `FIT-ANI-07` drops ids not on the skin | legacy deviation, still in `ow-tank` (`src/clip.rs`), despite `deviations.md` saying simulation deviations are gone | `docs/deviations.md` |
| STU class and field layouts | typed readers work against a local OWLib/TankLib checkout | the layouts come from a reference library at runtime, not from the client; the real-data test skips when the checkout is absent | `src/classreg.rs`, `src/fieldnames.rs`, `formats/stu.md` |
| Hero health pools | values are read from the install and checked against a published source | no client read and no test pins the values; the old "pools verified" line was wrong | `src/entity.rs`, `formats/stu.md` |
| Weapon, graph and volley semantics | data records, slots and graph links parse | no address is cited for slot behaviour, per-node dispatch, the fire loop or damage; `weapons` has one trivial test | `src/weapons.rs`, `src/graph.rs`, `formats/weapons.md` |
| Map entity placeable body | placeables are skipped by their size field | the body is not decoded; its reference-shaped qwords do not resolve | `formats/maps.md` |

## B. Read and implemented, but not fully proven

These are the strongest `ow-tank` results. They are exact on the operator's
install and have client addresses. `fn/CmfKey154504` and `fn/CmfIv154504`
pass the differential and mutation stages, but their coverage stage cannot
measure the client's body (an acknowledged gap), so the manifest row stays
here under the closing rule. The map-resolver and material rows have no
differential unit; the manifest row has two, but their coverage stage cannot
measure the body.

| area | client address | what is proven today | what is missing |
|---|---|---|---|
| Map resolver | `0x14143b5c0`, `0x141ff5c80` | 34/34 chunks, 1,358 placeables, 229 models, 550 entities; bit-exact bounds; the mode-entry identifier (`m_A9253C68`) and the 63-variation pairing are pinned; the replay header's map GUID is the base map, not a scene map | counts and bounds are regression pins, not a client trace; no harness unit; the producer of the level-load GUID lists is unread |
| Manifest build 154504 | `0x142e48a20`, `0x142e48700`, table `0x1436100c0` | `fn/CmfKey154504` and `fn/CmfIv154504` pass 364 synthetic differential cases each on every channel (all 32 key bytes, all 16 IV bytes, table `0x00`/`0xFF`, build and data-count extremes, digest bytes `0x00`/`0xFF` at all 20 indices); 1/1 and 5/5 mutants killed; the install manifest still decrypts to 1,301,598 records | the coverage stage cannot measure the real body (the linear sweep desynchronises at the overlapping-instruction opaque-predicate prologue), so both units set the coverage gate to 0 with an acknowledged gap; the key table may come from `OW_KEYTABLE`/TACTLib, and the test does not assert the installed exe supplied it; builds 154088 and 150851 are untested |
| Material data join | `0x141b88cb0`, `0x141b894d0`, `0x141b7b9d0` | model → look → material → data → texture chain opens on the install | the client's runtime material application and sampler are unread; the viewer is not the renderer |

## C. Open, with no implementation

| area | state | next anchor |
|---|---|---|
| Streaming policy | which chunks load when is unread | `formats/maps.md` |
| Map chunk kind `0x12` body | not tested | `formats/maps.md` |
| Replay frame payload | container prefix only | `formats/replay.md` |
| Audio decode | Wwise metadata and WEM headers only | `formats/sound.md` |
| Shader instance data | containers parse; real instance layout unvalidated | `formats/shaders.md` |

## D. Closed: verified by a differential unit

| area | unit | client address | result |
|---|---|---|---|
| Chunk-GUID formula | `dev/verify_harness/units/fn-ChunkGuid.json` (`fn/ChunkGuid`) | `MapChunkGuid` at `0x141b32e20`, kind table `0x1439aa320` | verified: 364 differential cases, every channel; 100% coverage (0/0 branches); 3/3 viable mutants killed |
| Bundle entry lookup | `dev/verify_harness/units/fn-BundleLookup.json` (`fn/BundleLookup`) | `BundleEntryLookup` at `0x141fd5820` (`0x141fd4a50` is the caller) | verified: 364 differential cases, every channel; 100% coverage (29/29 reachable branches of 35 sites, six structurally dead edges declared); 12/12 mutants killed |

The unit's probe reads the kind from the same table slot the client indexes
and calls the shipped `ow_tank::mapguid::chunk_guid(map, kind)`. The oracle
reads the identical fixture table bytes, so the formula itself — the kind
shift, the map mask and the base OR — is what the unit proves.

`fn/BundleLookup` calls the shipped `ow_tank::bundle::entry_lookup` and
`Bundle::offset_of` through the same probe. It covers the client's tagged
handle guard, the width gate and all three entry widths, counts 0/1/2/3/8,
first/middle/last hits, absent before/between/after, duplicate GUIDs, offset
`0`, GUID `0` and `u64::MAX`, unsorted tables and a table ending on the
payload's last byte. The client returns `payload + offset`; the fixture passes
its payload's guest base and the probe returns `guest_base + offset`, so both
sides compare one guest `u64`. The read also found a real divergence: the
shipped search used half-open bounds `[0, count)` while the client walks
`0..count - 1`; both agree on a GUID-sorted install table and differ on an
unsorted one, so the search now follows the client's walk exactly.

## E. Verified consumers (no A–C row to close)

A verified unit can prove a stage that is already implemented without closing
the open row next to it. These are recorded separately so section D stays the
list of rows that actually moved.

| area | unit | client address | result |
|---|---|---|---|
| Bundle directory lookup | `dev/verify_harness/units/fn-BundleDirectoryLookup.json` (`fn/BundleDirectoryLookup`) | `BundleDirectoryLookup` at `0x141fd5160` (loaded-bundle guard `FUN_141fcbf40`, handle status `FUN_141fd3970`) | verified: 364 differential cases, every channel; 93.3% coverage (14/15 reachable branches of 18 sites; three sites declare one structurally dead edge each) with one named gap — the loaded-bundle guard's false edge needs the live content-database set; 35/35 mutants killed (one further type-count gate flip declared equivalent; one signature-line mutant does not compile and is counted killed). It verifies the **consumer** of the directory blob; the producer row in section A stays open. |

The unit's probe follows the blob's absolute guest pointers through the same
state region the oracle reads: the directory object sits at its own image
address because the function loads the type-entry pointer twice, through
`param_1+0x18` and through the fixed qword at `0x1441b3ba0`. It reads the
0x20-byte type entry, the group's sorted/index arrays, the running listed
count and the tagged handle's status object. The fixture writes every byte:
multiple type blocks and groups, the key at even and odd type indices,
first/middle/last rows, absent before/between/after, duplicate GUIDs, sorted
and unsorted arrays, counts 0/1/2/3/4/8, every handle status, GUID `0` and
`u64::MAX`, and two sentinels just past the declared type and group counts so
an implementation that runs past them is caught.

## Closing a row

A row moves to "verified" only when a `dev/verify_harness/units/fn-*.json`
unit names the current-image address, a `candidateCrate: "ow-tank"` probe, and
the unit passes `python dev/verify_harness/cli.py <unit>` with full coverage
and its mutation gate. A unit whose coverage stage cannot measure the body
(an acknowledged gap with the gate at 0) proves behaviour but does not close a
row. A row may instead move to `deviations.md` if it is legacy code kept
temporarily; it may not simply disappear.

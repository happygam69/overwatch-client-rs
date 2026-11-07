# `ow-tank`: what is not 1 to 1

The companion to [`deviations.md`](deviations.md). That file is the enforced
`FIT-*` list of legacy code with a marker in the tree. This file is the honest
state of everything else in `ow-tank` that is **not** proven identical to the
client. The texture-specific detail is in
[`texture-1to1.md`](texture-1to1.md).

**Ground truth is the complete current `Overwatch.exe` image** (base
`0x140000000`). A process capture (`0x7ff6...`) is a different build or a
different run; it is not evidence. Code whose only citation is a capture is
**open**, not read. See `provenance.md`.

**"Verified" has one meaning here:** a differential-harness unit passes all
five stages against the client's bytes. As of 2026-11-05 the harness ledger
has **77 units and sixteen targeting `ow-tank`**, all `verified`, including
`fn/ChunkGuid`, `fn/MapChunkCellBytesFixup` and `fn/BundleLookup` (section D),
`fn/BundleDirectoryLookup` (section E, a verified consumer), the three `MRNM`
units and six texture leaves, and `fn/CmfKey154504` / `fn/CmfIv154504`
(section B;
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
| Bone-index join | `skeleton.rs` parses `mskl`'s Lookup and `skin.rs` maps a vertex's raw blend index through it (slot → bone); the client's reader `MsklTableBuild` (`0x141b894d0`) is what sizes the tables, and the install proves the direction: the Lookup is a permutation on all 7,328 skinned models, `remap_count` is the largest raw index plus one (7,308, the other 20 leave spare slots), the file stores its exact inverse at `mskl +88`, and the geometry prefers it 5–8× over the raw index (`bonemap_probe`) | the *consumer* is unread: whether the client adds the submesh `BoneIdOffset`, how it bounds the index and whether `0xFFFF` means "unbound" are the reference's rules, and all three are unexercisable on this install (every offset `0`, no Lookup hole). Only the first four-wide blend-index set is read: 454 models declare `semantic 4 / index 1` and their eight weights sum to 255, so those influences are dropped | the function that joins a mesh index to a bone (CPU-side, or the buffer upload of the remap) |
| Animation-to-skin join | `FIT-ANI-07` drops ids not on the skin | legacy deviation, still in `ow-tank` (`src/clip.rs`), despite `deviations.md` saying simulation deviations are gone | `docs/deviations.md` |
| STU class and field layouts | typed readers work against a local OWLib/TankLib checkout | the layouts come from a reference library at runtime, not from the client; the real-data test skips when the checkout is absent | `src/classreg.rs`, `src/fieldnames.rs`, `formats/stu.md` |
| Hero health pools | values are read from the install and checked against a published source | no client read and no test pins the values; the old "pools verified" line was wrong | `src/entity.rs`, `formats/stu.md` |
| Weapon, graph and volley semantics | data records, slots and graph links parse | no address is cited for slot behaviour, per-node dispatch, the fire loop or damage; `weapons` has one trivial test | `src/weapons.rs`, `src/graph.rs`, `formats/weapons.md` |
| Map entity placeable body | placeables are skipped by their size field | the body is not decoded; its reference-shaped qwords do not resolve | `formats/maps.md` |
| Shader instance (`0x086`) | `shader.rs` parses a reference-derived 108-byte header and its tables | this build registers **no loader** for the type: the asset-type job table at `0x143a7c3a0` maps `0x086` to 0 while `0x085` is 23 and `0x087` is 25, and no type-`0x086` asset exists in the registry, so there is no client code to read and no data to check | `formats/shaders.md` |
| LOD selection | `Model::finest_level` takes the union of every submesh LOD byte (skipping `0`/`0x7f`) and keeps the submeshes whose bit is the union's lowest set bit — the client's own rule at the finest level (`0x141b40be0`) | the client picks that level per instance from distance and a quality term (`0x141b3ce00`), and the per-submesh inclusion predicate that consumes the draw record's LOD byte (`+0x134`) is unread; the viewer always draws the finest | `src/model.rs`, `formats/model.md` |
| Vertex-element decode and mesh assembly | `model.rs` turns `MRNM`'s verified element array and streams into positions/UVs/colours/joints, remaps indices and compacts each submesh's vertex array | the **element record is read** from the client's render-mesh writer (`0x14210c450`: `{semantic, index, format, stream}`, packed = `stride << 22 | offset << 12`), its per-format byte sizes are pinned by offset adjacency and stride, and 637 records on 68 models show the packed stride agreeing with the descriptor; the **value conversion is not read and cannot be** — the client never converts vertex bytes on the CPU (no half-float instruction in the image), so the file's format byte must be joined to the engine enum the D3D11 path uses (`0x1434965d0`), and that join is open. The decoder's floats and the compacted vertex array are the viewer's | read the pipeline element-list builder (caller of the vtable slot `0x143497040`), which is the only missing piece |

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
| Model chunk table and `MRNM` runtime block | `0x141b5e1e0`, `0x141b5df70`, `0x141b893e0`, `0x141b89390`, `0x141b88f10` | the per-tag version gate (MDLC `0x165`, MRNM `0x1f`, MSTU `2`), the fixup, the size formula and the whole runtime block are read; `fn/MrnmFixup` (7/7 mutants), `fn/MrnmRuntimeSize` (6/6) and `fn/MrnmHandler` (12/12) pass every channel; MDLC's material list and MSTU's `{u64 offset; u32 size}` header are read and 60/60 install MSTU chunks parse | MDLC has no differential unit (the tail-size function `0x141b88e30` has no Ghidra entry and the harness cannot express absolute pointers in the fixture); the element decode and mesh assembly are section A; MESD/CLTH/MOCL/mskl are not parsed |
| Effect chunks | `0x143610490` (kind table), `0x142e4a780` (component builder), `0x142e4e8e0` (fixup), `0x142e4fbe0`/`0x142e50110` (array sub-chunks) | `EFCT` root and 0x34 kinds are read; the builder's per-tag payload copies are implemented and seven components are typed (`ECMD`, `ECEF`, `ECEN`, `ECEC`, `ECSO`, `ECVS`, `ECRR`), each field's GUID type pinned on the install (3,369 models, 72,654 sounds, 1,846 voice stimuli, ...) | no harness unit (the builder allocates through a vtable); the runtime record layout and the array consumers are unread; `ECMP`/`ECSS`/`ECAT`/`ESHT`/`ECCV` have no client read site, so they stay raw |

## C. Open, with no implementation

| area | state | next anchor |
|---|---|---|
| Streaming policy | which chunks load when is unread | `formats/maps.md` |
| Map chunk kind `0x12` cell-to-shape decoder | the container and the 17-pointer fixup are implemented/verified (section D); the 17 arrays are not yet turned into shapes | `formats/maps.md`, `formats/domino.md` |
| Replay frame payload | container prefix only | `formats/replay.md` |
| Audio decode | Wwise metadata and WEM headers only | `formats/sound.md` |

## D. Closed: verified by a differential unit

| area | unit | client address | result |
|---|---|---|---|
| Chunk-GUID formula | `dev/verify_harness/units/fn-ChunkGuid.json` (`fn/ChunkGuid`) | `MapChunkGuid` at `0x141b32e20`, kind table `0x1439aa320` | verified: 364 differential cases, every channel; 100% coverage (0/0 branches); 3/3 viable mutants killed |
| Bundle entry lookup | `dev/verify_harness/units/fn-BundleLookup.json` (`fn/BundleLookup`) | `BundleEntryLookup` at `0x141fd5820` (`0x141fd4a50` is the caller) | verified: 364 differential cases, every channel; 100% coverage (29/29 reachable branches of 35 sites, six structurally dead edges declared); 12/12 mutants killed |
| Collision cell-array fixup | `dev/verify_harness/units/fn-MapChunkCellBytesFixup.json` (`fn/MapChunkCellBytesFixup`) | `MapChunkCellBytesFixup` at `0x141ff51a0` | verified: 264 differential cases plus the edge pool, the whole 0xC00-byte block compared, the four relative-pointer slots masked; 33.3% branch coverage (1/3 reachable sites; the two skipped sites are the `JZ` inside the allocate-then-deref-null mode-0 blocks) with two declared dead edges; 12/12 mutants killed. It proves the runtime 17-pointer conversion, not the cell-to-shape decoder. The container itself is the install test `crates/ow-tank/tests/collision_cells_real.rs` (220 cells, 17 arrays, tiling and boundaries pinned). |

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

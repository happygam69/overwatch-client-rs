# Maps, chunks and world assembly

A map is a set of chunk assets. A chunk holds a placeable list; each placeable
names a model and a look. The install supplies the data; the client's
streaming policy and collision path are open.

## Current state

- **Data: read.** Chunk assets, the placeable kind vocabulary, the kind byte in
  the asset GUID and the model/look joins.
- **World assembly: read.** Models, skeletons and hardpoints are assembled by
  `ow-scene`.
- **Map-to-chunk link: read.** The client synthesises the chunk GUIDs from the
  selected map asset's index (`FUN_141b32e20`, `0x141b32e20`); see below. The
  kind table it indexes is read from the image and is `Sources::chunk_kinds`.
- **Header payload location: read and implemented.** 42 of the 305 type-`0x09F`
  headers are in the encoded store; the other 263 are inside type-`0x09C`
  bundles. The client's bundle flag and bundle entry lookup are read
  (`FUN_141fd5820`, `0x141fd5820`) and implemented in `ow-tank::bundle`; the
  practice-range header opens through it and parses as `STUMapHeader`.
- **Map asset chunk set: read and implemented.** `LoadMapAssetChunks`
  (`0x141ff5c80`) requests four stored references (`+0x1B8`, the `+0x1B0`
  list, `+0x1C0`, `+0x1C8`) before the 31 table-slot GUIDs. That is the whole
  "second chunk path": kinds `0x10`, `0x11`, `0x12` and `0x23` are stored in
  the map asset. Implemented in `ow-tank::mapresolve`; the practice-range
  variation opens 34 chunks, 1,358 placeables, 229 model references and 550
  entity references (`tests/map_resolve_real.rs`).
- **Map variations: pairing and identifiers read; the client has no selector.**
  `m_D97BC44F` has one entry per `m_78715D57` variation (practice 3/3, Cairo
  10/10, header `0x07900000000000d4` 63/63), and each variation's
  `m_BF231F12` is its registered scene map asset. The mode entry's
  `m_A9253C68` is a `STUIdentifier` (`0xD800000000000000 | u32`), not an
  asset: 30 distinct identifiers cover the install's 1,612 mode entries. No
  client code reads the two arrays or the header class at runtime, and the map
  the game loads arrives as a type-`0x002` GUID in the level-load message; see
  "Who chooses a variation" below.
- **Practice-range launch: viewer default, not the client's selector.**
  `ow-client`'s default finds the header whose `m_mapName` is
  `Maps\TrainingRoom\Training Room - Practice`, then takes the first paired
  variation whose mode's `STUGameMode.displayName` (`0x5DB91CE2`) is exactly
  `Practice Range`. It resolves that variation's scene map through
  `mapresolve::resolve_scene` and draws only that chunk set. The client's map
  input is a GUID, not a mode, so this remains a deterministic viewer choice
  and the two matching modes are reported.
- **Level load: message-driven.** Jam message `0x501B` carries four GUID
  vectors (`+0x78`, `+0x90`, `+0xA8`, `+0xC0`); the client preloads them and
  opens the map catalog. The replay header's map GUID is a header's `m_map`
  (base map), not a variation scene map. See "Who chooses a variation".
- **Model groups: expanded.** A kind-`0x01` placeable's own transform is
  identity; its 304 groups carry the looks and its 2,449 entries carry the
  world transforms. The practice-range client places the entries, not one
  instance per group placeable.
- **Bundle directory: consumer verified, producer open.** The lookup
  `BundleDirectoryLookup` (`0x141fd5160`) is a verified harness unit
  (`fn/BundleDirectoryLookup`); the *producer* of the directory blob is still
  not read.
- **Open:** the client's *producer* of the bundle directory (the blob handed to
  the bundle job), the producer of the level-load GUID lists (which resolves a
  map request to the scene map), the streaming policy (which chunks load when)
  and the entity-placeable layout. The level's collision mesh is read
  (`formats/chunk-kinds.md` section 4b) and the walk uses it.


## The chunk vocabulary

The client's own names:

Placeable kinds:

```text
MAP_PLACEABLE_MODEL_GROUP  MAP_PLACEABLE_LIGHT   MAP_PLACEABLE_ENTITY
MAP_PLACEABLE_OCCLUDER     MAP_PLACEABLE_AREA    MAP_PLACEABLE_SOUND
MAP_PLACEABLE_REFLECTIONPOINT  MAP_PLACEABLE_LABEL  MAP_PLACEABLE_TEXT
MAP_PLACEABLE_EFFECT       MAP_PLACEABLE_FOG     MAP_PLACEABLE_UNKNOWN
teMAP_PLACEABLE_TYPE_FRACTURABLE_CHILD_BREAKABLE
teMAP_PLACEABLE_TYPE_SH_PROXY_VOLUME  teMAP_PLACEABLE_TYPE_DIFFUSE_GI_VOLUME
teMAP_PLACEABLE_TYPE_TRIGGER_VOLUME    MAP_PLACEABLE_DEPLOYMENT_ZONE_VOLUME
```

Chunk kinds: `MAP_CHUNK_COLLISION`, `MAP_CHUNK_PATHING`,
`MAP_CHUNK_SOUNDAREA`, `MAP_CHUNK_DYNAMIC_SPAWN_GROUPS`,
`teMAP_CHUNK_ADDITIVE_COLLISION`, `teMAP_CHUNK_ADDITIVE_PATHING`,
`teMAP_CHUNK_STREAMING`.

The chunk's asset GUID holds the kind id in bits 32..39 (the reference's
`GetChunkKey`). This is what lets the reader parse a chunk by its declared
kind instead of guessing.

## Placeable kinds

Walking each placeable by its declared size (the reference reader's
`teMapChunk.cs` walk) over the install:

| id | kind | assets read | placeables | reference fields |
|---|---|---|---|---|
| `0x01` | model group | 18 | 6,880 | 15,475/15,475 model refs |
| `0x04` | reflection point | 17 | 1,862 | no GUID fields |
| `0x07` | text | 2 | 98 | 294/294 |
| `0x09` | light | 17 | 9,076 | 2,090/2,090 |
| `0x0A` | area | 17 | 587 | no GUID fields |
| `0x0B` | entity | 20 | 39,156 | **0/39,156 resolve** |
| `0x0C` | sound | 17 | 2,641 | 2,641/2,641 |
| `0x0D` | effect | 15 | 2,734 | 2,734/2,734 |

Two boundaries:

- **No `0x08` bucket exists on this install.** The reference's
  `MAP_PLACEABLE_MODEL` (id `0x08`) matches no sampled chunk. The flat "Model"
  reading is the `0x01` model group.
- **The entity body is open.** Its 39,156 reference-shaped qwords never
  resolve to an asset, so the field is not a GUID — either the layout differs
  or the values are runtime ids. The size walk itself is exact (every
  placeable ends where the next begins).

Reader: `ow-tank::mapchunk` (`Kind::from_type_id`,
`MapChunk::parse` takes the declared kind). The per-kind numbers are pinned by
`crates/ow-tank/tests/mapchunk_kinds.rs`.

### The entity component vocabulary is data (2026-11-03)

An entity placeable's body is a table of component-instance-data records, each
`{u32 component id, u32 relative offset}` (`ow_tank::mapchunk::InstanceDataRef`).
The word is **not** an STU class hash: it is a small component id, and the STU
asset at `record offset` is the component's data, whose **root instance hash
names the class**. `crates/ow-tank/examples/map_entity_probe.rs` reads the
practice range this way: 34 chunks, **550 entity placeables, 644 records, 13
distinct ids**, 475 of the entities carrying the model component.

| id | root class hash | class | entities |
|---|---|---|---|
| `0x02` | `0x02DEEEA0` | `STUModelComponentInstanceData` | 475 |
| `0x03` | `0x75526BC2` | `STUStatescriptComponentInstanceData` | 47 |
| `0x06` | `0xB1B9C667` | `STUWaypointInstanceData` | 10 |
| `0x09` | `0x83DEC8C7` | `STU_83DEC8C7` (unnamed) | 6 |
| `0x0b` | `0x64100592` | `STUTargetTagInstanceData` | 49 |
| `0x0d` | `0x0993C1FE` | `STU_0993C1FE` (unnamed) | 4 |
| `0x0f` | `0x50CC8187` | `STU_50CC8187` (unnamed) | 8 |
| `0x10` | `0x99257985` | `STUECAISpawnerInstanceData` | 1 |
| `0x11` | `0xF199249B` | `STUECHintPointInstanceData` | 30 |
| `0x17` | `0x5AD3B86B` | `STU_5AD3B86B` (unnamed) | 4 |
| `0x1a` | `0xA53894DC` | `STU_A53894DC` (unnamed) | 4 |
| `0x1e` | `0x139CCB2F` | `STU_139CCB2F` (unnamed) | 4 |
| `0x20` | `0x7D175935` | `STU_7D175935` (unnamed) | 2 |

The classes are the reference checkout's names for the data; nothing in this
table is shipped here. Every named component's stream parses as a full STU v2
asset at its offset, so the map's component data is readable in place — the
engine class behind each component id is the remaining link, and
`docs/formats/static-collision.md` now ties the collision builder's second
argument (the 0x300-byte scene component) to the model component hypothesis.

## Spawn records and the practice range's spawn room (2026-11-09)

**Data: read. Selector: open.** The map's spawn-bearing chunks are kinds
`0x16` (`MAP_PLACEABLE_TYPE_SPAWN_GROUP`), `0x19`
(`MAP_PLACEABLE_SPAWN_VOLUME`), `0x1E`
(`MAP_PLACEABLE_TYPE_DYNAMIC_SPAWN_POINT`) and `0x1B`
(`MAP_CHUNK_DYNAMIC_SPAWN_GROUPS`). On the practice variation
(`0x080000000F69002E`, map id `0x0F69002E`) the spawn-volume, dynamic-point and
dynamic-group chunks are the empty 16-byte header, and the 25 kind-`0x16`
records are all AI bot groups (`Enemy Sniper Bot`, `Friendly Firing Bot`, ...).
**The practice map ships no player spawn record.**

The map's two `teMAP_PLACEABLE_TYPE_TRIGGER_VOLUME` (kind `0x17`) placeables
are readable. Each begins with an STU blob at `+0x64` (a 4-byte payload length,
then `STUD`), followed by an oriented box at `+0xA0`:

| offset | type | meaning |
|---|---|---|
| `+0xA0` | `f32[3]` | unit yaw axis `(x, 0, z)` |
| `+0xAC` | `f32[3]` | box centre, world space |
| `+0xB8` | `f32[3]` | half extents `(x, y, z)` |

The practice volumes are `(71.169, 0.948, 53.110)` with half extents
`(11.861, 5.179, 7.608)` and `(58.663, 15.800, -22.654)` with half extents
`(3.500, 9.000, 9.250)`. The first centre lies inside the
`MAP_PLACEABLE_AREA` box centred `(70.546, 0.498, 53.797)` (half extents
`(12.663, 6.326, 8.184)`); the second lies inside none. The first volume is the
hero-select room: it contains the `Change Hero` (`(66.913, -0.937, 45.648)`),
`Settings` (`(80.274, -2.139, 53.563)`), `Reset` (`(78.632, -2.139, 55.879)`)
and `Activate` (`(68.149, -4.000, 56.987)`) text placeables, and the collision
floor under its centre is `y = -4.006` (the box's own bottom is `-4.231`).

`ow-tank::mapresolve::spawn_room` reads that volume (the trigger-volume centre
inside an area box), `ow_tank::range_world::RangeWorld` carries it, and
`ow-client` starts its walker on the floor under the volume centre. `--spawn x
y z` overrides the point. This is a **declared viewer default** for the open
spawn-selection stage, not a read of the client's selector. Install test
`spawn_room_real` pins the volume.

## The model and look join

A placement carries two references:

| offset in placeable | value | type |
|---|---|---|
| `+0x00` | the model | `0x00C` |
| `+0x20` | the model's **look** | `0x01A` |

Every one of 14,093 placements in a 40-chunk census names a look, and all 8,331
distinct looks resolve. A look is a `STUModelLook` (`0x5207484B`); its first
field (`0xBAFDAFBA`) is an inline array of `STUModelMaterial` (`0x494B66C1`):

| element field | meaning |
|---|---|
| `0x33E51FDC` | `m_material` — the material asset (`0x008`) |
| `0xDC05EA3B` | `MaterialID` — the id the model names it by |

The join is the model's material id against the look's id. The full chain to
a texture is in `formats/materials.md`.

## Map headers and the map-to-chunk link

The client's `STUMapHeader` is a component registered at `0x14031c580` with
class hash `0x02514B6A` and in-memory size `0x240` (576 bytes). Its assets are
tool type **`0x09F`** (GUID prefix `0x0790...`), not type `0x079`. The client's
type table, checked against the install, lists 305 type-`0x09F` assets. All 305
records are present in both base manifests (plain and Prism) of both regions and
in the enUS text and speech manifests; 42 have a
payload in the encoded store and parse as class `0x02514B6A` with `mapName`
(field hash `0x506FA8D8`) and a map reference (field hash `0x4E87690F`). The
other 263 have no encoding entry at all — their payloads are in **bundles**
(below).

The publisher's type table labels tool type `0x079` (GUID prefix `0x01e0...`)
`VoiceCategory`. It is **not** `STUMapHeader`. The 25 type-`0x079` records are
not in the map chain.

### The chain

```text
map selection
  -> STUMapHeader (type 0x09F, class 0x02514B6A)
  -> a paired m_D97BC44F mode entry / m_78715D57 variation
  -> the variation's m_BF231F12 scene map asset (type 0x002)
  -> chunk GUIDs: 4 stored references + the client's synthesised slots
```

The chunk GUIDs are not stored in the map header. The client computes each one
in `FUN_141b32e20` (`0x141b32e20`):

```text
guid = 0x0DD0000000000000
     | (chunkKind[typeIndex] << 32)
     | (mapGuid & 0xFFFFFFFF)
```

`chunkKind[]` is the 0x24-entry table at `0x1439aa320`, stride `0x20`, kind at
`+8`; `FUN_141b32e70` (`0x141b32e70`) maps a chunk GUID's `(guid >> 32) & 0xFF`
back to the same table. The map loader `FUN_141ff5c80` (`0x141ff5c80`) takes
the selected map GUID from `+0x70` of the loaded map object and calls
`FUN_141b32e20` for type indices `0x1F`, `0x20`, `0x23` and `1..=0x1C`, loading
each resulting GUID.

Those indices are **table slots**, and they are not every chunk a map ships.
The table's slot → kind mapping is read from the image, and four kinds sit
outside the loader's set:

| slot | kind | chunks in the manifest |
|---|---|---|
| `0x1D` | `0x11` | 305 |
| `0x1E` | `0x12` | 305 |
| `0x21` | `0x10` | 82 |
| `0x22` | `0x23` | 323 |

1,015 of the manifest's type-`0x0BC` records are outside the loader's 31 slots.
They are not a second path: the map asset stores those GUIDs itself.
`LoadMapAssetChunks` (`0x141ff5c80`) reads, in this order:

```text
+0x1B8         u64   one chunk GUID (kind 0x11 on every map seen)
+0x1B0/+0x1B4  u32 count, u32 relative offset -> count more chunk GUIDs
+0x1C0         u64   one chunk GUID (kind 0x12 on every map seen)
+0x1C8         u64   one chunk GUID (kind 0x23 on every map seen)
then slots 0x1F, 0x20, 0x23 and 1..=0x1C through FUN_141b32e20
```

A stored value of zero is the client's "nothing here" (`FUN_141fd3950`,
`0x141fd3950` returns `-1` and the loader does not register the handle). The
`+0x1B0` list is the only slot for kind `0x10`; kinds `0x11`, `0x12` and `0x23`
have fixed fields and can also appear in the list. `ow-tank::mapresolve` reads
this layout and reproduces the loader's order; `tests/map_resolve_real.rs`
walks the practice range's 34 chunks.

### The loader's runtime fields in the map blob

The map asset is a **plain binary** (type `0x002`), and the loader writes its
runtime state into the same buffer beyond the shipped fields. `0x141ff5c80`
stores one asset **handle** per requested chunk in the blob:

| handle | source |
|---|---|
| `+0x1D0` | `+0x1B8` (kind `0x11`, collision) |
| `+0x1D8` | `+0x1C0` (kind `0x12`, pathing) |
| `+0x1E0` | `+0x1C8` (kind `0x23` on every map seen) |
| `+0x1F0` | slot `0x1F` (kind `0x13`, sound area) |
| `+0x1E8` | slot `0x20` (kind `0x1B`, dynamic spawn groups) |
| `+0x1F8` | slot `0x23` (kind `0x27`, streaming) |
| `+0x208 + 8*i` | slot `1..=0x1C` (the scene/placeable chunks) |
| `+0x2E8` | the `+0x1B0`/`+0x1B4` extra-GUID list |

The chunk completion `0x142e49400` (registered on each request through
`0x140550660(handle, callback, map, 0)`) then decodes every handle
(`handle & ~0x3f`, the tag check `^ [block+8] & 0x3f`, payload at `[block]`)
and writes the **payload pointers** into the blob:

| payload field | chunk |
|---|---|
| `+0x2F8` | kind `0x11` collision |
| `+0x300` | kind `0x12` pathing |
| `+0x308` | kind `0x23` streaming/sub-asset |
| `+0x310` | slot `0x20` |
| `+0x318` | slot `0x1F` |
| `+0x320` | slot `0x23` |
| `+0x2F0` | the extra-chunk payload array (one per `+0x2E8` GUID) |

`+0x410` is set when the map is loaded (the predicate `0x141ff5bd0` tests it and
runs `0x142e48c50` -> `0x142e49400` when it is still zero). The chunk payloads
are therefore reachable from the map blob without any further asset lookup, and
`+0x2F8` is the collision chunk's handoff point to its consumer.

Proof against the install: the map asset `0x0800000000F47508` (type `0x002`,
2928 bytes) is readable, and the manifest holds exactly the 31 chunk GUIDs the
client's table predicts for it (`0x0DD0000100F47508`, `0x0DD0000300F47508`,
... `0x0DD0002800F47508`), with the same 31 kind ids. The manifest's 41,811
type-`0x0BC` assets are these synthesised chunks plus the stored ones.

Proof against the client's bytes: `dev/verify_harness/units/fn-ChunkGuid.json`
(unit `fn/ChunkGuid`) compares `ow_tank::mapguid::chunk_guid` with
`MapChunkGuid` at `0x141b32e20`. The probe reads the kind from the same
stride-`0x20` slot of the fixture table that the oracle indexes, then calls
the shipped function; both sides read identical bytes. The unit passes 364
differential cases on every channel, reports 100% coverage (the function has
no branches) and kills 3/3 viable mutants.

The shift in the image is a 64-bit register shift, so only the kind's low 32
bits reach bits 32..63; the map argument is a `uint`, so only its low 32 bits
are used. The former `ow-tank` mask `kind & 0xFFFF` was wrong for kind
`0x1_0000` and was removed.

### Base map and variation map

A `STUMapHeader` names two different type-`0x002` kinds of map asset:

| header field | offset | meaning |
|---|---|---|
| `m_map` (`0x4E87690F`) | `+0xA0` | the base map: map bounds/settings and the collision/pathing chunks |
| `m_364ADD04` | `+0x150` | the header's layer key (a type-`0x002` GUID with bit 40 set; not registered as an asset) |
| `m_D97BC44F` | `+0xF0` | one `STU_71B2D30A` mode entry per variation |
| `m_78715D57` | `+0x110` | one `STU_7FB10A24` variation per mode entry |
| mode entry `m_A9253C68` | mode `+0x38` | the map-variant **identifier** (`STUIdentifier`, class `0x9CADF2EC`: `0xD800000000000000 \| u32`, not an asset) |
| variation `m_BF231F12` | variation `+0xD0` | that variation's **scene** map asset (a registered type-`0x002` asset) |
| variation `m_364ADD04` | variation `+0xC0` | that variation's layer key |

The offsets are the ones the client registers at `RegisterStuMapHeaderFields`
(`0x14143b5c0`) and `RegisterStuMapHeaderTail` (`0x14143bbe5`);
`STU_7FB10A24` is registered at `RegisterStuMapVariation` (`0x14142dbf0`), and
`STU_71B2D30A` at `RegisterStuMapVariationMode` (`0x14142ee0c`) where
`m_A9253C68` is registered through `FUN_140f81de0` with class `0x9CADF2EC`
(`STUIdentifier`, 8 bytes). `m_78715D57` and `m_D97BC44F` are paired **by
index**, which their lengths prove on the install: the practice header has
3/3, Cairo 10/10, and header `0x07900000000000d4` 63/63. `m_C0CA4671` is a
different display list: 21 entries for that 63-variation header, and on Cairo
its 10 modes are in another order.

The identifier census (all 305 headers, 1,612 mode entries) finds **30 distinct
identifiers**. The practice header's three entries all carry
`0xD800000000008F9D`; header `0x07900000000000d4` carries three
(`...A039`, `...A062`, `...A0C4`), 21 entries each over 20 modes. A variation is
therefore a `(m_gameMode, m_A9253C68)` pair, not a mode alone. The pair is
still not unique: on that header mode `0x0230000000000016` names six entries,
two scene maps per identifier (different scene map and `m_resourceKey`). No
mode-only or mode-plus-identifier rule can select one scene map.

Cairo is the case that separates the two map GUIDs: the header's `m_map` is
`0x080000000000005B`, which has only the two collision/pathing chunks (kinds
`0x11` and `0x12`), while its ten variations name ten different
`m_BF231F12` assets. Variation 0's `0x08000000EB17A0F1` is 2144 B and has the
31 scene chunks under its own index plus the base map's two collision GUIDs at
`+0x1B8`/`+0x1C0`. `m_364ADD04` is `0x080001000000005B` for every Cairo
variation, so it is a layer key, not the scene asset.

For the practice range the two indices coincide, so both formulas produce the
same 31 scene GUIDs; the resolver uses `m_BF231F12` because Cairo proves that
is the asset the loader actually opens.

### Who chooses a variation: not the client

The current image contains **no function that reads a header's mode or
variation array**. This is a static-search result, not an inference from one
call site. An index-based reader (field order rather than field hash) cannot be
excluded by a hash search alone; the offset-pattern scan below covers the
native-layout case, and the data shows that even such a reader could not pick
one scene map from a mode (or from a mode plus identifier).

- `m_D97BC44F` (`0xD97BC44F`) occurs in `.text` only at `0x14143ba49`, inside
  `RegisterStuMapHeaderFields` (`0x14143b5c0`).
- `m_78715D57` (`0x78715D57`) occurs only at `0x14143bc95`, inside
  `RegisterStuMapHeaderTail` (`0x14143bbe5`).
- The class hash `0x02514B6A` occurs at only two addresses: `0x14031c5e2`
  inside `RegisterStuMapHeader` (`0x14031c580`) and `0x1403216b8` inside the
  lazy asset-type descriptor init at `0x1403216a0`, which stores tool type
  `0x09F` at `0x143ed6948` and class `0x02514B6A` at `0x143ed694c` for the STU
  asset loader. Neither is a consumer.
- The registered field descriptors (`0x143ed7580` for `m_D97BC44F`,
  `0x143ed75f0` for `m_78715D57`, `0x143ed68b0` for `m_BF231F12`) and the class
  definition at `0x143eebf70` are referenced only by the registration and
  initialisation functions. A consumer would have to reference one of them; no
  such reference exists.
- A whole-image instruction scan (10,946,277 instructions) found **no**
  function that touches `+0xF0` and `+0x110` on one base register together with
  the element stride (`0x58` or `0xD8`) and one of the header's other
  distinctive offsets (`+0x150`, `+0x170`). Code that baked the native array
  offsets would need exactly that shape. The scan is a check on the
  hash-reference result, not the only proof.

What the client does instead is load the map it is given:

1. **The game's map identity is the base map.** The replay header's map field
   (`replay.rs`, block `+0x08`) holds a type-`0x002` GUID. The install check
   pins `0x080000000000069E` as the `m_map` of header `0x079000000000069E`,
   and as the `m_BF231F12` of none of the 305 headers
   (`tests/map_resolve_real.rs::a_replay_map_guid_is_the_base_map_not_a_scene_map`).
   The map catalog (`STUMapCatalog` `0x25DA0DF5`, asset
   `0x01c0000000000041`) pairs every header GUID with that base-map GUID.
2. **Level load is a message.** Jam message `0x501B` is dispatched by the
   opcode switch at `0x14063b3f0`; its handler `0x140618b30` copies four GUID
   vectors from message offsets `+0x78`, `+0x90`, `+0xA8`, `+0xC0` (the message
   field descriptors at `0x143b437f0` name those four offsets) and calls the
   preload at `0x14077b7d0`. The preload registers the `+0x78`/`+0x90` lists
   into GUID tables (`FUN_140847be0`, `FUN_140847cc0`), requests the
   `+0xA8`/`+0xC0` lists as assets (`FUN_141fe1610`) and opens the map catalog
   (`FUN_14077c8a0`). The `+0x78` table is filtered later by `FUN_140843800`,
   which loads each GUID and reads flags at `+0x388`/`+0x389` of the loaded
   object.
3. **The variation is already resolved when the message is built.** The
   client's read path never sees a `(mode, variant)` request; it sees GUIDs.
   Who builds the message (the game server, or the local session setup for an
   offline practice range) is the remaining question, and it is outside the
   code path read here. See `docs/open-questions.md` item 11.1.

The practical consequence: `ow-tank` must not invent a mode→variation selector.
The arrays are a data pairing; a consumer has to be handed the scene map (or at
least the exact variation index) by the producer of the level-load message.



### Where a header payload comes from

A type-`0x09F` record's bytes are not always in the encoded store. The CMF
record's byte at `+12` (the `unknown` field in `manifest.rs`) tracks where the
payload lives on this install. Values `0..6` are in the store; the same values
with bit 7 set (`128..134`) are bundle-hosted. On a 150-record sample per value,
the low classes read 149–150/150 and the high classes 0–12/150. The flag wins
when both exist: the practice-range map asset is bundle-hosted **and** has an
encoding entry, and the client reads the bundle. For map headers the split is
exact:

| record byte | headers | payload location |
|---|---|---|
| `4` | 42 | encoded store (`open_ckey` succeeds) |
| `132` (`4 \| 0x80`) | 263 | a bundle entry table; no encoding entry |

The 263 are not missing: they are in bundle assets. The manifest that registers
them is the same one this project opens (`Win_SPWin_RDEV_EExt.cmf`, ckey
`06FE7940166B50A134509E172B02135D`, build 154504); the difference is the
container lookup, not the manifest.

#### The client's flag chain (read)

`docs/formats/casc.md` records the record byte itself. The client turns it into
its bundle decision in three steps:

1. **Manifest merge.** `FUN_141feff80` (`0x141feff80`) copies a raw 29-byte
   record into the in-memory 32-byte record and keeps the class bits in the low
   byte of the flags dword at `+0xc`. Record byte bit 7 is moved to **flags bit
   16** (`(byte[12] & 0x80) << 9`). The same function sets bit 17 and bit 19
   when the content key is all zero.
2. **Query.** `FUN_141fef610` (`0x141fef610`) returns `{size, class byte, flags,
   content key, manifest}` for a GUID and maps in-memory flag bit 16 to **result
   flag bit 9** (`0x200`), i.e. `out_flags & 0x200` is exactly
   `record.unknown & 0x80`.
3. **Load.** `FUN_141fd4a50` (`0x141fd4a50`) asks the query. A record with bit 9
   clear goes to the encoded store (`FUN_141fd3950`, `0x141fd3950`); a record
   with bit 9 set goes to the **bundle directory** (`DAT_1441b3b88`) instead,
   and there is no fallback to the store even when the content key happens to
   have an encoding entry.

#### The bundle entry table (read)

A bundle is a type-`0x09C` asset (GUID prefix `0x0D90...`). Its payload is read
by `FUN_141fd5820` (`0x141fd5820`):

```text
+0x00  i32  entry count
+0x04  u8   offset width (1, 2 or 4; any other value has no stride)
+0x05  entry * count
         u64 GUID
         offset, `width` bytes, relative to the start of this payload
```

- The entries are **GUID-sorted**; the client binary-searches them.
- Offset `0` means "not in this bundle" and the lookup fails.
- The lookup carries **no length**. The asset's length is the CMF record's
  declared size, which is why the caller passes that size separately
  (`FUN_141fd4a50` → the record's `size`).
- The client does not bound-check the offset against the payload. `ow-tank`
  reports an out-of-range span instead of reading past the buffer; on a valid
  install this cannot change a result.

The search is a verified harness unit: `fn/BundleLookup`
(`dev/verify_harness/units/fn-BundleLookup.json`) compares the shipped
`ow_tank::bundle::offset_of` and the client-contract wrapper
`bundle::entry_lookup` with `BundleEntryLookup` at `0x141fd5820`. It passes 364
synthetic differential cases on every channel, 100% branch coverage (29/29
reachable branches of 35 sites; six sites declare one structurally dead edge)
and kills 12/12 mutants. The read settled the bounds: the client walks `0` to
`count - 1`, not the usual half-open `[0, count)`, and takes the midpoint as
`(lo + hi) >> 1`. The old half-open walk agreed on every GUID-sorted install
table but returns a different answer on an unsorted one, so `offset_of` now
takes the client's walk operation for operation. The unit does not prove the
producer of the directory blob below.

#### The bundle directory (format read, producer open)

The client does not scan bundles per request; it keeps a per-type directory
(`DAT_1441b3b88`). The job that builds the asset requests from it is
`FUN_141fd53a0` (`0x141fd53a0`) and the GUID lookup is `FUN_141fd5160`
(`0x141fd5160`):

```text
type block          u16 type at +0xe, u16 group count at +0xc, next-block
                    stride at +8 (u32), group array at +0 (base-relative)
group (0x28 B)      +0x00 GUID list        (count: i16 at +0x24)
                    +0x08 sorted GUID array (count: i32 at +0x20)
                    +0x10 u16 index array, one per sorted GUID
                    +0x18 the bundle's own GUID
```

`FUN_141fd5160` finds the type block, binary-searches the group's sorted GUIDs,
and returns the request handle of the group's bundle. The client hands this blob
to the job from its content database (`FUN_141fcbb10` → `FUN_141fe81d0`, fields
`+0x70` and `+0x264`). **Where that blob is produced is not read**; see
`docs/open-questions.md` item 11.2.

The lookup is a verified harness unit: `fn/BundleDirectoryLookup`
(`dev/verify_harness/units/fn-BundleDirectoryLookup.json`) compares the shipped
`ow_tank::bundle::directory_lookup` with `BundleDirectoryLookup` at
`0x141fd5160`. It passes 364 synthetic differential cases on every channel and
93.3% branch coverage (14/15 reachable branches of 18 sites: three sites
declare one structurally dead edge each, and one guarded edge is a named gap);
the mutation stage kills 35/35 scored mutants (one further type-count gate flip
is declared equivalent, and one signature-line mutant does not compile and is
counted killed by the harness).
Every compared byte is synthetic; the state region places the directory object
at its own image address (`0x1441b3b88`) because the function reads the
type-entry pointer twice, once through the object's `+0x18` and once through
the fixed qword at `0x1441b3ba0`, which is that same address. The blob the
lookup walks carries absolute guest pointers: `FUN_141fd53a0` rebases the
producer's base-relative fields before the lookup runs, so the model follows
the post-fixup form.

What the unit added to the read:

- The object is a type-entry array at `+0x18` and an `i32` type count at
  `+0x20`; an entry is `0x20` bytes (`+0` `i32` key, `+8` type block, `+0x10`
  bundle-handle array). The block's `+8` stride and `+0x0e` type id are not
  read by the lookup.
- The type key is the asset type id: the bit-reversed 12-bit field at GUID
  bits 48..59, plus one, sign-extended from 12 bits. It is the same value as
  `ow_tank::guid::type_of` except field `0xFFF`, where the client's 12-bit
  sign extension wraps to `0`; a type-`0x09C` bundle GUID keys to `0x09C`. A
  first read of the mask agreed on every `0x0D90...` GUID and was wrong for a
  GUID with a non-zero top nibble; the harness caught it on a random one
  (`0xA72C...` → `847`).
- A hit's index entry is its position in the bundle's listed GUID array, and
  the running sum of the preceding groups' `i16` listed counts (`+0x24`) makes
  one handle array span all the groups of a type. A loaded handle (status 4)
  returns at once; the first unready handle is remembered and a later loaded
  group wins; no hit leaves the client's absent value `-1`.
- The handle state comes from `FUN_141fd3970`: a `-1` handle, a handle that
  masks to zero and a low-six-bit tag mismatch are unusable; `obj+0xa` bit 0
  clear means unready; `obj+8` bit 15 set means known-but-not-loaded; otherwise
  the handle is loaded. The decompiler printed the last test as
  `& 0xffffff01`; the disassembly is `SHR EAX,0xf` then `AND AL,1`, and the
  unit's first differential run diverged on a bit-31 object because of it.
- The loaded-bundle guard is `FUN_141fcbf40`. Only its GUID-`0` answer is
  reachable in emulation: a non-zero GUID tail-calls `FUN_141fcf330` with a
  pointer derived from the runtime-patched qword at `0x1439aef30`, and the
  shipped image's value yields a null base, so the oracle faults before the
  guard can answer "not loaded". The unit declares that false edge as a named
  acknowledged gap and sets its coverage gate to 93%; the gap is not a silent
  exclusion.

This verifies a **consumer** of the directory blob, not where the blob comes
from: the producer row in `ow-tank-1to1.md` section A stays open.

#### What the producer side reads as (2026-10-31; blob source narrowed)

The job and the hand-off around it are read operation for operation; the
*writer* of the blob is still not read. What is settled:

- `BundleDirectoryRequestAssets` (`0x141fd53a0`) is phase 5 of the bundle
  job (dispatched by `FUN_141fd48f0` at `0x141fd48f0`; phase 2 teardown is
  `FUN_141fd5700` at `0x141fd5700`). It takes the blob pointer from the job's
  `+0x28` slot and the type count from the content database (`FUN_141fcbb10`
  at `0x141fcbb10` → `FUN_141fe81d0` at `0x141fe81d0`, fields `+0x70` blob and
  `+0x264` count, moved out and zeroed), sizes the job's type-entry vector
  (`FUN_141fd5020`, `FUN_141fd4920`), rebases every base-relative pointer in
  the blob in place, then requests every listed GUID (`FUN_141fd3950`).
- The per-asset request `AssetRequestBundleAware` (`0x141fd4a50`) sends a
  bundle-hosted record (query flag `0x200`) to `BundleDirectoryLookup` and
  `FUN_141fd6a50`, everything else to the store (`FUN_141fd3950`).
- The content database publishes `+0x70`/`+0x264` in `FUN_141feb350` (same
  body in `FUN_141feb4a8`/`FUN_141feb69e` at `0x141feb4a8`/`0x141feb69e`): it
  copies four sections verbatim out of a content stream (sizes and counts
  from the stream's `0x58`-byte header, magic check in `FUN_141feb9e0`) into
  `+0x78` (`0x1e`-stride records), `+0x160` (variable-stride bundle records,
  rebased and hash-inserted via `FUN_141fe79a0`), `+0x58`/`+0x50`, and the
  `+0x70`/`+0x264` blob (`FUN_141feb190` is the dup). The grouping, the sorted
  GUID arrays, the index arrays and the base-relative emission are **not**
  computed here: they arrive prebuilt in the stream.
- The `+0x160` readers are `FUN_141fe89a0` (first entry of a type) and
  `FUN_141fe8f90` (GUID match, then the `FUN_141fcbf40` loaded-bundle guard
  on element `+0x10`); the collector `FUN_141fea840` copies loaded-bundle
  elements into a flat `0x20`-stride array with a compact pass.

So the blob is **not** assembled from bundle payloads anywhere in the read
path: it is copied out of the content stream. The remaining question is who
writes that stream (its group/sort/emit and its 0x58-byte header source).
Until that writer is read, `ow-tank` keeps its manifest scan and the
producer row stays open.

What `ow-tank` does: `GameData::open_asset` implements steps 2–3 and the entry
lookup exactly, and builds the directory's *content* by scanning the manifest's
type-`0x09C` records once (`BundleIndex`). That is the flat form of the
client's per-type directory — same GUID → (bundle, offset) relation — but the
*producer* is our scan, not the client's blob. This is the one non-1:1 stage in
the path and it is recorded as open.

Install check (read-only scan of the base manifest's type-`0x09C` records):

```text
38,316 records on prefix 0x0D90
38,313 parse with that layout; widths 1:461, 2:37,303, 4:549
598,302 entries in total
263 of the 305 map-header GUIDs appear in an entry list — exactly the 263
  without an encoding entry; none of the 42 store-resident ones does
five bundles hold them: 0x0d90847bfc92c67c, 0x0d90892e32cab495,
  0x0d90968e484f8c33, 0x0d90b51ea991b1ad, 0x0d90bfbbabedd823
all 263 slices parse as class 0x02514B6A; 263 distinct `mapName` values
```

The same scan through `BundleIndex` over the base **and** text manifests (the
registry `GameData` holds) reads 38,320 bundles, 0 malformed tables and 721,204 entry rows, with **0 duplicate
GUIDs** — every asset lives in exactly one bundle, so "first bundle wins" cannot
differ from the client's directory order. All 305 type-`0x09F` headers open
through `GameData::open_asset`: 42 from the store, 263 from bundles, 305/305
parsing as class `0x02514B6A` with a `mapName`.

### The practice-range header

```text
GUID          0x0790000000000688
CMF record    1132 B, record byte 132, ckey c53097e194cd50fcfcc17b43eeee10ea
store         no encoding entry in any manifest; no CDN route in any local index
bundle        0x0d90968e484f8c33, entry 1, offset 3593
bytes         1132 B at 3593..4725 of the 203,817-byte bundle payload
parse         STUMapHeader, mapName "Maps\TrainingRoom\Training Room - Practice",
              m_map 0x0800000000000688, m_mapType 0
```

All of that is reproduced by `ow-tank`'s own implementation of the client path:
`tests/map_header_bundle_real.rs` opens the header, finds bundle
`0x0d90968e484f8c33` at offset `3593`, parses class `0x02514B6A`, reads
`mapName` and `m_map`, and opens the map asset.

The map asset (`0x0800000000000688`, 1056 B) is **bundle-hosted too**: its
record byte is `134` (`6 | 0x80`), not `6`. An earlier note here said it was in
the encoded store; that was wrong. It is also readable through the same path,
and its record's content key does have an encoding entry (a 154-byte stored
record), which the client does **not** use because the flag wins. The two chunk
assets the manifest registers under that index (`0x0dd0001100000688`,
`0x0dd0001200000688`) are in the encoded store (record byte `6`).

This base map is not the scene. It has no stored chunk GUIDs and its 31
synthesised GUIDs are absent from the manifest; its index carries the two
collision/pathing chunk records the variations name explicitly.

#### The practice-range variations and scene

The header's `m_78715D57` has three entries, paired with `m_D97BC44F` by index.
The variant name text asset (`m_0342E00E.m_D978BBDC`, `0x0de00000000185d1`) is
`"Evening"` for all three.

| index | mode (`m_gameMode`) | variant (`m_A9253C68`) | mode name | layer key (`m_364ADD04`) | scene map (`m_BF231F12`) |
|---|---|---|---|---|---|
| 0 | `0x0230000000000018` | `0xD800000000008F9D` | Practice Range | `0x080001000f69002e` | `0x080000000f69002e` |
| 1 | `0x0230000000000143` | `0xD800000000008F9D` | Stadium | `0x0800010038792434` | `0x0800000038792434` |
| 2 | `0x023000000000005a` | `0xD800000000008F9D` | Practice Range | `0x08000100878409c8` | `0x08000000878409c8` |

The three mode entries share one map-variant identifier, and the identifier is
not a registered asset (`tests/map_resolve_real.rs` asserts both). The two
"Practice Range" modes are distinct entries; `ow-tank::mapresolve` exposes the
pairing, the per-mode filter and the named practice-range query below, but
none is claimed as the client's own selector.

#### The launch's selection rule (a viewer default)

`ow-tank::mapresolve::find_practice_range` is a data query, not a seed:

1. Enumerate every type-`0x09F` record and open it through
   `GameData::open_asset` + `STUMapHeader` (all 305 open on this install).
2. Keep the one whose `m_mapName` is byte-equal to
   `Maps\TrainingRoom\Training Room - Practice`; zero or more than one is an
   error, never a guess.
3. For each paired `STU_71B2D30A`, open its `m_gameMode` (`0x023000...`)
   as `STUGameMode` (`0x494C33C4`) and read `displayName` (`0x5DB91CE2`), a
   `teString` asset.
4. Take the first variation, in header order, whose display name is exactly
   `Practice Range`.

On the practice header two entries name `Practice Range` — mode `0x18`
(variation 0) and mode `0x5a` (variation 2) — and the rule takes variation 0.
That first-in-header-order choice is a **viewer default**, not the client's
rule: the client never reads this header at runtime and is handed the map GUID
by the level-load message (see "Who chooses a variation"). `--list-maps` prints
both rows, so the ambiguity is visible. The input side remains item 11.1 in
`docs/open-questions.md`.

`ow-client` uses `find_practice_range` for both the default launch and
`--practice-range`. A failure (no header, duplicate header, no matching mode,
or a requested chunk that is unregistered/unopened) exits non-zero with the
resolver's reason. It never falls back to the sandbox or to a first-N chunk
list.

#### Model-group geometry

The practice variation's kind-`0x01` chunk (252,816 B) holds 229 model-group
placeables. A group placeable's own translation/scale/rotation is identity; its
`teMapPlaceableModelGroup` body carries one model, then 304 groups (each with a
look) and 2,449 entries (each with its world transform). `ow-scene` expands the
entries, groups instances by `(model, look)`, and joins each submesh's `MDLC`
id to the look's `MaterialID`. Measured on the practice range: 229 distinct
models, 2,449 placed instances, 872 draws, 289 looks, 120 base-colour textures
decoded, 817 textured submesh draws, 55 placeholder draws (148 flat-fill and 3
no-sRGB material entries are reported, not painted). The resolver's 1,358
placeables include non-model kinds (550 entity, 143 light, 193 effect, ...);
only the model kinds contribute geometry.

`tests/map_resolve_real.rs` opens mode `0x18`'s variation 0 and its scene map
`0x080000000f69002e`. The map asset is 1440 B and stores three chunk GUIDs in
the loader order:

```text
+0x1B8  0x0dd0001100000688   kind 0x11, under the base map's index 0x0688
+0x1C0  0x0dd0001200000688   kind 0x12, under the base map's index 0x0688
+0x1C8  0x0dd000231aaeb587   kind 0x23, under 0x1aaeb587
+0x1B0   0 extra GUIDs
```

The 31 synthesised chunks under `0x0f69002e` are exactly the manifest's type-
`0x0BC` records for that index, one per loader slot (`0x01, 0x03, 0x04, 0x06,
0x07, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x13, 0x15, 0x16, 0x17,
0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x20, 0x21, 0x22, 0x24, 0x25,
0x26, 0x27, 0x28`). The full request is 34 chunks and all 34 open:

```text
requested       34
opened          34
placeables      1,358  (229 naming a model, 550 naming an entity)
bounds min      (-82.19081, -32.118107, -138.51625)
bounds max      (189.49182, 61.90211, 101.723595)
```

The bounds are pinned bit-exact in the test. Kind `0x01` (model group) carries
229 placeables; kind `0x0B` (entity) carries 550; the 21.8 MB kind-`0x11` and
8.8 MB kind-`0x12` chunks carry no placeables — they are the collision/pathing
payloads, shared with the base map.

Variations 1 and 2 are byte-identical to each other in all 31 chunk content
keys; variation 0 differs in kind `0x0B` (202,688 vs 336,208 B) and kind `0x16`
(43,296 vs 60,288 B). The model-group chunk (kind `0x01`) is shared by all
three, so the geometry is the same and the entity/spawn set is what makes the
practice and Stadium variations different.


### OWLib's path (reference only)

OWLib reaches map headers through `ListMaps` -> `TrackedFiles[0x9F]` ->
`MapHeader.Load` -> `GetInstance<STUMapHeader>` -> `OpenFile`. Its
`ProductHandler_Tank.Bundles.cs` indexes type-`0x0D90` bundle entries before
the CMF lookup. That is the same shape the client uses, but it is only a
reference: OWLib's newest CMF provider is `ProCMF_154088`, so it cannot open
build 154504, and its bundle lookup is a flat scan rather than the client's
per-type directory. OWLib has no static map table; `STUMapCatalog` holds
references, not inline headers.

The install's map catalog is readable: tool type `0x039`, asset
`0x01c0000000000041` (7,184 B), 305 entries of
`(m_660E53F7 header GUID, m_7BF4EF38 map GUID)`. The practice entry is
`0x0790000000000688 -> 0x0800000000000688`. It names the base map; the
variations come from the header. The client opens this catalog lazily in
`FUN_14077c8a0` (`0x14077c8a0`), which is called from the level-preload path
`FUN_14077b7d0` (`0x14077b7d0`), so the catalog is part of the same
message-driven load.

### Streaming and selection

`teMAP_CHUNK_STREAMING` names the streaming kind. The policy - which chunks
load when - is **not read**. The chunk set for a map asset, including the four
stored kinds and the 31 synthesised GUIDs, is read and implemented.

**The variation choice is not a client function.** The header's mode and
variation arrays are registered data with no runtime consumer in the image
("Who chooses a variation"); the map reaches the client as a type-`0x002` GUID
in the level-load message. What is open is the producer of that message: the
server, or the local-session setup that builds it for an offline map.

**A chunk list is not a map.** The client's chunk set is the synthesised set
for one map GUID; a proximity list is not a substitute.

## Collision

The map ships collision chunks kind `0x11` (`MAP_CHUNK_COLLISION`) and kind
`0x10` (`teMAP_CHUNK_ADDITIVE_COLLISION`); pathing is kind `0x12`
(`MAP_CHUNK_PATHING`) and `0x23` (additive). That mapping is now **read** from
the client's own kind table and its name decoder — full evidence in
`formats/chunk-kinds.md`. Consequently the 220-cell container recorded below is
the **pathing** chunk; until 2026-11-02 this file called it the collision chunk,
and that label is corrected in place here but not rewritten throughout, so read
"collision chunk" in the older paragraphs as "pathing chunk". The client draws
the visual mesh from this chunk; the walk's collision comes from the level
collider (`formats/chunk-kinds.md` section 4b, `ow_tank::collision_level`,
install test `collision_level_real`), not from this container.

### The pathing chunk's container: 220 cells (2026-11-01; kind corrected 2026-11-02)

The practice range's pathing chunk `0x0DD0001200000688` (kind `0x12`, the
base map's `+0x1C0` reference) is **8,792,256** bytes (`0x8628C0`). Read as data, it
is a self-describing container, not a flat mesh:

```text
+0x00000  u64  RVA 0x00020      -> the first STUD blob
+0x00008  u64  RVA 0x862720     -> the second STUD blob
+0x00010  u64  1
+0x00018  u64  RVA 0x862878     -> a 0x48-byte matrix block
+0x00020  STUD v1 blob: magic u32 0x53545544 (`DUTS` on disk), count 1,
          record {i32 0x20, u32 0}
+0x00040  the one instance of the first blob
+0x862720  STUD v1 blob: magic u32 0x53545544, count 1,
          record {i32 0x20, u32 0}
+0x862740  the one instance of the second blob
+0x862878  4x4 matrix (1.0 diagonal, then 51.59, -10.49, -6.36, 205.8 ...)
```

**The first component is a registered client class.** Its instance starts with
`{u32 class hash; u32 next}` = `0x9199BF6B`, `0`. The class is registered by
`0x1400fdcb0` through `RegisterComponentTypeDescriptor` (`0x1421cd360`, the
descriptor at BSS `0x143c3f060`): type `0x9199BF6B`, **size `0x78`**, align 8,
constructor `0x140f91550` (vtable `0x14314acc8`, destructor `0x140f8f310`), four
declared fields. Its field values in the practice payload are RVAs into the
same payload: `+0x10 -> 0x98`, `+0x58 -> 0x2358`, `+0x68 -> 0x2428`, plus a
u64 `0x0F3E456F3835B2C6` at `+0x20`. The three targets are, in order, a small
hash/offset table at `0x98`, a per-sector table at `0x2358` (a count and
repeated `{u32 7, quad bounds}` records) and the **cell array** at `0x2428`
(decoded below: 220 cells that tile the payload).

**The second component is not registered.** Its class hash `0x99F089FE` has no
immediate anywhere in the image, so the client has no component class for it
and the loader must skip it. Its fields repeat the sector pattern: `0x88`,
`0x11C`, `6`, `0x98` and a run of `{u32 7, quad bounds}` records, so it reads
like tool-side metadata for the same sectors and not gameplay data.

**The chunk asset fixes the RVAs in place.** `FinishMapChunkAsset`
(`0x141ff54e0`) handles the collision kind through its slot `0x1E` branch:
`0x142e49360` converts the header's `+0x00`, `+0x08` and `+0x18` RVAs to
pointers, `FUN_1421d26d0` (`0x1421d26d0`, the STUD loader) loads the blob at
`+0x20`, and `FUN_141ff51a0` (`0x141ff51a0`) runs a 17-pointer fixup pass over
the record at `+0x68`. The relocator family used by these per-slot branches is
read: `0x142e49270` (two pointers), `0x142e492a0` (two counted arrays,
`0x58`/`0x38` stride), `0x142e49360` (three pointers), `0x142e493a0`
(`+0x2E8`/`+0x2F0`) and `0x142e493e0` (`+0x08`). Each treats a stored `0` as
null and adds the payload base to every non-zero RVA.

**`0x142e49360` is implemented and verified.** The client's body is 61 bytes
and reads:

```text
+0x00  u64  RVA -> pointer (zero stays zero)   the first STUD blob
+0x08  u64  RVA -> pointer (zero stays zero)   the second blob
+0x10  u64  not touched
+0x18  u64  RVA -> pointer (zero stays zero)   the third blob
```

Each conversion is the same three instructions: a `lea` of the value plus the
record's own address, a `test` of the value, and a `cmove` back to zero when
the value was zero. The crate's `chunkkind::fixup_header_in_place` is that
expression, operation for operation, and `fn/MapChunkHeaderFixup` proves it
against the client's bytes (160 differential cases, 4/4 mutants killed). On
the practice range's pathing chunk the three stored values are `0x20`,
`0x862720` and `0x862878` - the two blobs at the very end of the chunk - and
the fixup turns them into pointers at the record's address plus each value.

The other three relocators are **not** implemented: `0x142e49270`,
`0x142e493a0` and `0x142e493e0` are read here but the crate has no code for
them.

### The cell container, decoded (2026-11-01)

All offsets are relative to the first STUD blob (its magic dword). The client
compares that dword with `0x53545544` (`StuLoadSubAssets`, `0x1421d26d0`), so
the on-disk bytes are `44 55 54 53` (`DUTS`) even though the format is called
`STUD`. All 220 entries validate: every cell, record and offset is inside the
payload, and the cells tile it without gaps.

```text
container + 0x2428   array header {u32 count; u32 pad; u32 data_off}
container + data_off entries[count], 16 B each  {u64 cell_off; u64 0}
container + cell_off cell                       {u64 block_size; u64 record_off}
container + record_off record                  17 x u64 self-relative offsets,
                                               then 0x18 B of header, then the
                                               17 arrays; array k starts at
                                               offset[k] and array 16 runs to
                                               record + block_size
```

Practice chunk (**file** offsets; the blob starts at `0x20`): count 220,
header at `0x2448`, entries at `0x2458` (the last of the 220 ends exactly at
the first cell, `0x3218`), first record `0x3228`, first block size `0x67F8`,
and the last cell ends at `0x862720` — exactly where the payload's second
STUD blob begins. The container-relative values are `0x2428` (header),
`0x2438` (entries), `0x31F8` (first cell), `0x3208` (first record) and
`0x862700` (last cell's end); the Rust reader and its test use those. `record + block_size` is the
next cell, so the
first record's 17 offsets are `0xA0, 0xC8, 0x130, 0x1E0, 0x228, 0x280, 0x388,
0x648, 0x888, 0x8D0, 0x998, 0xD78, 0x1188, 0x1548, 0x2A58, 0x5618, 0x65F8`.
**Empty arrays are zero-length**: a cell with no geometry can have equal
boundaries (cell 86 has `0xA0` six times, then `0xA8`) and zero header
bitfields, so an offset is never a sentinel — it is a boundary, and the array
between two equal boundaries has no elements.
The `0x18`-byte record header carries two packed bitfields, two `0xFFFFFFFF`
sentinels and two counts; they match array 0 (8-byte items: 5/8/15 in cells
0/1/219) and array 9 (8-byte items: 25/16/40), so it is a real per-cell
descriptor.

The arrays are visible from their contents: `arr3`/`arr11` are u16 index runs
with `0xFF` separators, `arr12` interleaves `{u16 index; u16 0xFF}`, `arr8`/
`arr16` are byte attributes and `arr13`/`arr14` hold 4-byte quantized vertex
data. Their sizes are not a single stride across cells, so the decode is
per-array, not one uniform record.

**The container and the runtime fixup are implemented and tested
(2026-11-02).** `ow_tank::collision::parse` reads the container header, the
v1 `STUD` record table, the `0x78`-byte component, its `+0x68` cell array and
every cell's 17 array slices, validating the cell tiling and the boundaries.
`crates/ow-tank/tests/collision_cells_real.rs` opens the practice range's
`0x0DD0001200000688` from the install and pins the count (`220`), the first
cell (`0x31F8`, record `0x3208`, block `0x67F8`), the 17 boundaries
(`0xA0 .. 0x65F8`), the last cell's end (`0x862700`) and every array's extent.
Those are container-relative offsets; the file offsets are `+0x20`.

`ow_tank::collision::fixup_cell_bytes` is the read expression of
`MapChunkCellBytesFixup` (`0x141ff51a0`): the outer `{inner header, mode}` pair,
the inner header's two encodings (mode 1 count at `+8`, otherwise count at
`+0` and entries at `+8`), the entry's two encodings (mode 1 record pointer in
the first qword, otherwise at `+8` of the object it names), and the 17-qword
conversion (`q == 0` stays null, otherwise `record + q`). The differential
unit `fn/MapChunkCellBytesFixup` runs 264 iterations plus the edge pool and
kills 12/12 mutants; the two allocate-then-deref-null mode-0 branches are
declared dead (the allocator dereferences the process heap vtable at
`0x144188200`, which is null in the offline image). The file-to-runtime entry
conversion (`{cell_off, 0}` to `{record pointer, 1}`) is the loader's, not
this function's; it is recorded as the observation in
`formats/domino.md` and remains unverified.

**The consumer path is read.** The geometry object (the `0x910`-byte class
registered by the node-type-2 factory) is finalised by `GeometryFinalize`
(`0x14213d0b0`). The object's layout is fixed by its clear function
`GeometryClear` (`0x1421384e0`), which frees exactly these arrays:

| field | stride | count | role |
|---|---|---|---|
| `+0xf8`/`+0x104` | 0x50 | | transform sources (copied to the `+0xe0` pool, `+0xf0` count) |
| `+0x108`/`+0x114` | **0x60** | | **cell entries** |
| `+0x118`/`+0x124` | 0x100 | | a second entry array (freed entry-wise, `+0x30` sub-array) |
| `+0x128`/`+0x134`, `+0x138`/`+0x144` | 4 | | u32 lists |
| `+0xb0`/`+0xc0`, `+0xc8`/`+0xd8` | 0xb0 | | record pools with free-list at `+0xa8` |

Finalise then copies the `0x50`-stride transforms into the `+0xe0` pool and,
per **0x60-byte cell entry** (`geometry+0x108`, count `geometry+0x114`):

- allocates a `0xb0` record from the `+0xb0` pool, copies the entry's five
  `vec4`s (`+0x00`, `+0x10`, `+0x20`, `+0x30`, `+0x40`) into its `+0x00..0x50`,
  and **steals** the entry's u16 array: `{u64 ptr; u32 count; u8 flag}` at
  `+0x50`/`+0x58`/`+0x5c` moves to the record's `+0x90`/`+0x98`/`+0x9c` and the
  entry's copy is zeroed;
- creates one **body** with `OwnerCreateBody` (`0x142cc3d90`) — args are the
  owner index and an eight-float config of two quads; it rejects infinities and
  a zero length, requires `owner+0x15c == 0`, allocates with `0x142cbff20` and
  returns `{index, generation, owner}` (stored at the record's `+0x2c`);
- builds a **type-3 (mesh) source on the stack** and calls `ShapeAddToBody`:
  type byte at `+0x67`, scale `1.0f` at `+0x68`, mesh source pointer at `+0x70`
  = **record + 0x20** (the three `vec4`s at entry `+0x20..0x50`), the shape id
  from `geometry+0x0a`, and the geometry's own float settings from
  `geometry+0x44..0x5c`; the returned handle is stored at the record's `+0x50`;
- the second phase rewrites the stolen u16 indices through the id table built
  from the transform loop, and applies `geometry+0x310`'s transform.

So the **cell entry is 0x60 bytes**, not 0x100, and each entry yields exactly
one body and one mesh shape.

**The brush-built path is a different geometry (2026-11-01).** The node-type-2
node's build (`GeometryBuild`, `0x14213c440`) never reads the collision chunk:
it takes the node's model (`node+0x88` -> `+0x38` -> `+0x40` -> `+0x48`, a brush
tree), copies the collision parameters (`0x142132cf0`), allocates one `0x60`
cell entry from `geometry+0x108`, builds its transform from the object's
`+0x98` instance array (`FUN_142ced620`), converts the brush tree to a
triangle mesh (`0x142cf23c0`), thickens and closes it (`0x142cf2010`), appends
the entry's u16 array to `geometry+0xf8` (`0x142139080`), creates the owner's
body (`OwnerCreateBody`) and finalises. So the 220-cell STU chunk feeds a
**different node type**, not this one. The node-type registry add
(`0x141c5ff40`, now created in the project as `RegisterNodeType`) has **144**
call sites, one per node type (measured 2026-11-02, not the 100 estimated
here); the type table is static data 0x28 apart from `0x143b8e5c8`, and
`dev/probe_node_types.py` dumps it — see "The node-type table is recovered"
below.

**What the brush build writes, and what that means for the decoder
(2026-11-02).** The 0x60 cell entry's `+0x20` is a **0x30-byte type-3 mesh
source** (`{u64 nodes; u64 second; u64 third; u32 count; 5 floats}`), the same
struct `ShapeEntryFromDescriptor` (`0x142cb7f10`) copies into the type-3 pool
record and `MeshShapeFill` (`0x142cae540`) builds the walkable mesh from. The
brush path fills it here: `MeshToColliderFaces` (`0x142cf2010`) is called with
`entry+0x20` (its fill), a loop then stamps the brush's two u16 ids
(`+0xa8`/`+0xaa`) into each 0x20-byte face's `+0x18`/`+0x1a`, `0x142139080`
appends the entry's u16 array, and `GeometryFinalize`'s only call site
(`0x14213cb52`) turns the entry into one body and one type-3 shape. Because the
brush path and the chunk path must reach the *same* walkable mesh, the chunk
decoder's output form is fixed: the three pointers of that source, built from
the cell's quantized vertices (`arr13`/`arr14`) and its `0xFF`-separated index
runs (`arr3`/`arr11`/`arr12`). The one unread piece on that path is the **BVH
builder** that fills the node block the narrowphase walks. Direct-caller maps
(`dev/data/read/callers.py`) put `BrushTreeToMesh` at exactly four sites
(`0x14213a7ab`, `0x14213af82`, `0x14213c2c8`, `0x14213c9c0`) and
`MeshToColliderFaces` at two (`0x14213a995`, `0x14213c9d7`), all brush paths,
so the chunk path does not go through a brush tree.

**The consumer wants three arrays of 0x60-byte records (2026-11-02).**
`GeometryIngestCandidateB` (`0x14213528a`, 3097 bytes) is the per-entry shape
builder, and it fixes the decoder's output form exactly. Per iteration:

- the outer loop runs while a counter is below `[node+0xd8]` and walks two
  cursors: `0x30` per step through the **mesh-source array**
  (`[node+0xf8]` = pointer, `[node+0x100]` = count) and `0x14` per step through
  the **entry array** it fills;
- the `0x30`-byte source is **three `{pointer, count}` pairs** at `+8`/`+0x10`,
  `+0x38`/`+0x40` and `+0x68`/`+0x70` — not three `vec4`s. `func_141b53b70`
  copies `record[i]` at a **`0x60` stride** from each pair's array, so each of
  the three arrays holds `0x60`-byte records and the pair's second word is
  the record count;
- per entry it creates one body (`OwnerCreateBody` with the node's `+0x9c`
  shape id and the node's float settings) and one shape per source record
  (`ShapeAddToBody` with a stack descriptor whose type word at `+0x1e` is
  `0x100`, id at `+0x7a`, the two stamp ids at `+0x7e`/`+0x20`, and
  `0x100ffff`/`0x200` at `+0x82`/`+0x86`), then connects them with
  `FUN_142cb34f0(body, quad, point, 1)`;
- the connect point is the **nearest** one found by `FUN_142cb9a50(shape,
  quad)`: the loop keeps the shape whose returned point is closest to the
  node's `+0xb0` triple, squared-distance compared;
- afterwards the node's `+0xb0..+0xcc` accumulator is zeroed and `+0xe4`/`+0xe6`
  are set.

So the decoder's contract is now concrete: **three arrays of `0x60`-byte
records plus their counts**, per entry, in a `0x30`-byte source. What fills
them from the 17 cell arrays is still open.

**Negative result: the 17 arrays are not record views (2026-11-02).** Measured
over the practice range's 220 cells, no array is a `0x60`- (or `0x30`-, or
`0x20`-) stride view: `arr7` divides by `0x60` in 44/220 cells, `arr12` in
14/220, `arr14` in 22/220 — the rest divide in fewer, so a straight per-cell
overlay of a fixed record size is excluded. Every array **is** 4-byte
granular in every cell measured, and 26 cells have an empty first eight arrays.
Totals: `arr0` 11 984, `arr1` 56 488, `arr2` 62 800, `arr3` 43 336, `arr4`
54 160, `arr5` 168 472, `arr6` 400 184, `arr7` 148 928, `arr8` 19 352, `arr9`
23 368, `arr10` 250 720, `arr11` 277 496, `arr12` 319 360, `arr13` 1 781 600,
`arr14` 4 338 448, `arr15` 695 816, `arr16` 87 784. Cell 0's lengths: 40, 104,
176, 72, 88, 264, 704, 576, 72, 200, 992, 1040, 960, 5392, 11200, 4064, 512.
The data must therefore be **built** into the `0x60` records, not reinterpreted
from an array — which is consistent with the brush path, where
`MeshToColliderFaces` writes the source rather than pointing at it.

**The geometry object, and the brush ingest end to end (2026-11-02).**
The object every ingest path writes is now read from its constructor
(`0x142135c10`, vtable `0x143394ed0` then `0x1433950b8`), its two destructors
(`0x142135e30`, `0x14213a310`) and the ingest itself:

| field | meaning |
|---|---|
| `+0xb0` | source array base, count at `+0xc0`, last-id (u32, init `-1`) at `+0xc4`, **stride `0xb0`** |
| `+0xd0..+0xf4` | three `{base, base2, u32 count, u32 last-id = -1}` groups, `0x18` bytes each, ending exactly at `+0xf8` |
| `+0xf8`, `+0x108`, `+0x118`, `+0x128`, `+0x138` | five vector headers, `0x10` bytes each: `{void *data; u32 capacity; u32 count}` |

So `+0x108` is `{data, cap @ +0x110, count @ +0x114}` and `+0x118` is
`{data, cap @ +0x120, count @ +0x124}` — the counts sit **inside** the header,
not at `+0x100`/`+0x110` as the earlier note guessed.

**Correction (2026-11-02).** The step-4 text below is superseded: the ingest
appends **one `0x100`-byte entry** to `geometry+0x118` through `0x1421360a0`
(the `0x100` appender; `0x14213a510` calls it at `0x14213aadc`), and fills that
entry through `+0xf0` (stores at `+0xa0`, `+0xb0`, `+0xd0`, `+0xf0`). The
`0x60`-byte entries belong to `GeometryBuild`: `0x142136010`, the `0x60`
appender, has exactly one caller, `0x14213c924`. So "one `0x60` entry per
snapshot record" and `MeshToColliderFaces(entry+0x20, …)` are withdrawn; the
entry's own fill offsets past the stores above are **open** (the old `+0x20`
mesh-source offset is not re-read). The `0x100` appender has **two** callers,
this ingest and `0x14213acf0` (`0x14213b00c`), both brush. Detail:
`formats/shape-ingest.md`.

`AppendCollisionEntryFromBrushSource` (`0x14213a510`, 2011 bytes) is the brush
ingest, read end to end. Given a source index it:

1. snapshots the source's brush (`this+0xb0 + index*0xb0`) into a `0x28`-byte
   heap record — `{vertices, nodes, u32 vertexCount @ +0x10, u32 nodeCount @
   +0x14, u32 @ +0x18, u32 index @ +0x20 = -1}` — copying `count<<5` bytes of
   0x20-byte vertices and `count*0x28` bytes of tree nodes, then marks
   `source+0x58 = 1`;
2. runs `BrushTreeToMesh` and measures the mesh (`0x142cf10e0`);
3. applies the area test. There are two branches, chosen by `this+0x34`:
   - `this+0x34 != 0`: the entry is appended as soon as **any** tree node is a
     leaf (`node+0x24 != 0`);
   - otherwise the scale `this+0x38` must be positive, and for every leaf node
     it sums the chord length between the node's vertex (`+0x8`, index `*0x20`
     into the vertex array) and its child's vertex (`node[+0x4].+0x8`), scales
     the sum by `this+0x38`, and **skips** the mesh when that value squared is
     `<=` the measure returned by `0x142cf10e0`;
4. appends one `0x60` entry per `0x28`-byte snapshot record (the loop steps
   `0x28` over the snapshot array); each record gives exactly one entry:
   - grow when `count == capacity`: `capacity += capacity>>1`, minimum 2, and
     the allocation is `capacity * 0x60` — the entry stride, confirmed by the
     `LEA reg,[reg+reg*2]; SHL 5` pair;
   - `entry[0x00..0x20]` = a raw copy of the record's first `0x20` bytes
     (`entry+0x20` is the `0x30`-byte mesh source);
   - `MeshToColliderFaces(entry+0x20, mesh, this+0x20, word[this+0x0a])`;
   - per generated face (`source.count @ entry+0x40`, array @ `entry+0x30`,
     stride `0x20`): `face+0x18 = this[0xa8]`, `face+0x1a = this[0xaa]`;
     `FUN_142139080(this+0xf8, entry+0x50)` appends the handle.

The driver (the function that calls the ingest at `0x14213bd61`) walks the
source array by index, requires `source+0xa0 == 0`, `source+0x58 == 1`
avoided, `source+0x59 != 0`; resolves the source's asset through
`0x141fd3a90` and requires **kind `0xa`** (from `0x141fd3b20`), checks its
class with `0x1421ce150` against the rdata at `0x143f505c0`, builds the
descriptor from the source's floats `+0x78/+0x7c/+0x80/+0x84/+0x88/+0x8c`, and
calls the ingest. So `+0xb0` holds brush assets, not cells.

The build pass then turns the entries into world shapes: per entry it calls
`ShapeAddToBody` and stores the handle at `entry+0x50`, then walks the
`+0x118` records, calling `FUN_142cd7b20` on each face of each record (`face
array @ +0x30`, count `@ +0x38`, stride `0x40`).

**Negative result: this whole object is brush-only, so the cells are elsewhere
(2026-11-02).** Three scans settle it, and together they close this line of
attack:

1. stores to the `0x60` entry count `+0x114` across the whole image are exactly
   **three** sites, all in the geometry module: `0x14213a965` (the brush ingest
   above), `0x1421388dd` (a clear) and `0x14213d559` (the build's reset);
2. the `+0x118` header's element size is **`0x100`**, not `0x50`: its append
   helper `FUN_1421360a0` (19 bytes) grows `capacity += capacity>>1` (minimum 2)
   with a `capacity << 8` allocation, copies `count << 8` bytes, and returns
   `count * 0x100 + base`;
3. that helper has **two** callers, both brush source ingests: `AppendCollisionEntryFromBrushSource` (`0x14213aadc`) and `0x14213acf0` (`0x14213b00c`); corrected 2026-11-02.

So the geometry object only ever receives brush material: nothing on the cell
path appends either vector, and the `0x100`-byte record the brush ingest fills
(`{vertices, tree nodes, two more pointers, two floats, the scaled quad, the two
stamp ids, a handle}`) is a brush collider source, not a cell. Combined with the
measurement that no cell array is a `0x60`-stride view, **the chunk's cells do
not feed this node class at all** — they must be built by a different one. That
is the next anchor: the node class that owns the chunk component the fixup
writes (`sub+0x68` for slot `0x1e`, `sub+0x08` for slot `0x22`).

**The shape path has no chunk input (2026-11-02).** The Domino world's shape
entries (`[record+0x2b8]`, element `0xc0`) and the five per-type pools
(`+0x2d0..+0x330`) can only be created through `ShapeAddToBody` or
`ShapeAddToOwner`; both route through `ShapeEntryFromDescriptor`, whose two
callers are the two add paths. Every caller of either add path that appends
collision material is a brush-mesh producer: `0x142136010` (the `0x60` array)
has one caller, `GeometryBuild`; `0x1421360a0` (the `0x100` array) has two,
both brush ingests; and `BrushTreeToMesh` (`0x142cf23c0`) has exactly four
callers — the four brush appenders. A verified store census of the pool fields
finds their only writer besides construct/clear/reset is those add paths
(`formats/shape-ingest.md`, 119,802 decoded function groups, the `-1` free-head
sentinel as the discriminator). The collision chunk (kind `0x11`) is therefore
**not** the Domino shape source, and the remaining way it could be is an inline
appender that grows one of `geometry+0x114`/`+0x124` without calling either
helper — which no decoded store shows but which a displacement scan cannot
exclude.

**Ghidra was hiding this function behind false `noreturn`s (2026-11-02).**
`0x14213a510` was stored as a 167-byte function; its real body is 2011 bytes.
Three callees were marked non-returning although every call site continues:
`0x141bb00e0` (the size/alignment allocator used as `Alloc(size, align)`),
`0x142ced8f0` (a throw helper) and `0x142efe2b0` (the vector-grow memcpy). Any
function that allocates or grows a vector after calling one of them was
truncated, which is why "no function at this address" kept coming back for
addresses that clearly hold code. The audit recipe is in
`ghidra-project.md` and it is mechanical: `clear_flow_and_repair` in dry-run
mode **reports** the offending callees, and `set_function_no_return` clears
them.

**What that leaves.** The chunk-side navigation is exact, the payload's cell
layout is decoded, and the consumer chain is read up to the geometry finaliser
(`0x14213d0b0`) and `OwnerCreateBody` (`0x142cc3d90`). The one open link is the
**cell decoder**: whoever writes `geometry+0x108`/`+0x114` from a 17-array
record. Searching the image for the payload's 64-bit values as immediates finds
nothing, and the class's own methods are the generic STU getters (the
`0x140f8fxxx`/`0x140f9xxxx` module is one getter per class descriptor, no
traversal). The Domino-side ingest is read (`formats/domino.md`:
`ShapePoolAlloc` `0x142cb00c0`, `ShapeEntryFromDescriptor` `0x142cb7f10`,
`ShapeAddToBody` `0x142cb3940`, `ShapeAddToOwner` `0x142cd4ef0`), and it
consumes a descriptor with a type byte at `+0x67` and per-type data at
`+0x68`/`+0x70`. The **array encoding the fixup expects** is now observed
(`formats/domino.md`, "The cell array's runtime form"), so the decoder's output
form is known; finding the decoder itself is the next anchor.

**The class's field table is not in the image (2026-11-01).** The registered
descriptor (`0x143c3f060`) stores its field table at `+0x28` (pointer) and
`+0x98` (count = 4); `FUN_140f84590` installs `0x143c3c620` as that table, and
`0x1421cdb90` only **sorts/finalises** it (`desc+0x28 + desc+0x98*0x38`,
jumping to `0x1421cce50`). In the installed image both `0x143c3c620` (the
table) and `0x143c3efc0` (`desc+0x08`) are **all zero** — the block's source
is the file, not a runtime buffer. So the four field hashes/offsets are built
by the registry at runtime and **do not exist as immediates**; the consumer
cannot be found by searching for a field hash. `FUN_1421cdfd0(desc, object)`
walks the table and calls `[field_type+0x48]` with `object + entry[8]`, so a
field entry is `{?, offset@+8, type@+0x10}` with a 0x38 stride.

**The registered name is stripped.** Class names are stored XOR-obfuscated as
`[u32: length in the low 24 bits, 4th byte = encoded flag][name bytes]`, the
name XORed with the eight bytes immediately before the header (`0x1438846c0`
for this class). Decoded, the collision component's name at `0x1438846cc` is
**`Unknown`**, and so is the 0x910-byte class registered at `0x143aa024c` by
`0x142029f00`. The shipping client does not carry readable class/field names.

**Real names come from the reference registry, matched by hash.** The class
hash is the stable key, and the local OWLib checkout's registry resolves it
exactly (`cargo run -p ow-tank --bin stu -- --class 0x9199BF6B`):

```text
0x9199BF6B  STU_9199BF6B  size 120 (0x78)
  +0x08  field 0x81DEFC98  inline, 40 bytes
  +0x30  field 0xFC6A2C09  inline, 40 bytes
  +0x58  field 0x9DBA1F4E  array of instances, 16 bytes
  +0x68  field 0xD7C9DAAC  m_cellBytes, array, 16 bytes
```

Size and all four offsets match the client's registration and the payload's
instance exactly, so the mapping is a check, not an invention: field 4
(`+0x68` = RVA `0x2428`) is **`m_cellBytes`** (the per-cell payloads, count
`0xDC` = 220 at the head), and field 3 (`+0x58` = RVA `0x2358`) is the array
that goes with it — the tables measured there hold repeated `{u32 7, quad
bounds}` records and a 220-entry RVA list, so *cells* is the working reading,
not a decoded record yet. The other three fields are hash-named in the
reference too (`m_81DEFC98`, `m_FC6A2C09`, `m_9DBA1F4E`); there is no readable
name for them anywhere, so they stay hash-cited rather than invented. The
second component's hash `0x99F089FE` is not in the reference either.

**The ingest's navigation is read (2026-11-01).** The chunk *asset* does not
parse its payload:

- `0x141ff33f0` registers two classes: the map asset (type `1`) and the chunk
  asset (type `0xBC`, size `0x1f`). Their virtual slots are the map's
  `LoadMapAssetChunks` (`0x141ff5c80`), `0x141ff5bd0` and
  `ReleaseMapAssetChunks` (`0x141ff58b0`), and the chunk's
  `FinishMapChunkAsset` (`0x141ff54e0`) plus `0x141ff53e0`.
- `0x141ff53e0` is the only virtual that walks bytes: for **chunk slot `8`**
  it treats the payload as a container of GUID-keyed sub-chunks
  (`[+0]` count, `[+8]` offset to the first record, `0x141b32e50` to resolve a
  GUID, `0x1421c3780` per record). Nothing there reads triangle data.
- So the collision reader lives with the consumer, not the asset. The
  consumer is the domino world: the query resolves a `u16` handle through the
  table at `0x1442a0bc0` to a record whose fixture pool is `+0x2b8` and whose
  per-type geometry pools are `+0x2d0..+0x330`.

**The two collision chunks are identified by slot (2026-11-02).**
`FinishMapChunkAsset` (`0x141ff54e0`) dispatches on
`ChunkSlotForGuid(work+0x70)` and has a case for slot **`0x1e`** and one for
slot **`0x22`**. It was stored as a 12-byte function until the false-`noreturn`
repair (see `ghidra-project.md`); it is now **964 bytes and decompiles end to
end**, which confirms the hand-read dispatch below and adds its two ends: every
case falls through to the upload accounting `FUN_141fe37a0(work, data, work+0x1c,
1)` and the function returns `0x11`, and the full case list is `7`, `8`, `0xf`,
`0x1d`, `0x1e`, `0x1f`, `0x21`, `0x22`, `0x23`. Both collision slots run the
same three steps and differ only in the sub-asset offset:

```text
slot 0x1e:  MapChunkHeaderFixup(payload); sub = StuLoadSubAssets(cache, *payload, 0)
            MapChunkCellBytesFixup(sub + 0x68)
slot 0x22:  MapChunkHeaderFixup(payload); sub = StuLoadSubAssets(cache, *payload, 0)
            MapChunkCellBytesFixup(sub + 0x08)
both:       if (payload[1] != 0) StuLoadSubAssets(cache, payload[1], 0)
```

So the cell container the verified fixup rewrites is **the chunk's STU
sub-asset plus a per-slot header** — `0x68` bytes for slot `0x1e`, `0x08` for
slot `0x22`. Which of the two is `MAP_CHUNK_COLLISION` and which is
`teMAP_CHUNK_ADDITIVE_COLLISION` needs the kind table; they are otherwise
identical. The neighbouring slots are: `0x1d` and `0x21` both call `FUN_142e492a0` (the
two collision kinds), `0x1f` calls `FUN_142e493e0` (soundarea) and `0x23` calls
`FUN_142e49270` (streaming) — corrected 2026-11-02, see "Which chunk kind is
which" and `formats/chunk-kinds.md`. `0x1e`-style header fixup also appears for `0x1e` only; slots
`7`/`0xf` do their own inline relinking (0x40-stride records with a 0x10
array, 0x58-stride with 0x20), and slot `8` is the GUID-keyed STU container.

**The consumer of those cells is still unwritten.** The fixup only rewrites
pointers; nothing in this dispatch builds shapes. The ingest therefore splits
cleanly in two, and only the second half is open:

| half | state |
|---|---|
| chunk → STU sub-asset → cell container (slot `0x1e`/`0x22`, header, 17 arrays, pointer fixup) | **read, `fn/MapChunkCellBytesFixup` verified** |
| cell container → shape records in the query world | **open** |

**Ruled out on the consumer side (2026-11-02).** Two hypotheses were tested and
both fail, so neither is worth re-trying:

- *the geometry object's vectors*: the `0x60` entry count (`+0x114`) has only
  three writers image-wide and the `0x100`-element vector's append helper
  (`0x1421360a0`) has two callers, both brush ingests — that class is
  brush-only;
- *`FUN_1420c63b0`* (10 500 bytes, four `ShapeAddToBody` sites, the obvious
  "world ingest"): it has **no code caller** and no plain pointer to it
  anywhere. Its three data references (`0x14378243c`, `0x1437824dc`,
  `0x14469c0c4`) sit in tables whose neighbouring words are RVAs pointing
  **inside the same function**, i.e. internal switch tables, not vtable slots.
  Its only call xref is a self-recursion at `0x1420c8db5`. So it is not the
  scene's ingest entry point.

What is known about the consumer's target, for whoever picks this up: the fixup
rewrites the cell records **in place**, so the arrays become `{pointer, count}`
pairs at the same offsets, and the cell block is reached as
`StuLoadSubAssets(cache, chunk->payload[0], 0) + 0x68` (slot `0x1e`) or `+ 0x08`
(slot `0x22`). A consumer therefore needs the collision sub-asset base and a
17-array walk; `ow_tank::collision` already produces exactly that shape from the
practice range's chunk, so a consumer found in code can be checked against real
data immediately.

**The handle table's single writer is the registry install, not a per-record
call.** The whole image has exactly one store to `0x1442a0bc0`:
`0x142cc3bb0`, which stores the registry object's array pointer and, at
`0x142cc3bd1`, the sibling global `0x1442a0bc8` (the inline-storage pointer).
`0x142cc3c80` is the registry's teardown and `0x142cc3be0` is a per-record
body-reset pass (it walks `[record+0x288]`'s `0x1c0`-stride bodies zeroing
`+0x120`/`+0x130` after a `+0x15c` lock check). Handles are therefore engine
state; the map's collision record is created by the world-side path, which is
where the fixture/geometry fill has to be found.

**Search routes tried and rejected (2026-11-01), so nobody repeats them:**

- `imul reg, reg, 0xc0` (the fixture stride): 24 hits, none in the collision
  module — the two near misses (`0x142c90be0`, `0x142c93190`) multiply by
  `0xc0` for an unrelated `0x20`-stride matcher.
- a raw disp32 scan for `0x2b8`/`0x2d0`/`0x2e8`/`0x300`/`0x318`/`0x330`:
  ~1,700 functions, useless — those displacements are ordinary stack offsets.
  A pool reference must be matched with its instruction context, not the
  displacement alone.
- the strings `COLLISION`, `Collision`, `collision`, `ADDITIVE`, `Additive`:
  **absent from the image**, so the chunk-kind names come from the install's
  data only and cannot be used as xref anchors.

**The tree is built from the fixture pool, lazily.** `DmBroadphaseReinit`
(`0x142cbdc60`) is a BVH builder, not an ingest: its first argument is the
pass sub-record base (`record+0x1b0`), it reads the query descriptor's eight
quads (`R8 + 0x00..0x70`), calls the verified `0x142cd5540`
(`fn/CollisionSweepBounds`) for the swept bounds, walks the per-pass record
array at `+0x44` (`R14 = RCX + 0x44`, where the root index lives — `-1` means
empty), and splits with a `0x100`-entry stack node pool (`[RBP+0xa8]`,
count at `[RBP+0x4ac]`) using the epsilons at `0x1435ecad0`/`0x1435ecaf0`.
So the tree's *shape* is a function of the fixture set at query time; the
open question stays exactly one level down — **who fills
`[record+0x2b8]`'s fixtures**.

**Also checked and not useful:** the region at `0x14473d800` looked like a
function-offset table, but it is `.pdata` in an RVA+checksum encoding (its
extents match `.pdata` exactly, including the truncated ones), so it adds no
precision to the coverage windows the units declare.

**The node-type table is recovered (2026-11-02).** The registry add had no
function in the project image, so every one of its callers was invisible
("0 callers" on all of them) and the documented plan "walk the 100 thunks"
could not run. It is `RegisterNodeType`, `0x141c5ff40` (52 bytes), now created
in the project image; it has **144 call sites**, not 100:

```text
RegisterNodeType(slot, descriptor)
  slot[1] = descriptor[0]      ; u16 type id
  slot[2] = descriptor[1]      ; the class registrar
  slot[3] = descriptor[2]      ; context
  slot[0] = head; head = slot  ; a singly-linked list, head 0x144188170
```

The slots are **static per-type data**, 0x28 bytes apart from `0x143b8e5c8`,
so the type table can be read straight out of the image without disassembly.
Each type's registrar is a small `SUB RSP,0x48` stub that reads
`MOV ECX,<type>`, `LEA RCX,[<class registrar>]`, `LEA RCX,[<slot>]` and calls
`RegisterNodeType`; `dev/probe_node_types.py` scans for the call and reads
both constants out of the site.

**The class registrars obfuscate their own names, and the names are useless as
an anchor.** A class registrar (e.g. type `0x13` = `0x142191d00`, 44 bytes)
allocates a `0x30`-byte descriptor, stores the type id at `byte[+8]`, decodes
a name in place — `name[i] ^= key[i & 7]`, length `[len] & 0xffffff`, key at
`len-8`, name at `len+4` — and calls `RegisterTypeWithSize`. The mechanism is
recovered, but the strings are generic: type `0x13`'s two triples decode to
`"Unknown"` and `"Unknown"`, so the obfuscation protects the string, it does
not identify the node. Whatever identifies the collision node, it is not a
name.

**Project defect to know about.** `GeometryBuild` (`0x14213c440`) is stored as
an **84-byte** function with no callers and no callees, while its real body is
`0x14213c440..~0x14213d0b0` (3,184 bytes; the probe reaches 430 instructions
before the sweep stops at conditional edges). Anything that asks the project
for this function's callers or callees gets nothing, which is why the
"node-type-2 build's callers" question kept coming back empty. Read it from
the recovered image, not from the project.

**Where that leaves the ingest.** The type table is enumerable (144 types,
each with a class registrar), the brush path is fully read, the chunk
container is parsed and byte-fixed, and the cell layout is decoded. What is
still unwritten is the **chunk node type's build**: which of the 144 types
takes the collision chunk and turns its cells into the `0x30`-byte type-3 mesh
source. The discriminator for the next pass is the class size handed to
`RegisterTypeWithSize` plus the class vtable's build slot — a chunk build must
fill that mesh source from the cell's quantized vertices (`arr13`/`arr14`) and
its `0xFF`-separated index runs (`arr3`/`arr11`/`arr12`) **without** calling
`BrushTreeToMesh`. `dev/probe_node_types.py --ingest` lists types whose build
touches a collider producer as a starting filter.

### The pathing container's consumer - "the cell path's consumer": **open** (2026-11-02, census corrected)

This is the **pathing** container, not the collision one (`formats/chunk-kinds.md`);
"cell path" is the historical name. Nothing below identifies it as collision, and
the census result — no shape producer reads it — is what a server-side pathing
artifact should look like.

**Retraction.** The first version of this section said "no client code turns
the cells into shapes in this image" and "the payload is never re-parsed". Both
claims are **withdrawn**. They rested on Ghidra's caller/callee metadata, which
this project's own notes say is incomplete: `ghidra-project.md` records that
`.pdata` entries chain and fragment and that whole regions are undefined, so a
function's caller list is not a census. Two concrete corrections:

* `ShapeAddToBody` (`0x142cb3940`): Ghidra reported 3 callers; a verified
  `E8 rel32` scan finds **27** call sites in **13** functions, every site
  decoded at an instruction boundary. Ghidra's three are a subset.
* The claim "exactly two functions read `+0x10`/`+0x58`/`+0x68` off one base
  and call `OwnerCreateBody` plus `ShapeAddToBody`" was **wrong**:
  `0x14208d010` and `0x142090e00` call neither builder (their callees are
  `0x142015510`, `0x142083d50` and `__chkstk`). The functions that do hold
  those sites are `0x14208d109` and `0x1420919b7`, and their `+0x58`/`+0x68`
  operands are `f32` radius/height fields of the mover settings and unrelated
  8-byte pointer fields — **not** the component's 16-byte `{pointer, mode}`
  array headers.

The state is **open**. What follows is a census and a list of attempts, not a
proof that the reader is absent.

**The corrected shape-producer census (2026-11-02).** Union of Ghidra's caller
list and a verified direct scan for the four add/build handles plus
`MeshShapeFill`, then one row per producing function. Every site was decoded;
one site (`0x140ca6478`) is real only under a local-window decode because the
mover module's obfuscated stretches desync a decode from the function's begin,
and it is kept. `dev/data/read/census.py` reproduces the whole thing
(`python dev/data/read/census.py 0x142cb3940 0x142cc3d90 0x142cd4ef0
0x142cb7f10 0x142cae540`).

| target | E8 sites | Ghidra caller funcs | union functions |
|---|---:|---:|---:|
| `ShapeAddToBody` `0x142cb3940` | 27 | 3 | 13 |
| `OwnerCreateBody` `0x142cc3d90` | 17 | 5 | 15 |
| `ShapeAddToOwner` `0x142cd4ef0` | 1 | 1 | 1 |
| `ShapeEntryFromDescriptor` `0x142cb7f10` | 2 | 1 | 2 |
| `MeshShapeFill` `0x142cae540` | 5 | 2 | 5 |

28 distinct producing functions, each classified by what it builds from (the
52 verified sites are grouped; a site appears twice where one function calls
two of the handles):

| function | builder sites | builds from | sees a chunk payload? |
|---|---|---|---|
| `0x140ca4f1c`, `0x140ca649e` (one `ShapeAddToBody` each) and `0x140ca6227` (one `OwnerCreateBody`) | 3 | mover-body parts: `0x140ca4f1c` reads `[rbx+0x1a68]` (the mover's own body) and `0x140ca649e`'s descriptor is built on the stack; `0x140ca6227`'s site is only reachable by a local-window decode because the module's obfuscated stretch desyncs a decode from the fragment begin | no: no loader call, no `[reg+0x60]` read |
| `0x14201b1cd` | 3 + 2 | per-object bodies and shapes from a transform built out of `[r14+0xd0]`, `[r14+0xf0]`, `[r14+0x298]` via `0x141ffab90` | no: the `0x800` mask appears, but no `[reg+0x60]` load |
| `0x14202c660` | 1 | the scene: `OwnerCreateBody` for the domino world at geometry `+0x31f8` (read earlier) | no |
| `0x1420652d7` | 1 | resolves an object through the pointer-decode `0x142e47a30`, then adds a shape and walks the result's faces with `0x142cd7200`/`0x142cd7b20` | no: the decoded object is a domino-side record, not a chunk payload |
| `0x142065184`, `0x1420686b1`, `0x14206877c`, `0x14208d109`, `0x1420917f5`, `0x1420919b7`, `0x1420cade1` | 19 sites in total | the mover/prop per-part builders: radii and heights read as `f32` from `[rdi+0x58]`, `[rdi+0x60]`, `[rdi+0x64]`, `[rdi+0x68]`, plus a stack descriptor; `0x14206877c` makes eight shapes from its arguments and calls nothing | no: no loader call, and the `+0x58`/`+0x68` operands are scalars, not 16-byte array headers |
| `0x142cf35a0` | 1 `ShapeAddToOwner` + 1 `OwnerCreateBody` | the character controller's own capsule, from the controller's config | no |
| `0x142cdf9f2` | 1 | a domino body filled from a `0x40`-stride record array (`count << 6` bytes copied from `[rsi+0x18]`), body id from `word[rsi+0x58]` | no |
| `0x142cb7f10` (`ShapeEntryFromDescriptor`), `0x142cb0a73`, `0x142cd4350` | 3 | the generic descriptor → `0xc0` shape entry plus the per-type record; the qword reads at `+0x68` are the **Domino descriptor's** per-type data | no: different struct, no loader call |
| `0x140c2813f`, `0x142078562`, `0x1420a02f6`, `0x14210bac0` | 4 `MeshShapeFill` | mesh shapes: `0x140c2813f` from a collider (`ColliderMeshEmit`), `0x142078562` fills `0x58`-stride mesh records whose source is at `+0x20`, `0x1420a02f6` builds mesh records from the model's holder list (`+0x140`) through `0x141b53af0`, `0x14210bac0` scales the filled vertices | no: no loader call |
| `0x142134ba2`, `0x14213528a`, `0x14213ca9f`, `0x14213d0d9`, `0x14213d5b9` | 8 | the geometry object: candidates A and B, `GeometryBuild` (`0x14213c440`), `GeometryFinalize` (`0x14213d0b0`) and the pool-B record creator | no: brush/pool-sourced, read in earlier sessions |
| `0x1420c63b0` | 5 | a 10 500-byte world ingest with four `ShapeAddToBody` sites and one `OwnerCreateBody`; no code caller and no plain pointer (its data xrefs are internal switch tables). Scanned over its full Ghidra extent (`0x2904` bytes) it reads qwords at `+0x10` but **not** at `+0x58`/`+0x68`, so it is not reading a component header | no header read; input unread past the entry |

**What the census shows, and what it does not.** No producing function calls
`StuLoadSubAssets`; none loads a qword from `+0x60` (the chunk work record's
payload field); only `0x142cb7f10` loads a qword at `+0x68`, and that is the
domino descriptor. So no shape producer can see a collision-chunk payload, and
the component's `{pointer, mode}` headers are read by none of them. That does
not close the question — a consumer could exist that builds no shape through
these five handles, or reach the component through a route this census does not
enumerate.

**The whole-copy route was checked and is empty.** A consumer could copy the
`0x78`-byte instance and read the copy elsewhere, which an offset scan would
miss. Two spellings were measured image-wide:

* **`memcpy`.** 8 923 `memcpy`/`memmove` call sites; exactly **one** passes
  `0x78` as the size — `0x142402824` in `FUN_142401de0` — and it copies a
  hard-coded diagnostic string fragment (`, which is not defined. The
  innermost scope is searched first…`), not a component.
* **Inline 16-byte moves.** 15 runs of six or more consecutive `xmm` loads off
  one base; every one covers `0x70` or `0x80` bytes, none `0x78`. A `0x78`-byte
  inline block copy does not occur.

Even a copy would not hide a consumer: a copy preserves the field offsets, so
reading the cells from a copy still loads a qword at `+0x68` of the copied
structure, which the producer census above tests for.

**Byte-level facts that stand** (independent of Ghidra metadata): the `STUD`
magic `0x53545544` occurs three times in the image — twice inside
`StuLoadSubAssets` (`0x1421d26d0`, which compares it and names it in its error
string) and once in the STU writer `FUN_1421d2ab0` (reached only from
`0x1421c30b0`), which emits it. So the loader and the writer are the only
functions that mention the magic; no third function re-reads a blob header.

**Why the chunk is still a plausible source.** The consumer target is read: the
scene ctor `0x14202c660` builds a `0x3210`-byte geometry at `scene+0x620`
(`GeometryBaseCtor` `0x14209a8f0`: 128 slots of `0x60` at `+0x60`), creates two
domino worlds (`0x142cc3c80`) at `+0x31f8`/`+0x31fa`, a body at `+0x31fc`, and
registers the geometry with the world (`0x14209ae30`, which stores it at
`world+0x160` via `0x142cc4a90`). Nothing read so far fills those worlds from
the collision chunk, but nothing proves they cannot be filled from it either.

### The three next readers: Domino-side, and unreachable (2026-11-02)

The three producers named as the next anchors were read. None of them touches a
collision-chunk payload or a `0x9199BF6B` field; all three work on the **Domino
owner's** vectors. Each was also tested for an invocation route, because a
producer with no `E8` caller can still be reached through a pointer table.

**Anchor 1 — `0x1420c63b0` (10 500 bytes).** Signature
`(owner, u32 index, char, callback, out, out)`.
* What it reads: `[owner+0xd0]` + `index * 0x38` — a **`0x38`-byte record
  array** whose `+0x28` is a count that must be non-zero and whose `+0x34` is a
  skip byte; `[owner+0xc0]` (records whose `+0x3e` u16 is compared with the
  index and whose `+0x38` u16 selects a transform), `[owner+0xe0]` (a
  `0x20`-stride array of 64-bit transform handles, `-1` meaning the identity
  table `0x143391530`), `[owner+0xc8]` (a node count), and the object at
  `[[owner+0x90]+0x250]` resolved through the pointer-decode
  (`0x142001f80`, `[[..+0x8b0]]+0x18`). It writes float/index data into the two
  caller records (`param_6+0x10/0x20/0x30/0x40`) and finishes with one
  `OwnerCreateBody` and four `ShapeAddToBody`.
* So it is a **node/transform-driven mesh builder** over a `0x38`-stride record
  array — the primitive/brush shape family, not the cell container.
* How it is reached: **nothing reaches it.** No `E8` call (its only one is its
  own recursion at `0x1420c8db5`), no `E9` tail jump, no immediate in code, no
  8-byte pointer anywhere, and no obfuscated-pointer encoding of the address
  (`add 0x1e2421a4798a328 / ror 0x22 / xor 0xe1379e3cb308170d / ror 0x2b`) in
  the image either. The three dword-RVA hits (`0x14378243c`, `0x1437824dc`,
  `0x14469c0c4`) sit in self-contained tables whose neighbours are code ranges
  of the same function and which no code reads.

**Anchor 2 — the three `MeshShapeFill` producers.**
* `0x140c2813f`: writes the mesh source at `[record+0x58]` with
  `ColliderMeshEmit` (`0x142ccc120`) from a collider, then fills a shape with
  it. Geometry comes from the **brush collider** (`MeshToColliderFaces` ->
  `ColliderFaceAppend` -> `ColliderMeshEmit`), which is the brush route.
* `0x142078562`: computes four `vec4`s from a `vec3` at `[r15-8]` (an
  axis-aligned box plus its transform) into a **`0x58`-stride record** at
  `+0x10/0x20/0x30/0x40`, then calls `MeshShapeFill` with `record + 0x20`. The
  source arrays are the record's own, built here; the geometry is a **computed
  box**, not stored data.
* `0x1420a02f6`: a **shape-source assembler**. `[r8+0x140]`/`[r8+0x148]` is
  an inline-or-heap container of pointers; the selected pointer is a descriptor
  with a type byte `+0x2a == 1`, a mode at `+0x20` (0..3) and data pointers at
  `+8/+0x10/+0x18`. Per mode it unpacks `{pointer, count}` pairs with
  `0x141b53b70` (`[pair+8]`/`[pair+0x18]`, inline when the tag word is 1) or
  `0x141b53af0`, and indexes a second descriptor's arrays at **`0x60` stride**
  (`[r14+0x20] + [r14+0x34] * 0x60`, mode 0) or **`0x40` stride**
  (`[r14+8] + [r14+0x28] * 0x40`, mode 3). This is the consumer of the
  three-array mesh-source form the ingest contract describes — the *end* a
  chunk decoder would have to feed — but its descriptors come from the owner's
  `+0x140` container, not from a chunk. **Correction (2026-11-02): it is not
  unreachable.** The address above is a `.pdata` **chained fragment**; the chain
  root is **`0x14209fc90`** (six chained entries spanning
  `0x14209fc90..0x1420a07a0`, not four to `0x1420a075d`), and that root has
  one `E8` at **`0x1420251f5`** inside `0x142024d30`, which is called from the
  collision component's per-frame update slot (`0x142019944`, slot
  `0x142f7d688`) and from `0x14201fbe3`. The earlier "unreachable" verdict
  measured the fragment start and does not hold.
  **Correction (check 15 of `formats/static-collision.md`): it is not a chunk
  consumer, and three details above are wrong.** (a) `r8` is the **`STUModel`**
  the assembler resolves from the component's node — `[[node+0x110]+0x28]`,
  falling back to the accessor `0x141b3d1c0(node)` — so `+0x140`/`+0x148` is the
  model's **holder list**, field `0xCB4D298D`, the same slot and element type
  the per-model body builder walks at `0x14201b6c0`; it is not the assembler's
  container, which is a separate `0x50`-byte context (`Alloc(0x50, 0)` at
  `0x1420251d1`, constructor `0x14209ea30`). (b) The mode at `+0x20` is the
  four-arm shape switch `0`, `1`, `2`, `4` — `3` is skipped, as in the body
  builder. (c) The `0x60`/`0x40` strides above are two of **four**: mode `0`
  `0x60` (array `+0x20`, count `+0x34`), mode `1` `0x70` (`+0x18`/`+0x30`, with a
  `0x10`-byte count header), mode `2` `0x50` (`+0x10`/`+0x2c`), mode `4` `0x40`
  (`+0x08`/`+0x28`). The descriptors are the model's holders whose **mode byte
  `+0x2a` is `1`** (the body builder takes `0`), and that byte is the holder
  class's declared field `0x89AABA94` at `+0x2a` (`0x141510382`), so no chunk
  decoder feeds this function: on the practice range 0 of 279 holders carry it
  and the assembler returns before allocating.

**Anchor 3 — `0x142cdf9f2` (0x131 bytes).** A **Domino body-list appender**:
`rsi` is a vector holder `{data +0x18, capacity +0x20, count +0x24}`; it grows
the vector (`capacity += capacity>>1`, minimum 2, `0x142cadee0` with a `0x10`
allocator tag) and copies `count << 6` bytes, so the element is **`0x40`
bytes**; it then takes slot `count` and writes `+0x20` = 0, `+0x24`/`+0x28` =
two floats from the caller's transform record (`[rdx+0x20]`, `[rdx+0x40]`),
`+0x2c` = `0xffff`, `+0x2e` = `0xff`, `+0x30` = a `u16`, `+0x34` = the body
from `OwnerCreateBody(word[rsi+0x58], ...)`, `+0x3c` = 0, then calls
`0x142cb4f20(body, byte[rsi+0x160] + 1)`, the handle-table setter for
`0x1442a0bc0`. The `0x40`-byte records are **produced by this function itself**
— it appends them to the owner's vector; nothing in it reads a chunk.
Reachability: no caller, no pointer, no encoded pointer; its only references
are the same `.pdata`-copy and code-range tables as above.

**Why a "data reference" here is not an invocation route.** Every function start
in this image carries at least one DATA reference in the `0x1445xxxx`-`0x1447xxxx`
region (sampled: 8 of 8 random function starts), and `ShapeAddToBody` itself has
one (`0x14473d0d4`) beside its 27 call sites — so those references are the
image's `.pdata`/range copies, not callbacks. The extra references on the
functions above are the same family: code-range pairs whose tables nothing
reads (0 code xrefs to the table addresses) and which chain into each other.
`dev/data/read/refs.py` prints all six spellings for a given address
(8-byte pointer, 4-byte RVA, both obfuscated forms), and
`docs/ghidra-project.md` documents why Ghidra's caller metadata cannot be used
alone.

**Reachability caveat (2026-11-02, then corrected the same day).** The
"unreachable" test above used `E8`, `E9`, 8-byte pointers, `imm64` and the
encoded-pointer idiom — not 4-byte RVA tables. A first pass then read the
`0x14469xxxx` region as a dispatch table and called `0x14208edf6` reachable;
that was **wrong**: those records are 12-byte `.pdata` copies
(`{unwind RVA, begin RVA, end RVA}`), not dispatches. The only computed jump
found is the one at `0x14208e74e..0x14208e75f` (`mov ecx,[base+rax*4+0x2090988];
add; jmp rcx`), and its table selects **fragments of one function body**, so it
proves the fragment is a switch case, not that anything calls the function. So
the three "unreachable" claims here are **not** relied on either way; what
stands is the content census (no decoded store fills the pools from a chunk
payload), not a reachability argument.

**What remains.** The chunk path is neither opened nor closed by these reads:
the three next readers are Domino/brush-side, so they cannot be
the cell consumer for a live path, and they say nothing about a consumer that
does not exist yet. The single next reader is the **producer of the owner's
mesh-descriptor container at `+0x140`/`+0x148`** (whoever fills it with the
`+0x2a == 1` descriptors that `0x1420a02f6` consumes): that is the last object
between a stored mesh source and a shape. One narrowing measurement: in the
collision/domino module (`0x14200000`..`0x142d0000`) **no store** to `+0x140`
or `+0x148` exists at all (a decoded scan of every `.pdata` function in that
range, register or immediate source), so the container is filled by a
constructor or loader outside that module — the next search starts there.

**That producer is read (check 15 of `formats/static-collision.md`), and it is
not a chunk loader.** The `+0x140`/`+0x148` list is the **model's** holder list
(field `0xCB4D298D` of `STUModel`), the same list the per-model body builder
walks, and the `+0x2a == 1` byte is the holder class's declared field
`0x89AABA94` at `+0x2a` — filled by the model asset's instance loader, which is
exactly why no store to `+0x140`/`+0x148` exists in the code module: the array
header is written by the reflection loader from the asset, not by a hand-written
store. `0x1420a02f6`'s own container is a `0x50`-byte context created empty at
`0x1420251d1`; its four arrays have no reader in the read set. On the practice
range 0 of 279 holders carry the mode byte, so the assembler takes nothing.

### Which chunk kind is which: **read** (2026-11-02)

The mapping is settled from the client's own bytes, twice over:

* the kind table `0x1439aa320` (`0x24` entries, stride `0x20`, kind at `+8`,
  name pointer at `+0x18`) and the name decoder `0x1404c19b0` — per name, the
  length is the low 24 bits of the dword at `name-4`, the key is the eight bytes
  at `name-0xC`, and byte `i` XORs with `key[i & 7]`. All 36 names decode, and
  the eight placeable names this project had already established from the
  install come out exactly, which is the check that the decode is right;
* the per-slot dispatch in `FinishMapChunkAsset` (`0x141ff54e0`): the two kinds
  the table names collision are exactly the two sent to `FUN_142e492a0`
  (slots `0x1D`/`0x21`), and the two named pathing are exactly the two sent to
  `StuLoadSubAssets` + `MapChunkCellBytesFixup` (slots `0x1E`/`0x22`).

| kind | slot | name |
|---|---|---|
| `0x11` | `0x1D` | `MAP_CHUNK_COLLISION` |
| `0x12` | `0x1E` | `MAP_CHUNK_PATHING` |
| `0x10` | `0x21` | `teMAP_CHUNK_ADDITIVE_COLLISION` |
| `0x23` | `0x22` | `teMAP_CHUNK_ADDITIVE_PATHING` |

So the practice range's `0x0DD0001100000688` (21.8 MB) is collision and
`0x0DD0001200000688` (8.79 MB) is pathing. The collision container, its sector
array and its fixup are read in `formats/chunk-kinds.md` and implemented in
`ow_tank::chunkkind` (7 tests, install-pinned).

**The collision consumer is still open, and that is the blocker now.** The
dispatch only fixes the payload up; `FUN_141fe37a0` stores it at `work+0x60`;
`FUN_142e492a0` is called only from `FinishMapChunkAsset`; no producer in the
28-function census can see a sector array; and nothing reads the two strides
(`0x58`, `0x38`) together with the counts at `+0x08`/`+0x0C`. The missing read is
the one both routes name: **who fills the Domino shape-entry pool at
`[record+0x2b8]`**. One correction to the earlier text: `BuildBvh`
(`0x142ccb1b0`) is **not** callerless — a verified `E8 rel32` scan finds five
call sites in three functions (`0x142cca7d9`, `0x142ccb2db`, `0x142ccbc1c`), so
the collider builder is reachable and its callers are candidates.

### The pathing component's three arrays tile the blob (2026-11-02)

The pathing chunk's instance is a container, and its three array-bearing fields
tile the blob with no gap. All offsets are container-relative (the blob starts at payload
`+0x20`), and each header is the same `{u32 count; u32 pad; u32 data_off}` v1
header as the verified `m_cellBytes` field:

```text
+0x98   {222, pad, 0xA8}     222 x 0x28  -> 0x2358   nested in the field at +0x08
+0x2358 {6,   pad, 0x2368}     6 x 0x20  -> 0x2428   the field at +0x58
+0x2428 {220, pad, 0x2438}   220 x 0x10  -> 0x31F8   m_cellBytes, the cells
```

The strides are **derived, not read**: each array runs from its data offset to
the next structure's header and the stride is that span divided by the count
(`0x22B0/222 = 0x28`, `0xC0/6 = 0x20`, `0xDC0/220 = 0x10`). A span that does
not divide exactly is refused rather than rounded
(`ow_tank::collision::tables`, `Error::ArrayNotTiled`).

The element contents were **observed, not interpreted**, and nothing here names
a field:

* the six `0x20`-byte elements each hold eight zero bytes, `07 00 00 00`,
  `00 00 00 00`, then sixteen bytes whose first three lanes read as plausible
  coordinates and whose fourth lane is zero;
* the `0x28`-byte elements hold two 8-byte hashes, a `u32` that increases by
  one or two between neighbours, four zero bytes, a `u64` that is the same in
  all 222 (`0x0208BF0D9A50FD55`), and an 8-byte value that equals a cell's
  block size;
* 220 of the 222 elements carry the cells' block sizes in cell order, with one
  extra element before the correspondence shifts: elements `0..=121` match
  cells `0..=121`, elements `122` and `133` carry the same value (`0xA30`, a
  size another cell also has) and are extras, and after them the shift is two
  (`element 221` matches `cell 219`). The meaning of the extras is unread.

**Evidence class:** data read from the install plus a tiling proof; there is no
read expression for the element meaning or for the stride constant, and
`ow_tank::collision::tables` documents exactly that. The install test
`collision_cells_real::the_collision_component_holds_three_tiled_arrays` pins
the three headers, the tiling chain, the strides and the raw bytes of the first
element of each array.

## Verification

```text
cargo test --release -p ow-tank --test map_header_bundle_real -- --nocapture
cargo test --release -p ow-tank --test map_resolve_real -- --nocapture
cargo test --release -p ow-tank --test practice_range_real -- --nocapture
cargo test --release -p ow-tank --test mapchunk_kinds
cargo test --release -p ow-tank --test manifest_154504_real -- --nocapture
cargo run --release -p ow-client -- --practice-range --shot C:\Temp\practice_client.png --width 1280 --height 720
cargo run --release -p ow-client -- --practice-range --tour C:\Temp\practice_client_tour --count 4
cargo run --release -p ow-client -- --list-maps
cargo run -p ow-tank --bin look -- --why 0x0790000000000688
cargo run -p ow-tank --bin look -- --why 0x0800000000000688
cargo run -p ow-tank --bin look -- --why 0x0dd0001100000688
cargo run -p ow-tank --bin look -- --hex 0x0800000000000688 --len 1056
cargo run -p ow-tank --bin look -- --root --filter "Win_SPWin_RDEV_"
cargo run -p ow-tank --bin assoc -- --type 0x09F --limit 30
cargo run -p ow-tank --bin assoc -- --type 0x0BC --limit 5
```

`--why` now prints the CMF record byte and whether the record is bundle-hosted;
`--hex` opens through the same client path, so a bundle-hosted asset dumps too.

`tests/map_header_bundle_real.rs` is the header oracle: it opens the practice
header through `GameData::open_asset`, asserts the record byte, the bundle GUID
and offset, the 1132-byte length, class `0x02514B6A`, `mapName` and `m_map`,
opens the base map asset, then reads the chunk-kind table from the installed
executable, finds the two chunk assets under the base map's index, checks each
GUID against the formula and opens and parses both. It also asserts that the
base map's 31 synthesised slots yield nothing local, which is why the base is
not the scene. The run prints the index size (38,320 bundles, 0 malformed, 721,204
entries, 0 duplicates) and the header census (305/305).

`tests/map_resolve_real.rs` is the scene oracle: it opens the same header,
reads the three `(mode, variant, layer, scene map)` variations from
`m_D97BC44F` / `m_78715D57`, selects mode `0x18`'s single variation, opens its
`m_BF231F12` map asset, asserts the four stored chunk references, checks the 31
synthesised GUIDs against the manifest's own records under the variation map's
index, then opens and walks all 34 chunks. It pins the placeable count (1,358),
the model and entity counts (229/550) and the bounds bit-for-bit. It also
proves that the base map opens no chunk and that variation 0 differs from
variation 1 in the entity chunk while sharing the model-group chunk. Two
further tests pin the selector finding: the 63-variation header has three
identifiers (21 entries each) and one mode names six entries, two scene maps
per identifier; and the replay header's map GUID (`0x080000000000069E`) is a
header's `m_map`, not any variation's scene map. The practice test also pins
the shared `STUIdentifier` variant, which is not an asset.

**Not tested:** the client's bundle-directory producer (the index is our scan of
the same relation); the producer of the level-load GUID lists (the client never
reads the header's mode/variation arrays, so no client selector exists to
test); the chunk body layout of kind `0x12` at the placeable level (its size
walk stops at the first placeable, so the test reports rather than asserts the
count).


The default `ow-client` launch draws the practice range's resolver-set chunks
and model-group entries with the viewer's `wgpu` renderer. It is a viewer, not
a claim that the client's renderer is reimplemented. `--chunks-from` remains an
explicit diagnostic; a chunk list is not a map.

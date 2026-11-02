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
  map request to the scene map), the streaming policy (which chunks load when),
  the entity-placeable layout and the collision mesh.


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

The map ships collision chunks (`MAP_CHUNK_COLLISION` kind `0x12`,
`teMAP_CHUNK_ADDITIVE_COLLISION`). The client draws the visual mesh only;
there is no character collision implementation.

### The collision chunk payload (2026-11-01)

The practice range's collision chunk `0x0DD0001200000688` is **8,792,256**
bytes (`0x8628C0`) and is the base map's `+0x1C0` reference. Read as data, it
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
`FinishMapChunkAsset` (`0x141ff54e0`, 12 bytes — the project shows a truncated
body, read it from the image) dispatches on `ChunkSlotForGuid(work+0x70)` and
has a case for slot **`0x1e`** and one for slot **`0x22`**. Both run the same
three steps and differ only in the sub-asset offset:

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
identical. The neighbouring slots are other systems and are not collision:
`0x1f`/`0x1d` → `FUN_142e492a0`/`FUN_142e493e0`, `0x21`/`0x23` →
`FUN_142e49270`, `0x1e`-style header fixup also appears for `0x1e` only; slots
`7`/`0xf` do their own inline relinking (0x40-stride records with a 0x10
array, 0x58-stride with 0x20), and slot `8` is the GUID-keyed STU container.

**The consumer of those cells is still unwritten.** The fixup only rewrites
pointers; nothing in this dispatch builds shapes. The ingest therefore splits
cleanly in two, and only the second half is open:

| half | state |
|---|---|
| chunk → STU sub-asset → cell container (slot `0x1e`/`0x22`, header, 17 arrays, pointer fixup) | **read, `fn/MapChunkCellBytesFixup` verified** |
| cell container → shape records in the query world | **open** |

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

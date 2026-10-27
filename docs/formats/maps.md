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
- **Map variations: data pairing read; selector open.** `m_D97BC44F` has one
  entry per `m_78715D57` variation (practice 3/3, Cairo 10/10, header
  `0x07900000000000d4` 63/63) while `m_C0CA4671` is a shorter display list
  (21 entries for that 63-variation header). Each variation's `m_BF231F12` is
  its registered scene map asset; the client function that chooses a variation
  for a game mode is not read.
- **Practice-range launch: data-selected, implemented.** `ow-client`'s default
  finds the header whose `m_mapName` is
  `Maps\TrainingRoom\Training Room - Practice`, then takes the first paired
  variation whose mode's `STUGameMode.displayName` (`0x5DB91CE2`) is exactly
  `Practice Range`. It resolves that variation's scene map through
  `mapresolve::resolve_scene` and draws only that chunk set. The selection is a
  query over the install, not the client's own mode selector.
- **Model groups: expanded.** A kind-`0x01` placeable's own transform is
  identity; its 304 groups carry the looks and its 2,449 entries carry the
  world transforms. The practice-range client places the entries, not one
  instance per group placeable.
- **Open:** the client's *producer* of the bundle directory (the blob handed to
  the bundle job), the variation-selection caller, the streaming policy (which
  chunks load when), the entity-placeable layout and the collision mesh.


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
| variation `m_BF231F12` | variation `+0xD0` | that variation's **scene** map asset (a registered type-`0x002` asset) |
| variation `m_364ADD04` | variation `+0xC0` | that variation's layer key |

The offsets are the ones the client registers at `RegisterStuMapHeaderFields`
(`0x14143b5c0`) and `RegisterStuMapHeaderTail` (`0x14143bbe5`);
`STU_7FB10A24` is registered at `RegisterStuMapVariation` (`0x14142dbf0`).
`m_78715D57` and `m_D97BC44F` are paired **by index**, which their lengths prove
on the install: the practice header has 3/3, Cairo 10/10, and header
`0x07900000000000d4` 63/63. `m_C0CA4671` is a different display list: 21
entries for that 63-variation header, and on Cairo its 10 modes are in another
order. The client function that consumes the pairing for a game mode is **not
read**; see `docs/open-questions.md` item 11b.

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
`docs/open-questions.md` item 11b.

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

| index | mode (`m_gameMode`) | mode name | layer key (`m_364ADD04`) | scene map (`m_BF231F12`) |
|---|---|---|---|---|
| 0 | `0x0230000000000018` | Practice Range | `0x080001000f69002e` | `0x080000000f69002e` |
| 1 | `0x0230000000000143` | Stadium | `0x0800010038792434` | `0x0800000038792434` |
| 2 | `0x023000000000005a` | Practice Range | `0x08000100878409c8` | `0x08000000878409c8` |

The two "Practice Range" modes are distinct entries; a mode query returns the
one variation whose `STU_71B2D30A.m_gameMode` equals the requested mode. The
client's selector is not read, so `ow-tank::mapresolve` exposes the pairing, the
per-mode filter and the named practice-range query below; none is claimed as
the client's own selector.

#### The launch's selection rule

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
That first-in-header-order choice is a **launch affinity**, because this header
offers no other discriminator in the read fields; it is not a claim that the
client's mode selector is read. `--list-maps` prints both rows, so the
ambiguity is visible. The selector caller remains item 11b in
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
variations come from the header.

### Streaming and selection

`teMAP_CHUNK_STREAMING` names the streaming kind. The policy - which chunks
load when, and how the engine chooses among a header's variations - is **not
read**. The chunk set for a map asset, including the four stored kinds and the
31 synthesised GUIDs, is read and implemented.

**A chunk list is not a map.** The client's chunk set is the synthesised set
for one map GUID; a proximity list is not a substitute.

## Collision

The map ships collision chunks (`MAP_CHUNK_COLLISION`,
`teMAP_CHUNK_ADDITIVE_COLLISION`). The client draws the visual mesh only;
there is no character collision implementation. Reading the collision chunk
layout and the client's step/sweep path is the next collision work.

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
reads the three `(mode, layer, scene map)` variations from `m_D97BC44F` /
`m_78715D57`, selects mode `0x18`'s single variation, opens its
`m_BF231F12` map asset, asserts the four stored chunk references, checks the 31
synthesised GUIDs against the manifest's own records under the variation map's
index, then opens and walks all 34 chunks. It pins the placeable count (1,358),
the model and entity counts (229/550) and the bounds bit-for-bit. It also
proves that the base map opens no chunk and that variation 0 differs from
variation 1 in the entity chunk while sharing the model-group chunk.

**Not tested:** the client's bundle-directory producer (the index is our scan of
the same relation); the client function that selects a variation for a game
mode (the pairing and the per-mode query are read, the caller is not); the
chunk body layout of kind `0x12` at the placeable level (its size walk stops at
the first placeable, so the test reports rather than asserts the count).


The default `ow-client` launch draws the practice range's resolver-set chunks
and model-group entries with the viewer's `wgpu` renderer. It is a viewer, not
a claim that the client's renderer is reimplemented. `--chunks-from` remains an
explicit diagnostic; a chunk list is not a map.

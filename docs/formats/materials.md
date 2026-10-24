# Materials and textures

## Current state

- **Chain: read and joined.** Model -> MDLC material id -> look -> material
  asset (`0x008`) -> material data (`0x0B3`) -> texture (`0x004`). The join has
  an ambiguity decline.
- **Textures: decoded.** BC1-7 and BC6H decode to RGBA8, oracle-checked
  against the Microsoft reference decoder; 98.7% readable census.
- **Data limit:** the `0x008` material asset is 16/2000 readable on this
  install (~1%; 2/300 in a spread sample). The readable subset renders
  textured; the rest is content the client streams, not a parsing gap.

**Verify:**

```text
cargo run -p ow-tank --bin look -- --join 400
cargo run -p ow-tank --bin tex -- --census 400
cargo test --release -p ow-tank --test bc6h
```

The path from a model to a textured surface:

```text
model          MDLC: an array of 64-bit material values
  material     type 008: shader group (085), shader source (088),
               unknown (03A), material data (0B3)
    data       type 0B3: STUMaterialData, holding texture references
      texture  type 004: a header and DXT/BC payload
```

Addresses are capture coordinates for build `2.25.0.0.154088`; capture
translations are in `docs/ghidra-project.md`. Nothing below is copied from the
game.

## File side

### The model's MDLC chunk

The payload has `MaterialOffset` (i64 at +8, relative to the payload) and
`MaterialCount` (u16 at +104), then `count` 64-bit values at that offset. A
second offset/pointer at payload+0 has its count as the byte at +106; on most
models it is zero.

The u64 values recur across models that share a material, so they are a
property of the material, not the model. They are shared and stable: 6,598
distinct across 52,626 values, an ~8x reuse factor.

They are **not asset GUIDs**. Decoding each value with the project's GUID
type/index split gives random type fields, and 0 of 52,626 values resolve as
assets. Building the set of every 8-byte value in every locally available
material asset (819 assets, 1,246 distinct qwords) and testing all model values
gives 60 hits, all the zero value; the reverse finds no non-zero overlap.

The model also ships a **u32 array**, but it is a separate array, not derived
from the u64 values. It is present in only a minority of models (99 of the
first 2000), holds small slot ids such as `[1, 2, 0x15, 0x16]`, and never
equals `low32` of the paired u64 values. Its count is the byte at +106.

`mskl` exists in the files (lowercase tag; 83 of the first 575 models). Its u32
arrays hold small ids, its u16 array is index-like with `0xffff` sentinels, and
its u64 array ships as all `0xffffffffffffffff` — a placeholder table the
runtime fills, or an "unset" marker.

### The material asset (008)

A material is a plain 32-byte header with four GUIDs, not an STU:

| offset | reference | type |
|---|---|---|
| `+0x00` | shader group | `0x085` |
| `+0x08` | shader source | `0x088` |
| `+0x10` | unknown | `0x03A` |
| `+0x18` | material data | `0x0B3` |

The four GUIDs are not obfuscated. The model's own `MSTU` does not repeat the
MDLC values; its reference fields do not deobfuscate to material GUIDs under
the STU scheme with any of the model's field hashes (tested over every byte
offset of the blob; the only type-008 decode was a one-in-4096 coincidence).

### The material data (0B3)

A header then a texture array: texture offset i64 at +16, texture count as the
byte at +60, and each record a texture GUID (`0x004`) followed by a CRC32 of
the input name and a flag byte.

### The texture (004)

A 32-byte header: `u16 flags`, `u8 mip_count`, `u8 format` (a DXGI id, stored
one higher from 26 up, so the byte is decremented first), `u8 surfaces`,
`u8 usage`, `u8 payload_count`, `u8 unknown`, `u16 width`, `u16 height`,
`u16 data_size`, then two u64s.

Formats seen in a 400-asset census: BC1 (187), BC4 (96), BC5 (56), R8G8B8A8
(5), BC7 (39). Decoded: 344 of 400 at the time of that census; the BC7 decoder
is now implemented and oracle-checked.

The payload chain is two or three links:

* `payload_count == 0`, or a payload whose `image_size` covers level 0: blocks
  follow the 32-byte header.
* Otherwise payload 0 is embedded at +0x20 (four u32s: `mips`, `surfaces`,
  `image_size`, `header_size`) and can hold only the small mips. Measured: a
  512x512 BC5 texture embeds 21840 B = the sum of mips 2..7, with levels 0..1
  in a separate asset.
* The separate payload asset is found by GUID: type `0x04D`, payload index in
  the locale nibble (`guid::payload_of`). For `0x0c0000000000007f` it resolves
  to `0x032000010000007f`, 262160 bytes = a 512x512 BC5 level plus the payload
  header. Chains of three are real (1024x1024 BC5), so the reader tries
  payloads 1..3 and keeps the first whose block run covers level 0.

A 1024x32 RGBA8 texture (`0x0c00000000000003`) decodes to a hue-gradient strip
atlas, the tint ramp a team-colour pass samples; an 8x8 BC1 texture
(`0x0c0000000000000a`) to a flat near-white thumbnail. Both were written as
PNGs and re-decoded independently.

## The id join

A model placement carries a second reference. A `--refs` scan over 16,357 model
placeables in 41 map chunks finds exactly two resolving qword offsets:

| offset in placeable | value | type |
|---|---|---|
| `+0x00` | the model | `0x00C` |
| `+0x20` | the model's **look** | `0x01A` |

Every one of 14,093 placements in a 40-chunk census names a look, and all 8,331
distinct looks resolve. `Placeable.look` is now read at body +0x20.

A look is a `STUModelLook` (`0x5207484B`). Its first field, `0xBAFDAFBA`, is an
inline array of `STUModelMaterial` (`0x494B66C1`), 8-byte fields inside a
20-byte element:

| element field | meaning |
|---|---|
| `0x33E51FDC` | `m_material` — a `teStructuredDataAssetRef`, the material asset |
| `0xDC05EA3B` | `MaterialID` — the id the model names it by |

The asset ref is read **GUID first** in this position. The bytes settle it: the
first qword decodes to type `0x008` with a small index, the second to no type
in the table at all.

The join is the model's value against the look's id. Measured over 400
placements:

```text
400 placement(s): 381 model read, 62 look read, 0 look with no materials
  model material ids matching the look's id AT THE SAME INDEX: 60
  matching ANY of the look's ids: 84
```

Every model material id (84 of 84) appears among its look's ids. The 381-vs-62
gap is asset readability, not the join. The ids are values like
`0x6c39d35e401e3b9d`.

`ow_tank::material::base_texture_of_id` implements the join. An id naming two
materials is declined: `material_of_id` returns the material only when exactly
one look entry carries the id; zero and two-or-more both give `None`, and the
scene leaves those submeshes on the placeholder and names the id. On Tracer's
look (`0x098000000000028d`, 8 entries against 9 model ids with one id shared by
two submeshes): 0 missing, 0 ambiguous.

## Client path

The deserializer `FUN_7ff64ba31b80` copies the MDLC values **verbatim**:
`count*8` bytes from the relocated `MaterialOffset` into the runtime array at
`+0xb8` (pointer at `+0x38`). The u32 array is copied separately to runtime
`+0x30` (pointer) with its count at `+0xac`. There is no transform at parse
time. The mskl builder `FUN_7ff64ba323a0` is likewise a repack: payload-relative
offsets, no transform.

The client works with 32-bit keys, not the 64-bit values:

* `MaterialLookup_Index` (`0x7ff64b9e3320`) answers a u32 key with a material
  index by a linear scan of the u32 array (count at +0x66), returning -1 when
  absent.
* `FUN_7ff64ba2c880` caches key resolution with FNV-1a-32 over
  `(seed ^ table+0x60)`; the basis `0x811c9dc5` is set in
  `FUN_7ff64ba2ca20`.
* The per-model `+0x88` object is the material table. Its `+0x48` is a packed
  u64 material key, its `+0x60` is the material count, and `+0x00` is an array
  of 6-byte records whose `+4` is a u16 layer-pair index.
* `FUN_7ff7c68e4560(registry, keyA, keyB)` scans a packed-u64 key array
  (registry `+0x2f0`, count `+0x2f8`) and returns a 0x38-byte entry from
  `+0x330`; a miss is a no-return assert.
* `FUN_7ff7c68a3320(tableObj, key)` resolves a key to a record index; its body
  is obfuscated and is read by its call sites.
* `FUN_7ff7c68ec880(binding, tableObj, count)` binds 0xC-byte records
  `{u32 key, u16 index, ...}`: resolve key -> index, clamp by the u64-value
  count, store at record+4, sort. The pass is skipped when an FNV-1a hash of
  the count matches the binding's cached `+0x00`.
* `FUN_7ff7c68f30a0` + `FUN_7ff7c68f2830` bind renderer slots: slot count from
  `tableObj+0x60`, weights clamped to `[0,1]`, indices sorted, per-vertex
  average weight at container+4, per-slot weights at +8; layered entries merge
  through the table record's u16 at +4.

The material-parameter applier `FUN_7ff7c68f5940` takes a material record and
the model/material descriptor. Two field-bag reads (`src+0x68` -> `+0x190`,
`src+0x48` -> `+0x194`) feed the descriptor lookup; the scan keys are the
descriptor's u32s at `+0xac`/`+0xb0`. For each key it scans the model's 8-byte
key records at `model+0x48` (count = u16 at `model+0x06`), reads the parallel
8-byte value record at `model+0x58 + i*8` as a u16 at
`record + 4 * (i32 at record+4)` (a self-relative dword offset), converts to
float, scales by `DAT_7ff7c7d5d84c` (1/30), clamps <= 0 to 0, and stores the
min/max pair at `+0x240`/`+0x250`.

The field readers (`FUN_7ff7c68edb60` float, `edd60` u32, `ede00`, `edea0`
short, `edf90` byte) all take `(ctx, field)` and read a typed field record: an
int at `field+8` is the type tag; positive resolves through the ctx tables,
otherwise the raw dword at `field+0x10` is returned. Every `src+NNN` in the
material path is therefore a field in a bag, not a plain struct member.

The shipped u64 values are the **loader's input**, not the runtime records. A
probe over 3,796 models and 9,049 shipped values finds only 15 (0.17%) whose
low or high dword lands a u16 inside the MDLC payload, and all are zero. The
runtime arrays at `model+0x48`/`+0x58` are built at load time.

## Ruled out

| family | test | result |
|---|---|---|
| bit permutation + XOR | XOR-differences of same-model pairs had min popcount 22, median 32 | impossible for a bit permutation |
| permutation + constant | derive from value 1 against all 99,392 material GUIDs, require values 2 and 3; 71 permutations x 3 types | no |
| additive `K*guid + C` | GCD of all pairwise differences | 1 |
| common hashes | xxhash64/32/3, murmur3, fnv1/fnv1a, crc32/64, djb2, sdbm, jenkins96/oat, city, over LE/BE/index-only/TankLib forms | 0 landings |
| finalizer-only mixes | fmix64, splitmix64, xxhash avalanche, with/without XOR seeds | 0 landings |
| content key | first/last 8 bytes of manifest content keys | 0 landings |
| 008 GUID hypothesis | decode every model value as an asset type/index | 0/52,626 resolve; 60 hits all zero |
| u32 key = low32(u64) | probe u32 and u64 arrays across models | u32 values are small ids; counts differ; no |

The id question is now answered by the look join above, not by a hash.

## Why the files stop

The material assets named by the look are often not in this install. Sampling
across each whole type (a prefix sample is a sample of one package):

| type | what | in the registry | readable here |
|---|---|---|---|
| `0x075` | hero | 135 | **135 / 135** |
| `0x004` | texture | 205,512 | **293 / 300** |
| `0x00C` | model | 32,280 | **278 / 300** |
| `0x058` | statescript graph | 56 | 45 / 56 |
| `0x01A` | look | 43,139 | 57 / 300 |
| `0x0B3` | material data | 121,027 | 52 / 300 |
| `0x008` | material | 99,392 | **2 / 300** (~1%) |
| `0x078` | see `animation.md` | 5,117 | **0 / 300** |

The failures are `key … is not present in this installation`. The install's
keyring holds 327 entries; the community keyring the reference checkout ships
(312 entries) adds nothing measurable. The files are a **content split**, not
a keyring or lookup defect: geometry, textures, heroes and graphs are
installed; the material/look/material-data family is fetched on demand.

Every storage mechanism was checked for one missing material
(`0x0e000000000000e3`, Tracer's body material, 144 B):

* not in the sixteen bucket tables (`data/*.idx`), the per-archive indexes
  (`indices/*.index`) or `ecache`;
* the `shmem` writable layer (`data/`, `ecache/`, `pro/`) has the file as
  **20 KB of zeros** — the client never wrote it;
* no loose file, and the content key is absent from the install and download
  manifests and from a raw scan of the decoded 111 MB encoding manifest;
* all three region manifests have 1-2 of 200 readable; no other asset shares
  the content key; there is no second install on this machine.

The modern per-archive indexes do add **2,559,788** keys the bucket tables do
not have (`indices/*.index`: 3,684,920 keys against 1,147,893 buckets), which
makes client-downloaded files visible when present. They do not create the
missing material here. The archive number cannot be read off the CDN's 826-entry
list; it is learned from entries the bucket tables already place (the wrong
order once mapped every location to the wrong archive and took material
readability from 9/600 to 0/600).

In memory the material is not the on-disk shape. A captured heap scanned for
the four GUID signature (types `0x085`, `0x088`, `0x03A`, `0x0B3`) finds **0 of
208 MB**; at runtime it is a structure of pointers, not the packed record.

## The hero route

A hero's textures are named directly on the hero asset, so a textured model
does not depend on the material chain above. The gameplay entity's
`STUModelComponent` (`0x191835F9`) carries `0x9297ADB4`, a `STUModelLook`
(`0x01A`); hero 3 (`0x0400000000000001`) resolves to look
`0x098000000000028D`. That look names 8 materials and their ids, and the join
holds: the model's MDLC ids match the look's ids (9 submeshes over 8 materials;
the body's main submesh and one small one share `0x1F434DE703EA0BE4`).
`ow_tank::entity::hero_parts` reads it programmatically.

**This install blocks most of it.** Of those 8 materials, 3 are readable but
have no base colour: `0x0C000000000027ED` (BC5, mean RGB 126,126,0 — a
normal/mask), `0x0C0000000000039C` (BC7, mean RGB 0,0,0 — an alpha/mask),
`0x0C00000000005449` (BC1, mean RGB 35,34,35 — dark grey). The other 5 are
absent. **0 of 9 submeshes** have a readable base colour; the two textures the
hero asset names directly (`0x238EF37E` -> `0x0C00000000038C3D`, `0xFB366254`
-> `0x0C00000000007DF7`) are face and portrait UI assets, not body textures.

The base-colour rule is "the material's first **sRGB** texture" plus "it must
have structure in **at least two channels**" (per-channel standard deviation
< 2 rejected). Each rejection came from a texture that would otherwise have
been painted:

| texture | why it is not albedo |
|---|---|
| BC5 128x128, mean (126, 126, 0) | two-channel normal/mask pair |
| BC7 256x256, mean (0, 0, 0), sd 0.0 | flat fill (alpha mask) |
| BC1 128x128, mean (35, 34, 35), sd 0.0 | flat fill; painted the black forearm patch |
| BC1 512x512, mean (0, 0, 0), sd (8.2, 0.0, 0.0) | one channel; painted Reaper black-with-red |

Coverage on this install (`ow-tank-hero --textures --heroes 40`):

| hero | materials | readable | with a real colour map | submeshes |
|---|---|---|---|---|
| 2 (Reaper) | 1 | 1 | 0 | 1 |
| 3 (Tracer) | 8 | 3 | 0 | 9 |
| 4 | 7 | 4 | 3 | 7 |
| 7 (Reinhardt) | 3 | 2 | 2 | 2 |
| 8 | 7 | 4 | 3 | 8 |
| 9 | 7 | 4 | 3 | 9 |
| 10 | 6 | 3 | 2 | 6 |
| 21 | 9 | 1 | 1 | 18 |
| 22 | 7 | 3 | 2 | 7 |
| 32 | 2 | 2 | 2 | 2 |

`ow-view --hero 7 --hero-only` draws Reinhardt fully painted from his own
4096x4096 BC7 colour map. The rest are blank because their content key has no
encoding entry in the manifests this install ships; the client streams those
files. A readable texture resolves all the way to `archive data.052 at 0x…`.

## The 0xD4 chunk trees

All 332 installed type-`0xD4` assets were enumerated; 32 cannot load (content
key absent, the same streamed-content wall) and 300 parse as chunk trees. Each
is `EFCT`/`ECMP` and nested, not an STU: `0x0cb0000000000109` holds 51 chunks
with tags `EFCT`, `ECMP`, `ECTr`, `ECCN`, `ESHT`, and so on. The payloads do
not parse as STU blobs, so the route to a skin's own look is still missing.
Two `0xD4` references point directly at textures (`0x0c000000000026b`,
`0x0c00000000006f33`, both readable) — an effect pair, which is why they are
not skin content.

For Tracer there is **no fully textured variant on this install**: 5 materials
absent, 1 data-map only, 2 flat fills. The wiring is complete; the wall is
content, not code.

## Open

- The missing `0x008`/`0x0B3` content is streamed. Observing the client's own
  fetch, fetching from the CDN, or reading the runtime pointer graph are all
  operator decisions.
- The model's `MSTU` field hashes are the install-local route to texture
  references inside the model, skipping the material wrapper.
- A runtime capture with a hero rendered would hold decrypted material data as
  a pointer structure; a byte scan does not find it.

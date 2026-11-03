# Chunk kinds: which kind is collision

`docs/formats/maps.md` has called kind `0x12` `MAP_CHUNK_COLLISION` since the
cell container was decoded, and its own dispatch section says that assignment
was never read. It is now read, from two independent places, and the answer is
the other way round: **kind `0x11` is `MAP_CHUNK_COLLISION` and kind `0x12` is
`MAP_CHUNK_PATHING`.**

This file records the read, the per-slot consumers, the collision payload's
container, and what is still open. Nothing here is a guess: every name below is
decoded from the client's own bytes, and every consumer is the function the
client's own dispatch calls.

## 1. The kind table, and the name decode

The table is at `0x1439aa320`: `0x24` entries, stride `0x20`.

| offset | type | meaning |
|---|---|---|
| `+0x00` | `u64` | the slot index (0..=0x23) |
| `+0x08` | `u64` | the kind id: the byte a chunk GUID carries at bits 32..39 |
| `+0x10` | `u32` | raw; no reader found |
| `+0x14` | `u32` | raw; no reader found |
| `+0x18` | `u64` | pointer to the entry's obfuscated name |

Two readers pin the layout:

- `MapChunkGuid` (`0x141b32e20`) builds a chunk GUID as
  `0x0DD0000000000000 | (table[slot].id << 32) | map`.
- `ChunkSlotForGuid` (`0x141b32e70`) scans all `0x24` entries comparing the
  `+0x08` id with `(guid >> 32) & 0xFF` and returns the matching entry's `+0x00`
  slot. So "which kind is a GUID" and "which slot is that kind" are the same
  table read.

The names are obfuscated, and the routine that removes the obfuscation is at
**`0x1404c19b0`** (61 `int3` bytes before it, one `ret` at `0x1404c24ee`). It
deobfuscates all `0x24` names in place, one inlined block per entry in slot
order. Per entry the expression is:

```text
p    = table[i].+0x18          (the pointer)
len  = *(u32 *)(p - 4) & 0xFFFFFF
flag = *(u8  *)(p - 3)         (1 = still encoded; the routine clears it)
for k in 0..len:  p[k] ^= *(u8 *)(p - 0xC + (k & 7))
```

Nothing else is mixed in — no length-dependent key and no rotation. Each name
blob is `[8-byte key][u32 header: length in the low 24 bits, the flag in the top
byte][name bytes]`, and the table stores a pointer to the *name*, i.e. header
`+4`. The block for slot `0x1D` sits at `0x1404c22c3..0x1404c2313`; the length
dword it masks is at `0x1439aa0f0` and the key it XORs from is `0x1439aa0e8`,
so the arithmetic above is the client's, read from the instruction stream.

**Checks on the decode.** The eight placeable names this project had already
established from the install (`MAP_PLACEABLE_MODEL_GROUP`, `_ENTITY`, `_LIGHT`,
`_SOUND`, `_EFFECT`, `_TEXT`, `_REFLECTIONPOINT`, `_AREA`) come out of slots
`0x01`, `0x08`, `0x06`, `0x09`, `0x0A`, `0x05`, `0x03`, `0x07` exactly, and the
seven chunk names come out as the seven the vocabulary list in
`formats/maps.md` names. A wrong key or offset would break the first group
immediately.

### The table

| slot | kind | name |
|---|---|---|
| `0x00` | `0x00` | `MAP_PLACEABLE_UNKNOWN` |
| `0x01` | `0x01` | `MAP_PLACEABLE_MODEL_GROUP` |
| `0x02` | `0x03` | `MAP_PLACEABLE_OCCLUDER` |
| `0x03` | `0x04` | `MAP_PLACEABLE_REFLECTIONPOINT` |
| `0x04` | `0x06` | `MAP_PLACEABLE_LABEL` |
| `0x05` | `0x07` | `MAP_PLACEABLE_TEXT` |
| `0x06` | `0x09` | `MAP_PLACEABLE_LIGHT` |
| `0x07` | `0x0A` | `MAP_PLACEABLE_AREA` |
| `0x08` | `0x0B` | `MAP_PLACEABLE_ENTITY` |
| `0x09` | `0x0C` | `MAP_PLACEABLE_SOUND` |
| `0x0A` | `0x0D` | `MAP_PLACEABLE_EFFECT` |
| `0x0B` | `0x0E` | `MAP_PLACEABLE_FOG` |
| `0x0C` | `0x0F` | `MAP_PLACEABLE_INDOORBOX` |
| `0x0D` | `0x15` | `MAP_PLACEABLE_TYPE_MAP_REGION` |
| `0x0E` | `0x16` | `MAP_PLACEABLE_TYPE_SPAWN_GROUP` |
| `0x0F` | `0x17` | `teMAP_PLACEABLE_TYPE_TRIGGER_VOLUME` |
| `0x10` | `0x18` | `MAP_PLACEABLE_SEQUENCE` |
| `0x11` | `0x19` | `MAP_PLACEABLE_SPAWN_VOLUME` |
| `0x12` | `0x1A` | `MAP_PLACEABLE_LENSFLARE` |
| `0x13` | `0x1C` | `MAP_PLACEABLE_GLOBAL_SHADOW_REGION` |
| `0x14` | `0x1D` | `MAP_PLACEABLE_TYPE_EFFECTVOLUME` |
| `0x15` | `0x1E` | `MAP_PLACEABLE_TYPE_DYNAMIC_SPAWN_POINT` |
| `0x16` | `0x20` | `MAP_PLACEABLE_TYPE_AI_HINT_VOLUME` |
| `0x17` | `0x21` | `MAP_PLACEABLE_TYPE_CLOWN_CAR` |
| `0x18` | `0x22` | `MAP_PLACEABLE_TYPE_WATER` |
| `0x19` | `0x24` | `MAP_PLACEABLE_DEPLOYMENT_ZONE_VOLUME` |
| `0x1A` | `0x25` | `teMAP_PLACEABLE_TYPE_FRACTURABLE_CHILD_BREAKABLE` |
| `0x1B` | `0x26` | `teMAP_PLACEABLE_TYPE_SH_PROXY_VOLUME` |
| `0x1C` | `0x28` | `teMAP_PLACEABLE_TYPE_DIFFUSE_GI_VOLUME` |
| **`0x1D`** | **`0x11`** | **`MAP_CHUNK_COLLISION`** |
| **`0x1E`** | **`0x12`** | **`MAP_CHUNK_PATHING`** |
| `0x1F` | `0x13` | `MAP_CHUNK_SOUNDAREA` |
| `0x20` | `0x1B` | `MAP_CHUNK_DYNAMIC_SPAWN_GROUPS` |
| **`0x21`** | **`0x10`** | **`teMAP_CHUNK_ADDITIVE_COLLISION`** |
| **`0x22`** | **`0x23`** | **`teMAP_CHUNK_ADDITIVE_PATHING`** |
| `0x23` | `0x27` | `teMAP_CHUNK_STREAMING` |

**Evidence class:** read. Static data (`0x1439aa320`) plus the decoder's own
arithmetic (`0x1404c19b0`), checked against eight names established from the
install by a different route. No external source was used and no scheme was
invented.

## 2. The per-slot consumers

`FinishMapChunkAsset` (`0x141ff54e0`, 964 bytes) is the chunk asset's completion
virtual. It dispatches on `ChunkSlotForGuid(work + 0x70)`, so the case numbers
are **table slots**, not kinds. Read end to end, its cases are `7`, `8`, `0xf`,
`0x1d`, `0x1e`, `0x1f`, `0x21`, `0x22`, `0x23`, and every case falls through to
the same accounting call `FUN_141fe37a0(work, payload, size, 1)` and returns
`0x11`.

| slot | kind | name | what the case does |
|---|---|---|---|
| `0x1D` | `0x11` | `MAP_CHUNK_COLLISION` | `FUN_142e492a0(payload)` only, then the accounting call |
| `0x1E` | `0x12` | `MAP_CHUNK_PATHING` | `MapChunkHeaderFixup(payload)`, then `StuLoadSubAssets(*payload) + 0x68`, then `MapChunkCellBytesFixup` on that; then, if `payload[1] != 0`, `StuLoadSubAssets(payload[1])` |
| `0x1F` | `0x13` | `MAP_CHUNK_SOUNDAREA` | `FUN_142e493e0(payload)`: one pointer, at `+0x08`, null when zero |
| `0x20` | `0x1B` | `MAP_CHUNK_DYNAMIC_SPAWN_GROUPS` | *no case* — the slot is not dispatched on |
| `0x21` | `0x10` | `teMAP_CHUNK_ADDITIVE_COLLISION` | `FUN_142e492a0(payload)` only, then the accounting call |
| `0x22` | `0x23` | `teMAP_CHUNK_ADDITIVE_PATHING` | `FUN_142e49270(payload)`, then `StuLoadSubAssets(*payload) + 8`, then `MapChunkCellBytesFixup`; then `StuLoadSubAssets(payload[1])` |
| `0x23` | `0x27` | `teMAP_CHUNK_STREAMING` | `FUN_142e49270(payload)`: two pointers at `+0x00`/`+0x08`, null when zero |

The other three cases are not chunk kinds in the sense used here: slot `8` is
the GUID-keyed sub-chunk container (it walks `{count, offset, per-record
`0x141b32e50` lookup}` and loads each sub-asset, then calls `FUN_1421c34b0`),
and slots `7` and `0xf` do their own inline relinking of `0x40`-stride records
(with a `0x10`-stride sub-array) and, for slot `7`, also `0x58`-stride records
with a `0x20`-stride sub-array.

**What this proves.** The two kinds the client *names* collision are exactly the
two it sends to the *same* fixup, `FUN_142e492a0`; and the two it names pathing
are exactly the two whose payload it hands to `StuLoadSubAssets` and then to
`MapChunkCellBytesFixup`. There is no third candidate: soundarea and streaming
get a one- or two-pointer fixup and no sub-asset pass.

**Correction to `formats/maps.md`.** Its dispatch section pairs the slots as
"`0x1f`/`0x1d` → `FUN_142e492a0`/`FUN_142e493e0`". The code says otherwise:
`0x1d` and `0x21` both call `FUN_142e492a0`, and `0x1f` alone calls
`FUN_142e493e0`. The earlier pairing is what made the collision/pathing
question look undecidable.

### The fixup the two collision kinds share

`FUN_142e492a0` (`0x142e492a0`, read in full) does two things:

1. **Header pointers.** It converts four qwords at `+0x10`, `+0x18`, `+0x20`,
   `+0x28` to `payload + value`, and a stored zero stays zero (an explicit null
   test, like the other slot fixups).
2. **Two element arrays.** It walks the array at `+0x20` for `count_a` elements
   of `0x58` bytes (count at `+0x08`) and the array at `+0x28` for `count_b`
   elements of `0x38` bytes (count at `+0x0c`), and adds the payload base to
   four qwords of each `0x58` element (`+0x00`, `+0x08`, `+0x10`, `+0x18`) and
   to three qwords of each `0x38` element (`+0x00`, `+0x08`, `+0x10`). Those two
   passes are `0x141b53a70` and `0x141b53a60`, and they are **unconditional**:
   an element field of zero becomes the payload base, it does not stay null.

The header conversion happens before either walk, because the walks take their
bases from the two converted fields. A negative count runs no pass — the client
compares a signed counter against the count.

That asymmetry (null-preserving in the header, unconditional in the elements) is
the fixup's whole behaviour and is implemented operation for operation in
`ow-tank::chunkkind::fixup_in_place`.

## 3. The decision

| kind | slot | name | evidence |
|---|---|---|---|
| `0x11` | `0x1D` | `MAP_CHUNK_COLLISION` | name table (read) **and** the slot `FUN_142e492a0` serves |
| `0x12` | `0x1E` | `MAP_CHUNK_PATHING` | name table (read) **and** the `StuLoadSubAssets` + `MapChunkCellBytesFixup` slot |
| `0x10` | `0x21` | `teMAP_CHUNK_ADDITIVE_COLLISION` | name table (read) **and** the same `FUN_142e492a0` slot |
| `0x23` | `0x22` | `teMAP_CHUNK_ADDITIVE_PATHING` | name table (read) **and** the cell-container slot |

**Evidence class:** two independent reads that agree — the decoded name table,
and the identity of the function each slot's case calls. The payload shapes
agree as well (`0x11` is 21.8 MB of sectors, vertices and index runs; `0x12` is
the 8.8 MB 220-cell container), but that is corroboration, not the basis.

**Consequences for the rest of the docs** (recorded here because this file owns
no other doc):

- `formats/maps.md`'s collision section, and `ow-tank::collision`'s module
  comment, call the 220-cell `STU_9199BF6B` container "the collision chunk". By
  the kind table it is the **pathing** chunk (`MAP_CHUNK_PATHING`, kind `0x12`,
  slot `0x1E`, the base map's `+0x1C0`). Everything those texts record about the
  container, the tiling and the verified `MapChunkCellBytesFixup` expression
  stands; only the *label* of the kind is wrong. A navmesh is the natural
  reading of 220 cells with quantised vertices, `0xFF`-separated index runs and
  per-cell attribute bytes, which is why the label was plausible.
- The same applies to the "collision" language around `m_cellBytes` and around
  `docs/formats/brush-collision.md`'s "the cell path". The *cell* path is the
  pathing path.
- The `+0x1B8` reference (`kind 0x11` on every map seen) is the collision chunk,
  and `+0x1C0` (`kind 0x12`) is the pathing chunk.

## 4. The collision chunk's payload

The collision slot never calls `StuLoadSubAssets`, so the payload is the
container: no `STUD` blob, no sub-asset. Its header is what `FUN_142e492a0`
reads.

| offset | type | meaning |
|---|---|---|
| `+0x00` | `u64` | not touched by the fixup; unread |
| `+0x08` | `i32` | `count_a` — elements of the `0x58`-byte array at `+0x20` |
| `+0x0C` | `i32` | `count_b` — elements of the `0x38`-byte array at `+0x28` |
| `+0x10` | `u64` | RVA, converted; zero stays zero; unread meaning |
| `+0x18` | `u64` | RVA, converted; zero stays zero; unread meaning |
| `+0x20` | `u64` | RVA of array A, converted |
| `+0x28` | `u64` | RVA of array B, converted |
| `+0x30` | `u64` | not touched by the fixup; unread |

### Practice range, `0x0DD0001100000688` (21,858,688 B)

Header: `+0x00` 0, `count_a` **36**, `count_b` **1**, `+0x10` 0, `+0x18` 0,
array A `0x14D3810`, array B `0x14D8948`, `+0x30` `0xFFFFFFFFFFFFFFFF`.

The payload tiles exactly, with no slack, and the whole 21,858,688 bytes are
accounted for:

```text
[0x000000, 0x000040)  header (0x38) + 8 bytes to the first run
[0x000040, 0x31CE30)  array B's first run
[0x31CE30, 0xA581D0)  array B's second run
[0xA581D0, 0x14D3810) array B's third run
[0x14D3810, 0x14D8948) 36 sector records of 0x58
[0x14D8948, 0x14D8A00) array B's single record of 0x38  (ends the payload)
```

**Array A — 36 sector records of `0x58`.** Each record holds four RVAs at
`+0x00`, `+0x08`, `+0x10`, `+0x18` (converted by the fixup), then floats and
counts that the fixup does not touch. Measured over all 36 records, and
**derived, not read**: the three dwords at `+0x2C`, `+0x30` and `+0x34` are the
element counts of the four runs, and the four runs tile the region between this
record and the next:

```text
run at +0x00:  count(+0x2C) elements, 12 bytes each
run at +0x18:  count(+0x30) elements,  1 byte  each
run at +0x08:  count(+0x30) elements, 16 bytes each
run at +0x10:  count(+0x34) elements,  4 bytes each
```

The memory order is always `+0x00`, `+0x18`, `+0x08`, `+0x10`. Pinned examples:

| sector | four RVAs (`+0x00`,`+0x08`,`+0x10`,`+0x18`) | counts (`+0x2C`,`+0x30`,`+0x34`) |
|---|---|---|
| 0 | `0x14D4470`, `0x14D460C`, `0x14D47CC`, `0x14D45F0` | 32, 28, 116 |
| 4 | `0x14D5920`, `0x14D5986`, `0x14D59E6`, `0x14D5980` | 8, 6, 24 |
| 35 | starts `0x14D87A0`, ends `0x14D8948` | 12, 8, 36 |

Counts per sector over the whole chunk: `(32,28,116)` ×4, `(8,6,24)` ×4,
`(10,10,36)` ×4, `(13,10,42)` ×4, `(14,9,42)` ×4, `(8,6,24)` ×10, `(12,8,36)`
×6. The first run reads as `f32` triples: sector 0's first vertex is
`(16.224941, 8.705036, -3.820958)` by bits `0x4181CCAE 0x410B47D4 0xC0748A94`.
**The stride and the counts are exact for all 36 records; the field meanings are
an observation, and no read expression names them.**

**Array B — one record of `0x38`.** Three RVAs at `+0x00`, `+0x08`, `+0x10`,
converted by the fixup: `0x31CE30`, `0x40`, `0xA581D0` — exactly the three run
boundaries above, ascending and starting straight after the header. The rest of
the record is fixed data (`+0x18` `0x00031CDF00039D9D`, `+0x28` `0x4935E87A`,
`+0x30` `0x1FFF`) and is not converted. The first run is `0x10`-byte records
with a float triple (`-103.46868, -38.509998, -161.02832, 206.15868` in the
first one); the second is `0x10`-byte records of `u32` indices (`0x1D5FF, 0x1D603,
0x1D5FD, 0x1D602`, then `0xFFFFFFFF, 0x1D5FE, 0xB5, 0x1FFF`); the third is
`0x10`-byte records. Those contents are **observed, not interpreted**.

The separate kind-`0x12` chunk does *not* parse as this container: its `+0x20`
qword is inside its `STUD` sub-asset blob, so the count/offset pair does not
resolve and the reader refuses it. That is asserted in the test, so the two
payloads cannot be confused again.

**One payload sampled.** The additive collision kind `0x10` (slot `0x21`)
runs the same fixup and is therefore expected to carry the same header; no
kind-`0x10` chunk was opened, so that is expected, not read.

## 5. What is implemented, and how it is proven

`crates/ow-tank/src/chunkkind.rs` (new):

- `TABLE`, the 0x24 rows above, with `entry_for_kind`, `slot_for_kind`,
  `kind_of_guid`, `kind_name`, and the seven chunk-kind constants.
- `Collision::parse`, the header `FUN_142e492a0` reads plus bounds checks.
- `Collision::sectors` / `tails`, iterators over the `0x58` and `0x38` arrays,
  exposing the converted qwords and (for a sector) the four runs.
- `Sector::counts`, `Sector::runs`, `Collision::run_bytes`,
  `Sector::vertices` (the last is a convenience over the observed 12-byte
  stride, and says so).
- `Collision::check_sector_tiling`, which requires every sector's four runs to
  be adjacent, to start at the sector's lowest pointer, and to end at the next
  sector's first pointer (array B's base for the last one).
- `fixup_in_place`, the read expression of `FUN_142e492a0`.

`crates/ow-tank/tests/chunkkind_real.rs` (new), 7 tests, all passing:

1. the two collision kinds are `0x11`/`0x10` and the two pathing kinds are
   `0x12`/`0x23`, by name and by slot;
2. the table carries the eight placeable names this project already knew, every
   slot is present once and every kind id is distinct;
3. the fixup's arithmetic: header RVAs keep a zero as zero, every sector and
   tail qword gets the base whether or not it was zero, and the tail's fourth
   qword is not converted;
4. a non-positive count runs no pass, and a negative count is refused by the
   parser;
5. truncated payloads, and counts that run past the payload, are refused;
6. **install:** `0x0DD0001100000688` opens at 21,858,688 bytes, parses with
   `count_a` 36 / `count_b` 1 / arrays at `0x14D3810` / `0x14D8948`, sector 0 and
   sector 4's RVAs and counts, sector 35's end, the tiling over all 36 sectors,
   sector 0's 32 vertices and the first vertex bit-exact, and array B's three
   RVAs;
7. **install:** the kind-`0x12` chunk opens at 8,792,256 bytes and is refused by
   the collision reader.

The container measurements above come from `dev/chunk11_probe.py` (a probe that
asks `ow-tank-look --hex` for the windows it needs and decodes them; it never
copies the 21.8 MB asset), and every one of them is pinned in the test.

Not a harness unit: the differential harness drives a function through
`crates/ow-tank/src/bin/verify_probe.rs` and a fixture generator under
`dev/verify_harness/`, and neither file is part of this change's ownership. The
fixup's exactness is therefore proven by the synthetic edge cases above plus the
real payload, not by an oracle call. **Not tested** against the shipped bytes of
`0x142e492a0`.

## 6. What is open

1. **The consumer of the collision sectors is not read.** This is the blocker
   this file was asked to settle, and it stays open. What was checked:
   - the dispatch only *fixes up* the payload; no case in
     `FinishMapChunkAsset` builds anything;
   - the payload handoff `FUN_141fe37a0` (`0x141fe37a0`) merely stores the
     pointer at `work + 0x60`, sets flags and bumps a per-type byte counter — it
     consumes nothing;
   - the shape-producer census in `formats/maps.md` (the 28 functions that call
     `ShapeAddToBody` / `OwnerCreateBody` / `ShapeAddToOwner` /
     `ShapeEntryFromDescriptor` / `MeshShapeFill` at 52 verified sites) contains
     no function that can see a chunk payload, and `FUN_142e492a0` is called
     only from `FinishMapChunkAsset` (its slots `0x1d` and `0x21`);
   - `FUN_142e4d920` (the other caller of `FUN_141fe37a0` in the same module) is
     a graph/serializer allocator over its own vtable objects, not a chunk
     reader;
   - an image-wide search for the two strides together (`0x58` and `0x38`
     immediates) leaves 10 functions outside the fixup itself, and none of the
     two examined reads a count at `+0x08`/`+0x0C` with a pointer at
     `+0x20`/`+0x28`. **Correction (parent, 2026-11-02):** `BuildBvh`
     (`0x142ccb1b0`) is *not* callerless — a verified `E8 rel32` scan finds five
     call sites in three functions (`0x142cca7d9`, `0x142ccb2db`,
     `0x142ccbc1c`), so the collider builder is reachable and its callers are
     candidates for the sector reader.

**Added 2026-11-02 (parent): the chunk is not the shape source.** A verified
store census of the Domino world's shape pools (`formats/shape-ingest.md`)
shows the pools' contents can only be created through `ShapeAddToBody` /
`ShapeAddToOwner`, and every caller of those that appends collision material is
a brush-mesh producer; `BrushTreeToMesh`'s four callers are exactly the four
brush appenders. So the collision chunk does **not** feed the world's shapes
through any decoded producer. The only remaining route is an inline appender
that grows `geometry+0x114`/`+0x124` without either helper — no decoded store
shows one, but a displacement scan cannot exclude it.

   The read that is missing, stated precisely: **whoever fills the domino
   fixture pool at `[record+0x2b8]`** — the same missing link the brush route
   left open. The collision chunk is now known to be its input (kind `0x11`), and
   it is no longer plausible that the cell container is: that is the pathing
   chunk.
2. **What the sector counts and flags mean.** `+0x2C`/`+0x30`/`+0x34` are
   element counts by the tiling proof; the floats at `+0x20..+0x28`, `+0x38`,
   `+0x3C`, `+0x40`, `+0x48` and the dwords at `+0x4C`/`+0x50` are unread. The
   header's `+0x00` and `+0x30`, array B's fields above `+0x10`, and the two
   dwords at `+0x10`/`+0x14` of every kind-table entry are unread.
3. **The additive collision payload was not opened.** Same fixup, no sample.
4. **The 8 bytes at `+0x38`..`+0x40`** are outside the header as `FUN_142e492a0`
   models it and before array B's first run; they are covered by the payload's
   exact tiling but no structure names them.

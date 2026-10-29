# Materials and textures

The path from a model to a textured surface:

```text
model          MDLC: an array of 64-bit material values
  material     type 008: shader group (085), shader source (088),
               unknown (03A), material data (0B3)
    data       type 0B3: STUMaterialData, holding texture references
      texture  type 004: a header and DXT/BC payload
```

## Current state

- **Chain: read and joined** on the install side. The join has an ambiguity
  decline.
- **Textures: decoded.** BC1, BC2, BC3, BC4_UNORM, BC5_UNORM and BC7 decode to
  RGBA8; BC6H decodes to half floats plus a clamped 8-bit view; BC6H,
  R16G16B16A16_FLOAT and R16_FLOAT keep their exact half words for the upload,
  while R8G8, R16_UNORM and RGBA8 keep the 8-bit view. R16G16_FLOAT (DXGI 34),
  the signed BC4/BC5 variants and the BGRA/depth/video formats the client's
  tables can name are reported as unsupported, not mis-decoded. The in-tree
  BC6H and BC7 tests are derived from the published specifications (Microsoft's
  BC6H/BC7 pages and the Khronos `EXT_texture_compression_bptc` extension);
  this document has no separate BC6H or BC7 section. `dev/tex_oracle.py` is a
  cross-check against an external `texture2ddecoder` wheel that is not in the
  repository, and it refuses BC6H by default. The `dev/bc6h_oracle.py` and
  `dev/bc6h_tables.py` files its docstring names are not present in this tree,
  so there is no reproducible `bcdec`/`bc7decomp` cross-check here.
- **Bundle-aware reads:** models, looks, materials, material data and textures
  open through `GameData::open_asset`, so bundle-hosted assets are included.
- **Practice-range result (re-measured after the record parser):** the
  `practice_launch` test prints 289 looks, 594 material entries, 120 textures
  decoded, 817 textured submesh draws, 55 placeholder draws, 148 flat fills, 3
  entries without an sRGB base colour, 0 unreadable and 0 ambiguous. These
  counts are **printed, not asserted**: the test asserts
  `draws.len() == 872` (the 817 + 55 split) and `textures.len() > 1`. Accepting
  the `0x0F1` aliases did not move these counts on this install; they match the
  pre-record-parse measurement. Printed with
  `cargo test --release -p ow-scene --test practice_launch -- --nocapture`.
- **Material-data textures:** read at the record/binding layer. The count is
  the byte at `+0x3C`, records are 16 bytes (GUID, zlib-CRC32 input hash, four
  flag bytes), and the client's consumer selects them by hash. The full registry
  census is 121,035 type-`0B3` assets: 11 unreadable, 1,817 empty, 119,207 with
  records and 693,111 records — 693,028 type `0x004` and **83 type `0x0F1`
  aliases in 73 payloads**. The generic material → shader join and the albedo
  name are open.
- **Runtime material structure:** open. The texture-record layer is read
  (count byte, 16-byte records, name-hash comparison); how a material instance
  is built from it is not.

## The model's MDLC chunk

The payload has `MaterialOffset` (i64 at +8, relative to the payload) and
`MaterialCount` (u16 at +104), then `count` 64-bit values at that offset. A
second offset/pointer at payload+0 has its count as the byte at +106; on most
models it is zero.

The u64 values recur across models that share a material: 6,598 distinct
across 52,626 values (~8x reuse). They are **not asset GUIDs**: decoding each
value with the GUID type/index split gives random type fields, and 0 of 52,626
values resolve as assets.

The model also ships a **u32 array**, separate from the u64 values. It is
present in a minority of models, holds small slot ids, and never equals the
low 32 bits of the paired u64 values. Its count is the byte at +106.

`mskl` exists in the files (lowercase tag). Its u32 arrays hold small ids, its
u16 array is index-like with `0xffff` sentinels, and its u64 array ships as all
`0xffffffffffffffff` — a placeholder table the runtime fills.

## The material asset (`0x008`)

A plain 32-byte header with four GUIDs, not an STU:

| offset | reference | type |
|---|---|---|
| `+0x00` | shader group | `0x085` |
| `+0x08` | shader source | `0x088` |
| `+0x10` | unknown | `0x03A` |
| `+0x18` | material data | `0x0B3` |

The GUIDs are not obfuscated. The model's own `MSTU` does not repeat the MDLC
values; its reference fields do not deobfuscate to material GUIDs under the STU
scheme with any tested field hash.

## The material data (`0x0B3`)

The `MATERIAL DATA` asset is job 28: the type registry at `0x143a7c3a0`
maps type `0x0B3` to 28 (`FUN_141fd8300` reads it), and `FUN_141b19160`
registers the job. Its load callback is `FUN_141b1a7b0` and its finalize
callback `FUN_141b1a790`. The material asset (`0x008`, job 2, loader
`FUN_141b1a8e0`) names the material data at `+0x18`.

The load callback rewrites seven base-relative `i64` fields at `+0x00` through
`+0x30` to `payload + value`; a stored `0` stays null. The file therefore
holds offsets and the resident image holds pointers:

| offset | field | notes |
|---|---|---|
| `+0x00` | `i64` parameter-array offset | `0x48` on every sampled asset |
| `+0x08` | `i64` base-relative field | non-zero in 208 of 4,000 sampled payloads; meaning open |
| `+0x10` | `i64` texture-array offset | read by the loader |
| `+0x18` | `i64` pointer-array D offset | `0x10`-byte records; first qword relative; non-zero in 263 of 4,000 |
| `+0x20` | `i64` pointer-array E offset | `0x10`-byte records; second qword relative; zero in the sample |
| `+0x28` | `i64` field | zero in the sample |
| `+0x30` | `i64` field | zero in the sample |
| `+0x38` | packed parameter counts | loader does not read it; `0x10011` observed. Parameters are **not** implemented. |
| `+0x3C` | packed counts | byte 0 texture count, byte 1 array-D count, byte 2 array-E count, byte 3 zero |
| `+0x40` | `i64` field | zero |

The loader reads the **bytes** at `+0x3D` and `+0x3E` for arrays D and E. The
texture count is the **byte** at `+0x3C`, not the whole `u32`: the texture-set
consumer `FUN_141f1ca80` reads `byte [payload+0x3C]` and walks
`payload + [payload+0x10] + i*16`. On `0x04D000000000358E` the packed word is
`0x106` (6 textures, 1 array-D record); reading it as a u32 would ask for 262
records and run past EOF. The array does **not** always end at EOF: trailing
parameter values and other data follow it, so the invariant is
`offset + 16*count <= length`, not equality.

A texture record is 16 bytes:

| offset | field | notes |
|---|---|---|
| `+0x00` | `u64` texture GUID | normally type `0x004`; 83 type-`0x0F1` alias records observed in 693,111; the consumer copies either without a type test |
| `+0x08` | `u32` input-name hash | zlib CRC32, below |
| `+0x0C` | four flag bytes | consumer does not read them. Install: `0` or `0x0000_8000` (the `0x80` is at `+0x0D`, so the old “flag byte at +12” wording named the wrong byte). |

### The name hash

The client's hash is the standard reflected **zlib CRC32**:
`FUN_141b52ca0(ptr, len, initial)` with polynomial `0xEDB88320` and the table
at `0x143283ce0`, `~initial` on entry and the final complement on return.
`FUN_141bb9810` is the lazy string wrapper that calls it with initial 0 over
exactly `len` bytes. There is no NUL terminator in the hash and the name is
case-sensitive: `CRC32("basecolor") = 0xAD10F602`, `CRC32("BaseColor") =
0xEE205BA5`. Evidence class: **read** for the function and the byte range;
**install** for the record values. The `dev/verify_harness` unit
`fn/Crc32NameHash` compares the Rust leaf against `FUN_141b52ca0` (364 cases,
100% branch coverage, 5/5 mutants).

### The binding

The texture records are selected **by input-name hash**, not by slot index.
`FUN_141f1ca80` (the texture-set consumer) reads the count byte at `+0x3C`,
walks the 16-byte records, and compares the low `u32` at `record+8` with a
fixed expected-hash table at `0x143359748` (eight `u32`s in that consumer).
On a match it copies the record's GUID and resolves it as an asset. The same
shape appears in `FUN_141f1db80` (record hash `0xD9987936`) and
`FUN_141f1e2e0` (record hash `0x0CDA8A9E`, fallback texture
`0x0C000000000001EE`). The expected list belongs to the consumer/material
class, not to the record.

The hash namespace appears to be shared with the shader's **reflection data**
(`0x040`, job 24, `SHADER REFLECTION DATA`): it is registered in
`FUN_141e05cb0` and its loader is `FUN_141e74360`. Its resource records are
`{u32 name_hash, u8 slot, u8 flags, u8 register, u8 space}`. `FUN_141dfea90`
scans them for a name hash, requires flag bit 1 at `+5`, and returns the slot
byte at `+4`. A real reflection asset (`0x0FC08002A9997C7F`) carries
`0xAD10F602` as a resource name. That a material record hash and a shader
resource name hash coincide is **install/inference**, not a read expression:
the reflection loader's shape is read, but the generic function that joins a
material's data to a particular shader's expected list is not, and neither is
which input name (if any) the client treats as albedo.

## The texture (`0x004`)

The header is the one the texture load callback at `0x141dacf00` reads; the
descriptor builder is `0x142e2d900`.

| offset | field | notes |
|---|---|---|
| `+0x00` | `u16 flags` | bit 2 marks a volume texture; copied into the object's flags at `+0x08` |
| `+0x02` | `u8 mip_count` | loader rejects `> 0x10` |
| `+0x03` | `u8 format` | the client's **engine enum**, not a DXGI id; loader rejects `> 0x71` |
| `+0x04` | `u8 surfaces` | array size, or depth for a volume texture |
| `+0x05` | `u8 usage` | |
| `+0x06` | `u8 payload_count` | loader rejects `>= 5` |
| `+0x07` | `u8 unknown` | value 1 adds `0x4000` to the object's flags |
| `+0x08` | `u16 width` | loader rejects 0 |
| `+0x0A` | `u16 height` | loader rejects 0 |
| `+0x0C` | `u32 total_size` | every mip, surface and depth slice; **not** a u16 |
| `+0x10` | `u8 base_mips` | loader rejects `> mip_count` |
| `+0x11` | `u8 chunk_mask` | only bit 3 is read (the streamed flag); low bits are open; see below |
| `+0x12` | `u8 reserved` | carried in the descriptor at bits 59..62 |
| `+0x13` | 13 bytes | unread by the loader |

Evidence class: **read** for the fields, the checks and the descriptor; **install**
for the values (the six pinned assets below).

The loader's acceptance order, read from the callback at `0x141dacf00` (which
reads the format byte at header `+3` first), is: `format <= 0x71`; `mip_count <= 0x10`;
`width != 0`; `height != 0`; `payload_count < 5`; `base_mips <= mip_count`. The
0x20-byte availability guarantee is **not** in this callback: it comes from the
job registration at `0x141d2ec30`. The loader does not reject on `surfaces`,
`total_size` or the reserved bytes. `TextureHeader::parse` performs the same six
field checks in the same order and adds its own Rust-side 0x20-byte bounds check
before reading any field.

The descriptor the builder assembles is a u64:

```text
bits  0..13  width - 1        bits 39..42  mip_count (as stored)
bits 14..27  height - 1       bits 43..47  usage
bits 28..38  surfaces - 1     bits 48..50  payload_count
bits 51..54  base_mips        bits 55..58  chunk_mask
bits 59..62  byte +0x12       bit  63      clear
```

The object's flags word at `+0x08` is `flags | 0x100000`, plus `0x4000` when
byte `+0x07` is 1. The builder also adds `total_size` to process-wide counters
and stores `total_size` at object `+0x40`; those are bookkeeping, not part of
the descriptor.

### total_size and the size walk

`total_size` is the asset's on-disk byte count, summed over every level and
surface. The level's resource dimensions are `max(width >> mip, 1)` and
`max(height >> mip, 1)` (`0x142e2db70` and `0x142e2db50`). The renderer's block
helper `0x142e36ec0` carries the block width (4, 6 or 8) and the bytes per block
(8 or 16) for the ids it knows; it is **not** a universal per-format footprint.
The ids `0x47`, `0x4a`, `0x4d`, `0x50`, `0x52`, `0x53`, `0x55`, `0x5f` and
`0x65` have no entry; its EAC families are 4x4 for `0x66..0x6a`, 6x6 for
`0x6b..0x6d` and 8x8 for `0x6e..0x70`. It also has no zero-dimension guard:
that guard belongs to the separate runtime walk `0x142e37070`.

For the canonical 4x4 block ids the level arithmetic is: round width and height
up to whole 4x4 blocks, multiply by the block bytes, then multiply by the depth.
`row_pitch` is the rounded width times the block bytes and `slice_pitch` is
`row_pitch` times the rounded height; the level size is `slice_pitch` times the
depth. The rounding still stores one whole block when the resource dimension is
2x2 or 1x1, so those levels remain one block.
[`crate::texture::level_dimensions`] is the dimension half and
[`crate::texture::level_byte_size`] the stored-size half. They are exact only for
those canonical 4x4 BC ids; for the 6x6/8x8 EAC families and the remaining
mobile ids they extrapolate rather than follow a read expression. The recorded
`total_size` equals the sum over all levels and surfaces on all 400 sampled
assets — the same sum the chunk's `image_size` uses — but every sampled format is
a 4x4 BC id, so that equality is not evidence for the extrapolated ones. The
client's `0x142e37260` likewise returns 8 or 16 for a subset of ids and 0 for
several accepted ids, so it is not a universal cooked-size formula either.

The client also has a *runtime* size walk at `0x142e37070`:

```text
total = 0
for each of mip_count levels:
    total += width * height * depth
    width  = max(width  >> 1, min)     min = 4 for block formats, else 1
    height = max(height >> 1, min)
    depth  = max(depth  >> 1, 1)
size = bpp * surfaces * total >> 3
```

The two agree whenever every halved dimension stays on a multiple of 4 — a
power-of-two block texture at least 4x4, and the six pinned assets. They differ
for a non-power-of-two block texture, and also for a block texture below 4x4
(2x2 or 1x1), where the runtime walk multiplies pixels and the recorded sum
carries the 4x4 block ceil: 5 of a 400-asset sample are non-power-of-two, and
their recorded `total_size` is 20 to 84 bytes larger than the walk. The recorded
sum follows the 4x4 block-ceil arithmetic above, using `0x142e36ec0`'s block
geometry for the ids it carries; `0x142e37070` is a separate memory-accounting
walk and is smaller for those 5 assets. Both are client expressions; on the
sampled 4x4 assets the field follows the first.

`bpp` comes from the format switch (`0x142e372b0`): 128/96/64/32/16/8/4/2/1/12
bits per pixel for the linear families, 4 or 8 for the block families. The
walk's own "is block" list is BC1..BC5, BC6H and BC7 only — the mobile ids
`0x65..0x71` are not in it, so the walk clamps them at 1, exactly as the client
does. A zero width, height, depth, mip count or surface count gives zero.

Both switches read their cases from tables in `.text`, not from globals: the
format switch's dword jump offsets are at `0x142e373d0..0x142e3741c` and its
byte case table at `0x142e37420..0x142e37491`; the size walk's dispatch block is
`0x142e37164..0x142e37189`. Those are the current-image addresses; the tables
are not at `0x143e...`.

For a volume texture (`flags & 4`), the header's `surfaces` byte is the depth and
the surface count is 1; both formulas are called with those values
(`0x142e2dbb0`).

### The payload chain

`payload_count == 0` means there is no chunk header: the block run starts at
`+0x20` and is `total_size` bytes long. The descriptor builder `0x142e2d900`
stores that pointer at object `+0x38` and `total_size` at `+0x40`, and the
no-payload branch of the renderer setup `0x141e29210` builds the whole
subresource list from exactly those two values, with first mip 0 and the last
mip `mip_count - 1`.

`payload_count >= 1` means a 16-byte chunk header sits at `+0x20` (the chunk
pointer is read: `0x142e2d900` stores `header+0x20` at object `+0x38`):

| offset | field | read at |
|---|---|---|
| `+0x00` | `u32 first_from_smallest` | `0x142e38740` |
| `+0x04` | `u32 level_count` | `0x142255e10` |
| `+0x08` | `u32 image_size` (block bytes in this chunk, all surfaces) | `0x142e38750` |
| `+0x0C` | `u32 header_size` (`0x10` on this install) | `0x142e38730` adds it to the chunk pointer |

Evidence: **read** for all four fields and their offsets; **install** for the
values (the six pinned assets and the bundle-aware 400-asset scan: every sampled
chunk has `header_size == 0x10` and an `image_size` equal to the per-level sum
of the levels it claims).

The data starts at `+0x20 + header_size`. `first_from_smallest` counts back from
the smallest mip, so the chunk's first level is
`mip_count - 1 - first_from_smallest`. The embedded chunk holds level 0 exactly
when that is 0. Otherwise the missing levels are in separate payload assets
(type `0x04D`), each with the same 16-byte chunk header at its own `+0x00` and
its data at `header_size`. Which levels a payload holds is its own header, not
its index; the install observation that the highest payload happens to carry
level 0 is recorded below and is not the selection rule. The payload GUID is
`0x142e37520`:

```text
parent & 0xF0FFFFFFFF
  | (index & 0xF) << 32
  | (base & 3) << 40        base = 0 for a texture parent
  | 0x0320000000000000      type 0x04D
```

`chunk_mask`'s low bits are **not read**. Only bit 3 is the streamed flag, and
the install relation `payload_count == popcount(chunk_mask)` (all 400 sampled
assets) is an observation, not a per-bit meaning: `0x0C000000000470E8` has mask
`0x0B` with payloads 1 and 2, and `0x0C000000000064D2` has mask `0x05` with
payload 1.

Requested payloads and residency are the loader's arithmetic, using the
**descriptor fields** (mip_count 4 bits, base_mips 4 bits, chunk_mask 4 bits):

```text
streamed = popcount((chunk_mask & 0xF) >> 3)   # only bit 3 survives this field
resident = if payload_count > 1 { streamed } else { 0 }   # object +0x0C
requests = if payload_count > 1
             and (mip_count & 0xF) - (base_mips & 0xF) > streamed
           then max(1, payload_count - streamed - 1)
           else 0
```

The 4-bit truncation is reachable: the loader accepts `mip_count == 16`
(accepted as `<= 0x10`), which the descriptor stores as 0. The requests are
payload indices `1..=requests`. The highest index (`payload_count - 1`) is the
streamed chunk when mask bit 3 is set, which is why some assets request it
through the streaming path rather than immediately. The build's alternative
residency branch — `mip_count - chunk.level_count`, reached only through
`DAT_14414ce69` at `0x141dacf00` — is not taken: that byte is 0 in this image.

Which levels a payload supplies is its **own chunk header**, not its index:
`first_from_smallest` and `level_count` at the payload's `+0x00`. The two
arrival paths differ. `0x141dabee0` reads the chunk accessors
(`0x142e38730`/`40`/`50` and `0x142255e10`) and copies the chunk's own
`level_count` levels. The rebase path `0x141dac720` calls no chunk accessor: it
copies from the new base to `mip_count`. The loader requests payload indices
`1..=requests` immediately and fetches the highest index lazily when
`chunk_mask` bit 3 marks it streamed. On the six pinned assets the highest index
happens to hold level 0 (`0x0C000000000009C1`: payload 2 is level 0 and payload
1 is level 1; `0x0C0000000003A505`: payload 1 is level 0 and is streamed). That
is an **install** observation, not the selection rule: the client does not
select a payload by index. The decoder reads every payload the header names
(`1..=payload_count-1`) and places each by its chunk header, so the whole chain
is available; a payload that does not open is reported rather than skipped.

The byte order inside one run is **read** from the client's subresource-record
builder `0x141e296a0`. Its outer loop is the array element and, for a cube, the
six faces; its inner loop is the mip levels from the run's first level upwards
(largest first). The data pointer advances by each level's own size
(`0x142e36ec0`), so a run is **surface-major, and within a surface mip 0
first**. The record index is the D3D12 subresource number
`(array * faces + face) * mip_count + mip` (`0x142e36e80`; `faces` is 6 for
renderer texture types 4/7 and 1 otherwise). The upload/copy path `0x141f0a000`
walks those records with its record-array bit 1 set (`0x141f0a53a`), giving
`mip + surface * mip_count`, where `surface` is the flat layer index
`array * faces + face` — the same surface-major order. The embedded tail is
uploaded when the texture is created (`0x141dac400` → `0x141e29210`); a payload
is uploaded when it arrives. The `0x141dabee0` path reads the chunk accessors
and copies the payload's own `level_count` levels; the rebase path `0x141dac720`
copies from the new base to `mip_count` without a chunk accessor.

The resource shape comes from the kind chain
`0x142e2dc70` (descriptor flags → kind 0..6) → `0x141e7ba50` (kind → renderer
type 1..7) → `0x143357d40[type]` (type → resource kind 0..8) → `0x1426545d0`
(resource kind → D3D12 dimension). The dimension switch is **kinds 0/1 =
`TEXTURE1D`** (D3D12 dimension 2), **kinds 2..7 = `TEXTURE2D`** (3), **kind 8 =
`TEXTURE3D`** (4). The cube kinds 6/7 set `DepthOrArraySize = array_count * 6`;
`MipLevels` comes from the renderer descriptor's mip field. So a cube is a 2D
array with six layers per cube, and a `flags & 1` 1-row image is a **1D**
resource: `0x0C0000000003B039` has flags `0x0001`, 1024×1, 1 mip and 1 surface,
so kind 0 → type 1 → resource kind 0 → `TEXTURE1D`. The viewer always creates a
`TextureDimension::D2` texture and a 2D view of layer 0, so it uploads that
asset as a 1-row 2D texture; the 1D resource/SRV dimension is **not read** and
is open. The cube-vs-2D-array view dimension and the cube-face direction order
are likewise open (see "Open").

When the embedded chunk does not start at level 0, the **resident base**
(`resident_mips`, the descriptor's `+0x0c`) is subtracted from the level index:
`0x141e7ba50` builds the initial texture with `mip_count - resident` levels and
the chunk's levels are copied at `first_level - resident`. A streamed payload's
arrival (`0x141dabee0`) reads that payload's chunk header and places its own
`level_count` levels; the rebase path `0x141dac720` then creates the resource at
the new base and copies the existing tail down, so the final resource is the
full chain. The decoder reads that final chain: the union of the embedded chunk
and every payload, each placed at its own absolute level.

### Format mapping

The stored byte is the engine enum. Two client tables resolve it:
`ENGINE_TO_PM[stored] = pm` from `0x143356f50` (0x72 8-byte records, byte 0),
and `PM_TO_DXGI[pm]` from `0x14348f500` (u32). The composition, for every byte
the loader accepts, is:

| stored byte(s) | DXGI | format |
|---|---|---|
| 0x02 | 2 | R32G32B32A32_FLOAT |
| 0x01 0x03 | 3 | R32G32B32A32_UINT |
| 0x04 | 4 | R32G32B32A32_SINT |
| 0x06 | 6 | R32G32B32_FLOAT |
| 0x05 0x07 | 7 | R32G32B32_UINT |
| 0x08 | 8 | R32G32B32_SINT |
| 0x0A | 10 | R16G16B16A16_FLOAT |
| 0x0B | 11 | R16G16B16A16_UNORM |
| 0x09 0x0C | 12 | R16G16B16A16_UINT |
| 0x0D | 13 | R16G16B16A16_SNORM |
| 0x0E | 14 | R16G16B16A16_SINT |
| 0x10 | 16 | R32G32_FLOAT |
| 0x0F 0x11 | 17 | R32G32_UINT |
| 0x12 | 18 | R32G32_SINT |
| 0x14 | 20 | D32_FLOAT_S8X24_UINT |
| 0x18 0x5A | 24 | R10G10B10A2_UNORM |
| 0x17 0x1A | 25 | R10G10B10A2_UINT |
| 0x1B | 26 | R11G11B10_FLOAT |
| 0x1D | 28 | R8G8B8A8_UNORM |
| 0x1E | 29 | R8G8B8A8_UNORM_SRGB |
| 0x1C 0x1F | 30 | R8G8B8A8_UINT |
| 0x20 | 31 | R8G8B8A8_SNORM |
| 0x21 | 32 | R8G8B8A8_SINT |
| 0x23 | 34 | R16G16_FLOAT |
| 0x24 | 35 | R16G16_UNORM |
| 0x22 0x25 | 36 | R16G16_UINT |
| 0x27 | 38 | R16G16_SINT |
| 0x29 | 40 | D32_FLOAT |
| 0x2A | 41 | R32_FLOAT |
| 0x28 0x2B | 42 | R32_UINT |
| 0x2C | 43 | R32_SINT |
| 0x2D 0x2E 0x2F | 45 | D24_UNORM_S8_UINT |
| 0x32 | 49 | R8G8_UNORM |
| 0x31 0x33 | 50 | R8G8_UINT |
| 0x34 | 51 | R8G8_SNORM |
| 0x35 | 52 | R8G8_SINT |
| 0x37 | 54 | R16_FLOAT |
| 0x38 | 55 | D16_UNORM |
| 0x39 | 56 | R16_UNORM |
| 0x36 0x3A | 57 | R16_UINT |
| 0x3B | 58 | R16_SNORM |
| 0x3C | 59 | R16_SINT |
| 0x3E | 61 | R8_UNORM |
| 0x3D 0x3F | 62 | R8_UINT |
| 0x40 | 63 | R8_SNORM |
| 0x41 | 64 | R8_SINT |
| 0x42 | 65 | A8_UNORM |
| 0x44 | 67 | R9G9B9E5_SHAREDEXP |
| 0x47 0x48 | 71 | BC1_UNORM |
| 0x49 | 72 | BC1_UNORM_SRGB |
| 0x4A 0x4B | 74 | BC2_UNORM |
| 0x4C | 75 | BC2_UNORM_SRGB |
| 0x4D 0x4E | 77 | BC3_UNORM |
| 0x4F | 78 | BC3_UNORM_SRGB |
| 0x50 0x51 | 80 | BC4_UNORM |
| 0x52 | 81 | BC4_SNORM |
| 0x53 0x54 | 83 | BC5_UNORM |
| 0x55 | 84 | BC5_SNORM |
| 0x56 | 85 | B5G6R5_UNORM |
| 0x58 0x59 0x5B 0x5D | 87 | B8G8R8A8_UNORM |
| 0x5C 0x5E | 91 | B8G8R8A8_UNORM_SRGB |
| 0x60 | 95 | BC6H_UF16 |
| 0x5F 0x61 | 96 | BC6H_SF16 |
| 0x62 0x63 | 98 | BC7_UNORM |
| 0x64 | 99 | BC7_UNORM_SRGB |
| 0x71 | 103 | NV12 |

Every other byte the loader accepts — 0x00, 0x13, 0x15, 0x16, 0x19, 0x26,
0x30, 0x43, 0x45, 0x46, 0x57, 0x65..0x70 — maps to no DXGI format (`0`). The
`0x65..0x70` group still has a renderer family and block size (0x65..0x67 are
family 1 at 4 bpp; 0x68..0x70 are families 8/9/10 at 8/4/2 bpp, and
`0x142e371c0` gives their block dimensions as 4x4, 6x6 and 8x8), but the DXGI
table has no entry for them: they are the mobile block-compression families with
no D3D equivalent, so a decoder that cannot render them must say unsupported.

Stored `0x33` is **R8G8_UINT** (DXGI 50), not R16_UNORM: the old repo mapping
was wrong, and the census's 19 assets of that byte are two 8-bit channels, not
one 16-bit channel.

### Decode correction (2026-10-29)

BC2 and BC3 now decode the colour half with the four-colour block even when
`c0 <= c1`; BC1 keeps its three-colour branch (index 2 is the integer average,
index 3 transparent black). Evidence: the client uploads BC2/BC3 as DXGI
74/75/77/78 through the engine→PM table `0x143356f50` and the PM→DXGI table
`0x14348f500`, and the D3D BC2/BC3 rule is a four-colour colour block plus a
separate alpha half. The four-colour rule is a format-specification check, not a
new client value.

Verified against the current image on 2026-10-29: stored `0x4A`/`0x4B` → PM
`0x3B` → DXGI 74, `0x4C` → PM `0x3C` → 75, `0x4D`/`0x4E` → PM `0x3D` → 77 and
`0x4F` → PM `0x3E` → 78. The resource builder `0x1426545d0` writes that id into
the resource `Format`, the SRV helper `0x142653b90` uses it for the colour view,
and the copy path `0x141f0a000` moves 16-byte block records without CPU pixel
expansion. The image contains no client palette function for BC1/BC2/BC3: the
palette arithmetic is the GPU's D3D rule. The format identity and the absence of
a CPU decode are **read**; the palette arithmetic is **specification**.

sRGB is the DXGI id, with no separate client predicate:
`R8G8B8A8_UNORM_SRGB` (29), `BC1/BC2/BC3_UNORM_SRGB` (72/75/78),
`B8G8R8A8_UNORM_SRGB` (91) and `BC7_UNORM_SRGB` (99). The stored bytes the
install's material lists use are the five non-BGRA ones — 30, 73, 76, 79 and 100
— so that is the material-side observation; the predicate also accepts the BGRA
bytes 0x5C/0x5E, which no sampled material texture uses. The material side
derives its colour-format test through this mapping instead of keeping a second
list.

### Upload colour space

The stored byte's DXGI id decides the colour space, and nothing else does. An id
in the sRGB set above (`R8G8B8A8_UNORM_SRGB` 29, `BC1_UNORM_SRGB` 72,
`BC2_UNORM_SRGB` 75, `BC3_UNORM_SRGB` 78, `B8G8R8A8_UNORM_SRGB` 91,
`BC7_UNORM_SRGB` 99) is a colour map; every other id is sampled raw.
`ow_tank::texture::is_srgb_format` is that predicate and `is_srgb_dxgi` is the
id set.

The client uses the same table on both sides of the upload:

- the D3D12 resource-descriptor builder at `0x1426545d0` writes
  `0x14348f500[pm]` into the descriptor's `Format` field. It switches to the
  typeless companion table at `0x14348f730` when the texture object's `+0x3c`
  byte is set, or for a depth format whose `+4` bit 0 is set; that makes the
  **resource** typeless, not a different colour space;
- the texture SRV builder at `0x14264be60` writes the format returned by
  `0x142653b90` into the view descriptor (the `Format` store is at
  `0x14264c09b`, past the auto-detected `FUN_14264be60` body end at
  `0x14264c012`, so it is in the unanalysed tail rather than absent). For every
  colour format that is again `0x14348f500[pm]`; the only overrides are the
  depth/video plane ids (renderer formats 131..136 → plane views), never an sRGB
  retag.

So the asset's own DXGI id is the rule: the client neither promotes a linear
texture to sRGB nor drops an sRGB texture to linear at the view. The viewer
carries the predicate through `SceneTexture::srgb` and, on the 8-bit path,
uploads `Rgba8UnormSrgb` when it is true and `Rgba8Unorm` when it is false
(`ow-client/src/surface.rs::upload_format`). The formats with exact half values
take the `Rgba16Float` substitution described below. The 1×1 white slot and the
UV checker are viewer-generated and stay sRGB so the grey checker does not
shift.

Evidence: both tables and both builders are read from the current image. The
viewer's upload is **not** the client's D3D12 call — it writes RGBA8, or
`Rgba16Float` for the exact-half formats, through wgpu with no client sampler —
but it now uploads every decoded subresource
(mip levels and array layers) in the client's copy order. The mip chain, the
array layout and the SRV format are read (`0x141e296a0`, `0x141f0a000`,
`0x1426545d0`, and the format at `0x142653b90`); only the plane selection, the
view dimension, the cube-face direction order and the sampler remain open, so
the viewer binds a 2D view of layer 0 rather than a cube view.

### 16-bit and HDR upload (2026-10-29)

The client's resource keeps the DXGI format; it does not expand 16-bit or HDR
values to bytes on the CPU. Re-read from the current image:

| stored byte | `0x143356f50[stored]` (PM) | `0x14348f500[PM]` (DXGI) | format |
|---|---|---|---|
| 0x0A | 0x32 | 10 | R16G16B16A16_FLOAT |
| 0x37 | 0x0B | 54 | R16_FLOAT |
| 0x39 | 0x07 | 56 | R16_UNORM |
| 0x60 | 0x44 | 95 | BC6H_UF16 |
| 0x61 | 0x43 | 96 | BC6H_SF16 |

The resource builder `0x1426545d0` writes `0x14348f500[pm]` into the resource
descriptor's Format field (the `param_1[8]` store), and the copy path
`0x141f0a000` copies the stored block records without CPU pixel expansion. The
identities are **read**; they were checked against the image again on
2026-10-29.

Decoding is the viewer's, and so is the upload resource:

- BC6H (DXGI 95/96), R16G16B16A16_FLOAT (10) and R16_FLOAT (54) now decode to
  exact half words, four per pixel in `r, g, b, a` order
  (`Texture::rgba16`, `SceneTextureLevel::rgba16`). R16_FLOAT is padded
  `(r, 0, 0, 1.0)` because that is what a D3D one-channel float view supplies
  for the missing channels. No value is rounded or clamped on this path; a
  test compares the stored bytes and a value outside [0, 1] survives bit for
  bit.
- `ow-client/src/surface.rs` uploads those levels as
  `wgpu::TextureFormat::Rgba16Float`, eight bytes per pixel with a 256-byte
  row pitch. **`Rgba16Float` is a viewer substitution** for the client's DXGI
  10/54/95/96, not a claim that the client creates that resource: it has the
  same four 16-bit float channels, and the client's own resource is the DXGI
  format above. `Rgba16Float` is a filterable float sample type in WebGPU
  without an optional feature, so the existing
  `TextureSampleType::Float { filterable: true }` bind layout accepts it (a
  unit test asks `TextureFormat::sample_type`).
- R16_UNORM (56) is **still clamped**: the decoder rounds it to the 8-bit view
  and does not keep its raw `u16`, so no `R16Unorm` upload path exists. UINT
  formats likewise use the UNORM path (see "Open"). BC1–BC5 and BC7 are
  decoded on the CPU to RGBA8 for the viewer; the client keeps them compressed
  and the GPU decodes. Those, and the sampler, remain open below.

### The six pinned assets

| GUID | format (stored → DXGI) | mips × surfaces | total_size | descriptor | chunk |
|---|---|---|---|---|---|
| `0x0C000000000009C1` | 0x49 → 72 BC1_SRGB | 11 × 1 | 699064 | `0x03CB158000FFC3FF` | `[8, 9, 43704, 16]` |
| `0x0C0000000006A2C6` | 0x60 → 95 BC6H_UF16 | 8 × 6 | 131232 | `0x00C18400501FC07F` | `[7, 8, 131232, 16]` |
| `0x0C0000000003A505` | 0x64 → 99 BC7_SRGB | 10 × 1 | 349552 | `0x04CA2500007FC1FF` | `[8, 9, 87408, 16]` |
| `0x0C000000000064D2` | 0x51 → 80 BC4_UNORM | 8 × 1 | 174760 | `0x02BA2400007FC1FF` | `[6, 7, 43688, 16]` |
| `0x0C000000000470E8` | 0x51 → 80 BC4_UNORM | 9 × 1 | 699048 | `0x05BB248000FFC3FF` | `[6, 7, 43688, 16]` |
| `0x0C000000000470D8` | 0x51 → 80 BC4_UNORM | 8 × 1 | 174760 | `0x04BA2400007FC1FF` | `[6, 7, 43688, 16]` |

The cubemap embeds all eight levels. The other five keep level 0 in a separate
payload, and the payload's own chunk header names the level: on `9C1` payload 2
is level 0 and payload 1 is level 1; on `3A505` payload 1 is level 0 and is the
streamed (bit 3) chunk; on `64D2` payload 1 is level 0; on `470E8` payload 2 is
level 0 and payload 1 is level 1; on `470D8` payload 1 is level 0. Requests are
`[1, 2]`, none, none, `[1]`, `[1]` and none respectively. `ow-tank::texture`
pins the header bytes, descriptors, chunk fields, size walks, payload GUIDs and
the synthetic surface-major/mip-ascending layout in its unit tests;
`tests/texture_real.rs` re-reads the same six headers from the install and scans
400 live assets for the invariants. `tests/texture_levels_real.rs` pins the
cubemap and the two multi-mip assets at offsets derived from their headers, plus
one raw `payload_count == 0` six-surface asset (`0x0C000000000117C3`) at
independently computed surface/mip offsets. Its 400-asset scan then checks that
every decoded level is present, in D3D12 subresource order, at its own
dimensions and byte size, and contiguous in the source run. The scan compares
each level to the source at the offset the decoder computed, so it proves
completeness, order, sizes and source contiguity; the offset formulas themselves
are pinned by the four assets and the synthetic layout test, not independently
by all 400.

### Differential units

The oracle is the recovery image
`dev/data/offline_recover/module_offline.bin` (75,726,848 bytes). Before these
units were added, its bytes at `0x142e2d900`, `0x142e36ec0` and `0x143356f50`
were compared with the Ghidra image and agree at all three addresses. The
texture parser leaves now proved operation for operation are:

| unit | client function | cases | coverage | mutants |
|---|---|---|---|---|
| `fn/TextureDescriptor` | `0x142e2d900` descriptor builder | 364 | 100% (1/1) | 12/12 |
| `fn/TextureDescriptorSizeWalk` | `0x142e2dbb0` wrapper; the callee `0x142e37070` is not separately measured | 364 | 100% (1/1) | 11/11, one declared equivalent |
| `fn/TextureFormatFamilyBpp` | `0x142e372b0` format switch | 374 | 100% (1/1) | 12/12 |

`fn/TexturePayloadGuid`, `fn/TextureRequestCount` and
`fn/TextureCountBitsShifted` are also verified (12/12, 1/1 and 2/2 mutants).
The ledger is local and a row is current only when its recorded candidate hash
matches the tree; the texture rows predate the current uncommitted edits, so
re-run them before treating them as fresh.
`fn/TextureCountBitsShifted` has no branches, so its reported 100% branch
coverage is vacuous. The descriptor unit compares only the 16-byte result prefix
(descriptor qword and flags dword); the builder's sentinels, pointers and
`total_size` bookkeeping past `+0x10` are not part of its result. The size unit
measures the wrapper `0x142e2dbb0`, not the callee walk `0x142e37070`; it keeps
`bias <= mip_count` because the client's 32-bit subtraction otherwise makes the
walk run almost 2^32 times. The format unit sweeps `0..=0xff`; that is wider
than the loader's `0..=0x71` domain, and the bytes above 0x71 are outside the
loader rather than part of its proof. The loader `0x141dacf00`, the chunk
accessors (`0x142e38730`/`40`/`50`, `0x142255e10`), the record builder
`0x141e296a0` and the per-level size helper `0x142e36ec0` are read and
install-checked, but no oracle unit drives them.

## The id join

A model placement carries a second reference. A scan over 16,357 model
placeables in 41 map chunks finds exactly two resolving qword offsets:

| offset in placeable | value | type |
|---|---|---|
| `+0x00` | the model | `0x00C` |
| `+0x20` | the model's **look** | `0x01A` |

Every one of 14,093 placements in a 40-chunk census names a look, and all 8,331
distinct looks resolve. A look is a `STUModelLook` (`0x5207484B`); its first
field (`0xBAFDAFBA`) is an inline array of `STUModelMaterial` (`0x494B66C1`):

| element field | meaning |
|---|---|
| `0x33E51FDC` | `m_material` — the material asset |
| `0xDC05EA3B` | `MaterialID` — the id the model names it by |

The join is the model's value against the look's id. The practice-range scene
resolves 289 looks and 594 material entries through `GameData::open_asset`;
the earlier store-only samples undercounted the family and are superseded.

`ow_tank::material::base_texture_of_id` implements the join. An id naming two
materials is declined; the scene leaves those submeshes on the placeholder and
names the id.

## Client-side read

`MdlcMaterialRead` (`0x141b88cb0`) copies the MDLC values verbatim into the
runtime array at `+0xb8`; the u32 array is copied separately to `+0x30`.
`MsklTableBuild` (`0x141b894d0`) repacks the payload-relative offsets with no
transform.

The current-image functions work with 32-bit keys, not the 64-bit values:

- `MaterialAssetLookup` (`0x141b7b9d0`) answers a key from a packed-u64 key
  array and returns a 0x38-byte entry; a miss is a no-return assert.
- `MaterialKeyBind` (`0x141b839b0`) resolves keys and clamps by the u64-value
  count.
- `MaterialSlotBind` (`0x141b8a1d0`) sets the slot count and binds each
  material through the record's u16 at +4.
- `MaterialParamApply` (`0x141b8ca70`) resolves the material descriptor, scans
  the model's key/value records, and stores the min/max times.

How the runtime arrays are built at load time is open. The shipped u64 values
are the loader's input; the runtime record shape is not read.

## Where the store-only read stopped

Earlier samples used `Store::open_ckey` only. That path skips bundle-hosted
assets, so it made the look/material/material-data family look far smaller than
it is. `GameData::open_asset` now follows the client's CMF record flag and reads
the encoded store or the bundle entry. The practice scene is the current check:
289 looks, 594 material entries and 120 decoded textures resolve. The 55
placeholders are named per submesh (ambiguous material id, no sRGB base colour
or a flat fill), not assumed to be missing install content.

## The hero route

A hero's textures are named directly on the hero asset, so a textured model
does not depend on the material chain above. The gameplay entity's
`STUModelComponent` (`0x191835F9`) carries `0x9297ADB4`, a `STUModelLook`
(`0x01A`); hero 3 resolves to look `0x098000000000028D`, which names 8
materials for 9 submeshes. `ow_tank::entity::hero_parts` reads it.

The viewer's base-colour rule is "the material's first **sRGB** texture" plus
"it must have structure in at least two channels". Several readable textures
are normal/mask or flat fills and are rejected; the practice-range launch names
each remaining placeholder per submesh. This is not the client's rule: the
client selects a texture by the input-name hash in its record, and which name
is albedo for a given material class is not read.

## Open

- The client's runtime material pointer structure and material application are
  not read; the viewer is not the client's renderer.
- The generic material-data → shader join is not read. The records and their
  input-name hashes are read (`FUN_141f1ca80` and its siblings), and the
  shader reflection data uses the same hash namespace, but the function that
  chooses a material's expected input list is not. `FUN_141f1ca80`'s expected
  list is eight ordered `u32` hashes at `0x143359748`; the viewer does not walk
  that order, so the base-colour choice is still its first-sRGB heuristic (see
  above), not 1:1.
- The full census leaves 11 registered type-`0B3` assets that do not open
  through `GameData::open_asset`; their records are not in the 693,111 total.
- The record flag word at `+0x0C` is not read by the texture-set consumer.
  Install shows `0` or `0x0000_8000`; bit 15's meaning is open.
- The parameter array (`+0x00` offset, packed counts at `+0x38`) is not
  implemented; only its offset and the packed count word are noted.
- The base-colour rule still leaves 55 placeholder draws, 148 flat fills and 3
  no-sRGB entries on the practice range. Each needs a per-submesh reason.
- The model's `MSTU` field hashes are the install-local route to texture
  references inside the model, skipping the material wrapper.
- The texture sampler path is not read, and the viewer's upload is not the
  client's D3D12 call: it writes RGBA8 through wgpu, and `Rgba16Float` for the
  exact-half formats (the substitution documented above). The uploaded
  subresources/levels now follow the client's copy order (surface-major, mip
  ascending) and the resource shape (`mip_count` levels, `surfaces` layers),
  but the client samples a DXGI-format resource through its own sampler, so
  rendering is not 1:1.
- The SRV view dimension (`TEXTURECUBE` vs `TEXTURE2D_ARRAY`) and the cube-face
  direction order are not read. The resource is a 2D array of 6·N layers
  (`0x1426545d0`); the viewer binds a 2D view of layer 0 with the full mip
  chain.
- The `TEXTURE1D` resource and SRV dimension are not implemented. The kind chain
  ends in D3D12 dimension 2 for resource kinds 0/1 (`0x1426545d0`), and
  `0x0C0000000003B039` (flags `0x0001`, 1024×1, one mip, one surface) is a 1D
  texture in the client. The viewer always creates a `TextureDimension::D2`
  texture and binds a 2D view, so it uploads that asset as a 1-row 2D image.
  Open.
- The alternate copy path `0x141f11f90` is not read: it is selected by the
  record-array flag bit 0 (`0x141f0a4e6`), with object flags bit 11 (`0x800`)
  on the same object. Its body is an unresolved `__chkstk` stub in this image,
  and no sampled record array sets bit 0.
- A header `mip_count == 0` is accepted by the loader (it rejects only
  `> 0x10`), but the decoder returns `NoData`; the record builder would produce
  zero levels and a zero-mip D3D12 resource. No sampled asset has it.
- The volume depth axis is not implemented: for `flags & 4` the client creates a
  3D resource and one subresource per mip over the depth slices; the decoder
  returns `UnsupportedLayout("volume")`, and the 3D SRV view dimension is not
  read. No volume asset appears in the 400-asset sample.
- BGRA (`B8G8R8A8_UNORM`/`_SRGB`) and the signed BC4/BC5 variants are mapped by
  the client's tables but not decoded; the EAC/mobile families `0x65..0x70`
  likewise, including their 6x6/8x8 block sizes. They are reported unsupported,
  and `cooked_size`/`level_byte_size` have no exact arithmetic for them: they
  fall back to a 4x4 block or a linear size.
- R16_UNORM (DXGI 56) is still clamped: the decoder rounds the raw 16-bit
  value to the 8-bit view and does not keep it, so there is no `R16Unorm`
  upload path. `R16Unorm` is a filterable float sample type, so adding it would
  not need a second bind layout, but the exact-value decode is not implemented
  and is open.
- BC1–BC5 and BC7 are decoded on the CPU to RGBA8 for the viewer, with the BC
  palette and block arithmetic coming from the published formats (the client
  leaves the blocks to the GPU). That CPU decode, the `Rgba16Float`
  substitution for DXGI 10/54/95/96 and the sampler path are viewer choices,
  not the client's D3D12 calls; the 8-bit formats and UINT values are therefore
  not 1:1 at the upload.
- `chunk_mask`'s low bits remain unread (only bit 3 is the streamed flag); the
  install relation `payload_count == popcount(chunk_mask)` is an observation.
- A header `mip_count == 16` is accepted by the loader, but the descriptor's
  4-bit field stores 0 and the renderer would see no levels. The descriptor's
  width field is 14 bits while the header field is a `u16`, so a width above
  16384 truncates as well. No sampled asset has either; the decoder uses the
  header bytes. Open.

### Remaining texture 1:1 gaps (audit 2026-10-29)

These are the texture-path findings that the differential units do not cover;
an item already stated above is strengthened there rather than repeated here.

- **`image_size` used as a stride:** the decoder treats the chunk's
  `image_size` as an exact per-surface run (`image_size / surfaces`). The
  client's record builder `0x141e296a0` advances by each level's own size, so a
  payload padded between surfaces would diverge. No sampled asset is padded,
  and the padded case is unread.
- **Eager all-payload decode:** `parse_asset` opens and decodes every payload
  the header names. The client keeps the immediate request set resident and
  fetches the streamed highest index lazily; the arrival path `0x141dabee0`
  places a payload's chunk levels, and `0x141dac720` rebases the resource when
  the base moves. Residency and lazy arrival are not modelled.
- **UINT formats as UNORM:** the `_UINT` ids that reach a decoder
  (`R8G8B8A8_UINT` 30 and `R8G8_UINT` 50) use the same UNORM path and upload as
  `Rgba8Unorm`; their integer values are not preserved. The other `_UINT` ids
  are reported unsupported.
- **Material fallback:** the viewer does not use the fallback texture
  `0x0C000000000001EE` that `0x141f1e2e0` substitutes when its input hash
  misses.
- **`0x0F1` aliases:** the record consumer copies a type-`0x0F1` GUID like a
  `0x004` one, but the alias asset's own layout and indirection are not read.
- **No oracle units:** the loader `0x141dacf00`, the chunk accessors
  (`0x142e38730`/`40`/`50`, `0x142255e10`), the record builder `0x141e296a0`
  and the per-level size helper `0x142e36ec0` are read and install-checked but
  not differentially driven.

## Verification

```text
cargo run -p ow-tank --bin look -- --join 400
cargo run -p ow-tank --bin tex -- --census 400
cargo test -p ow-tank material --lib
cargo test --release -p ow-tank --test material_real
cargo test --release -p ow-tank --test material_real -- --ignored --nocapture
cargo test --release -p ow-scene --test practice_launch -- --nocapture
cargo test --release -p ow-tank --test texture_real
cargo test --release -p ow-tank --test texture_levels_real -- --nocapture
cargo test --release -p ow-tank --test bc6h
python dev/verify_harness/cli.py fn/Crc32NameHash
python dev/verify_harness/cli.py fn/TexturePayloadGuid
python dev/verify_harness/cli.py fn/TextureRequestCount
python dev/verify_harness/cli.py fn/TextureCountBitsShifted
python dev/verify_harness/cli.py fn/TextureDescriptor
python dev/verify_harness/cli.py fn/TextureDescriptorSizeWalk
python dev/verify_harness/cli.py fn/TextureFormatFamilyBpp
```

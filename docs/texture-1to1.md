# Texture parsing: what is not 1 to 1

The texture-specific companion to [`ow-tank-1to1.md`](ow-tank-1to1.md) and
[`formats/materials.md`](formats/materials.md). It records what the current
tree proves, what it only reads, and what is still a viewer substitute.

**Ground truth is the complete current `Overwatch.exe` image** (base
`0x140000000`). A process capture is a different build or run and is not
evidence. Evidence classes are the ones in [`provenance.md`](provenance.md):
**read** means the client expression was read; **install** means the value was
observed on the operator's installation; **inference** is a conclusion that is
not a client expression.

**"1 to 1" has one meaning here:** the client code or value was read, the Rust
implementation follows it operation for operation, and a test proves the result
identical. A read-and-implemented function that is only install-checked is not
closed. A viewer-side decode is not the client's renderer, even when its values
are exact.

As of 2026-10-30.

## Status at a glance

- **Container parsing is read and close to exact.** The 32-byte header, the six
  loader checks and their order, the descriptor bitfield, the engine→PM→DXGI
  tables, the chunk header, the payload GUID, the request/residency arithmetic
  and the subresource order match the current image.
- **The remaining parsing gaps are edges the install does not exercise:**
  `image_size` used as an exact per-surface stride, 6x6/8x8 EAC block sizes,
  descriptor field truncation above 16384 or at `mip_count == 16`, eager
  all-payload decoding, and a few API-domain details.
- **Decode and upload are viewer-side.** The client uploads DXGI resources and
  lets the GPU decode. The tree's CPU decode, its `Rgba16Float` substitution and
  its sampler are presentation choices, not the client's D3D12 calls.
- **Material selection is still the largest visible gap.** The client selects a
  texture by an ordered expected-hash table; the viewer still chooses the first
  sRGB record. The eight hashes are read; their names are not.
- **Two corrections landed on 2026-10-29/30:** BC2/BC3 now use the four-colour
  colour block, and BC6H/RGBA16F/R16F keep exact half words through the upload.

## A. Read and exact

These rows are read from the current image and reproduced. "Proof" says what is
actually tested; a read expression without a differential unit is not called
verified.

| area | client address | what matches | proof |
|---|---|---|---|
| Header fields and checks | `0x141dacf00`; 0x20-byte guarantee `0x141d2ec30` | field offsets/widths/endianness; six checks in order: format <= `0x71`, mip_count <= `0x10`, width != 0, height != 0, payload_count < 5, base_mips <= mip_count | read; install-checked; no oracle unit |
| Descriptor builder | `0x142e2d900` | bits 0..13 width-1, 14..27 height-1, 28..38 surfaces-1, 39..42 mip_count, 43..47 usage, 48..50 payload_count, 51..54 base_mips, 55..58 chunk_mask, 59..62 reserved, bit 63 clear; `surfaces == 0` wraps to 2048 | `fn/TextureDescriptor`: 364 cases, 100% branch coverage, 12/12 mutants; 16-byte result prefix only |
| Object flags | `0x141dacf00` / `0x142e2d900` | `flags \| 0x100000`, plus `0x4000` when byte `+0x07` is 1 | read; descriptor unit pins `+0x07 == 0`; the `0x4000` path is not in the unit |
| Engine format table | `0x143356f50` | all 114 stored bytes `0x00..0x71` map to the recorded PM value (8-byte records, byte 0) | read; the table is transcribed, not unit-driven |
| PM→DXGI table | `0x14348f500`, typeless `0x14348f730` | reachable PM entries and the sRGB ids 29/72/75/78/91/99 | read; `fn/TextureFormatFamilyBpp` covers family/bpp only |
| Format family/bpp switch | `0x142e372b0` | linear and block families and the stored bpp | `fn/TextureFormatFamilyBpp`: 374 cases, 100% (1/1), 12/12; domain `0..=0xff`, wider than the loader's `0..=0x71` |
| Chunk header | `0x142e38730` (data pointer), `0x142e38740` (first), `0x142e38750` (image size), `0x142255e10` (count) | 16-byte header at `+0x20`; data at `chunk + header_size`; first level `mip_count - 1 - first_from_smallest` | read; install-checked; no oracle unit |
| Payload GUID | `0x142e37520` | `parent & 0xF0FFFFFFFF \| (index & 0xF) << 32 \| (base & 3) << 40 \| 0x0320000000000000` | `fn/TexturePayloadGuid`: 12/12 |
| Request/residency | `0x142e37600`, `0x142e374d0`, gate `0x142e2dd70` | streamed = popcount((chunk_mask & 0xF) >> 3); resident = streamed when payload_count > 1; requests = max(1, payload_count - streamed - 1) only when the mip gap exceeds streamed | request leaf 1/1; popcount 2/2 but zero branches; the `payload_count > 1` gate is outside the leaf |
| Subresource order | `0x141e296a0`, index `0x142e36e80`, copy `0x141f0a000` | outer array/face, inner mip ascending; data pointer advances by each level's own size; index `(array*faces + face)*mip_count + mip`; copy index `mip + surface*mip_count` | read; pinned by synthetic and four install assets; no oracle unit |
| Resource shape | `0x142e2dc70` → `0x141e7ba50` → `0x143357d40` → `0x1426545d0` | kinds 0/1 = `TEXTURE1D`, 2..7 = `TEXTURE2D`, 8 = `TEXTURE3D`; cube = 2D array with `array*6` layers | read; viewer does not bind those dimensions yet |
| Material texture records | `0x141f1ca80` | count byte at `+0x3C`, 16-byte records, GUID at +0, input hash at +8, flags at +0x0C; consumer copies the GUID without a type test | record layer install-checked; CRC leaf `fn/Crc32NameHash` 5/5; consumer has no unit |
| Material name hash | `0x141b52ca0` | reflected zlib CRC32, polynomial `0xEDB88320`, no NUL, case-sensitive; `crc32("basecolor") = 0xAD10F602` | `fn/Crc32NameHash`: 5/5 |
| `total_size` on sampled assets | `0x142e36ec0` | 4x4 block ceil and linear arithmetic match the recorded field on all 400 sampled assets | install-checked; no oracle unit; the sampled formats are all 4x4 BC ids |

## B. Parsing edges that are not 1 to 1

| finding | Rust / current tree | client behaviour | why it is open | next anchor |
|---|---|---|---|---|
| `image_size` as an exact stride | `texture.rs::build_texture` treats `image_size / surfaces` as the per-surface stride and requires `prefix * surfaces == image_size` | `0x141e296a0` advances by each level's own `0x142e36ec0` size; the `image_size` argument is not used for placement. The setup paths read `0x142e38750`, so a padded chunk is not fully read | a padded or rounded chunk is rejected here where the record builder would copy it; no sampled asset is padded | derive the stride from the summed level sizes; keep `image_size` as a bound; test sum±1 |
| EAC/mobile block sizes | `cooked_size`/`level_byte_size` assume 4x4 for every `is_block_format` id and use `bpp*2` bytes per block | `0x142e371c0` is 4x4 for canonical ids and `0x66..0x6a`, 6x6 for `0x6b..0x6d`, 8x8 for `0x6e..0x70`; `0x142e37260` returns 0 for `0x47,0x4a,0x4d,0x50,0x52,0x53,0x55,0x5f,0x65` | the sizes are exact only for canonical 4x4 ids; the EAC families are not decoded, so the arithmetic is an extrapolation | implement `block_dims`/`block_bytes` from the client helpers; return unsupported where both are 0 |
| Descriptor field truncation | level layout uses the header's u16 width/height and u8 mip count | `0x142e2d900` stores width/height as 14 bits and mip count as 4 bits; the record builder uses those fields | width/height above 16384 and `mip_count == 16` diverge; no sampled asset; D3D12's own limit makes the width case theoretical | emulate `0x141e7ba50` + `0x141e296a0` with a 20000x20000 header and `mip_count == 16` |
| Eager all-payload decode | `parse_asset` opens and decodes every payload the header names; a missing level returns `PayloadNotLoaded` | the loader requests `1..=requests` immediately and the streamed highest lazily; a missing payload can still leave the resident tail drawable | final bytes match when all payloads exist; partial availability is not modelled | read the request/arrival bookkeeping; model the resident tail |
| Header shorter than 0x20 | `TextureHeader::parse` rejects it | the callback reads `+3` first; the 0x20-byte guarantee is the job registration, not the callback | truncated/malformed assets only; a Rust-side safety check, not a client check | document as policy; add a 0x1f/0x20 header vector |
| Request leaf parameter width | `TextureHeader::request_count` passes the header's 8-bit payload count | `0x141dacf00` passes the descriptor's 3-bit `(desc >> 48) & 7` | equivalent for every accepted header (payload_count < 5); hand-built headers above 7 diverge | pass `& 7` or state the accepted domain |
| `surfaces == 0` | `depth_and_surfaces` yields 2048 and the decoder builds 2048 layers, but `Texture.surfaces` stays 0 | the descriptor's `(surfaces-1) & 0x7ff` also yields 2048 | no byte error; a model/consumer inconsistency on an asset class the install does not have | unit: surfaces=0, one mip, `image_size` = 2048 level bytes |

## C. Decode and upload: viewer substitutions

The client creates a DXGI resource and copies blocks. It does not expand BC1–BC5
pixels on the CPU, and it does not retag the colour space at the view. The
tree's decoder exists so the viewer can draw; its output is not the client's
resource bytes.

| finding | current tree | client behaviour | why it is not 1 to 1 |
|---|---|---|---|
| BC1–BC5 CPU decode | BC1/2/3/4_UNORM/5_UNORM decode to RGBA8 | resource is BC1/2/3/4/5; the GPU decodes with D3D-allowed promotion/rounding | a CPU RGBA8 view is one allowed implementation, not the client's bytes; for bit-exactness upload the blocks |
| BC7/BC6H | CPU decode; BC6H keeps exact halves; BC7 decodes per spec | resource is BC7/BC6H; the GPU decodes | BC6H/RGBA16F/R16F values are exact, but the resource format is a viewer substitution; BC7 CPU decode is still a reference path |
| UINT formats | DXGI 30 (`R8G8B8A8_UINT`) and 50 (`R8G8_UINT`) use the same byte path and upload as `Rgba8Unorm` | the view format is `_UINT`; the shader reads integers | 0..255 becomes 0..1; 19 install assets use stored `0x33` (DXGI 50) |
| R16_UNORM | rounded to the 8-bit view; `rgba16` stays empty | DXGI 56 `R16_UNORM` | the exact unorm value is not preserved; no `R16Unorm` upload path |
| HDR/16-bit upload | BC6H/RGBA16F/R16F carry exact halves and upload as `Rgba16Float` | DXGI 95/96/10/54 | values are exact; the resource format is a viewer substitution. The swapchain is `Bgra8UnormSrgb`, so values above 1.0 clamp at presentation; there is no HDR target or tonemap |
| BGRA / signed BC / depth / video / mobile EAC | mapped by the client tables, reported unsupported | client can name/render some of them | refused is honest, not mis-decoded, but not 1 to 1 coverage |
| 1D / cube / volume views | viewer always creates a 2D texture and binds a D2 view of layer 0 | `0x142e2dc70` flags: bit 0 = 1D, bit 2 = 3D, bit 3 = cube; `0x1426545d0` maps kinds 0/1 to `TEXTURE1D`, 2..7 to `TEXTURE2D`, 8 to `TEXTURE3D` | `0x0C0000000006A2C6` is a six-layer cube sampled as face 0; `0x0C0000000003B039` (1024x1) is uploaded as 1-row 2D; volume is refused |
| Sampler | repeat + linear min/mag; mip filter left at the default | client sampler state unread | wrapping, mip transitions and filtering are not the client's |
| DDS export | BC1/2/3 use DXT1/3/5; BC7/BC6H use a DX10 header; linear/float formats write RGBA8 with no DXGI id | the asset's DXGI id is authoritative | the export is a viewer artifact; float and sRGB exports are lossy |

## D. Material selection

The client's texture-set consumer `0x141f1ca80` walks the material-data blocks
and fills an **8-slot array in table order**, taking the first record for each
expected hash. The expected table is at `0x143359748`:

`0x4B9F2A16 0x3C981A80 0xA5914B3A 0xD2967BAC 0x4CF2EE0F 0x3BF5DE99 0xA2FC8F23 0xD5FBBFB5`

The viewer's `base_texture_of_base` instead takes the **first sRGB record in
record order**. If a material has albedo and emissive sRGB entries, the wrong
one can be painted. A plaintext string search over the image did not recover the
eight input names; the name mapping is open.

Also open:

- the fallback texture `0x0C000000000001EE` that `0x141f1e2e0` substitutes for
  hash `0x0CDA8A9E` when the hash misses; sibling `0x141f1db80` uses
  `0xD9987936`; the viewer returns `None` and falls through to white;
- `0x0F1` texture aliases: the consumer copies the GUID without a type test,
  but the alias asset's own layout/indirection is not read;
- the generic material-data → shader join and the runtime material pointer
  structure; `MdlcMaterialRead` (`0x141b88cb0`), `MaterialAssetLookup`
  (`0x141b7b9d0`) and `MaterialSlotBind` (`0x141b8a1d0`) are read, but the join
  key is an install observation (the viewer matches the full u64 MDLC value,
  while the runtime uses 32-bit keys).

On the practice range, the measured counts are 289 looks, 594 material entries,
120 decoded textures, 817 textured draws, 55 placeholder draws, 148 flat fills
and 3 entries without an sRGB base colour. These are **printed, not asserted**;
the test asserts 872 draws and more than one texture. The 55 placeholders are
material-selection outcomes (flat/no-sRGB or an id miss), not texture parse
failures; the per-draw split is not logged.

## E. Proof status

The texture-related differential units in the ledger are:

| unit | client function | cases | coverage | mutants | limitation |
|---|---|---|---|---|---|
| `fn/TextureDescriptor` | `0x142e2d900` | 364 | 100% (1/1) | 12/12 | compares only the 16-byte descriptor/flags prefix; `+0x4000` and bookkeeping past `+0x10` are outside |
| `fn/TextureDescriptorSizeWalk` | `0x142e2dbb0` over `0x142e37070` | 364 | 100% (1/1) | 11/11 + 1 equivalent | measures the wrapper, not the callee walk; `bias > mip_count` is excluded |
| `fn/TextureFormatFamilyBpp` | `0x142e372b0` | 374 | 100% (1/1) | 12/12 | sweeps `0..=0xff`, wider than the loader domain `0..=0x71` |
| `fn/TexturePayloadGuid` | `0x142e37520` | 364 | 100% | 12/12 | formula only |
| `fn/TextureRequestCount` | `0x142e37600` | 364 | 100% (1/1) | 1/1 | one mutant; the `payload_count > 1` gate is outside |
| `fn/TextureCountBitsShifted` | `0x142e374d0` | 364 | 100% but **zero branches** | 2/2 | the coverage figure is vacuous; the differential is the proof |
| `fn/Crc32NameHash` | `0x141b52ca0` | 364 | 100% | 5/5 | lengths only `0..64` |

No oracle unit drives:

- the loader `0x141dacf00` or its acceptance order;
- the chunk accessors `0x142e38730`/`40`/`50` and `0x142255e10`;
- the subresource record builder `0x141e296a0` or the copy path `0x141f0a000`;
- the per-level footprint `0x142e36ec0` or the block helpers `0x142e37260`/`0x142e371c0`;
- the PM→DXGI composition `0x142653b90` / `0x14348f500`;
- the material texture-set consumer `0x141f1ca80`.

The install tests prove the decoder's completeness/order/size against the
source at the offsets the decoder computed, but not the offset formula
independently for all 400 assets. The six pinned assets and the synthetic layout
test pin the formulas. The practice-range counts and the `npot_block == 5` /
20–84-byte size relations are printed, not asserted. The harness ledger is
local: a row is current only while its recorded candidate hash matches the tree;
the texture rows predate the latest uncommitted edits, so re-run them before
citing them as fresh.

## F. Next anchors, in order

1. **Exact `R16_UNORM`:** keep the raw `u16` and add an `R16Unorm` upload path.
   The client view is DXGI 56; the bind layout already accepts a filterable
   float sample.
2. **View dimensions:** pin the `0x14264be60` SRV-dimension switch and bind
   `D1`, `D2Array`, `Cube`/`CubeArray` and `D3` views; read the cube-face
   direction order. This closes the six-layer cubemap and the 1D asset.
3. **UINT path:** either a separate integer bind layout/shader path for DXGI
   30/50 or an explicit unsupported report; do not present integer values as
   normalized floats.
4. **Material selection:** implement the ordered 8-hash fill from
   `0x141f1ca80`, recover the input names from shader-source/reflection data,
   and add the `0x0CDA8A9E` fallback and the `0x0F1` alias resolution.
5. **Parser edges:** replace the `image_size` equality gate with a per-level
   bound; model 6x6/8x8 EAC sizes; use descriptor fields where the client does.
6. **Oracle units:** add the loader, chunk-accessor, record-builder and
   per-level-size units; they are the cheapest way to turn the read rows above
   into verified ones.

## G. Recent changes

- **2026-10-29 — BC2/BC3 colour block.** BC2/BC3 now always use the four-colour
  palette; BC1 keeps its three-colour branch. The image proves the DXGI
  identity (74/75/77/78) and the absence of a CPU palette decoder; the palette
  arithmetic is D3D specification.
- **2026-10-29 — parser documentation audit.** Corrected the loader's `+3`
  field read and the 0x20 job-registration guarantee; the `0x142e36ec0`
  geometry limits; the `0x141dabee0` arrival versus `0x141dac720` rebase; the
  SRV plane format; and the unit domains/coverage caveats.
- **2026-10-30 — 16-bit/HDR upload.** BC6H, RGBA16F and R16F keep exact half
  words and upload as `Rgba16Float`; the resource format is a documented viewer
  substitution. R16_UNORM, UINT and the CPU BC decode remain open.

## Verification

```text
cargo test -p ow-tank texture
cargo test -p ow-scene texture_colour_space
cargo test -p ow-client surface::tests
cargo test --release -p ow-tank --test texture_real
cargo test --release -p ow-tank --test texture_levels_real
cargo test --release -p ow-tank --test bc6h
cargo xtask verify --fast
```

The harness units are listed in `docs/formats/materials.md` and run with
`python dev/verify_harness/cli.py fn/<Name>`.

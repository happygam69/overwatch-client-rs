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
- **Textures: decoded.** BC1-BC7 and BC6H decode to RGBA8, checked against the
  Microsoft reference decoder.
- **Bundle-aware reads:** models, looks, materials, material data and textures
  open through `GameData::open_asset`, so bundle-hosted assets are included.
- **Practice-range result:** 289 looks, 594 material entries, 120 textures
  decoded, 817 textured submesh draws, 55 placeholder draws, 148 flat fills and
  3 entries without an sRGB base colour. Those are reported per submesh, not
  hidden.
- **Runtime material structure:** open.

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

A header then a texture array: texture offset i64 at +16, texture count as the
byte at +60, and each record a texture GUID (`0x004`) followed by a CRC32 of
the input name and a flag byte.

## The texture (`0x004`)

A 32-byte header: `u16 flags`, `u8 mip_count`, `u8 format` (a DXGI id, stored
one higher from 26 up, so decrement first), `u8 surfaces`, `u8 usage`,
`u8 payload_count`, `u8 unknown`, `u16 width`, `u16 height`, `u16 data_size`,
then two u64s.

Formats seen in a 400-asset census: BC1 (187), BC4 (96), BC5 (56), R8G8B8A8
(5), BC7 (39). The payload chain is two or three links:

- `payload_count == 0`, or a payload whose `image_size` covers level 0: blocks
  follow the 32-byte header.
- Otherwise payload 0 is embedded at +0x20 (`mips`, `surfaces`, `image_size`,
  `header_size`) and holds only the small mips.
- The separate payload asset is type `0x04D`, payload index in the locale
  nibble (`guid::payload_of`). Chains of three are real; the reader tries
  payloads 1..3 and keeps the first whose block run covers level 0.

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

The base-colour rule is "the material's first **sRGB** texture" plus "it must
have structure in at least two channels". Several readable textures are
normal/mask or flat fills and are rejected; the practice-range launch names each
remaining placeholder per submesh.

## Open

- The client's runtime material pointer structure and material application are
  not read; the viewer is not the client's renderer.
- The base-colour rule still leaves 55 placeholder draws, 148 flat fills and 3
  no-sRGB entries on the practice range. Each needs a per-submesh reason.
- The model's `MSTU` field hashes are the install-local route to texture
  references inside the model, skipping the material wrapper.
- The texture sampler path is not read.

## Verification

```text
cargo run -p ow-tank --bin look -- --join 400
cargo run -p ow-tank --bin tex -- --census 400
cargo test --release -p ow-tank --test bc6h
```

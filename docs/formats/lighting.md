# Lighting manifests

A lighting manifest is a flat table: a small header, then one 8-byte GUID per
lighting chunk. It is the only thing the install ships that lists a map's
lighting chunks by name.

## Layout (install)

```text
0x00  u32 magic "EEDC"   (= 0x43444545 little-endian)
0x04  u32 data_size      asset length - 8
0x08  u32 unknown1       constant 45 on every manifest inspected
0x0C  u32 unknown2       0 on 26 of 27 manifests, 1 on the other
0x10  u32 unknown3       0 on every manifest inspected
0x14  u32 unknown4       equal to unknown5, or exactly 32 greater
0x18  u32 unknown5       a multiple of 32
0x1C  u32 chunk_count    40 + 8 * count == asset length
0x20  u32 unknown6       0
0x24  u32 unknown7       0
0x28  u64 chunk GUID x chunk_count, every one type 0x0BE
```

The install sweep: **27 of 323** registry manifests openable (296 are the
streamed-content wall), **0 parse failures**; 9,998 chunk GUIDs named, all
9,998 type `0x0BE`; both length invariants held 27/27.

`unknown4`/`unknown5` look like a pair of byte offsets or sizes into the chunk
(the observed pairs include `800/800`, `992/992`, `1056/1024`), but their
consumer is not read.

## In the code

`ow-tank::lighting` (`crates/ow-tank/src/lighting.rs`): `TYPE` `0x0BD`,
`CHUNK_TYPE` `0x0BE`, `MAGIC`, `parse(data) -> LightingManifest` with the
header fields and `Vec<Chunk>`. Synthetic tests pin every header field against
hand-written bytes.

## Open

- What a lighting chunk (`0x0BE`) holds — only the manifest is parsed.
- The meaning of `unknown1..7`.
- Whether the reference's 32-byte shape is an older layout or a different
  asset.

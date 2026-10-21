# Lighting manifests (`0x0BD`) and their chunks (`0x0BE`)

A lighting manifest is a flat table: a small header, then one 8-byte GUID per
lighting chunk. It is the only thing the install ships that lists a map's
lighting chunks by name.

Evidence classes: *install* for the layout below (read off `E:\Overwatch`,
build 2.25.0.0.154088, by `cargo test -p ow-tank --release --test
lighting_real` and `cargo run -p ow-tank --release --bin light`);
*decompiled* for the reference reader (`teLightingManifest`, TankLib) — which
describes a **32-byte header with a different field set that matches no
manifest on the install**. The install wins; the divergence is recorded rather
than reconciled.

## Layout (install)

```text
0x00  u32 magic "EEDC"   (= 0x43444545 little-endian)
0x04  u32 data_size      asset length - 8 (27/27 test, 24/24 probe)
0x08  u32 unknown1       constant 45 on every manifest inspected
0x0C  u32 unknown2       0 on 26 of 27 manifests inspected, 1 on the other
0x10  u32 unknown3       0 on every manifest inspected
0x14  u32 unknown4       equal to unknown5, or exactly 32 greater (all 27)
0x18  u32 unknown5       a multiple of 32 on every manifest inspected
0x1C  u32 chunk_count    40 + 8 * count == asset length on every one
0x20  u32 unknown6       0 on every manifest inspected
0x24  u32 unknown7       0 on every manifest inspected
0x28  u64 chunk GUID x chunk_count, every one of type 0x0BE
```

`unknown4`/`unknown5` look like a pair of byte offsets or sizes into the chunk
(the observed pairs include `800/800`, `992/992`, `1056/1024` — multiples of 32,
equal or differing by exactly 32), but their consumer was not read, so they
stay unknowns.

## Measured

* Test sweep of the install: **27 of 323** registry manifests openable (296
  are the streamed-content wall), **0 parse failures**; 9,998 chunk GUIDs
  named in total, **9,998/9,998 type `0x0BE`**; both length invariants held
  27/27.
* Probe sample of the first 200: 24 openable, 0 failures; 2,372 chunk refs,
  all type `0x0BE`; magic `43444545` on all; invariants 24/24.

## In the code

`ow-tank::lighting` (`crates/ow-tank/src/lighting.rs`): `TYPE` 0x0BD,
`CHUNK_TYPE` 0x0BE, `MAGIC`, `parse(data) -> LightingManifest` with the
`Header` fields above and `Vec<Chunk>`. Synthetic tests pin every header field
against hand-written bytes (a field at the wrong offset still parses, so each
one is corrupted individually).

## Open

* what a lighting chunk (`0x0BE`) holds — only the manifest is parsed;
* the meaning of `unknown1..7`;
* whether the reference's 32-byte shape is an older build's layout or a
  different asset — unresolved, no consumer read.

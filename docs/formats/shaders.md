# Shaders: the asset containers (`0x085` groups, `0x087` code, `0x086` instances)

Shader *behaviour* — the bytecode — stays out of scope by design
(`docs/roadmap.md`). The asset containers are ordinary files, and they are now
read: a group names the code blobs and sources a pipeline tier uses; a code
blob is a gzip stream; an instance binds a code GUID to constant buffers,
shader resources, sampler states and a vertex layout. A material asset points
at a shader group and a shader source (`docs/formats/materials.md`), so this
is the next hop of that chain.

Evidence classes: *install* for the group and code layouts (read off
`E:\Overwatch`, build 2.25.0.0.154088, by `cargo test -p ow-tank --release
--test shader_real` and `cargo run -p ow-tank --release --bin shader`);
*decompiled* for the instance layout (TankLib `teShaderInstance.cs`),
validated against **nothing** — this install's registry holds **zero**
type-`0x086` assets.

## Group (`0x085`)

Header 184 bytes; `N = u32` at `0x64`; six arrays follow it, each starting
exactly where the previous ended. Verified on the 100 openable groups (of the
first 400 sampled; 48,573 in the registry — the rest are the streamed-content
wall), with the probe sample (30/30) agreeing:

```text
0x00  u64 unknown0          a type-0x040 GUID; likely the group's source
0x08  u64 x_offset          always 184 (the header end)
0x10  u64 a_offset          X = 24-byte records x N, all zero observed
0x18  u64 null              -1 on every group inspected
0x20  {i16,i16,i16,i16} x3  small numbers (< N), meaning unknown
0x38  u64 zero
0x40  {i16,i16,i16,i16} x3  small numbers (< N), meaning unknown
0x58  u64 b_offset          B = u32 x N, padded to 8; all zero observed
0x60  u32 unknown60         seen 0x01100001, 0x01000001, 0x00200004
0x64  u32 count             N
0x68  u64 c_offset          C = u64 x N, code GUIDs (type 0x087)
0x70  u64 d_offset          D = u64 x N, all 0xFF observed
0x78  u64 e_offset          E = u64 x N, source GUIDs (type 0x040)
0x80  u64 f_offset          F = u64 x N, all 0xFF observed; ends at EOF
0x88  u64 zero
0x90  u64 zero
0x98  u64 source_guid       type 0x088
0xA0  u64 zero
0xA8  u64 zero
0xB0  u64 0x000000FFFFFFFFFF
```

The C entries were type `0x087` on **2,933 of 2,933** entries across those 100
groups. The E entries were type `0x040` on 2,882 of 2,933 — the other **51 are
type `0x001`** (unexplained; possibly an inline/empty shader). The parser
checks every array offset against `N` before reading anything, so a group
whose layout differs fails instead of yielding garbage.

## Code (`0x087`)

Header at least 140 bytes; the payload is a **gzip stream**. Verified on the
144 code assets the install test could open (of the first 200 sampled; 147,881
in the registry) with the probe's 30/30 sample agreeing:

```text
0x00  u64 unknown0           -1 on every code asset inspected
0x08  u64 unknown1           seen 0x0000000140000401 and 0x0000000140000402
0x70  u64 data_offset        where the gzip stream starts
0x84  u32 compressed_size    equal to asset length - data_offset, always
0x88  u32 uncompressed_size  the inflated length, always
```

The parser inflates eagerly and keeps the bytes; `compressed_size`,
`uncompressed_size` and the gzip magic are all checked, so a misdeclared asset
fails loudly.

## Instance (`0x086`)

The layout in `shader.rs` (108-byte header, then optional tables: shader
resources, sampler states, vertex input elements, constant-buffer headers with
per-buffer part lists and skeletons) is a port of the reference reader and is
**not tested**: no type-`0x086` asset exists in this build. The code lives
behind `parse_instance` so a build that does ship them can exercise it, and
the module's synthetic tests pin the reference's read order and gating.

## Open

* the semantics of the group's unnamed arrays (A/B/D/F contents, `unknown60`,
  `unknown0` vs `source_guid`) — offsets and types are pinned, meanings are
  fitted or blank;
* the 51 type-`0x001` source refs in E;
* the instance layout against real data (none installed here);
* what consumes a group (the renderer's tier/instance selection) — client
  side, no capture holds the renderer's code pages (see
  `docs/formats/rendering.md`).

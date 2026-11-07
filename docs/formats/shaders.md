# Shaders: the asset containers

Shader *behaviour* — the bytecode — is out of scope for the gameplay rewrite.
The asset containers are ordinary files and are read: a group names the code
blobs and sources a pipeline tier uses; a code blob is a gzip stream; an
instance binds a code GUID to constant buffers, resources, sampler states and a
vertex layout. A material asset points at a shader group and a shader source
(`formats/materials.md`), so this is the next hop of that chain.

## Group (`0x085`)

Header 184 bytes; `N = u32` at `0x64`; eight arrays follow, each starting where
the previous ended.

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
0x80  u64 f_offset          F = u64 x N, all 0xFF observed
0x88  u64 g_offset          G = u64 x N, one hash-like value per record
0x90  u64 h_offset          H = u64 x N, all 0xFF observed; ends at EOF
0x98  u64 source_guid       type 0x088
0xA0  u64 zero
0xA8  u64 zero
0xB0  u64 0x000000FFFFFFFFFF
```

The C entries were type `0x087` on 5,844 of 5,844 entries across 200 parsed
groups. The E entries were type `0x040` on 1,662 of 1,734; the other 72 are
type `0x001` (unexplained). The parser checks every array offset against `N`
before reading, so a different layout fails instead of yielding garbage.

## Code (`0x087`)

Header at least 140 bytes; the payload is a **gzip stream**.

```text
0x00  u64 unknown0           -1 on every code asset inspected
0x08  u64 unknown1           seen 0x0000000140000401 / ...0402
0x70  u64 data_offset        where the gzip stream starts
0x84  u32 compressed_size    asset length - data_offset
0x88  u32 uncompressed_size  the inflated length
```

The parser inflates eagerly and keeps the bytes; the sizes and gzip magic are
checked.

## Instance (`0x086`)

The layout in `shader.rs` (108-byte header, then optional tables) is a port of
the reference reader and is **not tested against real data**. This build does
not merely lack assets: it registers no loader for the type. The asset-type job
table at `0x143a7c3a0` maps type `0x085` (group) to job 23 and `0x087` (code) to
25, and type `0x086` to **0** — no job — while the registry holds zero type-`0x086`
assets. The module's synthetic tests pin the read order and gating, nothing
more.

## Open

- The semantics of the group's unnamed arrays (`unknown60`, `unknown0` vs
  `source_guid`).
- The 72 type-`0x001` source refs.
- The instance layout against real data.
- What consumes a group (the renderer's tier/instance selection).

## Verification

```text
cargo test --release -p ow-tank --test shader_real
cargo run -p ow-tank --release --bin shader
```

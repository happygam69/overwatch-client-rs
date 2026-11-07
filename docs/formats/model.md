# Models: the asset's chunk table, MDLC, MRNM and MSTU

A model is a chunked asset (`MODL`) whose payload chunks are read by one of two
parallel dispatchers: a **sizing** pass and a **load** pass. Both use the same
tag table, the same per-chunk version gate and the same order.

## The chunk table (read)

**Sizing:** `FUN_141b5e1e0` (model object `+0x08` is the running total). For
each record it compares `*(u16 *)(record + 0xe)` — the chunk version — and
returns `7` on a mismatch, `6` on a tag it does not know.

**Load:** `FUN_141b5df70`, in this order. Its first argument is the asset-side
struct holding the seven chunk *records*; `param_1[0xd]` is the model object
being built.

| tag | version | reader |
|---|---|---|
| `MDLC` | `0x165` | `MdlcMaterialRead` `0x141b88cb0` (fills model `+0x00..+0xb8` and the two tail arrays) |
| `MRNM` | `0x1f` | `FUN_141b893e0` (fixup) then `MrnmHandler` `0x141b88f10`; the handler's first store is model `+0x00` = the runtime block |
| `mskl` | `0x20` | `MsklTableBuild` |
| `MOCL` | `0x14` | `MoclRead`; 989 of the install's 32,280 models carry it |
| `MESD` | `2` | copied verbatim; the copy's first two qwords are rebased; model `+0x20` = the copy |
| `CLTH` | `0x18` | copied verbatim; `0x141b88a00`; model `+0x18` = the copy; 1,145 models carry it (the tag is `CLTH`, not the reference's `HTLC`) |
| `MSTU` | `2` | `FUN_141b88df0` → `FUN_1421d88c0`; model `+0x28` = the STU object (or null) |

The sizing pass stores each chunk record's size at `+0x48..+0x60` of its own
object; it does not touch the model object.

A record is `{tag u32, serialized size i32, chunk size i32, marker u16,
version u16}` and its payload starts at `record + 0x10`
([`chunks.rs`](../../crates/ow-tank/src/chunks.rs)).

`ow-tank::model::parse` gates `MDLC`, `MRNM` and `MSTU` on those versions; it
parses neither `MESD`, `CLTH`, `MOCL` nor `mskl` (nothing here needs them yet).

The four tags are the client's own: the two dispatchers compare
`0x4d4f434c` (`MOCL`) and `0x434c5448` (`CLTH`) at `0x141b5e0a2`/`0x141b5e1f4` and
`0x141b5e139`/`0x141b5e205`. The names `LCOM` and `HTLC` that an earlier version
of this page used are those words' bytes read in the wrong order, and `HTLC`
occurs nowhere in the image. A census of all 32,280 models confirms the same
seven tags: `MDLC`, `MSTU`, `MRNM`, `mskl`, `MESD`, `CLTH`, `MOCL`.

**`MHRP` does not exist in this build.** The reference's hardpoint chunk has no
four-byte tag constant anywhere in the image and no model carries it, and the
tag gate refuses one: `ModelChunkSize` (`0x141b5e1e0`) returns `6` for any tag
outside the seven and the load pass runs only when it returns `0`. This
build's hardpoints are `STUModelHardPoint` instances in the model's `MSTU`
(`ow-tank::hardpoint`, docs/formats/animation.md).

## MDLC: the material list (read)

`MdlcMaterialRead` (`0x141b88cb0`) is a field copy plus two `memcpy`s; its
payload is `record + 0x10`. Read instruction for instruction:

| source | destination | bytes | what it is |
|---|---|---|---|
| `+0x10` | `+0x40` | 16 (`MOVUPS`) | box minimum, `vec3` + a zero lane |
| `+0x20` | `+0x50` | 16 (`MOVUPS`) | box maximum, `vec3` + a constant `0.001` lane |
| `+0x30` | `+0x60` | 16 (`MOVUPS`) | sphere centre, `vec3` + a zero lane |
| `+0x40` | `+0x70` | 4 | sphere radius |
| `+0x44..+0x50` | — | — | **never read** (the sphere row's dead lanes) |
| `+0x50`…`+0x64` (6 × 4) | `+0x84..+0x98` | 24 | a flags word (`0x80e`/`0x800` on two models) and five values, two of them `1.0f` |
| `+0x68` | `+0x9c` | 2 | material count (`u16`) |
| `+0x6a` | `+0xac` | 1 | second-array count (`u8`) |
| `+0x6b` | `+0xa9` | 1 | `BSF`: lowest set bit of the byte, `0xFF` when zero |
| `+0x6c` | `+0xa8` | 1 | |
| `+0x70` | `+0xa0` | 8 | |
| `+0x78`, `+0x79` | `+0xaa`, `+0xab` | 1 each | |
| pointer `+0x08`, count `+0x68` | tail at `+0xb0` | 8 each | the material GUIDs |
| pointer `+0x00`, count `+0x6a` | after the GUIDs | 4 each | the second array |

The destination's tail size is `((u16[+0x68] * 8 + 0xb3) & ~3) + u8[+0x6a] * 4`
— the 21 bytes at `0x141b88e30`. `ModelChunkSize` (`0x141b5e1e0`) calls it, and
`ModelChunkLoad` (`0x141b5df70`) then runs the copy. The size pass also
**relocates** the two pointers in place (`stored value + payload base`), so the
file stores payload-relative offsets and the client adds the base at load time.

**The bound, and what the earlier reading got wrong.** The first three `vec4`
rows are the model's own bound, and it reproduces the geometry. Two independent
models (`0x0d00000000000109`, a character; `0x0d000000000006a4`, a view model):

```text
MDLC 0x10 (-0.6616, -0.0029, -0.4976)  decoded min (-0.659, -0.003, -0.498)
MDLC 0x20 ( 0.6700,  1.8404,  0.2020)  decoded max ( 0.669,  1.839,  0.201)
MDLC 0x10 (-0.6803,  0.9141, -0.1883)  decoded min (-0.680,  0.914, -0.188)
MDLC 0x20 ( 0.6813,  1.5591,  0.0654)  decoded max ( 0.680,  1.558,  0.064)
```

`0x30`/`0x40` are a sphere: every sampled vertex lies within `0.963` of the
centre with `radius = 0.9801`, while the box's half-diagonal is `1.17` — so the
radius bounds the *mesh*, not the box corners. The lanes at `0x1c`/`0x3c` are
zero and `0x2c` is exactly `0.0010` on both models.

Install-wide, on 68 models / 390 submeshes at every LOD: **every vertex is
inside both** (worst overrun 0.000000), and the box's worst extent gap against
the decoded vertices is **0.0143 m**. `tests/model_attrs_real.rs` asserts those
three numbers, so the field meaning is measured, not inferred from the copy
loop.

So the old reader's `bounding_box: [[f32; 4]; 4]` was a misreading — it is not a
matrix — but the region **is** bound data, and deleting the field lost it.
`ow-tank::model::Model::bounds` now carries `min`, `max`, `centre` and `radius`.

`ow-tank::model::parse` reads the material count, the GUID array and the bound.
The extraction is a 1:1 copy of the loop but is **not yet** covered by a
differential unit. The two obstacles recorded earlier are narrower now:

* the tail size **is** known (the formula above), so a fixture can size its
  destination;
* absolute pointers **are** expressible: `fn/MrnmHandler` relocates fixture
  slots the same way the client does.

What remains is that `0x141b88e30` has **no Ghidra function entry** — confirmed
through the bridge: `get_functions 0x141b88e30` reports no function at or
containing that address, while `ModelChunkSize` renders the call as
`func_0x000141b88e30`. The oracle can still be pointed at those 21 bytes
directly; there is simply no function object to name. Tests: the synthetic
cases in `model.rs` pin both the material list and the bound, and
`tests/model_attrs_real.rs` asserts the bound contains every decoded vertex on
the install.

## MRNM: the render-mesh descriptors (read and verified)

Three functions, each a `dev/verify_harness` unit built from the shipped code:

| client | unit | what it does |
|---|---|---|
| `FUN_141b893e0` | `fn/MrnmFixup` | offset → pointer, in place: header `+0x00/+0x08/+0x10`; per vertex buffer `+0x10` and, only when `flags & 0x10` is **clear**, `+0x50/+0x58`; per index buffer `+0x08`. A stored zero stays zero; submesh descriptors are untouched. |
| `FUN_141b89390` | `fn/MrnmRuntimeSize` | `submeshes*0xa0 + 0xc0 + vertex_buffers*0xf8 + index_buffers*0x18` (32-bit). |
| `MrnmHandler` `0x141b88f10` | `fn/MrnmHandler` | fills the runtime block: header, then submesh `0xa0`, vertex-buffer `0xf8` and index-buffer `0x18` records. |

The Rust port is `ow-tank::mrnm`; the ledger binds it to the shipped crate and
the probe (`crates/ow-tank/src/bin/verify_probe.rs`, entries `mrnm_fixup`,
`mrnm_runtime_size`, `mrnm_load`).

### The runtime block

`(dst + 0xcf) & !0xf` = `dst + 0xc0` when `dst` is 16-byte aligned, then:

```text
+0x00 u64   OR of every submesh's +0x28 qword, skipping -1
+0x08 u32   payload +0x18   (whole-model vertex count)
+0x0c u32   payload +0x1c   (submesh count)
+0x10 u16   payload +0x20   (material count)
+0x12 u8    payload +0x22   (vertex-buffer count)
+0x13 u8    payload +0x23   (index-buffer count)
+0x14 u32   payload +0x24
+0x18 .. +0xa4   five 0x1c-byte blocks from payload +0x28/+0x44/+0x60/+0x7c/+0x98
+0xa8 u64   submesh-array pointer      (+0xc0)
+0xb0 u64   vertex-buffer-array pointer
+0xb8 u64   index-buffer-array pointer
+0xc0       submesh array,   0xa0 each
            vertex array,    0xf8 each
            index array,     0x18 each
```

Per **submesh** (source → runtime): `+0x00 → +0x00`, `+0x04 → +0x04`,
`+0x08 → +0x08`, `+0x0c → +0x10`, `+0x10 → +0x20`, `+0x14 → +0x24`,
`+0x18 → +0x28`, `+0x1c → +0x30`, `+0x20 → +0x34`, `+0x24 → +0x38`,
`+0x28 (u64) → +0x40`, `+0x30 (u64) → +0x48`, `+0x38 → +0x50`,
`+0x3c → +0x54`, `+0x40..+0x60 (32 B) → +0x58`, `+0x60 → +0x78`,
`+0x64 → +0x7c`, `+0x68 (u16) → +0x80`, `+0x6a (u16) → +0x82`,
`+0x6c (u16) → +0x86`, `+0x6e..+0x76 (8 B) → +0x88`, `+0x76 → +0x90`,
`+0x77..+0x7a → +0x91..+0x94`, `+0x7b → +0x95` (a zero takes the model
object's `+0xaa`), `+0x7c (u16) → +0x84`. `+0x0c`, `+0x2c` and `+0x3c` of the
runtime record are zeroed; `+0x14..+0x20` and `+0x96..+0xa0` are not written.

Per **vertex buffer** (`0x60 → 0xf8`): `+0x00 → +0x10`, `+0x04 → +0x14`,
`+0x08 → +0x18`, `+0x09 → +0x19`, `+0x0a → +0x1a`, `+0x0b → +0x1b`,
`+0x10 (pointer) → element copy at +0x1c` (`+0x0a * 8` bytes), `+0x18..+0x50
(7 × u64) → +0xb0..+0xe8`, `+0x50/+0x58 → +0xa0/+0xa8` **only when
`flags & 0x10` is set** (those are the two vertex-data GUIDs); otherwise
`+0xa0..+0xb0` is zeroed. Runtime `+0x00` and `+0x08` are `-1`.

Per **index buffer** (`0x10 → 0x18`): `+0x00 → +0x08`, `+0x08 (pointer) →
+0x10`; runtime `+0x00` is `-1`, `+0x10` is zero.

**Consequence for readers:** the runtime vertex-buffer record does *not* keep
the in-chunk stream offsets — it zeroes them. A reader that walks the payload
must read the streams from the payload descriptor (`+0x50/+0x58`, already
rebased by the fixup) and load an asset instead only when bit `0x10` is set.

### The vertex element record (read, 2026-11-05)

The runtime block keeps the element descriptors because they are the only
record of what a vertex *is*: per vertex buffer, `+0x1a` is the element count
and `+0x1c` starts an array of `count * 8` bytes. Each record is

```text
+0  u8   semantic   0 position, 1 normal, 3 tangent, 4 blend indices,
                    5 blend weights, 8 colour, 9 texcoord, 16 vertex index
+1  u8   index      the semantic's own index (texcoord/colour set)
+2  u8   format     0x02 three floats, 0x03 four, 0x04 half x2, 0x07 u16 x4,
                    0x08 u8 x4, 0x09 snorm 8 x4, 0x0c u32
+3  u8   stream     0 or 1
+4  u32  packed     offset << 12 | stride << 22
```

Both halves of the packed dword are **read from the client**, not inferred.
The client's own render-mesh writer `FUN_14210c450` emits exactly this record
and builds the dword as `stride << 22 | offset << 12`; it writes position as
`0x02` (three floats) or `0x03` (four, with the vertex index in `.w`), normal
and tangent as `0x09`, texcoord as `0x04`, the vertex index as `0x0c`, and one
format no model file carries, `0x13`. The D3D11 consumer
`pm_dx11::Device::CreateRenderPipeline` (`0x142667be0`) reads the same form and
resolves the format byte through the 64-entry table at `0x1434965d0`.

**The sizes are pinned by the file itself.** Element offsets are adjacent and
the last element of a stream ends exactly on that stream's stride. On Tracer's
character mesh `0x0d00000000000109` stream 0 is 24 bytes (three floats at 0,
four `u16` at 12, four `u8` at 20) and stream 1 is 20 bytes (normal `0x09` at 0,
tangent `0x09` at 4, texcoords `0x04` at 8 and 12, `u32 0x0c` at 16).
`tests/model_attrs_real` compares the packed stride with the descriptor's
stride over 68 models and **637 element records: 0 disagreements, 0 format
bytes outside the table**. The unit test
`model::tests::the_element_record_is_the_client_layout` pins the same records
and the tiling arithmetic.

**Values are not read, and cannot be.** The client never converts vertex
bytes to floats on the CPU — there is no half-float conversion instruction
anywhere in the image — so it hands the format byte to D3D11 and the GPU does
the conversion. Consequences:

* the mapping from the file's format byte (`0x00..0x13`) to the *engine*
  format byte that `0x1434965d0` indexes is **open**. It is not a table in the
  image (a 13-entry model-format table does not exist), and the pipeline's
  element list arrives already built at `CreateRenderPipeline`, so its builder
  is elsewhere;
* the floats `ow-tank::model` produces are the viewer's reading of the file.
  The one rule the GPU documents is applied: SNORM is `max(x/127, -1)`, so
  byte `0x80` is exactly -1 (a bare /127 gives -1.0079; OWLib's /128 is a
  third answer). The `0x13` format the client's writer uses is left
  undecoded rather than given an invented reading.

### Not read

* the **mesh assembly**: index remap, compact vertex array, `uv_sets`/colours,
  `lod` selection. The descriptors those read are verified; the assembly is
  the viewer's. The bone-index half of the assembly (the mesh's raw blend
  indices through the skeleton's Lookup) is read and measured separately —
  `formats/animation.md` § "Bone indices" — and its client consumer is open.
* the **format byte to engine format join** (`0x00..0x13` → the byte
  `0x1434965d0` indexes), and therefore the value interpretation.
* the runtime block's `-1` slots, its `+0x00` mask and the five `0x1c` blocks
  are copied but not interpreted anywhere yet.

## LOD selection (read, 2026-11-05)

The submesh LOD byte is **a set of levels, one bit per level**, not an ordinal
(payload `+0x78`, copied to runtime `+0x92` by `MrnmHandler`). The chain, in
current-image addresses:

1. **Union.** `0x141b40be0` walks four model slots at `+0xe8` (streamed LOD
   models); per submesh it ORs the byte into a mask, **skipping exactly `0` and
   `0x7f`** (read at `0x141b40c62`, the test is `byte != 0 && byte != 0x7f`).
   `0xFF` is not special-cased, so it would enter the mask with every bit set.
2. **Availability.** `0x141b3def0` builds the levels the instance can use from
   the two-byte pair table at `+0x170` (count `+0x178`) and
   `(*(u32 *)(model0 + 0xa0) >> 18)`; the union is ANDed with it.
3. **Range.** The lowest set bit of the result is stored at `+0x20e`, the
   highest at `+0x20f`; an empty mask stores `0` in both (not `0xff`).
4. **Pick.** `high - 3 + quality`, clamped to `[low, high]`, where quality is
   `(0x141b2c0a0() == 1) + 0x141b2c0b0()` with the mode byte `+0x212` (`0x11`
   forces 0; `0x0e` caps it at 1). The index reads a per-level float from
   runtime `+0x50 + index*4` into `+0x1d4`.
5. **Distance refine.** `0x141b3ce00` walks from `high` down while
   `distance² > ((i - base) * scale)²` (minimum 5.0, 0.9/1.1 hysteresis, x0.5
   in mode `0x11`) and returns the index; `0x141b3d970` computes the cross-fade
   fraction `(index - low) / (high - low)`.
6. **Draw record.** The draw path re-reads the byte (`0x141f3c1a4`,
   `0x141f3c254`, same `0`/`0x7f` skip) and `0x141e20d10` stores it in the draw
   record at `+0x134` (`0x141e20d4e`).

`ow-tank::model` implements the union and the **finest** part of step 3:
`Model::lod_mask` is the union, `Model::finest_level` is the bit test at the
lowest set bit. So a submesh carrying `3` is drawn at the two finest levels, a
submesh carrying `0` is never drawn, and `0xFF` is drawn at every level. The
client's per-instance pick (steps 4-5) is **not** implemented - a viewer has no
per-instance distance - and the final per-submesh inclusion predicate (the
consumer of step 6) is **open**: image-wide, runtime `+0x92` is read only by the
two union builders (`0x141b40c62`, `0x141f3c1a4`/`0x141f3c254`), and no byte
reader of the draw record's `+0x134` was found.

Measured on the install: every LOD byte is `1`, `2`, `4` or `8` (one bit). A
hero body carries 9 submeshes at `1`, 8 at `2`, 6 at `4` and 6 at `8`; one
model carries one each of `8/4/2/1`; no `0`, `0x7f` or `0xFF` was seen. The
three rules that exist (this tree's bit test, the reference exporter's
`lod & min`, and the old equality `lod == min`) therefore agree on today's
data; they diverge on `3`, `0` and `0xFF`, which is why the tree uses the bit
test (`model::tests::the_finest_level_is_the_lowest_bit_of_the_lod_union`).

## MSTU: the structured-data header (read)

`FUN_141b5df70` rebases the payload's first qword (offset → pointer), then
`FUN_141b88df0`: if `*(u32 *)(payload + 8)` is zero, the model's STU is null;
otherwise `FUN_1421d88c0(model, *(u64 *)payload, *(u32 *)(payload + 8), 0)` —
the STU v2 blob loader already ported in `ow-stu`.

So the header is `{u64 offset; u32 size; u32 unused}` and `offset` is relative
to the MSTU payload. The old reader took the size as a `u64`; the client reads
four bytes (fixed). `ow-tank::mstu::parse` slices `payload[offset..offset+size]`
and parses it with `ow-stu`.

Install evidence (60 models, `mstu_real`): `offset == 16` on 60/60; the slice
is in bounds on 60/60; the STU blob parses on 60/60; an asset-relative reading
parses on 0/60.

## Reproduce

```sh
python dev/verify_harness/cli.py fn/MrnmFixup
python dev/verify_harness/cli.py fn/MrnmRuntimeSize
python dev/verify_harness/cli.py fn/MrnmHandler
cargo test -p ow-tank --lib model:: mstu::
cargo test -p ow-tank --test model_mesh_real --test mstu_real
cargo test -p ow-tank --release --test model_attrs_real -- --nocapture   # 637 element records, stride agreement
```

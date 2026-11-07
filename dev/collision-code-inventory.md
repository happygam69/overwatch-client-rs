# Collision code inventory

Read-only inventory of every collision-related parsing/decoding module in this
repository, so a senior engineer can compare each stage against the game
client's own code. Nothing was modified except this file; no build or test was
run.

Scope read: `crates/ow-tank/src/{collision,collision_level,brush,
model_collision,mesh_shape,mesh_world,domino_world,range_world,chunkkind}.rs`
and `crates/ow-sim/src/{collide,collider,fan,walk,policy}.rs`. Marker grep
(`FIT-`, `TODO`, `not tested`, `unverified`, `open`, `FIXME`) over those
modules and the whole `crates/` tree. Adjacent modules inspected for context
(not owned by this report): `ow-tank/src/brush_mesh.rs`,
`ow-tank/src/brush_convert.rs`, `ow-tank/src/model_body.rs`,
`ow-tank/src/model_shape_source.rs`, `ow-sim/src/shapes.rs`,
`ow-sim/src/bin/verify_probe.rs`, `ow-tank/src/bin/verify_probe.rs`.

Evidence classes as the code states them: **read** (a value flow from the
image, with the address), **observed/measured** (a property of the operator's
payload, e.g. a tiling that divides exactly), **declared open** (no reader
found; the feature is not implemented).

---

## 0. One-paragraph map

`ow-tank` holds the **ingest**: it turns the install's own bytes into the
client's record layouts. `chunkkind` = the kind table, the
`MAP_CHUNK_COLLISION` sector/tail container and its in-place fixup.
`collision_level` = the level's three mesh arrays out of that container.
`collision` = the *pathing* chunk's cell container plus the read
`MapChunkCellBytesFixup`. `mesh_shape`/`mesh_world` = the `STU_537E56E4` block
unpack, the `MeshShapeFill` record, and a guest-address image of the three
arrays. `model_collision` = the registered class `C`, its array tag and its
flag fold. `brush` = the brush vertex/node records and the ring builder.
`domino_world` = the world-slot encoding and the body handle. `range_world` =
the practice range assembled as one collision world.

`ow-sim` holds the **sweep**. `collide` is the read swept-query stack (bounds
stage, query driver, sphere/capsule/type-3 mesh narrowphases, box-triangle
overlap, layer filter, shape transform, per-type dispatch, simplex reductions,
closest-point GJK). `collider` is the collider-mesh build chain (shell -> face
records -> BVH -> flattened source). `walk` is the character step built on the
verified leaf. `policy` is the narrowphase's hit callback. `fan` is the wish
writer's candidate fan, which consumes sweeps through a seam trait.

Verification is not uniform. Fourteen units are **differentially verified** by
`dev/verify_harness` (byte-for-byte against the real function under an
emulator); the rest is unit-pinned constants/vectors or real-install
measurement. Three stages are explicitly open: the cell-to-shape decoder
(`ow-tank::collision`), the sector-to-shape decoder (`ow-tank::chunkkind`), and
the brush tree->mesh conversion (`ow-tank::brush`; partly landed in
`brush_mesh`/`brush_convert`, which are outside this report).

---

## 1. Per module

### 1.1 `crates/ow-tank/src/collision.rs` — 950 lines

**What it does.** Reads the **pathing** chunk (`MAP_CHUNK_PATHING`, kind `0x12`)
as the client's cell container, and re-expresses the client's runtime cell
fixup. The module name is historical: the container is `STU_9199BF6B`, whose
`+0x68` field `m_cellBytes` holds a v1 array header
`{u32 count; u32 pad; u32 data_off}` over a block-RVA table of `0x10`-byte
entries; each cell holds `{u64 block_size; u64 record_off}` and each record is
17 self-relative qwords followed by 17 arrays. `tables()` reads the same
instance's other two array-bearing fields (`+0x08`'s nested array at `+0x10`,
and `+0x58`) and derives the three element strides **from** the exact tiling of
the blob (a span that does not divide by the count is an error, never a rounded
value). `fixup_cell_bytes()` is the read expression of
`MapChunkCellBytesFixup` (`0x141ff51a0`): every record qword `q` becomes
`record + q` (or stays `0` when `q` is zero), for the runtime array encodings
mode 1 and mode 2/3.

**Evidence class.** Container form = read (`StuLoadSubAssets` `0x1421d26d0`
compares the first dword with `0x53545544`; the loader resolves the header's
first RVA; `FinishMapChunkAsset` fixes the component in place). Fixup = read
instruction for instruction, pinned by `fn/MapChunkCellBytesFixup`. Strides =
**derived from an observed tiling**. Element *meaning* = declared not read.

**Public API**

```
pub const COMPONENT_CLASS: u32 = 0x9199_BF6B;
pub const CELL_FIELD_OFFSET: usize = 0x68;
pub const CELL_ARRAYS: usize = 17;
pub const CELL_FIRST_ARRAY: u64 = 0x88 + 0x18;
pub const NESTED_FIELD_OFFSET: usize = 0x10;
pub const ARRAY_FIELD_OFFSET: usize = 0x58;

pub enum Error { Truncated(&'static str), BadMagic, InstanceCount(u32),
                 BadInstance(i32), WrongClass(u32),
                 OutOfRange { cell: usize, what: &'static str, value: u64 },
                 BoundariesNotSorted { cell: usize },
                 FirstArrayTooLow { cell: usize, value: u64 },
                 NotTiled { cell: usize },
                 ArrayNotTiled { field: usize, count: u32, span: u64 } }
impl fmt::Display for Error; impl std::error::Error for Error;

pub struct Cell<'a> { pub index: usize, pub cell_offset: u64,
                      pub record_offset: u64, pub block_size: u64,
                      pub boundaries: [u64; CELL_ARRAYS],
                      pub arrays: [&'a [u8]; CELL_ARRAYS] }
pub struct Cells<'a> { pub blob_offset: usize, pub instance: usize,
                       pub class: u32, pub count: u32,
                       pub data_offset: u64, pub cells: Vec<Cell<'a>> }
pub fn parse(payload: &[u8]) -> Result<Cells<'_>, Error>

pub struct ArrayHeader { pub field: usize, pub header_offset: u64,
                         pub count: u32, pub data_offset: u64, pub stride: u64 }
impl ArrayHeader { pub fn end(&self) -> u64 }
pub struct Tables { pub blob_offset: usize, pub instance: usize,
                    pub nested: ArrayHeader, pub array: ArrayHeader,
                    pub cell_bytes: ArrayHeader }
pub fn tables(payload: &[u8]) -> Result<Tables, Error>

pub struct CellMemory<'a>;
impl<'a> CellMemory<'a> {
    pub fn new(base: u64, bytes: &'a mut [u8]) -> Self;
    pub fn read_u64(&self, addr: u64) -> Result<u64, FixupError>;
    pub fn read_u32(&self, addr: u64) -> Result<u32, FixupError>;
    pub fn write_u64(&mut self, addr: u64, value: u64) -> Result<(), FixupError>;
}
pub enum FixupError { OutOfBounds(u64), AllocationRequired(u64) }
pub fn fixup_cell_bytes(mem: &mut CellMemory<'_>, header: u64) -> Result<(), FixupError>
```

Private: `rd_u32`, `rd_u64`, `rd_i32`, `locate`, `read_header`, `tiled`.

**Doc-comment claims to check.** The client's allocation branches (header mode
0, entry mode 0) are rejected rather than modelled (`collision.rs:25-30`); the
ledger records them as *declared dead* (they allocate, then dereference the
zeroed allocation). The 17-boundary form and the tiling are measurements
(`collision.rs:31-35`). The loop is "a signed 32-bit counter compared with a
signed 32-bit count, exactly as the client's `CMP ESI, [RCX]` / `JGE` does"
(`collision.rs:576-577`).

### 1.2 `crates/ow-tank/src/collision_level.rs` — 233 lines

**What it does.** Parses a `MAP_CHUNK_COLLISION` (kind `0x11`) payload into the
client's own three mesh record arrays, using `chunkkind::Collision` for the
container. The `0x38` tail element is the level's collider: three RVAs and three
counts (`+0x18` nodes, `+0x1c` vertices, `+0x20` triangles), plus the default
layer mask at `+0x30`. It validates: `count_b == 1`, the three runs tile the body
at exactly `VERTEX_LEN 0x10` / `NODE_LEN 0x20` / `TRIANGLE_LEN 0x20`, the last
run ends where the sector array begins, the root node's box is finite and not
inverted, and every triangle corner indexes a real vertex. No conversion, no
rebuild — the client copies pointers, not data.

**Evidence class.** Read plus measured. The module doc prints the practice
range's numbers (`0x40` / `203,999` vertices; `0x31CE30` / `236,957` nodes;
`0xA581D0` / `343,474` triangles; strides `16.000`, `32.000`, `32.000`), the
first node's box `(-103.469, -38.510, -161.028) .. (206.159, 51.071, 148.317)`,
first triangle corners `120319, 120323, 120317`, mask `0x1FFF`.

**Public API**

```
pub struct LevelMesh { pub arrays: MeshArrays, pub words: MeshSource,
                       pub sectors: u32, pub root: ([f32; 3], [f32; 3]),
                       pub mask: u16 }
pub fn parse(payload: &[u8]) -> Result<LevelMesh, String>
```

Private: `mod tail { NODES 0x00, VERTICES 0x08, TRIANGLES 0x10, NODE_COUNT 0x18,
VERTEX_COUNT 0x1c, TRIANGLE_COUNT 0x20, MASK 0x30 }`, `u64_at`, `u32_at`,
`f32_at`. No unit tests in this file; exercised by
`crates/ow-tank/tests/collision_level_real.rs`.

The tail's field order is *not* memory order — `+0x00` is the node RVA and
`+0x08` the vertex RVA, while the body stores vertices first; the parser sorts
the runs by offset before tiling (`collision_level.rs:154-172`).

### 1.3 `crates/ow-tank/src/brush.rs` — 796 lines

**What it does.** Reads a **brush** — a closed ring of quantised 2-D points in XY
with a per-node direction flag — as the record layout the client's geometry
ingest writes: `0x20`-byte object header `{vertex ptr, node ptr, u32 vertex
count, u32 node count, u32 root}`, `0x20`-byte vertices
`{i64 x, i64 y, i64 divisor, u32 index}`, `0x28`-byte nodes
`{i32 prev, next, vertex, +0x0c, next_ring, x, y, prev_x, prev_y, u8 leaf,
u8 descending}`. It implements two read expressions exactly:
`Vertex::position` (the vertex decoder) and `build_from_instances` (the ring
builder, including the ordering pass and its equal-pair rejection).

**Evidence class.** Read, and bit-exact against the client's own bytes: the
vertex expectations in the unit test were produced by running `0x142ced510`
under `dev/brush_vertex_oracle.py`; the ring expectations came from
`dev/brush_mesh_oracle.py`. The doc names three independent readers of the
record shapes (vertex decoder `0x142ced510`, ring walk `0x142cf0360`, edge-list
builder `0x142ced1a0`) plus the single writer `0x142ced620`.

**Public API**

```
pub const HEADER_SIZE: usize = 0x20;
pub const VERTEX_SIZE: usize = 0x20;
pub const NODE_SIZE: usize = 0x28;
pub const QUANTISATION: f32 = 400.0;
pub const INSTANCE_LANES: usize = 2;

pub enum Error { Truncated { record: &'static str, need: usize, got: usize },
                 ZeroDivisor { index: usize },
                 BadIndex { node: usize, field: &'static str, value: i32 } }

pub struct Vertex { pub x: i64, pub y: i64, pub divisor: i64, pub index: u32 }
impl Vertex { pub fn parse(bytes: &[u8]) -> Result<Vertex>;
              pub fn position(&self) -> Option<[f32; 4]> }

pub struct Node { pub prev: i32, pub next: i32, pub vertex: i32,
                  pub next_ring: i32, pub x: i32, pub y: i32,
                  pub prev_x: i32, pub prev_y: i32,
                  pub leaf: u8, pub descending: u8 }
impl Node { pub fn parse(bytes: &[u8]) -> Result<Node>;
            pub fn order(&self) -> Option<std::cmp::Ordering> }

pub struct Brush { pub vertices: Vec<Vertex>, pub nodes: Vec<Node>, pub root: u32 }
impl Brush {
    pub fn parse(vertices: &[u8], nodes: &[u8], vertex_count: u32,
                 node_count: u32, root: u32) -> Result<Brush>;
    pub fn rings(&self) -> Vec<Vec<usize>>;
    pub fn positions(&self) -> Vec<Option<[f32; 4]>>;
}

pub fn quantise(value: f32, scale: f32) -> i32;
pub fn build_from_instances(instances: &[[f32; 4]], scales: [f32; 2]) -> Option<Brush>;
```

Private: `cvttss2si` (truncate toward zero; `i32::MIN` for NaN/out-of-range).

**Doc-comment claims.** The read expression is spelled out as
`((float)(v % d) * (1.0f / (float)d) + (float)(v / d)) * 0.0025f`
(`brush.rs:143-148`); `z` and `w` are always `0.0` because thickness arrives
later in `MeshToColliderFaces`; `None` is returned exactly where the client's
signed integer divide would fault. `quantise` is two separate 32-bit multiplies
in the client's order (`MULPS` scale first, `400.0` second). The builder's
rejection case (two consecutive equal points) zeroes the client's brush and
sets root `-1`, which this port reports as `None`.

### 1.4 `crates/ow-tank/src/model_collision.rs` — 207 lines

**What it does.** Encodes the identity of the model's collision descriptor `C`
(registered class instance reached as `[[[node+0x38]+0x40]+0x48]`) and the two
read expressions the client applies to it: the `+0x98` array slot with its
`+0xa0` tag (`ArrayRecord::points_and_count`, used twice inside `GeometryBuild`)
and the 14-byte flag-record fold at `+0x178`/`+0x186` (`flag_mask`, byte order
`6, 2, 13, 12, 8, 1, 3, 5, 4, 11, 10, 0, 9, 7`). It decodes no asset by itself.

**Evidence class.** Read: class registration (descriptor `0x143e74830`, `+0xa0`
size `0x198`, `+0xb8` type hash, field table `0x143e74790`, vtable
`0x1431dd118`, construct `0x1413354b0`, factory `0x141335610`); 22 of the
class's declared field hashes are compared by its own per-field dispatch
functions (`docs/formats/model-collision.md`).

**Public API**

```
pub const CLASS_HASH: u32 = 0x564b1018;   pub const CLASS_SIZE: u32 = 0x198;
pub const CLASS_VTABLE: u64 = 0x1431dd118;
pub const CLASS_DESCRIPTOR: u64 = 0x143e74830;
pub const CLASS_FIELD_TABLE: u64 = 0x143e74790;
pub const CLASS_CONSTRUCT: u64 = 0x1413354b0;
pub const CLASS_FACTORY: u64 = 0x141335610;
pub const OFF_ARRAY_SLOT: usize = 0x98;   pub const OFF_ARRAY_TAG: usize = 0xa0;
pub const OFF_FLAGS_A: usize = 0x178;     pub const OFF_FLAGS_B: usize = 0x186;
pub const OFF_BYTE_A: usize = 0x194;      pub const OFF_BYTE_B: usize = 0x195;

pub struct ArrayRecord { pub first: u64, pub second: u64, pub tag: u64 }
impl ArrayRecord { pub fn points_and_count(&self) -> (u64, u32);
                   pub fn is_empty(&self) -> bool }
pub fn flag_mask(record: &[u8; 14]) -> u16;
```

**Inconsistency to flag.** The module doc refers to `[instance_array]`
(`model_collision.rs:16`) as a documented item, but no item of that name exists —
the function is `ArrayRecord::points_and_count`. A broken intra-doc link, not a
behaviour gap.

### 1.5 `crates/ow-tank/src/mesh_shape.rs` — 192 lines

**What it does.** The read side of the model's prebuilt collision mesh
`STU_537E56E4`: the field offsets `0x141b53af0` unpacks, the
`{data pointer, u64 count}` 16-byte array header with its tag rule (header at
`pointer` when the tag is exactly `1`, at `pointer + 8` otherwise), the
`0x30`-byte `MeshSource` those unpack into, and `MeshShapeFill`'s 19-dword
`0x4c`-byte shape record (four scale lanes, four reciprocal lanes from a
`divss`, the three data pointers as low/high dword pairs, four copied words,
and one float multiplied by the scale).

**Evidence class.** Read (`0x141b53af0`, `MeshShapeFill` `0x142cae540`), plus
the field hashes the class declares. "There is no vertex or index conversion in
the client" (`mesh_shape.rs:27-29`).

**Public API**

```
pub const OFF_NODES: usize = 0x08;       pub const OFF_POINTS: usize = 0x18;
pub const OFF_TRIANGLES: usize = 0x28;   pub const OFF_NODE_COUNT: usize = 0x38;
pub const OFF_38B71436: usize = 0x3c;    pub const OFF_HEIGHT: usize = 0x40;
pub const OFF_5BD33E19: usize = 0x44;    pub const POINT_SIZE: usize = 16;
pub const RECORD_DWORDS: usize = 19;

pub fn header_offset(tag: u64) -> usize;
pub struct ArrayHeader { pub data: u64, pub count: u64 }
pub fn array_header(slot: [u64; 2], header: ArrayHeader) -> ArrayHeader;
pub struct MeshSource { pub nodes: u64, pub points: u64, pub triangles: u64,
                        pub node_count: u32, pub point_count: u32,
                        pub field_3c: u32, pub height: u32, pub field_44: f32 }
pub fn shape_record(source: &MeshSource, scale: f32) -> [u32; RECORD_DWORDS];
```

**Inconsistency to flag.** `array_header(slot, header)` discards its arguments
(`let _ = header_offset(slot[1]); header`) and returns `header` unchanged
(`mesh_shape.rs:66-68`). It is a placeholder for "apply the tag rule to a real
block"; the only thing it proves is `header_offset`.

### 1.6 `crates/ow-tank/src/mesh_world.rs` — 712 lines

**What it does.** Lays a mesh block's three arrays into one byte buffer behind
guest addresses and builds, per shape, the `MeshShapeFill` record the collision
query reads, plus the query block and shape transform the type-3 leaf reads. It
is the world image the type-3 walk and the differential fixtures use.

**Evidence class.** Read layout: the constant table in the module doc cites
`MeshSweep` (`0x142caed30`) for every stride and offset (`NODE_LEN 0x20`,
`NODE_MIN 0x00`, `NODE_MAX 0x0c`, `NODE_CHILD 0x18`, `NODE_MASK 0x1c`,
`NODE_AXIS 0x1e`, `NODE_LEAF 0x1f`, `TRIANGLE_LEN 0x20`, `TRIANGLE_CORNERS 0x00`,
`TRIANGLE_IDS 0x18`, `TRIANGLE_MASK 0x1c`, `VERTEX_LEN 0x10`), plus a second,
independent confirmation from the range's own data (model
`0x0d0000000000078c`: `288` node bytes / `m_nodeCount 9` = `0x20`; `384`
triangle bytes / the `0x38B71436` word `12` = `0x20`). The query block is read
from `0x142cd5540` and `0x142caed30`; the dispatcher `0x142cc45a0` copies `0x73`
bytes.

**Public API**

```
pub const NODE_LEN 0x20, NODE_MIN 0x00, NODE_MAX 0x0c, NODE_CHILD 0x18,
          NODE_MASK 0x1c, NODE_AXIS 0x1e, NODE_LEAF 0x1f;
pub const TRIANGLE_LEN 0x20, TRIANGLE_CORNERS 0x00, TRIANGLE_IDS 0x18,
          TRIANGLE_MASK 0x1c;
pub const VERTEX_LEN 0x10, RECORD_LEN 0x50, RECORD_WRITTEN 0x4c;
pub const QUERY_BLOCK_LEN 0x78, QUERY_START 0x00, QUERY_END 0x10,
          QUERY_MOTION 0x20, QUERY_SUPPORT_POINTS 0x30, QUERY_SUPPORT_COUNT 0x38,
          QUERY_ROTATION 0x40, QUERY_EXPANSION 0x50, QUERY_MASK 0x70,
          QUERY_FLAG_72 0x72;
pub const XFORM_LEN 0x20, XFORM_POSITION 0x00, XFORM_ROTATION 0x10;
pub const FAST_EXPANSION_LIMIT: f32 = 0.005;
pub const IDENTITY_ROTATION: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

pub struct MeshArrays { pub nodes: Vec<u8>, pub points: Vec<u8>,
                        pub triangles: Vec<u8> }
impl MeshArrays { pub fn node_count(&self) -> usize;
                  pub fn point_count(&self) -> usize;
                  pub fn triangle_count(&self) -> usize;
                  pub fn is_strided(&self) -> bool }

pub struct ShapeSlot { pub nodes: u64, pub points: u64, pub triangles: u64,
                       pub node_count: u32, pub point_count: u32,
                       pub field_3c: u32, pub height: u32, pub field_44: f32 }
impl ShapeSlot { pub fn to_source(&self) -> MeshSource }

pub struct MeshWorld { /* base, bytes, shapes */ }
impl MeshWorld {
    pub fn new(base: u64) -> Self;
    pub fn base(&self) -> u64;
    pub fn bytes(&self) -> &[u8];
    pub fn shapes(&self) -> &[ShapeSlot];
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    pub fn offset_of(&self, address: u64) -> Option<usize>;
    pub fn push(&mut self, arrays: &MeshArrays, words: &MeshSource) -> usize;
    pub fn arrays(&self, index: usize) -> MeshArrays;
    pub fn shape(&self, index: usize) -> &ShapeSlot;
    pub fn source(&self, index: usize) -> MeshSource;
    pub fn record(&self, index: usize, scale: f32) -> [u8; RECORD_LEN];
}

pub fn query_block(start: [f32; 3], end: [f32; 3], motion: f32,
                   expansion: f32, mask: u16) -> [u8; QUERY_BLOCK_LEN];
pub fn shape_xform(position: [f32; 3], rotation: [f32; 4]) -> [u8; XFORM_LEN];
pub fn node_bounds(node: &[u8]) -> ([f32; 3], [f32; 3]);
```

`MeshWorld::push` panics when an array is not a whole record count or a count
disagrees with the block's word (three `#[should_panic]` tests pin this). A
compile-time `const _: () = { assert!(...) }` block pins that every walk-read
field lies inside its record and that the written record fits the element
(`mesh_world.rs:150-157`).

**Doc-comment claim to check.** `[QUERY_FLAG_72]` is described as "(read by the
unread slow path only)" (`mesh_world.rs:118-120`), yet `query_block` writes `0`
there.

### 1.7 `crates/ow-tank/src/domino_world.rs` — 247 lines

**What it does.** Two bit encodings of the per-model collision body: the Domino
**world slot** (the `0x3fff` bound, the empty-slot sentinel, and the xor/add
encode-decode pair that are exact inverses mod 2^64) and the **body handle**
`((slot:u16) << 0x30) | ((generation:u16) << 0x20) | (index:u32)`, plus the
invalid handle's three-field test and the body-pool stride (`[world+0x288] +
index*0x1c0`, generation at `+0x198`).

**Evidence class.** Read (`OwnerCreateBody` `0x142cc3d90`, allocator
`0x142cc3c80`, the builder `0x14201ae60`'s guard at `0x14201ae9f..0x14201aecc`,
table `0x1442a0bc0`). Nothing here touches game data.

**Public API**

```
pub const WORLD_TABLE: u64 = 0x1442_0bc0;
pub const WORLD_SLOT_LIMIT: u16 = 0x3fff;
pub const EMPTY_SLOT: u64 = 0xc439_701c_d97e_c6aa;
pub const WORLD_KEY: u64 = 0x7760_0ba5_adcc_265c;
pub const WORLD_ADD_STORE: u64 = 0x4cd9_6477_2bb2_a04e;
pub const WORLD_ADD_LOAD: u64 = 0xb326_9b88_d44d_5fb2;
pub const WORLD_BODY_POOL: usize = 0x288;
pub const WORLD_BODY_STRIDE: usize = 0x1c0;
pub const BODY_GENERATION: usize = 0x198;
pub const INVALID_BODY_HANDLE: u64 = 0xffff_0000_ffff_ffff;

pub fn encode_world_pointer(world: u64) -> u64;
pub fn decode_world_pointer(entry: u64) -> u64;
pub fn is_empty_slot(entry: u64) -> bool;
pub fn slot_indexes_a_world(slot: u16) -> bool;
pub fn pack_body_handle(slot: u16, generation: u16, index: u32) -> u64;
pub fn body_handle_parts(handle: u64) -> (u16, u16, u32);
pub fn handle_world_slot(handle: u64) -> u16;
pub fn is_invalid_body_handle(handle: u64) -> bool;
pub fn body_entry_offset(index: u32) -> usize;
```

**Value worth a client cross-check.** `pub const WORLD_TABLE: u64 = 0x1442_0bc0;`
(`domino_world.rs:41-43`) while the doc comment on the same item says the table
is `0x1442a0bc0`. The constant as written is `0x0000_0000_1442_0bc0` — six hex
digits short of the documented address. It is only cited, never used by code,
but any reader comparing the two will see the discrepancy.

### 1.8 `crates/ow-tank/src/range_world.rs` — 535 lines

**What it does.** Assembles the practice range into one collision world: every
placed model's selected holder meshes (`STUModel+0x140` holder array, field
`0xCB4D298D`; embedded `STU_537E56E4` through holder field `0xB7C8314A`) pushed
into one `MeshWorld`, one `PlacedShape` per (placement, mesh holder), plus the
map's own `MAP_CHUNK_COLLISION` and additive-collision bodies at the origin. It
also exposes `placements()` (the map chunk's entry transforms) and
`holder_rows()` (every holder of every distinct model, with its shape mode, mask,
filter byte, whether it embeds a mesh, and the descriptor type the client would
build).

**Evidence class.** Read plus install-measured. The holder loop and the filter
are `0x14201ae60`'s (`docs/formats/static-collision.md`, checks 9 and 12);
`descriptor_scale` is `0x14201c434`; shape modes map through
`model_body::shape_for`.

**Public API**

```
pub const MESH_BLOCK: u32 = 0x537E_56E4;
pub const STU_MODEL: u32 = 0x871B_D3D0;
pub const WORLD_BASE: u64 = 0x0000_0001_0000_0000;

pub struct Placement { pub model: u64, pub translation: [f32; 3],
                       pub scale: [f32; 3], pub rotation: [f32; 4] }
pub struct PlacedShape { pub shape: usize, pub model: u64,
                         pub translation: [f32; 3], pub scale: [f32; 3],
                         pub rotation: [f32; 4], pub low: [f32; 3],
                         pub high: [f32; 3] }
impl PlacedShape { pub fn world_bounds(&self) -> ([f32; 3], [f32; 3]);
                   pub fn descriptor_scale(&self) -> f32 }

pub struct HolderRow { pub model: u64, pub shape_mode: i32, pub mode_byte: u8,
                       pub mask: u64, pub has_mesh: bool, pub selected: bool,
                       pub shape: Option<model_body::Shape> }

pub struct RangeWorld { pub mesh: MeshWorld, pub placed: Vec<PlacedShape>,
                        pub level_shapes: usize,
                        pub spawn_room: Option<mapresolve::SpawnRoom> }
impl RangeWorld {
    pub fn build(data: &GameData, sources: &Sources,
                 reg: &StuRegistry) -> Result<Self, String>;
    pub fn point_count(&self) -> usize;
}

pub fn placements(data: &GameData, sources: &Sources) -> Result<Vec<Placement>, String>;
pub fn holder_rows(data: &GameData, sources: &Sources,
                   reg: &StuRegistry) -> Result<Vec<HolderRow>, String>;
```

Private: `practice_scene`, `placements_from`, `u32_field`, `array_items`,
`bytes_field`, `pointbytes_field`, `block_arrays`, `holder_instances`,
`holder_mesh`, `field_u64`, `model_shapes`, `local_bounds`, `quat`.
No unit tests in this file.

**Behaviour to note.** Only shape mode `0` (mesh, descriptor type 3) becomes a
shape here; `holder_rows` reports the others but `RangeWorld::build` skips them
(`range_world.rs:389-396`). `RangeWorld::build` prints a line per level chunk to
stdout (`range_world.rs:496`) — an ingest side effect inside a library function.

### 1.9 `crates/ow-tank/src/chunkkind.rs` — 835 lines

**What it does.** Two jobs. (a) The client's `0x24`-entry chunk-kind table at
`0x1439aa320` (stride `0x20`, `{u64 slot; u64 id; u32 a; u32 b; char *name}`),
with the id at `+8` being the byte in GUID bits 32..39, the deobfuscation rule of
`0x1404c19b0` (length from the dword at `pointer - 4`, low 24 bits; name byte `i`
xored with byte `i & 7` of the eight bytes at `pointer - 0xC`), and the
`MAP_CHUNK_COLLISION = 0x11` / `MAP_CHUNK_PATHING = 0x12` /
`teMAP_CHUNK_ADDITIVE_COLLISION = 0x10` / `teMAP_CHUNK_ADDITIVE_PATHING = 0x23`
ids. (b) The collision payload container (`0x38` header; sector array at `+0x20`,
stride `0x58`; tail array at `+0x28`, stride `0x38`), the sector's four
`(offset, length)` runs, the tiling invariant, and the read expression of
`FUN_142e492a0` (`fixup_in_place`: header RVAs keep zero as zero, every element
qword gets `base` added whatever it holds).

**Evidence class.** Read for the table, names, header and fixup. **Observed**
for the tiling and for the meaning of the three counts at `+0x2C`/`+0x30`/`+0x34`
— the module says so explicitly. No unit tests in this file.

**Public API**

```
pub const KIND_TABLE_ADDRESS: u64 = 0x1_439a_a320;
pub const KIND_NAMES_DECODER: u64 = 0x1_404c_19b0;
pub const CHUNK_GUID_PREFIX: u64 = 0x0DD0_0000_0000_0000;
pub const KIND_COLLISION: u8 = 0x11;          pub const KIND_PATHING: u8 = 0x12;
pub const KIND_SOUNDAREA: u8 = 0x13;
pub const KIND_DYNAMIC_SPAWN_GROUPS: u8 = 0x1B;
pub const KIND_ADDITIVE_COLLISION: u8 = 0x10;
pub const KIND_ADDITIVE_PATHING: u8 = 0x23;
pub const KIND_STREAMING: u8 = 0x27;

pub struct Entry { pub slot: u8, pub id: u8, pub raw_10: u32, pub raw_14: u32,
                   pub name: &'static str }
pub const TABLE: [Entry; 0x24];
pub fn entry_for_kind(kind: u8) -> Option<&'static Entry>;
pub fn slot_for_kind(kind: u8) -> Option<u8>;
pub fn kind_of_guid(guid: u64) -> u8;
pub fn kind_name(guid: u64) -> Option<&'static str>;

pub const SECTOR_SIZE: usize = 0x58;   pub const TAIL_SIZE: usize = 0x38;
pub const HEADER_SIZE: usize = 0x38;   pub const SECTOR_POINTERS: usize = 4;
pub const TAIL_POINTERS: usize = 3;

pub enum Error { Truncated(&'static str), NegativeCount { field: usize, value: i32 },
                 OutOfRange { what: &'static str, value: u64 },
                 SectorNotTiled { sector: usize } }

pub struct Header { pub unknown_00: u64, pub count_a: i32, pub count_b: i32,
                    pub rva_10: u64, pub rva_18: u64, pub rva_20: u64,
                    pub rva_28: u64, pub unknown_30: u64 }
pub struct Collision<'a> { pub data: &'a [u8], pub header: Header }
impl<'a> Collision<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Collision<'a>>;
    pub fn sectors(&self) -> Sectors<'a>;
    pub fn tails(&self) -> Tails<'a>;
    pub fn run_bytes(&self, run: (u64, u64)) -> Option<&'a [u8]>;
    pub fn check_sector_tiling(&self) -> Result<()>;
}
pub struct Sector<'a> { pub index: usize, pub raw: &'a [u8], pub pointers: [u64; 4] }
impl<'a> Sector<'a> { pub fn counts(&self) -> [u32; 3];
                      pub fn runs(&self) -> [(u64, u64); 4];
                      pub fn vertices(&self, payload: &[u8]) -> Vec<[f32; 3]> }
pub struct Sectors<'a>;  // Iterator<Item = Sector<'a>>
pub struct Tail<'a> { pub index: usize, pub raw: &'a [u8], pub pointers: [u64; 3] }
pub struct Tails<'a>;    // Iterator<Item = Tail<'a>>
pub fn fixup_in_place(data: &mut [u8], base: u64) -> Result<()>;
```

Private: `u32_at`, `i32_at`, `u64_at`, `set_u64`, `Header::read`, `tiled`.

**Notes.** The dispatch split is stated in the module doc: `FinishMapChunkAsset`
(`0x141ff54e0`) sends slots `0x1D` **and** `0x21` to `FUN_142e492a0`, and slots
`0x1E`/`0x22` to the cell path. `Sector::vertices` is explicitly "a convenience
and not part of the parse" (`chunkkind.rs:666-669`).

### 1.10 `crates/ow-sim/src/collide.rs` — 4576 lines

**What it does.** The read swept-collision stack, in five sections.

1. **Bounds stage** (`0x142cd5540`): `rotation_rows`, `transform_point`,
   `sweep_bounds` — builds the rotation from the block's local quaternion,
   transforms the local support points, folds them, and writes `start`, `end`,
   `expansion` (`start = P0 + (max+min)*0.5`). Op for op: `2q` as `ADDPS`, the
   diagonal as `(1 - |2y^2|) - |2z^2|`, the point sum
   `(px*mx + py*my) + pz*mz`, the fold as `MINPS`/`MAXPS` with "second operand
   wins on NaN".
2. **Query driver** (`0x142cbd510`): `PassGeometry` + `pass_geometry`,
   `aabb_reject` (sign-bit OR of the two difference quads, lanes 0..2),
   `moving_box_reject` (SAT with the `0.005` separation constant), `query_driver`
   (four passes, worklist popped from the end, two folds: the per-lane running
   minimum in `XMM6` and the comparison operand at `[RBP-0x40]` that only moves
   on an all-lane hit). The seam is the `QueryWorld` trait.
3. **Narrowphases**: `sphere_sweep` (type 0, `0x142ccdf40`), `capsule_sweep`
   (type 1, `0x142ccd7b0`), the type-3 mesh walk `mesh_sweep`
   (`0x142caed10` wrapper / `0x142caed30` body) with its support-point hull
   (`support_hull_affine`/`support_hull`), `mesh_triangle` (`0x142cce430`,
   analytic path vs. GJK handoff), `box_triangle_overlap` (`0x142cd58a0`) and
   `mesh_gather` (`0x142caea70`).
4. **Filter / transform / dispatch**: `shape_filter` (`0x142cbb350`),
   `shape_world_transform` + `compose_pose` (`0x142cb8b80`), `shape_narrowphase`
   (`0x142cc1610`, with `Narrowphase::{Filtered, Rejected, Hit, UnreadType}`).
5. **Closest-point GJK** (`0x142cc9460`): `Simplex`/`SimplexVertex` byte
   layouts, `simplex3` (`0x142cc8d80`), `simplex4` (`0x142cc7740`),
   `reduce_segment`, `simplex_point`, `gjk_rotation_rows`, `closest_point`.

`triangle_degenerate` (`0x142cd56e0`) sits between sections 3 and 5 and is the
per-triangle skip test `collider` also uses.

**Evidence class.** Read, and for most of it **differentially verified**:
`fn/CollisionSweepBounds`, `fn/QueryDriver`, `fn/CollisionSphere`,
`fn/CollisionCapsule`, `fn/CollisionShapeFilter`, `fn/BoxTriangleOverlap`,
`fn/MeshGather`, `fn/MeshSweep`, `fn/ClosestPointGjk`, `fn/ShapeNarrowphase`,
`fn/TriangleDegenerate`, `fn/ShapeWorldTransform` — see §3c for coverage and
mutant counts. Each verified unit carries its own `acknowledged_gaps` list; the
collision-relevant ones are reproduced in §5.

**Public API** (signatures verbatim from the source)

```
pub type Quad = [f32; 4];

pub fn rotation_rows(q: Quad) -> [Quad; 3];
pub fn transform_point(p: [f32; 3], rows: &[Quad; 3]) -> Quad;
pub fn sweep_bounds(block: &[u8], points: &[u8]) -> (Quad, Quad, Quad);

pub struct PassGeometry { pub start: Quad, pub delta: Quad, pub epsilon: Quad,
                          pub low: Quad, pub high: Quad }
pub fn pass_geometry(start: Quad, end: Quad, motion: Quad) -> PassGeometry;
pub fn aabb_reject(geometry: &PassGeometry, entry_min: Quad, entry_max: Quad,
                   expansion: Quad) -> bool;            // true = rejected
pub fn moving_box_reject(geometry: &PassGeometry, entry_min: Quad,
                         entry_max: Quad, expansion: Quad) -> bool;
pub fn query_driver(pass_roots: [i32; 4], block: &[u8], points: &[u8],
                    flags: u8, world: &mut dyn QueryWorld) -> Quad;
pub trait QueryWorld {
    fn entry(&self, pass: usize, index: i32) -> Option<&[u8]>;
    fn narrowphase(&mut self, pass: usize, id: i32, hit: &mut Quad) -> i32;
}

pub const SPHERE_RECORD_LEN: usize = 0x20;
pub const SHAPE_XFORM_LEN: usize = 0x20;
pub const CAPSULE_RECORD_LEN: usize = 0x30;
pub const MESH_RECORD_LEN: usize = 0x50;
pub const MESH_NODE_LEN: usize = 0x20;
pub const MESH_TRIANGLE_LEN: usize = 0x20;
pub const MESH_VERTEX_LEN: usize = 0x10;
pub const SIMPLEX_VERTEX_LEN: usize = 0x50;
pub const SIMPLEX_BYTES_LEN: usize = 0x150;

pub fn sphere_sweep(record: &[u8], block: &[u8], xform: &[u8]) -> Option<[Quad; 3]>;
pub fn capsule_sweep(record: &[u8], block: &[u8], xform: &[u8]) -> Option<[Quad; 3]>;

pub struct MeshWorld<'a> { pub base: u64, pub bytes: &'a [u8] }
pub struct MeshHit { pub point: Quad, pub normal: Quad, pub fraction: Quad,
                     pub triangle: i32, pub ids: [u16; 2] }
pub struct MeshSweep { pub hit: Option<MeshHit>, pub internals: i32, pub leaves: i32 }

pub fn box_triangle_overlap(box8: &[u8], p0: &[u8], p1: &[u8], p2: &[u8]) -> u64;
pub fn mesh_gather(record: &[u8], world: &MeshWorld<'_>, box8: &[u8],
                   mask: u16) -> Vec<u32>;
pub fn mesh_sweep(record: &[u8], world: &MeshWorld, block: &[u8],
                  xform: &[u8]) -> MeshSweep;

pub fn shape_filter(a: &[u8], b: &[u8]) -> bool;
pub fn shape_world_transform(entry: &[u8], record: &[u8], owners: &[u8],
                             poses: &[u8]) -> [Quad; 2];

pub enum Narrowphase { Filtered, Rejected, Hit, UnreadType(i8) }
pub struct NarrowphaseWorld<'a> { pub record: &'a [u8], pub entries: &'a [u8],
                                  pub owners: &'a [u8], pub poses: &'a [u8],
                                  pub filter: &'a [u8],
                                  pub pools: [&'a [u8]; 5],
                                  pub mesh: Option<MeshWorld<'a>> }
pub fn shape_narrowphase(world: &NarrowphaseWorld, block: &[u8], shape: i32,
                         out: &mut Quad) -> Narrowphase;

pub fn triangle_degenerate(a: Quad, b: Quad, c: Quad) -> u32;   // 1 = skip

pub struct SimplexVertex { pub dw: Quad, pub p: Quad, pub w: Quad,
                           pub bary: Quad, pub index: [i32; 2], pub pad: [u8; 8] }
pub struct Simplex { pub vertex: [SimplexVertex; 4], pub count: i32 }
impl Simplex { pub fn from_bytes(bytes: &[u8]) -> Simplex;
               pub fn write_to(&self, bytes: &mut [u8]); }
pub fn simplex3(out: &mut Quad, s: &mut Simplex) -> bool;
pub fn simplex4(out: &mut Quad, s: &mut Simplex) -> bool;

pub struct GjkShape { pub radius: Quad, pub translation: Quad, pub rotation: Quad }
pub struct GjkInput<'a> { pub a: &'a [Quad], pub b: &'a [Quad],
                          pub shape_a: GjkShape, pub shape_b: GjkShape,
                          pub direction: Quad, pub motion: Quad,
                          pub flag0: u8, pub flag1: u8 }
pub struct GjkOutput { pub point: Quad, pub normal: Quad, pub residual: Quad,
                       pub iterations: i32 }
pub fn closest_point(input: &GjkInput<'_>, out: &mut GjkOutput) -> bool;
```

Private helpers that matter to a comparison: `abs_lane`, `rot3` (`SHUFPS
0xc9`), `rot3_inverted` (`0xd2`), `shufps`, `xor_mask`, `and_mask`, `dot3`,
`all_lanes`, `per_lane_sqrt`, `length_sq`, `rotate_to_local`,
`rotate_to_world_point`, `rotate_to_world_normal`, `quad_min`/`quad_max`/
`quad_abs`, `sign_bit`, `lt_mask`, `cross`, `support_hull_affine`,
`support_hull`, `mesh_triangle`, `gjk_rotation_rows`, `gjk_transform`,
`reduce_segment`, `simplex_point`, `abs16`, `eps`, `eps_parallel`, `splat`,
`sse_all`. Named constants: `DIAG_BASE`, `HALF`, `QUERY_EPSILON`,
`QUERY_SEPARATION`, `QUERY_HALF`, `ZERO_QUAD`, `EDGE_EPS`, `CONJUGATE`,
`PRODUCT_MASK_Z`/`X`/`Y`, `KEEP_LOW_THREE`, `ONE`.

**Module-doc claims a reviewer should test.** (a) "Both are implemented and
verified" for the two triangle paths (`collide.rs:63-68`); (b) the two-fold
description of the driver's running minimum vs. comparison operand
(`collide.rs:397-405`); (c) "The client does not bounds-check the shape index or
the pool index; neither is a state it can be in, so an out-of-range index reports
[`Narrowphase::Rejected`] rather than reading past the fixture"
(`collide.rs:1973-1976`) — a deliberate deviation from the client's
out-of-bounds behaviour, justified as unreachable.

### 1.11 `crates/ow-sim/src/collider.rs` — 997 lines

**What it does.** The collider-mesh build chain, in the client's order: `shell`
(`MeshToColliderFaces` `0x142cf2010`: every vertex twice, z raised/lowered by
epsilon, indices in three blocks — outward, inward reversed, then stitching pairs
only when `epsilon >= 0.005`), `faces` (`ColliderFaceAppend` `0x142cca460`:
`0x60`-byte face records, degenerate triangles skipped, area accumulated),
`build` (BVH `0x142ccb1b0`: the clustered half-split when every centroid extent
is below `0.005`, else the 64-bin surface-area split) and `finalize`
(`0x142ccad80`: `0x20`-byte runtime nodes, `0x10`-stride vertices with `w`
forced to zero, `0x20`-byte triangles, every box padded by at least `0.01`).
`build_source` / `build_source_with_ids` chain them.

**Evidence class.** Read; `fn/BuildBvh` is verified (100% coverage, 10/11
mutants). No unit in `dev/verify_harness/units` covers `shell`, `faces` or
`finalize`.

**Public API**

```
pub const FACE_LEN: usize = 0x60;       pub const BUILD_NODE_LEN: usize = 0x50;
pub const RUNTIME_NODE_LEN: usize = 0x20; pub const TRIANGLE_LEN: usize = 0x20;
pub const VERTEX_LEN: usize = 0x10;     pub const SOURCE_LEN: usize = 0x30;
pub const LEAF_SIZE: usize = 4;

pub struct SourceFace { pub index: [i32; 3], pub present: [bool; 3] }
pub struct ShellIndex { pub index: [i32; 3], pub mode: u16, pub a: u16, pub b: u16 }
pub struct Shell { pub vertices: Vec<[f32; 4]>, pub indices: Vec<ShellIndex> }
pub struct Face { pub min: [f32; 4], pub max: [f32; 4], pub centroid: [f32; 4],
                  pub marker: i32, pub area: f32, pub index: [i32; 3],
                  pub mode: u16, pub a: u16, pub b: u16, pub source: usize,
                  pub bucket: i32, pub id_bits: u16 }
impl Face { pub fn width(&self) -> [f32; 3] }
pub struct BuildNode { pub min: [f32; 4], pub max: [f32; 4], pub first_face: u32,
                       pub axis: i32, pub first_child: u32, pub second_child: u32,
                       pub face_count: u32, pub height: u32, pub id_bits: u16 }
pub struct ColliderMesh { pub nodes: Vec<u8>, pub vertices: Vec<u8>,
                          pub triangles: Vec<u8>, pub build_node_count: u32,
                          pub vertex_count: u32, pub face_count: u32,
                          pub area: f32 }

pub fn shell(source_vertices: &[[f32; 4]], source_faces: &[SourceFace],
             epsilon: f32, id: u16) -> Shell;
pub fn faces(vertices: &[[f32; 4]], indices: &[ShellIndex]) -> (Vec<Face>, f32);
pub enum Unread { EmptyBinSplit }
pub fn build(faces: &[Face]) -> Result<(Vec<BuildNode>, Vec<Face>), Unread>;
pub fn write_build_nodes(nodes: &[BuildNode], face_base: u64, out: &mut [u8]);
pub fn finalize(nodes: &[BuildNode], faces: &[Face], vertices: &[[f32; 4]],
                area: f32) -> ColliderMesh;
pub fn build_source(source_vertices: &[[f32; 4]], source_faces: &[SourceFace],
                    epsilon: f32) -> Option<ColliderMesh>;
pub fn build_source_with_ids(source_vertices: &[[f32; 4]],
                             source_faces: &[SourceFace], epsilon: f32,
                             tag: impl Fn([i32; 3]) -> ShellIndex) -> Option<ColliderMesh>;
```

Private: `skip_triangle`, `triangle_double_area`, `box_area`, `build_node`,
`widest_axis`, `struct Bin`, `bin_index`, `bin_split`, `flatten`, `put_f32`, and
the `*_quad`/`minss`/`maxss_bits` lane helpers. Constants
`SHELL_THRESHOLD = 0.005` (`0x142f897c4`), `HALF = 0.5` (`0x142f80ff8`),
`BOX_PADDING = 0.01` (`0x14311e0c0`), `BIN_COUNT = 64`, `BIN_SCALE = 64.0`
(`0x14305af0c`).

**Doc/code inconsistency to flag.** The `build` doc says the bin-split path
"returns [`Unread::BinCost`]" (`collider.rs:319-321`) but `enum Unread` declares
only `EmptyBinSplit` (`collider.rs:302-306`). The bin split *is* implemented
(`bin_split`, `collider.rs:516`); the only error produced is `EmptyBinSplit`,
when no boundary leaves faces on both sides. A reader comparing doc against code
will be misled; the code, not the doc, is what runs. `build_source_with_ids`
also swallows any `Unread` with `.ok()?` and reports `None`, indistinguishable
from "the source produced no faces".

### 1.12 `crates/ow-sim/src/fan.rs` — 1697 lines

**What it does.** The read parts of the wish writer `FUN_140cb86c0`
(`0x140cb86c0`): the seed (`s = dot(position - M*wish, dir)` plus the state term
plus the `+0.1` bias), the fan's speed and radius, the candidate rotations
(pentagon 72-degree step, `2 pi / N` ring step), the four acceptance rules
(`accepts_pass1`, `accepts_pass2`, `accepts_ring`, `accepts_refine`), the
refinement over five pentagon directions, the back-transform direction and its
`-normal` sibling, the back-snap, pass 0's two-mask probe, the memo test, the
endpoints, the projection, the reflection flags, and the composition `solve`.

**Collision relevance.** It is a *consumer* of the collision query; it parses no
geometry. The seam is
`pub trait CandidateSolver { fn evaluate(&mut self, a: [f32; 4], b: [f32; 4]) -> Option<SweepHit>; }`
— the evaluator `0x1420328c0` is **not** implemented here.

**Evidence class.** Read, with every constant citing the instruction that loads
it. The module doc states the whole writer's stage order was read on 2026-11-07
and that the composition still implements the **earlier** reading of that order.

**Public API**

```
pub const FLAGS_BLOCK 0x10d0, WISH 0x1140, VELOCITY 0x1150, TURN_LIMIT 0x5c8,
          PASS_ENABLED 0x659, SPEED_CAP_CLEAR [0x5d0,0x5d4,0x5d8],
          SPEED_CAP_SET [0x5dc,0x5e0,0x5e4], FRAME_COUNTER 0xe8,
          SPEED_FLOOR 0x5cc, RADIUS_SCALE 0x5b8, RADIUS_INPUT 0x65c,
          FAN_COUNT 0x5bc, ACTIVE 0x610;
pub const SEED_BIAS 0.1, SPEED_GAIN 1.1, FRAME_SECONDS 1e-6,
          TURN_LIMIT_MAX 1.570_784_4, DEG_TO_RAD 0.017_453_292,
          RADIUS_MARGIN 0.015, RADIUS_FLOOR 0.35,
          LENGTH_EPSILON from_bits(0x0080_0000),
          PENTAGON_COS 0.309_016_97, PENTAGON_SIN 0.951_056_54, TWO_PI 6.283_185_5,
          BATCH_TAIL_SNAP_MAX_KIND 1, REFINE_EPSILON 1.175_494_4e-36,
          REFINE_STEP 0.02, REFINE_DIR 0.005, REFINE_GROUND_DROP 0.04,
          MEMO_TOLERANCE 0.004, SNAP_DIVIDE_FLOOR from_bits(0x38fa_0000),
          PASS_ZERO_MASKS [0x01, 0x0e];

pub fn turn_limit(body: &[u8]) -> f32;
pub fn speed_cap(body: &[u8], flag_bit_set: bool) -> f32;
pub fn frame_time(body: &[u8]) -> f32;
pub fn fan_speed(body: &[u8], flag_bit_set: bool, helper_e: f32) -> f32;
pub fn fan_radius(body: &[u8]) -> f32;
pub fn seed(body: &[u8], position: [f32; 4], matrix: Option<&[f32; 16]>) -> f32;
pub fn seed_state_term(body: &[u8], position: [f32; 4], ramp: [f32; 4],
                       query: [f32; 4]) -> f32;
pub fn biased_seed(seed: f32) -> f32;
pub fn rotate_pair(pair: [f32; 2], cos: f32, sin: f32) -> [f32; 2];
pub fn pentagon_offsets(start: [f32; 2]) -> [[f32; 2]; 5];
pub fn ring_step(body: &[u8]) -> Option<f32>;
pub fn ring_offsets(start: [f32; 2], n: usize) -> Vec<[f32; 2]>;
pub fn accepts_pass1(score: f32, y: f32, best_score: f32, best_y: f32) -> bool;
pub fn accepts_pass2(score: f32, best_score: f32) -> bool;
pub fn accepts_ring(alignment: f32, threshold: f32, gate: bool) -> bool;
pub fn accepts_refine(facing: f32, best_y: f32, flag_bit_0x10000: bool) -> bool;
pub fn refine_pentagon(center: [f32; 4], start: [f32; 2], refined: [f32; 4],
                       dir: [f32; 4], best_y: f32, flag_bit_0x10000: bool,
                       solver: &mut dyn CandidateSolver) -> Option<SweepHit>;
pub enum BatchTail { /* Snap | Refine */ }
pub fn batch_tail(kind: i32) -> BatchTail;
pub fn horizontal_unit(v: [f32; 4], w: f32) -> Option<[f32; 4]>;
pub struct RingSweep { /* count, radius, step, ... */ }
pub fn ring_sweep(count: u32, configured_radius: f32, radius_scale: f32) -> Option<RingSweep>;
pub fn ring_offset(pair: [f32; 2], radius: f32) -> [f32; 4];
pub fn shape_ring_offset(pair: [f32; 2], seed: f32) -> [f32; 4];
pub fn corner_pair(centre: [f32; 4], half_x: f32, half_y: f32)
                   -> ([f32; 4], [f32; 4]);
pub fn shape_probes(centre: [f32; 4], min: [f32; 4], far: [f32; 4])
                    -> [[f32; 4]; 5];
pub fn pass_zero(mut probe: impl FnMut(u8) -> Option<SweepHit>) -> Option<SweepHit>;
pub fn back_direction(descriptor: [f32; 4]) -> [f32; 4];
pub fn refine_direction(normal: [f32; 4], descriptor_w: f32,
                        fallback: [f32; 4]) -> [f32; 4];
pub fn refined_offset(pentagon: [f32; 2], refined: [f32; 4]) -> [f32; 4];
pub fn refined_sweep(center: [f32; 4], pentagon: [f32; 2], refined: [f32; 4])
                     -> ([f32; 4], [f32; 4]);
pub struct SweepHit { pub score: f32, pub normal: [f32; 4], pub word: u32 }
pub fn memo_matches(cached: [[f32; 3]; 2], candidate: [[f32; 3]; 2]) -> bool;
pub fn facing_threshold(body: &[u8]) -> f32;
pub fn endpoints(point: [f32; 4], dir: [f32; 4], dir_w: f32, seed: f32,
                 speed: f32) -> ([f32; 4], [f32; 4]);
pub fn projection(point: [f32; 4], dir: [f32; 4]) -> f32;
pub fn project_back(cand: [f32; 4], dir: [f32; 4], dir_w: f32) -> ([f32; 4], f32);
pub fn back_snap(out: [f32; 4], normal: [f32; 4], start: [f32; 4],
                 end: [f32; 4]) -> Option<[f32; 4]>;
pub fn reflection_flags(word: u32) -> (bool, bool);
pub struct SolveContext { pub point: [f32; 4], pub dir: [f32; 4], pub dir_w: f32,
                          pub seed: f32, pub speed: f32,
                          pub ring: Option<RingSweep> }
pub struct SolvedWish { pub direction: [f32; 4], pub normal: [f32; 4],
                        pub score: f32, pub reflection: (bool, bool),
                        pub refined: bool }
pub fn solve(body: &[u8], ctx: &SolveContext,
             solver: &mut dyn CandidateSolver) -> Option<SolvedWish>;
pub trait CandidateSolver {
    fn evaluate(&mut self, a: [f32; 4], b: [f32; 4]) -> Option<SweepHit>;
}
pub fn facing(dir: [f32; 4], normal: [f32; 4]) -> f32;
```

### 1.13 `crates/ow-sim/src/walk.rs` — 744 lines

**What it does.** The character's step: a uniform grid broadphase over the
placed mesh shapes, one verified `collide::mesh_sweep` per candidate shape with
the no-support-point query block and the layer mask `0xffff`, the smallest hit
fraction wins, then the fold (travel the fraction, project the residual onto the
contact plane), the wall/floor split by the contact normal's up lane, the contact
skin re-seat, and a downward floor snap.

**Evidence class.** Mixed by design. The motion (`MoverAdvance` `0x140ca7f30`,
`SetCharacterControllerVelocity` `0x142cbd1b0`) and the query
(`0x142cd5540`/`0x142caed30`/`0x142cce430`/`0x142cc9460`, `0x142cbd510`) are
read; **the slide and the slope limit are not**, and the module says so.

**Public API**

```
pub const MESH_MASK: u16 = 0xffff;
pub const QUERY_MOTION: f32 = 1.0;
pub const QUERY_START 0x00, QUERY_END 0x10, QUERY_MOTION_AT 0x20,
          QUERY_SUPPORT_COUNT 0x38, QUERY_EXPANSION 0x50, QUERY_MASK 0x70,
          QUERY_BLOCK_LEN 0x80;
pub const XFORM_POSITION 0x00, XFORM_ROTATION 0x10, XFORM_LEN 0x20;
pub const SLIDE_ITERATIONS: usize = 3;
pub const CONTACT_SKIN: f32 = 0.005;
pub const WALKABLE_SLOPE: f32 = 0.7;
pub const CELL: f32 = 8.0;

pub fn query_block(start: [f32; 3], end: [f32; 3], radius: f32, mask: u16)
                   -> [u8; QUERY_BLOCK_LEN];
pub fn shape_xform(position: [f32; 3], rotation: [f32; 4]) -> [u8; XFORM_LEN];
pub struct Shape { pub record: Vec<u8>, pub xform: [u8; XFORM_LEN],
                   pub low: [f32; 3], pub high: [f32; 3] }
pub struct Sweep { pub fraction: f32, pub normal: [f32; 3],
                   pub point: [f32; 3], pub shape: usize }
pub struct World { /* base, bytes, shapes, cells */ }
impl World {
    pub fn new(base: u64, bytes: Vec<u8>, shapes: Vec<Shape>) -> World;
    pub fn shape_count(&self) -> usize;
    pub fn byte_len(&self) -> usize;
    pub fn extent(&self) -> ([f32; 3], [f32; 3]);
    pub fn sweep(&self, start: [f32; 3], end: [f32; 3], radius: f32) -> Option<Sweep>;
    pub fn floor(&self, at: [f32; 3], radius: f32, reach: f32) -> Option<Sweep>;
}
pub struct Advanced { pub pos: [f32; 3], pub grounded: bool,
                      pub contact: Option<[f32; 3]>, pub contacts: u32,
                      pub travelled: f32 }
pub fn advance(world: &World, pos: [f32; 3], delta: [f32; 3], radius: f32) -> Advanced;
pub fn snap_to_floor(world: &World, pos: [f32; 3], radius: f32,
                     reach: f32) -> Option<[f32; 3]>;
```

**Tuning constants a reviewer should question** (all documented as fitted, not
read): `WALKABLE_SLOPE = 0.7`, `SLIDE_ITERATIONS = 3`, `CONTACT_SKIN = 0.005`,
`CELL = 8.0`, plus the hard-coded up-lane thresholds `hit.normal[1] > 0.5` in
`World::floor` (`walk.rs:282`) and `n[1] > 0.5` in `advance` (`walk.rs:382`).
`advance` also clamps `hit.fraction` into `[0, 1]` and breaks on
`length(remaining) <= 1e-6` (`walk.rs:328-395`); none of these is a read
expression.

### 1.14 `crates/ow-sim/src/policy.rs` — 856 lines

**What it does.** The narrowphase's hit callback `0x14202bc90`: the world gate,
the layer/exclusion screen `0x1420af950`, the optional filter, the mode switch
(mode 0 = presence test, mode 1 = fill the `0x50`-byte answer record, mode 2+ =
the classifier), the geometry key decode chain (including the obfuscated
constants and the in-place decode `0x140543440`), the negative-id variant
resolution `0x141b3c890`, and `PolicySolver` — a `CandidateSolver` that answers
the fan with the policy's decision instead of running a sweep.

**Evidence class.** Read, with the geometry object supplied by a trait because a
real map needs the cell-to-shape ingest, which is open.

**Public API**

```
pub const NO_HIT: u8 = 1;  pub const ACCEPTED: u8 = 2;  pub const REJECTED: u8 = 3;
pub const KEY_UNSET: u16 = 0xffff;  pub const ID_PENDING: u16 = 1;

pub struct PackedShape { /* descriptor fields, Default */ }
pub struct HitRecord { /* 0x50-byte answer record */ }
pub struct HitFilter<'a> { /* required/forbidden masks + excluded worlds */ }
pub struct Hit<'a> { /* narrowphase hit object */ }
pub trait Geometry { /* table, slot word, index base, variant table */ }
pub trait VariantLookup { /* 0x141b3c890(table, &out, index) */ }
pub struct World<'a> { /* mask + geometry */ }

pub fn decode_geometry_pointer(raw: u64) -> u64;
pub fn dead_presence_term(slot_word: u64) -> u64;
pub fn geometry_key(geometry: &dyn Geometry) -> u64;
pub fn dead_key(geometry: &dyn Geometry) -> u64;
pub fn passes_layer_filter(filter: &HitFilter, world: &World) -> bool;
pub fn candidate_hit_policy(/* signature at policy.rs:331 */) -> u8;
pub struct PolicySolver<'a> { /* world, packed, ctx, mode */ }
impl<'a> PolicySolver<'a> {
    pub fn new(world: Option<&'a World<'a>>, packed: PackedShape, ctx: u64,
               mode: u32) -> Self;
}
impl CandidateSolver for PolicySolver<'_> {
    fn evaluate(&mut self, _a: [f32; 4], _b: [f32; 4]) -> Option<SweepHit>;
}
```

Private: `ror64`, `geometry_key_of`, `resolve_id`, and the key-decode constants
`KEY_ADD`, `KEY_XOR`, `KEY_ROT`, `KEY_ROT_INNER`, `ENTRY_ADD`, `ENTRY_XOR`,
`ENTRY_SUB`, `ENTRY_ROT`, `DECODE_XOR`, `DECODE_SUB`, `H_ROT`.

**Notable finding the module itself records.** The first call site's presence
term is `(word & 0x0f) >> 4`, always zero, so the dedupe scan behind it is
unreachable in this build; `dead_presence_term` keeps the formula so a test can
prove it never fires (`policy.rs:17-27`, `policy.rs:254-262`).

---

## 2. Ghidra addresses cited in source comments/doc-comments

Every address below appears in a comment or doc comment of the reviewed modules.
"What the comment says it does" is the source's own claim, not a verified
restatement. Addresses are current-image (`0x140000000`-based).

### 2.1 `ow-tank/src/collision.rs`

| address | name in the comment | what the comment says |
|---|---|---|
| `0x141ff51a0` | `MapChunkCellBytesFixup` | converts each record's 17 self-relative qwords to pointers, in place (`collision.rs:22`, `:578`) |
| `0x1421d26d0` | `StuLoadSubAssets` | compares the first dword with `0x53545544` (`collision.rs:192-194`) |
| `0x140f8f310` | `CollisionChunkComponentDtor` | releases exactly the sub-object at `+0x10` of the `+0x08` field, so that is where the nested array lives (`collision.rs:355-358`) |

### 2.2 `ow-tank/src/collision_level.rs`

| address | name | what the comment says |
|---|---|---|
| `0x142caed30` | the type-3 leaf | "reads exactly these offsets" — the stride and field agreement that makes the tail a mesh source (`collision_level.rs:29`) |

### 2.3 `ow-tank/src/brush.rs`

| address | name | what the comment says |
|---|---|---|
| `0x142ced510` | vertex decoder (`FUN_142ced510`) | the read expression of `Vertex::position` (`:29`, `:141`, `:498`) |
| `0x142cf0360` | ring walk / hull builder | follows `next_ring`; one of three independent readers of the record shapes (`:29-30`, `:43`, `:175`) |
| `0x142ced1a0` | edge-list builder | third independent reader of the record shapes (`:30`) |
| `0x142ced620` | ring builder / writer | "writes exactly these fields"; the ordering pass (`:30`, `:53`, `:56`, `:230`, `:392`) |
| `0x142cf23c0` | `BrushTreeToMesh` | the tree-to-mesh walk; only the op classification is read (`:42`) |
| `0x142cf2010` | `MeshToColliderFaces` | the face conversion, already the read expression in `ow_sim::collider::shell` (`:47`) |
| `0x14213a510` | the ingest copy | copies the whole arrays into a `0x28`-byte snapshot record; reads the leaf byte (`:56`, `:196`, `:257`) |
| `0x1435ed130` | constant | holds `400.0` in all four lanes: the quantisation divisor (`:61-63`) |
| `0x1431102d0` | constant | holds `{1.0, 1.0, 0.0, 0.0}`: the instance's `z`/`w` are multiplied by zero (`:64-66`) |

### 2.4 `ow-tank/src/model_collision.rs`

| address | name | what the comment says |
|---|---|---|
| `0x143e74830` | descriptor of `C` | `desc+0xa0` = `0x198` size, `desc+0xb8` = type hash (`:7`, `:27`, `:34`) |
| `0x143e74790` | field table | stored at `desc+0x08` (`:8`, `:36`) |
| `0x1413354b0` | construct-in-place hook | writes the class's three inline sub-object vtables (`:8`, `:31`, `:38`) |
| `0x141335610` | factory | allocate `0x198` and construct (`:8`, `:40`) |
| `0x1431dd118` | vtable | written by the constructor (`:8`, `:32`) |
| `0x14213c440` | `GeometryBuild` | turns the `0x98`/`0xa0` slot into a point array + count, twice (`:16`) |
| `0x141b53c40` | flag fold | folds one 14-byte flag record into a `u16` (`:20`, `:88`) |

### 2.5 `ow-tank/src/mesh_shape.rs`

| address | name | what the comment says |
|---|---|---|
| `0x141b53af0` | the block unpacker | reads each array's 16-byte header at `pointer` (tag 1) or `pointer + 8`, into a `0x30`-byte output (`:8`, `:14`, `:47`, `:65`, `:73`, `:176`) |
| `0x142cae540` | `MeshShapeFill` | fills the 19-dword / `0x4c`-byte shape record (`:8-9`, `:94`) |
| `0x142caf6c0` | the collision query | the consumer the record is handed to (`:9`) |

### 2.6 `ow-tank/src/mesh_world.rs`

| address | name | what the comment says |
|---|---|---|
| `0x142caed30` | `MeshSweep` (type-3 body) | every stride/offset in the constant table; the query-block reads; the node/triangle/vertex loads (`:11`, `:15`, `:39`, `:57`, `:73`, `:125`, `:165`, `:380`, `:406`, `:426`, `:495`, `:662`) |
| `0x141b53af0` | the block unpacker | hands the three pointers to the `0x30`-byte source (`:8`, `:197`) |
| `0x142cae540` | `MeshShapeFill` | copies the three pointers into the record at `+0x20`/`+0x28`/`+0x30` (`:9`) |
| `0x142cd5540` | `CollisionSweepBounds` | the query block it and the sweep read (`:380`, `:387`) |
| `0x142cc45a0` | handle dispatcher | copies the descriptor into a `0x73`-byte stack block (`:42-44`) |
| `0x142cce430` | `MeshTriangleTest` | the `0.005` fast-path limit; the handoff to the GJK (`:48-51`, `:134`) |
| `0x142cc9460` | `ClosestPointGjk` | the slow path the other states take (`:51`) |

### 2.7 `ow-tank/src/domino_world.rs`

| address | name | what the comment says |
|---|---|---|
| `0x14201ae60` | builder | which id it hands to `OwnerCreateBody` (`:5`) |
| `0x142cc3d90` | `OwnerCreateBody` | the `u16` slot guard, the sentinel test, the world-pointer decode, the handle pack (`:9`, `:19`) |
| `0x142cc3c80` | world allocator | fills the first empty slot with the inverse expression (`:17`, `:51`) |
| `0x14201ae9f..0x14201aecc` | builder guard | the three-part invalid-handle test on `[component+0x88]` (`:35`) |
| `0x1442a0bc0` | world table | "the global world table (`0x1442a0bc0`)" — note the constant is written `0x1442_0bc0` (`:41-43`) |

### 2.8 `ow-tank/src/range_world.rs`

| address | name | what the comment says |
|---|---|---|
| `0x14201ae60` | holder loop / body builder | the holder loop the mesh selection mirrors (`:6`) |
| `0x14201c434` | descriptor scale | `maxss(scale, 0.01)` when the three lanes agree within `0.01`, else `1.0` (`:120`) |

### 2.9 `ow-tank/src/chunkkind.rs`

| address | name | what the comment says |
|---|---|---|
| `0x1439aa320` | kind table | `0x24` entries of `0x20` bytes (`:8`, `:81`, `:120`) |
| `0x1404c19b0` | name deobfuscator | decodes all `0x24` names once, in table order (`:18`, `:83`) |
| `0x141b32e20` | `MapChunkGuid` | reads the id from the GUID (`:14`) |
| `0x141b32e70` | `ChunkSlotForGuid` | searches the table for a GUID's id and returns the slot (`:14-15`) |
| `0x141ff54e0` | `FinishMapChunkAsset` | dispatches slots `0x1D`/`0x21` to `FUN_142e492a0`, slots `0x1E`/`0x22` to the cell path (`:35`) |
| `0x142e492a0` | collision chunk fixup | the header and the two element passes; `fixup_in_place` (`:35`, `:774`) |

### 2.10 `ow-sim/src/collide.rs` — entry points and stages

| address | name | what the comment says |
|---|---|---|
| `0x142cd5540` | `CollisionSweepBounds` | first stage of the swept query; writes start/end/expansion (`:3`, `:169`, `:254`) |
| `0x142cbd510` | the query driver | four passes, worklist, two folds, narrowphase dispatch (`:4`, `:226`, `:374`) |
| `0x142cc45a0` | handle dispatcher | builds the `0x80`-byte query block (`:15-16`) |
| `0x142cce430` | `MeshTriangleTest` | analytic vs. GJK path selection (`:63`, `:1251`, `:1254`) |
| `0x142cc9460` | closest-point GJK | the slow path; `closest_point` (`:65`, `:1267`, `:2606`) |
| `0x142cc7740` | four-vertex reduction | `simplex4`; `.pdata 0x142cc7740..0x142cc8d72`, `0x1632` bytes (`:68`, `:2869`) |
| `0x142caed30` | type-3 mesh body | `mesh_sweep` (`:750`, `:1560`) |
| `0x142caed10` | type-3 wrapper | zeroes the triangle callback argument (`:750`, `:1568`) |
| `0x142caef15..0x142caf1a5` | support-point branch | the affine map and the hull fold (`:39`, `:1501`) |
| `0x142caef15..0x142caf055` | support-point affine | `support_hull_affine` (`:1426`) |
| `0x142caf170` | recentred support array | the array the triangle descriptor points at (`:1262`, `:1552`) |
| `0x142caf5c1` | motion shrink | the motion budget shrinks to the last accepted fraction (`:1578`) |
| `0x142caf5d7` | box re-derive | `lo`/`hi` re-derived from the hit point after every acceptance (`:1626`) |
| `0x142cce6e8` | GJK descriptor build | the stack descriptor the slow path builds (`:1266`) |
| `0x142cd58a0` | box/triangle overlap | `box_triangle_overlap`; `0x4ec`-byte window (`:908`) |
| `0x142caea70` | fan refinement gather | `mesh_gather`; `.pdata` covers only the first `0x65` bytes (`:1022`) |
| `0x142cadee0` / `0x142efe2b0` / `0x142cadf00` | id-container alloc/copy/free | the growth path that is **not** modelled (`:1052-1053`) |
| `0x142cd56e0` | degenerate-triangle predicate | `triangle_degenerate` (`:2251`) |
| `0x142cbb350` | layer filter | `shape_filter` (`:1754`, `:1788`) |
| `0x142cb8b80` | shape world frame | `shape_world_transform` (`:1832`) |
| `0x142cb8bbf..0x142cb8cb9` | composed-pose branch | `compose_pose` (`:1860`) |
| `0x142cd4ef0` | `ShapeAddToOwner` | sets the owner-attached state `entry[0xb0] != 1` (`:1812`) |
| `0x142cb3940` | `ShapeAddToBody` | sets the body-attached state `entry[0xb0] == 1` (`:1813`) |
| `0x142cc1610` | shape narrowphase | transform, filter, dispatch on the type byte, publish `+0x20` (`:1930`, `:1977`) |
| `0x142ccdf40` | type-0 sphere | `sphere_sweep` (`:519`, `:648`) |
| `0x142ccd7b0` | type-1 capsule | `capsule_sweep` (`:2098`, `:2100`) |
| `0x142cc8d80` | 3-simplex reduction | `simplex3` (`:2331`, `:2391`) |
| `0x142cc9b8b` | inline 2-vertex reduction | `reduce_segment` (`:3192`) |
| `0x142cc9c7f` | degenerate rejection | the `None` return of `reduce_segment` (`:3194`) |
| `0x142cce0c0..0x142cce0fe` | world-to-local rotation | `rotate_to_local` (`:599`) |
| `0x142cce317..0x142cce35a` | local-to-world (point) | `rotate_to_world_point` (`:613`) |
| `0x142cce369..0x142cce3af` | local-to-world (normal) | `rotate_to_world_normal` (`:625`) |
| `0x143110818` + `0x143391030` | squared-length epsilon | the same value splatted (`:523`) |
| `0x142f817a0` | sign-mask splat | `negate` and the back-snap's `-dot` (`:555`) |
| `0x142f84a00` | zero quad | the quad the sign-select and pivot terms multiply by (`:1852`) |
| `0x142fa62f0` | one quad | the barycentric dividend (`:2374`) |

Driver-internal instruction sites cited in the doc: `0x142cbd5e0` (pass skip),
`0x142cbd603` (re-seed), `0x142cbd62c` (bounds call), `0x142cbd63a..0x142cbd667`
(per-pass geometry, with `:647` delta, `:651` reach, `:655` epsilon, `:664`
MINPS, `:667` MAXPS), `0x142cbd692` (root push), `0x142cbd6a4` (pop from end),
`0x142cbd6bb` (`SHL RBX,0x6` entry index), `0x142cbd6bf..0x142cbd6e3` (AABB
reject), `0x142cbd6e9..0x142cbd749` (moving-box reject), `0x142cbd756` (leaf
test), `0x142cbd75f` (child push order), `0x142cbd791..0x142cbd7c3` (re-seat),
`0x142cbd7c9` (pass end), `0x142cbd799` (compare operand), `0x142cbd7a1` (fold
min), `0x142cbd7ae` (all-lane rewrite), `0x142cbd7ff` (return), `0x142cbd80f`
(flag shift), `0x142cbd812` (entry stride `0x28`) — `collide.rs:243-250`,
`:385-405`, `:428-505`.

Constant-load addresses: `0x1435ecad0` (FLT_EPSILON splat), `0x1435ecaf0`
(separation `0x3ba3d70a`), `0x142f844e0` (driver's `0.5`), `0x1435ecda0` (the
`0.005` fast-path splat), `0x1435ecfc0` (`0.5` quad), `0x1435ecfd0` (edge
tolerance `0.005`), `0x1435ecd80` (parallel-sweep epsilon), `0x1430a4bf0`
(`(1,1,1,0)`), `0x1430a4b40` (`0x7fffffff` mask and keep-low-three),
`0x143110d50`/`0x143110d60`/`0x143110d70`/`0x142f860e0` (conjugate and product
sign masks).

### 2.11 `ow-sim/src/collider.rs`

| address | name | what the comment says |
|---|---|---|
| `0x142cf23c0` | `BrushTreeToMesh` | the tree-to-mesh stage that produces the source (`:4`) |
| `0x142cf2010` | `MeshToColliderFaces` | the double-sided shell (`:5`, `:172`, `:175`) |
| `0x142cca460` | `ColliderFaceAppend` | `0x60`-byte face records from the `0x14`-byte index records, skipping degenerate triangles, accumulating area (`:6`, `:235`, `:240`) |
| `0x142ccb1b0` | BVH builder | the build over the `0x60`-stride faces (`:7`, `:19`, `:281`, `:308`) |
| `0x142ccad80` | flattener | build nodes to the `0x30`-byte runtime source (`:8`, `:582`, `:585`) |
| `0x142cd56e0` | per-triangle skip test | the test `MeshToColliderFaces` applies (`:150`) |
| `0x142ccb2d0` | centroid spread decision | which of the two split paths runs (`:316`) |
| `0x142ccb2a2` | centroid union | decides the path (`:358`) |
| `0x142ccb314` | clustered path | box union then array half (`:319`, `:406`) |
| `0x142ccb462`, `0x142ccb462-0x142ccbd37` | bin split | the 64-bin surface-area split and its cost expression (`:303`, `:320`, `:368`) |
| `0x142ccbc17` | partition | the client swaps each qualifying face into the next free slot (`:372`) |
| `0x142ccb4b4`, `0x142ccb356` | axis order | `x` beats `y`, `y` beats `z`, `z` default (`:449`) |
| `0x142ccb670` | bin index | the bin a centroid falls in on one axis (`:503`) |
| `0x142ccb891-0x142ccbb96` | bin accumulation | assign bins, prefix/suffix boxes, cheapest boundary (`:511`) |
| `0x142f897c4` | threshold | the shell-wall threshold and the BVH flat-cluster threshold, `0.005` (`:30`) |
| `0x142f80ff8` | half | `0.5` (`:32`) |
| `0x14311e0c0` | padding floor | the leaf box padding floor, `0.01` (`:34`) |
| `0x14305af0c` | bin scale | `64.0` (`:288`) |
| `0x14213c440` | `GeometryBuild` | the whole chain, as it does for a brush (`:684`) |

### 2.12 `ow-sim/src/fan.rs` (collision-query consumer)

| address | name | what the comment says |
|---|---|---|
| `0x140cb86c0` | the wish writer | the fan solver's owner (`:2`, `:41`, `:56`) |
| `0x140cb87d6..0x140cb8874` | speed read | the speed expression (`:178`) |
| `0x140cb8766..0x140cb87ba` | seed read | `dot(position - M*wish, dir)` (`:221`) |
| `0x140cb88f9..0x140cb892c` | second seed term | `dot(query - (position - ramp), dir)` (`:243`) |
| `0x140cb8975` | bias | `+0.1` before the search (`:97`, `:256`) |
| `0x140cb884c` | speed gain | `1.1` (`:99`) |
| `0x140cb90f1..0x140cb91d8` | pass 1 acceptance | `abs(score) < best_score` **and** `y > best_y` (`:317`) |
| `0x140cb9be5..0x140cb9c16` | pass 2 acceptance | `abs(score) < best_score` only (`:327`) |
| `0x140cb9b6e` | ring acceptance site | gate && alignment > threshold (`:334`) |
| `0x140cb9770` | facing comparison | `dir.y*n.y + dir.x*n.x + dir.z*n.z` (`:1021`) |
| `0x140cb9842..0x140cb9a20` | refinement | pentagon step `0.02`, refined dir `0.005`, ground probe `0.04`, keep/replace/break (`:23`, `:363`) |
| `0x140cb9864` | batch tail dispatch | `kind <= 1` snap, else pentagon (`:403`, `:416`) |
| `0x140cb988d` / `0x140cb96c1` | `BatchTail` variants | back-snap vs. five-direction refinement (`:410`, `:412`) |
| `0x140cb9580` | back-transform direction | `-xyz` horizontal unit, `y` zeroed, `w` from `-w`, `(0,0,1,0)` fallback (`:20`, `:434`, `:472`) |
| `0x140cb95a8` | refinement direction | `-normal.xyz`, falling back to the previous (`:604`) |
| `0x140cb98a0..0x140cb98bc` | refined endpoints | the raised endpoint and the `0.04` ground probe (`:640`) |
| `0x140cb927a..0x140cb92e2` | radius read | `max(body[0x65c],0.35) * body[0x5b8] - 0.015` (`:214`) |
| `0x140cb9322..0x140cb9332` | ring step | `2 pi / N` (`:290`) |
| `0x140cb92f3`, `0x140cb92fa` | snap endpoints | the batch ring's own endpoints (`:54`) |
| `0x140cb8d4a`, `0x140cb8e26`, `0x140cb8e3c`, `0x140cb8e4e`, `0x140cb9263`, `0x140cb9273`, `0x140cb9ae0` | stage order | gate, pass-0 continuation, arm pick, shape pass, ring, batch ring, incremental ring (`:43-50`) |
| `0x140cc1b97` | pass 0 selection | the second probe wins only on a strictly smaller score (`:579`) |
| `0x140cc1870` | `MoverWishQueryCached` | six lane comparisons, `<= 0.004` (`:696`) |
| `0x140ca7400` | turn limit | `clamp(body[0x5c8] * pi/180, 0, 1.5707844)` (`:75`, `:136`, `:710`) |
| `0x140ca7430` | speed cap | max of the three cap slots (`:146`) |
| `0x140ca72b0` | frame time | `(f32)(i64)body[0xe8] * 1e-6` (`:171`) |
| `0x141ae3eb0` | shape local box | `(min, far)` corner quads from `shape+0x2d28`/`+0x2d2c` (`:528`) |
| `0x1426c40f0` | rotation helper | low half sine, high half cosine (`:484`) |
| `0x1420328c0` | the evaluator | the candidate scorer that is **not** implemented (`:33`, `:450`, `:671`, `:1000`) |
| `0x14203dae0` | dispatcher | carries the block's `+0x490`/`+0x498` to the descriptor's `+0x28`/`+0x30` (`:1011`) |
| `0x142cc45a0` | query-block dispatcher | carries the same to the block's `+0x30`/`+0x38` (`:1012`) |
| `0x142cbd510` | query driver | walks the domino's island list at `domino+0x1b0+0x44` (`:1015`) |
| `0x140cc2290` | mesh-face refinement | read but not implemented (`:35`, `:920`) |
| `0x142f85108`, `0x1431102c0`, `0x142f844b8`, `0x143110820`, `0x143058fac`, `0x142f7aad0`, `0x143110824`, `0x143110828`, `0x142f88c18`, `0x142f844d8`, `0x142fb8e58`, `0x38fa0000` | constants | see §1.12 for each value (`:101-119`, `:321`, `:426`, `:782`) |

### 2.13 `ow-sim/src/walk.rs`

| address | name | what the comment says |
|---|---|---|
| `0x140ca7f30` | `MoverAdvance` | `(scale_a*body[0xc0] + wish)*scale_b` (`:7`) |
| `0x142cbd1b0` | `SetCharacterControllerVelocity` | publishes the displacement (`:10`) |
| `0x142caed30` / `0x142cce430` | the type-3 leaf | the verified sweep each query runs (`:13`) |
| `0x142cd5540` | bounds / query block | `P0`, `P1`, motion, expansion, mask (`:14`, `:103`) |
| `0x142cc9460` | `ClosestPointGjk` | the path the `0.35` expansion takes (`:17`) |
| `0x142cbd510` | the driver | folds each accepted hit by the fraction and re-seats the box (`:19`, `:73`) |
| `0x142cbcee0` -> `0x142cf3f80` | `RunCharacterControllerSweep` | the Domino-side shape/pair walk; the read part is the fold only (`:29`) |
| `0x142caf6f0` | support fold | adds the contact margin (`:74`) |
| `FUN_140cc1440` | step-up stage | a separate read stage, named in the floor-snap doc (`:411-412`) |

### 2.14 `ow-sim/src/policy.rs`

| address | name | what the comment says |
|---|---|---|
| `0x14202bc90` | the hit policy | the callback implemented here (`:1`, `:325`) |
| `0x14202c580` | the thunk | a four-move thunk that rotates `(hit, packed, ctx)` (`:5-6`) |
| `0x1420af950` | layer/exclusion screen | 62 bytes, rejects on mask/exclusion (`:141`, `:310`) |
| `0x14202beba..0x14202bd06` | geometry pointer decode | `ror64(ror64(raw + KEY_ADD, 0x22) ^ KEY_XOR, 0x2b)` (`:238-243`) |
| `0x14202bd06`, `:bd22`, `:beb0`, `:bd1b`, `:bed8`, `:bee8`, `:bef0`, `:bee0`, `:be70`, `:bcf2`, `:bf3a`, `:bf4c..:c026`, `:bf93` | policy sites | the key-decode constants, `KEY_UNSET`, `ID_PENDING`, the `PackedShape` field offsets, negative-id resolution (`:42-85`, `:418`) |
| `0x140543481`, `0x1405434a1`, `0x140543440` | in-place decode | the entry value's xor/subtrahend and the in-place decode (`:58-60`, `:274`, `:293`) |
| `0x143b79598 + 0x198`, `0x1438480f3` | anti-tamper | the xor applied to the entry, and the salt byte (`:208`, `:210`, `:272-273`) |
| `0x141b3c890` | variant table | fills an id/kind pair (`:214-217`) |
| `0x1420c2360` | world getter | produces the world the policy reads (`:221`) |
| `0x1420328c0` | evaluator | the client's ordering fan -> evaluator -> query -> policy (`:3`, `:450`) |
| `0x142cc1610` | narrowphase | the wrapper that calls this policy (`:5`) |

---

## 3. Tests that exercise collision code

### (a) Pure unit tests with hardcoded vectors (no install)

`crates/ow-tank/src/collision.rs` — `mod tests`, 9 tests, synthetic byte payloads:

| test | asserts |
|---|---|
| `converts_mode_one_records_in_place` (`:680`) | header mode 1 + entry mode 1: two records of 17 qwords; zero qwords stay zero, non-zero become `record + q` |
| `converts_header_and_entry_mode_two` (`:714`) | header mode 2 (count at `inner+0`, entries at `inner+8`) + entry mode 2 (record pointer at `+8` of the object) |
| `empty_container_is_a_no_op` (`:744`) | a zero count exits before any entry is read (block byte-identical) |
| `mode_zero_requires_the_allocator` (`:757`) | header mode 0 with a positive count returns `AllocationRequired(header)` |
| `parses_a_synthetic_two_cell_container` (`:808`) | count, class, `data_offset 0x110`, boundaries, array lengths, second cell offset |
| `rejects_a_wrong_magic_and_broken_tiling` (`:833`) | flipped magic gives `BadMagic`; a record one byte too long gives `NotTiled { cell: 1 }` |
| `out_of_bounds_pointer_is_rejected` (`:846`) | a pointer outside the block gives `OutOfBounds(BASE+0x1008)` instead of a read |
| `tables_derive_their_strides_from_the_tiling` (`:894`) | strides `0x28`/`0x20`/`0x10` and the three `end()` values from a tiling payload |
| `tables_refuse_a_span_that_does_not_divide` (`:937`) | 3 elements over a `0x50` span gives `ArrayNotTiled { field: 0x10, count: 3, span: 0x50 }` |

`crates/ow-tank/src/brush.rs` — 11 tests, expectations produced by the client oracle scripts:

| test | asserts |
|---|---|
| `the_header_is_the_size_the_builder_writes` (`:490`) | `HEADER_SIZE 0x20`, `VERTEX_SIZE 0x20`, `NODE_SIZE 0x28` |
| `the_vertex_decoder_matches_the_client` (`:502`) | 11 `(x, y, divisor)` cases, all four lanes bit-exact vs `0x142ced510` (`dev/brush_vertex_oracle.py`) |
| `a_zero_divisor_is_refused` (`:546`) | `position()` is `None` where the client's divide would fault |
| `the_quantiser_rounds_half_away_from_zero` (`:561`) | `roundf` semantics and the two-multiply order (`0.00125*400 = 0.5 -> 1`, `-0.5 -> -1`, `0.00375*400 -> 2`, ...) |
| `the_square_ring_matches_the_client` (`:578`) | the full 4-node square ring field for field, including the two nodes the ordering pass swapped |
| `the_other_windings_match_the_client` (`:635`) | the clockwise square and a triangle, `(x, y, prev_x, prev_y, descending)` per node |
| `a_repeated_point_rejects_the_brush` (`:682`) | two consecutive equal points give `None` |
| `the_two_scales_are_separate` (`:694`) | non-uniform scales `[0.5, 2.0]` reach the two lanes separately; divisor always `1` |
| `no_instances_is_an_empty_brush` (`:709`) | empty input gives an empty brush with root `0` |
| `the_records_round_trip_and_are_guarded` (`:719`) | builder output -> bytes -> `Brush::parse` -> identical; `rings()` = `[[0,1,2]]`; truncated blocks, a bad root and a zero divisor all error |
| `the_order_comparison_is_the_pass_one` (`:782`) | no accepted brush holds an equal pair; every `descending == 1` node's `order()` is `Greater` |

`crates/ow-tank/src/model_collision.rs` — 5 tests: `the_array_record_is_read_the_way_geometry_build_reads_it` (`:108`, tag 0/1 select the qword order, all four values distinct bit patterns), `the_tag_is_a_whole_qword_test` (`:133`, only exactly `1` swaps), `a_zero_count_is_refused` (`:153`), `the_flag_fold_uses_the_client_byte_order` (`:178`, 14 ones give `0x3fff`, clearing byte 6 clears `0x2000`, byte 7 is the lowest bit, byte 6 the highest), `the_class_identity_is_the_registered_one` (`:197`, the constants and the non-overlapping field ladder).

`crates/ow-tank/src/mesh_shape.rs` — 4 tests: `the_array_header_offset_is_the_tag_test` (`:140`), `the_shape_record_is_mesh_shape_fill_dword_for_dword` (`:149`, every dword of the `0x4c` record), `a_zero_scale_is_an_infinity_lane` (`:170`, `divss` by zero gives `inf` lanes), `the_field_offsets_are_the_unpackers` (`:179`).

`crates/ow-tank/src/mesh_world.rs` — 10 tests: `the_strides_are_the_walks_and_the_narrowphases` (`:483`, cross-checked against `ow_sim::collide::MESH_*`), `the_record_offsets_are_the_walks` (`:498`), `the_query_block_is_the_no_support_point_query` (`:516`), `the_arrays_are_appended_and_each_shape_keeps_its_own_addresses` (`:559`), `the_record_addresses_the_shapes_own_arrays` (`:615`), `the_node_bounds_are_the_two_corners` (`:664`), `the_identity_xform_is_the_shapes_own_frame` (`:703`), plus three `#[should_panic]` guards: `a_short_array_is_refused`, `a_node_count_that_disagrees_is_refused`, `a_point_count_that_disagrees_is_refused`.

`crates/ow-tank/src/domino_world.rs` — 7 tests: `the_two_addends_are_inverses_modulo_2_pow_64` (`:132`), `the_world_pointer_round_trips_bit_exactly` (`:139`), `the_body_handle_is_slot_then_generation_then_index` (`:159`), `the_invalid_body_handle_is_ffff_0000_ffff_ffff` (`:179`), `the_slot_bound_is_strictly_below_0x3fff` (`:211`), `the_empty_slot_is_the_encoding_of_a_null_world` (`:223`), `the_body_pool_is_0x1c0_per_index` (`:238`).

`crates/ow-tank/src/chunkkind.rs`, `collision_level.rs`, `range_world.rs` — **no unit tests** in the module.

`crates/ow-sim/src/collide.rs` — `mod mesh_gather_test` (2), `mod tests` (42), `mod mesh_case0` (2) — 46 test functions in the file:

| group | tests (line) |
|---|---|
| gather | `the_adjacent_leaf_is_walked_before_the_far_one` (`:1234`), `the_guard_word_is_a_signed_boundary` (`:1253`) |
| driver geometry | `pass_geometry_folds_the_swept_box_against_the_start` (`:3299`), `pass_geometry_keeps_the_reach_on_nan` (`:3310`), `aabb_accepts_an_overlap_and_rejects_a_gap` (`:3320`), `aabb_ignores_the_fourth_lane_and_reads_the_sign_bit` (`:3331`), `moving_box_accepts_a_box_the_sweep_passes_through` (`:3352`), `moving_box_rejects_a_box_the_sweep_misses` (`:3362`), `moving_box_matches_the_client_lane_permutation` (`:3371`), `moving_box_uses_the_separation_constant_strictly` (`:3412`) |
| driver walk | `driver_visits_the_second_child_first` (`:3533`), `driver_prunes_a_leaf_the_swept_box_cannot_reach` (`:3544`), `driver_skips_passes_whose_flag_bit_is_clear` (`:3555`), `driver_end_the_sweep_answer_stops_the_walk` (`:3570`), `driver_folds_every_hit_into_the_running_motion` (`:3580`) |
| sweep bounds | `identity_quaternion_is_the_point_cloud_bounds` (`:3611`), `quarter_turn_about_z_swaps_x_and_y` (`:3622`), `one_point_has_no_extent` (`:3632`), `zero_count_leaves_the_outputs_untouched` (`:3648`), `minps_keeps_the_second_operand_on_nan` (`:3663`) |
| sphere | `a_sweep_through_the_centre_hits_the_near_surface` (`:3711`), `a_sweep_away_from_the_sphere_misses` (`:3724`), `starting_inside_reports_only_when_the_flag_is_set` (`:3732`) |
| filter | `the_filter_mask_mode_is_the_intersection` (`:3745`), `the_filter_id_rule_matches_the_client` (`:3754`) |
| capsule | `a_degenerate_capsule_is_a_sphere_at_the_midpoint` (`:3801`), `a_sweep_down_the_axis_is_bound_by_the_near_cap` (`:3813`), `a_sweep_starting_inside_the_capped_volume_reports_at_the_start` (`:3828`) |
| mesh sweep | `a_mesh_sweep_down_onto_a_quad_reports_the_triangle_that_contains_it` (`:4041`), `a_mesh_walk_reports_the_nodes_it_expanded` (`:4065`), `a_mesh_walk_skips_a_node_whose_layer_mask_is_clear` (`:4083`), `a_mesh_walk_skips_a_triangle_whose_layer_mask_is_clear` (`:4100`), `a_mesh_sweep_beside_the_quad_misses` (`:4119`), `a_mesh_shape_without_a_root_leaves_the_output_untouched` (`:4138`) |
| support hull | `the_support_hull_affine_is_the_read_map` (`:4197`), `an_identity_block_quaternion_is_the_identity_rotation` (`:4220`), `the_support_hull_folds_two_points_into_the_walk_hull` (`:4233`), `a_support_point_hull_reaches_the_client_walk` (`:4278`), `the_pinned_translated_support_point_contact_is_reproduced` (`:4311`), `a_support_point_hull_beside_the_mesh_clears_the_leaves` (`:4340`), `the_support_point_step_decides_the_leaf_reach` (`:4362`) |
| mesh transform | `the_mesh_scale_multiplies_the_local_hit` (`:4430`), `the_mesh_transform_moves_the_contact_into_world_space` (`:4452`), `the_last_hit_triangle_wins` (`:4470`) |
| fixture / NaN pins | `the_python_fixture_case_hits` (`mesh_case0`, `:4527`; reads `dev/data/read/case0_*.bin`, a dev fixture on disk, not the install), `composed_pose_infinite_origin_is_the_qnan_indefinite` (`mesh_case0`, `:4556`, `#[cfg(target_arch = "x86_64")]`; pins `0xffc00000` where the emulator answers `0x7fc00000`) |

`crates/ow-sim/src/collider.rs` — `mod tests` (6) + `mod swap_partition` (1):
`the_shell_holds_both_surfaces` (`:790`, both z halves, x/y/w pass through `+ 0.0` / `- 0.0`, outward then inward block, every record `mode 0`, `a = 7`, `b = 0`),
`a_thick_shell_closes_its_edges` (`:813`, `epsilon 0.01` adds two wall records per present entry, first wall `[0, 4, 5]`),
`a_quad_becomes_four_faces` (`:827`, four faces, area `2.0`, the first record's min/max/centroid/area/index),
`the_flat_path_splits_at_the_half` (`:855`, ten clustered faces halve to leaves `(0,2) (2,3) (5,2) (7,3)`, height 3),
`the_bin_path_splits_at_the_cheapest_bucket` (`:891`, two clusters split at the gap, 4 + 4 faces, order preserved),
`a_source_flattens_into_the_runtime_nodes` (`:929`, one runtime node holding four triangles, `-0.01`/`1.01` in x, `-0.02`/`0.02` in z, the first triangle's three indices),
`the_client_swaps_rather_than_shifting` (`swap_partition`, end of file, asserts the exact permutation `[0,1,2,3,7,5,6,4]` and buckets `[0,10,21,0,0,42,63,21]`).

`crates/ow-sim/src/fan.rs` — 45 unit tests on synthetic body buffers: `turn_limit_converts_and_clamps`, `speed_cap_takes_the_maximum_of_three`, `speed_cap_falls_back_to_the_last_slot_on_nan`, `frame_time_is_ticks_times_a_microsecond`, `fan_speed_uses_the_velocity_length_and_the_floor`, `fan_radius_floors_scales_and_shrinks`, `seed_projects_the_delta_onto_the_direction`, `seed_uses_the_transformed_wish`, `state_term_subtracts_the_ramp_before_projecting`, `biased_seed_adds_a_tenth`, `rotate_pair_is_the_client_rotation`, `pentagon_rotates_by_72_degrees_five_times`, `ring_step_is_two_pi_over_the_count`, `the_batch_tail_snaps_for_kinds_zero_and_one`, `ring_offsets_wrap_after_a_full_turn`, `the_first_pass_needs_a_better_score_and_a_higher_y`, `the_fine_pass_only_needs_a_better_score`, `the_back_direction_normalizes_horizontally_and_drops_y`, `a_flat_descriptor_falls_back_to_positive_z`, `the_refinement_direction_falls_back_when_the_normal_is_degenerate`, `the_refined_offset_scales_both_terms`, `the_refinement_breaks_on_the_first_facing_hit`, `the_ring_acceptance_is_gated_and_strict`, `the_ring_sweep_radius_is_floored_at_0_35`, `the_shape_ring_shortens_the_seed_and_floats_at_0_015`, `the_refinement_acceptance_is_strict_and_flag_bypassable`, `the_refined_sweep_drops_the_ground_probe_by_four_centimetres`, `the_two_endpoints_carry_different_scalars`, `the_memo_tolerance_is_inclusive_and_ignores_w`, `pass_zero_probes_the_two_masks_in_order`, `pass_zero_keeps_the_first_probe_on_a_tie`, `pass_zero_lets_a_miss_beat_a_late_hit`, `the_shape_box_is_asymmetric_in_x_and_z`, `the_shape_probes_follow_the_store_order`, `the_facing_threshold_is_the_turn_limits_cosine`, `the_projection_is_absolute_and_ordered`, `the_back_transform_removes_the_component_along_the_mover`, `the_reflection_flags_read_the_third_byte`, `the_back_snap_seats_y_where_the_sweep_crosses_the_direction_plane`, `the_back_snap_skips_below_the_divide_floor_and_outside_the_sweep`, `the_gate_stops_the_producer_before_any_probe`, `a_misaligned_ring_hit_is_rejected`, `the_ring_keeps_the_closest_candidate_not_the_first`, `an_aligned_ring_hit_flows_through_refine_and_the_back_transform`.

`crates/ow-sim/src/walk.rs` — 10 tests on synthetic worlds: `the_floor_stops_a_downward_sweep`, `open_floor_motion_is_not_clamped`, `a_wall_stops_the_normal_and_keeps_the_tangent`, `a_sustained_wall_push_never_tunnels`, `a_horizontal_walk_cannot_climb_a_tilted_wall`, `the_fold_leaves_the_body_outside_the_query_radius`, `a_downward_step_lands_grounded`, `the_floor_snap_lands_a_radius_above_the_surface`, `the_shape_transform_rotates_the_query_into_the_shape`, `the_query_block_is_the_read_layout`.

`crates/ow-sim/src/policy.rs` — 14 tests with a `FakeGeometry`: `mode_zero_is_a_presence_test`, `mode_one_fills_the_record`, `a_negative_id_resolves_through_the_variant_table`, `without_the_presence_bit_the_pending_id_stands`, `a_null_world_skips_the_filter_and_the_decode`, `the_layer_filter_rejects_missing_forbidden_and_excluded_worlds`, `the_optional_filter_rejects`, `the_classifier_decides_mode_two_and_up`, `the_classifier_sees_the_filter_argument`, `the_pointer_decode_inverts_the_documented_encoder`, `the_solver_hands_the_fan_the_records_own_values`, `the_solver_reports_nothing_when_the_filter_rejects`, `the_first_sites_presence_term_is_always_zero`, `the_key_is_zero_without_the_presence_bit_and_stable_with_it`.

### (b) Tests that read the operator's install (real-data tests)

All under `crates/ow-tank/tests/`; each opens the install via a local
`fn open() -> Option<GameData>` and returns early when it is not readable.

| file | test | asserts |
|---|---|---|
| `chunkkind_real.rs` | `the_two_collision_kinds_are_0x11_and_0x10` (`:41`) | the kind table's collision ids and slots |
| | `the_table_holds_the_placeable_names_this_project_already_knew` (`:86`) | eight placeable names match names read from the install by other means |
| | `the_fixup_keeps_a_zero_header_rva_null_and_adds_to_every_element_qword` (`:145`) | `fixup_in_place` semantics on a synthetic payload |
| | `a_non_positive_count_runs_no_pass` (`:168`) | a `<= 0` count does nothing |
| | `a_truncated_payload_is_refused` (`:185`) | a short payload gives `Truncated` |
| | `the_practice_range_collision_chunk_parses_as_36_sectors` (`:198`) | the real `0x0DD0001100000688` payload: header, sector pointers, tail pointers, tiling |
| | `the_practice_range_pathing_chunk_is_the_cell_container` (`:305`) | the real pathing chunk is the cell container `ow_tank::collision` reads |
| `collision_cells_real.rs` | `the_practice_range_collision_chunk_parses_as_220_cells` (`:31`) | 220 cells, every boundary checked against the payload, first cell layout pinned |
| | `the_collision_component_holds_three_tiled_arrays` (`:126`) | the three arrays and their derived strides tile the blob |
| `collision_level_real.rs` | `the_range_level_collision_is_one_world_space_mesh` (`:36`) | the level's three arrays tile; counts, root box and mask |
| `mesh_world_real.rs` | `the_pinned_model_mesh_sweeps_to_an_exact_hit` (`:304`) | one real block (`STU_537E56E4` of model `0x0d0000000000078c`) swept by `mesh_sweep`; hit point, normal, fraction, triangle index and ids pinned |
| | `the_ranges_mesh_shapes_assemble_into_one_world` (`:432`) | every holder `model_body::holder_selected` keeps assembles into one `MeshWorld`, each record addressing its own arrays |
| `model_collision_real.rs` | `the_collision_descriptor_is_reachable_through_its_asset_reference` (`:133`) | `C` (`0x564B1018`) is reached through nine entity assets' `0x09CB5A77`/`0xA7ED6024` reference; `FSTU` holds exactly one instance |
| | `the_real_brush_ring_and_mesh_are_pinned` (`:230`) | the real descriptor's instance points through `build_from_instances` into a ring and a mesh |
| | `the_practice_range_places_no_model_that_carries_the_descriptor` (`:298`) | the range places no model carrying the descriptor |
| `model_mesh_real.rs` | `the_range_models_prebuilt_mesh_and_its_shape_record` (`:110`) | the three arrays of `STU_537E56E4` from the install plus the record built at unit scale (file offsets stand in for the runtime pointers) |
| `range_world_real.rs` | `the_range_is_a_sweepable_world` (`:76`) | `RangeWorld::build` from the install, then a real sweep through `ow_sim::walk::World` |
| `brush_real.rs` | `no_asset_of_the_brush_source_type_exists_in_this_install` (`:48`) | no asset of GUID type `0x00A` exists — pins the negative measurement |
| | `a_model_carries_no_brush_container_chunk` (`:77`) | no model chunk set carries a brush container |
| | `the_record_sizes_are_the_ones_the_ingest_copies` (`:116`) | the `0x20`/`0x28` record sizes |
| `static_collision_real.rs` | `check_1_the_range_names_no_fracturable_descriptor` (`:70`) | byte-for-byte search of the range's kind-`0x0B` payloads for the nine `C` assets and the seven entity assets |
| | `check_2_the_models_carry_the_prebuilt_mesh_and_its_embedder` (`:152`) | counts and the embedder chain of `STU_537E56E4` in the models' `MSTU` |
| `mapchunk_kinds.rs` | (many) | placeable-body layouts at exact offsets plus a real-install sweep of `Placeable` bodies; the collision-relevant part is that model chunks still parse and resolve their references (`model_and_single_model_read_the_file_layout` `:62`, `model_group_reads_nested_groups_and_entries` `:94`, `kind_ids_match_the_reference_enum` `:463`, and the sweep test at the file end) |

### (c) Differential / emulator tests

The harness is `dev/verify_harness/` (Python driver: `runner.py`, `emu_oracle.py`,
`cli.py`, `ledger.py`, one `*_fixture.py` per unit). The Rust side is
`crates/ow-sim/src/bin/verify_probe.rs` (entries `collision_sweep_bounds`,
`collision_sphere`, `collision_capsule`, `collision_filter`,
`box_triangle_overlap`, `collision_transform`, `collision_narrowphase`,
`query_driver`, `triangle_degenerate`, `gjk_simplex3`, `gjk_simplex4`,
`gjk_closest`, `build_bvh`, `mesh_sweep`, `mesh_gather`, `shape_*`) and
`crates/ow-tank/src/bin/verify_probe.rs` (`cell_bytes_fixup`). Verdicts are in
`dev/verify_harness/ledger.json`.

| unit | client function | verified candidate | status / coverage / mutants |
|---|---|---|---|
| `fn/CollisionSweepBounds` | `0x142cd5540` | `collide::sweep_bounds` | verified, 100%, 11/11 |
| `fn/QueryDriver` | `0x142cbd510` | `collide::query_driver` | verified, 100%, 11/12 |
| `fn/CollisionSphere` | `0x142ccdf40` | `collide::sphere_sweep` | verified, 88.9%, 12/12 |
| `fn/CollisionCapsule` | `0x142ccd7b0` | `collide::capsule_sweep` | verified, 94.4%, 12/12 |
| `fn/CollisionShapeFilter` | `0x142cbb350` | `collide::shape_filter` | verified, 100%, 12/12 |
| `fn/ShapeWorldTransform` | `0x142cb8b80` | `collide::shape_world_transform` | verified, 100%, 9/9 |
| `fn/ShapeNarrowphase` | `0x142cc1610` | `collide::shape_narrowphase` | verified, 100%, 11/12 |
| `fn/BoxTriangleOverlap` | `0x142cd58a0` | `collide::box_triangle_overlap` | verified, 100%, 12/12 |
| `fn/MeshGather` | `0x142caea70` | `collide::mesh_gather` | verified, 90%, 28/28 |
| `fn/MeshSweep` | `0x142caed30` | `collide::mesh_sweep` | verified, 61.1%, 11/12 |
| `fn/ClosestPointGjk` | `0x142cc9460` | `collide::closest_point` (+ `simplex3`/`simplex4`) | verified, 74.5%, 12/12 |
| `fn/TriangleDegenerate` | `0x142cd56e0` | `collide::triangle_degenerate` | verified, 100%, 9/9 |
| `fn/BuildBvh` | `0x142ccb1b0` | `collider::build` | verified, 100%, 10/11 |
| `fn/MapChunkCellBytesFixup` | `0x141ff51a0` | `collision::fixup_cell_bytes` (`ow-tank` probe) | verified, 33.3%, 12/12 |
| `fn/ShapeBox`, `fn/ShapeSphere`, `fn/ShapeCapsule`, `fn/ShapePoints`, `fn/ShapeDispatch` | shape bounds leaves, `0x142cb9030` | `ow_sim::shapes::*` (adjacent module, outside this report's scope) | verified, 75-100%, 1/1-10/10 |

Dev-only differential probes that touch collision (not gated units):
`dev/gjk_closest_diff.py`, `dev/gjk_simplex_diff.py` (400 seeded quadruples),
`dev/gjk_simplex_probe.py`, `dev/gjk_trace.py`, `dev/gjk_trace_one.py`,
`dev/mesh_support_diff.py`, `dev/brush_vertex_oracle.py`,
`dev/brush_mesh_oracle.py`. Fixtures:
`dev/verify_harness/{box_triangle_fixture,cell_bytes_fixture,
collision_capsule_fixture,collision_filter_fixture,collision_narrowphase_fixture,
collision_sphere_fixture,collision_sweep_bounds_fixture,collision_transform_fixture,
gjk_closest_fixture,mesh_fixture,mesh_gather_fixture,query_driver_fixture,
bvh_fixture,triangle_fixture,shape_*_fixture}.py`.

---

## 4. Marker grep results

`FIT-` — **no occurrence** in any reviewed module. Repo-wide the `FIT-*` markers
live in `crates/ow-client/src/main.rs` (`FIT-ANI-02`, `FIT-CAM-03`, `FIT-CAM-06`,
`FIT-PRE-02`), `crates/ow-scene/src/lib.rs` (`FIT-CAM-05`, `FIT-PRE-01`,
`FIT-ANI-04`, `FIT-CAM-06`) and `crates/ow-tank/src/clip.rs` (`FIT-ANI-07`).
None is collision code (`docs/deviations.md` lists the same eight rows).

`TODO` / `FIXME` / `XXX` / `HACK` — none in the reviewed modules.

`not tested` / `unverified` — none written as such in the reviewed modules; the
equivalent statements are written as "open" / "not read" / "not ported" and are
collected verbatim in §5. The differential ledger uses the word *untested* for
the id-container growth path (§5, `fn/MeshGather`).

`open` — `collision.rs:40`, `chunkkind.rs:640`, `collide.rs:656`, `walk.rs:33`,
`walk.rs:86`, `policy.rs:21` (the remaining `open*` hits in `range_world.rs` are
`open_asset` calls).

---

## 5. What the code admits it does not do (verbatim, with file:line)

**Cell container to collision shapes (open)**

> `crates/ow-tank/src/collision.rs:33-40`
> "What the elements *mean* is not read, and the record conversion is not in
> this module.
>
> This is the ingest's container stage. It does **not** turn cells into
> collision shapes: the cell-to-shape decoder has no reader in the image
> (`docs/formats/maps.md`, "The cell path's consumer is not in the image"),
> so it stays open (`docs/open-questions.md` item 6)."

**Sector container to collision shapes (open)**

> `crates/ow-tank/src/chunkkind.rs:74-76`
> "The consumer that turns a sector into collision shapes is not in this
> module and is not read (`docs/formats/chunk-kinds.md`)."

> `crates/ow-tank/src/chunkkind.rs:635-641`
> "Not read from code: the tiling check shows these three values are the
> element counts of the four runs (`+0x2C` for the run at `+0x00` at a `0x0C`
> stride, `+0x30` for the runs at `+0x18` (one byte each) and `+0x08` (`0x10`
> bytes each), `+0x34` for the run at `+0x10` (four bytes each)). Their meaning
> is therefore still open, and the names stay positional."

> `crates/ow-tank/src/chunkkind.rs:666-669`
> "The stride and the count are the tiling ones; that the 12 bytes are three
> `f32`s is an observation (they read as map-scale coordinates), not a read
> expression, so this helper is a convenience and not part of the parse."

**Brush tree to mesh (open)**

> `crates/ow-tank/src/brush.rs:6-10`
> "This module reads the brush's own record formats and the two pure
> conversions the client performs on them. The map's chunk route is the other
> producer and has no reader found yet (`docs/formats/maps.md`); neither route
> is complete."

> `crates/ow-tank/src/brush.rs:40-49`
> "# What is not here
>
> * The tree-to-mesh walk (`BrushTreeToMesh`, `0x142cf23c0`) and its hull
>   builder (`0x142cf0360`) are read only as far as the per-node op
>   classification. The six op case bodies and the final triangle emission are
>   **not read**, so nothing here converts a brush to a mesh. That is why
>   [`Node::leaf`] and [`Node::descending`] are recorded fields and not logic.
> * The face conversion `MeshToColliderFaces` (`0x142cf2010`) is read and is
>   already the read expression in `ow_sim::collider::shell`; it is not
>   duplicated here."

**Sphere narrowphase with support points (open)**

> `crates/ow-sim/src/collide.rs:653-656`
> "Only the count `<= 0` path is implemented: the count `> 0` path hands the
> block to `0x142cc9460`, whose body is verified as `fn/ClosestPointGjk`, but
> this leaf's handoff to it is not ported, so a sphere query with support points
> is still open."

**Capsule narrowphase support-point handoff (not ported)**

> `crates/ow-sim/src/collide.rs:2091-2094`
> "The leaf has three paths and this implements the two that do not need the
> support-point routine (the routine itself is verified as
> `fn/ClosestPointGjk`; the handoff inside this leaf is not ported)"

**The id container's growth (not modelled)**

> `crates/ow-sim/src/collide.rs:1049-1055`
> "The client appends the ids into a
> `{+0x200 data, +0x208 capacity, +0x20c count}` container that doubles through
> the allocator `0x142cadee0`, copies with `0x142efe2b0` and frees through
> `0x142cadf00`. That growth is memory management, not a gameplay value: the
> refinement passes an inline 128-entry buffer, so this port returns the ids
> in a `Vec` and does not model the container. The unit pins a capacity above
> every hit count and states the gap."

**Unread narrowphase types**

> `crates/ow-sim/src/collide.rs:1939-1940`
> "/// The type byte names a primitive that has not been read (`2` and `4+`).
> UnreadType(i8),"

**Build stage with no reader (the collider BVH)**

> `crates/ow-sim/src/collider.rs:299-305`
> "/// A build stage whose client code has not been read. The build stops rather
> /// than inventing the missing rule. ...
> /// The surface-area split (`0x142ccb462`-`0x142ccbd37`) found no boundary
> /// with faces on both sides. The client's recovery from that is unread.
> EmptyBinSplit,"

> `crates/ow-sim/src/collider.rs:319-321`
> "/// - otherwise the face-bin split runs (`0x142ccb462`). Its binning is read
> but its cost expression is not, so this path returns [`Unread::BinCost`]."

(stale: no `BinCost` variant exists — see §1.11.)

> `crates/ow-sim/src/collider.rs:85-86`
> "/// The neighbour links at `+0x3c..+0x4c` have no known reader and are not
> modelled."

**The character slide and the slope limit (open)**

> `crates/ow-sim/src/walk.rs:25-33`
> "# What is the client's own
>
> The **slide**: how the residual motion after a contact is composed into the
> next sweep. In the client that is `RunCharacterControllerSweep`
> (`0x142cbcee0` -> `0x142cf3f80`), whose Domino-side body is a shape/pair walk
> that needs the client's pools; the read part of it is the fold above. This
> module's slide is the minimal projection of the residual onto the contact
> plane, in the client's order (`dot`, then subtract the normal component),
> and it is declared open in `docs/open-questions.md`."

> `crates/ow-sim/src/walk.rs:82-86`
> "/// The steepest surface a walk may climb, as the contact normal's up lane. A
> /// contact flatter than this is a wall: the walk slides along it and keeps its
> /// height. The client's own slope limit is not read; `0.7` (45.6 degrees) is the
> /// value that keeps the practice range's 48-degree bevels out of the walkable
> /// set, and it is declared open in `docs/open-questions.md`."

**Range ingest: two deliberate omissions**

> `crates/ow-tank/src/range_world.rs:12-21`
> "Two things are deliberately absent:
>
> - the Domino body/shape handles (`OwnerCreateBody`, `ShapeAddToBody`). They
>   allocate from a running client's pools; the client has no pools, so the
>   shapes are a list here and `ow_sim::walk` sweeps it directly with the
>   verified type-3 narrowphase.
> - the descriptor's mask/filter words. The range's holders all pass
>   `holder_selected` with the world mask all ones, so the shape's layer is
>   the all-ones default the class sets (`model_body::world_mask`)."

**The fan's evaluator, ring flavours and mesh refinement (open)**

> `crates/ow-sim/src/fan.rs:33-40`
> "What is **not** here yet: the evaluator `0x1420328c0` that scores a
> candidate (77 call sites, a general candidate test), pass 0's
> `MoverWishQueryCached` memory, the two ring flavours, and `0x140cc2290`'s
> mesh-face refinement. Those are inputs to this module, not outputs:
> [`accepts_pass1`], [`accepts_pass2`] and [`accepts_refine`] take the
> evaluator's answers as arguments rather than inventing them,
> [`refine_pentagon`] and [`solve`] take solver through [`CandidateSolver`]."

> `crates/ow-sim/src/fan.rs:57-61`
> "The composition here still implements the earlier reading of that order:
> gate, endpoints, threshold, memo, ring -> refine -> snap. Writing the two
> ring flavours and the shape pass is what closes the gap, and the order they
> hang off is no longer a guess."

> `crates/ow-sim/src/fan.rs:1005-1007`
> "The producer's own loop over the offsets, the two acceptance rules and the
> per-candidate refinement are **not** here yet — the refinement block is the
> fan's remaining open read (see `docs/open-questions.md`), so this trait is
> the whole of what item 2 landed."

**The hit policy's geometry object (open)**

> `crates/ow-sim/src/policy.rs:15-21`
> "What is **not** here, because it is data rather than arithmetic: the
> geometry object behind `[world+8]`. The policy reads its table
> (`[geometry+0x80]`), its slot word (`[geometry+0x110]`), its index base
> (`byte[geometry+0x130]`) and its variant table (`[key+0x250]`). Those are
> supplied here as a [`Geometry`] implementation, so the policy can be
> tested against synthetic worlds; a real map needs the collision ingest
> (`docs/formats/maps.md`, cells -> shapes), which is still open."

> `crates/ow-sim/src/policy.rs:452-455`
> "This type is that chain minus the sweep build (which lives inside the
> evaluator and is not modelled yet), so `evaluate` ignores its two endpoints
> and answers with the policy's decision for the descriptor it was constructed
> with."

**Deliberate deviations with a stated justification**

> `crates/ow-sim/src/collide.rs:1973-1976`
> "The client does not bounds-check the shape index or the pool index; neither
> is a state it can be in, so an out-of-range index reports
> [`Narrowphase::Rejected`] rather than reading past the fixture, which is the
> same convention `mesh_sweep` uses for a pointer outside its blob."

> `crates/ow-tank/src/collision.rs:25-30`
> "The client's allocation branches (header mode 0, entry mode 0) are rejected
> here: those paths allocate and then dereference the zeroed allocation, and the
> shipping loader converts the file's entries before this pass runs."

**Acknowledged gaps in the verified units** (`dev/verify_harness/ledger.json`,
field `acknowledged_gaps`) — condensed, collision units only:

- `fn/MeshSweep`: only the count `<= 0` fast-path mesh is verified; the
  support-point path, the slow-enter path (expansion > `0.005`), a BVH axis >= 2
  (whose direction slot is never written), the depth-64 stack realloc at
  `0x142cadee0`, the BVH builder and the geometry ingest remain open. Seven of
  22 branch sites are permanently uncovered (a 61% ceiling).
- `fn/ClosestPointGjk`: branch coverage 74.5% (35 of 47 swept sites); the 12
  uncovered sites are recorded, not declared unreachable; the four-vertex
  reduction `0x142cc7740` is a separate function whose mutants are not in this
  unit; no NaN lane is compared because the emulator's softfloat is not faithful
  to x86 for invalid operations and for `ADDPS`/`SUBPS` with a NaN second
  operand.
- `fn/CollisionSphere`: only `count <= 0`; the count `> 0` path, the other four
  types, the hit policy and the geometry ingest are open.
- `fn/CollisionCapsule`: only the `count <= 0` paths; the support-point path,
  types 2, 3 and 4+, the hit policy and the ingest remain open.
- `fn/MapChunkCellBytesFixup`: only the non-allocating encodings (header modes 1
  and 2/3, entry modes 1 and 2/3); the mode-0 allocation branches are declared
  unreachable; the file-to-runtime conversion of cell entries runs before the
  unit and is not verified; the cell-to-shape decoder, the `0x100`-byte sector
  entries, `ShapeAddToBody` and the live world remain open.
- `fn/MeshGather`: the id container's growth (allocator `0x142cadee0`, copy
  `0x142efe2b0`, free `0x142cadf00`) is **untested**, not verified; the fan
  refinement `0x140cc2290` is read but not implemented.
- `fn/QueryDriver`: the worklist push/free are not modelled (a `Vec` stands in);
  the hit callback is a constant stub; only sphere, capsule and type-3 run; **no
  type-3 leaf** is placed, for two measured reasons (the support-point hull path
  diverges, and the emulator's own result depends on the preceding call); the
  wrapper's return value is not modelled; the realloc path faults offline.
- `fn/ShapeNarrowphase`: types 2 (`0x142cd3c80` via `0x142ccd770`) and 4+
  (`0x142cd0ae0`) are transcribed, but the tree converter `0x142cb5000` is only
  partly read, so the Rust reports `UnreadType`; the sink push
  (`FUN_142cc1450`) and the hit callback are outside the unit.
- `fn/BoxTriangleOverlap`: only the overlap test; the BVH walk `0x142caea70`,
  the record fill `0x142cae760` and the refinement `0x140cc2290` are read but
  not implemented.
- `fn/ShapeWorldTransform`: only the leaf; which pool a real world fills and
  which attach state a real entry carries (`0x142cb3940`/`0x142cd4ef0`) are not
  exercised; NaN fidelity limited as above.
- `fn/TriangleDegenerate`: only the predicate; its two callers (the sector build
  inside `0x142cca460` and the brush path) are unread.
- `fn/CollisionSweepBounds`: only the bounds stage (the driver, the shape
  transform and the five per-type narrowphases now have their own units).
- `fn/BuildBvh`: the harness compares the node array only; the per-node
  neighbour links the appender writes and the finalizer's flattening order are
  exercised by `fn/ColliderFaceAppend`, not here.
- `fn/ShapeDispatch`: type 3 (`0x142cae5a0`) is not implemented (the candidate
  returns false); branch coverage 4/5.

---


## 6. Review checklist: points most worth a second look

1. `mesh_shape::array_header` returns its second argument unchanged
   (`mesh_shape.rs:66-68`) — the tag rule is documented but not applied.
2. `collider::build`'s doc names `Unread::BinCost`, which does not exist
   (`collider.rs:321`).
3. `domino_world::WORLD_TABLE` is `0x1442_0bc0` while its own doc says
   `0x1442a0bc0` (`domino_world.rs:41-43`).
4. `model_collision`'s doc link `[instance_array]` resolves to nothing
   (`model_collision.rs:16`).
5. `walk::advance` contains four non-read behaviours: `SLIDE_ITERATIONS = 3`, the
   `1e-6` remaining-motion break, the `fraction.clamp(0.0, 1.0)`, and the wall
   test `normal[1].abs() < WALKABLE_SLOPE` (`walk.rs:328-395`).
6. `walk::World::floor` and `Advanced::grounded` both hard-code an up-lane
   threshold of `0.5` (`walk.rs:282`, `walk.rs:382`) — not a cited client
   constant.
7. `range_world::RangeWorld::build` prints to stdout inside a library
   (`range_world.rs:496`).
8. `collide::shape_narrowphase` substitutes `Rejected` for the client's
   out-of-bounds read (`collide.rs:1973-1976`) — a documented deviation, and the
   only one in that module.
9. `ow-tank::collision` derives three element strides from an observed tiling.
   The derivation is checked (`ArrayNotTiled`), but it is not a read; a payload
   with the same span and a wrong count is rejected rather than misdescribed,
   which is the correct failure mode.
10. Two "verified" labels cover less than the sentence suggests: `fn/MeshSweep`
    at 61% branch coverage and `fn/MapChunkCellBytesFixup` at 33%. Both carry
    explicit gap lists, reproduced in section 5.
11. `ow-tank::range_world` turns only shape mode `0` into a shape
    (`range_world.rs:389-396`), so point-cloud, capsule and sphere holders are
    reported by `holder_rows` but absent from the world the walk sweeps.
12. `crates/ow-tank/src/brush.rs` has no **production** consumer. The only
    callers of `build_from_instances` outside the module are the `#[cfg(test)]`
    modules of `crates/ow-tank/src/brush_mesh.rs` (line 373) and
    `crates/ow-tank/src/brush_convert.rs` (line 1276), plus the integration
    tests `crates/ow-tank/tests/brush_real.rs:123` and
    `crates/ow-tank/tests/model_collision_real.rs:258`. Nothing in `ow-client`,
    `ow-scene`, `range_world` or `ow-sim` constructs a brush, so the brush
    records are decoded and unit-tested but not yet swept by the live walk.

# Domino: the physics library

Domino is the client's rigid-body physics library: a world, bodies, fixtures,
broadphase trees and a fixed-step island solver. How the local character uses
it — its own mover step, a `dmBody`, or both — is part of the movement
question in `formats/character-controller.md`.

## Current state

- **Read in the current image (2026-10-31):** the world constructor
  (`DmWorldConstruct`), the inv-dt writer (`DmWorldInvDt0Writer`), the substep
  driver (`DmWorldSubstepDriver`), the catch-up loop (`FUN_142cdfc50`), the
  island assembler (`FUN_142cc2510`), the island solve driver
  (`FUN_142cf81d0`), the per-constraint dispatcher (`ConstraintDispatch`), the
  broadphase refit walker (`FUN_142cb3330`) and the residual query
  (`DmResidualQuery`). The world layout, the body stride, the constraint record
  and the solver-pool strides below are each confirmed by two or more
  independent reads (constructor write + consumer read, or instruction-level
  disassembly). Field roles (mass, velocity, ...) are read expressions, not
  guesses; single-function reads are marked as such.
### The collision fixture pool and its management API (2026-11-01)

This is the structure the fan's query walks, read from the client's own
allocator and its release/update paths.

**The pool table.** `record+0x288 + 0x18*i`, one record per pool:

```text
+0x00  pointer to the elements
+0x08  u32  (zeroed by both the constructor and the destructor)
+0x0c  u32  capacity
+0x10  u32  (zeroed by both)
+0x14  i32  free-list head, -1 when empty
```

The eight records, in order, with their element strides:

| index | offset | element | stride |
|---|---|---|---|
| 0 | `0x288` | bodies | `0x1c0` |
| 1 | `0x2a0` | shape transforms | `0x50` |
| 2 | `0x2b8` | **fixtures** | `0xc0` |
| 3 | `0x2d0` | type 0 records (sphere) | `0x20` |
| 4 | `0x2e8` | type 1 records (capsule) | `0x30` |
| 5 | `0x300` | type 2 records | `0x50` |
| 6 | `0x318` | type 3 records | `0x50` |
| 7 | `0x330` | type 4+ records | `0x50` |

`DmWorldConstruct` (`0x142cbeb70`) allocates them from the config, one count
per pool: `config+0x20` bodies (`0x1c0`), `+0x24` owners (`0x50`), `+0x28`
shape entries (`0xc0`), `+0x2c..+0x3c` the five shape-type pools, then
`config+0x40..+0x6c` the auxiliary pools (the read tail shows
`0x3c0` (`0x110`), `0x3d8` (4), `0x3f0` (`0x120`), `0x408` (`0x1d0`),
`0x420` (`0x2e0`), `0x438` (`0x310`), `0x450` (`0x180`), `0x468` (`0x200`);
the middle of the list and its order are not decoded), and `config+0x70`
islands (`0xaa0` at `record+0x4a0`). It takes a
size hint at `config+0x6c` (default `0x400`), computes the arena size when
`config+0x6c < 1`, initialises the four pass records at `record+0x1b0`
(`0x142cf3080`), the containers at `record+0x90` (`0x142cee200`) and
`record+0x8` (`0x142cedfe0`), three auxiliary pools at `record+0xb8`/`+0xd8`/
`+0xf8` with element sizes `0x100`/`0x200`/`0x300` (`0x142cfd110`, counts
default `0x100`/`0x200`/`0x300`, floored at 8), puts the vtable `0x143b2e258`
at
`record+0x170`, and allocates an index array at `record+0x180`
(`config+0x20 * 4` bytes). Then it writes each pool's **free chain**: for the
bodies, `body[i] + 0x00 = i + 1` for `i < count-1` and `-1` at the last
(`0x142cbed50..0x142cbed85`), with the same shape for the other seven.
The destructor's group starts at `0x142cc04f0` (`0x142cc054f` is inside it):
it frees each pointer and resets `{0, 0, 0, -1}`.

**The fixture record**, stride `0xc0` (`index*3 << 6`):

| offset | meaning |
|---|---|
| `+0x00`/`+0x10` | AABB min / max (the query's rejection test) |
| `+0x20` | shape type byte (the narrowphase dispatch) |
| `+0x24` | index into that type's pool |
| `+0x50` | transform index (qword, `-1` when released) |
| `+0x54` | bucket-chain link (dword, `-1` terminates) |
| `+0x88` | the float the tree insert keys on |
| `+0x98` | layer filter |
| `+0xa0` | shape ids — **and the free-list link when free** |
| `+0xa4` | a further index, `-1` = none |
| `+0xa8` | the u16 bucket key |

The free chain lives in the same dword the query reads as the first shape id:
a free fixture has no ids, so `+0xa0` is reused as the next free index. The
used count is `record+0x2c0` and the free head `record+0x2cc` (the pool
record's `+0x14`).

**Release** `0x142cb0d08(owner, record, index)`:

```text
if record[0x168]: virtual_0x08(record[0x168], fixture)
unlink index from the bucket chain: head owner[0x158], link fixture[0x54]
0x142cb83e0(fixture, record)          ; reset the fixture's contents
fixture[0x50] = -1
fixture[0xa0] = record[0x2cc]         ; push onto the free list
record[0x2c0]--                        ; used count
record[0x2cc] = index                  ; new free head
owner[0x15c]--                         ; the owner's fixture count
if owner[0x1ac] == 2: recompute owner[0x30] from the bounds (0x142cb1b90)
```

`0x142cd4555` is the same release without the callback and without the AABB
refresh. `0x142cb83e0` therefore *only* runs on release: the create path does
not use it.

**Update / insert** `0x142cd4610`: it builds the AABB from the shape's bounds
quad using `0x1430a4bf0` (`(1,1,1,0)`) and the sign mask `0x1430a4b40`, then
walks the bucket chain from `[owner+0x24]` calling
`0x142cb9110(fixture, record, &self, &bounds)` per fixture, and for each with
`fixture+0xa4 != -1` calls `0x142cf32d0(record+0x1b0, fixture+0x88, fixture)`;
it ends with `0x142caf970(record+0x1b0, record)`. `0x142cb3330(owner, record)`
does the tree-insert pass alone, and the `0x142cb3245` chunk does the
leaf-insert pass over one chain.

**Correction (2026-11-01): `0x141d71200` is a constructor, not the fill.**
The earlier note read its stores to `object+0x2b8`/`+0x2d0..+0x330` and
`+0x31f8` as a geometry fill. Decompiled, it is the class constructor: it
zero-fills the object (`param_1[0..0x88e]`, i.e. ~0x45a0 bytes, with a loop
block in the middle), stores the vtable `0x14331c570` at `+0x00`, and ends in
the allocation stub `FUN_141c23430(+0x3c38, 0xa0)`. Every pool/handle store it
makes is a **zero**: `+0x31f8` is cleared, not published. Its only caller is
the thunk `0x1404ee530`, and that thunk sits at slot `+0x40` of
`0x142f744a0`, which is a **constructor/thunk table** (0x142f744a0 is
referenced by nothing in the image; the neighbouring slots are constructors
and destructors of the same module), not an instance vtable. So
`0x141d71200` says nothing about who fills a world.

**The fill path is read now, one level down (2026-11-01).** The pools are
allocated in bulk by `DmWorldConstruct`, so there is no per-element free-list
pop at create time; the per-element allocator is
`FUN_142cb00c0(pool_record)` (name `ShapePoolAlloc`):

```text
if pool[0x14] (free head) == -1:                       ; grow
    new_cap = pool[0x0c] / 2 + pool[0x0c]; if < 2 -> 2
    new = alloc(new_cap * stride); zero(new); memcpy(old, pool[0x08] * stride)
    pool[0] = new; pool[0x0c] = new_cap
idx = pool[0x14]
pool[0x14] = entry(idx)[link_offset]                   ; the type's own link field
entry(idx)[link_offset] = idx
pool[0x08]++                                           ; used count
pool[0x10] = max(idx + 1, pool[0x10])                  ; high-water mark
return idx
```

The link offset is the type's own field (bodies +0x00, owners +0x20, shape
entries +0xa0, type 0 +0x14, type 1 +0x24, types 2/3/4 +0x4c; the same dwords
the release path reads). This is the "pop" the earlier note searched for; it
is shared by every pool because each entry carries its own next index in its
first free dword.

`FUN_142cb7f10(entry, record, source)` (`ShapeEntryFromDescriptor`) is the
geometry writer. It copies the source descriptor's bounds quad, filter words,
ids, radii and flags into the 0xc0-byte shape entry, then switches on the
source's **type byte at `source+0x67`** and allocates the per-type pool
record:

| type | pool | stride | allocator | fill |
|---|---|---|---|---|
| 0 | `record+0x2d0` | 0x20 | inline free-list pop | `param_3[0x0e..0x11]` + radius = `|vec3|` |
| 1 | `record+0x2e8` | 0x30 | `FUN_142cb5e80` | `param_3[0x0e..0x12]` |
| 2 | `record+0x300` | 0x50 | `FUN_142cb6100` | `FUN_142cd31f0(pool, param_3[0x0d], param_3+0x0e, param_3[0x12], record+8)` |
| 3 | `record+0x318` | 0x50 | `FUN_142cb6290` | `FUN_142cae540(pool, param_3[0x0e], param_3[0x0d])` |
| 4+ | `record+0x330` | 0x50 | `FUN_142cb5fc0` | `FUN_142ccf330(pool, param_3[0x0d], *(u16*)(param_3+0x0c))` |

The **source descriptor** is at least 0x98 bytes and is read at these offsets:

- `+0x00/+0x10` bounds quads, `+0x20..0x4c` filter words / ids / radii,
  `+0x5c`/`+0x5e`/`+0x60`/`+0x62` the u16 ids, `+0x64..0x67` the flags and the
  **type byte** (`+0x67`), `+0x88..0x94` floats (radius, height, scale).
- Type 0 (sphere): `+0x68` is 40 bytes of `{centre quad, radius}` (the builder
  takes `|vec3|`). Type 1 (capsule): `+0x68` is 40 bytes of two quads. Types
  2/4: `+0x68` is a pointer to their own source block and `+0x60` a u16.
- **Type 3 (mesh)**: `+0x68` is a **float scale**, `+0x70` a pointer to a
  **0x30-byte mesh source** (`MeshShapeFill`, `0x142cae540`):

  ```text
  +0x00 u64  -> pool+0x20
  +0x08 u64  -> pool+0x28
  +0x10 u64  -> pool+0x30   the block the type-3 narrowphase walks
  +0x18 u32  count          (pool+0x38)
  +0x1c f32  -> pool+0x3c
  +0x20 f32  -> pool+0x40
  +0x24 f32  -> pool+0x44
  +0x28 f32  -> pool+0x48 (scaled by the descriptor's scale)
  +0x2c f32  -> pool+0x4c
  ```

  The pool record also stores `scale` at `+0x00..0x0c` and `1/scale` at
  `+0x10..0x1c`, so the narrowphase can work in either space. Type 2
  (`ShapeFillType2`, `0x142cd31f0`) likewise copies a vertex array
  (`+0x00`), an index/flag source (`+0x08`), and counts at `+0x2c`/`+0x30`,
  scaling by the descriptor's scale; its pool record holds an allocated
  vertex array (`+0x20`), a transformed copy (`+0x30`), a byte array
  (`+0x40`) and a u32 array (`+0x50`).

`FUN_142cb3940(body_handle, source)` (`ShapeAddToBody`) and
`FUN_142cd4ef0(owner_handle, source)` (`ShapeAddToOwner`) are the two
"add a shape to a world" entry points: each decodes a handle, checks the
generation, allocates a shape entry, calls `FUN_142cb7f10`, appends the entry
to the body's/owner's chain (`+0x50`/`+0x54`, or `+0x56`/`+0x57`), refreshes
bounds and returns the new entry's handle `{index, generation}`. Their callers
are the character/actor collision setup (`0x1420c63b0`, `0x142cf35a0`) and
`0x14201b...`, not yet a map loader.

So the remaining unknown is **the producer of the `source` descriptors**: who
turns the map's collision chunk (see `formats/maps.md`, "The collision chunk
payload") into the descriptors this builder consumes. The builder reads the
source up to `+0x93`, so the descriptor is at least 0x98 bytes; its exact size
and field hashes are not yet read.

**The cell array's runtime form (2026-11-01, observed under the oracle).**
`MapChunkCellBytesFixup` (`0x141ff51a0`) walks the collision chunk's
`m_cellBytes` field (`instance+0x68`, a 16-byte `{ptr; mode}` header) and
converts 17 self-relative qwords per cell record. Running it under the unicorn
oracle with synthetic headers settles the encoding (`dev/probe_cellbytes.py`):

- **mode 1** works: count is the `u32` at `header+8`, the entry array is
  `*(u64*)header`, entries are 16 B `{Q, 1}` and the record is `*(u64*)Q`;
- **mode 2/3** (count at `header+0`, entries at `*(u64*)(header+8)`) take the
  entry's second qword as the mode, and only `1` means "Q is the record"; any
  other non-zero value reads the record pointer from `*(u64*)(Q+8)`;
- entry second qword `0` aborts (`FUN_141baff30`), so the file's raw
  `{offset, 0}` cells must be converted before the fixup runs;
- a record offset of `0` becomes a null pointer; every other value becomes
  `record + offset`, in all 17 slots.

So the decoder emits, per cell, a 16-byte reference cell whose first qword is a
"tag" and whose second qword is the record pointer, and the array header holds
a pointer to those cells plus the count. That matches the header the payload
carries: at `container+0x2428` the header is `{u32 220; u32 0; u32 0x2438}` and
the entries at `0x2458` are `{u64 cell_off; u64 0}` pointing at cells
`{u64 block_size; u64 record_off}` — i.e. the file stores the tag as the block
size, and the loader turns the second qword into the record pointer.

**The fixup is verified (2026-11-02).** `ow_tank::collision::fixup_cell_bytes`
reproduces `0x141ff51a0` operation for operation, and the differential unit
`fn/MapChunkCellBytesFixup` runs it against the real bytes: 264 iterations plus
the edge pool, every pointee byte compared after the call, 12/12 mutants
killed. The "header" in the two encoding bullets above is the **inner** header
the field's first qword points at; the field's second qword is the outer mode
that selects the inner layout. The unit covers outer mode 1 and 2/3 and entry
mode 1 and 2/3. The two mode-0 branches allocate with `Alloc` and then read
the zeroed allocation; they are declared dead, and the shipping loader converts
the file's `{offset, 0}` entries before this pass (that conversion is still an
observation, not a verified unit).

**Mesh -> collider faces (2026-11-01, `0x142cf2010`).** The type-3 source is
produced from a triangle mesh: input is `{vec4 vertices @0x00, 0x10 stride,
count @0x08; faces @0x10, 0x18 stride of three `{i32 index; i32 link}` pairs,
count @0x18}` where `link == -1` marks an **open edge**. The builder emits
`2*count` vertices (`z + thickness` and `z - thickness`, i.e. a slab) and
`2*count` faces of **0x14 bytes** `{i32 i0; i32 i1; i32 i2; u16 id; u16 pad;
u32 flags}`: every face plus its reversed copy offset by `count`, plus two
side triangles per open edge. `FUN_142ccc120(entry+0x20, ...)` then fills the
0x30-byte type-3 mesh source from `{vertices, faces, count}`.

**Brush tree -> mesh (2026-11-01, `0x142cf23c0`).** The brush CSG walk reads
`node+0x28` as an op code 0..5 over **0x60-byte hulls**, maintains a 0x30-byte
vertex pool and a 0x60-stride hull array, and emits the mesh into
`{vertices @0x00, count @0x08, faces @0x10, count @0x18}`. That mesh is what
`0x142cf2010` thickens, so the two functions are the whole "brush -> collider"
producer for node-type-2 geometry.

**The world create is read** (`0x142cc3c80`). It is the only caller of
`DmWorldConstruct`:

```text
walk the handle table 0x1442a0bc0 for a slot holding the sentinel 0xc439701cd97ec6aa
  (or past the used count 0x1442a0bd0); the index found is the handle
record = 0x142cadee0(size = 0x4d0, align = 0x10, no-init)   ; the record is 0x4d0 bytes
DmWorldConstruct(record, config)
record = record ^ 0x77600ba5adcc265c, then + 0x4cd964772bb2a04e   ; the encoded value
table[handle] = encoded
record[0x194] = (u16)handle                 ; the record's own self-index
0x1442a0bd0++                              ; the used count
if 0x1442a0bc8: 0x142694c50(...)           ; the registry's hook
return (u16)handle
```

`0x142cc3d90(handle)` is the inverse: it rejects `handle > 0x3fff` and the
sentinel, then decodes. A world is therefore an `0x4d0`-byte header plus its
eight pools, addressed by a `u16` handle.

**The geometry object owns two worlds.** `0x14202c660` initialises the object
the fan reads at `[world+0x620]`:

```text
object[0x317c] = 0x8000000c
object[0x31e0] = 3 * helper()
object[0x31e8] = 0x1433676b0          ; an interface pointer
object[0x31f0] = owner
object[0x31f8] = -1 (invalid handle)   ; world A
object[0x31fa] = -1                    ; world B
object[0x31fc] = [0x1433674e8]
object[0x3204] = 0, object[0x3206] = 1
[owner+0x620] = object
if config = owner_or_config[0x18]:
    [owner+0x630] = *(config + 0x10)   ; 16 bytes
    object[0x31f8] = 0x142cc3c80(config)   ; world A
    object[0x31fa] = 0x142cc3c80(config)   ; world B
    object[0x31fc] = 0x142cc3d90(object[0x31f8])
    0x142cc4af0(object[0x31f8], &object[0x31e8], 0x143aa0238)   ; install the interface
```

So a single geometry delegate creates **two** worlds from one config, and the
fan's `[geometry+0x31f8]` is the first of them. The query side therefore never
sees an empty world by accident: it creates it, then installs an interface on
it — that interface is where the fixtures come from.

**The owner's initialise step and the instantiation path (2026-11-01).**
`0x14202c660` is the **world/geometry owner's initialise**, not just the
geometry: it writes the owner's fields (`+0x450`, `+0x454`, `+0x700`,
`+0x750`, `+0x820`...), and only when `[owner+0x454] & 0x200` does it allocate
the **0x3210-byte** geometry object and store it at `owner+0x620`. That
object is `0x3160` base + `0xb0` interface: `FUN_14209a8f0` initialises the
base (128 slots of `0x60` at `+0x60`, four pools of `0x100` at
`+0x10`/`+0x20`/`+0x30`/`+0x40`), the vtable `0x143367678` goes at `+0x3160`,
`0x14209ae30(geometry, worldA_handle, arg)` registers it and sets its
`+0x50`, the two worlds come from `config[0x18]` via `0x142cc3c80`, and
`0x142cc4af0(worldA, &geometry[0x31e8], ...)` sets `worldA[0x168]`. The
class-info record at `0x143367670` is `{ctor 0x14202c660, 0x142028270,
0x14209d850, 0x14209d900, 0x14209d9b0, 0x14209da60, 0x14209db10,
0x14209bf50}` (the last six are job-system deferral/cleanup helpers, not
collision), and the interface vtable at `0x1433676b0` is just destructor +
two pure slots.

The base class is built by `FUN_1420271d0` (0x3160 bytes, vtable
`0x143367630`, fields `+0x5c0..+0x8dc` including the `0x41a00000`/`0x3f800000`
constants and the `0x100ffff` flag pair). Its allocator `FUN_1420266d0`
(`0x1420266d0`) inlines that constructor and takes the object memory from a
per-type pool (`0x141c4b270(type+0x10)`), and its caller is the registry:
`FUN_1404f6e10` registers `FUN_142029f00` as node type 2 in `0x141c5ff40`.
`FUN_142029f00` allocates a 0x30-byte type descriptor (vtable `0x1433654a0`)
and registers a **0x910-byte** class through `0x141c4b050` →
`TypeRegistrationCore`. The descriptor's slot 2 (`0x1407803f0`) is the
**release** path, not an instantiate: it de-obfuscates the instance handle,
calls the instance's first vtable entry, and returns the memory through the
allocation strategy at `[type-info+0x10]` (slot `0x58`, `0x141c5f420`, a
per-block free-list return; the allocator is the same strategy's slot `0x50`,
`0x141c5ef30`). The class's creation is slot 1 (`0x1420266d0`) followed by the
node's slot 8 (`0x14202c660`) with the record; the "key that selects the
geometry class" is withdrawn in `formats/static-collision.md` check 17.

**The allocation is found (2026-11-01).** The pop is `FUN_142cb00c0`
(`ShapePoolAlloc`, see the fill-path section above). It takes **the pool
record's address** (`record+0x2b8`, `+0x2d0`, ...), not the world record, which
is why the earlier searches failed: the free head is `pool[0x14]` (i.e.
`record+0x2cc` for the shape pool) and the next-index link is the entry's own
type-specific dword (`+0xa0` for shape entries), not a separate field. The
allocator pops one index (`idx = free_head`, `free_head = entry[idx].link`),
bumps the used count and the high-water mark, and grows by `cap/2 + cap`
(min 2) when the list is empty. It is reached from `ShapeEntryFromDescriptor`
(`0x142cb7f10`), the per-type allocators and `ShapeAddToBody`/`ShapeAddToOwner`.

- **Read (2026-11-01):** `DmBroadphaseReinit` (`0x142cbdc60`) is a BVH
  builder over the pass sub-records, not an ingest: it takes
  `record+0x1b0` (the four `0x28`-stride pass records), reads the query
  descriptor's eight quads, calls `0x142cd5540` for the swept bounds, and
  writes each pass's root index at `+0x44` through a `0x100`-entry stack node
  pool. `-1` in a slot means "no tree". Its callers arrive through
  `0x142cc48d0` (the handle-based cast wrapper), so the tree is refreshed on
  demand from whatever the fixture pool holds.
- **Open (2026-11-01 navigation):** the handle registry's owner is an engine
  object installed at `0x142cc3bb0` (the only writer of the `0x1442a0bc0`
  table); `0x142cc3c80` tears it down and `0x142cc3be0` resets a record's
  bodies. The fixture/geometry fill is world-side, not handle-side, and is
  still unread. The dedicated `dmBody` mass/fixture writer (no single body
  constructor was located in this pass; bodies are zeroed pool records whose
  fields are filled by paths not yet attributed), the consumer of
  `m_inv_dt0`, the numeric tick pair (the dt scalar and substep/iteration
  counts flow in from above `FUN_14208c190` and are not read), and the closed
  form of `RotationInertiaSolve` (a conditioned-sweep solver, verified not the
  integrator).

## The current-image functions

| function | address | role |
|---|---|---|
| `DmTickKeeper` | `0x14208c070` | the frame-side tick keeper: decides how many steps run |
| `DmWorldConstruct` | `0x142cbeb70` | the world constructor |
| `DmWorldInvDt0Writer` | `0x142cc3050` | writes `m_inv_dt0` from the step's dt |
| `DmWorldSubstepDriver` | `0x142cc3630` | the substep driver |
| `DmBroadphaseReinit` | `0x142cbdc60` | re-initialises the broadphase |
| `ConstraintDispatch` | `0x142cd9c90` | dispatches a constraint to its solver |
| `RotationInertiaSolve` | `0x142d07170` | the rotational inertia solve |
| `DmResidualQuery` | `0x142cdf710` | the residual query that ends a catch-up loop |

The class descriptors and the body/world layout are derived below from the
constructors and the consumers operation for operation. Every address is a
current-image address (`0x140000000` + RVA). No offset from the earlier
capture-era notes is reused.

## `dmWorld`: pool layout from `DmWorldConstruct` (`0x142cbeb70`)

The constructor takes a config struct (`param_2`) and the world (`param_1`).
Each pool is one `FUN_142cadee0` allocation, zeroed by `FUN_142efe950`, with an
index free-list chain (`0, 1, ...`, `0xffffffff` terminator) written into the
records. Pool record strides reappear exactly as the solver-type strides in
`ConstraintDispatch` (`0x142cd9c90`), which is the second read.

| world offset | content | stride | config count at | second read |
|---|---|---|---|---|
| `+0x8` | list head | — | — | `DmTickKeeper` walks `*(+0x8)+0x2a8` (`0x14208c0cb`) |
| `+0x90` | allocator | — | — | island temp allocs in `FUN_142cc2510` |
| `+0xb8`/`+0xd8`/`+0xf8` | free lists (`0x100`/`0x200`/`0x300`) | — | — | — |
| `+0x118` | init via `FUN_142694c30` | — | — | — |
| `+0x120..0x158` | island lists | — | — | — |
| `+0x15c` | u32 lock word | — | — | `FUN_142cc4cd0` (`0x142cc4cd0`) |
| `+0x160`/`+0x168`/`+0x170`/`+0x178` | job/vtable pointers (`+0x170` = `PTR_PTR_143b2e258`) | — | — | job calls via `+0x170` in `FUN_142cc2ae0` |
| `+0x180` | u64 array, `config+0x20` entries | 4 | `+0x20` | `FUN_142cc2ae0` body-index source |
| `+0x188` | i32 count (`config+0x20`) | — | `+0x20` | `FUN_142cc2ae0` loop bound |
| `+0x18c` | u32 step counter, reset at `0x7fffffff` via `FUN_142cc1820` | — | — | touch-mark compare in `FUN_142cc2510` |
| `+0x190` | u8 broadphase-dirty; cleared after `FUN_142caf970(+0x1b0)` | — | — | read + cleared in both `DmWorldInvDt0Writer` (`0x142cc307c`/`0x142cc30a1`) and `DmWorldSubstepDriver` |
| `+0x191` | u16 `0x100` | — | — | — |
| `+0x192` | u8 phase gate for `FUN_142cc18c0` | — | — | `DmWorldInvDt0Writer` (`0x142cc313f`) |
| `+0x194` | u16 `0xffff` | — | — | — |
| `+0x198` | f32 `m_inv_dt0` = lane 0 of `1/dt`, stored only when all four dt lanes are `> 0` | — | — | store insn `0x142cc31ed`; consumer open |
| `+0x19a..` | (padding) | — | — | — |
| `+0x1a0..0x1ac` | f32×4 gravity, copied from config `+0x10..0x1c` | — | `+0x10` | velocity integrate in `FUN_142cf81d0` |
| `+0x1b0` | broadphase (`FUN_142cf3080` inits; `FUN_142caf970`/`FUN_142cf3330` step it) | — | — | stepped in `DmWorldInvDt0Writer` (`0x142cc31f5`), the substep driver and `FUN_142cc2510` |
| `+0x288` | **body pool base**, `config+0x20` records | **`0x1c0`** | `+0x20` | `IMUL 0x1c0` in dispatch (`0x142cd9ca7`), substep (`0x142cc36f9`), island (`FUN_142cc2510`), solve (`FUN_142cf81d0`) |
| `+0x294` | i32 body capacity | — | — | bounds check `0x142cc36ea` |
| `+0x2a0` | pool, stride `0x50`, count `config+0x24` | `0x50` | `+0x24` | — |
| `+0x2b8` | pair pool, stride `0xc0`, count `config+0x28`; per-record next at `+0x54`, link at `+0x48`/`+0x50` | `0xc0` | `+0x28` | pair walks in `FUN_142cc2510` and `FUN_142cb3330` |
| `+0x2d0` | pool, stride `0x20`, count `config+0x2c` | `0x20` | `+0x2c` | — |
| `+0x2e8` | pool, stride `0x30`, count `config+0x30` | `0x30` | `+0x30` | — |
| `+0x300` | pool, stride `0x50`, count `config+0x34` | `0x50` | `+0x34` | — |
| `+0x318` | pool, stride `0x50`, count `config+0x38` | `0x50` | `+0x38` | — |
| `+0x330` | pool, stride `0x50`, count `config+0x3c` | `0x50` | `+0x3c` | — |
| `+0x348` | constraint-link pool, stride `0x58`, count `config+0x40+0x44`; fields `[0]`/`[1]` island indices, `[2]` step stamp, `+0x51`/`+0x52` flags | `0x58` | `+0x40` | island link walk in `FUN_142cc2510` |
| `+0x360` | pool, stride `0x18`, count `config+0x40` | `0x18` | `+0x40` | — |
| `+0x378` | pool, stride `0x40`, count `config+0x44` | `0x40` | `+0x44` | — |
| `+0x390` | **constraint pool**, stride `0xe0` (see record below) | `0xe0` | — | 0xe0-stride dispatch loops in `FUN_142cf81d0` and `FUN_142cc2ae0` |
| `+0x3a8` | type-1 solver pool, count `config+0x48` | `0x60` | `+0x48` | dispatch case 1: `+slot*0x60 + [+0x3a8]` (`0x142cd9d7c`) |
| `+0x3c0` | type-2 solver pool, count `config+0x4c` | `0x110` | `+0x4c` | dispatch case 2: `IMUL 0x110` (`0x142cd9d9b`) |
| `+0x3d8` | pool, stride 4, count `config+0x50` (no solver case 3 exists) | 4 | `+0x50` | — |
| `+0x3f0` | type-4 solver pool, count `config+0x54` | `0x120` | `+0x54` | dispatch case 4 (`0x142cd9dd4`) |
| `+0x408` | type-5 solver pool, count `config+0x58` | `0x1d0` | `+0x58` | dispatch case 5 (`0x142cd9e01`) |
| `+0x420` | type-6 solver pool, count `config+0x5c` | `0x2e0` | `+0x5c` | dispatch case 6 → `RotationInertiaSolve` (`0x142cd9e39`) |
| `+0x438` | type-7 solver pool, count `config+0x60` | `0x310` | `+0x60` | dispatch case 7 (`0x142cd9e5d`) |
| `+0x450` | type-8 solver pool, count `config+0x64` | `0x180` | `+0x64` | dispatch case 8 (`0x142cd9e8c`) |
| `+0x468` | type-9 solver pool, count `config+0x68` | `0x200` | `+0x68` | dispatch case 9 (`0x142cd9eaf`) |
| `+0x480..0x498` | spare pools (`config+0x20`) | — | `+0x20` | — |
| `+0x4a0` | island pool, stride `0xaa0`, count `config+0x70` | `0xaa0` | `+0x70` | island struct (`+0x100` ref array, `+0x10c` count) consumed by the substep driver |
| `+0x4a8` | i32 island capacity (`config+0x70`) | — | `+0x70` | — |
| `+0x4c4`/`+0x4c5`/`+0x4c6` | u8 flags from config `+0x75`/`+0x74` (fixed `1` at `+0x4c5`) | — | `+0x74` | `+0x4c5` read in both dt paths (`0x142cc30e2`, `0x142cc374b`); `+0x4c6` gates `FUN_142cc2ae0` (`0x142cc3191`) |
| `+0x400` | u8 set when the constraint-sum path is taken | — | — | — |

## `dmBody`: stride `0x1c0`, field offsets with types

Record base `world+0x288`, index `i`, byte `base + i*0x1c0`. Two-read
(constructor + consumer, or two consumers) entries are the load-bearing schema;
entries read in only one function are marked `(1 fn)` and need a second read
before use.

| offset | type | role, with reads |
|---|---|---|
| `+0x0` | u32 | index / free-list link: constructor writes the chain; substep validates `== idx` (`0x142cc3707`); dispatch indexes it (`0x142cd9ca7`) |
| `+0x10..0x1c` | f32×4 | aux vector written by the solve writeback (cross-product terms) `(1 fn)` |
| `+0x20..0x2c` | f32×4 | quaternion copy: writeback stores solver quat; `RotationInertiaSolve` reads it as `param_2/param_3 +0x20` |
| `+0x30..0x3c` | f32×4 | linear velocity: velocity stage reads; writeback stores; catch-up zeroing stores `(1 fn: `FUN_142cf81d0`)` |
| `+0x40..0x4c` | f32×4 | angular velocity: velocity stage reads; writeback stores `(1 fn)` |
| `+0x50..0x5c` | f32×4 | solver input copy, read at solve-driver head; kinematic branch stages `+0x50..0x6f` through `+0x80..0x9f` `(1 fn)` |
| `+0x60..0x6c` | f32×4 | orientation quaternion: head read builds the inertia basis; writeback stores `(1 fn)` |
| `+0x70..0x7c` | f32×4 | inertia diagonal: `RotationInertiaSolve` reads `body+0x70`; writeback reads it |
| `+0x80..0x9f` | f32×8 | staged previous step (kinematic branch) / zeroed (dynamic branch) `(1 fn)` |
| `+0xb0..0xdc` | f32 3×3 | rotation rows used when `+0x1b7` is set `(1 fn)` |
| `+0xf0..0xff` | f32×4 | position: dispatch copies `+0xf0` to the constraint (`0x142cd9cdb`); solve driver reads it |
| `+0x100..0x10f` | f32×4 | torque-ish input to the inertia sandwich `(1 fn)` |
| `+0x110..0x11f` | f32×4 | velocity input: dispatch copies to constraint `+0x70` (`0x142cd9cfa`); solve driver reads it |
| `+0x120..0x12f` | f32×4 | dispatch copies to constraint `+0x80` (`0x142cd9d05`) `(1 fn: dispatch)` |
| `+0x130..0x13f` | f32×4 | force: `V += (... + F) * invMass * dt`; zeroed on sleep `(1 fn)` |
| `+0x140..0x14c` | f32×4 | torque: angular integrate reads `+0x140`; zeroed on sleep `(1 fn)` |
| `+0x158` | i32 | pair-list head into `world+0x2b8` (`-1` empty): island walk (`FUN_142cc2510`) and refit walk (`FUN_142cb3330`) |
| `+0x160` | u32 | island link into `world+0x390` (`0xffffffff` empty) `(1 fn)` |
| `+0x164`/`+0x168`/`+0x16c` | f32 | mass factors: `V += (([0x164]·[0x16c])·G[world+0x1a0] + F[0x130]) · [0x168] · dt + V[0x30]`; dispatch copies `[0x168]` to constraint `+0x90` (`0x142cd9cbf`) |
| `+0x170`/`+0x174` | f32 | damping: `min(1, max(0, 1 - [0x174]·dt))` scales `V`, `min(1, max(0, 1 - [0x170]·dt))` scales `W` `(1 fn)` |
| `+0x17c` | f32 | sleep residual: accumulates `dt` while slow, reset when fast `(1 fn)` |
| `+0x190` | u32 | island touch-mark = world step counter `+0x18c` `(1 fn: `FUN_142cc2510`)` |
| `+0x198` | u16 | generation: substep checks `== ref>>0x20` (`0x142cc3711`); writer open |
| `+0x1ac` | u8 | kind: `2` static (zeroed velocities, skipped by integrate), `1` kinematic, else dynamic: solve driver + island tail + pair walk |
| `+0x1ad` | u8 | sleep-enable `(1 fn)` |
| `+0x1b2` | u8 | pair-class filter in `FUN_142cc2ae0` `(1 fn)` |
| `+0x1b3` | u8 | active flag: substep sets `1` during validation (`0x142cc3723`), clears after; island requires it |
| `+0x1b5` | u8 | `!= 0` skips the angular π/4 clamp `(1 fn)` |
| `+0x1b7` | u8 | `!= 0` takes the 3×3-matrix inertia branch `(1 fn)` |

## Constraint record: stride `0xe0` in `world+0x390`

Filled by `ConstraintDispatch` (`0x142cd9c90`) from the two bodies
(`bodyA = base + [c+0xb0]*0x1c0`, `bodyB = base + [c+0xbc]*0x1c0`; insns
`0x142cd9ca0..0x142cd9cb8`):

- `+0xa4` u8 type (cases 1, 2, 4, 5, 6, 7, 8, 9; no case 3 — matches the
  stride-4 pool at `world+0x3d8` having no solver pool use).
- `+0xa8` i32 solver-slot index into the per-type pool.
- `+0xb0`/`+0xbc` i32 body indices.
- `+0x90`/`+0x94` f32 from `bodyA+0x168` / `bodyB+0x168` (`0x142cd9cbf`).
- `+0x10..0x48` scratch = `bodyA+0xf0..0x12f` (64 bytes).
- `+0x50..0x68` scratch = `bodyB+0xf0..0x10f`; `+0x70..0x7c` = `bodyB+0x110..0x11c`;
  `+0x80..0x8c` = `bodyB+0x120..0x12c`.
- `+0xa0` u32 step stamp, `+0xb4` u32 link, `+0xc8`/`+0xcc` u32 island-local
  indices (second read: `FUN_142cc2510` / `FUN_142cc2ae0` walks).

## The tick, the substep and the integration order

1. **Tick keeper** `DmTickKeeper` (`0x14208c070`, insns `0x14208c078..0x14208c181`):
   return if `[+0x40] == 0`; else `max(+0xef8, +0xefc)` sets/clears flag
   `[+0xf08] & 0x10` and zeroes `+0xef8`; then recurse into children
   (`*(+0x8)+0x2a8`, next `+0x2a0`, eligible bit `0x1000000000000` at `+0x280`)
   whose time (`+0x268-0x20 → +0x128 → +0x40`, via `FUN_142cdfc00`) is behind.
2. **Catch-up loop** `FUN_142cdfc50` (`0x142cdfc50`): while the residual query
   minimum is `< 0.5` and fewer than `param_3` iterations, run one step
   (`FUN_142cc4cd0` → `DmWorldSubstepDriver`, or `FUN_142cc4bb0`) then
   `DmResidualQuery`. Threshold `0.5` is the loop bound, not a per-body test.
3. **Substep driver** `DmWorldSubstepDriver` (`0x142cc3630`): the dt scalar is
   broadcast to a 4-lane vector; return early if any lane is `0`
   (`0x142cc367a..0x142cc367d`); validate each island ref
   (`idx < +0x294` at `0x142cc36ea`, `body+0 == idx` at `0x142cc3707`,
   `body+0x198 == ref>>0x20` at `0x142cc3711`); set `body+0x1b3 = 1`
   (`0x142cc3723`); compute `inv_dt = (1,1,1,1)/dt` lane-wise with the
   `1.17549435e-35` denormal guard (ones vector at oracle `0x142fa62f0`,
   guard at `0x143391030`); call `FUN_142cc2510` `param_5` times; clear the
   `0x1b3` flags (`0x142cc37dc`).
4. **Inv-dt writer** `DmWorldInvDt0Writer` (`0x142cc3050`): same lane-wise
   reciprocal; stores lane 0 to `world+0x198` only when all dt lanes are `> 0`
   (`0x142cc31dc..0x142cc31ed`); the `+0x192` phase calls `FUN_142cc18c0`
   (`0x142cc3173`), the `+0x4c6` phase calls `FUN_142cc2ae0` (`0x142cc31be`);
   both phases end in the broadphase step `FUN_142cf3330(+0x1b0)`
   (`0x142cc31f5`).
5. **Island step** `FUN_142cc2510` (`0x142cc2510`): bump `world+0x18c`;
   assemble the island (touch-mark `body+0x190`, pair walk via `world+0x2b8`,
   link walk via `world+0x348` validated by `FUN_142cc56b0`, second list via
   `world+0x390`); run the solve driver; broadphase-refit each moved body
   (`body+0x1ac != 2` → `FUN_142cb3330`, which walks `body+0x158` and calls
   `FUN_142cf32d0` on `world+0x1b0`); sync the broadphase
   (`FUN_142caf970(+0x1b0)`); free the temp lists.
6. **Solve driver** `FUN_142cf81d0` (`0x142cf81d0`), in order:
   - velocity integrate per dynamic body: `V += (([0x164]·[0x16c])·G + F) ·
     [0x168] · dt` with `G = world+0x1a0`, `F = body+0x130`, dt = step lanes
     `+0x50..0x5f`; angular integrate through the inertia basis built from
     `body+0x60` quat and `body+0x70` diagonal with `body+0x140` torque;
     damping clamp `min(1, max(0, ...))` from `body+0x174` (linear) /
     `body+0x170` (angular) against the ones vector (`0x14306ad20`); NaN-guard
     (`& 0x7f800000 == INFINITY` → zero) — oracle zeros at `0x142f84a00`.
   - one `ConstraintDispatch` pass over the `0xe0` records, then
     `FUN_142d1a220`; `param+0x80` iteration rounds of `FUN_142cd9ff0` +
     `FUN_142d1bec0`; `FUN_142d1c690`.
   - position integrate with clamps: NaN-guard; `|v·dt|² > 16.0` scales by
     `4.0/|v·dt|` (oracle `0x142f84b70`); `|w·dt|² > 0.6168503` scales by
     `(π/4)/|w·dt|` (oracle `0x1435ed080`; `0.6168503 = (π/4)²`) unless
     `body+0x1b5`; `p += v·dt`; `q += 0.5·dt·(w×q)` normalised when
     `|q|² > 1.17549435e-35`; writeback to `body+0x50..0x6c` /
     `body+0x20..0x2c` and the aux vector at `body+0x10..0x1c`; store
     `body+0x30..0x4c` when NaN-free; `FUN_142cb30f0` per body.
   - `param+0x84` final rounds of `FUN_142cd9ef0` + `FUN_142d1b8e0`
     (`0x3e4ccccd` = `0.4` twice); sleep pass when `param+0x89` is set:
     per-body `|W|² > 0.007615436` resets residual `body+0x17c`, else
     `|V|² > 0.0025000002` (`0.05²`) resets it, else `body+0x17c += dt` and
     the island minimum decides; if the minimum is `< 0.5` velocities, forces
     (`+0x130`) and torques (`+0x140`) are zeroed.
7. **Residual query** `DmResidualQuery` (`0x142cdf710`, insns
   `0x142cdf727..0x142cdf8ae`): per island-group item (stride `0x40`,
   `count = +0x24`, `base = +0x18`): handle at `+0x34` via `FUN_142cb4140`,
   vector via `FUN_142cb3d00`; fast (`|v|² > 0.007615436`) writes residual
   `0.0`, else slow (`|v|² > 0.0025000002`) accumulates `dt` into `+0x20`;
   tracks the minimum in `*param_3`; recurses via `+0x140`/`+0x150`.
8. **Rotation-inertia solve** `RotationInertiaSolve` (`0x142d07170`): case-6
   solver reached from dispatch (`0x142cd9e39`); reads body quaternions
   (`+0x20`), inverse-mass pair (`constraint+0x90/+0x94` ← `body+0x168`),
   inertia diagonals (`body+0x70`) and the type-6 state
   (`slot*0x2e0 + world+0x420`); writes back `+0x50..0x6c`/`+0xb0..0xbc` of
   the solver state. Full closed form open: it is a conditioned-sweep
   solver, not the integrator.

## What must be read next

1. **The `dmBody` mass/fixture writer.** No dedicated body constructor was
   located in this pass; the writers of `+0x164/+0x168/+0x16c`,
   `+0x170/+0x174`, `+0x198` and the fixture fields are unattributed.
   Candidates: the callers of the pool allocator for single `0x1c0` records,
   and the game-layer body-creation path (mover link in
   `formats/character-controller.md` is still open on the force-vs-sweep
   question).
2. **The tick pair.** The dt scalar, the substep count and the iteration
   counts (`param+0x80`, `param+0x84`, catch-up `param_3`) flow in from above
   `FUN_142cdfc50` ← `FUN_14208c190`; their numeric values are not read.
3. **The `m_inv_dt0` consumer.** The writer is read; no reader was found.
4. **`RotationInertiaSolve` closed form.** The reads/writes above are the
   interface; the internal sweep arithmetic is not reduced.

## Verification

Once the schema and the arithmetic are read, a pure body step gets a golden
vector and a multi-step run gets a first-divergence test. A formula that is
"close" is not a result. Status of this pass: schema tables above are
current-image reads with the `(1 fn)` marks showing what still needs a second
read. This pass changed no Rust code, so it adds no test of its own; the
Domino arithmetic that is implemented is tested in `ow-sim`.

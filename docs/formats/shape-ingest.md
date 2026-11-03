# Shape ingest: who fills the Domino shape-entry pool

The collision query walks the Domino world's shape entries at `[record+0x2b8]`
(stride `0xc0`) and the five per-type pools at `[record+0x2d0]`..`[record+0x330]`
(`formats/domino.md`). Both routes into collision — the brush
(`formats/brush-collision.md`) and the map's collision chunk
(`formats/maps.md`, `formats/chunk-kinds.md`) — were said to end at the same
missing link: **nothing read filled those pools.**

This file answers it. The answer is short.

**The pools are written by exactly one allocator pair, reached only from
`ShapeAddToBody` and `ShapeAddToOwner`; and every producer that fills the
*map's* shapes does so from a `BrushTreeToMesh` result.** There is no chunk
input on the shape path: the four callers of `BrushTreeToMesh` are the four
producers that append the geometry object's collision entries, the two append
helpers between them are the only way an entry gets into the object the
finaliser drains, and the finaliser is the only bulk `ShapeAddToBody` caller
that reads those arrays. The collision chunk (`kind 0x11`) therefore does
**not** feed the Domino shape pool by any route read here.

Every address is a current-image address (`0x140000000` + RVA). Nothing below
quotes disassembly: it records addresses and conclusions.

## 1. The pool table, and where it is allocated

`DmWorldConstruct` (`0x142cbeb70`, 5,025 bytes) lays out eight pool records at
`record+0x288 + 0x18*i`, one per pool, from eight counts in its config
argument:

```text
+0x00  pointer to the elements
+0x08  u32  used count
+0x0c  u32  capacity
+0x10  u32  high-water mark (highest index handed out + 1)
+0x14  i32  free-list head, -1 when empty
```

| index | base | element | stride | config count |
|---|---|---|---|---|
| 0 | `+0x288` | bodies | `0x1c0` | `config+0x20` |
| 1 | `+0x2a0` | owners | `0x50` | `config+0x24` |
| **2** | **`+0x2b8`** | **shape entries** | **`0xc0`** | **`config+0x28`** |
| 3 | `+0x2d0` | type 0 (sphere) | `0x20` | `config+0x2c` |
| 4 | `+0x2e8` | type 1 (capsule) | `0x30` | `config+0x30` |
| 5 | `+0x300` | type 2 | `0x50` | `config+0x34` |
| 6 | `+0x318` | type 3 (mesh) | `0x50` | `config+0x38` |
| 7 | `+0x330` | type 4+ | `0x50` | `config+0x3c` |

So the shape-entry pool's four scalars are `record+0x2c0` (used),
`+0x2c4` (capacity), `+0x2c8` (high-water) and `+0x2cc` (free head); the five
type pools' are at `base+0x08`, `+0x0c`, `+0x10`, `+0x14`.

Three behaviours of that constructor matter for the census below:

- a **zero count** allocates nothing and writes the record as
  `{0, 0, 0, 0, -1}` — all four scalars zero except the free head, which is
  `-1`. That `-1` store is the discriminator used in §2;
- a **non-zero count** allocates `count * stride` zeroed bytes and then writes
  each element's **free-chain link** with `i + 1`, and `-1` at the last one.
  The link offset is per pool: bodies `+0x00`, owners `+0x20`, **shape entries
  `+0xa0`**, type 0 `+0x14`, type 1 `+0x24`, types 2/3/4+ `+0x4c`. It is the
  same dword each pool's release path reads;
- the free head starts at `0`, i.e. element 0.

The island pool (`record+0x4a0`, `0xaa0`-stride, count `config+0x70`) is
allocated by the same function in the same shape; it is not needed for this
question and is not censused here.

`DmWorldClear` (`0x142cbe130`, 0x300 bytes) is the reset: it writes the same
eight records to `{0, 0, 0, 0, -1}` without touching the allocations. The
destructor is **`0x142cc04f0`** (1,216 bytes) — it frees each pointer and
writes the same `{0, 0, 0, -1}` shape. (`formats/domino.md` cites
`0x142cc054f`; that address is *inside* `0x142cc04f0`, not a function start.
The same offcut exists for two other citations, see §7.)

## 2. Who writes those fields: the verified store census

`dev/pool_scan.py` is the census. It reads the offline image, keeps the
`.pdata` entries whose `UNWIND_INFO` does **not** carry `UNW_FLAG_CHAININFO`
(119,802 function groups — the same count `docs/ghidra-project.md` reports),
and decodes each group linearly from its start to the next group's start. A
function group is a real function plus the cold fragments MSVC places directly
after it, so a chained fragment no longer decodes as a function of its own; the
first version of this scan read all 201,246 `.pdata` entries and produced
mid-body garbage as a result. When the strict decode stops early the rest is
decoded in `skipdata` mode and the truncation is recorded, so a probe that never
reaches a store cannot be mistaken for "there is no store".

**The displacement space is not enough.** Asking for every instruction whose
memory operand has a non-stack base and one of the 40 pool displacements
returns 5,703 stores, and asking for functions that touch at least three of the
eight pool bases returns **1,129** functions: `0x288`..`0x344` is ordinary
object territory (a class with floats there looks identical). One of the
"winners" shows it plainly — `0x141cca710` writes `0.4`, `0.72`, `0.1`… as
`f32` at those offsets.

What *is* exclusive is the **free-head sentinel**: `mov dword [reg+off],
-1` into a pool record's `+0x14`. Asking which functions store `-1` into
**all eight** free-head fields (`+0x29c`, `+0x2b4`, `+0x2cc`, `+0x2e4`,
`+0x2fc`, `+0x314`, `+0x32c`, `+0x344`), with a non-stack base:

| function | -1 sites (base, free head of each pool) |
|---|---|
| `DmWorldConstruct` `0x142cbeb70` | `0x142cbecdb`, `0x142cbedae`, `0x142cbee77`, `0x142cbef4a`, `0x142cbf002`, `0x142cbf0c7`, `0x142cbf188`, `0x142cbf249` |
| `DmWorldClear` `0x142cbe130` | `0x142cbe1a4`, `0x142cbe1c2`, `0x142cbe1e0`, `0x142cbe1fe`, `0x142cbe21c`, `0x142cbe23a`, `0x142cbe258`, `0x142cbe276` |
| the destructor `0x142cc04f0` | `0x142cc05b7`, `0x142cc05e1`, `0x142cc060b`, `0x142cc0635`, `0x142cc065f`, `0x142cc0689`, `0x142cc06b3`, `0x142cc06dd` |

The only other `-1` stores into any single one of those eight displacements in
the whole image are seven functions with exactly one hit each, all unrelated
objects (one field, no free chain, no pool): `0x141b774b0` (`+0x29c`),
`0x141dbc7b0`, `0x141dbce40`, `0x141dc5050`, `0x141dcaae0` (`+0x2cc` each),
`0x141f1bb70` (`+0x344`), `0x14203c700` (`+0x2e4`).

**Conclusion (read).** The shape-entry pool *record* — base, used, capacity,
high-water and free head — is written by exactly three functions: the
constructor, the reset and the destructor. No other function in the image
initialises a pool record, so every element in the pool got there through the
pool's own allocator, which is §3.

Reproduce: `python dev/pool_scan.py sentinel` prints exactly the table above and
the `ALL` marks; `python dev/pool_scan.py writes` and `python dev/pool_scan.py
at 0x142cbee77` show the surrounding stores.

## 3. Who writes the pool's contents: the allocator pair

The per-element allocator is `ShapePoolAlloc` (`0x142cb00c0`): pop
`pool[+0x14]`, take the next index from that element's link dword, bump
`pool[+0x08]`, raise `pool[+0x10]`; when the free head is `-1`, grow to
`capacity/2 + capacity` (minimum 2), allocate, zero, copy `used * stride` bytes
and re-point. `formats/domino.md` records that expression; this is the census
of who may call it.

| target | E8 sites | functions |
|---|---:|---|
| `ShapePoolAlloc` `0x142cb00c0` | 2 | `0x142cb0a20` (`0x142cb0a4a`), `0x142cd4350` (`0x142cd437a`) |
| `ShapeEntryFromDescriptor` `0x142cb7f10` | 2 | `0x142cb0a20` (`0x142cb0a87`), `0x142cd4350` (`0x142cd43af`) |
| `0x142cb0a20` | 1 | `ShapeAddToBody` `0x142cb3940` (`0x142cb39e5`) |
| `0x142cd4350` | 1 | `ShapeAddToOwner` `0x142cd4ef0` (`0x142cd4f94`) |
| `ShapeAddToBody` `0x142cb3940` | 27 | 12 function groups |
| `ShapeAddToOwner` `0x142cd4ef0` | 1 | `0x142cf35a0` (`0x142cf37a6`) |

`0x142cb0a20` is the shape-entry creator: allocate an index, fill the `0xc0`
record from the caller's descriptor. `0x142cd4350` is the owner-side twin. Each
has exactly one caller, so an entry can only be created by `ShapeAddToBody` or
`ShapeAddToOwner`.

(The census reports **12** function groups for `ShapeAddToBody`, not the 13 of
`formats/maps.md`: that count treated the chained fragment `0x14201b1cd` as a
function of its own. Its three sites `0x14201bc8e`, `0x14201bce1`,
`0x14201c019` belong to the group starting `0x14201ae60`.)

The five per-type pools are the same story — each type's fill helper has one
caller, and it is the descriptor copier:

| per-type helper | E8 sites | functions |
|---|---:|---|
| `ShapePoolAllocType1` `0x142cb5e80` | 1 | `ShapeEntryFromDescriptor` (`0x142cb816b`) |
| `ShapePoolAllocType2` `0x142cb6100` | 1 | `ShapeEntryFromDescriptor` (`0x142cb8108`) |
| `ShapePoolAllocType3` `0x142cb6290` | 1 | `ShapeEntryFromDescriptor` (`0x142cb80ac`) |
| `ShapePoolAllocType4` `0x142cb5fc0` | 1 | `ShapeEntryFromDescriptor` (`0x142cb8079`) |
| `MeshShapeFill` `0x142cae540` | 5 | `0x142cb7f10` (`0x142cb80d4`), `0x140c264b0` (`0x140c28288`), `0x142077580` (`0x14207879f`), `0x14209fc90` (`0x1420a06eb`), `0x14210bac0` (`0x14210bc96`) |
| `ShapeFillType2` `0x142cd31f0` | 2 | `0x142cb7f10` (`0x142cb8140`), `0x142cd35a0` (`0x142cd35b8`) |

`MeshShapeFill`'s four extra callers build a mesh record and then add a shape
themselves (`formats/maps.md`'s producer census already classifies them: a
computed box, a scaled fill and the brush collider emitter, plus
**`0x14209fc90`**, which is **not** an owner-container assembler: it reads the
**model** at `[[node+0x110]+0x28]` (else `0x141b3d1c0(node)`) and walks
`STUModel+0x140` with the mode filter `holder+0x2a == 1` -- the per-model body
builder uses the same list with `== 0`, so the two are two modes of one holder
class; its scratch context is `0x50` bytes, allocated and freed around the call
at `0x1420251d1`/`0x142025229`). `ShapeFillType2`'s second route is
`0x14209fc90` → `0x142cd35a0`, which fills a type-2 record for the mover.

**Conclusion (read).** Every `0xc0` shape entry and every per-type record in a
world is created by `ShapeAddToBody` or `ShapeAddToOwner` from a descriptor the
caller builds. No function reaches a pool without one of those two, so "who
fills the shape-entry pool" reduces exactly to "who calls `ShapeAddToBody` with
map geometry".

## 4. The map-side caller: `GeometryFinalize`

Of the twelve `ShapeAddToBody` caller groups, one builds shapes in bulk from the
scene's geometry object: **`GeometryFinalize` `0x14213d0b0`** (3,376 bytes,
counting its two cold fragments; called by `0x14213ce50`, which has no `E8`
caller — the geometry is finalised through its owner's vtable). It has two
`ShapeAddToBody` sites, `0x14213d530` and `0x14213d9bb`, one per source array.
Of the twelve caller groups it is the only one whose input is a list of
*collision entries*; the other eleven are the mover/prop part builders of
`formats/maps.md`'s producer table plus the two geometry-module producers
`0x1421347b0` and `0x142135030` ("candidates A and B" in that table), and none
of them appends a collision entry — §5's helper census excludes them.

Its inputs, all on the geometry object:

| field | what | element | count |
|---|---|---|---|
| `+0x108` | collision entries, **array A** | `0x60` | `+0x114` (capacity `+0x110`) |
| `+0x118` | collision entries, **array B** | `0x100` | `+0x124` (capacity `+0x120`) |
| `+0xf8` | transform sources | `0x50` | `+0x104` (capacity `+0x100`) |
| `+0xb0` | the per-cell pool **built here** | `0xb0` | `+0xc0` (free chain via `ColliderMeshPoolPush`) |
| `+0xe0` | the transform pool **built here** | `0x50` | `+0xe8` (free head `+0xf4`) |

For array A the finaliser takes an index from `ColliderMeshPoolPush(geometry+0xb0)`,
copies the `0x60` entry into that pool record at `+0x00`, `+0x10`,
`+0x20`..`+0x4f`, and its `+0x50`/`+0x58`/`+0x5c` into the record's
`+0x90`/`+0x98`/`+0x9c`, then emits one shape per cell through
`ShapeAddToBody` with a stack descriptor whose mesh source is the record's
`+0x20` block. Array B is walked with a `0x100` stride and reaches the second
site.

So the two arrays are the *only* inputs to the map's shapes, and §5 settles who
appends them.

## 5. The two append helpers, and their exclusive callers

| helper | appends | E8 sites | callers |
|---|---:|---:|---|
| `0x142136010` | one `0x60`-byte element (grow `count/2+count`, min 2) | 1 | `GeometryBuild` `0x14213c440` at `0x14213c924`, argument materialised at `0x14213c91d` as the array struct at `geometry+0x108` |
| `0x1421360a0` | one `0x100`-byte element (same growth) | 2 | `AppendCollisionEntryFromBrushSource` `0x14213a510` at `0x14213aadc` and `0x14213acf0` at `0x14213b00c`; both materialise `geometry+0x118` (`0x14213aad5`, `0x14213b005`) |

Both helpers take the *array struct* pointer, so the same helper serves any
array with that layout; that is why the argument, not the helper, decides which
array is appended.

And every one of those three callers is a `BrushTreeToMesh` caller. The mesh
conversion `BrushTreeToMesh` (`0x142cf23c0`) has **four** callers in the whole
image:

| caller | site | role |
|---|---|---|
| `GeometryBuild` `0x14213c440` | `0x14213c9c0` | the node-type-2 build: model → brush ring → mesh → one array-A entry (`formats/brush-collision.md`) |
| `AppendCollisionEntryFromBrushSource` `0x14213a510` | `0x14213a7ab` | brush source snapshot → mesh → one array-B entry |
| `0x14213acf0` | `0x14213af82` | a `0x28`-stride brush node list → mesh → one array-B entry per node |
| `0x14213c240` | `0x14213c2c8` | the shared mesher step of the two array-B appenders (`0x14213a510` at `0x14213aab7`, `0x14213acf0` at `0x14213afa0`) |

`0x14213a510` and `0x14213acf0` are called only from `0x14213bbc0` (sites
`0x14213bd61` and `0x14213beaf`) — the brush-asset driver whose GUID-type-`0x0A`
gate `formats/brush-collision.md` records. `0x14213bbc0` and `0x14213c440`
have no `E8` caller (both are reached through a vtable / node-type table).

**Conclusion (read).** A collision entry enters the geometry object only
through `0x142136010` or `0x1421360a0`, and every caller of either is a
`BrushTreeToMesh` caller. The producers of the map's shapes are therefore the
node-type-2 brush build and the brush-source driver, and nothing else.

## 6. The three `BuildBvh` callers

`BuildBvh` (`0x142ccb1b0`) is the collider **face**-BVH builder, not a sector
reader. Its argument is an array of `0x60`-stride items of six 16-byte boxes
(the collider face record) plus a 64-bin SAH scratch; its output is a `0x50`
node array in `state+0x30` with the used count at `+0x38`. It is already a
verified unit (`dev/verify_harness/units/fn-BuildBvh.json`, candidate
`ow_sim::collider::build`), whose fixture declares exactly that signature.

Its five `E8` sites are in three functions, and all three are two-way split
wrappers over the *same* `0x60`-stride face array: each unions the item boxes,
picks an axis, writes a node and calls `BuildBvh` twice.

| function | sites | extent |
|---|---|---|
| `0x142cca7d9` | `0x142ccad25` | 0x59a |
| `0x142ccb2db` | `0x142ccb3df`, `0x142ccb411` | 0x187 |
| `0x142ccbc1c` | `0x142ccbcac`, `0x142ccbcdc` | 0x11b |

None is reachable: **0** `E8` sites to any of the three, **0** `E9`, no 8-byte
pointer and no encoded pointer (`dev/data/read/refs.py`). Their dword-RVA hits
(`0x14382ae5c`, `0x14382ae70`, `0x14382aea8`) are entries of a sorted
function-RVA table in `.rdata` whose neighbours are the RVAs of `0x140041021`,
`0x140127410`, `0x1401b6408` and of `BuildBvh` itself (`0x14382ae58`) — a
shared table, not a dispatch slot per function.

So "a sector reader must end in a BVH" does not hold as a route to the shape
pool: the only BVH on this path is the face BVH, and its faces come from
`ColliderFaceAppend`/`ColliderMeshEmit`/`MeshToColliderFaces`, which is the
brush route again (`formats/brush-collision.md`).

## 7. The decision, and what it changes

**The source data is the brush, not the collision chunk.** Four independent
read results agree:

1. the pool *record* is written only by `DmWorldConstruct`, `DmWorldClear` and
   the destructor (§2, sentinel census);
2. the pool *contents* can only be created through `ShapeAddToBody` /
   `ShapeAddToOwner` → `0x142cb0a20` / `0x142cd4350` → `ShapePoolAlloc` +
   `ShapeEntryFromDescriptor` (§3, exclusive single-caller chains);
3. the map-side `ShapeAddToBody` caller is `GeometryFinalize`, whose only two
   inputs are the geometry's `0x60`- and `0x100`-byte collision-entry arrays
   (§4);
4. those arrays are appended only by `0x142136010` and `0x1421360a0`, whose
   three callers are all `BrushTreeToMesh` callers (§5).

Consequences for the collision-chunk work:

- **The collision chunk's sectors are not a producer of Domino shapes.** The
  sector array (`0x58` stride, counts at `+0x2c`/`+0x30`/`+0x34`) has no reader
  on this path, and the two array-B appenders read a brush, not sectors. This
  closes the "who fills `[record+0x2b8]`" question with an answer that is *not*
  "a chunk decoder we have not found": the pool is fed by the brush and there is
  no second feeder.
- **Implementing the chunk as a shape ingest would be inventing a route the
  client does not have.** The chunk's consumer is still unread (§8), but it is
  not this one.
- **The brush remains the only producer**, and it is blocked where
  `formats/brush-collision.md` says: `C`'s class and its `+0x98`/`+0xa0` array
  encoding, and the absence of type-`0x00A` assets in the manifests.

Corrections this read makes to the other files (they own their text; this file
records them so the parent can fold them in):

| file | text | correction |
|---|---|---|
| `formats/domino.md` | the destructor is `0x142cc054f` | the function starts at **`0x142cc04f0`**; `0x142cc054f` is inside it |
| `formats/domino.md` | `0x142cb0a73` as the creator's address | the group starts at **`0x142cb0a20`** |
| `formats/maps.md` | `0x1421360a0` "has exactly one caller, the brush ingest" | **two** callers, `0x14213a510` and `0x14213acf0`, both brush |
| `formats/maps.md` | "27 `ShapeAddToBody` sites in 13 functions" | 27 sites in **12** function groups (a chained fragment was counted as a function) |
| `docs/function-names.csv` (row `0x14213a510`) | `AppendCollisionEntryFromBrushSource` "appends one `0x60` entry" | it appends one **`0x100`**-byte entry to the array at `geometry+0x118` (`0x1421360a0`, argument at `0x14213aad5`) and fills it through `+0xf0` (stores at `+0xa0`, `+0xb0`, `+0xd0`, `+0xf0`); the `0x60`-byte entries are `GeometryBuild`'s, into `geometry+0x108` |
| `formats/brush-collision.md` | its driver is "`0x14213bbe6`" | the driver's group starts at **`0x14213bbc0`**; `0x14213bbe6` is inside it |
| `formats/maps.md` | "the geometry object's `+0x114` has only three writers" | `+0x114` is array A's count; its only append path is `0x142136010` ← `GeometryBuild`, and `GeometryFinalize` resets it after draining (§4). The "three writers" figure was not reproduced by this scan and should be re-derived before it is relied on |

## 8. What is open

1. **An inline entry appender.** §5 is a census of the two helpers. A function
   that grew and wrote one of the arrays itself, without calling either helper,
   would not appear — a displacement scan cannot separate `[reg+0x114]` on a
   geometry object from the same displacement on any other class (258 such
   writes in the geometry module's address range alone, plus float fields). The
   read that would close it: an entry writer that bumps `geometry+0x114` or
   `+0x124` inline, which no decoded store in that range shows, but which this
   method cannot exclude.
2. **The node-type table is not enumerated.** 144 node types are registrable
   (`formats/maps.md`, `dev/probe_node_types.py`). Only type 2 (`GeometryBuild`)
   and the GUID-type-`0x0A` brush driver were followed. A third node type whose
   build calls one of the two append helpers would be caught by §5's census
   (it would be a caller), so this does not contradict the conclusion — but it
   is not enumerated.
3. **`0x14213acf0` is read only in outline.** It walks a `0x28`-stride brush
   node list, keeps or drops each node by a chord/leaf area test, meshes it with
   `BrushTreeToMesh` and writes one `0x100` entry; the entry's fields past
   `+0xb0` (the handles at `+0x40`.. and the scale quad) are not decoded.
4. **GeometryBuild's `0x60` array-A entry** is read only as far as the
   finaliser consumes it (`+0x00`, `+0x10`, `+0x20`..`+0x4f`, `+0x50`, `+0x58`,
   `+0x5c`). Its own field meanings are not named.
5. **The collision chunk's own consumer** stays open exactly as
   `formats/chunk-kinds.md` §6 records — now with the additional knowledge that
   it is not the Domino shape pool.

**Not tested.** This file's conclusion is a census of call sites, verified by
byte scan and instruction decode, not a differential unit. There is nothing to
implement: no conversion is read, so `ow-tank` gains no module from this work.
The census is reproducible with the two probes below, and every address above
is a `.pdata` function start unless it is named as an inside address.

## 9. Probes

| tool | question |
|---|---|
| `dev/pool_scan.py writes` | every store with a non-stack base into a pool-table displacement |
| `dev/pool_scan.py reads [offs…]` | the loads |
| `dev/pool_scan.py func 0xADDR` | every pool-field access inside one function group |
| `dev/pool_scan.py calls 0xTARGET` | verified `E8` census of a target, grouped into non-chained function groups |
| `dev/pool_scan.py at 0xADDR` | the instructions around one site, to check the operand |
| `dev/pool_scan.py sentinel` | the exclusive pool-record writer census of §2 |
| `dev/pool_lea_args.py 0x108 0x118` | every `lea reg,[base+off]` for the offsets, with the call it feeds — how the append helpers' arguments were pinned |

`pool_scan.py` also carries the group builder: `.pdata` entries with
`UNW_FLAG_CHAININFO` clear, grouped to the next such start. That is what makes
its function counts agree with the 119,802 of `docs/ghidra-project.md`.

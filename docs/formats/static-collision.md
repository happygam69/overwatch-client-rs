# Static collision: three bounded checks

The brush route turned out to be the *fracturable* path: the descriptor `C`
(`0x564B1018`, `formats/model-collision.md`) is reached through entity components
and its payload is a fracture-piece list, and no practice-range asset carries it.
This file asks the other question -- where the map's **static** collision comes
from -- with three bounded checks, each of which records its counts and a verdict
even when the verdict is negative.

Evidence class per claim: **install** = measured on the operator's installation;
**decompiled** / **assembly** = read from the Ghidra program `owmod_clean`,
`Overwatch.exe`; **registration** = read from a class registration function.
Every address is a current-image address (`0x140000000` + RVA).

## Status in one line

The range's entity chunks name **none** of the fracturable assets (0 of 550
placeables, 0 byte-level hits in 202,688 bytes); the range's **models do carry a
prebuilt collision mesh** (`STU_537E56E4`: 161 of 229 models, 272 blocks); its
**consumer is found** -- `FUN_14210bac0` walks the model's collision list, which
this build keeps at **`+0x140`** (not the reference's `+0x118`), unpacks the
block with `0x141b53af0`, fills a `0x4c`-byte shape record with `MeshShapeFill`
(`0x142cae540`) and hands it to the collision query `0x142caf6c0`; and the
kind-`0x11` chunk's candidate consumer `0x1420a02f6` is **not reachable** under
the four-byte-RVA dispatch method either. There is **no vertex or index
conversion** in that path -- the three arrays are passed by pointer -- and the
path is **query-only**: `0x142caf6c0` tests triangles in a stack buffer and
writes nothing (check 6). The character's sweep does not use it either: it
sweeps the **Domino world** through an encoded handle at `world+0x08`/`+0x10`
(`0x142cbcee0` -> thunk `0x142cf5660` -> `0x142cf3f80`), with no vtable dispatch
and no call to the scene owner (check 7). The range's static collision has no
producer in the read paths -- **but the per-model route changes the picture**:
the models carry their collision per model asset (163 of 229 have a holder list,
279 holders, 272 mesh blocks, one `STUModel` each, no model-to-model reference),
and the client contains a per-instance builder (`0x14201af49` + chunk
`0x14201b1cd..`) that turns each holder's mesh into a Domino body shape through
`ShapeAddToBody`/`OwnerCreateBody`. The one open link is that the builder has no
static reference and must be handed to a runtime table (check 8).

**That last line is superseded by check 9.** The builder is the function at
**`0x14201ae60`** (six contiguous `.pdata` fragments to `0x14201c887`), and it
has **four `E8` callers**, all state-sync functions of the same collision
component: `0x142022030` ("make the body match the state"), `0x1420195e0`
(mask-membership change), `0x142014b50` and `0x142015f70`. Two of those are
reached from the component's per-frame update, and the component's own
guards are runtime scene-graph reads (the node's model object
`+0xe8 -> +0x28`, the node's Domino owner id `+0x620 -> +0x31f8`), so the
per-model route is **live code, not an editor path**. On this map **all 272 mesh
holders** of the 229 models pass its filter (mask all ones, filter byte and
shape mode default `0`), so a placed instance of any of the **161**
mesh-carrying models builds one body with one type-3 mesh shape per holder.
What is still unread is which *static* map nodes are given the component (check
9's open list). **Check 10 narrows that further**: the id the builder hands to
`OwnerCreateBody` is a Domino **world slot** in the same table (`0x1442a0bc0`)
and the same encoded-pointer decode the character sweep uses, the node creates
its own static body in that same slot (`[node+0x620]+0x31fc`), and the
deciding gate is four runtime node/entity bits -- none of them model-asset data.
The one thing still open is the allocation site that gives a node the component
at all.

**Checks 11 and 12 close most of that.** Check 11: the world the character sweeps
is reached through the *same* accessor and the *same* slot the per-model bodies
use -- the `u16` at `[node+0x620]+0x31f8`, read by `0x14202f800` -- so the two
routes name one slot family, and the character's own shape entry is created in
that world (`0x142cc3f50`). With no `+0x620` record the accessor returns the
global fallback `0xffff`, which both `OwnerCreateBody` and `0x142cc3f50` reject,
so `[node+0x454]` bit `0x200` gates *both* routes at once. Check 12: the
collision component is the class constructed by **`0x14200a9a0`**, allocated at
**`0x14200a5f0`** with size **`0xad0`**, and registered by **`0x14200bab0`** as
kind flags `0x14`, size `0xad0`, name `Unknown`; its primary vtable is
`0x142f7d608` and its base interface `0x142f7d648` (not `0x142f7d320`, which is
a neighbouring folded vtable). The pair-with-a-node entry is `0x14200f840` ->
primary slot 4 = `0x14201cf40`, which supplies `FUN_142017010`'s arguments from
its *entity* argument; the component's own attach hook `0x14200ee80` takes the
node from `[entity+0x250]` and tests no asset-derived condition. Whether an
entity carries this component is therefore decided in the engine's component
driver, which is unread -- see check 12's open item and the `not tested` row.

## Check 1 -- the range's entity references vs the nine entity / seven `C` assets

`crates/ow-tank/tests/static_collision_real.rs::check_1_...` (install).

The range's scene resolves to 34 chunks; the kind-`0x0B` (entity) chunk is one
asset, **`0x0DD0_000B_0F69_002E`**, holding **550 placeables** in **202,688
bytes**. Each of the 16 candidate GUIDs (nine entity assets whose
`0x09CB5A77` component references `C`, and the seven `C` assets themselves) was
searched for **byte for byte**, as an 8-byte little-endian reference at *any*
offset, which is stricter than a parsed-field walk: a reference cannot hide at a
different field offset from this test.

| measurement | result |
|---|---|
| kind-`0x0B` chunks in the range | 1 (`0x0DD0_000B_0F69_002E`) |
| placeables in it | 550 |
| bytes searched | 202,688 |
| candidate GUIDs searched | 16 |
| byte-level hits | **0** |
| distinct parsed `entity`/`identifier` field values | 80 |
| of those, equal to a candidate | **0** |

The parsed values look like real entity GUIDs (`0x040000000000008b`,
`0x040000000000011b`, ...), so the field is populated and this is not a parsing
artefact.

**Verdict: negative.** The practice range names no fracturable descriptor and no
entity that carries one; the brush path is not live for this map. (This also
disposes of the hope in `formats/model-collision.md`'s open item 3: the range is
not merely "not placing the `C` assets among its models" -- its entity data does
not name them at all.)

## Check 2 -- `STU_537E56E4` in the range's models

`crates/ow-tank/tests/static_collision_real.rs::check_2_...` (install), plus a code
read for the consumer.

`STU_537E56E4` is the prebuilt collision block `C+0x08` holds: `m_nodes`
(`0x9B2111C5`), `m_88FCECD7` points, `m_triangles` (`0x0D50490E`), `m_nodeCount`,
`m_height` and one unnamed float. Scanning the range's **229** model assets'
`MSTU` blobs:

| measurement | result |
|---|---|
| models with an `MSTU` | 229 of 229 |
| models carrying the mesh block | **161** |
| mesh-block instances | 272 |
| holder instances (class `0x2DD96D6D`) | 279 |
| holders with no mesh block under them | 7 |
| meshes per model (histogram) | `{0: 68, 1: 60, 2: 96, 3: 2, 4: 2, 6: 1}` |

Every mesh block is embedded the same way: parent class **`0x2DD96D6D`** through
field **`0xB7C8314A`** -- the same field hash `C` uses for the block at `C+0x90`.
The block's own stored fields in the install (hash, stored width): `0x9B2111C5`
4, `0x38B71436` 4, `0x5BD33E19` 4, `0x88FCECD7` 4, `0xA16426C6` 4, `0xF91B8EDD`
4, `0x0D50490E` 4 -- each an `i32` dynamic-block offset, which is why they read as
4 bytes rather than the class's 16-byte header.

**The data-side chain is therefore read:**

```text
model asset (type 0x00C) -> MSTU -> STUModel (0x871BD3D0)
  field 0xCB4D298D (reference +0x118, this build +0x140; check 4)   array of
    0x2DD96D6D
    instance of 0x2DD96D6D
      field 0xB7C8314A                 embedded STU_537E56E4
        m_nodes / m_88FCECD7 / m_triangles / m_nodeCount / m_height
```

`STUModel`'s field `0xCB4D298D` is the model's collision list; that is the
"which field/class references the model's collision" answer, read from the
class's own declaration and confirmed by the embedder field of all 272 blocks.
Check 4 fixes the field's *runtime* offset for this build: `+0x140`, not the
reference's `+0x118`.

**The consumer is not found.** Two measurements, both negative:

- `0x9B2111C5`, `0x0D50490E` and `0xCB4D298D` occur in code only as
  `mov r8d, <hash>` immediates that feed **metadata** calls, never a comparison
  against a value: the node hash at `0x140fe41b4`, `0x140fe5fd5`, `0x141502582`
  and `0x1416aab01`, the triangle hash at `0x140fd2fd5` and `0x141502689`, and
  the collision-list hash at `0x14150bf65`. The first five call the field-table
  helper `0x1421d4f70`/`0x1421d4f30` (a field declaration: hash in `r8d`, its
  offset/size on the stack); the class hashes `0x537E56E4` and `0x2DD96D6D` are
  the two classes' own registrations (`0x14034adf0`, `0x14034e290`). So the
  immediates are the registration of the very fields and classes this check is
  about -- a reflection-driven *reader* would compare a hash in a dispatch, and
  none of these do.
- The node-type-2 path does not read the block. `GeometryBuild` (`0x14213c440`)
  reads `C+0xf0`, `C+0x98` (the `0xB7C8314A` block's points) and
  `GeometryCopyParams`' field set (`C+0xc0`/`+0xd0`, `+0x110`..`+0x16c`,
  `+0x178`/`+0x186`, `+0x194`/`+0x195`); **`C+0x08` is not in that set**, and the
  geometry factory `FUN_1420d0ef0` (`0x1420d0ef0`) only allocates the geometry
  object and calls its slots. So the prebuilt node/triangle mesh has no reader in
  the type-2 path, and `C`'s own `+0x08` copy of it is dead for that route.

**Verdict at this point: the range has a prebuilt collision mesh.** (The
consumer was found afterwards; see check 4. It is `FUN_14210bac0`, and the field
it walks is at `+0x140` in this build.)

## Check 3 -- the kind-`0x11` chunk's consumer, with the RVA-dispatch method

The chunk `0x0DD0_0011_0000_0688` (kind `0x11`, `MAP_CHUNK_COLLISION`) is 21.8 MB
of 36 sectors of `0x58`, each threading four runs (12-byte vertices, 1-byte
attributes, 16-byte records, 4-byte indices) -- measured earlier in
`formats/maps.md`. The candidate consumer is `0x1420a02f6`, a **mesh-source
assembler** over the owner's descriptor container at `+0x140`/`+0x148`, which
`formats/maps.md` called unreachable on the strength of the *old* reachability
test (E8, E9, 8-byte pointers, `imm64`, the encoded-pointer idiom) and not the
four-byte-RVA form. This check re-tests it with that form.

### The method, and why it is the right one

The client's computed jumps over small integer switches are **image-base-relative
RVA tables**. The canonical site is `0x14208e747..0x14208e75f` (assembly):

```text
lea  rcx, [rip - <image base>]         ; rcx = image base
mov  ecx, [rcx + rax*4 + 0x2090988]    ; a 4-byte RVA, indexed by the switch
lea  rax, [rip - <image base>]
add  rcx, rax                          ; absolute target = base + rva
jmp  rcx
```

with the index source right before it: `cmp eax, 7; ja <fallback>; cdqe` -- an
eight-entry switch whose out-of-range case is itself one of the table's entries.
The table at `0x142090988` holds `0x20905d3, 0x2090561, 0x208ec9a, 0x208e761,
0x208edf6, 0x208f545, 0x208feac, 0x20900b6`, i.e. **fragments of one function
body**, which is exactly why `0x14208edf6` is a valid entry.

`dev/static_rva_tables.py` enumerates every indexing site of that form and every
table whose first entries are code RVAs, then answers for a target RVA (and for
every RVA inside a function's span, because an entry may be a fragment):

| measurement | result |
|---|---|
| indexing sites (`mov r32, [base + rax*4 + disp32]`) | 524 |
| distinct table RVA candidates | 416 |
| of those, holding code RVAs | 277 |
| **validation:** `0x208edf6` | **HIT** table `0x142090988`, indexed at `0x14208e74e` |
| `0x20a02f6`, or any RVA in `0x20a02f6..0x20a075d` | **0 hits over 277 tables** |

### The three four-byte occurrences, classified

`0x20a02f6` occurs as a 4-byte value exactly three times, and none is a dispatch
entry:

| address | region | what it is |
|---|---|---|
| `0x14377f744` | `.rdata` | the code-range copy: the triple `{0x209ff9d, 0x20a02f6, 0x377f724}` is `{begin, end, unwind}` of the **preceding** function, so the value is an *end*, not a start |
| `0x14469a460` | `.pdata` copy | the same range's `end` in the 12-byte `.pdata` mirror |
| `0x14469a468` | `.pdata` copy | the *begin* of `0x20a02f6..0x20a075d` in that mirror |

`.pdata` itself confirms the function: `0x20a02f6..0x20a075d`, length `0x467`,
which starts exactly where `0x209ff9d..0x20a02f6` ends. The remaining spellings
are all empty for this address: **no** `E8`, **no** `E9`, **no** rip-relative
`lea`/`mov` (the whole-image scan `dev/xref_rva.py` reports 0 hits), **no** 8-byte
absolute pointer, and no image-base-relative dispatch entry.

**Verdict: unreachable**, now under the dispatch method as well, and the method is
demonstrated on a known positive in the same run. Consequently the
"who fills the owner's descriptor container at `+0x140`/`+0x148`" question cannot
be answered through this consumer: the assembler is not on any static path, so
whatever fills the container does so for a caller that does not exist in a static
call graph.

**The exact next read** is unchanged and is the same one `formats/maps.md` names:
the **producer of the owner's `+0x140`/`+0x148` container** (the descriptors with
`+0x2a == 1`, mode at `+0x20`, data pointers at `+8/+0x10/+0x18` that
`0x1420a02f6` would consume). `formats/maps.md` already narrowed it: no store to
`+0x140` or `+0x148` exists anywhere in the collision/domino module
(`0x14200000`..`0x142d0000`), so the producer sits in a constructor or loader
outside it. Adding this check's result to that: the producer is not reached
*through* the assembler, so it must be found from the container's own type.

## Check 4 -- the consumer of the model's collision list: **found**

This is the read the previous section left open, and it started with a correction
to the field's offset.

### The collision list is at `+0x140` in this build, not `+0x118`

The reference's `STUModel` declares `0xCB4D298D` at `+0x118` and is 472 (`0x1D8`)
bytes; the client's registration of the same class (`0x14034af00`) gives **size
`0x200`**. The field's real offset is in the client's own field registration:

- `0x1421d4f70` is the field-descriptor constructor. It stores its **fifth
  argument (the stack word at `[rsp+0x20]`) into the descriptor at `+0x08`**, the
  field's offset slot, and the field hash (`r8d`) at `+0x30`.
- For `C`'s fields that fifth argument is exactly the offset the client's code
  reads: `0x3963555A` -> `0x110` at `0x141333889` (and `GeometryCopyParams` reads
  `C+0x110`), `0x12B475D4` -> `0x194` at `0x141334499` (and it reads `C+0x194`).
  So the argument **is** the runtime offset.
- `0xCB4D298D` is declared once, at `0x14150bf65` (helper `0x1421d4f70`), with
  fifth argument **`0x140`**.

The client therefore keeps the model's collision list at **`+0x140`** (its tag
word at `+0x148`), and the earlier searches for `+0x118` were aimed at the
reference's build, not the install's.

### Who reads it

`crates/ow-tank/tests/model_mesh_real.rs` and `dev/static_mesh_consumers.py`
(install / assembly):

- `FUN_14210bac0` (`0x14210bac0..0x14210bfa5`, `0x4e5` bytes) is the reader. Its
  arm is exactly the array-header idiom at the corrected offset (assembly at
  `0x14210bb6b` and `0x14210bc04`): `rcx = [rdi+0x140]; cmp qword [rdi+0x148],
  1; lea rax,[rcx+8]; cmovne rax,rcx; movsxd rcx,[rax]` -- the count -- then the
  element array through the same tag test.
- Each element is a **pointer** to a holder instance. The reader requires
  `holder+0x20 == 0` (the holder's mode word) and `holder+0x08 != 0` (the
  embedded mesh block) -- the `0x2DD96D6D` layout from check 2: `+0x08` mesh,
  `+0x20` mode, `+0x24` an index into an attachment table.
- Decompiled, its entry is `param_2+0xe8 -> object; object+0x28 -> the STUModel
  instance; object+0x08 -> an attachment table whose `+0x66` is a `short`
  count`. That is the same `+0xe8` slot the node-type-2 update promotes
  (`formats/model-collision.md`), so the model object reached from a node and the
  model object read here have the same shape.
- Its caller is `FUN_1421088e0` (`0x1421088e0`, call sites `0x142108bbf` and
  `0x142108d18`), a **broadphase**: four bucket arrays at `param_1+0x1e8`
  (count plus `0x40`-stride records), a bounding-sphere cull against
  `model+0x450`, then `FUN_14210bac0` per candidate, breaking on the first hit and
  invoking a caller-supplied callback. `FUN_1421088e0`'s own caller is
  `FUN_14212e070` (`0x14212e070`).

### What the client does with the mesh: pass it on

The block becomes a shape record that is handed to the collision query. Three
functions, all read:

| function | what it does |
|---|---|
| `0x141b53af0` | **the unpacker**: from the block, writes a `0x30`-byte source -- `[+0x00]` node data pointer, `[+0x08]` point data pointer, `[+0x10]` triangle data pointer, `[+0x18]` `m_nodeCount`, `[+0x1c]` the point array's count, `[+0x20]` `m_38B71436`, `[+0x24]` `m_height`, `[+0x28]` `m_5BD33E19`. Each pointer is read through the block's own tag rule: the 16-byte header `{data pointer, u64 count}` sits at the field's pointer when the tag at `field+8` is `1`, else eight bytes past it. The offsets are `+0x08` (nodes), `+0x18` (points), `+0x28` (triangles), `+0x38` (nodeCount), `+0x3c`, `+0x40` (height), `+0x44` -- the class's declared offsets. |
| `MeshShapeFill` `0x142cae540` | writes a **`0x4c`-byte (19-dword) record**: four scale lanes at `+0x00`, four `1/scale` lanes (a `divss`) at `+0x10`, the three data pointers at `+0x20`/`+0x28`/`+0x30`, four copied words at `+0x38`/`+0x3c`/`+0x40`/`+0x44`, and `scale * m_5BD33E19` at `+0x48`. |
| `0x142caf6c0` | the consumer of the record: a `0x408`-byte-frame function that takes the record and the query's parameters. |

**There is no vertex or index conversion anywhere in this path**: the arrays stay
as stored and only their *pointers* are copied. `ow_tank::mesh_shape` is the read
expression of the first two (offsets, tag rule, record layout), with unit tests;
it deliberately converts nothing.

### The block in the install (install)

`crates/ow-tank/tests/model_mesh_real.rs`, on the practice-range scene:

| measurement | result |
|---|---|
| models carrying the block | 161 of 229 |
| blocks | 272 |
| first block, model `0x0d0000000000078c` | node array **288 B** (`9 x 32`), **8** points, triangle array **384 B** (`24 x 16`), `nodeCount` **9**, `+0x3c` **12**, `height` **5**, `+0x44` **96.0f** |
| its first four points (bits) | `00000000 00000000 40800000 00000000`, `40800000 00000000 40800000 00000000`, `00000000 40800000 40800000 00000000`, `40800000 40800000 40800000 00000000` -- the corners of a 4-unit box |
| its first twelve triangle words | `4, 0, 0, 0, 5, 0, 0, 0, 6, 0, 0, 0` |
| the record at unit scale | `3f800000` x8, then the three offsets, `9`, `8`, `12`, `5`, `42c00000` |

The node array is `32` bytes per node (`288 / 9`) and the triangle array `16`
bytes per record (`384 / 24`) -- both **derived from the byte counts**, not from a
decoded record, because the record layout lives in the query.

### What is still open here, exactly

The element layouts of the node array (`32` B) and the triangle array (`16` B).
`0x141b53af0` hands their *pointers* over and nothing in the read path dereferences
them, so the next read is **`0x142caf6c0`**, the Domino mesh query, the only
function that walks them. A second question it answers: whether this path
*queries* the model's mesh (the record built per test, the broadphase sweeping
four buckets and stopping at the first hit) or also *ingests* it into a Domino
body -- no `ShapeAddToBody`, `ShapeEntryFromDescriptor` or `OwnerCreateBody` call
appears in `FUN_14210bac0`, so as read it is query-only.

## Check 5 -- the second geometry class (`Alloc(0xf0)`, `0x142132f20`): not the consumer

`FUN_1420d0ef0`'s other branch allocates `0xf0` bytes and constructs
`0x142132f20` (`0x142132f20..0x142132fd8`). The constructor installs the base
vtable `0x143394ed0` and then **`0x143394fb0`**, whose slots are:

| slot | function | reads (displacements) |
|---|---|---|
| `+0x00` | `0x142133010` | `+0x08`, `+0x30` |
| `+0x08` | `0x1421334b0` | `+0x08`, `+0x40`, `+0x48` |
| `+0x10` | `0x142133790` | `+0x28` |
| `+0x18` | `0x1421347b0` | `+0x10`, `+0x18`, `+0x28`, `+0x40`, `+0x48`, `+0x90`, `+0xb0`, `+0xf0` |
| `+0x20` | `0x142134790` | `+0x28` |
| `+0x28` | `0x142133350` | -- |
| `+0x30` | `0x142133b30` | `+0x08`, `+0x10`, `+0x18`, `+0x30`, `+0x40`, `+0x48`, `+0x90`, `+0xa0`, `+0xf0` |
| `+0x38` | `0x140548d00` (generic) | `+0x08` .. `+0x40` |
| `+0x40` | `0x142133cf0` | `+0x08`, `+0x10`, `+0x18`, `+0x30`, `+0xa0` |
| `+0x48` | `0x142135b70` | `+0x08`, `+0x10`, `+0x40`, `+0x48` |
| `+0x50` | `0x142134f70` | `+0x08`, `+0x10`, `+0x18`, `+0x30`, `+0x40`, `+0x48`, `+0xa0`, `+0xc0`; **calls `OwnerCreateBody` (`0x142cc3d90`)** at `0x142135330` |
| `+0x58` | `0x142135030` | `+0x08`, `+0x10`, `+0xa0` |
| `+0x60` | `0x1421332a0` | `+0x10`, `+0x28`, `+0xa0`, `+0xb0`, `+0xc0` |
| `+0x68` | `0x142133280` | `+0x30` |
| `+0x70` | `0x142133110` | `+0x08`, `+0xa0` |
| `+0x78` | `0x14053a420` (generic) | `+0x08`, `+0x10`, `+0x18` |
| `+0x80` | `0x142133da0` | -- |
| `+0x88`, `+0x90` | `0x14053a420` (generic) | as above |
| `+0x98` | `0x1421337e0` | `+0x10`, `+0xa0` |

**No slot reads `+0x140`/`+0x148` or `+0x118`/`+0x120`,** and none calls
`MeshShapeFill`, `0x141b53af0` or `0x142caf6c0` (`dev/static_slots.py --vt
0x143394fb0`). The `0xf0` class builds geometry from its own packed source array
and calls `OwnerCreateBody` on it; the model's prebuilt mesh is not its input.

## Check 6 -- the classification: the model-mesh path is **query-only**

The question this settles: is the model's prebuilt mesh the character's collision
world, or a query-only system? It is **query-only**, and the character's world is
a different structure that none of the read paths fills with the practice range's
geometry.

### What `0x142caf6c0` does with the `0x4c`-byte record

Read end to end (decompiled; the function's second chunk starts at `0x142caf6ed`):
its parameters are the shape record, a **point array** `param_2[16]`, an `int*`
count `param_3`, a per-lane **margin** `param_4`, and a `u16` `param_5`.

1. It bounds the point array (`minps`/`maxps` over `*param_3` points) and expands
   that box by the margin lanes into `min - margin` / `max + margin`.
2. `0x142caea70(record, buffer, &box, flags)` collects candidate triangles from
   the mesh into an **inline stack buffer of `0x80` entries**
   (`puStack_58 = auStack_258; uStack_50 = 0x80`), freed through
   `0x142cadf00` if it spills.
3. Per collected triangle it reads the record's **scale lanes** `record[0..3]`,
   the **points pointer** at record `+0x28` and the **triangle array** at record
   `+0x30`, takes the triangle's record at `index * 0x20`, scales the three
   vertices by the record's scale lanes, copies the triangle record's `+0x18`
   (`i32`) and `+0x1c` (`u16`), builds a per-triangle transform with
   `0x142cc5ba0`, and runs the test `0x142cc6740`, breaking out when
   `movmskps(...) == 0xf` (every lane separated).

**It fills no persistent structure.** There is no world, no body, no shape entry:
the only thing it builds is the stack triangle list. That is the definition of a
query.

It also corrects the previous turn's byte-count guess: the triangle record stride
is **`0x20` (32 bytes)**, so the block measured there (`384 B`) holds **12**
triangle records -- a box's twelve triangles for eight points -- not the `24 x 16`
the byte count alone suggested. The node array stays `32` B per node (`288 / 9`).

### The caller chain and the system it belongs to

```text
FUN_141fb87e0            the query's entry (called from 0x141fb69f0-fn and 0x141fb8460-fn)
  FUN_14202c3a0          decode the owner at [param_1+0x6a8], fill the query context, call, then 0x14212d710
    FUN_14212e070        the query dispatcher over the owner object
      FUN_1421088e0      broadphase: four buckets at param_1+0x1e8, sphere cull, first hit wins
        FUN_14210bac0    the model's collision list at +0x140 (check 4)
          0x142caf6c0    the per-triangle test above
```

`FUN_14212e070(param_1, param_2)` is a **query owner**, not a world:

- `[param_1+0x2b0]` is an array of `0x18`-byte entries with its count at
  `[param_1+0x2b8]`; each entry is tested against the query's mask
  (`[ctx+8] & entry[8]`), the entry's bounding sphere at `+0x170..+0x180`
  against the query's point at `[ctx_object+0x450..]` and radius at `+0x460`,
  then `0x1420af7c0` and `0x1420b01e0`.
- Six bounding volumes at `+0x20`, `+0x70`, `+0xc0`, `+0x110`, `+0x160` and
  `+0x1b0` (`0x50` stride) are box-tested against the query point and, when they
  pass, `FUN_142101fb0(volume, ctx)` runs.
- `[param_1+0x2d8]` is a **lazily initialised** sub-structure: it is taken under
  a lock word at `[param_1+0x2c0]` (`LOCK CMPXCHG` driving a `0x400` flag), and
  the call inside it is `FUN_1421088e0` -- the broadphase that ends at the mesh
  test.

**Why it is not the Domino world.** `formats/domino.md`'s world has its body pool
at `+0x288` (stride `0x1c0`), the pair pool at `+0x2b8` (stride `0xc0`), the
constraint pool at `+0x390` (stride `0xe0`) and its counts in the config block.
Here `+0x2b8` is a *count* of `0x18`-byte entries held at `+0x2b0`; there is no
body pool, no constraint pool and no config, and nothing here is ever written --
the owner's entries are only read and tested.

`FUN_14202c3a0` supplies the query: `param_3` carries the point/threshold block
(`param_3[0..4]`, flags at `+0x22`, a `u64` mask at `+0x10`) and a **callback
`FUN_14203d6c0`** in the stack context it fills (`local_4d8`), with `0x80000020`
thresholds, `0xffff` and `0xf` flags. After the query it runs
`0x14212d710(owner, param_2, param_3)` unless `param_3[0x10] & 0x400000000000000`.

### The character path is separate (assembly)

- `RunCharacterControllerSweep` (`0x142cbcee0`) has **nine** `E8` sites, and they
  are the mover/AI modules: `MoverAdvance` (`0x140ca7f30`), `0x140dc6e3b`,
  `0x140eb7b52`, `0x140ee8960`, `0x140f148f7`, `0x140f17983`. `MoverAdvance`
  itself is called from `MoverInvoker` (`0x1407ffc50`).
- The mesh-query chain has **no** `E8` site anywhere in the mover modules
  (`0x140c00000..0x141100000`): `0x142caf6c0`, `FUN_1421088e0`,
  `FUN_14210bac0` and `FUN_14212e070` are called only from `0x141fb8xxx` and
  `0x1420xxxxx`.
- The character's own query driver `0x142cbd510` is reached from
  `0x142cc4728`-fn, inside the Domino module, not from the mesh chain.

So the character sweeps the Domino world while the model mesh answers a scene-side
query; the two share the low-level triangle test `0x142caf6c0` (its fourth caller
is in `0x142cb8e70`-fn, the Domino module) and nothing else.

### The `f0` class's slot `+0x50`: what it actually builds

`0x142134f70` (`0x142134f70..0x142135370`, slot `+0x50` of `0x143394fb0`) reads, at
`0x1421351c4` (assembly):

```text
mov rcx, [rax + 0x38]      ; the node's loaded asset object
mov rax, [rcx + 0x40]      ; its STU instance
mov r14, [rax + 0x48]      ; C -- the fracturable descriptor
...  CALL OwnerCreateBody (0x142cc3d90) at 0x142135330
```

That is **the same chain `GeometryBuild` walks** (`formats/model-collision.md`),
and the body it creates is built from **`C`** -- the fracturable descriptor whose
asset carries the `FRAC` chunk -- not from the model's prebuilt mesh and not from
the map's collision-chunk sectors. The `f0` class is the fracturable flavour of
node-type-2 geometry.

### Verdict

**The model-mesh path is query-only.** It is a scene-side static-collision query
component (`FUN_14212e070`'s owner, with a lazily built broadphase at `+0x2d8`),
consumed through `FUN_14202c3a0` and its hit callback `FUN_14203d6c0`, entered
from `FUN_141fb87e0`. It writes nothing, so it cannot be the character's world.

**None of the read paths fills the Domino world the character sweeps with the
practice range's static collision.** The separation, with its addresses:

| path | what it does | evidence |
|---|---|---|
| the model mesh | per-query triangle tests in a stack buffer; no body | `0x142caf6c0` read end to end (this check) |
| the geometry classes | `GeometryBuild` creates a body from `C`'s params; the `f0` class creates one from **`C`**; neither reads the model mesh | `0x14213c440`; `0x142134f70` at `0x1421351c4` |
| the character | sweeps the Domino world from `MoverAdvance`; also calls the ingest entry `0x142cbcd40` for its **own** body | `0x142cbcee0`'s nine sites; `0x142cbcd40`'s six sites, one of which (`0x140ca6123`) is inside `MoverAdvance` |
| the map's kind-`0x11` chunk | its candidate consumer `0x1420a02f6` is unreachable | check 3 |

The **exact missing read**: the producer that turns map data into Domino shape
entries. The ingest chain is `0x142cbcd40` -> `0x142cf35a0` ->
`ShapeAddToOwner` (`0x142cd4ef0`) -> `0x142cd4350` ->
`ShapeEntryFromDescriptor` (`0x142cb7f10`), and its only six entry points are
`MoverAdvance` (`0x140ca6123`) and five further mover/scene functions
(`0x140801c24`, `0x140dc6e14`, `0x140eb7b22`, `0x140f1528b`, `0x140f17f4a`) --
all feeding a character-like body, none a map. Combined with check 3 (the chunk's
assembler is unreachable under every reference form), **the practice range's
static collision has no producer in the paths read so far**. That is a scope
statement for the operator, not a missing instruction: either the map feed is a
runtime registration no static reference reaches, or the character's world in this
build is built by a system outside these paths.

## Check 7 -- what world `RunCharacterControllerSweep` sweeps: the **Domino world**

This is the classification that decides the scope question: the character's sweep
does **not** go through the scene-side model-mesh query. It sweeps the Domino
world, through body/shape handles, with no vtable dispatch anywhere on the path.

### The arguments and the hop

`RunCharacterControllerSweep` (`0x142cbcee0..0x142cbcf6f`, decompiled):

| argument | role |
|---|---|
| `param_1` | the **world/query struct**; only `*param_1` (its first qword) is used, as the first argument of the sweep |
| `param_2` | the **output record**: the sweep writes `+0x200` (a `0x20`-stride array) with count `+0x208`/`+0x20c`, `+0x610` (another array) with `+0x618`/`+0x61c`, and the result floats `+0x620..+0x63c`, `+0x640`/`+0x644` |
| `param_3` | the **query block**: two vec4s (`param_3[0..7]`), a radius at `+0x20`, an iteration count at `[param_3+0x2c]`, flags at `[param_3+0x20]` |

The one meaningful call is `0x142cf5660(*param_1, buffer, &query)`. That address
is a **10-byte thunk**, not a function:

```text
test rcx, rcx
jne  0x142cf3f80
ret
```

so the sweep's body is **`0x142cf3f80`** (`0x142cf3f80..0x142cf42da`, four
`.pdata` chunks).

### What `0x142cf3f80` works on: handles, not a vtable

The world struct's **first dword is a variant tag**:

- `*world == 1` (rigid): the handle is `[world+0x10]`, and the calls are
  `0x142cb5260`, `0x142cc4cd0`, `0x142cb47f0`, `0x142cb4140`, `0x142cb3d00`,
  all in the 0x142cb-0x142cc (Domino) range.
- otherwise (character): the handle is `[world+0x08]`, and the calls are
  `0x142cd4ff0` (a no-`.pdata` leaf with the same handle decode as
  `VariantPosition`), `0x142cd50f0`, `0x142cd5070`, `0x142cd5170`,
  `FUN_142cd4d90`, `FUN_142cd5270`.

**There is no vtable dispatch on this path**: a scan of all four chunks finds
**zero indirect calls**, and the only direct calls are those Domino accessors
plus the allocator (`0x142cadee0`), the free (`0x142cadf00`), the grow memcpy
(`0x142efe2b0`) and the stack check (`0x1426c3d40`). No scene-owner function
(`FUN_14212e070`, `FUN_1421088e0`, `FUN_14210bac0`, `0x142caf6c0`,
`0x142caea70`) is called, so the "vtable route" is checked and empty as well as
the call graph.

### The handles decode to the Domino world (decompiled)

Every accessor begins with the same handle decode:

```text
slot  = handle >> 0x30                     ; a world slot, rejected at >= 0x3fff
world = *(u64 *)(0x1442a0bc0 + slot*8) + 0xb3269b88d44d5fb2 ^ 0x77600ba5adcc265c
```

(the encoded-pointer form the project documents; `0x1442a0bc0` is the world
handle table `formats/maps.md` names). Then:

- `VariantPosition` (`0x142cbcec0`), variant `!= 1`: index guard `[world+0x2ac]`,
  entry `[world+0x2a0] + index*0x50`, `id` at `+0x20` and generation at `+0x34`,
  the position at `+0x10` -- the **shape-entry pool `+0x2a0`, stride `0x50`** of
  `formats/domino.md`.
  Variant `== 1`: guard `[world+0x294]`, entry `[world+0x288] + index*0x1c0`,
  `id` at `+0x00` and generation at `+0x198`, position at `+0x10` -- the **body
  pool `+0x288`, stride `0x1c0`**.
- `FUN_142cd4d90`: walks the shape entry's pair list `+0x24` into the **pair pool
  `[world+0x2b8]`, stride `0xc0`** (next at `+0x54`, link at `+0x48`), and each
  pair's `+0x58`-stride records at `[world+0x348]`, calling `0x142cc56b0` on
  each -- the Domino pair/constraint-link walk.
- `FUN_142cd5270`: returns the shape entry's `+0x10` vec4, or a value from
  `FUN_142cd4610(entry, ...)` when the query point is finite.

So the "world" the character sweep uses is the **Domino world**: `+0x288` body
pool (`0x1c0`), `+0x294` body count, `+0x2a0` shape pool (`0x50`), `+0x2ac` its
count, `+0x2b8` pair pool (`0xc0`), `+0x348` link pool (`0x58`). It is reached
through an encoded handle and a global table, which is why no static reference
from the mover points at the world object itself.

### Where the world struct comes from

The nine call sites pass the struct in `RCX`, and two shapes appear:

- a `mov rcx, <local>` immediate owner: `0x140dc6edc` (`rcx = rdi`),
  `0x140eb7bcb` (`rcx = rbx`), `0x140f15a9b` (`rcx = r13`) -- the caller's own
  controller record;
- a **field read**: `0x140ee8aca`, `0x140ee9631`, `0x140ee99df`, `0x140ee9d06`
  all do `rcx = [[rdi+8] + 0x1ad8]`, i.e. the world struct hangs at **`+0x1ad8`**
  of the controller/entity record those AI/mover functions carry.

`MoverAdvance` (`0x140ca7f30`) is one of the nine. So the owner of the world the
character sweeps is the **controller record's Domino world**, not the scene
geometry object and not the query owner of checks 4-6.

### Verdict, and the scope statement

**`RunCharacterControllerSweep` sweeps the Domino world** -- via the thunk
`0x142cf5660` into `0x142cf3f80`, on a handle at `world+0x08` (character shape
pool `+0x2a0`) or `world+0x10` (rigid body pool `+0x288`), with no vtable and no
call into the scene-side model-mesh query. The model-mesh path stays a separate,
query-only system (check 6).

**No map-side producer inserts shapes into that world in the paths read.** The
ingest chain is `0x142cbcd40` -> `0x142cf35a0` -> `ShapeAddToOwner`
(`0x142cd4ef0`) -> `0x142cd4350` -> `ShapeEntryFromDescriptor` (`0x142cb7f10`),
and its six `E8` entry points are `MoverAdvance` (`0x140ca6123`) and five further
mover/scene functions (`0x140801c24`, `0x140dc6e14`, `0x140eb7b22`,
`0x140f1528b`, `0x140f17f4a`): every one of them feeds a character-like body,
none a map. Together with check 3 (the kind-`0x11` chunk's assembler
`0x1420a02f6` is unreachable under `E8`/`E9`/pointer/`imm64`/encoded-pointer and
four-byte-RVA dispatch tests), **the practice range's static collision has no
producer in the read paths**. That is the scope statement: the range's world is
either fed by a registration no static reference reaches, or built by a system
outside these paths.

## Check 8 -- the per-model route: the models **do** carry collision, and a per-model body builder exists

The operator's hypothesis, in one line: the map is a set of models, each model
carries its own collision, so the client should build collision per placed model
instead of ingesting a map-wide chunk. Against this image and this install it is
**confirmed on both sides that can be read** -- the collision is per model asset,
and the client contains a builder that turns it into a Domino body shape -- with
one open link: that builder has no static reference.

### The client's `STUModel` schema, in full

`0x14034af00` registers only the **class** (hash `0x871BD3D0`, size `0x200`,
align 8, the hooks); it makes no field registration. The **fields** are
registered by four installers, each field through `0x1421d4f70` /
`0x1421d4f30`, which store the hash in the descriptor's `+0x30`, the type
argument in `+0x34` and the **runtime offset** (the call argument) in `+0x08`.
`dev/static_model_schema.py <installer>` prints a table like this one:

| installer | fields | offsets |
|---|---|---|
| `0x14150b78c` | 8 | `+0x08` .. `+0x140` |
| `0x14150bfe0` | 10 | `+0x150` .. `+0x1E0` |
| `0x14150c7e8` | 3 | `+0x1E8` .. `+0x1F4` |
| `0x14150ca47` | 4 | `+0x1F8` .. `+0x1FB` |

The four are consecutive in one module and together cover `+0x08`..`+0x1FB` of
the `0x200`-byte class with **no gap and no overlap**, which is what identifies
them as this class's installers (a neighbouring installer in the same module,
`0x14150a2b1`, registers the *same hashes* at a different, compact set of
offsets -- it belongs to another class).

The complete schema, in the client's own order, with the reference's declared
layout as the cross-check ("ref") and the type argument where the client passes
one:

| hash | offset | ref | type argument | kind |
|---|---|---|---|---|
| `0xC4A1DCA0` | `+0x08` | `+0x08` | -- | inline instance (96 B) |
| `0xE52AB6E7` | `+0x68` | `+0x68` | -- | inline instance `m_info` (80 B) |
| `0xF5E50593` | `+0xB8` | `+0xB8` | -- | inline instance (48 B) |
| `0xE86D2174` | `+0xE8` | -- | -- | **client-only field** |
| `0x818D011C` | `+0x110` | `+0xE8` | -- | `m_skeleton`, an asset ref |
| `0xFD232466` | `+0x120` | `+0xF8` | -- | instance array |
| `0xC45F5F6F` | `+0x130` | `+0x108` | -- | `m_hardPoints`, instance array |
| `0xCB4D298D` | `+0x140` | `+0x118` | -- | **the collision holder list** |
| `0x51CD98D2` | `+0x150` | `+0x128` | -- | instance array |
| `0x8289A6C9` | `+0x160` | `+0x138` | `0x4189AEA5` | instance array |
| `0x0C8F8DC6` | `+0x170` | `+0x148` | -- | instance array |
| `0xE9A9B868` | `+0x180` | `+0x158` | `0xCBD8CDF3` | array of asset refs |
| `0xC6B4CACB` | `+0x190` | `+0x168` | `0xCBD8CDF3` | `u32` array |
| `0xAD47190C` | `+0x1A0` | `+0x178` | **`0x871BD3D0`** | asset ref **to a model** |
| `0x37ED05D0` | `+0x1B0` | `+0x188` | **`0x871BD3D0`** | asset ref **to a model** |
| `0xC67B4D05` | `+0x1C0` | `+0x198` | **`0x871BD3D0`** | **array to models** |
| `0x43CEF9BB` | `+0x1D0` | `+0x1A8` | `0x5CA5A90D` | asset ref |
| `0x7E48C526` | `+0x1E0` | `+0x1B8` | `0x5CA5A90D` | `m_breakable` |
| `0x78985272` | `+0x1E8` | `+0x1C0` | -- | inline instance |
| `0x87916047` | `+0x1F0` | `+0x1C8` | -- | enum |
| `0x53BD772C` | `+0x1F4` | `+0x1CC` | -- | enum |
| `0xD64F7813` | `+0x1F8` | `+0x1D0` | -- | `u8` |
| `0xADC54E57` | `+0x1F9` | `+0x1D1` | -- | `u8` |
| `0x81AF6609` | `+0x1FA` | `+0x1D2` | -- | enum (`m_usageCategory`) |
| `0x646E63C6` | `+0x1FB` | `+0x1D3` | -- | `u8` |

Every reference hash appears; the only client-only entry is `0xE86D2174` at
`+0xE8` -- the field that shifts the reference's `+0xE8`.. tail to `+0x110`.. in
this build, which is why the collision list is at `+0x140` here and `+0x118`
there.

**Which fields are collision-like.** By what their type reads:

- **`0xCB4D298D` (`+0x140`)** -- the only field whose readers build geometry: an
  array of holders (`0x2DD96D6D`), each with the mesh block `STU_537E56E4` at
  `+0x08` (unpacked by `0x141b53af0`), a filter mask at `+0x10`, an inline
  parameter instance at `+0x18` (the `0x4EFF450F` family: floats at
  `+0x30`/`+0x38`/`+0x40`/`+0x44`/`+0x54`/`+0x58`, a byte at `+0x76`), mode
  `i32` at `+0x20`, an index at `+0x24`, `u16` at `+0x28`, byte at `+0x2A`.
- `0xFD232466`, `0xC45F5F6F` (`m_hardPoints`), `0x51CD98D2`, `0x8289A6C9`,
  `0x0C8F8DC6` -- other instance arrays; no reader of them builds a shape
  (see check 6's reader census), and in this install they are essentially empty.
- `0xAD47190C`, `0x37ED05D0`, `0xC67B4D05` -- **model-typed references**
  (`0x871BD3D0`). The schema therefore *allows* a model to name another model, a
  separate collision model. Measured below: **none does**.
- There is **no hull, capsule or box field, and no second mesh field**: the only
  mesh is `STU_537E56E4` through the holder list.

### The range's models, per model (install)

`crates/ow-tank/tests/model_schema_real.rs` over the practice range's 229 model
assets:

| measurement | result |
|---|---|
| `STUModel` instances per model asset | **1** on all 229 |
| models with a holder list (`0xCB4D298D`) | **163** (66 have none) |
| holders | **279** |
| holders per model (histogram) | `{0: 66, 1: 61, 2: 96, 3: 2, 4: 2, 6: 2}` |
| models carrying a mesh block in their own `MSTU` | **161** (272 blocks; 7 holders carry no mesh) |
| models whose `0xAD47190C`/`0x37ED05D0`/`0xC67B4D05` name another model | **0** |
| referenced models carrying a mesh block | 0 of 0 |
| other collision-like arrays | `0xFD232466` on 1 model (2 elements), `0xC45F5F6F` on 1 model (1 element), the rest absent |

So the collision sits **on the model asset itself** -- one `STUModel` per model,
holders as separate top-level instances of the same `MSTU` blob -- and **no model
in the range references another model**, so the "visual model -> separate
collision model" shape the hypothesis allows is not what this map does.

### The per-instance body path: read, and it is a **body** path

`0x14201af49` with its chunk `0x14201b1cd..0x14201c887` (and two siblings,
`0x1420c685d..0x1420c8784` and `0x1420cade1..0x1420ccdeb`, with the same shape)
walks the model's list and builds body shapes:

The builder's holder walk is at `0x14201b6c0` (the `+0x140` slot with its tag
at `+0x148`), the three filter tests at `0x14201b6de`, `0x14201b6e8` and
`0x14201b6fd`, the parameter instance load at `0x14201b706`, the mesh unpack
call at `0x14201c3e6`, the source pointer store at `0x14201c421`, the two
`ShapeAddToBody` calls at `0x14201bc8e` and `0x14201bce1`, and `OwnerCreateBody`
at `0x14201b571`.

So a holder with mode `+0x2a == 0`, a mesh, and a passing mask becomes a **Domino
shape on a Domino body** -- `ShapeAddToBody` (`0x142cb3940`) and
`OwnerCreateBody` (`0x142cc3d90`), the ingest side, not a query. Note the two
mode fields differ: the *query* consumer `FUN_14210bac0` tests `holder+0x20 == 0`
and its other branch is a **skip** (`jne 0x14210bf47`, check 6), while this
builder tests the byte at `holder+0x2a`.

**Where the builder is reached.** This section originally scanned the fragment
starts and found no reference; **that scan was wrong** -- the addresses are
fragment starts inside a six-entry `.pdata` chain whose root is `0x14201ae60`.
The root has four `E8` callers. See check 9 ("the four callers"), which
supersedes this paragraph.

### Verdict

**The map's models do carry collision, one set per model asset, and the client
does contain a per-instance builder that turns it into a Domino body shape** --
the operator's hypothesis is right about the shape of the system, and the
"collision is per model" direction is the one this image supports: the fracturable
route (check 6) and the map chunk (check 3) are both closed, while this route is
per model and reads exactly the mesh the range's models carry.

What is missing is the **trigger**: the builder (`0x14201af49` + its chunk
`0x14201b1cd`, and its two siblings `0x1420c685d` and `0x1420cade1`) has no static
reference of any of the six forms above, so it must be handed to a runtime table.
The **exact missing read** is that registration: who stores `0x14201af49` (or a
sibling) into a registry/vtable at load, and at which type it is registered -- the
same class of runtime table that registers node types through `0x141c5ff40`, and
the same wall that `0x1420a02f6` sits behind (check 3). Until it is read, whether
the range's placeables hit this builder cannot be stated from the image.

> **This section's address is wrong, and so is its "no reference" result.
> Check 9 replaces it.** `0x14201af49` is not a function: it is the third of six
> `.pdata` fragments of the function that starts at **`0x14201ae60`**, and that
> function has **four** `E8` callers. The two "siblings" this section calls
> "the same shape" are **not** the same: neither walks the model's holder list
> (check 9 classifies them). The rest of check 8 -- the schema, the per-model
> inventory and the descriptor pass-through -- stands.

## Check 9 -- the builder is `0x14201ae60`, four state-sync functions call it, and the range's 272 mesh holders all become shapes

### The function's true extent

`.pdata` makes `0x14201ae60` the root of a **six-entry unwind chain**: the root
entry `0x14201ae60..0x14201aee1` (unwind flags `3`, both handlers) and five
chained fragments at `0x14201aee1`, `0x14201af25`, `0x14201af49`, `0x14201b1ad`
and `0x14201b1cd`, the last covering `0x14201b1cd..0x14201c887`. The six are
**contiguous**, so the body is `0x14201ae60..0x14201c887` (0x1a27 bytes), and the
root begins a real prologue: four callee-saved pushes, a frame-pointer `lea`
that puts `rbp` inside the frame, a `0x3a8`-byte frame, then the `/GS` cookie
load. The addresses the
earlier notes called the builder and its chunk are fragment starts inside this
function; a reference search aimed at them finds nothing because the function
starts earlier.

`dev/fn_extent.py <addr>` prints the chain (root, every fragment, and whether the
union extent is contiguous) for any address; every address in this check was
resolved through it. `dev/callers.py <addr>` then reports `E8`, `E9`, `jcc`,
8-byte-pointer and 4-byte-RVA references, each mapped to the **chain root** of
its containing function.

### What `0x14201ae60` does

The two arguments are `rcx` = the collision component and `rdx` = a second
object (see "the four callers": each caller decodes it through the engine's
pointer-registry idiom before the call). In the client's order:

1. **Guards.** The function returns unless **all** of these hold:
   - `[component+0x88]` (the body handle) **is** the invalid sentinel
     `0xffff_0000_ffff_ffff` -- so the builder runs only when no body exists yet;
   - `0x141ffe7b0(second)` is true, i.e. **bit 7 of `[second+0x242]` is clear**
     (a leaf accessor: load the byte, shift, invert);
   - `[component+0x250] != 0` (the component's scene node) and
     `[[component+0x250]+0xe8] != 0` (the node's model object);
   - `[[second+0x250]] != 0` and `0x14202f800([second+0x250])` is not `0xffff`
     -- that leaf reads the **Domino owner id** from
     `[[second+0x250]+0x620]` at `+0x31f8` (a `u16`), falling back to a global
     `u16` when the node has no owner record;
   - `[[[[component+0x250]+0xe8]]+0x28] != 0`: the **model**. `0x141b3d1c0` is
     the same expression out of line (`[node+0xe8] -> [that+0x28]`), and it is
     the accessor the two siblings use.
2. **The mask word.** `eax = [component+0x420]`, `bt rax,0x19`: **bit 25**
   selects `[component+0x580]` and otherwise `[component+0x470]` is the `u64`
   the holder filter tests against.
3. **The holder list.** `[model+0x140]` (field `0xCB4D298D`) with the tag at
   `[model+0x148]`; the tag rule is the one `ArrayRecord` in
   `ow_tank::model_collision` documents -- tag `1` puts the data word first and
   the count second, anything else puts the count first -- and the elements are
   read in index order.
4. **The filter**, per holder: `[holder+0x2a] == 0`, then `[holder+0x08] != 0`
   (the embedded mesh block), then `mask == u64::MAX || (mask_word & mask) != 0`.
5. **The shape**, from the `i32` at `[holder+0x20]`, a four-arm switch:

   | `[holder+0x20]` | descriptor type byte (`+0x67`) | which source it fills |
   |---|---|---|
   | `0` | `3` (mesh) | `0x141b53af0` unpacks `[holder+0x08]` |
   | `1` | `2` | `0x141b53b70` then `0x1420c2220` |
   | `2` | `1` | `0x141b53c40` then `0x1420c20d0` |
   | `4` | `0` (sphere) | `0x141b53c40`; also adds a shape to the body at `+0x90` |
   | anything else | -- | skipped |

   The switch is a `test`/`je` then two `sub ecx,1`/`je` and a `cmp ecx,2`/`jne`,
   so it is arithmetic on the mode value, not a jump table: `3`, negatives and
   values above `4` all fall through to the skip. (The decompiler presents these
   four arms in a different textual order from the address order; the switch
   tests are at `0x14201b9ba`..`0x14201b9da` and the arms begin at
   `0x14201c1f8`, `0x14201c029`, `0x14201bddc`, `0x14201b9e0`.)
6. **The body.** `OwnerCreateBody` (`0x142cc3d90`) is called with the owner id
   and a `0x30`-byte config (the entity's transform quad, plus the maximum of the
   holders' `[params+0x4c]` float as its last field), and the result is stored at
   `[component+0x88]`. A second body at `+0x90` is created only when
   `[component+0xd8] != 0`, `0x141b53a80` of the list's maximum mode byte returns
   `0`, and the global `_DAT_14414e45c != 2`; a third handle from `0x142cc3ea0`
   goes to `+0x9c`.
7. **The shapes.** One `ShapeAddToBody` (`0x142cb3940`) per selected holder,
   with `[component+0x88]` as the body; the sphere arm adds a second call on
   `[component+0x90]`. Afterwards `0x14201c507`..`0x14201c817` forms two
   four-lane vectors from the model's bounds and the body's current values
   (`0x142cb4350` reads three, `0x142cb4e90` and `0x142cb50e0` write), and
   `0x142030f10` is called on `[second+0x250]` when `[component+0x386]` is set.
   What the two vectors mean (centre of mass and inertia is the likely reading)
   was **not** read.

### The descriptor it fills

The builder's stack descriptor is the pointer `ShapeAddToBody` receives
(`lea rdx,[rbp+0x70]` at both call sites), and its offsets agree with the
consumer's reads documented in `formats/domino.md` (`+0x67` type, `+0x68` scale,
`+0x70` source), which is a second, independent read of the same layout. The
fields this function writes, with the arm that writes them:

| offset | width | value | arm |
|---|---|---|---|
| `+0x00` | quad | the entity's bounds quad, first lane pair | all |
| `+0x08` | `f32` | `100.0` (`0x42c80000`) | all |
| `+0x10` | `u16` | `1` | all |
| `+0x14` | 2 x `u32` | `-1`, `-1` | all |
| `+0x20` | `f32` | `0.8` (`0x3f4ccccd`) then the holder's parameter floats | all |
| `+0x50` | `f32` | `10.0` (`0x41200000`) | all |
| `+0x54` | `f32` | `0.7` (`0x3f333333`) | all |
| `+0x58` | `u16` | the shape's collision category, four-way: `0x800` if the parameter copy's byte at `[rbp+0x146]` is set, else `0x100` if the byte at `[rbp+0x145]` is clear, else `0x1000` when `[component+0xb9]` is set and `1` when it is not | all |
| `+0x5a` | `u16` | `0x141b53c40`'s fold of a 14-byte flag record (`model_collision::flag_mask`); the record it is given was not followed | all |
| `+0x5c` | `u16` | `[component+0x98]` | all |
| `+0x5e`, `+0x60` | `u16` | two words taken from the stack block the holder's parameter instance is unpacked into (`[rbp+0x160]`, `[rbp+0x168]`); that block's own field mapping was not followed | all |
| `+0x62` | `u16` | `0xffff` | mesh arm |
| `+0x64`, `+0x65` | `u8` | flag bytes | all |
| `+0x66` | `u8` | the arm's flag byte: `0` on the mesh, point-cloud and sphere arms; the capsule arm stores a computed byte here | per arm |
| `+0x67` | `u8` | the type byte of the table above | per arm |
| `+0x68` | `f32` | the mesh scale: `maxss(scale, 0.01)` when uniform, `1.0` when baked | mesh |
| `+0x70` | `u64` | the source: `&` the unpacked `0x30` record when uniform, `&` the baked copy otherwise | mesh |

**The mesh scale's two paths.** `0x14201c3eb` takes the entity's three scale
components, forms `sx - sy` and `sy - sz`, and compares each absolute value with
`0.01` using `comiss` + `jae`. If either is `0.01` or more (an infinity is; a NaN
disassembles as "not greater or equal") the shape is **baked**: a `0x30`-byte
record is appended to the component's array at `+0xa8`/`+0xb0` (`0x141027b80`
grows it), `0x142ccbd60(record, &unpacked, &scales)` writes the scaled mesh, and
the descriptor gets scale `1.0` and that record as its source. Otherwise the
descriptor points at the unpacked record and stores `maxss(scale, 0.01)`.

### The four callers

All four `E8` sites resolve to small functions in the same module as the builder,
and each pairs the call with `0x14201e790` (`0x14201e790..0x14201ea43`), the
**teardown** (one of the four reaches it by tail jump instead of a call): it
releases the bodies at `+0x88`,
`+0x90` and `+0x9c` through `0x142cc4150`/`0x142cc4230`, frees every `0x30`-byte
baked record at `+0xa8` (`0x142ccc1a0`), truncates that array and clears the flag
bytes `+0xb9`, `+0xba`, `+0x331`, `+0x384`, `+0x386`.

| caller root | call site | callers of it | what it is |
|---|---|---|---|
| `0x142022030` | `0x142022219` | `0x142018cc0` (`0x142018d09`), `0x142024d30` (`0x1420258c6`), and an `E9` at `0x14201616b` that sits in a region with **no `.pdata` entry** (it may not be code) | **"make the body match the state"**: it compares "has a body" (`[component+0x88]` valid) with "should have one" (`_DAT_14414e45c == 2 || [component+0xd0] == 1`), tears down when they differ, and rebuilds when the answer is "should" |
| `0x1420195e0` | `0x14201985c` | `0x142023cd0` (`0x142023d45`), which is itself called from `0x142017c60`, `0x142018f40`, `0x14201cf40` and `0x14200ed00` | **mask-membership change**: it walks the model's holders itself, looks each holder's index (`+0x24`) up in `[component+0x8b0]` with `0x140678850`, and for every holder whose mask membership differs between the caller's two mask words it tears down and rebuilds. The model comes from `0x141b3d1c0` (the node's model). `0x142023cd0` re-reads the mask word pair (`[+0x420]` bit 25 -> `0x470`/`0x580`), calls `0x142023780` and then this pass with both words; `0x142018f40` is the per-id reference-count hub that calls it |
| `0x142014b50` | `0x142014d07` | `0x140a2d390` (`0x140a2f6c8`, inside that function's second `.pdata` fragment) | **rebuild with a new mask set**: it clears two of the component's bytes (`+0x58`, `+0x59`), decodes the second argument through the registry idiom, tears down and rebuilds |
| `0x142015f70` | `0x142016129` | `0x1420ec020` (`0x1420ec4d8`), `0x1420ec670` (`0x1420ecb4d`) | **destroy or build**: one branch **tail-jumps** into the teardown (`E9` at `0x142015f95`, with `.pdata` showing no separate root because the jump *is* the entry fragment), clearing `[+0x331]` and the `u16` at `+0x384` on the way; the other branch is the same decode/build |

(Every `E8`/`E9`/`jcc`/8-byte-pointer/4-byte-RVA reference to each of these
roots was counted with `dev/callers.py`; the only 4-byte hits are the
`.pdata` records themselves and their copies inside the unwind data, which the
raw scan shows as `RUNTIME_FUNCTION` triples -- `begin` `0x14201ae60`, `end`,
`unwind` -- not dispatch entries.)

The teardown `0x14201e790` has **eight `E8` callers plus one `E9`**: the four
builder callers above, plus `0x14200f7b0`, `0x142011970`, `0x142020a90`,
`0x142021920` and `0x142024d30`, and the tail jump from `0x142015f70`. So the
build and the teardown are the two halves of one "refresh this component's
collision body" operation, driven from several places in the same module.

**The live update.** `0x142019880` is the component's per-frame update and is a
**function-pointer slot** of the shared base interface at `.rdata`
`0x142f7d688`; the "make the body match the state" function above sits at
`0x142f7d678` of the same table, and `0x14201cf40` at `0x142f7d628`. The table's
base is `0x142f7d320` and `.text` loads it with `lea` in place of a vtable store;
it carries no RTTI locator (its `-8` word is `4`) and **no 8-byte pointer to it
exists anywhere in the image**, so its class cannot be named from RTTI. What its
slots do is readable: `0x142019880` calls `0x142024d30` ("promote the pending
node and rebuild") when bit 0 of `[component+0x400]` is set and takes the
`0x1420218b0` path when it is clear, and
`0x142024d30` itself only reaches the physics setup when the node's `u16` at
`+0x1c` has bit `0x20` set and bit `0x40` clear. `0x142018f40` is the
**mask-reference hub**: it takes an array of `u32` ids and a sign `+-1`, adds the
sign to the `i32` count paired with each id in one of two 16-byte array slots at
`component+0x4c8` and `+0x4d8` (selected by its last argument), appending an
entry when the id is new, and then calls `0x142023cd0` whenever the component has
a node with a model object. That is a membership counter: it is what lets a
placed object's collision appear when it enters a collision layer and disappear
when it leaves.

### The two earlier "siblings"

Neither is a copy of `0x14201ae60`, and neither is a body builder for a model's
holder list:

| function | true extent | what it is |
|---|---|---|
| `0x1420c63b0` | root `0x1420c63b0..0x1420c646b` + fragments to `0x1420c8784` (contiguous) | a **per-attachment shape builder**: it indexes a table of `0x38`-byte records (`[owner+0xd0]`, count `[owner+0xf8]`), switches on each record's type code (cases `8`, `9`, ... are visible), and for the mesh case calls the same `0x141b53af0` + `ShapeAddToBody`. It takes the model from `0x141b3d1c0(node)` and calls the component's `0x142023d70` refresh |
| `0x1420ca550` | root `0x1420ca550..0x1420ca5ba` + fragments to `0x1420ccdeb` (contiguous) | the same shape for a second attachment set: same `0x38`-byte record table, same unpacker, same `ShapeAddToBody`, four `0x142cc3ea0` calls |

`0x1420c63b0`'s only `E8` caller is `0x1420c8a50` (`0x1420c8db5`), and
`0x1420c63b0` calls `0x1420c8a50` back: the pair recurses over a sub-object tree.
`0x1420ca550`'s callers are `0x1420c45e0` (`0x1420c4d4f`), `0x1420c9f10`
(`0x1420ca12e`), `0x1420d0db0` (`0x1420d0e48`) and the two jump thunks
`0x1420d0ce0`/`0x1420d0d50`. All three functions share the descriptor write
targets, the mesh unpacker and the two Domino entry points, so the *family* is
one system: the component's own body build (`0x14201ae60`) and two
attachment-driven builds (`0x1420c63b0`, `0x1420ca550`).

### The verdict for the practice range

**The collision really is per model instance, and the builder is live code, not
an editor path.** The reasons, in order of strength:

1. It has four static callers, all of them state-sync functions of the same
   runtime component (table above); none is an editor, exporter or tool function.
   Two of them are themselves slots of the shared base-interface table
   (`0x142018cc0` at `0x142f7d678`), i.e. reached through the component's virtual
   interface, and one of them (`0x142018f40`) is a reference-count hub for
   collision-layer membership -- a streaming/range mechanism, not a tool.
2. Its guards are runtime scene-graph reads: the second argument's byte
   `+0x242`, the node's model object `+0xe8 -> +0x28` (the same accessor
   `0x141b3d1c0` other systems use), the node's Domino owner id
   `+0x620 -> +0x31f8`, and the component's existing body handle.
3. It is the **only** producer of `ShapeAddToBody` shapes that walks a model's
   holder list. `domino.md`'s open question ("who produces the source
descriptors") is answered by this function for model geometry.

**The range's numbers, from the install** (`model_body_real::
the_range_models_holder_plan`): of the 229 model assets the range's
model-group placeables name, **279 holders** exist, **272** carry a mesh block,
**279** carry the mask word and every one of those is **all ones**, and **none**
carries the filter byte (`+0x2A`) or the shape mode (`+0x20`) -- both are the
class default `0`. So under the builder's filter **all 272 mesh holders are
selected**, each as shape mode `0`, i.e. **one type-3 mesh shape per holder**,
over the **161** models that carry a mesh block. The seven holders without a
mesh block are the only ones skipped. Pinned: model `0x0d0000000000078c` has
exactly one holder, mask all ones, mesh present, and its plan is one mesh shape.

So: **yes -- a placed instance of any of the range's 161 mesh-carrying models
builds one Domino body with one mesh shape per holder, 272 of them per
instantiation set.** What still cannot be stated is how many of the range's
placeables get the component (see below).

### What is still open here

1. **The component's owner, and therefore which map nodes get one.** The
   interface table `0x142f7d320` has no RTTI and no pointer to itself in the
   image, so the class that implements it cannot be named from the image; the
   site that gives a *static map prop's* node a collision component was not read.
   The nearest read facts are the conditions the component itself checks: the
   node's `+0x1c` bits `0x20`/`0x40` and the byte `+0x242` bit 7. Until that is
   read, "does the range's stone wall get a body" stays open -- the builder is
   live, but the *number of bodies* on this map is not established.
2. **The second argument's type.** Each caller decodes it through the same
   pointer-registry idiom (the `+0x110`/`+0x80` table walk keyed by an id from
   `0x142001f80`, ending in a rotate and a mask), so the class it points at is
   not named here. Only two of its fields are read by the builder: the byte at
   `+0x242` and the node pointer at `+0x250` used to find the owner id.
3. **The descriptor's numbers above `+0x20`.** The bounds quads at `+0x00`/
   `+0x10`, the filter words at `+0x24`..`+0x4c` and the floats at `+0x88`..`+0x94`
   are written from the holder's parameter instance (`+0x18`), the component's
   fields and the entity's transform; the per-field source of every one of those
   writes was not followed to the field level, so `ow_tank::model_body` converts
   the filter, the switch, the type bytes and the mesh scale, and leaves the rest
   documented but unconverted.
4. **The inertia/centre-of-mass block.** `0x14201c507`..`0x14201c817` computes
   the body's centre-of-mass offset and inertia from the shapes and sets them
   with `0x142cb4e90`/`0x142cb50e0`; the per-shape area/inertia terms
   (`0x142cc41a0`-family) were not read.

## Check 10 -- the owner id is a **world slot**, and the node's own body lives in it

The question this settles: the `u16` the builder hands to `OwnerCreateBody` at
`[node+0x620]+0x31f8` -- is it a world slot, a body id or an entity id? It is a
**world slot in the same global table and the same encoded-pointer idiom the
character sweep's handle decode uses**, so the per-model body is a Domino body
in a Domino world, not a scene-side record.

### What the builder hands over

`0x14201ae60` reads the id once, at the top, and passes it unchanged
(assembly, current image):

| address | what it does |
|---|---|
| `0x14201aee9` | `[node+0xe8]` -- the node's model object; a zero here returns |
| `0x14201aef9` | the node itself, from `[second+0x250]`; a zero returns |
| `0x14201af0d` | calls `0x14202f800(node, &out)` |
| `0x14201af1a` | rejects the id when it equals `0xffff` |
| `0x14201b560` | loads that same stack word as the first argument |
| `0x14201b571` | `OwnerCreateBody` (`0x142cc3d90`) |
| `0x14201b57d` | stores the returned handle at `[component+0x88]` |

The accessor `0x14202f800` is a leaf with no `.pdata`: it loads
`[node+0x620]`, and on a non-null record returns the `u16` at `+0x31f8`; with
no record it falls back to a global `u16` at `0x143363470` (this is the
"a node with no owner record still has an id" case).

### What `OwnerCreateBody` does with a `u16`

`0x142cc3d90` (263 bytes, decompiled) takes the id and a `0x30`-byte config and:

1. rejects `id >= 0x3fff`;
2. rejects the slot whose entry in the table at **`0x1442a0bc0`** is the
   empty-slot sentinel `0xc439_701c_d97e_c6aa`;
3. resolves the world pointer as `(entry + 0xb3269b88d44d5fb2) ^ 0x77600ba5adcc265c`
   -- the project's encoded-pointer idiom;
4. requires the world's `+0x15c` dword to be `0`, allocates a body index with
   `0x142cbff20`, and returns
   `(slot << 0x30) | (generation << 0x20) | index`, the generation read from
   the body entry `[world+0x288] + index*0x1c0` at `+0x198`;
5. on any refusal returns `0xffff_0000_ffff_ffff`.

Every constant here is the sweep's: `0x1442a0bc0` is the world handle table
check 7 decodes handles through, `0x3fff` is the slot bound those accessors
reject at, the two addends/XOR word are the same, and the body pool
`[world+0x288]` with stride `0x1c0` and the generation at `+0x198` is the pool
`VariantPosition`'s rigid branch reads. **The `u16` is therefore a world slot,
not a body id and not an entity id** -- a body id would be the `index` field,
which the id never touches.

### Where the slot comes from, and what else uses it

The slot is filled by the node's own geometry initialiser
`FUN_14202c660` (`0x14202c660..0x14202d676`), which the node's creation path
reaches through the table entry at `0x143367670`:

| address | what it does |
|---|---|
| `0x14202d0e1` | tests bit `0x200` of the node's flags dword at `+0x454`; without it the node gets **no** `+0x620` record at all |
| `0x14202d112` | the `0x3210`-byte record: `GeometryBaseCtor`, its two tables `0x143367678`/`0x1433676b0`, body handle `0xffff_0000_ffff_ffff` at `+0x31fc` |
| `0x14202d198` | stores it at `[node+0x620]` |
| `0x14202d1d1`, `0x14202d1e8` | two `FUN_142cc3c80(config)` calls: the world slots at `+0x31f8` and `+0x31fa` |
| `0x14202d202`, `0x14202d210` | both must not be `0xffff` |
| `0x14202d268`/`0x14202d26f` | `OwnerCreateBody(+0x31f8, config)` -- the **node's own static body** |
| `0x14202d27b` | stores that handle at `+0x31fc` |
| `0x14202d290`, `0x14202d2aa` | `0x142cc4af0`/`0x142cc4b50` on the slot |
| `0x14202d2f5` | `0x14209ae30(record, +0x31f8, node)` -- the geometry registration |

`FUN_142cc3c80` (59 bytes of loop, decompiled) is the **world allocator**: it
walks the table at `0x1442a0bc0` for the first entry equal to the sentinel,
allocates a `0x4d0`-byte world, calls `DmWorldConstruct(world, config)`, and
stores the encoded pointer `(world ^ 0x77600ba5adcc265c) + 0x4cd964772bb2a04e`
with the slot written back into `world+0x194`. The two addends are exact
inverses modulo `2^64`, so the allocator's encode and `OwnerCreateBody`'s decode
round-trip; and the empty-slot sentinel is not a separate constant -- it is
exactly *the encoding of a null world pointer*, which is why "entry ==
sentinel" reads as "slot free". It has four `E8` callers: the node initialiser
(two of them), `0x1407670e0`, and the Domino context initialiser
`0x142cde350`.

Note what the allocator does **not** do: it never looks for an existing world
by key, so each call is a new world. Sharing therefore has to come from the
caller, and one caller shows how: `0x142cde350` (the Domino context
initialiser, two `E8` callers `0x142cdf8e0`/`0x142cdf930`) writes its world slot
at `ctx+0x58` and, when it is given a parent context, **copies the parent's slot
instead of allocating** and links itself into the parent's child list at
`+0x140`. So a nested context inherits its parent's world; a node's own
initialiser is the top of such a chain and allocates.

So: the id the builder uses is the slot the node itself created a body in
(`record+0x31fc`) and registered its geometry in. The per-model body is a
sibling of the node's own static body, in the node's world.

### Verdict

| question | answer | evidence |
|---|---|---|
| is the `u16` at `+0x31f8` a world slot? | **yes** | `OwnerCreateBody` indexes `0x1442a0bc0` with it and rejects `>= 0x3fff`; `FUN_142cc3c80` writes it there |
| is it a body id? | no | a body id is the handle's low `u32`, produced *from* the slot; the id is used before any body exists |
| is it an entity id? | no | it is a table index into `0x1442a0bc0`, the table check 7 documents as the world handle table |
| is the world the character sweeps reached the same way? | **yes, same table and same decode** | check 7's accessors compute `(*(u64*)(0x1442a0bc0 + slot*8) + K2) ^ K1`; `OwnerCreateBody` computes the identical expression from the `u16` |
| does the per-model body land in the character's world? | **open, in one place** | the component's handle `[+0x88]` uses the same packing (`slot << 0x30`, `[component+0x88]` tested against `0xffff_0000_ffff_ffff` at `0x14201ae9f`..`0x14201aecc`), so the bodies are *addressable* by the sweep's decode; whether the character's own handle names the same slot is a runtime value this turn did not read |

### The attachment gate, read to the field

The gate is a chain of four checks, all runtime node state:

| # | address | step |
|---|---|---|
| 1 | `0x14202d0e1` | `[node+0x454]` bit `0x200` -- decides whether the node has a `+0x620` record at all, i.e. whether `0x14202f800` can ever return a non-`0xffff` id |
| 2 | `0x142024fb4`/`0x142024fbb` | the component's pending node (`+0x490`) is promoted into its current node (`+0x250`) |
| 3 | `0x142024fee`, `0x142024ff2`, `0x142024ffb` | the node's `u16` at `+0x1c`: **bit `0x40` set or bit `0x20` clear is a refusal** (so the gate is `0x20` set and `0x40` clear) |
| 4 | `0x142025004`, `0x142025013` | the node's model object `[node+0xe8]` and its `+0x84` bit `0x80` |

plus the builder's own two: the second argument's byte `+0x242` bit 7 must be
clear (leaf `0x141ffe7b0`, exactly `(byte >> 7 ^ 1) & 1`), and the model
`[node+0xe8] -> +0x28` (leaf `0x141b3d1c0`) must be non-null.

**None of these four is model-asset data.** The model asset contributes only
the holder list the builder then walks; the deciding bits are the node's own
flags (`+0x1c`, and `+0x454` whose bit `0x200` `0x14202c660` copies from the node
creation record's dword at `+0x10`, at `0x14202c6e4`) and the node's model
object. A placed model node therefore satisfies steps 2-4 as soon as it has a
model object; step 1 is the one that can fail, and it is decided by the node's
creation record, not by the `MSTU`.

### The component's class: what the table is, and what is not read

Read facts, all current-image:

- `0x142f7d320` is in `.rdata` (`0x142f2d000`..`0x143845fff`). Its `-8` word is
  `4`, not a locator, and **no 8-byte pointer to it exists anywhere in the
  image** (an 8-byte scan of the whole image returns **0** hits) -- every
  reference is rip-relative (`dev/rip_refs.py 0x142f7d320`). So no RTTI and no
  self-pointer, as check 9 said.
- Of those references, **six are code under a `.pdata` root** and they are all
  tiny: `0x1405418b0`, `0x140541ae0`, `0x1405813d0`, `0x141b29b20`,
  `0x140581630` (sizes `0x38`, `0x58`, `0x68`, `0x78`, `0xd8` -- each stores the
  table at `[this]` and, flagged, frees that size) and the constructor
  `0x141c58170` (a `0x38`-byte object with the table at `[this]`).
- Of the 171 references inside the `0x142f00000..0x142f2ffff` range, 162 are in
  regions with **no `.pdata` entry**: tiny `.text` initialiser stubs
  (`0x142f017e0`, `0x142f16ca0`.., `0x142f2a5f0`..), each of which copies the
  *same* table address into a **global of its own** (the other 9 are inside
  ordinary `.pdata`-covered functions, e.g. `0x142f16d90`, `0x142f2a730`). That is the signature of
  vtable folding: many classes whose vtable content is identical collapse onto
  one address, so this table cannot name one class -- it is a shared base
  interface.
- The component the builder receives is **not** the object holding that table:
  every interface slot runs with `this` twenty bytes *past* the object the
  non-virtual workers get. Three independent field pairs prove it: the slot
  reads `[this+0x230]`, `[this+0x400]`, `[this+0x470]` where the worker
  `FUN_142024d30` reads `[object+0x250]`, `[object+0x420]`, `[object+0x490]`,
  and the slot passes `this - 0x20` to that worker (`0x142019916`). The body
  handle is `[this+0x68]` in the slot and `[object+0x88]` in the builder -- the
  same field, 0x20 apart.
- The component's node-binding entry is **`FUN_142017010`** (516 bytes): it
  refuses when a pending node (`[object+0x490]`) is already set, detaches when
  its node argument is null (`0x142021920`, `0x14201ea50`), and otherwise
  resolves the node through `0x141b34010`, calls `0x141b3c6d0(node, arg3, 1,
  [object+0xa8c])`, and on success stores the callback pair at `+0x568`/`+0x570`
  and calls `0x142022420(object, node, 0)`. It also reads the flags qword at
  `[object+0x420]` and the two words it hands to the stack context at `+0x280`
  and `+0x288`. It has **twelve `E8` callers** (`0x140a29960`, `0x140a2d120`,
  `0x142016f10`, `0x142017c60`, `0x142019880`, `0x14201cf40` twice,
  `0x142020a90`, `0x142035010`, `0x142070ab0`, `0x1420ea390`), so it is a shared
  entry, not one component type's private method. `0x142016f10` is the
  "re-attach only when the node's owner changed" wrapper: it compares the
  caller's id with the id decoded from the node's encoded pointer at `+0x118`.

**What is not read, and is the exact missing read for "which nodes receive the
component":** the site that *allocates* this component for a node. It is not
`RegisterComponentTypeDescriptor` (`0x1421cd360`): none of the ~7,956
registrations' hook arguments is in the `0x1420xxxx` module, and none installs
`0x142f7d320` (the registrations that do mention component sizes -- `0xa98`,
`0xaf0`, `0xc70`, `0xd98`, `0xda8`, `0xdc8`, `0x1010`, `0x1150` -- all pass a
`.rdata`/`.text` hook set that the component's own slots do not match). The
lookup that pairs a node with this component therefore happens through a table
this turn did not open: the one that gives `0x142017010` its arguments. Until it
is read, "how many of the range's 550 placeables get a component" stays open,
and with it the number of bodies.

### The range, against the gate

| gate | decided by | does the range decide it from asset data? |
|---|---|---|
| `0x200` in `[node+0x454]` (record exists) | the node creation record's dword at `+0x10` (its third dword), copied by `0x14202c6e4` | **no** -- it is per-node runtime state, not in the `MSTU` |
| `+0x1c` bit `0x20` set, `0x40` clear | the scene-graph flags word, written by the link/insert helpers (`0x141b25205`, `0x141b256f8` and the other 16-bit stores at `+0x1c`) | **no** |
| `[node+0xe8] != 0` and its `+0x28` model | the node's loaded model object | yes, in the weak sense: a placed model node has one |
| `[second+0x242]` bit 7 clear | the entity record behind the node | **no** |

So no install-data test can prove that the range's props pass. What *can* be
said, and is said by check 9's numbers, is that if a placed node passes, the
two worlds and the body are already there: the node created its `+0x620`
record and its own body from the same slot, and every one of the 272 mesh
holders becomes a shape on a second body in that same world.

## Check 11 -- the character's world slot **is** the node's slot, through the same accessor

The question this settles: who creates the qword at `[controller+0x1ad8]`, and
which world does it name -- the per-node world (`[node+0x620]+0x31f8`), a global
world, or another slot? It is the per-node slot, read through the same accessor
the per-model builder uses. This is the last link between "the per-model bodies
exist" and "a frame can step".

### The field is the sweep's first argument

`dev/disp_scan.py 0x1ad8 --writes` (new tool) scans the displacement bytes and
re-decodes each occurrence with capstone, preferring a REX-prefixed decode, so a
qword load is not mistaken for the dword load whose shorter decode ends at the
same place. It finds twelve write or compare operands at that displacement (ten
qword stores, one of them inside a bulk-zeroing run at `0x142661310`, and two
comparisons) and 65 memory operands in all. Only **`0x140ca6132`** gives the mover
class's field a live value; the store in the same class at `0x140ca3b08` only
zeroes it, and the remaining sites lie in other classes (they do not read this
class's `+0x1a60`/`+0x1a64` neighbourhood). The read that matters is
**`0x140ee8aca`**, inside `0x140ee8960..0x140eea630`: the qword at the record's
`+0x1ad8` becomes the first argument, and `RunCharacterControllerSweep`
(`0x142cbcee0`) is called at **`0x140ee8ad1`**. So this field is exactly the
"world/query struct" check 7 describes.

### The one store that gives it a live value

**`0x140ca6132`**, inside `FUN_140ca5db0` (`0x140ca5db0..0x140ca6197`, a single
`.pdata` root, no chain): the field receives the qword returned by
**`0x142cbcd40`**, called at `0x140ca6128` with a stack descriptor. The class is
the **mover base**: `ConstructMoverBase` (`0x140ca37d0`) builds a `0x1b70`-byte
base sub-object and installs the base vtable `0x1431101e8`; inside it the field
is zeroed at **`0x140ca3b08`** (the register stored there is zero at that point),
so the only live value comes from that one store.

### The descriptor that store reads

`FUN_140ca5db0` builds its descriptor on the stack at `rbp-0x80`. The offsets,
read from the function's own stores:

| descriptor offset | value | written at |
|---|---|---|
| `+0x00` dword | `0` -- the query tag: a **character shape**, not a rigid body | `0x140ca608c` |
| `+0x10`, `+0x20` | the mover's wish position and its radii | `0x140ca6072`, `0x140ca607b` |
| `+0x24` dword | `-1` | `0x140ca6085` |
| `+0x40` u16 | the shape-category word (`8`, `0x10`, `0x18`, or `2`) | `0x140ca60c0` |
| `+0x42` u16 | `0x10b9`, or `0x10a1` when `[mover+0x1a7e]` is set | `0x140ca60ae`, `0x140ca60b7` |
| `+0x44` u16 | `[mover+0x23c]` | `0x140ca60d4` |
| `+0x46` u16 | **the world slot** | `0x140ca60b3` |
| `+0x58` qword | the second argument | `0x140ca611f` |

The slot at `+0x46` is what `0x14202f800(node)` returns: the call is at
**`0x140ca609b`**, its result is read as a `u16` at **`0x140ca60ab`** and stored
at **`0x140ca60b3`**. The node is `[mover+0x240]`, and `[mover+0x240]` was
assigned `[param_2+0x250]` at the top of the function -- a scene node.

`0x14202f800` is a leaf with no `.pdata` (checked with `dev/fn_extent.py`): it
returns the `u16` at `[node+0x620]+0x31f8` (`0x14202f80c`, `0x14202f812`), and
with **no** `+0x620` record it returns the `u16` at the global `0x1433674e0`
(`0x14202f81c`, `0x14202f823`). That global holds **`0xffff`**. It is the same
accessor the per-model builder calls at `0x14201af0d` (check 10).

### What `0x142cbcd40` builds, and where the slot ends up

`0x142cbcd40` allocates a `0x10`-byte holder through `0x142cadee0`, copies the
caller's descriptor into a `0xa0`-byte stack record with the slot moved to
`+0x86`, then calls **`0x142cf5440`** and stores the result as the holder's first
qword. `0x142cf5440` is `0x142cadee0(0x590, 0x10, ...)` plus
**`0x142cf35a0(new, record)`**, so the sweep's first argument (`*holder`) is that
`0x590`-byte record; `0x142cbcd40` returns the holder, which the caller stores at
`[mover+0x1ad8]`.

`0x142cf35a0` fills the record:

| record offset | value |
|---|---|
| `+0x00` dword | the descriptor's tag (`0` here) |
| `+0x04` u16 | **the world slot** (the descriptor's `+0x86`) |
| `+0x08` qword | `0xffff_ffff` with `0xffff_0000` at `+0x0c`; later the **shape** handle |
| `+0x10` qword | `0xffff_ffff` with `0xffff_0000` at `+0x14`; later the **body** handle |

and then branches on the tag: tag `0` calls **`0x142cc3f50(slot, &shape)`** and
stores the returned handle at `+0x08`; tag `1` calls `OwnerCreateBody(slot,
&config)` and stores that handle at `+0x10`. This is exactly the layout
`0x142cf3f80` walks (check 7: tag at `+0x00`, shape handle at `+0x08` when the tag
is not `1`, body handle at `+0x10` when it is), so the two reads confirm each
other.

`0x142cc3f50` (`0x142cc3f50..0x142cc47f`, decompiled) is the *shape-entry* twin of
`OwnerCreateBody`: it rejects a slot `>= 0x3fff`, rejects the empty-slot sentinel
`0xc439_701c_d97e_c6aa` in the table at **`0x1442a0bc0`**, decodes the world with
the same two addends and XOR (`+0xb3269b88d44d5fb2`, `^ 0x77600ba5adcc265c`),
requires `[world+0x15c] == 0`, allocates a `0x50`-stride entry from the shape pool
at `world+0x2a0` with `0x142cbe600`, fills it with `0x142cd4290`, and returns
`(slot << 0x30) | (generation << 0x20) | index` with the generation read from the
entry at `+0x34`. Every constant is the one check 7 and check 10 already record,
so the shape entry lands in the world named by the *slot*.

### Verdict

| question | answer | evidence |
|---|---|---|
| what creates `[controller+0x1ad8]`? | the mover's per-sweep setup `FUN_140ca5db0`, from `0x142cbcd40`'s return, at `0x140ca6132` | `0x140ee8aca` reads it for the sweep; `0x140ca3b08` only zeroes it |
| what is the struct at `+0x1ad8`? | a holder whose first qword is a `0x590`-byte query record: tag, world slot, shape/body handle | `0x142cf35a0`'s field writes; `0x142cf3f80`'s reads |
| which world slot does it name? | the **per-node** slot `[node+0x620]+0x31f8`, via `0x14202f800(node)` | `0x140ca609b` -> `0x14202f800`; `0x14202f80c` |
| a global world? | only as the accessor's fallback, and that fallback is `0xffff`, which both world users reject | `0x14202f81c`; global `0x1433674e0` = `0xffff` |
| is it the same slot the per-model bodies use? | **yes, the same accessor and the same field** | `0x14201af0d` (builder) and `0x140ca609b` (mover) both call `0x14202f800`, which reads one field |
| same *node*, therefore same world? | **runtime**, not decidable here | the mover takes `[X+0x250]`; the builder takes `[[second+0x250]]`; both are a `+0x250` node pointer |

**Consequence for the gate.** Because the accessor's fallback is `0xffff` and
both `OwnerCreateBody` (`0x142cc3d90`) and `0x142cc3f50` reject it, the node
record bit `[node+0x454] & 0x200` gates **both** routes at once: without it the
per-model bodies have no world *and* the character's shape entry has none. That
makes step 1 of check 10's attachment gate the single decisive bit of the whole
per-model route -- and it is the node's own creation state, not a model asset.

**Not read here:** `0x142cbd190(holder, [mover+0x650])`, called at `0x140ca6139`
right after the store, and the meaning of the mover's `+0x650`. Also not read: the
`[param_2+0x298] == 0` guard that decides whether the query is rebuilt at all.

## Check 12 -- the component's allocation site, class and registration

This is question A of the hand-off: the code that allocates the collision
component and pairs it with a node, the class it belongs to, and what makes a
node get one.

### The constructor: `0x14200a9a0`

`0x14200a9a0` (1966 bytes, `this` in `RCX`, returns `this`) initialises **every
field** the builder and the state-sync functions read, which is what identifies
it:

| field | value | written at |
|---|---|---|
| `+0x00` | primary vtable **`0x142f7d608`** | `0x14200a9e1` |
| `+0x08` | the encoded-owner sentinel `0xca3e1a1d3d45d5ca` | `0x14200a9cc` |
| `+0x14` | `1` | `0x14200aa06` |
| `+0x20` | base-interface vtable **`0x142f7d648`** | `0x14200a9f5` |
| `+0x28` byte | `0` | `0x14200a9c8` |
| `+0x58` | a float | `0x14200ad46` |
| `+0x88`, `+0x90`, `+0x9c` | the three body handles (invalid sentinel) | `0x14200af08`, `0x14200af0f`, `0x14200af37` |
| `+0x98` u16, `+0xba` dword | `0`, `0x10000` | `0x14200af64`, `0x14200af6c` |
| `+0xa8`, `+0xb0` | the baked-record array (null) | `0x14200a9da`, `0x14200a9ee` |
| `+0x118` | third vtable `0x142f7d458` | `0x14200aa0a` |
| `+0x250`, `+0x490` | the current and pending **node** (null) | `0x14200aeb6`, `0x14200aebd` |
| `+0x384` u16, `+0x386` byte | `0` | `0x14200b06e`, `0x14200b076` |
| `+0x400` | 16 bytes | `0x14200b0df` |
| `+0x420` | the flags qword | `0x14200ac2f` |
| `+0x470`, `+0x580` | the two mask words | `0x14200af95`, `0x14200afb8` |
| `+0x4c8`, `+0x4d8` | the two `u32`-id membership slots | `0x14200ab12`, `0x14200ab20` |
| `+0x568`, `+0x570` | the callback pair | `0x14200b013`, `0x14200b01a` |
| `+0x8b0` | the holder-index container (null) | `0x14200b021` |
| `+0xa8c` | a dword (`EBP` at that store) | `0x14200aeb0` |
| `+0xaa0`, `+0xaa8` | the id array and its count | `0x14200b0fb`, `0x14200b102` |

The `+0x08` value decodes to **zero** under the rotate/XOR idiom the class's own
methods apply to it (`+0x1e2421a4798a328`, rotate by `0x22`, `^ 0xe1379e3cb308170d`,
rotate by `0x2b`), so at construction the owner back-pointer is the encoded null.

### The allocation: `0x14200a5f0`, size `0xad0`

`0x14200a5f0(container, flag)` has two branches:

- `flag != 0` (the test is at `0x14200a5f6`): it allocates through the
  size-mangling thunk `0x140553a70` with **`0xad0`** (`0x14200a5fa`, call at
  `0x14200a5ff`), calls the constructor on the result (`0x14200a60a`), validates
  the object's vtable word and hands the object to `0x140542320` when the check
  fails (`0x14200a623`, `0x14200a62b`), calls primary slot 1 (`0x140551680`) at
  `0x14200a63b`, and returns the object (`0x14200a648`);
- `flag == 0`: it takes the instance from the reflected-type container at
  `container+0x10` (`0x14200a651`, `0x141c4b270` at `0x14200a658`) and, when the
  byte `[container+0x28] & 0x10` is set (`0x14200a65d`), runs the same
  initialisation in place -- the tail jump at `0x14200a66b` enters the
  constructor; otherwise it returns the instance untouched (`0x14200a675`).

So the class size is **`0xad0`**. Its only reference outside `.pdata` in the
whole image is the 8-byte pointer at **`0x1433633d0`**, which is slot 1 of the
vtable at `0x1433633c8`.

### The registration: `0x14200bab0`, kind `0x14`, name `Unknown`

`0x14200bab0` (root `0x14200bab0..0x14200badc` plus the chained fragment
`0x14200badc..0x14200bbce`, contiguous) allocates `0x30` bytes, reads the build id
from `[parent+0x10]` (`0x14200bb2c`), installs the vtable **`0x1433633c8`**
(`0x14200bb2f`, stored at `0x14200bb3b`), writes the byte `4` at `+0x08`
(`0x14200bb42`), initialises a reflected-type container at `+0x10` (`0x14200bb3e`,
`0x141c4aea0` at `0x14200bb46`), and calls
`RegisterTypeWithSize(container, name, 0x14, 0xad0, 0, buildId)` (`0x14200bb36`
carries the size `0xad0`).

- **size** `0xad0` -- two independent reads: the registration and the allocation.
- **kind flags** `0x14` -- from `RegisterTypeWithSize` (`0x141c4b050`) this is the
  `8|4` family, i.e. it builds the `0x68`-byte type core at `0x14328c7b8`; the
  registration's own `+0x18` gets `0x14`.
- **name** -- the registered name is the 7-byte string at `0x143aa01cc` XOR the
  8-byte key at `0x143aa01c0` (length at `0x143aa01c8`), which decodes to
  `Unknown`. The neighbouring registration in the same `.data` block uses the
  same decoy name, so the class has no script name in this image.

### The vtables, decoded from the image

| vtable | object offset | slots |
|---|---|---|
| `0x142f7d608` **primary** | `+0x00` | 0 `0x140548c50`, 1 `0x140551680`, 2 `0x14054cf00`, 3 `0x14053a420`, **4 `0x14201cf40`**, **5 `0x142020a90`** (the destructor read as
`FUN_142020a90`), 6 `0x14053a420`, 7 `0x14053a070` |
| `0x142f7d648` **base interface** | `+0x20` | 0 `0x14054816c`, **1 `0x14200ee80`**, 2/3 `0x14053a420`, **4 `0x14200f7b0`**, 5 `0x14053a070`, **6 `0x142018cc0`**, **7 `0x142011970`**, **8 `0x142019880`**, 9 `0x142011f40`, 10/11 `0x14053a420` |
| `0x142f7d458` **third** | `+0x118` | 0 `0x14053a420`, 1 `0x14054c660`, 2 `0x14054f730`, 3 `0x14054db20`, 4 `0x1405488c0`, 5 `0x14054ce20` |
| `0x1433633c8` **type object** | its own object | 0 `0x14077fea0`, **1 `0x14200a5f0`** (create instance), 2 `0x14077fd10`, 3 `0x14077fe80`, 4 `0x14200a680`, 5 `0x14053a420` |

**This corrects check 10's "the table's base is `0x142f7d320`".** `0x142f7d320` is a
*neighbouring* folded vtable: its slot 0 is the `0x38`-byte class's deleting
destructor `0x1405418b0`. The component's base interface starts at
**`0x142f7d648`**, and the three slots checks 9 and 10 name as `0x142f7d628`,
`0x142f7d678` and `0x142f7d688` are **primary slot 4** and **interface slots 6
and 8** respectively. That is why the table looked like a 100-slot interface: it
is four tables butted together in `.rdata` with no padding between them.

### The pair-with-a-node entry: `0x14200f840`

`0x14200f840` (105 bytes) does four things: calls the constructor with the
caller's `RCX` (so the *caller* owns the storage), stores the encoded owner from
its third argument at `+0x08` (`0x14200f886`), writes the byte `4` at `+0x10`
(`0x14200f871`), and calls **primary slot 4 = `0x14201cf40(object, entity)`**
(`0x14200f894`). It has **no** static reference: `dev/callers.py` reports 0 `E8`,
0 `E9`, 0 `jcc`, 0 8-byte pointer, and its single 4-byte hit at `0x141139f6c` is
the displacement field of a rip-relative load (`0x141139f69`), not a dispatch
entry. In this image the wrapper is therefore unreferenced; the live path is the
type object's slot 1 (`0x14200a5f0`) plus the virtual attach.

### Where `FUN_142017010`'s eight arguments come from

`0x14201cf40` is the *attach*: it has exactly one reference -- the vtable slot
`0x142f7d628`. Its two arguments are the component and an **entity-side object**
that answers component lookups by type id through its own vtable (slot 2, at
`+0x10`; three ids are visible: `4`, `0x11`, `0x31`). It resolves the component's
encoded owner at `+0x08` into the registry object, then supplies `FUN_142017010`
from data:

| `FUN_142017010` argument | source |
|---|---|
| 1 (component) | `this` |
| 2 (node id) | the qword at `+0xa8` of the object the entity returned for a type id; or the entity argument itself |
| 3 (node id) | the qword at `+0xb0` of that same object; or the returned object |
| 4 | `0` |
| 5, 6 (callback pair) | `0`, or a virtual call's result |
| 7 | a byte from an entity virtual |
| 8 | `0`, or the stack context word |

A second, independent supplier is `0x140a29960`, which takes the eight arguments
from an **array of `0x10`-byte `{id, pointer}` records** at `param_1+0x38` (count
at `param_1+0x40`), matched against `[param_2+0x134]`: the matching record's
pointer becomes argument 3, `[node+0xa8]` argument 2, and arguments 4..8 are `0`.
So "the table that supplies the eight arguments" is a **runtime registration
array**, not map or asset data.

Both node arguments are then resolved through the registry by `0x141b34010`
inside `FUN_142017010`, so they are ids/handles rather than raw pointers -- which
is why an unread node creation record, not a missing object, is what keeps the
body count unstated.

### The component's own attach hook: `0x14200ee80` (= interface slot 1)

`0x14200ee80(interface_this, entity)` takes the node from **`[entity+0x250]`**
(an object whose transform quad sits at `+0xd0`, i.e. the entity record) and pairs
with it directly: `0x14201cbe0(this-0x20, entity)`,
`0x1420218b0(this-0x20, entity, node)` when the component already has a node, and
`0x141b4c700(this+0x130, [entity+0x250])`; then it copies the entity's transform
quad (`+0xd0..+0xf8`) and `[node+0x6c8]` into the component's transform block at
`+0x390..+0x3f7`. It has no `E8` caller either -- only the table pointer at
`0x142f7d650`. **It tests no asset-derived condition**: it pairs with whatever
node the entity carries.

### What makes a node get one -- the exact state of the read

Read facts:

1. The creation is driven by the reflected **type object** (vtable
   `0x1433633c8`, built by `0x14200bab0`): its slot 1 allocates and constructs,
   its slots 0/2/3 are engine functions in the `0x14077fxxx` range, and its slot
   4 is per-type (`0x14200a680`).
2. Nothing in the image references the factory except that table, and nothing
   references `0x14200bab0` directly (`dev/callers.py`: 0 `E8`/`E9`/`jcc`/8-byte;
   its four 4-byte occurrences are the `.pdata` record and its unwind copies plus
   one coincidence at `0x140a64f5c`, where the bytes are the tail of a
   rip-relative displacement and a nearby immediate, not a dispatch entry). So
   the *driver* that walks an entity's component list and calls slot 1 is code
   the static call graph does not name.
3. The attach hooks themselves (interface slots 1 and 4) decide nothing from
   asset data: they take the entity's node and bind.
4. The node's own side is read: `0x14202c660` stores the node creation record's
   dword at `+0x10` into `[node+0x454]` at **`0x14202c6e4`** (the read is
   `0x14202c6e0`), and passes the record's qword at `+0x18` to the world
   allocator `0x142cc3c80` twice for `+0x31f8`/`+0x31fa`. But the *producer* of
   that record is unread: `0x14202c660` is reached only through the 8-byte
   pointer at `0x143367670`, which is **slot 32** (byte offset `0x100`) of the
   scene-node table at `0x143367570` (installed into a global by the stub at
   `0x142f2c201`), i.e. through the node's own vtable.

**Verdict.** The gate is **not asset-derived in the code read**, and it is not
runtime-only in the sense of "an unpredictable value" either: it is *undecided
here* because the two inputs -- whether the entity carries this component, and
what the node creation record's `+0x10`/`+0x18` carry -- come from the entity
system and the node type, neither of which was read. No install test can stand in
for that: the practice range's placeable records say nothing the client's own
component driver has been shown to consult, and inventing a mapping from a
placeable record to component kind `0x14` would be exactly the seed-list guess
`AGENTS.md` rule 11 forbids. **The install test is therefore not written, and the
`not tested` row in the checks table below is extended rather than removed.**

**The exact missing read** (three, in order):

1. the engine-side driver that instantiates a reflected type for an entity --
   the caller of `0x1433633c8` slot 1 (`0x14200a5f0`) and of interface slot 4
   (`0x14201cf40`), in the entity/component module around `0x14077f000`;
2. the producer of the node creation record consumed at `0x14202c660` -- i.e.
   the caller of slot 32 (byte offset `0x100`) of the table at `0x143367570`;
3. `[mover+0x650]` and `0x142cbd190` from check 11, if the sweep's per-frame
   rebuild matters for the frame loop.

## What this leaves

| line | state |
|---|---|
| the fracturable brush path on this map | closed: the range names no `C` (check 1) |
| the map's kind-`0x11` chunk | not reachable; no producer read (check 3) |
| the model's prebuilt collision mesh | **per model asset**: 163/229 models carry a holder list (279 holders, 272 mesh blocks), one `STUModel` each, no model-to-model reference (check 8) |
| a per-instance body builder | **read: `0x14201ae60..0x14201c887`**, four `E8` callers (state-sync functions of the collision component), teardown `0x14201e790`; it walks the model's holders and emits one `ShapeAddToBody` shape each (check 9) |
| which holders become shapes on this map | **all 272 mesh holders**: mask all ones, filter byte and shape mode at the class default `0`; 161 of the 229 models (check 9) |
| the two "siblings" | not copies: `0x1420c63b0` and `0x1420ca550` are attachment-driven shape builders over a `0x38`-byte record table (check 9) |
| the owner id the builder uses | **read: a Domino world slot** in the table at `0x1442a0bc0`, the same table and decode the sweep's handles use; produced by the allocator `0x142cc3c80` and stored at `[node+0x620]+0x31f8` by the node initialiser `0x14202c660` (check 10) |
| the node's own body and the per-model body | both from `OwnerCreateBody` on that one slot: `+0x31fc` (node) and `[component+0x88]` (per-model), so they share a world (check 10) |
| whether the character's world is that world | **read: yes, same slot family** -- the sweep's query record carries `0x14202f800(node)`'s `u16` at `+0x04` and `0x142cc3f50` creates the character's shape entry in the world that slot names, i.e. the same `[node+0x620]+0x31f8` the per-model bodies use; the accessor's fallback is `0xffff`, which both world users reject (check 11) |
| the component's owner class | **read: constructor `0x14200a9a0`**, allocated at `0x14200a5f0` with size **`0xad0`**, registered by `0x14200bab0` as kind `0x14`, size `0xad0`, name `Unknown`; primary vtable `0x142f7d608`, base interface **`0x142f7d648`** (the earlier `0x142f7d320` was a neighbouring folded vtable), third `0x142f7d458`, type object `0x1433633c8` (check 12) |
| the attachment gate | **read field by field**: node `+0x454` bit `0x200` (record exists), node `+0x1c` bit `0x20` set / `0x40` clear, node `+0xe8` model object and its `+0x28` model, entity `+0x242` bit 7 clear; all four are runtime node/entity state, none is model-asset data (check 10), and bit `0x200` is the deciding bit for **both** the per-model bodies and the character's world slot (check 11) |
| which entities carry the component | **open**: the component pairs with whatever node its entity has (interface slots 1 and 4 test nothing asset-derived); the driver that instantiates the type for an entity is unread (check 12) |
| the descriptor's bounds/filter fields | documented per offset, not converted: their sources (holder parameters, component state, entity transform) were not followed field by field (check 9) |
| the scene-side mesh query | separate and query-only (checks 4, 6) |
| the character's world | the Domino world, swept through encoded handles (check 7) |

Ordered next reads (the per-model route):

1. **The entity-side driver that instantiates a reflected type for an entity.**
   Check 12 reads the class end to end -- constructor `0x14200a9a0`, allocation
   `0x14200a5f0` (`0xad0`), registration `0x14200bab0` (kind `0x14`), primary
   vtable `0x142f7d608`, interface `0x142f7d648` -- and reads the attach hooks
   (interface slot 1 `0x14200ee80`, primary slot 4 `0x14201cf40`), none of which
   tests an asset-derived condition. All of them are reached virtually only. The
   missing link is therefore the **engine driver** that walks an entity's
   component list and calls the type object's slot 1 (`0x1433633c8 + 8`,
   `0x14200a5f0`), in the entity/component module around `0x14077f000`. Until it
   is read, the number of bodies on this map is unstated.
2. **The producer of the node creation record.** `0x14202c660` copies the
   record's dword at `+0x10` into `[node+0x454]` at `0x14202c6e4` (read at
   `0x14202c6e0`) and hands the qword at `+0x18` to the world allocator
   `0x142cc3c80`; the record itself arrives only through the node's own vtable
   (slot 32 of the table at `0x143367570`, whose 8-byte entry is
   `0x143367670`). Reading that slot's caller decides whether bit `0x200` is
   asset-derived -- and with it whether an install test can settle the range.
2. **The mover's per-frame rebuild.** `[mover+0x650]` and `0x142cbd190`
   (`0x140ca6139`) are called right after the sweep's query record is created
   (check 11); they belong to the frame loop rather than to the collision build.
3. **The descriptor's remaining fields.** The parameter instance at
   `[holder+0x18]` (`0x4EFF450F`) supplies the filter words and radii the
   descriptor carries at `+0x24`..`+0x4c`, and the entity's transform supplies
   the quads at `+0x00`/`+0x10`; each write is at a known address (see check 9's
   table) but its source field was not followed.
4. **The inertia block.** `0x14201c507`..`0x14201c817` turns the built shapes'
   bounds into the body's centre of mass and inertia; the per-shape terms
   (`0x142cc41a0` family) are unread.

## Checks

| claim | check |
|---|---|
| the range names none of the 16 fracturable GUIDs | `static_collision_real::check_1_the_range_names_no_fracturable_descriptor` (550 placeables, 202,688 bytes, 0 hits) |
| the range's models carry the mesh block, 161 of 229 | `static_collision_real::check_2_the_models_carry_the_prebuilt_mesh_and_its_embedder` |
| every mesh block is embedded by `0x2DD96D6D` via `0xB7C8314A` | same test (embedder map asserted) |
| the mesh-block field hashes occur only in metadata calls | `dev/static_field_scan.py 0x9B2111C5 0x0D50490E 0xCB4D298D` (7 sites, all `mov r8d, <hash>` feeding `0x1421d4f70`/`0x1421d4f30` or a class registration) |
| the dispatch tables, and the known positive | `dev/static_rva_tables.py 0x208edf6` -> HIT `0x142090988` |
| `0x1420a02f6` is selected by no dispatch table | `dev/static_rva_tables.py 0x20a02f6 0x20a075d` -> 0 hits over 277 tables |
| `0x1420a02f6` has no other static reference | `dev/xref_rva.py ... 0x20a02f6` (0 pages), plus the `E8`/`E9`/8-byte counts in `dev/static_rva_tables.py`'s occurrence report |
| the collision list is at `+0x140` in this build | `dev/static_field_scan.py 0xCB4D298D` -> the single registration at `0x14150bf65` with offset `0x140`; the same helper's offsets for `C` match `GeometryCopyParams` |
| `FUN_14210bac0` reads it, and the `0xf0` class does not | `dev/static_slots.py --vt 0x143394fb0` (no `+0x140`/`+0x118` read, no mesh call) |
| the block's stored mesh, bit-exact | `model_mesh_real::the_range_models_prebuilt_mesh_and_its_shape_record` |
| the mesh test fills nothing and returns | check 6: `0x142caf6c0` writes only its stack triangle list (`0x200` bytes at `auStack_258`, `0x80` entries) |
| the query owner is not the Domino world | check 6: `+0x2b0`/`+0x2b8` are `0x18`-byte entries plus a count, and `formats/domino.md`'s `+0x2b8` is a `0xc0`-stride pair pool |
| the character path never calls the mesh chain | check 6: the chain's `E8` sites are all in `0x141fb8xxx`/`0x1420xxxxx`; `RunCharacterControllerSweep`'s nine are mover/AI |
| the character sweep uses the Domino world, not the scene owner | check 7: 0 indirect calls in `0x142cf3f80`'s four chunks; only Domino accessors called; `VariantPosition` reads `[world+0x2a0]` (0x50) / `[world+0x288]` (0x1c0) |
| the sweep's world is reached through the encoded handle table | check 7: `world = *(u64*)(0x1442a0bc0 + slot*8) + K1 ^ K2` in every accessor |
| the client's `STUModel` schema is complete and gap-free | check 8: four installers cover `+0x08`..`+0x1FB` of the `0x200`-byte class; `dev/static_model_schema.py <installer>` |
| the collision is per model asset | `model_schema_real::the_range_models_collision_fields_and_where_they_live` (1 `STUModel` per model, 163 with a list, 279 holders, 0 model-to-model refs) |
| the per-instance builder reads the mesh into a body | check 8: `0x14201af49`/`0x14201b1cd` walk `+0x140`, filter `holder+0x2a`, unpack `0x141b53af0`, call `ShapeAddToBody`/`OwnerCreateBody` |
| the builder has no static reference | check 8: E8/E9/jcc/8-byte/lea/imm/RVA-dispatch all negative except the internal chunk branch |
| **that is a fragment start, and the builder has four callers** | check 9: `.pdata` chain root `0x14201ae60` (six contiguous fragments to `0x14201c887`); `dev/fn_extent.py 0x14201af49` prints the chain; `dev/callers.py 0x14201ae60` -> 4 `E8`, 0 `E9`, 0 `jcc`, 0 8-byte pointer, 3 dword-RVA hits that are all `RUNTIME_FUNCTION` triples |
| the four callers are state-sync functions, none an editor | check 9's caller table; `dev/callers.py 0x142022030 0x1420195e0 0x142014b50 0x142015f70` |
| the component's update and its body sync are function-pointer slots | check 9: `0x142f7d688 -> 0x142019880`, `0x142f7d678 -> 0x142018cc0`, table base `0x142f7d320`, no RTTI locator, no 8-byte pointer to it in the image |
| the filter, the switch, the type bytes and the mesh scale | `model_body::tests::{the_world_mask_is_selected_by_bit_25_of_the_flags_dword, the_holder_filter_is_the_mode_byte_the_mesh_and_the_mask, the_shape_mode_is_the_four_arm_switch, the_descriptor_type_byte_is_the_shape, the_holder_array_is_the_tag_swap, the_mesh_scale_is_uniform_within_the_epsilon, the_mesh_descriptor_scale_is_maxss_or_one}` |
| the range's holders, against that filter | `model_body_real::the_range_models_holder_plan`: 279 holders, 272 mesh, mask all ones on all 279, filter byte and shape mode not carried (default `0`), shape histogram `{Mesh: 272, None: 7}`, 161 models with a shape, model `0x0d0000000000078c` pinned to one mesh shape |
| the two "siblings" are different functions | check 9: `0x1420c63b0` -> fragments to `0x1420c8784`, `0x1420ca550` -> fragments to `0x1420ccdeb`; both index a `0x38`-byte record table and switch on a type code, neither walks `+0x140` |
| the unpack and the shape record, dword for dword | `mesh_shape::tests::the_shape_record_is_mesh_shape_fill_dword_for_dword`, `the_array_header_offset_is_the_tag_test`, `a_zero_scale_is_an_infinity_lane`, `the_field_offsets_are_the_unpackers` |
| the owner id the builder uses is a world slot | check 10: `0x14201af0d` -> `0x14202f800`; `0x14201b560`/`0x14201b571` -> `OwnerCreateBody` with that id, which indexes `0x1442a0bc0` and rejects `>= 0x3fff` |
| the node creates its own body in that slot | check 10: `0x14202d1d1`, `0x14202d1e8` (allocator), `0x14202d268`/`0x14202d26f` (`OwnerCreateBody`), `0x14202d27b` (store at `+0x31fc`) |
| the record exists only for nodes with `+0x454` bit `0x200` | check 10: `0x14202d0e1`; the flags dword is copied from the node creation record earlier in the same function |
| the node gate is `+0x1c` bit `0x20` set and `0x40` clear | check 10: `0x142024fee`/`0x142024ff2`/`0x142024ffb` |
| the component's table is a folded base interface | check 10: `dev/rip_refs.py 0x142f7d320` -> 6 code sites (sizes `0x38`/`0x58`/`0x68`/`0x78`/`0xd8` plus the ctor `0x141c58170`) and 162 stubs (0 `.pdata`) copying it into globals; 0 eight-byte pointers |
| the component's virtuals run 0x20 bytes into the object | check 10: `0x142019916` passes `this - 0x20`; `[this+0x230]`/`[this+0x400]`/`[this+0x470]` correspond to `[object+0x250]`/`[object+0x420]`/`[object+0x490]` in `FUN_142024d30` |
| the world-slot codec round-trips, and the empty slot is a null world | `domino_world::tests::{the_two_addends_are_inverses_modulo_2_pow_64, the_world_pointer_round_trips_bit_exactly, the_empty_slot_is_the_encoding_of_a_null_world}` |
| the body handle packs slot/generation/index and the invalid one is `0xffff_0000_ffff_ffff` | `domino_world::tests::{the_body_handle_is_slot_then_generation_then_index, the_invalid_body_handle_is_ffff_0000_ffff_ffff, the_slot_bound_is_strictly_below_0x3fff, the_body_pool_is_0x1c0_per_index}` |
| the character's sweep argument is `[controller+0x1ad8]` | check 11: `0x140ee8aca` loads it and `0x140ee8ad1` calls `0x142cbcee0`; `dev/disp_scan.py 0x1ad8 --writes` (10 qword stores, 65 operands) |
| that field is created by the mover setup, from `0x142cbcd40` | check 11: `0x140ca6132` stores the call's return; `ConstructMoverBase`'s `0x140ca3b08` only zeroes it |
| the query record names the **node's** world slot | check 11: `0x140ca609b` calls `0x14202f800`, the value is read at `0x140ca60ab` and stored at `0x140ca60b3`; `0x142cf35a0` puts it at record `+0x04` and calls `0x142cc3f50(slot, ...)` |
| the accessor's fallback is the invalid slot | check 11: `0x14202f81c` reads the global `0x1433674e0`, which holds `0xffff` |
| `0x142cc3f50` creates the shape entry in that world | check 11: it bounds the slot at `0x3fff`, tests the sentinel `0xc439_701c_d97e_c6aa` in `0x1442a0bc0`, decodes with the same addends/XOR, and allocates from `world+0x2a0` (stride `0x50`) |
| the component's constructor is `0x14200a9a0` | check 12: its store table (vtables at `+0x00`/`+0x20`/`+0x118`, body handles at `+0x88`/`+0x90`/`+0x9c`, node at `+0x250`, pending node at `+0x490`, masks at `+0x470`/`+0x580`, id array at `+0xaa0`/`+0xaa8`) is exactly the field set the builder and the state-sync functions read |
| the class size is `0xad0`, twice over | check 12: `Alloc` argument in `0x14200a5f0` (`0x14200a60a`) and `RegisterTypeWithSize`'s size in `0x14200bab0` (`0x14200bb36`) |
| the registration is kind `0x14` and the name is `Unknown` | check 12: `0x14200bab0`'s call; the name is the 7 bytes at `0x143aa01cc` XOR the key at `0x143aa01c0` |
| the base interface is `0x142f7d648`, not `0x142f7d320` | check 12: `0x14200a9f5` stores `0x142f7d648` at object `+0x20`; decoded slots give checks 9/10's entries at interface 6, 7, 8 |
| the attach is `0x14200f840` -> primary slot 4 -> `0x14201cf40` | check 12: `0x14200f894` calls the slot; `dev/callers.py 0x14201cf40` -> one 8-byte hit at `0x142f7d628` only |
| `FUN_142017010`'s eight arguments come from runtime tables | check 12: `0x14201cf40` fills them from the entity's type-id lookups; `0x140a29960` fills them from the `0x10`-byte `{id, pointer}` array at `param_1+0x38` |
| the component's own attach tests no asset data | check 12: interface slot 1 `0x14200ee80` takes `[entity+0x250]` and binds |
| the range's props against the gate | **not tested**: the four gate bits are runtime node/entity state, not asset data (check 10's table), and check 12 shows the deciding input is the entity's component set plus the node creation record -- neither read -- so no install-side test can stand in for it |
| which of the range's 550 placeables carry the component | **not tested**: the component's own code decides nothing from asset data; the driver that instantiates it for an entity is unread (check 12's next read 1) |

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
conversion** in that path -- the three arrays are passed by pointer -- so what
remains is the record's element layouts, which live inside `0x142caf6c0`.

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

## What this leaves

| line | state |
|---|---|
| the fracturable brush path on this map | closed: the range names no `C` (check 1) |
| the model's prebuilt collision mesh | **present** in 161 of 229 range models; the data chain (`STUModel` `0xCB4D298D` at `+0x140` -> holder `0x2DD96D6D` -> `0xB7C8314A` -> `STU_537E56E4`) and its **consumer** (`FUN_14210bac0`, reached from the broadphase `FUN_1421088e0`) are read; the block is passed as a shape record, never converted |
| the `0xf0` geometry class | not the consumer: no slot reads the list or calls the mesh path (check 5) |
| the kind-`0x11` chunk's consumer | candidate `0x1420a02f6` unreachable under every reference form; the container's producer is the missing read |

Ordered next reads:

1. **`0x142caf6c0`, the Domino mesh query** -- the only function that walks the
   three arrays the shape record points at. It fixes the node record (`32` B by
   the byte count, `288 / 9`) and the triangle record (`16` B, `384 / 24`). That
   is the last read between the stored model mesh and a collider's geometry.
2. **Whether the model path ingests as well as queries**: no `ShapeAddToBody`,
   `ShapeEntryFromDescriptor` or `OwnerCreateBody` appears in `FUN_14210bac0`, so
   as read it is query-only; the ingest for a *model* would have to be
   `ShapeEntryFromDescriptor` (`0x142cb7f10`), which shares `MeshShapeFill` with
   this path.
3. The producer of an owner's `+0x140`/`+0x148` container is no longer open for
   the model route (the list is filled by the STU loader from `STUModel`'s
   field); it stays open only for the *kind-`0x11` chunk* form.

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
| the unpack and the shape record, dword for dword | `mesh_shape::tests::the_shape_record_is_mesh_shape_fill_dword_for_dword`, `the_array_header_offset_is_the_tag_test`, `a_zero_scale_is_an_infinity_lane`, `the_field_offsets_are_the_unpackers` |

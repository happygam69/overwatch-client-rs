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
prebuilt collision mesh** (`STU_537E56E4`: 161 of 229 models, 272 blocks) and the
data-side chain that reaches it is read, but **no consumer of it was found** in
code; and the kind-`0x11` chunk's candidate consumer `0x1420a02f6` is **not
reachable** under the four-byte-RVA dispatch method either. So the static source
is **not established**, and the next read on each line is named below.

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
  field 0xCB4D298D (declared +0x118)   array of 0x2DD96D6D
    instance of 0x2DD96D6D
      field 0xB7C8314A                 embedded STU_537E56E4
        m_nodes / m_88FCECD7 / m_triangles / m_nodeCount / m_height
```

`STUModel`'s field at `+0x118` is the model's collision list; that is the
"which field/class references the model's collision" answer, read from the
class's own declaration and confirmed by the embedder field of all 272 blocks.

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

**Verdict: the range has a prebuilt collision mesh, but nobody reads it in the
code read so far.** The exact missing read: **the consumer of `STUModel`'s field
`0xCB4D298D`** (the model collision list) -- the function that walks a model's
collision instances and turns them into Domino shapes. It is not the geometry
module (which never touches `+0x118`), so the search starts at the model asset's
own post-load path or at the node types that call `ShapeAddToBody`
(`0x142cb3940`): `probe_node_types.py` already lists node types 4 and 23 as
calling it, and type 4 is `0x14200bab0`, outside the geometry module.

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

## What this leaves

| line | state |
|---|---|
| the fracturable brush path on this map | closed: the range names no `C` (check 1) |
| a prebuilt model collision mesh | **present** in 161 of 229 range models; data chain read (`STUModel` `0xCB4D298D` -> `0x2DD96D6D` -> `0xB7C8314A` -> `STU_537E56E4`); **no consumer read** |
| the kind-`0x11` chunk's consumer | candidate `0x1420a02f6` unreachable under every reference form; the container's producer is the missing read |

Ordered next reads:

1. The consumer of `STUModel`'s `0xCB4D298D` -- the model collision list. If that
   consumer builds Domino shapes, the range's static collision is the models'
   prebuilt meshes and the kind-`0x11` chunk is not needed for it.
2. The producer of the owner's `+0x140`/`+0x148` container, from the container's
   own initialisers (not from `0x1420a02f6`).
3. The kind-`0x11` sector runs themselves: their shape is measured but no reader
   of the 36-sector layout has been found, so the chunk is neither the source nor
   excluded.

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

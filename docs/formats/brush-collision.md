# The brush: the collision route built from geometry

The client's character controller sweeps a Domino world whose shapes come from
**two** producers. One is the map's chunk route (the sibling read;
`formats/maps.md`) — loaded cells with no shape-building reader found yet, so it
is open, not excluded. The other is the one this file is about: a scene node of
type 2 owns a *model*, and the model's collision is a **brush** — a closed ring
of quantised 2-D points that the client meshes into triangles and thickens in Z
by the collision radius. That the brush feeds the sweep is read; that it is the
*only* producer is not.

This file records that route: what the node resolves to, the brush's record
formats, the two pure conversions, and exactly how far the read reaches. Every
address is a current-image address (`0x140000000` + RVA).

## Status in one line

The brush's **record formats**, its **vertex decode** and its **ring builder**
are read and implemented (`ow_tank::brush`, eleven units; every arithmetic
result in them is pinned to the client's own bytes). The **tree → mesh** walk is read only up to its per-node op
classification; the op case bodies and the triangle emission are **open**. No
install asset carrying a brush was found — measured, see the last section.

## The node, and what it resolves to

`GeometryBuild` (`0x14213c440`) is the node-type-2 build. Its chain from the
geometry object to the collision data is:

```text
geometry+0x80            the scene node
  [node+0x38]            the node's object
    [+0x40]              the object's model instance
      [+0x48]            C, the model's collision descriptor
```

Three of `C`'s fields are used before the brush build: `C+0xf0` is copied to
`geometry+0x90`; `[geometry+0x88]+0x250` goes through `0x14202f800`, whose
first `u16` becomes the geometry's shape id at `geometry+0x9c` (`-1` rejects the
build); and `0x141b5cfb0(C, out, geometry+0xaa)` resolves a reference block at
`C+0x24`/`C+0x28` and yields the `u16` stamped at `geometry+0xa8`. `C` also
carries the collision parameters `GeometryCopyParams` (`0x142132cf0`) copies
(`C+0xc0` a quaternion, floats at `+0x110`..`+0x16c` with the clamps recorded in
that function, two descriptor hashes at `+0x178`/`+0x186` via `0x141b53c40`, and
the two bytes at `+0x194`/`+0x195`), and the **instance points** the brush ring
is built from.

**The instance array's field encoding is open.** `GeometryBuild` reads two
qwords at `C+0x98`/`C+0xa0`: when the second is exactly `1` it takes the first
as a point array and the second as the point count, and otherwise it takes the
first as the count and the second as the array. That is the same
inline-singleton-versus-pointer shape the loader's array fields use, but which
of the two layouts `C` actually holds, and what `C`'s class is, is **not read**.
It stays open (see the last section), so the point list is not decoded from the
install here.

## The brush records (read)

The brush object is `0x20` bytes; the field table and its two record shapes are
pinned by the one writer (`FUN_142ced620`, `0x142ced620`) and by three
independent readers — the vertex decoder `FUN_142ced510` (`0x142ced510`), the
ring walk in `FUN_142cf0360` (`0x142cf0360`), and the edge-list builder
`FUN_142ced1a0` (`0x142ced1a0`). The ingest `AppendCollisionEntryFromBrushSource`
(`0x14213a510`) copies exactly these arrays: `count << 5` bytes of vertices and
`count * 0x28` bytes of nodes.

| brush field | meaning |
|---|---|
| `+0x00` | vertex array |
| `+0x08` | node array |
| `+0x10` | u32 vertex count |
| `+0x14` | u32 node count |
| `+0x18` | u32 root node (the first node of the first ring) |

**Vertex, `0x20` bytes.** `+0x00` and `+0x08` are signed 64-bit `x` and `y`,
`+0x10` is a signed 64-bit `divisor`, `+0x18` is a u32 ordinal. The world
position is `(x / divisor) * 0.0025` — that is `1/400` units, which is why the
builder multiplies by `400.0` (`0x1435ed130` holds `400.0` in all four lanes).
`FUN_142ced510` computes the quotient as *integer quotient plus scaled
remainder* in 32-bit floats, and writes `z = w = 0`: the brush is a flat XY
outline and the thickness arrives later. A zero divisor is the client's only
failure mode here (a signed integer divide fault), so `Vertex::position` answers
`None` there rather than inventing a value.

**Node, `0x28` bytes.**

| field | meaning |
|---|---|
| `+0x00` `prev` | previous node in the ring |
| `+0x04` `next` | next node in the ring |
| `+0x08` `vertex` | index into the vertex array |
| `+0x0c` | always `-1` (part of the `-1` qword the builder writes) |
| `+0x10` `next_ring` | first node of the next ring, `-1` at the end |
| `+0x14`/`+0x18` | this node's quantised `x`, `y` |
| `+0x1c`/`+0x20` | the *following* node's `x`, `y`, copied before the ordering pass |
| `+0x24` `leaf` | written `1`; the ingest's area test treats a non-zero value as "append this brush unconditionally" |
| `+0x25` `descending` | the ordering pass's direction byte |
| `+0x26` | uninitialised |

`FUN_142cf0360` walks the ring through `+0x04` until it returns to the root and
then follows `+0x10` to the next ring, so a brush is a list of rings and this
implementation walks them the same way (`Brush::rings`).

## The ring builder (read, verified)

`FUN_142ced620` turns a node's array of 4-float instance points plus two scales
into a brush. Only the first two lanes of an instance reach the ring: the scale
vector is multiplied against `{1.0, 1.0, 0.0, 0.0}` (`0x1431102d0`), so `z` and
`w` are zeroed before the quantisation. The steps, in the client's order:

1. each instance's `(x, y)` is quantised: `roundf((v * scale) * 400.0)` in
   32-bit — two separate multiplies, so the intermediate rounds twice — then
   truncated to `i32` (`CVTTSS2SI`, which answers the instruction's "integer
   indefinite" instead of saturating on overflow);
2. the vertices are written with divisor `1` and ordinal `i`;
3. the nodes form a ring: `prev = (n + i - 1) % n`, `next = (n + i + 1) % n`,
   `vertex = i`, `+0x0c` and `+0x10` set to `-1`, `leaf` set to `1`;
4. each node's `(x, y)` is copied into the **previous** node's `+0x1c`, so the
   record holds an edge's two endpoints;
5. one pass in index order compares a node's own pair with the copied pair —
   `y` descending first, then `x` **ascending** — and either leaves it
   (`descending = 1`), swaps the two pairs (`descending = 0`), or, when both
   fields are equal, frees the arrays, zeroes the counts and answers zero. That
   equal case is the builder's rejection: two consecutive points coincide.

`build_from_instances` is the read expression of steps 1–5. It is **verified
against the client's own bytes**: `dev/brush_vertex_oracle.py` runs
`0x142ced510` and `0x142ced620` under the emulator and its output for a square,
the reversed square, a triangle, a degenerate triple, a non-uniformly scaled
square and two half-way rounding cases is what the unit tests pin, field for
field. The builder allocates through the image's allocator hook, so the probe
installs a bump-allocator stub at that hook word (`0x1442a0158`) and declares
the region it hands out; that is why the differential is run here rather than in
a `dev/verify_harness` unit (see "What is open").

## The tree → mesh walk (partly read)

`BrushTreeToMesh` (`0x142cf23c0`) is fed the `0x28`-byte snapshot record the
ingest builds (`{vertices, nodes, u32 vertex count, u32 node count, u32 root,
u32 pad, u32 index}`) and writes a `0x20`-byte mesh: `{vertices, u32 vertex
count, triangles, u32 triangle count}` with 16-byte vertices and 24-byte
triangles. Its first stage is `FUN_142cf0360`, read as far as:

- the mesh's counts are `n` vertices (16 bytes each) and `n - 2 + 2 * rings`
  triangles (24 bytes each), where `rings` is what the walk from the root
  through `+0x10` reaches: it is zero for a single ring, so one ring gives the
  plain polygon triangulation `n - 2`;
- each ring node becomes a `0x30`-byte workspace vertex (the brush vertex's
  first `0x18` bytes, a pointer to its `0x60`-byte hull, the op at `+0x28`) and
  a `0x60`-byte hull (prev/next face links at `+0x28`/`+0x30`, the vertex
  pointer at `+0x38`, a closure pointer at `+0x40`, and the edge's two
  quantised endpoint pairs at `+0x48`);
- each node is classified into one of **six ops** by its own `descending` byte
  and its predecessor's: both clear → op 3; this one set and the predecessor
  clear → op 4 or 0; this one clear and the predecessor set → op 5 or 1, the
  choice between the pair being the sign of an integer cross product of the two
  nodes' endpoint pairs (exact expressions in the function; they are read, not
  guessed).

**Open from here:** the six op case bodies in `BrushTreeToMesh` (which mutate
the hull list through `FUN_142cf07d0`, `FUN_142cf0cc0`, `FUN_142cf01b0`,
`FUN_142cefd70`) and the final emission (`FUN_142cef7f0` / `FUN_142ceeee0` then
`FUN_142cf0ae0`), which is where the winding and the triangle indices are
decided. Until those are read, **no brush-to-mesh conversion is implemented** —
the six ops are recorded as a classification only. `crates/ow-tank/src/brush.rs`
does not convert a brush to a mesh and its module doc says so.

## The face conversion (read; already implemented elsewhere)

`MeshToColliderFaces` (`0x142cf2010`) is read end to end and is the second half
of the route: it takes a mesh, a radius and a surface id, writes the `0x30`-byte
mesh source at the cell entry's `+0x20`, and produces

- **`2n` vertices**: the source vertex with `z + radius` and the same vertex
  with `z - radius`. `x`, `y` and `w` pass through a signed-zero add, so only
  `-0.0` in the first block is affected — the thickening is Z only;
- **`f` outward plus `f` inward `0x14`-byte index records** (`{i0, i1, i2,
  zero, u16 surface id}` at `+0x00`/`+0x04`/`+0x08`/`+0x0c`/`+0x10`), the inward
  block being `(i2 + n, i1 + n, i0 + n)`;
- **two stitch triangles per open edge**, only when the radius is at least
  `0.005`: a face's three edge marks are at `+0x04`, `+0x0c`, `+0x14` of its
  `0x18`-byte source record, `-1` means open, and pair `k` stitches
  `(i0, i1)`, `(i1, i2)`, `(i2, i0)` as `(a, a + n, b + n)` and
  `(b + n, b, a)`.

This is the same expression `crates/ow-sim/src/collider.rs::shell` already
implements, so it is **not duplicated** here; `ow-tank`'s brush module stops at
the brush records. Its differential unit is still open for the reason recorded
in `docs/open-questions.md` item 6: it allocates its two arrays internally.

## The other route into the same ingest

`AppendCollisionEntryFromBrushSource` (`0x14213a510`) takes an *already built*
brush out of the geometry object's source array (`geometry+0xb0`, stride
`0xb0`, count at `+0xc0`) instead of building one from a node's instances. Its
driver (group start `0x14213bbc0`; the call site is `0x14213bd61`) requires
of each source:

- `source+0xa0 == 0`, `source+0x58 != 1` before the load and set to `1` after,
  and `source+0x59 != 0`, which it then clears;
- the asset named by the GUID at **`source+0x70`** resolves (`0x141fd3a90`)
  and its **GUID type is `0x0a`**: `0x141fd3b20` is the type extractor — the
  12-bit reversal of the type field plus one, the same numbering `ow-tank`'s
  `guid::type_of` uses — and `0x14213bc8f` compares its answer against `0x0a`;
- the loaded object's class check (`0x1421ce150` against the table at
  `0x143f505c0`) passes.

So on this route the brush belongs to an **asset of GUID type `0x00A`**, and
that is the only install-side name for a brush this session found.

## The install side: measured, and what is open

The question "which install asset carries the practice range's brush" is **not
answered**, and the measurement that says so is pinned by
`crates/ow-tank/tests/brush_real.rs`:

| measurement | result |
|---|---|
| records in the content manifests this install opens (`Win_SPWin_RDEV_EExt.cmf` plus the enUS text manifest) | 1,425,794 over 177 distinct types |
| records of GUID type `0x00A` (`0x141fd3b20`'s numbering) | **0**; the neighbouring types `0x00C` (32,280) and `0x00D` (34,027) are populated, so the zero is about the type, not the registry |
| the same census with the `WinPrism_` content manifest forced | 1,514,352 records, still **0** of type `0x00A` |
| chunk tags across 300 type-`0x00C` (model) assets | `MDLC`, `MRNM`, `MSTU`, `MESD`, `mskl`, `CLTH` — no brush container |

**What is open, in order of what would unblock it:**

1. **`C`'s class and its `+0x98` array encoding.** Reading which class the
   `[[[node+0x38]+0x40]+0x48]` object is, and whether `+0x98`/`+0xa0` is the
   inline-singleton or the counted-array form, is what lets the instance points
   be decoded from the install at all. That is the next anchor, and it is the
   one that would turn this file's format work into real geometry.
2. **Type-`0x00A` assets are not in the manifests.** They may be synthesised
   from a parent GUID the way texture payloads are (`guid::payload_of`), or
   held in the CASC index without a manifest record. Neither is read.
3. **The six brush ops and the emission in `BrushTreeToMesh`.** Listed above.
4. **A differential harness unit.** Both brush functions allocate through the
   image's allocator hook, and the cargo tier needs an entry in
   `crates/ow-tank/src/bin/verify_probe.rs`, which this session did not own. The
   differential was therefore run by `dev/brush_vertex_oracle.py` and its exact
   outputs are pinned in the unit tests. To make it a unit, add the probe entry
   `brush_vertex_decode` taking `(out ptr 16, vertex ptr 32)` and answering
   nothing, plus a `dev/verify_harness/brush_vertex_fixture.py` that builds
   `0x20`-byte vertex records; the oracle side is already written (the probe
   installs and documents the allocator stub the *builder* needs, which that
   unit does not).

## Checks

| claim | check |
|---|---|
| the record sizes (`0x20`, `0x28`) | `brush::tests::the_header_is_the_size_the_builder_writes`, and the install test's byte counts |
| the vertex decode, bit for bit | `brush::tests::the_vertex_decoder_matches_the_client` (eleven records, the client's own output bits) |
| the quantiser is `roundf` and not a truncation | `brush::tests::the_quantiser_rounds_half_away_from_zero` |
| the ring, both windings, and the swapped nodes | `brush::tests::the_square_ring_matches_the_client`, `the_other_windings_match_the_client` |
| the rejection case | `brush::tests::a_repeated_point_rejects_the_brush` |
| the two scales are separate lanes | `brush::tests::the_two_scales_are_separate` |
| the reader's guards | `brush::tests::the_records_round_trip_and_are_guarded` |
| no type-`0x00A` asset in this install | `tests/brush_real.rs::no_asset_of_the_brush_source_type_exists_in_this_install` |
| no brush chunk in a model | `tests/brush_real.rs::a_model_carries_no_brush_container_chunk` |

# The shape narrowphase wrapper and its type dispatch

`ShapeNarrowphase` `0x142cc1610` is the leaf that turns one `0xc0`-byte shape
entry into a hit record. It is the third of the four query links: the BVH walk
`0x142cbd510` produces shape indices, this function transforms and filters each
one and runs its per-type primitive, and the hit policy above it decides what
to do with the result. Evidence class: **read**, and the leaf is **verified**
against the client's bytes as `fn/ShapeNarrowphase`.

## The call shape

The leaf is `void`: it returns nothing and writes exactly one thing the caller
can see, a single 16-byte quad. That quad is the hit record's `+0x20` field, so
a caller that only knows "did it hit, and where" reads one quad and ignores the
rest.

Everything else it reaches through pointers, and the first of those is the
context it is handed as its first argument:

| context offset | meaning |
|---|---|
| `+0x00` | the world record |
| `+0x08` | **the filter's first eight bytes, inline** — the leaf is handed `context + 8` itself, not a pointer to the words |
| `+0x10` | the hit callback |
| `+0x18` | the callback's third argument |
| `+0x20` | the sink: when non-null, every outcome is pushed into it |

The inline filter is worth stating plainly because it is the one place the
wrapper's own frame is handed out as data: `ShapeQueryFilter` `0x142cbb350`
receives `context + 8` as its first argument and the entry's `+0x98` as its
second, so the query's layer words live at `context+0x08..0x10` and the shape's
at `entry+0x98..0xa0`.

The shape entry is `record[0x2b8] + shape * 0xc0`, where the shape index is the
leaf's fourth argument (a signed dword in `r9d`).

## What it does, in order

1. **Transform.** `ShapeWorldTransform` `0x142cb8b80` is called with the entry
   and a `0x20`-byte output on the wrapper's own frame: position at `+0x00`,
   quaternion at `+0x10`. That layout is exactly the `xform` the per-type
   primitives take, which is why the transform and the sweeps meet without an
   adapter.
2. **Filter.** `ShapeQueryFilter` `0x142cbb350` with `(context+8, entry+0x98)`.
   A rejection does not mean "no hit" — it is a third outcome with its own sink
   status (see below).
3. **Dispatch.** A call to the code label `0x142cb8d60`, which lives inside
   `ShapeWorldTransform`'s extent and was therefore never split into its own
   function. It is a real entry point: the wrapper calls it with five
   arguments, the fifth (the query block) written to its own `[rsp+0x20]` slot.
   The label reads `entry[0x20]` as a **signed byte** and tail-jumps to the
   per-type primitive with `rcx = element`, `rdx = hit record`, `r8 = block`,
   `r9 = xform`.

The dispatch's table, with every pool base taken from the world record and
every element indexed by the entry's `+0x24` (a signed dword):

| `entry[0x20]` | pool | element stride | primitive |
|---|---|---|---|
| `0` | `record+0x2d0` | `0x20` | `0x142ccdf40` sphere (`fn/CollisionSphere`) |
| `1` | `record+0x2e8` | `0x30` | `0x142ccd7b0` capsule (`fn/CollisionCapsule`) |
| `2` | `record+0x300` | `0x50` | `0x142cd3c80` — unread |
| `3` | `record+0x318` | `0x50` | `0x142caed10` `MeshSweepEntry`, which forwards to `0x142caed30` (`fn/MeshSweep`) |
| any other value (4 and up, and negative) | `record+0x330` | `0x50` | `0x142cd0ae0` — unread |

The type byte is compared as `0, 1, 2, 3` in that order and the last arm
catches everything else, so a negative type and type `4` share the
`record+0x330` pool.

`MeshSweepEntry` `0x142caed10` is 26 bytes and does nothing but forward, so the
type-3 pool element is the record the mesh walk expects. That element is
`0x50` bytes and there is **no single "mesh world" pointer** in it:

| element offset | meaning |
|---|---|
| `+0x00` | the scale, four lanes |
| `+0x10` | `1/scale`, four lanes |
| `+0x20` | the BVH root node |
| `+0x28` | the vertex array |
| `+0x30` | the triangle array |

The three pointers are **absolute addresses**, so the world the walk runs in
is simply the address space they live in. Composing the type-3 arm therefore
needs no extra field: `mesh_sweep` is handed the element and a world whose base
is the arena and whose bytes are the blob, and every pointer resolves. The
`+0x48` that an earlier note guessed at is inside the element's unused tail.

One more field the mesh arm depends on is in the **query block**, not the
element: `block+0x70` is a `u16` mask that every triangle's own mask at its
`+0x1c` is tested against. A zero block mask makes every triangle unusable, so
a fixture that leaves the block's tail zero sees a walk that never hits.

## The hit record

The record is `0x50` bytes on the wrapper's frame, initialised before the
primitive runs:

| offset | value |
|---|---|
| `+0x00`, `+0x10`, `+0x20` | three zero quads (the constant at `0x142f84a00`) |
| `+0x30` | `-1` (the triangle index; the mesh walk overwrites it) |
| `+0x34` | `0` |
| `+0x38` | `0xffff` |
| `+0x3c` | `0` (a qword) |

The primitives fill `+0x00` (the point), `+0x10` (the normal) and `+0x20` (the
fraction, broadcast across the quad); the mesh walk also fills `+0x30` with the
triangle index and `+0x34` with the triangle's two id words. After the
primitive returns a hit, the wrapper applies two overwrites of its own, from
the **entry**:

- `+0x34..0x38` (two words) from `entry[0xaa..0xae]`, **skipped when the type
  byte is 3 or 4** — those two types carry their own ids from the primitive;
- `+0x38..0x3a` from `entry[0xae..0xb0]`, unconditionally.

So for a sphere or capsule the ids in the record are the entry's, not the
element's.

## What it publishes

On a hit, in this order:

1. The callback. `entry[0xa0]` (a dword), `entry[0xa8]` (a word) and
   `record[0x194]` (a word) are packed into one eight-byte value, and
   `context[0x10]` is called as `(hit record, that value, context[0x18])`.
   The callback's return value becomes both the function's return value and
   the sink record's status word.
2. The quad. `*out = record[+0x20]`.
3. The sink, if `context[0x20]` is non-null: a sink record is assembled and
   pushed through `0x142cc1450` at `sink+0x80`.

The sink's status word is `1` for a hit, `2` when the filter passed and the
primitive found nothing, and `0` when the filter rejected the shape. So the
wrapper distinguishes "filtered" from "missed", which is why the rejection path
also builds and pushes a record (with a zeroed hit payload) instead of
returning early.

## Types 2 and 4+, read (2026-11-07)

Both primitives are now read from the instruction stream; neither is implemented
and neither has a differential unit, so both remain unverified. The working
note with every constant is `dev/data/notes/narrowphase_types_2_4.md`.

**Extent caveat first.** `dev/fn_extent.py` reports only the `.pdata` entry that
covers the entry address, and for these three functions that entry is much
shorter than the body. Type 2 `0x142cd3c80` has a `0x64`-byte root entry while
its chained cluster (5 entries) runs to `0x142cd4151`. The type-4 leaf
`0x142cd0540` has a `0xc4`-byte root with a 3-entry cluster to `0x142cd0818`,
and the read went past that to `0x142cd0ad4` on control flow - that wider bound
is not pinned by a chain walk and should be treated as a reading aid. Type 4+'s
owner `0x142cd0ae0` is a single complete `0xc00`-byte entry, the one case here
where the root is the body.

### Type 2 — a convex hull handed over as face planes

Pool `record+0x300`, stride `0x50`, filled by `ShapeFillType2` `0x142cd31f0`.
The primitive is `0x142cd3c80`. Element:

```text
+0x00 quad   world centre; written by the filler, never read by the primitive
+0x10 ptr    count0 world points, stride 0x10
+0x18 ptr    count1 planes (n.xyz, d), stride 0x10
+0x20 ptr    count1 bytes, copied, no reader here
+0x28 ptr    count2 dwords, copied, no reader here
+0x30 i32    count0      +0x34 i32 count1     +0x38 i32 count2
+0x3c f32    scale^3 * src[0x38]  (volume)
+0x40 f32    scale^2 * src[0x3c]  (area)
+0x44 f32    powf(0.238732398 * volume, 1/3) - the equal-volume sphere radius
+0x4c i32    pool free-chain link
```

The destructor `0x142cd35d0` confirms the four allocation sizes
(`count0*16`, `count1*16`, `count1`, `count2*4`).

1. **Gate:** all four lanes of `block+0x50` (expansion) at or below `0.005`
   **and** `dword[block+0x38] == 0` - the same gate and constant the type-3
   triangle test uses. Anything else goes to the GJK path.
2. **Analytic path.** `S = R(start - pos)`, `E = R(end - pos)`, `D = E - S`:
   note the two rotations are subtracted, not a rotation of the difference. The
   walk keeps `tmax = 0`, `tmin = block+0x20` (the motion quad, four
   independent lanes) and `best = -1` over the `count1` planes: per plane
   `A = dot3(n, S)`, `B = dot3(n, D)`, `dist = d - A`, `q = |B|^2`, with the
   `q` guard `0x143110818` (`1.1754944e-35`, an effective `|B|` threshold of
   `sqrt(q/3)`). Entering (`B < 0` in every lane) raises `tmax` to `dist/B`
   and records the plane; exiting (`B > 0`) lowers `tmin`; `B = 0` with
   `dist < 0` rejects immediately; any mixed lane pattern does nothing. After
   **every** plane, including the first, `tmin < tmax` failing on any lane
   rejects.
3. **Hit:** the record's fraction is `tmax`, the point is
   `start + (end - start) * tmax`, and the normal is the winning plane's
   `xyz` rotated to world through the client's **mixed** quaternion order
   (`cross = rot3(v)*q - v*rot3(q)`, `spin = qw*v`,
   `s = spin + rot3(cross)`, then a second pass and `2*rot3(cross2) + v`) -
   the same asymmetry the point rotation does not have.
4. **Inside path** when no plane was entered: `block+0x72 == 0` rejects,
   otherwise the point is `block+0x00`, the normal is `normalize(start - end)`
   (sign and all) and the fraction is `0`.
5. **GJK path:** `0x142cc9460(out, desc)` with a descriptor built exactly as
   the type-3 triangle test builds it, only the shape-A element differs;
   shape B is the motion shape, `+0x80` the travel vector, `+0x90` the motion
   quad, `+0xa0 = block+0x72`, `+0xa1 = 0`.

Return is `al` only: `1` hit, `0` miss with the record **untouched** and no
callback (the wrapper makes that call). Callers test `al`.

### Type 4+ — one kind, a height-field grid

Pool `record+0x330`, stride `0x50`, filled by `ShapeFillType4` `0x142ccf330`
(at `0x142cb809b`, from `[descriptor+0x68]` and `word[descriptor+0x60]`). There
is **no family dispatch** in this branch: the only discriminator is each cell's
tag byte, and no sub-record offset varies. Owner `0x142cd0ae0`, per-cell leaf
`0x142cd0540`.

```text
+0x00 i32  count0 (grid points, axis 0)   +0x04 i32 count1 (axis 1)
+0x08 f32  cell size                     +0x0c i32 2*(count0-1)*(count1-1)
+0x10 quad local AABB min                +0x20 quad local AABB max
+0x30 u16  default surface id           +0x38 ptr grid, count0*count1 * 8 B
+0x40 ptr  optional per-cell callback    +0x4c i32 free-chain link
```

A grid entry is 8 bytes: `0..32` corner height, `32..48` surface id,
`48..56` callback argument, `56..64` tag - tags `0..3` solid (they pick the
diagonal), `4` empty. Cells are `(count0-1) x (count1-1)`.

1. The support hull is inlined (`q = xform.quat`, `b = block+0x40`,
   `v = R(start - pos)`); its points go to a stack buffer of `0x40` quads with
   a heap fallback.
2. `expansion = block+0x50`, raised by `0.02` when every lane is positive;
   `centre = (hi + lo) * 0.5`, `step = ((hi - lo) * 0.5 + expansion) + 0.02`,
   and the points are recentred in place.
3. Each axis is widened by `step` along the travel direction, and the cell
   index is `floor(v / cell)` with the compiler's floor idiom.
4. A rectangle scan over both index ranges calls the leaf, then a 2D DDA
   (`dt = cell / |span|`, first crossing `((idx+1)*cell - start) / |span|`
   ascending and `(start - idx*cell) / |span|` descending, direction `±1`)
   stops once both parameters pass `last_fraction + 0.01` or an index hits its
   limit.
5. Each cell gets a `0x78`-byte copy of the query block: `+0x00` centre,
   `+0x10` end, `+0x20` the constant `(1,1,1,1)`, `+0x30` the recentred
   points, `+0x38` their count, `+0x40` `(0,0,0,1)`, `+0x50` the expansion,
   `+0x60` the caller's `+0x60`, `+0x70` `0xffff`, `+0x72` the caller's byte.
6. Per cell: a bounds rejection, the empty tag, the four corner heights, an
   AABB rejection against `box ± step`, a separate-axis rejection in lanes
   0..2, then the diagonal from the tag and parity and **two**
   `0x142cce430` triangle tests with ids `2*((count1-1)*i + j) + {0,1}`. After
   the first hit the copied block's motion quad becomes that fraction.
7. The answer rotates the local point and normal to world through the same
   mixed order, publishes the fraction, the triangle id, the surface id from
   the entry's bits, and the callback's return word (or the element's default
   `+0x30`), with `0xffff` in the `+0x38` word and the tested/hit counters at
   `+0x3c`/`+0x40`. **The last hit in scan order wins.**

Return is again `al` only, and a miss leaves the record untouched.

**What stays open on both.** The descriptor's source fields
(`src+0x10`, `src+0x18`, the counts) are not tied to a named class; the ingest
uses the descriptor's transform while the primitive uses the entry's, and the
two agree only while the entry pose is identity. Type 4's DDA parameter and
increment formulas need a lane-by-lane re-derivation before implementation
even though the index arithmetic is settled, and no producer of a `+0x330`
descriptor was traced. Neither primitive has a unit under `dev/verify_harness/`,
so `Narrowphase::UnreadType` stays the honest answer for now.

## What is verified, and what is not

`ow_sim::collide::shape_narrowphase` follows the above operation for operation
and is verified as `fn/ShapeNarrowphase`: 264 cases, every channel, branch
coverage 100% over the wrapper's own `0x204` bytes (3/3 reachable of 7 sites,
with 4 single-edge sites declared), and 11 of 12 mutants killed. The corpus
covers all three primitives that are implemented — sphere, capsule and the
type-3 mesh walk — so the practice range's own type is exercised through the
wrapper.

The unit drives the wrapper directly, with a world assembled from one arena
blob and the sink left null, so:

- **Not verified: the sink push** `0x142cc1450` and the sink record's layout.
  It is the reason all four declared dead edges are dead.
- **Not verified: the hit callback's return path**, beyond the fact that it is
  called with those three arguments in that order. The unit uses a `ret` byte
  in the arena as the callback.
- **Not verified: types 2 and 4+.** Both primitives are read (above) but not
  implemented, so the Rust reports `Narrowphase::UnreadType` for them rather
  than guessing. Type 4 is the remaining declared dead edge at `0x142cc1700`.

## The lesson the fixture taught

Two of the harness's own conventions are easy to get wrong and both cost a
round here: the emulator stops when the program counter reaches its page of
`ret`, so a fixture cannot use that page as a callback; and a leaf that has a
frame cookie reads `0x143ac5f40`, which has to be declared as an allowed image
read. A fixture whose world is one arena blob must also place the blob at arena
offset `0` if the candidate resolves pointers by subtracting the arena base.

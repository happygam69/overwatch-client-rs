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
- **Not verified: types 2 and 4+.** Both primitives are unread, so the Rust
  reports `Narrowphase::UnreadType` for them rather than guessing. Type 4 is
the remaining declared dead edge at `0x142cc1700`.

## The lesson the fixture taught

Two of the harness's own conventions are easy to get wrong and both cost a
round here: the emulator stops when the program counter reaches its page of
`ret`, so a fixture cannot use that page as a callback; and a leaf that has a
frame cookie reads `0x143ac5f40`, which has to be declared as an allowed image
read. A fixture whose world is one arena blob must also place the blob at arena
offset `0` if the candidate resolves pointers by subtracting the arena base.

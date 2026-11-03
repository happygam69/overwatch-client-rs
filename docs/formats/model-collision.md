# The model's collision descriptor `C`

`formats/brush-collision.md` follows the node-type-2 route from the scene node to
the brush and stops at the descriptor `C` -- the object whose instance points the
brush ring is built from. `formats/brush-mesh.md` recorded `C`'s **instance
encoding** but left `C`'s **class** open, because a displacement census cannot
name a type. This file closes the class **by type identity**: every registered
class in the image is enumerable from its registration call, and `C` is the one
whose size, constructor and field-hash dispatch agree with the offsets the
client's own code reads.

Every address is a current-image address (`0x140000000` + RVA). Evidence class is
marked per claim: **decompiled** = read from the Ghidra program `owmod_clean`,
`Overwatch.exe`; **assembly** = read from the instruction stream where the
decompiler mis-renders; **registration** = read from the client's own class
registration; **reference** = the OWLib checkout's STU declarations, used as a
*check* on an offset the client's code already fixed.

## Status in one line

`C` is registered class **`0x564B1018`, size `0x198`**, vtable `0x1431dd118`;
its instance-point array is the `Vec3A[]` field `0x88FCECD7` of the inline
sub-object at `C+0x90`, read through a 16-byte slot at `C+0x98` plus a tag at
`C+0xa0`. That read expression is implemented in
`ow_tank::model_collision::ArrayRecord` with unit tests. **Two things stay
open**: the class id of the object that *owns* the `+0x48` pointer to `C` (the
producer of `[node+0xe8]`, still unread -- see "What is open"), and a real brush
instance from the install (no asset carrying `C` has been located yet, which is a
data question, not a code one).

## The chain, re-read

`GeometryBuild` (`0x14213c440`, slot `+0x18` of the geometry class's vtable
`0x1433950b8`) loads, in this order (decompiled + assembly):

```text
geometry+0x80                 the scene node
  [node+0x38]                 the node's loaded asset object
    [+0x40]                   the object's STU instance
      [+0x48]                 C   (loaded instance of a referenced asset)
```

Nothing else is loaded on the way: `[+0x48]` is a plain qword load and is tested
for zero, so `C` is reached as a **resolved asset reference**, not as a vtable
dispatch and not as an inline sub-object. `C+0xf0` then becomes `geometry+0x90`,
`[geometry+0x88]+0x250` goes through `0x14202f800` for the shape id at
`geometry+0x9c` (`-1` rejects the build), and `C`'s collision parameters are
copied by `GeometryCopyParams` (`0x142132cf0`).

The `0x240`-byte geometry class's constructor `0x142135c10` (the `Alloc(0x240)`
branch of `0x1420d0ef0`) installs a base vtable `0x143394ed0` at `+0x00`,
initialises the sub-object at `+0x78`, and only then overwrites `+0x00` with
**`0x1433950b8`**, whose slot `+0x08` is the node setter `0x142138ce0` and whose
slot `+0x18` is `GeometryBuild`. (The same base vtable is all the `0xf0`-byte
branch `0x142132f20` installs; it is the geometry *base*, not the type-2
geometry.)

The setter `0x142138ce0` writes `geometry+0x78` from its third argument and
`geometry+0x80` from the node, so `geometry+0x80` is the node -- the first link
of the chain above is confirmed from the store side. **Partly read:** the same
setter also writes `geometry+0x88`, and the value comes from a hash probe over
the qword at `node+0x08` that calls the GUID resolver `0x141fd3a90` on a miss.
The probe's body (`0x142138e27`..`0x142138e89`) does not decode linearly, so what
`geometry+0x88` points at is unread; `GeometryBuild` only uses `[+0x250]` of it
and this file draws no conclusion from it.

## Where the update is reached (the earlier "no caller" corrected)

The node-type-2 update is **one function at `0x14204cb40`**; `0x14204cbce` is its
second half, not a separate entry. The entry tests `arg2`, builds a `0x58`-byte
frame and reads `[node+0xe8]`, the pending-instance slot: with the slot **empty**
it clears bit 0 of the dword at **`node+0x70`** and returns, and only with a slot
value does it fall into the rest. The `SETZ` at the top of the second half
consumes the flags of the compare `dword [0x14414e49c] == 2` at `0x14204cbc4`,
which is why that half cannot be read standing alone -- and why the earlier "no
caller" result was an artefact of asking about `0x14204cbce` instead of
`0x14204cb40`.

Read from `0x14204cb40`, the update has **exactly one static caller**: an `E8` at
`0x14204831e`, inside the function `0x14204824a`..`0x1420483f6` (assembly). That
function is the node's geometry dispatch:

- it receives the *base* object in `RCX` and calls the update with **`this -
  0x20`** as the node, i.e. the node object begins `0x20` bytes before the
  sub-object the dispatcher is handed;
- it chooses between the type-2 update `0x14204cb40` and `0x14204bbc0` on **bit 0
  of the dword at `this+0x50`** -- `node+0x70`, the very bit the update clears
  when `[node+0xe8]` is empty -- and after the type-2 call it clears that bit and
  calls `0x1405cc1d0(arg2, 1)`;
- `arg2` is passed straight through to the update, which is where the update's
  `0x1405cc1d0(arg2, 2)` and `[arg2+0x250]` reads come from.

The dispatcher itself has **no** static reference -- no `E8`, no `E9`, no 8-byte
pointer, and no branch into it from the function before it -- so the scene tick
holds it as a runtime function pointer. That, and the store into `[node+0xe8]`,
are the two reads that would close `C`'s owner.

### The 4-byte-RVA test the census owed

`0x204cbce` does occur as a 4-byte value six times, and none of them is a
dispatch table: four are entries of the `.rdata` function/unwind block at
`0x14377a914`, `0x14377a930`, `0x14377a948` and `0x14377a958` (where the value is
a function's **end** address, the entry beginning at `0x14204cb40`), and two are
the `.pdata` copy at `0x14469698c` and `0x144696994`. `0x141b251d0`'s 229 `E8`
call sites likewise say nothing about identity; they say it is a generic release.

## Step 1 -- the release path: what class family the released objects are in

The node-type-2 update (`0x14204cb40`) clears the two slots
`[node+0x38]`/`[node+0x40]` through `0x141b251d0` before it promotes
`[node+0xe8]` into `[node+0x38]`. Read at `0x141b251d0` (assembly, and the
decompiler agrees on the call shape):

- it takes the object in `RCX` and, when it is non-null, calls **vtable slot
  `+0x18`** and requires the returned `int` to be `> 0` (a non-positive answer
  goes to the diagnostic at `0x1407a2610`);
- it sets bit `0x2000` of a `u16` flags word at **`+0x1c`**, then, if bit `0x10`
  is set, unlinks the object from an intrusive doubly-linked list through
  **`+0x08`** (`prev`) and **`+0x10`** (`next`). Flags bits `0x0c` choose one of
  **two** global list pairs -- `0x1441387a0`/`0x1441387a8` with counter
  `0x1441387b0`, and `0x1441387b8`/`0x1441387c0` with counter `0x1441387c8`;
- it then calls `0x141b253c0` on the object. That function is **not** a
  destructor: it ORs bit `0x4000` into the same flags word and links the object
  into a *per-thread* list (head `0x144138788`, keyed by the thread id at
  `0x1441407d8`; the entry it allocates is `0x28` bytes). So it is a
  **deferred-destruction queue**, not a free;
- finally it decrements a 12-bit-biased global counter at `0x14413879c`.

So `0x141b251d0` is a **generic release of a refcounted resource object**, and
the two slots it clears hold instances of that resource base -- `{vtable @0x00,
list prev @0x08, list next @0x10, u16 flags @0x1c}`. **The class id in this path
is the vtable pointer at `+0x00`**, and it is a runtime value: no static constant
in the chain names either class. That is why the release path reports the
*family* (a refcounted resource in one of two global caches) and not the class;
the class has to come from the other direction, the registered type that has
`C`'s layout.

The two released objects are the node's *loaded asset object* (`+0x38`, whose
`+0x40` is the STU instance) and a second slot (`+0x40`) cleared the same way;
`[node+0xe8]` holds a freshly loaded instance before it is promoted.

## Step 2 -- `C`'s class, by type identity

### The registry is enumerable from the call sites

Every registered class is one call to `RegisterComponentTypeDescriptor`
(`0x1421cd360`, 7,956 call sites) from a generated registration function. The
call carries the descriptor's BSS address (`RCX`), the type hash (`R8D`), the
instance size at `[rsp+0x20]`, the alignment at `[rsp+0x28]` and the
construct-in-place hook at `[rsp+0x80]`. `dev/model_registry.py` recovers those
five values for every call site, which turns "which class has this layout?" into
a lookup. The descriptor's fields, from `0x1421cd360` itself (registration):
vtable `+0x00`, field-table pointer `+0x08`, hooks at `+0x50`, `+0x58`, `+0x60`
(construct in place), `+0x68`, `+0x70`, `+0x78`, `+0x80`, `+0x88`, `+0x90`, size
`+0xa0`, align `+0xa4`, `+0xa8`, `+0xb0`, type hash `+0xb8`, `+0xbc`.

### The client's reads fix `C`'s offsets

| read at | by | value |
|---|---|---|
| `C+0x24`/`C+0x28` | `0x141b5cfb0` | a reference block, yielding `geometry+0xa8` (`& 0x7fff`); the block's layout is from `formats/brush-collision.md` |
| `C+0x98` / `C+0xa0` | `GeometryBuild` | the instance-point array slot and its tag |
| `C+0xc0`, `C+0xd0` | `GeometryCopyParams` | 8 bytes each into the config block |
| `C+0xf0` | `GeometryBuild` | into `geometry+0x90` |
| `C+0x110`, `C+0x114` | `GeometryCopyParams` | scaled by the build's radius |
| `C+0x118`, `C+0x11c` | `GeometryCopyParams` | `1/x`, with a `1.17549435e-36` guard |
| `C+0x130`..`C+0x174` | `GeometryCopyParams` | collision scalars, each clamped |
| `C+0x178`, `C+0x186` | `0x141b53c40` | 16-bit folds into two `u16` ids |
| `C+0x194` | `GeometryCopyParams` | a flag: set means the output field becomes `-1.0`, clear means `max(0, C+0x160)` |
| `C+0x195` | `GeometryCopyParams` | copied to the output byte |

### The class that matches

Searching the reference registry for a class declaring a field at **`0x186`**
returns **exactly one of 7,782**: `0x564B1018`, **size 408 = `0x198`**, with
fields at `0x110`/`0x114` (a `Vec2`), `0x118`/`0x11c` (a `Vec2`),
`0x120`/`0x128` (two more `Vec2`), floats from `0x130` to `0x174`, two **14-byte**
inline instances at `0x178` and `0x186`, and two bytes at `0x194` and `0x195`. A
14-byte field explains the `0x178`/`0x186` spacing exactly, and `0x186 + 14 =
0x194` closes on the two bytes. The client's registration of the same hash gives
**size `0x198`, align 8** -- the reference's declared size, the first independent
agreement.

Four further agreements make the identification a read rather than a guess:

1. **The constructor installs the class's inline sub-objects.** `0x1413354b0`
   (`desc+0x60`) writes a vtable at `+0x00` (`0x1431dd118`) *and* at **`+0x08`**
   (`0x1431dceb8`), **`+0x50`** (`0x1431dd080`) and **`+0x90`** (`0x1431dcf50`) --
   exactly the three inline-instance fields the class declares at `+0x08` (72
   bytes), `+0x50` (64 bytes) and `+0x90` (24 bytes).
2. **The constructor writes the four asset-ref sentinels.** It stores `-1` at
   **`+0xb8`, `+0xc8`, `+0xd8`, `+0xe8`**, and those are exactly the class's four
   16-byte `teStructuredDataAssetRef` fields. It also zeroes `+0x18`, `+0x28`,
   `+0x38`, `+0x60`, `+0x70`, `+0x80`, `+0xa0`, `+0xb0`, `+0x100`.
3. **The class's own field dispatch compares its declared field hashes.**
   `desc+0x68` = `0x141334ea0` and `desc+0x70` = `0x141335e60` are hash-dispatch
   tables over the class's fields, and **22 of the class's 36 declared field
   hashes appear as immediates in both**. They are the scalar ones --
   `0x3963555A` (`+0x110`), `0xA19B623C` (`+0x118`), `0x3115DC66` (`+0x130`),
   `0x7ACAB593` (`+0x134`), `0xCC99A0E8` (`+0x138`), `0x30ACA439` (`+0x13c`),
   `0xF1C1F010` (`+0x140`), `0x7CBA482F` (`+0x144`), `0x6CC45E85` (`+0x148`),
   `0xAC098B42` (`+0x14c`), `0xBA0D381D` (`+0x150`), `0x2C434213` (`+0x154`),
   `0xB8126548` (`+0x158`), `0x0AB1D1CD` (`+0x15c`), `0x6B532115` (`+0x160`),
   `0x4912A876` (`+0x164`), `0xAF04D648` (`+0x168`), `0x7E9BF4A0` (`+0x16c`),
   `0xCBCB2AB9` (`+0x178`), `0x077CF4AA` (`+0x186`), `0x12B475D4` (`+0x194`),
   `0x82C0FDDF` (`+0x195`). The non-scalar fields (inline instances, arrays,
   asset refs) are not compared against a default there, which is why they are
   absent from the dispatch.
4. **`GeometryCopyParams` reads no offset the class does not declare.** Every
   offset in the table above lands on a declared field, and the two 14-byte
   inline instances at `+0x178`/`+0x186` explain the `0x141b53c40` calls.

Registered identity of `C` (all from the client's own registration):

| value | address / number |
|---|---|
| type hash | `0x564B1018` |
| size / align | `0x198` / 8 |
| type descriptor | `0x143e74830` |
| field table (`desc+0x08`) | `0x143e74790` |
| construct in place (`desc+0x60`) | `0x1413354b0` |
| factory (`desc+0x58`: allocates `0x198` then constructs) | `0x141335610` |
| per-class field dispatch (`desc+0x68`, `desc+0x70`) | `0x141334ea0`, `0x141335e60` |
| instance vtable (written by the constructor) | `0x1431dd118` |
| own registration function | `0x1402db070` |

### What the class's fields are

Only the fields the client reads are quoted here; the hash and offset of each
were checked against the client (agreement 3 above).

| offset | hash | declared type | the client reads it as |
|---|---|---|---|
| `+0x08` | `0x8649028E` | inline instance, 72 bytes | -- (a sub-object with its own vtable) |
| `+0x50` | `0xC9C8A2AB` | inline instance, 64 bytes | -- |
| `+0x90` | `0xB7C8314A` | inline instance, 24 bytes | -- (contains the point array) |
| `+0xa8` | `0xB48F1D22` | string, 16 bytes | -- |
| `+0xb8`, `+0xc8`, `+0xd8`, `+0xe8` | `0x9544A61B`, `0x1BE620F1`, `0x33E51FDC`, `0x85DFE5BC` | asset refs, 16 bytes | -- |
| `+0xf8` | `0x20DE3871` | inline array, 16 bytes | -- |
| `+0x108` | `0xEDF0511C` | `u64` | -- |
| `+0x110` | `0x3963555A` | `Vec2` | scaled by the radius |
| `+0x130`..`+0x174` | (23 fields) | 22 `f32` and one `u32` | the collision scalars |
| `+0x178` | `0xCBCB2AB9` | inline `STU_933AD0A9` (14 `u8`) | folded to a `u16` |
| `+0x186` | `0x077CF4AA` | inline `STU_933AD0A9` | folded to a `u16` |
| `+0x194`, `+0x195` | `0x12B475D4`, `0x82C0FDDF` | `u8`, `u8` | a flag and a byte |

## The instance-point array (`C+0x98`) -- read, and now implemented

`formats/brush-collision.md` called this the open part of `C`. It is read
(decompiled, then confirmed on the instruction stream at `0x14213c54b` and
`0x14213c936`):

```text
A = *(u64 *)(C + 0x98)              // a pointer, not the header itself
if (*(u64 *)(C + 0xa0) == 1) {      // the tag is compared as a whole qword
    points = *(u64 *)(A + 0)
    count  = *(u32 *)(A + 8)
} else {
    count  = *(u32 *)(A + 0)
    points = *(u64 *)(A + 8)
}
if (count == 0) -> the build is rejected
```

`A` is a 16-byte record holding `{count, data}` in one order and `{data, count}`
in the other; the tag says which. `GeometryBuild` passes `points`/`count` as the
arguments of the ring builder `0x142ced620`, whose input is 16-byte `Vec3A`
instances -- which is the declared field at `C+0x98`: the inline sub-object
`STU_06A1888C` (at `C+0x90`) has exactly one field, `0x88FCECD7`, a `Vec3A` array
with a 16-byte header. So **the model's instance points are the `Vec3A` array
`0x88FCECD7`**, and `C`'s collision ring is built from them.

The slot-plus-tag shape is not private to this class: the image has 144 sites
matching "compare a 16-byte slot's second qword with `1`, then `cmovne` the
pointer by `+8`", across many classes and offsets. It is the client's generic
accessor for an array field whose header may be stored in either order. **Not
read:** who writes the record and the tag (the loader), and so what makes a tag
`1` -- the read expression is exact, the producer is not.

`ow_tank::model_collision::ArrayRecord::points_and_count` is that expression,
operation for operation, and `ArrayRecord::is_empty` is the count guard.

## The two 14-byte flag records (`C+0x178`, `C+0x186`)

`0x141b53c40` takes the address of one of them and returns a `u16`. Read as
assembly: it is `h = (h << 1) + byte` over the record's fourteen bytes, wrapping
to 16 bits, with the bytes folded in the order the compiler emitted them --
indices `6, 2, 13, 12, 8, 1, 3, 5, 4, 11, 10, 0, 9, 7`. `STU_933AD0A9` is
fourteen `u8` flags (the reference gives defaults of `1` for all but the seventh),
so the fold is a 14-bit mask of those flags, which is what `GeometryCopyParams`
stores into the two `u16` ids of its output block.
`ow_tank::model_collision::flag_mask` is that fold and its test pins the byte
order (`0x3fff` for fourteen ones; byte 6 is the heaviest bit, byte 7 the
lightest).

## The deserialiser: where the instance is filled

`C` is a **retail (version 2) structured-data instance**, so its field order is
not the class declaration's: a v2 payload begins with an `i32` index into the
instance's own field bag and stores fields *in bag order*, only those that differ
from the class default (`formats/stu.md`). What the class supplies is the
defaults and the offsets; what the payload supplies is the order and the
presence. The class's own hooks are:

- `desc+0x60` = `0x1413354b0`: **construct in place** on payload bytes. This is
  the hook the STUD loader calls per record (`0x1421d26d0`), which is why the
  constructor writes only the vtable, the three inline sub-object vtables and the
  four asset-ref sentinels.
- `desc+0x58` = `0x141335610`: **allocate `0x198` and construct** -- the
  standalone instance factory, which installs the same vtable.

No per-class *field reader* exists in the image: `desc+0x68`/`desc+0x70` are
per-field hash dispatch (name and default tests), and the offset conversion is
the generic pass (`0x1421d3710`, `formats/stu.md`). So "the deserialiser's field
order" resolves to: the order is the asset's, and the class-declared order in the
table above is what a version-1 payload would use.

## Where `C+0x48` is filled -- read, and the one missing read

`C` is reached as the **loaded instance of an asset reference**:

- the reference registry lists every class that declares a field of type
  `STU_564B1018`. There are two, and both are asset refs: `STU_09CB5A77` (size
  `0x30`, base `STUEntityComponent`) with `teStructuredDataAssetRef<STU_564B1018>`
  at **`+0x08`**, and `STU_2FD9D8D6` (size `0x38`) with the same type at
  **`+0x28`**;
- the client reads the pointer at `[[node+0x38]+0x40]+0x48` -- a pointer taken
  from an object *base plus `0x40`*, then `+0x48`. `0x48` is exactly `0x08 +
  0x40`, i.e. the declared `STU_09CB5A77` field offset plus the size of one
  object base.

**What is not read**, exactly: the identity of the class at `[node+0x38]` and of
its `[+0x40]` STU instance, and so whether that `+0x40` shift is the runtime
header of an entity component. Neither class appears as a constant anywhere in
the chain -- the release path's class id is a vtable read at runtime (step 1) and
the `+0x48` store is a plain pointer store. The **one missing read** is the
producer of `[node+0xe8]`: whoever stores the freshly loaded instance there is
the loader that also owns `[+0x40]` and the `+0x48` write, and its allocation
site names the class. The update that promotes `+0xe8` is reachable -- one static
caller, the dispatcher at `0x14204824a` -- but the dispatcher is held as a
runtime function pointer, so the walk stops one hop short of the scene tick, and
nothing on the path names either class.

An inferred reading that is **not** claimed here: that `+0x40` is the
entity-component runtime header and that `node+0x38` is an instance of a
`STU_09CB5A77`-like component. The arithmetic fits, but nothing in the image was
read to confirm it, so it stays a hypothesis and no code depends on it.

## The install side: what is open

`C`'s encoding is now fully read (array slot, tag, count guard, the two flag
folds, the scalar offsets), but **no install asset carrying a `C` instance has
been located**, so the practice range's brush is still not decoded. This is a
data question with a known shape, and it is the next anchor in this order:

1. **Find a model's `C`.** `C` is a *referenced asset*, so its payload is its
   own STU asset rather than a field of the model's `MSTU`; the two referrers
   above are components, so the reference is resolved through the entity
   definition of whatever spawns the brush. A cheaper check first: parse each
   model asset's `MSTU` (`ow_tank::mstu`) and look for an `InstanceInfo` whose
   type hash is `0x564B1018`; if any model carries it inline, its `0x88FCECD7`
   array is a real brush input.
2. Then `brush::build_from_instances` -> `brush_mesh::hull_stage` ->
   `brush_convert`, all of which are implemented and pinned against the client,
   so a real point array can be taken to a mesh without new code.
3. The remaining code item is `C`'s owner class (above); the map-side
   collision-chunk cell decoder of `formats/brush-collision.md` is unchanged.

## Checks

| claim | check |
|---|---|
| the array slot's two orders and the tag test | `model_collision::tests::the_array_record_is_read_the_way_geometry_build_reads_it`, `the_tag_is_a_whole_qword_test` |
| the zero-count rejection | `model_collision::tests::a_zero_count_is_refused` |
| the 14-byte fold and its byte order | `model_collision::tests::the_flag_fold_uses_the_client_byte_order` |
| the class identity and field offsets are consistent | `model_collision::tests::the_class_identity_is_the_registered_one` |
| the registry enumeration itself | `dev/model_registry.py --hash 0x564b1018` (size `0x198`, align 8, ctor `0x1413354b0`) |
| the class's declared layout | `dev/model_class_search.py --min-size 0x190 --offset 0x186` (one class of 7,782) |
| the field-hash dispatch agreement | `dev/model_class_search.py`'s parse against the immediates of `0x141334ea0` / `0x141335e60` (22 of 36) |

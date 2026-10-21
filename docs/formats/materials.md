# The material chain, as read from the files and from the client

## Current state

- **Chain: read and joined** - model -> material id -> look -> material
  asset (`0x008`) -> material data (`0x0B3`) -> texture (`0x004`), with an
  ambiguity decline on the id join.
- **Textures: decoded and oracle-checked** - BC1-7 and BC6H (Microsoft
  reference decoder, zero differences over real blocks); 98.7% readable
  census.
- **Data limit:** the `0x008` material asset is 16/2000 readable on this
  install (keyring wall). The readable subset renders textured; the rest is
  the same wall, not a parsing gap.

**Verify:** `cargo run -p ow-tank --bin look -- --join 400`;
`cargo test --release -p ow-tank --test bc6h`.


The path from a model to a textured surface is four hops, and the first hop is
the one that resists the files:

```text
model          MDLC chunk: an array of 64-bit material values
  material     type 008: four GUIDs -- shader group, shader source,
               an unknown (03A), and material data     [confirmed in files]
    data       type 0B3: STUMaterialData, holding texture references
      texture  type 004: a header and DXT/BC payload
```

This document records what has been established about the first hop from the
retail install's files **and from decompilation of the captured running
client**, what has been ruled out, and where the remaining unknown sits. Nothing
below is copied from the game; every address is a coordinate in the operator's
own capture under `dev/dumps_range` (gitignored), recorded so the work can be
re-derived.

## From the files

* The `MDLC` payload is `MaterialOffset` (i64 at +8, relative to payload) and
  `MaterialCount` (u16 at +104), then `count` 64-bit values at that offset.
  A second offset/pointer sits at payload+0 with its count as the byte at
  payload+106; on most models inspected it is zero (no second array).
* The same 64-bit value recurs across models that share a material, so the
  value is a property of the material, not of the model.
* A material asset (type 008, 144 bytes on the install inspected) begins with
  four plain 64-bit GUIDs — shader group (085), shader source (088), an unknown
  (03A), material data (0B3) — confirmed by decoding the raw bytes of installed
  materials against `teResourceGUID`'s type layout. These four are **not**
  obfuscated.
* The model's own `MSTU` structured data does not repeat the MDLC values, and
  its reference fields do not deobfuscate to material GUIDs under the STU
  scheme with any of the model's field hashes (tested over every byte offset
  of the blob; the only type-008 decode was a one-in-4096 coincidence).

## Ruled out, with the test that ruled it out

The values are not a disguised GUID under any cheap scheme. Each of these was
decided by a test, not by inspection:

| family | test | result |
|---|---|---|
| bit permutation (+ XOR) | XOR-differences of values must have low popcount; same-model pairs had min 22, median 32 | impossible for any bit permutation |
| permutation + constant | derive constant from value 1 against all 99,392 material GUIDs, require values 2 and 3 to land, run over the full GUID list | 71 permutations x 3 candidate types: no |
| additive linear (`K*guid + C`) | GCD of all pairwise value differences | 1 |
| xxhash64/xxh32/xxh3, murmur3 (32/64/128), fnv1/fnv1a (32/64), crc32/crc64 (reflected/forward, seeds 0/~0, inv), djb2, sdbm, jenkins96, jenkins-oat, city | run each over LE/BE GUID bytes, index-only bytes, TankLib string forms; intersect with observed values (halves for 32-bit forms) | 0 landings |
| finalizer-only mixes | `fmix64`, `splitmix64`, xxhash avalanche, with/without XOR seeds | 0 landings |
| content key | first/last 8 bytes of manifest content keys | 0 landings |

So the mapping is either a keyed/uncommon hash, or an invertible
nonlinear transform (e.g. a Feistel/ARX mix). It cannot be recovered from the
files alone; it must be read from the code that produces or consumes it.

## From the captured client (Ghidra, `dev/dumps_range`)

Capture regions are raw binaries named `base_size_type.bin`; the Ghidra
programs are opened at their recorded base (`x86:LE:64`). The addresses below
belong to build `2.25.0.0.154088` as captured; they will move on the next patch.

### The model apply path

* Chunk dispatch: module `7ff649e60000`, function `FUN_7ff64ba068b0` in
  program `7ff64b9ff000_00010000` — a compare chain over file-form chunk ids
  (`0x4D444C43` = MDLC with version 357, `0x4D524E4D` = MRNM, `0x4D535455` =
  MSTU, plus HTLC/DSEM/MOCL/MSKL).
* Apply: at `0x7ff64ba066b5` the applier checks the MDLC record and calls the
  **MDLC deserializer `FUN_7ff64ba31b80`** (program `7ff64ba1d000_00025000`).
* `FUN_7ff64ba31b80` copies the header, then **memcpys the material values
  verbatim** — `count*8` bytes from the relocated `MaterialOffset` into an
  in-struct slot whose pointer is published at runtime+0x38, and the second
  array (payload+0, `byte[106]*4` bytes) alongside. There is **no transform at
  parse time**. A size helper at `0x7ff64ba31d00` re-derives the count from
  payload+104 and the 0xB3 base.

### The material-table machinery (u32 keys)

The client does not look materials up by the 64-bit value in the code that has
been read so far; it works with **32-bit keys**:

* `FUN_7ff64ba323a0` builds the material table blob: 6-byte chain records
  (count at +0x64), two 64-byte arrays, two 0x30-record arrays, a 32-byte
  array, a **u32 key array at +0x38 with its count at +0x66**, u16 arrays, and
  a **u64 array at +0x58 with count at +0x6a**. The u32-key count (+0x66) and
  the u64 count (+0x6a) come from source fields +0x6e and +0x72.
* `FUN_7ff64cd19350` (program `7ff64cd0f000`) derives those counts on the
  packed model: `+0x6e = word[+0x140] - word[+0x13e] + word[+0x13c]` — an
  **index range**, i.e. models reference slices of a shared table — and
  `+0x72` defaults to `+0x6e` (equal counts: one u32 key per u64 material).
* `MaterialLookup_Index` at `0x7ff64b9e3320` (program `7ff64b9d3000`) answers
  a u32 key with a material index: a linear scan of the u32 array (count at
  +0x66), returning -1 when absent. Its body is obfuscated junk around a
  clean final loop — the loop is the contract.
* `FUN_7ff64ba2c880` caches key resolution with **FNV-1a-32** over
  `(seed ^ table+0x60)`; the FNV basis `0x811c9dc5` is initialised in the
  binding object constructor `FUN_7ff64ba2ca20`.
* Consumers: `FUN_7ff64ba3eb80` and `BindModelMaterials`
  (`FUN_7ff64ba25c40`) take u32 keys from the entity side (`+0xe0`, `+0xe8`,
  and a 16-byte entry array at +0x10c), look them up, clamp against the MDLC
  material count, and drive 0x30-byte colour/parameter records.
* The entity keys come from templates: `FUN_7ff64ba2de50` returns the raw
  qword at `ref+0x18` from an archetype at `entity+0xa8`, offsets `+0x1c0` /
  `+0x1e0`.

### The per-material construction loop

In program `7ff64aaeb000_0002f000`, inside (the obfuscated)
`FUN_7ff64aaef3e0`, the readable window at `0x7ff64aaff0a0..0x7ff64aaff2da`
is a two-loop construction over the MDLC runtime (`rbp`: bbox floats at +0x40,
counts u16 at +0x98/+0x9a/+0x9c, self-relative offset at +0x90):

* cursors `r13 = rbp + dword[rbp+0x90]` (and `r12`) walk **length-prefixed
  runs** — each element: `u32 size`, then `size` bytes, cursor advancing by
  `4 + size`;
* per element: allocate (`0x7ff64ba58fb0`), memcpy, wrap
  (`0x7ff64c099c90/0x7ff64c099f00`), then a **virtual type query against a
  global type-id** (`0x7ff64dbdb7b0` in loop A bounded by `word[rbp+0x9a]`;
  `0x7ff64dbd9c20` in loop B bounded by **`word[rbp+0x9c]`, the material
  count**);
* loop A switches on a field at `obj+0x40` (values 0..5) selecting slot sizes
  0x20/0x30/0x40/0x50, then calls **`0x7ff64a44b430`** with a slot in the
  destination model (`r14+size`) and stores the wrapped object pointer;
* after both loops the header counts/bbox are copied into the destination
  (`r14+0x90..`, xmm from `rbp+0x40` to `r14+0xa0`).

`0x7ff64a44b430` turned out to be a growable-array append (capacity math at
+8/+0xc, doubles, returns `&back()` for the caller to store the wrapped object
pointer into) — infrastructure, not the resolver. The type-ids
(`0x7ff64dbdb7b0`, `0x7ff64dbd9c20`) are self-referential id-structs in data
region `7ff64db38000` with no names (the client's RTTI-lite compares by
pointer identity).

### The pointer-decode idiom (not the material transform)

The first read of `0x7ff64a44b550` opens with
`ror 41 → xor 0x72fee1aad1a4bf06 → sub 0x675814fad68ae3d5 → ror 58` on a
loaded qword, which looks like *the* decode. It is not: the canonical copy at
`0x7ff64a3ab0c0` (region `7ff64a3a6000`) shows the result is used as a
**pointer** — `mov rax,[rbx]; call [rax+8]` — so this chain is the engine's
obfuscated-pointer idiom, present at 587/600 K1/K2a constant sites across the
modules. As a material decode it was tested anyway (both arrangements, both
directions, against all 1,514,319 registry GUIDs): zero hits. Anyone who finds
these constants again should read them as pointer protection first.

### Two more consumers, and the low-32 evidence

Region `7ff64abd3000_00012000` holds two more count readers:

* `0x7ff64abd71d1`: takes `word[entry+0x9c]` (material count), scales it to a
  float index, clamps to `count-1`, computes a flat index
  `edi * material_index + slot`, bounds-checks against `dword[base+0x38]` and
  reads a **u32 array with element stride 4** — while an inner loop at
  `0x7ff64abd71bf` does `cmp r15d, dword[rax + r8*8]`: a **u32 key compared
  against the low 32 bits of 8-byte entries**. That is the strongest evidence
  yet for `u32 key == low32(64-bit material value)`, which fits everything
  else: entity lookups take `*(u32*)(binding+0xe0)` while the binding stores
  the full template qword, the per-model counts for keys and values are equal,
  and the scan is linear.
* `0x7ff64abddc11`: uses the same count for interpolation
  (`cvtsi2ss(count-1)`), then passes the struct on to `0x7ff64b7f23a0`
  (region `7ff64b7d8000`) — a consumer still to be read.

### Notes on the capture

* The asset-core region `7ff64aaeb000_0002f000` is **obfuscated**: its
  functions decompile into hundreds of unreachable blocks around clean
  windows. Read it by disassembling address windows, not by decompiling whole
  functions. Capstone on the raw region files works when the Ghidra bridge
  is wedged by one of those decompiles.
* No decoded MDLC buffer was found in the heap scans (decode-parse-free, same
  as map chunks), and a structural scan for the runtime self-pointer
  (`[x+0x38] == x+0xb8`) produced only a false positive.
* The material GUIDs on this install have the form `0x0e000000XXXXXXXX`
  (type 008, mangled `0xE00`); 99,392 of them, ~3 in 45 downloaded.

## The model chunk set, read end to end

The per-model object deserializer is `FUN_7ff64b9e6640` (it calls both the
MDLC deserializer and the mskl blob builder). It is the full chunk set of one
model, and every branch is now read:

| chunk | version | handler | what it does |
|---|---|---|---|
| `MDLC` | 357 (0x165) | `FUN_7ff64ba31b80` | the material list; full layout below |
| `MRNM` | 31 (0x1f) | `FUN_7ff64ba31de0` | render mesh tables |
| `mskl` | 32 (0x20) | `FUN_7ff64ba323a0` | the table blob (below) |
| `MOCL` | 20 (0x14) | `FUN_7ff64ba31d20` | occlusion/limits table |
| `DSEM` | 2 | inline | memcpy + self-relative fixup of the two leading qwords, pointer cached at runtime+0x20 |
| `HTLC` | 24 (0x18) | `FUN_7ff64ba318d0` (fixup) | memcpy + fixup, pointer cached at runtime+0x18 |
| `MSTU` | 2 | `FUN_7ff64ba31cc0` | structured data; adds chunk word to runtime+0x80 |

The dispatch keys on the chunk word (ascii reversed) and the version, and the
handler receives the payload (chunk+0x10) plus the already-built runtime.

### The MDLC payload, field by field

The deserializer copies two arrays and a header block; the copy is verbatim in
every field, confirmed by an independent decompiler (REA over a minimal PE of
the same capture):

* `+0x00` qword: payload-relative offset of a **u32 array**; count is the
  **byte at `+0x6a`**; the array is copied to just past the u64 array, and the
  pointer is stored at runtime `+0x30`.
* `+0x08` qword: payload-relative offset of the **u64 material values**; count
  is the **u16 at `+0x68`**; copied to runtime `+0xb8` (aligned) with the
  pointer stored at runtime `+0x38`.
* `+0x10..0x28`: copied as three 16-byte loads to runtime `+0x40` (the box).
* `+0x40/+0x50/+0x54/+0x58/+0x5c/+0x60/+0x64` dwords copied to `+0x70/
  +0x84/+0x88/+0x8c/+0x90/+0x94/+0x98`.
* `+0x68` u16 count of u64 values → runtime `+0x9c`; `+0x6a` u8 count of u32
  values → `+0xac`; `+0x6b` byte is reduced with `BSF` (index of the lowest set
  bit, 0xff when zero) → `+0xa9`; `+0x6c` byte → `+0xa8`; `+0x70` qword →
  `+0xa0`; `+0x78`/`+0x79` bytes → `+0xaa`/`+0xab`.
* the first six runtime qwords (`+0x00..+0x28`) are zeroed at the end.

So the u32 array the earlier readers keyed on is **not derived** from the u64
values anywhere in the deserializer; it is its own shipped array, and the
`byte[+0x6a]` count means it is empty for most models.

### The mskl chunk and the blob it builds

`FUN_7ff64ba323a0` packs the mskl payload into a blob whose fields are
payload-relative offsets, all copied verbatim:

* blob+0x00 self-pointer to the data area (payload+0x81, aligned);
* from payload+0x00, 6-byte records × count(`+0x6c`);
* from payload+0x08 and +0x10, two 64-byte-record arrays × count(`+0x6c`);
* from payload+0x18 and +0x20, two 0x30-record arrays × count(`+0x6c`);
* from payload+0x28, a 0x20-byte-record array × count(`+0x6c`);
* from payload+0x30, a **u32 array** × count(`+0x6c`);
* from payload+0x38, a **u32 array** × count(`+0x6e`);
* from payload+0x58, a u16 array × count(`+0x6c`);
* from payload+0x60, a **u64 array** × count(`+0x72`);
* counts land at blob+0x64 (`+0x6c`), +0x66 (`+0x6e`), +0x6a (`+0x72`), plus a
  dword from payload+0x68 at blob+0x60 and 14 bytes from payload+0x74 at
  blob+0x6c;
* the finished blob pointer is stored into the model runtime at `+0x08`.

There is **no transform** in the builder either: it is a repack.

### What the install actually ships (probe over the live registry)

`dev/mdlc_probe` (read-only, opens the install the same way the rest of the
tree does) walks the model assets and prints both arrays:

* 31,280 model assets exist; the MDLC payload is 144 bytes on simple models,
  with the u64 array at payload+0x80.
* Shared u64 values across models are real and common (`0x81b09ab0f566b1d2`
  recurs; the earlier `dev/range_mdl_values.txt` shows the same shape).
* The **u32 array is present in only a minority of models** (99 of the first
  2000 scanned were non-empty) and its values are **small integers** — slot
  ids, e.g. `[1, 2, 0x15, 0x16]`, `[1, 2]`, `[2]` — with counts 1..4. They are
  not hashes and never equal `low32` of the paired u64 values.
* **mskl exists in the files** (lowercase tag; 83 of the first 575 models).
  Its u32 arrays hold small integers in the same id space, its u16 array is
  index-like with 0xffff sentinels, and its **u64 array ships as all
  0xffffffffffffffff** — a placeholder table the runtime fills, or an "unset"
  marker. The two u32 arrays have *different* counts (e.g. 79 and 79 vs 62;
  229 and 130 vs 199), so the key array is not a low-half view of the value
  array.

That answers the old question 2 in the negative, and it reframes question 1:
the join from a model to a material asset does not run through a per-model
hash of the GUID. The u32 keys in the table are *indices*, and the u64 values
in the table are written at runtime.

### The count-writer cluster, re-read

The four sites the earlier pass had flagged by offset were re-read from their
own capture regions (programs built with `dev/mkpe.py`, decompiled through
REA, and matching the Ghidra project's reading of the same bytes):

* `0x7ff64c686447` in `FUN_7ff64c686130`: normalises counts — takes `abs()` of
  the u16 at +0x6e and, when a flag bit (0x20) is clear, copies `+0x6e` into
  `+0x72`. The struct it operates on is a parse object (0x10-byte records,
  status codes 0x40/0x51/0x55), not the model runtime.
* `0x7ff64c6a1879` in `FUN_7ff64c6a15e0`: computes a **capacity** —
  `count * 12 / 10`, clamped to `max - min`, with a default of 1000 — and then
  copies `+0x6e` into `+0x72`. This is a 1.2× growth margin, not a material
  count.
* `0x7ff64c6b445b` and `0x7ff64c6c88af` are not inside any analysed function
  in the region programs (unreached code or data); nothing to attribute.

So `+0x6e`/`+0x72` recur across unrelated structs (the `+0x72 = +0x6e`
*default* is real, but it is a default in several different objects), and the
cluster is **not** the material-table writer. The earlier listing by offset
alone was ambiguous; this pass separates the real uses.

### The other consumers, read

* `0x7ff64b7f23a0` (the consumer passed the struct) is a **blend-weight
  normaliser**: it takes a float weight array, sums the positive weights,
  refines a reciprocal (one Newton step), and appends `(u16 index, float
  weight)` pairs into a fixed record with its count at +0x24 — the per-vertex
  material-layer stack (capacity 3-4 entries).
* `0x7ff64ba2e360` (the `+0x88` chain) is a **7-instruction lazy accessor**:
  when `obj+0xb0` is null and `obj+0xe2` is non-zero it caches
  `*( *( *(obj+0x18) + 8 ) + 0x88 )` into `obj+0xb0`. It has **29 callers** in
  the v2b capture (all in `0x7ff7c68fxxxx..0x7ff7c691xxxx`, the same band as
  the material binding code; the range count of 16 was a partial capture).
  None of the call sites is in the deserializer.

#### The `+0x88` object is the per-model material table, and its consumers are read

Eight callers plus the binding helpers were decompiled (v2b; exec = v2b −
`0x71820000`). The object's fields, as its consumers use them:

| field | use |
|---|---|
| `+0x00` | array of **6-byte records**; the reader indexes `record[i] + 4` as a u16 (the layer-pair index) |
| `+0x0c` | u32, an extra FNV seed for the slot-cache hash |
| `+0x20` / `+0x30` | fallback material pointer (used when `+0x30 == 0`) / its presence flag |
| `+0x48` | **packed u64 material key** (= two u32s) — the material-asset lookup key |
| `+0x60` | **material count**: copied into the renderer binding's `+0xf0` and used as the slot count and the FNV seed |

* **The material-asset lookup** is `FUN_7ff7c68e4560(registry, keyA, keyB)`: it
  scans a packed-u64 key array (registry `+0x2f0`, count `+0x2f8`) and returns
  the matching **0x38-byte entry** from the array at `+0x330`; a miss falls
  into a **no-return assert path** (`FUN_7ff7c6d50570`). The consumer passes
  `tableObj+0x48` as the key — this is the client-side material-asset hop.
* **The record-index lookup** is `FUN_7ff7c68a3320(tableObj, key)` → record
  index (short, `-1` = absent). Its body is obfuscated (nibble decode tables
  `0x7ff7c7d083d0`/`0x7ff7c7d083e0` over the same obfuscation state as the
  dataflow query path), so it is read by its call sites' use, not its own code.
* **The u64-array binding** is `FUN_7ff7c68ec880(binding, tableObj, count)`:
  the binding is an array of **0xC-byte records** `{u32 key, u16 index, …}`;
  per record it resolves `key → index` via the table, **clamps the index by
  `count`** (the deserialized blob's u16 at `+0x9c`, the u64-value count),
  stores the short at record+4, counts the resolved entries at binding+4 and
  sorts the records. The whole pass is skipped when an FNV-1a hash of the
  count (seeded by `tableObj+0x60`) matches the binding's cached `+0x00`.
* **The slot binder** is `FUN_7ff7c68f30a0(container, tableObj, material,
  flag)` + `FUN_7ff7c68f2830`: the container's slot count is set from
  `tableObj+0x60`; weights are clamped to `[0,1]`; the resolved indices are
  insertion-sorted ascending; per-vertex average weight is stored at
  container+4 with the per-slot weight array at +8; layered entries merge
  through the table record's u16 at `+4` (`0x7ff7c68f3b90` is the merge
  helper). Variants `0x7ff7c68f3000/31a0/3200/3300` are the flag/paths for the
  same binder.

So the file's u32 keys are consumed as **6-byte table records → resolved
indices bounded by the u64-value count → renderer weight slots**, and the
material-asset *asset* is reached through `tableObj+0x48` against the packed
u64 key registry (0x38-byte entries). The one thing the client path does not
interpret is the **content of the u64 values themselves** — they remain the
array whose length bounds the indices.

### What consumes the per-material arrays: the applier, and the field-record family

The client's **material-parameter applier** `FUN_7ff7c68f5940` (v2b, 3,848
bytes) is the consumer the earlier rounds were missing, and reading it also
identifies the *field-record* family every material read goes through.

* **The applier** takes `(materialRecord, {model, materialDescriptor})` and
  first resolves the material's **descriptor**: two field-bag reads
  (`FUN_7ff7c68edd60(ctx, src+0x68)` → record+0x190, a u32;
  `FUN_7ff7c68ede00(ctx, src+0x48)` → record+0x194) feed a lookup
  (`FUN_7ff7c68ee1c0`) whose result is validated by a type query, with a
  fallback `FUN_7ff7c5545160(key, 6)`. The **scan keys** are that descriptor's
  u32s at `+0xac` and `+0xb0` (`plVar14[0x16]`). For each key it scans the
  model's **8-byte key records** (at `model+0x48`, count = the u16 at
  `model+0x06`) comparing the record's first dword. On a match it reads the
  **parallel 8-byte value record** (`model+0x58 + i*8`) as a **u16 at
  `record + 4 × (i32 at record+4)`** — a *self-relative dword offset* —
  converts it to float, scales it by `DAT_7ff7c7d5d84c` (**1/30**), clamps ≤0
  to 0, and stores it into the renderer material record's min/max pair
  (`+0x240`/`+0x250`; the other variant writes `+0x228`). So the per-material
  records are consumed as **references to u16 scalar parameters in
  1/30-second units**, used as the material's time bounds.
* **The field-record family.** `FUN_7ff7c68edb60` (float), `edd60` (u32),
  `ede00`, `edea0` (short) and `edf90` (byte) all take `(ctx, field)` and read
  a **typed field record**: an int at `field+8` is the type tag; when it is
  positive the value resolves dynamically through the ctx tables (`ctx+0x98`,
  `ctx+0xa0` → `+0x50`, cached at `ctx+0xa8` when the byte at `ctx+0xe2` is
  set, via `FUN_7ff7c689dfa0`), otherwise the raw dword at `field+0x10` is
  returned. Every `src+NNN` in the material path is therefore a *field* in a
  bag, not a plain struct member — the same shape as the statescript
  config-var records, and it explains why the same reader family appears in
  both systems.
* **The shipped values are not the runtime records.** The applier's
  value-record form (self-relative dword offset to a u16) does **not** hold
  for the file's u64 array: a probe over 3,796 models and 9,049 shipped values
  (`dev/mdlc_probe/examples/u64role.rs`) finds only **15 values (0.17%)** whose
  low or high dword lands a u16 inside the MDLC payload — and all of them are
  the zero value. So the runtime record arrays at `model+0x48`/`+0x58` are
  **built at load time**, and the shipped u64 value is the *loader's input*,
  not the record. (The probe also re-confirms the file-side shape: the values
  are shared across models and are not GUIDs, asset ids or offsets.)

So the per-material **parameters** the renderer uses come from the runtime
record arrays, addressed by keys taken from the material's descriptor (itself
resolved from field-bag reads); the shipped u64 values remain opaque ids whose
*conversion* is the loader step still to read.

## The texture payload, decoded (2026-10-15)

The last hop of the chain is now readable *and* decodable: `ow-tank::texture`
parses a texture asset and turns its blocks into RGBA8, with synthetic-block
tests, and `ow-tank-tex` dumps one to a PNG or censuses the install.

**The texture asset.** 32-byte header — `u16 flags`, `u8 mip_count`, `u8 format`
(a DXGI id, stored one higher from 26 up, so the byte is decremented first:
"the ow2 hack" the TankLib reference records), `u8 surfaces`, `u8 usage`,
`u8 payload_count`, `u8 unknown`, `u16 width`, `u16 height`, `u16 data_size`,
then two `u64`s. Formats seen in a 400-asset census: **BC1** (`byte 72/73`, 187
assets), **BC4** (`81`, 96), **BC5** (`84`, 56), **R8G8B8A8** (`29/30`, 5) and
**BC7** (`100`, 39). Decoded: **344 of 400**; the 56 failures are all BC7, which
the module recognises *by id and refuses* rather than mis-decoding — so a BC7
decoder is the one format piece outstanding.

**The payload chain is a chain of two or three.** The blocks are not always at
`+0x20`:

* `payload_count == 0` (or a payload whose `image_size` covers level 0) → the
  blocks follow the 32-byte header;
* otherwise payload 0 is *embedded* at `+0x20` (a four-`u32` header: `mips`,
  `surfaces`, `image_size`, `header_size`) and it can hold **only the small
  mips**: measured, a 512×512 BC5 texture embeds 21840 B = the sum of its mips
  2..7, with levels 0..1 in a *separate* asset;
* the separate payload asset is found by GUID: type `0x04D`, and the payload
  index lands in the locale nibble (`guid::payload_of`) — verified against the
  registry: for `0x0c0000000000007f` it resolves to `0x032000010000007f`, 262160
  bytes = a 512×512 BC5 level (262144) plus the payload header. Chains of three
  are real (1024×1024 BC5), so the reader tries payloads 1..3 and keeps the
  first whose block run covers level 0.

**What a decoded texture looks like.** `0x0c00000000000003` (1024×32, RGBA8)
decodes to a **hue-gradient strip atlas** — the tint ramp a team-colour pass
samples — and `0x0c0000000000000a` (8×8 BC1) to a flat near-white thumbnail.
Both were written as PNGs and re-decoded independently (chunk CRCs and the
inflated scanlines were checked outside this tree, which is how a CRC that
covered the body but not the chunk type was caught).

```text
cargo run -p ow-tank --bin tex -- --census 400
cargo run -p ow-tank --bin tex -- --guid 0x0c00000000000003 --png D:/ow-dumps/tex.png
```

**Still open for materials:** the *model → material* binding. This round adds one
negative and one route: the model's own STU (the `MSTU` chunk, an STU blob
behind an (offset, size) header — 479 instances for the character model) names
**no** material (008), material-data (0B3) or texture (004) asset, so the
MDLC u64 values are not explained by it either; and the *map* side has a route
the MDLC focus missed — `Placeable.look`, "the model's look/variant", which is
how a placed prop's material set is chosen. The hero side's texture references
exist directly on the entity (two type-004 assets in one component), so a first
textured hero does not need the MDLC join to be solved.

## Where the unknown sits

1. **What the u64 value *is*.** The client-side consumption is read (above):
   the u64 array is indexed by indices resolved through the `+0x88` table and
   clamped to the array's count. What the values are *not* is now measured, not
   guessed — the (008)-id hypothesis is **falsified**:

   * **They are not asset GUIDs.** Decoding each value with the project's GUID
     type/index split gives random type fields (0x5fd…0xe8a across the first
     models) and **0 of 52,626 values resolve as assets** (`dev/mdlc_probe/
     examples/join.rs`).
   * **No (008) asset carries them as 8-byte ids.** Building the set of every
     8-byte value in every locally available material asset (819 of the
     manifest's 99,392 type-008 assets; 1,246 distinct qwords — GUID-shaped
     refs like `0x04d0…`, `0x0e10…`, `0x0210…`, `0x09c…`, plus zeros) and
     testing all 52,626 model values across 19,055 models yields **60 hits,
     all of them the value `0x0000000000000000`** (empty slots); the reverse
     direction finds no non-zero overlap (`dev/mdlc_probe/examples/join2.rs`).
   * The values are shared and stable (6,598 distinct across 52,626 values —
     an ~8× reuse factor), so they live in a shared id space, but one that is
     **not** the material-asset id space reachable from files.

   Remaining hypotheses, in order of cost: a per-material hash consumed by the
   renderer's pipeline selection (the value is only ever used as an array
   index downstream, so its bits may matter only as identity); or a build-local
   id resolved through a table that ships outside the assets read so far.

Once the id's role is settled, the rest of the chain is already readable from
files: material (008) gives shader group/source and material data (0B3), and
STUMaterialData gives texture references (004).

## The chain, closed: the u64 values are the *look's* material ids (2026-10-15)

The id question above is answered, and the answer was on the map side all
along. A model's `MDLC` u64 values are **not** asset GUIDs and never were: they
are the ids the *placement's look* uses to name its materials.

**1. Every model placement carries a second reference.** A `--refs` scan
(`ow-tank-look --refs 60`) walks every 8-byte-aligned qword of every model
placeable in 41 map chunks and asks the registry whether it resolves. Exactly
**two offsets hit on all 16,357** model placeables:

| offset in the placeable body | value | type |
|---|---|---|
| `+0x00` | the model | `0x00C` |
| `+0x20` | the model's **look** | `0x01A` |

so `Placeable.look` (declared in `mapchunk.rs` but never filled) is read at
`body + 0x20`. Every one of the 14,093 placements in a 40-chunk census names a
look, and all 8,331 distinct looks resolve. The reference library calls the same
field `ModelLook` (`DataTool/FindLogic/Combo.cs`, `teMapChunk.cs`), which is
where the name and the type id come from.

**2. A look is a `STUModelLook`, and it lists materials.** Type hash
`0x5207484B` — taken from the reference and then *found in the bytes* of a real
look (`ow-tank-look --walk 0x0980000000000156` prints `[0] type 0x5207484b`)
before anything was built on it. Its first field, `0xBAFDAFBA`, is an inline
array of `STUModelMaterial` (`0x494B66C1`), 8-byte fields inside a 20-byte
element:

| element field | meaning |
|---|---|
| `0x33E51FDC` | `m_material` — a `teStructuredDataAssetRef`, the material asset |
| `0xDC05EA3B` | `MaterialID` — the id the *model* names it by |

The asset ref is read **GUID first** in this position. (The reference's
`Deserialize_Array` reads padding first, but that path is for arrays whose
elements *are* refs; here the elements are instances, so this field goes through
the ordinary reader. The bytes settle it: the first qword decodes to type
`0x008` with a small index, the second to no type in the table at all —
`ow-tank-look --look 0x0980000000000c59` prints both halves and both decoded
types.)

**3. The join is the model's value against the look's id.** Measured over 400
placements (`ow-tank-look --join 400`):

```text
400 placement(s): 381 model read, 62 look read, 0 look with no materials
  model material ids matching the look's id AT THE SAME INDEX: 60
  matching ANY of the look's ids: 84
```

Every model material id (84 of 84) appears among its look's ids. The 381-vs-62
gap is asset readability, not the join (below). This is the number that was
never obtainable from the model alone, and it retires the "build-local id in a
table outside the assets" branch: the table is the look, it ships in the map
chunk's own asset, and the ids are values like `0x6c39d35e401e3b9d`.

**4. Material → textures, from the reference's own structures.** A material
(`0x008`) is a plain 32-byte header, four GUIDs, *not* a STU:

| offset | reference | type |
|---|---|---|
| `+0x00` | shader group | `0x085` |
| `+0x08` | shader source | `0x088` |
| `+0x10` | — | `0x03A` |
| `+0x18` | **material data** | `0x0B3` |

The material data is a header then a texture array: texture offset `i64` at
`+16`, texture count as the byte at `+60`, and each record a texture GUID
(`0x004`) followed by a CRC32 of the input name and a flag byte.

`ow-tank::material` implements 2-4 (`materials_of_look`, `textures_of_material`,
`textures_of_material_data`, `base_texture_of`) with tests; `ow-tank-look`
probes it (`--census`, `--refs`, `--pairs`, `--join`, `--walk`, `--look`,
`--mdlc`, `--probe-type`, `--keyring`).

### The chain stops at the material asset, and that is a data limit, not a gap

The join above is proven and the material assets it names are identified — but
they cannot be read. Sampling **across** each type (a prefix sample is a sample
of one package, which is what made an earlier census read as "all failures are
BC7"):

| type | what it is | readable in this install |
|---|---|---|
| `0x004` | texture | **293 / 300** |
| `0x00C` | model | 278 / 300 |
| `0x01A` | look | 57 / 300 |
| `0x0B3` | material data | 53 / 300 |
| `0x008` | **material** | **16 / 2000** |

The failures are `key … is not present in this installation`. The install's
keyring holds **327 entries**; the community keyring the reference checkout
ships (`TankLib/Overwatch.keyring`, 312 entries) adds nothing measurable —
pointing `OW_SUPPORT_KEYRING` at an empty file changes no census. So the missing
keys are not somewhere else on this machine: **the material assets this map
needs are not decryptable from this install**, and the route that reaches
textures through them is read but not usable here.

### The hero route does not need materials at all

A hero's textures are named **directly on the hero asset**, so a first textured
model does not depend on the chain above. On hero index 3 (the `--play`
default) two STU instances carry a `0x004` reference each:

| instance type | field | texture |
|---|---|---|
| `0x238ef37e` | `0x6f1ecb16` | `0x0c00000000038c3d` |
| `0xfb366254` | `0x3cd6dc1e` | `0x0c00000000007df7` |

Both resolve and both are **BC7**, which is the one format the decoder does not
have yet — so the hero's first texture is one decoder away, not one key away.

### The hero route, measured (2026-10-15)

The same chain reaches a hero: the gameplay entity's `STUModelComponent`
(`0x191835F9`) carries **`0x9297ADB4`**, a `STUModelLook` (type `0x01A`) — read
from hero 3 (`0x0400000000000001` → `0x098000000000028D`), and now read
programmatically by `ow_tank::entity::hero_parts`. That look names 8 materials
and their ids, and the join holds: the model's `MDLC` ids match the look's ids
(9 submeshes over 8 materials; the body's main submesh and one small one share
`0x1F434DE703EA0BE4`).

**But this install blocks most of it.** Of those 8 materials:

| readable | what it gives |
|---|---|
| 3 | texture lists, but **no base colour**: `0x0C000000000027ED` (BC5, mean RGB 126,126,0 — a normal/mask map), `0x0C0000000000039C` (BC7, mean RGB 0,0,0 — an alpha/mask map), `0x0C00000000005449` (BC1, mean RGB 35,34,35 — dark grey) |
| 5 | `key … is not present in this installation`, including the material behind the body's 19,596-triangle submesh |

The base-colour rule the harness uses is "the material's first **sRGB**
texture" (colour data is stored sRGB, data maps are not — `ow_tank::material::
is_srgb_format`), plus "and it must not be a flat fill" (per-channel standard
deviation < 2 rejected): painting a normal map or a solid-black mask as albedo
looks like a texture while being wrong — it put a black patch on each forearm
plate for one round. On this install that leaves the hero entirely on the
placeholder: **0 of 9 submeshes** have a readable base colour.

So `ow-view --hero 3 --hero-only` draws 3 of the model's 9 submeshes with their
own texture and 6 on the 1×1 white placeholder, and prints exactly that list.
The two textures the *hero asset* names directly (`0x238EF37E`'s
`0x0C00000000038C3D` and `0xFB366254`'s `0x0C00000000007DF7`) are the face and
portrait images — UI assets, not body textures, which is a mistake this round
made and corrected.

Reproduce:

```text
cargo run -p ow-tank --bin model  -- --hero 3              # per-submesh material ids and UV ranges
cargo run -p ow-tank --bin look   -- --hex 0x0e00000000000aab --len 0x30   # a readable material's four GUIDs
cargo run -p ow-tank --bin look   -- --hex 0x04d0000000003e03 --len 0x60   # its material data: 4 texture records
cargo run -p ow-view --release    -- --hero 3 --hero-only --shot out.png
```

## The retail manifest opens, and heroes render textured (2026-10-15, third pass)

Two corrections, one of them ours and large:

1. **Our reader was reading the wrong manifest.** Build 154088's content-manifest
   key derivation was not implemented here (only 150851's), so
   `Win_SPWin_RDEV_EExt.cmf` — the plain retail manifest whose content matches
   what is installed — could not be opened, and we fell back to
   `WinPrism_SPWin_RDEV_EExt.cmf`, which ships **in the clear** but carries a
   different content set. The derivation is published in the reference as
   `TACLib/Core/Product/Tank/CMF/ProCMF_154088.cs` (a 512-byte table, a key walk
   from `length * buildVersion` stepping by `(buildVersion * dataCount) % 7`, and
   an IV seeded from `digest[7] * table[0]`), and is now implemented: all three
   plain manifests open. `ow-tank-look --keyring` prints the reasons it could not
   before, so this cannot regress quietly.
2. **The base-colour rule, twice corrected by measurement.** A material's
   texture list holds several maps: base colour is its first **sRGB** entry
   (`is_srgb_format`), and it must have structure in **at least two channels**.
   Each rejection came from a texture that would otherwise have been painted:

   | texture | why it is not albedo |
   |---|---|
   | BC5 128×128, mean (126, 126, 0) | two-channel: a normal/mask pair |
   | BC7 256×256, mean (0, 0, 0), sd 0.0 | a flat fill (alpha mask) |
   | BC1 128×128, mean (35, 34, 35), sd 0.0 | a flat fill — this painted the black forearm patch |
   | BC1 512×512, mean (0, 0, 0), sd (8.2, 0.0, 0.0) | one channel: a mask (this painted Reaper black-with-red) |

**What a hero's coverage looks like now** (`ow-tank-hero --textures --heroes 40`):

| hero | materials | readable | with a real colour map | submeshes |
|---|---|---|---|---|
| 2 (Reaper) | 1 | 1 | **0** | 1 |
| 3 (Tracer) | 8 | 3 | **0** | 9 |
| 4 | 7 | 4 | 3 | 7 |
| 7 (Reinhardt) | 3 | 2 | 2 | 2 |
| 8 | 7 | 4 | 3 | 8 |
| 9 | 7 | 4 | 3 | 9 |
| 10 | 6 | 3 | 2 | 6 |
| 21 | 9 | 1 | 1 | 18 |
| 22 | 7 | 3 | 2 | 7 |
| 32 | 2 | 2 | 2 | 2 |

`ow-view --hero 7 --hero-only` draws Reinhardt **fully painted** from his own
4096×4096 BC7 colour map — the first hero whose whole body is textured from the
install.

**Why the rest are blank is *not* a keyring problem**, and `ow-tank-look --why
0xGUID` says so step by step: the asset is in the root manifest, but its content
key has **no encoding entry** in the manifests this install ships, so its file is
not part of the local install (the client streams it from the CDN). A texture we
*can* read resolves all the way to `archive data.052 at 0x….` — the same chain,
one step further.

### Routes to the textures this install does not have locally

* **The model's own `MSTU` structured data** (readable, no keys): its field
  hashes would de-obfuscate the texture references *inside the model*, skipping
  the material wrapper entirely. This is the install-local route and the next
  planned read.
* **A capture with the hero rendered** (the operator's call, `AGENTS.md` rule 7):
  the client's memory then holds the decrypted material data — the project's
  established method, and the same shape as the movement captures.
* **Downloading the missing content**, which is what the client itself does.

## How the reference finds content, and the two layers we were missing (2026-10-15, fourth pass)

The operator's question — *can we learn how OWLib does it?* — had a concrete
answer, and both layers were ours:

1. **A build-config line is `<content key> <encoding key>`.** TACTLib's
   `BuildConfig.GetFileRecord` reads `vals[0]` into `ContentKey` and `vals[1]`
   into `EncodingKey`. We read *both* as encoding keys, so one half of the
   encoding table always failed to open (it is found by content key, through the
   table itself) and half the build's content keys looked "not in this
   installation". `ensure_encoding` now tries each value in both roles and
   iterates, so the halves resolve each other.
2. **The modern per-archive indexes exist and we were not reading them.**
   `indices/<archive-hash>.index` (the reference's `CDNIndexHandler.OpenIndexFile`
   → `ParseIndex`: 12-byte footer at `len − checksumSize − 12`, pages of
   `[ekey 16][size u32 BE][offset 4–6 B BE]`, per-page last keys and checksums)
   names files the sixteen legacy bucket tables do not. Measured on this install:

   | source | keys |
   |---|---|
   | `data/*.idx` bucket tables | 1,147,893 |
   | `indices/*.index` per-archive indexes | 3,684,920 |
   | **new keys the buckets did not have** | **2,559,788** |

   `data.*.idx` in `ecache/` are read too (config content). **The archive number
   cannot be read off the CDN's `archives` list**: it lists 826 archives and this
   install has 93 local `data.000…092` files, so an index file's local number is
   *learned* from entries the bucket tables already place, and bucket entries win
   when both know a key. (Trusting the list order mapped every location to the
   wrong archive and took material readability from 9 of 600 to **0 of 600** —
   the regression that proved the numbering assumption wrong.)

**What this does and does not unlock.** The coverage of assets we could already
read is unchanged (textures 270 of 300 in a spread sample, materials 9 of 600),
and the 2.56M new keys are what a *client-downloaded* file would be found
through — so content the client streams and caches is now visible to this reader,
where before it would have been invisible even if present. For the assets that
still fail, the wall is measured and not ours: their **content key is absent from
the encoding manifest this install ships** (a raw byte search of the decoded
111 MB table, not a lookup miss), and the build's *other* encoding half
(`5c98ebc3…`) resolves neither by encoding key nor by content key locally — those
files are streamed.

## Why materials are ~1% readable while textures are ~98% (2026-10-15, fifth pass)

The operator asked exactly this. The answer is measured, and it is **not** a
keyring problem and **not** our lookup — the files are not in this install.

**The per-type picture** (spread samples across each whole type, so this is a
sample of the type and not of one package):

| type | what | in the registry | readable here |
|---|---|---|---|
| `0x075` | hero | 135 | **135 / 135** |
| `0x004` | texture | 205,512 | **293 / 300** |
| `0x00C` | model | 32,280 | **278 / 300** |
| `0x058` | statescript graph | 56 | 45 / 56 |
| `0x01A` | look | 43,139 | 57 / 300 |
| `0x0B3` | material data | 121,027 | 52 / 300 |
| `0x008` | **material** | 99,392 | **2 / 300** (≈1%; the count wobbles 0–20 per sample because the base rate is that low) |
| `0x078` | (see `animation.md`) | 5,117 | **0 / 300** |

So it is a **content split**, not a defect: geometry, textures, heroes and
graphs are installed, while the material/look/material-data family is fetched by
the client on demand.

**Every storage mechanism a modern CASC install has was checked** for one of
those materials (`0x0e000000000000e3`, Tracer's body material, declared 144 B):

| checked | result |
|---|---|
| sixteen bucket tables (`data/*.idx`) | key not present |
| per-archive indexes (`indices/*.index`) | key not present |
| `ecache` bucket tables | key not present |
| `shmem` writable layer (`data/`, `ecache/`, and `pro/`, all three) | **the file exists and is 20 KB of zeros** — the client has never written anything here |
| loose files | none exist in this install |
| install and download manifests | content key absent |
| the decoded encoding manifest (111 MB) | **absent by raw byte search**, so this is not a lookup miss |
| the encoding manifest's *other* key role (content key) | could not be opened either |
| all three region manifests (RDEV / RCN / RKR) | 1-2 of 200 readable in each |
| sharing: does any *other* asset use this content key? | no — it is unique to this asset |
| a second install on this machine | none |

**In memory it is not the on-disk shape.** A captured heap was scanned for the
material's on-disk signature (four GUIDs of types
`0x085`, `0x088`, `0x03A`, `0x0B3`) and finds **0 of 208 MB** in
`module_heap.bin`: at runtime a material is a structure of pointers, not the
packed GUID record, so recovering it from a capture means decoding the runtime
structure — a bigger read than a byte scan.

**What this leaves, in order of cost:**

1. **Observe the client's own fetch.** The `shmem` files are the write layer for
   streamed content and they are empty today; running the game with a hero and
   re-measuring (`ow-tank-look --index-stats`, then the per-type census) would
   say whether the client persists what it streams on this machine. The
   operator's call, since it runs the client.
2. **Fetch the content from Blizzard's CDN** — what the launcher does; also the
   operator's call.
3. **The runtime read** from a capture with the hero rendered (pointer graph, not
   a byte pattern).

Meanwhile the code path needs nothing: eight heroes' materials *are* here (see
the coverage table above), and those render fully painted.

### The `0xD4` chunk trees, and what their payloads hold (2026-10-16)

The four `0x0D4` references Tracer's hero names were candidates for *skins*
once, and two of them point straight at textures — that part of the earlier
finding stands. What changed is what those references are, and what can be
read from them. Every check below was read from the install.

All **332 installed type-`0xD4` assets** were enumerated (`anim --list
0x0d4`). 32 cannot be loaded at all — their content key is absent from this
install, exactly the streamed-content wall documented above — and 300 load.
Every one of the 300 parses as a chunk tree (`skel --guid`, no failures). Each
tree is `EFCT`/`ECMP` and nested under it, not an STU: the first skin examined,
`0x0cb0000000000109`, holds 51 chunks whose tags run `EFCT`, `ECMP`, `ECTr`,
`ECCN`, `ESHT`, and so on — a nested chunk container, not an STU. Whether these
trees are a hero's *skins* or its *ability* effects is not decided here; no
cosmetic list names them anywhere.

The payloads are not readable — in the skin examined (`0x0cb0000000000109`), neither the `EFCT` chunk nor an `ECMP` chunk
parses as an STU blob (`skel --guid --stu`, both refuse), so the route to a
*skin's* own look is still missing — the chunk tree enumerates the skin's
shape but not what it references. Two `0x0D4` references still point directly
at textures (`0x0c000000000026b`, `0x0c00000000006f33`, both readable) — an
effect pair, which is why they are not skin content.

What is certain for Tracer: **no fully textured variant on this install**,
because her look's materials are not here (5 absent, 1 data-map only, 2 flat
fills).

### The join is now a function, with an ambiguity rule (2026-10-18)

The MDLC slot value → look id → texture list walk the scene already did by
hand is now `ow_tank::material::base_texture_of_id`, and the two loose ends
are closed (*install*):

* **Positional lookup uses the sRGB rule too.** `base_texture_of` (by slot
  index) used to return the material's *first* texture; it now returns the
  base colour (`base_texture_of_material`), so the two call shapes cannot
  disagree about what "the texture" is.
* **An id naming two materials is declined.** `material_of_id` returns the
  material only when exactly one look entry carries the id; zero matches and
  two-or-more both give `None`, and the scene leaves those submeshes on the
  placeholder and names the id in its report. Measured on Tracer's look
  (`0x098000000000028d`, 8 entries against 9 model ids with one id shared by
  two submeshes): 0 missing, 0 ambiguous.

The two materials whose lists *do* hold sRGB entries (`0x0e…e4` →
`0x0c…039c`, `0x0e…23bd` → `0x0c…05449`) still do not paint: both textures
are flat fills (sd 0.0), and the scene's flat-fill rule keeps them on the
placeholder by design — a normal map or a black patch rendered as albedo
*looks* like a texture while being wrong. So the honest statement stands:
the wiring is complete and the wall is the keyring, not the code.

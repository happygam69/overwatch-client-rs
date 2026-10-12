# The material chain, as read from the files and from the client

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
| permutation + constant | derive constant from value 1 against all 99,392 material GUIDs, require values 2 and 3 to land (`ow-probe resolve-material --values`) | 71 permutations x 3 candidate types: no |
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

## Where the unknown sits

1. **What turns a material GUID into the 64-bit MDLC value** (export-time
   hash/transform) or **what turns the value back** (load-time). Families
   ruled out are listed above, now including the pointer-decode idiom. The
   code to read next: the global material table builder (the `+0x88` store
   that `FUN_7ff64ba2e360` reads through the
   `*( *( *(obj+0x18) + 8 ) + 0x88)` chain), the count-writer cluster
   (`0x7ff64c686447`, `0x7ff64c6a1879`, `0x7ff64c6b445b`, `0x7ff64c6c88af`),
   and `0x7ff64b7f23a0`.
2. **Whether the u32 key is exactly `low32` of the u64 value** — strong
   evidence in both reader sites; needs one confirmation where both halves are
   visible at once (the table builder will show it).
3. **How the model's u64 array reaches textures at all** — the table carries
   colour/parameter records; the material-asset (008) hop may be resolved
   elsewhere (global table) rather than per-model.

Once (1) is answered, the rest of the chain is already readable from files:
material (008) gives shader group/source and material data (0B3), and
STUMaterialData gives texture references (004).

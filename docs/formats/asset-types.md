# Asset types: the client's own registry

`ow-tank` reads assets: models, materials, textures, clips, sounds. The
client loads them too, and it says so in one table. This file records that
table, read from the current image, because it is the shortest way from "I
need the client function for this asset" to "here is the function".

**Evidence class:** read from the image (`dev/data/offline_recover/module_offline.bin`,
base `0x140000000`) and confirmed by the instruction stream of the reader.
Every address below is a current-image address.

## 1. The registry: asset type -> loader job

`JobForType` at **`0x141fd8300`** is the reader. What it does, with the
address of each step:

| step | what it does |
|---|---|
| `0x141fd8307` | rejects a type above `0x192` |
| `0x141fd8312` | takes the job from the u32 type table |
| `0x141fd831c` | returns the qword of the job table |

So there are two tables:

| table | address | shape | where it lives |
|---|---|---|---|
| type -> job | `0x143a7c3a0` | 403 `u32` entries, one per type `0x00..0x192` | `.data`, in the file |
| job -> job object | `0x1441b3cb0` | `u64` entries, indexed by job id | **zero-fill** (BSS): empty in the file, filled at startup |

The second table is the one that holds the client's actual loaders, and it is
not in the file: `.data` declares `VirtualSize 0x0CC43BC` but only
`0x0333600` raw bytes, so RVA `0x03B79600..0x0450A3BC` is zero at rest. The
job ids in the first table are therefore **compile-time constants** that name
runtime slots, and the job object behind a slot is built by a registration
call at startup.

The type -> job half is fully readable from the file:

- 33 distinct job ids are used (`0..39`, with gaps);
- **job 10 serves 194 of the 403 types** - the generic asset job;
- 163 types map to job 0, which is the "no loader" slot;
- the material data (`0x0B3`) is job 28, which is what `formats/materials.md`
  records; the shader group (`0x085`) and shader code (`0x087`) are jobs 23 and
  25, and `0x086` is job 0 - the three facts `formats/shaders.md` cites.

Read it with:

```text
python dev/fn.py jobs          # the whole table
python dev/fn.py find type:0x0b3
```

## 1b. The job table: which function loads which type

**The job id -> loader mapping is read.** `0x141fd89f0` is the call that
fills a job slot, so its callers are the registration functions, and the
descriptor each one passes holds:

| descriptor field | what it is |
|---|---|
| `+0x00` | the asset's (deobfuscated) name |
| `+0x08` | the **job id** - this is what indexes the job table |
| `+0x1C` | a flags word |
| `+0x48` | the **load** callback |
| `+0x78` | the **release** callback |

`0x141fd89f0` reads `descriptor + 8`, refuses the registration when that slot
is already filled (`0x141fd8a04`), and stores the descriptor into the slot
(`0x141fd8a21`). The write is why the job table was invisible to a
displacement scan: the address is computed with a `lea` and the store then
uses **no displacement**, so the table's offset never appears in an
instruction's memory operand.

32 of the 33 jobs are resolved this way; the whole table is

```text
python dev/fn.py jobs            # job, its types, its registration, its loaders
python dev/fn.py jobs --crate ow-tank   # and whether the crate cites them
```

| job | types | load | release |
|---|---|---|---|
| 1 | `0x04`, `0xf1` | `0x141dacf00` | `0x141dad3e0` |
| 2 | `0x08`, `0x65`, `0x127` | `0x141b1a8e0` | `0x141b1a790` |
| 3 | `0x06` | `0x141b24b80` | `0x141b24b00` |
| 4 | `0x27` | **not resolved** | - |
| 5 | `0x50` | `0x141d2f780` | `0x141d2f630` |
| 9 | `0x43` | `0x141d5b680` | `0x1421a3ce0` |
| 10 | 194 types | `0x1421bccc0` | `0x1421bcab0` |
| 12 | `0x0D`, `0x4A`, `0x8E`, `0x8F`, `0x12B` | `0x141ff71b0` | `0x141ff7410` |
| 13 | `0x0D4` | `0x141ff85e0` | `0x141ff8f10` |
| 14 | `0x0B2` | `0x141d5b680` | `0x1421a3ce0` |
| 15 | `0x02` | `0x141ff5c80` (`LoadMapAssetChunks`) | `0x141ff58b0` |
| 17 | `0x3F`, `0x0BB` | `0x141d5b680` | `0x1421a3ce0` |
| 18 | `0x0C`, `0x118` | `0x141b1a960` | `0x141b1a950` |
| 19 | `0x0E` | `0x141d5b680` | `0x141b1a790` |
| 20 | `0x07` | `0x141b1a960` | `0x141b1a950` |
| 22 | `0x10`, `0x7C`, `0xA9` | `0x141b1d600` | - |
| 23 | `0x85`, `0x88` | `0x141e73f30` | `0x141e73eb0` |
| 24 | `0x40` | `0x141e74360` | `0x141e74300` |
| 25 | `0x87` | `0x141e73a80` | `0x141e73a10` |
| 26 | `0x4D` | `0x141dace00` | `0x141dacc40` |
| 27 | `0x9C`, `0xB6`, `0x170` | `0x141d5b680` | - |
| **28** | **`0x0B3`** | **`0x141b1a7b0`** | **`0x141b1a790`** |
| 29 | `0x0BD` | `0x141d5b680` | `0x141b1a790` |
| 30 | `0x0BE` | `0x141d5b680` | `0x141b1a790` |
| 31 | `0x0BC` | `0x141ff54e0` (`FinishMapChunkAsset`) | `0x141ff53e0` |
| 32 | `0x0CB` | `0x141b1a6f0` | `0x141b1a660` |
| 33 | `0x112` | `0x141b1a650` | `0x141b1a640` |
| 34 | `0x63`, `0x122` | `0x1421bd660` | `0x1421bd4b0` |
| 35 | `0x71` | `0x141d5b680` | `0x1421a3ce0` |
| 37 | `0x10C` | `0x141d5b680` | `0x1421a3ce0` |
| 38 | `0x0C`, `0x112`, `0x161` | `0x141b1a9f0` | `0x141b1a970` |
| 39 | `0x181` | `0x141d5b680` | `0x1421a3ce0` |

**This confirms two claims the project already made, from the image rather
than from inference:** the material asset (type `0x008`) is job 2 with the
loader `0x141b1a8e0`, and the material data (type `0x0B3`) is job 28 with the
load callback `0x141b1a7b0` and the finalize callback `0x141b1a790` - the
three addresses `formats/materials.md` cites.

**What is still open:** job 4 (type `0x27`), whose registration stores its job
id in a way this read does not resolve, and the semantics of the descriptor
fields other than `+8`, `+0x48` and `+0x78`. The registration functions also
deobfuscate the asset's **name** (the client hides it with the same
XOR-with-a-key loop `formats/chunk-kinds.md` documents), so the names are
decodable too - the first one, at `0x1439a70a4`, is `MODEL`.

## 1c. The client's own names for its assets

Every registration function **deobfuscates the name of the asset it
registers**, with the same XOR-with-a-key loop `formats/chunk-kinds.md`
documents: byte `i` of the blob is XORed with `key[i & 7]`, the key sits 12
bytes before the blob, and the length is a `u32` masked with `0xffffff` in
the header at `blob - 4`. The name pointer and the asset's **type id** are
then stored next to each other, and the type id is what the type table
indexes - so the names can be read straight out of the image, with no
reference library.

```text
python dev/fn.py names
```

| the client's name | type | job | load | release |
|---|---|---|---|---|
| `MODEL` | `0x00C` | 18 | `0x141b1a960` | `0x141b1a950` |
| `MODELVERTEXDATA` | `0x161` | 38 | `0x141b1a9f0` | `0x141b1a970` |
| `FRACTURABLE` | `0x112` | 33 | `0x141b1a650` | `0x141b1a640` |
| `SKELETON` | `0x007` | 20 | `0x141b1a960` | `0x141b1a950` |
| `MATERIAL` | `0x014` | 0 (no loader) | - | - |
| `MATERIAL DATA` | `0x0B3` | 28 | `0x141b1a7b0` | `0x141b1a790` |
| `TXTR` | `0x0F1` | 1 | `0x141dacf00` | `0x141dad3e0` |
| `TXTR_PAYLOAD` | `0x04D` | 26 | `0x141dace00` | `0x141dacc40` |
| `LIGHTINGDATA` | `0x00E` | 19 | `0x141d5b680` | `0x141b1a790` |
| `LIGHTINGMANIFEST` | `0x0BD` | 29 | `0x141d5b680` | `0x141b1a790` |
| `LIGHTINGCHUNK` | `0x0BE` | 30 | `0x141d5b680` | `0x141b1a790` |
| `SHADER GROUP` | **not resolved** | 23 | `0x141e73f30` | `0x141e73eb0` |
| `SHADER REFLECTION DATA` | `0x040` | 24 | `0x141e74360` | `0x141e74300` |
| `SHADER CODE` | `0x087` | 25 | `0x141e73a80` | `0x141e73a10` |
| `MAPDATA` | `0x002` | 15 | `0x141ff5c80` | `0x141ff58b0` |
| `SOUNDBANK` | `0x043` | 9 | `0x141d5b680` | `0x1421a3ce0` |
| `VOICEWEMFILE` | `0x0B2` | 14 | `0x141d5b680` | `0x1421a3ce0` |
| `SOUNDWEMFILE` | `0x0BB` | 17 | `0x141d5b680` | `0x1421a3ce0` |

The same functions register the **config variables** - `SSAO.HBAO_Radius`,
`SSAO.HBAO_Strength`, `SSAO.ForceCinematicQuality` and a dozen more, each
with its own help text ("The actual eye-space AO radius is set to
radius*SceneScale. default = 1.0."). They have no asset type, which is why
the table above does not list them.

**This is the client's own vocabulary**, and it settles two things the crate
had taken from a reference library: `MATERIAL DATA` is the name of type
`0x0B3` (the material data asset of `formats/materials.md`) and `SHADER CODE`
is the name of type `0x087` (the shader code asset of `formats/shaders.md`).

**Still open:** the type id of `SHADER GROUP` and of `STU`. Both are the
same shape, and the reason is measurable: their name records live in the
**zero-fill** region (`0x1441c2240` and `0x1441b1210` are BSS, not file
bytes), and the type id arrives in a register whose value is a runtime global
(`mov r9, qword ptr [rip + …]` from `0x1441b12b0`, also zero at rest). It
is not a static constant in the image, so this read cannot recover it.
Job 23 serves types `0x085` and `0x088`, and `SHADER GROUP` is the only
unregistered name left in that function, but that is an elimination, not a
read - so the tool reports it as unknown rather than guessing.

## 2. The chunk tag: a big-endian u32

The client does not compare chunk tags as strings. It compares a `u32` read
from the chunk header against a **big-endian** immediate: the four bytes
`MRNM` in the file are the value `0x4d524e4d`.

| tag | the comparison | function |
|---|---|---|
| `MDLC` | the immediate `0x4d444c43` at `0x141b5dfe5` | `ModelChunkLoad` `0x141b5df70` |
| `MRNM` | the immediate `0x4d524e4d` at `0x141b5e02c` | `ModelChunkLoad` `0x141b5df70` |
| `MSTU` | the immediate `0x4d535455` at `0x141b5e17e` | `ModelChunkLoad` `0x141b5df70` |
| `MRNM` | the immediate `0x4d524e4d` at `0x141b5e2ff` | `ModelChunkSize` `0x141b5e1e0` |
| `MSTU` | the immediate `0x4d535455` at `0x141b5e306` | `ModelChunkSize` `0x141b5e1e0` |

`ModelChunkLoad` is the **model chunk dispatcher**: it switches on the tag
and hands the payload to the per-format reader. The `0x141b5e300` block is
the same switch again for the size pass.

Find them by tag, without knowing any address:

```text
python dev/fn.py find MRNM
python dev/fn.py find MDLC
```

The tool searches `.text` for the four bytes, then confirms each candidate by
decoding the function that holds it, so a hit is an instruction that really
carries the immediate and not a displacement that happens to match.

## 3. From one format to all its functions

The dispatcher is the head of a cluster: the per-format reader, the fixup and
the size function are all reachable from it. `dev/fn.py scope` walks that
cluster over the whole-image index:

```text
python dev/fn.py scope 0x141b5df70 --depth 2
```

which gives, for the model chunk dispatcher, the 12 functions that implement
it - `MdlcMaterialRead`, `LoadMstuChunk`, `MoclRead`, `MrnmHandler`,
`MsklTableBuild` and the rest - each with its extent, its name where a human
has given one, and whether a differential unit already proves it.

## Verification

```text
python dev/fn.py jobs
python dev/fn.py find MRNM
python dev/fn.py read 0x141b88f10
```

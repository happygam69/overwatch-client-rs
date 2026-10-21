# Maps and streaming: the scene's map-load path, decompiled

## Current state

- **Data: read** - chunk assets, placeable lists and model joins.
- **Code path: read** - the scene map-load routine, GUID resolution, the
  0/1/3/5 state machine and the client's own placeable/chunk-kind
  vocabulary.
- **Open:** the secure struct's obfuscated field map, the loader object's
  internals, and the streaming policy.

**Verify:** `cargo test --release -p ow-tank --test mapchunk_kinds`.


The map *data* path was already read file-side (`ow-tank` reads chunk assets as
placeable lists; the range map was recovered by chunk proximity). This round
adds the **code path**: the scene object's map-load routine, its GUID/asset
resolution, its state machine, and the vocabulary the client itself uses for
placeables and chunk kinds. Coordinates are v2b session; range = v2b −
`0x17AEC0000`, exec = v2b − `0x71820000`.

## The load routine

`FUN_7ff7c6dabbb0` (v2b; range **`0x7ff64beebbb0`**, exec `0x7ff75556bbb0`;
2,091 bytes, read by REA over a two-region minimal PE — `dev/mkpe.py`, sha256
`4212adc1…`) is the map-load step. Its shape, in order:

* **thread/TLS guard** then scene header writes: `+0x458` (a context value),
  **`+0x454` = the request's flags dword** (the field every branch tests),
  `+0x6fc`, `+0x450 = 0`, `+0x715 = 0`, `+0x608 = 0`,
  **`+0x700` = a monotonic timestamp** (double; `t*const − const` — the
  `%9.03f` the load log prints), and **`+0x750` = a lock-incremented serial**
  (`_DAT_7ff64e1444ac`).
* **obfuscated request binding** when `flags & 0x20`: the request struct's
  fields are copied into the scene through a *secure struct* — the scene holds
  a key at `+0x6a8`, the real base is
  `([scene+0x6a8] ^ 0x87567f996cb1c8b)`, and every field lives at that base
  plus its own huge magic displacement (`+0xb459c3a58bc28c62`, `…cba`, `…cd2`,
  `…ce2`, `…cda`, `…cea`, `…cf2`, `…cfa`, `…d6a`, `…d72`, `…caa`, `…d8a`, …).
  One request qword is additionally transformed
  (`(v ^ 0xd2d24c08a64bf18b) + 0x2338e22e43a0d40d`, rotate, add, rotate) before
  storage. This is why the disassembly of the same site reads as a pile of
  XOR/ADD/MOV pairs against two magic constants.
* **the map GUID is validated**: from `request[0]` a 12-bit field is
  reconstructed with the classic swap-mask dance
  (`0xaaaa…/0xcccc…/0xf0f0…`) and accepted only when
  `(((byte)(v >> 0x38) | ((v >> 0x20) & 0xff0000) >> 8) + 1 & 0xfff) == 2` —
  a check on the GUID's type/index nibbles.
* **the map asset is resolved**: the obfuscated struct's `…c52` slot holds a
  raw value; the code builds a GUID as
  **`(value & 0xffffffff) | 0x079000000000000`** — asset **type `0x0790`** —
  looks it up (`func_0x7ff64be906b0`), queries it (`vtable+0x10` against
  descriptor `0x7ff64de76610`), then binds the asset's data into scene
  `+0x878..+0x8a4` (two triples plus a dword pair; the header offsets/box).
  This is the `STUMapHeader`-class asset the file side reads.
* **state machine**: the state lives in the secure struct at `…cc2` and takes
  the values observed in the code — **0** (fail/cleared), **1** (loading,
  after a 0xa0-byte loader object is constructed by `func_0x7ff64bf29130` and
  started by `func_0x7ff64bf291f0(loader, scene)`), **3** (error/missing —
  reached when the map asset does not resolve, when the validation fails, or
  from the diagnostic path `FUN_7ff64bef0340(0x1d)`), and **5** (no map).
  The "Map Load Started/Completed" log lines
  (`"teScene - Map Load %s: 0x%016llX %9.03f"` — Started at range
  `0x7ff64beec309`, Completed at `0x7ff64bf00d5e`) sit exactly on these
  transitions, printing the map GUID and the `+0x700` timestamp.
* **sub-asset kick-off** when `flags & 1` is clear: four globals
  (`_DAT_7ff64e149d50`, `_DAT_7ff64e149a78`, `_DAT_7ff64e149d20`,
  `_DAT_7ff64e1444a8`) are resolved and bound into the secure struct's `…d8a`,
  `…d82`, `…ae2` slots, with 0x18- and 0x28-byte helper objects started
  (`func_0x7ff64bff7390`, `func_0x7ff64bff4530(scene)`) — the map's
  collision / pathing / sound-area sub-loads implied by the chunk vocabulary
  below.
* **arena allocation**: two rounds of `2 × 16` pools of `0x800` bytes with
  `0x100` entries and LOCK-incremented counters, at `scene+0x158..`; then
  `+0x708 = 1`.
* **later stages** (each gated by flags): a navigation/occlusion-shaped object
  when `flags & 0x200` (0x3210 bytes, two vtables `0x7ff64d2d2818` /
  `0x7ff64d2d2850`, `threads*3` field, handles from
  `func_0x7ff64cb8cd70(asset)` stored as `u16` at `+0x31f8`/`+0x31fa`, a
  descriptor with 1.0/1.0 scales and `0x10101` bytes); subsystem inits at
  `scene+0xe0` and `scene+0x100` (with sizes `0x88`/`0x28`); a
  `0x51000000000004d` GUID event; a `0x400`-byte object at `scene+0x828`; a
  `flags & 2` path binding `_DAT_7ff64e149b30` with
  `func_0x7ff64bfc9940(obj, 0x4000, 0x8000)`; and a final
  `func_0x7ff64ba5fc70(0x24, scene)`.

Two engine idioms show up in the same function and are now confirmed in this
system as well: the **name-record decode** (the load log's message string is
decoded in place before the log call) and the **service-locator chain**
(`((_DAT_7ff64d938f78 + K1) ^ K2) + K3` yields an object whose slots `+0x8e8`,
`+0x368/+0x378/+0x380`, `+0x98`, `+0x2d0` are called) — the same obfuscated
pointer idiom `materials.md` documented, here as *global object* access.

## The client's own placeable and chunk vocabulary

The name table (record format `{u8 key[8]; u32 count|flag<<24; chars}`) at v2b
`0x7ff7c87f3c04..0x7ff7c87f4044` carries 54 map names. Placeable kinds:

```text
MAP_PLACEABLE_MODEL_GROUP  MAP_PLACEABLE_LIGHT   MAP_PLACEABLE_ENTITY
MAP_PLACEABLE_OCCLUDER     MAP_PLACEABLE_AREA    MAP_PLACEABLE_SOUND
MAP_PLACEABLE_REFLECTIONPOINT  MAP_PLACEABLE_LABEL  MAP_PLACEABLE_TEXT
MAP_PLACEABLE_EFFECT       MAP_PLACEABLE_FOG     MAP_PLACEABLE_UNKNOWN
teMAP_PLACEABLE_TYPE_FRACTURABLE_CHILD_BREAKABLE
teMAP_PLACEABLE_TYPE_SH_PROXY_VOLUME  teMAP_PLACEABLE_TYPE_DIFFUSE_GI_VOLUME
teMAP_PLACEABLE_TYPE_TRIGGER_VOLUME    MAP_PLACEABLE_DEPLOYMENT_ZONE_VOLUME
```

and chunk kinds: `MAP_CHUNK_COLLISION`, `MAP_CHUNK_PATHING`,
`MAP_CHUNK_SOUNDAREA`, `MAP_CHUNK_DYNAMIC_SPAWN_GROUPS`,
`teMAP_CHUNK_ADDITIVE_COLLISION`, `teMAP_CHUNK_ADDITIVE_PATHING`,
**`teMAP_CHUNK_STREAMING`**. This is the vocabulary the placeholder-file reader
(`ow-tank::mapchunk`) had to *guess* (`Kind::detect`) — the client's own names
for those kinds are now recorded. The names have no code reference in the
capture (they are registry/enum names, like the animation block), so they are
documentation of the vocabulary, not of the loader's switch.

## The file-side placeable kinds, resolved (2026-10-17)

The vocabulary above gives the names; the asset GUID carries the kind. A map
chunk's asset GUID holds the kind id in bits 32..39 (the reference's
`GetChunkKey`); the sweep below groups the real chunks by that byte and parses
each with the declared kind's body reader. Walking each placeable by its
declared size — the reference reader's `teMapChunk.cs` walk — gives, over the
assets sampled by
`cargo test -p ow-tank --release --test mapchunk_kinds -- --nocapture` on
`E:\Overwatch` (build 2.25.0.0.154088):

| id | kind | assets read | placeables | reference field resolves |
|---|---|---|---|---|
| 0x01 | model group | 18 | 6,880 | 15,475/15,475 model refs |
| 0x04 | reflection point | 17 | 1,862 | no GUID fields declared |
| 0x07 | text | 2 | 98 | 294/294 |
| 0x09 | light | 17 | 9,076 | 2,090/2,090 |
| 0x0A | area | 17 | 587 | no GUID fields declared |
| 0x0B | entity | 20 | 39,156 | **0/39,156 resolve** |
| 0x0C | sound | 17 | 2,641 | 2,641/2,641 |
| 0x0D | effect | 15 | 2,734 | 2,734/2,734 |

Two boundaries:

* **No `0x08` bucket exists on the install.** The reference enum's
  `MAP_PLACEABLE_MODEL` (id 0x08) matched no sampled chunk; what an older
  reader reported as a flat "Model" is the **0x01 model group** (a group's
  first entry places its transform near where a single model's would sit,
  which is why the flat read looked right). The 0x02 single-model kind is in
  the reference and handled in code, but no sampled chunk declares it.
* **The entity body is fitted, not proved.** Its 39,156 reference-shaped
  qwords never resolve to an asset, so the field at that offset is *not* a GUID
  — either the layout is different or the values are runtime ids. Treat the
  entity placeable layout as open; the size walk itself is exact (every
  placeable ends where the next begins).

Readers: `ow-tank::mapchunk` (`Kind::from_type_id`, `MapChunk::parse` takes
the declared kind, `detect` keeps the old two-kind heuristic for callers with
no kind); tests and the per-kind numbers: `crates/ow-tank/tests/mapchunk_kinds.rs`.

## What this closes and what it leaves

Closed: the map-load entry, the map GUID's type (`0x0790`), the asset
resolution and header binding, the load state machine's values and log lines,
the arena allocation, and the sub-asset kick-offs by their globals. Left open,
with addresses: the secure struct's field semantics (obfuscated displacements
must be mapped one by one), the loader object's internals (`0x7ff64bf29130` /
`0x7ff64bf291f0`, inside the unanalyzed band), the streaming *policy* (which
chunks load when — `teMAP_CHUNK_STREAMING` names the kind; its consumer is in
the loader), and the map-to-chunk link the file side still recovers by
proximity.

## Method

String sweep for the map vocabulary; RIP-relative scan for the two load-log
call sites; region wrap + REA analysis and pseudocode for the load routine
(`FUN_7ff64beebbb0`), with its disassembly windows read in the Ghidra project
for the flag branches and the state comparisons.

# Effects: the `EFCT` container and its component chunks

An effect is an ordinary chunked asset (`0xF123456F`) whose root chunk is
`EFCT`. Its children are *components*: a model to play, an entity to spawn, a
sound, a nested effect, a ribbon, a particle system, and so on. Each component
is a chunk tag of its own, and the client keeps a fixed vocabulary of them.

Six asset types carry an effect root: **`0x00D`, `0x04A`, `0x08E`, `0x08F`,
`0x0D4`, `0x12B`**. On the reference install that is 66,293 assets
(34,027 + 15,696 + 980 + 15,228 + 332 + 30); 66,289 open and every one of those
parses as `EFCT`. The four that do not open are CASC misses (type `0x08F`), not
layout failures.

Reader: `ow-tank::effect` (`Effect::parse`, `Chunk::model` and the other typed
accessors). Reader tests: `crates/ow-tank/tests/effect_real.rs`.

## The kind table (`0x143610490`)

The client keeps 0x35 records of `{u32 tag, u32 slot}` at `0x143610490`, the
first of them empty (`{0, 0}`), so 0x34 tags have a slot. The slot is what the
runtime record's `+0xb8` carries; a search that finds nothing writes `0x35` (the
record count). The builder
(`FUN_142e4a780`) performs that search inline for every branch, so the table *is*
the client's list of "chunks that become runtime components":

```text
slot 01 EELB   slot 0E ECLE   slot 1B ECEP   slot 28 TCGM
slot 02 ECEE   slot 0F ECRM   slot 1C EcCB   slot 29 TCCO
slot 03 ECPR   slot 10 ECRP   slot 1D PRTR   slot 2A ECSH
slot 04 ECMD   slot 11 ECRB   slot 1E SHBR   slot 2B TCMF
slot 05 ECGP   slot 12 ECEA   slot 1F ECVS   slot 2C TCDF
slot 06 TCCT   slot 13 EcCS   slot 20 ECTL   slot 2D TCMB
slot 07 ECSO   slot 14 ECGS   slot 21 ECTS   slot 2E TCSE
slot 08 ECEF   slot 15 ECES   slot 22 ECDJ   slot 2F BLND
slot 09 ECDL   slot 16 ECEN   slot 23 ECCm   slot 30 ECLF
slot 0A ECRR   slot 17 TCCE   slot 24 ECSQ   slot 31 ECKB
slot 0B ECLT   slot 18 ECME   slot 25 ECI_   slot 32 TCCC
slot 0C ECDF   slot 19 ECFS   slot 26 ECSC   slot 33 CLBN
slot 0D ECRE   slot 1A ECEC   slot 27 TCER   slot 34 CLTN
```

## The builder's prologue, read in full (2026-11-20)

`EffectComponentBuild` (`0x142e4a780`, **9,432 bytes in three `.pdata`
pieces**) is the client's component constructor, and it is the largest function
`ow-tank` cites. Every branch — 31 `if` chains and 6 `switch` cases, covering
the 52 tags the kind table names — runs the same prologue first, then copies its
component's payload. The prologue is what this section records.

Arguments: `param_1` is the chunk context (`+0x08` the chunk's record, `+0x10`
its payload, `+0x18`/`+0x20` two optional array sub-records), `param_3` the
runtime record being built.

### The header copy: record `+0x20..+0x75` into runtime `+0x60..+0xbb`

All offsets below are **payload** offsets (`payload = record + 0x10`), which is
the frame this crate uses.

| runtime | payload | width | | runtime | payload | width |
|---|---|---|---|---|---|---|
| `+0x60` | `+0x10` | 8 | | `+0x98` | `+0x44` | 2 |
| `+0x68` | `+0x18` | 8 | | `+0x9c` | `+0x46` | 2 |
| `+0x70` | `+0x20` | 8 | | `+0x9e` | `+0x48` | 2 |
| `+0x78` | `+0x40` | 4 | | `+0xa0` | `+0x4a` | 2 |
| `+0x7c` | `+0x28` | 4 | | `+0xa2` | `+0x4c` | 2 |
| `+0x80` | `+0x2c` | 4 | | `+0xa4` | `+0x4e` | 2 |
| `+0x84` | `+0x30` | 4 | | `+0xa6` | `+0x50` | 2 |
| `+0x88` | `+0x34` | 4 | | `+0xa8` | `+0x52` | 2 |
| `+0x90` | `+0x38` | 2 | | `+0xaa` | `+0x54` | 2 |
| `+0x92` | `+0x3a` | 2 | | `+0xac` | `+0x56` | 2 |
| `+0x94` | `+0x3c` | 2 | | `+0xae` | `+0x58` | 2 |
| `+0x96` | `+0x3e` | 2 | | `+0xb0` | `+0x5c` | 4 |

then seven bytes and the run count:

| runtime | payload | what |
|---|---|---|
| `+0xb4` | `+0x60` | byte |
| `+0xb5` | `+0x61` | byte |
| `+0xb6` | `+0x66` | **the run count** (see below), copied *and* used |
| `+0xb7` | `+0x62` | byte |
| `+0xb8` | — | the **kind slot**, written by the tag branch |
| `+0xb9` | `+0x63` | byte |
| `+0xba` | `+0x64` | byte |
| `+0xbb` | `+0x65` | byte |

Five payload bytes in that window are **not** copied: `+0x36`, `+0x40..+0x43`,
`+0x5a`, `+0x5b` and `+0x5d..+0x5f`.

This table is why `ow-tank::effect::header::DWORDS` now reads
`[0x40, 0x28, 0x2c, 0x30, 0x34]`; the previous `[0x40, 0x18, 0x1c, 0x20, 0x24]`
mixed the record and payload frames and sat 16 bytes high. `QWORDS`
(`[0x10, 0x18, 0x20]`) was already right; `ARRAYS` was wrong the same way and is
now `[0x00, 0x08]`.

### Two f32 runs converted to Q12

The record carries **two runs of `count` numbers**, at runtime `+0xc0`:

```text
count  = payload[0x66]                        ; the byte copied to runtime +0xb6
run A  = quantize_q12(payload + 0x00, count)  ; into runtime +0xc0
run B  = quantize_q12(payload + 0x08, count)  ; into runtime +0xc0 + 4 * count
runtime +0x50 = runtime +0xc0                 ; pointer to run A
runtime +0x58 = runtime +0xc0 + 4 * count     ; pointer to run B
```

The conversion is the client's `ScaleFloatRunToQ12` (`0x141c6c830`), already
read and verified as `fn/FloatToQ12` (value × 4096, truncated, saturating,
through integer bit manipulation) and shipped as `ow_sim::fixed::quantize_q12`.
`ow-tank` does not run it: nothing in the crate consumes the runtime record yet,
so the port waits for that consumer instead of shipping an unconsumed copy of
the loop. Both runs' sources are the payload's first two qwords, which is what
`header::ARRAYS` names.

**And on this install the runs are empty.** The count byte at `payload + 0x66`
is `0` on **all 7,342 sampled effects** (every asset of the six types, capped
per type), so `+0xc0` is never written with data, both run pointers point at
each other, and the Q12 path is read from the builder but never exercised by
this game's assets. `tests/effect_real.rs` prints the count histogram and
asserts the run windows stay inside the payload either way, so a patch that
starts shipping run data is visible rather than silent.

### The aligned component payload

The prologue occupies `0xc0 + 8 * count` bytes of the runtime record; it then
rounds that up to 16 and stores the result as a **32-bit offset at runtime
`+0x8c`**. Every tag branch writes its fields at `runtime + that offset + N`, so
the component's copied payload is a separate, 16-byte-aligned region of the
record and the header copy above is fixed-size.

### The two optional array sub-records

`param_1 + 0x18` and `param_1 + 0x20` are optional; each is a chunk record whose
payload starts at `+0x10`.

- `+0x18` → `FUN_142e4fbe0(context, sub + 0x10, runtime, &size)`: the array
  sub-chunk handler the crate already names. It advances the record's size (it
  is handed the aligned offset and writes the new one back) and a `0` return
  aborts the build. The same handler is called inside several tag branches
  (`ECLT`, `ECPR`, `ECRM`), which is how a component's variable-length array
  gets its place.
- `+0x20` → a fixed two-run copy: `count = sub[0x20]` into runtime `+0x48`, the
  first run (`4 * count` bytes) from `sub + 0x10` with its pointer at runtime
  `+0x38`, then the second from `sub + 0x18` with its pointer at `+0x40`.

### The kind table, checked against the image

`python` reading `0x143610490` gives 0x35 records of 8 bytes: `{u32 tag, u8
slot, 3 bytes zero}`, and record `i` carries slot `i` (record 0 is all zero).
**The tag is stored as the little-endian dword**, so `.rdata` reads `BLEE` for
the file's `EELB`. All 52 non-empty tags match `ow_tank::effect::KIND_TABLE`
exactly, in order — the transcription is verified, not assumed.

## Payload layouts the client reads

`FUN_142e4a780` builds one runtime record per chunk. Each tag's branch copies a
fixed set of fields out of the chunk record's payload; the record layout is the
destination and the payload offsets below are the sources. `payload = record +
0x10`, so a source offset `0x10 + n` is the payload's byte `n`.

Only fields whose meaning is pinned by the data are named. What pins them is the
GUID type: a `u64` at the same payload offset with one asset type on every
occurrence of the tag is a reference to that type, not an integer. The counts in
the table are the sweep's.

### `ECMD` — model (chunk version 1, payload 0x20)

| offset | width | field | install type |
|---|---|---|---|
| `+0x00` | 8 | the model | `0x00C` on 3,369/3,369 |
| `+0x08` | 8 | the model's look | `0x01A` on 3,369/3,369 |
| `+0x10` | 8 | an animation; zero when none plays | `0x006` on 1,854/1,854 non-zero |
| `+0x18` | 1 | unknown | — |
| `+0x19` | 1 | unknown | — |

The reference reader names the same three references in the same order but
stops at 24 bytes. The payload is 32 here.

### `ECEF` — a nested effect (version 0, payload 0x20)

| offset | width | field | install type |
|---|---|---|---|
| `+0x00` | 8 | the effect to play | `0x00D` on 4,647/4,647 |
| `+0x08..+0x0c` | 1 each | five unknown bytes | — |

### `ECEN` — entity (version 2, payload 0x50)

| offset | width | field | install type |
|---|---|---|---|
| `+0x00` | 8 | the entity | `0x003` on 2,434/2,434 |
| `+0x08` | 8 | the identifier the reference calls `Identifier` | `0x01C` on 1,026/1,026 non-zero |
| `+0x10` | 8 | unknown | `0x0A6` on 84/84 non-zero |
| `+0x18` | 8 | unknown | `0x0CE` on 20/20 non-zero |
| `+0x20` | 8 | unknown | — |
| `+0x28` | 4 | unknown | — |
| `+0x2c..+0x3e` | 2 × 9 | nine unknown words | — |
| `+0x3e..+0x45` | 1 × 7 | seven unknown bytes | — |

### `ECEC` — entity control (version 0, payload 0x40)

| offset | width | field | install type |
|---|---|---|---|
| `+0x00` | 8 | an animation; zero when none | `0x006` on 3,251/3,251 non-zero |
| `+0x08` | 8 | unknown | — |
| `+0x10` | 8 | the identifier (the reference puts it at `+0x08`) | `0x01C` on 8,543/8,543 non-zero |
| `+0x18` | 8 | unknown | — |
| `+0x20` | 4 | unknown | — |
| `+0x24..+0x36` | 2 × 9 | nine unknown words | — |
| `+0x36` | 2 | unknown | — |

The reference's `Action` byte has no counterpart in the current payload: the
client copies no byte after `+0x36`, so its `Action` (Show/PlayAnim) is gone from
the shipping layout.

### `ECSO` — sound (version 0, payload 0x20)

| offset | width | field | install type |
|---|---|---|---|
| `+0x00` | 8 | the sound | `0x02C` on 72,654/72,654 |
| `+0x08` | 8 | unknown | — |
| `+0x10` | 2 | unknown | — |
| `+0x12` | 1 | unknown; the record's `+0x18` bit 3 also feeds this byte | — |

### `ECVS` — voice stimulus (version 1, payload 0x30)

| offset | width | field | install type |
|---|---|---|---|
| `+0x00` | 8 | the voice stimulus | `0x078` on 1,846/1,846 |
| `+0x08` | 4 | unknown | — |
| `+0x0c` | 4 | unknown | — |
| `+0x10` | 4 | unknown | — |
| `+0x14` | 1 | unknown | — |
| `+0x18` | 8 | unknown | — |
| `+0x20` | 8 | unknown | — |

### `ECRR` — ribbon renderer (version 0, payload 0x40)

| offset | width | field | install type |
|---|---|---|---|
| `+0x00` | 8 | a model | `0x00C` on 9,479/9,479 |
| `+0x08` | 4 | unknown | — |
| `+0x0c..+0x1a` | 2 × 7 | unknown words | — |
| `+0x1c..+0x24` | 2 × 4 | unknown words | — |
| `+0x26..+0x38` | 2 × 9 | unknown words | — |
| `+0x38` | 1 | unknown | — |

### The rest

`FUN_142e4a780`'s body names 45 chunk tags. Forty of them are in the kind table;
the other five (`ECCn`, `ECDM`, `ECET`, `ECRF`, `EcCT`) are older spellings or
tags the table does not carry. Twelve kind-table entries are **not** in this
builder — the `TC*` family (`TCCE`, `TCCT`, `TCCC`, `TCER`, `TCGM`, `TCCO`,
`TCDF`, `TCMF`, `TCMB`, `TCSE`) plus `PRTR` and `SHBR` — and this module keeps
them raw.

Some components carry counts and offset arrays; their branches hand off to
`FUN_142e4fbe0` (five arrays: one of `u32` and four of `u8`, count at
`+0x28`) and `FUN_142e50110` (a `u16` array plus a byte array, count at
`+0x51`). The fixup `FUN_142e4e8e0` converts the stored offsets to pointers in
place. No field meaning beyond the seven components above was read, so
`effect.rs` keeps those payloads raw rather than inventing names.

Chunk versions are constant per tag on this install; the module pins the 20
measured ones in `OBSERVED_VERSIONS`. The fixup does compare a version for some
tags before converting an offset, but the tag a compare belongs to was not
recovered reliably enough to publish per-tag gates; the table is marked
**install**, not **read**.

## What the client does not read

Five tags that the reference reader treats as effect chunks have **no read site
in this image**:

| tag | reference name | payload on the install |
|---|---|---|
| `ECMP` | timing/prefix chunk | 112..608 B, version 0 |
| `ECSS` | shader setup | 96..128 B, version 2 |
| `ECAT` | effect category hardpoint | 16 B, version 0 |
| `ESHT` | effect sheet hardpoint | 32 B, version 0 |
| `ECCV` | (not in the reference) | 80..20,208 B |

The evidence: each tag's four-byte id occurs **nowhere** in the image in either
byte order (a byte-pattern search over the whole program), and none of them is in
the kind table or the builder's tag chain. `ECSS` payloads *do* hold the
material/material-data pair at `+0x08`/`+0x10` (types `0x008`/`0x0B3` on
226,056/226,056), and `ECAT`/`ESHT` hold a hardpoint GUID (type `0x03C`) at
`+0x00`, so the bytes are meaningful — they are simply not read by the shipping
client. They stay raw in `effect.rs`.

This matters for the reference comparison: OWLib's `teEffectChunkShaderSetup`,
`teEffectChunkComponent` (timing) and `teEffect_CAT`/`teEffect_SHT` describe tags
the client does not have code for, so no 1:1 implementation of them can exist
from this image.

## The reference's component structs are stale

`TankLib/Chunks/teEffectComponent*.cs` describes the **pre-2.x** payloads. The
sizes measured over the whole install are:

| tag | reference struct | this build |
|---|---|---|
| `ECMD` | 24 B (3 GUIDs) | **32 B** (+2 bytes) |
| `ECEF` | 8 B (1 GUID) | **32 B** (+5 bytes copied) |
| `ECEN` | 16 B (2 GUIDs) | **80 B** |
| `ECEC` | 17 B (2 GUIDs + 1 byte) | **64 B**, identifier moved to `+0x10` |
| `ECSO` | 8 B (1 GUID) | **32 B** |
| `ECVS` | 8 B (1 GUID) | **48 B** |
| `ECRR` | 8 B (1 GUID) | **64 B** |
| `ECPR` | 40 B | **112 B** |
| `ECAT` | 8 B | 16 B |
| `ESHT` | 8 B | 32 B |

The project's old `effect.rs` was a port of those structs, so it read the first
8/24 bytes of each payload and presented them as the whole component. It is
replaced by the client's copies above.

## Verification

```text
cargo test -p ow-tank --lib effect
cargo test -p ow-tank --release --test effect_real -- --nocapture
```

The unit tests build each component at the exact offsets the client copies and
check the accessors, the kind table's round trip and the rejections. The install
test parses the six types, asserts every component's GUID type, the payload
size each component has, and that every measured version matches.

## Open

- The meaning of the unnamed fields in every component (the reference's names
  cannot be carried over: the layout moved).
- The runtime record layout: only the payload side is read, not what each
  destination field means at run time.
- The array sub-chunk handlers (`FUN_142e4fbe0`, `FUN_142e50110`): the count and
  the four/one-byte arrays are copied, but the consumer of the arrays is not
  read.
- The timing/prefix chunks (`ECMP`) and the shader setup (`ECSS`): data only, no
  client reader in this image.
- The effect system's runtime behaviour (spawn, playback, replacement) is out of
  scope here and belongs to `docs/open-questions.md`.

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

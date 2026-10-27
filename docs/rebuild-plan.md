# Rebuild plan: current state

The client is one executable. Its default launch resolves the real practice
range from the operator's install and draws that map's own chunk set. Gameplay
is not implemented yet.

**The rule: 1 to 1 or not at all.** A gameplay feature counts only when the
client's own code or value has been read from the current `Overwatch.exe`, the
Rust implementation follows it operation for operation, and a test proves the
result identical. Anything else stays open.

## Stage status

| stage | state | gate |
|---|---|---|
| 1. CASC primitives against the client | 5 units verified; the rest open | `cli.py casc` |
| 2. build-154504 manifest | derivation and bundle path read and implemented | records GUID-sorted; all headers open |
| 3. map resolver | practice range resolves and renders | 34/34 chunks, exact bounds |
| 4. materials and textures | first pass in the viewer; client renderer not claimed | open |
| 5. the one executable | done | default launch opens the practice range |
| 6. gameplay | next | no movement/combat claim yet |

## Stage 1 - CASC primitives

The oracle is the client's own bytes, emulated; the candidate is the shipped
Rust function. `python dev/verify_harness/cli.py casc` runs only the CASC units
and fails if any is not verified.

| client function | address | Rust | unit |
|---|---|---|---|
| `Salsa20Xor` | `0x1424e32c0` | `ow_casc::blte::salsa20` | `fn/Salsa20Xor` |
| `Salsa20StateInit` | `0x1424e3230` | `ow_casc::blte::salsa20_state_init` | `fn/Salsa20StateInit` |
| `DynamicIndexHash` | `0x142512c60` | `ow_casc::index::hash_index` | `fn/DynamicIndexHash` |
| `DynamicIndexKeyHash` | `0x142513230` | `ow_casc::index::key_hash` | `fn/DynamicIndexKeyHash` |
| `EncodingHeaderParse` | `0x142498920` | `ow_casc::encoding::write_header_fields` | `fn/EncodingHeaderParse` |

All five are bit-exact over pinned and swept inputs, with 100% of the reachable
branch sites and their scored mutants killed. The oracle found and fixed two
real index-hash defects: the zero-length return and the trailing whole
twelve-byte group.

Still open: `DynamicIndexSegmentParse` (`0x14250fcf0`), `DynamicIndexFindEntry`
(`0x1425111a0`), `DynamicIndexDecodeEntries` (`0x1425209a0`),
`BlteFrameHeaderParse` (`0x1424a90e0`), `BlteBlockRead` (`0x1424d61b0`) and
`DynamicContainerRead` (`0x1424f48d0`).

## Stage 2 - the build-154504 manifest

**Done.** The 512-byte table is at `0x1436100c0`; the installed client's
`Overwatch.exe` supplies it at runtime, so no key bytes are vendored.
`CmfKey154504` (`0x142e48a20`) and `CmfIv154504` (`0x142e48700`) derive the
AES-256-CBC key and IV. The encrypted `Win_SPWin_RDEV_EExt.cmf` decrypts to
1,301,598 GUID-sorted records.

The CMF record byte at `+12` decides where a payload lives: bit 7 clear means
the encoded store, bit 7 set means a type-`0x09C` bundle. `GameData::open_asset`
implements that branch. The bundle entry table (`i32 count`, `u8 width`,
GUID-sorted `u64 GUID` + offset) is read from `FUN_141fd5820`
(`0x141fd5820`) and implemented in `ow-tank::bundle`. All 305 type-`0x09F`
map headers open: 42 from the store, 263 from bundles.

Open: the producer of the client's bundle directory blob. Our index scans the
same manifest relation; the client builds the directory in its content
database. This is the one non-1:1 stage in the asset path.

## Stage 3 - the map resolver

**The chain is read and implemented.**

```text
STUMapHeader (type 0x09F, class 0x02514B6A)
  -> paired mode entry + variation
  -> variation's type-0x002 scene map (m_BF231F12)
  -> map asset's stored chunk references
  -> 31 synthesised chunk GUIDs
```

The client computes each synthesised chunk in `FUN_141b32e20`
(`0x141b32e20`):

```text
guid = 0x0DD0000000000000 | (kind << 32) | (map & 0xFFFFFFFF)
```

`FUN_141ff5c80` (`0x141ff5c80`) reads four stored references from the map
asset (`+0x1B8`, the `+0x1B0`/`+0x1B4` list, `+0x1C0`, `+0x1C8`) and then the
31 table slots. The former "second chunk path" is that stored set, not a
separate loader. `ow-tank::mapresolve` implements the whole chain and opens
every chunk through `GameData::open_asset`.

The practice header `0x0790000000000688` resolves by `mapName`
`Maps\TrainingRoom\Training Room - Practice`. Mode
`0x0230000000000018` selects variation 0, scene map
`0x080000000f69002e`: 34 requested chunks, 34 opened, 1,358 placeables
(229 model-group references, 550 entity references), bounds pinned bit-exact
by `tests/map_resolve_real.rs`.

Open: the producer of the level-load GUID lists. The client has no
mode→variation selector: the header's mode/variation field hashes occur only in
the registration functions, and the map arrives as type-`0x002` GUIDs in Jam
message `0x501B`. The practice header has two modes named `Practice Range`
(mode `0x18` and mode `0x5a`); the launch takes the first in header order. That
is a viewer default, not the client's selector. Also open: the streaming
policy, which chunks load when.

## Stage 4 - materials and textures

The viewer resolves model -> look -> material -> texture and reports what it
could not resolve. On the practice range it counts 289 looks, 594 material
entries, 120 decoded textures, 817 textured submesh draws, 55 placeholder
draws, 148 flat fills and 3 materials with no sRGB base colour.

This is not the client's renderer and the stage 4 gate is not claimed. A white
placeholder is a diagnostic, never a silent material.

## Stage 5 - the one executable

**Done.** `cargo run --release -p ow-client` opens the practice range by
header name and mode display name. It draws only the resolver's requested
chunk set; there is no first-N chunk default, proximity cluster or sandbox
default. `--practice-range` is an explicit alias; `--chunks`,
`--chunks-from`, `--chunk-file` and `--cluster` are diagnostics; `--no-map`
is an explicit stand-in course.

Reported launch:

```text
header       0x0790000000000688
mapName      Maps\TrainingRoom\Training Room - Practice
mode         0x0230000000000018  Practice Range
variation    0 of 3
scene map    0x080000000f69002e
chunks       34 requested, 34 opened, 0 unregistered, 0 unopened
geometry     229 distinct models, 2,449 placements, 872 draws
```

## Stage 6 - gameplay

Only after the world is correct:

1. movement: `MoverInvoker` caller -> `MoverAdvance` -> Domino;
2. camera: per-frame look consumer, FP component, pose/spring;
3. animation: reference retarget, Pose2d VM;
4. combat: fire loop, projectile/raycast, damage and pools;
5. collision: step/ledge/slope and a solid map mesh.

Each follows the same rule: read the client expression, implement it operation
for operation, prove it against the client's bytes.

## Verification

```sh
cargo xtask verify                         # fmt, clippy, tests, index, deviations
cargo xtask verify --fast                  # without install-reading tests
python dev/verify_harness/cli.py casc      # the CASC oracle
cargo run --release -p ow-client           # the practice range
cargo run --release -p ow-client -- --list-maps
```

## What is not allowed

- A hand-picked GUID list or proximity seed as a map.
- A placeholder texture passed off as a material.
- A fitted movement rate.
- A manifest accepted without the GUID-sorted check.
- A "close enough" result.

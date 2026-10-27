# Replays (`phl` / `prp`)

What the game records when a highlight is saved, and how much of it can be read
without a Zstd decoder.

This is the game's own container, not the project's simulation trace. A trace
is a test fixture; a replay is a recording of the real client read back from
disk.

## Where the files are

Replays are **not in the install**, and there is **no asset type id** for them.
They live under
`%LOCALAPPDATA%\Blizzard Entertainment\Overwatch\<account>\Highlights\`. A
recursive search of the install finds no `*.prp` and no `Highlights` folder.

Each file is named with a hyphenated GUID and starts with `phl`
("Player HighLight"). The `prp` ("Player RePlay") block sits at the tail, with
a little-endian `int32` block length immediately before its magic. Reader:
`ow-tank/src/replay.rs` (`find_replay`, `Container`).

## The container prefix

`phl`, little-endian, file-relative:

| offset | size | field | check |
|---|---|---|---|
| `+0x00` | 3 | magic `p`,`h`,`l` | first bytes of all 15 files |
| `+0x03` | 1 | format version | 5 in all 15 |
| `+0x04` | 32 | checksum | not verified |
| `+0x24` | 4 | data length | equals `file length − 40` in all 15 |
| `+0x28` | 32 | four unnamed 64-bit fields | per account |
| `+0x48` | 4 | replay version | 457 / 442 / 494 across three builds |
| `+0x4C` | 4 | build number | equals the account's build folder |
| `+0x50` | 8 | player id | equals the account folder name |
| `+0x58` | 4 | flags | `0x1` (top-five) in all 15 |
| `+0x5C` | 8 | map GUID | equals the embedded replay's map, 15/15 |
| `+0x64` | 8 | game-mode GUID | equals the embedded replay's, 15/15 |

`data_length == file length − 40` fixes the prefix at 40 bytes; the two GUID
agreements fix the field order. What lives between `+0x6C` and the `prp` block
is not decoded; the reader finds the block by scanning.

## The `prp` block

Little-endian, block-relative:

| offset | size | field | status |
|---|---|---|---|
| `+0x00` | 3 | magic `p`,`r`,`p` | verified 15/15 |
| `+0x03` | 1 | format version | verified — 11 |
| `+0x04` | 4 | build number | verified |
| `+0x08` | 8 | map GUID | verified, matches the container |
| `+0x10` | 8 | game-mode GUID | verified, matches the container |
| `+0x18` | 1 | unknown 1 | not verified |
| `+0x19` | 4 | unknown 2 | not verified |
| `+0x1d` | 4 | unknown 3 | not verified |
| `+0x21` | 32 | map checksum | not verified |
| `+0x41` | 4 | params block length | reads 0 in all 15 |
| `+0x45` | … | params block | not verified |
| … | 4 + … | highlight info, gated on `unknown 1 & 4` | not verified |
| … | … | Zstd payload | present, not decoded |

The reference's own reader notes that its filler-record walk is sometimes wrong
on these files (`tePlayerHighlight.cs:106-110`), and following its declaration
from `+0x68` does not land on the `prp` block in any of the 15.

## Open

- The frame payload is a Zstd stream; it is not decoded, so positions, damage
  events and tick data are not reachable this way.
- The params block and the highlight info.
- The 32-byte checksum.

## Verification

```text
cargo test --release -p ow-tank --test replay_real
cargo run -p ow-tank --bin replay -- --help
```

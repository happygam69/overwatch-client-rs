# Replays (`prp`)

What the game records when you save a highlight, and how much of it can be read
without a Zstd decoder.

**This is not our simulation trace.** Every other "replay" in this tree means
re-running our own movement/combat model against a recorded input script
(`ow-sim/src/trace.rs`, `docs/rewrite-plan.md` § M0). That is a test fixture we
made. This document is about the game's own container: a recording of the real
client, read back from disk.

## Where the files are, and the asset-type answer

Replays are **not** in the game install, and there is **no asset type id** for
them.

* **installed** — OWLib's asset-type enum (`TankTonka/Models/Common.cs`,
  `AssetRepoType`) has no replay member, going through the whole list from
  `Texture = 0x4` upwards. Nothing in `OWLib` looks for a replay in the content
  manifest either. A recursive search of `E:\Overwatch` found no `*.prp` file
  and no directory named `Highlights`.
* **the reference reads user data** — `DataTool/ToolLogic/List/Misc/ListHighlights.cs:74-79`
  walks `%LOCALAPPDATA%\Blizzard Entertainment\Overwatch\<account>\`, opens
  `<account>\Highlights\` (with a `PTR\` level inserted on the test server) and
  reads every file in it. The reference never lists an install path for
  highlights anywhere.

So: no asset type id, no manifest entry, nothing to resolve. A replay is reached
by path, not by GUID.

> Evidence class: **install** for the search result and the absence of a type id
> in the reference's enum; the path itself is **published-check** in the sense
> that it is the reference's declared behaviour, cross-checked against the
> folders that actually exist on this machine.

### What is on this machine

15 extension-less files across 3 accounts (a fourth account folder has an empty
`Highlights` directory):

| account | files | client build |
|---|---|---|
| `1177677997` | 5 | 136696 |
| `1177835882` | 5 | 136208 |
| `471143481` | 5 | 154088 |

The account number in the folder name equals the `player_id` field inside the
file (verified on 15/15), and the sibling `Hotfix\<n>` folder's number equals
the container's build field (verified on 15/15). That is two independent
cross-checks that these files are what they claim to be.

Each file is named with a hyphenated GUID and **starts with `phl`**, not `prp`:

```text
70 68 6c 05 ....
p  h  l  v
```

`phl` is "Player HighLight" — the highlight container, `tePlayerHighlight.cs`.
The `prp` block ("Player RePlay") sits at the **tail** of each file, with a
little-endian `int32` block length immediately in front of its magic. Reader:
`ow-tank/src/replay.rs` (`find_replay`, `Container`).

## The container's verified prefix

`phl`, little-endian throughout. Offsets are file-relative.

| offset | size | field | how it was checked |
|---|---|---|---|
| `+0x00` | 3 | magic `p`,`h`,`l` | first three bytes of all 15 files |
| `+0x03` | 1 | format version | 5 in all 15 |
| `+0x04` | 32 | checksum | 32 bytes; the value is not verified |
| `+0x24` | 4 | data length | equals `file length − 40` in all 15 |
| `+0x28` | 32 | four unnamed 64-bit fields | values differ per account, same within an account |
| `+0x48` | 4 | replay version | 457 / 442 / 494 across the three builds |
| `+0x4C` | 4 | build number | equals the account's own build folder |
| `+0x50` | 8 | player id | equals the account folder name |
| `+0x58` | 4 | flags | `0x1` (top-five) in all 15 |
| `+0x5C` | 8 | map GUID | equals the embedded replay's map, 15/15 |
| `+0x64` | 8 | game-mode GUID | equals the embedded replay's game mode, 15/15 |

`data_length` equalling `file length − 40` is the load-bearing one: it is what
fixes the header prefix at exactly 40 bytes, and everything above sits inside
those 40 bytes plus the 32 bytes at `+0x28`. The two GUID cross-checks at
`+0x5C`/`+0x64` are what fix the container's own layout rather than the replay's.

> Evidence class: **install** — every row measured on all 15 files of this
> machine's Highlights folders. The field *order* is from the reference's
> `tePlayerHighlight.cs:37-57`; the *offsets* come from the bytes, and the two
> GUID agreements are what confirm the order.

The reference's own reader carries a note that its filler-record walk is
sometimes wrong on these files (`tePlayerHighlight.cs:106-110`, "too many filler
structs read"), and following its declaration from `+0x68` does not land on the
`prp` block in any of the 15. **What lives between `+0x6C` and the `prp` block
is not decoded here** — this reader stops at the verified prefix and finds the
block by scanning. Not tested.

## The `prp` block

Little-endian. Offsets are block-relative, where `+0x00` is the magic.

| offset | size | field | status |
|---|---|---|---|
| `+0x00` | 3 | magic `p`,`r`,`p` | **verified** (15/15) |
| `+0x03` | 1 | format version | **verified** — 11 in all 15 |
| `+0x04` | 4 | build number | **verified** — 136854 / 136238 / 154088 |
| `+0x08` | 8 | map GUID | **verified** — matches the container's, 15/15 |
| `+0x10` | 8 | game-mode GUID | **verified** — matches the container's, 15/15 |
| `+0x18` | 1 | unknown 1 | **not verified** — the flag bit that would gate the highlight block |
| `+0x19` | 4 | unknown 2 | **not verified** |
| `+0x1D` | 4 | unknown 3 | **not verified** |
| `+0x21` | 32 | map checksum | **not verified** |
| `+0x41` | 4 | params block length | **not verified** — reads 0 in all 15 |
| `+0x45` | … | params block | **not verified** |
| … | 4 + … | highlight info, gated on `unknown 1 & 4` | **not verified** |
| … | … | Zstd payload | **verified present**, **not decoded** |

Two things pin the verified rows:

* the build number at `+0x04` is a plausible client build for every file, and
  equals the container's own build for the newest account (154088). For the two
  older accounts the replay's build is *newer* than the container's (136854 vs
  136696, 136238 vs 136208), which is the signature of a container written once
  and the replay re-recorded later, not of a wrong offset.
* the map and game-mode GUIDs at `+0x08`/`+0x10` are identical to the
  container's own two GUIDs in **15 out of 15** files. Two 64-bit values landing
  on the right pair at the right two offsets is not a coincidence.

The block's own length is **verified**: the `int32` immediately in front of the
magic equals the number of bytes that follow it in all 15 files.
`find_replay` checks this rather than trusting it, because a span that runs off
the end of the buffer is how a length field becomes a crash.

> Evidence class: **install** for every verified row, measured on all 15 files.
> **decompiled** is not available for these fields — the reference reader
> (`tePlayerReplay.cs`) is a *declaration*, not a reading of these bytes, and
> where it disagrees with the files the files won, per `docs/provenance.md`.

### What the declared layout gets wrong on these files

`ow-tank/src/replay.rs` implements `tePlayerReplay`'s field order faithfully, and
on these files it **fails** rather than returning a roster. That is deliberate.
The measurements:

* `params_block_length` at `+0x41` reads `0` in **15/15** files. Every plausible
  4-byte read in the `+0x40`…`+0x4F` window is zero in at least one file, so the
  field is not simply misaligned by a small amount.
* walking the declared params block from `+0x45` produces a hero count of
  `-1696756060` or `1479841985` depending on the file — the reader correctly
  refuses it (`NegativeCount`) rather than allocating.
* the distance from the block's start to the Zstd frame is **493, 500, 520 or
  589 bytes** across the 15 files. Under the declared layout that distance must
  equal `4 + 36·heroes + 4·cosmetics (+ 4 + 142 + name_length)` when a highlight
  description is present. `493` fits that shape; `500` and `520` do not, because
  427 and 451 are not multiples of four and would require the highlight
  description — which requires `unknown 1 & 4 != 0`, and `unknown 1` reads `0`
  in every file.
* exhaustive search: for **no** starting offset in `+0x14`…`+0xC8` of any of the
  15 files does the declared params block both parse plausibly and end exactly
  where the Zstd frame begins. Zero of fifteen.

So either the header grew fields this build writes and the reference has not
caught up (the block declares version 11; the container declares 5), or the
field order is different. **Not established.** The three unknown scalars are
where a new field or a changed width would go, and a 45-byte tail
(`+0x18`…`+0x44`) does not fit any reading of these bytes.

Reading the block past `+0x18` is declared, not measured. The code says so at the
call site, `Replay::payload_offset` lets a caller check it against
`first_zstd_frame`, and `docs/rewrite-plan.md`'s "declare it, don't fake it" rule
is why the reader raises an error instead of returning the nonsense numbers.

## The payload: present, sized, not decoded

Everything after the header and params is a **Zstd frame** holding the recorded
game state — where every player was on every tick. It is the part worth having
and the part this crate cannot give you.

* **compressed length: known.** The frame runs to the end of the block, and the
  block's length is fixed by the container. 293 424 bytes for the sample above,
  255 759 to 376 569 across the 15.
* **uncompressed length: not recorded.** The Zstd frame header's content-size
  field is *absent* in 15/15 files — the frame descriptor byte is `0x00`, so the
  field's width is zero. This is a property of what the game wrote, not of this
  reader: no amount of parsing recovers a number that is not in the file.
  `ZstdBlock::uncompressed_len()` returns `None` and says so.
* **decoding: refused.** `ZstdBlock::decode()` returns
  `Error::PayloadNotDecoded { compressed_len, declared_uncompressed_len }`. The
  asset layer carries no Zstd decoder and adding one is out of scope, so the
  honest answer is a named error rather than a fabricated buffer of zeros.

### The parity claim, stated narrowly

OWLib does not decode the payload either. Its `Logical.ZstdBuffer` reader reads
every remaining byte and hands them back **compressed**; the call that would
decompress them is commented out (`TankLib/Helpers/DataSerializer/Logical.cs:196`),
and the decompress helper it kept allocates a fixed 1 MB buffer with the comment
"1MB should be enough for anyone!" (`Logical.cs:207`), which is a second reason
to distrust it. So the parity claim is exactly this: *both implementations stop
at the compressed frame, and both report no uncompressed length.* Neither
implements decompression; nothing here is a claim of having matched a working
decoder, because there is no reference for one to match.

## Open

1. The `+0x18`…`+0x44` header tail. Needs either a newer reference or a reading
   of the client (decompilation, per `AGENTS.md`'s tooling map) to settle.
2. The params block and the hero roster, which follow from (1).
3. The `phl` body between `+0x6C` and the `prp` block: highlight descriptions,
   hero rosters, and a 13-byte "filler" record the reference itself says it
   sometimes over-reads. This reader does not decode it.
4. The replay checksum. The reference computes it as HMAC-SHA256 with a fixed
   32-byte key over an unnamed input (`ReplayChecksum.cs:43-49`). This crate
   carries no SHA-2, so the 32 bytes are exposed and the derivation is not.
   **Not tested.**
5. The highlight's `phl` body holds the highlight's own GUID, and that value is
   the file's name — **verified 15/15**. The stored form is Windows' in-memory
   `Guid` layout, i.e. the first three groups byte-swapped and the last eight
   bytes in order (the same rule [`Uuid::to_hyphenated`] prints back). A plain
   16-byte reversal does **not** match, so a comparison has to use that rule. It
   sits at file offset `169` (12 files) or `177` (3 files), inside the undecoded
   `phl` body and well before the `prp` block. The offset moves with the body's
   length, which is why it is not wired into [`Container`]: there is no fixed
   slot to read it from.

> Evidence class: **install** — measured by byte comparison on all 15 files.

## Reproducing

```text
cargo test -p ow-tank --lib replay                     # synthetic round-trip
cargo test -p ow-tank --test replay_real -- --nocapture # the sweep, prints what it saw
cargo run -p ow-tank --bin replay -- --find            # where highlights live
cargo run -p ow-tank --bin replay -- "<a Highlights folder>"
```

The sweep reads `%LOCALAPPDATA%\Blizzard Entertainment\Overwatch\*\Highlights`,
or `OW_HIGHLIGHTS` if set. It prints one line per file and asserts only what the
bytes support: the container's 40-byte prefix, the block's length prefix, the
map/game-mode agreement, and that a Zstd frame sits inside the block. The
declared layout's behaviour is reported, not asserted.
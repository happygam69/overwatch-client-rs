# Sound and audio assets

Following a sound from the place that plays it to the bytes that carry it, and
reading the header of those bytes. **It stops at the header.** No audio is
decoded and no waveform is produced here.

```text
sound asset 0x02C
  STUSound
    m_C32C2195  ->  STU_0FA0034C  (the Wwise media block)
      m_soundWEMFiles   -> a reference array of STU_FBCC5EB2 wrappers
      m_wwiseWEMFileIDs -> a uint[] running parallel to it
      m_soundBank       -> a type-0x043 asset, a raw Wwise bank (BKHD)
                            |
                            v
                     payload assets, type 0x03F
                       RIFF / WAVE, fmt + data (+ an unexplained `hash` chunk)
                       -> this module parses the header and stops
```

## Where the audio is, measured

**Every type that carries voice audio lives in a second, per-locale manifest
that `GameData::open` does not load.** A census from the base manifest alone
would report the voice layer as empty.

```text
manifest(s): Win_SPWin_RDEV_EExt.cmf, Win_SPWin_RDEV_LenUS_text_EExt.cmf
speech manifest: Win_SPWin_RDEV_LenUS_speech_EExt.cmf  (66,220 records)
```

| type | reference name | base manifest | speech manifest | what it is |
|---|---|---|---|---|
| `0x2C` | Sound | 36,981 | 0 | metadata |
| `0x2D` | SoundSwitch | 6 | 0 | metadata |
| `0x2E` | SoundSwitchGroup | 21 | 0 | metadata |
| `0x2F` | SoundParameter | 305 | 0 | metadata |
| `0x30` | SoundStateGroup | 91 | 0 | metadata |
| `0x31` | SoundState | 345 | 0 | metadata |
| `0x55` | SoundSpace | 1,233 | 0 | metadata |
| `0x5F` | VoiceSet | 0 | 294 | metadata |
| `0x70` | VoiceLineSet | 272 | 0 | metadata |
| `0xA3` | SoundIntegrity | 1 | 0 | metadata |
| `0xB2` | VoiceWEMFile | 0 | 65,921 | audio (voice) |
| `0x43` | *not in the enumeration* | 1,148 | 0 | raw Wwise banks (`BKHD`), container read |
| `0xBB` | *not in the enumeration* | 0 | 2 | audio (music), Wwise Opus |
| `0x3F` | *not in the enumeration* | 65,122 | 0 | audio (sound effects) |

`0x3F` is sound effects and `0xBB` is music: the 500 sampled `0x3F` assets are
Wwise Vorbis at 44.1 or 48 kHz, and both `0xBB` assets are Wwise Opus, one of
them twelve channels long.

A hero's voices are reachable: `STUVoiceSetComponent` (`0x8745618A`) at field
`0x8A4FF89C`; 39 of 39 hero entities read a voice set and all 39 landed in the
speech manifest.

## The install is mostly empty

Blizzard ships a manifest record for every sound the game can play and
downloads only what has been heard. Of 36,981 sound assets, 57 opened. Of
65,122 WEM payloads, all 400 sampled opened, because those are the ones the
heard sounds point at. Zero readable assets for `0x2D`, `0x2F` and `0xA3` is a
measurement, not a reader failure.

The four types with no layout at all stay unread. Dispatch in this layer is on
the root instance class rather than the type id; `0x2E` reads as
`STUSoundSwitch` because its root class is `0x89006ADB`.

## What the rewrite needs

Audio is middleware to **call**, not to reimplement. The faithful path is
Wwise event posting against the shipped soundbanks, not a DSP
reimplementation. The event-posting sites are not read yet; the Wwise SDK and
bank data are present.

## The sound bank container (`0x043`)

The client does not parse a `0x043` asset as an STU. Asset type `0x043` is
registered against job id 9 (`docs/formats/asset-types.md` §1b), and the loader
that job calls hands the bytes to the Wwise middleware the image statically
links. Two functions there are the whole container:

- **`WwiseBankHeaderParse` (`0x1428fc300`, 380 B)** — the `BKHD` section.
- **`WwiseBankLoad` (`0x1428fa3a0`)** — the section loop that follows it.

A bank is a flat sequence of sections: an 8-byte header (`id`, `size`, both
little-endian) and `size` payload bytes. The first section must be `BKHD`
(`0x44484b42`); the loop dispatches on the id and seeks past the payload of
an id it does not handle. The file is expected to tile exactly — the loop
stops on a short read.

### The `BKHD` payload

| offset | width | field | what the client does with it |
|---|---|---|---|
| `0x00` | `u32` | version | the gate below |
| `0x04` | `u32` | bank id | passed to the `HIRC` reader |
| `0x08` | `u32` | language id | passed to `HIRC`/`STID` |
| `0x0c` | `u16` | alignment | the `DATA` handler above version `0x86`; `0x10` at or below. Which `u16` this is was settled by measurement: `0x0e` is zero on every bank here and cannot be a modulus, and every `DIDX` offset would have to divide by it |
| `0x0e` | `u16` | unknown | copied and masked only |
| `0x10` | `u32` | unknown | copied and masked only |
| `0x14` | `u32` | bank type (`>= 0x8f`) | must be `0` or `0x1e..=0x1f`, else status 100 |
| `0x18` | 16 B | unknown (`>= 0x8f`) | copied |

**The first 16 payload bytes are XORed with a 16-byte key table at
`0x144293db8` before any field is read.** That address is in the image's
zero-fill `.data` tail (`docs/ghidra-image-audit.md` §2.1) and has two readers
and no writer, so the key is zero on this install and the XOR is the identity.
The data agrees: all 1,045 readable banks decrypt to version `0x91`.

**Version gate.** `0x76..=0x91` is accepted. `0x76..=0x90` additionally sets a
media-only flag: a section other than `DATA`/`DIDX` is then rejected with
status `0x40`. Versions `<= 0x75` and `>= 0x92` are status `0x40`.

**Section sizes.** The reader consumes `0x14` bytes of the payload below
version `0x8f` and `0x28` from `0x8f` on, then skips the rest of the declared
size. `parse` follows that: the declared payload may be larger than the read
header (one bank here declares `0x30`).

### The sections

The dispatcher names these ids: `DATA`, `DIDX`, `HIRC`, `STID`, `STMG`, `ENVS`,
`PLAT`, `INIT`. Only two are read at the container level here:

- **`DIDX`** is a flat array of 12-byte records — `u32` media id, `u32` offset
  into `DATA`, `u32` size. The client's count is `size / 12` (floor) and it
  reads `count * 12` bytes, so a trailing partial record stays in the stream.
- **`DATA`** holds the media bytes; its alignment is the header `u16` above
  version `0x86` and the literal `0x10` at or below.

`HIRC` (the object graph — events, containers, sounds), `STID`/`STMG` (state),
`ENVS` (environment curves) and `PLAT`/`INIT` have their own readers at
`0x1428fcc40`, `0x1428fd1a0`, `0x1428fc530`, `0x1428fc480` and `0x1428fd020`.
Their bodies are **not read yet**, so the container is the deliverable and the
section payloads stay raw.

### Measured on this install

Every registered `0x043` asset was opened and parsed (`tests/bank_real.rs`):

```text
registered 1,148   opened 1,045   not downloaded 103
version 0x91       all 1,045
BKHD payload       1,044 at 0x28, 1 at 0x30
alignment          0x10 on every bank
sections           BKHD+HIRC 1,044; BKHD+INIT+STMG+HIRC+ENVS+PLAT 1
DIDX/DATA          none — no bank on this install carries media
```

That last line is the important one: this install keeps all audio in the
separate `0x03F`/`0x0BB` payload assets, so the `DATA`/`DIDX` path is exercised
by synthetic units only and the `HIRC` object graph is where a bank's events
would come from.

Reader: `crates/ow-tank/src/wwise_bank.rs`. CLI: `ow-tank-sound bank <guid>`.

## Open

- The Wwise event-posting path and the soundbank event ids. The container is
  read; `HIRC` — the object graph that holds the events — is not.
- The `STID`/`STMG`/`ENVS`/`PLAT`/`INIT` readers (`0x1428fd1a0`,
  `0x1428fc530`, `0x1428fc480`, `0x1428fd020`): headers only, no bodies read.
- Who writes the bank header XOR key at `0x144293db8`, if anything. There is no
  writer in the image, so on this install the key is the zero fill.
- Decoding WEM/Vorbis/Opus into a waveform.
- The four types with no reference layout.
- The per-locale speech manifest is not loaded by the default install reader.

## Verification

```text
cargo run -p ow-tank --bin sound -- types --cap 400
cargo run -p ow-tank --bin sound -- bank 0x042000000001e405
cargo run -p ow-tank --bin sound -- speech
cargo test --release -p ow-tank --test sound_real
cargo test --release -p ow-tank --test bank_real
```

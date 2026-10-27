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
| `0x43` | *not in the enumeration* | 1,148 | 0 | raw Wwise banks (`BKHD`) |
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

## Open

- The Wwise event-posting path and the soundbank event ids.
- Decoding WEM/Vorbis/Opus into a waveform.
- The four types with no reference layout.
- The per-locale speech manifest is not loaded by the default install reader.

## Verification

```text
cargo run -p ow-tank --bin sound -- types --cap 400
cargo run -p ow-tank --bin sound -- speech
cargo test --release -p ow-tank --test sound_real
```

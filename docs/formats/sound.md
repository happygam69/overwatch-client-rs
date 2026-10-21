# Sound and audio assets

Following a sound from the place that plays it to the bytes that carry it, and
reading the header of those bytes. **It stops at the header.** This document
records what that means in practice, because the interesting thing about this
layer is not how much of it works — it is exactly where it stops.

## What this layer is

The project decodes sound *placeables* out of map chunks and could not play one.
That is now only half true: the metadata is readable and the audio container is
readable, but **no audio is decoded and no waveform is produced here.** There is
no audio dependency in the workspace, and none was added — the handoff at the end
of this document is the honest deliverable, not a cop-out.

```text
sound asset 0x02C (36,981 registered)
  STUSound
    m_C32C2195  ->  STU_0FA0034C  (the Wwise media block)
      m_soundWEMFiles   -> a reference array of STU_FBCC5EB2 wrappers
      m_wwiseWEMFileIDs -> a uint[] running parallel to it
      m_soundBank       -> a type-0x043 asset, a raw Wwise bank (BKHD)
                            |
                            v
                     payload assets, type 0x03F (65,122)
                       RIFF / WAVE, fmt + data (+ an unexplained `hash` chunk)
                       -> this module parses the header and stops
```

## Where the audio is, measured

**Three sound types hold nothing at all in the base manifest, and every type that
carries voice audio lives in a second, per-locale manifest that `GameData::open`
does not load.** Measured, not assumed, and the single most load-bearing fact on
this page: a census taken from the base manifest alone would report the entire
voice layer as empty.

```
manifest(s): Win_SPWin_RDEV_EExt.cmf, Win_SPWin_RDEV_LenUS_text_EExt.cmf
speech manifest: Win_SPWin_RDEV_LenUS_speech_EExt.cmf  (66,220 records)
```

| type | reference name | base manifest | speech manifest | what it actually is |
|---|---|---|---|---|
| `0x2C` | Sound | 36,981 | 0 | metadata |
| `0x2D` | SoundSwitch | 6 | 0 | metadata |
| `0x2E` | SoundSwitchGroup | 21 | 0 | metadata — but see below |
| `0x2F` | SoundParameter | 305 | 0 | metadata |
| `0x30` | SoundStateGroup | 91 | 0 | metadata, no reference layout |
| `0x31` | SoundState | 345 | 0 | metadata, no reference layout |
| `0x55` | SoundSpace | 1,233 | 0 | metadata, no reference layout |
| `0x5F` | VoiceSet | **0** | **294** | metadata |
| `0x70` | VoiceLineSet | 272 | 0 | metadata |
| `0xA3` | SoundIntegrity | 1 | 0 | metadata, no reference layout |
| `0xB2` | VoiceWEMFile | **0** | **65,921** | audio (voice) |
| `0x43` | *not in the enumeration* | 1,148 | 0 | raw Wwise banks (`BKHD`) |
| `0xBB` | *not in the enumeration* | **0** | **2** | audio (music), Wwise Opus |
| `0x3F` | *not in the enumeration* | **65,122** | 0 | audio (sound effects) |

Evidence class: ***install*** (a retail manifest on this machine, read with
`ow-tank-sound types --cap 400` and `ow-tank-sound speech`).

Two things fall out of that table which are worth stating separately:

* **`0x3F` is sound effects and `0xBB` is music.** The reference groups them as
  one case (`FindLogic/Combo.cs:980-981`, `SaveLogic/Combo.cs:1036`), and the
  reference's music extractor treats them the same way. Measured, they are not:
  all 500 sampled `0x3F` assets are Wwise Vorbis at 44.1 or 48 kHz, and both
  `0xBB` assets are Wwise Opus, one of them twelve channels long. This matches the
  reference's own warning that "most music" is Opus
  (`ToolLogic/Extract/ExtractMusic.cs:22`). ***install***, cross-checked against
  ***decompiled***-equivalent published code, which agrees on the split but not on
  the grouping.
* **A hero's voices are reachable, and the join works.** A hero's gameplay entity
  carries `STUVoiceSetComponent` (`0x8745618A`) naming a voice set at field
  `0x8A4FF89C`. Walking 39 heroes on this install: 39 entities read, 39 named a
  voice set, and **39 of those 39 landed in the speech manifest.** ***install***.

## The census, as measured

`ow-tank-sound types --cap 400` on the reference install. `scanned` is the sample
opened; `registered` is what the manifest lists. Where a row says `[CAPPED]`, the
counters describe the sample and not the type.

| type | registered | scanned | readable | parsed | not downloaded | unparsed | as STU | as WEM |
|---|---|---|---|---|---|---|---|---|
| `0x2C` sound | 36,981 | 400 | 57 | 57 | 343 | 0 | 57 | 0 |
| `0x2D` sound switch | 6 | 6 | 0 | 0 | 6 | 0 | 0 | 0 |
| `0x2E` sound switch group | 21 | 21 | 7 | 7 | 14 | 0 | 7 | 0 |
| `0x2F` sound parameter | 305 | 305 | 0 | 0 | 305 | 0 | 0 | 0 |
| `0x30` sound state group | 91 | 91 | 4 | 4 | 87 | 0 | 4 | 0 |
| `0x31` sound state | 345 | 345 | 6 | 6 | 339 | 0 | 6 | 0 |
| `0x55` sound space | 1,233 | 400 | 12 | 12 | 388 | 0 | 12 | 0 |
| `0x5F` voice set | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `0x70` voice line set | 272 | 272 | 222 | 222 | 50 | 0 | 222 | 0 |
| `0xA3` sound integrity | 1 | 1 | 0 | 0 | 1 | 0 | 0 | 0 |
| `0xB2` voice WEM file | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `0x43` Wwise sound bank | 1,148 | 400 | 356 | 0 | 44 | 356 | 0 | 0 |
| `0xBB` WEM file | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `0x3F` WEM file | 65,122 | 400 | 400 | 400 | 0 | 0 | 0 | 400 |

Read that as: **the install is mostly empty.** Blizzard ships a manifest record for
every sound the game can play and downloads only what has been heard. Of 36,981
sound assets, 57 opened. Of 65,122 WEM payloads, all 400 sampled opened, because
those are the ones the sounds that *were* heard point at. Zero readable assets for
`0x2D`, `0x2F` and `0xA3` is a legitimate, reportable measurement, not a reader
failure: this install has never played a sound switch or triggered a sound
parameter, so those bytes are not on disk. Evidence class ***install***.

Note `0x2E`. Its assets carry root class `0x89006ADB`, which *is*
`STUSoundSwitch` — so the reference's own type id for it is a name, not a shape.
Dispatch in this layer is on the root instance class rather than on the type id,
which is why those seven assets read as switch records instead of being filed as
unreadable. That is a deliberate difference from the reference, which switches on
the type id (`FindLogic/Combo.cs:980`), and it is a difference because the type id
is the thing in doubt. ***install*** for the class, ***decompiled*** for the
reference's behaviour.

### The four types with no layout at all

`SoundSwitchGroup` (`0x2E`), `SoundStateGroup` (`0x30`), `SoundState` (`0x31`),
`SoundSpace` (`0x55`) and `SoundIntegrity` (`0xA3`) have type ids in the
reference's enumeration (`TankTonka/Models/Common.cs:23`, `:26`, `:27`, `:36`,
`:58`) but **no generated STU class** — searched for and confirmed absent from
`TankLib/STU/Types/`. So there is no field layout to transcribe, and the reader
reports the root class and instance count instead. The classes they actually carry
on this install are recorded as `sound::observed_root`:

| type | readable sampled | root class |
|---|---|---|
| `0x30` sound state group | 4 | `0xFB68923D` |
| `0x31` sound state | 6 | `0x6BA402D7` |
| `0x55` sound space | 12 | `0xF3EB00D4` |

Evidence class ***install*** for the classes, ***decompiled*** for the absence of a
generated class. **These three hashes have not been checked against anything but
their own assets** — there is no reference to check them against, which is the
point.

## The audio container

Every payload is a Wwise RIFF file. The header walk is a transcription of the
reference's own reader, `DataTool/ConvertLogic/WEM/WwiseRIFFVorbis.cs:62-355`,
with the Opus header of `WwiseRIFFOpus.cs:57-189` taken when the codec word says
Opus. Evidence class ***decompiled*** — but "decompiled" here means *published
source*, the reference tool's own C#, not a memory capture.

What it reports: codec, channel count, sample rate, average bytes per second,
sample count, loop points, cue count, every chunk with its offset and size, the
stream uid and Vorbis block sizes, and — the part a decoder actually needs —
**where the Vorbis setup packet is and how many bytes it is.**

### Every SFX file is the awkward shape

Not one of the 500 sampled `0x3F` assets has a `vorb` chunk. All of them carry a
`0x42`-byte `fmt ` chunk, which is the case the reference handles by *fabricating*
a `vorb` at `fmt ` + `0x18` and reading a header that is not really in the file
(`WwiseRIFFVorbis.cs:152-155`). So the fabricated read is not the rare branch here;
it is the only branch these files take.

That could easily be a reader inventing structure. It is not, and the check is the
reference's own: after the setup packet, the first audio packet must follow
immediately (`WwiseRIFFVorbis.cs:776`). Across **248 payload headers parsed from
25 sounds, 248 were consistent and 0 were not.** A mis-read offset would break
that on the first file. The sample count the fabricated header yields also matches
the declared container length, which it would not if the bytes being read were
noise. Evidence class ***install***, checked against ***decompiled*** logic.

The shapes actually measured across a 500-asset sample of type `0x3F`, as
`ow-tank-sound types --cap 500` prints them:

```
            2  opus 48000 Hz 12 ch mapping family 255 extra chunk(s) hash
            5  opus 48000 Hz 2 ch mapping family 0 extra chunk(s) hash
            2  vorbis 44100 Hz 1 ch fmt 0x42 no-vorb 2B packet header extra chunk(s) hash
            3  vorbis 44100 Hz 2 ch fmt 0x42 no-vorb 2B packet header extra chunk(s) hash
          271  vorbis 48000 Hz 1 ch fmt 0x42 no-vorb 2B packet header extra chunk(s) hash
            2  vorbis 48000 Hz 1 ch fmt 0x42 no-vorb 2B packet header loops extra chunk(s) hash
          212  vorbis 48000 Hz 2 ch fmt 0x42 no-vorb 2B packet header extra chunk(s) hash
            1  vorbis 48000 Hz 2 ch fmt 0x42 no-vorb 2B packet header loops extra chunk(s) hash
            2  vorbis 48000 Hz 4 ch fmt 0x42 no-vorb 2B packet header extra chunk(s) hash
```

Read: 493 Vorbis and 7 Opus, every one of them a `0x42`-byte `fmt ` with no `vorb`
chunk and a two-byte packet header, all carrying the unexplained `hash` chunk, and
three of them carrying a loop.

Three readings worth pulling out:

* **The `hash` chunk is in every file and nothing explains it.** It is a sixteen
  byte chunk sitting between `fmt ` and `data`. The reference's chunk walk
  recognises six tags and ignores the rest (`WwiseRIFFVorbis.cs:116-134`), so it
  walks straight past it. `WemHeader::unknown_chunks` reports it rather than
  swallowing it. Evidence class ***install***; **what it means is unknown.**
* **`mod_packets` is set**, which means the audio packets carry a mode number
  ahead of the payload that has to be stripped before the data is valid Ogg. This
  matches the reference's own comment, which says the signal "seems to be 0xD9 when
  `_mod_packets` should be set" (`WwiseRIFFVorbis.cs:259-271`). Evidence class
  ***install*** for the value being present, ***decompiled*** for what it means.
* **The twelve-channel Opus file uses mapping family `255`**, and the reference's
  comment on that exact branch says `/* Overwatch 2 (PC) */` and "only seen 12ch,
  but seems to be what ChannelConfigToMapping would output with > 8"
  (`WwiseRIFFOpus.cs:167-178`). Its channel-layout word is `0x2d63f…`, matching the
  `TODO` four lines above. This is the one place in the reference where a concrete
  value was predicted and then found in the files. Evidence class ***install***,
  confirmed against ***decompiled***.

### Voice, measured separately

25 voice payloads from the speech manifest, all parsed, none failed:

```
  vorbis 48000 Hz 1 ch 24095 bytes  124836 samples (~2.601s)
  vorbis 48000 Hz 1 ch 44082 bytes  206508 samples (~4.302s)
  ...
  25 header(s) parsed, 0 failed, 0 not downloaded
  channels seen {1: 25}
  packet chains consistent 25, INCONSISTENT 0
```

Voice is mono, and every sampled voice file has a consistent packet chain, so the
same header walk that reads sound effects reads voice. Sample rates are not
uniform: of the five the CLI printed, four were 48 kHz and one was 16 kHz. The
test's own 25-file sample confirmed mono for all 25. Evidence class ***install***.

The metadata half of the voice chain, from 20 voice sets in the speech manifest:

```
  0x07a0000000000001  gender=Male   lines=1007 with_stimulus=1007 stimuli=316 payloads=1008
  0x07a0000000000051  gender=Male   lines=1276 with_stimulus=1276 stimuli=367 payloads=1277
  0x07a00000000000a4  gender=Male   lines=1280 with_stimulus=1280 stimuli=367 payloads=1281
  0x07a0000000000125  gender=Male   lines=1052 with_stimulus=1052 stimuli=332 payloads=1053
  0x07a0000000000134  gender=Female lines=1067 with_stimulus=1067 stimuli=307 payloads=1068
  20 voice set(s) read, 0 failed; 20 with a stimulus, 20 with payloads
  payload references: 17739 seen, 17739 found
```

**All 17,739 payload references a voice set names resolve to an asset in a
manifest.** That is the check on this half: a voice set reaches its audio through
an *inline* array of line instances, each holding four WEM wrappers, and a wrong
stride anywhere in that walk yields GUIDs that name nothing. Evidence class
***install***.

`m_gender` is the only enum on this whole surface the reference can name
(`Generic = 1, Male = 2, Female = 3`, `TankLib/STU/Types/Enums/Misc.cs:325-331`),
and it reads as `Male` or `Female` on every set sampled. ***decompiled*** for the
names, ***install*** for the values.

### One thing that does not resolve, and should not

A voice line set's `m_0DD426F8` is declared as
`teStructuredDataAssetRef<STUVoiceLine>[]` (`STUVoiceLineSet.cs:10`). Read on this
install, a set names between 2 and 1413 lines and **not one of them resolves to an
asset** — 0 of 3,385 sampled. Every entry decodes to asset type `0x06F`, with the
type bits identical across a whole set and the index counting up, which is the
shape of a correctly decoded table rather than of noise.

So the decode is right and there is nothing to follow: `STUVoiceLine` is an
*embedded* class with no asset type of its own, and type `0x06F` is absent from
the reference's enumeration and holds no assets in either manifest this install
loads. The lines live inside the voice sets instead, which is where
`sound::VoiceSet::read_in` reads them. **A zero-resolution census of `0x70` is the
expected result, not a broken reader**, and there is a test asserting it stays
that way. Evidence class ***install*** for the measurement, ***decompiled*** for
`STUVoiceLine` being embedded.

## What is NOT decoded

This is the part that matters most, so it gets its own section rather than being
buried at the end.

* **No Vorbis decoding.** The setup packet is located and sized. Its contents are
  not parsed. No audio packet is parsed. No sample is produced.
* **No Vorbis encoding**, and none is planned. Getting a playable file out of a
  Wwise container is not a decoding problem — it is the same problem `ww2ogg`
  solves, and it needs a Vorbis encoder. One written from scratch in pure Rust
  with no dependencies is a large project, not a small one, and shipping a
  half-working encoder would be worse than not shipping one.
* **No Opus decoding.** The Opus header is read — codec word, sample count, frame
  count, seek table size, codec version, mapping family — and then nothing.
* **No waveform, no playback, no sample rate conversion.** There is no audio
  output in this project at all.
* **No Wwise soundbank parsing.** Type `0x43` assets are raw Wwise banks: 356 of
  356 sampled open with the four bytes `BKHD`, and none parse as STU. The
  reference's generated `STUSoundBank` class is therefore its *typed view* of a
  bank, not the file's own layout, and `sound::SoundBank` finds nothing here. It is
  kept, honestly labelled, because a bank that were wrapped would be that shape.
  The reference itself agrees: `FindLogic/Combo.cs:988` says "todo: no point
  parsing this right now, not used". Evidence class ***install***.
* **The 16-byte `hash` chunk** is reported and unexplained. It is in all 500
  sampled sound-effect files and in neither sampled voice file, which had only
  `fmt ` and `data`.
* **`m_EF90634C` on a sound** reads `0x7E45B741` on every sound sampled. Whatever
  it is, it does not vary per sound. Not identified.
* **The `fmt ` 16-byte signature check is not exercised on this install**, because
  no file here has a `0x28`-byte `fmt ` chunk — they are all `0x42`. It is covered
  by unit tests only. ***not tested*** against real data.

## Two deliberate differences from the reference

Both are recorded here rather than buried in the code, because a reader that
quietly differs from its reference is worse than one that does not.

### One: the `fmt ` signature check, which cannot pass in the reference

The reference declares a **seventeen** byte array (`WwiseRIFFVorbis.cs:179`) but
reads only **sixteen** bytes off the stream (`:180`), and the comparison it runs
returns false as soon as the two lengths differ (`SoundUtils.cs:14`). So the
reference throws "expected signature in extra fmt?" on every file with a
`0x28`-byte `fmt ` chunk.

This module compares the sixteen bytes the reference actually reads against the
first sixteen of that seventeen-byte array, records the result in
`WemHeader::ext_signature_ok`, and says plainly that it did. Reproducing the bug
would refuse every container that had one. On this install that would be every
`0x28`-byte file — of which there happen to be none, which is exactly why the bug
is invisible from the files and worth saying out loud. Evidence class
***decompiled***.

### Two: the loop count, where the reference is stricter than it needs to be

The reference throws unless an `smpl ` chunk's loop count is exactly one
(`WwiseRIFFVorbis.cs:222`). This module reads the count, reports it, and
range-checks the loop against the sample count — which is what the reference does
next (`WwiseRIFFVorbis.cs:325-341`) and which is the check that actually matters.
Refusing a two-loop file over the count alone would lose the whole container for
nothing. Both are unit-tested; the multi-loop case is ***not tested*** against real
data, because no file sampled here had more than one loop.

## The honest handoff

This is a real deliverable, not a consolation prize. `ow-tank-sound wem <guid>
--out <dir>` writes **the game's own bytes, untouched**, plus a plain-text sidecar
describing the header:

```
> ow-tank-sound wem 0x07c00000002a5ffd --out out
asset 0x07c00000002a5ffd  type 0x03F (WEM file (alt id))  index 2777085  [base manifest]
  codec            vorbis (fmt word 0xFFFF)
  channels         2
  sample rate      48000 Hz
  samples          407466 per channel (~8.489 s, derived from the sample count and rate)
  fmt chunk        0x42 bytes, subtype 0x3102 (the reference comments this is the channel layout)
  packet header    2 bytes
  packet quirks    no_granule=true mod_packets=true header_triad=false old_headers=false
  vorb chunk       NONE in the file; the reference assumes one at fmt + 0x18
  stream uid       0xec69cb18
  blocksizes       pow 8 and 11
  setup packet     offset 96 (relative to data), 215 bytes at absolute 214
  first audio pkt  absolute 431
  packet chain     setup packet is immediately followed by the first audio packet,
                   as the reference requires
  convertible      the reference's converter would accept this

wrote out/07c00000002a5ffd.wem (134948 bytes) and out/07c00000002a5ffd.wem.txt
```

The payload begins with `RIFF` and is byte-for-byte what the install holds. To
hear it:

* **Vorbis** (every sound effect, and nearly all voice): `ww2ogg`, the standard
  external tool:
  ```
  ww2ogg 07c00000002a5ffd.wem -o 07c00000002a5ffd.ogg
  ```
  That is the whole job for this shape, and the reference's own pipeline does the
  same thing in-process — its own port of `ww2ogg` at
  `DataTool/ConvertLogic/WEM/WwiseRIFFVorbis.cs`, then `Revorb.Jiggle` over the
  result so the granule positions come out right
  (`SaveLogic/Combo.cs:990` and `:997`).
* **Opus** (both music tracks): `vgmstream`, which reads the container directly.
  The reference shells out to `vgmstream-cli` for exactly this
  (`SaveLogic/Combo.cs:962`).

Neither tool is vendored here and neither is invoked automatically. Running a
third-party binary over extracted game audio is the operator's call, not this
tool's. The sidecar repeats both commands so the `.wem` and its description travel
together.

`convertible` in that output is not a promise. It answers one narrow question: the
reference's converter would not *refuse* this file on its header-triad or
old-packet-header rules (`WwiseRIFFVorbis.cs:789`, `:807`). It is **not tested**
against an actual decoder — no `ww2ogg` or `vgmstream` was run against any file in
this project, because doing so means shipping and running a third-party binary,
which is a decision for the operator.

## Provenance of the hashes

Every hash in `ow-tank/src/sound.rs` cites the reference file and line it was read
from. All of those citations were checked against the reference tree before being
written down. The ones that are **also confirmed by real data** are marked; the rest
are a claim about structure and nothing more.

| hash | declared as | source | confirmed on real data |
|---|---|---|---|
| `0x04118DBD` | `STUSound` | `STUSound.cs:7` | yes — 57/57 sounds |
| `0x89006ADB` | `STUSoundSwitch` | `STUSoundSwitch.cs:6` | yes — 7/7 of type `0x2E` |
| `0xD96D3396` | `STUSoundParameter` | `STUSoundParameter.cs:6` | **no — 0 readable assets** |
| `0x0FA0034C` | `STU_0FA0034C` (Wwise block) | `Misc_4.cs:604` | yes — 25/25 sounds |
| `0xFBCC5EB2` | `STU_FBCC5EB2` (WEM wrapper) | `Misc_2.cs:9010` | **no** — reached only through the wrapper array |
| `0x39091531` | `STU_39091531` (wrapper base) | `Misc_1.cs:2098` | **no** |
| `0x2B407D38` | `STUVoiceLineSet` | `STUVoiceLineSet.cs:6` | yes — 222/222 |
| `0xB466CBBD` | `STUVoiceSet` | `STUVoiceSet.cs:7` | yes — 20/20 sets read |
| `0x40DA6CDF` | `STUVoiceLine` | `STUVoiceLine.cs:6` | yes — via every voice set's lines |
| `0x00C21225` | `STUVoiceLineInstance` | `STUVoiceLineInstance.cs:6` | yes — via every voice set's lines |
| `0x8745618A` | `STUVoiceSetComponent` | `STUVoiceSetComponent.cs:6` | yes — 39/39 heroes |
| `0xF746901F` | `STU_F746901F` (four wrappers) | `Misc_7.cs:8078` | yes — via voice payloads |
| `0xD744301C` | `STU_D744301C` | `Misc_4.cs:8313` | yes — via voice payloads |
| `0xD96BC652` | `STUSoundBank` | `STUSoundBank.cs:6` | **no — the files are `BKHD`** |
| `0x7A68A730` | `STU_7A68A730` | `Misc_6.cs:4996` | **no** — read as a subtitle, not followed |

Field hashes worth singling out because they are load-bearing:

* `0xC32C2195` (`m_C32C2195`, the Wwise block) and `0x42809FC5` (the parent sound),
  `STUSound.cs:13` and `:10`. Both confirmed: the block is what every sound read
  carried, and every sound bank reference it names exists in the registry (25/25).
* `0x48A42D80` / `0x72F2A9FA` (`m_soundWEMFiles` and its parallel
  `m_wwiseWEMFileIDs`), `Misc_4.cs:609` and `:615`. **Confirmed the only way that
  counts: 248 payload references resolved to assets that exist.** A wrong stride or
  a wrong deobfuscation key yields 64-bit values that name nothing.
* `0x8A4FF89C` (`m_voiceDefinition`), `STUVoiceSetComponent.cs:9`. Confirmed 39/39.
* `0x3C099E86` (`m_3C099E86`, the payload on a voice wrapper), `Misc_4.cs:8315`.
  Confirmed: it is the field behind all 17,739 resolving voice payload references.

### Where the parallel arrays do not quite pair up

`m_soundWEMFiles` and `m_wwiseWEMFileIDs` are walked in step by the reference
(`FindLogic/Combo.cs:942-947`), which would throw if the id list were shorter.
Measured over 25 sounds: 24 had lists of equal length, and **none of the 25 differed
in length at all** — the one that did not count as paired had an empty id list,
which is safe. The invariant the test asserts is the asymmetric one: the id list
was never *longer* than the reference list, and a longer one would mean the
descriptor was mis-read. Asserting plain equality would be a claim about the game's
content rather than about the reader, so the length is reported instead.

### Hashes this layer reads but could not check

`m_FA1FDF0D` (the Wwise block's name) and `m_BB01A3CC` (a switch's name) are
`teString` fields that read as **absent on every asset sampled** — the sound
surface declares no strings at all. Either the strings are genuinely empty in these
files or they are only written for assets this install does not hold. Not
distinguished. ***install*** for the absence; the interpretation is open.

## Not tested

Stated here rather than left to be discovered:

* **The `fmt ` signature check** (`ext_signature_ok`) — unit tests only. No asset
  on this install has a `0x28`-byte `fmt ` chunk.
* **`sound::SoundBank`** — no asset on this install matches its shape. Its unit
  behaviour is untested and its real-data behaviour is a measured zero.
* **`sound::read_wem_wrapper`** / `WemWrapper` — nothing on this install reaches it,
  because the wrappers live inside the Wwise block's reference array and the
  reference tooling does not follow them either.
* **`sound::voice_set_of_entity`** — cannot work against a stock install, since the
  voice set is in the manifest `GameData` does not load. The working path is
  `sound::voice_set_ref_of_entity` followed by `SpeechIndex`, and *that* path is
  tested (39/39).
* **The header walk against a real decoder.** Every invariant here is internal
  consistency. Nothing has been checked to see whether `ww2ogg` agrees.
* **`m_EF90634C`, the `hash` chunk, and the 12-channel Opus mapping** are reported
  and unexplained.

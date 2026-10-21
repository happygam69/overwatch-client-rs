//! The sound asset layer: Wwise metadata, and the RIFF container that carries
//! the audio.
//!
//! This is the layer that lets the project answer "what sound does this place,
//! this hero, this line play" and "how long is that sound" — and it stops
//! there. **It does not decode audio.** Getting a waveform out of an Overwatch
//! sound file means running an external Vorbis/Ogg tool; see
//! `docs/formats/sound.md` § "The honest handoff" for the procedure and for
//! what is deliberately absent here.
//!
//! ## Where the numbers come from
//!
//! Asset type ids come from the reference tooling's own enumeration
//! (`TankTonka/Models/Common.cs`). Class and field hashes come from its
//! generated STU declarations (`TankLib/STU/Types/*.cs`), which are a
//! *published reconstruction*: a claim about structure, not a measurement of
//! this install. Every hash this module hardcodes cites the file and line it was
//! read from, and `docs/formats/sound.md` records which of them were then
//! checked against real bytes.
//!
//! Two reference files are cited here and the distinction matters, so it is
//! stated once: a bare **`Combo.cs`** means `DataTool/FindLogic/Combo.cs`, the
//! walker that decides which fields of an asset to follow. The writer that
//! actually decodes audio is `DataTool/SaveLogic/Combo.cs`, and it is always
//! named in full.
//!
//! ## Most field names here are hashes, and that is expected
//!
//! The reference tooling could name only a small minority of the 23,204 fields
//! its own generator declares — 480 distinct field hashes across all of them.
//! So on `STUSound`, one field out of twenty is named and nineteen are
//! `m_<hash>`. Where the reference tooling itself has only a hash, this module
//! keeps the hash in the name (`UNNAMED_6657412F`) rather than inventing a word.
//! A name this module prints is therefore always a *reference-tooling* name,
//! never one fitted here.
//!
//! ## Five of the eleven named types have no reference layout at all
//!
//! `SoundSwitchGroup` (`0x2E`), `SoundStateGroup` (`0x30`), `SoundState`
//! (`0x31`), `SoundSpace` (`0x55`) and `SoundIntegrity` (`0xA3`) have type ids
//! in the reference enumeration but **no generated STU class** — the reference
//! generator could not name them, so there is no field layout to transcribe.
//! This module counts them, reports their root class and raw field bag, and
//! records them as [`Record::Unnamed`] rather than inventing a reader. That is a
//! finding, not a gap. The classes they actually carry are in
//! [`observed_root`].
//!
//! One of the five does have a real reader by accident: type `0x2E`
//! (`SoundSwitchGroup` in the reference's naming) carries root class `0x89006ADB`,
//! which *is* `STUSoundSwitch`, so its assets read as switch records. Dispatch here
//! is on the root class rather than on the type id precisely because of that.
//!
//! ## Where the audio actually is, measured
//!
//! Three of the asset types this module surveys are empty in the *base* manifest
//! and everything else lives in a second one. Measured with
//! `ow-tank-sound types` and `ow-tank-sound speech` on the reference install:
//!
//! | type | base manifest | speech manifest |
//! |---|---|---|
//! | `0x03F` WEM file | 65,122 registered, every sampled one a Wwise container | none |
//! | `0x0B2` voice WEM file | **0** | 65,921 |
//! | `0x0BB` WEM file | **0** | 2, both Wwise Opus |
//! | `0x05F` voice set | **0** | 294 |
//!
//! So `0x03F` is sound effects and `0x0BB` is the music, which is what the
//! reference's own extract tools separate (`ToolLogic/Extract/ExtractMusic.cs:26`).
//! Nothing about audio can be found from the base manifest alone, which is why
//! [`SpeechIndex`] exists rather than a flag on [`GameData::open`].
//!
//! ## Every SFX file on this install is the awkward shape
//!
//! Not one of the 500 sampled type-`0x03F` assets carries a `vorb` chunk. All of
//! them have a `0x42`-byte `fmt ` chunk, which is the case the reference handles by
//! *fabricating* a `vorb` at `fmt ` + `0x18` and reading a header that is not
//! really there (`WwiseRIFFVorbis.cs:152-155`). That fabricated read is not a
//! guess: on every sampled file the packet chain it produces is internally
//! consistent — the setup packet is immediately followed by the first audio packet,
//! which is the reference's own check — and the sample count it yields matches the
//! declared container length. [`WemHeader::vorb_synthesised`] records that this is
//! what happened, because a claim about the file which is not true should not be
//! reported as though it were.
//!
//! ## Not everything lives in the manifest this project loads
//!
//! Voice audio is in a **separate, per-locale manifest** (the `speech` one), and
//! [`crate::GameData::open`] deliberately does not read it — nothing else in this
//! crate needs audio. So a `VoiceSet` in the speech manifest is invisible to
//! [`crate::GameData::guids_of_type`]. [`SpeechIndex`] opens that manifest on
//! demand and lists what it holds; `ow-tank-sound speech` does so.
//!
//! ## The reading is bounds-checked throughout
//!
//! Every offset out of a RIFF chunk and every offset out of the STU dynamic
//! block is range-checked before it is used. A header that does not match what
//! the reference expects is [`Error`] rather than a number read out of noise.

use std::collections::BTreeMap;
use std::fmt;

use crate::keytable::Sources;
use crate::{guid, manifest, GameData, ManifestSelector, Result};
use ow_stu::{FieldInfo, Stu};

// ---------------------------------------------------------------------------
// Asset types
// ---------------------------------------------------------------------------

/// A name reduced to something comparable: spaces, hyphens and underscores
/// dropped, case folded. So `"voice set"`, `"VoiceSet"`, `"voice-set"` and
/// `"VOICE_SET"` all reduce to the same string, which is what the CLI's `--help`
/// list promises.
fn squash(name: &str) -> String {
    name.chars()
        .filter(|c| !c.is_whitespace() && *c != '-' && *c != '_')
        .flat_map(char::to_lowercase)
        .collect()
}

/// Type ids of the sound surface.
///
/// The first eleven are read from `TankTonka/Models/Common.cs`, the reference
/// tooling's own enumeration, at the lines named on each. The last three are
/// **not** in that enumeration: they are the ids the reference tooling switches
/// on when it walks a sound's audio payload
/// (`DataTool/FindLogic/Combo.cs:980`, `:987`, `:1166`), so they are carried here
/// rather than left as bare literals at the call site.
pub mod types {
    /// `Sound = 0x2C` (`TankTonka/Models/Common.cs:22`).
    pub const SOUND: u16 = 0x02C;
    /// `SoundSwitch = 0x2D` (`TankTonka/Models/Common.cs:24`).
    pub const SOUND_SWITCH: u16 = 0x02D;
    /// `SoundSwitchGroup = 0x2E` (`TankTonka/Models/Common.cs:23`).
    pub const SOUND_SWITCH_GROUP: u16 = 0x02E;
    /// `SoundParameter = 0x2F` (`TankTonka/Models/Common.cs:25`).
    pub const SOUND_PARAMETER: u16 = 0x02F;
    /// `SoundStateGroup = 0x30` (`TankTonka/Models/Common.cs:26`).
    pub const SOUND_STATE_GROUP: u16 = 0x030;
    /// `SoundState = 0x31` (`TankTonka/Models/Common.cs:27`).
    pub const SOUND_STATE: u16 = 0x031;
    /// `SoundSpace = 0x55` (`TankTonka/Models/Common.cs:36`).
    pub const SOUND_SPACE: u16 = 0x055;
    /// `VoiceSet = 0x5F` (`TankTonka/Models/Common.cs:39`).
    pub const VOICE_SET: u16 = 0x05F;
    /// `VoiceLineSet = 0x70` (`TankTonka/Models/Common.cs:44`).
    pub const VOICE_LINE_SET: u16 = 0x070;
    /// The asset type a voice line set's `m_0DD426F8` entries decode to —
    /// **measured, not declared.** It is absent from `Common.cs`'s enumeration, and
    /// this install's base and speech manifests hold no assets of it, so what it
    /// is called in the game is unknown. Kept because a reader that decodes a
    /// reference should be able to say what type it decoded, and because a type
    /// appearing in references but never in a registry is itself the finding.
    pub const VOICE_LINE_REF: u16 = 0x06F;
    /// `SoundIntegrity = 0xA3` (`TankTonka/Models/Common.cs:58`).
    pub const SOUND_INTEGRITY: u16 = 0x0A3;
    /// `VoiceWEMFile = 0xB2` (`TankTonka/Models/Common.cs:66`).
    pub const VOICE_WEM_FILE: u16 = 0x0B2;

    /// A Wwise sound bank, the container of Wwise events. Opened as a raw
    /// Wwise bank by `ExtractMusic` (`ToolLogic/Extract/ExtractMusic.cs:26`).
    /// **Not** in `Common.cs`'s enumeration.
    pub const SOUND_BANK: u16 = 0x043;
    /// A non-voice WEM payload asset. `Combo.cs:981` groups it with `0x3F` as
    /// "a sound file". **Not** in `Common.cs`'s enumeration.
    pub const SOUND_FILE: u16 = 0x0BB;
    /// The other payload id `Combo.cs:980` groups with [`SOUND_FILE`].
    /// **Not** in `Common.cs`'s enumeration.
    pub const SOUND_FILE_ALT: u16 = 0x03F;

    /// Every type this module surveys, in the order [`survey_sound`] walks them.
    pub const ALL: &[u16] = &[
        SOUND,
        SOUND_SWITCH,
        SOUND_SWITCH_GROUP,
        SOUND_PARAMETER,
        SOUND_STATE_GROUP,
        SOUND_STATE,
        SOUND_SPACE,
        VOICE_SET,
        VOICE_LINE_SET,
        SOUND_INTEGRITY,
        VOICE_WEM_FILE,
        SOUND_BANK,
        SOUND_FILE,
        SOUND_FILE_ALT,
    ];

    /// Types whose bytes are expected to be a Wwise RIFF container rather than
    /// an STU asset.
    ///
    /// The reference's *walker* groups them (`FindLogic/Combo.cs:980` and `:981`
    /// for `0x3F`/`0xBB`, `:1166` for `0xB2`), and its *writer* is what actually
    /// hands each one to a RIFF reader: `SaveLogic/Combo.cs:1036` opens the asset
    /// and calls `ConvertSoundFile`, which picks the Vorbis or the Opus RIFF
    /// reader from the codec word (`SaveLogic/Combo.cs:926-932`).
    ///
    /// Whether this holds on this install is measured rather than assumed:
    /// `Survey::as_stu` and `Survey::as_wem` record what actually happened for
    /// each type.
    pub const WEM_PAYLOAD: &[u16] = &[VOICE_WEM_FILE, SOUND_FILE, SOUND_FILE_ALT];

    /// Whether a type's payload is expected to be audio rather than structured
    /// data.
    pub fn is_wem_payload(type_id: u16) -> bool {
        WEM_PAYLOAD.contains(&type_id)
    }

    /// Human-readable name, for the CLI and for error messages.
    pub fn label(type_id: u16) -> &'static str {
        match type_id {
            SOUND => "sound",
            SOUND_SWITCH => "sound switch",
            SOUND_SWITCH_GROUP => "sound switch group",
            SOUND_PARAMETER => "sound parameter",
            SOUND_STATE_GROUP => "sound state group",
            SOUND_STATE => "sound state",
            SOUND_SPACE => "sound space",
            VOICE_SET => "voice set",
            VOICE_LINE_SET => "voice line set",
            SOUND_INTEGRITY => "sound integrity",
            VOICE_WEM_FILE => "voice WEM file",
            SOUND_BANK => "Wwise sound bank",
            SOUND_FILE => "WEM file",
            SOUND_FILE_ALT => "WEM file (alt id)",
            _ => "sound",
        }
    }

    /// The type a subcommand name selects: matched against [`label`] with
    /// spaces removed and case ignored, or against a literal `0x0B2`-style id.
    pub fn by_name(name: &str) -> Option<u16> {
        let squashed = super::squash(name);
        ALL.iter()
            .copied()
            .find(|t| super::squash(label(*t)) == squashed || format!("{t:#05x}") == squashed)
    }
}

// ---------------------------------------------------------------------------
// Class hashes
// ---------------------------------------------------------------------------

/// Instance class hashes for the sound surface.
///
/// Every one of these cites the reference tooling file and line it was read
/// from. A class the reference does not declare has no constant here at all,
/// which is how "no layout known" stays visible instead of becoming a guess.
pub mod class {
    /// `STUSound` — `TankLib/STU/Types/STUSound.cs:7`.
    pub const SOUND: u32 = 0x0411_8DBD;
    /// `STUSoundSwitch` — `TankLib/STU/Types/STUSoundSwitch.cs:6`.
    pub const SOUND_SWITCH: u32 = 0x8900_6ADB;
    /// `STUSoundParameter` — `TankLib/STU/Types/STUSoundParameter.cs:6`.
    pub const SOUND_PARAMETER: u32 = 0xD96D_3396;
    /// `STUSoundBank` — `TankLib/STU/Types/STUSoundBank.cs:6`.
    pub const SOUND_BANK: u32 = 0xD96B_C652;
    /// `STU_39091531`, the base both `STUSoundBank` and `STU_FBCC5EB2` derive
    /// from — `TankLib/STU/Types/Misc_1.cs:2098`.
    pub const BANK_BASE: u32 = 0x3909_1531;
    /// `STU_0FA0034C`, the Wwise media block embedded in a sound. Named
    /// `m_C32C2195` on `STUSound`; `Combo.cs:939-960` reads its WEM lists.
    /// — `TankLib/STU/Types/Misc_4.cs:604`.
    pub const WWISE_BLOCK: u32 = 0x0FA0_034C;
    /// `STU_FBCC5EB2`, the wrapper a sound's WEM lists point at
    /// — `TankLib/STU/Types/Misc_2.cs:9010`.
    pub const WEM_FILE: u32 = 0xFBCC_5EB2;
    /// `STUVoiceSet` — `TankLib/STU/Types/STUVoiceSet.cs:7`.
    pub const VOICE_SET: u32 = 0xB466_CBBD;
    /// `STUVoiceLineSet` — `TankLib/STU/Types/STUVoiceLineSet.cs:6`.
    pub const VOICE_LINE_SET: u32 = 0x2B40_7D38;
    /// `STUVoiceLine` — `TankLib/STU/Types/STUVoiceLine.cs:6`.
    pub const VOICE_LINE: u32 = 0x40DA_6CDF;
    /// `STUVoiceLineInstance`, the element of a voice set's line list —
    /// `TankLib/STU/Types/STUVoiceLineInstance.cs:6`.
    pub const VOICE_LINE_INSTANCE: u32 = 0x00C2_1225;
    /// `STUVoiceSetComponent`, the entity component that names a voice set —
    /// `TankLib/STU/Types/STUVoiceSetComponent.cs:6`. This is the join from a
    /// hero's entity to its voices.
    pub const VOICE_SET_COMPONENT: u32 = 0x8745_618A;
    /// `STU_F746901F`, the inline holder of four WEM wrappers on a voice line
    /// instance — `TankLib/STU/Types/Misc_7.cs:8078`.
    pub const VOICE_INSTANCE_WEMS: u32 = 0xF746_901F;
    /// `STU_D744301C`, one of that holder's four wrappers, and the class that
    /// actually carries `m_3C099E86` — `TankLib/STU/Types/Misc_4.cs:8313`.
    pub const VOICE_WEM: u32 = 0xD744_301C;
    /// `STU_7A68A730`, the *alternate* set of four WEM wrappers a voice line
    /// holds. `Combo.cs:1058` reads it for the subtitle, not for audio.
    /// — `TankLib/STU/Types/Misc_6.cs:4996`.
    pub const VOICE_LINE_WEMS: u32 = 0x7A68_A730;
}

/// Root instance classes measured on this install for the types the reference
/// tooling has no generated class for.
///
/// These are **not** from the reference. The reference generator could not name
/// these five types, so there is no published class hash to transcribe; these were
/// read off real assets and are reported so a census says what shape actually
/// shipped instead of only "a shape nobody has read".
///
/// The counts behind each are from `ow-tank-sound types --cap 40` on the
/// reference install (see `docs/formats/sound.md` § census). A single-asset
/// observation is marked as one, because it is one.
pub mod observed_root {
    /// `SoundStateGroup` (`0x30`): 3 readable assets sampled, all this class.
    pub const SOUND_STATE_GROUP: u32 = 0xFB68_923D;
    /// `SoundState` (`0x31`): **one** readable asset sampled, this class. A single
    /// observation is not a pattern.
    pub const SOUND_STATE: u32 = 0x6BA4_02D7;
    /// `SoundSpace` (`0x55`): 3 readable assets sampled, all this class.
    pub const SOUND_SPACE: u32 = 0xF3EB_00D4;
}

// ---------------------------------------------------------------------------
// Field hashes
// ---------------------------------------------------------------------------

/// Field hashes, grouped by the class that declares them.
///
/// Asset references are sixteen bytes wide with the GUID in the **first** eight,
/// which is what [`ow_stu::Stu::read_asset_ref`] reads. An *array element* puts
/// the GUID at `+8` instead — see the note on [`read_ref_array`].
pub mod field {
    /// `STUSound` (`STUSound.cs:7`). Only the two fields the reference tooling
    /// itself reads are named; the rest of the class is eight floats, two `uint`s,
    /// three unnamed enums and eight bytes with no names to give them.
    pub mod sound {
        /// `m_42809FC5`, a reference to another `STUSound`.
        pub const PARENT: u32 = 0x4280_9FC5;
        /// `m_C32C2195`, the embedded `STU_0FA0034C` Wwise media block. Read at
        /// `Combo.cs:938`.
        pub const WWISE_BLOCK: u32 = 0xC32C_2195;
        /// `m_D138B819`, a float whose generated default is `1f`
        /// (`STUSound.cs:23`) — the only float on the class with a unit default.
        pub const UNNAMED_D138B819: u32 = 0xD138_B819;
        /// `m_0BF329D0`, a float whose generated default is `150f`
        /// (`STUSound.cs:50`).
        pub const UNNAMED_0BF329D0: u32 = 0x0BF3_29D0;
        /// `m_6657412F`, a `uint` whose generated default is `3`
        /// (`STUSound.cs:26`).
        pub const UNNAMED_6657412F: u32 = 0x6657_412F;
        /// `m_EF90634C`, the class's other `uint` (`STUSound.cs:53`).
        pub const UNNAMED_EF90634C: u32 = 0xEF90_634C;
        /// `m_B62657B8`, the only byte on the class whose generated default is
        /// `1` (`STUSound.cs:62`).
        pub const UNNAMED_B62657B8: u32 = 0xB626_57B8;
        /// `Enum_255164EF` (`STUSound.cs:32`); every member of that enum is
        /// unnamed in the reference, so the raw integer is carried.
        pub const UNNAMED_97FF2EB7: u32 = 0x97FF_2EB7;
        /// `Enum_D2BF23BC` (`STUSound.cs:41`), every member unnamed.
        pub const UNNAMED_43F0D346: u32 = 0x43F0_D346;
        /// `Enum_5BDA56C9` (`STUSound.cs:44`), every member unnamed.
        pub const UNNAMED_D6EDF627: u32 = 0xD6ED_F627;
    }

    /// `STU_0FA0034C` (`Misc_4.cs:604`). This is the block that turns a sound
    /// into playable audio: it holds the payload asset references and the Wwise
    /// media ids that key them.
    pub mod wwise_block {
        /// `m_FA1FDF0D`, a `teString`.
        pub const NAME: u32 = 0xFA1F_DF0D;
        /// `m_soundWEMFiles`, a reference array of `STU_FBCC5EB2`.
        /// `Combo.cs:939` walks it.
        pub const WEM_FILES: u32 = 0x48A4_2D80;
        /// `m_soundWEMStreams`, the same shape for streamed rather than
        /// pre-loaded audio. `Combo.cs:951`.
        pub const WEM_STREAMS: u32 = 0x316E_BD9D;
        /// `m_wwiseWEMFileIDs`, a bare `uint[]` parallel to [`WEM_FILES`]:
        /// `Combo.cs:946` reads element `i` of it while walking element `i` of
        /// the reference array.
        pub const WWISE_FILE_IDS: u32 = 0x72F2_A9FA;
        /// `m_wwiseWEMStreamIDs`, parallel to [`WEM_STREAMS`]. `Combo.cs:958`.
        pub const WWISE_STREAM_IDS: u32 = 0x5A7E_C887;
        /// `m_soundBank`, the `STUSoundBank` the media lives in.
        pub const SOUND_BANK: u32 = 0x7575_4566;
        /// `m_wwiseEventIDPlay` (`Misc_4.cs`).
        pub const EVENT_PLAY: u32 = 0x5CEC_8D67;
        /// `m_wwiseEventIDStop` (`Misc_4.cs`).
        pub const EVENT_STOP: u32 = 0xEE7B_11F5;
    }

    /// `STU_F746901F`'s four wrapper fields (`Misc_7.cs:8078`), in the order
    /// `Combo.cs:1075-1076` reads them. The reference treats the four as a set;
    /// the order is kept so the output is stable.
    pub const VOICE_WEM_FIELDS: [u32; 4] = [
        0x1485_B834, // m_1485B834
        0x7980_27DE, // m_798027DE
        0xA84A_A2B5, // m_A84AA2B5
        0xD872_E45C, // m_D872E45C
    ];

    /// `STU_D744301C` (`Misc_4.cs:8313`).
    pub mod wem {
        /// `m_3C099E86`, the payload asset. `Combo.cs:1079` adds it to the
        /// line's sound-file set.
        pub const PAYLOAD: u32 = 0x3C09_9E86;
    }

    /// `STUSoundSwitch` (`STUSoundSwitch.cs:6`).
    pub mod switch_ {
        /// `m_032194C8`, a reference to `STU_11B8F80B` — the switch's group.
        pub const GROUP: u32 = 0x0321_94C8;
        /// `m_BB01A3CC`, a `teString`: the switch's own name.
        pub const NAME: u32 = 0xBB01_A3CC;
        /// `m_E87C7AF9`, a `uint`.
        pub const UNNAMED_E87C7AF9: u32 = 0xE87C_7AF9;
    }

    /// `STUSoundParameter` (`STUSoundParameter.cs:6`).
    pub mod parameter {
        /// `m_C27830B3`, a `teString`.
        pub const NAME: u32 = 0xC278_30B3;
        /// `m_B04205C5`, a `uint`.
        pub const UNNAMED_B04205C5: u32 = 0xB042_05C5;
    }

    /// `STUVoiceSet` (`STUVoiceSet.cs:7`).
    pub mod voice_set {
        /// `m_externalSound`, a reference to a `STUSound` played instead of a
        /// voice line. The same hash `STUVoiceLine` uses (`STUVoiceLine.cs:13`),
        /// which is what identifies it.
        pub const EXTERNAL_SOUND: u32 = 0x386B_8E9C;
        /// `m_voiceLineInstances`, an **inline** array of
        /// `STUVoiceLineInstance` (`Combo.cs:1037` walks it by index).
        pub const LINE_INSTANCES: u32 = 0xF51B_70C4;
        /// `m_gender`, `Enum_0C014B4A`. The only *named* enum anywhere on this
        /// surface: `Generic = 1`, `Male = 2`, `Female = 3` (`Enums/Misc.cs:325`).
        pub const GENDER: u32 = 0x7D88_A63A;
    }

    /// `STUVoiceLineInstance` (`STUVoiceLineInstance.cs:6`).
    pub mod voice_line_instance {
        /// `m_voiceLineRuntime`, an embedded `STUVoiceLine` — the actual line.
        pub const VOICE_LINE: u32 = 0xBC47_4019;
        /// `m_AF226247`, an embedded `STU_F746901F`, the four WEM wrappers this
        /// line instance can play. `Combo.cs:1073-1081`.
        pub const WEMS: u32 = 0xAF22_6247;
        /// `m_resourceKey`, a `ulong` (`STUVoiceLineInstance.cs:19`).
        pub const RESOURCE_KEY: u32 = 0x38BF_B46C;
    }

    /// `STUVoiceLine` (`STUVoiceLine.cs:6`).
    pub mod voice_line {
        /// `m_stimulus`, the `STUVoiceStimulus` that triggers this line.
        pub const STIMULUS: u32 = 0x401F_5484;
        /// `m_externalSound`.
        pub const EXTERNAL_SOUND: u32 = 0x386B_8E9C;
        /// `m_set`, the `STUVoiceLineSet` this line belongs to.
        pub const SET: u32 = 0xBA55_45D6;
        /// `m_6148094F`, a reference to a `STU_7A68A730`; `Combo.cs:1059` reads
        /// it as the line's subtitle.
        pub const SUBTITLE: u32 = 0x6148_094F;
        /// `m_BD1B6F64`, a reference array of `STUVoiceConversation`.
        pub const CONVERSATIONS: u32 = 0xBD1B_6F64;
        /// `m_weight`, a float whose generated default is `1f`
        /// (`STUVoiceLine.cs:34`).
        pub const WEIGHT: u32 = 0x9CDD_C24D;
    }

    /// `STUVoiceLineSet` (`STUVoiceLineSet.cs:6`).
    pub mod voice_line_set {
        /// `m_0DD426F8`, a reference array of `STUVoiceLine`.
        pub const LINES: u32 = 0x0DD4_26F8;
    }

    /// `STUVoiceSetComponent` (`STUVoiceSetComponent.cs:6`) — the entity-side
    /// half of the voice chain.
    pub mod voice_set_component {
        /// `m_voiceDefinition`, the `STUVoiceSet`.
        pub const VOICE_SET: u32 = 0x8A4F_F89C;
    }

    /// `STU_39091531` (`Misc_1.cs:2098`), the base of the WEM wrapper and the
    /// sound bank.
    pub mod wem_wrapper {
        /// `m_fileSize`, a `uint`. On a WEM wrapper this is the size of the audio
        /// payload — the one number here that speaks about the audio rather than
        /// about the metadata.
        pub const FILE_SIZE: u32 = 0xFE1C_5928;
        /// `m_267B9B97`, a reference array of `STU_39091531`.
        pub const REFS: u32 = 0x267B_9B97;
        /// `m_5369BDC2`, a `uint` (`Misc_1.cs`).
        pub const UNNAMED_5369BDC2: u32 = 0x5369_BDC2;
    }

    /// `STUSoundBank` (`STUSoundBank.cs:6`).
    pub mod sound_bank {
        /// `m_F99A0938`, a `teString`. `STU_FBCC5EB2` declares the same hash at
        /// the same offset (`Misc_2.cs:9011`), which is what identifies it as the
        /// wrapper's own name rather than the bank's alone.
        pub const NAME: u32 = 0xF99A_0938;
        /// `m_27D46586`, a second `teString`. The reference does not read it and
        /// neither does this.
        pub const UNNAMED_27D46586: u32 = 0x27D4_6586;
        /// `m_2075DBF6`, an inline array of `STU_0FA0034C` — the bank's media
        /// list.
        pub const MEDIA: u32 = 0x2075_DBF6;
        /// `m_wwiseBankID`, a `uint`. One of the very few fields on this whole
        /// surface whose generated name *is* real (`STUSoundBank.cs:19`).
        pub const WWISE_BANK_ID: u32 = 0xF780_5734;
    }

    /// `STU_FBCC5EB2`'s second string (`Misc_2.cs:9013`).
    pub const WEM_WRAPPER_NAME_2: u32 = 0x770C_FAB9;
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Why a Wwise container did not read.
///
/// Every variant names the place it gave up, because "this is not a WEM" and
/// "this WEM's `vorb` chunk is a size the reference does not handle" are very
/// different findings, and the census reports them separately.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The first four bytes are neither `RIFF` nor `RIFX`.
    NotRiff { found: [u8; 4] },
    /// The four bytes at offset 8 are not `WAVE`.
    NotWave { found: [u8; 4] },
    /// A read ran past the end of the buffer.
    Truncated {
        what: &'static str,
        at: usize,
        need: usize,
        have: usize,
    },
    /// A chunk the reader needs was not found.
    MissingChunk(&'static str),
    /// A chunk's declared size does not fit inside the container.
    ChunkOutOfRange {
        id: [u8; 4],
        offset: usize,
        size: u32,
        len: usize,
    },
    /// A field held a value the reference's own reader rejects.
    BadField {
        what: &'static str,
        found: u64,
        wanted: u64,
    },
    /// A structural rule the reference's reader enforces, stated in prose.
    Vorbis(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotRiff { found } => write!(
                f,
                "not a RIFF container: the first four bytes are {}",
                tag(*found)
            ),
            Error::NotWave { found } => write!(
                f,
                "RIFF container is not a WAVE: the bytes at offset 8 are {}",
                tag(*found)
            ),
            Error::Truncated {
                what,
                at,
                need,
                have,
            } => write!(
                f,
                "{what} needs {need} byte(s) at offset {at} but the buffer holds {have}"
            ),
            Error::MissingChunk(what) => write!(f, "expected a {what} chunk and found none"),
            Error::ChunkOutOfRange {
                id,
                offset,
                size,
                len,
            } => write!(
                f,
                "chunk {} declares {size} byte(s) at offset {offset}, past the {len}-byte container",
                tag(*id)
            ),
            Error::BadField { what, found, wanted } => write!(
                f,
                "{what} is {found} (0x{found:x}); the reference reader requires {wanted} (0x{wanted:x})"
            ),
            Error::Vorbis(m) => write!(f, "wem header: {m}"),
        }
    }
}

impl std::error::Error for Error {}

type WemResult<T> = std::result::Result<T, Error>;

/// A four-byte tag rendered readably, or as hex when it is not printable.
fn tag(bytes: [u8; 4]) -> String {
    if bytes.iter().all(|b| (0x20..0x7f).contains(b)) {
        String::from_utf8_lossy(&bytes).into_owned()
    } else {
        format!("0x{:08x}", u32::from_le_bytes(bytes))
    }
}

// ---------------------------------------------------------------------------
// The Wwise RIFF container
// ---------------------------------------------------------------------------

/// Which codec a Wwise container holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    /// Wwise Vorbis: `fmt ` codec word `0xFFFF`, audio in `vorb` + `data`.
    Vorbis,
    /// Wwise Opus: `fmt ` codec word `0x3041`, audio in `data` with a `seek`
    /// table. The reference's music path (`ExtractMusic.cs:22`) warns that most
    /// music is Opus.
    Opus,
    /// A codec word this reader does not know.
    Other(u16),
}

impl Codec {
    /// The codec word for each codec, read from the reference's readers:
    /// `WwiseRIFFVorbis.cs:158` requires `0xFFFF`, `WwiseRIFFOpus.cs:122`
    /// requires `0x3041`.
    pub fn from_word(word: u16) -> Codec {
        match word {
            0xFFFF => Codec::Vorbis,
            0x3041 => Codec::Opus,
            other => Codec::Other(other),
        }
    }

    /// The codec word `fmt ` must hold for this codec.
    pub fn word(self) -> u16 {
        match self {
            Codec::Vorbis => 0xFFFF,
            Codec::Opus => 0x3041,
            Codec::Other(w) => w,
        }
    }
}

impl fmt::Display for Codec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Codec::Vorbis => f.write_str("vorbis"),
            Codec::Opus => f.write_str("opus"),
            Codec::Other(w) => write!(f, "other(0x{w:04x})"),
        }
    }
}

/// One chunk of the container, by position.
///
/// `offset` is where the *payload* starts — eight bytes past the chunk's own tag
/// and size — and `size` is the declared payload length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chunk {
    pub id: [u8; 4],
    pub offset: usize,
    pub size: u32,
}

impl Chunk {
    /// The chunk's tag as text.
    pub fn tag(&self) -> String {
        tag(self.id)
    }

    /// End of the payload, exclusive.
    pub fn end(&self) -> usize {
        self.offset + self.size as usize
    }
}

/// Everything a Wwise RIFF header says, read without decoding any audio.
///
/// The field set is the reference reader's (`WwiseRIFFVorbis.cs:62-355`), and so
/// are the *rejections*: a `fmt ` codec word that is not `0xFFFF` for Vorbis is
/// [`Error::BadField`] here exactly as it is there, because a reader that
/// accepted it would be reading noise. Where the reference fabricates a missing
/// `vorb` chunk it records that it did so in
/// [`vorb_synthesised`](Self::vorb_synthesised), because that is a claim about
/// the file which is not true.
///
/// This is a **header walk**. It gives format, channels, sample rate, sample
/// count, loop points, and where the Vorbis setup packet begins and how big it
/// is. It does not produce sound.
#[derive(Debug, Clone, PartialEq)]
pub struct WemHeader {
    /// Declared container length: the `RIFF` size field plus eight.
    pub total_len: u32,
    /// True for a `RIFX` container, whose integers are big-endian.
    pub big_endian: bool,
    /// Chunks in file order.
    pub chunks: Vec<Chunk>,
    pub codec: Codec,
    /// Channel count.
    pub channels: u16,
    /// Samples per second per channel.
    pub sample_rate: u32,
    /// The `fmt ` chunk's average-bytes-per-second word. For Vorbis the
    /// reference turns this into a nominal bitrate
    /// (`WwiseRIFFVorbis.cs:385`), so it is not a measured figure.
    pub avg_bytes_per_second: u32,
    /// The `fmt ` chunk's declared size.
    pub fmt_size: u32,
    /// The `fmt ` chunk's extension word at `+0x12`. **Vorbis only** — the Opus
    /// `fmt ` has no such word, and the reference leaves this zero for Opus.
    pub ext_unk: u16,
    /// The `fmt ` chunk's subtype word: Vorbis at `+0x14`, Opus at `+0x14` behind
    /// an extra-size gate. The reference comments that this is "clearly just the
    /// channel layout" and lists the values it has seen
    /// (`WwiseRIFFVorbis.cs:345-354`); that comment is reproduced in
    /// `docs/formats/sound.md` rather than encoded as an enum here, because the
    /// reference's own switch over it is commented out.
    pub subtype: u32,
    /// Whether the sixteen signature bytes after a `0x28`-byte `fmt ` matched the
    /// reference's expected value. `None` when the `fmt ` chunk is not `0x28`
    /// bytes.
    pub ext_signature_ok: Option<bool>,
    /// Cue count, from the `cue ` chunk.
    pub cue_count: Option<u32>,
    /// Loop count, from the `smpl ` chunk: the number the chunk declares.
    /// [`loop_start`](Self::loop_start) and [`loop_end`](Self::loop_end) are the
    /// **first** loop's points, because the reference reads exactly one pair
    /// (`WwiseRIFFVorbis.cs:224-226`) and there is no layout here for the rest.
    pub loop_count: u32,
    /// Loop start, in samples, already ordered and clamped.
    pub loop_start: u32,
    /// Loop end, in samples, after the reference's end-plus-one adjustment.
    pub loop_end: u32,
    /// Total samples per channel, from the `vorb` chunk.
    pub sample_count: u32,
    /// Whether packets carry no granule position, which means a two-byte packet
    /// header instead of six. Derived from the `vorb` chunk's size.
    pub no_granule: bool,
    /// Whether audio packets carry a mode number ahead of the payload, which the
    /// reference has to strip before the data is a valid Ogg page.
    pub mod_packets: bool,
    /// Whether the container carries the older Wwise "header triad". The
    /// reference cannot convert those (`WwiseRIFFVorbis.cs:789`).
    pub header_triad_present: bool,
    /// Whether packets use the old header layout.
    pub old_packet_headers: bool,
    /// Whether there was no `vorb` chunk and one was assumed at `fmt ` + `0x18`.
    pub vorb_synthesised: bool,
    /// The `vorb` chunk's stream uid, when it has one.
    pub uid: Option<u32>,
    /// Vorbis blocksize exponent 0, when present.
    pub blocksize0_pow: Option<u8>,
    /// Vorbis blocksize exponent 1, when present.
    pub blocksize1_pow: Option<u8>,
    /// Bytes of header before each packet's payload: two or six. Zero for Opus,
    /// whose packets are raw.
    pub packet_header_size: usize,
    /// Where the `data` chunk's payload starts.
    pub data_offset: usize,
    /// The `data` chunk's payload length.
    pub data_size: usize,
    /// Offset of the Vorbis setup packet, relative to `data_offset`.
    pub setup_packet_offset: Option<u32>,
    /// Offset of the first audio packet, relative to `data_offset`.
    pub first_audio_packet_offset: Option<u32>,
    /// Absolute offset of the setup packet: `data_offset + setup_packet_offset`.
    pub setup_packet_at: Option<usize>,
    /// Absolute offset of the first audio packet.
    pub first_audio_packet_at: Option<usize>,
    /// Declared packet size at the setup packet's offset.
    pub setup_packet_size: Option<u16>,
    /// Opus only: declared sample count (`WwiseRIFFOpus.cs:135`).
    pub num_samples: Option<u32>,
    /// Opus only: how many `u16` frame sizes the `fmt ` chunk says the `seek`
    /// chunk holds (`WwiseRIFFOpus.cs:137`).
    pub seek_table_count: Option<u32>,
    /// Opus only: the `seek` chunk's own declared byte size. The reference reads
    /// `seek_table_count` frames out of it, so the two have to agree; both are
    /// reported rather than only the one that is used, so a mismatch is visible.
    pub seek_chunk_size: Option<u32>,
    /// Opus only: the codec version byte, which the reference requires to be 1
    /// (`WwiseRIFFOpus.cs:145`).
    pub codec_version: Option<u8>,
    /// Opus only: the channel-mapping family byte at `+0x23`
    /// (`WwiseRIFFOpus.cs:140`). The reference treats `1` and `255` as known and
    /// `255` is commented as "Overwatch 2 (PC)", which is the only place in this
    /// layer's reference that names the game explicitly.
    pub mapping_family: Option<u8>,
}

impl WemHeader {
    /// Length in seconds, from the sample count and the sample rate.
    ///
    /// **Derived, not read**: it is the quotient of two read values. `None` when
    /// the rate is zero or the sample count is missing. For Opus,
    /// [`num_samples`](Self::num_samples) is used rather than
    /// [`sample_count`](Self::sample_count), because Opus carries its sample count
    /// in the `fmt ` chunk.
    pub fn duration_seconds(&self) -> Option<f32> {
        if self.sample_rate == 0 {
            return None;
        }
        let samples = match self.codec {
            Codec::Opus => self.num_samples? as u64,
            _ => self.sample_count as u64,
        };
        if samples == 0 {
            return None;
        }
        Some(samples as f32 / self.sample_rate as f32)
    }

    /// Bytes of Vorbis setup header, which an external decoder needs and which
    /// this reader does not interpret.
    pub fn setup_packet_len(&self) -> Option<usize> {
        self.setup_packet_size.map(|s| s as usize)
    }

    /// Whether the reference's own converter could turn this file into an Ogg.
    ///
    /// It says no for a header triad (`WwiseRIFFVorbis.cs:789`) and for old
    /// packet headers (`WwiseRIFFVorbis.cs:807`), both of which the reference
    /// throws on rather than handles.
    pub fn convertible_by_reference(&self) -> bool {
        !self.header_triad_present && !self.old_packet_headers
    }

    /// Whether the setup packet and the first audio packet actually sit where the
    /// `vorb` chunk says.
    ///
    /// The reference performs exactly this check
    /// (`WwiseRIFFVorbis.cs:776`: "first audio packet doesn't follow setup packet"), so
    /// it is the invariant that catches a mis-read `vorb` offset. It holds only
    /// when the header is Vorbis and both packets were located inside the `data`
    /// chunk. A *fabricated* `vorb` still counts: the reference reads the offsets
    /// from its stand-in and runs the same check, and on the reference install
    /// that is exactly the shape real SFX files take — a `0x42`-byte `fmt ` and no
    /// `vorb` chunk at all.
    ///
    /// **Not tested against a reference converter** - it checks internal
    /// consistency, not that a decoder agrees.
    pub fn setup_packet_consistent(&self) -> Option<bool> {
        if self.codec != Codec::Vorbis {
            return None;
        }
        let setup_at = self.setup_packet_at?;
        let first_at = self.first_audio_packet_at?;
        let size = self.setup_packet_size? as usize;
        Some(setup_at + self.packet_header_size + size == first_at)
    }

    /// Chunk tags this module does not know, as `(tag, offset, size)`.
    ///
    /// The reference's chunk walk recognises six tags and ignores the rest
    /// (`WwiseRIFFVorbis.cs:116-134`). Ignoring is right for reading, wrong for
    /// reporting: a file carrying a tag nobody has explained is a finding. On the
    /// reference install the common unknown tag is `hash`, a sixteen-byte chunk
    /// that sits between `fmt ` and `data`.
    pub fn unknown_chunks(&self) -> Vec<Chunk> {
        const KNOWN: [&[u8; 4]; 7] = [
            b"fmt ", b"cue ", b"LIST", b"smpl", b"vorb", b"data", b"seek",
        ];
        self.chunks
            .iter()
            .filter(|c| !KNOWN.contains(&&c.id))
            .copied()
            .collect()
    }

    /// A one-line summary, for the CLI.
    pub fn summary(&self) -> String {
        let secs = match self.duration_seconds() {
            Some(s) => format!("~{s:.3}s"),
            None => "unknown length".to_string(),
        };
        let loop_text = if self.loop_count == 0 {
            "no loop".to_string()
        } else {
            format!("loop {}..{}", self.loop_start, self.loop_end)
        };
        // Opus keeps its sample count in the `fmt ` chunk, so printing `sample_count`
        // for it would print the Vorbis `vorb` field, which is zero in every Opus
        // file.
        let samples = match self.codec {
            Codec::Opus => self.num_samples.unwrap_or(0),
            _ => self.sample_count,
        };
        format!(
            "{} {} Hz {} ch {} samples ({}), {}, {}-byte packet header, {} chunk(s)",
            self.codec,
            self.sample_rate,
            self.channels,
            samples,
            secs,
            loop_text,
            self.packet_header_size,
            self.chunks.len()
        )
    }
}

/// The four bytes a raw Wwise soundbank opens with.
///
/// Measured on this install: every readable type-`0x043` asset begins `BKHD`.
/// Such a file is a Wwise bank, which is a different container from the
/// `RIFF`/`WAVE` media container this module parses; see
/// [`class::SOUND_BANK`] and the note on [`SoundBank`].
pub const WWISE_BANK_MAGIC: [u8; 4] = *b"BKHD";

/// The bytes the reference's `fmt `-chunk signature check compares against, for a
/// `0x28`-byte `fmt ` (`WwiseRIFFVorbis.cs:179`).
///
/// **The reference's own check can never pass.** It declares a **seventeen**-byte
/// array but reads only **sixteen** bytes off the stream (`WwiseRIFFVorbis.cs:180`),
/// and the byte-for-byte comparison it then runs
/// (`SoundUtils.cs:7-19`, which returns false as soon as the two lengths differ)
/// therefore fails on every file. So a real `0x28`-byte `fmt ` chunk — which
/// every SFX file on the reference install carries — would be rejected by the
/// reference outright with "expected signature in extra fmt?".
///
/// The array is kept here at its true length of seventeen so the claim is
/// checkable, and [`parse_wem`] compares the **sixteen** bytes the reference
/// reads against the first sixteen of it. That is a deliberate difference from
/// the reference, and the only one in this module: a reader that reproduced the
/// bug would refuse every container on this install.
///
/// The last byte, `0x71`, is the one the reference never looks at.
pub const FMT_EXT_SIGNATURE: [u8; 17] = [
    1, 0, 0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xAA, 0, 0, 0x38, 0x9b, 0x71,
];

/// How many bytes of [`FMT_EXT_SIGNATURE`] are actually compared: the number the
/// reference reads off the stream (`WwiseRIFFVorbis.cs:180`).
pub const FMT_EXT_SIGNATURE_CHECKED: usize = 16;

/// Where those bytes sit inside a `0x28`-byte `fmt ` chunk.
///
/// **Derived, not read**: the reference uses one sequential `BinaryReader` across
/// the whole `fmt ` chunk, so the offset is the running total of what it has
/// already read — codec, channels, rate, average, block-align and bits-per-sample
/// (`0x10`), the extra-size word (`0x12`), the extension word (`0x14`) and the
/// subtype word (`0x18`).
pub const FMT_EXT_SIGNATURE_OFFSET: usize = 0x18;

/// `vorb` chunk sizes the reference's reader accepts: `(size, no_granule,
/// seek_offset)`.
///
/// Transcribed from `WwiseRIFFVorbis.cs:233-320`. The seek offset is where the
/// reader takes `setup_packet_offset` and `first_audio_packet_offset` from: it is
/// `0x10` for the two *no-granule* shapes and `0x18` for every other one, because
/// the two branches of that switch are keyed on `no_granule` rather than on the
/// size. A `vorb` size outside this table is [`Error::Vorbis`], because the
/// reference throws on it rather than guessing.
///
/// A size of `0` is in the table because the reference accepts it, and it is a
/// real reading rather than a typo: the reader seeks to `vorb + 0` and reads as if
/// there were a header there, running into whatever follows the empty chunk. This
/// module does the same, so its sample count and packet offsets for such a file
/// are the bytes behind the empty chunk. It is reported as
/// [`no_granule`](WemHeader::no_granule) being true with a synthesised-looking
/// header, which is exactly what the reference sees.
pub const VORB_SHAPES: &[(u32, bool, usize)] = &[
    (0x00, true, 0x10),
    (0x28, false, 0x18),
    (0x2A, true, 0x10),
    (0x2C, false, 0x18),
    (0x32, false, 0x18),
    (0x34, false, 0x18),
];

/// A read-only view of a buffer whose integers may be either endianness.
///
/// Every accessor is bounds-checked, which is what lets the RIFF walk trust a
/// declared size it has not verified.
struct Rd<'a> {
    data: &'a [u8],
    big_endian: bool,
}

impl<'a> Rd<'a> {
    fn u16_at(&self, at: usize) -> Option<u16> {
        let b = self.data.get(at..at + 2)?;
        Some(if self.big_endian {
            u16::from_be_bytes([b[0], b[1]])
        } else {
            u16::from_le_bytes([b[0], b[1]])
        })
    }

    fn u32_at(&self, at: usize) -> Option<u32> {
        let b = self.data.get(at..at + 4)?;
        Some(if self.big_endian {
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        } else {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        })
    }

    fn u8_at(&self, at: usize) -> Option<u8> {
        self.data.get(at).copied()
    }

    /// A four-byte read that must fit, as an error rather than `None`.
    fn need32(&self, at: usize, what: &'static str) -> WemResult<u32> {
        self.u32_at(at).ok_or(Error::Truncated {
            what,
            at,
            need: 4,
            have: self.data.len(),
        })
    }

    /// A two-byte read that must fit, as an error rather than `None`.
    fn need16(&self, at: usize, what: &'static str) -> WemResult<u16> {
        self.u16_at(at).ok_or(Error::Truncated {
            what,
            at,
            need: 2,
            have: self.data.len(),
        })
    }
}

/// Read a Wwise RIFF container's header.
///
/// `bytes` is the asset's whole payload. The walk is a transcription of
/// `DataTool/ConvertLogic/WEM/WwiseRIFFVorbis.cs:62-355`, including its
/// rejections, with the Opus header of `WwiseRIFFOpus.cs:57-189` taken when the
/// codec word says Opus.
///
/// It reads no audio and decodes no Vorbis. What it can tell you is where the
/// setup packet is, how big it is, and how many channels, samples and
/// milliseconds the file declares — which is what an external decoder needs to
/// be pointed at the right bytes.
pub fn parse_wem(bytes: &[u8]) -> WemResult<WemHeader> {
    let id4 =
        |at: usize| -> Option<[u8; 4]> { bytes.get(at..at + 4).map(|b| [b[0], b[1], b[2], b[3]]) };
    let id4_or = |at: usize| id4(at).unwrap_or([0, 0, 0, 0]);

    let riff = id4_or(0);
    let big_endian = match riff {
        [b'R', b'I', b'F', b'F'] => false,
        [b'R', b'I', b'F', b'X'] => true,
        _ => return Err(Error::NotRiff { found: riff }),
    };
    let rd = Rd {
        data: bytes,
        big_endian,
    };

    // The RIFF size field counts everything after itself, so the container is
    // eight bytes longer. The reference adds the eight before comparing against
    // the file size (`WwiseRIFFVorbis.cs:84`).
    let declared = rd.need32(4, "the RIFF size field")?;
    let Some(total_len) = declared.checked_add(8) else {
        return Err(Error::Truncated {
            what: "the RIFF size field",
            at: 4,
            need: 4,
            have: bytes.len(),
        });
    };
    if total_len as usize > bytes.len() {
        return Err(Error::Truncated {
            what: "the declared RIFF container",
            at: 0,
            need: total_len as usize,
            have: bytes.len(),
        });
    }

    let wave = id4_or(8);
    if wave != *b"WAVE" {
        return Err(Error::NotWave { found: wave });
    }
    let riff_end = total_len as usize;

    // ---- chunk walk -------------------------------------------------------
    let mut chunks: Vec<Chunk> = Vec::new();
    let mut at = 12usize;
    while at < riff_end {
        // The reference checks the eight-byte chunk header before reading it
        // (`WwiseRIFFVorbis.cs:110`).
        if at + 8 > riff_end {
            return Err(Error::Truncated {
                what: "a chunk header",
                at,
                need: 8,
                have: riff_end,
            });
        }
        let id = id4_or(at);
        let size = rd.need32(at + 4, "a chunk size field")?;
        let payload = at + 8;
        let Some(end) = payload
            .checked_add(size as usize)
            .filter(|e| *e <= riff_end)
        else {
            return Err(Error::ChunkOutOfRange {
                id,
                offset: at,
                size,
                len: riff_end,
            });
        };
        chunks.push(Chunk {
            id,
            offset: payload,
            size,
        });
        // Chunks are packed: the reference advances by eight plus the size, with
        // no word alignment (`WwiseRIFFVorbis.cs:136`).
        at = end;
    }

    let find = |want: &[u8; 4]| chunks.iter().find(|c| &c.id == want).copied();

    // The reference requires at least a `fmt ` or a `data`
    // (`WwiseRIFFVorbis.cs:143`) and then, in practice, both.
    if find(b"fmt ").is_none() && find(b"data").is_none() {
        return Err(Error::MissingChunk("fmt or data"));
    }
    let Some(fmt) = find(b"fmt ") else {
        return Err(Error::MissingChunk("fmt "));
    };
    let Some(data) = find(b"data") else {
        return Err(Error::MissingChunk("data"));
    };

    // ---- fmt --------------------------------------------------------------
    // The codec word comes first, because the two codecs have different `fmt `
    // layouts and different rules on the rest of it. Reading the Vorbis rules
    // before dispatching is what made an Opus container report "no vorb chunk".
    let fmt_size = fmt.size;
    let codec_word = rd.need16(fmt.offset, "the fmt codec word")?;
    let channels = rd.need16(fmt.offset + 2, "the fmt channel count")?;
    let sample_rate = rd.need32(fmt.offset + 4, "the fmt sample rate")?;
    let avg_bytes_per_second = rd.u32_at(fmt.offset + 8).unwrap_or(0);

    // Both codecs leave the block-align and bits-per-sample words at zero, but they
    // do not agree on what a non-zero one means. The Vorbis reference insists on
    // each separately (`WwiseRIFFVorbis.cs:166-167`); the Opus reference rejects
    // the *pair* only when both are non-zero (`WwiseRIFFOpus.cs:126-128`), so an
    // Opus file may legitimately carry a non-zero block size. The check therefore
    // lives in the Vorbis branch below, not here — reading it before dispatching
    // was reporting "bad block align" for containers the reference accepts.
    let block_align = rd.need16(fmt.offset + 12, "the fmt block-align word")?;
    let bits_per_sample = rd.need16(fmt.offset + 14, "the fmt bits-per-sample word")?;

    let codec = Codec::from_word(codec_word);
    let mut vorb = find(b"vorb");
    let vorb_synthesised = vorb.is_none();

    // ---- assemble ---------------------------------------------------------
    let mut header = WemHeader {
        total_len,
        big_endian,
        // A clone rather than a move: `find` above borrows the chunk list, and a
        // container holds a handful of chunks, so the copy is cheaper than
        // reordering every lookup.
        chunks: chunks.clone(),
        codec,
        channels,
        sample_rate,
        avg_bytes_per_second,
        fmt_size,
        ext_unk: 0,
        subtype: 0,
        ext_signature_ok: None,
        cue_count: find(b"cue ").and_then(|c| rd.u32_at(c.offset)),
        loop_count: 0,
        loop_start: 0,
        loop_end: 0,
        sample_count: 0,
        no_granule: false,
        mod_packets: false,
        header_triad_present: false,
        old_packet_headers: false,
        vorb_synthesised,
        uid: None,
        blocksize0_pow: None,
        blocksize1_pow: None,
        packet_header_size: 6,
        data_offset: data.offset,
        data_size: data.size as usize,
        setup_packet_offset: None,
        first_audio_packet_offset: None,
        setup_packet_at: None,
        first_audio_packet_at: None,
        setup_packet_size: None,
        num_samples: None,
        seek_table_count: None,
        seek_chunk_size: None,
        codec_version: None,
        mapping_family: None,
    };

    // Opus takes a different header path in the reference (`WwiseRIFFOpus.cs`): its
    // `fmt ` is at least `0x10` bytes, its extra-size word sits at `+0x10`, the
    // channel-layout word at `+0x14`, and the sample and frame counts at `+0x18`
    // and `+0x1C`. Its `fmt ` has no extra-size-versus-size consistency check and
    // no signature, so nothing below the codec word is shared with the Vorbis path.
    if codec == Codec::Opus {
        if fmt_size < 0x10 {
            return Err(Error::Vorbis(format!(
                "opus fmt chunk is {fmt_size:#x} bytes; the reference requires at least 0x10 \
                 (WwiseRIFFOpus.cs:97)"
            )));
        }
        // The reference requires both a data chunk and a seek table
        // (`WwiseRIFFOpus.cs:101`); without the seek table it cannot size the
        // frame table at all.
        if data.size == 0 {
            return Err(Error::MissingChunk("data"));
        }
        let Some(seek) = find(b"seek") else {
            return Err(Error::MissingChunk("seek"));
        };
        if seek.size == 0 {
            return Err(Error::Vorbis(
                "the opus seek chunk declares zero bytes (WwiseRIFFOpus.cs:101)".into(),
            ));
        }
        // The channel-layout word is only present when the extra-size word says so
        // (`WwiseRIFFOpus.cs:113-120`).
        let extra_size = rd.u16_at(fmt.offset + 0x10).unwrap_or(0);
        if extra_size >= 0x6 {
            header.subtype = rd.u32_at(fmt.offset + 0x14).unwrap_or(0);
        }
        header.num_samples = rd.u32_at(fmt.offset + 0x18);
        // The reference takes the frame count from the `fmt ` chunk and then reads
        // that many `u16` from the seek chunk (`WwiseRIFFOpus.cs:137`, `:184-188`),
        // so the two must agree for the conversion to stay inside the chunk.
        header.seek_table_count = rd.u32_at(fmt.offset + 0x1C);
        header.seek_chunk_size = Some(seek.size);
        header.codec_version = rd.u8_at(fmt.offset + 0x22);
        header.mapping_family = rd.u8_at(fmt.offset + 0x23);
        if let Some(version) = header.codec_version {
            if version != 1 {
                return Err(Error::BadField {
                    what: "the opus codec version byte",
                    found: version as u64,
                    wanted: 1,
                });
            }
        }
        if channels > 255 {
            return Err(Error::Vorbis(format!(
                "{} channels is more than the reference will remap (WwiseRIFFOpus.cs:130)",
                channels
            )));
        }
        // The reference's mapping matrix covers at most eight channels
        // (`WwiseRIFFOpus.cs:40-49`), so a remapping family with more channels
        // than that is a layout it has no table for (`WwiseRIFFOpus.cs:141-143`).
        if header.mapping_family == Some(1) && channels > 8 {
            return Err(Error::Vorbis(format!(
                "{channels} channels under mapping family 1 is more than the reference's \
                 mapping table covers (WwiseRIFFOpus.cs:141)"
            )));
        }
        header.packet_header_size = 0;
        // Opus carries no setup packet: the `fmt ` chunk *is* the header and the
        // `data` chunk opens at the first audio packet.
        header.first_audio_packet_at = Some(data.offset);
        return Ok(header);
    }

    if let Codec::Other(word) = codec {
        return Err(Error::BadField {
            what: "the fmt codec word",
            found: word as u64,
            wanted: Codec::Vorbis.word() as u64,
        });
    }

    // From here on the container is Vorbis, and the reference's two strict rules
    // about the two zero words apply (`WwiseRIFFVorbis.cs:166-167`).
    if block_align != 0 {
        return Err(Error::BadField {
            what: "the fmt block-align word",
            found: block_align as u64,
            wanted: 0,
        });
    }
    if bits_per_sample != 0 {
        return Err(Error::BadField {
            what: "the fmt bits-per-sample word",
            found: bits_per_sample as u64,
            wanted: 0,
        });
    }

    // ---- the Vorbis `fmt ` tail -------------------------------------------
    let extra_size = rd.need16(fmt.offset + 16, "the fmt extra-size word")?;
    if fmt_size.wrapping_sub(0x12) != extra_size as u32 {
        return Err(Error::BadField {
            what: "the fmt extra-size word",
            found: extra_size as u64,
            wanted: fmt_size.wrapping_sub(0x12) as u64,
        });
    }
    if fmt_size.wrapping_sub(0x12) >= 2 {
        header.ext_unk = rd.u16_at(fmt.offset + 18).unwrap_or(0);
    }
    if fmt_size.wrapping_sub(0x12) >= 6 {
        header.subtype = rd.u32_at(fmt.offset + 20).unwrap_or(0);
    }

    // A `0x28`-byte fmt chunk ends in a sixteen-byte signature. See
    // [`FMT_EXT_SIGNATURE`] for why this compares sixteen bytes against a
    // seventeen-byte array while the reference compares against all seventeen.
    if fmt_size == 0x28 {
        let at = fmt.offset + FMT_EXT_SIGNATURE_OFFSET;
        let got = bytes.get(at..at + FMT_EXT_SIGNATURE_CHECKED);
        header.ext_signature_ok =
            Some(got == Some(&FMT_EXT_SIGNATURE[..FMT_EXT_SIGNATURE_CHECKED]));
    }

    // The reference requires a `0x42` fmt chunk when there is no vorb, in which
    // case it fabricates one at `fmt ` + `0x18` (`WwiseRIFFVorbis.cs:147-155`).
    if vorb.is_some() && !matches!(fmt_size, 0x28 | 0x18 | 0x12) {
        return Err(Error::Vorbis(format!(
            "fmt chunk is {fmt_size:#x} bytes, which the reference does not accept alongside a \
             vorb chunk (it accepts 0x28, 0x18 and 0x12)"
        )));
    }
    if vorb_synthesised && fmt_size != 0x42 {
        return Err(Error::Vorbis(format!(
            "no vorb chunk, and the fmt chunk is {fmt_size:#x} bytes rather than the 0x42 the \
             reference requires when vorb is missing"
        )));
    }
    if vorb_synthesised {
        vorb = Some(Chunk {
            id: *b"vorb",
            offset: fmt.offset + 0x18,
            size: 0,
        });
    }
    let vorb = vorb.expect("a vorb chunk or its stand-in");

    // ---- cue and smpl -----------------------------------------------------
    // `cue_count` is already set from the chunk walk above; `smpl` is read here
    // because a loop cannot be validated until the sample count is known.
    //
    // **Deliberate difference from the reference**: it throws unless the loop count
    // is exactly one (`WwiseRIFFVorbis.cs:222`). A file with two loops is a
    // perfectly readable file, and refusing it would lose the whole container over
    // a number this layer already validates properly — the loop points are range-
    // checked against the sample count below, exactly as the reference does. So
    // [`loop_count`](Self::loop_count) is reported as found and every loop this
    // reader accepts is checked.
    if let Some(smpl) = find(b"smpl") {
        // Loop count at +0x1C, then start and end at +0x2C
        // (`WwiseRIFFVorbis.cs:219-226`).
        header.loop_count = rd.u32_at(smpl.offset + 0x1C).unwrap_or(0);
        header.loop_start = rd.u32_at(smpl.offset + 0x2C).unwrap_or(0);
        header.loop_end = rd.u32_at(smpl.offset + 0x30).unwrap_or(0);
    }

    // ---- vorb -------------------------------------------------------------
    let vorb_size = vorb.size;
    let Some(&(_, no_granule, seek_offset)) = VORB_SHAPES.iter().find(|s| s.0 == vorb_size) else {
        return Err(Error::Vorbis(format!(
            "vorb chunk is {vorb_size:#x} bytes; the reference accepts 0, 0x28, 0x2a, 0x2c, 0x32 and 0x34"
        )));
    };
    header.no_granule = no_granule;
    header.packet_header_size = if no_granule { 2 } else { 6 };
    header.sample_count = rd.u32_at(vorb.offset).unwrap_or(0);

    if no_granule {
        // The modification word at +0x04: the reference sets `mod_packets` unless
        // the word is one of four values (`WwiseRIFFVorbis.cs:272-274`).
        let mod_signal = rd.u32_at(vorb.offset + 0x04).unwrap_or(0);
        header.mod_packets = !matches!(mod_signal, 0x4A | 0x4B | 0x69 | 0x70);
    }

    let setup_rel = rd.u32_at(vorb.offset + seek_offset);
    let first_rel = rd.u32_at(vorb.offset + seek_offset + 4);
    header.setup_packet_offset = setup_rel;
    header.first_audio_packet_offset = first_rel;

    match vorb_size {
        0x28 | 0x2C => {
            // The reference leaves uid and the blocksizes unset here
            // (`WwiseRIFFVorbis.cs:305-310`).
            header.header_triad_present = true;
            header.old_packet_headers = true;
        }
        0x00 | 0x2A => {
            header.uid = rd.u32_at(vorb.offset + 0x24);
            header.blocksize0_pow = rd.u8_at(vorb.offset + 0x28);
            header.blocksize1_pow = rd.u8_at(vorb.offset + 0x29);
        }
        0x32 | 0x34 => {
            header.uid = rd.u32_at(vorb.offset + 0x2C);
            header.blocksize0_pow = rd.u8_at(vorb.offset + 0x30);
            header.blocksize1_pow = rd.u8_at(vorb.offset + 0x31);
        }
        _ => unreachable!("the size was matched by VORB_SHAPES above"),
    }

    // ---- loop adjustment --------------------------------------------------
    // Transcribed from `WwiseRIFFVorbis.cs:325-341`: an end of zero means "to the
    // end of the file", otherwise the stored end is a last-sample index and
    // becomes a count. A loop that does not fit the sample count is dropped.
    if header.loop_count != 0 {
        if header.loop_end == 0 {
            header.loop_end = header.sample_count;
        } else {
            header.loop_end += 1;
        }
        if header.loop_start > header.loop_end {
            std::mem::swap(&mut header.loop_start, &mut header.loop_end);
        }
        if header.loop_start > header.sample_count
            || header.loop_end > header.sample_count
            || header.loop_start > header.loop_end
        {
            header.loop_start = 0;
            header.loop_end = 0;
            header.loop_count = 0;
        }
    }

    // ---- packet locations -------------------------------------------------
    // The setup packet sits at `data_offset + setup_packet_offset`, and its
    // declared size is the first word of the packet header there. Reading it is a
    // bounds check plus one read; nothing about the packet's contents is
    // interpreted.
    if let Some(rel) = setup_rel {
        if let Some(at) = data
            .offset
            .checked_add(rel as usize)
            .filter(|a| *a < data.end())
        {
            header.setup_packet_at = Some(at);
            header.setup_packet_size = rd.u16_at(at);
            if let Some(first) = first_rel {
                header.first_audio_packet_at = data
                    .offset
                    .checked_add(first as usize)
                    .filter(|a| *a < data.end());
            }
        }
    }

    Ok(header)
}

/// A WEM payload that was read, with its asset identity.
#[derive(Debug, Clone, PartialEq)]
pub struct WemFile {
    pub asset: u64,
    /// Size the manifest recorded for the asset, when known.
    pub manifest_size: Option<u32>,
    pub header: WemHeader,
}

// ---------------------------------------------------------------------------
// Metadata readers
// ---------------------------------------------------------------------------

/// `Enum_0C014B4A`, the voice set's gender (`Enums/Misc.cs:325`). The only enum
/// anywhere on this surface with members the reference can name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gender {
    Generic,
    Male,
    Female,
    Unnamed(i32),
}

impl Gender {
    pub fn from_raw(raw: i32) -> Gender {
        match raw {
            1 => Gender::Generic,
            2 => Gender::Male,
            3 => Gender::Female,
            other => Gender::Unnamed(other),
        }
    }
}

impl fmt::Display for Gender {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Gender::Generic => f.write_str("generic"),
            Gender::Male => f.write_str("male"),
            Gender::Female => f.write_str("female"),
            Gender::Unnamed(v) => write!(f, "unnamed({v})"),
        }
    }
}

/// Every numeric field on an instance, by width, keyed by field hash.
///
/// This exists so nothing on a class is lost. A four-byte field is ambiguous
/// between `u32` and `f32` on the wire, so it is carried as raw bits and read
/// through [`Numbers::f32`] or [`Numbers::u32`]; the caller decides which, from
/// what the reference declares the field to be.
///
/// Strings are read here because `read_string` is unambiguous. Asset references
/// are **not**: an eight-byte field that is not a reference reads as noise, so
/// only fields the reference declares as references are read as such.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Numbers {
    /// Raw four-byte fields.
    pub words: BTreeMap<u32, u32>,
    /// One-byte fields.
    pub bytes: BTreeMap<u32, u8>,
    /// `teString` fields, decoded.
    pub strings: BTreeMap<u32, String>,
}

impl Numbers {
    /// A four-byte field as a float. No transformation: the bits are read as an
    /// IEEE-754 single.
    pub fn f32(&self, hash: u32) -> Option<f32> {
        self.words.get(&hash).map(|v| f32::from_bits(*v))
    }

    /// A four-byte field as a signed integer.
    pub fn i32(&self, hash: u32) -> Option<i32> {
        self.words.get(&hash).map(|v| *v as i32)
    }

    /// A four-byte field as an unsigned integer.
    pub fn u32(&self, hash: u32) -> Option<u32> {
        self.words.get(&hash).copied()
    }

    /// A one-byte field.
    pub fn u8(&self, hash: u32) -> Option<u8> {
        self.bytes.get(&hash).copied()
    }

    /// A string field.
    pub fn string(&self, hash: u32) -> Option<&str> {
        self.strings.get(&hash).map(|s| s.as_str())
    }

    /// Collect every field of the given widths off one instance.
    pub fn read(stu: &mut Stu, instance: usize) -> Numbers {
        // Resolve the layout once: calling `fields` per candidate would recompute
        // it, and the offsets are what every branch below needs.
        let layout: Vec<(u32, usize, usize)> = stu
            .fields(instance)
            .iter()
            .map(|f| (f.hash, f.offset, f.size))
            .collect();
        let base = stu.payload_start(instance);
        let mut out = Numbers::default();
        // Narrow widths first. `Stu` has no one-byte accessor, so the instance
        // payload is indexed directly, and `data` is already clamped to the
        // instance's own length.
        {
            let payload = stu.data(instance);
            for (hash, offset, size) in &layout {
                match size {
                    1 => {
                        if let Some(v) = payload.get(*offset).copied() {
                            out.bytes.insert(*hash, v);
                        }
                    }
                    4 => {
                        if let Some(v) = stu.u32_at(base + offset) {
                            out.words.insert(*hash, v);
                        }
                    }
                    _ => {}
                }
            }
        }
        // Then the 16-byte fields, which need the mutable borrow `read_string`
        // takes.
        //
        // A 16-byte field is either a `teString` or an asset reference, and the
        // two read differently. `read_string` returns `None` for bytes that are
        // not a length-prefixed string, so trying it here is the safe order: a
        // mis-read reference yields no string rather than a plausible one.
        for (hash, _offset, size) in layout {
            if size == 16 {
                if let Some(s) = stu.read_string(instance, hash) {
                    out.strings.insert(hash, s);
                }
            }
        }
        out
    }
}

/// `STU_0FA0034C`: the Wwise media block embedded in a sound (`Misc_4.cs:604`).
///
/// This is the block that matters. `m_soundWEMFiles` is a reference array of
/// payload assets, and `m_wwiseWEMFileIDs` is a `uint[]` running parallel to it —
/// `Combo.cs:942-947` zips the two, so element `i` of one keys element `i` of the
/// other. Carrying both is what lets a caller name a file by its Wwise media id as
/// well as by GUID.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WemBlock {
    /// `m_FA1FDF0D`.
    pub name: Option<String>,
    /// `m_soundWEMFiles`: the pre-loaded payloads.
    pub wem_files: Vec<u64>,
    /// `m_soundWEMStreams`: the streamed payloads.
    pub wem_streams: Vec<u64>,
    /// `m_wwiseWEMFileIDs`, parallel to [`wem_files`](Self::wem_files).
    pub wwise_file_ids: Vec<u32>,
    /// `m_wwiseWEMStreamIDs`, parallel to [`wem_streams`](Self::wem_streams).
    pub wwise_stream_ids: Vec<u32>,
    /// `m_soundBank`.
    pub sound_bank: Option<u64>,
    /// `m_wwiseEventIDPlay`.
    pub event_play: Option<u32>,
    /// `m_wwiseEventIDStop`.
    pub event_stop: Option<u32>,
    /// Every remaining numeric and string field on the block.
    pub numbers: Numbers,
}

impl WemBlock {
    /// The pre-loaded payloads paired with the Wwise media id that keys each.
    ///
    /// Zipped exactly as `Combo.cs:942-947` does it: a shorter id list means the
    /// tail pairs with id 0 rather than being dropped, which is what the
    /// reference's own indexing does.
    pub fn files_with_ids(&self) -> Vec<(u32, u64)> {
        self.wem_files
            .iter()
            .enumerate()
            .map(|(i, guid)| (self.wwise_file_ids.get(i).copied().unwrap_or(0), *guid))
            .collect()
    }

    fn read_in(stu: &mut Stu, instance: usize) -> WemBlock {
        WemBlock {
            name: stu.read_string(instance, field::wwise_block::NAME),
            wem_files: read_ref_array(stu, instance, field::wwise_block::WEM_FILES),
            wem_streams: read_ref_array(stu, instance, field::wwise_block::WEM_STREAMS),
            wwise_file_ids: read_u32_array(stu, instance, field::wwise_block::WWISE_FILE_IDS),
            wwise_stream_ids: read_u32_array(stu, instance, field::wwise_block::WWISE_STREAM_IDS),
            sound_bank: stu.read_asset_ref(instance, field::wwise_block::SOUND_BANK),
            event_play: stu.read_u32(instance, field::wwise_block::EVENT_PLAY),
            event_stop: stu.read_u32(instance, field::wwise_block::EVENT_STOP),
            numbers: Numbers::read(stu, instance),
        }
    }

    /// Read a media block out of an inline array element — the shape a sound
    /// bank's `m_2075DBF6` entries take.
    fn read_element(stu: &Stu, element: &Element) -> WemBlock {
        WemBlock {
            name: element.string(stu, field::wwise_block::NAME),
            wem_files: read_ref_array_at(stu, element, field::wwise_block::WEM_FILES),
            wem_streams: read_ref_array_at(stu, element, field::wwise_block::WEM_STREAMS),
            wwise_file_ids: read_u32_array_at(stu, element, field::wwise_block::WWISE_FILE_IDS),
            wwise_stream_ids: read_u32_array_at(stu, element, field::wwise_block::WWISE_STREAM_IDS),
            sound_bank: element.asset_ref(stu, field::wwise_block::SOUND_BANK),
            event_play: element.u32(stu, field::wwise_block::EVENT_PLAY),
            event_stop: element.u32(stu, field::wwise_block::EVENT_STOP),
            numbers: Numbers::default(),
        }
    }
}

/// `STUSound` (`STUSound.cs:7`), the type of every placeable sound.
#[derive(Debug, Clone, PartialEq)]
pub struct Sound {
    pub asset: u64,
    /// The instance's own class. A subclass is not the same record, so this is
    /// reported rather than assumed equal to [`class::SOUND`].
    pub root_class: u32,
    /// `m_42809FC5`: the sound this one derives from, when it declares one.
    pub parent_sound: Option<u64>,
    /// `m_C32C2195`, the Wwise media block.
    pub wwise: Option<WemBlock>,
    pub numbers: Numbers,
}

impl Sound {
    /// Every payload asset this sound can play, pre-loaded list first.
    pub fn payload_assets(&self) -> Vec<u64> {
        let mut out = Vec::new();
        if let Some(block) = &self.wwise {
            out.extend(block.wem_files.iter().copied().filter(|g| *g != 0));
            out.extend(block.wem_streams.iter().copied().filter(|g| *g != 0));
        }
        out
    }

    /// The pre-loaded payloads keyed by Wwise media id.
    pub fn payload_with_ids(&self) -> Vec<(u32, u64)> {
        self.wwise
            .as_ref()
            .map(|b| b.files_with_ids())
            .unwrap_or_default()
    }

    fn read_in(stu: &mut Stu, asset: u64) -> Option<Sound> {
        let instance = stu.instance(class::SOUND)?;
        Some(Sound {
            asset,
            root_class: stu.instances[instance].hash,
            parent_sound: stu.read_asset_ref(instance, field::sound::PARENT),
            wwise: embedded(stu, instance, class::WWISE_BLOCK).map(|i| WemBlock::read_in(stu, i)),
            numbers: Numbers::read(stu, instance),
        })
    }
}

/// `STUSoundSwitch` (`STUSoundSwitch.cs:6`).
#[derive(Debug, Clone, PartialEq)]
pub struct SoundSwitch {
    pub asset: u64,
    pub root_class: u32,
    /// `m_032194C8`, the `STU_11B8F80B` this switch belongs to.
    pub group: Option<u64>,
    /// `m_BB01A3CC`.
    pub name: Option<String>,
    /// `m_E87C7AF9`.
    pub value: Option<u32>,
    pub numbers: Numbers,
}

impl SoundSwitch {
    fn read_in(stu: &mut Stu, asset: u64) -> Option<SoundSwitch> {
        let instance = stu.instance(class::SOUND_SWITCH)?;
        Some(SoundSwitch {
            asset,
            root_class: stu.instances[instance].hash,
            group: stu.read_asset_ref(instance, field::switch_::GROUP),
            name: stu.read_string(instance, field::switch_::NAME),
            value: stu.read_u32(instance, field::switch_::UNNAMED_E87C7AF9),
            numbers: Numbers::read(stu, instance),
        })
    }
}

/// `STUSoundParameter` (`STUSoundParameter.cs:6`).
#[derive(Debug, Clone, PartialEq)]
pub struct SoundParameter {
    pub asset: u64,
    pub root_class: u32,
    /// `m_C27830B3`.
    pub name: Option<String>,
    /// `m_B04205C5`.
    pub value: Option<u32>,
    pub numbers: Numbers,
}

impl SoundParameter {
    fn read_in(stu: &mut Stu, asset: u64) -> Option<SoundParameter> {
        let instance = stu.instance(class::SOUND_PARAMETER)?;
        Some(SoundParameter {
            asset,
            root_class: stu.instances[instance].hash,
            name: stu.read_string(instance, field::parameter::NAME),
            value: stu.read_u32(instance, field::parameter::UNNAMED_B04205C5),
            numbers: Numbers::read(stu, instance),
        })
    }
}

/// `STUSoundBank` (`STUSoundBank.cs:6`): the Wwise bank a sound's media lives in.
///
/// **Not tested against real data, and on this install it does not apply.** A
/// `Sound`'s `m_soundBank` field does point at a type-`0x043` asset, but those
/// assets are **raw Wwise banks**, opening with the four bytes `BKHD`, not STU
/// assets: measured 38 of 40 sampled assets of type `0x043` open with `BKHD` and
/// none parse as STU. So the generated `STUSoundBank` class is the reference's
/// typed *view* of a bank rather than the file's own layout, and this reader
/// finds nothing here. The reader is kept because it costs nothing and because a
/// bank that *were* wrapped would be this shape; it is kept honestly by saying so.
#[derive(Debug, Clone, PartialEq)]
pub struct SoundBank {
    pub asset: u64,
    pub root_class: u32,
    /// `m_F99A0938`.
    pub name: Option<String>,
    /// `m_27D46586`, the bank's second string. The reference does not read it and
    /// neither does this.
    pub unnamed_27d46586: Option<String>,
    /// `m_wwiseBankID`. The one numeric field on this surface the reference names
    /// for real.
    pub wwise_bank_id: Option<u32>,
    /// `m_2075DBF6`: the bank's inline media list, one entry per embedded
    /// `STU_0FA0034C`.
    pub media: Vec<WemBlock>,
}

impl SoundBank {
    /// The bank's Wwise media ids, one per media entry, in declaration order.
    pub fn media_ids(&self) -> Vec<u32> {
        self.media
            .iter()
            .map(|b| b.wwise_file_ids.first().copied().unwrap_or(0))
            .collect()
    }

    /// How many payload assets the bank names.
    pub fn payload_count(&self) -> usize {
        self.media
            .iter()
            .map(|b| b.wem_files.len() + b.wem_streams.len())
            .sum()
    }

    fn read_in(stu: &mut Stu, asset: u64) -> Option<SoundBank> {
        let instance = stu.instance(class::SOUND_BANK)?;
        // `m_2075DBF6` is an inline array of `STU_0FA0034C`, so its elements are
        // read as [`Element`]s rather than looked up in the instance table.
        let elements = Element::read_all(stu, instance, field::sound_bank::MEDIA);
        let media = elements
            .iter()
            .map(|element| WemBlock::read_element(stu, element))
            .collect();
        Some(SoundBank {
            asset,
            root_class: stu.instances[instance].hash,
            name: stu.read_string(instance, field::sound_bank::NAME),
            unnamed_27d46586: stu.read_string(instance, field::sound_bank::UNNAMED_27D46586),
            wwise_bank_id: stu.read_u32(instance, field::sound_bank::WWISE_BANK_ID),
            media,
        })
    }
}

/// `STUVoiceLine` (`STUVoiceLine.cs:6`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VoiceLine {
    /// `m_stimulus`: what triggers the line.
    pub stimulus: Option<u64>,
    /// `m_externalSound`: a sound played instead of a voice line.
    pub external_sound: Option<u64>,
    /// `m_set`: the line set this line belongs to.
    pub set: Option<u64>,
    /// `m_6148094F`, which `Combo.cs:1059` reads as the subtitle.
    pub subtitle: Option<u64>,
    /// `m_BD1B6F64`.
    pub conversations: Vec<u64>,
    /// `m_weight`, the line's selection weight.
    pub weight: Option<f32>,
    pub numbers: Numbers,
}

impl VoiceLine {
    /// Read a line out of an instance in the instance table — the shape a
    /// `m_voiceLineRuntime` embedded field points at.
    fn read_embedded(stu: &mut Stu, instance: usize) -> VoiceLine {
        VoiceLine {
            stimulus: stu.read_asset_ref(instance, field::voice_line::STIMULUS),
            external_sound: stu.read_asset_ref(instance, field::voice_line::EXTERNAL_SOUND),
            set: stu.read_asset_ref(instance, field::voice_line::SET),
            subtitle: stu.read_asset_ref(instance, field::voice_line::SUBTITLE),
            conversations: read_ref_array(stu, instance, field::voice_line::CONVERSATIONS),
            weight: stu.read_f32(instance, field::voice_line::WEIGHT),
            numbers: Numbers::read(stu, instance),
        }
    }

    /// Read a line out of an inline array element.
    ///
    /// Carries fewer fields than [`read_embedded`](Self::read_embedded): there is
    /// no instance, so there is nothing to sweep for unnamed numbers. That is a
    /// real difference in what this reader can report, not an oversight.
    fn read_element(stu: &Stu, element: &Element) -> VoiceLine {
        VoiceLine {
            stimulus: element.asset_ref(stu, field::voice_line::STIMULUS),
            external_sound: element.asset_ref(stu, field::voice_line::EXTERNAL_SOUND),
            set: element.asset_ref(stu, field::voice_line::SET),
            subtitle: element.asset_ref(stu, field::voice_line::SUBTITLE),
            conversations: read_ref_array_at(stu, element, field::voice_line::CONVERSATIONS),
            weight: element.f32(stu, field::voice_line::WEIGHT),
            numbers: Numbers::default(),
        }
    }
}

/// One line of a voice set: a stimulus, a weight, and the payloads it can play.
///
/// This is the audio-side sibling of [`crate::unlocks::VoiceConversation`]: the
/// conversation asset says *which* lines exist, and a voice set says *what each
/// one plays*.
#[derive(Debug, Clone, PartialEq)]
pub struct VoiceLineInstance {
    /// The `STUVoiceLine` this instance wraps, read off the embedded field.
    pub line: Option<VoiceLine>,
    /// `m_AF226247`'s payload assets, reached through `STU_F746901F` and
    /// `STU_D744301C` exactly as `Combo.cs:1073-1081` walks them.
    pub payloads: Vec<u64>,
    /// `m_resourceKey`, a `ulong` rather than a reference.
    pub resource_key: Option<u64>,
}

impl VoiceLineInstance {
    /// Every payload this line instance can play, from both places it can name
    /// one, de-duplicated in order.
    pub fn all_payloads(&self) -> Vec<u64> {
        let mut out = self.payloads.clone();
        if let Some(line) = &self.line {
            let external = line.external_sound.unwrap_or(0);
            if external != 0 && !out.contains(&external) {
                out.push(external);
            }
        }
        out
    }
}

/// `STUVoiceSet` (`STUVoiceSet.cs:7`): a character's whole voice repertoire.
#[derive(Debug, Clone, PartialEq)]
pub struct VoiceSet {
    pub asset: u64,
    pub root_class: u32,
    /// `m_externalSound`, the same hash `STUVoiceLine` uses
    /// (`STUVoiceLine.cs:13`) — which is what identifies it.
    pub external_sound: Option<u64>,
    /// `m_gender`.
    pub gender: Option<Gender>,
    /// `m_voiceLineInstances`, one entry per line the set can play.
    pub lines: Vec<VoiceLineInstance>,
    pub numbers: Numbers,
}

impl VoiceSet {
    /// How many of the set's lines declare a stimulus, which is what makes a set
    /// addressable by *what happened in the game* rather than by index.
    pub fn lines_with_stimulus(&self) -> usize {
        self.lines
            .iter()
            .filter(|l| l.line.as_ref().and_then(|l| l.stimulus).unwrap_or(0) != 0)
            .count()
    }

    /// Every distinct payload asset any of the set's lines can play.
    pub fn payload_assets(&self) -> Vec<u64> {
        let mut out: Vec<u64> = Vec::new();
        for line in &self.lines {
            for guid in line.all_payloads() {
                if guid != 0 && !out.contains(&guid) {
                    out.push(guid);
                }
            }
        }
        out
    }

    /// The distinct stimuli the set answers, in declaration order. This is the
    /// "in which situations does this character speak" list.
    pub fn stimuli(&self) -> Vec<u64> {
        let mut out: Vec<u64> = Vec::new();
        for line in &self.lines {
            let s = line.line.as_ref().and_then(|l| l.stimulus).unwrap_or(0);
            if s != 0 && !out.contains(&s) {
                out.push(s);
            }
        }
        out
    }

    fn read_in(stu: &mut Stu, asset: u64) -> Option<VoiceSet> {
        let instance = stu.instance(class::VOICE_SET)?;

        // `m_voiceLineInstances` is an **inline** instance array, so its elements
        // are not in `Stu::instances`. Reaching them through the instance table
        // finds nothing at all — which is exactly what a first attempt did, and why
        // every voice set on the reference install read as having zero lines.
        let elements = Element::read_all(stu, instance, field::voice_set::LINE_INSTANCES);
        let mut lines = Vec::with_capacity(elements.len());
        for element in &elements {
            // `STU_F746901F` holds four wrappers; each wrapper's `m_3C099E86` is
            // a payload asset. `Combo.cs:1074-1079` reads exactly that. The holder
            // is a real embedded instance, so its own embedded wrappers are still
            // reached through the instance table.
            let mut payloads = Vec::new();
            if let Some(holder) = element.embedded(stu, field::voice_line_instance::WEMS) {
                for holder_field in field::VOICE_WEM_FIELDS {
                    let Some(wrapper) = embedded_by_field(stu, holder, holder_field) else {
                        continue;
                    };
                    if let Some(payload) = stu.read_asset_ref(wrapper, field::wem::PAYLOAD) {
                        if payload != 0 {
                            payloads.push(payload);
                        }
                    }
                }
            }

            lines.push(VoiceLineInstance {
                line: match element.embedded(stu, field::voice_line_instance::VOICE_LINE) {
                    Some(line) => Some(VoiceLine::read_embedded(stu, line)),
                    None => Some(VoiceLine::read_element(stu, element)),
                },
                payloads,
                resource_key: element
                    .u64(stu, field::voice_line_instance::RESOURCE_KEY)
                    .filter(|v| *v != 0),
            });
        }

        Some(VoiceSet {
            asset,
            root_class: stu.instances[instance].hash,
            external_sound: stu.read_asset_ref(instance, field::voice_set::EXTERNAL_SOUND),
            gender: stu
                .read_u32(instance, field::voice_set::GENDER)
                .map(|v| Gender::from_raw(v as i32)),
            lines,
            numbers: Numbers::read(stu, instance),
        })
    }
}

/// `STUVoiceLineSet` (`STUVoiceLineSet.cs:6`).
///
/// **Its line references resolve to nothing, and that is what the data is.**
/// Measured on the reference install: a readable set names between 2 and 1413
/// lines, and **not one** of them names an asset in the registry or in the speech
/// manifest. Every one decodes to asset type [`types::VOICE_LINE_REF`] (`0x06F`),
/// with the type bits identical across a whole set and the index counting up — the
/// shape of a correctly decoded table of same-type references to files this install
/// does not hold. The reference declares the field as
/// `teStructuredDataAssetRef<STUVoiceLine>[]` (`STUVoiceLineSet.cs:10`) but
/// `STUVoiceLine` is an *embedded* class with no asset type of its own, so this
/// cannot be followed to a line. Do not treat a zero-resolution census of this type
/// as a broken reader.
#[derive(Debug, Clone, PartialEq)]
pub struct VoiceLineSet {
    pub asset: u64,
    pub root_class: u32,
    /// `m_0DD426F8`. See the note on the type: these do not resolve to assets.
    pub lines: Vec<u64>,
}

impl VoiceLineSet {
    fn read_in(stu: &mut Stu, asset: u64) -> Option<VoiceLineSet> {
        let instance = stu.instance(class::VOICE_LINE_SET)?;
        Some(VoiceLineSet {
            asset,
            root_class: stu.instances[instance].hash,
            lines: read_ref_array(stu, instance, field::voice_line_set::LINES),
        })
    }
}

/// `STU_FBCC5EB2`, the wrapper a sound's WEM lists point at (`Misc_2.cs:9010`).
///
/// The reference reads its `m_fileSize` — which is the audio payload's size — and
/// stops there. So does this.
#[derive(Debug, Clone, PartialEq)]
pub struct WemWrapper {
    pub asset: u64,
    pub root_class: u32,
    /// `m_fileSize`, from the base `STU_39091531`.
    pub file_size: Option<u32>,
    /// `m_267B9B97`, the base's reference array.
    pub refs: Vec<u64>,
    /// `m_F99A0938` — the same hash and offset `STUSoundBank` uses, which is what
    /// identifies it as the wrapper's own name.
    pub name: Option<String>,
    /// `m_770CFAB9`, the wrapper's second string.
    pub unnamed_770cfab9: Option<String>,
    pub numbers: Numbers,
}

impl WemWrapper {
    fn read_in(stu: &mut Stu, asset: u64) -> Option<WemWrapper> {
        let instance = stu
            .instances
            .iter()
            .position(|i| i.hash == class::WEM_FILE)?;
        Some(WemWrapper {
            asset,
            root_class: stu.instances[instance].hash,
            file_size: stu.read_u32(instance, field::wem_wrapper::FILE_SIZE),
            refs: read_ref_array(stu, instance, field::wem_wrapper::REFS),
            name: stu.read_string(instance, field::sound_bank::NAME),
            unnamed_770cfab9: stu.read_string(instance, field::WEM_WRAPPER_NAME_2),
            numbers: Numbers::read(stu, instance),
        })
    }
}

// ---------------------------------------------------------------------------
// Whole-asset readers
// ---------------------------------------------------------------------------

/// Read a [`Sound`] out of an already-parsed asset.
///
/// This is the same reader [`read_sound`] uses, exposed for callers that already
/// hold a [`Stu`] — the CLI following a sound to its payloads, for instance. It
/// returns `None` when the asset has no `STUSound` instance, which is what "this
/// asset is not a sound" looks like.
pub fn read_sound_in(stu: &mut Stu, asset: u64) -> Option<Sound> {
    Sound::read_in(stu, asset)
}

/// Read a [`VoiceSet`] out of an already-parsed asset.
pub fn read_voice_set_in(stu: &mut Stu, asset: u64) -> Option<VoiceSet> {
    VoiceSet::read_in(stu, asset)
}

/// Read a [`VoiceLineSet`] out of an already-parsed asset.
///
/// The speech manifest holds no reader of its own for these, so a caller walking
/// the speech side opens the asset's bytes itself and comes through here.
pub fn read_voice_line_set_in(stu: &mut Stu, asset: u64) -> Option<VoiceLineSet> {
    VoiceLineSet::read_in(stu, asset)
}

/// Read one [`Sound`] by GUID.
pub fn read_sound(data: &GameData, asset: u64) -> Result<Sound> {
    let mut stu = data.stu_of(asset)?;
    Sound::read_in(&mut stu, asset).ok_or_else(|| {
        crate::Error::NotFound(format!("sound 0x{asset:016x} has no STUSound instance"))
    })
}

/// Read one [`SoundSwitch`] by GUID.
pub fn read_sound_switch(data: &GameData, asset: u64) -> Result<SoundSwitch> {
    let mut stu = data.stu_of(asset)?;
    SoundSwitch::read_in(&mut stu, asset).ok_or_else(|| {
        crate::Error::NotFound(format!(
            "sound switch 0x{asset:016x} has no switch instance"
        ))
    })
}

/// Read one [`SoundParameter`] by GUID.
pub fn read_sound_parameter(data: &GameData, asset: u64) -> Result<SoundParameter> {
    let mut stu = data.stu_of(asset)?;
    SoundParameter::read_in(&mut stu, asset).ok_or_else(|| {
        crate::Error::NotFound(format!(
            "sound parameter 0x{asset:016x} has no parameter instance"
        ))
    })
}

/// Read one [`SoundBank`] by GUID.
pub fn read_sound_bank(data: &GameData, asset: u64) -> Result<SoundBank> {
    let mut stu = data.stu_of(asset)?;
    SoundBank::read_in(&mut stu, asset).ok_or_else(|| {
        crate::Error::NotFound(format!("sound bank 0x{asset:016x} has no bank instance"))
    })
}

/// Read one [`VoiceSet`] by GUID.
pub fn read_voice_set(data: &GameData, asset: u64) -> Result<VoiceSet> {
    let mut stu = data.stu_of(asset)?;
    VoiceSet::read_in(&mut stu, asset).ok_or_else(|| {
        crate::Error::NotFound(format!(
            "voice set 0x{asset:016x} has no voice-set instance"
        ))
    })
}

/// Read one [`VoiceLineSet`] by GUID.
pub fn read_voice_line_set(data: &GameData, asset: u64) -> Result<VoiceLineSet> {
    let mut stu = data.stu_of(asset)?;
    VoiceLineSet::read_in(&mut stu, asset).ok_or_else(|| {
        crate::Error::NotFound(format!(
            "voice line set 0x{asset:016x} has no line-set instance"
        ))
    })
}

/// Read one [`WemWrapper`] — a `STU_FBCC5EB2` — by GUID.
///
/// The wrapper is not an asset type of its own: it is what a
/// [`Sound`]'s `m_soundWEMFiles` entries point at, so it is reached by
/// following [`Sound::payload_assets`] rather than by surveying a type. What it
/// adds over the payload is `m_fileSize`, the declared audio size, which is
/// readable even when the payload itself is not downloaded.
pub fn read_wem_wrapper(data: &GameData, asset: u64) -> Result<WemWrapper> {
    let mut stu = data.stu_of(asset)?;
    WemWrapper::read_in(&mut stu, asset).ok_or_else(|| {
        crate::Error::NotFound(format!(
            "0x{asset:016x} has no STU_FBCC5EB2 (0x{:08X}) wrapper instance",
            class::WEM_FILE
        ))
    })
}

/// Parse the header of one WEM payload asset by GUID.
///
/// This is the "read a sound file" entry point: it opens the asset, walks its RIFF
/// header, and stops. It hands the bytes back as well, so a caller that wants to
/// give the payload to an external tool has everything in hand.
pub fn read_wem(data: &GameData, asset: u64) -> std::result::Result<(WemHeader, Vec<u8>), Error> {
    let bytes = data
        .bytes_of(asset)
        .map_err(|e| Error::Vorbis(format!("asset 0x{asset:016x} could not be opened: {e}")))?;
    let header = parse_wem(&bytes)?;
    Ok((header, bytes))
}

/// Every sound asset in the loaded manifest, in registry order.
pub fn sounds(data: &GameData) -> Vec<Sound> {
    read_type(data, types::SOUND, Sound::read_in)
}

/// Every sound switch in the loaded manifest.
pub fn sound_switches(data: &GameData) -> Vec<SoundSwitch> {
    read_type(data, types::SOUND_SWITCH, SoundSwitch::read_in)
}

/// Every sound parameter in the loaded manifest.
pub fn sound_parameters(data: &GameData) -> Vec<SoundParameter> {
    read_type(data, types::SOUND_PARAMETER, SoundParameter::read_in)
}

/// Every voice set in the loaded manifest.
pub fn voice_sets(data: &GameData) -> Vec<VoiceSet> {
    read_type(data, types::VOICE_SET, VoiceSet::read_in)
}

/// The GUID of the voice set a hero's gameplay entity names, found by walking
/// the entity for [`class::VOICE_SET_COMPONENT`].
///
/// **This is the join that makes voice reachable from gameplay**: a hero's
/// `STUVoiceSetComponent` (`STUVoiceSetComponent.cs:6`) names the voice set at
/// field `0x8A4FF89C`. It lives here rather than in [`crate::entity`] because
/// that module is not this file's to change; the report notes it as a place that
/// should move.
///
/// Returns `None` when the entity declares no voice-set component, which is normal
/// for a non-hero entity.
///
/// **This returns a GUID, not a [`VoiceSet`], and that is deliberate.** The set it
/// names lives in the *speech* manifest, which [`GameData`] does not load, so a
/// caller has to read it through a [`SpeechIndex`]. See [`voice_set_of_entity`]
/// below for the narrower function this sits above.
pub fn voice_set_ref_of_entity(data: &GameData, entity: u64) -> Result<Option<u64>> {
    let mut stu = data.stu_of(entity)?;
    for i in 0..stu.instances.len() {
        if stu.instances[i].hash != class::VOICE_SET_COMPONENT {
            continue;
        }
        let Some(voice_set) = stu.read_asset_ref(i, field::voice_set_component::VOICE_SET) else {
            continue;
        };
        if voice_set == 0 {
            continue;
        }
        return Ok(Some(voice_set));
    }
    Ok(None)
}

/// Read the voice set a hero's gameplay entity names, in one step.
///
/// **Not tested end to end, and it cannot work against a stock install.** The set
/// is in the speech manifest and [`GameData`] does not load it, so on the reference
/// install this always yields `None` — measured, not assumed. Use
/// [`voice_set_ref_of_entity`] and then read through a [`SpeechIndex`]; that path
/// *is* exercised on real data.
pub fn voice_set_of_entity(data: &GameData, entity: u64) -> Result<Option<VoiceSet>> {
    let Some(voice_set) = voice_set_ref_of_entity(data, entity)? else {
        return Ok(None);
    };
    let mut stu = data.stu_of(entity)?;
    Ok(VoiceSet::read_in(&mut stu, voice_set))
}

// ---------------------------------------------------------------------------
// Survey: the type_histogram analogue for the sound surface
// ---------------------------------------------------------------------------

/// One readable record, whatever sound surface it came from.
#[derive(Debug, Clone, PartialEq)]
pub enum Record {
    Sound(Box<Sound>),
    SoundSwitch(Box<SoundSwitch>),
    SoundParameter(Box<SoundParameter>),
    SoundBank(Box<SoundBank>),
    VoiceSet(Box<VoiceSet>),
    VoiceLineSet(Box<VoiceLineSet>),
    WemWrapper(Box<WemWrapper>),
    /// A Wwise RIFF container: the header was read and no audio was decoded.
    Wem(Box<WemFile>),
    /// The asset parsed, and this reader has no class for its root. Recorded
    /// rather than skipped: "there is a shape here nobody has read" is exactly
    /// what a survey exists to report.
    Unnamed {
        asset: u64,
        root_class: u32,
        instances: usize,
    },
}

impl Record {
    /// The asset GUID this record came from.
    pub fn asset(&self) -> u64 {
        match self {
            Record::Sound(r) => r.asset,
            Record::SoundSwitch(r) => r.asset,
            Record::SoundParameter(r) => r.asset,
            Record::SoundBank(r) => r.asset,
            Record::VoiceSet(r) => r.asset,
            Record::VoiceLineSet(r) => r.asset,
            Record::WemWrapper(r) => r.asset,
            Record::Wem(r) => r.asset,
            Record::Unnamed { asset, .. } => *asset,
        }
    }

    /// The root instance class, where the class is the record's identity. Zero
    /// for the WEM payload types, which are not STU assets at all.
    pub fn root_class(&self) -> u32 {
        match self {
            Record::Sound(r) => r.root_class,
            Record::SoundSwitch(r) => r.root_class,
            Record::SoundParameter(r) => r.root_class,
            Record::SoundBank(r) => r.root_class,
            Record::VoiceSet(r) => r.root_class,
            Record::VoiceLineSet(r) => r.root_class,
            Record::WemWrapper(r) => r.root_class,
            Record::Unnamed { root_class, .. } => *root_class,
            Record::Wem(_) => 0,
        }
    }

    /// A one-line summary, for the CLI.
    ///
    /// Every number in it is read from the asset. The one exception is a duration,
    /// which is a quotient of two read values and is marked with `~`.
    pub fn summary(&self) -> String {
        fn guid_or_none(g: Option<u64>) -> String {
            match g {
                Some(g) if g != 0 => format!("0x{g:016x}"),
                _ => "none".to_string(),
            }
        }
        fn guid_list(refs: &[u64]) -> String {
            if refs.is_empty() {
                return "none".to_string();
            }
            let head: Vec<String> = refs.iter().take(3).map(|g| format!("0x{g:016x}")).collect();
            if refs.len() > head.len() {
                format!("{} (+{} more)", head.join(", "), refs.len() - head.len())
            } else {
                head.join(", ")
            }
        }
        match self {
            Record::Sound(r) => {
                let ids: Vec<String> = r
                    .payload_with_ids()
                    .iter()
                    .take(4)
                    .map(|(id, _)| format!("{id}"))
                    .collect();
                let files = r
                    .wwise
                    .as_ref()
                    .map(|b| b.wem_files.clone())
                    .unwrap_or_default();
                format!(
                    "sound           class 0x{:08X} parent={} bank={} files=[{}] ids=[{}] streams={} play_event={:?}",
                    r.root_class,
                    guid_or_none(r.parent_sound),
                    guid_or_none(r.wwise.as_ref().and_then(|b| b.sound_bank)),
                    guid_list(&files),
                    ids.join(","),
                    r.wwise.as_ref().map(|b| b.wem_streams.len()).unwrap_or(0),
                    r.wwise.as_ref().and_then(|b| b.event_play),
                )
            }
            Record::SoundSwitch(r) => format!(
                "sound switch    class 0x{:08X} group={} name={:?} value={:?}",
                r.root_class,
                guid_or_none(r.group),
                r.name,
                r.value
            ),
            Record::SoundParameter(r) => format!(
                "sound param     class 0x{:08X} name={:?} value={:?}",
                r.root_class, r.name, r.value
            ),
            Record::SoundBank(r) => format!(
                "sound bank      class 0x{:08X} name={:?} wwise_id={:?} media={} payloads={}",
                r.root_class,
                r.name,
                r.wwise_bank_id,
                r.media.len(),
                r.payload_count()
            ),
            Record::VoiceSet(r) => format!(
                "voice set       class 0x{:08X} gender={} lines={} with_stimulus={} stimuli={} external_sound={} payloads={}",
                r.root_class,
                r.gender
                    .map(|g| g.to_string())
                    .unwrap_or_else(|| "unset".into()),
                r.lines.len(),
                r.lines_with_stimulus(),
                r.stimuli().len(),
                guid_or_none(r.external_sound),
                r.payload_assets().len()
            ),
            Record::VoiceLineSet(r) => {
                let first = r.lines.first();
                format!(
                    "voice line set  class 0x{:08X} lines={} first={} (asset type 0x{:03X}, \
                     a type this install has no assets of)",
                    r.root_class,
                    r.lines.len(),
                    first
                        .map(|g| format!("0x{g:016x}"))
                        .unwrap_or_else(|| "none".into()),
                    first.map(|g| guid::type_of(*g)).unwrap_or(0),
                )
            }
            Record::WemWrapper(r) => format!(
                "wem wrapper     class 0x{:08X} file_size={:?} name={:?} refs={}",
                r.root_class,
                r.file_size,
                r.name,
                r.refs.len()
            ),
            Record::Wem(r) => {
                let secs = r
                    .header
                    .duration_seconds()
                    .map(|s| format!("~{s:.3}s"))
                    .unwrap_or_else(|| "unknown length".into());
                format!(
                    "WEM             {} {} samples, {}, codec={} setup@{} ({} B), audio@{}",
                    r.header.codec,
                    r.header.sample_count,
                    secs,
                    if r.header.convertible_by_reference() {
                        "convertible by the reference"
                    } else {
                        "header triad / old headers, the reference refuses it"
                    },
                    r.header
                        .setup_packet_at
                        .map(|a| a.to_string())
                        .unwrap_or_else(|| "none".into()),
                    r.header
                        .setup_packet_len()
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "none".into()),
                    r.header
                        .first_audio_packet_at
                        .map(|a| a.to_string())
                        .unwrap_or_else(|| "none".into()),
                )
            }
            Record::Unnamed {
                root_class,
                instances,
                ..
            } => format!("UNNAMED         root class 0x{root_class:08X}, {instances} instance(s)"),
        }
    }
}

/// A census of one sound asset type across the install.
///
/// The sound-surface analogue of [`crate::unlocks::Survey`], and of
/// [`GameData::type_histogram`](crate::GameData::type_histogram): counts first,
/// then a bounded sample of records. `records` holds at most `limit` entries
/// while every counter describes **all** the assets scanned, so a limit narrows
/// what is printed without narrowing what is measured.
#[derive(Debug, Default)]
pub struct Survey {
    pub type_id: u16,
    /// Assets of this type in the registry.
    pub assets: usize,
    /// Assets whose bytes opened.
    pub readable: usize,
    /// Assets whose bytes did not open: in the registry but not downloaded.
    pub unreadable: usize,
    /// Assets that opened but parsed as neither an STU nor a RIFF container.
    pub unparsed: usize,
    /// Assets that parsed as an STU.
    pub as_stu: usize,
    /// Assets that parsed as a Wwise RIFF container.
    pub as_wem: usize,
    /// Assets that parsed as a version 1 STU, which has no per-instance field bag,
    /// so a record would read as empty rather than wrong.
    pub version_1: usize,
    /// Total instances across every STU-parsed asset.
    pub instances: usize,
    /// First-instance class to number of assets: which shapes actually shipped.
    pub root_classes: BTreeMap<u32, usize>,
    /// Assets whose first instance is a class no reader here claims.
    pub unrecognised: usize,
    /// A bounded sample of records.
    pub records: Vec<Record>,
    /// The first few read failures, verbatim.
    pub errors: Vec<String>,
    /// A bounded sample of assets that are in the registry but not on disk.
    ///
    /// These are real GUIDs, and handing one to `ow-tank-sound wem` is the way to
    /// see the "registered but not downloaded" path rather than taking it on trust.
    pub missing_sample: Vec<u64>,
    /// Assets this survey actually opened. Equal to `assets` unless capped.
    pub scanned: usize,
    /// Whether `scanned` is less than `assets`.
    pub capped: bool,
}

impl Survey {
    /// Assets that opened and parsed, within what was scanned.
    pub fn parsed(&self) -> usize {
        self.as_stu + self.as_wem
    }

    /// How many sample records are typed rather than [`Record::Unnamed`].
    pub fn typed_in_sample(&self) -> usize {
        self.records
            .iter()
            .filter(|r| !matches!(r, Record::Unnamed { .. }))
            .count()
    }
}

/// Walk one sound asset type across the install and report what is there.
pub fn survey_type(data: &GameData, type_id: u16, limit: usize) -> Survey {
    survey_type_capped(data, type_id, limit, usize::MAX)
}

/// [`survey_type`] over at most `cap` assets, striding over the rest.
///
/// The cap exists because a retail manifest records a great many sound assets and
/// downloads none of them: opening all of them is a CASC miss each and the survey
/// takes minutes. `scanned` and `capped` say what happened, so a capped census is
/// never mistaken for a complete one.
pub fn survey_type_capped(data: &GameData, type_id: u16, limit: usize, cap: usize) -> Survey {
    let mut survey = Survey {
        type_id,
        ..Survey::default()
    };
    let guids = data.guids_of_type(type_id);
    survey.assets = guids.len();
    let stride = (guids.len() / cap.max(1)).max(1);
    let sampled: Vec<u64> = guids.into_iter().step_by(stride).take(cap).collect();
    survey.capped = sampled.len() < survey.assets;

    for asset in sampled {
        survey.scanned += 1;
        let bytes = match data.bytes_of(asset) {
            Ok(bytes) => bytes,
            Err(_) => {
                survey.unreadable += 1;
                if survey.missing_sample.len() < 4 {
                    survey.missing_sample.push(asset);
                }
                continue;
            }
        };
        survey.readable += 1;

        // Which reader to try is decided by a four-byte peek, not by the declared
        // kind: a RIFF container's tag is `RIFF` or `RIFX` and no STU asset begins
        // with either, while version 2 STU has no magic at all (`ow-stu/src/lib.rs:8`).
        // So the peek is a *discriminator*, and which kind a type really is on this
        // install is still what the counters measure — see `docs/formats/sound.md`.
        //
        // `Stu::parse` takes ownership, so the STU attempt works on a copy. That
        // copy is only made for non-RIFF assets, which are the metadata ones and are
        // small; a WEM is never copied, because the peek already excluded it.
        let is_riff = matches!(bytes.get(0..4), Some(b"RIFF" | b"RIFX"));
        if !is_riff {
            if let Ok(mut stu) = Stu::parse(bytes.clone()) {
                survey.as_stu += 1;
                if stu.is_v1() {
                    survey.version_1 += 1;
                }
                survey.instances += stu.instances.len();
                if let Some(root) = stu.instances.first().map(|i| i.hash) {
                    *survey.root_classes.entry(root).or_insert(0) += 1;
                }
                let record = record_for(type_id, &mut stu, asset);
                if matches!(record, Record::Unnamed { .. }) {
                    survey.unrecognised += 1;
                }
                if survey.records.len() < limit {
                    survey.records.push(record);
                }
                continue;
            }
        }

        let size = data.registry().get(asset).map(|r| r.size).unwrap_or(0);
        match parse_wem(&bytes) {
            Ok(header) => {
                survey.as_wem += 1;
                if survey.records.len() < limit {
                    survey.records.push(Record::Wem(Box::new(WemFile {
                        asset,
                        manifest_size: Some(size),
                        header,
                    })));
                }
            }
            Err(e) => {
                survey.unparsed += 1;
                if survey.errors.len() < 8 {
                    // A `BKHD` header is a Wwise soundbank: a Wwise file, but not
                    // a Wwise *media container*. Saying so keeps "wrong container
                    // format" distinct from "wrong file for this type".
                    let hint = match &e {
                        Error::NotRiff { found } if &found[..3] == b"BKH" => {
                            "  [BKHD: a Wwise soundbank, not a media container]"
                        }
                        _ => "",
                    };
                    survey.errors.push(format!("0x{asset:016x}: {e}{hint}"));
                }
            }
        }
    }
    survey
}

/// Every sound type, surveyed over at most `cap` assets each. `limit` is the
/// record sample.
pub fn survey_sound_capped(data: &GameData, limit: usize, cap: usize) -> Vec<Survey> {
    types::ALL
        .iter()
        .map(|t| survey_type_capped(data, *t, limit, cap))
        .collect()
}

/// Every sound type, surveyed in full.
pub fn survey_sound(data: &GameData, limit: usize) -> Vec<Survey> {
    survey_sound_capped(data, limit, usize::MAX)
}

/// The reader for one sound asset type, or [`Record::Unnamed`] when nothing here
/// claims the asset's root class.
///
/// **Dispatch is on the root instance's own class, not on the asset type.** That
/// is a measurement, not a preference: on the reference install, assets of type
/// `0x2E` (`SoundSwitchGroup` in the reference's enumeration) carry root class
/// `0x89006ADB`, which is `STUSoundSwitch` — so keying on the type id would file
/// seven readable switch assets under "unnamed". The reference tooling dispatches
/// on the type id (`Combo.cs:980`); that is a difference from it, and it is
/// deliberate, because the type id is the thing in doubt.
///
/// The types the reference has no class for — `0x30`, `0x31`, `0x55`, `0xA3` —
/// have no arm here on purpose, and fall through to [`unnamed`].
fn record_for(type_id: u16, stu: &mut Stu, asset: u64) -> Record {
    let _ = type_id;
    let Some(root) = stu.instances.first().map(|i| i.hash) else {
        return unnamed(stu, asset);
    };
    match root {
        class::SOUND => wrap(
            Sound::read_in(stu, asset),
            |r| Record::Sound(Box::new(r)),
            stu,
            asset,
        ),
        class::SOUND_SWITCH => wrap(
            SoundSwitch::read_in(stu, asset),
            |r| Record::SoundSwitch(Box::new(r)),
            stu,
            asset,
        ),
        class::SOUND_PARAMETER => wrap(
            SoundParameter::read_in(stu, asset),
            |r| Record::SoundParameter(Box::new(r)),
            stu,
            asset,
        ),
        class::SOUND_BANK => wrap(
            SoundBank::read_in(stu, asset),
            |r| Record::SoundBank(Box::new(r)),
            stu,
            asset,
        ),
        class::VOICE_SET => wrap(
            VoiceSet::read_in(stu, asset),
            |r| Record::VoiceSet(Box::new(r)),
            stu,
            asset,
        ),
        class::VOICE_LINE_SET => wrap(
            VoiceLineSet::read_in(stu, asset),
            |r| Record::VoiceLineSet(Box::new(r)),
            stu,
            asset,
        ),
        class::WEM_FILE => wrap(
            WemWrapper::read_in(stu, asset),
            |r| Record::WemWrapper(Box::new(r)),
            stu,
            asset,
        ),
        _ => unnamed(stu, asset),
    }
}

fn wrap<T>(read: Option<T>, make: impl FnOnce(T) -> Record, stu: &Stu, asset: u64) -> Record {
    match read {
        Some(value) => make(value),
        None => unnamed(stu, asset),
    }
}

fn unnamed(stu: &Stu, asset: u64) -> Record {
    Record::Unnamed {
        asset,
        root_class: stu.instances.first().map(|i| i.hash).unwrap_or(0),
        instances: stu.instances.len(),
    }
}

// ---------------------------------------------------------------------------
// The speech manifest
// ---------------------------------------------------------------------------

/// One record of a manifest this module opened itself.
///
/// The content key is all that is needed to read the bytes, which is why this is
/// a plain struct rather than a borrow of a [`manifest::Registry`]: the CLI builds
/// one, hands it around, and drops the registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpeechAsset {
    pub guid: u64,
    pub size: u32,
    pub content_key: [u8; 16],
}

/// The per-locale *speech* manifest: where voice audio lives.
///
/// [`GameData::open`](crate::GameData::open) deliberately does not load it —
/// nothing else in this crate reads audio, and a second manifest costs memory the
/// renderer does not need. So voice assets are invisible to
/// [`GameData::guids_of_type`](crate::GameData::guids_of_type) until this is
/// opened.
///
/// The manifest is identified by a marker in its file name and a locale tag
/// (`manifest.rs:274`), so an install with no speech manifest for the requested
/// locale yields `Ok(None)` rather than an error: not having enUS voice is a fact
/// about the install, not a failure to read it.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeechIndex {
    /// Manifest file name, for the report.
    pub manifest: String,
    /// Records, in GUID order.
    pub assets: Vec<SpeechAsset>,
}

impl SpeechIndex {
    /// Open the speech manifest for the selector's platform, region and locale.
    pub fn open(
        data: &GameData,
        selector: &ManifestSelector,
        sources: &Sources,
    ) -> Result<Option<SpeechIndex>> {
        let root_file = data.root_file()?;
        let candidates = manifest::Registry::find_manifest_candidates(
            &root_file,
            manifest::ManifestKind::Speech,
            &selector.platform,
            &selector.region,
            Some(&selector.text_locale),
        );
        for (name, content_key) in candidates {
            if let Ok(registry) =
                manifest::Registry::open(data.store(), &name, &content_key, sources)
            {
                let assets = registry
                    .records()
                    .iter()
                    .map(|r| SpeechAsset {
                        guid: r.guid,
                        size: r.size,
                        content_key: r.content_key,
                    })
                    .collect();
                return Ok(Some(SpeechIndex {
                    manifest: name,
                    assets,
                }));
            }
        }
        Ok(None)
    }

    /// Every asset of a type, in GUID order.
    pub fn guids_of_type(&self, type_id: u16) -> Vec<u64> {
        self.assets
            .iter()
            .map(|a| a.guid)
            .filter(|g| guid::type_of(*g) == type_id)
            .collect()
    }

    /// The record for a GUID, if this manifest holds it.
    pub fn get(&self, guid: u64) -> Option<&SpeechAsset> {
        // Records arrive GUID-sorted from the manifest reader, so this is a
        // binary search rather than a scan.
        self.assets
            .binary_search_by_key(&guid, |a| a.guid)
            .ok()
            .map(|i| &self.assets[i])
    }

    /// Read an asset's bytes. Fails when the manifest does not hold the GUID, or
    /// when the payload is not downloaded.
    pub fn bytes_of(&self, data: &GameData, guid: u64) -> Result<Vec<u8>> {
        let record = self.get(guid).ok_or_else(|| {
            crate::Error::NotFound(format!(
                "0x{guid:016x} is not in the speech manifest {}",
                self.manifest
            ))
        })?;
        Ok(data.open_ckey(&record.content_key)?)
    }

    /// Parse a WEM payload held by this manifest.
    pub fn read_wem(
        &self,
        data: &GameData,
        guid: u64,
    ) -> std::result::Result<(WemHeader, Vec<u8>), Error> {
        let bytes = self.bytes_of(data, guid).map_err(|e| {
            Error::Vorbis(format!(
                "speech asset 0x{guid:016x} could not be opened: {e}"
            ))
        })?;
        let header = parse_wem(&bytes)?;
        Ok((header, bytes))
    }

    /// How many assets of each type this manifest holds.
    pub fn type_histogram(&self) -> BTreeMap<u16, usize> {
        let mut out = BTreeMap::new();
        for asset in &self.assets {
            *out.entry(guid::type_of(asset.guid)).or_insert(0) += 1;
        }
        out
    }
}

/// Open the speech manifest for the default selector.
pub fn open_speech(data: &GameData, sources: &Sources) -> Result<Option<SpeechIndex>> {
    SpeechIndex::open(data, &ManifestSelector::default(), sources)
}

// ---------------------------------------------------------------------------
// Low-level helpers the raw STU layer does not have
// ---------------------------------------------------------------------------

/// Walk every asset of `type_id`, parse it, and hand it to `read`.
///
/// Assets that do not open or do not parse are skipped, matching
/// [`crate::unlocks`]'s reader: not every asset of a type is present in the loaded
/// manifests, and a census that stopped at the first gap would be a census of
/// nothing.
fn read_type<T>(
    data: &GameData,
    type_id: u16,
    read: impl Fn(&mut Stu, u64) -> Option<T>,
) -> Vec<T> {
    let mut out = Vec::new();
    for asset in data.guids_of_type(type_id) {
        let Ok(mut stu) = data.stu_of(asset) else {
            continue;
        };
        if let Some(value) = read(&mut stu, asset) {
            out.push(value);
        }
    }
    out
}

/// One element of an inline array, resolved far enough to read its fields.
///
/// An inline array is laid out *inside* its owner's payload: each element is an
/// `i32` field-bag index followed by the element's own fields
/// (`ow-stu/src/types.rs:1084-1092`). Those elements are **not** in
/// [`Stu::instances`], so every accessor here works from the element's own offset
/// rather than from an instance index. That is the whole reason this type exists:
/// [`Stu::read_string`] and [`Stu::read_asset_ref`] take an instance index, which
/// an inline element does not have.
///
/// The field layouts come from [`Stu::layout_at`], which is public and is the same
/// call the typed layer makes, so the offsets here are the same offsets — not a
/// second guess at them.
struct Element {
    /// Absolute offset of the element, its bag index included.
    start: usize,
    /// Field layout, offsets relative to [`start`](Self::start).
    layout: Vec<FieldInfo>,
}

impl Element {
    /// Walk an inline instance array and resolve each element.
    ///
    /// [`Stu::inline_elements`] yields each element's absolute offset and its bag
    /// index; an element whose bag resolves to nothing is skipped rather than read
    /// at a guessed layout.
    fn read_all(stu: &Stu, owner: usize, field_hash: u32) -> Vec<Element> {
        stu.inline_elements(owner, field_hash)
            .into_iter()
            .filter_map(|(_, at)| {
                let bag = stu.i32_at(at)?;
                if bag < 0 {
                    return None;
                }
                Some(Element {
                    start: at,
                    layout: stu.layout_at(at, bag),
                })
            })
            .collect()
    }

    /// Absolute offset of one field, from its hash.
    fn offset_of(&self, hash: u32) -> Option<usize> {
        self.layout
            .iter()
            .find(|f| f.hash == hash)
            .map(|f| self.start + f.offset)
    }

    /// The instance index an embedded-instance field points at.
    ///
    /// A version 2 embedded field stores the target's index in the instance table
    /// (`ow-stu/src/types.rs:1058-1066`), so this is the one field an element can
    /// hand straight back to the instance-keyed readers.
    fn embedded(&self, stu: &Stu, hash: u32) -> Option<usize> {
        let index = stu.i32_at(self.offset_of(hash)?)?;
        if index < 0 || index as usize >= stu.instances.len() {
            return None;
        }
        Some(index as usize)
    }

    /// A `teString` field, decoded. The same three-step walk as
    /// [`Stu::read_string`], reached from an offset instead of an instance.
    fn string(&self, stu: &Stu, hash: u32) -> Option<String> {
        let at = self.offset_of(hash)?;
        let dyn_off = stu.i32_at(at)?;
        if dyn_off < 0 {
            return None;
        }
        let block = stu.dyn_data();
        let base = dyn_off as usize;
        let len = le_i32(block, base)?;
        if len < 0 {
            return None;
        }
        if len == 0 {
            return Some(String::new());
        }
        let text_off = le_i64(block, base + 8)?;
        if text_off < 0 {
            return None;
        }
        let start = text_off as usize;
        let end = start.checked_add(len as usize)?;
        let bytes = block.get(start..end)?;
        Some(String::from_utf8_lossy(bytes).into_owned())
    }

    /// An asset-reference field, deobfuscated.
    fn asset_ref(&self, stu: &Stu, hash: u32) -> Option<u64> {
        let stored = stu.u64_at(self.offset_of(hash)?)?;
        Some(ow_stu::deobfuscate(stu.header_checksum, hash, stored))
    }

    fn u32(&self, stu: &Stu, hash: u32) -> Option<u32> {
        stu.u32_at(self.offset_of(hash)?)
    }

    fn f32(&self, stu: &Stu, hash: u32) -> Option<f32> {
        self.u32(stu, hash).map(f32::from_bits)
    }

    fn u64(&self, stu: &Stu, hash: u32) -> Option<u64> {
        stu.u64_at(self.offset_of(hash)?)
    }
}

/// The instance embedded at a field of `embedder`, if there is one.
///
/// A missing embedded instance is normal — a sound need not declare a Wwise
/// block — so absence is `None` rather than an error. The field hash matters: a
/// sound embeds exactly one `STU_0FA0034C`, but a voice line instance embeds a
/// line *and* a four-wrapper holder, and looking them up by class alone would be
/// ambiguous.
fn embedded_by_field(stu: &Stu, embedder: usize, field_hash: u32) -> Option<usize> {
    stu.instances
        .iter()
        .position(|i| i.embedder_index == embedder as i32 && i.embedder_field == field_hash)
}

/// The instance of a class embedded at `embedder`, when the class alone
/// identifies it.
fn embedded(stu: &Stu, embedder: usize, class: u32) -> Option<usize> {
    stu.instances
        .iter()
        .position(|i| i.embedder_index == embedder as i32 && i.hash == class)
}

/// A `teStructuredDataAssetRef<T>[]` field, read out of the dynamic block.
///
/// Element layout is the typed layer's (`ow_stu/src/types.rs:1030`): sixteen bytes
/// per element with the GUID at `+8`, deobfuscated with the *array's* field hash.
/// Both facts are copied rather than re-derived; a wrong stride yields GUIDs that
/// resolve to nothing, which is how the wrong layout is caught.
fn read_ref_array(stu: &mut Stu, instance: usize, field_hash: u32) -> Vec<u64> {
    let Some((offset, _size)) = stu.field_bytes(instance, field_hash) else {
        return Vec::new();
    };
    read_ref_array_at_block(
        stu,
        stu.payload_start(instance) + offset,
        field_hash,
        /* max_count */ 4096,
    )
}

/// The same array, reached from an inline element's field offset.
fn read_ref_array_at(stu: &Stu, element: &Element, field_hash: u32) -> Vec<u64> {
    let Some(at) = element.offset_of(field_hash) else {
        return Vec::new();
    };
    read_ref_array_at_block(stu, at, field_hash, 4096)
}

fn read_ref_array_at_block(stu: &Stu, at: usize, field_hash: u32, max_count: i32) -> Vec<u64> {
    let Some(field_offset) = stu.i32_at(at) else {
        return Vec::new();
    };
    if field_offset < 0 {
        return Vec::new();
    }
    let dyn_block = stu.dyn_data();
    let base = field_offset as usize;
    let count = le_i32(dyn_block, base).unwrap_or(0);
    if count <= 0 || count > max_count {
        return Vec::new();
    }
    let data_offset = le_i64(dyn_block, base + 8).unwrap_or(-1);
    if data_offset < 0 {
        return Vec::new();
    }
    let start = data_offset as usize;
    match start.checked_add(count as usize * 16) {
        Some(end) if end <= dyn_block.len() => {}
        // A count that runs past the block is a descriptor this reader does not
        // understand, and reading it would be reading noise.
        _ => return Vec::new(),
    }
    let checksum = stu.header_checksum;
    let mut out = Vec::with_capacity(count as usize);
    for k in 0..count as usize {
        let Some(stored) = le_u64(dyn_block, start + k * 16 + 8) else {
            break;
        };
        out.push(ow_stu::deobfuscate(checksum, field_hash, stored));
    }
    out
}

/// A bare `uint[]` in the dynamic block, four bytes per element.
///
/// **No deobfuscation**: a bare `uint[]` is not a `teStructuredDataAssetRef[]`, so
/// there is no per-element field hash and the typed layer only deobfuscates what
/// it read through `FieldType::AssetRef` (`ow_stu/src/types.rs:1030`). These are
/// the stored bytes.
fn read_u32_array(stu: &mut Stu, instance: usize, field_hash: u32) -> Vec<u32> {
    let Some((offset, _size)) = stu.field_bytes(instance, field_hash) else {
        return Vec::new();
    };
    read_u32_array_at_block(stu, stu.payload_start(instance) + offset)
}

/// The same array, reached from an inline element's field offset.
fn read_u32_array_at(stu: &Stu, element: &Element, field_hash: u32) -> Vec<u32> {
    let Some(at) = element.offset_of(field_hash) else {
        return Vec::new();
    };
    read_u32_array_at_block(stu, at)
}

fn read_u32_array_at_block(stu: &Stu, at: usize) -> Vec<u32> {
    let Some(field_offset) = stu.i32_at(at) else {
        return Vec::new();
    };
    if field_offset < 0 {
        return Vec::new();
    }
    let dyn_block = stu.dyn_data();
    let base = field_offset as usize;
    let count = le_i32(dyn_block, base).unwrap_or(0);
    // The bound is looser than for a reference array because a Wwise media-id list
    // can legitimately be long; the block-length check below is what actually
    // bounds the read.
    if count <= 0 || count > 65_536 {
        return Vec::new();
    }
    let data_offset = le_i64(dyn_block, base + 8).unwrap_or(-1);
    if data_offset < 0 {
        return Vec::new();
    }
    let start = data_offset as usize;
    let count = count as usize;
    match start.checked_add(count * 4) {
        Some(end) if end <= dyn_block.len() => {}
        _ => return Vec::new(),
    }
    (0..count)
        .map(|k| le_u32(dyn_block, start + k * 4).unwrap_or(0))
        .collect()
}

fn le_i32(data: &[u8], at: usize) -> Option<i32> {
    data.get(at..at + 4)
        .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn le_u32(data: &[u8], at: usize) -> Option<u32> {
    data.get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn le_i64(data: &[u8], at: usize) -> Option<i64> {
    data.get(at..at + 8)
        .map(|b| i64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
}

fn le_u64(data: &[u8], at: usize) -> Option<u64> {
    data.get(at..at + 8)
        .map(|b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Assemble a RIFF container from `(tag, payload)` pairs.
    ///
    /// The container's own size field is computed, so a test that changes a
    /// chunk's payload cannot leave the fixture internally inconsistent.
    fn riff(parts: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let body: usize = parts.iter().map(|(_, p)| 8 + p.len()).sum();
        let total = 12 + body;
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&((total - 8) as u32).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        for (tag, payload) in parts {
            out.extend_from_slice(*tag);
            out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            out.extend_from_slice(payload);
        }
        out
    }

    /// A `fmt ` payload of `size` bytes, with the vorbis codec word and, for a
    /// `0x28`-byte chunk, the signature the reference checks.
    fn fmt_payload(size: u32) -> Vec<u8> {
        let mut p = Vec::new();
        p.extend_from_slice(&0xFFFFu16.to_le_bytes()); // codec
        p.extend_from_slice(&2u16.to_le_bytes()); // channels
        p.extend_from_slice(&48_000u32.to_le_bytes()); // sample rate
        p.extend_from_slice(&12_000u32.to_le_bytes()); // avg bytes/sec
        p.extend_from_slice(&0u16.to_le_bytes()); // block align
        p.extend_from_slice(&0u16.to_le_bytes()); // bits per sample
        p.extend_from_slice(&(size.wrapping_sub(0x12) as u16).to_le_bytes()); // extra
        p.extend_from_slice(&0x1234u16.to_le_bytes()); // ext unk
        p.extend_from_slice(&3u32.to_le_bytes()); // subtype
        if size == 0x28 {
            p.extend_from_slice(&FMT_EXT_SIGNATURE);
        }
        p.resize(size as usize, 0);
        p
    }

    /// A `vorb` payload of `size` bytes, with the two packet offsets written
    /// where the shape table says they are read from.
    ///
    /// A size of `0` gets an empty payload on purpose: the reference reads a
    /// header at `vorb + 0` even when the chunk declares no bytes, so there is
    /// nothing here to write a sample count into. Tests over that shape check
    /// only what does not depend on the values.
    fn vorb_payload(size: u32, samples: u32, setup_rel: u32, first_rel: u32) -> Vec<u8> {
        let mut p = vec![0u8; size as usize];
        let (_, _, seek) = VORB_SHAPES
            .iter()
            .find(|s| s.0 == size)
            .expect("the shape table must cover the test size");
        if size >= 4 {
            p[0..4].copy_from_slice(&samples.to_le_bytes());
        }
        if *seek + 8 <= p.len() {
            p[*seek..*seek + 4].copy_from_slice(&setup_rel.to_le_bytes());
            p[*seek + 4..*seek + 8].copy_from_slice(&first_rel.to_le_bytes());
        }
        p
    }

    /// A `data` payload: a setup packet then an audio packet, each behind a
    /// six-byte header.
    fn data_payload(setup_size: u16, audio_size: u16, samples: u32) -> Vec<u8> {
        let mut p = Vec::new();
        p.extend_from_slice(&setup_size.to_le_bytes());
        p.extend_from_slice(&0u32.to_le_bytes()); // setup granule
        p.resize(p.len() + setup_size as usize, 0xAA);
        p.extend_from_slice(&audio_size.to_le_bytes());
        p.extend_from_slice(&samples.to_le_bytes()); // audio granule
        p.resize(p.len() + audio_size as usize, 0xBB);
        p
    }

    #[test]
    fn vorbis_header_reads_the_shape_the_reference_describes() {
        let samples = 24_000u32;
        let parts = [
            (b"fmt ", fmt_payload(0x28)),
            (b"vorb", vorb_payload(0x32, samples, 0, 12)),
            (b"data", data_payload(12, 20, samples)),
        ];
        let header = parse_wem(&riff(&parts)).expect("the synthetic WEM must parse");

        assert_eq!(header.codec, Codec::Vorbis);
        assert!(!header.big_endian, "RIFF is little-endian");
        assert_eq!(header.channels, 2);
        assert_eq!(header.sample_rate, 48_000);
        assert_eq!(header.sample_count, samples);
        assert_eq!(
            header.subtype, 3,
            "two channels, per the reference's comment"
        );
        assert_eq!(
            header.ext_signature_ok,
            Some(true),
            "a 0x28-byte fmt chunk carries the signature the reference checks"
        );
        assert_eq!(
            header.packet_header_size, 6,
            "a granule-carrying vorb uses six-byte packet headers"
        );
        assert!(!header.no_granule);
        assert!(!header.vorb_synthesised);
        assert!(!header.header_triad_present);
        assert!(header.convertible_by_reference());
        assert_eq!(header.cue_count, None, "the fixture has no cue chunk");
        assert_eq!(header.loop_count, 0);
        assert_eq!(header.chunks.len(), 3);
    }

    #[test]
    fn duration_is_the_quotient_of_two_read_values() {
        let parts = [
            (b"fmt ", fmt_payload(0x28)),
            (b"vorb", vorb_payload(0x32, 24_000, 0, 12)),
            (b"data", data_payload(12, 20, 24_000)),
        ];
        let header = parse_wem(&riff(&parts)).unwrap();
        // 24,000 samples at 48,000 Hz is half a second.
        assert_eq!(header.duration_seconds(), Some(0.5));

        let mut no_rate = header.clone();
        no_rate.sample_rate = 0;
        assert_eq!(
            no_rate.duration_seconds(),
            None,
            "a zero rate is not a length"
        );

        let mut no_samples = header;
        no_samples.sample_count = 0;
        assert_eq!(
            no_samples.duration_seconds(),
            None,
            "zero samples is not a length"
        );
    }

    #[test]
    fn the_setup_packet_is_located_and_sized_from_the_data_chunk() {
        let parts = [
            (b"fmt ", fmt_payload(0x28)),
            (b"vorb", vorb_payload(0x32, 24_000, 0, 12)),
            (b"data", data_payload(12, 20, 24_000)),
        ];
        let bytes = riff(&parts);
        let header = parse_wem(&bytes).unwrap();

        assert_eq!(header.setup_packet_offset, Some(0));
        assert_eq!(header.setup_packet_at, Some(header.data_offset));
        assert_eq!(header.setup_packet_len(), Some(12));
        assert_eq!(header.first_audio_packet_offset, Some(12));
        assert_eq!(header.first_audio_packet_at, Some(header.data_offset + 12));
        // And the declared size really is the first word of the packet header.
        let at = header.data_offset;
        assert_eq!(u16::from_le_bytes([bytes[at], bytes[at + 1]]), 12);
    }

    #[test]
    fn a_smpl_chunk_becomes_a_loop_that_fits_the_sample_count() {
        let samples = 24_000u32;
        // Loop start 100, stored end 999. The reference treats a non-zero end as a
        // last-sample index and turns it into a count.
        let mut smpl = vec![0u8; 0x34];
        smpl[0x1C..0x20].copy_from_slice(&1u32.to_le_bytes());
        smpl[0x2C..0x30].copy_from_slice(&100u32.to_le_bytes());
        smpl[0x30..0x34].copy_from_slice(&999u32.to_le_bytes());

        let parts = [
            (b"fmt ", fmt_payload(0x28)),
            (b"vorb", vorb_payload(0x32, samples, 0, 12)),
            (b"smpl", smpl),
            (b"data", data_payload(12, 20, samples)),
        ];
        let header = parse_wem(&riff(&parts)).unwrap();
        assert_eq!(header.loop_count, 1);
        assert_eq!(header.loop_start, 100);
        assert_eq!(header.loop_end, 1000, "a stored end gains one");
    }

    #[test]
    fn a_loop_beyond_the_sample_count_is_dropped_not_clamped() {
        let samples = 1_000u32;
        let mut smpl = vec![0u8; 0x34];
        smpl[0x1C..0x20].copy_from_slice(&1u32.to_le_bytes());
        smpl[0x2C..0x30].copy_from_slice(&100u32.to_le_bytes());
        smpl[0x30..0x34].copy_from_slice(&9_999u32.to_le_bytes());

        let parts = [
            (b"fmt ", fmt_payload(0x28)),
            (b"vorb", vorb_payload(0x32, samples, 0, 12)),
            (b"smpl", smpl),
            (b"data", data_payload(12, 20, samples)),
        ];
        let header = parse_wem(&riff(&parts)).unwrap();
        assert_eq!(
            header.loop_count, 0,
            "a loop past the end is dropped, as the reference does"
        );
        assert_eq!(header.loop_start, 0);
        assert_eq!(header.loop_end, 0);
    }

    #[test]
    fn a_granule_less_vorb_uses_two_byte_packet_headers() {
        // Size 0 is in the table with no_granule = true.
        assert!(VORB_SHAPES[0].1);
        let parts = [
            (b"fmt ", fmt_payload(0x18)),
            (b"vorb", vorb_payload(0x00, 4_800, 0, 8)),
            (b"data", data_payload(8, 8, 4_800)),
        ];
        let header = parse_wem(&riff(&parts)).unwrap();
        assert!(header.no_granule);
        assert_eq!(header.packet_header_size, 2);
        // The modification word at +0x04 is zero here, which is not one of the four
        // "unmodified" values, so packets are modified.
        assert!(header.mod_packets);
    }

    #[test]
    fn the_four_unmodified_signals_mean_packets_are_not_modified() {
        for signal in [0x4Au32, 0x4B, 0x69, 0x70] {
            // Shape `0x2a` rather than `0x00`: both are granule-less, but a
            // size-0 chunk has no bytes to write the signal into.
            let mut vorb = vorb_payload(0x2A, 24_000, 0, 12);
            vorb[4..8].copy_from_slice(&signal.to_le_bytes());
            let parts = [
                (b"fmt ", fmt_payload(0x18)),
                (b"vorb", vorb),
                (b"data", data_payload(12, 20, 24_000)),
            ];
            let header = parse_wem(&riff(&parts)).unwrap();
            assert!(
                !header.mod_packets,
                "signal {signal:#x} is one of the four the reference reads as unmodified"
            );
        }
    }

    #[test]
    fn a_granule_less_vorb_with_an_unknown_signal_is_modified() {
        // The reference's own comment lists what it has seen and says "seems to be
        // 0xD9 when _mod_packets should be set" (`WwiseRIFFVorbis.cs:259-271`).
        let mut vorb = vorb_payload(0x2A, 24_000, 0, 12);
        vorb[4..8].copy_from_slice(&0xD9u32.to_le_bytes());
        let parts = [
            (b"fmt ", fmt_payload(0x18)),
            (b"vorb", vorb),
            (b"data", data_payload(12, 20, 24_000)),
        ];
        assert!(parse_wem(&riff(&parts)).unwrap().mod_packets);
    }

    #[test]
    fn a_header_triad_is_reported_as_unconvertible() {
        // The 0x28-byte vorb is the triad shape: the reference refuses it.
        let parts = [
            (b"fmt ", fmt_payload(0x28)),
            (b"vorb", vorb_payload(0x28, 24_000, 0, 12)),
            (b"data", data_payload(12, 20, 24_000)),
        ];
        let header = parse_wem(&riff(&parts)).unwrap();
        assert!(header.header_triad_present);
        assert!(header.old_packet_headers);
        assert!(
            !header.convertible_by_reference(),
            "the reference throws on a header triad"
        );
    }

    #[test]
    fn a_missing_vorb_chunk_is_reported_as_synthesised() {
        // The reference fabricates a vorb at fmt + 0x18 when the fmt chunk is 0x42
        // bytes and the file carried none. Saying so matters, because that is a
        // claim about the file which is not true.
        let parts = [
            (b"fmt ", fmt_payload(0x42)),
            (b"data", data_payload(12, 20, 24_000)),
        ];
        let header = parse_wem(&riff(&parts)).unwrap();
        assert!(header.vorb_synthesised);
        assert_eq!(
            header.sample_count, 0,
            "a fabricated vorb reads as zero samples"
        );
    }

    /// An Opus `fmt ` payload: codec `0x3041`, the given channels, a non-zero
    /// block size, the given sample and frame counts, and a valid codec version.
    ///
    /// The offsets follow `WwiseRIFFOpus.cs:105-140`, including the two bytes the
    /// reference skips between the extra-size word at `+0x10` and the channel
    /// layout at `+0x14`.
    fn opus_fmt(channels: u16, samples: u32, frames: u32) -> Vec<u8> {
        let mut p = Vec::new();
        p.extend_from_slice(&0x3041u16.to_le_bytes()); // codec: opus
        p.extend_from_slice(&channels.to_le_bytes());
        p.extend_from_slice(&48_000u32.to_le_bytes()); // sample rate
        p.extend_from_slice(&64_000u32.to_le_bytes()); // average bitrate
        p.extend_from_slice(&6u16.to_le_bytes()); // block size, non-zero
        p.extend_from_slice(&0u16.to_le_bytes()); // bits per sample
        p.extend_from_slice(&0xAu16.to_le_bytes()); // extra size at +0x10
        p.extend_from_slice(&0u16.to_le_bytes()); // padding to +0x14
                                                  // The reference treats a layout whose low byte equals the channel count as
                                                  // "recognised" and pulls the channel type out of the next nibble
                                                  // (`WwiseRIFFOpus.cs:116-120`), so the fixture follows that convention.
        p.extend_from_slice(&u32::from(channels).to_le_bytes());
        p.extend_from_slice(&samples.to_le_bytes()); // num samples at +0x18
        p.extend_from_slice(&frames.to_le_bytes()); // frame count at +0x1C
        p.extend_from_slice(&0u16.to_le_bytes()); // skip at +0x20
        p.push(1); // codec version at +0x22
        p.push(0); // mapping family at +0x23
        p.resize(0x40, 0);
        p
    }

    #[test]
    fn an_opus_container_is_read_by_its_own_header() {
        // Opus keeps its sample count in the `fmt ` chunk, so `sample_count` — the
        // Vorbis `vorb` field — stays zero for one and printing it would print a
        // number that does not apply. `duration_seconds` uses the right one.
        let parts = [
            (b"fmt ", opus_fmt(2, 96_000, 3)),
            (b"seek", vec![0u8; 6]), // three u16 frames
            (b"data", vec![0xCC; 32]),
        ];
        let header = parse_wem(&riff(&parts)).unwrap();
        assert_eq!(header.codec, Codec::Opus);
        assert_eq!(header.num_samples, Some(96_000));
        assert_eq!(
            header.sample_count, 0,
            "no vorb chunk means no vorb sample count"
        );
        assert_eq!(header.seek_table_count, Some(3));
        assert_eq!(
            header.seek_chunk_size,
            Some(6),
            "three u16 frames is a six-byte seek chunk, and both numbers are reported"
        );
        assert_eq!(header.codec_version, Some(1));
        assert_eq!(header.mapping_family, Some(0));
        assert_eq!(header.packet_header_size, 0, "opus packets are raw");
        assert!(
            header.setup_packet_at.is_none(),
            "opus has no setup packet: the fmt chunk is the whole header"
        );
        assert_eq!(
            header.first_audio_packet_at,
            Some(header.data_offset),
            "opus audio starts at the data chunk"
        );
        assert_eq!(
            header.duration_seconds(),
            Some(2.0),
            "96,000 samples at 48,000 Hz is two seconds"
        );
    }

    #[test]
    fn an_opus_container_without_a_seek_table_is_rejected() {
        // The reference cannot size its frame table without one
        // (`WwiseRIFFOpus.cs:101`), so a media-only Opus file is not something
        // this reader should report as read.
        let parts = [(b"fmt ", opus_fmt(2, 96_000, 0)), (b"data", vec![0xCC; 32])];
        assert!(matches!(
            parse_wem(&riff(&parts)),
            Err(Error::MissingChunk("seek"))
        ));
    }

    #[test]
    fn an_opus_codec_version_other_than_one_is_rejected() {
        let mut fmt = opus_fmt(2, 96_000, 1);
        fmt[0x22] = 2;
        let parts = [
            (b"fmt ", fmt),
            (b"seek", vec![0u8; 2]),
            (b"data", vec![0xCC; 16]),
        ];
        assert!(matches!(
            parse_wem(&riff(&parts)),
            Err(Error::BadField {
                what: "the opus codec version byte",
                ..
            })
        ));
    }

    #[test]
    fn a_non_zero_block_align_is_fine_for_opus_and_not_for_vorbis() {
        // The two codecs disagree here, and getting it wrong either rejects legal
        // files or accepts a layout the Vorbis reference throws on. Both fixtures
        // carry a non-zero block size, which only Opus allows.
        let opus_parts = [
            (b"fmt ", opus_fmt(2, 96_000, 1)),
            (b"seek", vec![0u8; 2]),
            (b"data", vec![0xCC; 16]),
        ];
        assert_eq!(
            parse_wem(&riff(&opus_parts))
                .expect("opus allows a non-zero block size")
                .codec,
            Codec::Opus
        );

        let mut vorbis = fmt_payload(0x28);
        vorbis[12..14].copy_from_slice(&7u16.to_le_bytes());
        let vorbis_parts = [
            (b"fmt ", vorbis),
            (b"vorb", vorb_payload(0x32, 24_000, 0, 12)),
            (b"data", data_payload(12, 20, 24_000)),
        ];
        assert!(matches!(
            parse_wem(&riff(&vorbis_parts)),
            Err(Error::BadField {
                what: "the fmt block-align word",
                ..
            })
        ));
    }

    #[test]
    fn more_opus_channels_than_the_reference_can_remap_is_rejected() {
        // Its mapping matrix tops out at eight channels (`WwiseRIFFOpus.cs:40-49`),
        // and the check only applies under mapping family 1 (`WwiseRIFFOpus.cs:141`).
        let mut fmt = opus_fmt(12, 96_000, 1);
        fmt[0x23] = 1;
        let parts = [
            (b"fmt ", fmt),
            (b"seek", vec![0u8; 2]),
            (b"data", vec![0xCC; 16]),
        ];
        assert!(matches!(
            parse_wem(&riff(&parts)),
            Err(Error::Vorbis(m)) if m.contains("mapping family")
        ));
    }

    #[test]
    fn what_is_not_a_container_says_so_rather_than_reading_noise() {
        assert_eq!(
            parse_wem(b"OggS\x00\x02\x00\x00\x00\x00\x00\x00"),
            Err(Error::NotRiff { found: *b"OggS" })
        );
        let mut wrong_form = Vec::new();
        wrong_form.extend_from_slice(b"RIFF");
        wrong_form.extend_from_slice(&4u32.to_le_bytes());
        wrong_form.extend_from_slice(b"AVI ");
        assert_eq!(
            parse_wem(&wrong_form),
            Err(Error::NotWave { found: *b"AVI " })
        );
    }

    #[test]
    fn a_truncated_container_reports_the_shortfall() {
        let parts = [
            (b"fmt ", fmt_payload(0x28)),
            (b"vorb", vorb_payload(0x32, 24_000, 0, 12)),
            (b"data", data_payload(12, 20, 24_000)),
        ];
        let bytes = riff(&parts);
        match parse_wem(&bytes[..20]) {
            Err(Error::Truncated { at, need, have, .. }) => {
                assert_eq!(at, 0, "the shortfall is the container itself");
                assert_eq!(need, bytes.len() as usize);
                assert_eq!(have, 20);
            }
            other => panic!("expected a truncation error, got {other:?}"),
        }
    }

    #[test]
    fn a_chunk_that_runs_past_the_container_is_rejected() {
        // The container's own size field has to agree with the buffer first, or the
        // shortfall is the container rather than the chunk — that is the previous
        // test. Here the container is consistent and one chunk inside it is not.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(20u32).to_le_bytes()); // 20 + 8 = 28 bytes
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(b"fmt ");
        bytes.extend_from_slice(&0x7FFF_FFFFu32.to_le_bytes()); // absurd size
        bytes.resize(28, 0);
        match parse_wem(&bytes) {
            Err(Error::ChunkOutOfRange {
                size, offset, len, ..
            }) => {
                assert_eq!(size, 0x7FFF_FFFF);
                assert_eq!(offset, 12, "the fmt chunk starts at byte 12");
                assert_eq!(len, 28, "the limit reported is the container's own end");
            }
            other => panic!("expected an out-of-range chunk, got {other:?}"),
        }
    }

    #[test]
    fn a_container_that_claims_more_than_it_holds_is_a_truncation() {
        // The reference checks the RIFF size against the file size before it walks
        // anything (`WwiseRIFFVorbis.cs:85`), so a container claiming 1008 bytes in
        // a 20-byte buffer is reported as short rather than as a bad chunk.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&1000u32.to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        bytes.resize(20, 0);
        match parse_wem(&bytes) {
            Err(Error::Truncated { need, have, .. }) => {
                assert_eq!(need, 1008);
                assert_eq!(have, 20);
            }
            other => panic!("expected a truncation error, got {other:?}"),
        }
    }

    #[test]
    fn a_fmt_field_the_reference_rejects_is_an_error_not_a_value() {
        // Block align must be zero for the vorbis fmt layout.
        let mut fmt = fmt_payload(0x28);
        fmt[12..14].copy_from_slice(&2u16.to_le_bytes());
        let parts = [
            (b"fmt ", fmt),
            (b"vorb", vorb_payload(0x32, 24_000, 0, 12)),
            (b"data", data_payload(12, 20, 24_000)),
        ];
        assert!(matches!(
            parse_wem(&riff(&parts)),
            Err(Error::BadField { .. })
        ));
    }

    #[test]
    fn a_vorb_size_outside_the_shape_table_is_rejected() {
        let parts = [
            (b"fmt ", fmt_payload(0x28)),
            (b"vorb", vec![0u8; 0x24]), // 0x24 is not in the table
            (b"data", data_payload(12, 20, 24_000)),
        ];
        match parse_wem(&riff(&parts)) {
            Err(Error::Vorbis(m)) => assert!(m.contains("0x24"), "names the size: {m}"),
            other => panic!("expected a vorb-size rejection, got {other:?}"),
        }
    }

    #[test]
    fn every_vorb_shape_in_the_table_is_accepted() {
        // The table is a transcription. If a size is listed but unreadable, the
        // table is wrong, and this is what says so.
        for &(size, no_granule, _) in VORB_SHAPES {
            let fmt_size = if no_granule { 0x18 } else { 0x28 };
            let parts = [
                (b"fmt ", fmt_payload(fmt_size)),
                (b"vorb", vorb_payload(size, 24_000, 0, 8)),
                (b"data", data_payload(8, 20, 24_000)),
            ];
            let header = parse_wem(&riff(&parts))
                .unwrap_or_else(|e| panic!("vorb size {size:#x} must be readable: {e}"));
            assert_eq!(header.no_granule, no_granule, "vorb size {size:#x}");
            assert_eq!(
                header.packet_header_size,
                if no_granule { 2 } else { 6 },
                "vorb size {size:#x}"
            );
            // A size-0 chunk has no sample count to read, so it is the one shape
            // the count cannot be checked on. Everything else must report the
            // value the fixture wrote.
            if size >= 4 {
                assert_eq!(header.sample_count, 24_000, "vorb size {size:#x}");
            }
            // The uid and blocksizes are read from a size-specific offset, which is
            // the part of the table most likely to be transcribed wrongly.
            let expect_uid = match size {
                0x00 | 0x2A => Some(0x24),
                0x32 | 0x34 => Some(0x2C),
                // The reference leaves them unset for the two header-triad shapes.
                _ => None,
            };
            match expect_uid {
                Some(at) => {
                    // The fixture is zero-filled, so reading the uid must land on
                    // the same place the table says it does: inside the chunk.
                    assert!(
                        header.uid.is_some(),
                        "vorb size {size:#x}: expected a uid at +{at:#x}"
                    );
                }
                None => assert!(
                    header.uid.is_none(),
                    "vorb size {size:#x}: the reference leaves the uid unset"
                ),
            }
        }
    }

    #[test]
    fn type_names_round_trip_and_reject_nonsense() {
        for &t in types::ALL {
            let label = types::label(t);
            assert_eq!(
                types::by_name(&squash(label)),
                Some(t),
                "the label {label:?} must select its own type"
            );
            assert_eq!(types::by_name(&format!("{t:#05x}")), Some(t));
        }
        // Case, spaces, hyphens and underscores are all ignored, because the CLI's
        // own help text lists the types with hyphens in them.
        assert_eq!(types::by_name("voice set"), Some(types::VOICE_SET));
        assert_eq!(types::by_name("VOICE SET"), Some(types::VOICE_SET));
        assert_eq!(types::by_name("VoiceSet"), Some(types::VOICE_SET));
        assert_eq!(types::by_name("voice_set"), Some(types::VOICE_SET));
        assert_eq!(
            types::by_name("voice-wem-file"),
            Some(types::VOICE_WEM_FILE)
        );
        assert_eq!(types::by_name("0x0b2"), Some(types::VOICE_WEM_FILE));
        assert_eq!(types::by_name("not a type"), None);
    }

    #[test]
    fn the_fmt_signature_is_compared_at_the_offset_the_reference_reaches() {
        // Seventeen bytes are declared and sixteen are read, so the reference's own
        // check can never pass. This is the one deliberate difference in the
        // module, and it is pinned here so it cannot be "fixed" back into the bug.
        assert_eq!(
            FMT_EXT_SIGNATURE.len(),
            17,
            "the reference's array is seventeen bytes (WwiseRIFFVorbis.cs:179)"
        );
        assert_eq!(
            FMT_EXT_SIGNATURE_CHECKED, 16,
            "the reference reads sixteen bytes (WwiseRIFFVorbis.cs:180)"
        );
        assert_eq!(
            FMT_EXT_SIGNATURE[FMT_EXT_SIGNATURE.len() - 1],
            0x71,
            "the byte the reference never looks at is the last one"
        );

        // A `0x28`-byte fmt chunk must carry the sixteen bytes at `+0x18`.
        let parts = [
            (b"fmt ", fmt_payload(0x28)),
            (b"vorb", vorb_payload(0x32, 24_000, 0, 12)),
            (b"data", data_payload(12, 20, 24_000)),
        ];
        let good = parse_wem(&riff(&parts)).unwrap();
        assert_eq!(good.ext_signature_ok, Some(true));

        // Corrupt one byte of the sixteen and it is reported, not swallowed.
        let mut fmt = fmt_payload(0x28);
        fmt[FMT_EXT_SIGNATURE_OFFSET + 3] ^= 0xFF;
        let parts = [
            (b"fmt ", fmt),
            (b"vorb", vorb_payload(0x32, 24_000, 0, 12)),
            (b"data", data_payload(12, 20, 24_000)),
        ];
        let bad = parse_wem(&riff(&parts)).unwrap();
        assert_eq!(bad.ext_signature_ok, Some(false));

        // A fmt chunk that is not `0x28` bytes has no signature field at all, which
        // is `None` rather than a false "it did not match".
        let parts = [
            (b"fmt ", fmt_payload(0x18)),
            (b"vorb", vorb_payload(0x2A, 24_000, 0, 12)),
            (b"data", data_payload(12, 20, 24_000)),
        ];
        assert_eq!(parse_wem(&riff(&parts)).unwrap().ext_signature_ok, None);
    }

    #[test]
    fn the_payload_types_are_the_ones_the_reference_opens_as_audio() {
        for t in [
            types::VOICE_WEM_FILE,
            types::SOUND_FILE,
            types::SOUND_FILE_ALT,
        ] {
            assert!(types::is_wem_payload(t), "{t:#05x} is a payload type");
        }
        for t in [types::SOUND, types::VOICE_SET, types::SOUND_PARAMETER] {
            assert!(
                !types::is_wem_payload(t),
                "{t:#05x} is structured data, not audio"
            );
        }
    }

    #[test]
    fn gender_is_the_one_named_enum_and_carries_unknown_values() {
        assert_eq!(Gender::from_raw(1), Gender::Generic);
        assert_eq!(Gender::from_raw(2), Gender::Male);
        assert_eq!(Gender::from_raw(3), Gender::Female);
        assert_eq!(Gender::from_raw(9), Gender::Unnamed(9));
        assert_eq!(Gender::from_raw(0), Gender::Unnamed(0));
        assert_eq!(Gender::Male.to_string(), "male");
    }

    #[test]
    fn numbers_read_the_widths_the_class_declares() {
        let mut nums = Numbers::default();
        nums.words.insert(0xAAAA_0001, 1.5f32.to_bits());
        nums.words.insert(0xAAAA_0002, 7);
        nums.bytes.insert(0xAAAA_0003, 3);
        nums.strings.insert(0xAAAA_0004, "hello".to_string());

        assert_eq!(nums.f32(0xAAAA_0001), Some(1.5));
        assert_eq!(nums.u32(0xAAAA_0002), Some(7));
        assert_eq!(nums.i32(0xAAAA_0002), Some(7));
        assert_eq!(nums.u8(0xAAAA_0003), Some(3));
        assert_eq!(nums.string(0xAAAA_0004), Some("hello"));
        // An absent field is absent, not zero.
        assert_eq!(nums.u32(0xBBBB_0001), None);
        assert_eq!(nums.f32(0xBBBB_0001), None);
    }

    #[test]
    fn a_record_summary_carries_no_numbers_that_were_not_read() {
        // An unnamed record says exactly what it knows: the class and the count.
        let record = Record::Unnamed {
            asset: 0x1234,
            root_class: 0xDEAD_BEEF,
            instances: 3,
        };
        let line = record.summary();
        assert!(line.contains("0xDEADBEEF"), "{line}");
        assert!(line.contains("3 instance"), "{line}");
        assert_eq!(record.asset(), 0x1234);
        assert_eq!(record.root_class(), 0xDEAD_BEEF);
    }
}
